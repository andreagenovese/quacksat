# quacksat agent protocol — wire specification v1

Status: v1, implemented by `backends/agent`, `backends/direct`, and the
reference bridge in `bridge/`. Italian copy: `agent-protocol.it.md`.
Audience: implementers of bridges/agents ([Arkimede](https://arkimede.ai/) or anything else).

## Transport

- WebSocket, `ws://` or `wss://`. The satellite is the client.
- Optional auth: `Authorization: Bearer <token>` on the upgrade
  request. A server that rejects auth closes with HTTP 401/403.
- **Text messages** are single JSON objects with a mandatory `"type"`.
- **Binary messages** are raw audio payloads, direction-dependent:
  satellite→server is mic audio; server→satellite is TTS audio. No
  header — format is fixed by `session.start` (mic) and `tts.start`
  (TTS).
- Unknown event types MUST be ignored (log-and-skip), never treated as
  errors. Unknown fields in known events MUST be ignored. This is the
  compatibility rule; there is no version gate.
- Either side may `ping`; the peer answers `pong` echoing the payload.
- A closed connection ends the session. The satellite reconnects with
  a fixed backoff (2 s) and starts a fresh session.

## Session lifecycle

```
satellite                              server (bridge/agent)
    │ ── session.start ──────────────────► │
    │ ◄────────────────── session.ready ── │
    │            (idle: wake word armed)   │
    │ ── wake ───────────────────────────► │
    │ ── [binary mic audio] ─────────────► │   streaming
    │ ── utterance.end ──────────────────► │   (or ◄─ listen.stop)
    │ ◄─────────────────────── tts.start ─ │
    │ ◄───────────────── [binary tts] ──── │   speaking (half-duplex)
    │ ◄───────────────────────── tts.end ─ │
    │ ◄────────────────────── listen.start │   follow-up turn (no wake)
    │ ── [binary mic audio] ─────────────► │
    │              ...                     │
```

Mic states on the satellite: **idle** (wake armed, nothing streamed) →
**streaming** (after a local wake or `listen.start`) → back to idle on
`utterance.end` (local VAD) or `listen.stop`. While TTS is playing the
satellite is deaf (half-duplex, ADR 0003); a `listen.start` received
during playback takes effect when playback ends.

Tool calls may arrive at any time while the session is open, including
during streaming or playback.

## Events: satellite → server

### `session.start`
First message on every connection.

```json
{
  "type": "session.start",
  "version": 1,
  "satellite": {"name": "quacksat", "version": "0.1.0"},
  "audio": {"rate": 16000, "channels": 1, "format": "s16le"},
  "tools": [
    {
      "name": "robot.move",
      "description": "Move the robot for a bounded time. Speeds are clamped.",
      "parameters": {
        "type": "object",
        "properties": {
          "vx": {"type": "number", "description": "m/s forward"},
          "vy": {"type": "number", "description": "m/s left"},
          "vyaw": {"type": "number", "description": "rad/s counterclockwise"},
          "duration_s": {"type": "number", "maximum": 3.0}
        },
        "required": ["duration_s"]
      }
    }
  ]
}
```

`tools` is the complete offered surface; it is empty when the robot is
unreachable. The schema shape is standard JSON Schema, directly usable
as OpenAI `tools[].function.parameters` or an MCP tool listing.

### `wake`
Local wake word fired. The satellite starts streaming mic audio
immediately after this event (pre-roll included).

```json
{"type": "wake", "model": "hey_daffy", "score": 0.93}
```

### binary frames
Mic audio in the `session.start` format, ~32 ms per frame. Sent only
in the streaming state.

### `utterance.end`
The local VAD closed the utterance; streaming stops.

```json
{"type": "utterance.end"}
```

### `tool.result`
Answer to exactly one `tool.call`, matched by `id`.

```json
{"type": "tool.result", "id": "t1", "ok": true, "data": {"fallen": false}}
{"type": "tool.result", "id": "t2", "ok": false, "error": "unknown tool"}
```

`ok: false` is a normal outcome (refused by allowlist, robot
unreachable, unsupported); `error` says why, in text meant for the
LLM to read.

### `pong`
Echo of a received `ping`, payload included.

## Events: server → satellite

### `session.ready`
Ack of `session.start`.

```json
{"type": "session.ready", "version": 1, "agent": {"name": "bridge"}}
```

### `listen.start` / `listen.stop`
Open/close the satellite mic without a wake word. `listen.start`
during TTS playback is honored after playback ends. `listen.stop`
while idle is a no-op.

```json
{"type": "listen.start"}
```

### `tts.start`, binary frames, `tts.end`
One spoken reply. Audio is raw PCM in the declared format, streamed;
the satellite plays it through its half-duplex player and drops mic
input meanwhile. `tts.end` closes the clip; the satellite finishes
playback before processing further audio-affecting events.

```json
{"type": "tts.start", "rate": 22050, "channels": 1, "format": "s16le"}
```

A new `tts.start` before the previous clip ended kills the previous
playback (single-child rule, ADR 0003).

### `tool.call`
```json
{"type": "tool.call", "id": "t1", "name": "robot.state", "args": {}}
```

`id` is an opaque string chosen by the server, unique per in-flight
call. The satellite executes sequentially in arrival order and always
answers with a matching `tool.result`. Servers should apply a timeout
(suggested 30 s) and treat a missing result as `ok: false`.

### `error`
Informational; the session continues.

```json
{"type": "error", "message": "stt failed: connection refused"}
```

### `ping`

```json
{"type": "ping", "t": 1725100000}
```

## Tool surface v1

Declared by the satellite; all arguments clamped satellite-side.

| Tool | Args | Effect |
|---|---|---|
| `robot.sound` | `{tag}` ∈ robotd's SoundTag set | expressive duck cue via `robot.sound` |
| `robot.look` | `{x, y?, z?}` meters, trunk frame, clamped | aim the gaze at a point via robotd's `robot.look` IK; result reports `clamped` when the point is out of reach |
| `robot.head` | `{pitch?, yaw?, roll?}` rad, clamped | expressive head pose (looking at things is `robot.look`'s job); omitted angles re-center |
| `robot.skill` | `{name}` ∈ {ground_pick, kick_left, kick_right, sit_toggle, roulade} | one-shot skill via `robot.do` |
| `robot.move` | `{vx?, vy?, vyaw?, duration_s}` (duration ≤ 3.0 s) | timed walk: intents pumped ≥20 Hz for the duration, then silence — the deadman remains the backstop |
| `robot.state` | `{}` | condensed `robot.state`/`robot.health`: pose, fallen, battery, mode |
| `robot.get_frame` | `{}` | **v1: always `ok: false, error: "unsupported"`** (waits for mediad camera access) |

Anything not listed does not exist on the wire; the satellite's
allowlist is exhaustive by construction.

## Compatibility and evolution

- `version` is informational (microduck's API-version doctrine: what
  actually breaks a peer is a shape that moved, and it refuses itself
  by name). Additive changes (new events, new fields, new tools) are
  minor and safe by the ignore rules.
- The event/binary split maps 1:1 onto a WebRTC datachannel/track for
  a future remote/full-duplex binding (ADR 0004 §1).
