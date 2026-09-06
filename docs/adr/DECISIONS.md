# Decision log — Recall-MCP

One entry per notable decision, appended as decisions land. ADR-001…006 below
expand the summaries embedded in `docs/ARCHITECTURE.md`; later numbers are new.

## D-001 (ADR-001) — Rust workspace with a pure-core crate

`recall-core` holds models, the `Store` trait + SQLite implementation, embedders,
and the scorer with **no axum/MCP/async types**. Server and CLI are thin shells.
Chosen to make the branch-coverage target testable without an HTTP runtime and to
keep the MCP transport swappable.

## D-002 (ADR-002, amended) — SQLite + FTS5; brute-force cosine scan instead of sqlite-vec

SQLite (bundled, WAL) with an external-content FTS5 index trigger-synced to
`memories` is the store. **Deviation from the ARCHITECTURE.md schema comment**: the
vector index is *not* sqlite-vec in v1 — embeddings live in the `memories.embedding`
BLOB and recall does an exact brute-force cosine scan over the namespace (capped by
`RecallParams.scan_cap`, default 10k rows).

Rationale: exact (not approximate) similarity, deterministic explainable
breakdowns, no extension-loading risk on windows-gnu, and the PRD's own risk note
("document 1M-row ceiling") matches scan-based search at v1 scale. Revisit when
stores exceed ~100k memories or p95 recall latency regresses beyond budget.

## D-003 (ADR-003) — HashEmbedder default; OpenAI embedder as opt-in feature

