//! SQLite-backed [`Store`] with an FTS5 keyword index (external content,
//! trigger-synced) and BLOB-stored embeddings (ADR-002).
//!
//! Encryption at rest (D-014): when opened with a [`StoreKey`], `memories.text`
//! stores only AEAD ciphertext (`enc:v1:...`) and the FTS index is built from
//! keyed HMAC token digests, so no plaintext reaches disk. The keyword pass
//! digests query tokens with the same key, keeping BM25 ranking identical.
//! Legacy plaintext databases are upgraded in place on the first keyed open.

use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};
use tracing::debug;

use crate::crypto::{StoreKey, key_check_context, text_context};
use crate::error::{RecallError, Result};
use crate::models::{Memory, Namespace, NamespaceCount, NewMemory, RecallHit, RecallParams, Stats};
use crate::scorer::{HybridScorer, RawScore};
use crate::store::Store;
use crate::util::{blob_to_f32, f32_to_blob, tokenize};

const SCHEMA: &str = r#"
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;
PRAGMA synchronous = NORMAL;

CREATE TABLE IF NOT EXISTS store_meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS namespaces (
    id         TEXT PRIMARY KEY,
    name       TEXT NOT NULL UNIQUE,
    created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS memories (
    id               TEXT PRIMARY KEY,
    namespace_id     TEXT NOT NULL REFERENCES namespaces(id) ON DELETE CASCADE,
    text             TEXT NOT NULL,
    fts_tokens       TEXT NOT NULL DEFAULT '',
    tags             TEXT NOT NULL DEFAULT '[]',
    embedding        BLOB,
    source           TEXT NOT NULL DEFAULT 'agent',
    pinned           INTEGER NOT NULL DEFAULT 0,
    created_at       INTEGER NOT NULL,
    last_accessed_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_memories_namespace ON memories(namespace_id);

CREATE VIRTUAL TABLE IF NOT EXISTS memories_fts USING fts5(
    fts_tokens,
    content = 'memories',
    content_rowid = 'rowid'
);

CREATE TRIGGER IF NOT EXISTS memories_fts_insert AFTER INSERT ON memories BEGIN
    INSERT INTO memories_fts(rowid, fts_tokens) VALUES (new.rowid, new.fts_tokens);
END;

CREATE TRIGGER IF NOT EXISTS memories_fts_delete AFTER DELETE ON memories BEGIN
    INSERT INTO memories_fts(memories_fts, rowid, fts_tokens)
    VALUES ('delete', old.rowid, old.fts_tokens);
END;

-- Keeps the external-content index exact for text rewrites (memory updates).
-- Access-time `last_accessed_at` updates must not churn the index, hence the
-- WHEN guard and the column restriction.
CREATE TRIGGER IF NOT EXISTS memories_fts_update AFTER UPDATE OF text, fts_tokens ON memories
WHEN old.fts_tokens IS NOT new.fts_tokens
BEGIN
    INSERT INTO memories_fts(memories_fts, rowid, fts_tokens)
    VALUES ('delete', old.rowid, old.fts_tokens);
    INSERT INTO memories_fts(rowid, fts_tokens) VALUES (new.rowid, new.fts_tokens);
END;
"#;

/// Minimal FTS5 expression builder: double-quoted OR of tokens, so arbitrary
/// user input can neither break MATCH syntax nor inject operators. When a key
/// is supplied (encrypted mode), each token is replaced by its keyed digest —
/// the same transformation applied to stored documents, so ranking is
/// unchanged while query and index stay plaintext-free.
pub fn build_fts_query(query: &str, key: Option<&StoreKey>) -> Option<String> {
    let tokens = tokenize(query);
    if tokens.is_empty() {
        return None;
    }
    Some(
        tokens
            .iter()
            .map(|t| {
                let term = match key {
                    Some(k) => k.token_digest(t),
                    None => t.replace('"', ""),
                };
                format!("\"{term}\"")
            })
            .collect::<Vec<_>>()
            .join(" OR "),
    )
}

pub struct SqliteStore {
    conn: Connection,
    key: Option<StoreKey>,
}

/// Metadata keys in `store_meta`.
const META_AT_REST: &str = "at_rest";
const META_KEY_CHECK: &str = "key_check";
const AT_REST_PLAIN: &str = "plain";
const AT_REST_ENC: &str = "enc_v1";
/// Known plaintext encrypted as the key check: a wrong key fails at open.
const KEY_CHECK_PLAINTEXT: &str = "recall-mcp key check v1";

/// Raw column view of a `memories` row, before tag parsing and decryption.
struct MemoryRow {
    id: String,
    namespace_id: String,
    text_stored: String,
    tags_json: String,
    embedding: Option<Vec<u8>>,
    source: String,
    pinned: bool,
    created_at: i64,
    last_accessed_at: i64,
}

const MEMORY_COLUMNS: &str =
    "id, namespace_id, text, tags, embedding, source, pinned, created_at, last_accessed_at";

fn memory_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<MemoryRow> {
    Ok(MemoryRow {
        id: row.get("id")?,
        namespace_id: row.get("namespace_id")?,
        text_stored: row.get("text")?,
        tags_json: row.get("tags")?,
        embedding: row.get("embedding")?,
        source: row.get("source")?,
        pinned: row.get::<_, i64>("pinned")? != 0,
        created_at: row.get("created_at")?,
        last_accessed_at: row.get("last_accessed_at")?,
    })
}

impl MemoryRow {
    /// Parse tags and decrypt text (keyed store) into a domain [`Memory`].
    fn into_memory(self, key: Option<&StoreKey>) -> Result<Memory> {
        let tags = serde_json::from_str(&self.tags_json).map_err(RecallError::Serialization)?;
        let text = match (key, StoreKey::is_encrypted(&self.text_stored)) {
            (Some(k), true) => k.decrypt(&text_context(&self.id), &self.text_stored)?,
            // Legacy/mixed rows written before the key existed read as-is.
            (_, false) => self.text_stored,
            (None, true) => {
                return Err(RecallError::Crypto(
                    "encrypted memory value present but no key was provided".into(),
                ));
            }
        };
        Ok(Memory {
            id: self.id,
            namespace_id: self.namespace_id,
            text,
            tags,
            embedding: self.embedding.as_deref().and_then(blob_to_f32),
            source: self.source,
            pinned: self.pinned,
            created_at: self.created_at,
            last_accessed_at: self.last_accessed_at,
        })
    }
}

impl SqliteStore {
    /// Open (creating if needed) a plaintext database file. `:memory:` is
    /// honored. Refuses databases that are encrypted at rest — open with
    /// [`SqliteStore::open_with_key`] instead.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        Self::open_inner(path, None)
    }

    /// Open (creating if needed) with encryption at rest. New memories are
    /// stored encrypted; an existing plaintext database is upgraded in place
    /// on first open (D-014). The key is verified against a stored check
    /// value, so a wrong key fails immediately.
    pub fn open_with_key<P: AsRef<Path>>(path: P, key: &StoreKey) -> Result<Self> {
        Self::open_inner(path, Some(key.clone()))
    }

    /// In-memory database, mainly for tests and ephemeral tools (plaintext).
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn, key: None })
    }

    /// In-memory encrypted database (tests).
    pub fn open_in_memory_with_key(key: &StoreKey) -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(SCHEMA)?;
        let store = Self {
            conn,
            key: Some(key.clone()),
        };
        store.init_mode()?;
        Ok(store)
    }

    fn open_inner<P: AsRef<Path>>(path: P, key: Option<StoreKey>) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch(SCHEMA)?;
        let needs_upgrade = legacy_schema(&conn)?;
        if needs_upgrade {
            upgrade_legacy_schema(&conn)?;
        }
        let store = Self { conn, key };
        // Legacy layout always stored plaintext.
        if needs_upgrade {
            store.copy_plaintext_fts_tokens()?;
        }
        store.init_mode()?;
        if store.key.is_some() && needs_upgrade {
            // The plaintext era wrote legacy rows and (pre-upgrade) FTS
            // content into pages that may linger after deletion; VACUUM
            // compacts them away after the in-place upgrade.
            store.conn.execute_batch("VACUUM")?;
        }
        Ok(store)
    }

    /// Enforce mode consistency and complete the keyed-mode setup:
    /// verify the stored key check, encrypt any remaining plaintext rows,
    /// and persist the mode marker.
    fn init_mode(&self) -> Result<()> {
        let marker = self.meta_get(META_AT_REST)?;
        match (&self.key, marker.as_deref()) {
            (None, Some(AT_REST_ENC)) => {
                return Err(RecallError::Crypto(
                    "database is encrypted at rest: set RECALL_MCP_KEY (64 hex chars) \
                     or RECALL_MCP_KEY_FILE to open it"
                        .into(),
                ));
            }
            (Some(_), Some(AT_REST_ENC)) => self.verify_key_check()?,
            // First keyed open (fresh or legacy/marked-plain): encrypt everything.
            (Some(_), _) => {
                self.encrypt_remaining_plaintext()?;
                self.meta_set(META_AT_REST, AT_REST_ENC)?;
                self.meta_set(
                    META_KEY_CHECK,
                    &self
                        .key
                        .as_ref()
                        .expect("checked above")
                        .encrypt(&key_check_context(), KEY_CHECK_PLAINTEXT),
                )?;
            }
            (None, _) => {
                if marker.is_none() {
                    self.meta_set(META_AT_REST, AT_REST_PLAIN)?;
                }
            }
        }
        Ok(())
    }

    /// Open-time proof that the supplied key is the one the database was
    /// written with (AEAD decrypt fails on any other key).
    fn verify_key_check(&self) -> Result<()> {
        let key = self.key.as_ref().expect("keyed open");
        if let Some(check) = self.meta_get(META_KEY_CHECK)? {
            key.decrypt(&key_check_context(), &check)?;
        }
        Ok(())
    }

    /// Encrypt every remaining plaintext row in one transaction (idempotent:
    /// already-encrypted rows are skipped). The FTS update trigger keeps the
    /// index exact as each row's token digest replaces its plaintext copy.
    fn encrypt_remaining_plaintext(&self) -> Result<()> {
        let key = match &self.key {
            Some(k) => k,
            None => return Ok(()),
        };
        let ids: Vec<String> = {
            let mut stmt = self
                .conn
                .prepare("SELECT id FROM memories WHERE text NOT LIKE 'enc:v1:%'")?;
            let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
            rows.collect::<std::result::Result<Vec<_>, _>>()?
        };
        if ids.is_empty() {
            return Ok(());
        }
        self.conn.execute_batch("BEGIN")?;
        let result = (|| -> Result<()> {
            let mut stmt = self
                .conn
                .prepare_cached("UPDATE memories SET text = ?1, fts_tokens = ?2 WHERE id = ?3")?;
            for id in &ids {
                let memory = self
                    .get_memory(id)?
                    .ok_or_else(|| RecallError::MemoryNotFound(id.clone()))?;
                let encrypted = key.encrypt(&text_context(id), &memory.text);
                let tokens = key.fts_tokens(&memory.text);
                stmt.execute(params![encrypted, tokens, id])?;
            }
            Ok(())
        })();
        match result {
            Ok(()) => self.conn.execute_batch("COMMIT")?,
            Err(e) => {
                let _ = self.conn.execute_batch("ROLLBACK");
                return Err(e);
            }
        }
        debug!(count = ids.len(), "encrypted existing plaintext memories");
        Ok(())
    }

    /// Legacy (pre-encryption-schema) databases indexed `memories.text`
    /// directly. Copy it into the new `fts_tokens` column so the rebuilt
    /// index matches; the keyed-mode pass replaces it with digests next.
    /// The copy must run with triggers dropped: firing the update trigger
    /// with values that were never indexed corrupts an external-content
    /// FTS5 index.
    fn copy_plaintext_fts_tokens(&self) -> Result<()> {
        self.conn.execute_batch(
            "DROP TRIGGER IF EXISTS memories_fts_insert;
             DROP TRIGGER IF EXISTS memories_fts_delete;
             DROP TRIGGER IF EXISTS memories_fts_update;
             UPDATE memories SET fts_tokens = text;",
        )?;
        self.conn.execute_batch(SCHEMA)?;
        self.conn
            .execute_batch("INSERT INTO memories_fts(memories_fts) VALUES ('rebuild');")?;
        Ok(())
    }

    fn meta_get(&self, key: &str) -> Result<Option<String>> {
        let mut stmt = self
            .conn
            .prepare_cached("SELECT value FROM store_meta WHERE key = ?1")?;
        Ok(stmt
            .query_row(params![key], |r| r.get::<_, String>(0))
            .optional()?)
    }

    fn meta_set(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO store_meta (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    /// Storage form of memory text: ciphertext in keyed mode, plaintext
    /// otherwise; plus the FTS document (keyed digests or the text itself).
    fn stored_text(&self, id: &str, text: &str) -> (String, String) {
        match &self.key {
            Some(k) => (k.encrypt(&text_context(id), text), k.fts_tokens(text)),
            None => (text.to_string(), text.to_string()),
        }
    }

    fn namespace_id(&self, name: &str) -> Result<Option<String>> {
        let mut stmt = self
            .conn
            .prepare_cached("SELECT id FROM namespaces WHERE name = ?1")?;
        Ok(stmt
            .query_row(params![name], |r| r.get::<_, String>(0))
            .optional()?)
    }
}

/// True when `memories` predates the encryption schema (no `fts_tokens`
/// column, FTS index over `text`).
fn legacy_schema(conn: &Connection) -> Result<bool> {
    let has_column: i64 = conn.query_row(
        "SELECT COUNT(*) FROM pragma_table_info('memories') WHERE name = 'fts_tokens'",
        [],
        |r| r.get(0),
    )?;
    Ok(has_column == 0 && table_exists(conn, "memories")?)
}

fn table_exists(conn: &Connection, name: &str) -> Result<bool> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
        params![name],
        |r| r.get(0),
    )?;
    Ok(n > 0)
}

