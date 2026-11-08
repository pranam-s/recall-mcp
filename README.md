# Recall-MCP

mcp-name: io.github.pranam-s/recall-mcp

Recall-MCP is a local-first memory broker for AI agents. One SQLite file and one
server hold short records that agents write and read back through MCP over stdio
or streamable HTTP, REST, or the CLI. Every recall hit carries the score breakdown
behind its rank: `{bm25, vector, recency, pinned_boost, total}`.

Keyword search works offline with no model. An optional local ONNX embedder adds
semantic matching without sending text anywhere. An optional per-database key
encrypts memory text with ChaCha20-Poly1305.

Status: **0.5.0**, tagged 2026-11-08. The project is pre-1.0;
[docs/EVALUATION.md](docs/EVALUATION.md) records the measured results.

## Why it exists

AI agents start every session from zero. Recall-MCP gives them one place to keep
observations, decisions, and preferences, and a way to retrieve them later using
keyword, vector, and recency signals.

```text
total = w_bm25 * norm_bm25 + w_vector * cosine + w_recency * exp(-days/tau) + pinned_boost
defaults: 0.45 / 0.45 / 0.10, tau = 30 days, pinned_boost = 0.2 (tunable per request)
```

Every hit reports its score components, so a caller can see why a memory ranked
where it did. On the 100-query golden set the hash embedder scores 98/100 hit-rate@5
(0.980, MRR@10 0.975). The 50-query paraphrase set with ONNX scores 50/50
(1.000, MRR@10 0.740); the hash diagnostic on that set is 31/50 (0.620). Method
and output are in [docs/EVALUATION.md](docs/EVALUATION.md).

Namespaces organize workspaces, and recall always filters by namespace. They are
not a security boundary: one API key can read and write every namespace, so do
not point mutually untrusted agents at one server.

## Install

