//! MCP server exposing `remember`, `recall`, `forget`, `list_memories`.
//!
//! Transport-neutral tool layer (rmcp 3.x, MCP spec 2026-07-28) mounted over
//! stdio and streamable-HTTP. The streamable-HTTP mount is guarded by the same
//! API-key middleware as `/v1/*`; stdio needs none (it is a local child process).

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::model::ContentBlock;
use rmcp::{ServerHandler, ServiceExt, tool, tool_handler, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::state::AppState;

type McpError = rmcp::ErrorData;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RememberParams {
    /// Namespace (workspace) to store the memory in.
    pub namespace: String,
    /// The memory text: one observation, decision, or preference.
    pub text: String,
    /// Optional tags for filtering and UI grouping.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Pin the memory so it always receives a ranking boost.
    #[serde(default)]
    pub pinned: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RecallToolParams {
    /// Namespace (workspace) to search. Memories never leak across namespaces.
    pub namespace: String,
    /// Free-text query.
    pub query: String,
    /// How many hits to return (1..=100, default 5).
    #[serde(default)]
    pub k: Option<usize>,
    /// Recency decay constant in days; null disables the recency term.
    #[serde(default)]
    pub tau_days: Option<Option<f64>>,
    /// Flat boost applied to pinned memories (default 0.2).
    #[serde(default)]
    pub pinned_boost: Option<f64>,
    /// Only return memories carrying ALL of these tags (exact match).
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ForgetParam {
    /// Id of the memory to delete.
    pub id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct UpdateMemoryParams {
    /// Id of the memory to update.
    pub id: String,
    /// New text (re-embedded automatically). Omit to keep the current text.
    #[serde(default)]
    pub text: Option<String>,
    /// New tags; omitted means tags stay unchanged.
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    /// Change the pinned flag; omitted means unchanged.
    #[serde(default)]
    pub pinned: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListMemoriesParams {
    /// Namespace to list.
    pub namespace: String,
    /// Maximum number of memories to return (default 50).
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Clone)]
pub struct RecallTools {
    state: AppState,
    tool_router: ToolRouter<Self>,
}

impl RecallTools {
    pub fn new(state: AppState) -> Self {
        Self {
            state,
            tool_router: Self::tool_router(),
        }
    }

    fn tool_json(value: impl serde::Serialize) -> Result<CallToolResult, McpError> {
        Ok(CallToolResult::success(vec![ContentBlock::json(value)?]))
    }

    fn domain_err(e: recall_core::RecallError) -> McpError {
        match &e {
            // Caller-fixable problems are "invalid params"; anything else is internal.
            recall_core::RecallError::InvalidInput(_)
            | recall_core::RecallError::NamespaceNotFound(_) => {
                McpError::invalid_params(e.to_string(), None)
            }
            _ => McpError::internal_error(e.to_string(), None),
        }
    }
}

#[tool_router(router = tool_router)]
impl RecallTools {
    #[tool(
        description = "Store a memory (observation, decision, preference) in a namespace. The namespace is created automatically if unknown. Returns the stored record with its id."
    )]
    fn remember(
        &self,
        Parameters(params): Parameters<RememberParams>,
    ) -> Result<CallToolResult, McpError> {
        // MCP callers are agents: remember must "just work" (PRD user story),
        // so unlike the strict REST API we auto-create the namespace.
        let namespace = params.namespace.trim().to_string();
        if namespace.is_empty() {
            return Err(McpError::invalid_params(
                "namespace must not be empty",
                None,
            ));
        }
        self.state
            .with_store(|store| store.get_or_create_namespace(&namespace))
            .map_err(Self::domain_err)?;
        let embedding = self.state.embedder.embed(&params.text);
        let memory = recall_core::NewMemory {
            namespace,
            text: params.text,
            tags: params.tags,
            source: Some("mcp".to_string()),
            pinned: params.pinned,
            created_at: None,
            id: None,
            embedding: Some(embedding),
        };
        let stored = self
            .state
            .with_store(|store| store.insert_memory(&memory))
            .map_err(Self::domain_err)?;
        Self::tool_json(&stored)
    }

    #[tool(
        description = "Hybrid recall: BM25 keyword + embedding similarity + recency decay, ranked with per-hit score breakdowns (bm25, vector, recency, pinned_boost, total)."
    )]
    fn recall(
        &self,
        Parameters(params): Parameters<RecallToolParams>,
    ) -> Result<CallToolResult, McpError> {
        let mut tool_params = recall_core::RecallParams::default();
        if let Some(k) = params.k {
            tool_params.k = k;
        }
        if let Some(tau) = params.tau_days {
            tool_params.tau_days = tau;
        }
        if let Some(boost) = params.pinned_boost {
            tool_params.pinned_boost = boost;
        }
        tool_params.tags = params.tags;
        tool_params.validate().map_err(Self::domain_err)?;
        let query_embedding = self.state.embedder.embed(&params.query);
        let hits = self
            .state
            .with_store(|store| {
                store.recall(
                    &params.namespace,
                    &params.query,
                    Some(&query_embedding),
                    &tool_params,
                    recall_core::unix_now(),
                )
            })
            .map_err(Self::domain_err)?;
        Self::tool_json(&hits)
    }

    #[tool(description = "Forget (permanently delete) one memory by id.")]
    fn forget(
        &self,
        Parameters(params): Parameters<ForgetParam>,
    ) -> Result<CallToolResult, McpError> {
        let deleted = self
            .state
            .with_store(|store| store.delete_memory(&params.id))
            .map_err(Self::domain_err)?;
        Self::tool_json(serde_json::json!({ "id": params.id, "deleted": deleted }))
    }

    #[tool(
        description = "Update a memory in place: revise its text (re-embedded automatically), replace its tags, or change its pinned flag. Only the provided fields change; the id and creation time are preserved."
    )]
    fn update_memory(
        &self,
        Parameters(params): Parameters<UpdateMemoryParams>,
    ) -> Result<CallToolResult, McpError> {
        if params.text.as_deref().is_some_and(|t| t.trim().is_empty()) {
            return Err(McpError::invalid_params("text must not be empty", None));
        }
        let mut update = recall_core::MemoryUpdate {
            text: params.text,
            tags: params.tags,
            pinned: params.pinned,
            source: None,
            embedding: None,
        };
        if let Some(text) = &update.text {
            update.embedding = Some(self.state.embedder.embed(text));
        }
        let memory = self
            .state
            .with_store(|store| store.update_memory(&params.id, &update))
            .map_err(Self::domain_err)?;
        Self::tool_json(&memory)
    }

    #[tool(description = "List memories in a namespace, newest first.")]
    fn list_memories(
        &self,
        Parameters(params): Parameters<ListMemoriesParams>,
    ) -> Result<CallToolResult, McpError> {
        let limit = params.limit.unwrap_or(50);
        let memories = self
            .state
            .with_store(|store| store.list_memories(Some(&params.namespace), limit))
            .map_err(Self::domain_err)?;
        Self::tool_json(&memories)
    }
}

