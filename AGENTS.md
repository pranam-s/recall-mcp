# AGENTS.md: Recall-MCP

## Overview

Recall-MCP is a local-first semantic memory broker for AI agents. It ships a
Rust MCP server (stdio and streamable HTTP, built on the SDK for the 2026-07-28
spec; the wire `initialize` handshake answers `2025-11-25`, see D-004) with six
tools: `remember`, `recall`, `forget`, `update_memory`, `capture`, and
`list_memories`. The same engine runs behind REST, an OpenAPI document, a web
UI, and a CLI. One SQLite file holds the store.

Retrieval combines BM25, cosine similarity, and recency decay. Every recall
result carries a score breakdown that shows how the rank was produced.

## Docs map

| Document | Contents |
|---|---|
| docs/ARCHITECTURE.md | Layering, data model, recall path, encryption, embedders, surfaces, capture, validation, testing |
| docs/EVALUATION.md | Measured test, coverage, retrieval, token, web, and dependency results |
| docs/adr/DECISIONS.md | Decision records D-001 through D-021 and dated amendments |
| docs/clients.md | MCP client configuration and troubleshooting |
| docs/TOKEN-EFFICIENCY.md | Compact response formats and measured byte sizes |
| evals/README.md | Retrieval fixtures, labelling rules, and results |
| docs/style-guides/ | Rust API, error handling, and tooling conventions |
| CONTRIBUTING.md | Contribution process |
| RELEASING.md | Release process |

Read docs/ARCHITECTURE.md, docs/EVALUATION.md, and docs/adr/DECISIONS.md before
planning work.

## Architecture boundaries (hard rules)

- `crates/recall-core` is a pure domain crate: no HTTP server, MCP, or async
  runtime types. JSON wire DTOs and the opt-in blocking embedder client are
  allowed. Models, validation, compact views, `Store` trait, `Embedder` trait,
  `HybridScorer`.
- `recall-server` (axum + rmcp) and `recall-cli` (clap) are thin shells over
  core. Same engine behind REST, MCP, and CLI; no logic drift between them.
- Published package names are `recall-mcp-core`, `recall-mcp-server`, and
  `recall-mcp-cli`; lib imports stay `recall_core` / `recall_server`. Binaries
  are `recall` and `recall-cli`.
- Decisions live in docs/adr/DECISIONS.md (D-001 through D-021). Do not silently
  deviate: if a decision is wrong, propose a dated amendment first.

## Style guides (load before writing code)

- docs/style-guides/rust-api-guidelines.md (C-* checklist; core is the public API)
- docs/style-guides/rust-error-handling.md (typed errors in core; anyhow only in app crates)
- docs/style-guides/rust-tooling-conventions.md (Cargo/rustfmt/clippy/perf conventions)

## Toolchains

- Primary: Windows MSVC through the project wrapper:
  `.\scripts\with-msvc.ps1 cargo ...`. Quote the separator when passing rustc
  flags: `... '--' -D warnings`.
- Secondary: `cargo +stable-x86_64-pc-windows-gnu ... --target
  x86_64-pc-windows-gnu`.
- MSRV: 1.89 default features, 1.92 all features. `rust-toolchain.toml` pins
  stable with rustfmt, clippy, and llvm-tools-preview.

## Testing policy

- Gate commands: `cargo test --workspace --locked`,
  `cargo fmt --all -- --check`,
  `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`.
  All green before any commit is "done".
- Coverage: 90% lines AND regions on recall-core via
  `cargo llvm-cov -p recall-mcp-core --fail-under-lines 90 --fail-under-regions 90`.
  cargo-llvm-cov 0.9.1 has no branch fail-under; `--branch` needs nightly, so
  true branch coverage is a recorded tooling gap (D-010 amendment). Numbers are
  in docs/EVALUATION.md.
- windows-gnu caveat: llvm-cov cannot run on windows-gnu (D-010); measure on
  Windows MSVC. Never estimate or fabricate a coverage number.
- The 10k harness is an ignored manual gate with a 25 ms p95 design target.
  Never lower it or claim it is met.
- Web: `cd web && npm run build && npx playwright test` (axe AA must stay 0 violations).
- Retrieval quality: evals/ golden set gates hit-rate@5 >= 0.85 (100 queries)
  plus MRR@10; the ONNX semantic set gates hit-rate@5 >= 0.85. No external
  benchmark claim.

## Verify before claiming

"Done" means implemented, tests executed and green locally, committed. Never
claim a pass, benchmark, or coverage number you did not run. State partial or
incomplete work plainly in the docs that cover it; reviews check it. Do not
paste usernames, personal paths, hostnames, or keys into docs or fixtures.

## Commits

Conventional commits (`feat:`, `fix:`, `docs:`, `test:`, `chore:`), one green
slice per commit, tests in the same commit as the change they cover.

## Accessibility (web/)

The web UI targets screen-reader users. Semantic HTML first; every control
keyboard-operable with visible `:focus-visible`; labels bound to inputs;
aria-live for async status; test with @axe-core/playwright, keep WCAG 2.1 AA
at 0 violations. No mouse-only or hover-only interactions. Use native
`<dialog>` with focus return for destructive confirmation.
