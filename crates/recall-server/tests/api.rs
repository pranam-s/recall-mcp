//! HTTP API integration tests via the axum/tower test client against real
//! (temp-file) SQLite stores.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use recall_core::shared;
use recall_server::{ServerConfig, build_router};
use serde_json::{Value, json};
use tower::ServiceExt;

fn app_with(api_key: Option<String>) -> axum::Router {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = shared(recall_core::SqliteStore::open(dir.path().join("test.db")).expect("db"));
    // Leak the tempdir so the db outlives the test (small, test-only).
    std::mem::forget(dir);
    build_router(
        store,
        &ServerConfig {
            api_key,
            web_dir: None,
            ..Default::default()
        },
    )
}

async fn json_response(app: axum::Router, req: Request<Body>) -> (StatusCode, Value) {
    let res = app.oneshot(req).await.expect("response");
    let status = res.status();
    let bytes = res.into_body().collect().await.expect("body").to_bytes();
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes)
            .unwrap_or(Value::String(String::from_utf8_lossy(&bytes).to_string()))
    };
    (status, value)
}

fn request(method: &str, uri: &str, body: Option<Value>) -> Request<Body> {
    let mut builder = Request::builder().method(method).uri(uri);
    if body.is_some() {
        builder = builder.header("content-type", "application/json");
    }
    builder
        .body(Body::from(body.map(|b| b.to_string()).unwrap_or_default()))
        .expect("request")
}

