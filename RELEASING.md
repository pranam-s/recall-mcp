# Releasing Recall-MCP

Releases are owner-gated. The implementation branch prepares everything; the
owner runs the publish steps and decides when a tag becomes public.

## One-time setup

- A crates.io token with publish rights for `recall-mcp-core`,
  `recall-mcp-server`, and `recall-mcp-cli`, stored as the `CARGO_REGISTRY_TOKEN`
  repository secret.
- A GitHub environment named `release` for the publish job.
- The GitHub Actions billing blocker cleared, or a self-hosted runner.

Actions are currently disabled on this repository for billing reasons. Do not
claim CI passed until a run actually exists.

## Release checklist

1. Confirm the version is `X.Y.Z` everywhere:
   - `[workspace.package] version`
   - path-dependency `version` fields
   - `crates/recall-server/src/openapi.json` `info.version`
   - the MCP `version` literal in `crates/recall-server/src/mcp.rs`
   - `CITATION.cff` (`version` and `date-released`)
   - `server.json`
   - README status line, `docs/STATUS.md` heading, CHANGELOG
   - `releases/NOTES-X.Y.Z.md`
2. Run the gates from a clean worktree (MSVC wrapper for the primary run):

   ```powershell
   .\scripts\with-msvc.ps1 cargo fmt --all -- --check
   .\scripts\with-msvc.ps1 cargo clippy --workspace --all-targets --all-features --locked '--' -D warnings
   .\scripts\with-msvc.ps1 cargo test --workspace --locked
   .\scripts\with-msvc.ps1 cargo test --workspace --doc --locked
   .\scripts\with-msvc.ps1 cargo llvm-cov -p recall-mcp-core --fail-under-lines 90 --fail-under-regions 90 --summary-only --locked
   cargo audit
   cargo deny check
   cargo +stable-x86_64-pc-windows-gnu test --workspace --target x86_64-pc-windows-gnu --locked
   cd web; npm ci; npm audit --audit-level=high; npm run build; npx playwright test
   ```

3. Record the measured numbers in `docs/EVALUATION.md`: tests, coverage, p95 on
   the 10k fixture (including failures), golden and semantic evals, web e2e and
   axe results. Run the p95 harness as an ignored release test; it is a manual
   gate and has failed on the development host.
4. Merge the release commit to `master` after review.
5. Tag: `git tag -a vX.Y.Z -m "Recall-MCP X.Y.Z"` and push the tag. The release
   workflow verifies the version pins and runs the gates again.
6. Publish in order: `recall-mcp-core`, wait for the index, then
   `recall-mcp-server`, wait, then `recall-mcp-cli`. The workflow does this with
   `CARGO_REGISTRY_TOKEN`.
7. Confirm the GitHub release, binary archives, provenance attestations, and
   SBOM are attached.
8. Build and push the Docker image to the tag named in `server.json`
   (`ghcr.io/pranam-s/recall-mcp:X.Y.Z`) with the
   `io.modelcontextprotocol.server.name` label from the Dockerfile. The release
   workflow does not push images; this is an explicit owner step until a
   registry login is added.
9. Smoke-test the published crates in a clean directory:

   ```sh
   cargo install recall-mcp-cli --version X.Y.Z
   recall --version
   recall remember --db /tmp/smoke.db --namespace smoke --text "release smoke"
   recall recall --db /tmp/smoke.db --namespace smoke --query release
   ```

## Rollback

Crates.io versions cannot be overwritten. If a published crate is broken:

1. yank the affected version (`cargo yank --version X.Y.Z <crate>`),
2. publish a patch version with the fix,
3. note the yank and the replacement in CHANGELOG and the release notes.

Never delete a tag that has a published crate behind it; document the problem
and ship a new tag.
