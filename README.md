# Recall-MCP

[![CI](https://github.com/pranam/recall-mcp/actions/workflows/ci.yml/badge.svg)](https://github.com/pranam/recall-mcp/actions/workflows/ci.yml)

**Local-first semantic memory broker for AI agents** — an MCP server (stdio +
streamable-HTTP) that any MCP-capable agent can use to `remember`, `recall`,
`forget`, `update_memory`, and `list_memories`, backed by one SQLite file. Also
exposes a plain HTTP/JSON API with an OpenAPI document, a web UI that explains
every ranking, and a CLI for scripting.

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
# 1. Run the whole test suite (88 tests: unit + integration + MCP conformance + eval)
cargo test --workspace

# 1b. Measure recall latency on the 10k fixture (criterion + exact percentiles).
#     Measured: p95 ≈ 67 ms at 10k — see docs/EVALUATION.md before trusting the 25 ms PRD budget.
cargo bench -p recall-core
cargo test -p recall-core --release --test p95_latency -- --ignored --nocapture

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

### Encryption at rest (optional, per database)

Set `RECALL_MCP_KEY` (64 hex chars = 256-bit key, or `RECALL_MCP_KEY_FILE`
pointing at a file containing them) and every store the CLI/server opens is
encrypted at rest: memory text is sealed with ChaCha20-Poly1305 and the keyword
index is built from keyed HMAC token digests, so plaintext never touches disk —
keyword search still works. Databases created or first opened with a key are
upgraded in place (plaintext rows are re-encrypted on first keyed open); an
encrypted database refuses to open without the key. Generate a key with:
`openssl rand -hex 32`. Caveats (details in D-014): `export` output is
plaintext JSON by design, and namespace names/tags/embeddings are not encrypted.

### Local semantic embeddings (optional `onnx` feature)

The default `HashEmbedder` is lexical. For semantic matching fully offline,
build with `--features onnx` (`OnnxEmbedder`, all-MiniLM-L6-v2 quantized,
384-dim): the model downloads once from Hugging Face into
`.fastembed_cache/`, and ONNX Runtime is loaded at runtime — set
`ORT_DYLIB_PATH` to `onnxruntime.dll` (windows-gnu has no ort prebuilt; see
D-013). On the golden set, HashEmbedder scores 0.980 hit-rate@5, the ONNX
embedder 1.000 (docs/EVALUATION.md).

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

- `cargo clippy --workspace --all-targets -- -D warnings` — 0 warnings (2026-09-06)
- `cargo fmt --all` enforced
- CI (`.github/workflows/ci.yml`): fmt + clippy, workspace tests, llvm-cov
  90/90 gate on recall-core (linux-gnu, D-010), windows-gnu test job
- Conventional commits; every feature slice landed with its tests
