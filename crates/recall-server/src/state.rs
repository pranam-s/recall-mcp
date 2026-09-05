//! Shared application state.

use std::sync::Arc;

use recall_core::SharedStore;

#[derive(Clone)]
pub struct AppState {
    pub store: SharedStore,
    pub embedder: Arc<dyn recall_core::Embedder>,
    pub api_key: Option<String>,
}

impl AppState {
    pub fn new(store: SharedStore, embedder: Arc<dyn recall_core::Embedder>, api_key: Option<String>) -> Self {
        Self { store, embedder, api_key }
    }

    /// Run `f` with exclusive access to the store, recovering from a poisoned
    /// mutex (a panicked handler must not permanently brick the server).
    pub fn with_store<R>(&self, f: impl FnOnce(&dyn recall_core::Store) -> R) -> R {
        let mut guard = self.store.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        f(&mut *guard)
    }
}
