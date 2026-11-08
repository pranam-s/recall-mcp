# Style guide: Rust API Guidelines (primary)

Source: <https://rust-lang.github.io/api-guidelines/checklist.html>, maintained
by the Rust project. The checklist was checked on 2026-09-06.

## What the checklist covers

A C-*-prefixed checklist of conventions for public Rust APIs across nine
areas: naming, interoperability, macros, documentation, predictability,
flexibility, type safety, dependability, and future-proofing.

## Items adopted

- **C-CASE**: "casing conforms to RFC 430 conventions" (types `UpperCamelCase`,
  methods/fields `snake_case`, consts `SCREAMING_SNAKE_CASE`).
- **C-CONV**: "ad-hoc conversions follow `as_`, `to_`, `into_` conventions"
  (a cheap borrow uses `as_`, a mostly free conversion uses `to_`, and a
  consuming conversion uses `into_`).
- **C-GETTER**: getter names match the field (`created_at()`, not `get_created_at`).
- **C-ITER / C-ITER-TY**: "methods named `iter`, `iter_mut`, `into_iter`";
  iterator types named after the producing method (`Memories`, `MemoriesMut`).
- **C-FEATURE**: feature names: noun phrases, no placeholder words
  (`openai`, not `use-openai-support-thing`).
- **C-COMMON-TRAITS**: eagerly derive `Debug, Clone, Copy, PartialEq, Eq,
  PartialOrd, Ord, Hash, Default` where they make sense.
- **C-CONV-TRAITS**: implement `From`, `AsRef`, `AsMut` rather than
  bespoke conversion methods.
- **C-SERDE**: all DTOs (`Memory`, `ScoreBreakdown`, export/import format)
  derive serde with `#[serde(rename_all = "snake_case")]` so the JSON API, MCP
  tool args, and export format stay byte-compatible.
- **C-SEND-SYNC**: "types are `Send` and `Sync` where possible". `SqliteStore`
  is an intentional exception (D-007: `Send`-only, shared via
  `Arc<Mutex<dyn Store>>`); document the deviation in the trait docs.
- **C-GOOD-ERR**: "error types are meaningful and well-behaved" (library
  errors are concrete `enum`s implementing `std::error::Error`; see the
  [error-handling guide](./rust-error-handling.md)).
- **C-CTOR**: "constructors are static, inherent `new` methods": no
  builder-only or trait-based construction for `HybridScorer`, stores.
- **C-NEWTYPE / C-CUSTOM-TYPE**: newtypes for `MemoryId`, `NamespaceId`;
  no bare `bool` parameters where a meaningful type reads better.
- **C-BUILDER**: `RecallParams` keeps its builder; complex values get builders.
- **C-VALIDATE**: public functions validate arguments and return typed
  errors (`RecallParams`: `k` between 1 and 100, non-negative weights).
- **C-DEBUG / C-DEBUG-NONEMPTY**: every public type `Debug`-derives with
  non-empty output.
- **C-SEALED**: not adopted for `Embedder` in 0.5.0. The trait is public and
  open; new required methods would be a breaking change. New behavior is added
  as a defaulted method (`fingerprint`, `embedding_invertible`) to keep
  downstream impls working. Sealing remains an option for a future major bump.
- **C-STRUCT-PRIVATE**: public structs expose getters; fields stay private
  unless the type is a plain DTO.
- **C-STABLE**: release crates depend only on stable Rust. MSVC is the primary
  Windows toolchain and GNU is secondary; both use stable (D-005 as amended).
- **C-METADATA / C-RELNOTES / C-CRATE-DOC / C-EXAMPLE / C-QUESTION-MARK**:
  full `Cargo.toml` metadata, crate-level docs, and `?`-style examples where
  they exist. Doc tests are not yet one per item, which is a recorded gap.

## Where it applies

It applies fully to `recall-core`, the public library crate. `recall-server`
and `recall-cli` are applications: adopt the naming and documentation items,
and skip API-surface items that only matter for external consumers. `web/` is
out of scope.
