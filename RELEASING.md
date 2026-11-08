# Releasing Recall-MCP

This is the maintainer checklist for a tagged release. The repository has three
Rust crates, a web UI, and release archives.

## Shipped assets

The v0.5.0 GitHub release carries `recall-mcp-windows-x86_64.zip` and
`SHA256SUMS.txt`. The zip contains `recall.exe`, `recall-cli.exe`, `LICENSE`, and
`README.md`. Those assets were built and uploaded by hand; the release workflow
has not produced a release yet.

## Publishing setup

The crates.io packages have not been published. Do not submit `server.json` to
the MCP registry until they are: its cargo package entry points at crates.io and
will not resolve before then. The release workflow's publish job needs:

- a crates.io API token with publish rights for `recall-mcp-core`,
  `recall-mcp-server`, and `recall-mcp-cli`, stored as the
  `CARGO_REGISTRY_TOKEN` repository secret;
- a GitHub environment named `release`.

The workflow is optional maintainer automation. When it is enabled, a `v*` tag
push or a manual run with an existing tag starts it. It verifies the pins, runs
the gates, builds Linux, Windows, and macOS archives, generates an SBOM and
provenance attestations, and creates the GitHub release from the notes file.

## Version pins

Set the version to `X.Y.Z` in:

- `[workspace.package] version` in `Cargo.toml`;
- the path-dependency `version` fields;
- `Cargo.lock` (run `cargo check --locked` after the manifest change);
- the `info.version` field in `crates/recall-server/src/openapi.json`;
- the MCP `version` literal in `crates/recall-server/src/mcp.rs`;
- `CITATION.cff` (`version` and `date-released`);
- `server.json`;
- `web/package.json`;
- the README status line and `CHANGELOG.md`;
- `releases/NOTES-X.Y.Z.md`.

The release workflow checks the Cargo, OpenAPI, MCP, CITATION, `server.json`, and
release-notes pins before it publishes or builds anything. `Cargo.lock`,
`web/package.json`, the README, and `CHANGELOG.md` still need a manual check.

## Gates

Run these from a clean checkout. The MSVC wrapper is the primary Windows run.

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

Record the measured results in `docs/EVALUATION.md`: tests, coverage, the golden
and semantic evals, and the web and axe results. Run the ignored 10k p95 harness,
a manual gate with a 25 ms design target, and record its result there too. The web
build must keep axe WCAG 2.1 AA at 0 violations.

The release workflow runs the Rust gates again on the tag when it is used.

## Tag and GitHub release

1. Merge the release commit to `master`.
2. Create an annotated tag and push it:

   ```sh
   git tag -a vX.Y.Z -m "Recall-MCP X.Y.Z"
   git push origin vX.Y.Z
   ```

3. Build the release assets for the platforms you ship. The v0.5.0 release was
   assembled by hand: the Windows x86_64 zip and `SHA256SUMS.txt` were uploaded
   to the GitHub release, with the body taken from `releases/NOTES-X.Y.Z.md`.
4. Check the release assets. The v0.5.0 release carries
   `recall-mcp-windows-x86_64.zip` with `recall.exe`, `recall-cli.exe`, `LICENSE`,
   and `README.md`, plus `SHA256SUMS.txt`.

The release workflow, when used, verifies the pins, runs the gates, builds Linux,
Windows, and macOS archives, generates an SBOM and provenance attestations, and
creates the GitHub release from `releases/NOTES-X.Y.Z.md`.

The publish job in the workflow handles crates.io in the order below. It needs
the token and environment described above and has not been run yet.

## Crates.io

The manifests use the names `recall-mcp-core`, `recall-mcp-server`, and
`recall-mcp-cli`, and no version is on the registry. If you decide to publish,
use this order and wait for the index between steps:

```sh
cargo publish -p recall-mcp-core --locked
cargo publish -p recall-mcp-server --locked
cargo publish -p recall-mcp-cli --locked
```

The workflow's publish job does the same thing with `CARGO_REGISTRY_TOKEN` and
the `release` environment. After publishing, smoke-test the crates in a clean
directory:

```sh
cargo install recall-mcp-cli --version X.Y.Z
recall --version
recall remember --db smoke.db --namespace smoke --text "release smoke"
recall recall --db smoke.db --namespace smoke --query release
```

## Docker

The `Dockerfile` is in the repository, but no image is published, and `server.json`
does not list an OCI package yet. When an image is published, add the OCI package
to `server.json` with the release tag and the Dockerfile's
`io.modelcontextprotocol.server.name` label.

## Rollback

Crates.io versions cannot be overwritten. If a published crate is broken:

1. yank the affected version with `cargo yank --version X.Y.Z <crate>`;
2. publish a patch version with the fix;
3. note the yank and the replacement in `CHANGELOG.md` and the release notes.

Never delete a tag that has a published crate behind it. Document the problem and
ship a new tag.
