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
        assert_eq!(
            doc["info"]["version"],
            env!("CARGO_PKG_VERSION"),
            "OpenAPI version must follow the crate version"
        );
        // Both auth schemes are documented and applied by default.
        assert_eq!(
            doc["components"]["securitySchemes"]["bearerAuth"]["scheme"],
            "bearer"
        );
        assert_eq!(
            doc["components"]["securitySchemes"]["apiKeyAuth"]["name"],
            "X-API-Key"
        );
        assert!(doc["security"].as_array().unwrap().len() >= 2);
        assert!(
            doc["paths"]["/healthz"]["get"]["security"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        let paths = doc["paths"].as_object().unwrap();
        for path in [
            "/v1/namespaces",
            "/v1/memories",
            "/v1/memories/{id}",
            "/v1/recall",
            "/v1/capture",
            "/v1/stats",
            "/metrics",
            "/healthz",
            "/mcp",
        ] {
            assert!(paths.contains_key(path), "missing {path}");
        }
        // Drift guards: the update endpoint and the recall tag filter must
        // stay documented alongside the core surface.
        assert!(
            paths["/v1/memories/{id}"]["patch"].is_object(),
            "PATCH /v1/memories/{{id}} must be documented"
        );
        assert!(
            paths["/v1/recall"]["post"]["requestBody"]["content"]["application/json"]["schema"]
                ["properties"]["tags"]
                .is_object(),
            "recall tags filter must be documented"
        );
        assert!(doc["components"]["schemas"]["ScoreBreakdown"].is_object());
        assert!(doc["components"]["schemas"]["MemoryUpdate"].is_object());
        // v0.5 compact/full views and the paginated shape.
        for schema in [
            "CompactMemory",
            "CompactRecallHit",
            "MemoryListPage",
            "CaptureSummary",
            "EmbedderInfo",
            "MemoryOrCompact",
            "RecallHitOrFull",
        ] {
            assert!(
                doc["components"]["schemas"][schema].is_object(),
                "missing schema {schema}"
            );
        }
        assert!(
            paths["/v1/memories"]["get"]["parameters"]
                .as_array()
                .unwrap()
                .iter()
                .any(|p| p["name"] == "offset"),
            "list pagination (offset) must be documented"
        );
        assert!(
            paths["/v1/memories"]["get"]["parameters"]
                .as_array()
                .unwrap()
                .iter()
                .any(|p| p["name"] == "include_embedding"),
            "include_embedding must be documented"
        );
        assert!(
            paths["/v1/memories"]["get"]["responses"]["200"]["headers"]["X-Total-Count"]
                .is_object(),
            "X-Total-Count pagination header must be documented"
        );
        assert!(
            paths["/v1/capture"]["post"]["requestBody"]["content"]["application/json"]["schema"]
                ["properties"]["transcript"]
                .is_object(),
            "capture ingest must be documented"
        );
        // Error contract: 401/404/405/413/415/429 shapes.
        for response in [
            "Unauthorized",
            "NotFound",
            "MethodNotAllowed",
            "PayloadTooLarge",
            "UnsupportedMediaType",
            "RateLimited",
        ] {
            assert!(
                doc["components"]["responses"][response].is_object(),
                "missing error response {response}"
            );
        }
        // Every operation can be called with the wrong method; document the
        // 405 (and its Allow header) everywhere rather than only on /v1/stats.
        for (path, operations) in paths {
            for method in ["get", "post", "delete", "patch", "put"] {
                if let Some(operation) = operations.get(method) {
                    assert!(
                        operation["responses"]["405"].is_object(),
                        "{method} {path} must document the JSON 405 response"
                    );
                }
            }
        }
        // MCP doc names all six tools and the 2026-07-28 discover lifecycle.
        let mcp = paths["/mcp"]["post"]["description"].as_str().unwrap();
        for tool in [
            "remember",
            "recall",
            "forget",
            "update_memory",
            "list_memories",
            "capture",
        ] {
            assert!(mcp.contains(tool), "MCP description must name {tool}");
        }
        assert!(mcp.contains("server/discover"), "{mcp}");
    }
}
