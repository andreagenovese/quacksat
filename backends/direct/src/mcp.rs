//! The duck's own MCP server: the robot tool catalog served over
//! stateless Streamable HTTP (`POST /mcp`, JSON-RPC 2.0), so any
//! MCP-capable agent — Arkimede, Claude Desktop, Claude Code — can drive
//! the body directly, with or without voice.
//!
//! Same allowlist and clamps as every other tool path
//! (`quacksat_core::tools`); the bearer token is mandatory (an HTTP
//! server accepting motion commands on the robot never runs open), and
//! robotd's deadman remains the last line underneath.
//!
//! Deliberately minimal: the stateless subset of the transport
//! (`initialize`, `tools/list`, `tools/call`, `ping`; notifications
//! answered 202; no SSE stream, no sessions). Hand-rolled because the
//! official Rust SDK is async-runtime-based and this crate is not.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};

use quacksat_core::robotd::Control;
use quacksat_core::tools;
use serde_json::{Value, json};

pub type SharedControl = Arc<Mutex<Option<Control>>>;

/// Poison-tolerant lock: a panicked holder must not brick the robot path.
pub fn lock(control: &SharedControl) -> std::sync::MutexGuard<'_, Option<Control>> {
    control
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Accept loop; run it on its own thread. One thread per connection —
/// MCP traffic is a trickle, not a flood.
pub fn serve(listener: TcpListener, control: SharedControl, token: String) {
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let control = control.clone();
        let token = token.clone();
        std::thread::spawn(move || {
            if let Err(e) = handle_connection(stream, &control, &token) {
                tracing::debug!(error = %e, "mcp connection ended");
            }
        });
    }
}

fn handle_connection(
    mut stream: TcpStream,
    control: &SharedControl,
    token: &str,
) -> anyhow::Result<()> {
    while let Some(request) = read_http_request(&mut stream)? {
        let authorized = request
            .headers
            .iter()
            .any(|(k, v)| k == "authorization" && v == &format!("Bearer {token}"));
        if !authorized {
            respond(
                &mut stream,
                "401 Unauthorized",
                &[("WWW-Authenticate", "Bearer")],
                b"",
            )?;
            continue;
        }
        if request.method != "POST" {
            respond(
                &mut stream,
                "405 Method Not Allowed",
                &[("Allow", "POST")],
                b"",
            )?;
            continue;
        }
        let message: Value = match serde_json::from_slice(&request.body) {
            Ok(message) => message,
            Err(_) => {
                respond_json(
                    &mut stream,
                    &json!({"jsonrpc": "2.0", "id": null,
                            "error": {"code": -32700, "message": "parse error"}}),
                )?;
                continue;
            }
        };
        // A notification (no id) is acknowledged and not answered.
        let Some(id) = message.get("id").filter(|id| !id.is_null()).cloned() else {
            respond(&mut stream, "202 Accepted", &[], b"")?;
            continue;
        };
        let method = message.get("method").and_then(Value::as_str).unwrap_or("");
        let params = message.get("params").cloned().unwrap_or_else(|| json!({}));
        let reply = match dispatch(method, &params, control) {
            Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
            Err(error) => json!({"jsonrpc": "2.0", "id": id,
                                 "error": {"code": -32601, "message": error}}),
        };
        respond_json(&mut stream, &reply)?;
    }
    Ok(())
}

fn dispatch(method: &str, params: &Value, control: &SharedControl) -> Result<Value, String> {
    match method {
        "initialize" => {
            let requested = params
                .get("protocolVersion")
                .and_then(Value::as_str)
                .unwrap_or("2025-03-26");
            Ok(json!({
                "protocolVersion": requested,
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "quacksat-robot", "version": env!("CARGO_PKG_VERSION")},
            }))
        }
        "ping" => Ok(json!({})),
        "tools/list" => {
            let tools: Vec<Value> = tools::catalog()
                .as_array()
                .into_iter()
                .flatten()
                .map(|tool| {
                    json!({
                        // MCP tool names avoid dots; mapped back on call.
                        "name": tool["name"].as_str().unwrap_or_default().replace('.', "_"),
                        "description": tool.get("description").cloned().unwrap_or(json!("")),
                        "inputSchema": tool.get("parameters").cloned()
                            .unwrap_or(json!({"type": "object", "properties": {}})),
                    })
                })
                .collect();
            Ok(json!({"tools": tools}))
        }
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            let wire_name = name.replacen('_', ".", 1);
            let arguments = params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            tracing::info!(tool = %wire_name, %arguments, "mcp tool call");
            let mut guard = lock(control);
            let payload = match tools::execute(&wire_name, &arguments, &mut guard) {
                Ok(data) => json!({"ok": true, "data": data}),
                Err(error) => {
                    tracing::info!(%error, "mcp tool refused");
                    json!({"ok": false, "error": error})
                }
            };
            let is_error = !payload["ok"].as_bool().unwrap_or(false);
            Ok(json!({
                "content": [{"type": "text", "text": payload.to_string()}],
                "isError": is_error,
            }))
        }
        other => Err(format!("method not found: {other}")),
    }
}

struct HttpRequest {
    method: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

/// Read one HTTP/1.1 request; `None` on a cleanly closed connection.
fn read_http_request(stream: &mut TcpStream) -> anyhow::Result<Option<HttpRequest>> {
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        match stream.read(&mut byte)? {
            0 if head.is_empty() => return Ok(None),
            0 => anyhow::bail!("connection closed mid-request"),
            _ => head.push(byte[0]),
        }
        if head.len() > 64 * 1024 {
            anyhow::bail!("request head too large");
        }
    }
    let head = String::from_utf8_lossy(&head);
    let mut lines = head.lines();
    let method = lines
        .next()
        .and_then(|l| l.split_whitespace().next())
        .unwrap_or("")
        .to_string();
    let headers: Vec<(String, String)> = lines
        .filter_map(|line| {
            let (k, v) = line.split_once(':')?;
            Some((k.trim().to_ascii_lowercase(), v.trim().to_string()))
        })
        .collect();
    let length: usize = headers
        .iter()
        .find(|(k, _)| k == "content-length")
        .and_then(|(_, v)| v.parse().ok())
        .unwrap_or(0);
    anyhow::ensure!(length <= 1024 * 1024, "request body too large");
    let mut body = vec![0u8; length];
    stream.read_exact(&mut body)?;
    Ok(Some(HttpRequest {
        method,
        headers,
        body,
    }))
}

fn respond_json(stream: &mut TcpStream, body: &Value) -> anyhow::Result<()> {
    respond(
        stream,
        "200 OK",
        &[("Content-Type", "application/json")],
        body.to_string().as_bytes(),
    )
}

fn respond(
    stream: &mut TcpStream,
    status: &str,
    headers: &[(&str, &str)],
    body: &[u8],
) -> anyhow::Result<()> {
    let mut head = format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\n", body.len());
    for (k, v) in headers {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes())?;
    stream.write_all(body)?;
    stream.flush()?;
    Ok(())
}
