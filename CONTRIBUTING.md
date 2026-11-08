# Contributing to Recall-MCP

Recall-MCP is an MCP memory broker with one engine behind three surfaces (MCP,
REST, CLI), a pure-domain core crate, and a small web UI. This file covers the
build, the gates, and the conventions the codebase follows.

## Setup

- Rust: workspace MSRV 1.89; all features need 1.92. Edition 2024.
- Windows: `scripts/with-msvc.ps1` loads the Visual Studio Build Tools
  environment (`vcvars64.bat`) without polluting the shell. GNU remains a
  supported secondary toolchain.
- Node 24+ only for `web/`.
- No global installs are required. Optional ONNX work needs
  `ORT_DYLIB_PATH` pointing at onnxruntime 1.28.0; the model downloads on first
  use.

Published package names are `recall-mcp-core`, `recall-mcp-server`, and
`recall-mcp-cli`. The lib targets keep the short imports `recall_core` and
`recall_server`. The binaries are `recall` and `recall-cli`.

## Build and test

```powershell
.\scripts\with-msvc.ps1 cargo build --workspace
.\scripts\with-msvc.ps1 cargo test --workspace --locked
cargo +stable-x86_64-pc-windows-gnu test --workspace --target x86_64-pc-windows-gnu --locked
```

The suite has unit, integration, real-binary CLI, MCP conformance on both
transports, golden and semantic evals, property tests, and web e2e. Three
tests are ignored manual diagnostics (FTS query-plan probe, p95, and cost
breakdown). The ONNX semantic gate is behind `--features onnx`.

## Gates before a PR

Primary MSVC run:

```powershell
.\scripts\with-msvc.ps1 cargo fmt --all -- --check
.\scripts\with-msvc.ps1 cargo clippy --workspace --all-targets --all-features --locked '--' -D warnings
.\scripts\with-msvc.ps1 cargo test --workspace --locked
.\scripts\with-msvc.ps1 cargo test --workspace --doc --locked
.\scripts\with-msvc.ps1 cargo llvm-cov -p recall-mcp-core --fail-under-lines 90 --fail-under-regions 90 --summary-only --locked
cargo audit
cargo deny check
```

`'--'` is quoted because Windows PowerShell otherwise swallows the separator
before `-D warnings`. The wrapper forwards arguments verbatim.

Reliability and quality:

- recall-core coverage: lines and regions >= 90. Measured 95.39% / 94.19% on
  2026-11-08. Branch coverage is unmeasured on stable cargo-llvm-cov (D-010
  amendment).
- Golden set: hit-rate@5 >= 0.85 on 100 queries; MRR@10 is reported and gated
  at 0.80. The semantic set is gated on ONNX.
- p95: the ignored 10k harness is a manual gate at 25 ms. It failed on the
  2026-11-08 host (29.6 to 35.3 ms). Do not lower the threshold to make it pass.
- Web: `cd web && npm ci && npm audit --audit-level=high && npm run build &&
  npx playwright test`. axe WCAG 2.1 AA must stay at 0 violations.

## Conventions

- Conventional commits, one green change per commit, tests in the same commit.
- `recall-core` is a pure domain crate: no axum, rmcp, tokio, or HTTP/JSON DTO
  types. Server and CLI are thin shells over the same engine.
- No `unwrap`/`expect` outside tests; a justified opt-out is a local `#[allow]`
  with the invariant in a comment.
- Wire changes update the OpenAPI document, conformance tests, and docs in the
  same PR. Response-shape changes need a migration note.
- Decisions live in `docs/adr/DECISIONS.md`. Propose a dated amendment instead
  of silently deviating.
- Do not paste command output into docs unless you ran the command at that
  commit. Do not paste usernames, drive letters, email addresses, hostnames, or
  API keys. Use placeholders.

## Source-of-truth map

| Question | File |
|---|---|
| How does it work? | `docs/ARCHITECTURE.md` |
| Why was it decided? | `docs/adr/DECISIONS.md` |
| What works today? | `docs/STATUS.md` |
| What was measured? | `docs/EVALUATION.md`, `evals/README.md` |
| How do clients connect? | `docs/clients.md` |
| How do I release? | `RELEASING.md` |
| How do I configure or use it? | `README.md` |

## Scope

Bug fixes, hardening, docs, tests, toolchain and dependency maintenance are in
scope. Larger product ideas (ANN retrieval beyond roughly 100k memories per
namespace, encrypted embeddings, multi-user auth) are deferred; see `ideas.md`.

## Reporting issues

Include the exact command, expected vs actual, and the state of `cargo test`
and `cargo clippy`. For MCP questions, name the transport and negotiated
protocol version. Security issues go through the private advisory flow in
`SECURITY.md`, not a public issue.

## License

By contributing you agree that your contributions are licensed under the MIT
License covering the repository.
