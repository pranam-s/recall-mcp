//! recall-cli — scripting and serving entry point for Recall-MCP.

// Curated clippy opt-ins (docs/style-guides/rust-tooling-conventions.md).
#![warn(clippy::dbg_macro, clippy::todo, clippy::unimplemented)]

use std::io::IsTerminal as _;
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use recall_core::export::{ExportData, export, import};
use recall_core::views::{
    CompactCaptureSummary, CompactListPage, CompactMemory, CompactRecallHit, render_hits_text,
};
use recall_core::{
    Embedder, MemoryUpdate, NewMemory, RecallParams, SharedStore, SqliteStore, shared, unix_now,
};
use recall_server::build_router;

#[derive(Parser)]
#[command(
    name = "recall",
    version,
    about = "Local-first semantic memory broker (MCP + HTTP + CLI)",
    propagate_version = true
)]
struct Cli {
    /// SQLite database file (created if missing). Applies to every subcommand.
    #[arg(
        long,
        global = true,
        env = "RECALL_MCP_DB",
        default_value = "recall.db"
    )]
    db: PathBuf,
    /// Embedder for remember/recall/update and the server: `hash` (default),
    /// `onnx` or `openai` (env RECALL_MCP_EMBEDDER).
    #[arg(long, global = true)]
    embedder: Option<String>,
    /// Open a store whose embedder fingerprint/dimensions differ from the
    /// current embedder. Mixing vector spaces degrades recall; the warning is
    /// explicit and the stored fingerprint is not rewritten.
    #[arg(long, global = true, env = "RECALL_MCP_ALLOW_EMBEDDER_MISMATCH")]
    allow_embedder_mismatch: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum OutputFormat {
    Json,
    Text,
}

