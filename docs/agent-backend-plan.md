# Path B plan: agent backend, bridge, and protocol (task 6)

Status: **implemented** (2026-09-01) — kept as the design record for
path B; the wire contract lives in ADR 0004 + `docs/agent-protocol.md`.
Supersedes the sketch in `quacksat-ha-vs-agent.md` for path B
details. Italian copy: `agent-backend-plan.it.md`.

## Shape

```
┌───────────── duck ─────────────┐      ┌───────────────── server ─────────────────┐
│ backends/agent (Rust)          │      │ bridge/ (Python, reference)              │
│ mic → wake → VAD → audio ────── WS ───► orchestrator: VAD-segmented turns        │
│ speaker ◄── tts stream ─────────────── │  ├─ STT  → {stt_base_url}/audio/transcriptions
│ tool exec → allowlist → robotd │      │  ├─ LLM  → {llm_base_url}/chat/completions
└────────────────────────────────┘      │  ├─ TTS  → {tts_base_url}/audio/speech   │
                                        │  └─ MCP server "robot" (the tool core)  │
                                        └──────────────────────────────────────────┘
```

## Decisions

1. **Transport: WebSocket, not WebRTC** (for now). Binary frames = raw
   16 kHz mono S16LE audio; text frames = JSON events. Rationale: the
   conversation is turn-based and half-duplex by design (no AEC, ADR
   0003); home LAN; a neutral "bring your own agent" protocol must be
   implementable with any language's stdlib. The protocol is
   transport-agnostic: events ↔ WebRTC datachannel and audio ↔ track map
   1:1 if remote/full-duplex/video ever demand it (documented evolution,
   not v0).

2. **Protocol (ADR 0004, to be specced first).**
   - satellite → bridge: `session.start` (name, audio format, offered
     robot tools), `wake` (model, score), binary audio while streaming,
     `utterance.end` (local VAD), `tool.result {id, ok, data}`.
   - bridge → satellite: `listen.start` / `listen.stop` (mic control —
     enables multi-turn without re-waking), `tts.start {rate}` + binary
     audio + `tts.end` (streamed into the half-duplex Player),
     `tool.call {id, name, args}`, `error`, ping/pong.

3. **Tools execute on the satellite.** The satellite declares its tools
   in `session.start`; quacksat enforces an explicit allowlist (btd's
   exhaustive-match pattern) and translates to robotd RPCs. v0 surface:
   `robot.sound`, `robot.head`, `robot.skill`, `robot.state`, and a
   *timed* `robot.move {vx, vy, vyaw, duration}` (quacksat pumps intents
   for the duration, then stops — an LLM never holds an open throttle;
   the deadman stays the last line). `robot.get_frame` is declared but
   `unsupported` until mediad exposes the camera (mapping roadmap).

