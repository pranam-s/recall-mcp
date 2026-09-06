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
