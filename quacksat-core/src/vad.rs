//! Energy VAD with an adaptive noise floor — the same family of heuristic
//! as pet-detect's SoundSentry. Good enough to gate wake-word evaluation
//! and to segment utterances for the backends; not a speech classifier.

/// Frames of hangover before speech is declared over (~256 ms at 32 ms/frame),
/// so natural pauses inside a sentence don't split it.
const HANGOVER_FRAMES: u32 = 8;
/// Speech threshold as a multiple of the noise floor.
const TRIGGER_RATIO: f32 = 3.0;
/// The floor never adapts below this (i16 RMS units); an anechoic-quiet
/// room must not turn breathing into speech.
const MIN_FLOOR: f32 = 30.0;
/// EMA weight for floor adaptation while not in speech.
const FLOOR_ALPHA: f32 = 0.05;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VadEvent {
    SpeechStart,
    SpeechEnd,
}

pub struct Vad {
    floor: f32,
    in_speech: bool,
    hangover: u32,
}

impl Vad {
    pub fn new() -> Self {
        Self {
            floor: 200.0,
            in_speech: false,
            hangover: 0,
        }
    }

    pub fn in_speech(&self) -> bool {
        self.in_speech
    }

    /// Feed one frame; returns the transition it caused, if any.
    pub fn feed(&mut self, frame: &[i16]) -> Option<VadEvent> {
        let rms = rms(frame);
        let active = rms > self.floor * TRIGGER_RATIO;

        if active {
            // Drift up 20× slower during speech: a sentence barely moves the
            // floor, but a room that turned loud for good stops counting as
            // one endless utterance after a few seconds.
            self.floor = (self.floor + FLOOR_ALPHA / 20.0 * (rms - self.floor)).max(MIN_FLOOR);
        } else {
            self.floor = (self.floor + FLOOR_ALPHA * (rms - self.floor)).max(MIN_FLOOR);
        }

        match (self.in_speech, active) {
            (false, true) => {
                self.in_speech = true;
                self.hangover = HANGOVER_FRAMES;
                Some(VadEvent::SpeechStart)
            }
            (true, true) => {
                self.hangover = HANGOVER_FRAMES;
                None
            }
            (true, false) => {
                self.hangover = self.hangover.saturating_sub(1);
                if self.hangover == 0 {
                    self.in_speech = false;
                    Some(VadEvent::SpeechEnd)
                } else {
                    None
                }
            }
            (false, false) => None,
        }
    }
}

impl Default for Vad {
    fn default() -> Self {
        Self::new()
    }
}

fn rms(frame: &[i16]) -> f32 {
    if frame.is_empty() {
        return 0.0;
    }
    let sum: f64 = frame.iter().map(|&s| (s as f64) * (s as f64)).sum();
    (sum / frame.len() as f64).sqrt() as f32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::FRAME_SAMPLES;

    fn frame(amplitude: i16) -> Vec<i16> {
        // Alternate sign so the RMS equals the amplitude.
        (0..FRAME_SAMPLES)
            .map(|i| if i % 2 == 0 { amplitude } else { -amplitude })
            .collect()
    }

    #[test]
    fn detects_burst_and_end_after_hangover() {
        let mut vad = Vad::new();
        for _ in 0..20 {
            assert_eq!(vad.feed(&frame(50)), None);
        }
        assert_eq!(vad.feed(&frame(8000)), Some(VadEvent::SpeechStart));
        assert!(vad.in_speech());
        let mut end_at = None;
        for i in 0..20 {
            if vad.feed(&frame(50)) == Some(VadEvent::SpeechEnd) {
                end_at = Some(i);
                break;
            }
        }
        assert_eq!(end_at, Some((HANGOVER_FRAMES - 1) as usize));
        assert!(!vad.in_speech());
    }

    #[test]
    fn floor_adapts_to_a_louder_room() {
        let mut vad = Vad::new();
        // A steady 2000-RMS room must stop counting as speech once adapted.
        for _ in 0..200 {
            vad.feed(&frame(2000));
        }
        assert!(!vad.in_speech());
        assert_eq!(vad.feed(&frame(2000)), None);
        // But a shout over that room still triggers.
        assert_eq!(vad.feed(&frame(20_000)), Some(VadEvent::SpeechStart));
    }

    #[test]
    fn silence_never_triggers() {
        let mut vad = Vad::new();
        for _ in 0..100 {
            assert_eq!(vad.feed(&frame(5)), None);
        }
    }
}
