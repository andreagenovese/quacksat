use std::process::Child;
use std::sync::mpsc::{Receiver, sync_channel};

use anyhow::Context;
use duck_ipc_proto as proto;
use quacksat_core::config::{Backend, Config};
use quacksat_core::robotd::Control;
use quacksat_core::vad::{Vad, VadEvent};
use quacksat_core::{audio, wake};

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with_writer(std::io::stderr)
        .init();

    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| Config::DEFAULT_PATH.to_string());
    let config = Config::load(&path).with_context(|| format!("loading config from {path}"))?;

    tracing::info!(backend = ?config.backend, "quacksat starting");

    let (mut capture_child, frames) = start_capture(&config)?;
    let result = match config.backend {
        Backend::None => run_bringup(&config, frames),
        Backend::Wyoming => quacksat_backend_wyoming::run(&config, frames),
        Backend::Agent => quacksat_backend_agent::run(&config, frames),
    };
    let _ = capture_child.kill();
    let _ = capture_child.wait();
    result
}

/// Start the continuous capture (ADR 0003) and its pump thread; every
/// backend consumes the same 16 kHz mono frame stream.
fn start_capture(config: &Config) -> anyhow::Result<(Child, Receiver<Vec<i16>>)> {
    let (child, stdout) = match &config.audio.capture_command {
        Some(command) => audio::capture::spawn_custom(command)
            .with_context(|| format!("starting capture command {command:?}"))?,
        None => audio::capture::spawn_arecord(&config.audio.capture_device).with_context(|| {
            format!(
                "starting capture on {} (is arecord installed? is pet_detect holding the mic?)",
                config.audio.capture_device
            )
        })?,
    };
    let (tx, rx) = sync_channel(32);
    std::thread::spawn(move || {
        if let Err(e) = audio::capture::pump(stdout, tx) {
            // A held device or a dying arecord surfaces here, once, loudly —
            // never a silent retry duel (ADR 0003).
            tracing::error!(error = %e, "capture stopped");
        }
    });
    match &config.audio.capture_command {
        Some(command) => tracing::info!(?command, "listening (custom capture)"),
        None => tracing::info!(device = %config.audio.capture_device, "listening"),
    }
    Ok((child, rx))
}

/// Bring-up mode: capture → VAD → wake, chirping through robotd on wake.
/// Exercises every core piece against `robotd --fake` or the real robot
/// without needing a voice backend.
fn run_bringup(config: &Config, frames: Receiver<Vec<i16>>) -> anyhow::Result<()> {
    let mut control = match Control::connect(&config.robotd_socket) {
        Ok(control) => Some(control),
        Err(e) => {
            tracing::warn!(error = %e, "robotd unreachable — running without the robot");
            None
        }
    };

    let mut vad = Vad::new();
    let mut detector = wake::from_config(&config.wake)?;

    for frame in frames {
        match vad.feed(&frame) {
            Some(VadEvent::SpeechStart) => tracing::debug!("speech start"),
            Some(VadEvent::SpeechEnd) => tracing::debug!("speech end"),
            None => {}
        }
        if detector.feed(&frame) {
            tracing::info!("wake");
            if let Some(c) = &mut control {
                let chirp = proto::Call::RobotSound(proto::SoundParams {
                    tag: proto::SoundTag::Chirp,
                    hold: None,
                });
                match c.intent(&chirp) {
                    Ok(result) if result.accepted => {}
                    Ok(result) => tracing::info!(reason = ?result.reason, "chirp refused"),
                    Err(e) => {
                        tracing::warn!(error = %e, "robotd lost — continuing without it");
                        control = None;
                    }
                }
            }
        }
    }
    anyhow::bail!("capture channel closed")
}
