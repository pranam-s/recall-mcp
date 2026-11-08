//! Central input validation for every surface (REST, MCP, CLI, import).
//!
//! Limits exist to keep one request (or one stored row) from turning into an
//! unbounded allocation, an unbounded embedding call, or an unbounded SQL
//! bind. They are deliberately generous for agent memories (which are short
//! by design) while still failing closed on hostile input.

use crate::error::{RecallError, Result};

/// Longest accepted recall query, in characters.
pub const MAX_QUERY_CHARS: usize = 10_000;
/// Longest accepted memory/document text, in characters.
pub const MAX_TEXT_CHARS: usize = 100_000;
/// Longest accepted namespace name, in characters.
pub const MAX_NAMESPACE_CHARS: usize = 200;
/// Longest accepted memory id, in characters.
pub const MAX_ID_CHARS: usize = 256;
/// Longest accepted single tag, in characters.
pub const MAX_TAG_CHARS: usize = 100;
/// Most tags accepted on one memory or filter.
pub const MAX_TAGS: usize = 100;
/// Longest accepted `source` label, in characters.
pub const MAX_SOURCE_CHARS: usize = 100;
/// Highest accepted embedding dimensionality. Real embedders use 256-3072;
/// anything above this is a caller mistake or an allocation attack.
pub const MAX_EMBEDDING_DIMS: usize = 8192;
/// Most messages accepted in one capture request.
pub const MAX_CAPTURE_MESSAGES: usize = 1_000;
/// Most total transcript characters accepted in one capture request.
pub const MAX_CAPTURE_CHARS: usize = 2_000_000;
/// Largest chunk size the capture pipeline will honor.
pub const MAX_CHUNK_CHARS: usize = 8_000;

fn chars(value: &str) -> usize {
    value.chars().count()
}

/// A namespace name: non-empty after trimming, bounded, and free of control
/// characters (they make logs and terminal output ambiguous).
pub fn validate_namespace(name: &str) -> Result<()> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(RecallError::InvalidInput(
            "namespace must not be empty".into(),
        ));
    }
    if chars(trimmed) > MAX_NAMESPACE_CHARS {
        return Err(RecallError::InvalidInput(format!(
            "namespace must be at most {MAX_NAMESPACE_CHARS} characters"
        )));
    }
    if trimmed.chars().any(char::is_control) {
        return Err(RecallError::InvalidInput(
            "namespace must not contain control characters".into(),
        ));
    }
    Ok(())
}

/// Memory or chunk text: non-empty after trimming and bounded.
pub fn validate_text(text: &str) -> Result<()> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err(RecallError::InvalidInput(
            "memory text must not be empty".into(),
        ));
    }
    if chars(trimmed) > MAX_TEXT_CHARS {
        return Err(RecallError::InvalidInput(format!(
            "memory text must be at most {MAX_TEXT_CHARS} characters"
        )));
    }
    Ok(())
}

/// A recall query: non-empty after trimming and bounded well below the
/// document limit (queries are short; tokenization is linear).
pub fn validate_query(query: &str) -> Result<()> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Err(RecallError::InvalidInput("query must not be empty".into()));
    }
    if chars(trimmed) > MAX_QUERY_CHARS {
        return Err(RecallError::InvalidInput(format!(
            "query must be at most {MAX_QUERY_CHARS} characters"
        )));
    }
    Ok(())
}

/// A memory id: non-empty, bounded, and control-character free.
pub fn validate_id(id: &str) -> Result<()> {
    let trimmed = id.trim();
    if trimmed.is_empty() {
        return Err(RecallError::InvalidInput(
            "memory id must not be empty".into(),
        ));
    }
    if chars(trimmed) > MAX_ID_CHARS {
        return Err(RecallError::InvalidInput(format!(
            "memory id must be at most {MAX_ID_CHARS} characters"
        )));
    }
    if trimmed.chars().any(char::is_control) {
        return Err(RecallError::InvalidInput(
            "memory id must not contain control characters".into(),
        ));
    }
    Ok(())
}

/// A tag list: bounded in count, each tag non-empty and bounded.
pub fn validate_tags(tags: &[String]) -> Result<()> {
    if tags.len() > MAX_TAGS {
        return Err(RecallError::InvalidInput(format!(
            "at most {MAX_TAGS} tags are accepted"
        )));
    }
    for tag in tags {
        let trimmed = tag.trim();
        if trimmed.is_empty() {
            return Err(RecallError::InvalidInput("tags must not be empty".into()));
        }
        if chars(trimmed) > MAX_TAG_CHARS {
            return Err(RecallError::InvalidInput(format!(
                "each tag must be at most {MAX_TAG_CHARS} characters"
            )));
        }
    }
    Ok(())
}