#[tool_handler(
    router = self.tool_router,
    name = "recall-mcp",
    version = "0.1.0",
    instructions = "Local-first semantic memory broker. Use remember to persist decisions/observations, recall to retrieve ranked memories with score breakdowns (optionally filtered by tags), update_memory to revise an existing memory, forget to delete, and list_memories to browse a namespace. Namespaces isolate workspaces."
)]
impl ServerHandler for RecallTools {}

/// Build the streamable-HTTP MCP service for mounting at `/mcp`.
pub fn streamable_http_service(
    state: &AppState,
) -> rmcp::transport::streamable_http_server::StreamableHttpService<
    RecallTools,
    rmcp::transport::streamable_http_server::session::local::LocalSessionManager,
> {
    use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
    use rmcp::transport::streamable_http_server::{
        StreamableHttpServerConfig, StreamableHttpService,
    };
    let config = StreamableHttpServerConfig::default()
        .with_legacy_session_mode(false)
        .with_json_response(true);
    let state = state.clone();
    StreamableHttpService::new(
        move || Ok(RecallTools::new(state.clone())),
        LocalSessionManager::default().into(),
        config,
    )
}

/// Serve MCP over stdio until the client disconnects (used by `recall-cli serve --stdio`).
pub async fn run_stdio_server(state: AppState) -> anyhow::Result<()> {
    let service = RecallTools::new(state)
        .serve(rmcp::transport::io::stdio())
        .await?;
    service.waiting().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_state() -> AppState {
        let store =
            recall_core::shared(recall_core::SqliteStore::open_in_memory().expect("in-memory db"));
        let embedder: std::sync::Arc<dyn recall_core::Embedder> =
            std::sync::Arc::new(recall_core::HashEmbedder::new_256());
        AppState::new(store, embedder, None)
    }

    #[test]
    fn server_info_pins_name_and_spec_version_literal() {
        let info = RecallTools::new(test_state()).get_info();
        assert_eq!(info.server_info.name, "recall-mcp");
        // Guard against drift between the macro attribute literal and the crate version.
        assert_eq!(info.server_info.version, env!("CARGO_PKG_VERSION"));
        assert!(
            info.capabilities.tools.is_some(),
            "tools capability must be declared"
        );
        assert!(info.instructions.is_some());
    }
}
