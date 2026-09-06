# STATUS — Recall-MCP v0.2 (2026-09-06)

Exact done/partial list. "Done" means: implemented, tests executed and green on
this machine, committed.

## DONE

1. **Workspace scaffold** — cargo workspace `recall-core` / `recall-server` /
   `recall-cli` + `web/`; dependency versions verified against crates.io/npm on
   2026-09-06 (rmcp 3.2.0, axum 0.8.9, rusqlite 0.40.2, tokio 1.53.1, clap 4.6.6,
   React 19.2.8, Vite 8.2.2, Tailwind 4.3.3, Playwright 1.63.0). New in v0.2:
   chacha20poly1305 0.11 / hmac 0.13 / sha2 0.11 / hex 0.4 / base64 0.22,
   criterion 0.8, fastembed 6.0.2 (optional).
2. **recall-core** — models, `Store` trait, SQLite store (bundled rusqlite, WAL,
   external-content FTS5 trigger-synced, BLOB embeddings), `HashEmbedder`,
   `HybridScorer` with per-component breakdowns, export/import v1. New in v0.2:
   encryption at rest (D-014), `update_memory` (D-015), recall tag filter
   (D-015), schema-v2 migration from v0.1 databases.
3. **ONNX embedder (D-013, spike PASSED → adopted)** — `OnnxEmbedder` behind the
   non-default `onnx` feature (fastembed 6.0.2 / ort 2.0.0-rc.13,
   `ort-load-dynamic` + HF model download over rustls). windows-gnu works via
   the official `onnxruntime-win-x64-1.28.0.dll` loaded at runtime
   (`ORT_DYLIB_PATH`); ort has no windows-gnu prebuilt binaries (attempt 1 of
   the spike failed exactly there — evidence in `spikes/onnx-spike` and D-013).
   Golden-set hit-rate@5 with all-MiniLM-L6-v2-Q: **1.000 (50/50)** vs
   HashEmbedder's 0.980. Model download (~23 MB, once) documented in README.
   Not yet wired into the CLI/server selection (default stays HashEmbedder);
   usable today via the feature-gated eval/tests.
4. **Encryption at rest (D-014)** — application-level AEAD of `memories.text`
   (ChaCha20-Poly1305, per-record nonce, id-bound AAD) + FTS index over keyed
   HMAC-SHA256 token digests, so keyword search works with zero plaintext on
   disk. Key from `RECALL_MCP_KEY` / `RECALL_MCP_KEY_FILE` (auto-enables when
   present); wrong key fails at open via a stored key-check blob; encrypted DBs
   refuse unkeyed opens; v0.1 plaintext databases upgrade in place on first
   keyed open. Tests read the raw DB file bytes and assert the secret and its
   tokens are absent (unit + real-binary CLI e2e, WAL included).
5. **Memory update + tag filter (D-015)** — `Store::update_memory` (in-place
   partial update; text changes re-embed at the server/CLI layer and clear
   stale vectors at the core layer; FTS kept exact by a guarded UPDATE
   trigger); MCP tool `update_memory` (5 tools), REST `PATCH
   /v1/memories/{id}`, CLI `update`; `recall` gained an ALL-of tag filter
   enforced in both the FTS and vector passes (REST body field, MCP tool arg,
   CLI `--tags`).
6. **recall-server** — REST per ARCHITECTURE.md (+ PATCH), tracing + CORS,
   API-key middleware, MCP over stdio and streamable-HTTP (rmcp 3.2.0, spec
   2026-07-28, stateless JSON mode). Conformance smoke tests pass on both
   transports; OpenAPI hand-written with drift-guard tests (D-006).
7. **recall-cli** — `serve` (HTTP+MCP+UI, or `--stdio`), `remember`, `recall`,
   `update`, `export`, `import`; global `--db`; JSON output;
   `RECALL_MCP_API_KEY` / `RECALL_MCP_KEY` / `RECALL_MCP_KEY_FILE` env
   handling (malformed key fails closed, never silently plaintext). CLI e2e
   (real binary) green, incl. encrypted-at-rest flow; stdio conformance green.
8. **Golden-set eval** — `evals/corpus.json` (40) + `evals/golden.json`
   (exactly 50 queries); gate hit-rate@5 ≥ 0.85. **HashEmbedder: 0.980
   (49/50) PASS; OnnxEmbedder: 1.000 (50/50) PASS.**
9. **Benchmark harness (new in v0.2)** — criterion bench
   (`cargo bench -p recall-core`, 10k-memory fixture, hybrid + keyword-only)
   and an exact percentile harness (`tests/p95_latency.rs`, ignored, release).
   **Actual measured result: p95 recall = 67.2 ms at 10k memories — the PRD's
   25 ms budget is NOT met at that size** (first time measured; analysis and
   remedies in docs/EVALUATION.md). The benchmark now makes regressions visible
   run-over-run.
10. **CI (new in v0.2)** — `.github/workflows/ci.yml`: lint job (fmt + clippy
    `-D warnings`), linux-gnu job (workspace tests + llvm-cov 90/90 gate with
    lcov artifact — the D-010 mechanism), windows-gnu job (workspace tests on
    MSYS2 MINGW64, D-005 toolchain). Badge added to README. **Committed but
    never executed: the repo has no `origin` remote yet** — the first push is
    expected to surface CI config friction (action versions, msys2 PATH), which
    is normal workflow-debugging, not a code risk.
11. **Hygiene** — `cargo clippy --workspace --all-targets -- -D warnings`:
    0 warnings; `cargo fmt --all` clean; conventional-commit history, one
    commit per green slice.
12. **Docs** — README quickstart (verified commands only, encryption + ONNX
    sections), docs/EVALUATION.md (measured results incl. the failed p95 gate),
    this file, docs/adr/DECISIONS.md (D-001…D-015).

## PARTIAL

1. **Coverage gate (90/90 on recall-core)** — enforced by CI (see #10), not
   measurable on this windows-gnu VM (D-010). No percentage claimed anywhere;
   until CI runs, coverage is unmeasured, by policy.
2. **OpenAI embedder** — implemented behind the `openai` feature (rustls);
   request/response logic unit tested, connection-error path exercised, but no
   live API call has been made (no key). Not on any default code path.
3. **Web UI after v0.2** — unchanged and untouched; its Playwright/axe green
   results date from v0.1 (same day). The v0.2 API additions (PATCH, tags) are
   backward-compatible and the UI does not use them yet.
4. **p95 remediation** — the measured 67–73 ms p95 at 10k needs the slim-scan /
   FTS-ranking / sqlite-vec work item (EVALUATION.md). At smaller namespace
   sizes recall is single-digit milliseconds.

## NOT DONE

- OAuth/multi-user auth (PRD non-goal), TLS, cloud sync, entity graphs
- Auto-capture/consolidation flow (COMPETITORS.md rec #2/#4 — update path and
  temporal validity are the foundations, now in place)
- Generated TS client from OpenAPI (hand-typed `web/src/api.ts`; D-006)
- Encryption of namespace names/tags/embeddings and of export files (scoped
  out of D-014, documented there)
- ONNX embedder selection flag on CLI/server (`--embedder onnx`), intra-op
  thread tuning
