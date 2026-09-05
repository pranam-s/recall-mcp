# Evaluation — Recall-MCP v0.1 (2026-09-06)

Everything below was actually run on this machine (Windows 11 VM, GNU toolchain,
rustc 1.98.1). Nothing is claimed that was not executed. Numbers are exact.

## Test suite (all green)

`cargo test --workspace` — 66 tests, 0 failures:

| Crate | Tests | What they cover |
|---|---|---|
| recall-core (unit) | 49 | scorer math + determinism, embedder determinism/edge cases, namespace/memory CRUD + validation, FTS sync across delete, recall ranking/normalization/pinning/k/decay-off/zero-embedding, stats, export/import round-trip + idempotency, error mapping, auth key extraction, OpenAPI doc validity, OpenAI request/parse + connection-error path (behind `openai` feature) |
| recall-core (eval) | 1 | golden-set hit-rate@5 gate (below) |
| recall-server (lib) | 5 | healthz open with auth on, server-info spec pin, error→status mapping, auth extraction |
| recall-server (api) | 5 | REST integration: namespaces (409/400), memories (404/400/201 + auto-embed), recall 200/400/404 + breakdown identity + determinism + decay-off, stats, full API-key matrix (401 missing/wrong, Bearer and X-API-Key pass, healthz/openapi open) |
| recall-server (mcp_http) | 2 | MCP conformance over streamable-HTTP: initialize → tools/list (4 tools, schemas) → tools/call remember/recall/list_memories/forget → unknown-tool JSON-RPC error → namespace-error path; /mcp honors API key (401 without, works with Bearer) |
| recall-cli (unit) | 1 | API-key precedence (CLI > env, blank ignored) |
| recall-cli (e2e) | 2 | real binary: remember → recall → export → import round-trip; negative paths (unknown namespace, blank text) |
| recall-cli (mcp_stdio) | 1 | MCP conformance over stdio as a spawned child process: initialize → initialized → tools/list → tools/call all four tools |

`cargo clippy --workspace --all-targets` — **0 warnings**. `cargo fmt --all` applied.

## Retrieval quality (PRD metric)

- Golden set: `evals/golden.json` — exactly 50 queries with expected ids, authored
  against `evals/corpus.json` (40 memories, 3 namespaces).
- **Actual hit-rate@5 = 0.980 (49/50), gate ≥ 0.85 — PASS.**
- The single miss is honest and instructive: query *"which integration do leavers
  miss most"* against the churn memory (target says "leaving", "calendar
  integrations"). The default HashEmbedder is purely lexical, so a query with no
  token overlap cannot match. This is a known property of the offline embedder,
  not a bug; swapping in a semantic `Embedder` impl (trait-ready) would fix it.

## MCP conformance

Both transports pass the same smoke flow (initialize → tools/list → tools/call):
- streamable-HTTP (spec `2026-07-28` negotiated, stateless JSON mode)
- stdio (rmcp negotiates its default `2025-11-25`; SDK behavior, see D-004)

## Web UI accessibility / e2e

`npx playwright test` (production server serving `web/dist`):
- create memory → recall → score-breakdown table: PASS (incl. keyboard-focus assertion)
- axe (@axe-core/playwright, tags wcag2a, wcag2aa, wcag21a, wcag21aa): **0 violations**

## Coverage — honest status: NOT MEASURED on this machine (gate documented)

The PRD asks for ≥90% lines and branches on recall-core, gated with
`cargo llvm-cov --fail-under-lines 90 --fail-under-branches 90`. Attempts on
windows-gnu, in order:

1. `cargo llvm-cov -p recall-core` → `error[E0463]: can't find crate for 'profiler_builtins'`
   (profiler runtime not distributed for windows-gnu; `llvm-tools-preview` added).
2. nightly-x86_64-pc-windows-gnu + `rust-src` +
   `CARGO_UNSTABLE_BUILD_STD=std,panic_abort` → std rebuilt, same E0463.
3. `CARGO_UNSTABLE_BUILD_STD=std,panic_abort,profiler_builtins` → build of
   `profiler_builtins` itself fails (compiler-rt sources not shipped with rust-src).
4. Alternatives ruled out: cargo-tarpaulin (no Windows support), grcov (needs the
   same `-C instrument-coverage` runtime), MSVC toolchain (excluded by ADR-005).

**Decision (D-010):** the 90/90 gate runs on a linux-gnu CI runner with the
standard command; on this Windows/GNU VM the number is reported as *not
measurable* rather than estimated. The unit-test table above is the honest
proxy for branch behavior — every error branch and filter arm listed has an
asserting test — but no coverage percentage is claimed here.

## Performance (PRD: p95 recall < 25 ms on 10k memories)

**Not measured in v0.1** — no benchmark harness was built within scope. The
design bounds are documented (brute-force scan capped at `scan_cap` rows of
256-dim f32 ≈ 1 KB each; FTS5 candidates capped at 200) but the latency number
is unclaimed until `criterion`/`hyperfine` numbers exist. Listed as v1.1 work.

## Limitations

- HashEmbedder is lexical: synonym/paraphrase queries with zero token overlap
  miss (see the golden-set miss above).
- Vector search is an exact scan, capped at `scan_cap` (10k) rows per namespace;
  ARCHITECTURE.md's sqlite-vec table is deliberately deferred (D-002).
- No memory update path (insert/delete only) — FTS5 sync triggers cover
  insert/delete; update would need the third trigger.
- Single-user, single-process (one SQLite file, `Mutex<dyn Store>`). No TLS; run
  behind a reverse proxy or keep it on loopback.
- OpenAI embedder compiles and its request/parse logic is tested, but no live API
  call has been executed (no key available) — flagged in STATUS.
- API-key comparison is not constant-time; acceptable for a local tool, upgrade
  path noted.
