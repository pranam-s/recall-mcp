# Style guide: toolchain conventions (Cargo, rustfmt, clippy, performance)

Sources (checked 2026-09-06):
- Cargo Book (official): <https://doc.rust-lang.org/cargo/> (workspaces,
  features, registry and semver conventions)
- rustfmt: <https://github.com/rust-lang/rustfmt> (default style; the tool is
  authoritative, so do not fight it with a heavily customized `rustfmt.toml`)
- Clippy: <https://doc.rust-lang.org/clippy/> (lint groups and `#[allow]`
  etiquette)
- The Rust Performance Book: <https://nnethercote.github.io/perf-book/>
  (profiling-first methodology; relevant to the 25 ms p95 recall target)
- Feature naming follows Rust API Guidelines C-FEATURE; metadata follows
  C-METADATA

## Cargo conventions

- Shared dependencies are pinned once in the workspace `Cargo.toml` with
  `[workspace.dependencies]`, and members use `workspace = true` (a cargo
  1.64+ feature). This keeps the three crates in lockstep.
- Pre-1.0 semver treats a minor bump as breaking: bump `0.x` when a public type
  changes, and document it in the release notes (C-RELNOTES). The Cargo Book's
  "Semver Compatibility" chapter is the reference.
- Features are additive and named with noun phrases (C-FEATURE): `openai` and
  `onnx` exist, and new features follow the same rule. The default feature set
  is empty, so the default build stays offline and dependency-light (D-003).
- Every published crate carries `description`, `license`, `repository`,
  `readme`, `keywords`, and `categories` (C-METADATA).
- `rusqlite` uses `features = ["bundled"]`, so windows-gnu (D-005) needs no
  system SQLite and gcc is the only external prerequisite. MSVC builds use the
  same bundled path through the project wrapper.

## rustfmt

Use the default style and run `cargo fmt --all` in CI and pre-commit. Do not
add a `rustfmt.toml` without a concrete need; the rustfmt project treats the
default style as the ecosystem norm, and stable-channel options are frozen.

## Clippy

- Run `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`.
  The bar is zero warnings.
- Enable a curated set of `pedantic` lints per crate with justification
  comments, such as `clippy::unwrap_used`, `clippy::expect_used`,
  `clippy::dbg_macro`, and `clippy::todo`. A blanket
  `#![warn(clippy::pedantic)]` adds noise. Every `#[allow(...)]` needs a
  one-line reason.
- Never silence a lint by deleting the check it pointed at.

## Performance methodology (Performance Book)

- Measure before optimizing. The book's workflow is profile, identify, fix,
  then re-measure. The target is 25 ms p95 at 10k memories; build a criterion
  benchmark on `HybridScorer` and the recall scan path before changing code.
- The book's low-risk wins: avoid unnecessary allocation and clones in the hot
  path, prefer iterators over collect-then-scan, and avoid `unsafe` used only
  to skip bounds checks (this project does not need it). Build the shipped
  binary in release mode with `codegen-units=1`.

## Testing conventions (Cargo Book and ecosystem norm)

- Unit tests live inline (`#[cfg(test)] mod tests`) in `recall-core`.
  Integration tests live per crate in `tests/`. Public behavior gets a test
  even when a doc test would be nicer.
- The gate commands are `cargo test --workspace --locked`,
  `cargo fmt --all -- --check`, and
  `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`.
- `cargo llvm-cov` cannot run on windows-gnu because the profiler runtime is
  missing (D-010). It runs on Windows MSVC and is enforced on the Linux CI
  coverage job. The gate is 90% lines and 90% regions on `recall-core`
  (cargo-llvm-cov 0.9.1 has no branch fail-under; see the D-010 amendment).