#[derive(Subcommand)]
enum Command {
    /// Run the server: HTTP API + MCP streamable-HTTP (and the web UI if built),
    /// or MCP over stdio with --stdio.
    Serve {
        /// Serve MCP over stdio instead of HTTP.
        #[arg(long)]
        stdio: bool,
        /// Bind address for the HTTP server (env RECALL_MCP_BIND; default
        /// 127.0.0.1:8787). Ignored with --stdio.
        #[arg(long)]
        bind: Option<String>,
        /// Optional JSON config file (bind, rate_limit, rate_burst,
        /// body_limit, embedder) — lowest precedence after flags and env.
        #[arg(long)]
        config: Option<PathBuf>,
        /// API key for /v1/*, /metrics and /mcp (falls back to RECALL_MCP_API_KEY).
        #[arg(long)]
        api_key: Option<String>,
        /// Serve the built web UI from this directory at /.
        #[arg(long)]
        web_dir: Option<PathBuf>,
        /// Rate limit in sustained requests/second per key (env
        /// RECALL_MCP_RATE_LIMIT). Omit to disable limiting.
        #[arg(long)]
        rate_limit: Option<f64>,
        /// Rate-limit burst capacity (default: max(2x rate, 10)).
        #[arg(long)]
        rate_burst: Option<f64>,
        /// Maximum request body size in bytes (env RECALL_MCP_BODY_LIMIT).
        #[arg(long)]
        body_limit: Option<usize>,
        /// Browser origin allowed cross-origin API access (repeatable), e.g.
        /// `--allow-origin http://localhost:5173` for the Vite dev server.
        /// Default: same-origin only — no CORS, foreign origins rejected.
        #[arg(long = "allow-origin", value_name = "ORIGIN", alias = "allow-origins")]
        allow_origin: Vec<String>,
        /// Extra Host header values to accept besides loopback hosts
        /// (repeatable), for non-loopback bindings such as a LAN IP or a
        /// container healthcheck name. Requests with other Host values are
        /// rejected as misdirected (DNS-rebinding defense).
        #[arg(long = "allow-host", value_name = "HOST", alias = "allow-hosts")]
        allow_host: Vec<String>,
    },
    /// Store one memory.
    Remember {
        #[arg(long, env = "RECALL_MCP_NAMESPACE")]
        namespace: String,
        #[arg(long)]
        text: String,
        #[arg(long, value_delimiter = ',')]
        tags: Vec<String>,
        #[arg(long)]
        pinned: bool,
        /// Full old record shape instead of the compact default.
        #[arg(long)]
        full: bool,
        /// Include the stored embedding vector in the printed JSON.
        #[arg(long)]
        show_embedding: bool,
        /// Pretty-print JSON even when stdout is a pipe.
        #[arg(long)]
        pretty: bool,
    },
    /// Recall memories (ranked hits with score breakdowns).
    Recall {
        #[arg(long, env = "RECALL_MCP_NAMESPACE")]
        namespace: String,
        #[arg(long)]
        query: String,
        /// Number of hits.
        #[arg(long, default_value_t = 5)]
        k: usize,
        /// Recency decay constant in days; 0 disables the recency term.
        #[arg(long, default_value_t = 30.0)]
        tau_days: f64,
        /// Only return memories carrying ALL of these tags.
        #[arg(long, value_delimiter = ',')]
        tags: Vec<String>,
        /// Output format (default json; text is compact and keeps ranking).
        #[arg(long, value_enum, default_value_t = OutputFormat::Json)]
        format: OutputFormat,
        /// Full old record shape instead of the compact default.
        #[arg(long)]
        full: bool,
        /// Include each hit's embedding vector in the printed JSON.
        #[arg(long)]
        show_embedding: bool,
        /// Pretty-print JSON even when stdout is a pipe.
        #[arg(long)]
        pretty: bool,
    },
    /// Update a memory in place (only the given fields change).
    Update {
        /// Id of the memory to update.
        #[arg(long)]
        id: String,
        /// New text (re-embedded automatically).
        #[arg(long)]
        text: Option<String>,
        /// Replacement tags (replaces the whole set).
        #[arg(long, value_delimiter = ',')]
        tags: Option<Vec<String>>,
        /// Change the pinned flag.
        #[arg(long)]
        pinned: Option<bool>,
        /// Full old record shape instead of the compact default.
        #[arg(long)]
        full: bool,
        /// Include the stored embedding vector in the printed JSON.
        #[arg(long)]
        show_embedding: bool,
        /// Pretty-print JSON even when stdout is a pipe.
        #[arg(long)]
        pretty: bool,
    },
    /// Permanently delete one memory by id.
    Forget {
        #[arg(long)]
        id: String,
        /// Pretty-print JSON even when stdout is a pipe.
        #[arg(long)]
        pretty: bool,
    },
    /// List memories in a namespace, newest first.
    List {
        #[arg(long, env = "RECALL_MCP_NAMESPACE")]
        namespace: Option<String>,
        #[arg(long, default_value_t = 20)]
        limit: usize,
        #[arg(long, default_value_t = 0)]
        offset: usize,
        /// Full old record shape instead of the compact default.
        #[arg(long)]
        full: bool,
        /// Include embedding vectors in the printed JSON.
        #[arg(long)]
        show_embedding: bool,
        /// Pretty-print JSON even when stdout is a pipe.
        #[arg(long)]
        pretty: bool,
    },
    /// Ingest a transcript JSON file as chunked memories.
    Capture {
        #[arg(long, env = "RECALL_MCP_NAMESPACE")]
        namespace: String,
        /// JSON file containing an array of `{role, content, at}` messages.
        #[arg(long)]
        file: PathBuf,
        /// Extra tags applied to every captured chunk.
        #[arg(long, value_delimiter = ',')]
        tags: Vec<String>,
        /// Chunk size cap in characters (default 1000).
        #[arg(long)]
        max_chunk_chars: Option<usize>,
        /// Include every captured record instead of the summary.
        #[arg(long)]
        include_memories: bool,
        /// Include embeddings in the captured records.
        #[arg(long)]
        show_embedding: bool,
        /// Pretty-print JSON even when stdout is a pipe.
        #[arg(long)]
        pretty: bool,
    },
    /// Print store statistics.
    Stats {
        /// Pretty-print JSON even when stdout is a pipe.
        #[arg(long)]
        pretty: bool,
    },
    /// Generate a fresh 256-bit store key and write it owner-only.
    Keygen {
        /// Output file (created with owner-only permissions).
        #[arg(long)]
        out: PathBuf,
    },
    /// Export the whole store to a JSON file.
    Export {
        /// Output file (defaults to stdout).
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Import a previously exported JSON file (existing ids are skipped).
    Import {
        #[arg(long)]
        file: PathBuf,
    },
    /// Hot backup: checkpoint the WAL and write a compacted snapshot of the
    /// database to a new file (encrypted stores produce encrypted backups).
    Backup {
        /// Backup target file; must not exist.
        #[arg(long)]
        out: PathBuf,
    },
    /// Compact the database in place (VACUUM) and checkpoint the WAL.
    Vacuum,
}

fn open_store(db: &PathBuf) -> Result<SharedStore> {
    // Encryption at rest (D-014): when a key is configured, the store is
    // opened in keyed mode — new memories are AEAD-encrypted on disk and
    // existing plaintext databases are upgraded in place.
    let store = match encryption_key()? {
        Some(key) => SqliteStore::open_with_key(db, &key),
        None => SqliteStore::open(db),
    }
    .with_context(|| format!("open database {}", db.display()))?;
    Ok(shared(store))
}

/// Read UTF-8 text with an optional byte-order mark stripped. Config files,
/// key files, and exports written by Windows editors commonly start with a
/// BOM; refusing them for an invisible character is hostile.
fn read_text(path: &PathBuf) -> Result<String> {
    let bytes = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(&bytes);
    String::from_utf8(bytes.to_vec()).with_context(|| format!("{} is not UTF-8", path.display()))
}

/// Guard the store's vector space against the resolved embedder. Returns the
/// status so `serve` can log adoption warnings; a mismatch fails unless the
/// operator passed the explicit escape hatch.
fn guard_embedder(
    store: &SharedStore,
    embedder: &dyn Embedder,
    allow_mismatch: bool,
) -> Result<recall_core::EmbedderStatus> {
    let guard = store.lock().unwrap_or_else(|p| p.into_inner());
    let status = guard
        .check_embedder(embedder, allow_mismatch)
        .context("check store embedder")?;
    match &status {
        recall_core::EmbedderStatus::AdoptedLegacy { stored_dimensions } => {
            tracing::warn!(
                embedder = %embedder.name(),
                stored_dimensions = ?stored_dimensions,
                "store had no embedder fingerprint; adopted it because the dimensions match. \
                 If this store was written by a different embedder, re-embed the records"
            );
        }
        recall_core::EmbedderStatus::MismatchAllowed { stored, current } => {
            tracing::warn!(
                stored = %stored.fingerprint,
                current = %current.fingerprint,
                "embedder mismatch allowed; recall quality will degrade because vector spaces differ"
            );
        }
        _ => {}
    }
    Ok(status)
}

/// Key from `RECALL_MCP_KEY` (64 hex chars) or a file path in
/// `RECALL_MCP_KEY_FILE` whose contents are the 64-hex-char key. A malformed
/// key is an error, never a silent fallback to plaintext.
fn encryption_key() -> Result<Option<recall_core::crypto::StoreKey>> {
    let from_env = std::env::var("RECALL_MCP_KEY")
        .ok()
        .filter(|k| !k.trim().is_empty());
    let from_file = if from_env.is_some() {
        None
    } else {
        match std::env::var("RECALL_MCP_KEY_FILE") {
            Ok(path) if !path.trim().is_empty() => {
                let text = read_text(&PathBuf::from(&path))
                    .with_context(|| format!("read RECALL_MCP_KEY_FILE {path}"))?;
                Some(text)
            }
            _ => None,
        }
    };
    match from_env.or(from_file) {
        Some(raw) => Ok(Some(recall_core::crypto::StoreKey::from_hex(raw.trim())?)),
        None => Ok(None),
    }
}

fn print_json(value: serde_json::Value, pretty_flag: bool) -> Result<()> {
    // Compact when piped (one line, token-friendly), pretty on a terminal or
    // with --pretty.
    if pretty_flag || std::io::stdout().is_terminal() {
        println!("{}", serde_json::to_string_pretty(&value)?);
    } else {
        println!("{}", serde_json::to_string(&value)?);
    }
    Ok(())
}

/// Remove every `embedding` field from serialized memory JSON (works at any
/// nesting depth, so it covers both a single record and a hit list). The
/// default CLI output omits the vector: it is storage detail with
/// no signal for a human at a terminal, and for a screen-reader user it is
/// hundreds of spoken zeros per record. `--show-embedding` opts back in.
fn strip_embeddings(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            map.remove("embedding");
            for child in map.values_mut() {
                strip_embeddings(child);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                strip_embeddings(item);
            }
        }
        _ => {}
    }
}

