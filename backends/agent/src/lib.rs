//! Agent backend (path B, ADR 0004): streams audio and events over
//! WebSocket to a bridge running STT → LLM (tool calling) → TTS, and
//! executes the agent's robot tool calls against robotd. Wire contract:
//! docs/agent-protocol.md.

pub mod session;
pub mod tools;

use std::sync::mpsc;

use quacksat_core::config::Config;
use quacksat_core::playback::Player;
use quacksat_core::robotd::{Control, RECONNECT_DELAY};
use quacksat_core::wake;
use tungstenite::client::IntoClientRequest;

/// Run the backend forever: connect, run a session, reconnect on loss
/// with a fixed backoff (sessions are stateless on the wire).
pub fn run(config: &Config, frames: mpsc::Receiver<Vec<i16>>) -> anyhow::Result<()> {
    let mut control = match Control::connect(&config.robotd_socket) {
        Ok(control) => Some(control),
        Err(e) => {
            tracing::warn!(error = %e, "robotd unreachable — running without the robot");
            None
        }
    };
    let mut player = match &config.audio.playback_program {
        Some(program) => Player::with_program(&config.audio.playback_device, program),
        None => Player::new(&config.audio.playback_device),
    };
    let mut detector = wake::from_config(&config.wake)?;

    loop {
        match connect(config) {
            Ok(ws) => {
                tracing::info!(url = %config.agent.url, "bridge connected");
                let mut deps = session::Deps {
                    config,
                    frames: &frames,
                    detector: detector.as_mut(),
                    player: &mut player,
                    control: &mut control,
                };
                match session::run_session(ws, &mut deps) {
                    Ok(()) => tracing::info!("bridge disconnected"),
                    Err(e) => tracing::warn!(error = %e, "session failed"),
                }
                player.stop();
                detector.reset();
                while frames.try_recv().is_ok() {}
            }
            Err(e) => tracing::warn!(url = %config.agent.url, error = %e, "bridge unreachable"),
        }
        std::thread::sleep(RECONNECT_DELAY);
    }
}

fn connect(
    config: &Config,
) -> anyhow::Result<tungstenite::WebSocket<tungstenite::stream::MaybeTlsStream<std::net::TcpStream>>>
{
    let mut request = config.agent.url.clone().into_client_request()?;
    if let Some(token) = &config.agent.token {
        request
            .headers_mut()
            .insert("Authorization", format!("Bearer {token}").parse()?);
    }
    let (ws, _response) = tungstenite::connect(request)?;
    Ok(ws)
}
