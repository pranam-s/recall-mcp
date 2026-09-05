//! REST API handlers (`/v1/*`, `/healthz`, `/openapi.json`).

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use recall_core::{NewMemory, RecallError, RecallHit, RecallParams, Weights};
use serde::Deserialize;

use crate::error::HttpError;
use crate::state::AppState;

pub async fn healthz() -> impl IntoResponse {
    Json(serde_json::json!({
        "status": "ok",
        "service": "recall-mcp",
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

pub async fn openapi() -> impl IntoResponse {
    ([(axum::http::header::CONTENT_TYPE, "application/json")], crate::openapi::DOCUMENT)
}

#[derive(Debug, Deserialize)]
pub struct CreateNamespace {
    pub name: String,
}

pub async fn create_namespace(
    State(state): State<AppState>,
    Json(body): Json<CreateNamespace>,
) -> Result<Response, HttpError> {
    let ns = state.with_store(|store| store.create_namespace(&body.name))?;
    tracing::info!(namespace = %ns.name, "created namespace via API");
    Ok((StatusCode::CREATED, Json(ns)).into_response())
}

pub async fn list_namespaces(State(state): State<AppState>) -> Result<Response, HttpError> {
    let namespaces = state.with_store(|store| store.list_namespaces())?;
    Ok(Json(namespaces).into_response())
}

pub async fn create_memory(
    State(state): State<AppState>,
    Json(mut body): Json<NewMemory>,
) -> Result<Response, HttpError> {
    if body.text.trim().is_empty() {
        return Err(RecallError::InvalidInput("text must not be empty".into()).into());
    }
    // Embed here when the caller did not supply a vector (imports pass their own).
    if body.embedding.is_none() {
        body.embedding = Some(state.embedder.embed(&body.text));
    }
    let memory = state.with_store(|store| store.insert_memory(&body))?;
    tracing::info!(memory_id = %memory.id, namespace = %memory.namespace_id, "created memory via API");
    Ok((StatusCode::CREATED, Json(memory)).into_response())
}

#[derive(Debug, Deserialize)]
pub struct ListMemoriesQuery {
    pub namespace: Option<String>,
    pub limit: Option<usize>,
}

pub async fn list_memories(
    State(state): State<AppState>,
    Query(query): Query<ListMemoriesQuery>,
) -> Result<Response, HttpError> {
    let limit = query.limit.unwrap_or(100);
    let memories = state.with_store(|store| store.list_memories(query.namespace.as_deref(), limit))?;
    Ok(Json(memories).into_response())
}

pub async fn delete_memory(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Response, HttpError> {
    let deleted = state.with_store(|store| store.delete_memory(&id))?;
    if deleted {
        Ok(StatusCode::NO_CONTENT.into_response())
    } else {
        Err(RecallError::MemoryNotFound(id).into())
    }
}

/// Standard serde helper: distinguish absent (`None`) from explicit `null` (`Some(None)`).
fn deserialize_double_option<'de, D, T>(de: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::Deserialize<'de>,
{
    serde::Deserialize::deserialize(de).map(Some)
}

/// Request body for `/v1/recall` — all knobs from ARCHITECTURE.md are tunable.
#[derive(Debug, Deserialize)]
pub struct RecallRequest {
    pub namespace: String,
    pub query: String,
    pub k: Option<usize>,
    /// `null` explicitly disables the recency term; absent keeps the 30-day default.
    #[serde(default, deserialize_with = "deserialize_double_option")]
    pub tau_days: Option<Option<f64>>,
    pub weights: Option<Weights>,
    pub pinned_boost: Option<f64>,
}

pub async fn recall(
    State(state): State<AppState>,
    Json(body): Json<RecallRequest>,
) -> Result<Json<Vec<RecallHit>>, HttpError> {
    let mut params = RecallParams::default();
    if let Some(k) = body.k {
        params.k = k;
    }
    if let Some(tau) = body.tau_days {
        params.tau_days = tau;
    }
    if let Some(w) = body.weights {
        params.weights = w;
    }
    if let Some(boost) = body.pinned_boost {
        params.pinned_boost = boost;
    }
    params.validate().map_err(HttpError)?;

    let query_embedding = state.embedder.embed(&body.query);
    let hits = state.with_store(|store| {
        store.recall(
            &body.namespace,
            &body.query,
            Some(&query_embedding),
            &params,
            recall_core::unix_now(),
        )
    })?;
    tracing::debug!(namespace = %body.namespace, hits = hits.len(), "recall served");
    Ok(Json(hits))
}

pub async fn stats(State(state): State<AppState>) -> Result<Response, HttpError> {
    let stats = state.with_store(|store| store.stats())?;
    Ok(Json(stats).into_response())
}