/// Serialize a value for printing, stripping embedding vectors unless the
/// command's `--show-embedding` flag was passed.
fn memory_json(mut value: serde_json::Value, show_embedding: bool) -> Result<serde_json::Value> {
    if !show_embedding {
        strip_embeddings(&mut value);
    }
    Ok(value)
}

/// Write a file restricted to the current user where the OS supports it
/// (POSIX 0600; Windows uses the default user-profile ACLs). Used for
/// plaintext exports so sensitive backups are not world-readable (AR-021).
fn write_private(path: &PathBuf, bytes: &[u8]) -> Result<()> {
    std::fs::write(path, bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

/// Restrict an existing file to the current user where the OS supports it.
fn restrict_permissions(path: &PathBuf) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}

fn env_api_key() -> Option<String> {
    std::env::var("RECALL_MCP_API_KEY").ok()
}

fn effective_api_key(cli_key: Option<&String>, env_value: Option<String>) -> Option<String> {
    cli_key
        .cloned()
        .or(env_value)
        .filter(|k| !k.trim().is_empty())
}

struct ServeArgs {
    db: PathBuf,
    stdio: bool,
    bind: Option<String>,
    api_key: Option<String>,
    web_dir: Option<PathBuf>,
    rate_limit: Option<f64>,
    rate_burst: Option<f64>,
    body_limit: Option<usize>,
    embedder: Option<String>,
    allow_origin: Vec<String>,
    allow_host: Vec<String>,
    allow_embedder_mismatch: bool,
}

/// Optional serve overrides from a JSON file (`--config`), merged with the
/// lowest precedence: flags > environment > config file > defaults.
/// Secrets (API key, encryption key) are deliberately not file-configurable.
#[derive(serde::Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct ServeConfigFile {
    bind: Option<String>,
    rate_limit: Option<f64>,
    rate_burst: Option<f64>,
    body_limit: Option<usize>,
    embedder: Option<String>,
    #[serde(alias = "allow_origin")]
    allow_origins: Option<Vec<String>>,
    #[serde(alias = "allow_host")]
    allow_hosts: Option<Vec<String>>,
}

/// Validate an opt-in browser origin: `http`/`https` scheme, non-empty host,
/// optional numeric port, and no path/query/fragment. Rejecting a path here
/// catches the common `https://app.example/` typo at startup instead of
/// silently never matching the browser's `Origin` header (which has no path).
fn validate_origin(origin: &str) -> Result<()> {
    let (scheme, authority) = origin
        .split_once("://")
        .ok_or_else(|| anyhow::anyhow!("expected scheme://host[:port]"))?;
    if !matches!(scheme.to_ascii_lowercase().as_str(), "http" | "https") {
        anyhow::bail!("scheme must be http or https");
    }
    if authority.is_empty() || authority.contains('/') || authority.contains('?') {
        anyhow::bail!("expected scheme://host[:port] with no path, query, or fragment");
    }
    if origin.parse::<axum::http::HeaderValue>().is_err() {
        anyhow::bail!("not a valid HTTP origin value");
    }
    Ok(())
}

fn env_parse<T: std::str::FromStr>(name: &str) -> Result<Option<T>> {
    match std::env::var(name) {
        Ok(raw) if !raw.trim().is_empty() => raw
            .trim()
            .parse::<T>()
            .map(Some)
            .map_err(|_| anyhow::anyhow!("invalid {name}: {raw:?}")),
        _ => Ok(None),
    }
}

/// Embedder-name resolution shared by serve and the one-shot commands:
/// flag > env `RECALL_MCP_EMBEDDER` > config-file value > default `hash`.
/// Blank flag/env/file values fall through to the next layer.
fn embedder_name(flag: Option<&str>, file: Option<&str>) -> Result<String> {
    if let Some(f) = flag.map(str::trim).filter(|f| !f.is_empty()) {
        return Ok(f.to_string());
    }
    if let Some(e) = env_parse::<String>("RECALL_MCP_EMBEDDER")? {
        return Ok(e);
    }
    if let Some(f) = file.map(str::trim).filter(|f| !f.is_empty()) {
        return Ok(f.to_string());
    }
    Ok("hash".to_string())
}

