//! The duck's MCP server over real HTTP: bearer auth, initialize,
//! tools/list mirroring the catalog, tools/call through the shared
//! allowlist, notifications acknowledged, unknown methods refused.

use std::net::TcpListener;
use std::sync::{Arc, Mutex};

use quacksat_backend_direct::mcp;
use serde_json::{Value, json};

fn client() -> ureq::Agent {
    ureq::Agent::config_builder()
        .http_status_as_error(false)
        .build()
        .into()
}

fn post(agent: &ureq::Agent, url: &str, token: Option<&str>, body: &Value) -> (u16, Value) {
    let mut request = agent.post(url).header("Content-Type", "application/json");
    if let Some(token) = token {
        request = request.header("Authorization", &format!("Bearer {token}"));
    }
    let mut response = request.send(body.to_string().as_bytes()).unwrap();
    let status = response.status().as_u16();
    let text = response.body_mut().read_to_string().unwrap_or_default();
    let parsed = serde_json::from_str(&text).unwrap_or(Value::Null);
    (status, parsed)
}

#[test]
fn mcp_server_speaks_the_stateless_protocol() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/mcp", listener.local_addr().unwrap());
    let control = Arc::new(Mutex::new(None));
    std::thread::spawn(move || mcp::serve(listener, control, "sesame".to_string()));
    let agent = client();

    // No token → 401; wrong token → 401.
    let (status, _) = post(
        &agent,
        &url,
        None,
        &json!({"jsonrpc": "2.0", "id": 1, "method": "ping"}),
    );
    assert_eq!(status, 401);
    let (status, _) = post(
        &agent,
        &url,
        Some("wrong"),
        &json!({"jsonrpc": "2.0", "id": 1, "method": "ping"}),
    );
    assert_eq!(status, 401);

    // initialize.
    let (status, reply) = post(
        &agent,
        &url,
        Some("sesame"),
        &json!({"jsonrpc": "2.0", "id": 1, "method": "initialize",
                "params": {"protocolVersion": "2025-06-18", "capabilities": {},
                           "clientInfo": {"name": "test", "version": "0"}}}),
    );
    assert_eq!(status, 200);
    assert_eq!(reply["result"]["serverInfo"]["name"], "quacksat-robot");
    assert_eq!(reply["result"]["protocolVersion"], "2025-06-18");

    // Initialized notification (no id) → 202, no body.
    let (status, reply) = post(
        &agent,
        &url,
        Some("sesame"),
        &json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
    );
    assert_eq!(status, 202);
    assert_eq!(reply, Value::Null);

    // tools/list mirrors the catalog, dots replaced.
    let (status, reply) = post(
        &agent,
        &url,
        Some("sesame"),
        &json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
    );
    assert_eq!(status, 200);
    let names: Vec<&str> = reply["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"robot_look"));
    assert!(names.contains(&"robot_move"));
    assert_eq!(names.len(), 7);

    // tools/call: unsupported and no-robot outcomes travel as isError.
    let (_, reply) = post(
        &agent,
        &url,
        Some("sesame"),
        &json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call",
                "params": {"name": "robot_get_frame", "arguments": {}}}),
    );
    assert_eq!(reply["result"]["isError"], true);
    assert!(
        reply["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("unsupported")
    );

    let (_, reply) = post(
        &agent,
        &url,
        Some("sesame"),
        &json!({"jsonrpc": "2.0", "id": 4, "method": "tools/call",
                "params": {"name": "robot_state", "arguments": {}}}),
    );
    assert_eq!(reply["result"]["isError"], true);
    assert!(
        reply["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("robot unreachable")
    );

    // Unknown method → JSON-RPC error.
    let (_, reply) = post(
        &agent,
        &url,
        Some("sesame"),
        &json!({"jsonrpc": "2.0", "id": 5, "method": "resources/list"}),
    );
    assert_eq!(reply["error"]["code"], -32601);
}