/// One-time migration of a pre-encryption database to schema v2:
/// add `fts_tokens`, recreate the FTS virtual table over it, and drop the
/// old text-based triggers (fresh ones are created by `SCHEMA` on next run /
/// below). Content is rebuilt by [`SqliteStore::copy_plaintext_fts_tokens`].
fn upgrade_legacy_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"BEGIN;
           DROP TRIGGER IF EXISTS memories_fts_insert;
           DROP TRIGGER IF EXISTS memories_fts_delete;
           DROP TRIGGER IF EXISTS memories_fts_update;
           ALTER TABLE memories ADD COLUMN fts_tokens TEXT NOT NULL DEFAULT '';
           DROP TABLE memories_fts;
           CREATE VIRTUAL TABLE memories_fts USING fts5(
               fts_tokens,
               content = 'memories',
               content_rowid = 'rowid'
           );
           COMMIT;"#,
    )?;
    conn.execute_batch(SCHEMA)?;
    debug!("upgraded legacy schema to encryption-capable layout");
    Ok(())
}

impl Store for SqliteStore {
    fn create_namespace(&self, name: &str) -> Result<Namespace> {
        let name = name.trim();
        if name.is_empty() {
            return Err(RecallError::InvalidInput(
                "namespace name must not be empty".into(),
            ));
        }
        if let Some(existing) = self.find_namespace(name)? {
            return Err(RecallError::DuplicateNamespace(existing.name));
        }
        let id = new_id();
        let now = crate::unix_now();
        self.conn.execute(
            "INSERT INTO namespaces (id, name, created_at) VALUES (?1, ?2, ?3)",
            params![id, name, now],
        )?;
        debug!(namespace = name, "created namespace");
        Ok(Namespace {
            id,
            name: name.to_string(),
            created_at: now,
        })
    }

