# Evaluation — Recall-MCP v0.3.1 (2026-09-11; adversarial-review fix pass)

Everything below was actually run on this machine (Windows 11 VM, GNU toolchain,
rustc 1.98.1). Nothing is claimed that was not executed. Numbers are exact.
This VM (VMware, 6 cores / 12 GB) shows ±10–15% run-to-run variance on the
latency benchmarks; where it matters, multiple runs are listed and never
averaged silently.

## Test suite (all green)

`cargo test --workspace` — **158 tests, 0 failures** (131 at the 2026-09-10
re-verification → 156 at the 2026-09-11 adversarial-review fix pass → 158
here: the AR-006 mutation-driven AAD pin test plus a count audit that found
the previous headline one short — the `--allow-origin` validation test from
`743bcd5` was never reflected in the cli-unit row; fmt clean,
clippy `--all-targets` 0 warnings), plus 3 ignored diagnostics in the default
build (1 core FTS query-plan probe + 2 release-only p95/cost harnesses; 4 with
`--features onnx`):

| Suite | Tests | What they cover |
|---|---|---|
| recall-core (unit, incl. crypto) | 94 | scorer math + determinism, embedder determinism/edge cases, embedder selection, namespace/memory CRUD + validation, update path, recall tag filter, FTS sync, ranking/normalization/pinning/k/decay-off/zero-embedding, pagination + count_memories (incl. wrapping `usize::MAX` LIMIT/OFFSET saturation, AR-004), maintenance, stats, export/import (**atomic import: a failed import leaves no partial rows or namespaces, AR-023**), error mapping, auth key extraction, OpenAPI doc validity, keyed FTS query builder, BLOB/packed-slice cosine bit-equality, recall top-k heap exactness vs brute force at 300 memories, packed-cache reuse/rebuild/write-invalidation/scan-cap parity, capture chunking, key/AEAD/digest properties, **HKDF key separation (RFC 5869 test-vector pin, subkeys ≠ master and ≠ each other, cross-decryption fails, AR-002), pre-HKDF store migration in one transaction (rows re-sealed, FTS digests recomputed, reopen idempotent, wrong key still fails), keyed upgrade shreds plaintext (secure_delete + truncated WAL, forensic canary test, AR-013), `last_accessed_at` is write-time only (AR-018), AAD wire-format literals pinned (mutation-found gap, AR-006)** |
| recall-core (proptests) | 4 | crypto roundtrip + AAD binding over arbitrary UTF-8, FTS MATCH-syntax validity + keyed 32-hex digests, chunking cap/content-preservation/determinism, scorer total = weighted component sum + component bounds + future-timestamp clamp (512 cases each by default) |
| recall-core (eval) | 1 | golden-set hit-rate@5 gate (below) |
| recall-core (onnx + eval, behind `onnx`) | 4 | model metadata, empty→zero vector, semantic ordering, golden set with OnnxEmbedder |
| recall-core (ignored, release) | 2 | exact p95 latency harness + cost-breakdown diagnostic (below) |
| recall-server (lib) | 32 | healthz open with auth on, server-info spec pin, error→status mapping, auth extraction, constant-time key matching, OpenAPI drift guards, 413 body limit, 429 rate limiting with Retry-After + per-key independence, **failed-key requests are throttled by the anonymous bucket (AR-012)**, /metrics content, timed embedder wrapper, router embedder wiring, **AR-001 origin guard: foreign Host → 421, cross-origin Origin → 403 with no CORS grant, same-origin and header-less clients served, `--allow-origin` echo + preflight + other origins still rejected, `--allow-host` extension**, **`page_limit` clamp unit tests (usize::MAX → 10 000)**, **MCP `list_memories` caps usize::MAX end-to-end with 10 001 memories seeded (AR-004)** |
| recall-server (api) | 8 | REST integration: namespaces, memories CRUD, recall + breakdown identity + determinism, PATCH update + tag filter, stats, full API-key matrix, list pagination with X-Total-Count, capture ingest e2e, **caller-supplied embedding with wrong dims → 400, matched dims → 201 (AR-005)** |
| recall-server (mcp_http) | 2 | MCP conformance over streamable-HTTP (6 tools, both error paths, API-key honored) |
| recall-server (proptests) | 1 | token-bucket partition/burst bound/independent buckets over generated configs |
| recall-cli (unit) | 8 | API-key precedence, embedder-name precedence, serve_config glue + fail-closed validation, `--allow-origin` validation |
| recall-cli (e2e) | 7 | real binary: export/import round trip; update + tag-filtered recall; encryption-at-rest e2e; backup + vacuum; `--embedder` end-to-end |
| recall-cli (mcp_stdio) | 1 | MCP conformance over stdio as a spawned child process |

