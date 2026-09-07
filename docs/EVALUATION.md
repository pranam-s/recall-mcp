# Evaluation — Recall-MCP v0.3 (2026-09-07; evening pass: D-019 packed cache + embedder selection)

Everything below was actually run on this machine (Windows 11 VM, GNU toolchain,
rustc 1.98.1). Nothing is claimed that was not executed. Numbers are exact.
This VM (VMware, 6 cores / 12 GB) shows ±10–15% run-to-run variance on the
latency benchmarks; where it matters, multiple runs are listed and never
averaged silently.

## Test suite (all green)

`cargo test --workspace` — **126 tests, 0 failures** (110 at the v0.3 morning
pass → 126), plus 2 ignored release-only diagnostics (p95 harness + cost
breakdown):

| Suite | Tests | What they cover |
|---|---|---|
| recall-core (unit, incl. crypto) | 85 | scorer math + determinism, embedder determinism/edge cases, **embedder selection (`parse_embedder_name`/`select_embedder`: all kinds, case/whitespace tolerance, unknown-name errors, feature-gated errors)**, namespace/memory CRUD + validation, update path, recall tag filter, FTS sync, ranking/normalization/pinning/k/decay-off/zero-embedding, pagination (offset pages tile without overlap) + count_memories, maintenance (checkpoint, vacuum, VACUUM INTO backup restores, keyed backup stays encrypted, overwrite refused), stats, export/import, error mapping, auth key extraction, OpenAPI doc validity, keyed FTS query builder, BLOB-cosine bit-equality vs the reference (incl. hoisted-norm variant), **packed-slice cosine bit-equality**, recall top-k heap exactness vs brute force at 300 memories, **packed-cache reuse/rebuild policy (namespace/dim/cap), write invalidation (insert/update/pin/delete/namespace switch), scan-cap parity through the cache**, capture chunking (boundaries, hard wrap, unicode, determinism), key/AEAD/digest properties |
| recall-core (eval) | 1 | golden-set hit-rate@5 gate (below) |
| recall-core (onnx + eval, behind `onnx`) | 4 | model metadata, empty→zero vector, semantic ordering, golden set with OnnxEmbedder |
| recall-core (ignored, release) | 2 | exact p95 latency harness + cost-breakdown diagnostic (below) |
| recall-server (lib) | 16 | healthz open with auth on, server-info spec pin, error→status mapping, auth extraction, constant-time key matching properties, OpenAPI drift guards for capture / metrics / pagination / X-Total-Count / 429 shape, 413 body limit, 429 rate limiting with Retry-After + per-key independence, /metrics content (counters, histogram, gauges), timed embedder wrapper, **router embeds with the configured embedder (fixed-vector embedder → cosine 1.0 end-to-end)** |
| recall-server (api) | 8 | REST integration: namespaces, memories CRUD, recall + breakdown identity + determinism, PATCH update + tag filter, stats, full API-key matrix, list pagination with X-Total-Count, capture ingest e2e (chunking, role tags, recall integration, negative paths) |
| recall-server (mcp_http) | 2 | MCP conformance over streamable-HTTP (5 tools, both error paths, API-key honored) |
| recall-cli (unit) | 7 | API-key precedence, **embedder-name precedence (flag > env > file > default), embedder resolution errors, `--embedder onnx` fails closed without the feature, serve_config glue: file+flags land in `ServerConfig` (bind/rate/burst/body-limit/embedder/api-key, per-knob precedence, burst default), serve_config fail-closed (negative rate, unknown file keys, burst < rate, unknown embedder, zero body limit clamps to 1), serve_config onnx-without-feature refused** |
| recall-cli (e2e) | 6 | real binary: export/import round trip; update + tag-filtered recall; encryption-at-rest e2e; backup (restores, recalls, refuses overwrite) + vacuum (keeps data); **`--embedder` end-to-end (unknown name refuses remember and serve, hash round-trips)** |
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

## Encryption at rest (D-14, unchanged in v0.3)

Verified by tests that open the database file bytes after writes (unit + real
binary CLI e2e, WAL included). New in v0.3: `recall-cli backup` copies
ciphertext verbatim, and the e2e suite proves a keyed backup restores, recalls,
refuses an unkeyed open, and refuses to overwrite an existing target.

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
than claimed**. Note for the first CI run: v0.3 and the evening pass added
core code (capture, maintenance, pagination, packed cache, embedder
selection) and their tests in the same commits; the two ignored release tests
are excluded from coverage measurement by `--ignored` semantics, as in v0.2.

## Limitations

- Recall latency at exactly 10k memories still exceeds the PRD budget on the
  exact harness (p95 ≈ 26–27 ms vs 25 ms; measured above). The blob-read
  floor is gone (packed cache, D-019); the residual is the FTS keyword pass
  plus fetch/refresh, and the D-002 ANN/packed-vector revisit remains the
  structural fix beyond ~100k. Remediation path is dated and measured, not
  speculative.
- HashEmbedder is lexical: synonym/paraphrase queries with zero token overlap
  miss (fixed by the optional `onnx` embedder, which scores 1.000).
- Encryption (D-014) covers `memories.text` only; namespace names, tags,
  source, and embedding vectors stay plaintext by design; `export` output is
  plaintext JSON. In-place-upgraded databases may retain plaintext remnants in
  freelist pages — `recall-cli vacuum` compacts them; strict guarantees favor
  `backup` into a fresh keyed database.
- OpenAI embedder compiles and its request/parse logic is tested, but no live
  API call has been made (no key available) — not on any default code path.
- Rate limiting keys on the presented API key (shared `anonymous` bucket when
  absent) — it throttles per-key floods and key-guessing, not distributed
  spoofed-key rotation; that threat model needs the Team Server (D-017).
- CI workflow (`.github/workflows/ci.yml`) is committed but has not executed
  (no remote configured yet).
- The `/metrics` endpoint counts the `/metrics` request only after its body is
  rendered; scrape-in-scrape self-inclusion is off by one by design.