    fn get_or_create_namespace(&self, name: &str) -> Result<Namespace> {
        if let Some(existing) = self.find_namespace(name)? {
            return Ok(existing);
        }
        self.create_namespace(name)
    }

    fn list_namespaces(&self) -> Result<Vec<Namespace>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, name, created_at FROM namespaces ORDER BY created_at, name")?;
        let rows = stmt.query_map([], |r| {
            Ok(Namespace {
                id: r.get(0)?,
                name: r.get(1)?,
                created_at: r.get(2)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    fn find_namespace(&self, name: &str) -> Result<Option<Namespace>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, name, created_at FROM namespaces WHERE name = ?1")?;
        Ok(stmt
            .query_row(params![name], |r| {
                Ok(Namespace {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    created_at: r.get(2)?,
                })
            })
            .optional()?)
    }

    fn insert_memory(&self, memory: &NewMemory) -> Result<Memory> {
        let text = memory.text.trim();
        if text.is_empty() {
            return Err(RecallError::InvalidInput(
                "memory text must not be empty".into(),
            ));
        }
        let ns = self
            .namespace_id(&memory.namespace)?
            .ok_or_else(|| RecallError::NamespaceNotFound(memory.namespace.clone()))?;

        let id = match &memory.id {
            Some(id) if id.trim().is_empty() => {
                return Err(RecallError::InvalidInput(
                    "memory id must not be blank".into(),
                ));
            }
            Some(id) => id.clone(),
            None => new_id(),
        };
        let now = crate::unix_now();
        let created_at = memory.created_at.unwrap_or(now);
        let tags = serde_json::to_string(&memory.tags)?;
        let embedding_blob = memory.embedding.as_deref().map(f32_to_blob);
        let source = memory.source.clone().unwrap_or_else(|| "agent".to_string());
        let (stored_text, fts_tokens) = self.stored_text(&id, text);

        // Distinguish "duplicate id" from other constraint failures for a precise error.
        let inserted = self.conn.execute(
            "INSERT INTO memories (id, namespace_id, text, fts_tokens, tags, embedding, source, pinned, created_at, last_accessed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)",
            params![
                id,
                ns,
                stored_text,
                fts_tokens,
                tags,
                embedding_blob,
                source,
                i64::from(memory.pinned),
                created_at
            ],
        );
        match inserted {
            Ok(_) => {}
            Err(rusqlite::Error::SqliteFailure(e, _))
                if e.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                return Err(RecallError::InvalidInput(format!(
                    "memory id already exists: {id}"
                )));
            }
            Err(e) => return Err(e.into()),
        }
        debug!(memory_id = %id, namespace = %memory.namespace, "inserted memory");
        self.get_memory(&id)?
            .ok_or_else(|| RecallError::InvalidInput("inserted memory not found".into()))
    }

    fn get_memory(&self, id: &str) -> Result<Option<Memory>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {MEMORY_COLUMNS} FROM memories WHERE id = ?1"
        ))?;
        let mut rows = stmt.query(params![id])?;
        match rows.next()? {
            Some(row) => Ok(Some(memory_row(row)?.into_memory(self.key.as_ref())?)),
            None => Ok(None),
        }
    }

    fn delete_memory(&self, id: &str) -> Result<bool> {
        let n = self
            .conn
            .execute("DELETE FROM memories WHERE id = ?1", params![id])?;
        Ok(n > 0)
    }

    fn list_memories(&self, namespace: Option<&str>, limit: usize) -> Result<Vec<Memory>> {
        if limit == 0 {
            return Err(RecallError::InvalidInput("limit must be > 0".into()));
        }
        let sql = match namespace {
            Some(_) => format!(
                "SELECT {MEMORY_COLUMNS} FROM memories WHERE namespace_id = ?1
                 ORDER BY created_at DESC, id ASC LIMIT ?2"
            ),
            None => format!(
                "SELECT {MEMORY_COLUMNS} FROM memories
                 ORDER BY created_at DESC, id ASC LIMIT ?1"
            ),
        };
        let ns = match namespace {
            Some(name) => Some(
                self.namespace_id(name)?
                    .ok_or_else(|| RecallError::NamespaceNotFound(name.to_string()))?,
            ),
            None => None,
        };
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = match ns {
            Some(ns) => stmt.query_map(params![ns, limit as i64], memory_row)?,
            None => stmt.query_map(params![limit as i64], memory_row)?,
        };
        let mut out = Vec::new();
        for row in rows {
            out.push(row?.into_memory(self.key.as_ref())?);
        }
        Ok(out)
    }

    fn recall(
        &self,
        namespace: &str,
        query: &str,
        query_embedding: Option<&[f32]>,
        p: &RecallParams,
        now: i64,
    ) -> Result<Vec<RecallHit>> {
        p.validate()?;
        if query.trim().is_empty() {
            return Err(RecallError::InvalidInput("query must not be empty".into()));
        }
        let ns = self
            .namespace_id(namespace)?
            .ok_or_else(|| RecallError::NamespaceNotFound(namespace.to_string()))?;

        // Pass 1 — keyword candidates from FTS5 (bm25, more negative = better).
        let mut keyword: std::collections::HashMap<String, f64> = std::collections::HashMap::new();
        if let Some(fts_query) = build_fts_query(query, self.key.as_ref()) {
            let mut stmt = self.conn.prepare(
                "SELECT m.id, bm25(memories_fts) AS rank_score
                 FROM memories_fts
                 JOIN memories m ON m.rowid = memories_fts.rowid
                 WHERE memories_fts MATCH ?1 AND m.namespace_id = ?2
                 ORDER BY rank_score
                 LIMIT ?3",
            )?;
            let rows = stmt.query_map(params![fts_query, ns, p.candidate_cap as i64], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?))
            })?;
            for row in rows {
                let (id, rank) = row?;
                keyword.insert(id, -rank);
            }
        }
        let max_bm25 = keyword.values().copied().fold(0.0f64, f64::max);

        // Pass 2 — brute-force cosine over the namespace (exact, explainable; ADR-002).
        let query_vec: Option<&[f32]> = query_embedding.filter(|v| !v.is_empty());
        let mut rows_by_id: std::collections::HashMap<String, Memory> =
            std::collections::HashMap::new();
        {
            let mut stmt = self.conn.prepare(&format!(
                "SELECT {MEMORY_COLUMNS} FROM memories WHERE namespace_id = ?1 LIMIT ?2"
            ))?;
            let rows = stmt.query_map(params![ns, p.scan_cap as i64], memory_row)?;
            for row in rows {
                let m = row?.into_memory(self.key.as_ref())?;
                rows_by_id.insert(m.id.clone(), m);
            }
        }
        let mut vector: std::collections::HashMap<String, f64> = std::collections::HashMap::new();
        if let Some(qv) = query_vec {
            for (id, m) in &rows_by_id {
                if let Some(mv) = &m.embedding {
                    let sim = crate::util::cosine(qv, mv);
                    if sim != 0.0 {
                        vector.insert(id.clone(), sim);
                    }
                }
            }
        }

        // Merge candidate sets and score.
        let scorer = HybridScorer {
            weights: p.weights,
            tau_days: p.tau_days,
            pinned_boost: p.pinned_boost,
        };
        let mut hits: Vec<RecallHit> = Vec::new();
        let mut candidate_ids: std::collections::HashSet<&String> = keyword.keys().collect();
        candidate_ids.extend(vector.keys());
        for id in candidate_ids {
            let memory = match rows_by_id.get(id) {
                Some(m) => m,
                // A deleted row between passes is simply skipped.
                None => continue,
            };
            let raw_bm25 = keyword.get(id).copied().unwrap_or(0.0);
            let bm25_normalized = if max_bm25 > 0.0 {
                raw_bm25 / max_bm25
            } else {
                0.0
            };
            let cosine = vector.get(id).copied().unwrap_or(0.0);
            let breakdown = scorer.score(
                RawScore {
                    bm25_normalized,
                    cosine,
                    created_at: memory.created_at,
                    pinned: memory.pinned,
                },
                now,
            );
            hits.push(RecallHit {
                memory: memory.clone(),
                breakdown,
            });
        }

        hits.sort_by(|a, b| {
            b.breakdown
                .total
                .partial_cmp(&a.breakdown.total)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(b.memory.created_at.cmp(&a.memory.created_at))
                .then(a.memory.id.cmp(&b.memory.id))
        });
        hits.truncate(p.k);

        if !hits.is_empty() {
            for hit in &mut hits {
                self.conn.execute(
                    "UPDATE memories SET last_accessed_at = ?1 WHERE id = ?2",
                    params![now, hit.memory.id],
                )?;
                // Keep the returned view consistent with the database.
                hit.memory.last_accessed_at = now;
            }
        }
        debug!(namespace, hits = hits.len(), "recall complete");
        Ok(hits)
    }

    fn stats(&self) -> Result<Stats> {
        let total_namespaces = self
            .conn
            .query_row("SELECT COUNT(*) FROM namespaces", [], |r| {
                r.get::<_, i64>(0)
            })?;
        let total_memories = self
            .conn
            .query_row("SELECT COUNT(*) FROM memories", [], |r| r.get::<_, i64>(0))?
            as u64;
        let pinned =
            self.conn
                .query_row("SELECT COUNT(*) FROM memories WHERE pinned = 1", [], |r| {
                    r.get::<_, i64>(0)
                })?;
        let mut stmt = self.conn.prepare(
            "SELECT n.name, COUNT(m.id) AS c
             FROM namespaces n LEFT JOIN memories m ON m.namespace_id = n.id
             GROUP BY n.id ORDER BY c DESC, n.name",
        )?;
        let per = stmt
            .query_map([], |r| {
                Ok(NamespaceCount {
                    namespace: r.get(0)?,
                    count: r.get::<_, i64>(1)? as u64,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(Stats {
            total_namespaces: total_namespaces as u64,
            total_memories,
            pinned_memories: pinned as u64,
            per_namespace: per,
        })
    }
}

fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embed::Embedder as _;
    use crate::embed::HashEmbedder;
    use crate::models::Weights;

    fn store() -> SqliteStore {
        SqliteStore::open_in_memory().expect("in-memory db")
    }

    fn keyed() -> SqliteStore {
        SqliteStore::open_in_memory_with_key(&test_key()).expect("in-memory keyed db")
    }

    fn test_key() -> StoreKey {
        StoreKey::from_hex("00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff")
            .expect("valid hex key")
    }

    fn ns(s: &SqliteStore, name: &str) -> Namespace {
        s.create_namespace(name).expect("namespace")
    }

    fn mem(namespace: &str, text: &str) -> NewMemory {
        NewMemory {
            namespace: namespace.into(),
            text: text.into(),
            tags: vec![],
            source: None,
            pinned: false,
            created_at: None,
            id: None,
            embedding: None,
        }
    }

    #[test]
    fn fts_query_builder_quotes_and_ors() {
        assert_eq!(
            build_fts_query("Deploy on Friday", None),
            Some("\"deploy\" OR \"on\" OR \"friday\"".to_string())
        );
        assert_eq!(build_fts_query("  ", None), None);
        assert_eq!(build_fts_query("", None), None);
        // Quotes inside tokens are stripped, so MATCH syntax can't be broken.
        assert_eq!(
            build_fts_query("a\"b OR c", None),
            Some("\"a\" OR \"b\" OR \"or\" OR \"c\"".to_string())
        );
        assert_eq!(build_fts_query("\"\"", None), None);
    }

    #[test]
    fn fts_query_builder_digests_tokens_in_keyed_mode() {
        let k = StoreKey::from_hex(&"ab".repeat(32)).unwrap();
        let plain = build_fts_query("deploy payments", None).unwrap();
        let keyed = build_fts_query("deploy payments", Some(&k)).unwrap();
        assert_eq!(
            keyed,
            format!(
                "\"{}\" OR \"{}\"",
                k.token_digest("deploy"),
                k.token_digest("payments")
            )
        );
        assert_ne!(plain, keyed);
        assert!(
            !keyed.contains("deploy"),
            "digest mode must not leak tokens"
        );
        assert_eq!(
            build_fts_query("deploy", Some(&k)),
            build_fts_query("deploy", Some(&k)),
            "digests are deterministic"
        );
    }

    #[test]
    fn namespace_crud_and_errors() {
        let s = store();
        assert!(s.list_namespaces().unwrap().is_empty());
        let w = ns(&s, "work");
        assert_eq!(w.name, "work");
        let dup = s.create_namespace("work").unwrap_err();
        assert!(matches!(dup, RecallError::DuplicateNamespace(_)));
        assert!(
            s.create_namespace("  ")
                .unwrap_err()
                .to_string()
                .contains("must not be empty")
        );
        let listed = s.list_namespaces().unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, w.id);
        assert_eq!(s.find_namespace("work").unwrap().unwrap().id, w.id);
        assert!(s.find_namespace("nope").unwrap().is_none());
        // get_or_create returns existing without error.
        assert_eq!(s.get_or_create_namespace("work").unwrap().id, w.id);
        assert_eq!(s.get_or_create_namespace("fresh").unwrap().name, "fresh");
    }

    #[test]
    fn memory_crud_roundtrip_with_embedding_and_tags() {
        let s = store();
        ns(&s, "w");
        let embedding = vec![0.5f32; 8];
        let m = s
            .insert_memory(&NewMemory {
                tags: vec!["rust".into(), "db".into()],
                source: Some("human".into()),
                pinned: true,
                created_at: Some(1_700_000_000),
                embedding: Some(embedding.clone()),
                ..mem("w", "  SQLite stores embeddings as blobs.  ")
            })
            .unwrap();
        assert_eq!(m.text, "SQLite stores embeddings as blobs.");
        assert_eq!(m.tags, vec!["rust", "db"]);
        assert_eq!(m.source, "human");
        assert!(m.pinned);
        assert_eq!(m.created_at, 1_700_000_000);
        assert_eq!(m.embedding, Some(embedding));
        assert_eq!(m.last_accessed_at, m.created_at);

        let fetched = s.get_memory(&m.id).unwrap().unwrap();
        assert_eq!(fetched, m);
        assert!(s.get_memory("missing").unwrap().is_none());
        assert!(s.delete_memory(&m.id).unwrap());
        assert!(!s.delete_memory(&m.id).unwrap());
        assert!(s.get_memory(&m.id).unwrap().is_none());
    }

    #[test]
    fn insert_validates_text_namespace_and_blank_id() {
        let s = store();
        assert!(matches!(
            s.insert_memory(&mem("ghost", "hi")).unwrap_err(),
            RecallError::NamespaceNotFound(_)
        ));
        ns(&s, "w");
        assert!(matches!(
            s.insert_memory(&mem("w", "   ")).unwrap_err(),
            RecallError::InvalidInput(_)
        ));
        let blank_id = NewMemory {
            id: Some("  ".into()),
            ..mem("w", "hello")
        };
        assert!(s.insert_memory(&blank_id).is_err());
    }

    #[test]
    fn duplicate_memory_id_is_reported_cleanly() {
        let s = store();
        ns(&s, "w");
        let m = NewMemory {
            id: Some("fixed-id".into()),
            ..mem("w", "first")
        };
        s.insert_memory(&m).unwrap();
        let again = NewMemory {
            id: Some("fixed-id".into()),
            ..mem("w", "second")
        };
        let err = s.insert_memory(&again).unwrap_err();
        assert!(err.to_string().contains("already exists"), "got: {err}");
    }

    #[test]
    fn insert_fills_defaults() {
        let s = store();
        ns(&s, "w");
        let m = s.insert_memory(&mem("w", "plain")).unwrap();
        assert_eq!(m.source, "agent");
        assert!(!m.pinned);
        assert!((crate::unix_now() - m.created_at).abs() < 5);
        assert!(m.embedding.is_none());
    }

    #[test]
    fn list_memories_filters_by_namespace_and_orders_newest_first() {
        let s = store();
        ns(&s, "a");
        ns(&s, "b");
        s.insert_memory(&NewMemory {
            created_at: Some(100),
            ..mem("a", "old a")
        })
        .unwrap();
        s.insert_memory(&NewMemory {
            created_at: Some(300),
            ..mem("a", "new a")
        })
        .unwrap();
        s.insert_memory(&NewMemory {
            created_at: Some(200),
            ..mem("b", "mid b")
        })
        .unwrap();

        let all_a = s.list_memories(Some("a"), 10).unwrap();
        assert_eq!(
            all_a.iter().map(|m| m.text.as_str()).collect::<Vec<_>>(),
            vec!["new a", "old a"]
        );
        let all = s.list_memories(None, 10).unwrap();
        assert_eq!(all.len(), 3);
        let one = s.list_memories(None, 1).unwrap();
        assert_eq!(one.len(), 1);
        assert_eq!(one[0].text, "new a");
        assert!(matches!(
            s.list_memories(Some("ghost"), 5).unwrap_err(),
            RecallError::NamespaceNotFound(_)
        ));
        assert!(s.list_memories(None, 0).is_err());
    }

    #[test]
    fn stats_counts_namespaces_memories_and_pinned() {
        let s = store();
        ns(&s, "a");
        ns(&s, "b");
        s.insert_memory(&NewMemory {
            pinned: true,
            ..mem("a", "p1")
        })
        .unwrap();
        s.insert_memory(&mem("a", "p2")).unwrap();
        let st = s.stats().unwrap();
        assert_eq!(st.total_namespaces, 2);
        assert_eq!(st.total_memories, 2);
        assert_eq!(st.pinned_memories, 1);
        assert_eq!(st.per_namespace.len(), 2);
        assert_eq!(st.per_namespace[0].namespace, "a");
        assert_eq!(st.per_namespace[0].count, 2);
        assert_eq!(st.per_namespace[1].count, 0);
    }

    #[test]
    fn recall_validates_params_query_and_namespace() {
        let s = store();
        let e = HashEmbedder::new_256();
        let p = RecallParams::default();
        assert!(matches!(
            s.recall("ghost", "q", Some(&e.embed("q")[..]), &p, 0)
                .unwrap_err(),
            RecallError::NamespaceNotFound(_)
        ));
        ns(&s, "w");
        assert!(matches!(
            s.recall("w", "   ", Some(&e.embed("q")[..]), &p, 0)
                .unwrap_err(),
            RecallError::InvalidInput(_)
        ));
        let bad_k = RecallParams {
            k: 0,
            ..Default::default()
        };
        assert!(s.recall("w", "q", None, &bad_k, 0).is_err());
    }

    #[test]
    fn recall_ranks_keyword_vector_and_pinned_with_breakdowns() {
        let s = store();
        let e = HashEmbedder::new_256();
        ns(&s, "w");
        let now = 1_786_000_000;
        let day = 86_400;
        let text = "deploy the payments service on friday";
        // Four memories chosen so components can be compared in isolation:
        // fresh full match, stale identical match, pinned stale identical match, and a non-match.
        let target = s
            .insert_memory(&NewMemory {
                created_at: Some(now - 100),
                embedding: Some(e.embed(text)),
                ..mem("w", text)
            })
            .unwrap();
        let stale = s
            .insert_memory(&NewMemory {
                created_at: Some(now - 30 * day),
                embedding: Some(e.embed(text)),
                ..mem("w", text)
            })
            .unwrap();
        let pinned = s
            .insert_memory(&NewMemory {
                created_at: Some(now - 30 * day),
                pinned: true,
                embedding: Some(e.embed(text)),
                ..mem("w", text)
            })
            .unwrap();
        s.insert_memory(&NewMemory {
            created_at: Some(now),
            embedding: Some(e.embed("weekly team sync notes")),
            ..mem("w", "weekly team sync notes")
        })
        .unwrap();

        let q = e.embed("deploy payments service");
        let hits = s
            .recall(
                "w",
                "deploy payments service",
                Some(&q),
                &RecallParams::default(),
                now,
            )
            .unwrap();
        assert!(!hits.is_empty());

        // Every hit carries a complete, consistent breakdown.
        for h in &hits {
            let b = &h.breakdown;
            let recomputed =
                0.45f64.mul_add(b.bm25, 0.45 * b.vector) + 0.10 * b.recency + b.pinned_boost;
            assert!(
                (recomputed - b.total).abs() < 1e-9,
                "breakdown must sum to total"
            );
            assert!((0.0..=1.0).contains(&b.bm25));
        }

        let by_id = |id: &str| hits.iter().find(|h| h.memory.id == id).unwrap();
        // The pinned copy ranks above the otherwise-identical unpinned copy by exactly the boost.
        let pinned_hit = by_id(&pinned.id);
        let stale_hit = by_id(&stale.id);
        assert!(
            pinned_hit.breakdown.total > stale_hit.breakdown.total,
            "pinned should outrank its unpinned twin"
        );
        assert!((pinned_hit.breakdown.total - stale_hit.breakdown.total - 0.2).abs() < 1e-9);
        // The fresh match beats the 30-day-old one on recency alone (identical text).
        let target_hit = by_id(&target.id);
        assert!(target_hit.breakdown.recency > stale_hit.breakdown.recency);
        assert!(target_hit.breakdown.total > stale_hit.breakdown.total);
        // Deterministic total ordering: pinned twin > fresh match > stale twin
        // (boost 0.2 > fresh recency ~0.1 > stale recency ~0). This is the
        // documented ranking behavior of the default weights.
        let ids = hits
            .iter()
            .map(|h| h.memory.id.as_str())
            .collect::<Vec<_>>();
        let pos = |id: &str| ids.iter().position(|i| *i == id).unwrap();
        assert!(pos(&pinned.id) < pos(&target.id));
        assert!(pos(&target.id) < pos(&stale.id));
        assert!(pinned_hit.breakdown.total > target_hit.breakdown.total);
        assert!(target_hit.breakdown.total > stale_hit.breakdown.total);

        // Recall refreshes last_accessed_at, both in the returned hits and on disk.
        assert_eq!(target_hit.memory.last_accessed_at, now);
        let touched = s.get_memory(&target.id).unwrap().unwrap();
        assert_eq!(touched.last_accessed_at, now);
    }

    #[test]
    fn recall_without_embedding_still_ranks_by_keyword() {
        let s = store();
        let e = HashEmbedder::new_256();
        ns(&s, "w");
        let now = 1_786_000_000;
        let hit_text = "rust ownership rules";
        s.insert_memory(&NewMemory {
            embedding: Some(e.embed(hit_text)),
            created_at: Some(now),
            ..mem("w", hit_text)
        })
        .unwrap();
        let hits = s
            .recall("w", "rust ownership", None, &RecallParams::default(), now)
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert!((hits[0].breakdown.vector - 0.0).abs() < 1e-9);
        assert!(hits[0].breakdown.bm25 > 0.0);
        assert!(hits[0].breakdown.total > 0.0);
    }

    #[test]
    fn recall_with_zero_query_embedding_skips_vector_pass() {
        let s = store();
        let e = HashEmbedder::new_256();
        ns(&s, "w");
        s.insert_memory(&NewMemory {
            embedding: Some(e.embed("alpha beta")),
            created_at: Some(1),
            ..mem("w", "alpha beta")
        })
        .unwrap();
        let zero = vec![0.0f32; 256];
        let hits = s
            .recall("w", "alpha", Some(&zero), &RecallParams::default(), 10)
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].breakdown.vector, 0.0);
    }

    #[test]
    fn recall_no_match_returns_empty_and_decay_disabled_zeroes_recency() {
        let s = store();
        let e = HashEmbedder::new_256();
        ns(&s, "w");
        s.insert_memory(&mem("w", "something unrelated")).unwrap();
        let q = e.embed("zzzqqq");
        let hits = s
            .recall("w", "zzzqqq", Some(&q), &RecallParams::default(), 5)
            .unwrap();
        assert!(hits.is_empty());

        let p = RecallParams {
            tau_days: None,
            ..Default::default()
        };
        let hits = s
            .recall("w", "something", Some(&e.embed("something")[..]), &p, 5)
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].breakdown.recency, 0.0);
        assert!(hits[0].breakdown.total > 0.0);
    }

    #[test]
    fn recall_respects_k_and_fills_vector_component() {
        let s = store();
        let e = HashEmbedder::new_256();
        ns(&s, "w");
        let now = 1_000;
        s.insert_memory(&NewMemory {
            created_at: Some(now),
            embedding: Some(e.embed("ferries")),
            ..mem("w", "ferries")
        })
        .unwrap();
        for i in 1..4 {
            let text = format!("note number {i} about ferries");
            s.insert_memory(&NewMemory {
                created_at: Some(now - i),
                embedding: Some(e.embed(&text)),
                ..mem("w", &text)
            })
            .unwrap();
        }
        let p = RecallParams {
            k: 2,
            ..Default::default()
        };
        let q = e.embed("ferries");
        let hits = s.recall("w", "ferries", Some(&q), &p, now).unwrap();
        assert_eq!(hits.len(), 2);
        // The exact-text, newest memory wins; bm25 normalization crowns it at 1.0
        // (shortest matching doc) and its embedding is identical to the query.
        assert!((hits[0].breakdown.bm25 - 1.0).abs() < 1e-9);
        assert_eq!(hits[0].memory.text, "ferries");
        assert!((hits[0].breakdown.vector - 1.0).abs() < 1e-5);
    }

    #[test]
    fn recall_is_deterministic() {
        let s = store();
        let e = HashEmbedder::new_256();
        ns(&s, "w");
        for text in ["alpha", "beta gamma", "gamma delta"] {
            s.insert_memory(&NewMemory {
                embedding: Some(e.embed(text)),
                ..mem("w", text)
            })
            .unwrap();
        }
        let q = e.embed("gamma");
        let a = s
            .recall("w", "gamma", Some(&q), &RecallParams::default(), 10)
            .unwrap();
        let b = s
            .recall("w", "gamma", Some(&q), &RecallParams::default(), 10)
            .unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn fts_index_stays_synced_across_delete() {
        let s = store();
        ns(&s, "w");
        let m = s
            .insert_memory(&mem("w", "unique widget fingerprint"))
            .unwrap();
        let q = "widget";
        let p = RecallParams::default();
        assert_eq!(s.recall("w", q, None, &p, 1).unwrap().len(), 1);
        s.delete_memory(&m.id).unwrap();
        assert!(
            s.recall("w", q, None, &p, 1).unwrap().is_empty(),
            "FTS row must go with the memory"
        );
    }

    #[test]
    fn weights_honor_custom_values_in_ranking() {
        let s = store();
        let e = HashEmbedder::new_256();
        ns(&s, "w");
        let now = 5_000;
        s.insert_memory(&NewMemory {
            created_at: Some(now),
            embedding: Some(e.embed("bright lizards fly")),
            ..mem("w", "bright lizards fly")
        })
        .unwrap();
        let q = e.embed("bright lizards fly");
        let p = RecallParams {
            weights: Weights {
                bm25: 0.0,
                vector: 1.0,
                recency: 0.0,
            },
            tau_days: Some(30.0),
            ..Default::default()
        };
        let hits = s.recall("w", "bright lizards", Some(&q), &p, now).unwrap();
        assert_eq!(hits.len(), 1);
        let b = &hits[0].breakdown;
        assert!(
            (b.total - b.vector).abs() < 1e-9,
            "only the vector weight should apply"
        );
    }

    // ---- Encryption at rest (D-014) ----

    const SECRET: &str = "the launch codes are hidden in the orchard";

    #[test]
    fn keyed_store_roundtrips_plaintext_through_api() {
        let s = keyed();
        ns(&s, "w");
        let m = s.insert_memory(&mem("w", SECRET)).unwrap();
        assert_eq!(m.text, SECRET);
        assert_eq!(s.get_memory(&m.id).unwrap().unwrap().text, SECRET);
        let listed = s.list_memories(Some("w"), 10).unwrap();
        assert_eq!(listed[0].text, SECRET);
        assert_eq!(
            s.meta_get(META_AT_REST).unwrap().as_deref(),
            Some(AT_REST_ENC)
        );
    }

    #[test]
    fn keyed_store_recalls_by_keyword_and_vector() {
        let s = keyed();
        let e = HashEmbedder::new_256();
        ns(&s, "w");
        let text = "deploy the payments service on friday";
        s.insert_memory(&NewMemory {
            embedding: Some(e.embed(text)),
            ..mem("w", text)
        })
        .unwrap();
        let q = e.embed(text);
        let hits = s
            .recall("w", text, Some(&q), &RecallParams::default(), 10)
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert!(hits[0].breakdown.bm25 > 0.0, "keyword pass must work keyed");
        assert!((hits[0].breakdown.vector - 1.0).abs() < 1e-5);
        assert_eq!(hits[0].memory.text, text);
    }

    #[test]
    fn encrypted_at_rest_keeps_plaintext_out_of_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("enc.db");
        let key = StoreKey::from_hex(&"7f".repeat(32)).unwrap();
        {
            let s = SqliteStore::open_with_key(&path, &key).unwrap();
            s.create_namespace("w").unwrap();
            s.insert_memory(&mem("w", SECRET)).unwrap();
        }
        // Raw bytes of the database (and its WAL) must not contain the secret.
        let db = std::fs::read(&path).unwrap();
        let wal = std::fs::read(path.with_extension("db-wal")).unwrap_or_default();
        for buf in [&db, &wal] {
            let hay = String::from_utf8_lossy(buf);
            assert!(!hay.contains("launch codes"), "plaintext leaked to disk");
            assert!(!hay.contains("orchard"), "token leaked to disk");
        }
    }

    #[test]
    fn fts_index_stores_digests_not_plaintext() {
        let s = keyed();
        ns(&s, "w");
        s.insert_memory(&mem("w", "xylophone taxonomy")).unwrap();
        let tokens: String = {
            let mut stmt = s.conn.prepare("SELECT fts_tokens FROM memories").unwrap();
            stmt.query_row([], |r| r.get::<_, String>(0)).unwrap()
        };
        assert!(tokens.split(' ').all(|t| t.len() == 32), "digests only");
        assert!(!tokens.contains("xylophone"));
        // The FTS inverted index itself must also be digest-only.
        let mut stmt = s.conn.prepare("SELECT * FROM memories_fts").unwrap();
        let mut rows = stmt.query([]).unwrap();
        while let Some(row) = rows.next().unwrap() {
            let doc: String = row.get(0).unwrap();
            assert!(!doc.contains("xylophone"));
        }
    }

    #[test]
    fn wrong_key_fails_at_open_via_key_check() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("k.db");
        let good = StoreKey::from_hex(&"aa".repeat(32)).unwrap();
        let bad = StoreKey::from_hex(&"bb".repeat(32)).unwrap();
        {
            let s = SqliteStore::open_with_key(&path, &good).unwrap();
            ns(&s, "w");
            s.insert_memory(&mem("w", SECRET)).unwrap();
        }
        let err = match SqliteStore::open_with_key(&path, &bad) {
            Ok(_) => panic!("wrong key must be rejected"),
            Err(e) => e,
        };
        assert!(
            err.to_string().contains("decryption failed"),
            "wrong key must be rejected: {err}"
        );
        // Correct key reopens and decrypts.
        let reopened = SqliteStore::open_with_key(&path, &good).unwrap();
        let m = reopened.list_memories(Some("w"), 10).unwrap();
        assert_eq!(m[0].text, SECRET);
    }

    #[test]
    fn encrypted_database_requires_a_key() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("k.db");
        let key = StoreKey::from_hex(&"cc".repeat(32)).unwrap();
        {
            let s = SqliteStore::open_with_key(&path, &key).unwrap();
            ns(&s, "w");
            s.insert_memory(&mem("w", SECRET)).unwrap();
        }
        let err = match SqliteStore::open(&path) {
            Ok(_) => panic!("plain open must refuse an encrypted database"),
            Err(e) => e,
        };
        assert!(
            err.to_string().contains("encrypted at rest"),
            "plain open must refuse: {err}"
        );
    }

    #[test]
    fn legacy_plaintext_db_is_upgraded_in_place() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("legacy.db");
        // Build a database with the CURRENT code but plaintext... we cannot
        // produce a true v1 file without the old schema, so hand-craft one
        // with the exact v1 layout (memories without fts_tokens, FTS over
        // `text`, text-based triggers).
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(
                r#"
                CREATE TABLE namespaces (
                    id TEXT PRIMARY KEY, name TEXT UNIQUE NOT NULL, created_at INTEGER NOT NULL);
                CREATE TABLE memories (
                    id TEXT PRIMARY KEY,
                    namespace_id TEXT NOT NULL REFERENCES namespaces(id) ON DELETE CASCADE,
                    text TEXT NOT NULL, tags TEXT NOT NULL DEFAULT '[]', embedding BLOB,
                    source TEXT NOT NULL DEFAULT 'agent', pinned INTEGER NOT NULL DEFAULT 0,
                    created_at INTEGER NOT NULL, last_accessed_at INTEGER NOT NULL);
                CREATE VIRTUAL TABLE memories_fts USING fts5(
                    text, content = 'memories', content_rowid = 'rowid');
                CREATE TRIGGER memories_fts_insert AFTER INSERT ON memories BEGIN
                    INSERT INTO memories_fts(rowid, text) VALUES (new.rowid, new.text);
                END;
                CREATE TRIGGER memories_fts_delete AFTER DELETE ON memories BEGIN
                    INSERT INTO memories_fts(memories_fts, rowid, text)
                    VALUES ('delete', old.rowid, old.text);
                END;
                INSERT INTO namespaces VALUES ('ns-1', 'w', 1);
                INSERT INTO memories (id, namespace_id, text, tags, created_at, last_accessed_at)
                    VALUES ('m-1', 'ns-1', 'the orchard launch codes', '[]', 1, 1);
                "#,
            )
            .unwrap();
        }
        // Keyed open upgrades: reads decrypt, keyword recall works, file is clean.
        let key = StoreKey::from_hex(&"d1".repeat(32)).unwrap();
        let s = SqliteStore::open_with_key(&path, &key).unwrap();
        let m = s.get_memory("m-1").unwrap().unwrap();
        assert_eq!(m.text, "the orchard launch codes");
        let hits = s
            .recall("w", "orchard", None, &RecallParams::default(), 2)
            .unwrap();
        assert_eq!(hits.len(), 1, "keyword recall must survive upgrade");
        // Marker written.
        assert_eq!(
            s.meta_get(META_AT_REST).unwrap().as_deref(),
            Some(AT_REST_ENC)
        );
        // Reopen without key: refused.
        assert!(SqliteStore::open(&path).is_err());
        // Reopen with key: still fine.
        let again = SqliteStore::open_with_key(&path, &key).unwrap();
        assert_eq!(again.get_memory("m-1").unwrap().unwrap().text, m.text);
    }

    #[test]
    fn mixed_legacy_rows_are_encrypted_on_keyed_open() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mixed.db");
        let key = StoreKey::from_hex(&"e2".repeat(32)).unwrap();
        // Plaintext DB via current code, then enable encryption.
        {
            let s = SqliteStore::open(&path).unwrap();
            ns(&s, "w");
            s.insert_memory(&mem("w", "plain as day observation"))
                .unwrap();
        }
        {
            let s = SqliteStore::open_with_key(&path, &key).unwrap();
            // Explicit later timestamp: list_memories orders newest first.
            s.insert_memory(&NewMemory {
                created_at: Some(2_000_000_000),
                ..mem("w", SECRET)
            })
            .unwrap();
        }
        let db_bytes = std::fs::read(&path).unwrap();
        let hay = String::from_utf8_lossy(&db_bytes);
        assert!(
            !hay.contains("plain as day"),
            "legacy row must be re-encrypted"
        );
        assert!(!hay.contains("launch"), "no plaintext anywhere");
        let s = SqliteStore::open_with_key(&path, &key).unwrap();
        let texts = s
            .list_memories(Some("w"), 10)
            .unwrap()
            .into_iter()
            .map(|m| m.text)
            .collect::<Vec<_>>();
        // Newest first: the SECRET memory was created after the legacy row.
        assert_eq!(
            texts,
            vec![SECRET.to_string(), "plain as day observation".to_string()]
        );
    }
}
