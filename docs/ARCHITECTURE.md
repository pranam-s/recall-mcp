# Recall-MCP architecture

This describes the shipped 0.5.0 design. Decision records and dated amendments
live in [adr/DECISIONS.md](adr/DECISIONS.md) (D-001 through D-021), and measured
numbers in [EVALUATION.md](EVALUATION.md).

The workspace uses Rust 2024, with Windows MSVC as the primary toolchain and
windows-gnu as the secondary one. The server is built on Axum 0.8 and Tokio,
stores data in SQLite through rusqlite 0.41 (bundled, with FTS5), and uses rmcp
3.9 for MCP. The SDK targets the MCP 2026-07-28 spec, and the wire `initialize`
handshake answers `2025-11-25` (D-004). The web UI uses React 19, Vite 8,
TypeScript, and Tailwind 4. Tests run with `cargo test`, coverage with
`cargo llvm-cov`, and browser tests with Playwright and axe.

## What the system is

A local-first semantic memory broker packaged three ways from one core: an MCP
server (stdio and streamable-HTTP), an HTTP/JSON API with a hand-written
OpenAPI document, and a CLI over the same engine. One SQLite file is the whole
store.

Retrieval is explainable by design. Every recall result carries
`{bm25, vector, recency, pinned_boost, total}`, and the web UI renders each
component. The components are computed and unit tested separately.

## Layering

```text
crates/
  recall-core/    pure domain: models, validation, compact views, Store trait,
                  SqliteStore (FTS5, WAL, crypto, packed vector cache,
                  maintenance), Embedder trait + hash/ONNX/OpenAI, HybridScorer,
                  capture chunking, export/import
  recall-server/  axum REST (/v1/*), OpenAPI, /metrics, auth, bounded token
                  bucket, Host/Origin guard, security headers, JSON errors, rmcp
                  tools over HTTP and stdio, spawn_blocking bridges
  recall-cli/     clap surface: serve, remember, recall, update, forget, list,
                  capture, stats, keygen, export, import, backup, vacuum
web/              React + Vite SPA, a pure API client, Playwright + axe e2e
evals/            100-memory corpus, 100 golden queries, 50 semantic queries
skills/           agent skill plus CLI and MCP references
docs/             architecture, decisions, evaluation, clients, token
                  efficiency, style guides
```

The pure-core rule (D-001) keeps the coverage gate achievable. The scorer,
store, embedders, chunker, crypto, validation, and views can be tested without
an HTTP runtime. The MCP transport can be swapped because core does not know it
exists. Input is validated in core and again at each edge. The duplication is
deliberate: a shared validation layer typed for async HTTP would drag axum types
back into core.

## Data model

```sql
store_meta(key TEXT PK, value TEXT)           -- mode, key check, forensic marker,
                                              -- embedder identity
namespaces(id TEXT PK, name TEXT UNIQUE, created_at INTEGER)
memories(id TEXT PK, namespace_id TEXT FK REFERENCES namespaces ON DELETE CASCADE,
         text TEXT NOT NULL,                  -- ciphertext in keyed mode
         fts_tokens TEXT NOT NULL DEFAULT '', -- plaintext text or HMAC digests
         tags TEXT /*json*/, embedding BLOB /*f32 LE, nullable*/,
         source TEXT DEFAULT 'agent', pinned INTEGER DEFAULT 0,
         created_at INTEGER, last_accessed_at INTEGER /*write-time only*/)
memories_fts(fts_tokens)   -- FTS5 external-content index, trigger-synced
```

`PRAGMA user_version` is 2, and a newer schema is refused instead of guessed
at. The connection sets a 5 s busy timeout, WAL, foreign keys,
`synchronous=NORMAL`, a larger page cache, and memory-mapped reads.

There is no vector extension. Similarity is an exact cosine scan over embedding
BLOBs in rowid order (D-002, amended by D-016 and D-019), with a bounded exact
top-k heap and a packed in-memory cache. `scan_cap` (default 10,000 rows per
namespace) bounds the vector scan, and `candidate_cap` (200) bounds keyword
candidates. Both are documented approximations.

The packed cache records `PRAGMA data_version`. A commit from another connection
changes that value, so a stale cache is rebuilt instead of serving old vectors.
Local writes invalidate the cache directly. Tag-filtered recalls bypass it
because the cache does not carry tags.

## Recall path

1. Keyword candidates. FTS5 `MATCH` over `memories_fts`, ordered by BM25 with a
   rowid tie-break. BM25 is normalized against the candidate set. In keyed mode
   both documents and queries are HMAC token digests, so ranking sees the same
   token multiplicities while plaintext stays off disk.
2. Vector candidates. An exact cosine pass reads `(rowid, embedding,
   created_at, pinned)` and computes from BLOB bytes with the query norm hoisted
   out of the loop. A bounded max-heap keeps the best vector-only rows. Outside
   the keyword set BM25 is zero, so a vector-only total is the final total and
   the heap provably cannot drop a row that belongs in the top k.
3. Scoring. `total = w_bm25*norm_bm25 + w_vector*cosine +
   w_recency*exp(-days/tau) + pinned_boost`; defaults 0.45/0.45/0.10, tau 30
   days, boost 0.2, tunable per request. Weights and tau are validated.
4. Ordering. Total desc, created_at desc, rowid asc. The candidate heap and
   final sort use the same comparator, so ties are deterministic.
5. Post-processing. Full rows are fetched only for survivors. Recall is
   read-only: `last_accessed_at` is written at insert/update time only (AR-018).

## Encryption at rest (D-014)

