//! Optional OpenAI-compatible embedding client, behind the `openai` feature
//! (ADR-003: API embedders are pluggable opt-ins; the default remains offline).
//!
//! The HTTP call itself is intentionally untested in CI (no live key); request
//! construction and response parsing are pure functions with unit tests.

use serde::{Deserialize, Serialize};

use crate::embed::Embedder;
use crate::error::{RecallError, Result};

#[derive(Debug, Clone)]
pub struct OpenAIEmbedder {
    endpoint: String,
    api_key: String,
    model: String,
    dims: usize,
}

#[derive(Debug, Serialize)]
struct EmbeddingRequest<'a> {
    model: &'a str,
    input: &'a str,
}

#[derive(Debug, Deserialize)]
struct EmbeddingResponse {
    data: Vec<EmbeddingItem>,
}

#[derive(Debug, Deserialize)]
struct EmbeddingItem {
    embedding: Vec<f32>,
}

impl OpenAIEmbedder {
    /// Create a client. `endpoint` is the full embeddings URL,
    /// e.g. `https://api.openai.com/v1/embeddings`.
    pub fn new(endpoint: impl Into<String>, api_key: impl Into<String>, model: impl Into<String>, dims: usize) -> Result<Self> {
        let api_key = api_key.into();
        if api_key.trim().is_empty() {
            return Err(RecallError::InvalidInput("OpenAI api key must not be empty".into()));
        }
        if dims == 0 {
            return Err(RecallError::InvalidInput("embedding dimension must be > 0".into()));
        }
        Ok(Self {
            endpoint: endpoint.into(),
            api_key,
            model: model.into(),
            dims,
        })
    }

    /// OpenAI's default embeddings endpoint.
    pub fn openai(api_key: impl Into<String>, model: impl Into<String>, dims: usize) -> Result<Self> {
        Self::new("https://api.openai.com/v1/embeddings", api_key, model, dims)
    }

    /// Build the JSON request body sent to the API.
    pub(crate) fn request_body(&self, text: &str) -> String {
        serde_json::to_string(&EmbeddingRequest { model: &self.model, input: text })
            .unwrap_or_else(|_| "{}".to_string())
    }

    /// Parse an API response body into the embedding vector, validating shape.
    pub(crate) fn parse_response(body: &str) -> Result<Vec<f32>> {
        let parsed: EmbeddingResponse = serde_json::from_str(body)
            .map_err(|e| RecallError::Embedder(format!("bad embedding response: {e}")))?;
        let mut items = parsed.data;
        if items.len() != 1 {
            return Err(RecallError::Embedder(format!(
                "expected exactly 1 embedding, got {}",
                items.len()
            )));
        }
        let vec = items.swap_remove(0).embedding;
        if vec.is_empty() {
            return Err(RecallError::Embedder("embedding is empty".into()));
        }
        Ok(vec)
    }
}

impl Embedder for OpenAIEmbedder {
    fn dimensions(&self) -> usize {
        self.dims
    }

    fn name(&self) -> &'static str {
        "openai"
    }

    fn embed(&self, text: &str) -> Vec<f32> {
        // Blocking HTTP; shells call this via `spawn_blocking`. Failures surface
        // as a zero-vector-free error path: we return an empty vector and let
        // callers treat missing/empty embeddings as "no vector signal".
        // (Errors are logged; the hybrid scorer degrades to keyword-only.)
        match self.embed_checked(text) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(error = %e, "OpenAI embed failed; degrading to keyword-only");
                Vec::new()
            }
        }
    }
}

impl OpenAIEmbedder {
    fn embed_checked(&self, text: &str) -> Result<Vec<f32>> {
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| RecallError::Embedder(e.to_string()))?;
        let response = client
            .post(&self.endpoint)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .body(self.request_body(text))
            .send()
            .map_err(|e| RecallError::Embedder(e.to_string()))?;
        let status = response.status();
        let body = response
            .text()
            .map_err(|e| RecallError::Embedder(e.to_string()))?;
        if !status.is_success() {
            return Err(RecallError::Embedder(format!("embeddings API returned {status}")));
        }
        Self::parse_response(&body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constructor_requires_key_and_dims() {
        assert!(OpenAIEmbedder::openai("", "text-embedding-3-small", 1536).is_err());
        assert!(OpenAIEmbedder::openai("  ", "m", 1536).is_err());
        assert!(OpenAIEmbedder::new("https://x/v1/embeddings", "key", "m", 0).is_err());
        let e = OpenAIEmbedder::openai("sk-test", "text-embedding-3-small", 1536).unwrap();
        assert_eq!(e.dimensions(), 1536);
        assert_eq!(e.name(), "openai");
    }

    #[test]
    fn request_body_matches_api_schema() {
        let e = OpenAIEmbedder::new("https://x", "k", "text-embedding-3-small", 1536).unwrap();
        assert_eq!(
            e.request_body("hello world"),
            r#"{"model":"text-embedding-3-small","input":"hello world"}"#
        );
    }

    #[test]
    fn parse_response_validates_payload() {
        let good = r#"{"data":[{"embedding":[0.1,0.2,0.3]}]}"#;
        assert_eq!(OpenAIEmbedder::parse_response(good).unwrap(), vec![0.1, 0.2, 0.3]);

        assert!(OpenAIEmbedder::parse_response("not json").is_err());
        assert!(OpenAIEmbedder::parse_response(r#"{"data":[]}"#).is_err());
        assert!(OpenAIEmbedder::parse_response(r#"{"data":[{"embedding":[]}]}"#).is_err());
        assert!(OpenAIEmbedder::parse_response(r#"{"data":[{"embedding":[1]},{"embedding":[2]}]}"#).is_err());
    }

    #[test]
    fn embed_degrades_to_empty_vector_on_connection_error() {
        // Port 1 on localhost refuses connections quickly; proves the error path.
        let e = OpenAIEmbedder::new("http://127.0.0.1:1/v1/embeddings", "k", "m", 4).unwrap();
        assert!(e.embed("hello").is_empty());
    }
}
