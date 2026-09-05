# Architecture — Recall-MCP

**Stack:** Rust (GNU toolchain, Windows) · Axum + Tokio · SQLite (rusqlite bundled) · sqlite-vec · MCP SDK (per research notes) · React 19 + Vite UI · cargo nextest + llvm-cov

## Components

```
crates/
  recall-core/       # domain: Memory, Namespace, Scorer, Embedder trait, Store trait
  recall-server/     # axum HTTP API + MCP server transports (stdio, streamable-HTTP)
  recall-cli/        # clap CLI: serve, remember, recall, export, import
web/                 # React 19 + Vite + TS UI ( talks to HTTP API only)
```

Rationale: workspace crates keep the domain pure (no axum types in core), which is what makes the
90% branch-coverage target achievable and the MCP transport swappable.

## Data model (SQLite)

```sql
namespaces(id TEXT PK, name TEXT UNIQUE, created_at INTEGER)
memories(id TEXT PK, namespace_id TEXT FK, text TEXT NOT NULL,
         tags TEXT/*json*/, embedding BLOB/*f32 vector, nullable*/,
         source TEXT DEFAULT 'agent', pinned INTEGER DEFAULT 0,
         created_at INTEGER, last_accessed_at INTEGER)
memories_fts(id, text)              -- FTS5, external-content, trigger-synced
-- sqlite-vec virtual table for embeddings (vec0), rowid-linked to memories
```

Embeddings behind trait `Embedder` (v1: `HashEmbedder` deterministic 256-dim, offline, for tests +
default; `OpenAIEmbedder` behind `--embedding-api` flag). Scorer behind trait `Scorer`.

## API surface (HTTP)

- `POST /v1/namespaces` · `GET /v1/namespaces`
- `POST /v1/memories` `{namespace, text, tags?}` → 201 memory
- `POST /v1/recall` `{namespace, query, k?, decay?}` → ranked `[memory + score_breakdown]`
  (`breakdown: {bm25, vector, recency, pinned_boost, total}`)
- `DELETE /v1/memories/{id}` · `GET /v1/memories?namespace=`
- `GET /healthz`, `GET /openapi.json` (utoipa or research-recommended)
- MCP: stdio (`recall-cli serve --stdio`) and streamable-HTTP (`/mcp`) exposing tools
  `remember`, `recall`, `forget`, `list_memories` — tool schemas per current MCP spec.

## Ranking (scoring design, v1)

`total = w_k*norm_bm25 + w_v*cosine + w_r*exp(-Δdays/τ) + pinned_boost`
Defaults: w_k=0.45, w_v=0.45, w_r=0.10, τ=30d, pinned_boost=0.2. Weights tunable via request.
Deterministic (integer-free float math is fine; tests assert on breakdown components).

## ADRs (summary — full text in docs/adr/)

- ADR-001 Rust workspace with pure-core crate: testability + coverage target; web/MCP as shells.
- ADR-002 SQLite + FTS5 + sqlite-vec over a vector DB: local-first single-file goal; 1M-row ceiling documented.
- ADR-003 HashEmbedder default: deterministic tests + true offline; API embedders as pluggable opt-in.
- ADR-004 MCP server via (research-selected SDK) with hand-rolled JSON-RPC fallback only if SDK is unready; spec version pinned in docs.
- ADR-005 GNU toolchain on Windows (no MSVC Build Tools dependency); gcc via scoop for bundled SQLite C compilation.
- ADR-006 UI is a pure API client (no SSR): simplest a11y-compliant stack; OpenAPI types via generated TS client if research notes recommend.

## Testing strategy

- recall-core: unit (scorer properties, embedder determinism, store CRUD) — target 100% branch.
- recall-server: integration via `axum::test` + tokio; MCP conformance smoke (initialize → tools/list → tools/call over stdio and HTTP).
- Golden-set eval test: 50 QA pairs, hit-rate@5 ≥ 0.85 gate.
- e2e: CLI against running server; UI Playwright: create/search/inspect breakdown.
- Coverage: cargo llvm-cov (or research-recommended alternative on windows-gnu) with
  `--fail-under-lines 90 --fail-under-branches 90` on recall-core (+server where practical).
