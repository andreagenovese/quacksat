//! quacksat core: audio capture, wake word, VAD, speaker output, and the
//! robotd client. Backends (wyoming, agent) build on top of this crate.

pub mod audio;
pub mod config;
pub mod playback;
pub mod robotd;
pub mod thinking;
pub mod tools;
pub mod vad;
pub mod wake;
