//! The satellite state machine, one Home Assistant connection at a time.
//!
//! Flow (mirrors rhasspy's wyoming-satellite in local-wake mode):
//! `describe` → `info`; `run-satellite` arms the wake word; on wake the
//! satellite sends `detection` + `run-pipeline` (asr → tts) and streams mic
//! chunks; a `transcript` stops the streaming; TTS comes back as
//! `audio-start`/`audio-chunk`/`audio-stop`, played half-duplex (mic frames
//! are dropped while the duck speaks — ADR 0003), answered with `played`.

use std::io::{BufReader, Read, Write};
use std::sync::mpsc;

use duck_ipc_proto as proto;
use quacksat_core::audio::{FRAME_SAMPLES, PIPELINE_RATE};
use quacksat_core::config::Config;
use quacksat_core::playback::Player;
use quacksat_core::robotd::Control;
use quacksat_core::wake::WakeDetector;
use serde_json::{Value, json};

use crate::protocol::{Event, read_event, write_event};

/// Everything a connection consumes; owned by the caller so state that must
/// outlive one HA connection (mic, player, robot) survives reconnects.
pub struct Deps<'a> {
    pub config: &'a Config,
    pub frames: &'a mpsc::Receiver<Vec<i16>>,
    pub detector: &'a mut dyn WakeDetector,
    pub player: &'a mut Player,
    pub control: &'a mut Option<Control>,
}

/// Unified input: mic frames and server events race into one channel so a
/// blocking std loop can serve both without an async runtime.
enum Input {
    Frame(Vec<i16>),
    Event(Event),
    Disconnected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// Connected, but HA has not said `run-satellite` (or said `pause-`).
    Paused,
    /// Armed: feeding the wake detector, not streaming.
    Waiting,
    /// Wake fired: forwarding mic chunks to the pipeline.
    Streaming,
}

pub fn serve(config: &Config, mut deps: Deps) -> anyhow::Result<()> {
    let listener = std::net::TcpListener::bind(&config.wyoming.bind)?;
    tracing::info!(bind = %config.wyoming.bind, "wyoming satellite listening");

    loop {
        let (stream, peer) = listener.accept()?;
        tracing::info!(%peer, "home assistant connected");
        match run_connection(stream, &mut deps) {
            Ok(()) => tracing::info!(%peer, "home assistant disconnected"),
            Err(e) => tracing::warn!(%peer, error = %e, "connection failed"),
        }
        deps.player.stop();
        // Frames that piled up while nobody consumed them are stale.
        while deps.frames.try_recv().is_ok() {}
    }
}

/// Serve one connection until the peer goes away. Generic over the stream
/// so tests drive it with a local socket pair.
pub fn run_connection<S>(stream: S, deps: &mut Deps) -> anyhow::Result<()>
where
    S: Read + Write + TryClone + Send + 'static,
{
    let reader_half = stream.try_clone()?;
    let mut writer = stream;

    // Server events arrive from a reader thread; mic frames are polled with
    // a short timeout in the loop itself — std mpsc has no select, and 50 ms
    // of event latency is invisible next to a spoken exchange.
    let (event_tx, rx) = mpsc::channel();
    let reader_thread = std::thread::spawn(move || {
        let mut reader = BufReader::new(reader_half);
        loop {
            match read_event(&mut reader) {
                Ok(Some(event)) => {
                    if event_tx.send(Input::Event(event)).is_err() {
                        return;
                    }
                }
                Ok(None) | Err(_) => {
                    let _ = event_tx.send(Input::Disconnected);
                    return;
                }
            }
        }
    });

    let result = event_loop(&mut writer, &rx, deps);

    // Dropping rx ends the reader thread on its next send.
    drop(rx);
    let _ = reader_thread;
    result
}

fn event_loop(
    writer: &mut impl Write,
    rx: &mpsc::Receiver<Input>,
    deps: &mut Deps,
) -> anyhow::Result<()> {
    let mut mode = Mode::Paused;
    let mut preroll = std::collections::VecDeque::with_capacity(PREROLL_FRAMES);

    loop {
        // Poll server events first, then mic frames, so a transcript stops
        // the streaming before more chunks go out.
        let input = match rx.try_recv() {
            Ok(input) => input,
            Err(mpsc::TryRecvError::Empty) => match deps.frames.recv_timeout(POLL) {
                Ok(frame) => Input::Frame(frame),
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    anyhow::bail!("mic capture channel closed")
                }
            },
            Err(mpsc::TryRecvError::Disconnected) => return Ok(()),
        };

        match input {
            Input::Disconnected => return Ok(()),
            Input::Event(event) => {
                if handle_event(writer, &event, &mut mode, deps)? == Flow::Closed {
                    return Ok(());
                }
            }
            Input::Frame(frame) => handle_frame(writer, &frame, &mut mode, &mut preroll, deps)?,
        }
    }
}

const POLL: std::time::Duration = std::time::Duration::from_millis(50);

#[derive(PartialEq)]
enum Flow {
    Open,
    Closed,
}

