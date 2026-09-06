# Evaluation — Recall-MCP v0.2 (2026-09-06)

Everything below was actually run on this machine (Windows 11 VM, GNU toolchain,
rustc 1.98.1). Nothing is claimed that was not executed. Numbers are exact.

## Test suite (all green)

`cargo test --workspace` — 88 tests, 0 failures:

| Crate | Tests | What they cover |
|---|---|---|
| recall-core (unit) | 68 | scorer math + determinism, embedder determinism/edge cases, namespace/memory CRUD + validation, **memory update path** (partial update, stale-embedding clearing, validation, keyed-mode reindex), **recall tag filter (AND semantics, both passes)**, FTS sync across delete/update, recall ranking/normalization/pinning/k/decay-off/zero-embedding, stats, export/import round-trip + idempotency, error mapping, auth key extraction, OpenAPI doc validity, **keyed FTS query builder**, OpenAI request/parse + connection-error path (behind `openai` feature) |
| recall-core (crypto, unit) | 8 | key hex/raw/generate round-trips + rejections, AEAD encrypt/decrypt with context binding, wrong-key/tamper/garbage rejection, FTS token digest multiplicity + key separation |
| recall-core (eval) | 1 | golden-set hit-rate@5 gate (below) |
| recall-core (onnx, unit, behind `onnx`) | 3 | model metadata/shapes, empty→zero vector, semantic ordering + same-shape determinism |
| recall-core (onnx, eval, behind `onnx`) | 1 | same golden set with OnnxEmbedder (below) |
| recall-core (p95, ignored test, release) | 1 | exact latency percentiles on the 10k fixture (below) |
| recall-server (lib) | 5 | healthz open with auth on, server-info spec pin, error→status mapping, auth extraction, OpenAPI drift guards (PATCH + tags documented) |
| recall-server (api) | 6 | REST integration: namespaces (409/400), memories (404/400/201 + auto-embed), recall 200/400/404 + breakdown identity + determinism + decay-off, **PATCH update (re-embed, 404/400 paths) + tag filter**, stats, full API-key matrix |
| recall-server (mcp_http) | 2 | MCP conformance over streamable-HTTP: initialize → tools/list (**5 tools incl. update_memory**) → tools/call remember/recall/**update_memory**/list_memories/forget → tag-filtered recall → unknown-tool JSON-RPC error → namespace-error path; /mcp honors API key |
| recall-cli (unit) | 1 | API-key precedence (CLI > env, blank ignored) |
| recall-cli (e2e) | 4 | real binary: remember → recall → export → import round-trip; negative paths; **update + tag-filtered recall**; **encryption at rest end-to-end (plaintext absent from the DB file, unkeyed open refused)** |
| recall-cli (mcp_stdio) | 1 | MCP conformance over stdio as a spawned child process: initialize → initialized → tools/list (5 tools) → tools/call all five tools |

`cargo clippy --workspace --all-targets -- -D warnings` — **0 warnings**.
`cargo fmt --all` clean. The `onnx` feature additionally gates the unit + eval
tests above (`cargo test -p recall-core --features onnx` with `ORT_DYLIB_PATH`
set, D-013).

## Retrieval quality (PRD metric)

- Golden set: `evals/golden.json` — exactly 50 queries with expected ids, authored
  against `evals/corpus.json` (40 memories, 3 namespaces).
