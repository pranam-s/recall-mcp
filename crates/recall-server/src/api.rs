//! REST API handlers (`/v1/*`, `/healthz`, `/openapi.json`).

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use recall_core::validation;
use recall_core::views::{CompactCaptureSummary, CompactListPage, CompactMemory, CompactRecallHit};
use recall_core::{MemoryUpdate, NewMemory, RecallError, RecallParams, Weights};
use serde::Deserialize;

use crate::error::{ApiJson, HttpError};
use crate::state::AppState;

pub async fn healthz() -> impl IntoResponse {
    Json(serde_json::json!({
        "status": "ok",
        "service": "recall-mcp",
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

pub async fn openapi() -> impl IntoResponse {
    (
        [(axum::http::header::CONTENT_TYPE, "application/json")],
        crate::openapi::DOCUMENT,
    )
}

#[derive(Debug, Deserialize)]
pub struct CreateNamespace {
    pub name: String,
}

pub async fn create_namespace(
    State(state): State<AppState>,
    ApiJson(body): ApiJson<CreateNamespace>,
) -> Result<Response, HttpError> {
    validation::validate_namespace(&body.name)?;
    let ns = state
        .with_store_async(move |store| store.create_namespace(&body.name))
        .await?;
    tracing::info!(namespace = %ns.name, "created namespace via API");
    Ok((StatusCode::CREATED, Json(ns)).into_response())
}

pub async fn list_namespaces(State(state): State<AppState>) -> Result<Response, HttpError> {
    let namespaces = state
        .with_store_async(|store| store.list_namespaces())
        .await?;
    Ok(Json(namespaces).into_response())
}

#[derive(Debug, Default, Deserialize)]
pub struct OutputQuery {
    /// Restore the full record (embedding, namespace_id, pinned, ...).
    #[serde(default)]
    pub include_embedding: bool,
}

pub async fn create_memory(
    State(state): State<AppState>,
    Query(output): Query<OutputQuery>,
    ApiJson(mut body): ApiJson<NewMemory>,
) -> Result<Response, HttpError> {
    validation::validate_namespace(&body.namespace)?;
    validation::validate_text(&body.text)?;
    validation::validate_tags(&body.tags)?;
    validation::validate_source(body.source.as_deref())?;
    validation::validate_embedding(body.embedding.as_deref())?;
    if let Some(id) = &body.id {
        validation::validate_id(id)?;
    }
    // A caller-supplied vector of the wrong length would silently zero every
    // cosine against this store's embedder (the mismatch rule returns 0.0) —
    // reject it instead of accepting silent scoring loss (AR-005).
    if let Some(vec) = &body.embedding
        && vec.len() != state.embedder.dimensions()
    {
        return Err(RecallError::InvalidInput(format!(
            "embedding has {} dims but the configured embedder {:?} produces {}",
            vec.len(),
            state.embedder.name(),
            state.embedder.dimensions()
        ))
        .into());
    }
    // Embed here when the caller did not supply a vector (imports pass their own).
    if body.embedding.is_none() {
        let embedding = state.embed_async(body.text.clone()).await?;
        validation::validate_embedding(Some(&embedding))?;
        body.embedding = Some(embedding);
    }
    let memory = state
        .with_store_async(move |store| store.insert_memory(&body))
        .await?;
    tracing::info!(memory_id = %memory.id, namespace = %memory.namespace_id, "created memory via API");
    let payload = if output.include_embedding {
        serde_json::to_value(&memory)?
    } else {
        serde_json::to_value(CompactMemory::from(&memory))?
    };
    Ok((StatusCode::CREATED, Json(payload)).into_response())
}

#[derive(Debug, Deserialize)]
pub struct ListMemoriesQuery {
    pub namespace: Option<String>,
    pub limit: Option<usize>,
    /// Rows to skip (pagination).
    pub offset: Option<usize>,
    /// Restore full records (including embeddings) in the page items.
    #[serde(default)]
    pub include_embedding: bool,
}

pub async fn list_memories(
    State(state): State<AppState>,
    Query(query): Query<ListMemoriesQuery>,
) -> Result<Response, HttpError> {
    if let Some(namespace) = &query.namespace {
        validation::validate_namespace(namespace)?;
    }
    // `page_limit` shares MAX_PAGE_LIMIT with the MCP `list_memories` tool;
    // see its docs for the negative-LIMIT hazard this clamp exists for.
    let limit = crate::page_limit(query.limit, 100);
    let offset = query.offset.unwrap_or(0);
    let include_embedding = query.include_embedding;
    let (memories, total) = state
        .with_store_async(move |store| {
            let total = store.count_memories(query.namespace.as_deref())?;
            let memories = store.list_memories(query.namespace.as_deref(), limit, offset)?;
            std::result::Result::Ok::<_, recall_core::RecallError>((memories, total))
        })
        .await?;
    let page: CompactListPage<serde_json::Value> = if include_embedding {
        CompactListPage::new(
            memories
                .iter()
                .map(serde_json::to_value)
                .collect::<std::result::Result<Vec<_>, _>>()?,
            total,
            limit,
            offset,
        )
    } else {
        CompactListPage::new(
            memories
                .iter()
                .map(|m| serde_json::to_value(CompactMemory::from(m)))
                .collect::<std::result::Result<Vec<_>, _>>()?,
            total,
            limit,
            offset,
        )
    };
    Ok((
        [(
            axum::http::HeaderName::from_static("x-total-count"),
            total.to_string(),
        )],
        Json(page),
    )
        .into_response())
}

pub async fn delete_memory(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Response, HttpError> {
    validation::validate_id(&id)?;
    let id_for_error = id.clone();
    let deleted = state
        .with_store_async(move |store| store.delete_memory(&id))
        .await?;
    if deleted {
        Ok(StatusCode::NO_CONTENT.into_response())
    } else {
        Err(RecallError::MemoryNotFound(id_for_error).into())
    }
}

/// Request body for `PATCH /v1/memories/{id}` — every field optional.
#[derive(Debug, Deserialize)]
pub struct UpdateMemoryRequest {
    pub text: Option<String>,
    pub tags: Option<Vec<String>>,
    pub source: Option<String>,
    pub pinned: Option<bool>,
}

pub async fn update_memory(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(output): Query<OutputQuery>,
    ApiJson(body): ApiJson<UpdateMemoryRequest>,
) -> Result<Json<serde_json::Value>, HttpError> {
    validation::validate_id(&id)?;
    if let Some(text) = &body.text {
        validation::validate_text(text)?;
    }
    if let Some(tags) = &body.tags {
        validation::validate_tags(tags)?;
    }
    validation::validate_source(body.source.as_deref())?;
    let mut update = MemoryUpdate {
        text: body.text,
        tags: body.tags,
        source: body.source,
        pinned: body.pinned,
        embedding: None,
    };
    // Re-embed when the text changes (same policy as create).
    if let Some(text) = &update.text {
        let embedding = state.embed_async(text.clone()).await?;
        validation::validate_embedding(Some(&embedding))?;
        update.embedding = Some(embedding);
    }
    let memory = state
        .with_store_async(move |store| store.update_memory(&id, &update))
        .await?;
    tracing::info!(memory_id = %memory.id, "updated memory via API");
    let payload = if output.include_embedding {
        serde_json::to_value(&memory)?
    } else {
        serde_json::to_value(CompactMemory::from(&memory))?
    };
    Ok(Json(payload))
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
    /// Restrict hits to memories carrying ALL of these tags.
    pub tags: Option<Vec<String>>,
}

pub async fn recall(
    State(state): State<AppState>,
    Query(output): Query<OutputQuery>,
    ApiJson(body): ApiJson<RecallRequest>,
) -> Result<Json<serde_json::Value>, HttpError> {
    validation::validate_namespace(&body.namespace)?;
    validation::validate_query(&body.query)?;
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
    if let Some(tags) = body.tags {
        params.tags = tags;
    }
    params.validate().map_err(HttpError)?;

    let query_embedding = state.embed_async(body.query.clone()).await?;
    let namespace = body.namespace.clone();
    let query = body.query;
    let hits = state
        .with_store_async(move |store| {
            store.recall(
                &namespace,
                &query,
                Some(&query_embedding),
                &params,
                recall_core::unix_now(),
            )
        })
        .await?;
    tracing::debug!(namespace = %body.namespace, hits = hits.len(), "recall served");
    let payload = if output.include_embedding {
        serde_json::to_value(&hits)?
    } else {
        serde_json::to_value(hits.iter().map(CompactRecallHit::from).collect::<Vec<_>>())?
    };
    Ok(Json(payload))
}

/// Stats plus the embedder identity the web UI (and operators) need to
/// explain why a store was written with particular vectors.
pub async fn stats(State(state): State<AppState>) -> Result<Response, HttpError> {
    let stats = state.with_store_async(|store| store.stats()).await?;
    let mut value = serde_json::to_value(stats)?;
    if let Some(object) = value.as_object_mut() {
        object.insert(
            "embedder".into(),
            serde_json::json!({
                "name": state.embedder.name(),
                "dimensions": state.embedder.dimensions(),
                "fingerprint": state.embedder.fingerprint(),
            }),
        );
    }
    Ok(Json(value).into_response())
}

/// Prometheus text exposition (0.0.4): request counters and latencies,
/// embedding timings, and live store gauges. Protected by the API-key
/// middleware like the rest of `/v1/*`.
pub async fn metrics(State(state): State<AppState>) -> Response {
    let stats = state
        .with_store_async(|store| store.stats())
        .await
        .unwrap_or_else(|_| {
            // A failing store still reports the request metrics.
            recall_core::Stats {
                total_namespaces: 0,
                total_memories: 0,
                pinned_memories: 0,
                per_namespace: Vec::new(),
            }
        });
    (
        [(
            axum::http::header::CONTENT_TYPE,
            "text/plain; version=0.0.4; charset=utf-8",
        )],
        state.metrics.render(&stats),
    )
        .into_response()
}

// ---- Capture ingestion (D-018): auto-capture hook ----
// Types and the chunk/embed/commit pipeline live in `crate::capture`, shared
// verbatim with the MCP `capture` tool.

#[derive(Debug, Default, Deserialize)]
pub struct CaptureOutputQuery {
    /// Include every captured record (default: summary only).
    #[serde(default)]
    pub include_memories: bool,
    /// With `include_memories`, return full records including embeddings.
    #[serde(default)]
    pub include_embedding: bool,
}

pub async fn capture(
    State(state): State<AppState>,
    Query(output): Query<CaptureOutputQuery>,
    ApiJson(body): ApiJson<crate::capture::CaptureRequest>,
) -> Result<Response, HttpError> {
    let embedder = state.embedder.clone();
    let prepared =
        tokio::task::spawn_blocking(move || crate::capture::prepare(embedder.as_ref(), &body))
            .await
            .map_err(|e| RecallError::Internal(format!("capture task failed: {e}")))??;
    let namespace = prepared.namespace.clone();
    let captured = state
        .with_store_async(move |store| crate::capture::commit(store, &prepared))
        .await?;
    tracing::info!(
        namespace = %namespace,
        memories = captured.len(),
        "captured transcript via API"
    );
    let summary = CompactCaptureSummary::new(&namespace, &captured);
    let payload = if output.include_memories {
        let memories = if output.include_embedding {
            serde_json::to_value(&captured)?
        } else {
            serde_json::to_value(captured.iter().map(CompactMemory::from).collect::<Vec<_>>())?
        };
        serde_json::json!({
            "namespace": summary.namespace,
            "captured": summary.captured,
            "first_id": summary.first_id,
            "last_id": summary.last_id,
            "memories": memories,
        })
    } else {
        serde_json::to_value(summary)?
    };
    Ok((StatusCode::CREATED, Json(payload)).into_response())
}
