## Summary

What changed and why.

## Scope

- [ ] Core (`recall-core`)
- [ ] Server (`recall-server`)
- [ ] CLI (`recall-cli`)
- [ ] Web
- [ ] Evals
- [ ] CI, release, or docs

## Breaking changes and migration

List any response-shape, CLI, or config change, with the migration step. Write
"none" when there is none.

## Verification

Paste the exact commands you ran and their results. Do not mark a gate as passed
if it did not run.

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
- [ ] `cargo test --workspace --locked`
- [ ] `cargo test --workspace --doc --locked`
- [ ] `cargo llvm-cov -p recall-mcp-core --fail-under-lines 90 --fail-under-regions 90`
- [ ] `cargo audit` and `cargo deny check`
- [ ] windows-gnu tests
- [ ] MSRV checks
- [ ] `cd web && npm run build && npx playwright test`
- [ ] ONNX eval (if the `onnx` feature changed)

## Known gaps

Anything not run, blocked, or deferred. Say plainly what is missing.

## Notes for the reviewer

Risky areas, design decisions, and links to the relevant ADR.
