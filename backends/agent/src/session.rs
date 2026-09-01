//! One WebSocket session with the bridge/agent, per the wire spec
//! (docs/agent-protocol.md). Single-threaded: the socket runs with a
//! short read timeout and mic frames are polled between reads — same
//! no-async, no-select approach as the wyoming backend.

use std::collections::VecDeque;
use std::net::TcpStream;
use std::sync::mpsc;
use std::time::Duration;

use quacksat_core::audio::{FRAME_SAMPLES, PIPELINE_RATE};
use quacksat_core::config::Config;
use quacksat_core::playback::Player;
use quacksat_core::robotd::Control;
use quacksat_core::vad::{Vad, VadEvent};
use quacksat_core::wake::WakeDetector;
use serde_json::{Value, json};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};

use quacksat_core::tools;

pub struct Deps<'a> {
    pub config: &'a Config,
    pub frames: &'a mpsc::Receiver<Vec<i16>>,
    pub detector: &'a mut dyn WakeDetector,
    pub player: &'a mut Player,
    pub control: &'a mut Option<Control>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mic {
    /// Wake word armed, nothing streamed.
    Idle,
    /// Forwarding mic frames to the bridge.
    Streaming,
}

/// ~320 ms of pre-roll flushed on wake, as in the wyoming backend.
const PREROLL_FRAMES: usize = 10;
const POLL: Duration = Duration::from_millis(50);
/// End-of-utterance silence (~800 ms at 32 ms/frame): long enough that a
/// natural pause between the wake phrase and the command does not close
/// the turn before the command is spoken.
const UTTERANCE_HANGOVER_FRAMES: u32 = 25;
/// Give up on an utterance if no speech starts within this many frames
/// (~6 s) or it runs longer than this (~15 s).
const NO_SPEECH_FRAMES: u32 = 187;
const MAX_UTTERANCE_FRAMES: u32 = 469;

type Ws = WebSocket<MaybeTlsStream<TcpStream>>;

