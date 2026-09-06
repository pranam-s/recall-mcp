# Recall-MCP

[![CI](https://github.com/pranam/recall-mcp/actions/workflows/ci.yml/badge.svg)](https://github.com/pranam/recall-mcp/actions/workflows/ci.yml)

**Local-first semantic memory broker for AI agents** — an MCP server (stdio +
streamable-HTTP) that any MCP-capable agent can use to `remember`, `recall`,
`forget`, `update_memory`, and `list_memories`, backed by one SQLite file. Also
exposes a plain HTTP/JSON API with an OpenAPI document, a `/metrics` endpoint
in Prometheus format, transcript auto-capture, a web UI that explains every
ranking, and a CLI for scripting (including hot backup).

- Status: **v0.3 production hardening** (see [docs/STATUS.md](docs/STATUS.md)
  for the honest done/partial list and [docs/EVALUATION.md](docs/EVALUATION.md)
  for measured results).

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

## Quickstart (commands verified green on 2026-09-07)

Prereqs: Rust (GNU toolchain used here; rustup override included in the repo),
gcc for bundled SQLite, Node 24.

```bash
# 1. Run the whole test suite (unit + integration + MCP conformance + eval)
cargo test --workspace

# 1b. Measure recall latency on the 10k fixture (criterion + exact percentiles).
#     Measured: p95 ≈ 33-35 ms at 10k — see docs/EVALUATION.md before trusting the 25 ms PRD budget.
cargo bench -p recall-core
cargo test -p recall-core --release --test p95_latency -- --ignored --nocapture

# 2. Serve REST + MCP + web UI (creates recall.db if missing)
cargo run -p recall-cli -- serve --bind 127.0.0.1:8787 --db recall.db
#   → REST:        http://127.0.0.1:8787/v1/...
#   → MCP:         http://127.0.0.1:8787/mcp  (streamable-HTTP, spec 2026-07-28)
#   → OpenAPI:     http://127.0.0.1:8787/openapi.json
#   → Metrics:     http://127.0.0.1:8787/metrics  (Prometheus text)
#   → Web UI:      http://127.0.0.1:8787/     (after step 4 builds web/dist)

# 3. Script with the CLI (same engine as MCP)
cargo run -p recall-cli -- remember --db recall.db --namespace work --text "We deploy on Thursdays" --tags ops
cargo run -p recall-cli -- recall  --db recall.db --namespace work --query "when do we deploy?" --k 3
cargo run -p recall-cli -- export  --db recall.db --out backup.json
cargo run -p recall-cli -- import  --db fresh.db --file backup.json
cargo run -p recall-cli -- backup  --db recall.db --out snapshot.db   # hot, compacted snapshot
cargo run -p recall-cli -- vacuum  --db recall.db                     # in-place compact + WAL checkpoint

# 4. Web UI + accessibility e2e (builds UI, serves it from the Rust binary, runs Playwright + axe)
cd web && npm install && npm run build && npx playwright test
```

### MCP client wiring

- **stdio:** `recall-cli serve --stdio --db recall.db`
- **streamable-HTTP:** point the client at `http://127.0.0.1:8787/mcp`
- **API key (recommended when not only loopback):** set `RECALL_MCP_API_KEY`
  (or `--api-key`). `/v1/*`, `/metrics` and `/mcp` then require
  `Authorization: Bearer` or `X-API-Key`; without a key the server is open by
  design — it is a local, single-user tool. `/healthz` and `/openapi.json`
  always stay open.

### Operating the server (v0.3)

- **Rate limiting** (off by default): `--rate-limit 50` allows 50 sustained
  requests/second per API key with burst `max(2×rate, 10)`; override with
  `--rate-burst`. Over-limit requests get `429` + `Retry-After`. Also via env
  `RECALL_MCP_RATE_LIMIT` or a `--config` JSON file.
- **Body limit**: requests are capped at 1 MiB by default (`--body-limit`,
  env `RECALL_MCP_BODY_LIMIT`); oversize → `413`.
- **Config file**: `serve --config serve.json` accepts
  `{"bind": "...", "rate_limit": 50, "rate_burst": 120, "body_limit": 1048576}`.
  Precedence: flag > env > file > default. Secrets are never file-configurable.
- **Graceful shutdown**: Ctrl-C/SIGTERM drains in-flight requests and
  checkpoints the WAL before exit.
- **Metrics**: `GET /metrics` (Prometheus 0.0.4) — request counter, status
  codes, per-route latency histogram, embedding timings, rate-limit counter,
  and live store gauges.
- **Pagination**: `GET /v1/memories?namespace=&limit=&offset=` with
  `X-Total-Count` carrying the unpaginated total.

### Auto-capture ingest (hook scaffold)

Point a session-end hook or IDE plugin at:

```bash
curl -X POST http://127.0.0.1:8787/v1/capture \
  -H 'content-type: application/json' \
  -d '{
    "namespace": "session-42",
    "tags": ["session:42"],
    "transcript": [
      {"role": "user",      "content": "we chose sqlite over a vector db", "at": 1786000000},
      {"role": "assistant", "content": "rationale: local-first, one file, FTS5 + brute-force cosine is enough at this scale"}
    ]
  }'
```

Each message is split into deterministic chunks (paragraph → sentence → hard
wrap at `max_chunk_chars`, default 1000), embedded server-side, tagged
(`role:user`, `role:assistant`, plus your tags) and stored with
`source = "capture"`; the namespace is created if missing.

### Encryption at rest (optional, per database)

Set `RECALL_MCP_KEY` (64 hex chars = 256-bit key, or `RECALL_MCP_KEY_FILE`
pointing at a file containing them) and every store the CLI/server opens is
encrypted at rest: memory text is sealed with ChaCha20-Poly1305 and the keyword
index is built from keyed HMAC token digests, so plaintext never touches disk —
keyword search still works. Databases created or first opened with a key are
upgraded in place (plaintext rows are re-encrypted on first keyed open); an
encrypted database refuses to open without the key. `recall-cli backup`
produces encrypted backups. Generate a key with: `openssl rand -hex 32`.
Caveats (details in D-014): `export` output is plaintext JSON by design, and
namespace names/tags/embeddings are not encrypted.

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
                  # embeddings, maintenance ops), Embedder trait (HashEmbedder
                  # default, OpenAI opt-in), explainable HybridScorer,
                  # capture chunking, export/import
  recall-server/  # axum REST API (/v1/*), OpenAPI doc, /metrics (Prometheus),
                  # API-key middleware (constant-time), token-bucket rate
                  # limiter, body limits, rmcp MCP tools over streamable-HTTP
                  # + stdio
  recall-cli/     # clap CLI: serve (HTTP+MCP / --stdio, graceful shutdown),
                  # remember, recall, update, export, import, backup, vacuum
web/              # React 19 + Vite 8 + TS + Tailwind 4 UI; Playwright + axe e2e
evals/            # fixture corpus (40 memories) + golden set (50 queries)
docs/             # PRD, architecture, decisions, evaluation, status
```

Ranking pipeline per query: FTS5 `MATCH` candidates (BM25, normalized to the
candidate set) merged with an exact cosine scan over the namespace's BLOB
embeddings — slimmed in v0.3 to scan only scoring columns and keep an exact
top-k candidate heap (D-016) → `HybridScorer` produces each component →
deterministic tie-break (total desc, created_at desc, id asc) → top-k with
`last_accessed_at` refreshed in one batched update. See
`docs/adr/DECISIONS.md` D-002 and D-016 for the storage trade-offs.

## Repository hygiene

- `cargo clippy --workspace --all-targets -- -D warnings` — 0 warnings (2026-09-07)
- `cargo fmt --all` enforced
- CI (`.github/workflows/ci.yml`): fmt + clippy, workspace tests, llvm-cov
  90/90 gate on recall-core (linux-gnu, D-010), windows-gnu test job
- Conventional commits; every feature slice landed with its tests
