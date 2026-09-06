//! recall-server — Axum HTTP API and MCP transports around the pure core.

pub mod api;
pub mod auth;
pub mod error;
pub mod mcp;
pub mod openapi;
pub mod state;

use std::sync::Arc;

use axum::Router;
use axum::routing::{delete, get, post};
use recall_core::HashEmbedder;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

use crate::state::AppState;

/// Server settings supplied by the binary (CLI `serve`).
#[derive(Debug, Clone, Default)]
pub struct ServerConfig {
    /// When set, all `/v1/*` and `/mcp` requests must present this key via
    /// `Authorization: Bearer <key>` or `X-API-Key: <key>`. Unset = open access
    /// (local-first default; see README security notes).
    pub api_key: Option<String>,
    /// Directory of the built web UI to serve at `/` (production e2e setup).
    pub web_dir: Option<std::path::PathBuf>,
}

/// Build the complete application router: REST API, MCP streamable-HTTP,
/// optional static UI, CORS, and tracing.
pub fn build_router(store: recall_core::SharedStore, config: &ServerConfig) -> Router {
    let embedder: Arc<dyn recall_core::Embedder> = Arc::new(HashEmbedder::new_256());
    let state = AppState::new(store, embedder, config.api_key.clone());

    // Probes and the OpenAPI document never require auth (monitoring + spec discovery).
    let open: Router<AppState> = Router::new()
        .route("/healthz", get(api::healthz))
        .route("/openapi.json", get(api::openapi));

    let auth = axum::middleware::from_fn_with_state(state.clone(), auth::require_api_key);

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
        .route("/v1/stats", get(api::stats))
        .layer(auth.clone());

    // The MCP endpoint mounts as its own scoped router so the API-key layer
    // wraps the streamable-HTTP transport cleanly (including its session routes).
    let mcp: Router<AppState> = Router::new()
        .nest_service("/mcp", mcp::streamable_http_service(&state))
        .layer(auth);

    let mut app = Router::new()
        .merge(open)
        .merge(protected)
        .merge(mcp)
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .with_state(state);

    if let Some(dir) = &config.web_dir {
        app = app.fallback_service(
            tower_http::services::ServeDir::new(dir).append_index_html_on_directories(true),
        );
    }
    app
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::Request;
    use tower::ServiceExt;

    #[tokio::test]
    async fn healthz_is_open_even_with_api_key() {
        let dir = tempfile::tempdir().unwrap();
        let store =
            recall_core::shared(recall_core::SqliteStore::open(dir.path().join("t.db")).unwrap());
        let app = build_router(
            store,
            &ServerConfig {
                api_key: Some("k".into()),
                web_dir: None,
            },
        );
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
}
