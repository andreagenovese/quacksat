//! Audio capture pipeline (ADR 0003): a continuous `arecord` child on the
//! codec at its native 2ch/48kHz, right channel extracted (the mic is wired
//! to Mic3R only — a mono downmix would average in a dead left channel),
//! FIR-decimated to 16kHz mono frames for VAD, wake word, and STT.

pub mod capture;
pub mod decimate;

/// The codec's native capture format (see docs/study/microduck-mic-path.md).
pub const HW_RATE: u32 = 48_000;
pub const HW_CHANNELS: usize = 2;

/// What the speech pipeline consumes.
pub const PIPELINE_RATE: u32 = 16_000;
/// 32 ms at 16 kHz — the granularity VAD and wake detectors see.
pub const FRAME_SAMPLES: usize = 512;
