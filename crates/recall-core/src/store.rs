//! Persistence abstraction. Sync by design: SQLite calls are sub-millisecond at
//! v1 scale, and a sync trait keeps the core testable without a runtime.
//!
//! Implementations are `Send` (rusqlite's `Connection` is not `Sync`), so
//! cross-thread sharing goes through [`crate::SharedStore`] (`Arc<Mutex<dyn Store>>`).

use crate::error::Result;
use crate::models::{Memory, Namespace, NewMemory, RecallHit, RecallParams, Stats};

pub trait Store: Send {
    /// Create a namespace; errors with [`crate::error::RecallError::DuplicateNamespace`]
    /// when the name is taken.
    fn create_namespace(&self, name: &str) -> Result<Namespace>;
    /// Idempotent variant used by writers/importers.
    fn get_or_create_namespace(&self, name: &str) -> Result<Namespace>;
    fn list_namespaces(&self) -> Result<Vec<Namespace>>;
    fn find_namespace(&self, name: &str) -> Result<Option<Namespace>>;

    /// Insert a memory. `namespace` must exist; `text` must be non-empty after trimming.
    /// `embedding`, when supplied, must match the embedder dimensionality (checked by caller)
    /// and is stored as a BLOB.
    fn insert_memory(&self, memory: &NewMemory) -> Result<Memory>;
    fn get_memory(&self, id: &str) -> Result<Option<Memory>>;
    /// `Ok(true)` when a row was deleted, `Ok(false)` when the id was unknown.
    fn delete_memory(&self, id: &str) -> Result<bool>;
    /// Newest first. `namespace` filters; `limit` must be > 0.
    fn list_memories(&self, namespace: Option<&str>, limit: usize) -> Result<Vec<Memory>>;

    /// Hybrid recall: FTS5 keyword candidates merged with a brute-force cosine scan,
    /// ranked by [`crate::scorer::HybridScorer`], each hit carrying its score breakdown.
    /// `query_embedding` of `None`/empty disables the vector term for this query.
    /// Returned hits have `last_accessed_at` refreshed to `now`.
    fn recall(
        &self,
        namespace: &str,
        query: &str,
        query_embedding: Option<&[f32]>,
        params: &RecallParams,
        now: i64,
    ) -> Result<Vec<RecallHit>>;

    fn stats(&self) -> Result<Stats>;
}
