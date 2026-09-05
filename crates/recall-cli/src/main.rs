//! recall-cli — scripting and serving entry point for Recall-MCP.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use recall_core::export::{ExportData, export, import};
use recall_core::{
    Embedder, HashEmbedder, NewMemory, RecallParams, SharedStore, SqliteStore, shared, unix_now,
};
use recall_server::{ServerConfig, build_router};

#[derive(Parser)]
#[command(
    name = "recall-cli",
    version,
    about = "Local-first semantic memory broker (MCP + HTTP + CLI)",
    propagate_version = true
)]
struct Cli {
    /// SQLite database file (created if missing). Applies to every subcommand.
    #[arg(long, global = true, default_value = "recall.db")]
    db: PathBuf,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run the server: HTTP API + MCP streamable-HTTP (and the web UI if built),
    /// or MCP over stdio with --stdio.
    Serve {
        /// Serve MCP over stdio instead of HTTP.
        #[arg(long)]
        stdio: bool,
        /// Bind address for the HTTP server (ignored with --stdio).
        #[arg(long, default_value = "127.0.0.1:8787")]
        bind: String,
        /// API key for /v1/* and /mcp (falls back to RECALL_MCP_API_KEY).
        #[arg(long)]
        api_key: Option<String>,
        /// Serve the built web UI from this directory at /.
        #[arg(long)]
        web_dir: Option<PathBuf>,
    },
    /// Store one memory (prints the created record as JSON).
    Remember {
        #[arg(long)]
        namespace: String,
        #[arg(long)]
        text: String,
        #[arg(long, value_delimiter = ',')]
        tags: Vec<String>,
        #[arg(long)]
        pinned: bool,
    },
    /// Recall memories (prints ranked hits with breakdowns as JSON).
    Recall {
        #[arg(long)]
        namespace: String,
        #[arg(long)]
        query: String,
        /// Number of hits.
        #[arg(long, default_value_t = 5)]
        k: usize,
        /// Recency decay constant in days; 0 disables the recency term.
        #[arg(long, default_value_t = 30.0)]
        tau_days: f64,
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
}

fn open_store(db: &PathBuf) -> Result<SharedStore> {
    let store = SqliteStore::open(db).with_context(|| format!("open database {}", db.display()))?;
    Ok(shared(store))
}

fn print_json(value: serde_json::Value) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(&value)?);
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
    bind: String,
    api_key: Option<String>,
    web_dir: Option<PathBuf>,
}

async fn serve(args: &ServeArgs) -> Result<()> {
    let store = open_store(&args.db)?;
    if args.stdio {
        tracing::info!("serving MCP over stdio");
        let state = recall_server::state::AppState::new(
            store,
            Arc::new(HashEmbedder::new_256()),
            effective_api_key(args.api_key.as_ref(), env_api_key()),
        );
        // stdio is the only stdout writer; tracing logs go to stderr.
        return recall_server::mcp::run_stdio_server(state).await;
    }

    let config = ServerConfig {
        api_key: effective_api_key(args.api_key.as_ref(), env_api_key()),
        web_dir: args.web_dir.clone(),
    };
    let app = build_router(store, &config);
    let listener = tokio::net::TcpListener::bind(&args.bind)
        .await
        .with_context(|| format!("bind {}", args.bind))?;
    let addr = listener.local_addr()?;
    tracing::info!(%addr, "recall-mcp HTTP server listening (REST + MCP at /mcp)");
    axum::serve(listener, app).await.context("server error")
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();
    let db = cli.db.clone();
    match &cli.command {
        Command::Serve {
            stdio,
            bind,
            api_key,
            web_dir,
        } => {
            serve(&ServeArgs {
                db,
                stdio: *stdio,
                bind: bind.clone(),
                api_key: api_key.clone(),
                web_dir: web_dir.clone(),
            })
            .await
        }
        Command::Remember {
            namespace,
            text,
            tags,
            pinned,
        } => {
            let store = open_store(&db)?;
            let embedder = HashEmbedder::new_256();
            // Scripted remember is agent-like: create the namespace if missing.
            let namespace = namespace.trim().to_string();
            if namespace.is_empty() {
                anyhow::bail!("namespace must not be empty");
            }
            let embedding = embedder.embed(text);
            // One guard for the whole read-modify sequence (the mutex is not re-entrant).
            let guard = store.lock().unwrap_or_else(|p| p.into_inner());
            guard.get_or_create_namespace(&namespace)?;
            let memory = guard.insert_memory(&NewMemory {
                namespace,
                text: text.clone(),
                tags: tags.clone(),
                source: Some("cli".to_string()),
                pinned: *pinned,
                created_at: Some(unix_now()),
                id: None,
                embedding: Some(embedding),
            })?;
            drop(guard);
            print_json(serde_json::to_value(&memory)?)
        }
        Command::Recall {
            namespace,
            query,
            k,
            tau_days,
        } => {
            let store = open_store(&db)?;
            let embedder: Arc<dyn Embedder> = Arc::new(HashEmbedder::new_256());
            let params = RecallParams {
                k: *k,
                tau_days: if *tau_days <= 0.0 {
                    None
                } else {
                    Some(*tau_days)
                },
                ..RecallParams::default()
            };
            let query_embedding = embedder.embed(query);
            let hits = store.lock().unwrap_or_else(|p| p.into_inner()).recall(
                namespace,
                query,
                Some(&query_embedding),
                &params,
                unix_now(),
            )?;
            print_json(serde_json::to_value(&hits)?)
        }
        Command::Export { out } => {
            let store = open_store(&db)?;
            let data = export(&*store.lock().unwrap_or_else(|p| p.into_inner()))?;
            let json = serde_json::to_string_pretty(&data)?;
            match out {
                Some(path) => {
                    std::fs::write(path, json)
                        .with_context(|| format!("write {}", path.display()))?;
                    println!(
                        "exported {} namespaces, {} memories to {}",
                        data.namespaces.len(),
                        data.memories.len(),
                        path.display()
                    );
                    Ok(())
                }
                None => {
                    println!("{json}");
                    Ok(())
                }
            }
        }
        Command::Import { file } => {
            let store = open_store(&db)?;
            let raw = std::fs::read_to_string(file)
                .with_context(|| format!("read {}", file.display()))?;
            let data: ExportData = serde_json::from_str(&raw)?;
            let report = import(&*store.lock().unwrap_or_else(|p| p.into_inner()), &data)?;
            print_json(serde_json::to_value(report)?)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