fn handle_event(
    writer: &mut impl Write,
    event: &Event,
    mode: &mut Mode,
    deps: &mut Deps,
) -> anyhow::Result<Flow> {
    match event.event_type.as_str() {
        "describe" => write_event(writer, &info_event(deps.config))?,
        "run-satellite" => {
            if *mode == Mode::Paused {
                *mode = Mode::Waiting;
                tracing::info!("satellite running — waiting for wake word");
            }
        }
        "pause-satellite" => {
            *mode = Mode::Paused;
            deps.player.stop();
            tracing::info!("satellite paused by home assistant");
        }
        "ping" => write_event(writer, &Event::new("pong", event.data.clone()))?,
        "transcript" => {
            if *mode == Mode::Streaming {
                *mode = Mode::Waiting;
                // Forget the wake phrase still sitting in the detector's
                // buffers, or it re-triggers on itself when listening resumes.
                deps.detector.reset();
            }
            let text = event.data.get("text").and_then(Value::as_str).unwrap_or("");
            tracing::info!(text, "transcript");
        }
        "audio-start" => {
            let rate = event
                .data
                .get("rate")
                .and_then(Value::as_u64)
                .unwrap_or(22_050) as u32;
            let channels = event
                .data
                .get("channels")
                .and_then(Value::as_u64)
                .unwrap_or(1) as u16;
            tracing::info!(rate, channels, "tts playback starting");
            if let Err(e) = deps.player.begin_stream(rate, channels) {
                tracing::warn!(error = %e, "speaker unavailable — tts dropped");
            }
        }
        "audio-chunk" => deps.player.write_stream(&event.payload),
        "audio-stop" => {
            // Half-duplex: block until the speaker drains, then flush the
            // mic frames that arrived while the duck was talking.
            deps.player.end_stream();
            while deps.frames.try_recv().is_ok() {}
            deps.detector.reset();
            write_event(writer, &Event::new("played", Value::Null))?;
            tracing::info!("tts played");
        }
        "error" => {
            let text = event.data.get("text").and_then(Value::as_str).unwrap_or("");
            tracing::warn!(text, "pipeline error");
            if *mode == Mode::Streaming {
                *mode = Mode::Waiting;
                deps.detector.reset();
            }
        }
        other => tracing::debug!(event = other, "ignored wyoming event"),
    }
    Ok(Flow::Open)
}

/// Pre-roll depth: ~320 ms of audio kept while waiting, flushed on wake so
/// the attack of the first word reaches ASR (the energy threshold trips a
/// frame or two after speech actually starts, and Whisper mangles a word
/// whose first phoneme is missing).
const PREROLL_FRAMES: usize = 10;

fn handle_frame(
    writer: &mut impl Write,
    frame: &[i16],
    mode: &mut Mode,
    preroll: &mut std::collections::VecDeque<Vec<i16>>,
    deps: &mut Deps,
) -> anyhow::Result<()> {
    match mode {
        Mode::Paused => {}
        Mode::Waiting => {
            // Suppress the wake word while the duck itself is talking.
            if deps.player.is_playing() {
                return Ok(());
            }
            if deps.detector.feed(frame) {
                tracing::info!("wake");
                if !chirp(deps.control) {
                    wake_ack(deps);
                }
                write_event(
                    writer,
                    &Event::new("detection", json!({"name": "quacksat"})),
                )?;
                write_event(
                    writer,
                    &Event::new(
                        "run-pipeline",
                        json!({
                            "start_stage": "asr",
                            "end_stage": "tts",
                            "restart_on_end": false,
                        }),
                    ),
                )?;
                write_event(writer, &Event::new("audio-start", audio_format()))?;
                *mode = Mode::Streaming;
                for buffered in preroll.drain(..) {
                    write_chunk(writer, &buffered)?;
                }
                write_chunk(writer, frame)?;
            } else {
                if preroll.len() == PREROLL_FRAMES {
                    preroll.pop_front();
                }
                preroll.push_back(frame.to_vec());
            }
        }
        Mode::Streaming => {
            // Skip frames while the local wake ack rings (no AEC).
            if !deps.player.is_playing() {
                write_chunk(writer, frame)?;
            }
        }
    }
    Ok(())
}

fn write_chunk(writer: &mut impl Write, frame: &[i16]) -> std::io::Result<()> {
    let bytes: Vec<u8> = frame.iter().flat_map(|s| s.to_le_bytes()).collect();
    write_event(
        writer,
        &Event::with_payload("audio-chunk", audio_format(), bytes),
    )
}

fn audio_format() -> Value {
    json!({"rate": PIPELINE_RATE, "width": 2, "channels": 1})
}

/// The expressive cue stays robotd's job (ADR 0003): a chirp acknowledges
/// the wake word while TTS remains quacksat's own aplay. Returns whether
/// the robot actually accepted it.
fn chirp(control: &mut Option<Control>) -> bool {
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

fn info_event(config: &Config) -> Event {
    Event::new(
        "info",
        json!({
            "asr": [], "tts": [], "handle": [], "intent": [], "wake": [],
            "satellite": {
                "name": config.wyoming.name,
                "attribution": {
                    "name": "quacksat",
                    "url": "https://github.com/andreagenovese/quacksat",
                },
                "installed": true,
                "description": "Mobile voice satellite on the Pollen Robotics Microduck",
                "version": env!("CARGO_PKG_VERSION"),
                "area": config.wyoming.area,
                "has_vad": false,
            },
        }),
    )
}

/// `TcpStream::try_clone` shaped as a trait so tests can drive
/// [`run_connection`] with any duplex stream.
pub trait TryClone: Sized {
    fn try_clone(&self) -> std::io::Result<Self>;
}

impl TryClone for std::net::TcpStream {
    fn try_clone(&self) -> std::io::Result<Self> {
        std::net::TcpStream::try_clone(self)
    }
}

impl TryClone for std::os::unix::net::UnixStream {
    fn try_clone(&self) -> std::io::Result<Self> {
        std::os::unix::net::UnixStream::try_clone(self)
    }
}

const _: () = {
    // FRAME_SAMPLES is what write_chunk ships per event; keep the constant
    // visible here so a resize is a conscious wire-format decision.
    assert!(FRAME_SAMPLES == 512);
};
