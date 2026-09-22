//! quacksat core: audio capture, wake word, VAD, speaker output, the
//! robotd lane and the robot's own tools — the voice satellite, and
//! nothing else. Navigation (the map, the places, the journeys) is a
//! daemon of its own since 2026-09-22 (`quack-navd`, in the quacknav
//! repo); [`nav_client`] is the lane that reaches it, and the
//! satellite works without it. Backends (wyoming, agent, direct)
//! build on this crate.

pub mod audio;
pub mod body;
pub mod config;
pub mod gait;
pub mod nav_client;
pub mod playback;
pub mod robotd;
pub mod thinking;
pub mod tools;
pub mod vad;
pub mod wake;
