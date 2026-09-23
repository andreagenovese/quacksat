//! One turn's worth of listening: when the mic counts, how long it stays
//! open, and what closes it.
//!
//! The rule the duck needs, and the one a bare VAD does not give: after
//! the wake acknowledgement the microphone stays open for a **minimum
//! window** whatever it hears, and only then does the end of speech close
//! the turn. Without the minimum, the duck's own acknowledgement —
//! leaving the speaker a hand's width from the single microphone, with no
//! echo cancellation (ADR 0003) — trips the VAD, the hangover expires
//! half a second later, and the turn closes on a recording of the quack
//! before the person has said anything. What reaches the transcriber then
//! is a second of nothing, and whisper answers a second of nothing with
//! its subtitle-credits hallucination.
//!
//! So: silence for the whole window is a silent turn, speech is waited
//! out to its end however long that takes, and a blip in the first
//! moments is neither.

use crate::vad::{Vad, VadEvent};

/// Frames are [`crate::audio::FRAME_SAMPLES`] at [`crate::audio::PIPELINE_RATE`]: 32 ms.
const FRAME_MS: u32 = 32;

/// Silence after speech that ends the turn — 0.8 s, long enough to speak
/// through a comma.
pub const HANGOVER_FRAMES: u32 = 25;
/// The mic stays open at least this long after the acknowledgement,
/// whatever it hears: 3 s, a person's pause between "hey Jarvis" and
/// what they actually want.
pub const MIN_LISTEN_FRAMES: u32 = 3_000 / FRAME_MS;
/// Nothing said by now and the turn was a false alarm (~6 s).
pub const NO_SPEECH_FRAMES: u32 = 187;
/// Nobody dictates to a duck (~15 s).
pub const MAX_UTTERANCE_FRAMES: u32 = 469;
/// Frames to throw away after the duck's own voice stops (~320 ms).
///
/// The playback program exits while the sound system is still emptying
/// its buffer, so "the program has gone" is not "the room is quiet", and
/// the frames in between carry the duck's voice into its own microphone.
///
/// Counted in frames rather than milliseconds on purpose: the echo is
/// made of audio, and audio is what frames measure. A wall-clock tail
/// says something different depending on how fast frames arrive — which
/// is how the first version of this passed on the robot and threw away a
/// whole burst-fed utterance in the tests.
pub const TAIL_FRAMES: u32 = 10;

/// What the frame just fed means for the turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Listened {
    /// Keep listening.
    Open,
    /// The utterance is complete: send what was collected.
    Done,
    /// The window passed with nothing said.
    Silent,
}

/// The listening window of one turn.
pub struct Listening {
    vad: Vad,
    frames: u32,
    speech_seen: bool,
    /// Frames still to be thrown away because the duck was talking.
    deaf: u32,
}

impl Default for Listening {
    fn default() -> Self {
        Self::new()
    }
}

impl Listening {
    pub fn new() -> Self {
        Self {
            vad: Vad::with_hangover(HANGOVER_FRAMES),
            frames: 0,
            speech_seen: false,
            deaf: 0,
        }
    }

    /// Whether anything has been taken for speech yet — the caller needs
    /// it to decide whether a closed turn deserves an answer.
    pub fn speech_seen(&self) -> bool {
        self.speech_seen
    }

    /// Frames counted into this turn (the acknowledgement's own frames
    /// are the caller's to drop before they get here).
    pub fn frames(&self) -> u32 {
        self.frames
    }

    /// The duck is talking: this frame is its own voice, and so are the
    /// [`TAIL_FRAMES`] after its player falls quiet. Call it instead of
    /// [`Listening::feed`] for every frame taken while
    /// [`crate::playback::Player::is_playing`] holds.
    pub fn deafen(&mut self) {
        self.deaf = TAIL_FRAMES;
    }