`cargo clippy --workspace --all-targets -- -D warnings` — **0 warnings**.
`cargo fmt --all` clean.

## Retrieval quality (PRD metric)

- Golden set: `evals/golden.json` — exactly 50 queries with expected ids.
- **HashEmbedder (default): hit-rate@5 = 0.980 (49/50), gate ≥ 0.85 — PASS**
  (re-measured 2026-09-07 evening with the D-019 packed-cache recall path;
  unchanged, as the cache is bit-identical to the SQL path — unit-tested).
- **OnnxEmbedder (`onnx` feature): hit-rate@5 = 1.000 (50/50) — PASS** (v0.2
  result, embedder and golden set untouched since; selectable at runtime via
  `--embedder onnx` since the evening pass).

## Performance — p95 at 10k: packed cache halves recall again; budget still not met at exactly 10k (honest)

Two harnesses, same fixture (10,000 embedded memories, one namespace — the
documented worst case for the exact cosine scan):

- `cargo test -p recall-core --release --test p95_latency -- --ignored
  --nocapture` — exact percentiles over 1,000 recalls including the
  `last_accessed_at` refresh;
- `cargo bench -p recall-core` — criterion, same fixture, release profile.

Baseline for the evening pass was re-measured immediately before the change
(22:07 IST) so the comparison shares the VM's load and temperature:

| Metric (release, 10k fixture) | v0.2 (2026-09-06) | v0.3 D-016 (2026-09-07 morning) | v0.3 D-019 packed cache (2026-09-07 evening) |
|---|---|---|---|
| `Store::recall` p95 (exact harness) | 67.2 / 72.5 ms | 33.2 / 33.7 / 34.0 / 34.8 / 38.4 / 38.9 / 48.0 ms (7 runs) | **26.2 / 26.6 / 27.2 ms** (3 runs; same-day pre-change baseline run: 55.9 ms) |
| `Store::recall` mean (exact harness) | 53.5 ms | 29.6–37.3 ms | **21.7–22.3 ms** (baseline run: 45.1) |
| criterion `hybrid_k5` | 80.5 ms (CI 79.0–82.0) | 28.3 / 32.2 ms | **16.16 ms (CI 15.95–16.40)** — criterion reports −49.1% (CI −50.1 to −48.2) vs the same-day baseline |
| criterion `keyword_only_k5` | 41.7 ms (CI 41.2–42.2) | 9.9 ms (CI 9.85–10.04) | 11.1 ms (CI 10.6–11.9; within VM noise of the morning value) |
| PRD budget p95 < 25 ms | breached | still breached at exactly 10k | **still breached at exactly 10k on the exact harness (by 1.2–2.2 ms)** |

What changed (D-019): the first untagged vector pass packs the namespace's
scan columns into contiguous memory (row-major f32 + rowid/created_at/pinned);
repeat recalls scan that buffer instead of re-reading ~10 MB of embedding
BLOBs from SQLite. Any write invalidates the cache; tag-filtered recalls keep
the SQL path; the packed cosine is bit-identical to the reference and the
`scan_cap` LIMIT semantics are preserved row-for-row (all unit-tested).