The v0.5.0 [GitHub release](https://github.com/pranam-s/recall-mcp/releases/tag/v0.5.0)
has a Windows x86_64 archive named `recall-mcp-windows-x86_64.zip`. It contains
`recall.exe`, `recall-cli.exe`, `LICENSE`, and `README.md`. Unzip it and add the
directory to `PATH`, or call `recall.exe` by its full path.

To build from source on any platform, install Rust stable and a C compiler for
the bundled SQLite build; MSVC and gcc both work. Node 24+ is only needed for the
web UI.

```sh
git clone https://github.com/pranam-s/recall-mcp
cd recall-mcp
cargo build --release --locked
```

The binaries land in `target/release/`: `recall` and the older `recall-cli`
alias. To install the CLI into Cargo's bin directory instead:

```sh
cargo install --path crates/recall-cli --locked
```

The crates.io packages `recall-mcp-core`, `recall-mcp-server`, and
`recall-mcp-cli` are not published yet; publication is pending. Release notes for
this line are in [releases/NOTES-0.5.0.md](releases/NOTES-0.5.0.md).

## Quickstart

The examples use a new `recall.db` in the current directory.

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

JSON is compact when stdout is a pipe and pretty on a terminal. `--pretty` forces
pretty output, `--full` restores the storage fields (`namespace_id`, `pinned`,
`last_accessed_at`), and `--show-embedding` includes the vector. Compact is the
default because one hash embedding is about 1.2 KB of JSON per record.

## Serving and MCP clients

```sh
recall serve --bind 127.0.0.1:8787 --db recall.db
```

That starts REST at `/v1/...`, MCP at `/mcp` over streamable HTTP, the OpenAPI
document at `/openapi.json`, and metrics at `/metrics`. If you built the web UI
with `cd web && npm ci && npm run build`, add `--web-dir web/dist` to serve it at
`/`.

```console
$ curl -s http://127.0.0.1:8787/healthz
{"service":"recall-mcp","status":"ok","version":"0.5.0"}
```

MCP over stdio uses `recall serve --stdio --db recall.db`. The six tools are
`remember`, `recall`, `forget`, `update_memory`, `list_memories`, and `capture`.
REST and MCP responses use compact records by default. Set
`include_embedding=true` for full records, and `include_memories=true` on capture
to return the stored chunks.

The server targets the MCP `2026-07-28` specification: stateless per-request
metadata, `server/discover`, and the `MCP-Protocol-Version`, `Mcp-Method`, and
`Mcp-Name` headers. A legacy `initialize` naming `2026-07-28` is answered with
`2025-11-25`, the newest version that still uses a handshake (D-004).

Client snippets for Claude Code, Codex, Cursor, OpenCode v2, Warp, and Antigravity
are in [docs/clients.md](docs/clients.md), along with the portable agent skill in
`skills/recall-mcp/`. The transports are covered by conformance tests; the
snippets are starting points and have not been verified against every client
version.

## Configuration

Flags take precedence over environment variables, then the JSON config file, then
defaults, per setting. The API key comes from a flag or environment variable; the
encryption key comes from the environment. Neither is file-configurable.

| Setting | Flag | Environment | Config file | Default |
|---|---|---|---|---|
| Database file | `--db` | `RECALL_MCP_DB` | none | `recall.db` |
| Namespace for one-shot commands | `--namespace` | `RECALL_MCP_NAMESPACE` | none | required for `remember`, `recall`, `capture` |
| Bind address | `--bind` | `RECALL_MCP_BIND` | `bind` | `127.0.0.1:8787` |
| Serve config file | `--config` | none | n/a | none |
| MCP over stdio | `--stdio` | none | none | off |
| Web UI directory | `--web-dir` | none | none | none |
| API key | `--api-key` | `RECALL_MCP_API_KEY` | excluded | off |
| Rate limit per key | `--rate-limit` | `RECALL_MCP_RATE_LIMIT` | `rate_limit` | off |
| Rate-limit burst | `--rate-burst` | none | `rate_burst` | `max(2 x rate, 10)` |
| Request body limit | `--body-limit` | `RECALL_MCP_BODY_LIMIT` | `body_limit` | 1 MiB |
| Embedder | `--embedder` | `RECALL_MCP_EMBEDDER` | `embedder` | `hash` |
| Allowed cross-origin | `--allow-origin` / `--allow-origins` | none | `allow_origins` / `allow_origin` | same-origin only |
| Extra Host values | `--allow-host` / `--allow-hosts` | none | `allow_hosts` / `allow_host` | loopback hosts |
| Embedder mismatch override | `--allow-embedder-mismatch` | `RECALL_MCP_ALLOW_EMBEDDER_MISMATCH` | none | off |
| Encryption key | none | `RECALL_MCP_KEY` or `RECALL_MCP_KEY_FILE` | excluded | off |
| OpenAI embedder | none | `RECALL_MCP_OPENAI_ENDPOINT`, `_API_KEY`, `_MODEL`, `_DIMS` | excluded | unset |

Over-limit requests get `429` with `Retry-After`; oversize bodies get `413`;
missing or wrong content types get `415`; unknown paths, wrong methods, bad query
parameters, and malformed JSON return the same JSON error shape. Foreign `Host`
and `Origin` values are rejected with `421` and `403`. Ctrl-C or SIGTERM drains
in-flight requests and checkpoints the WAL.

## Embedders

The default `hash` embedder is lexical, deterministic, and 256-dimensional. For
semantic matching fully offline, build with the `onnx` feature and run with
`--embedder onnx`. It uses quantized all-MiniLM-L6-v2 at 384 dimensions, downloads
the model once into `.fastembed_cache/`, and loads ONNX Runtime through
`ORT_DYLIB_PATH`.

`--embedder openai` exists for API-backed deployments. Its request, dimension,
and response parsing code is covered by tests, but it has never been called
against the live API (no key was available).

The store records the embedder fingerprint (`name@dimensions`, plus the model for
OpenAI) in `store_meta`. An empty store is stamped, a legacy store with matching
dimensions is adopted with a warning, and a fingerprint or dimension mismatch
fails with both sides named. `--allow-embedder-mismatch` is the explicit override
and does not rewrite the stored fingerprint.

## Capture

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

Messages are chunked deterministically (paragraph, then sentence, then a hard wrap
at `max_chunk_chars`, default 1000), embedded, and stored with
`source = "capture"`. Role prefixes become `role:*` tags. Namespace creation and
every chunk insert run in one `BEGIN IMMEDIATE` transaction, so a failure stores
nothing.

## Security model

The default posture is that your local user session can talk to the server.
Anything beyond that is opt-in.

Requests are checked against a loopback `Host` allowlist (`localhost`,
`127.0.0.1`, `::1`); anything else gets `421`. A foreign `Origin` gets `403`, and
the server grants no CORS headers by default. Origin comparison includes the
scheme and normalizes default ports. Opt in per origin with
`--allow-origin https://agent.example`, and add LAN or container hostnames with
`--allow-host`.

With an API key configured, `/v1/*`, `/metrics`, and `/mcp` require
`Authorization: Bearer` or `X-API-Key`. Comparison is constant time. The rate
limiter runs before auth, and only the configured key gets its own bucket;
invalid and missing keys share a bounded anonymous bucket, so forged keys cannot
dodge the limit or grow memory without bound. `/healthz` and `/openapi.json` stay
open.

Every response carries a strict CSP, `nosniff`, frame denial, `no-referrer`,
COOP/CORP, and a permissions policy. API, MCP, and metrics responses are
`no-store`.

With `RECALL_MCP_KEY` (64 hex characters) or `RECALL_MCP_KEY_FILE`, memory text is
sealed with ChaCha20-Poly1305 and the keyword index stores keyed HMAC digests.
Every plaintext-to-keyed transition runs with `secure_delete`, then VACUUM and
`wal_checkpoint(TRUNCATE)`, and leaves a retryable clean marker. A forensic test
covers a deleted plaintext row and a WAL frame from the plaintext era. An
encrypted store refuses an unkeyed open, and backups stay encrypted.

Encryption does not cover namespace names, tags, `source`, or embedding vectors;
those stay plaintext by design. Dense neural embeddings (`onnx`, `openai`) can be
inverted to text with published attacks, so keyed mode does not protect text
there. The server warns at startup, and the lexical `hash` embedder is the
at-rest confidentiality choice. Exports are plaintext JSON by design, and the CLI
warns.

Recalled text is data from past sessions. The server does not treat it as
instructions. The MCP tool descriptions say so, and the poisoning threat model is
D-021. Client errors are short and typed; internal faults return
`500 {"error":"internal server error"}` with details only in logs.

## Architecture

```text
crates/
  recall-core/    domain: models, validation, views, Store trait + SQLite
                  (FTS5, WAL, BLOB vectors, packed cache, data_version guard),
                  Embedder trait (hash default; ONNX and OpenAI behind features),
                  HybridScorer, capture chunking, export/import
  recall-server/  axum REST (/v1/*), OpenAPI, /metrics, API key, rate limiter,
                  Host/Origin guard, security headers, JSON errors, rmcp tools
                  over streamable HTTP and stdio
  recall-cli/     clap CLI: serve, remember, recall, update, forget, list,
                  capture, stats, keygen, export, import, backup, vacuum
web/              React 19 + Vite 8 + TypeScript UI, Playwright + axe tests
evals/            100-memory corpus, 100 golden queries, 50 semantic queries
skills/           agent skill and CLI/MCP references
docs/             architecture, decisions, evaluation, clients, token efficiency
```

Every surface is a thin shell over the same engine, and the MCP transport is
swappable because core does not know it exists.
[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) has the full design, and
[docs/adr/DECISIONS.md](docs/adr/DECISIONS.md) records why each choice was made.
[docs/TOKEN-EFFICIENCY.md](docs/TOKEN-EFFICIENCY.md) covers the compact response
formats and their measured byte sizes.

Recall runs an FTS5 keyword pass, an exact cosine pass over embedding BLOBs in
rowid order, and then merges and scores the candidates. The top `k` return with
their breakdowns. Rows beyond `scan_cap` get `vector = 0.0`, the keyword pass sees
at most `candidate_cap` candidates, and the first recall after a fresh process
fills the packed vector cache and is slower than steady state. Beyond roughly
100k memories per namespace, the D-002 ANN revisit applies.

## Performance

The 10k-memory recall harness is an ignored manual test. The 10k design target is
25 ms p95, where p95 means 95% of timed recalls finish at or below that value.
The harness and its method are described in
[docs/EVALUATION.md](docs/EVALUATION.md).

## Project status

Version 0.5.0 is the first public release. GitHub Actions runs the CI workflow on
pushes to `master` and on pull requests. It covers formatting, lint, tests on
Linux and Windows, coverage, MSRV, rustdoc, feature builds, the web build and
Playwright run, and the dependency audits.

Limitations:

- No Docker image is published. The `Dockerfile` is in the repository, but there
  is no pullable image.
- The OpenAI embedder has never been called against the live API.
- Client snippets are starting points, not verified against every client.
- Dense embeddings are invertible, so keyed mode does not protect text when
  `onnx` or `openai` is in use.
- ANN retrieval beyond roughly 100k memories per namespace is deferred (D-002).
- Branch coverage is unmeasured; cargo-llvm-cov has no branch fail-under on
  stable (D-010 amendment).
- Namespaces are not a security boundary.

## Contributing

Bug reports, fixes, tests, docs, and dependency updates are welcome. Read
[CONTRIBUTING.md](CONTRIBUTING.md) for the build, gates, and conventions. Security
issues go through the private advisory flow in [SECURITY.md](SECURITY.md).
Deferred ideas and the reasons behind them are in [ideas.md](ideas.md).

## License

MIT. See [LICENSE](LICENSE).
