# STATUS — Recall-MCP v0.1 (2026-09-06)

Exact done/partial list. "Done" means: implemented, tests executed and green on
this machine, committed.

## DONE

1. **Workspace scaffold** — cargo workspace `recall-core` / `recall-server` /
   `recall-cli` + `web/`; dependency versions verified against crates.io/npm on
   2026-09-06 (rmcp 3.2.0, axum 0.8.9, rusqlite 0.40.2, tokio 1.53.1, clap 4.6.6,
   React 19.2.8, Vite 8.2.2, Tailwind 4.3.3, Playwright 1.63.0).
2. **recall-core** — models, `Store` trait, SQLite store (bundled rusqlite, WAL,
   external-content FTS5 trigger-synced, BLOB embeddings), `HashEmbedder`
   (deterministic 256-dim feature hashing), `HybridScorer` with per-component
   breakdowns and deterministic tie-breaks, export/import v1 with idempotent
   import. 49 unit tests green.
3. **recall-server** — REST exactly per ARCHITECTURE.md (`POST/GET /v1/namespaces`,
   `POST /v1/memories`, `POST /v1/recall` with tunable weights/τ/pinned_boost/k,
   `DELETE /v1/memories/{id}`, `GET /v1/memories?namespace=`, `GET /v1/stats`,
   `GET /healthz`, `GET /openapi.json`), tracing + CORS, API-key middleware.
   12 integration tests green (incl. full auth matrix).
4. **MCP server (rmcp 3.2.0, spec 2026-07-28)** — tools `remember`, `recall`,
   `forget`, `list_memories` over stdio and streamable-HTTP (stateless JSON
   mode); API-key auth enforced on `/mcp`; tool failures map to JSON-RPC
   invalid-params vs internal errors. Conformance smoke tests (initialize →
   tools/list → tools/call) pass on **both** transports.
5. **recall-cli** — `serve` (HTTP+MCP+UI, or `--stdio`), `remember`, `recall`,
   `export`, `import`; global `--db`; JSON output; `RECALL_MCP_API_KEY` env
   fallback. CLI e2e round-trip (real binary) green; stdio conformance green.
6. **Golden-set eval** — `evals/corpus.json` (40 memories) + `evals/golden.json`
   (exactly 50 queries); test gates hit-rate@5 ≥ 0.85.
   **Actual: 0.980 (49/50). PASS.**
7. **web/** — React 19 + Vite 8 + TS (strict tsc) + Tailwind 4; namespaces
   list/create, search with score-breakdown table, stats; semantic HTML,
   labels, aria-live status, visible `:focus-visible`, fully keyboard-operable.
   `npm run build` green; Playwright e2e (create → recall → breakdown, against
   the production server serving `web/dist`) green; **axe WCAG 2.0/2.1 AA: 0
   violations**.
8. **Hygiene** — `cargo clippy --workspace --all-targets`: 0 warnings; `cargo
   fmt --all` clean; conventional-commit history, one commit per green slice.
9. **Docs** — README quickstart (verified commands only), docs/EVALUATION.md
   (measured results + honest coverage story), this file, docs/adr/DECISIONS.md
   (D-001…D-012).

## PARTIAL

1. **Coverage gate (90/90 on recall-core)** — the gate command is documented and
   the test suite was written branch-first, but `cargo llvm-cov` **cannot run on
   windows-gnu** (no profiler runtime; every workaround attempted and recorded in
   docs/EVALUATION.md and D-010). No percentage is claimed. Action: run
   `cargo llvm-cov -p recall-core --fail-under-lines 90 --fail-under-branches 90`
   on linux-gnu CI.
2. **OpenAI embedder** — implemented behind the `openai` feature (rustls);
   request/response logic unit tested and the connection-error path is exercised,
   but no live API call has been made (no key). Not on any default code path.
3. **Performance validation** — PRD's p95 < 25 ms on 10k memories is unmeasured
   (no benchmark harness in scope). Design caps documented in EVALUATION.md.

## NOT DONE (explicitly out of v0.1)

- sqlite-vec virtual table (deferred by decision D-002; exact scan used instead)
- Memory update/edit path (insert + delete only)
- OAuth/multi-user auth (PRD non-goal), TLS, cloud sync, entity graphs
- Generated TS client from OpenAPI (hand-typed `web/src/api.ts` used instead;
  D-006)