`RECALL_MCP_KEY` (64 hex characters) or `RECALL_MCP_KEY_FILE`. HKDF-SHA256
derives separate AEAD (`recall-mcp:v1:aead`) and FTS-MAC
(`recall-mcp:v1:fts-mac`) subkeys. `memories.text` is
`enc:v1:<base64(nonce || ciphertext || tag)>` under ChaCha20-Poly1305 with a
random 96-bit nonce and the row id bound as AAD. The FTS index stores
HMAC-SHA256 token digests truncated to 128 bits. A stored key-check blob makes
a wrong key fail at open.

Every plaintext-to-keyed transition runs with `secure_delete`, then VACUUM and
`wal_checkpoint(TRUNCATE)`. A `forensic_clean` marker is written `pending` in
the same transaction as the mode marker and key check, and set `done` only
after the checkpoint drains the WAL. An interrupted clean retries on the next
keyed open. Pre-HKDF stores migrate in one transaction. A forensic test covers a
deleted plaintext row and a plaintext-era WAL frame.

Documented residual plaintext: namespace names, tags, `source`, embeddings
(invertible for dense embedders), and `export` output.

## Embedders

The `Embedder` trait exposes `dimensions`, `name`, `fingerprint`, `embed`, and
`embedding_invertible`. Implementations: `HashEmbedder` (feature hashing,
256 dimensions, offline, default), `OnnxEmbedder` (all-MiniLM-L6-v2 quantized,
384 dimensions, `onnx` feature), and `OpenAIEmbedder` (`openai` feature, key
redacted in Debug, `dimensions` sent for text-embedding-3 models, response
dimensions validated).

Selection is runtime and fail-closed: an unknown name or a missing feature is a
typed error, never a silent fallback. The store stamps the embedder
fingerprint, name, and dimensions in `store_meta`. Empty stores stamp, legacy
stores with matching dimensions adopt with a warning, and a mismatch names both
sides and requires `--allow-embedder-mismatch`; that override does not rewrite
the stored fingerprint.

## HTTP and MCP surfaces

REST:

- `POST/GET /v1/namespaces`
- `POST/GET /v1/memories` (compact page by default; `include_embedding=true`
  restores full records)
- `PATCH/DELETE /v1/memories/{id}`
- `POST /v1/recall` (compact hits with breakdowns; `include_embedding=true`)
- `POST /v1/capture` (compact summary; `include_memories=true` opts in)
- `GET /v1/stats` (counts plus embedder identity)
- `GET /healthz`, `GET /openapi.json`, `GET /metrics`

MCP: six tools (`remember`, `recall`, `forget`, `update_memory`,
`list_memories`, `capture`) over stdio and streamable-HTTP. Responses are
compact by default; `include_embedding` restores full records. `list_memories`
defaults to 20 and caps at 100. `recall` accepts `format: "json"|"text"`; text
keeps ranking and the full breakdown and rejects embeddings. Stateless
2026-07-28 requests carry `_meta` plus `MCP-Protocol-Version`, `Mcp-Method`, and
`Mcp-Name`. Unknown-id `forget` is an invalid-params error, matching REST's
404.

Middleware order (outermost first): security headers, Host/Origin guard, JSON
error contract, CORS, tracing, body limit, then rate limit and auth on protected
routers. The limiter runs before auth; only a key matching the configured key
gets its own bucket, and the bucket map is bounded. The guard compares Host and
Origin including scheme and normalized default ports. API, MCP, and metrics
responses are `no-store` and carry CSP, nosniff, frame denial, referrer policy,
COOP/CORP, and a permissions policy.

Store and embedder work runs through `spawn_blocking`, so a slow model call
cannot starve the async runtime and no mutex guard crosses an await.

## Capture

Transcript messages are split deterministically (paragraph, then sentence, then
a hard character wrap; default 1000 characters, clamped to 100 to 8000). Each
chunk is embedded outside the store lock, tagged `role:<role>`, and stored with
`source="capture"`. Namespace creation and every chunk insert run in one
`BEGIN IMMEDIATE` transaction; if an insert fails, no namespace and no partial
transcript remain.

## Validation and limits

Core enforces namespace, text, query, id, tag, source, embedding dimension and
finiteness, capture message count, and total capture characters. Defaults:
query 10,000 characters, text 100,000, namespace 200, id 256, tag 100,
100 tags, embedding 8,192 dimensions, 1,000 capture messages, 2,000,000 capture
characters. Recall caps are bounded (`candidate_cap <= 10,000`,
`scan_cap <= 1,000,000`), list binds saturate instead of wrapping, and recency
arithmetic saturates.

## Testing strategy

- recall-core: unit tests, property tests (crypto/AAD, FTS syntax, chunking,
  scorer, token bucket), golden and semantic evals, ignored latency harnesses,
  and mutation testing for `crypto.rs`/`scorer.rs`.
- recall-server: router-level integration tests, JSON error contract, security
  headers, Host/Origin, rate limiter, slow embedder, MCP conformance over both
  transports, stateless 2026-07-28, and token budgets.
- recall-cli: real-binary e2e for every command, encryption, backup, compact
  output, keygen, the `recall` alias, and stdio MCP.
- Web: production-shaped server plus Playwright and axe AA.
- Coverage: lines and regions >= 90 on recall-core (D-010 amendment; no stable
  branch fail-under).

GitHub Actions runs the workspace tests, formatting, clippy, coverage, web
build with Playwright, and dependency checks on push and pull requests. The 10k
p95 design target is 25 ms; the p95 harness is an ignored test that runs only
when invoked manually.

## Known limitations

- Beyond roughly 100k memories per namespace, the D-002 ANN revisit applies.
- Dense embeddings are invertible, so keyed mode does not protect text with
  ONNX or OpenAI vectors.
- The OpenAI embedder has never been called live.
