# Recall-MCP

mcp-name: io.github.pranam-s/recall-mcp

**A local-first memory broker for AI agents.** One SQLite file, one server,
and every answer shows its work: agents `remember` decisions and `recall` them
later through MCP (stdio or streamable-HTTP), REST, or a CLI. Each hit carries
the score breakdown that ranked it: `{bm25, vector, recency, pinned_boost,
total}`. Keyword search works offline out of the box. An optional local ONNX
embedder adds semantic matching without sending text anywhere. An optional
per-database key seals memory text with ChaCha20-Poly1305.

Status: **0.5.0, tagged 2026-11-08**. See
[docs/STATUS.md](docs/STATUS.md) for the current done/partial list and
[docs/EVALUATION.md](docs/EVALUATION.md) for every measured number.

## Why

AI agents start every session from zero. Recall-MCP is a vendor-neutral memory
layer: store observations, decisions, and preferences as short records, then
retrieve them with hybrid scoring.

```text
total = w_bm25 * norm_bm25 + w_vector * cosine + w_recency * exp(-days/tau) + pinned_boost
defaults: 0.45 / 0.45 / 0.10, tau = 30 days, pinned_boost = 0.2 (tunable per request)
```

Retrieval quality on the 100-query golden set: hash embedder 98/100
hit-rate@5 (0.980, MRR@10 0.975). The 50-query paraphrase set is gated with
ONNX: 50/50 (1.000, MRR@10 0.740); the hash diagnostic is 31/50 (0.620).
Method and exact output are in [docs/EVALUATION.md](docs/EVALUATION.md).

Namespaces organize workspaces, and recall always filters by namespace. They
are not a security boundary: one API key can read and write every namespace,
so do not point mutually untrusted agents at one server.

## Install

Prerequisites: Rust stable and a C compiler for bundled SQLite (MSVC or gcc).
Node 24+ is only needed for the web UI.

```sh
cargo install recall-mcp-cli --version 0.5.0 --locked
# or install the checkout you are reading:
cargo install --path crates/recall-cli --locked
```

The package installs both `recall` and the older `recall-cli` alias.

## Try it

Script the short `recall` binary:

```console
$ recall remember --db recall.db --namespace work --text "We deploy on Thursdays" --tags ops
{"created_at":1794127372,"id":"19ca950c-d6b5-42e0-af12-1b41f143a998","source":"cli","tags":["ops"],"text":"We deploy on Thursdays"}

$ recall recall --db recall.db --namespace work --query "when do we deploy?" --k 1
[{"breakdown":{"bm25":1.0,"pinned_boost":0.0,"recency":1.0,"total":0.775,"vector":0.5},"created_at":1794127372,"id":"19ca950c-d6b5-42e0-af12-1b41f143a998","source":"cli","tags":["ops"],"text":"We deploy on Thursdays"}]

$ recall recall --db recall.db --namespace work --query "when do we deploy?" --format text
1. [0.775] id=19ca950c-d6b5-42e0-af12-1b41f143a998 created_at=1794127372 source=cli tags=ops
   We deploy on Thursdays
   bm25=1.000 vector=0.500 recency=1.000 pinned_boost=0.000
```

JSON is compact when stdout is a pipe and pretty on a terminal. `--pretty`
forces pretty output, `--full` restores the storage fields (`namespace_id`,
`pinned`, `last_accessed_at`), and `--show-embedding` includes the vector. The
defaults are compact because the embedding alone is about 1.2 KB of JSON per
record.

Serve REST, MCP, and the web UI:

```sh
recall serve --bind 127.0.0.1:8787 --db recall.db --web-dir web/dist
# REST:      http://127.0.0.1:8787/v1/...
# MCP:       http://127.0.0.1:8787/mcp  (streamable-HTTP)
# OpenAPI:   http://127.0.0.1:8787/openapi.json
# Metrics:   http://127.0.0.1:8787/metrics
# Web UI:    http://127.0.0.1:8787/
```