**Cost breakdown (diagnostic, release; hybrid / keyword-only / full-row
materialization, ms per call):** pre-change baseline 39.95 / 16.65 / 279.7;
post-change runs 20.3 / 15.1 / 227.2, 11.4 / 15.2 / 235.3, 22.9 / 19.5 /
272.4. Read: the vector pass added ≈ 23 ms over the keyword floor in the
same-day pre-change baseline, and adds −4 to +5 ms across the post-change
runs — i.e. within run noise of zero. The D-016-identified blob-read floor
is gone. What remains at the 10k worst case is the FTS keyword pass plus
survivor fetch and the batched refresh (≈ 15 ms floor in the sustained
harness), which no longer has a documented one-line remedy: it is a query-
plan problem, not a blob-throughput problem.

**Honest conclusion:** the PRD's 25 ms budget is met comfortably up to a few
thousand memories per namespace. At exactly 10k the exact harness still
measures p95 ≈ 26–27 ms (this VM's ±10–15% variance spans the line; criterion
puts the hybrid mean at 16 ms), so the budget is reported as not met, not
massaged. Remediation path, dated: (a) done — D-016 slim scan + D-019 packed
cache (2.1×, then another 1.9× on criterion hybrid); (b) next, if 10k-scale
namespaces matter in practice: attack the FTS pass (candidate query plan) —
unmeasured territory, no committed date; (c) beyond ~100k memories/namespace
the D-002 revisit (packed vector file or ANN index — LanceDB first choice on
windows-gnu) becomes mandatory regardless. Golden-set-scale namespaces recall
in single-digit milliseconds.

## Encryption at rest (D-14, hardened 2026-09-11)

Verified by tests that open the database file bytes after writes (unit + real
binary CLI e2e, WAL included). New in v0.3: `recall-cli backup` copies
ciphertext verbatim, and the e2e suite proves a keyed backup restores, recalls,
refuses an unkeyed open, and refuses to overwrite an existing target. New in
v0.3.1: HKDF key separation is pinned by an RFC 5869 test vector plus
separation/cross-decryption properties (AR-002); a pre-HKDF store is migrated
in one transaction with end-to-end assertions; and the in-place plaintext→keyed
upgrade is proven not to leave a marker plaintext recoverable from the main
database file or the WAL sidecar (secure_delete + VACUUM + truncated
checkpoint, AR-013).

## Mutation testing (AR-006, new)

`mutants.toml` gates the review-named modules (`crypto.rs`, `scorer.rs`).
Result of the first recorded run (2026-09-11, cargo-mutants 27.1.0,
`--in-place -j 1`, debug profile):

**51 mutants: 45 caught, 0 missed, 6 unviable — 45/45 (100%) of viable
mutants caught.** Final run wall-clock 4 min (warm in-place build). The
exact invocation lives in `mutants.toml` (cargo-mutants 27.x selects files
with `-f` globs, not a config key); raw per-mutant outcomes land in the
gitignored `mutants.out/` and are deleted after recording.

- **Caught (45 = 25 crypto + 20 scorer):** every function-body replacement
  on `StoreKey` (`decrypt` → `Ok(Default)`, `is_encrypted` → `true/false`,
  `from_hex` rejections, digest truncation) and the whole fusion formula in
  `HybridScorer::score` (all `+/-/*//` swaps, `%` substitutions, `-`
  deletion, `tau > 0.0` guard flips, `mul_add` operand swaps).
- **Missed on the first run (2), both on `key_check_context`:** replacing
  the literal with `String::new()` / `"xyzzy"` survived every behavioral
  test because the key-check AAD is applied symmetrically — the same
  function seals (sqlite.rs) and opens the stored check value, so any
  mutated literal still round-trips. The AAD strings are on-disk wire
  format, not re-derivable logic. **Fix:** a format-pinning unit test
  (`aad_context_literals_are_pinned_wire_format`, crypto.rs) asserts the
  exact literals — the same technique as the RFC 5869 vector and
  `ENC_PREFIX` pins — plus a cross-context rejection. The re-run confirmed
  **0 missed**; no survived mutant is left unjustified.
