# Recall-MCP status: 0.5.0 (2026-11-08)

Current state. "Done" means implemented, tested locally, and committed. Numbers
live in [EVALUATION.md](EVALUATION.md); this page says what exists and what does
not.

Release status: **0.5.0 is untagged until the owner tags and publishes it.**
GitHub Actions has not run (billing), so no CI result is claimed.

## Done in 0.5.0

1. **Dependencies and toolchain.** Full `cargo update`; `rusqlite` 0.41;
   rmcp 3.9; fastembed 7.6; tokio 1.54; uuid 1.29. Workspace MSRV 1.89, with
   1.92 for all features. `rust-toolchain.toml`, `deny.toml`, `audit.toml`,
   crate-local licenses, and `scripts/with-msvc.ps1` (vswhere plus a
   well-known-path fallback, vcvars64 import, argument forwarding). GNU remains
   supported as a secondary toolchain.
2. **Encryption forensic upgrade.** Every plaintext-to-keyed transition runs
   with `secure_delete`, VACUUM, and `wal_checkpoint(TRUNCATE)`, with a
   retryable `forensic_clean` marker. Mode marker, key check, and the pending
   marker are one transaction. A forensic test covers a deleted plaintext row
   and a plaintext-era WAL frame.
3. **Rate limiter.** Only a key matching the configured API key gets its own
   bucket; invalid and missing keys share the anonymous bucket. The map is hard
   bounded with an idle sweep and LRU eviction. Rotation and bound tests.
4. **Capture atomicity.** Namespace creation and every chunk insert run in one
   `write_tx` (`BEGIN IMMEDIATE`, 5 s busy timeout). A duplicate-id failure
   rolls back the namespace and all chunks.
5. **Deterministic recall.** The candidate heap and final sort agree on
   total desc, created_at desc, rowid asc. Tag-filtered scans use `ORDER BY
   rowid`; FTS candidates use a rowid tie-break. Ties are stable.
6. **Embedder fingerprint guard.** `Embedder::fingerprint()`; name, dimensions,
   and fingerprint in `store_meta`. Empty stores stamp, legacy stores with
   matching dimensions adopt with a warning, and mismatches name both sides and
   require the explicit override. Wired into serve and one-shot CLI commands.
7. **Cross-process cache safety.** The packed cache stores `PRAGMA
   data_version` and rebuilds when another connection commits. Schema
   `user_version` is 2; newer schemas are refused.
8. **Validation and DoS bounds.** Central limits for namespace, text, query,
   id, tag count and length, source, embedding dimensions and finiteness,
   capture message count and total characters, candidate/scan caps, plus
   saturating limits and recency arithmetic.
9. **API error contract.** Malformed JSON, missing or wrong content type,
   body-limit failures, unknown paths, wrong methods, and bad query/path
   parameters all return `{"error": ...}` with the right status (400/404/405/
   413/415/429). The OpenAPI document lists those responses.
10. **Security headers and origin scheme.** CSP, nosniff, frame denial,
    referrer policy, COOP/CORP, permissions policy, and `no-store` on API/MCP/
    metrics. Origin comparison includes scheme and normalizes default ports.
11. **Async offload.** Store and embedder work runs through `spawn_blocking`;
    no mutex guard crosses an await. A single-threaded slow-embedder test proves
    `/healthz` stays responsive.
12. **MCP transports.** Host/Origin allowlists propagate into rmcp's transport.
    Stateless 2026-07-28 conformance (discover, tools/list, tools/call) passes
    over HTTP and stdio with `_meta` and the required headers. The legacy
    2025-11-25 pin remains. Unknown-id `forget` is invalid-params.
13. **CLI/config hygiene.** BOM-tolerant config, key, and import reads; singular
    and plural `allow_origin(s)` / `allow_host(s)` aliases; scheme, authority,
    and path validation for origins; plaintext-backup warning and owner-only
    permissions; stdout-export warning; OpenAI key redaction, `dimensions` for
    text-embedding-3 models, and a dimension cap.
14. **Compact product surfaces.** `recall-core::views` is the single compact
    layer for MCP, REST, and CLI: compact memory, hit, list page, and capture
    summary, with `include_embedding` restoring full records. MCP
    `list_memories` defaults to 20 and caps at 100. `recall` takes
    `format=json|text`; text keeps ranking and the breakdown. tools/list
    measures 3,370 bytes (budget 4,600).
