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
    app_with_config(ServerConfig {
        api_key,
        web_dir: None,
        ..Default::default()
    })
}

fn app_with_config(config: ServerConfig) -> axum::Router {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = shared(recall_core::SqliteStore::open(dir.path().join("mcp.db")).expect("db"));
    std::mem::forget(dir);
    build_router(store, &config)
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

/// 2026-07-28 per-request metadata (SEP-2575): every non-initialize request
/// carries the protocol version and client capabilities in `_meta`.
fn draft_meta() -> Value {
    json!({
        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientCapabilities": {}
    })
}

async fn rpc_stateless(app: &axum::Router, payload: Value) -> (axum::http::StatusCode, Value) {
    let method = payload["method"].as_str().expect("method").to_string();
    let mut builder = Request::post("/mcp")
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
        .header("host", "localhost")
        .header("mcp-protocol-version", "2026-07-28")
        .header("mcp-method", method.clone());
    if method == "tools/call" {
        builder = builder.header(
            "mcp-name",
            payload["params"]["name"].as_str().expect("tool name"),
        );
    }
    let res = app
        .clone()
        .oneshot(builder.body(Body::from(payload.to_string())).unwrap())
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

#[tokio::test]
async fn compact_and_text_responses_stay_smaller_than_full_records() {
    let app = app_with(None);
    let remember = |id: u64, extra: Value| {
        let mut args = json!({
            "namespace": "budget",
            "text": "deploy the payments service on friday",
        });
        if let Some(object) = extra.as_object() {
            for (k, v) in object {
                args[k] = v.clone();
            }
        }
        tool_call(id, "remember", args)
    };
    let (_, body) = rpc(&app, remember(1, json!({}))).await;
    let compact = unwrap_result(&body).to_string();
    let (_, body) = rpc(&app, remember(2, json!({"include_embedding": true}))).await;
    let full = unwrap_result(&body).to_string();
    eprintln!("remember compact={} full={}", compact.len(), full.len());
    assert!(
        full.len() > compact.len() * 3,
        "full record should be much larger (embedding vector)"
    );

    let recall = |id: u64, extra: Value| {
        let mut args = json!({"namespace": "budget", "query": "payments friday"});
        if let Some(object) = extra.as_object() {
            for (k, v) in object {
                args[k] = v.clone();
            }
        }
        tool_call(id, "recall", args)
    };
    let (_, body) = rpc(&app, recall(3, json!({}))).await;
    let compact_hits = unwrap_result(&body).to_string();
    let (_, body) = rpc(&app, recall(4, json!({"include_embedding": true}))).await;
    let full_hits = unwrap_result(&body).to_string();
    let (_, body) = rpc(&app, recall(5, json!({"format": "text"}))).await;
    let text = unwrap_result(&body)["content"][0]["text"]
        .as_str()
        .unwrap()
        .to_string();
    eprintln!(
        "recall compact={} full={} text={}",
        compact_hits.len(),
        full_hits.len(),
        text.len()
    );
    assert!(full_hits.len() > compact_hits.len() * 3);
    assert!(text.len() < compact_hits.len());
    assert!(
        text.len() < 700,
        "text hit must stay compact: {}",
        text.len()
    );
    assert!(
        compact_hits.len() < 900,
        "one compact hit must stay under 900 bytes: {}",
        compact_hits.len()
    );
}

#[tokio::test]
async fn mcp_host_allowlist_rejects_foreign_and_accepts_listed_hosts() {
    let app = app_with_config(ServerConfig {
        allowed_hosts: vec!["mem.internal".into()],
        ..ServerConfig::default()
    });
    let list = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/list",
        "params": { "_meta": draft_meta() }
    });
    // The outer guard rejects a foreign Host on the MCP mount.
    let foreign = Request::post("/mcp")
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
        .header("host", "attacker.example")
        .header("mcp-protocol-version", "2026-07-28")
        .header("mcp-method", "tools/list")
        .body(Body::from(list.to_string()))
        .unwrap();
    let res = app.clone().oneshot(foreign).await.unwrap();
    assert_eq!(res.status(), axum::http::StatusCode::MISDIRECTED_REQUEST);
    // A host in the allowlist reaches the transport.
    let allowed = Request::post("/mcp")
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
        .header("host", "mem.internal:8787")
        .header("mcp-protocol-version", "2026-07-28")
        .header("mcp-method", "tools/list")
        .body(Body::from(list.to_string()))
        .unwrap();
    let res = app.oneshot(allowed).await.unwrap();
    assert_eq!(res.status(), axum::http::StatusCode::OK);
}