/// Build the embedder for one-shot commands (`remember`, `recall`, `update`):
/// resolve the name, then construct — a missing feature or runtime resource
/// (onnx dylib, OpenAI key) fails the command instead of silently degrading.
fn resolve_embedder(flag: Option<&str>) -> Result<std::sync::Arc<dyn Embedder>> {
    let name = embedder_name(flag, None)?;
    recall_core::select_embedder(&name).with_context(|| format!("select embedder {name:?}"))
}

fn serve_config(
    args: &ServeArgs,
    config_file: Option<&PathBuf>,
) -> Result<(recall_server::ServerConfig, String)> {
    let file: ServeConfigFile = match config_file {
        Some(path) => {
            let raw = read_text(path).with_context(|| format!("read config {}", path.display()))?;
            serde_json::from_str(&raw)
                .with_context(|| format!("parse config {}", path.display()))?
        }
        None => ServeConfigFile::default(),
    };
    // Precedence: flag > env > config file > default.
    let rate_limit = args
        .rate_limit
        .or(env_parse("RECALL_MCP_RATE_LIMIT")?)
        .or(file.rate_limit);
    let body_limit = args
        .body_limit
        .or(env_parse("RECALL_MCP_BODY_LIMIT")?)
        .or(file.body_limit);
    let bind = args
        .bind
        .clone()
        .or_else(|| {
            std::env::var("RECALL_MCP_BIND")
                .ok()
                .filter(|v| !v.trim().is_empty())
        })
        .or(file.bind)
        .unwrap_or_else(|| "127.0.0.1:8787".into());
    // Validation: fail closed on nonsense instead of serving unbounded.
    let rate_limit = match rate_limit {
        Some(rate) if !rate.is_finite() || rate <= 0.0 => {
            anyhow::bail!("rate limit must be finite and > 0, got {rate}")
        }
        Some(rate) => {
            let burst = args
                .rate_burst
                .or(file.rate_burst)
                .unwrap_or_else(|| (rate * 2.0).max(10.0));
            let limits = recall_server::ratelimit::RateLimit { rate, burst };
            limits
                .validate()
                .map_err(|e| anyhow::anyhow!("invalid rate limit: {e}"))?;
            Some(limits)
        }
        None => None,
    };
    let body_limit_bytes = body_limit
        .unwrap_or(recall_server::DEFAULT_BODY_LIMIT_BYTES)
        .max(1);
    let embedder_name = embedder_name(args.embedder.as_deref(), file.embedder.as_deref())?;
    let embedder = recall_core::select_embedder(&embedder_name)
        .with_context(|| format!("select embedder {embedder_name:?}"))?;
    // Cross-origin opt-in (AR-001): flags replace the config-file list; every
    // entry is trimmed and validated so a typo fails the startup, not the
    // first request.
    let allow_origin: Vec<String> = if args.allow_origin.is_empty() {
        file.allow_origins.unwrap_or_default()
    } else {
        args.allow_origin.clone()
    }
    .iter()
    .map(|o| o.trim().to_string())
    .collect();
    for origin in &allow_origin {
        validate_origin(origin).with_context(|| format!("invalid --allow-origin {origin:?}"))?;
    }
    let allow_host = if args.allow_host.is_empty() {
        file.allow_hosts.unwrap_or_default()
    } else {
        args.allow_host.clone()
    };
    let config = recall_server::ServerConfig {
        api_key: effective_api_key(args.api_key.as_ref(), env_api_key()),
        web_dir: args.web_dir.clone(),
        rate_limit,
        body_limit_bytes,
        embedder,
        allowed_origins: allow_origin,
        allowed_hosts: allow_host.iter().map(|h| h.trim().to_string()).collect(),
    };
    Ok((config, bind))
}

async fn serve(args: &ServeArgs, config_file: Option<&PathBuf>) -> Result<()> {
    // AR-003 honesty notice: with encryption at rest enabled, a dense neural
    // embedder still leaks memory text through the plaintext embedding
    // (published embedding-inversion attacks). Say so at startup instead of
    // letting the D-014 caveat hide in the docs.
    let keyed = encryption_key()?.is_some();
    let store = open_store(&args.db)?;
    let (config, bind) = serve_config(args, config_file)?;
    guard_embedder(
        &store,
        config.embedder.as_ref(),
        args.allow_embedder_mismatch,
    )?;
    if keyed && config.embedder.embedding_invertible() {
        tracing::warn!(
            embedder = %config.embedder.name(),
            "encryption at rest is ON but this embedder stores plaintext vectors that \
             published embedding-inversion attacks can partially reverse into memory text; \
             keyed mode does NOT protect text here (D-014). Use --embedder hash if at-rest \
             confidentiality matters"
        );
    }
    if args.stdio {
        tracing::info!("serving MCP over stdio");
        // stdio has no metrics; the embedder is used raw.
        let state = recall_server::state::AppState::new(
            store.clone(),
            config.embedder.clone(),
            config.api_key,
        );
        // stdio is the only stdout writer; tracing logs go to stderr.
        let result = recall_server::mcp::run_stdio_server(state).await;
        // Same shutdown hygiene as the HTTP path: flush the WAL so the next
        // open starts from a clean main file.
        store
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .checkpoint_wal()
            .context("WAL checkpoint on stdio shutdown")?;
        return result;
    }

    let app = build_router(store.clone(), &config);
    let listener = tokio::net::TcpListener::bind(&bind)
        .await
        .with_context(|| format!("bind {bind}"))?;
    let addr = listener.local_addr()?;
    tracing::info!(%addr, "recall-mcp HTTP server listening (REST + MCP at /mcp)");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("server error")?;
    // Drain finished: flush the WAL so the next open starts from a clean file.
    store
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .checkpoint_wal()
        .context("WAL checkpoint on shutdown")?;
    tracing::info!("shutdown complete (WAL checkpointed)");
    Ok(())
}

