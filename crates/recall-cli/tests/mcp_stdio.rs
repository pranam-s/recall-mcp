//! MCP conformance smoke test over stdio: spawn `recall-cli serve --stdio`
//! as a child process and drive initialize -> tools/list -> tools/call.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};

use serde_json::{Value, json};

struct StdioSession {
    child: Child,
    stdin: Option<ChildStdin>,
    reader: BufReader<std::process::ChildStdout>,
}

impl StdioSession {
    fn spawn(db_path: &str) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_recall-cli"))
            .args(["serve", "--stdio", "--db", db_path])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn recall-cli serve --stdio");
        let stdin = child.stdin.take().expect("stdin");
        let stdout = child.stdout.take().expect("stdout");
        Self {
            child,
            stdin: Some(stdin),
            reader: BufReader::new(stdout),
        }
    }

    fn send(&mut self, payload: &Value) {
        let mut line = payload.to_string();
        line.push('\n');
        self.stdin
            .as_mut()
            .expect("stdin open")
            .write_all(line.as_bytes())
            .expect("write request");
        self.stdin
            .as_mut()
            .expect("stdin open")
            .flush()
            .expect("flush request");
    }

    fn recv(&mut self) -> Value {
        let mut line = String::new();
        self.reader
            .read_line(&mut line)
            .expect("read response line");
        assert!(!line.is_empty(), "stdio transport closed before response");
        serde_json::from_str(&line).expect("response must be JSON-RPC")
    }

    fn request(&mut self, payload: &Value) -> Value {
        self.send(payload);
        self.recv()
    }
}

impl Drop for StdioSession {
    fn drop(&mut self) {
        // Closing stdin ends the session; the server should exit promptly.
        drop(self.stdin.take());
        let _ = self.child.wait();
    }
}

fn unwrap_result(body: &Value) -> &Value {
    body.get("result")
        .unwrap_or_else(|| panic!("JSON-RPC result expected, got: {body}"))
}

fn tool_text(result: &Value) -> Value {
    serde_json::from_str(result["content"][0]["text"].as_str().expect("text content"))
        .expect("tool output must be valid JSON")
}

#[test]
fn conformance_initialize_tools_list_call_over_stdio() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("stdio.db");
    let db = db_path.to_str().unwrap();
    let mut session = StdioSession::spawn(db);

    // 1) initialize
    let response = session.request(&json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2026-07-28",
            "capabilities": {},
            "clientInfo": { "name": "recall-conformance", "version": "0.0.1" }
        }
    }));
    assert_eq!(response["id"], 1);
    let result = unwrap_result(&response);
    assert_eq!(result["serverInfo"]["name"], "recall-mcp");
    assert!(result["capabilities"]["tools"].is_object());

    // 2) initialized notification (no response expected), then tools/list
    session.send(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
    let response = session.request(&json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}));
    let tools = unwrap_result(&response)["tools"]
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

    // 3) tools/call remember (auto-creates the namespace)
    let response = session.request(&tool_call(
        3,
        "remember",
        json!({
            "namespace": "stdio-test",
            "text": "stdio transport remembers decisions across sessions"
        }),
    ));
    let stored = tool_text(unwrap_result(&response));
    let id = stored["id"].as_str().expect("memory id").to_string();
    assert_eq!(stored["source"], "mcp");

    // 4) tools/call recall with breakdowns
    let response = session.request(&tool_call(
        4,
        "recall",
        json!({
            "namespace": "stdio-test",
            "query": "decisions across sessions"
        }),
    ));
    let hits = tool_text(unwrap_result(&response));
    assert_eq!(hits.as_array().unwrap().len(), 1);
    assert_eq!(hits[0]["id"], id.as_str());
    assert!(hits[0]["breakdown"]["total"].as_f64().unwrap() > 0.0);

    // 5) forget + list_memories round out the tool surface
    let response = session.request(&tool_call(5, "forget", json!({ "id": id })));
    assert_eq!(tool_text(unwrap_result(&response))["deleted"], true);
    let response = session.request(&tool_call(
        6,
        "list_memories",
        json!({ "namespace": "stdio-test" }),
    ));
    assert_eq!(
        tool_text(unwrap_result(&response))
            .as_array()
            .unwrap()
            .len(),
        0
    );
}

fn tool_call(id: u64, name: &str, args: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "tools/call",
        "params": { "name": name, "arguments": args }
    })
}