#[tokio::test]
async fn tools_list_stays_within_the_token_budget() {
    let app = app_with(None);
    let list = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/list",
        "params": { "_meta": draft_meta() }
    });
    let (status, body) = rpc_stateless(&app, list).await;
    assert_eq!(status, axum::http::StatusCode::OK, "{body}");
    let bytes = body.to_string().len();
    eprintln!("tools/list bytes: {bytes}");
    assert!(
        bytes <= 4600,
        "tools/list must stay <= 4600 bytes for context efficiency, got {bytes}:\n{body}"
    );
}

#[tokio::test]
async fn stateless_2026_07_28_discover_list_and_call_over_http() {
    // No initialize handshake: the 2026-07-28 revision replaced it with
    // per-request metadata, so discover -> list -> call each stand alone.
    let app = app_with(None);

    let discover = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "server/discover",
        "params": { "_meta": draft_meta() }
    });
    let (status, body) = rpc_stateless(&app, discover).await;
    assert_eq!(status, axum::http::StatusCode::OK, "{body}");
    let result = unwrap_result(&body);
    assert!(
        result["supportedVersions"]
            .as_array()
            .is_some_and(|versions| versions.iter().any(|v| v == "2026-07-28")),
        "discover must advertise 2026-07-28: {result}"
    );

    let list = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/list",
        "params": { "_meta": draft_meta() }
    });
    let (status, body) = rpc_stateless(&app, list).await;
    assert_eq!(status, axum::http::StatusCode::OK, "{body}");
    let names: Vec<&str> = unwrap_result(&body)["tools"]
        .as_array()
        .expect("tools array")
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    for expected in [
        "remember",
        "recall",
        "forget",
        "update_memory",
        "list_memories",
        "capture",
    ] {
        assert!(
            names.contains(&expected),
            "missing tool {expected}: {names:?}"
        );
    }

    let call = json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "tools/call",
        "params": {
            "name": "remember",
            "arguments": {"namespace": "stateless", "text": "per-request metadata works"},
            "_meta": draft_meta()
        }
    });
    let (status, body) = rpc_stateless(&app, call).await;
    assert_eq!(status, axum::http::StatusCode::OK, "{body}");
    let remembered = tool_text(unwrap_result(&body));
    assert_eq!(remembered["text"], "per-request metadata works");
}

