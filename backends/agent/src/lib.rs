//! Agent backend: streams audio and events over WebSocket to a bridge that
//! runs STT → LLM (tool calling) → TTS. The protocol is agent-neutral; a
//! minimal reference bridge lives in bridge/.
//!
//! Not implemented yet — see docs/adr/0002-interchangeable-backends.md.
