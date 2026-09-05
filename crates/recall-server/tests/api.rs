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
    build_router(store, &ServerConfig { api_key, web_dir: None })
}

async fn json_response(app: axum::Router, req: Request<Body>) -> (StatusCode, Value) {
    let res = app.oneshot(req).await.expect("response");
    let status = res.status();
    let bytes = res.into_body().collect().await.expect("body").to_bytes();
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::String(String::from_utf8_lossy(&bytes).to_string()))
    };
    (status, value)
}

fn request(method: &str, uri: &str, body: Option<Value>) -> Request<Body> {
    let mut builder = Request::builder().method(method).uri(uri);
    if body.is_some() {
        builder = builder.header("content-type", "application/json");
    }
    builder.body(Body::from(body.map(|b| b.to_string()).unwrap_or_default())).expect("request")
}

#[tokio::test]
async fn namespace_lifecycle() {
    let app = app_with(None);
    let (status, ns) = json_response(app.clone(), request("POST", "/v1/namespaces", Some(json!({"name": "work"})))).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(ns["name"], "work");

    // Duplicate -> 409
    let (status, err) = json_response(app.clone(), request("POST", "/v1/namespaces", Some(json!({"name": "work"})))).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(err["error"].as_str().unwrap().contains("already exists"));

    // Blank -> 400
    let (status, _) = json_response(app.clone(), request("POST", "/v1/namespaces", Some(json!({"name": "  "})))).await;
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
    let (_, ns) = json_response(app.clone(), request("POST", "/v1/namespaces", Some(json!({"name": "proj"})))).await;

    // 404 for unknown namespace
    let (status, _) = json_response(app.clone(), request("POST", "/v1/memories", Some(json!({"namespace": "ghost", "text": "x"})))).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // 400 for blank text
    let (status, _) = json_response(app.clone(), request("POST", "/v1/memories", Some(json!({"namespace": "proj", "text": "   "})))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // 201 with embedding auto-computed
    let (status, mem) = json_response(
        app.clone(),
        request("POST", "/v1/memories", Some(json!({"namespace": "proj", "text": "sqlite is the storage engine", "tags": ["db"], "pinned": true}))),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let id = mem["id"].as_str().expect("id").to_string();
    assert_eq!(mem["namespace_id"], ns["id"]);
    assert_eq!(mem["pinned"], true);
    assert_eq!(mem["tags"][0], "db");
    assert!(mem["embedding"].as_array().expect("embedding").len() == 256, "HashEmbedder must embed on create");

    // List filtered by namespace
    let (status, list) = json_response(app.clone(), request("GET", "/v1/memories?namespace=proj", None)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list.as_array().unwrap().len(), 1);

    // Delete -> 204 then 404
    let (status, _) = json_response(app.clone(), request("DELETE", &format!("/v1/memories/{id}"), None)).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = json_response(app.clone(), request("DELETE", &format!("/v1/memories/{id}"), None)).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // limit=0 -> 400
    let (status, _) = json_response(app.clone(), request("GET", "/v1/memories?limit=0", None)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn recall_returns_explainable_ranked_hits() {
    let app = app_with(None);
    json_response(app.clone(), request("POST", "/v1/namespaces", Some(json!({"name": "eng"})))).await;
    for text in [
        "we deploy the api on thursdays",
        "postgres connection pool size is 20",
        "the staging database lives in the vm",
    ] {
        let (status, _) = json_response(app.clone(), request("POST", "/v1/memories", Some(json!({"namespace": "eng", "text": text})))).await;
        assert_eq!(status, StatusCode::CREATED);
    }

    // 404 for unknown namespace
    let (status, _) = json_response(app.clone(), request("POST", "/v1/recall", Some(json!({"namespace": "nope", "query": "x"})))).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // 400 for bad k
    let (status, _) = json_response(app.clone(), request("POST", "/v1/recall", Some(json!({"namespace": "eng", "query": "x", "k": 0})))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, hits) = json_response(app.clone(), request("POST", "/v1/recall", Some(json!({"namespace": "eng", "query": "when do we deploy the api?", "k": 3})))).await;
    assert_eq!(status, StatusCode::OK);
    let hits = hits.as_array().expect("array response per ARCHITECTURE.md");
    assert!(!hits.is_empty());
    assert!(hits.len() <= 3);
    let top = &hits[0];
    assert!(top["text"].as_str().unwrap().contains("deploy"), "top hit should be the deploy memory");
    let bd = &top["breakdown"];
    assert!(bd["bm25"].as_f64().unwrap() > 0.0, "keyword score must be present");
    // The breakdown must decompose exactly per ARCHITECTURE.md's formula.
    let expected_total = 0.45f64.mul_add(bd["bm25"].as_f64().unwrap(), 0.45 * bd["vector"].as_f64().unwrap())
        + 0.10 * bd["recency"].as_f64().unwrap()
        + bd["pinned_boost"].as_f64().unwrap();
    assert!((bd["total"].as_f64().unwrap() - expected_total).abs() < 1e-9);
    assert!(bd["total"].as_f64().unwrap() > 0.0);
    // Determinism
    let (_, hits2) = json_response(app.clone(), request("POST", "/v1/recall", Some(json!({"namespace": "eng", "query": "when do we deploy the api?", "k": 3})))).await;
    assert_eq!(hits, hits2.as_array().unwrap());

    // decay disabled zeroes the recency component
    let (_, hits) = json_response(app.clone(), request("POST", "/v1/recall", Some(json!({"namespace": "eng", "query": "deploy", "k": 1, "tau_days": null})))).await;
    assert_eq!(hits[0]["breakdown"]["recency"], 0.0);
}

#[tokio::test]
async fn stats_and_healthz_and_openapi() {
    let app = app_with(None);
    json_response(app.clone(), request("POST", "/v1/namespaces", Some(json!({"name": "s"})))).await;
    json_response(app.clone(), request("POST", "/v1/memories", Some(json!({"namespace": "s", "text": "hello world", "pinned": true})))).await;

    let (status, health) = json_response(app.clone(), request("GET", "/healthz", None)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(health["status"], "ok");

    let (status, stats) = json_response(app.clone(), request("GET", "/v1/stats", None)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(stats["total_namespaces"], 1);
    assert_eq!(stats["total_memories"], 1);
    assert_eq!(stats["pinned_memories"], 1);
    assert_eq!(stats["per_namespace"][0]["count"], 1);

    let res = app.clone().oneshot(request("GET", "/openapi.json", None)).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert!(res.headers()["content-type"].to_str().unwrap().starts_with("application/json"));
}

#[tokio::test]
async fn api_key_auth_is_enforced_when_configured() {
    let app = app_with(Some("secret-key".to_string()));

    // Missing key -> 401 with WWW-Authenticate
    let res = app.clone().oneshot(request("GET", "/v1/namespaces", None)).await.unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(res.headers()["www-authenticate"], "Bearer");

    // Wrong key -> 401
    let mut req = request("GET", "/v1/namespaces", None);
    req.headers_mut().insert("authorization", "Bearer wrong".parse().unwrap());
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);

    // Correct bearer -> 200
    let mut req = request("POST", "/v1/namespaces", Some(json!({"name": "authed"})));
    req.headers_mut().insert("authorization", "Bearer secret-key".parse().unwrap());
    let (status, _) = json_response(app.clone(), req).await;
    assert_eq!(status, StatusCode::CREATED);

    // Correct X-API-Key -> 200
    let mut req = request("GET", "/v1/namespaces", None);
    req.headers_mut().insert("x-api-key", "secret-key".parse().unwrap());
    let (status, list) = json_response(app.clone(), req).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list[0]["name"], "authed");

    // healthz and openapi stay open (probes / spec discovery)
    let (status, _) = json_response(app.clone(), request("GET", "/healthz", None)).await;
    assert_eq!(status, StatusCode::OK);
    let res = app.oneshot(request("GET", "/openapi.json", None)).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
}