Live probes:

```console
$ curl -s http://127.0.0.1:8787/healthz
{"service":"recall-mcp","status":"ok","version":"0.5.0"}

$ curl -si -X POST http://127.0.0.1:8787/v1/namespaces -H 'content-type: application/json' -d '{"name":"work"}' | head -1
HTTP/1.1 201 Created
```

The REST list endpoint returns a compact page:

```json
{"items":[{"id":"...","text":"...","source":"cli","created_at":123}],"total":1,"limit":100,"offset":0,"has_more":false}
```

Pass `include_embedding=true` to restore full records. `POST /v1/capture`
returns a summary (`{namespace, captured, first_id, last_id}`) unless
`include_memories=true`.

### MCP clients

- stdio: `recall serve --stdio --db recall.db`
- streamable-HTTP: `http://127.0.0.1:8787/mcp`
- Six tools: `remember`, `recall`, `forget`, `update_memory`, `list_memories`,
  `capture`.

The server targets the MCP `2026-07-28` spec: stateless per-request metadata,
`server/discover`, and the `MCP-Protocol-Version`, `Mcp-Method`, and `Mcp-Name`
headers. A legacy `initialize` naming `2026-07-28` is answered with
`2025-11-25`, the newest version with a handshake (D-004).

Client snippets for Claude Code, Codex, Cursor, OpenCode v2, Warp, and
Antigravity are in [docs/clients.md](docs/clients.md). They are expected
starting points, not live-verified against every client version.

## Configuration

Precedence is flag, then environment, then config file, then default, per knob.
Secrets are env or flag only; they are never file-configurable.

| What | Flag | Environment | Config file | Default |
|---|---|---|---|---|
| Database file | `--db` | `RECALL_MCP_DB` | none | `recall.db` |
| Namespace for one-shot commands | `--namespace` | `RECALL_MCP_NAMESPACE` | none | required |
| Bind address | `--bind` | `RECALL_MCP_BIND` | `bind` | `127.0.0.1:8787` |
| MCP over stdio | `--stdio` | none | none | off |
| Web UI directory | `--web-dir` | none | none | none |
| Require API key | `--api-key` | `RECALL_MCP_API_KEY` | secret | off |
| Rate limit per key | `--rate-limit` | `RECALL_MCP_RATE_LIMIT` | `rate_limit` | off |
| Rate-limit burst | `--rate-burst` | none | `rate_burst` | `max(2 x rate, 10)` |
| Request body limit | `--body-limit` | `RECALL_MCP_BODY_LIMIT` | `body_limit` | 1 MiB |
| Embedder | `--embedder` | `RECALL_MCP_EMBEDDER` | `embedder` | `hash` |
| Allowed cross-origin | `--allow-origin` / `--allow-origins` | none | `allow_origins` / `allow_origin` | same-origin only |
| Extra Host values | `--allow-host` / `--allow-hosts` | none | `allow_hosts` / `allow_host` | loopback hosts |
| Embedder mismatch override | `--allow-embedder-mismatch` | `RECALL_MCP_ALLOW_EMBEDDER_MISMATCH` | none | off |
| Encryption key | none | `RECALL_MCP_KEY` or `RECALL_MCP_KEY_FILE` | secret | off |
| OpenAI embedder | none | `RECALL_MCP_OPENAI_ENDPOINT`, `_API_KEY`, `_MODEL`, `_DIMS` | secret | unset |

Over-limit requests get `429` with `Retry-After`; oversize bodies get `413`;
missing or wrong content types get `415`; unknown paths, wrong methods, bad
query parameters, and malformed JSON all return the same JSON error shape.
Foreign `Host` and `Origin` values are rejected (`421`/`403`). Ctrl-C or
SIGTERM drains requests and checkpoints the WAL.

### Embedders

