# ADR 0002: Two interchangeable voice backends selected at runtime

- Status: accepted
- Date: 2026-08-31

## Context

The duck should serve two setups: a Home Assistant household that wants an
Assist satellite, and an AI-agent setup where conversations are handled by
an external LLM with tool calling (e.g. Arkimede). Building only one of
these would either lock the project into HA or force every user to run a
custom agent stack.

## Decision

quacksat ships one binary with two backends, selected by the `backend` key
in `/etc/robot/quacksat.toml`:

- `wyoming` — registers as a Home Assistant Assist satellite over the
  Wyoming protocol; STT, intent handling, and TTS run in the HA pipeline.
- `agent` — streams audio and events over WebSocket to a bridge running
  STT → LLM (tool calling) → TTS.

Shared plumbing (mic capture, wake word, VAD, speaker output, robotd
client) lives in `quacksat-core`; each backend is its own crate under
`backends/`.

Build order: core + wyoming first, to validate the hardware against an
already-proven HA chain; then agent plus a minimal reference bridge.

The agent protocol is agent-neutral (audio streaming + events + tool
call/result). The bridge in this repo is a minimal reference
("bring your own agent"); the Arkimede integration lives in Arkimede.

## Consequences

- Runtime selection (config, not compile-time features) means one artifact
  to build, sign, and deploy through `updaterd` for both setups.
- The core/backend split forces a clean internal API for the audio pipeline
  and robot tools, which both backends consume.
- The Wyoming path doubles as the hardware validation harness: any audio
  bug found there is a core bug, not an agent-protocol bug.
- Keeping the agent protocol neutral means quacksat never grows an
  Arkimede-specific dependency.
