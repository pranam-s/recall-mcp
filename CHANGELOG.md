# Changelog

All notable changes to this project are documented here.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

Initial development phase — the crates (`recall-core`, `recall-server`,
`recall-cli`) remain at 0.1.0 until first publish. Project milestones so far
(details and honest done/partial lists in `docs/STATUS.md`):

- **publication pass** (2026-09-12) — dependency freshness (reqwest 0.12 →
  0.13, feature rename `rustls-tls` → `rustls`; every other direct dependency
  verified at its crates.io latest), `ServerConfig` Debug now redacts the API
  key, negotiated MCP protocol version pinned on both transports with the
  docs corrected to match (rmcp 3.3 answers `2025-11-25` to a `2026-07-28`
  initialize — that revision replaced the handshake with per-request
  metadata), keyed unicode round-trip test, `CITATION.cff`, CI actions bumped
  (checkout v7, upload-artifact v7), 160 tests green.
- **v0.3.1** (2026-09-11) — adversarial-review hardening: same-origin CORS
  default with Host validation, HKDF key separation with store migration,
  OpenAI dims validation, MCP/REST page-limit parity, atomic import,
  rate-limiter-before-auth, mutation + property testing layers (mutation
  score recorded in `docs/EVALUATION.md`), honest doc true-ups.
- **v0.3** (2026-09-07) — production hardening: rate limiting, `/metrics`,
  transcript auto-capture, hot backup (operations surface documented in the
  README).
- **v0.2** (2026-09-06) — measured status/evaluation pass, including an honest
  p95 breach, plus ONNX and encryption sections.
- **v0.1** — initial implementation (see git history).