The default `hash` embedder is lexical, deterministic, and 256-dimensional.
For semantic matching fully offline, build with `--features onnx` and run
`--embedder onnx` (all-MiniLM-L6-v2 quantized, 384 dimensions). The model
downloads once into `.fastembed_cache/`; ONNX Runtime loads through
`ORT_DYLIB_PATH`. `--embedder openai` exists for API-backed deployments; its
request, dimension, and parse logic is tested, but it has never been called
live here (no key).

The store records the embedder fingerprint (`name@dimensions`, plus the model
for OpenAI) in `store_meta`. An empty store is stamped, a legacy store with
matching dimensions is adopted with a warning, and a fingerprint or dimension
mismatch fails with both sides named. `--allow-embedder-mismatch` is the
explicit override and does not rewrite the stored fingerprint.

### Capture ingest

```bash
curl -X POST http://127.0.0.1:8787/v1/capture \
  -H 'content-type: application/json' \
  -d '{
    "namespace": "session-42",
    "tags": ["session:42"],
    "transcript": [
      {"role": "user", "content": "we chose sqlite over a vector db", "at": 1786000000},
      {"role": "assistant", "content": "rationale: local-first, one file, FTS5 plus brute-force cosine is enough at this scale"}
    ]
  }'
```

Messages are chunked deterministically (paragraph, then sentence, then hard
wrap at `max_chunk_chars`, default 1000), embedded, and stored with
`source = "capture"`. Role prefixes become `role:*` tags. Namespace creation
and every chunk insert run in one `BEGIN IMMEDIATE` transaction, so a failure
stores nothing.

## Security and threat model

The default posture is "your local user session can talk to it". Everything
beyond that is opt-in.

- **Browser isolation.** The guard accepts loopback `Host` values only
  (`localhost`, `127.0.0.1`, `::1`) and rejects foreign ones with `421`. A
  foreign `Origin` is rejected with `403`, and no CORS headers are granted by
  default. Origin comparison includes the scheme and normalizes default ports.
  Opt in per origin with `--allow-origin https://agent.example`; add LAN or
  container hostnames with `--allow-host`.
- **API key.** With a key configured, `/v1/*`, `/metrics`, and `/mcp` require
  `Authorization: Bearer` or `X-API-Key`. Matching is constant time. The rate
  limiter runs before auth, and only a key matching the configured one gets its
  own bucket; invalid and missing keys share the bounded anonymous bucket, so
  rotating forged keys cannot dodge the limit or grow memory without bound.
  `/healthz` and `/openapi.json` stay open.
- **Headers.** Every response carries a strict CSP, `nosniff`, frame denial,
  `no-referrer`, COOP/CORP, and a permissions policy. API, MCP, and metrics
  responses are `no-store`.
- **Encryption at rest.** With `RECALL_MCP_KEY` (64 hex characters) or
  `RECALL_MCP_KEY_FILE`, memory text is sealed with ChaCha20-Poly1305 and the
  keyword index stores keyed HMAC digests. Every plaintext-to-keyed transition
  runs with `secure_delete`, then VACUUM and `wal_checkpoint(TRUNCATE)`, and
  leaves a retryable clean marker. A forensic test covers a deleted plaintext
  row and a WAL frame from the plaintext era. An encrypted store refuses an
  unkeyed open, and backups stay encrypted.
- **What encryption does not cover.** Namespace names, tags, `source`, and
  embedding vectors stay plaintext by design. Dense neural embeddings
  (`onnx`, `openai`) are invertible to text with published attacks, so keyed
  mode does not protect text there; the server warns at startup and the lexical
  `hash` embedder is the at-rest confidentiality choice. Exports are plaintext
  JSON by design and the CLI warns.
- **Untrusted data.** Recalled text is data from past sessions, never
  instructions. The MCP tool descriptions say so; the poisoning threat model is
  D-021.
- **Error hygiene.** Client errors are short and typed. Internal faults return
  `500 {"error":"internal server error"}` with details only in logs.

## Architecture

