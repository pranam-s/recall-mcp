# Contributing to Recall-MCP

Recall-MCP is an MCP memory server with one engine behind MCP, REST, and the CLI.
This file covers the build, the gates, and the conventions the codebase follows.

## Setup

The Rust workspace has an MSRV of 1.89; all features need 1.92. The edition is
2024. Node 24+ is only needed for `web/`.

On Windows, `scripts/with-msvc.ps1` loads the Visual Studio Build Tools
environment (`vcvars64.bat`) for one command without changing the shell. GNU is a
supported secondary toolchain. Optional ONNX work needs `ORT_DYLIB_PATH` pointing
at onnxruntime 1.28.0; the model downloads on first use.

The package names are `recall-mcp-core`, `recall-mcp-server`, and
`recall-mcp-cli`. The library targets keep the short imports `recall_core` and
`recall_server`, and the binaries are `recall` and `recall-cli`.

## Build and test

```powershell
.\scripts\with-msvc.ps1 cargo build --workspace
.\scripts\with-msvc.ps1 cargo test --workspace --locked
cargo +stable-x86_64-pc-windows-gnu test --workspace --target x86_64-pc-windows-gnu --locked
```

The suite has unit, integration, real-binary CLI, MCP conformance on both
transports, golden and semantic evals, property tests, and web e2e. Three tests
are ignored manual diagnostics: the FTS query-plan probe, the p95 harness, and
the cost breakdown. The ONNX semantic gate is behind `--features onnx`.

## Local gates

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

Quality gates:

- recall-core coverage: lines and regions at or above 90. Branch coverage is
  unmeasured on stable cargo-llvm-cov (D-010 amendment).
- Golden set: hit-rate@5 at or above 0.85 on 100 queries; MRR@10 is reported and
  gated at 0.80. The semantic set is gated on ONNX.
- p95: the ignored 10k harness is a manual test with a 25 ms design target. The
  command and method are in `docs/EVALUATION.md`.
- Web: `cd web && npm ci && npm audit --audit-level=high && npm run build &&
  npx playwright test`. axe WCAG 2.1 AA must stay at 0 violations.

These local commands are the gates. Run them before you commit.

## Conventions

- Conventional commits, one green change per commit, tests in the same commit.
- `recall-core` is a pure domain crate: no HTTP server, MCP, or async runtime
  types. JSON wire DTOs and the opt-in blocking embedder client are allowed.
  Server and CLI are thin shells over the same engine.
- No `unwrap` or `expect` outside tests. A justified opt-out is a local `#[allow]`
  with the invariant in a comment.
- Wire changes update the OpenAPI document, conformance tests, and docs in the
  same pull request. Response-shape changes need a migration note.
- Decisions live in `docs/adr/DECISIONS.md`. Propose a dated amendment instead of
  silently deviating.
- Do not paste command output into docs unless you ran the command at that
  commit. Do not paste usernames, drive letters, email addresses, hostnames, or
  API keys. Use placeholders.

## Where to look

| Question | File |
|---|---|
| How does it work? | `docs/ARCHITECTURE.md` |
| Why was it decided? | `docs/adr/DECISIONS.md` |
| What was measured? | `docs/EVALUATION.md`, `evals/README.md` |
| How do clients connect? | `docs/clients.md` |
| How do I configure or use it? | `README.md` |
| How do I release? | `RELEASING.md` |
| What is deferred? | `ideas.md` |

## Scope

Bug fixes, hardening, docs, tests, toolchain and dependency maintenance are in
scope. Larger product ideas (ANN retrieval beyond roughly 100k memories per
namespace, encrypted embeddings, multi-user auth) are deferred; see `ideas.md`.

## Reporting issues

Include the exact command, expected and actual behavior, and the state of
`cargo test` and `cargo clippy`. For MCP questions, name the transport and the
negotiated protocol version. Security issues go through the private advisory flow
in `SECURITY.md`, not a public issue.

## License

By contributing you agree that your contributions are licensed under the MIT
License covering the repository.
