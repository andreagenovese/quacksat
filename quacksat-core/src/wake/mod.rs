//! Wake-word detection behind a trait: the openWakeWord detector for
//! production, the energy detector for bring-up, all interchangeable to
//! the pipeline.

pub mod oww;

use crate::config::{WakeConfig, WakeMode};
use crate::vad::{Vad, VadEvent};

pub trait WakeDetector: Send {
    /// Feed one 16 kHz mono frame; `true` means the wake word fired.
    fn feed(&mut self, frame: &[i16]) -> bool;

    /// Forget all buffered audio. Called when the pipeline resumes
    /// listening after a conversation turn, so the detector cannot
    /// re-trigger on its own stale wake phrase.
    fn reset(&mut self) {}

    /// Score of the detection that made the last `feed` return true, if
    /// the detector produces one. The bridge uses it for multi-duck wake
    /// arbitration (highest score = duck closest to the speaker).
    fn last_score(&self) -> Option<f32> {
        None
    }
}

pub fn from_config(config: &WakeConfig) -> anyhow::Result<Box<dyn WakeDetector>> {
    Ok(match config.mode {
        WakeMode::Openwakeword => Box::new(oww::OpenWakeWord::load(config)?),
        WakeMode::Energy => Box::new(EnergyWake::new()),
        WakeMode::Disabled => Box::new(NeverWake),
    })
}

/// Bring-up detector: wakes on a speech onset preceded by at least a second
/// of silence. Every utterance "wakes" — useful for exercising the pipeline
/// end to end, useless as a product.
pub struct EnergyWake {
    vad: Vad,
    silent_frames: u32,
}

/// ~1 s of silence (32 ms frames) required before an onset counts.
const MIN_SILENCE_FRAMES: u32 = 31;

impl EnergyWake {
    pub fn new() -> Self {
        Self {
            vad: Vad::new(),
            silent_frames: MIN_SILENCE_FRAMES,
        }
    }
}

impl Default for EnergyWake {
    fn default() -> Self {
        Self::new()
    }
}

impl WakeDetector for EnergyWake {
    fn feed(&mut self, frame: &[i16]) -> bool {
        let event = self.vad.feed(frame);
        match event {
            Some(VadEvent::SpeechStart) => {
                let woke = self.silent_frames >= MIN_SILENCE_FRAMES;
                self.silent_frames = 0;
                woke
            }
            _ => {
                if !self.vad.in_speech() {
                    self.silent_frames = self.silent_frames.saturating_add(1);
                }
                false
            }
        }
    }
}

pub struct NeverWake;

impl WakeDetector for NeverWake {
    fn feed(&mut self, _frame: &[i16]) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::FRAME_SAMPLES;

    fn frame(amplitude: i16) -> Vec<i16> {
        (0..FRAME_SAMPLES)
            .map(|i| if i % 2 == 0 { amplitude } else { -amplitude })
            .collect()
    }

    #[test]
    fn wakes_on_onset_after_silence_not_mid_conversation() {
        let mut wake = EnergyWake::new();
        // Fresh start counts as silence: first onset wakes.
        assert!(wake.feed(&frame(8000)));
        // Speech continues: no re-trigger.
        for _ in 0..5 {
            assert!(!wake.feed(&frame(8000)));
        }
        // Short pause (less than a second) then speech again: no wake.
        for _ in 0..15 {
            assert!(!wake.feed(&frame(10)));
        }
        assert!(!wake.feed(&frame(8000)));
        // Long silence then speech: wake again.
        for _ in 0..60 {
            assert!(!wake.feed(&frame(10)));
        }
        assert!(wake.feed(&frame(8000)));
    }

    #[test]
    fn disabled_never_wakes() {
        let mut wake = NeverWake;
        for _ in 0..100 {
            assert!(!wake.feed(&frame(20_000)));
        }
    }
}
