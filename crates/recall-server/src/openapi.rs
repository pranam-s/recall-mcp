//! The OpenAPI document served at `/openapi.json`.
//!
//! Hand-written (ADR-007): six endpoints, versioned with the crate; utoipa's
//! derive stack buys nothing here and pulls heavy macro machinery.

pub const DOCUMENT: &str = include_str!("openapi.json");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn document_is_valid_json_describing_this_api() {
        let doc: serde_json::Value =
            serde_json::from_str(DOCUMENT).expect("openapi.json must parse");
        assert_eq!(doc["openapi"], "3.1.0");
        let paths = doc["paths"].as_object().unwrap();
        for path in [
            "/v1/namespaces",
            "/v1/memories",
            "/v1/memories/{id}",
            "/v1/recall",
            "/v1/stats",
            "/healthz",
        ] {
            assert!(paths.contains_key(path), "missing {path}");
        }
        assert!(doc["components"]["schemas"]["ScoreBreakdown"].is_object());
    }
}