- **HashEmbedder (default): hit-rate@5 = 0.980 (49/50), gate ≥ 0.85 — PASS.**
  The single miss is the known lexical one (query "which integration do leavers
  miss most" vs a "leaving" memory — zero token overlap).
- **OnnxEmbedder (`onnx` feature, all-MiniLM-L6-v2-Q): hit-rate@5 = 1.000
  (50/50), same gate — PASS.** Real semantics fix the lexical miss while staying
  fully offline after the one-time model download (D-013).

## Encryption at rest (new in v0.2, D-014)

Verified by tests that open the database file bytes after writes:

- With `RECALL_MCP_KEY` set, `memories.text` stores only `enc:v1:` AEAD
  ciphertext (ChaCha20-Poly1305) and the FTS index contains only 128-bit keyed
  HMAC token digests — neither the secret nor its tokens appear in the raw DB
  file (also asserted end-to-end through the real CLI binary and WAL bytes).
- Wrong key fails at open (stored key-check blob); encrypted DB without a key
  refuses to open; legacy plaintext DBs upgrade in place on first keyed open and
  keyword recall survives the migration.

## MCP conformance

Both transports pass the same smoke flow (initialize → tools/list → tools/call):
- streamable-HTTP (spec `2026-07-28` negotiated, stateless JSON mode)
- stdio (rmcp negotiates its default `2025-11-25`; SDK behavior, see D-004)

## Web UI accessibility / e2e

`npx playwright test` (production server serving `web/dist`), last full run
2026-09-06 v0.1 state:
- create memory → recall → score-breakdown table: PASS (incl. keyboard-focus assertion)
- axe (@axe-core/playwright, tags wcag2a, wcag2aa, wcag21a, wcag21aa): **0 violations**

(Not re-run after the v0.2 backend slices; the UI and its API surface usage —
create/recall/breakdown — are unchanged by them.)

## Coverage — gate is CI-enforced; not measurable on this machine

Same D-010 story as v0.1: `cargo llvm-cov` cannot run on windows-gnu (no
profiler runtime). The gate
`cargo llvm-cov -p recall-core --fail-under-lines 90 --fail-under-branches 90`
is now **enforced in CI** (`.github/workflows/ci.yml`, linux-gnu job) instead of
merely documented. Note: this repo has no `origin` remote yet — the workflow is
committed but has not executed; until the first CI run, coverage remains
unmeasured rather than claimed.

## Performance — PRD budget measured for the first time: BREACHED at 10k

`cargo test -p recall-core --release --test p95_latency -- --ignored` (exact
percentiles over 1,000 recalls against 10,000 embedded memories in one
namespace — the documented worst case for the brute-force scan, D-002) and
`cargo bench -p recall-core` (criterion, same fixture), Windows 11 VMware VM
(6 cores / 12 GB), release profile:

| Metric (release, 10k fixture) | Measured |
|---|---|
| `Store::recall` p95 | **67.2 ms** (2 runs: 67.2 / 72.5) — budget 25 ms |
| `Store::recall` mean / p50 / p99 / max | 53.5 / 51.5 / 79.2 / 107.1 ms |
| criterion `recall_10k_memories/hybrid_k5` | 80.5 ms mean (CI 79.0–82.0, n=100) |
| criterion `recall_10k_memories/keyword_only_k5` | 41.7 ms mean (CI 41.2–42.2, n=100) |
| Query embedding (untimed; hash-256) | 0.023 ms avg |
| Fixture seeding | 10k memories in 2.50 s |

The criterion split is diagnostic: keyword-only (FTS MATCH over the full match
set, ranked, capped at 200) costs ~42 ms on its own; the brute-force vector
scan adds ~39 ms (10k rows materialized: JSON tags + 1 KB blob decode + cosine
each). Both passes are viable optimization targets before any index swap.

Honest reading: this is not a regression — it is the first time the number
exists, and the budget was aspirational. The cost is structural (D-002 chose an
exact brute-force scan): every query materializes all 10k rows (including JSON
tag parsing and 1 KB embedding-blob decode per row) and ranks the full FTS
match set before applying `candidate_cap`. Known, unimplemented remedies, in
expected-impact order: (1) slim scan query + decode embeddings without
allocation, deferring tags/text to candidates only; (2) rank FTS matches in SQL
with a LIMIT-aware ordering; (3) the D-002 revisit trigger — sqlite-vec or
LanceDB — now has its measured justification. Numbers on this VMware VM carry
virtualization overhead; relative improvements still transfer.

Until one of those lands, the PRD metric is honestly **not met at 10k
memories**; it is met comfortably at smaller namespace sizes (the golden-set
evals with 40 memories run in single-digit milliseconds end-to-end).

## Limitations

- HashEmbedder is lexical: synonym/paraphrase queries with zero token overlap
  miss (fixed in practice by the optional `onnx` embedder, which scores 1.000).
- Recall latency at 10k memories exceeds the PRD budget (measured above).
- Encryption (D-014) covers `memories.text` only: namespace names, tags,
  source, and embedding vectors (1 KB token sketches) stay plaintext by
  design; `export` output is plaintext JSON. In-place-upgraded databases may
  retain plaintext remnants in freelist pages — strict guarantees favor export
  into a fresh keyed database.
- OpenAI embedder compiles and its request/parse logic is tested, but no live
  API call has been executed (no key available) — flagged in STATUS.
- API-key comparison is not constant-time; acceptable for a local tool, upgrade
  path noted.
- CI workflow (`.github/workflows/ci.yml`) is committed but has not executed
  (no remote configured yet).
