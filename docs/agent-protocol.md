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

`satellite.name` comes from `[agent] name` in the satellite config and
identifies the duck to the server. With several ducks on one bridge,
give each a unique name: it keys the bridge's session registry and
becomes the `duck` argument of the bridge's MCP tools.

### `wake`
Local wake word fired. The satellite starts streaming mic audio
immediately after this event (pre-roll included).

```json
{"type": "wake", "model": "hey_daffy", "score": 0.93}
```

`score` is the detector's confidence (null for detectors that have
none). Servers with several connected ducks use it for wake
arbitration: wakes landing within a short window compete, the highest
score wins, the losers get `listen.stop`.

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

Servers should send one whenever a turn produces no reply (empty
transcript, failed STT/LLM): the satellite treats it as the end of the
wait — it drops its thinking pose and plays the give-up tock instead
of sitting silent until its reply timeout.

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
| `robot.skill` | `{name}` ∈ whatever `robot.skills` reported (stock: ground_pick, kick_left, kick_right, sit_toggle, roulade) | one-shot skill via `robot.do`; since daemon 0.14 the skill table is config, so the enum announced is the robot's own list and a name outside it is refused before the wire |
| `robot.move` | `{vx?, vy?, vyaw?, duration_s}` (duration ≤ 3.0 s) | timed walk: intents pumped ≥20 Hz for the duration, then silence — the deadman remains the backstop |
| `robot.state` | `{}` | condensed `robot.state`/`robot.health`: pose, fallen, battery, mode |
| `robot.get_frame` | `{}` | **v1: always `ok: false, error: "unsupported"`** (waits for mediad camera access) |

> The tools from `robot.where_am_i` down to `robot.go_to` are the
> navigation daemon's (`quack-navd`, ADR 0006). They appear in
> `session.start`'s catalog only when a daemon answers on
> `[nav] socket`; without one the satellite announces its own seven and
> the duck cannot be sent anywhere.

| `robot.where_am_i` | `{}` | the nearest remembered place and the distance to it (`at_place` inside its radius), from robotd's live map (`robot.map`, ADR 0005); `known: false` with a reason while the pose is untrusted (seated, searching, no map); an error when the robot does not map at all |
| `robot.remember_place` | `{name, radius_m?}` (0.3–6 m, default 1.5) | teach the current pose as `name`; the same name from another spot adds an anchor; refused while the pose is untrusted |
| `robot.forget_place` | `{name}` | drop a place |
| `robot.list_places` | `{}` | every place with anchors, radius, `stale` (taught before a map reset) and its distance when the pose is known |
| `robot.map_status` | `{}` | the map in numbers (mapping on/off and mode, tracking, seated, windows, submaps, loops, cell counts, pose), the free distance ahead/left/right/behind before a known wall (`clearance`, with what ends each ray: wall, unknown, edge, open), the cliff guard's view (`cliff`: whether it can see, and the nearest drop — stairs or a hole, invisible to the map — with its distance and bearing), plus a one-line hint on what to do next |
| `robot.map_step` | `{vx?, vy?, vyaw?, walk_s?, stop_s?, centre?, gap?}` (walk ≤ 3 s, stop ≤ 10 s, default 6) | one leg of a stop-and-scan mapping tour: a timed walk, then a stand so the stop reaches the map; reports `new_windows`, the clearance after the step and a hint. Refused while the duck is seated or mapping is off; a walk that would end within 25 cm of a mapped wall or of something the depth sensor sees is shortened to what fits (`shortened` names what limited it) and refused only when less than a second of walking fits; refused into unmapped space right at the beak (the sensor sees nothing under 10 cm), or toward a drop the cliff guard has seen (35 cm margin) — the error names the free sides. With `centre: true` the step keeps to the middle between mapped walls (the explorer's own legs ask for it); a wall closer than 20 cm on one side steers any step away (`steered`) |
| `robot.map_explore` | `{stop?, max_s?}` | map everything: a background job walks the duck to the most promising reachable frontier (free floor touching unknown, weighed by path cost per frontier cell so a wide opening a room away beats a sliver nearby; paths kept 15 cm off walls), stands to map it, repeats until nothing reachable is left or the budget runs out; every step is a `robot.map_step` with all its guards; when the way on is blocked it turns to the same hand every time (quack-nav's `[map] explore_turn`, right by default — the right-hand rule). Returns at once; `robot.map_status` carries `explore` (state, legs, frontiers_left, target). While it runs, `robot.move` and `robot.map_step` are refused. On reaching a nameless area the satellite asks "where are we?" out loud (the `direct` backend; quack-nav's `[map] ask_phrase`) and the answer is expected to become a `robot.remember_place` |
| `robot.map_save` | `{name}` | keep the map the duck is holding, under a name, in the robot's library (1–64 characters of letters, digits, `-` and `_`; saving again replaces it). Needs a robotd with the map library — an older one answers "this robot's software has no map library yet" |
| `robot.map_list` | `{}` | the saved maps: name, size and when each was written |
| `robot.map_match` | `{name?}` | is this one of the houses the duck has mapped before? Compares the map it is holding against every saved map (or one named) and answers candidates, best first: the name, where the live map sits inside the saved one (`x`, `y`, `yaw`), the wall residual, the share of live floor laid on saved walls, and a score (lower better), plus `live_cells` — how much map there was to ask with. Not a verdict: on an 8×8 depth sensor a flat and its own mirror image score alike, so a name is worth believing only when the same one comes back minutes later with a bigger map (which is what `[homecoming]` does) |
| `robot.map_load` | `{name}` | give the duck back a saved map. The map comes back, the position does not: the mapper starts lost inside it and searches, so `robot.map_status` shows `tracking: false` until a still window confirms a place |
| `robot.go_to` | `{place?, x?, y?, stop?, max_s?}` | walk to a place the duck knows (by name, from `robot.list_places`) or to a point in map metres: the cheapest route on the map it already has — no exploring — walked with the mapping job's own legs and guards, so stairs and unmapped obstacles stop it as they would a `robot.map_step`. Refuses at once, before walking, when the map shows no way there. Returns at once; follow it with `robot.map_status` (`explore.state`, `explore.target_distance_m`); `stop: true` stops it. While it runs, `robot.move` and `robot.map_step` are refused |

Anything not listed does not exist on the wire; the satellite's
allowlist is exhaustive by construction.

## Compatibility and evolution

- `version` is informational (microduck's API-version doctrine: what
  actually breaks a peer is a shape that moved, and it refuses itself
  by name). Additive changes (new events, new fields, new tools) are
  minor and safe by the ignore rules.
- The event/binary split maps 1:1 onto a WebRTC datachannel/track for
  a future remote/full-duplex binding (ADR 0004 §1).
