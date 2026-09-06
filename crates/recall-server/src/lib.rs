//! recall-server — Axum HTTP API and MCP transports around the pure core.

pub mod api;
pub mod auth;
pub mod error;
pub mod mcp;
pub mod metrics;
pub mod openapi;
pub mod ratelimit;
pub mod state;

use std::sync::Arc;

use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::routing::{delete, get, post};
use recall_core::HashEmbedder;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

use crate::metrics::{Metrics, TimedEmbedder};
use crate::state::AppState;

/// Default request-body cap: memory texts are short by design; captures are
/// chunked server-side. 1 MiB leaves generous headroom for bulk imports that
/// bring their own vectors.
pub const DEFAULT_BODY_LIMIT_BYTES: usize = 1024 * 1024;

/// Server settings supplied by the binary (CLI `serve`).
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// When set, all `/v1/*`, `/metrics` and `/mcp` requests must present this
    /// key via `Authorization: Bearer <key>` or `X-API-Key: <key>`. Unset =
    /// open access (local-first default; see README security notes).
    pub api_key: Option<String>,
    /// Directory of the built web UI to serve at `/` (production e2e setup).
    pub web_dir: Option<std::path::PathBuf>,
    /// When set, protected routes get per-key token-bucket rate limiting.
    pub rate_limit: Option<ratelimit::RateLimit>,
    /// Maximum accepted request body size in bytes.
    pub body_limit_bytes: usize,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            api_key: None,
            web_dir: None,
            rate_limit: None,
            body_limit_bytes: DEFAULT_BODY_LIMIT_BYTES,
        }
    }
}

/// Build the complete application router: REST API, MCP streamable-HTTP,
/// metrics, optional static UI, CORS, tracing, body limit, rate limiting.
pub fn build_router(store: recall_core::SharedStore, config: &ServerConfig) -> Router {
    let metrics = Arc::new(Metrics::new());
    // Every embed (REST, MCP, capture) flows through the timed wrapper.
    let embedder: Arc<dyn recall_core::Embedder> = Arc::new(TimedEmbedder {
        inner: Arc::new(HashEmbedder::new_256()),
        metrics: metrics.clone(),
    });
    let mut state = AppState::new(store, embedder, config.api_key.clone());
    state.metrics = metrics;
    state.rate_limiter = config
        .rate_limit
        .map(ratelimit::RateLimiter::new)
        .map(Arc::new);

    // Probes and the OpenAPI document never require auth or rate limiting
    // (monitoring + spec discovery must survive load).
    let open: Router<AppState> = Router::new()
        .route("/healthz", get(api::healthz))
        .route("/openapi.json", get(api::openapi));

    let auth = axum::middleware::from_fn_with_state(state.clone(), auth::require_api_key);
    let limit = axum::middleware::from_fn_with_state(state.clone(), ratelimit::enforce);

    let protected: Router<AppState> = Router::new()
        .route(
            "/v1/namespaces",
            post(api::create_namespace).get(api::list_namespaces),
        )
        .route(
            "/v1/memories",
            post(api::create_memory).get(api::list_memories),
        )
        .route(
            "/v1/memories/{id}",
            delete(api::delete_memory).patch(api::update_memory),
        )
        .route("/v1/recall", post(api::recall))
        .route("/v1/capture", post(api::capture))
        .route("/v1/stats", get(api::stats))
        .route("/metrics", get(api::metrics))
        .layer(limit.clone())
        .layer(auth.clone());

    // The MCP endpoint mounts as its own scoped router so the API-key layer
    // wraps the streamable-HTTP transport cleanly (including its session routes).
    let mcp: Router<AppState> = Router::new()
        .nest_service("/mcp", mcp::streamable_http_service(&state))
        .layer(limit)
        .layer(auth);

    let app = Router::new()
        .merge(open)
        .merge(protected)
        .merge(mcp)
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            metrics::observe,
        ))
        .layer(DefaultBodyLimit::max(config.body_limit_bytes))
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .with_state(state);

    match &config.web_dir {
        Some(dir) => app.fallback_service(
            tower_http::services::ServeDir::new(dir).append_index_html_on_directories(true),
        ),
        None => app,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::Request;
    use tower::ServiceExt;

    fn test_router(api_key: Option<String>) -> Router {
        let dir = tempfile::tempdir().unwrap();
        let store =
            recall_core::shared(recall_core::SqliteStore::open(dir.path().join("t.db")).unwrap());
        std::mem::forget(dir); // outlives the test; OS cleans the temp dir
        build_router(
            store,
            &ServerConfig {
                api_key,
                ..ServerConfig::default()
            },
        )
    }

    #[tokio::test]
    async fn healthz_is_open_even_with_api_key() {
        let app = test_router(Some("k".into()));
        let res = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/healthz")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), axum::http::StatusCode::OK);
    }

    #[tokio::test]
    async fn body_over_the_limit_is_rejected_with_413() {
        let app = test_router(None);
        let big = "x".repeat(DEFAULT_BODY_LIMIT_BYTES + 1);
        let res = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/memories")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(format!(
                        "{{\"namespace\":\"w\",\"text\":\"{big}\"}}"
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), axum::http::StatusCode::PAYLOAD_TOO_LARGE);
    }

    #[tokio::test]
    async fn rate_limiter_rejects_floods_with_429_and_retry_after() {
        let dir = tempfile::tempdir().unwrap();
        let store =
            recall_core::shared(recall_core::SqliteStore::open(dir.path().join("t.db")).unwrap());
        let app = build_router(
            store,
            &ServerConfig {
                rate_limit: Some(ratelimit::RateLimit {
                    rate: 1.0,
                    burst: 2.0,
                }),
                ..ServerConfig::default()
            },
        );
        let request = || {
            Request::builder()
                .method("GET")
                .uri("/v1/stats")
                .header("x-api-key", "bucket-a")
                .body(axum::body::Body::empty())
                .unwrap()
        };
        let first = app.clone().oneshot(request()).await.unwrap();
        assert_eq!(first.status(), axum::http::StatusCode::OK);
        let second = app.clone().oneshot(request()).await.unwrap();
        assert_eq!(second.status(), axum::http::StatusCode::OK);
        let third = app.clone().oneshot(request()).await.unwrap();
        assert_eq!(third.status(), axum::http::StatusCode::TOO_MANY_REQUESTS);
        assert!(
            third
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(0)
                >= 1,
            "Retry-After must carry a positive hint"
        );
        // A different key still has its own bucket.
        let other = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/v1/stats")
                    .header("x-api-key", "bucket-b")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(other.status(), axum::http::StatusCode::OK);
    }

    #[tokio::test]
    async fn metrics_endpoint_reports_seen_requests() {
        let app = test_router(None);
        let stats: axum::response::Response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/v1/stats")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(stats.status(), axum::http::StatusCode::OK);
        let res = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/metrics")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), axum::http::StatusCode::OK);
        let body = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let text = String::from_utf8(body.to_vec()).unwrap();
        assert!(
            text.contains("recall_requests_total 1"),
            "metrics must count both requests:\n{text}"
        );
        assert!(text.contains("recall_request_duration_seconds"));
        assert!(text.contains("recall_store_memories 0"));
    }
}
