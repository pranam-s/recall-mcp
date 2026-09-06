# STATUS — Recall-MCP v0.3 (2026-09-07)

Exact done/partial list. "Done" means: implemented, tests executed and green on
this machine, committed. This pass took the project from "complete portfolio
project" toward "production-grade service": the recall hot path was re-engined
(2.1×), and the operations surface (rate limiting, body limits, metrics,
graceful shutdown, backup/maintenance, pagination) was built out.

## DONE (v0.3 additions; v0.1/v0.2 history in git)

1. **Recall fast path (D-016)** — exact top-k candidate heap + slim vector
   scan: only `(rowid, embedding, created_at, pinned)` is scanned, cosine is
   computed from borrowed BLOB bytes with the query norm hoisted
   (bit-identical to the reference), full rows are fetched only for survivors
   (≤ `candidate_cap + k`), and the `last_accessed_at` refresh is one batched
   UPDATE. Candidate selection is provably exact (outside the keyword set bm25
   is 0, so the vector-only total IS the final total). `cache_size`/`mmap_size`
   pragmas for large stores. **Measured: criterion hybrid 80.5 → 28.3–32.2 ms;
   keyword-only 41.7 → 9.9 ms; p95 69.4 → ~33–48 ms across 7 runs (median
   ≈ 34.8).** Cost-breakdown diagnostic added to the release harness.
2. **Production hardening (D-017)** — constant-time API-key compare (subtle);
   request body limit (1 MiB default, 413 on exceed, flag/env/config);
   per-key token-bucket rate limiting on `/v1/*` + `/mcp` with 429 +
   Retry-After (off by default, `--rate-limit`/`--rate-burst`, validated
   fail-closed); JSON `--config` file with flag > env > file > default
   precedence (secrets deliberately excluded from files); graceful shutdown
   (Ctrl-C/SIGTERM → drain in-flight → WAL checkpoint); release profile
   `lto = "fat"`, `codegen-units = 1`, `strip = "symbols"`.
3. **Scalability surface** — `GET /v1/memories` paginates (`?limit=&offset=`)
   with `X-Total-Count` (new `Store::count_memories`); `GET /metrics` serves
   Prometheus text 0.0.4: request counter, status codes, per-route latency
   histogram, embedding timings (timed embedder wrapper covering REST/MCP/
   capture), rate-limit counter, live store gauges; auth-protected with the
   rest of `/v1/*`.
4. **Maintenance** — `Store::checkpoint_wal/vacuum/vacuum_into` plus CLI
   `backup --out` (hot compacted snapshot; refuses overwrite; keyed stores
   produce keyed backups — proven by e2e) and CLI `vacuum` (in-place compact +
   checkpoint). `recall-cli update` was already present from v0.2.
5. **Auto-capture ingest (D-018)** — `recall_core::capture::chunk_text`
   (deterministic paragraph → sentence → hard-wrap chunking, unicode-safe,
   unit-tested) and `POST /v1/capture` for agent transcripts: role prefixes +
   `role:*` tags, caller tags, `at` timestamps, server-side per-chunk
   embedding, auto-created namespace; e2e-tested including recall integration.
6. **Docs** — ADRs D-016/D-017/D-018; EVALUATION.md rewritten with only
   2026-09-07 measurements; README covers the full operations surface.
7. **Tests** — 88 → 110, all green: rate limiter (unit + middleware), body
   limit, metrics rendering + endpoint, constant-time matching, chunking,
   capture e2e, pagination + X-Total-Count, maintenance/backup/restore e2e,
   heap-exactness vs brute force, BLOB-cosine bit-equality, OpenAPI drift
   guards for every new surface. fmt clean; clippy `-D warnings` clean.
8. **Dependencies** — all direct deps verified current against crates.io on
   2026-09-07 (axum 0.8.9, tokio 1.53.1, rmcp 3.2.0, rusqlite 0.40.2, clap
   4.6.6, criterion 0.8.2, fastembed 6.0.2, tower-http 0.7.1); lockfile
   refreshed; base64 0.22 → 0.23 (the only semver-incompatible holdover;
   Engine API unchanged). New runtime dep: `subtle` 2 (constant-time eq).

Carried over from v0.2 (all still done): workspace with pure core, encryption
at rest (D-014), update-in-place + tag filter (D-015), MCP over stdio and
streamable-HTTP (5 tools), OpenAPI with drift guards, golden-set evals,
ONNX embedder spike (D-013), CI workflow definition.

## PARTIAL

1. **Coverage gate (90/90 on recall-core)** — enforced by the committed CI
   (linux-gnu job), not measurable on this windows-gnu VM (D-010). Until the
   first CI run, coverage is unmeasured, by policy. v0.3 core additions shipped
   with their tests in the same commits.
2. **p95 at 10k memories** — improved 2.1× (69.4 → median ≈ 34.8 ms) but the
   PRD's 25 ms budget is still not met at exactly 10k. The residual is SQLite
   blob-read throughput in the exact O(n) scan (measured in the new cost
   breakdown). Next steps are dated in EVALUATION.md: a `scan_cap`-bounded
   packed embedding cache first, then the D-002 ANN revisit past ~100k.
3. **Web UI** — unchanged in v0.3 (backward-compatible API additions only);
   Playwright + axe re-run green on 2026-09-07 against the v0.3 server. The UI
   does not yet surface pagination, `/metrics`, or capture.
4. **OpenAI embedder** — unchanged: logic unit-tested, never called live
   (no key). Not on any default code path.

## NOT DONE (v0.3)

- Config *file for secrets* — the JSON config intentionally excludes the API
  key and encryption key (no secrets on disk); they remain env/flag-only.
- MCP `capture` tool — REST-first by design (D-018); trivial to add later.
- ONNX embedder selection flag on CLI/server (`--embedder onnx`), intra-op
  thread tuning (model is usable today behind the cargo feature).
- Packed in-process embedding cache (the next p95 step, see above).
- OAuth/multi-user auth (PRD non-goal), TLS, cloud sync, entity graphs,
  consolidation flows, Team Server / monetization (per owner instruction).
- Generated TS client from OpenAPI (hand-typed `web/src/api.ts`; D-006).
- `origin` remote / first CI run — the coverage gate remains unmeasured until
  the workflow executes.
