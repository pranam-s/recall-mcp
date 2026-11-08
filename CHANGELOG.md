# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Changed

- Untagged keyword recalls probe the covering namespace index instead of
  reading each wide memory row, which cuts the FTS candidate pass.

### Fixed

- The web capture form no longer prefills the namespace, which removes a race
  with text the user had already typed.
- MCP `recall` treats `tau_days: null` as "disable recency decay", matching the
  REST behavior.
- `server.json` no longer lists an OCI package for an image that is not
  published.

## [0.5.0] - 2026-11-08

First public release, tagged `v0.5.0`. 0.4.0 was never tagged or published.

### Added

- Compact response views shared by MCP, REST, and CLI, with
  `include_embedding=true` restoring full records. Compact list pages and capture
  summaries. MCP `list_memories` defaults to 20 and caps at 100; `recall` supports
  `format=json|text`.
- A short `recall` binary beside `recall-cli`, plus `forget`, `list`, `capture`,
  `stats`, and `keygen` commands, `RECALL_MCP_DB` and `RECALL_MCP_NAMESPACE`
  fallbacks, and `--pretty`, `--full`, and `--show-embedding`.
- An embedder fingerprint guard in `store_meta` with an explicit
  `--allow-embedder-mismatch` override.
- Security headers (CSP, nosniff, frame denial, referrer policy, COOP/CORP,
  permissions policy) and `no-store` on API, MCP, and metrics responses.
- A JSON error contract for malformed JSON, content type, body limit, unknown
  paths, wrong methods, and bad query or path parameters.
- Stateless MCP 2026-07-28 conformance (`server/discover`, `_meta`, required
  headers) over HTTP and stdio.
- An API-key connection UI, 401 and 429 guidance with `Retry-After`, embedder
  identity in Stats, and pin, unpin, and delete actions with a native dialog.
- A 100-query golden eval with MRR@10, a 50-query ONNX-gated semantic set,
  `evals/README.md`, client docs, a skill, token-budget tests, a Dockerfile,
  release automation, and 0.5.0 release notes.

### Changed

- Rust workspace MSRV 1.89, with 1.92 for all features; added
  `rust-toolchain.toml`, `deny.toml`, `audit.toml`, and `scripts/with-msvc.ps1`.
- Dependencies refreshed (rmcp 3.9, fastembed 7.6, rusqlite 0.41, tokio 1.54,
  uuid 1.29, rustls 0.23.46, webpki roots 1.0.11, yoke 0.8.4).
- Capture is atomic: namespace creation and every chunk insert share one
  `BEGIN IMMEDIATE` transaction.
- The rate limiter buckets only a validated configured key; invalid and missing
  keys share a bounded anonymous bucket with idle sweep and LRU eviction.
- Recall tie-breaks use rowid in both the candidate heap and the final sort.
- The packed vector cache records `PRAGMA data_version` and rebuilds on another
  connection's commit. Schema `user_version` is 2; newer schemas are refused.
- Store and embedder work runs on the blocking pool, so slow model calls no
  longer occupy async workers.

### Performance

- The 10k recall design target is 25 ms p95.

### Fixed

- Every plaintext-to-keyed transition now runs the forensic VACUUM and WAL
  truncate, with a retryable marker, including legacy-schema upgrades.
- The crypto tamper test no longer depends on a random base64 character.
- ONNX model initialization is serialized, removing the test-time download race.
- Legacy schema detection runs before trigger creation.
- `EXPORT_SCAN_LIMIT`, recency arithmetic, and list binds are safe on 32-bit and
  at numeric extremes.
- The OpenAI key is redacted in Debug, text-embedding-3 requests send
  `dimensions`, and dimensions are capped.

### Breaking (pre-1.0)

- MCP and REST responses default to compact records; `include_embedding=true`
  restores the full shape.
- REST `GET /v1/memories` returns a page object instead of a bare array. The
  `X-Total-Count` header remains.
- MCP `recall` defaults to compact JSON hits; `format=text` is new.
- MCP `list_memories` caps at 100 instead of 10,000.

### Limitations

- The OpenAI embedder has never been called against the live API (no key).
- Client snippets are starting points, not verified against every client.
- The crates.io packages are not published yet.
- No Docker image is published.

## [0.4.0] - 2026-09-20

Never tagged or published. The work that led to 0.5.0 included:

- The package names became `recall-mcp-core`, `recall-mcp-server`, and
  `recall-mcp-cli`. The shorter names were already used by unrelated projects
  on crates.io, so the crates use the longer names. The library imports and the
  `recall-cli` binary kept their names, path dependencies gained version
  requirements, and the publish order is core, then server, then cli.
- The coverage gate moved to lines and regions. recall-core measured 95.00% lines
  and 94.03% regions; recall-server measured 96.83% and 96.13%.
- `cargo update` brought rmcp 3.4.0, and fastembed moved from 6 to 7 and was
  verified against onnxruntime 1.28.0.
- A license audit of 395 locked packages found them permissive and MIT-compatible.
  `cargo audit` over the 396-package lock found no vulnerabilities and one allowed
  unmaintained `paste` 1.0.15 warning (RUSTSEC-2024-0436, through rmcp's macro
  stack; no fix exists upstream and it is not a vulnerability).
- The CLI stopped printing embedding vectors by default. `--show-embedding`
  restores them; score breakdowns and the REST and MCP surfaces are unchanged.
- The README was rebuilt for public release, and `CONTRIBUTING.md` and `ideas.md`
  were added.

## [0.3.1] - 2026-09-18

- Adversarial hardening: a same-origin CORS default with Host validation, HKDF key
  separation with a store migration, OpenAI dimension validation, MCP and REST
  page-limit parity, atomic import, and rate limiting before auth. Mutation and
  property tests were added.
- The web UI gained server-driven pagination, a metrics view, and a capture form.
  Five Playwright tests covered keyboard-only paging and axe AA scans.
- Publication readiness: reqwest 0.13, the negotiated MCP protocol version pinned
  on both transports, a keyed unicode round-trip test, `CITATION.cff`, and 160
  tests.
- A 0.3-era p95 re-baseline covered the read-only recall path and added a warm-up
  before the timed region.
- The repository moved to GitHub at `pranam-s/recall-mcp`, and the README and
  CITATION URLs were updated.

## [0.3.0] - 2026-09-07

- Production hardening: rate limiting, `/metrics`, transcript capture, and hot
  backup.

## [0.2.0] - 2026-09-06

- Status and evaluation work.

## [0.1.0]

- Initial implementation.
