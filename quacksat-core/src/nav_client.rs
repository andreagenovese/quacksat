//! A client of `quack-navd`, the daemon that owns the map, the places
//! and everything that drives the body somewhere. The name is the
//! whole point: none of that is in here, only the wire that reaches
//! it.
//!
//! The satellite is a voice satellite (the user's, 2026-09-22: quacksat
//! stays the assistant, the navigation is its own repo). It does not
//! link the navigation in: it asks, at startup, whether a daemon is
//! listening on the configured socket. If one answers, its tools are
//! announced beside the satellite's own and executed there; if none
//! does, the duck simply cannot be told to go anywhere, and says so.
//!
//! The wire is robotd's: NDJSON, JSON-RPC 2.0, one connection per lane.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

use serde_json::{Value, json};

use crate::config::NavConfig;

/// How long a navigation call may take before the lane gives up. A
/// journey does not block here — `robot.go_to` starts a background job
/// and answers at once — so this is the daemon's own answering time.
const CALL_TIMEOUT: Duration = Duration::from_secs(10);

/// A question the navigation asks the person: "what is this place?".
/// The daemon raises it while mapping; the satellite speaks it. Carried
/// here so the backends can hold one before the lane exposes it.
#[derive(Debug, Clone)]
pub struct Question {
    pub phrase: String,
    pub pose: Option<(f64, f64, f64)>,
}

pub struct NavLane {
    socket: String,
    stream: Option<UnixStream>,
    next_id: u64,
    /// The names the daemon answered with at startup.
    tools: Vec<Value>,
}

impl NavLane {
    /// Ask the socket for its catalog. `None` when nothing answers —
    /// which is not an error: a satellite without navigation is a
    /// satellite.
    pub fn probe(config: &NavConfig) -> Option<Self> {
        if !config.enabled {
            return None;
        }
        let mut lane = NavLane {
            socket: config.socket.clone(),
            stream: None,
            next_id: 1,
            tools: Vec::new(),
        };
        match lane.request("nav.catalog", json!({})) {
            Ok(Value::Array(tools)) => {
                tracing::info!(socket = %config.socket, tools = tools.len(), "the navigation daemon answered");
                lane.tools = tools;
                Some(lane)
            }
            Ok(other) => {
                tracing::warn!(socket = %config.socket, answer = %other, "the navigation socket answered something else");
                None
            }
            Err(e) => {
                tracing::info!(socket = %config.socket, reason = %e, "no navigation daemon — the duck stays put");
                None
            }
        }
    }

    /// The daemon's catalog, as announced at startup.
    pub fn catalog(&self) -> Vec<Value> {
        self.tools.clone()
    }

    /// Whether the daemon announced this tool.
    pub fn handles(&self, name: &str) -> bool {
        self.tools.iter().any(|t| t.get("name").and_then(Value::as_str) == Some(name))
    }

    /// The explorer's pending question, if it has one. Polled by the
    /// satellite between utterances: the daemon raises the question
    /// while it maps, and only the satellite can speak it.
    pub fn take_question(&mut self) -> Option<Question> {
        let answer = self.request("nav.call", json!({"name": "nav.take_question", "args": {}})).ok()?;
        if answer.get("asking").and_then(Value::as_bool) != Some(true) {
            return None;
        }
        let pose = answer.get("pose").map(|p| {
            (
                p.get("x").and_then(Value::as_f64).unwrap_or_default(),
                p.get("y").and_then(Value::as_f64).unwrap_or_default(),
                p.get("yaw").and_then(Value::as_f64).unwrap_or_default(),
            )
        });
        Some(Question {
            phrase: answer.get("phrase").and_then(Value::as_str).unwrap_or_default().to_owned(),
            pose,
        })
    }

    /// Execute one of the daemon's tools.
    pub fn call(&mut self, name: &str, args: &Value) -> Result<Value, String> {
        self.request("nav.call", json!({"name": name, "args": args}))
            .map_err(|e| format!("the navigation daemon: {e}"))
    }

    fn connect(&mut self) -> Result<&mut UnixStream, String> {
        if self.stream.is_none() {
            let stream = UnixStream::connect(&self.socket).map_err(|e| e.to_string())?;
            stream.set_read_timeout(Some(CALL_TIMEOUT)).map_err(|e| e.to_string())?;
            stream.set_write_timeout(Some(CALL_TIMEOUT)).map_err(|e| e.to_string())?;
            self.stream = Some(stream);
        }
        Ok(self.stream.as_mut().expect("just connected"))
    }

    fn request(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        let line = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}).to_string();
        // One retry: the daemon may have been restarted under us.
        for attempt in 0..2 {
            let result = (|| -> Result<Value, String> {
                let stream = self.connect()?;
                stream.write_all(line.as_bytes()).map_err(|e| e.to_string())?;
                stream.write_all(b"\n").map_err(|e| e.to_string())?;
                stream.flush().map_err(|e| e.to_string())?;
                let mut reply = String::new();
                let mut reader = BufReader::new(stream.try_clone().map_err(|e| e.to_string())?);
                reader.read_line(&mut reply).map_err(|e| e.to_string())?;
                if reply.is_empty() {
                    return Err("the lane closed".into());
                }
                let value: Value = serde_json::from_str(&reply).map_err(|e| e.to_string())?;
                if let Some(error) = value.get("error") {
                    let message = error.get("message").and_then(Value::as_str).unwrap_or("refused");
                    return Err(message.to_string());
                }
                Ok(value.get("result").cloned().unwrap_or(Value::Null))
            })();
            match result {
                Ok(value) => return Ok(value),
                Err(e) if attempt == 0 => {
                    self.stream = None;
                    tracing::debug!(error = %e, "the navigation lane dropped; reconnecting");
                }
                Err(e) => return Err(e),
            }
        }
        unreachable!("the loop returns on both arms")
    }
}
