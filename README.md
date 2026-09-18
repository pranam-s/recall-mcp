# Recall-MCP

**Local-first semantic memory broker for AI agents**: an MCP server (stdio +
streamable-HTTP) that any MCP-capable agent can use to `remember`, `recall`,
`forget`, `update_memory`, and `list_memories`, backed by one SQLite file. Also
exposes a plain HTTP/JSON API with an OpenAPI document, a `/metrics` endpoint
in Prometheus format, transcript auto-capture, a web UI that explains every
ranking, and a CLI for scripting (including hot backup).

- Status: **v0.3.1** (see [docs/STATUS.md](docs/STATUS.md)
  for the honest done/partial list and [docs/EVALUATION.md](docs/EVALUATION.md)
  for measured results).

## Why

AI agents start every session from zero. Recall-MCP is a vendor-neutral memory
layer: agents store observations/decisions/preferences as short records and get
them back via **hybrid retrieval**: BM25 keyword scoring blended with embedding
similarity, recency decay, and pinned boosts, with a **score breakdown on every
result** (`{bm25, vector, recency, pinned_boost, total}`), never a black-box top-k.

```
total = w_bm25·norm_bm25 + w_vector·cosine + w_recency·exp(-Δdays/τ) + pinned_boost
defaults: w = 0.45 / 0.45 / 0.10, τ = 30 days, pinned_boost = 0.2 (all tunable per request)
```

Namespaces organize workspaces, and recall always filters by namespace, but
that is single-tenant organization, not a security boundary: one API key (or
the open loopback default) can read and write *every* namespace, so do not
point mutually untrusted agents at one server.

## Quickstart

Commands verified green on 2026-09-07; the test suite, CLI commands, serve,
and both MCP handshakes were re-verified 2026-09-12.

Prereqs: Rust (the GNU toolchain is used on Windows here; a rustup
*directory override*, not a committed file, selects it; see D-005),
gcc for bundled SQLite, Node 24+ (web UI build and Playwright e2e).

```bash
# 1. Run the whole test suite (unit + integration + MCP conformance + eval)
cargo test --workspace

# 1b. Measure recall latency on the 10k fixture (criterion + exact percentiles).
#     Measured (2026-09-14): criterion hybrid_k5 ≈ 15 ms; exact-harness p95
#     ≈ 20-21 ms at 10k in steady state (25 ms PRD budget met; the first run
#     after a fresh build is slower — see docs/EVALUATION.md).
cargo bench -p recall-core
cargo test -p recall-core --release --test p95_latency -- --ignored --nocapture

# 2. Serve REST + MCP + web UI (creates recall.db if missing)
cargo run -p recall-cli -- serve --bind 127.0.0.1:8787 --db recall.db
#   Use a different embedder everywhere (hash | onnx | openai):
cargo run -p recall-cli -- serve --embedder onnx   # needs --features onnx + ORT_DYLIB_PATH
#   → REST:        http://127.0.0.1:8787/v1/...
#   → MCP:         http://127.0.0.1:8787/mcp  (streamable-HTTP; SDK targets the
#                  MCP 2026-07-28 spec — `initialize` answers `2025-11-25`, the
#                  newest legacy handshake version, because 2026-07-28 replaced
#                  the handshake with per-request metadata — see D-004)
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

# CLI JSON output omits embedding vectors by default (screen-reader friendly:
# no hundreds of spoken zeros per record). Pass --show-embedding on
# remember/recall/update to include them.

# 4. Web UI + accessibility e2e (builds UI, serves it from the Rust binary, runs Playwright + axe)
cd web && npm install && npm run build && npx playwright test
```

### MCP client wiring

- **stdio:** `recall-cli serve --stdio --db recall.db`
- **streamable-HTTP:** point the client at `http://127.0.0.1:8787/mcp`
- **Browser isolation (default, on):** the server validates the `Host`
  header (loopback hosts only; DNS-rebinding defense, `421` otherwise) and
  enforces a same-origin policy: requests carrying a foreign `Origin` are
  rejected (`403`) and **no CORS headers are granted by default**, so a web
  page you visit cannot read or write your memories while `serve` runs. The
  production web UI is served same-origin and the Vite dev server proxies
  `/v1`, so nothing needs CORS by default. To let a specific web app or
  browser-based agent call the API cross-origin, opt in explicitly:
  `--allow-origin https://agent.example` (repeatable; the origin is echoed in
  CORS headers). Non-loopback bindings additionally accept
  `--allow-host <host>` (repeatable) for LAN IPs or healthcheck hostnames.
- **API key:** set `RECALL_MCP_API_KEY` (or `--api-key`) to require
  `Authorization: Bearer` or `X-API-Key` on `/v1/*`, `/metrics` and `/mcp`,
  which is recommended whenever anything beyond your own user session can
  reach the port (the web UI does not send an API key; see the note in "Web UI").
  `/healthz` and `/openapi.json` always stay open.

### Operating the server (v0.3)

- **Rate limiting** (off by default): `--rate-limit 50` allows 50 sustained
  requests/second per API key with burst `max(2×rate, 10)`; override with
  `--rate-burst`. Over-limit requests get `429` + `Retry-After`. Also via env
  `RECALL_MCP_RATE_LIMIT` or a `--config` JSON file.
- **Body limit**: requests are capped at 1 MiB by default (`--body-limit`,
  env `RECALL_MCP_BODY_LIMIT`); oversize → `413`.
