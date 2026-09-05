//! MCP conformance smoke tests over the streamable-HTTP transport:
//! initialize -> tools/list -> tools/call, plus API-key behavior on /mcp.

use axum::body::Body;
use axum::http::Request;
use http_body_util::BodyExt;
use recall_core::shared;
use recall_server::{ServerConfig, build_router};
use serde_json::{Value, json};
use tower::ServiceExt;

fn app_with(api_key: Option<String>) -> axum::Router {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = shared(recall_core::SqliteStore::open(dir.path().join("mcp.db")).expect("db"));
    std::mem::forget(dir);
    build_router(
        store,
        &ServerConfig {
            api_key,
            web_dir: None,
        },
    )
}

async fn rpc(app: &axum::Router, payload: Value) -> (axum::http::StatusCode, Value) {
    let res = app
        .clone()
        .oneshot(
            Request::post("/mcp")
                .header("content-type", "application/json")
                .header("accept", "application/json, text/event-stream")
                .header("host", "localhost")
                .body(Body::from(payload.to_string()))
                .unwrap(),
        )
        .await
        .expect("response");
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

fn initialize_request() -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2026-07-28",
            "capabilities": {},
            "clientInfo": { "name": "recall-conformance", "version": "0.0.1" }
        }
    })
}

fn tool_call(id: u64, name: &str, args: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "tools/call",
        "params": { "name": name, "arguments": args }
    })
}

fn unwrap_result(body: &Value) -> &Value {
    body.get("result")
        .unwrap_or_else(|| panic!("JSON-RPC result expected, got: {body}"))
}

fn tool_text(result: &Value) -> Value {
    let text = result["content"][0]["text"].as_str().expect("text content");
    serde_json::from_str(text).expect("tool output must be valid JSON")
}

#[tokio::test]
async fn conformance_initialize_tools_list_call_over_http() {
    let app = app_with(None);

    // 1) initialize — server identifies itself and its capabilities.
    let (status, body) = rpc(&app, initialize_request()).await;
    assert_eq!(status, axum::http::StatusCode::OK);
    let result = unwrap_result(&body);
    assert_eq!(result["serverInfo"]["name"], "recall-mcp");
    assert!(result["protocolVersion"].as_str().is_some());
    assert!(
        result["capabilities"]["tools"].is_object(),
        "tools capability must be advertised"
    );

    // 2) tools/list — exactly the four documented tools, each with an input schema.
    let (status, body) = rpc(
        &app,
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    let tools = unwrap_result(&body)["tools"]
        .as_array()
        .expect("tools array");
    let mut names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    names.sort();
    assert_eq!(names, vec!["forget", "list_memories", "recall", "remember"]);
    for tool in tools {
        assert!(
            tool["inputSchema"].is_object(),
            "every tool needs an input schema"
        );
        assert!(tool["description"].as_str().is_some());
    }

    // 3) tools/call remember -> memory stored and returned.
    let (status, body) = rpc(
        &app,
        tool_call(3, "remember", json!({"namespace": "conformance", "text": "the mcp transport speaks json-rpc", "tags": ["mcp"]})),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert_eq!(unwrap_result(&body)["isError"], false);
    let stored = tool_text(unwrap_result(&body));
    let memory_id = stored["id"].as_str().expect("stored memory id").to_string();
    assert_eq!(stored["text"], "the mcp transport speaks json-rpc");

    // 4) tools/call recall -> ranked hit with breakdown.
    let (status, body) = rpc(
        &app,
        tool_call(
            4,
            "recall",
            json!({"namespace": "conformance", "query": "json-rpc transport", "k": 2}),
        ),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    let hits = tool_text(unwrap_result(&body));
    assert_eq!(hits.as_array().unwrap().len(), 1);
    assert_eq!(hits[0]["id"], memory_id.as_str());
    assert!(hits[0]["breakdown"]["total"].as_f64().unwrap() > 0.0);

    // 5) tools/call list_memories -> still present.
    let (_, body) = rpc(
        &app,
        tool_call(5, "list_memories", json!({"namespace": "conformance"})),
    )
    .await;
    let memories = tool_text(unwrap_result(&body));
    assert_eq!(memories.as_array().unwrap().len(), 1);

    // 6) tools/call forget -> deleted: true, then list is empty.
    let (_, body) = rpc(&app, tool_call(6, "forget", json!({"id": memory_id}))).await;
    let forgotten = tool_text(unwrap_result(&body));
    assert_eq!(forgotten["deleted"], true);

    let (_, body) = rpc(
        &app,
        tool_call(7, "list_memories", json!({"namespace": "conformance"})),
    )
    .await;
    assert_eq!(tool_text(unwrap_result(&body)).as_array().unwrap().len(), 0);

    // 7) unknown tool -> JSON-RPC protocol error (invalid params / tool missing).
    let (status, body) = rpc(&app, tool_call(8, "nonexistent", json!({}))).await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert!(
        body["error"].is_object(),
        "unknown tool must be a JSON-RPC error: {body}"
    );

    // 8) caller-fixable tool failure -> isError result with message.
    let (_, body) = rpc(
        &app,
        tool_call(
            9,
            "recall",
            json!({"namespace": "ghost-namespace", "query": "anything"}),
        ),
    )
    .await;
    let err = &body["error"];
    assert!(
        err["message"]
            .as_str()
            .unwrap()
            .contains("namespace not found"),
        "{body}"
    );
}

#[tokio::test]
async fn mcp_endpoint_honors_api_key() {
    let app = app_with(Some("mcp-secret".to_string()));

    // No key -> 401, no protocol data leaks.
    let (status, body) = rpc(&app, initialize_request()).await;
    assert_eq!(status, axum::http::StatusCode::UNAUTHORIZED);
    assert!(body["error"].as_str().is_some());

    // Wrong key -> 401.
    let res = app
        .clone()
        .oneshot(
            Request::post("/mcp")
                .header("content-type", "application/json")
                .header("accept", "application/json")
                .header("host", "localhost")
                .header("x-api-key", "nope")
                .body(Body::from(initialize_request().to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), axum::http::StatusCode::UNAUTHORIZED);

    // Correct key -> full initialize succeeds.
    let res = app
        .clone()
        .oneshot(
            Request::post("/mcp")
                .header("content-type", "application/json")
                .header("accept", "application/json, text/event-stream")
                .header("host", "localhost")
                .header("authorization", "Bearer mcp-secret")
                .body(Body::from(initialize_request().to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), axum::http::StatusCode::OK);
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(unwrap_result(&body)["serverInfo"]["name"], "recall-mcp");
}
