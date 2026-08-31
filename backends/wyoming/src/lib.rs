//! Wyoming backend: registers quacksat as a Home Assistant Assist
//! satellite. quacksat listens on `[wyoming].bind`; HA's Wyoming
//! integration connects to it (Settings → Integrations → Wyoming Protocol
//! → host + port). Wake word runs locally; STT, intents, and TTS run in
//! the HA pipeline (ADR 0002).

pub mod protocol;
pub mod satellite;

use std::sync::mpsc;

use quacksat_core::config::Config;
use quacksat_core::playback::Player;
use quacksat_core::robotd::Control;
use quacksat_core::wake;

/// Run the satellite forever: capture is owned by the caller (the frames
/// receiver), robot and speaker are owned here and survive HA reconnects.
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
    let mut detector = wake::from_config(config.wake.mode);

    satellite::serve(
        config,
        satellite::Deps {
            config,
            frames: &frames,
            detector: detector.as_mut(),
            player: &mut player,
            control: &mut control,
        },
    )
}