- **Config file**: `serve --config serve.json` accepts
  `{"bind": "...", "rate_limit": 50, "rate_burst": 120, "body_limit": 1048576,
  "embedder": "hash"}`.
  Precedence: flag > env > file > default (per knob). Secrets are never
  file-configurable.
- **Embedder selection**: `--embedder hash|onnx|openai` (env
  `RECALL_MCP_EMBEDDER`) applies to `serve` (HTTP + stdio), `remember`,
  `recall`, and `update`. `onnx`/`openai` require their cargo features at
  build time (`cargo build --features onnx` / `--features openai`) and fail
  with a clear error otherwise; a store must be read with the embedder that
  wrote it.
- **Graceful shutdown**: Ctrl-C/SIGTERM drains in-flight requests and
  checkpoints the WAL before exit.
- **Metrics**: `GET /metrics` (Prometheus 0.0.4) with a request counter, status
  codes, per-route latency histogram, embedding timings, rate-limit counter,
  and live store gauges.
- **Pagination**: `GET /v1/memories?namespace=&limit=&offset=` with
  `X-Total-Count` carrying the unpaginated total.

### Web UI

Served same-origin from the binary at `/` once built (step 4); semantic HTML,
keyboard-first, axe-tested. Known limitation (AR-017): the UI does not send an
API key, so it only works against a server running without
`RECALL_MCP_API_KEY`, which is the intended local single-user mode. With a key
configured, use the CLI/REST/MCP clients, or extend `web/src/api.ts` to
collect and send `X-API-Key`.

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
index is built from keyed HMAC token digests, so plaintext never touches disk;
keyword search still works. Databases created or first opened with a key are
upgraded in place (plaintext rows are re-encrypted on first keyed open); an
encrypted database refuses to open without the key. `recall-cli backup`
produces encrypted backups. Generate a key with: `openssl rand -hex 32`.
Caveats (details in D-014): `export` output is plaintext JSON by design, and
namespace names/tags/embeddings are not encrypted. The embedding caveat
matters: with the semantic embedders (`onnx`, `openai`), published
embedding-inversion attacks can reconstruct memory **text** from the stored
vector with usable fidelity, so keyed mode does not protect text there. The
server logs a warning at startup when you combine a key with those embedders;
use the default `hash` embedder when at-rest confidentiality is the
requirement.

### Local semantic embeddings (optional `onnx` feature)

The default `HashEmbedder` is lexical. For semantic matching fully offline,
build with `--features onnx` and run with `--embedder onnx`
(`OnnxEmbedder`, all-MiniLM-L6-v2 quantized, 384-dim): the model downloads
once from Hugging Face into `.fastembed_cache/`, and ONNX Runtime is loaded
at runtime; set `ORT_DYLIB_PATH` to `onnxruntime.dll` (windows-gnu has no
ort prebuilt; see D-013). On the golden set, HashEmbedder scores 0.980
hit-rate@5, the ONNX embedder 1.000 (docs/EVALUATION.md). `--embedder openai`
(`--features openai`, configured via `RECALL_MCP_OPENAI_*` env) exists for
API-backed deployments; it has never been called live (no key).

## Architecture

```
crates/
  recall-core/    # pure domain: models, Store trait + SQLite (FTS5, WAL, BLOB
                  # embeddings, packed vector cache, maintenance ops),
                  # Embedder trait (HashEmbedder default; ONNX/OpenAI behind
                  # features + runtime selection), explainable HybridScorer,
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
embeddings, slimmed in v0.3 to scan only scoring columns with an exact top-k
candidate heap (D-016), then served from a packed in-memory vector cache so
repeat recalls stop re-reading embedding BLOBs from SQLite (D-019; any write
invalidates it) → `HybridScorer` produces each component → deterministic
tie-break (total desc, created_at desc, id asc) → top-k. See
`docs/adr/DECISIONS.md` D-002, D-016 and D-019 for the storage trade-offs.

Two documented approximations and one honest gap in that pipeline:

- **Scan-cap boundary**: the vector scan covers a namespace's first
  `scan_cap` rows in rowid order (default 10 000). In namespaces larger than
  the cap, a keyword hit stored beyond the window gets `vector = 0.0` in its
  breakdown: the keyword pass still finds it, but its total is lower than
  the same memory would earn in a smaller namespace.
- **Keyword candidate cap**: the FTS pass considers at most `candidate_cap`
  (200) candidates.
- **`last_accessed_at` is write-time only**: recalls do not refresh it (it
  had no consumer; AR-018). The column is reserved for a future
  eviction/recency-of-use policy.

## Repository hygiene

- `cargo clippy --workspace --all-targets -- -D warnings`: 0 warnings (2026-09-12)
- `cargo fmt --all` enforced
- CI (`.github/workflows/ci.yml`): fmt + clippy, workspace tests, llvm-cov
  gate on recall-core (lines + regions >= 90; linux-gnu, D-010), windows-gnu test job
- Conventional commits; every feature slice landed with its tests

## CI note (2026-09-16)

GitHub Actions is DISABLED on this repository by owner decision (no paid Actions: the account is billing-blocked and the owner declined spend). Every quality gate was verified by local execution at the recorded HEAD. Zero-cost remote option if ever wanted: a self-hosted runner (re-enable via Settings -> Actions, or `gh api -X PUT repos/pranam-s/recall-mcp/actions/permissions -F enabled=true`).
