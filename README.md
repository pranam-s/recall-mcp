# Recall-MCP

**Local-first semantic memory broker for AI agents** — an MCP server (stdio +
streamable-HTTP) that any MCP-capable agent can use to `remember`, `recall`,
`forget`, and `list_memories`, backed by one SQLite file. Also exposes a plain
HTTP/JSON API with an OpenAPI document, a web UI that explains every ranking,
and a CLI for scripting.

- Status: **v0.1 working end-to-end** (see [docs/STATUS.md](docs/STATUS.md) for
  the honest done/partial list and [docs/EVALUATION.md](docs/EVALUATION.md) for
  measured results).

## Why

AI agents start every session from zero. Recall-MCP is a vendor-neutral memory
layer: agents store observations/decisions/preferences as short records and get
them back via **hybrid retrieval** — BM25 keyword scoring blended with embedding
similarity, recency decay, and pinned boosts — with a **score breakdown on every
result** (`{bm25, vector, recency, pinned_boost, total}`), never a black-box top-k.

```
total = w_bm25·norm_bm25 + w_vector·cosine + w_recency·exp(-Δdays/τ) + pinned_boost
defaults: w = 0.45 / 0.45 / 0.10, τ = 30 days, pinned_boost = 0.2 (all tunable per request)
```

Namespaces isolate workspaces; memories never leak across them.

## Quickstart (commands verified green on 2026-09-06)

Prereqs: Rust (GNU toolchain used here; rustup override included in the repo),
gcc for bundled SQLite, Node 24.

```bash
# 1. Run the whole test suite (66 tests: unit + integration + MCP conformance + eval)
cargo test --workspace

# 2. Serve REST + MCP + web UI (creates recall.db if missing)
cargo run -p recall-cli -- serve --bind 127.0.0.1:8787 --db recall.db
#   → REST:        http://127.0.0.1:8787/v1/...
#   → MCP:         http://127.0.0.1:8787/mcp  (streamable-HTTP, spec 2026-07-28)
#   → OpenAPI:     http://127.0.0.1:8787/openapi.json
#   → Web UI:      http://127.0.0.1:8787/     (after step 4 builds web/dist)

# 3. Script with the CLI (same engine as MCP)
cargo run -p recall-cli -- remember --db recall.db --namespace work --text "We deploy on Thursdays" --tags ops
cargo run -p recall-cli -- recall  --db recall.db --namespace work --query "when do we deploy?" --k 3
cargo run -p recall-cli -- export  --db recall.db --out backup.json
cargo run -p recall-cli -- import  --db fresh.db --file backup.json

# 4. Web UI + accessibility e2e (builds UI, serves it from the Rust binary, runs Playwright + axe)
cd web && npm install && npm run build && npx playwright test
```

### MCP client wiring

- **stdio:** `recall-cli serve --stdio --db recall.db`
- **streamable-HTTP:** point the client at `http://127.0.0.1:8787/mcp`
- **API key (recommended when not only loopback):** set `RECALL_MCP_API_KEY`
  (or `--api-key`). `/v1/*` and `/mcp` then require `Authorization: Bearer` or
  `X-API-Key`; without a key the server is open by design — it is a local,
  single-user tool. `/healthz` and `/openapi.json` always stay open.

## Architecture

```
crates/
  recall-core/    # pure domain: models, Store trait + SQLite (FTS5, WAL, BLOB
                  # embeddings), Embedder trait (HashEmbedder default, OpenAI
                  # opt-in), explainable HybridScorer, export/import
  recall-server/  # axum REST API (/v1/*), OpenAPI doc, API-key middleware,
                  # rmcp MCP tools over streamable-HTTP + stdio
  recall-cli/     # clap CLI: serve (HTTP+MCP / --stdio), remember, recall, export, import
web/              # React 19 + Vite 8 + TS + Tailwind 4 UI; Playwright + axe e2e
evals/            # fixture corpus (40 memories) + golden set (50 queries)
docs/             # PRD, architecture, decisions, evaluation, status
```

Ranking pipeline per query: FTS5 `MATCH` candidates (BM25, normalized to the
candidate set) merged with an exact brute-force cosine scan over the namespace's
BLOB embeddings → `HybridScorer` produces each component → deterministic
tie-break (total desc, created_at desc, id asc) → top-k with `last_accessed_at`
refreshed. See `docs/adr/DECISIONS.md` D-002 for the sqlite-vec trade-off.

## Repository hygiene

- `cargo clippy --workspace --all-targets` — 0 warnings (2026-09-06)
- `cargo fmt --all` enforced
- Conventional commits; every feature slice landed with its tests