#[tokio::test]
async fn namespace_lifecycle() {
    let app = app_with(None);
    let (status, ns) = json_response(
        app.clone(),
        request("POST", "/v1/namespaces", Some(json!({"name": "work"}))),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(ns["name"], "work");

    // Duplicate -> 409
    let (status, err) = json_response(
        app.clone(),
        request("POST", "/v1/namespaces", Some(json!({"name": "work"}))),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(err["error"].as_str().unwrap().contains("already exists"));

    // Blank -> 400
    let (status, _) = json_response(
        app.clone(),
        request("POST", "/v1/namespaces", Some(json!({"name": "  "}))),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // List
    let (status, list) = json_response(app.clone(), request("GET", "/v1/namespaces", None)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list.as_array().unwrap().len(), 1);
    assert_eq!(list[0]["name"], "work");
}

#[tokio::test]
async fn memory_create_list_delete_with_validation() {
    let app = app_with(None);
    let (_, ns) = json_response(
        app.clone(),
        request("POST", "/v1/namespaces", Some(json!({"name": "proj"}))),
    )
    .await;

    // 404 for unknown namespace
    let (status, _) = json_response(
        app.clone(),
        request(
            "POST",
            "/v1/memories",
            Some(json!({"namespace": "ghost", "text": "x"})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // 400 for blank text
    let (status, _) = json_response(
        app.clone(),
        request(
            "POST",
            "/v1/memories",
            Some(json!({"namespace": "proj", "text": "   "})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // 201 with embedding auto-computed
    let (status, mem) = json_response(
        app.clone(),
        request("POST", "/v1/memories?include_embedding=true", Some(json!({"namespace": "proj", "text": "sqlite is the storage engine", "tags": ["db"], "pinned": true}))),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let id = mem["id"].as_str().expect("id").to_string();
    assert_eq!(mem["namespace_id"], ns["id"]);
    assert_eq!(mem["pinned"], true);
    assert_eq!(mem["tags"][0], "db");
    assert!(
        mem["embedding"].as_array().expect("embedding").len() == 256,
        "HashEmbedder must embed on create"
    );

    // AR-005: a caller-supplied vector whose length differs from the
    // configured embedder's dims would silently zero every cosine — reject
    // with 400 instead of accepting silent scoring loss.
    let (status, body) = json_response(
        app.clone(),
        request(
            "POST",
            "/v1/memories",
            Some(
                json!({"namespace": "proj", "text": "mismatched vector", "embedding": [0.1, 0.2]}),
            ),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        body["error"].as_str().unwrap().contains("dims"),
        "the error must name the dimension mismatch: {body}"
    );
    // A correctly-sized caller vector is accepted (import-style creates).
    let matched = vec![0.5f64; 256];
    let (status, _) = json_response(
        app.clone(),
        request(
            "POST",
            "/v1/memories",
            Some(json!({"namespace": "proj", "text": "matched vector", "embedding": matched})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    // List filtered by namespace (auto-embedded + matched-vector rows; the
    // mismatched one was rejected before storage)
    let (status, list) = json_response(
        app.clone(),
        request("GET", "/v1/memories?namespace=proj", None),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list["items"].as_array().unwrap().len(), 2);
    assert_eq!(list["total"], 2);

    // Delete -> 204 then 404
    let (status, _) = json_response(
        app.clone(),
        request("DELETE", &format!("/v1/memories/{id}"), None),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = json_response(
        app.clone(),
        request("DELETE", &format!("/v1/memories/{id}"), None),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // limit=0 -> 400
    let (status, _) =
        json_response(app.clone(), request("GET", "/v1/memories?limit=0", None)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn recall_returns_explainable_ranked_hits() {
    let app = app_with(None);
    json_response(
        app.clone(),
        request("POST", "/v1/namespaces", Some(json!({"name": "eng"}))),
    )
    .await;
    for text in [
        "we deploy the api on thursdays",
        "postgres connection pool size is 20",
        "the staging database lives in the vm",
    ] {
        let (status, _) = json_response(
            app.clone(),
            request(
                "POST",
                "/v1/memories",
                Some(json!({"namespace": "eng", "text": text})),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
    }

    // 404 for unknown namespace
    let (status, _) = json_response(
        app.clone(),
        request(
            "POST",
            "/v1/recall",
            Some(json!({"namespace": "nope", "query": "x"})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // 400 for bad k
    let (status, _) = json_response(
        app.clone(),
        request(
            "POST",
            "/v1/recall",
            Some(json!({"namespace": "eng", "query": "x", "k": 0})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, hits) = json_response(
        app.clone(),
        request(
            "POST",
            "/v1/recall",
            Some(json!({"namespace": "eng", "query": "when do we deploy the api?", "k": 3})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let hits = hits.as_array().expect("array response per ARCHITECTURE.md");
    assert!(!hits.is_empty());
    assert!(hits.len() <= 3);
    let top = &hits[0];
    assert!(
        top["text"].as_str().unwrap().contains("deploy"),
        "top hit should be the deploy memory"
    );
    let bd = &top["breakdown"];
    assert!(
        bd["bm25"].as_f64().unwrap() > 0.0,
        "keyword score must be present"
    );
    // The breakdown must decompose exactly per ARCHITECTURE.md's formula.
    let expected_total = 0.45f64.mul_add(
        bd["bm25"].as_f64().unwrap(),
        0.45 * bd["vector"].as_f64().unwrap(),
    ) + 0.10 * bd["recency"].as_f64().unwrap()
        + bd["pinned_boost"].as_f64().unwrap();
    assert!((bd["total"].as_f64().unwrap() - expected_total).abs() < 1e-9);
    assert!(bd["total"].as_f64().unwrap() > 0.0);
    // Determinism
    let (_, hits2) = json_response(
        app.clone(),
        request(
            "POST",
            "/v1/recall",
            Some(json!({"namespace": "eng", "query": "when do we deploy the api?", "k": 3})),
        ),
    )
    .await;
    assert_eq!(hits, hits2.as_array().unwrap());

    // decay disabled zeroes the recency component
    let (_, hits) = json_response(
        app.clone(),
        request(
            "POST",
            "/v1/recall",
            Some(json!({"namespace": "eng", "query": "deploy", "k": 1, "tau_days": null})),
        ),
    )
    .await;
    assert_eq!(hits[0]["breakdown"]["recency"], 0.0);
}

#[tokio::test]
async fn memory_update_and_recall_tag_filter() {
    let app = app_with(None);

    // Seed a namespace and two memories.
    let (status, _) = json_response(
        app.clone(),
        request("POST", "/v1/namespaces", Some(json!({"name": "upd"}))),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (_, mem_a) = json_response(
        app.clone(),
        request(
            "POST",
            "/v1/memories",
            Some(json!({
                "namespace": "upd",
                "text": "postgres wal tuning notes",
                "tags": ["db", "postgres"]
            })),
        ),
    )
    .await;
    let id_a = mem_a["id"].as_str().unwrap().to_string();
    let (_, mem_b) = json_response(
        app.clone(),
        request(
            "POST",
            "/v1/memories",
            Some(json!({
                "namespace": "upd",
                "text": "rust async runtime deep dive",
                "tags": ["rust"]
            })),
        ),
    )
    .await;
    assert_eq!(mem_b["tags"], json!(["rust"]));

    // PATCH: revise text and tags; the record embeds again server-side.
    let (status, updated) = json_response(
        app.clone(),
        request(
            "PATCH",
            &format!("/v1/memories/{id_a}?include_embedding=true"),
            Some(json!({
                "text": "postgres vacuum and autovacuum tuning",
                "tags": ["db", "maintenance"]
            })),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "patch failed: {updated}");
    assert_eq!(updated["id"], id_a.as_str());
    assert_eq!(updated["text"], "postgres vacuum and autovacuum tuning");
    assert_eq!(updated["tags"], json!(["db", "maintenance"]));
    assert!(updated["embedding"].is_array(), "text change must re-embed");

    // PATCH with unknown id -> 404; blank text -> 400; empty body -> 400.
    let (status, _) = json_response(
        app.clone(),
        request("PATCH", "/v1/memories/ghost", Some(json!({"pinned": true}))),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = json_response(
        app.clone(),
        request(
            "PATCH",
            &format!("/v1/memories/{id_a}"),
            Some(json!({"text": "  "})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = json_response(
        app.clone(),
        request("PATCH", &format!("/v1/memories/{id_a}"), Some(json!({}))),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // Tag filter on recall: ALL-of semantics.
    let (status, hits) = json_response(
        app.clone(),
        request(
            "POST",
            "/v1/recall",
            Some(json!({"namespace": "upd", "query": "postgres", "tags": ["db"]})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(hits.as_array().unwrap().len(), 1);
    assert_eq!(hits[0]["id"], id_a.as_str());

    // No memory carries both tags anymore after the tag replacement.
    let (_, hits) = json_response(
        app.clone(),
        request(
            "POST",
            "/v1/recall",
            Some(json!({"namespace": "upd", "query": "postgres", "tags": ["db", "postgres"]})),
        ),
    )
    .await;
    assert_eq!(hits.as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn stats_and_healthz_and_openapi() {
    let app = app_with(None);
    json_response(
        app.clone(),
        request("POST", "/v1/namespaces", Some(json!({"name": "s"}))),
    )
    .await;
    json_response(
        app.clone(),
        request(
            "POST",
            "/v1/memories",
            Some(json!({"namespace": "s", "text": "hello world", "pinned": true})),
        ),
    )
    .await;

    let (status, health) = json_response(app.clone(), request("GET", "/healthz", None)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(health["status"], "ok");

    let (status, stats) = json_response(app.clone(), request("GET", "/v1/stats", None)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(stats["total_namespaces"], 1);
    assert_eq!(stats["total_memories"], 1);
    assert_eq!(stats["pinned_memories"], 1);
    assert_eq!(stats["per_namespace"][0]["count"], 1);
    assert_eq!(stats["embedder"]["name"], "hash-256");
    assert_eq!(stats["embedder"]["dimensions"], 256);
    assert_eq!(
        stats["embedder"]["fingerprint"], "hash-256@256",
        "stats must expose the inner embedder's stable fingerprint"
    );

    let res = app
        .clone()
        .oneshot(request("GET", "/openapi.json", None))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert!(
        res.headers()["content-type"]
            .to_str()
            .unwrap()
            .starts_with("application/json")
    );
}

#[tokio::test]
async fn api_key_auth_is_enforced_when_configured() {
    let app = app_with(Some("secret-key".to_string()));

    // Missing key -> 401 with WWW-Authenticate
    let res = app
        .clone()
        .oneshot(request("GET", "/v1/namespaces", None))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(res.headers()["www-authenticate"], "Bearer");

    // Wrong key -> 401
    let mut req = request("GET", "/v1/namespaces", None);
    req.headers_mut()
        .insert("authorization", "Bearer wrong".parse().unwrap());
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);

    // Correct bearer -> 200
    let mut req = request("POST", "/v1/namespaces", Some(json!({"name": "authed"})));
    req.headers_mut()
        .insert("authorization", "Bearer secret-key".parse().unwrap());
    let (status, _) = json_response(app.clone(), req).await;
    assert_eq!(status, StatusCode::CREATED);

    // Correct X-API-Key -> 200
    let mut req = request("GET", "/v1/namespaces", None);
    req.headers_mut()
        .insert("x-api-key", "secret-key".parse().unwrap());
    let (status, list) = json_response(app.clone(), req).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list[0]["name"], "authed");

    // healthz and openapi stay open (probes / spec discovery)
    let (status, _) = json_response(app.clone(), request("GET", "/healthz", None)).await;
    assert_eq!(status, StatusCode::OK);
    let res = app
        .oneshot(request("GET", "/openapi.json", None))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
}

#[tokio::test]
async fn list_memories_paginates_with_total_count_header() {
    let app = app_with(None);
    json_response(
        app.clone(),
        request("POST", "/v1/namespaces", Some(json!({"name": "paged"}))),
    )
    .await;
    for i in 0..5 {
        let (status, _) = json_response(
            app.clone(),
            request(
                "POST",
                "/v1/memories",
                Some(json!({"namespace": "paged", "text": format!("note {i}"), "created_at": i})),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
    }

    let res = app
        .clone()
        .oneshot(request(
            "GET",
            "/v1/memories?namespace=paged&limit=2&offset=0",
            None,
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(
        res.headers()
            .get("x-total-count")
            .and_then(|v| v.to_str().ok()),
        Some("5"),
        "X-Total-Count must carry the unpaginated total"
    );
    let (status, page1) = json_response(
        app.clone(),
        request("GET", "/v1/memories?namespace=paged&limit=2&offset=0", None),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(page1["items"].as_array().unwrap().len(), 2);
    assert_eq!(page1["total"], 5);
    assert_eq!(page1["limit"], 2);
    assert_eq!(page1["offset"], 0);
    assert_eq!(page1["has_more"], true);
    let (status, page3) = json_response(
        app.clone(),
        request("GET", "/v1/memories?namespace=paged&limit=2&offset=4", None),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(page3["items"].as_array().unwrap().len(), 1);
    assert_eq!(page3["has_more"], false);
    // Pages tile the set without overlap (newest first).
    let ids: Vec<&str> = page1["items"]
        .as_array()
        .unwrap()
        .iter()
        .chain(page3["items"].as_array().unwrap())
        .map(|m| m["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids.len(), 3);
    let unique: std::collections::HashSet<&str> = ids.iter().copied().collect();
    assert_eq!(unique.len(), 3, "pages must not overlap");
}

#[tokio::test]
async fn capture_ingests_and_auto_chunks_a_transcript() {
    let app = app_with(None);
    let long = "This decision matters. ".repeat(80); // ~1760 chars
    let (status, report) = json_response(
        app.clone(),
        request(
            "POST",
            "/v1/capture?include_memories=true&include_embedding=true",
            Some(json!({
                "namespace": "session-42",
                "transcript": [
                    {"role": "user", "content": "please deploy the payments service on friday", "at": 1_786_000_000},
                    {"role": "assistant", "content": long}
                ],
                "tags": ["session:42"]
            })),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{report}");
    assert_eq!(report["namespace"], "session-42");
    let captured = report["captured"].as_u64().unwrap();
    assert!(captured >= 3, "long message must be chunked: {report}");
    assert_eq!(
        report["memories"].as_array().unwrap().len() as u64,
        captured
    );
    // Every memory carries the session tag; roles became role:* tags.
    for memory in report["memories"].as_array().unwrap() {
        let tags: Vec<&str> = memory["tags"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t.as_str().unwrap())
            .collect();
        assert!(tags.contains(&"session:42"), "{tags:?}");
        assert!(tags.contains(&"role:user") || tags.contains(&"role:assistant"));
        assert_eq!(memory["source"], "capture");
        assert_eq!(
            memory["namespace_id"],
            report["memories"][0]["namespace_id"]
        );
    }

    // Empty transcript and blank namespace are rejected.
    let (status, _) = json_response(
        app.clone(),
        request(
            "POST",
            "/v1/capture",
            Some(json!({"namespace": "x", "transcript": []})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = json_response(
        app.clone(),
        request(
            "POST",
            "/v1/capture",
            Some(json!({"namespace": "  ", "transcript": [{"content": "hi"}]})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // Captured text is retrievable through the normal hybrid recall path.
    let (status, hits) = json_response(
        app.clone(),
        request(
            "POST",
            "/v1/recall",
            Some(json!({"namespace": "session-42", "query": "deploy payments friday"})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        hits.as_array().unwrap().iter().any(|h| {
            h["text"]
                .as_str()
                .unwrap()
                .contains("deploy the payments service")
        }),
        "captured memory must be recallable: {hits}"
    );
}

#[tokio::test]
async fn framework_rejections_are_json_with_the_documented_statuses() {
    let app = app_with(None);
    let send = |req: Request<Body>| {
        let app = app.clone();
        async move {
            let res = app.oneshot(req).await.expect("response");
            let status = res.status();
            let content_type = res
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .unwrap_or_default()
                .to_string();
            let bytes = res.into_body().collect().await.expect("body").to_bytes();
            let body: Value =
                serde_json::from_slice(&bytes).expect("every error body must be JSON");
            (status, content_type, body)
        }
    };

    // Malformed JSON.
    let malformed = Request::builder()
        .method("POST")
        .uri("/v1/memories")
        .header("content-type", "application/json")
        .body(Body::from("{\"namespace\":"))
        .unwrap();
    let (status, content_type, body) = send(malformed).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        content_type.starts_with("application/json"),
        "{content_type}"
    );
    assert!(body["error"].is_string(), "{body}");

    // Missing/wrong content type.
    let no_type = Request::builder()
        .method("POST")
        .uri("/v1/memories")
        .body(Body::from(r#"{"namespace":"w","text":"x"}"#))
        .unwrap();
    let (status, _, body) = send(no_type).await;
    assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    assert!(body["error"].is_string(), "{body}");
    let text_type = Request::builder()
        .method("POST")
        .uri("/v1/memories")
        .header("content-type", "text/plain")
        .body(Body::from("hello"))
        .unwrap();
    let (status, _, body) = send(text_type).await;
    assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    assert!(body["error"].is_string(), "{body}");

    // Unknown path and wrong method.
    let (status, _, body) = send(request("GET", "/nope", None)).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(body["error"].is_string(), "{body}");
    let (status, _, body) = send(request("PUT", "/v1/stats", Some(json!({})))).await;
    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
    assert!(body["error"].is_string(), "{body}");

    // Bad query parameter.
    let (status, _, body) = send(request("GET", "/v1/memories?limit=not-a-number", None)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["error"].is_string(), "{body}");
}

#[tokio::test]
async fn oversize_body_and_security_headers_are_json_and_hardened() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = shared(recall_core::SqliteStore::open(dir.path().join("limits.db")).expect("db"));
    std::mem::forget(dir);
    let app = build_router(
        store,
        &ServerConfig {
            body_limit_bytes: 128,
            ..Default::default()
        },
    );
    let big = "x".repeat(1024);
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/memories")
                .header("content-type", "application/json")
                .body(Body::from(format!(r#"{{"namespace":"w","text":"{big}"}}"#)))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert!(
        res.headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .starts_with("application/json")
    );

    let res = app
        .oneshot(request("GET", "/v1/stats", None))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let headers = res.headers();
    let csp = headers
        .get("content-security-policy")
        .and_then(|v| v.to_str().ok())
        .expect("CSP");
    assert!(csp.contains("frame-ancestors 'none'"), "{csp}");
    assert_eq!(headers.get("x-content-type-options").unwrap(), "nosniff");
    assert_eq!(headers.get("x-frame-options").unwrap(), "DENY");
    assert_eq!(headers.get("referrer-policy").unwrap(), "no-referrer");
    assert_eq!(
        headers.get("cross-origin-opener-policy").unwrap(),
        "same-origin"
    );
    assert_eq!(
        headers.get("cross-origin-resource-policy").unwrap(),
        "same-origin"
    );
    assert!(headers.get("permissions-policy").is_some());
    assert_eq!(headers.get("cache-control").unwrap(), "no-store");
}

#[tokio::test]
async fn origin_scheme_must_match_and_default_ports_are_normalized() {
    let app = app_with(None);
    // Same host and port but https Origin on an http request: cross-origin.
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/stats")
                .header("host", "127.0.0.1:8787")
                .header("origin", "https://127.0.0.1:8787")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
    // An allowed origin without an explicit port equals the scheme default.
    let dir = tempfile::tempdir().expect("tempdir");
    let store = shared(recall_core::SqliteStore::open(dir.path().join("origin.db")).expect("db"));
    std::mem::forget(dir);
    let app = build_router(
        store,
        &ServerConfig {
            allowed_origins: vec!["https://agent.example".into()],
            ..Default::default()
        },
    );
    let res = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/stats")
                .header("host", "127.0.0.1:8787")
                .header("origin", "https://AGENT.EXAMPLE:443")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
}

#[tokio::test]
async fn capture_rolls_back_namespace_and_chunks_on_failure() {
    // A capture whose second chunk collides on id must leave no namespace
    // and no first chunk behind: the whole ingest is one transaction.
    let dir = tempfile::tempdir().expect("tempdir");
    let store = shared(recall_core::SqliteStore::open(dir.path().join("rollback.db")).expect("db"));
    std::mem::forget(dir);
    let prepared = recall_server::capture::PreparedCapture {
        namespace: "half-written".into(),
        memories: vec![
            recall_core::NewMemory {
                namespace: "half-written".into(),
                text: "first chunk".into(),
                tags: vec![],
                source: Some("capture".into()),
                pinned: false,
                created_at: None,
                id: Some("same-id".into()),
                embedding: None,
            },
            recall_core::NewMemory {
                namespace: "half-written".into(),
                text: "second chunk".into(),
                tags: vec![],
                source: Some("capture".into()),
                pinned: false,
                created_at: None,
                id: Some("same-id".into()),
                embedding: None,
            },
        ],
    };
    let outcome = {
        let guard = store.lock().unwrap();
        recall_server::capture::commit(&*guard, &prepared)
    };
    assert!(outcome.is_err(), "duplicate ids must fail the capture");
    let guard = store.lock().unwrap();
    assert!(
        guard.find_namespace("half-written").unwrap().is_none(),
        "the namespace creation must roll back with the failed inserts"
    );
    assert_eq!(guard.count_memories(None).unwrap(), 0);
}