#[tokio::test]
async fn conformance_initialize_tools_list_call_over_http() {
    let app = app_with(None);

    // 1) initialize — server identifies itself and its capabilities.
    let (status, body) = rpc(&app, initialize_request()).await;
    assert_eq!(status, axum::http::StatusCode::OK);
    let result = unwrap_result(&body);
    assert_eq!(result["serverInfo"]["name"], "recall-mcp");
    // Wire pin: rmcp 3.3 answers an `initialize` naming `2026-07-28` with
    // `2025-11-25` — the newest version that still HAS an initialize
    // handshake, because the 2026-07-28 revision replaced the handshake with
    // per-request metadata (rmcp `negotiate_protocol_version`). Pinned so an
    // SDK bump that changes the wire answer is caught, not silently absorbed.
    assert_eq!(result["protocolVersion"], "2025-11-25");
    assert!(
        result["capabilities"]["tools"].is_object(),
        "tools capability must be advertised"
    );

    // 2) tools/list — exactly the six documented tools, each with an input schema.
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
    assert_eq!(
        names,
        vec![
            "capture",
            "forget",
            "list_memories",
            "recall",
            "remember",
            "update_memory"
        ]
    );
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

    // 4b) tools/call update_memory -> text revised, tags replaced.
    let (_, body) = rpc(
        &app,
        tool_call(
            10,
            "update_memory",
            json!({"id": memory_id, "text": "the transport now speaks json-rpc fluently", "tags": ["mcp", "updated"]}),
        ),
    )
    .await;
    let updated = tool_text(unwrap_result(&body));
    assert_eq!(
        updated["text"],
        "the transport now speaks json-rpc fluently"
    );
    assert_eq!(updated["tags"], json!(["mcp", "updated"]));
    assert_eq!(updated["id"], memory_id.as_str());

    // 4c) recall with a tag filter finds the updated memory; the wrong filter finds nothing.
    let (_, body) = rpc(
        &app,
        tool_call(
            11,
            "recall",
            json!({"namespace": "conformance", "query": "json-rpc", "tags": ["updated"]}),
        ),
    )
    .await;
    let hits = tool_text(unwrap_result(&body));
    assert_eq!(hits.as_array().unwrap().len(), 1);
    let (_, body) = rpc(
        &app,
        tool_call(
            12,
            "recall",
            json!({"namespace": "conformance", "query": "json-rpc", "tags": ["unrelated"]}),
        ),
    )
    .await;
    assert_eq!(tool_text(unwrap_result(&body)).as_array().unwrap().len(), 0);

    // 5) tools/call list_memories -> still present.
    let (_, body) = rpc(
        &app,
        tool_call(5, "list_memories", json!({"namespace": "conformance"})),
    )
    .await;
    let memories = tool_text(unwrap_result(&body));
    assert_eq!(memories["items"].as_array().unwrap().len(), 1);
    assert_eq!(memories["total"], 1);

    // 6) tools/call forget -> deleted: true, then list is empty.
    let (_, body) = rpc(&app, tool_call(6, "forget", json!({"id": memory_id}))).await;
    let forgotten = tool_text(unwrap_result(&body));
    assert_eq!(forgotten["deleted"], true);

    let (_, body) = rpc(
        &app,
        tool_call(7, "list_memories", json!({"namespace": "conformance"})),
    )
    .await;
    assert_eq!(
        tool_text(unwrap_result(&body))["items"]
            .as_array()
            .unwrap()
            .len(),
        0
    );

    // 6b) tools/call capture -> transcript auto-chunked into capture memories
    // (runs after the forget steps so their exact-count assertions stay intact).
    let long = "Decided to keep the storage layer boring. ".repeat(60); // ~2520 chars
    let (_, body) = rpc(
        &app,
        tool_call(
            13,
            "capture",
            json!({
                "namespace": "conformance",
                "transcript": [
                    {"role": "user", "content": "please summarize the storage decision", "at": 1_786_000_500},
                    {"role": "assistant", "content": long}
                ],
                "tags": ["session:storage"],
                "include_memories": true
            }),
        ),
    )
    .await;
    assert_eq!(unwrap_result(&body)["isError"], false);
    let report = tool_text(unwrap_result(&body));
    assert_eq!(report["namespace"], "conformance");
    let captured = report["captured"].as_u64().expect("captured count");
    assert!(captured >= 3, "long message must be chunked: {report}");
    let memories = report["memories"].as_array().unwrap();
    assert_eq!(memories.len() as u64, captured);
    for memory in memories {
        assert_eq!(memory["source"], "capture");
        // The user message carries an explicit `at`; the assistant message
        // intentionally does not and takes the server clock.
        if memory["tags"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t == "role:user")
        {
            assert_eq!(memory["created_at"], 1_786_000_500);
        }
        let tags: Vec<&str> = memory["tags"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t.as_str().unwrap())
            .collect();
        assert!(tags.contains(&"session:storage"), "{tags:?}");
        assert!(tags.contains(&"role:user") || tags.contains(&"role:assistant"));
    }

    // Capture output is recallable through the same MCP surface.
    let (_, body) = rpc(
        &app,
        tool_call(
            14,
            "recall",
            json!({"namespace": "conformance", "query": "storage decision boring", "tags": ["session:storage"]}),
        ),
    )
    .await;
    let hits = tool_text(unwrap_result(&body));
    assert!(
        !hits.as_array().unwrap().is_empty(),
        "captured chunks must be recallable: {hits}"
    );

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