pub fn run_session(mut ws: Ws, deps: &mut Deps) -> anyhow::Result<()> {
    // Short socket timeouts turn the blocking read into a poll so mic
    // frames can be serviced in between.
    if let MaybeTlsStream::Plain(tcp) = ws.get_ref() {
        tcp.set_read_timeout(Some(POLL))?;
        tcp.set_write_timeout(Some(Duration::from_secs(5)))?;
    } else if let MaybeTlsStream::Rustls(tls) = ws.get_ref() {
        tls.get_ref().set_read_timeout(Some(POLL))?;
        tls.get_ref()
            .set_write_timeout(Some(Duration::from_secs(5)))?;
    }

    send_json(
        &mut ws,
        &json!({
            "type": "session.start",
            "version": 1,
            "satellite": {"name": "quacksat", "version": env!("CARGO_PKG_VERSION")},
            "audio": {"rate": PIPELINE_RATE, "channels": 1, "format": "s16le"},
            "tools": tools::catalog(),
        }),
    )?;

    let mut mic = Mic::Idle;
    let mut vad = Vad::new();
    let mut preroll: VecDeque<Vec<i16>> = VecDeque::with_capacity(PREROLL_FRAMES);
    let mut streamed_frames: u32 = 0;
    let mut speech_seen = false;
    let mut playing_tts = false;
    let mut listen_after_tts = false;

    loop {
        // 1. Socket first, so listen.stop / tool calls beat mic frames.
        match ws.read() {
            Ok(Message::Text(text)) => {
                let event: Value = match serde_json::from_str(&text) {
                    Ok(event) => event,
                    Err(e) => {
                        tracing::debug!(error = %e, "unparsable event skipped");
                        continue;
                    }
                };
                match event.get("type").and_then(Value::as_str).unwrap_or("") {
                    "session.ready" => {
                        let agent = event
                            .pointer("/agent/name")
                            .and_then(Value::as_str)
                            .unwrap_or("?");
                        tracing::info!(agent, "session ready");
                    }
                    "listen.start" => {
                        if playing_tts {
                            listen_after_tts = true;
                        } else if mic == Mic::Idle {
                            enter_streaming(
                                &mut mic,
                                &mut vad,
                                &mut streamed_frames,
                                &mut speech_seen,
                            );
                            tracing::info!("listening (bridge request)");
                        }
                    }
                    "listen.stop" => {
                        if mic == Mic::Streaming {
                            stop_streaming(&mut mic, deps);
                        }
                    }
                    "tts.start" => {
                        let rate =
                            event.get("rate").and_then(Value::as_u64).unwrap_or(22_050) as u32;
                        let channels =
                            event.get("channels").and_then(Value::as_u64).unwrap_or(1) as u16;
                        tracing::info!(rate, channels, "tts playback starting");
                        playing_tts = true;
                        if let Err(e) = deps.player.begin_stream(rate, channels) {
                            tracing::warn!(error = %e, "speaker unavailable — tts dropped");
                        }
                    }
                    "tts.end" => {
                        // Half-duplex completion point: drain playback,
                        // discard mic backlog, forget the wake phrase.
                        deps.player.end_stream();
                        while deps.frames.try_recv().is_ok() {}
                        deps.detector.reset();
                        playing_tts = false;
                        tracing::info!("tts played");
                        if std::mem::take(&mut listen_after_tts) {
                            enter_streaming(
                                &mut mic,
                                &mut vad,
                                &mut streamed_frames,
                                &mut speech_seen,
                            );
                            tracing::info!("listening (follow-up turn)");
                        }
                    }
                    "tool.call" => {
                        let id = event.get("id").and_then(Value::as_str).unwrap_or("");
                        let name = event.get("name").and_then(Value::as_str).unwrap_or("");
                        let args = event.get("args").cloned().unwrap_or_else(|| json!({}));
                        tracing::info!(id, name, "tool call");
                        let reply = match tools::execute(name, &args, deps.control) {
                            Ok(data) => {
                                json!({"type": "tool.result", "id": id, "ok": true, "data": data})
                            }
                            Err(error) => {
                                tracing::info!(id, error, "tool refused");
                                json!({"type": "tool.result", "id": id, "ok": false, "error": error})
                            }
                        };
                        send_json(&mut ws, &reply)?;
                    }
                    "ping" => {
                        let mut pong = event.clone();
                        pong["type"] = json!("pong");
                        send_json(&mut ws, &pong)?;
                    }
                    "error" => {
                        let message = event.get("message").and_then(Value::as_str).unwrap_or("");
                        tracing::warn!(message, "bridge error");
                    }
                    other => tracing::debug!(event = other, "ignored event"),
                }
                continue;
            }
            // TTS audio; anything binary outside a clip is dropped.
            Ok(Message::Binary(bytes)) => {
                if playing_tts {
                    deps.player.write_stream(&bytes);
                }
                continue;
            }
            Ok(Message::Close(_)) => return Ok(()),
            Ok(_) => continue, // WS-level ping/pong, handled by tungstenite
            Err(tungstenite::Error::Io(e))
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut => {}
            Err(tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed) => {
                return Ok(());
            }
            Err(e) => return Err(e.into()),
        }

        // 2. Then the mic — draining the whole backlog: the socket poll
        // above blocks up to 50 ms while frames arrive every 32 ms, so
        // one-frame-per-iteration falls behind real time and starves the
        // wake detector with gapped audio.
        while let Ok(frame) = deps.frames.try_recv() {
            if playing_tts {
                continue; // deaf while the duck talks (ADR 0003)
            }
            match mic {
                Mic::Idle => {
                    if deps.detector.feed(&frame) {
                        let model = match deps.config.wake.mode {
                            quacksat_core::config::WakeMode::Openwakeword => {
                                deps.config.wake.model.clone()
                            }
                            other => format!("{other:?}").to_lowercase(),
                        };
                        tracing::info!("wake");
                        if !chirp(deps.control) {
                            wake_ack(deps);
                        }
                        send_json(&mut ws, &json!({"type": "wake", "model": model}))?;
                        enter_streaming(&mut mic, &mut vad, &mut streamed_frames, &mut speech_seen);
                        for buffered in preroll.drain(..) {
                            send_audio(&mut ws, &buffered)?;
                        }
                        send_audio(&mut ws, &frame)?;
                    } else {
                        if preroll.len() == PREROLL_FRAMES {
                            preroll.pop_front();
                        }
                        preroll.push_back(frame);
                    }
                }
                Mic::Streaming => {
                    send_audio(&mut ws, &frame)?;
                    streamed_frames += 1;
                    match vad.feed(&frame) {
                        Some(VadEvent::SpeechStart) => speech_seen = true,
                        Some(VadEvent::SpeechEnd) => {
                            send_json(&mut ws, &json!({"type": "utterance.end"}))?;
                            stop_streaming(&mut mic, deps);
                            continue;
                        }
                        None => {}
                    }
                    let timed_out = (!speech_seen && streamed_frames >= NO_SPEECH_FRAMES)
                        || streamed_frames >= MAX_UTTERANCE_FRAMES;
                    if timed_out {
                        tracing::debug!(streamed_frames, speech_seen, "utterance timeout");
                        send_json(&mut ws, &json!({"type": "utterance.end"}))?;
                        stop_streaming(&mut mic, deps);
                    }
                }
            }
        }
    }
}