15. **CLI parity and alias.** `remember`, `recall`, `update`, `forget`, `list`,
    `capture`, `stats`, `keygen`, `export`, `import`, `backup`, `vacuum`,
    `serve`; `RECALL_MCP_DB` and `RECALL_MCP_NAMESPACE`; compact when piped,
    `--pretty`, `--full`, `--show-embedding`, `--format text`. `keygen` writes
    an owner-only key file. The short `recall` binary ships beside
    `recall-cli`.
16. **Client and skill docs.** `docs/clients.md` (expected setups, clearly not
    live-verified), `docs/TOKEN-EFFICIENCY.md`, and
    `skills/recall-mcp/SKILL.md` with CLI and MCP references.
17. **Web.** API-key Connection section (password input, Save/Clear,
    sessionStorage, `X-API-Key`, aria-live), 401/429 guidance with
    `Retry-After`, embedder identity in Stats, pin/unpin and delete with a
    native dialog and focus return, and a keyed-server Playwright test. Eight
    Playwright tests pass with axe AA at 0 violations.
18. **OpenAPI.** Version 0.5.0, bearer and API-key schemes, global security,
    compact/full schemas, paginated list, capture summary, six MCP tools,
    `server/discover`, and 401/404/405/413/415/429 shapes.
19. **Evals.** 100-memory corpus, 100 golden queries (50 lexical, 30
    tag-scoped, 20 temporal) at hit-rate@5 >= 0.85 with MRR@10, plus a
    50-query semantic set gated on ONNX and diagnostic on hash. No external
    benchmark claim.
20. **Release engineering.** Rewritten SHA-pinned CI, release and security
    workflows, Dependabot, `SECURITY.md`, issue/PR templates, `RELEASING.md`,
    `server.json`, Dockerfile, and a 0.5.0 release-notes file. None of the
    workflows has run remotely (billing).
21. **Docs and release metadata.** README is the user guide; STATUS is current
    state; EVALUATION holds measured numbers; design merged into ARCHITECTURE;
    history moved under `docs/history/`; ADRs carry dated amendments; version
    0.5.0 stamped across crates, MCP, OpenAPI, CITATION, server.json, README,
    STATUS, CHANGELOG, and release notes.

## Partial

1. **p95 at 10k memories.** The ignored release harness measured p95 29.6 to
   33.1 ms on this host on 2026-11-08, against the 25 ms PRD budget. It is a
   measured failure, recorded in EVALUATION, and the threshold was not lowered.
   Host-load variance is part of the record. This is the main performance gap
   in 0.5.0.
2. **CI enforcement.** GitHub Actions is disabled for billing. Workflows are
   committed and locally reviewed but have no remote run. Coverage, audit,
   deny, web, and cross-platform jobs were executed locally where this host
   supports them.
3. **OpenAI embedder.** Request construction, dimension validation, error
   paths, and redaction are unit-tested. It has never been called live because
   no key exists in this environment.
4. **Client setup snippets.** The transports are tested; the six client
   configurations in `docs/clients.md` are expected, not live-verified.
5. **Branch coverage.** cargo-llvm-cov 0.9.1 has no branch fail-under on stable;
   line and region coverage are measured (D-010 amendment).
6. **Web UI without an API key.** Open local mode still has no login; the UI
   sends a key when one is saved but cannot protect an unkeyed server.
7. **Dockerfile.** Committed with a multi-stage build and healthcheck, but not
   built here because the Docker daemon was unavailable. Treat it as unverified
   until CI or the owner builds it. No `ghcr.io` image has been pushed, so the
   OCI package in `server.json` is an owner-gated publish step.

## Not done

- OAuth or multi-user auth, TLS termination inside the server, cloud sync,
  entity graphs, consolidation flows, or a Team Server. These remain out of
  scope.
- A generated TypeScript client from OpenAPI; `web/src/api.ts` is hand-typed
  (D-006).
- Encrypting embeddings, tags, namespace names, or source labels at rest.
  Documented as D-014/D-021 design limits.
- Any external retrieval benchmark (LoCoMo, LongMemEval, or similar). None is
  claimed in 0.5.0.
