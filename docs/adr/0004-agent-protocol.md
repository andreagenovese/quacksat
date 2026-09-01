# ADR 0004: The agent protocol (path B)

- Status: accepted
- Date: 2026-08-31
- Inputs: ADR 0002 (interchangeable backends), ADR 0003 (half-duplex
  audio), `docs/agent-backend-plan.md`, survey of the [Arkimede](https://arkimede.ai/) platform

## Context

Path B connects the satellite to an AI agent: audio out, audio back,
and the agent able to act through tools — both home automation (the
agent's own) and the robot's body. The protocol must be neutral
("bring your own agent"): implementable by Arkimede, by a reference
bridge, or by anything else, without quacksat knowing which.

## Decisions

### 1. WebSocket, not WebRTC (for now)

Text frames carry JSON events; binary frames carry raw PCM audio.
Rationale: the conversation is turn-based and half-duplex by design
(no AEC — ADR 0003), it runs on the home LAN, and a neutral protocol
must be implementable with any language's standard library. WebRTC's
real advantages (lossy-network audio, NAT traversal, sub-50 ms jitter
budgets, video tracks) buy nothing here yet and would price every
implementer into ICE/SDP/DTLS complexity.

The protocol is deliberately transport-agnostic: events map 1:1 onto a
WebRTC datachannel and audio onto a track. Remote operation outside
the LAN, full-duplex barge-in (once an AEC exists), or live video are
the named triggers for adding a WebRTC binding. Not v0.

### 2. Wake word and VAD stay on the satellite

The bridge never sees always-on audio: the satellite streams only
after a local wake (pre-roll included) or when the bridge explicitly
reopens the mic for a follow-up turn (`listen.start` — this is what
gives path B multi-turn conversations without repeating the wake word,
which the Wyoming path cannot do). The satellite closes each utterance
with its local VAD (`utterance.end`); the bridge may also cut it short
(`listen.stop`).

### 3. Half-duplex playback

TTS streams into the satellite's single-child player (ADR 0003); mic
frames are dropped while the duck speaks, and the wake detector is
reset when listening resumes. No barge-in until an AEC exists.

### 4. Tools execute on the satellite, behind an explicit allowlist

The satellite declares its tools in `session.start` (name, description,
JSON-Schema parameters — directly projectable to both OpenAI tool
schemas and MCP tool listings). The bridge/agent requests execution
with `tool.call`; the satellite validates against an exhaustive
allowlist (btd's `permits()` pattern: a new tool fails to compile until
classified) and translates to robotd RPCs.

Safety rules baked into the tool semantics, not left to the agent:

- **`robot.move` is timed**: `{vx, vy, vyaw, duration_s}` with a
  clamped maximum duration. The satellite pumps intents at ≥20 Hz for
  the duration, then stops. An LLM never holds an open throttle;
  robotd's deadman remains the last line of defense.
- Argument ranges are clamped satellite-side (speeds, angles).
- `robot.shutdown`, `robot.enable`, and everything not explicitly
  offered simply does not exist on the wire.
- `robot.get_frame` is declared but returns `unsupported` until mediad
  exposes the camera (keeps the contract ready for the mapping
  roadmap).

### 5. The bridge's tool core is one MCP server

The reference bridge exposes the satellite's declared tools as an MCP
server (HTTP/SSE) and every consumer goes through it: MCP-native
agents (Arkimede) call it from inside their own loop; for plain
chat-completions providers with tool calling, the bridge runs the loop
and executes `tool_calls` through the same MCP layer; bare LLMs get
voice-only chat. One executor, one allowlist, one audit trail.

### 6. AI services are url+key in the OpenAI dialect

The bridge embeds no ML: LLM (`/chat/completions`), STT
(`/audio/transcriptions`), TTS (`/audio/speech`) are three configurable
OpenAI-dialect endpoints. Local privacy-preserving deployments use
existing OpenAI-dialect servers; the Arkimede preset is pure
configuration once Arkimede exposes its audio routes.

### 7. Sessions are stateless on the wire

Every (re)connection starts with `session.start`; the satellite
reconnects with a fixed backoff and re-announces itself. Conversation
memory, turn history, and multi-turn policy are the bridge/agent's
business. Auth is a bearer token on the WebSocket upgrade, optional on
a trusted LAN.

The full wire contract lives in `docs/agent-protocol.md`.

## Consequences

- Any agent platform reachable by WebSocket + three OpenAI-dialect
  URLs can drive the duck; none of them can drive it outside the
  allowlist.
- The satellite stays thin (no ML beyond the wake word) and the 1 GB
  board budget holds.
- No barge-in and no out-of-LAN operation in v0 — both have named
  evolution paths (AEC → full duplex; WebRTC binding → remote).
- The reference bridge carries the orchestration complexity (VAD
  turns, MCP server, provider profiles), keeping both the satellite
  and the agents simple.