/// A source label: bounded when present; blank is normalized by the store.
pub fn validate_source(source: Option<&str>) -> Result<()> {
    if let Some(source) = source
        && chars(source.trim()) > MAX_SOURCE_CHARS
    {
        return Err(RecallError::InvalidInput(format!(
            "source must be at most {MAX_SOURCE_CHARS} characters"
        )));
    }
    Ok(())
}

/// An embedding vector: bounded dimensionality and finite values. NaN or
/// infinities would poison every cosine in the store, so they are rejected at
/// the boundary instead of being persisted.
pub fn validate_embedding(embedding: Option<&[f32]>) -> Result<()> {
    let Some(embedding) = embedding else {
        return Ok(());
    };
    if embedding.is_empty() {
        return Err(RecallError::InvalidInput(
            "embedding must not be empty".into(),
        ));
    }
    if embedding.len() > MAX_EMBEDDING_DIMS {
        return Err(RecallError::InvalidInput(format!(
            "embedding must have at most {MAX_EMBEDDING_DIMS} dimensions"
        )));
    }
    if embedding.iter().any(|v| !v.is_finite()) {
        return Err(RecallError::InvalidInput(
            "embedding values must be finite".into(),
        ));
    }
    Ok(())
}

/// A capture request: message count and total character budget.
pub fn validate_capture(message_count: usize, total_chars: usize) -> Result<()> {
    if message_count == 0 {
        return Err(RecallError::InvalidInput(
            "transcript must contain at least one message".into(),
        ));
    }
    if message_count > MAX_CAPTURE_MESSAGES {
        return Err(RecallError::InvalidInput(format!(
            "transcript must contain at most {MAX_CAPTURE_MESSAGES} messages"
        )));
    }
    if total_chars > MAX_CAPTURE_CHARS {
        return Err(RecallError::InvalidInput(format!(
            "transcript must be at most {MAX_CAPTURE_CHARS} characters"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn namespaces_are_trimmed_bounded_and_control_free() {
        assert!(validate_namespace("work").is_ok());
        assert!(validate_namespace("  ").is_err());
        assert!(validate_namespace(&"n".repeat(MAX_NAMESPACE_CHARS + 1)).is_err());
        assert!(validate_namespace("bad\nname").is_err());
    }

    #[test]
    fn text_and_query_limits_are_enforced_in_characters() {
        assert!(validate_text("hello").is_ok());
        assert!(validate_text("   ").is_err());
        assert!(validate_text(&"x".repeat(MAX_TEXT_CHARS)).is_ok());
        assert!(validate_text(&"x".repeat(MAX_TEXT_CHARS + 1)).is_err());
        // Multi-byte characters count as one each.
        assert!(validate_query(&"é".repeat(MAX_QUERY_CHARS)).is_ok());
        assert!(validate_query(&"é".repeat(MAX_QUERY_CHARS + 1)).is_err());
    }

    #[test]
    fn ids_tags_sources_and_embeddings_are_bounded() {
        assert!(validate_id("id-1").is_ok());
        assert!(validate_id("").is_err());
        assert!(validate_id(&"i".repeat(MAX_ID_CHARS + 1)).is_err());
        assert!(validate_tags(&["a".into(), "b".into()]).is_ok());
        assert!(validate_tags(&vec!["t".into(); MAX_TAGS]).is_ok());
        assert!(validate_tags(&vec!["t".into(); MAX_TAGS + 1]).is_err());
        assert!(validate_tags(&["".into()]).is_err());
        assert!(validate_tags(&["t".repeat(MAX_TAG_CHARS + 1)]).is_err());
        assert!(validate_source(Some("cli")).is_ok());
        assert!(validate_source(Some(&"s".repeat(MAX_SOURCE_CHARS + 1))).is_err());
        assert!(validate_embedding(None).is_ok());
        assert!(validate_embedding(Some(&[0.5, -0.5])).is_ok());
        assert!(validate_embedding(Some(&[])).is_err());
        assert!(validate_embedding(Some(&[f32::NAN])).is_err());
        assert!(validate_embedding(Some(&[f32::INFINITY])).is_err());
        assert!(validate_embedding(Some(&vec![0.0; MAX_EMBEDDING_DIMS + 1])).is_err());
    }

    #[test]
    fn capture_bounds_are_enforced() {
        assert!(validate_capture(1, 10).is_ok());
        assert!(validate_capture(0, 10).is_err());
        assert!(validate_capture(MAX_CAPTURE_MESSAGES + 1, 10).is_err());
        assert!(validate_capture(1, MAX_CAPTURE_CHARS + 1).is_err());
    }
}