`HashEmbedder` is deterministic feature-hashing (FNV-1a to 256 buckets, sign from
the hash's top bit, L2-normalized): true offline, platform-stable, and good enough
for lexical retrieval. `OpenAIEmbedder` (OpenAI-compatible `/v1/embeddings`, rustls)
compiles behind the non-default `openai` feature; its request/parse logic is unit
tested, the live HTTP call is not (no key in CI) — stated plainly in STATUS.

## D-004 (ADR-004) — rmcp 3.2.0 SDK; no hand-rolled fallback

The official Rust MCP SDK (`rmcp` 3.2.0, released 2026-08-31) implements the
`2026-07-28` spec; no fallback needed. Tools are defined with `#[tool_router]` /
`#[tool]` / `#[tool_handler]`; streamable-HTTP uses `StreamableHttpService` in
stateless + JSON-response mode; stdio via `rmcp::transport::io::stdio()`.
Note: over streamable-HTTP the client's `2026-07-28` request is honored; over stdio
rmcp negotiates its default `2025-11-25` (SDK behavior, observed in tests).

## D-005 (ADR-005) — GNU toolchain on Windows, project-scoped

`stable-x86_64-pc-windows-gnu` via a directory rustup override (global default
left untouched); gcc 15.2.0 (scoop) compiles bundled SQLite. No MSVC Build Tools.

## D-006 (ADR-006) — UI is a pure API client; hand-written OpenAPI

`web/` is a Vite SPA talking only to `/v1/*` (same-origin; the server serves the
built assets when `--web-dir` is set). `openapi.json` is hand-written and served
from the binary with a parse-test — utoipa's derive machinery buys nothing for six
endpoints and pins macro versions. Drift is guarded by an openapi content test.

## D-007 — Store trait is `Send` (not `Sync`); shared via `Arc<Mutex<dyn Store>>`

rusqlite's `Connection` is `!Sync` (RefCell-based caches). Making the whole store
an interior-Mutex type leaked locking into every method; instead the trait requires
only `Send` and `AppState::with_store(|store| …)` wraps locking (poison-recovering).
A re-entrant lock bug in the CLI remember path was caught by e2e and fixed by
holding one guard per logical operation.

## D-008 — API-key auth for the 2026 MCP auth gap

When `RECALL_MCP_API_KEY` (or `--api-key`) is set, `/v1/*` **and** `/mcp` require
`Authorization: Bearer <key>` or `X-API-Key: <key>`; failures are `401` with
`WWW-Authenticate: Bearer` and a JSON error body (no info leak). `/healthz` and
`/openapi.json` stay open for probes/spec discovery. Unset key = open access —
the local-first default, stated in the README security notes rather than hidden.
stdio needs no auth (local child process).

## D-009 — MCP `remember` auto-creates namespaces; REST stays strict

The PRD user story is an agent calling `remember{workspace, text}` with no
setup step, so the MCP tool runs `get_or_create_namespace` first. The REST
`POST /v1/memories` still returns 404 for an unknown namespace (explicit API
semantics). The asymmetry is intentional and documented in the tool description.

## D-010 — llvm-cov cannot run on windows-gnu; coverage gate documented, not faked

`cargo llvm-cov` fails on `stable-x86_64-pc-windows-gnu`:
`error[E0463]: can't find crate for 'profiler_builtins'` — the profiler runtime is
not distributed for windows-gnu. Attempts: llvm-tools component (same error);
nightly-gnu + `CARGO_UNSTABLE_BUILD_STD=std,panic_abort` + rust-src (same error);
adding `profiler_builtins` to build-std (build fails inside profiler_builtins —
compiler-rt sources not shipped). Alternatives: cargo-tarpaulin (Linux-only),
grcov (same `-C instrument-coverage` requirement → same missing runtime), MSVC
toolchain (excluded by D-005). Resolution: the 90/90 gate is enforced via the
standard command (`cargo llvm-cov -p recall-core --fail-under-lines 90
--fail-under-branches 90`) on a linux-gnu CI runner; on this VM it is reported as
**not measurable** rather than guessed. See docs/EVALUATION.md.

## D-011 — e2e runs against one production-shaped server

`recall-cli serve --web-dir web/dist` serves the built UI and the API from a
throwaway DB; Playwright's `webServer` spawns exactly that process (`web/e2e/serve.mjs`)
and waits on `/healthz`. No vite dev server in the e2e path.

## D-012 — Frontend stack versions verified against npm on 2026-09-06

React 19.2.8, Vite 8.2.2, TypeScript 7.0.2 (strict `tsc --noEmit` in the build),
Tailwind 4.3.3 (CSS-first, `@tailwindcss/vite`), Playwright 1.63.0,
@axe-core/playwright 4.13.0, Node 24.

## D-013 — Local ONNX embeddings adopted behind the `onnx` feature (spike passed)

The spike (spikes/onnx-spike, this repo) answered the open windows-gnu question
from ALTERNATIVES.md §3. Attempt 1: `fastembed` default features
(`ort/download-binaries`) → build failure: `no prebuilt binaries available for
target x86_64-pc-windows-gnu` (ort 2.0.0-rc.13 ships only windows-MSVC
prebuilts). Attempt 2: `fastembed` with `default-features = false, features =
["ort-load-dynamic", "hf-hub-rustls-tls"]` plus the official
`onnxruntime-win-x64-1.28.0.dll` loaded at runtime via `ORT_DYLIB_PATH` (the
MSVC-built DLL is C ABI; LoadLibrary needs no GNU link step) — **works**:
rustc 1.98.1, stable-x86_64-pc-windows-gnu, all-MiniLM-L6-v2-Q, 384-dim
L2-normalized vectors, paraphrase-vs-unrelated cosine 0.759 vs 0.041,
same-batch-shape embeddings bit-identical, 3 texts in ~8.5 ms. Golden-set eval
with the same 0.85 gate: **hit-rate@5 = 1.000 (50/50)** vs HashEmbedder's
0.980 — the known lexical miss ("which integration do leavers miss most") is
fixed by real semantics. Decision: `OnnxEmbedder` (`fastembed` 6.x,
non-default `onnx` cargo feature, default model all-MiniLM-L6-v2-Q) with the
`Embedder` trait contract preserved (empty text → zero vector; inference
serialized behind a mutex; failures log and yield a zero vector). HashEmbedder
stays the default (deterministic, zero-download); batch-size is fixed at one
text per call to keep embeddings batch-shape-stable. Documented runtime
requirements: `ORT_DYLIB_PATH` → `onnxruntime.dll` 1.28.0 (download from the
onnxruntime GitHub releases), first model fetch from Hugging Face (rustls).

## D-014 — Encryption at rest: AEAD over `memories.text` + keyed FTS token digests

Competitor research (COMPETITORS.md rec #1) made plaintext single-file storage
a marketed weakness. Design, implemented in `recall-core::crypto` + the store:

- Key: 256-bit, from `RECALL_MCP_KEY` (64 hex chars) or `RECALL_MCP_KEY_FILE`;
  auto-enabled when present (no cargo feature — it is a runtime deployment
  choice). ChaCha20-Poly1305 (RustCrypto `chacha20poly1305` 0.11), random
  12-byte nonce per record, AAD binding ciphertext to the row id
  (`recall-mcp:v1:text:<id>`), stored as `enc:v1:<base64(nonce||ct||tag)>`.
- Keyword search survives encryption: the FTS5 index is built over
  `memories.fts_tokens`, which holds per-token HMAC-SHA256 digests (truncated
  to 128 bits) of the text under the same key. BM25 sees the same token
  multiplicities; queries are digested with the same key; nothing reversible
  is on disk. Token digests are deterministic, so two identical texts produce
  identical FTS documents (a deliberate, documented property).
- Migration: schema v2 adds `fts_tokens` + `store_meta`; legacy databases are
  upgraded in place on open (rebuild of the FTS index over the new column) and
  a keyed first open re-encrypts all plaintext rows in one transaction and
  stores a key-check blob, so a wrong key fails at open instead of corrupting
  reads. Fresh keyed databases never write plaintext. `VACUUM` runs after the
  legacy in-place upgrade; remnants of the plaintext era can persist in
  freelist pages of databases encrypted in place — for strict guarantees,
  export into a fresh keyed database.
- Scope: `memories.text` only. Namespace names, tags, source, and embeddings
  (1 KB f32 sketches — statistically weak leak) remain plaintext; exports are
  plaintext JSON by design (backup format). Documented in README security
  notes rather than hidden.
- Alternative rejected: SQLCipher (whole-file encryption) — changes the file
  format for every user (unencrypted DBs too), drags an OpenSSL-shaped
  dependency onto windows-gnu, and still leaves plaintext in the FTS content
  table unless the index is encrypted as well; application-level AEAD covers
  the actual sensitive column with none of that.

## D-015 — Memory update in place + ALL-of tag filter for recall

Update semantics: **in-place edit, not append**. `Store::update_memory(id,
MemoryUpdate)` changes only the fields present on the update; `id`,
`created_at`, and `last_accessed_at` are preserved. Rationale: agents quote a
memory id they just stored, so a supersede-by-id flow is one round trip; an
append-style log needs consolidation reads (v2 candidate alongside
COMPETITORS.md rec #4) and currently has no consumer. Policy details:

- Text change without an explicit embedding **clears** the stored vector — a
  stale vector that no longer matches the text is worse than no vector; the
  server and CLI layers re-embed automatically before updating, so API/MCP/CLI
  callers always keep embeddings fresh.
- Blank replacement text and empty updates are `invalid input`; unknown ids are
  `memory not found`.
- FTS5 stays exact via the (new) `memories_fts_update` trigger, guarded with
  `WHEN old.fts_tokens IS NOT new.fts_tokens`, so access-time
  `last_accessed_at` refreshes never churn the index. Works identically in
  encrypted mode because both the ciphertext and the token digest column are
  rewritten together (D-014).

Surfaces: MCP tool `update_memory` (five tools now), REST
`PATCH /v1/memories/{id}`, CLI `update`.

Tag filter: `recall` accepts a tag list with **ALL-of (AND) semantics** —
exact, case-sensitive string match against the stored tag arrays, enforced as a
correlated `json_each` count in both the FTS candidate pass and the vector scan
pass, so `k` applies *after* filtering. AND (not OR) chosen because recall is
for narrowing within a namespace; OR users can issue multiple recalls and merge
with the score breakdowns intact. `RecallParams` dropped `Copy` for the
`Vec<String>` field — it is only ever passed by reference.

## D-016 — Recall fast path: exact top-k heap + slim BLOB scan (measured)

The first measured p95 (67.2 ms at 10k memories, v0.2) tripped D-002's own
revisit trigger. Before swapping storage engines, the brute-force scan itself
was re-engineered (2026-09-07), keeping results exact:

- The vector pass scans only `(rowid, embedding, created_at, pinned)`, reads
  the BLOB through a borrowed `ValueRef` (zero Rust-side allocations), and
  computes cosine straight from BLOB bytes (`util::cosine_f32_blob*`) with the
  query norm hoisted out of the loop — bit-identical arithmetic to the
  reference `cosine`.
- Candidate selection is exact: outside the FTS candidate set bm25 is 0, so a
  row's *vector-only total* (cosine, recency, pinned boost via the same
  `HybridScorer`) **is** its final total. A bounded max-heap of the k best
  vector-only rows (comparator mirroring the final ranking, inverted) therefore
  provably contains every row that could reach the final top-k; everything it
  evicts sorts strictly below the heap's weakest member and can never surface.
  Full rows (text/tags/source) are fetched only for survivors — at most
  `candidate_cap + k` rows instead of the whole namespace — and the
  `last_accessed_at` refresh is one batched UPDATE.
- Ranking-order refinement: ties on `(total, created_at)` are now broken by
  insertion order (rowid) instead of the random UUID id. This only orders
  content-identical duplicates that straddle the k-boundary; outcomes are
  deterministic per database either way.
- Session pragmas `cache_size = -8000` (8 MiB) and `mmap_size = 128 MiB` keep
  large scans off the OS read path. Semantics-neutral.

Measured on the 10k fixture (this VM, release): criterion `hybrid_k5`
80.5 ms → 28.3 ms mean; `keyword_only_k5` 41.7 ms → 9.9 ms; exact-percentile
harness p95 69.4 ms → 33–35 ms (see EVALUATION.md for run-by-run numbers).
A rejected experiment: rewriting the FTS namespace filter as
`rowid IN (SELECT …)` made SQLite correlate the subquery per FTS match —
1.6 s per query; the JOIN formulation stays. The residual ~20 ms is SQLite
blob-read throughput in the O(n) scan — the structural fix is the D-002
revisit (packed in-process vectors or an ANN index); the trigger is now
formally measured rather than speculative.

## D-017 — Production hardening defaults for the HTTP/MCP surface

Adopted 2026-09-07 so the server is operable beyond "works on loopback":

- **Constant-time key comparison** (RustCrypto `subtle::ConstantTimeEq`) for
  API-key checks; timing no longer leaks key-prefix matches.
- **Body limit**: 1 MiB default on every request (`DefaultBodyLimit`),
  configurable via flag/env/config file; oversize → 413. Memory texts are
  short by design; bulk imports that bring their own vectors fit comfortably.
- **Rate limiting**: token bucket per presented key (one shared `anonymous`
  bucket when no key is presented, which also throttles key-guessing), applied
  with auth to `/v1/*` + `/mcp`; `429` + `Retry-After`. Off by default (the
  local-first single-user default); enabled by `--rate-limit` (sustained req/s)
  and optional `--rate-burst` (default max(2×rate, 10)). Validation fails
  closed on non-positive rates. `healthz`/`openapi.json` stay unlimited —
  probes must survive load.
- **Metrics**: `/metrics` in Prometheus text exposition (0.0.4) — request
  counter, status codes, per-route latency histogram, embedding timings (a
  timed wrapper around the shared embedder covers REST, MCP and capture),
  rate-limit counter, and live store gauges scraped through the store. It is
  auth-protected like `/v1/*` when a key is configured; scrapers send the key.
- **Graceful shutdown**: Ctrl-C/SIGTERM stops accepts, drains in-flight
  requests, then checkpoints the WAL so the next open starts from a clean
  main file. SIGTERM is a no-op arm on Windows.
- **Maintenance**: `Store::checkpoint_wal/vacuum/vacuum_into` with CLI
  `backup --out` (hot compacted snapshot via `VACUUM INTO`; refuses to
  overwrite; keyed stores yield keyed backups because ciphertext is copied
  verbatim) and CLI `vacuum` (in-place compact + checkpoint).
- **Release profile**: `lto = "fat"`, `codegen-units = 1`,
  `strip = "symbols"` for the shipped binary.
- **Config precedence**: CLI flag > environment (`RECALL_MCP_BIND`,
  `RECALL_MCP_RATE_LIMIT`, `RECALL_MCP_BODY_LIMIT`) > optional JSON
  `--config` file (bind, rate_limit, rate_burst, body_limit) > defaults.
  Secrets (API key, encryption key) remain env/flag-only — no secrets in
  world-readable files.

Rejected: tower-governor (extra dependency for ~60 lines of bucket math) and
scraping /metrics unauthenticated (would leak store sizes when a key is set).

## D-018 — Auto-capture ingestion: deterministic chunking, REST-first

COMPETITORS.md rec #2 ("manual remember() starves the store") landed as a
scaffold: `POST /v1/capture` takes `{namespace, transcript: [{role?, content,
at?}], tags?, max_chunk_chars?}` and stores memories with `source = "capture"`.

- Chunking lives in pure core (`recall_core::capture::chunk_text`): ≤ max_chars
  (default 1000, clamped 100..=8000) with paragraph → sentence → hard-wrap
  boundaries, greedy packing, unicode-safe, deterministic. Each chunk embeds
  itself (a chunk's vector must match the text a future query matches).
- Roles are prefixed into the text and added as `role:<role>` tags so
  tag-filtered recall and analytics work without new tables; caller tags are
  merged in; message `at` timestamps pass through to `created_at`.
- The namespace is auto-created, mirroring MCP `remember` (D-009): capture is
  the agent-shaped, no-setup path. Insertions happen under one store lock.
- REST-first is deliberate: hooks (session-end, IDE, CI) speak HTTP; an MCP
  `capture` tool would mostly duplicate `remember` for interactive agents. If
  demand appears, the same core chunking is reusable behind a tool in minutes.