- **Unviable (6, excluded from the denominator — they do not compile):**
  `MIN_BLOB_LEN` `+`→`-` (const underflow) and five `Default::default()`
  replacements for the non-`Default` `StoreKey` / `ScoreBreakdown` returns.

Side effect worth recording: mutant runs execute the proptest suites against
broken code, so proptest appends mutant-specific cases to
`*.proptest-regressions`; those entries pass against the real code and are
reverted after each run (they are run artifacts, not regressions).

## MCP conformance

Both transports pass the same smoke flow (initialize → tools/list → tools/call):
- streamable-HTTP (spec `2026-07-28` negotiated, stateless JSON mode)
- stdio (rmcp negotiates its default `2025-11-25`; SDK behavior, D-004)

## Web UI accessibility / e2e

`npx playwright test` (production server serving `web/dist`): create memory →
recall → score-breakdown table PASS (incl. keyboard-focus assertion); axe
(@axe-core/playwright, wcag2a/aa + wcag21a/aa): **0 violations**. Last full
run: 2026-09-07 22:53 IST (re-run after the evening pass — the server now
resolves its embedder through `ServerConfig`, default unchanged — and the v0.3
server changes; the UI's API usage, create/recall/breakdown, is unaffected).

## Coverage — gate is CI-enforced; not measurable on this machine

Same D-010 story: `cargo llvm-cov` cannot run on windows-gnu (no profiler
runtime). The gate `cargo llvm-cov -p recall-core --fail-under-lines 90
--fail-under-branches 90` runs in CI (linux-gnu job). The repo still has no
`origin` remote, so CI has not executed; coverage remains **unmeasured rather
than claimed**. Note for the first CI run: v0.3 and the 2026-09-11 pass added
core code (capture, maintenance, pagination, packed cache, embedder selection,
HKDF key separation + migration, origin guard, MCP clamp) and their tests in
the same commits; ignored tests are excluded from coverage measurement by
`--ignored` semantics, as in v0.2.

## Limitations

- Recall latency at exactly 10k memories still exceeds the PRD budget on the
  exact harness (p95 ≈ 26–27 ms vs 25 ms; measured above). The blob-read
  floor is gone (packed cache, D-019); the residual is the FTS keyword pass,
  and the D-002 ANN/packed-vector revisit remains the structural fix beyond
  ~100k. Remediation path is dated and measured, not speculative. Note: the
  p95 numbers above were measured before the AR-018 removal of the
  per-recall `last_accessed_at` write, which takes work off this path; the
  harness has not been re-baselined after that change.
- HashEmbedder is lexical: synonym/paraphrase queries with zero token overlap
  miss (fixed by the optional `onnx` embedder, which scores 1.000).
- Encryption (D-014) covers `memories.text` only; namespace names, tags,
  source, and embedding vectors stay plaintext by design, and dense
  embeddings are invertible to text (AR-003 — see D-014/D-021); `export`
  output is plaintext JSON (the CLI now warns and writes owner-only files).
  The in-place keyed upgrade shreds plaintext-era pages and WAL frames
  (AR-013, tested); filesystem copies made before the upgrade still contain
  plaintext — strict guarantees favor `backup` into a fresh keyed database.
- OpenAI embedder compiles and its request/parse logic is tested (including
  dims-mismatch rejection, AR-005), but no live API call has been made (no
  key available) — not on any default code path.
- Rate limiting keys on the presented API key and runs before auth (AR-012),
  so failed-key requests drain the shared `anonymous` bucket — key-guessing
  is throttled; distributed spoofed-key rotation is not (that threat model
  needs the Team Server, D-017).
- CI workflow (`.github/workflows/ci.yml`) is committed but has not executed
  (no remote configured yet).
- The `/metrics` endpoint counts the `/metrics` request only after its body is
  rendered; scrape-in-scrape self-inclusion is off by one by design.