/// Resolve on Ctrl-C or SIGTERM (SIGTERM is not delivered on Windows).
async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut sig) => {
                sig.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
    tracing::info!("shutdown signal received; draining in-flight requests");
}

pub async fn run() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();
    let db = cli.db.clone();
    let allow_mismatch = cli.allow_embedder_mismatch;
    match &cli.command {
        Command::Serve {
            stdio,
            bind,
            config,
            api_key,
            web_dir,
            rate_limit,
            rate_burst,
            body_limit,
            allow_origin,
            allow_host,
        } => {
            serve(
                &ServeArgs {
                    db,
                    stdio: *stdio,
                    bind: bind.clone(),
                    api_key: api_key.clone(),
                    web_dir: web_dir.clone(),
                    rate_limit: *rate_limit,
                    rate_burst: *rate_burst,
                    body_limit: *body_limit,
                    embedder: cli.embedder.clone(),
                    allow_origin: allow_origin.clone(),
                    allow_host: allow_host.clone(),
                    allow_embedder_mismatch: allow_mismatch,
                },
                config.as_ref(),
            )
            .await
        }
        Command::Remember {
            namespace,
            text,
            tags,
            pinned,
            full,
            show_embedding,
            pretty,
        } => {
            let store = open_store(&db)?;
            let embedder = resolve_embedder(cli.embedder.as_deref())?;
            guard_embedder(&store, embedder.as_ref(), allow_mismatch)?;
            recall_core::validation::validate_namespace(namespace)?;
            recall_core::validation::validate_text(text)?;
            recall_core::validation::validate_tags(tags)?;
            let embedding = embedder.embed(text);
            // One guard for the whole read-modify sequence (the mutex is not re-entrant).
            let guard = store.lock().unwrap_or_else(|p| p.into_inner());
            guard.get_or_create_namespace(namespace)?;
            let memory = guard.insert_memory(&NewMemory {
                namespace: namespace.clone(),
                text: text.clone(),
                tags: tags.clone(),
                source: Some("cli".to_string()),
                pinned: *pinned,
                created_at: Some(unix_now()),
                id: None,
                embedding: Some(embedding),
            })?;
            drop(guard);
            let value = if *full || *show_embedding {
                memory_json(serde_json::to_value(&memory)?, *show_embedding)?
            } else {
                serde_json::to_value(CompactMemory::from(&memory))?
            };
            print_json(value, *pretty)
        }
        Command::Recall {
            namespace,
            query,
            k,
            tau_days,
            tags,
            format,
            full,
            show_embedding,
            pretty,
        } => {
            if matches!(format, OutputFormat::Text) && *show_embedding {
                anyhow::bail!("--show-embedding cannot be combined with --format text");
            }
            let store = open_store(&db)?;
            let embedder = resolve_embedder(cli.embedder.as_deref())?;
            guard_embedder(&store, embedder.as_ref(), allow_mismatch)?;
            recall_core::validation::validate_namespace(namespace)?;
            recall_core::validation::validate_query(query)?;
            recall_core::validation::validate_tags(tags)?;
            let params = RecallParams {
                k: *k,
                tau_days: if *tau_days <= 0.0 {
                    None
                } else {
                    Some(*tau_days)
                },
                tags: tags.clone(),
                ..RecallParams::default()
            };
            params.validate()?;
            let query_embedding = embedder.embed(query);
            let hits = store.lock().unwrap_or_else(|p| p.into_inner()).recall(
                namespace,
                query,
                Some(&query_embedding),
                &params,
                unix_now(),
            )?;
            match format {
                OutputFormat::Text => {
                    print!("{}", render_hits_text(&hits));
                    Ok(())
                }
                OutputFormat::Json => {
                    let value = if *full || *show_embedding {
                        memory_json(serde_json::to_value(&hits)?, *show_embedding)?
                    } else {
                        serde_json::to_value(
                            hits.iter().map(CompactRecallHit::from).collect::<Vec<_>>(),
                        )?
                    };
                    print_json(value, *pretty)
                }
            }
        }
        Command::Update {
            id,
            text,
            tags,
            pinned,
            full,
            show_embedding,
            pretty,
        } => {
            let store = open_store(&db)?;
            let embedder = resolve_embedder(cli.embedder.as_deref())?;
            guard_embedder(&store, embedder.as_ref(), allow_mismatch)?;
            let mut update = MemoryUpdate {
                text: text.clone(),
                tags: tags.clone(),
                pinned: *pinned,
                source: None,
                embedding: None,
            };
            if let Some(t) = &update.text {
                update.embedding = Some(embedder.embed(t));
            }
            let memory = store
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .update_memory(id, &update)?;
            let value = if *full || *show_embedding {
                memory_json(serde_json::to_value(&memory)?, *show_embedding)?
            } else {
                serde_json::to_value(CompactMemory::from(&memory))?
            };
            print_json(value, *pretty)
        }
        Command::Forget { id, pretty } => {
            let store = open_store(&db)?;
            let deleted = store
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .delete_memory(id)?;
            if !deleted {
                anyhow::bail!("memory not found: {id}");
            }
            print_json(serde_json::json!({ "id": id, "deleted": true }), *pretty)
        }
        Command::List {
            namespace,
            limit,
            offset,
            full,
            show_embedding,
            pretty,
        } => {
            let store = open_store(&db)?;
            let limit = category_limit(*limit);
            let guard = store.lock().unwrap_or_else(|p| p.into_inner());
            let total = guard.count_memories(namespace.as_deref())?;
            let memories = guard.list_memories(namespace.as_deref(), limit, *offset)?;
            drop(guard);
            let page: CompactListPage<serde_json::Value> = if *full || *show_embedding {
                let items = memories
                    .iter()
                    .map(|m| -> Result<serde_json::Value> {
                        memory_json(serde_json::to_value(m)?, *show_embedding)
                    })
                    .collect::<Result<Vec<_>>>()?;
                CompactListPage::new(items, total, limit, *offset)
            } else {
                let items = memories
                    .iter()
                    .map(|m| serde_json::to_value(CompactMemory::from(m)))
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                CompactListPage::new(items, total, limit, *offset)
            };
            print_json(serde_json::to_value(page)?, *pretty)
        }
        Command::Capture {
            namespace,
            file,
            tags,
            max_chunk_chars,
            include_memories,
            show_embedding,
            pretty,
        } => {
            let store = open_store(&db)?;
            let embedder = resolve_embedder(cli.embedder.as_deref())?;
            guard_embedder(&store, embedder.as_ref(), allow_mismatch)?;
            let raw =
                read_text(file).with_context(|| format!("read transcript {}", file.display()))?;
            let transcript: Vec<recall_server::capture::CaptureMessage> =
                serde_json::from_str(&raw)
                    .with_context(|| format!("parse transcript {}", file.display()))?;
            let request = recall_server::capture::CaptureRequest {
                namespace: namespace.clone(),
                transcript,
                tags: tags.clone(),
                max_chunk_chars: *max_chunk_chars,
                include_memories: *include_memories,
                include_embedding: false,
            };
            let prepared = recall_server::capture::prepare(embedder.as_ref(), &request)?;
            let captured = {
                let guard = store.lock().unwrap_or_else(|p| p.into_inner());
                recall_server::capture::commit(&*guard, &prepared)?
            };
            let value = if *include_memories {
                let memories = if *show_embedding {
                    captured
                        .iter()
                        .map(serde_json::to_value)
                        .collect::<Result<Vec<_>, _>>()?
                } else {
                    captured
                        .iter()
                        .map(|m| serde_json::to_value(CompactMemory::from(m)))
                        .collect::<Result<Vec<_>, _>>()?
                };
                serde_json::to_value(serde_json::json!({
                    "namespace": namespace,
                    "captured": captured.len(),
                    "memories": memories,
                }))?
            } else {
                serde_json::to_value(CompactCaptureSummary::new(namespace, &captured))?
            };
            print_json(value, *pretty)
        }
        Command::Stats { pretty } => {
            let store = open_store(&db)?;
            let embedder = resolve_embedder(cli.embedder.as_deref())?;
            let stats = store.lock().unwrap_or_else(|p| p.into_inner()).stats()?;
            let mut value = serde_json::to_value(stats)?;
            if let Some(object) = value.as_object_mut() {
                object.insert(
                    "embedder".into(),
                    serde_json::json!({
                        "name": embedder.name(),
                        "dimensions": embedder.dimensions(),
                        "fingerprint": embedder.fingerprint(),
                    }),
                );
            }
            print_json(value, *pretty)
        }
        Command::Keygen { out } => {
            let key = recall_core::crypto::StoreKey::generate();
            if out.exists() {
                anyhow::bail!("refusing to overwrite existing key file {}", out.display());
            }
            write_private(out, format!("{}\n", key.to_hex()).as_bytes())
                .with_context(|| format!("write key file {}", out.display()))?;
            println!(
                "wrote a new 256-bit key to {} (owner-only where supported). \
                 Keep it secret; losing it means losing access to keyed stores.",
                out.display()
            );
            Ok(())
        }
        Command::Export { out } => {
            let store = open_store(&db)?;
            let data = export(&*store.lock().unwrap_or_else(|p| p.into_inner()))?;
            let json = serde_json::to_string_pretty(&data)?;
            match out {
                Some(path) => {
                    // Exports are plaintext JSON by design (D-014) — write
                    // them owner-only where the OS allows, and say so in the
                    // success line (AR-021).
                    write_private(path, json.as_bytes())
                        .with_context(|| format!("write {}", path.display()))?;
                    eprintln!(
                        "exported {} namespaces, {} memories to {} — WARNING: this file is \
                         plaintext JSON and bypasses encryption at rest",
                        data.namespaces.len(),
                        data.memories.len(),
                        path.display()
                    );
                    Ok(())
                }
                None => {
                    eprintln!(
                        "WARNING: exporting plaintext JSON to stdout bypasses encryption at rest"
                    );
                    println!("{json}");
                    Ok(())
                }
            }
        }
        Command::Import { file } => {
            let store = open_store(&db)?;
            let raw = read_text(file).with_context(|| format!("read {}", file.display()))?;
            let data: ExportData = serde_json::from_str(&raw)?;
            let report = import(&*store.lock().unwrap_or_else(|p| p.into_inner()), &data)?;
            print_json(serde_json::to_value(report)?, false)
        }
        Command::Backup { out } => {
            if out.exists() {
                anyhow::bail!(
                    "backup target {} already exists (VACUUM INTO never overwrites)",
                    out.display()
                );
            }
            let keyed = encryption_key()?.is_some();
            let store = open_store(&db)?;
            let guard = store.lock().unwrap_or_else(|p| p.into_inner());
            guard.checkpoint_wal().context("WAL checkpoint")?;
            guard
                .vacuum_into(out)
                .with_context(|| format!("backup into {}", out.display()))?;
            let stats = guard.stats()?;
            drop(guard);
            restrict_permissions(out)?;
            if !keyed {
                eprintln!(
                    "WARNING: this backup is a plaintext database copy; treat {} like the \
                     original unencrypted store and store it accordingly",
                    out.display()
                );
            }
            print_json(
                serde_json::json!({
                    "backup": out.display().to_string(),
                    "encrypted": keyed,
                    "total_namespaces": stats.total_namespaces,
                    "total_memories": stats.total_memories,
                }),
                false,
            )
        }
        Command::Vacuum => {
            let store = open_store(&db)?;
            let guard = store.lock().unwrap_or_else(|p| p.into_inner());
            guard.vacuum().context("VACUUM")?;
            guard.checkpoint_wal().context("WAL checkpoint")?;
            let stats = guard.stats()?;
            print_json(
                serde_json::json!({
                    "vacuumed": db.display().to_string(),
                    "total_namespaces": stats.total_namespaces,
                    "total_memories": stats.total_memories,
                }),
                false,
            )
        }
    }
}

