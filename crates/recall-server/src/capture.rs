//! Capture ingestion (D-018) shared by the REST `POST /v1/capture` endpoint
//! and the MCP `capture` tool: one request type, one chunking/embedding
//! pipeline, one storage path — REST and MCP cannot drift.

use recall_core::{Embedder, Memory, NewMemory, RecallError, Store};
use schemars::JsonSchema;
use serde::Deserialize;

/// One transcript message. `role` is prefixed into the stored text and added
/// as a `role:<role>` tag; `at` becomes the chunks' `created_at`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct CaptureMessage {
    pub role: Option<String>,
    pub content: String,
    #[serde(default)]
    pub at: Option<i64>,
}

/// Body of `POST /v1/capture` and the arguments of the MCP `capture` tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct CaptureRequest {
    pub namespace: String,
    pub transcript: Vec<CaptureMessage>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub max_chunk_chars: Option<usize>,
    #[serde(default)]
    pub include_memories: bool,
    #[serde(default)]
    pub include_embedding: bool,
}

/// A validated, fully embedded capture: chunks carry their vectors so the
/// storage commit never runs model inference under the store lock.
pub struct PreparedCapture {
    pub namespace: String,
    pub memories: Vec<NewMemory>,
}

/// Validate the request, chunk every message deterministically, and embed
/// each chunk. Inference happens here — outside any store lock.
pub fn prepare(
    embedder: &dyn Embedder,
    body: &CaptureRequest,
) -> Result<PreparedCapture, RecallError> {
    if body.namespace.trim().is_empty() {
        return Err(RecallError::InvalidInput(
            "namespace must not be empty".into(),
        ));
    }
    recall_core::validation::validate_namespace(&body.namespace)?;
    let total_chars: usize = body
        .transcript
        .iter()
        .map(|m| m.content.chars().count() + m.role.as_deref().map_or(0, |r| r.chars().count()))
        .sum();
    recall_core::validation::validate_capture(body.transcript.len(), total_chars)?;
    let max_chars = body
        .max_chunk_chars
        .unwrap_or(1000)
        .clamp(100, recall_core::validation::MAX_CHUNK_CHARS);
    let mut memories: Vec<NewMemory> = Vec::new();
    for message in &body.transcript {
        let content = message.content.trim();
        if content.is_empty() {
            continue;
        }
        let text = match message.role.as_deref().map(str::trim) {
            Some(role) if !role.is_empty() => format!("{role}: {content}"),
            _ => content.to_string(),
        };
        for chunk in recall_core::capture::chunk_text(&text, max_chars)? {
            if memories.len() >= recall_core::validation::MAX_CAPTURE_CHUNKS {
                recall_core::validation::validate_capture_chunks(memories.len() + 1)?;
            }
            let mut tags = body.tags.clone();
            if let Some(role) = message
                .role
                .as_deref()
                .map(str::trim)
                .filter(|r| !r.is_empty())
            {
                tags.push(format!("role:{role}"));
            }
            memories.push(NewMemory {
                namespace: body.namespace.trim().to_string(),
                embedding: Some(embedder.embed(&chunk)),
                text: chunk,
                tags,
                source: Some("capture".into()),
                pinned: false,
                created_at: message.at,
                id: None,
            });
        }
    }
    if memories.is_empty() {
        return Err(RecallError::InvalidInput(
            "transcript contained no captureable text".into(),
        ));
    }
    Ok(PreparedCapture {
        namespace: body.namespace.trim().to_string(),
        memories,
    })
}

/// Store a prepared capture: the namespace is auto-created (D-009) and every
/// chunk is inserted in ONE write transaction, so a failure part-way through
/// (duplicate id, validation error, disk full) leaves neither a stray
/// namespace nor a partial transcript behind. Callers hold the store lock via
/// `AppState::with_store`.
pub fn commit(store: &dyn Store, prepared: &PreparedCapture) -> Result<Vec<Memory>, RecallError> {
    let mut captured = Vec::with_capacity(prepared.memories.len());
    store.write_tx(&mut |tx| {
        tx.get_or_create_namespace(&prepared.namespace)?;
        captured.clear();
        for memory in &prepared.memories {
            captured.push(tx.insert_memory(memory)?);
        }
        Ok(())
    })?;
    Ok(captured)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct ZeroEmbedder;
    impl Embedder for ZeroEmbedder {
        fn dimensions(&self) -> usize {
            4
        }
        fn name(&self) -> &'static str {
            "zero-test"
        }
        fn embed(&self, _text: &str) -> Vec<f32> {
            vec![0.0; 4]
        }
    }

    #[test]
    fn prepare_rejects_a_capture_that_would_explode_into_too_many_chunks() {
        // Three 70k-character messages at a 100-character chunk cap would
        // produce about 2,100 chunks, above the 2,000 chunk ceiling.
        let body = CaptureRequest {
            namespace: "too-many".into(),
            transcript: (0..3)
                .map(|_| CaptureMessage {
                    role: None,
                    content: "a ".repeat(35_000),
                    at: None,
                })
                .collect(),
            tags: vec![],
            max_chunk_chars: Some(100),
            include_memories: false,
            include_embedding: false,
        };
        let err = prepare(&ZeroEmbedder, &body)
            .err()
            .expect("too many chunks must fail")
            .to_string();
        assert!(err.contains("chunks"), "{err}");
        assert!(err.contains("2000"), "{err}");
    }
}
