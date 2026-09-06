# Evaluation — Recall-MCP v0.3 (2026-09-07)

Everything below was actually run on this machine (Windows 11 VM, GNU toolchain,
rustc 1.98.1). Nothing is claimed that was not executed. Numbers are exact.
This VM (VMware, 6 cores / 12 GB) shows ±10–15% run-to-run variance on the
latency benchmarks; where it matters, multiple runs are listed and never
averaged silently.

## Test suite (all green)

`cargo test --workspace` — **110 tests, 0 failures** (88 in v0.2 → 110), plus
2 ignored release-only diagnostics (p95 harness + cost breakdown):

| Suite | Tests | What they cover |
|---|---|---|
| recall-core (unit, incl. crypto) | 77 | scorer math + determinism, embedder determinism/edge cases, namespace/memory CRUD + validation, update path, recall tag filter, FTS sync, ranking/normalization/pinning/k/decay-off/zero-embedding, **pagination (offset pages tile without overlap) + count_memories**, **maintenance (checkpoint, vacuum, VACUUM INTO backup restores, keyed backup stays encrypted, overwrite refused)**, stats, export/import, error mapping, auth key extraction, OpenAPI doc validity, keyed FTS query builder, **BLOB-cosine bit-equality vs the reference (incl. hoisted-norm variant)**, **recall top-k heap exactness vs brute force at 300 memories**, **capture chunking (boundaries, hard wrap, unicode, determinism)**, key/AEAD/digest properties |
| recall-core (eval) | 1 | golden-set hit-rate@5 gate (below) |
| recall-core (onnx + eval, behind `onnx`) | 4 | model metadata, empty→zero vector, semantic ordering, golden set with OnnxEmbedder |
| recall-core (ignored, release) | 2 | exact p95 latency harness + **cost-breakdown diagnostic** (below) |
| recall-server (lib) | 15 | healthz open with auth on, server-info spec pin, error→status mapping, auth extraction, **constant-time key matching properties**, **OpenAPI drift guards for capture / metrics / pagination / X-Total-Count / 429 shape**, **413 body limit**, **429 rate limiting with Retry-After + per-key independence**, **/metrics content (counters, histogram, gauges)**, **timed embedder wrapper** |
| recall-server (api) | 8 | REST integration: namespaces, memories CRUD, recall + breakdown identity + determinism, PATCH update + tag filter, stats, full API-key matrix, **list pagination with X-Total-Count**, **capture ingest e2e (chunking, role tags, recall integration, negative paths)** |
| recall-server (mcp_http) | 2 | MCP conformance over streamable-HTTP (5 tools, both error paths, API-key honored) |
| recall-cli (unit) | 1 | API-key precedence |
| recall-cli (e2e) | 5 | real binary: export/import round trip; update + tag-filtered recall; encryption-at-rest e2e; **backup (restores, recalls, refuses overwrite) + vacuum (keeps data)** |
| recall-cli (mcp_stdio) | 1 | MCP conformance over stdio as a spawned child process |

`cargo clippy --workspace --all-targets -- -D warnings` — **0 warnings**.
`cargo fmt --all` clean.

## Retrieval quality (PRD metric)

- Golden set: `evals/golden.json` — exactly 50 queries with expected ids.
- **HashEmbedder (default): hit-rate@5 = 0.980 (49/50), gate ≥ 0.85 — PASS**
  (unchanged by the D-016 fast path: candidate selection is provably exact).
- **OnnxEmbedder (`onnx` feature): hit-rate@5 = 1.000 (50/50) — PASS** (v0.2
  result, embedder and golden set untouched since).

## Performance — p95 at 10k: 2.1× faster, budget still not met (honest)

`cargo test -p recall-core --release --test p95_latency -- --ignored` (exact
percentiles, 1,000 recalls against 10,000 embedded memories, one namespace —
the documented worst case for the exact cosine scan) and
`cargo bench -p recall-core` (criterion, same fixture), release profile:

| Metric (release, 10k fixture) | v0.2 (2026-09-06) | v0.3 (2026-09-07) |
|---|---|---|
| `Store::recall` p95 | 67.2 / 72.5 ms | **33.2 / 33.7 / 34.0 / 34.8 / 38.4 / 38.9 / 48.0 ms** (7 runs; median ≈ 34.8) |
| `Store::recall` mean | 53.5 ms | 29.6–37.3 ms across the same runs |
| criterion `hybrid_k5` | 80.5 ms (CI 79.0–82.0) | 28.3 ms (CI 28.06–28.65) and 32.2 ms (CI 30.5–34.1) on separate runs |
| criterion `keyword_only_k5` | 41.7 ms (CI 41.2–42.2) | 9.9 ms (CI 9.85–10.04) |
| PRD budget p95 < 25 ms | breached | **still breached at exactly 10k** |

What changed (D-016): the vector pass no longer materializes every row — it
scans four slim columns, computes cosine directly from borrowed BLOB bytes
with the query norm hoisted, keeps an exact top-k heap (a row's vector-only
total is its final total outside the keyword set), fetches full rows only for
survivors, and batches the `last_accessed_at` refresh. `cache_size`/`mmap_size`
pragmas added. The keyword pass, scoring, and visible results are unchanged.

**Cost breakdown (new diagnostic, `recall_cost_breakdown_on_10k_memories`,
release, same fixture):** full-row materialization of 10k rows — the v0.2
vector-pass approach — costs ≈ 264 ms/call in isolation; the FTS-keyword pass
plus candidate handling is ≈ 10–16 ms; the slim scan adds ≈ 18–20 ms on top of
the keyword floor. That residual is SQLite blob-read throughput (≈ 10 MB of
embedding BLOBs stepped row-by-row), near the storage floor for this access
pattern — the micro-optimizations (rowid keys, borrowed blobs, hoisted norm,
batched refresh) bought little beyond the structural slim-scan change.

**Honest conclusion:** the PRD's 25 ms budget is met comfortably up to a few
thousand memories per namespace and not met at exactly 10k with the default
`scan_cap`. The dated remediation, now grounded in measurement: (a) a packed
in-process embedding cache bounded by `scan_cap` (≈ 10 MB at the default cap)
would remove the blob-read floor and is the next incremental step; (b) beyond
~100k memories/namespace, the D-002 revisit (packed vector file or ANN index —
LanceDB first choice on windows-gnu) becomes mandatory. The breach does not
worse a user at realistic namespace sizes: golden-set-scale namespaces recall
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
run: 2026-09-07 (re-run after the v0.3 server changes; the UI itself is
unchanged and its API usage — create/recall/breakdown — is unaffected).

## Coverage — gate is CI-enforced; not measurable on this machine

Same D-010 story: `cargo llvm-cov` cannot run on windows-gnu (no profiler
runtime). The gate `cargo llvm-cov -p recall-core --fail-under-lines 90
--fail-under-branches 90` runs in CI (linux-gnu job). The repo still has no
`origin` remote, so CI has not executed; coverage remains **unmeasured rather
than claimed**. Note for the first CI run: v0.3 added core code (capture,
maintenance, pagination) and core tests in the same commits; the two new
ignored release tests are excluded from coverage measurement by `--ignored`
semantics, as in v0.2.

## Limitations

- Recall latency at 10k memories still exceeds the PRD budget (measured above);
  remediation path is dated and measured, not speculative.
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