fn enter_streaming(mic: &mut Mic, vad: &mut Vad, streamed: &mut u32, speech_seen: &mut bool) {
    *mic = Mic::Streaming;
    *vad = Vad::with_hangover(UTTERANCE_HANGOVER_FRAMES);
    *streamed = 0;
    *speech_seen = false;
}

fn stop_streaming(mic: &mut Mic, deps: &mut Deps) {
    *mic = Mic::Idle;
    // Forget the utterance so the detector cannot wake on its own tail.
    deps.detector.reset();
}

/// The wake acknowledgement is robotd's job first (ADR 0003): a chirp
/// from the duck itself, independent of the bridge round-trip. Returns
/// whether the robot actually accepted it.
fn chirp(control: &mut Option<Control>) -> bool {
    use duck_ipc_proto as proto;
    let Some(c) = control else { return false };
    let call = proto::Call::RobotSound(proto::SoundParams {
        tag: proto::SoundTag::Chirp,
        hold: None,
    });
    match c.intent(&call) {
        Ok(result) if result.accepted => true,
        Ok(result) => {
            tracing::debug!(reason = ?result.reason, "chirp refused");
            false
        }
        Err(e) => {
            tracing::warn!(error = %e, "robotd lost — continuing without it");
            *control = None;
            false
        }
    }
}

/// Local fallback wake acknowledgement, so the user hears the duck is
/// listening even when robotd cannot quack (dev machine, empty bank).
fn wake_ack(deps: &mut Deps) {
    let result = match &deps.config.audio.wake_sound {
        Some(path) => deps.player.play_wav(std::path::Path::new(path)),
        None => deps
            .player
            .play_pcm(quacksat_core::playback::wake_ack_pcm()),
    };
    if let Err(e) = result {
        tracing::debug!(error = %e, "wake ack not played");
    }
}

fn send_json(ws: &mut Ws, event: &Value) -> anyhow::Result<()> {
    ws.send(Message::text(event.to_string()))?;
    Ok(())
}

fn send_audio(ws: &mut Ws, frame: &[i16]) -> anyhow::Result<()> {
    debug_assert!(frame.len() <= FRAME_SAMPLES);
    let bytes: Vec<u8> = frame.iter().flat_map(|s| s.to_le_bytes()).collect();
    ws.send(Message::binary(bytes))?;
    Ok(())
}
