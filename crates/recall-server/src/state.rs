//! Shared application state.

use std::sync::Arc;

use recall_core::SharedStore;

use crate::metrics::Metrics;
use crate::ratelimit::RateLimiter;

#[derive(Clone)]
pub struct AppState {
    pub store: SharedStore,
    pub embedder: Arc<dyn recall_core::Embedder>,
    pub api_key: Option<String>,
    /// Per-key token-bucket limiter; `None` disables limiting entirely.
    pub rate_limiter: Option<Arc<RateLimiter>>,
    /// Registry rendered at `/metrics`.
    pub metrics: Arc<Metrics>,
    /// Explicitly allowed cross-origin browser origins (AR-001 opt-in,
    /// `--allow-origin`). Empty = same-origin only.
    pub allowed_origins: Vec<String>,
    /// Extra Host header values the guard accepts besides loopback hosts
    /// (AR-001 opt-in, `--allow-host`).
    pub allowed_hosts: Vec<String>,
}

impl AppState {
    pub fn new(
        store: SharedStore,
        embedder: Arc<dyn recall_core::Embedder>,
        api_key: Option<String>,
    ) -> Self {
        Self {
            store,
            embedder,
            api_key,
            rate_limiter: None,
            metrics: Arc::new(Metrics::new()),
            allowed_origins: Vec::new(),
            allowed_hosts: Vec::new(),
        }
    }

    /// Run `f` with exclusive access to the store, recovering from a poisoned
    /// mutex (a panicked handler must not permanently brick the server).
    pub fn with_store<R>(&self, f: impl FnOnce(&dyn recall_core::Store) -> R) -> R {
        let mut guard = self
            .store
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        f(&mut *guard)
    }

    /// Async bridge for store work: the sync SQLite call runs on the blocking
    /// pool, and no `MutexGuard` is held across an `.await`.
    pub async fn with_store_async<R>(
        &self,
        f: impl FnOnce(&dyn recall_core::Store) -> recall_core::Result<R> + Send + 'static,
    ) -> recall_core::Result<R>
    where
        R: Send + 'static,
    {
        let store = self.store.clone();
        tokio::task::spawn_blocking(move || {
            let mut guard = store
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            f(&mut *guard)
        })
        .await
        .map_err(|e| recall_core::RecallError::Internal(format!("store task failed: {e}")))?
    }

    /// Async bridge for embedding: model inference (ONNX, HTTP) is blocking
    /// and may take hundreds of milliseconds, so it must never run on an
    /// async worker thread.
    pub async fn embed_async(&self, text: String) -> recall_core::Result<Vec<f32>> {
        let embedder = self.embedder.clone();
        tokio::task::spawn_blocking(move || embedder.embed(&text))
            .await
            .map_err(|e| recall_core::RecallError::Embedder(format!("embedding task failed: {e}")))
    }
}