4. **The MCP server is the bridge's tool core, not an add-on.** One MCP
   server (HTTP/SSE) exposes the satellite's declared tools; every
   consumer goes through it:
   - **Profile 1 — MCP-native agent** ([Arkimede](https://arkimede.ai/) today): the agent
     registers the bridge's MCP server and calls robot tools inside its
     own loop. No Arkimede changes needed — its OpenAI shim never sees
     the tool calls (they take the MCP side door).
   - **Profile 2 — chat-completions provider with tool calling**
     (OpenAI, Groq, llama.cpp…): the bridge runs the loop, declares the
     tools in the request, and executes returned `tool_calls` through
     the same internal MCP layer.
   - **Profile 3 — bare LLM** (no tools, no MCP): voice-only chat.
   Same executor, same allowlist, same audit in all profiles.

5. **STT/TTS/LLM are all url+key config in the OpenAI dialect.** The
   bridge contains no ML: `{llm,stt,tts}_base_url` + keys, speaking
   `/chat/completions`, `/audio/transcriptions`, `/audio/speech`. Local
   deployments use existing OpenAI-dialect servers (speaches /
   faster-whisper-server, openedai-speech for Piper, LocalAI); the repo
   ships a docker-compose example. The **Arkimede preset is pure
   configuration**: LLM on `/api/openai/v1` with an `ak_` key (works
   today), STT on its audio route and TTS once Arkimede exposes them —
   see `docs/VOICE_AUDIO_SERVICES.md` in the Arkimede repo for that
   work. Zero Arkimede-specific code in the bridge.

## Order of work (all delivered)

1. ADR 0004 + protocol spec doc (bilingual).
2. `backends/agent` (Rust): tungstenite + rustls client (sync, std
   threads like the rest), same `Deps` shape as the wyoming backend,
   reconnect with backoff; scripted-server tests mirroring the wyoming
   suite (full conversation incl. a tool call).
3. `bridge/` (Python): WS server, VAD segmentation, the three OpenAI
   clients, MCP server, profiles; `--fake` mode (canned replies, no
   models) for protocol tests; docker-compose example for local STT/TTS.
4. Live test on the Mac: voice → bridge (fake) → tool.call → chirp on
   `robotd --fake`; then bridge → Arkimede preset → real conversation
   with the home agent (home automation via Arkimede's internal MCP
   tools).
5. Phase 2 (Arkimede repo, optional): audio routes + piper-service (doc
   above), robot MCP registration, eventually a native `/voice` gateway
   replacing the bridge.

## Future: a `direct` backend (self-contained duck)

A third backend, `backend = "direct"`, where the satellite itself speaks
the OpenAI dialect — STT → LLM (tool calling) → TTS over plain HTTP,
tools executed in-process behind the same allowlist, no bridge and no
WebSocket hop. In Rust the resource cost on the board is negligible
(the orchestration computes nothing; the heavy models stay behind the
URLs).

Why it matters: **a fully self-contained duck is what most people
want** — plug in an API key (or any OpenAI-dialect endpoint) and talk,
with no home server, no containers, no bridge to operate. The current
architecture already accommodates it by construction: ADR 0002's
runtime backend selection plus ADR 0004's url+key services mean
`backends/direct` slots in beside `wyoming` and `agent` without
touching anything else.

Known trade-offs versus the bridge: conversation memory dies with the
battery, and nothing is shared between multiple ducks. The bridge
remains the right shape for a home with an always-on server; `direct`
is the right shape for everyone else.

**Implemented: an MCP server on the duck itself.** The direct
backend can expose the robot tool catalog as its own MCP server
(stateless Streamable HTTP, `[direct.mcp]` config, off by default) —
the same allowlist and clamps, served from the robot. This completes
the matrix (direct + Arkimede = voice + home automation + body,
no bridge) and makes the duck registrable by *any* MCP-capable client
(Claude Desktop, Claude Code, other agents) independently of voice.
Security posture: the bearer token is **mandatory** when enabled (an
HTTP server accepting motion commands on the robot is more sensitive
than the same server on the bridge); allowlist, clamps, and robotd's
deadman remain underneath. Known limits stay: battery and DHCP make
the duck a flapping MCP host — fine for tinkering, while the bridge
stays the solid registration target for daily Arkimede use.

## Future: multi-satellite (one bridge, many ducks)

The protocol already allows it: every duck opens its own WebSocket,
announces itself in `session.start` (name included), and gets an
independent session — its own audio buffer, turns, and history. What
the reference bridge still lacks is the plumbing where one duck must be
*chosen*; the design for it, when a second duck exists:

1. **Identity**: the session registry becomes a `name → session` map
   (the name already travels in `session.start`; it just needs to be
   configurable per duck).
2. **Addressed tools**: robot tools gain a `duck` argument, or the
   bridge exposes a per-duck catalog and the agent picks;
   `robot_state` with no argument answers for all of them.
3. **Wake arbitration** (the interesting one): two ducks in adjacent
   rooms both hear the wake word. As Home Assistant does with its
   satellites, the bridge collects `wake` events in a short window
   (~200 ms), the **highest score wins** (the duck closest to the
   speaker), and the others get `listen.stop`. The `wake` event
   carries its score precisely for this.
4. **Whole-home memory**: the conversation belongs to the house, not
   to a duck — start talking in the kitchen, continue in the living
   room. With an agent platform (Arkimede) this is nearly free: memory
   already lives in the agent; the bridge only routes the reply to the
   duck that captured the last utterance.

This is also *why* multi-duck demands the server-side bridge: wake
arbitration and shared memory need one point that sees every duck at
once — impossible with an on-board bridge or the `direct` backend.
Upstream's "chorale" (ducks singing in harmony) is the same instinct;
the quacksat version is distributed voice presence. Registry map and
wake arbitration are an afternoon of bridge work, zero satellite or
protocol changes.

## Out of scope (deliberate)

- Barge-in (needs AEC — ADR 0003 defers it).
- Speech-to-speech realtime (path C): a different bridge, same
  satellite protocol.
- mDNS/discovery, multi-satellite sessions.
