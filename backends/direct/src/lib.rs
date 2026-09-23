//! Direct backend: the self-contained duck. The satellite itself runs
//! STT → LLM (tool calling) → TTS against three OpenAI-dialect
//! endpoints (`[direct]` config) — no bridge, no home server, no
//! containers. Robot tools execute in-process behind the same allowlist
//! as the agent backend (`quacksat_core::tools`).
//!
//! Trade-offs versus the bridge (documented in
//! docs/agent-backend-plan.md): no MCP server for external agents, the
//! conversation dies with the process, nothing is shared between ducks.

pub mod mcp;
mod openai;
mod speakable;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;

use duck_ipc_proto as proto;
use quacksat_core::config::Config;
use quacksat_core::playback::Player;
use quacksat_core::thinking::ThinkingPose;
use quacksat_core::tools::{self, Robot};
use quacksat_core::vad::{Vad, VadEvent};
use quacksat_core::wake;
use serde_json::{Value, json};

/// Same utterance segmentation as the agent backend.
const UTTERANCE_HANGOVER_FRAMES: u32 = 25;
const NO_SPEECH_FRAMES: u32 = 187;
const MAX_UTTERANCE_FRAMES: u32 = 469;

pub fn run(config: &Config, frames: mpsc::Receiver<Vec<i16>>) -> anyhow::Result<()> {
    let control: mcp::SharedRobot =
        std::sync::Arc::new(std::sync::Mutex::new(Robot::connect(config)));
    if config.direct.mcp.enabled {
        anyhow::ensure!(
            !config.direct.mcp.token.is_empty(),
            "[direct.mcp] token is mandatory when the MCP server is enabled \
             (an HTTP server accepting motion commands never runs open)"
        );
        let listener =
            std::net::TcpListener::bind((config.direct.mcp.bind.as_str(), config.direct.mcp.port))?;
        tracing::info!(
            bind = %config.direct.mcp.bind, port = config.direct.mcp.port,
            "mcp server listening (POST /mcp, bearer auth)"
        );
        let mcp_control = control.clone();
        let token = config.direct.mcp.token.clone();
        std::thread::spawn(move || mcp::serve(listener, mcp_control, token));
    }
    // Waking up in a house the duck already knows is the navigation
    // daemon's business since the split (2026-09-22): quack-navd runs
    // its own homecoming at startup.
    let mut player = match &config.audio.playback_program {
        Some(program) => Player::with_program(&config.audio.playback_device, program),
        None => Player::new(&config.audio.playback_device),
    };
    let mut detector = wake::from_config(&config.wake)?;
    let mut history: Vec<Value> = Vec::new();

    tracing::info!(
        llm = %config.direct.llm.base_url,
        stt = %config.direct.stt.base_url,
        tts = %config.direct.tts.base_url,
        "direct backend ready — waiting for wake word"
    );

    loop {
        // Idle: feed the wake detector until it fires — unless the duck,
        // mapping on its own, has reached a place it has no name for and
        // wants to ask.
        let Ok(frame) = frames.recv() else {
            anyhow::bail!("capture channel closed");
        };
        // The duck asking "where am I?" while it maps: the daemon
        // raises it, the satellite speaks it (the split of 2026-09-22).
        if let Some(question) = mcp::lock(&control).nav.as_mut().and_then(|nav| nav.take_question()) {
            let phrase = question.phrase.clone();
            tracing::info!(pose = ?question.pose, %phrase, "asking where we are");
            speak(config, &mut player, &phrase);
            // The turn is the user's answer to a question the duck asked:
            // put the question on the record so the model reads the answer
            // as a name for this spot and remembers it.
            history.push(json!({"role": "assistant", "content": phrase}));
            while frames.try_recv().is_ok() {}
            match record_utterance(&frames, &mut player)? {
                Some(utterance) => {
                    match run_turn_posed(config, &utterance, &mut history, &control)
                    {
                        Ok(Some(reply)) => speak(config, &mut player, &reply),
                        Ok(None) => tracing::info!("no answer to the question"),
                        Err(e) => tracing::warn!(error = %e, "turn failed"),
                    }
                }
                None => tracing::info!("nobody answered where we are"),
            }
            while frames.try_recv().is_ok() {}
            detector.reset();
            continue;
        }
        if !detector.feed(&frame) {
            continue;
        }
        tracing::info!("wake");
        if !chirp(&control) {
            wake_ack(config, &mut player);
        }

        loop {
            let Some(utterance) = record_utterance(&frames, &mut player)? else {
                break; // silence — back to the wake word
            };
            match run_turn_posed(config, &utterance, &mut history, &control) {
                Ok(Some(reply)) => {
                    speak(config, &mut player, &reply);
                    while frames.try_recv().is_ok() {}
                    detector.reset();
                    if config.direct.follow_up {
                        tracing::info!("listening (follow-up turn)");
                        continue;
                    }
                }
                Ok(None) => {
                    tracing::info!("turn: nothing recognized");
                    sad_ack(&control, &mut player);
                }
                Err(e) => {
                    tracing::warn!(error = %e, "turn failed");
                    sad_ack(&control, &mut player);
                }
            }
            break;
        }
        detector.reset();
    }
}

