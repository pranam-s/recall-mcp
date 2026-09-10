# STATUS — Recall-MCP v0.3.1 (2026-09-11, adversarial-review fix pass)

Exact done/partial list. "Done" means: implemented, tests executed and green on
this machine, committed. This pass addressed the 2026-09-11 adversarial review
(docs/ADVERSARIAL-REVIEW.md): the P1 browser-exploitable loopback surface, two
crypto-hygiene gaps, the MCP/REST pagination drift, the OpenAI dims bug, the
owner-mandated mutation/property testing layers, and every actionable P3 plus
the stale-doc findings. The two v0.3 items below that STATUS previously listed
as NOT DONE — the MCP `capture` tool and ONNX intra-op thread tuning (D-020) —
had in fact shipped; they are now correctly listed as DONE.

## DONE (2026-09-11 security & honesty pass)

1. **AR-001 (P1): browser isolation by default** — the Host/Origin guard
   rejects foreign `Host` headers (DNS-rebinding → 421) and cross-origin
   `Origin`s (403); `CorsLayer::permissive()` is gone. CORS is granted only
   for explicit `--allow-origin` opt-ins (repeatable; also in the config
   file); `--allow-host` extends the Host allowlist for non-loopback
   bindings. README no longer describes loopback-without-key as safe.
2. **AR-002: key separation** — HKDF-SHA256 derives distinct AEAD and
   FTS-MAC subkeys (`recall-mcp:v1:aead` / `recall-mcp:v1:fts-mac`);
   pre-HKDF stores are detected at open via the key-check blob and migrated
   in one transaction (plus VACUUM + WAL checkpoint); RFC 5869 vector test
   pins the KDF.
3. **AR-003: embedding-leak honesty** — D-014 and README now state plainly
   that dense embeddings (onnx/openai) are invertible to text
   (vec2text-style attacks) so keyed mode does not protect text with them;
   `serve` logs a warning when keyed mode runs with an invertible embedder
   (`Embedder::embedding_invertible`).
4. **AR-004: MCP `list_memories` clamp** — shared `MAX_PAGE_LIMIT` (10 000)
   via `page_limit()` on REST and MCP; the store's LIMIT/OFFSET binds
   saturate via `i64::try_from` instead of wrapping. End-to-end test with
   10 001 memories proves the cap.
5. **AR-005: OpenAI dims validated** — response vectors must match the
   configured dims exactly (error names both sides); REST create rejects
   caller-supplied embeddings of the wrong length instead of silently
   zeroing all cosines.
6. **AR-006/AR-007: mutation + property testing** — `mutants.toml` (gated on
   crypto/scorer/util modules) with the first run recorded in EVALUATION.md;
   five proptest targets (crypto roundtrip + wrong-AAD, FTS query validity,
   chunking invariants, scorer linearity/bounds, token-bucket invariants).
7. **P3 wave** — AR-011 rmcp 3.3.0 + uuid 1.26.1; AR-012 rate limiter wraps
   auth so key-guessing is throttled; AR-013 keyed upgrades run with
   `secure_delete=ON` and finish with VACUUM + truncated WAL checkpoint
   (forensic canary test); AR-015 scan-cap/candidate-cap approximations
   documented; AR-016 shared reqwest client; AR-017 Web-UI-without-API-key
   limitation documented; AR-018 per-recall `last_accessed_at` writes
   removed (no consumer); AR-019 AGENTS.md tool list corrected; AR-021
   export writes owner-only files and warns about plaintext; AR-022 honest
   namespace-isolation phrasing; AR-008 D-021 memory-poisoning threat model
   + untrusted-data framing in MCP tool descriptions.

## DONE (v0.3, unchanged from the 2026-09-07 passes)

- **Packed in-process vector cache (D-019)** — hybrid_k5 31.8 → 16.16 ms
  (−49.1%); exact harness p95 55.9 → 26.2–27.2 ms. The 25 ms PRD budget at
  exactly 10k is still not met on the exact harness — honest.
- **`--embedder onnx|hash|openai` selection** (completing D-013) and
  **ONNX intra-op thread tuning (D-020)** — both shipped and tested
  (`onnx_embed.rs:63-77`, commit 0c2d07e); STATUS previously mislisted
  tuning as NOT DONE.
- **MCP `capture` tool** — shipped (mcp.rs + tests on both transports);
  STATUS previously mislisted it as NOT DONE.
- **Recall fast path (D-016)**, **production hardening (D-017)**,
  **auto-capture ingest (D-018)**, pagination + `/metrics`, CLI
  `backup`/`vacuum`, maintenance ops.

## PARTIAL

1. **Coverage gate (90/90 on recall-core)** — enforced by the committed CI
   (linux-gnu job), not measurable on this windows-gnu VM (D-010). Until the
   first CI run, coverage is unmeasured, by policy.
2. **p95 at 10k memories** — the PRD's 25 ms budget is still not met at
   exactly 10k on the exact harness (by 1.2–2.2 ms). The residual is the FTS
   keyword pass + survivor fetch; the D-002 ANN revisit beyond ~100k stays
   the structural fix. (Note: recalls are now read-only — see AR-018 — which
   removes a write from the measured path; the p95 harness has not been
   re-baselined after this pass.)
3. **Web UI** — keyboard/screen-reader accessible, axe-tested; does not yet
   surface pagination, `/metrics`, or capture; sends no API key (AR-017).
4. **OpenAI embedder** — logic unit-tested (request/parse/dims), selectable
   via `--embedder openai`, but never called live (no key). Not on any
   default code path.
5. **Embeddings/tags plaintext at rest** — documented decision (D-014, D-021,
   AR-003); encrypting them is a v2 candidate, not done.

## NOT DONE

- Config *file for secrets* — the JSON config intentionally excludes the API
  key and encryption key (no secrets on disk); they remain env/flag-only.
- OAuth/multi-user auth (PRD non-goal), TLS, cloud sync, entity graphs,
  consolidation flows, Team Server / monetization (per owner instruction).
- Generated TS client from OpenAPI (hand-typed `web/src/api.ts`; D-006).
- `origin` remote / first CI run — the coverage gate remains unmeasured until
  the workflow executes.