/// CLI page size: at least 1, capped at the same 10 000 the HTTP surfaces
/// use for non-MCP listing. MCP has its own tighter 100 budget.
fn category_limit(limit: usize) -> usize {
    limit.clamp(1, recall_server::MAX_PAGE_LIMIT)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serializes tests that mutate process-global environment variables.
    /// Poison-recovering, like `AppState::with_store`: one test's panic must
    /// not fail every later env test with `PoisonError`.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Deterministic serve_config tests: every env layer starts unset.
    ///
    /// SAFETY (edition-2024 unsafe env access): process-global mutation is
    /// confined to this test binary and single-threaded here — every
    /// env-touching test holds [`ENV_LOCK`].
    fn clear_serve_env() {
        for name in [
            "RECALL_MCP_RATE_LIMIT",
            "RECALL_MCP_BODY_LIMIT",
            "RECALL_MCP_BIND",
            "RECALL_MCP_EMBEDDER",
            "RECALL_MCP_API_KEY",
        ] {
            unsafe { std::env::remove_var(name) };
        }
    }

    fn serve_args() -> ServeArgs {
        ServeArgs {
            db: PathBuf::from("t.db"),
            stdio: false,
            bind: None,
            api_key: None,
            web_dir: None,
            rate_limit: None,
            rate_burst: None,
            body_limit: None,
            embedder: None,
            allow_origin: Vec::new(),
            allow_host: Vec::new(),
            allow_embedder_mismatch: false,
        }
    }

    #[test]
    fn api_key_prefers_cli_then_env_then_none() {
        let cli = Some("cli-key".to_string());
        assert!(effective_api_key(None, None).is_none());
        assert_eq!(
            effective_api_key(cli.as_ref(), None).as_deref(),
            Some("cli-key")
        );
        assert_eq!(
            effective_api_key(None, Some("env-key".into())).as_deref(),
            Some("env-key")
        );
        // CLI wins over env.
        assert_eq!(
            effective_api_key(cli.as_ref(), Some("env-key".into())).as_deref(),
            Some("cli-key")
        );
        // Blank values never count.
        assert!(effective_api_key(None, Some("  ".into())).is_none());
        assert!(effective_api_key(Some("  ".into()).as_ref(), None).is_none());
    }

    #[test]
    fn embedder_name_flag_beats_env_beats_file_beats_default() {
        let _guard = env_lock();
        // SAFETY: ENV_LOCK serializes process-global env access in tests.
        unsafe { std::env::remove_var("RECALL_MCP_EMBEDDER") };
        // Flag wins over everything.
        assert_eq!(
            embedder_name(Some("onnx"), Some("hash")).unwrap(),
            "onnx",
            "flag must beat file"
        );
        // Env over file, file over default.
        // SAFETY: ENV_LOCK serializes process-global env access in tests.
        unsafe { std::env::set_var("RECALL_MCP_EMBEDDER", "openai") };
        assert_eq!(embedder_name(None, Some("hash")).unwrap(), "openai");
        // SAFETY: ENV_LOCK serializes process-global env access in tests.
        unsafe { std::env::remove_var("RECALL_MCP_EMBEDDER") };
        assert_eq!(embedder_name(None, Some("openai")).unwrap(), "openai");
        assert_eq!(embedder_name(None, None).unwrap(), "hash");
        // Blank values fall through to the next layer.
        assert_eq!(embedder_name(Some("   "), Some("hash")).unwrap(), "hash");
        // SAFETY: ENV_LOCK serializes process-global env access in tests.
        unsafe { std::env::set_var("RECALL_MCP_EMBEDDER", "  ") };
        assert_eq!(embedder_name(None, Some("hash")).unwrap(), "hash");
        // SAFETY: ENV_LOCK serializes process-global env access in tests.
        unsafe { std::env::remove_var("RECALL_MCP_EMBEDDER") };
    }

    #[test]
    fn resolve_embedder_builds_hash_and_rejects_unknown() {
        let _guard = env_lock();
        // SAFETY: ENV_LOCK serializes process-global env access in tests.
        unsafe { std::env::remove_var("RECALL_MCP_EMBEDDER") };
        let e = resolve_embedder(None).expect("default embedder");
        assert_eq!(e.name(), "hash-256");
        // `.err()` — the success side is a trait object without Debug.
        let err = resolve_embedder(Some("gpt4"))
            .err()
            .expect("unknown embedder must fail");
        // anyhow's Display shows only the outer context; the typed error is
        // the root cause.
        assert!(
            err.root_cause().to_string().contains("unknown embedder"),
            "got: {err:#}"
        );
    }

    #[cfg(not(feature = "onnx"))]
    #[test]
    fn resolve_embedder_onnx_requires_the_feature() {
        let _guard = env_lock();
        // SAFETY: ENV_LOCK serializes process-global env access in tests.
        unsafe { std::env::remove_var("RECALL_MCP_EMBEDDER") };
        let err = resolve_embedder(Some("onnx"))
            .err()
            .expect("onnx must fail without the feature");
        assert!(
            err.root_cause().to_string().contains("onnx"),
            "error must name the embedder: {err:#}"
        );
    }

    /// Glue: a `--config` file lands in the produced `ServerConfig`, flags
    /// override it, and the rate-limit validation fails closed.
    #[test]
    fn serve_config_wires_file_and_flags_into_server_config() {
        let _guard = env_lock();
        clear_serve_env();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("serve.json");
        std::fs::write(
            &path,
            r#"{"bind": "0.0.0.0:9999", "rate_limit": 5, "rate_burst": 9,
                "body_limit": 2048, "embedder": "hash"}"#,
        )
        .unwrap();

        // File values land in the config (with the documented burst default).
        let (config, bind) = serve_config(&serve_args(), Some(&path)).unwrap();
        assert_eq!(bind, "0.0.0.0:9999");
        assert_eq!(
            config.rate_limit,
            Some(recall_server::ratelimit::RateLimit {
                rate: 5.0,
                burst: 9.0
            })
        );
        assert_eq!(config.body_limit_bytes, 2048);
        assert_eq!(config.embedder.name(), "hash-256");
        assert!(config.api_key.is_none());

        // Flags override the file *per knob*: the rate comes from the flag,
        // while the burst (no flag) still comes from the file — each setting
        // follows the precedence chain independently.
        let mut args = serve_args();
        args.rate_limit = Some(1.0);
        args.body_limit = Some(4096);
        args.bind = Some("127.0.0.1:1".into());
        args.api_key = Some("k".into());
        let (config, bind) = serve_config(&args, Some(&path)).unwrap();
        assert_eq!(bind, "127.0.0.1:1");
        assert_eq!(
            config.rate_limit,
            Some(recall_server::ratelimit::RateLimit {
                rate: 1.0,
                burst: 9.0
            })
        );
        assert_eq!(config.body_limit_bytes, 4096);
        assert_eq!(config.api_key.as_deref(), Some("k"));

        // With no flag and no file, the burst default is max(2x rate, 10).
        let mut args = serve_args();
        args.rate_limit = Some(1.0);
        let (config, _) = serve_config(&args, None).unwrap();
        assert_eq!(
            config.rate_limit,
            Some(recall_server::ratelimit::RateLimit {
                rate: 1.0,
                burst: 10.0
            })
        );

        // Defaults without flags or file.
        let (config, bind) = serve_config(&serve_args(), None).unwrap();
        assert_eq!(bind, "127.0.0.1:8787");
        assert_eq!(
            config.body_limit_bytes,
            recall_server::DEFAULT_BODY_LIMIT_BYTES
        );
        assert!(config.rate_limit.is_none());
    }

    #[test]
    fn serve_config_fails_closed_on_bad_values() {
        let _guard = env_lock();
        clear_serve_env();
        let dir = tempfile::tempdir().unwrap();
        let write = |body: &str| {
            let path = dir.path().join("bad.json");
            std::fs::write(&path, body).unwrap();
            path
        };

        // Non-positive rate limit (file layer) is refused.
        let path = write(r#"{"rate_limit": -1}"#);
        let err = serve_config(&serve_args(), Some(&path)).unwrap_err();
        assert!(err.to_string().contains("rate limit must be"), "got: {err}");

        // Unknown file keys are refused (no silent typos).
        let path = write(r#"{"api_key": "secrets-do-not-belong-in-files"}"#);
        let err = serve_config(&serve_args(), Some(&path)).unwrap_err();
        assert!(
            err.root_cause().to_string().contains("unknown field"),
            "got: {err:#}"
        );

        // A burst smaller than the rate is refused by the limiter's rules.
        let mut args = serve_args();
        args.rate_limit = Some(50.0);
        args.rate_burst = Some(1.0);
        let err = serve_config(&args, None).unwrap_err();
        assert!(err.to_string().contains("invalid rate limit"), "got: {err}");

        // An unknown embedder name is refused before the server starts.
        let path = write(r#"{"embedder": "gpt4"}"#);
        let err = serve_config(&serve_args(), Some(&path)).unwrap_err();
        assert!(
            err.root_cause().to_string().contains("unknown embedder"),
            "got: {err:#}"
        );

        // A zero body limit clamps to 1 rather than disabling the limit.
        let mut args = serve_args();
        args.body_limit = Some(0);
        let (config, _) = serve_config(&args, None).unwrap();
        assert_eq!(config.body_limit_bytes, 1);
    }

    #[test]
    fn allow_origin_is_validated_fail_closed() {
        let _guard = env_lock();
        clear_serve_env();
        // A malformed origin refuses the startup instead of failing on the
        // first cross-origin request.
        let mut args = serve_args();
        args.allow_origin = vec!["not-a-url".into()];
        let err = serve_config(&args, None).unwrap_err();
        assert!(err.to_string().contains("allow-origin"), "got: {err}");
        // A well-formed origin passes through trimmed.
        let mut args = serve_args();
        args.allow_origin = vec![" https://agent.example ".into()];
        let (config, _) = serve_config(&args, None).unwrap();
        assert_eq!(config.allowed_origins, vec!["https://agent.example"]);
        // The config-file list applies when no flag is given.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("origins.json");
        std::fs::write(&path, r#"{"allow_origins": ["https://from-file.example"]}"#).unwrap();
        let (config, _) = serve_config(&serve_args(), Some(&path)).unwrap();
        assert_eq!(config.allowed_origins, vec!["https://from-file.example"]);
    }

    #[cfg(not(feature = "onnx"))]
    #[test]
    fn serve_config_embedder_onnx_without_feature_fails_closed() {
        let _guard = env_lock();
        clear_serve_env();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("onnx.json");
        std::fs::write(&path, r#"{"embedder": "onnx"}"#).unwrap();
        let err = serve_config(&serve_args(), Some(&path)).unwrap_err();
        assert!(
            err.root_cause().to_string().contains("onnx"),
            "error must name the embedder: {err:#}"
        );
    }
}