/// Record one utterance from the mic: VAD-segmented, with the same
/// no-speech and max-length guards as the agent backend.
fn record_utterance(
    frames: &mpsc::Receiver<Vec<i16>>,
    player: &mut Player,
) -> anyhow::Result<Option<Vec<i16>>> {
    let mut vad = Vad::with_hangover(UTTERANCE_HANGOVER_FRAMES);
    let mut audio: Vec<i16> = Vec::new();
    let mut speech_seen = false;
    let mut count: u32 = 0;

    loop {
        let Ok(frame) = frames.recv() else {
            anyhow::bail!("capture channel closed");
        };
        // The wake ack is still sounding: the mic hears it (no AEC) and
        // the VAD would take it for speech and close the turn early.
        if player.is_playing() {
            continue;
        }
        audio.extend_from_slice(&frame);
        count += 1;
        match vad.feed(&frame) {
            Some(VadEvent::SpeechStart) => speech_seen = true,
            Some(VadEvent::SpeechEnd) => return Ok(Some(audio)),
            None => {}
        }
        if !speech_seen && count >= NO_SPEECH_FRAMES {
            return Ok(None);
        }
        if count >= MAX_UTTERANCE_FRAMES {
            return Ok(Some(audio));
        }
    }
}

/// Run one turn with the thinking pose alongside: the STT → LLM → TTS
/// round is blocking HTTP, so the pose ticks from a scoped thread through
/// the shared robotd handle. Once a tool executes the body is the
/// agent's: the pose stops without recentering (`acting`, the same
/// semantics as `ThinkingPose::release`).
fn run_turn_posed(
    config: &Config,
    utterance: &[i16],
    history: &mut Vec<Value>,
    control: &mcp::SharedRobot,
) -> anyhow::Result<Option<String>> {
    let stop = AtomicBool::new(false);
    let acting = AtomicBool::new(false);
    std::thread::scope(|scope| {
        scope.spawn(|| {
            let mut pose = ThinkingPose::from_config(&config.thinking);
            pose.begin();
            while !stop.load(Ordering::Relaxed) {
                if acting.load(Ordering::Relaxed) {
                    pose.release();
                }
                {
                    let mut guard = mcp::lock(control);
                    pose.tick(&mut guard.lane.control);
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            let mut guard = mcp::lock(control);
            pose.end(&mut guard.lane.control);
        });
        let result = run_turn(config, utterance, history, control, &acting);
        stop.store(true, Ordering::Relaxed);
        result
    })
}

fn run_turn(
    config: &Config,
    utterance: &[i16],
    history: &mut Vec<Value>,
    control: &mcp::SharedRobot,
    acting: &AtomicBool,
) -> anyhow::Result<Option<String>> {
    let wav = openai::pcm_to_wav(utterance, quacksat_core::audio::PIPELINE_RATE);
    let text = openai::transcribe(&config.direct.stt, wav)?;
    if text.is_empty() {
        return Ok(None);
    }
    tracing::info!(text = %text, "user");
    history.push(json!({"role": "user", "content": text}));

    let llm = &config.direct.llm;
    // Built here rather than held from startup: the skill list belongs
    // to the robot, and one that restarted under us may come back with a
    // different one (`Robot::redial`).
    let tools_catalog = {
        let mut robot = mcp::lock(control);
        robot.redial();
        openai::openai_tools(&tools::catalog(&robot))
    };
    let tools = llm.tool_calling.then_some(&tools_catalog);
    let mut reply = None;
    for _ in 0..llm.max_tool_rounds {
        let mut messages = vec![json!({"role": "system", "content": llm.system_prompt})];
        messages.extend(history.iter().cloned());
        let (content, tool_calls) = openai::complete(llm, &messages, tools)?;
        if tool_calls.is_empty() {
            reply = content;
            break;
        }
        history.push(json!({"role": "assistant", "content": content, "tool_calls": tool_calls}));
        for call in &tool_calls {
            let name = call
                .pointer("/function/name")
                .and_then(Value::as_str)
                .unwrap_or("");
            // The catalog projected dots to underscores; map back.
            let wire_name = name.replacen('_', ".", 1);
            let args: Value = call
                .pointer("/function/arguments")
                .and_then(Value::as_str)
                .and_then(|raw| serde_json::from_str(raw).ok())
                .unwrap_or_else(|| json!({}));
            tracing::info!(tool = %wire_name, %args, "tool call");
            // robot.head / robot.look pose the head on purpose: the
            // thinking sway must not clobber them. Any other tool
            // leaves the head free and the sway keeps going.
            if matches!(wire_name.as_str(), "robot.head" | "robot.look") {
                acting.store(true, Ordering::Relaxed);
            }
            let mut guard = mcp::lock(control);
            let result = match tools::execute(&wire_name, &args, &mut guard) {
                Ok(data) => json!({"ok": true, "data": data}),
                Err(error) => {
                    tracing::info!(%error, "tool refused");
                    json!({"ok": false, "error": error})
                }
            };
            history.push(json!({
                "role": "tool",
                "tool_call_id": call.get("id").cloned().unwrap_or(json!("")),
                "content": result.to_string(),
            }));
        }
    }

    let reply = reply.unwrap_or_else(|| "I could not finish that action.".to_string());
    let reply = speakable::speakable(&reply);
    if !reply.is_empty() {
        tracing::info!(text = %reply, "assistant");
        history.push(json!({"role": "assistant", "content": reply}));
    }
    let limit = llm.history_max_messages;
    if history.len() > limit {
        history.drain(..history.len() - limit);
    }
    Ok((!reply.is_empty()).then_some(reply))
}

fn speak(config: &Config, player: &mut Player, text: &str) {
    let (pcm, rate, channels) = match openai::synthesize(&config.direct.tts, text) {
        Ok(audio) => audio,
        Err(e) => {
            tracing::warn!(error = %e, "tts failed");
            return;
        }
    };
    tracing::info!(rate, channels, "tts playback starting");
    if let Err(e) = player.begin_stream(rate, channels) {
        tracing::warn!(error = %e, "speaker unavailable — tts dropped");
        return;
    }
    let bytes: Vec<u8> = pcm.iter().flat_map(|s| s.to_le_bytes()).collect();
    player.write_stream(&bytes);
    player.end_stream();
    tracing::info!("tts played");
}

fn chirp(control: &mcp::SharedRobot) -> bool {
    let mut guard = mcp::lock(control);
    let control = &mut guard.lane.control;
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

/// The give-up sound: robotd's low peck tock, or the local synthesized
/// sigh when the robot cannot play one.
fn sad_ack(control: &mcp::SharedRobot, player: &mut Player) {
    let played = {
        let mut guard = mcp::lock(control);
        quacksat_core::thinking::sad_tock(&mut guard.lane.control)
    };
    if played {
        return;
    }
    if let Err(e) = player.play_pcm(quacksat_core::playback::sad_pcm()) {
        tracing::debug!(error = %e, "sad ack not played");
    }
}

fn wake_ack(config: &Config, player: &mut Player) {
    let result = match &config.audio.wake_sound {
        Some(path) => player.play_wav(std::path::Path::new(path)),
        None => player.play_pcm(quacksat_core::playback::wake_ack_pcm()),
    };
    if let Err(e) = result {
        tracing::debug!(error = %e, "wake ack not played");
    }
}
