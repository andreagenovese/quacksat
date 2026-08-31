//! Wake-word detection behind a trait, so the bring-up detector and the
//! real model-based ones (microWakeWord / openWakeWord — a follow-up task:
//! openWakeWord's ONNX chain via a pure-Rust runtime is the C-dep-free
//! candidate) are interchangeable to the pipeline.

use crate::config::WakeMode;
use crate::vad::{Vad, VadEvent};

pub trait WakeDetector: Send {
    /// Feed one 16 kHz mono frame; `true` means the wake word fired.
    fn feed(&mut self, frame: &[i16]) -> bool;
}

pub fn from_config(mode: WakeMode) -> Box<dyn WakeDetector> {
    match mode {
        WakeMode::Energy => Box::new(EnergyWake::new()),
        WakeMode::Disabled => Box::new(NeverWake),
    }
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
