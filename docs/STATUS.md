# STATUS — Recall-MCP v0.3 (2026-09-07, second pass same day)

Exact done/partial list. "Done" means: implemented, tests executed and green on
this machine, committed. The first v0.3 pass re-engined the recall hot path
(2.1×) and built out the operations surface. This evening pass landed the two
remaining v0.3 items from the "NOT DONE" list: the measured p95 remedy (D-019
packed vector cache — criterion hybrid recall −49%) and the `--embedder`
selection flag (completing D-013), plus serve_config glue tests.

## DONE (evening-pass additions; morning v0.3 and v0.1/v0.2 history in git)

1. **Packed in-process vector cache (D-019)** — the first untagged vector pass
   packs the namespace's scan columns (row-major f32 + rowid/created_at/
   pinned) into contiguous memory; repeat recalls stop re-reading ~10 MB of
   embedding BLOBs from SQLite. Any insert/update/delete invalidates the
   cache; tag-filtered recalls keep the SQL path; the packed cosine is
   bit-identical to the reference; `scan_cap` LIMIT semantics are preserved
   row-for-row; mixed-dimension stores degrade to 0.0 exactly like the SQL
   path; `RefCell` soundness argument documented (same single-threaded access
   as D-007). **Measured: criterion hybrid_k5 31.8 → 16.16 ms (−49.1%, same-day
   baseline); exact harness p95 55.9 → 26.2–27.2 ms across 3 runs. The 25 ms
   PRD budget at exactly 10k is still not met on the exact harness — honest.**
   Cost breakdown: the vector pass now adds −4 to +5 ms over the keyword floor
   (was ≈ +23 ms); the residual is the FTS pass, a different problem.
2. **`--embedder onnx|hash|openai` selection** — global CLI flag (env
   `RECALL_MCP_EMBEDDER`, config-file `embedder`; flag > env > file > default)
   driving `serve` (HTTP + stdio), `remember`, `recall`, and `update`.
   Selection lives in core (`parse_embedder_name` + `select_embedder`):
   unknown names are typed errors listing accepted values; `onnx`/`openai`
   fail closed naming their cargo feature in builds without it; OpenAI
   configures from `RECALL_MCP_OPENAI_*` env. `ServerConfig` carries the
   resolved embedder so REST/MCP/capture embed identically (proven by a
   router test with a fixed-vector embedder). recall-cli forwards the
   `onnx`/`openai` features.
3. **serve_config glue tests** — file+flags land in `ServerConfig` (bind,
   rate_limit/rate_burst with per-knob precedence and the max(2×rate, 10)
   burst default, body_limit, embedder, api_key); fail-closed validation
   (negative rate, unknown file keys, burst < rate, unknown embedder,
   zero body limit clamps to 1); onnx-without-feature refused.
4. **Tests** — 110 → 126, all green: cache reuse/rebuild policy, write
   invalidation (insert/update/pin/delete/namespace switch), scan-cap parity,
   packed-slice cosine bit-equality, embedder parsing/selection, router
   embedder wiring, serve_config glue, real-binary `--embedder` e2e.
   Golden set re-run with the cache: 0.980 (49/50), gate ≥ 0.85 PASS.
   fmt clean; clippy `-D warnings` clean.

## DONE (morning v0.3; unchanged)

1. **Recall fast path (D-016)** — exact top-k candidate heap + slim vector
   scan, bit-identical cosine from borrowed BLOB bytes, full rows fetched only
   for survivors, batched `last_accessed_at` refresh, `cache_size`/`mmap_size`
   pragmas. Measured: hybrid 80.5 → 28.3–32.2 ms (criterion).
2. **Production hardening (D-017)** — constant-time API-key compare; body
   limit (1 MiB default, 413); per-key token-bucket rate limiting (429 +
   Retry-After, off by default); JSON `--config` file; graceful shutdown with
   WAL checkpoint; release profile LTO.
3. **Scalability surface** — pagination with `X-Total-Count`; `/metrics`
   Prometheus endpoint (counters, latency histogram, embedding timings, rate
   limit, store gauges).
4. **Maintenance** — `Store::checkpoint_wal/vacuum/vacuum_into`, CLI `backup`
   (keyed stores produce keyed backups) and CLI `vacuum`.
5. **Auto-capture ingest (D-018)** — deterministic chunking + `POST /v1/capture`.
6. **Docs** — ADRs D-016/D-017/D-018 (+ D-019 and the D-013 amendment this
   evening); EVALUATION.md with only measured numbers; README.
7. **Dependencies** — all direct deps verified current against crates.io on
   2026-09-07; lockfile refreshed again in the evening pass (fastembed
   6.0.2 → 6.0.3, tokenizers, rustls, hashlink — all behind the optional
   `onnx` feature; runtime deps unchanged). The onnx build compile-checks
   clean with 6.0.3; the onnx runtime tests were last executed with 6.0.2
   (they need `ORT_DYLIB_PATH` + a model download, and fastembed 6.0.3 is a
   patch release).

## PARTIAL

1. **Coverage gate (90/90 on recall-core)** — enforced by the committed CI
   (linux-gnu job), not measurable on this windows-gnu VM (D-010). Until the
   first CI run, coverage is unmeasured, by policy. D-019 and the embedder
   selection shipped with their tests in the same commits.
2. **p95 at 10k memories** — ≈ 2.6× total from v0.2 on the exact harness
   (69.4 → 26.2–27.2 ms across evening runs; criterion hybrid mean is 5.0×:
   80.5 → 16.16 ms) but the PRD's 25 ms budget is still not met at exactly 10k
   on the exact harness (by 1.2–2.2 ms). The residual is the FTS keyword pass
   + survivor fetch + refresh — no remaining one-line remedy is documented;
   the D-002 ANN/packed-vector revisit beyond ~100k stays the structural fix.
   Next incremental target, if 10k namespaces matter in practice: the FTS
   candidate query plan (unmeasured territory).
3. **Web UI** — unchanged; Playwright + axe re-run against the evening server
   (result recorded in EVALUATION.md). The UI does not yet surface pagination,
   `/metrics`, or capture.
4. **OpenAI embedder** — logic unit-tested, now selectable via
   `--embedder openai` (with the `openai` feature), but never called live
   (no key). Not on any default code path.

## NOT DONE (v0.3)

- Config *file for secrets* — the JSON config intentionally excludes the API
  key and encryption key (no secrets on disk); they remain env/flag-only.
- MCP `capture` tool — REST-first by design (D-018); trivial to add later.
- ONNX intra-op thread tuning (the embedder is fully selectable now; tuning
  is a fastembed/ort knob, not a selection problem).
- OAuth/multi-user auth (PRD non-goal), TLS, cloud sync, entity graphs,
  consolidation flows, Team Server / monetization (per owner instruction).
- Generated TS client from OpenAPI (hand-typed `web/src/api.ts`; D-006).
- `origin` remote / first CI run — the coverage gate remains unmeasured until
  the workflow executes.
