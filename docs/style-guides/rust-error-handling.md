# Style guide: Rust error handling

Sources (checked 2026-09-06):
- <https://docs.rs/thiserror> (v2.x: derives `std::error::Error` for library
  enums; v2 supports `core::error::Error` and `no_std`)
- <https://docs.rs/anyhow> (type-erased application errors and `.context()`)
- <https://blog.rust-lang.org/2024/02/08/Rust-1.76.0.html>: stabilized
  `core::error::Error`, the `Error` trait usable in `no_std`; current notes at
  <https://blog.rust-lang.org/category/releases/>
- <https://rust-lang.github.io/api-guidelines/checklist.html>: C-GOOD-ERR

## Rules

1. Libraries return concrete, typed errors. `recall-core` defines
   `#[derive(Debug, Error)] pub enum RecallError { ... }` with `#[error("...")]`
   messages and `#[from]` for wrapping (`#[from] rusqlite::Error` and
   `#[from] serde_json::Error`). Callers must be able to `match` on variants,
   and a library crate never leaks `anyhow::Error`. (API Guidelines C-GOOD-ERR:
   "error types are meaningful and well-behaved".)
2. Applications use type-erased errors with context. `recall-cli` and
   `recall-server` handlers convert core errors with `?` into an app-level
   error. Axum handlers map to typed HTTP statuses; the CLI prints a human
   message and exits non-zero. `anyhow::Context` is allowed in the two app
   crates only, never in `recall-core`.
3. Errors implement the standard trait. Everything bottoms out in
   `std::error::Error`, stabilized in `core` since 1.76, so `?`,
   `Box<dyn Error + Send + Sync>`, and ecosystem tooling work. Do not invent a
   custom `Error` trait.
4. Source chains are preserved. `#[source]` and `#[from]` keep the underlying
   rusqlite or HTTP error inspectable, and the top-level `Display` message
   answers what the program was trying to do.
5. No panics across boundaries. `unwrap()` and `expect()` belong in tests and
   in provably infallible cases with an `expect("reason")` message. Poison
   recovery for the store mutex stays in `AppState::with_store` (D-007).
6. HTTP and JSON mapping is explicit. Each `RecallError` variant maps to one
   status and JSON error shape in `recall-server` (404 unknown namespace, 400
   invalid params, 409 duplicate, 500 with no information leak; see D-008), and
   MCP tool errors map to JSON-RPC invalid-params or internal-error. Keep the
   mapping table in one module.

## Rejected and optional

- `eyre` and `color-eyre` give nicer reports for applications, but two app
  crates do not justify the extra dependency.
- `error-stack` provides typed context stacks, which is more machinery than
  these crates need and has little adoption.
- Snafu is a viable alternative to thiserror, but thiserror is the community
  default and already used here.