    pub fn feed(&mut self, frame: &[i16]) -> Listened {
        if self.deaf > 0 {
            self.deaf -= 1;
            return Listened::Open;
        }
        self.frames += 1;
        if let Some(VadEvent::SpeechStart) = self.vad.feed(frame) {
            self.speech_seen = true;
        }
        if self.frames >= MAX_UTTERANCE_FRAMES {
            return Listened::Done;
        }
        // Before the minimum window nothing closes the turn: an early
        // end of speech is the room, or the duck itself.
        if self.frames < MIN_LISTEN_FRAMES {
            return Listened::Open;
        }
        if self.speech_seen {
            // Speech is waited out to its end, however far past the
            // window that falls.
            if self.vad.in_speech() {
                Listened::Open
            } else {
                Listened::Done
            }
        } else if self.frames >= NO_SPEECH_FRAMES {
            Listened::Silent
        } else {
            Listened::Open
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::FRAME_SAMPLES;

    fn quiet() -> Vec<i16> {
        vec![5i16; FRAME_SAMPLES]
    }

    fn loud() -> Vec<i16> {
        (0..FRAME_SAMPLES)
            .map(|i| if i % 2 == 0 { 9000 } else { -9000 })
            .collect()
    }

    fn feed_n(listening: &mut Listening, frame: &[i16], n: u32) -> Option<Listened> {
        for _ in 0..n {
            match listening.feed(frame) {
                Listened::Open => {}
                other => return Some(other),
            }
        }
        None
    }

    #[test]
    fn a_room_that_says_nothing_is_a_silent_turn() {
        let mut listening = Listening::new();
        let outcome = feed_n(&mut listening, &quiet(), NO_SPEECH_FRAMES + 5);
        assert_eq!(outcome, Some(Listened::Silent));
        assert!(!listening.speech_seen());
    }

    /// The failure this module exists for: the duck hears its own
    /// acknowledgement, the VAD calls it speech and then silence — and
    /// the turn must NOT close on it, because the person has not spoken
    /// yet.
    #[test]
    fn its_own_ack_does_not_close_the_turn() {
        let mut listening = Listening::new();
        // A blip of "speech", then quiet, all inside the first second.
        assert_eq!(feed_n(&mut listening, &loud(), 6), None);
        assert_eq!(feed_n(&mut listening, &quiet(), HANGOVER_FRAMES + 2), None);
        // Still open well past the hangover that would have ended it.
        assert!(listening.frames() < MIN_LISTEN_FRAMES);

        // Now the person speaks, and the turn ends on *their* silence.
        assert_eq!(feed_n(&mut listening, &loud(), 40), None);
        let outcome = feed_n(&mut listening, &quiet(), HANGOVER_FRAMES + 40);
        assert_eq!(outcome, Some(Listened::Done));
        assert!(listening.speech_seen());
    }

    #[test]
    fn a_short_answer_waits_for_the_window_then_closes() {
        let mut listening = Listening::new();
        assert_eq!(feed_n(&mut listening, &loud(), 10), None);
        // Speech ended long before the window; the turn closes when the
        // window does, not a frame earlier.
        let outcome = feed_n(&mut listening, &quiet(), MIN_LISTEN_FRAMES);
        assert_eq!(outcome, Some(Listened::Done));
        assert!(listening.frames() >= MIN_LISTEN_FRAMES);
    }

    #[test]
    fn someone_still_talking_at_the_window_is_not_cut_off() {
        let mut listening = Listening::new();
        // Talking through the window and well past it.
        assert_eq!(feed_n(&mut listening, &loud(), MIN_LISTEN_FRAMES + 60), None);
        let outcome = feed_n(&mut listening, &quiet(), HANGOVER_FRAMES + 40);
        assert_eq!(outcome, Some(Listened::Done));
    }

    /// The frames right after the duck stops talking are still its own
    /// voice, and a turn must not be started — or ended — on them.
    #[test]
    fn the_tail_of_its_own_voice_is_not_heard() {
        let mut listening = Listening::new();
        // While the player runs, every frame is the duck's.
        for _ in 0..5 {
            listening.deafen();
        }
        // The player falls quiet, but the speaker has not: the next
        // frames are loud and must still count for nothing.
        let outcome = feed_n(&mut listening, &loud(), TAIL_FRAMES);
        assert_eq!(outcome, None);
        assert!(!listening.speech_seen(), "that was the duck, not a person");
        assert_eq!(listening.frames(), 0, "and none of it was this turn's");

        // One more frame and the microphone is the person's again.
        assert_eq!(feed_n(&mut listening, &loud(), 20), None);
        assert!(listening.speech_seen());
    }

    #[test]
    fn nobody_dictates_to_a_duck() {
        let mut listening = Listening::new();
        let outcome = feed_n(&mut listening, &loud(), MAX_UTTERANCE_FRAMES + 10);
        assert_eq!(outcome, Some(Listened::Done));
    }
}