```text
crates/
  recall-core/    pure domain: models, validation, views, Store trait + SQLite
                  (FTS5, WAL, BLOB vectors, packed cache, data_version guard),
                  Embedder trait (hash default; ONNX/OpenAI behind features),
                  explainable HybridScorer, capture chunking, export/import
  recall-server/  axum REST (/v1/*), OpenAPI, /metrics, API-key middleware,
                  bounded token-bucket limiter, Host/Origin guard, security
                  headers, JSON error contract, rmcp tools over streamable-HTTP
                  and stdio, spawn_blocking bridges for store and embedder work
  recall-cli/     clap CLI: serve (HTTP+MCP or stdio), remember, recall, update,
                  forget, list, capture, stats, keygen, export, import, backup,
                  vacuum; ships as `recall` and `recall-cli`
web/              React 19 + Vite 8 + TypeScript UI; Playwright + axe e2e
evals/            corpus (100 memories), golden set (100 queries), semantic set (50)
skills/           agent skill and CLI/MCP references
docs/             architecture, decisions, evaluation, status, clients, history
```

Every surface is a thin shell over the same engine. The MCP transport is
swappable because nothing in core knows it exists. Details:
[ARCHITECTURE.md](docs/ARCHITECTURE.md).

Retrieval pipeline:

1. FTS5 produces up to `candidate_cap` keyword candidates, ordered by BM25
   with a rowid tie-break.
2. A brute-force cosine pass covers the first `scan_cap` rows in rowid order,
   using the packed vector cache when no tag filter applies.
3. Candidates are merged and scored, then sorted by total desc, created_at
   desc, rowid asc.
4. The top `k` return with their breakdown.

Documented approximations: rows beyond `scan_cap` get `vector = 0.0` in the
breakdown, and the keyword pass sees at most `candidate_cap` candidates. The
first recall after a fresh process fills the packed cache and is slower than
steady state. Beyond roughly 100k memories per namespace, the D-002 ANN
revisit applies.

## Performance

The ignored 10k-memory harness measured p95 **27.3 to 35.3 ms** on this host on
2026-11-08, against the PRD budget of 25 ms. The budget is **not met here**;
the harness remains an ignored manual gate and was not relaxed. Means were
17.7 to 22.9 ms and p50 16.5 to 22.0 ms. Recorded runs and the older history are
in [docs/EVALUATION.md](docs/EVALUATION.md).

## Repository hygiene

- `cargo fmt --all --check`: clean.
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`:
  0 warnings.
- `cargo test --workspace`: green (exact counts in EVALUATION).
- Coverage on recall-core, measured with llvm-cov on windows-msvc:
  **95.39% lines / 94.19% regions**, gate 90/90.
- `cargo audit` and `cargo deny check`: pass; one allowed unmaintained
  transitive `paste` warning with a review note.
- Web: `npm run build` and 8 Playwright tests pass with axe WCAG 2.1 AA at
  0 violations.
- Conventional commits; each slice landed with its tests.
- Dependencies are permissively licensed (MIT-compatible).

## CI note

GitHub Actions is disabled on this repository for account billing reasons
(recorded 2026-09-16). The workflows in `.github/workflows/` are committed but
have not run remotely. Every number above was measured locally on
windows-msvc, not in CI.

## Releases

Releases are annotated tags; `v0.5.0` is the first. The crates publish as
[`recall-mcp-core`](https://crates.io/crates/recall-mcp-core),
[`recall-mcp-server`](https://crates.io/crates/recall-mcp-server), and
[`recall-mcp-cli`](https://crates.io/crates/recall-mcp-cli). The release notes
for this line are in [releases/NOTES-0.5.0.md](releases/NOTES-0.5.0.md); the
checklist is [RELEASING.md](RELEASING.md). Crates.io publication is owner-gated.

## License

MIT; see [LICENSE](LICENSE). Contributions are welcome; read
[CONTRIBUTING.md](CONTRIBUTING.md) for the gates and conventions. Deferred
ideas live in [ideas.md](ideas.md).
