# quacksat — handoff for Claude Code

Brief to paste as the first message (or to save as `CLAUDE.md` in the repo).

## Language policy

English is the primary language for everything (code, comments, docs, commits,
conversation). Every document in docs/ (and CLAUDE.md/README.md) is kept as a
pair: the English file is canonical; an Italian copy with the same basename
plus a `.it` suffix before the extension exists for the user. When creating or
editing a document, update both copies, English first.

## What it is

**quacksat**: mobile voice satellite for Home Assistant and for AI agents,
running on board the Microduck (Pollen Robotics / Hugging Face).
Independent project, not affiliated with Pollen Robotics. Apache-2.0 license.
Repo description: "Mobile voice satellite for Home Assistant and AI agents,
running on the Pollen Robotics Microduck".

## Decisions made

1. **Separate repo**, not a fork of `pollen-robotics/microduck`. We depend on
   their crates (`duck-ipc-proto`) and patch upstream via PR only when needed.
2. **Two interchangeable backends**, selected by `/etc/robot/quacksat.toml`:
   - `wyoming` → path A: satellite for HA Assist (Wyoming/ESPHome).
   - `agent` → path B: WebSocket towards a bridge STT → LLM (tool calling) → TTS.
   Order: first core + wyoming (validates the hardware against an already
   proven HA chain), then agent + a "bring your own agent" reference bridge.
3. **padd pattern**: quacksat is a NON-privileged client of robotd (minimal
   groups), it only sends intents/RPC, it never touches the bus. If it goes
   silent, the deadman protects the robot.
4. Neutral `agent` protocol (audio streaming + events + tool call/result); the
   bridge in the repo is a minimal reference; the [Arkimede](https://github.com/andreagenovese/yesSir) integration lives
   in Arkimede.

## Structure

```
quacksat/
├── CLAUDE.md
├── LICENSE (Apache-2.0) · NOTICE · README.md (con disclaimer non-affiliazione)
├── docs/
│   ├── study/      ← i documenti di studio già prodotti (vedi sotto)
│   └── adr/        ← 0001-repo-separato, 0002-backend-intercambiabili, ...
├── quacksat/       ← the binary: config, capture, backend dispatch
├── quacksat-core/  ← mic, wake word, VAD, speaker, tool robot → robotd
├── backends/wyoming/ · backends/agent/ · backends/direct/
├── bridge/         ← riferimento lato server per la strada B
├── systemd/        ← quacksat.service
└── scripts/        ← deploy su Radxa Zero 3 e sull'anatra
```

Study documents to put in `docs/study/`:
`microduck-architecture.md`, `microduck-flowchart.mermaid`,
`robotd-analysis.md`, `robotd-dataflow.mermaid`,
`quacksat-ha-vs-agent.md`, `quacksat-flows-comparison.mermaid`.

## Known technical constraints (from robotd-design.md and architecture.md)

- Board: Rockchip RK3566, 1 GB RAM, Armbian, systemd. Rust only in the Pollen
  stack; prefer pure-Rust crates (a C dep costs cross-build and board work).
- Cross-building from macOS lacks the aarch64 sysroot: use Docker/Linux to
  build for the board.
- IPC: JSON-RPC 2.0 / NDJSON over unix socket, `/run/robotd.sock`; intents as
  notifications (robot.move, robot.head), requests with a response (robot.stop,
  enable, skill). Maintenance namespace excluded from remote clients.
- **Speaker: exclusive PCM** — `sound.rs` keeps a single `aplay` child, a new
  sound kills the old one. The TTS must coordinate with sound.rs, not open
  ALSA in parallel. To verify in the code how to expose it.
- **Mic: already in continuous capture by `pet-detect/`** (40-band log-mel CNN).
  To verify: shareable ALSA dsnoop or exclusive access?
- Audio codec on the I²C bus shared with the ToF. Camera/mic pipeline slated
  for migration to `mediad` (roadmap M5): follow the commits before pinning
  down the audio design.
- Single mic, no hardware echo cancellation (unlike Voice PE).
- NP-F550 battery, ~1 h. Signed updates via updaterd: quacksat must be
  packaged as a separate, reinstallable unit, outside `releases/`.
- Remote dev: `ssh -L /tmp/robotd.sock:/run/robotd.sock` → quacksat runs on
  the Mac against the real robot. `robotd --fake` to work without hardware.

## Status (2026-09-01) and next steps

The original build plan (scaffold → microduck study → ADR 0003 → core →
wyoming → agent/bridge) is complete, plus a third `direct` backend and
the duck-side MCP server. Everything is validated live on a dev Mac
against real services; see README → Status.

Next:

1. **December 2026**: the physical duck arrives — on-hardware
   validation (aic3104 capture/playback per ADR 0003, real chirps,
   cross-build + deploy via `scripts/`, wake-word CPU budget on the
   RK3566).
2. Optional, in the Arkimede repo: phase 2 (native `/voice` gateway,
   tool passthrough on the OpenAI shim) — briefs live there.
3. Later, per `docs/todo-map.md`: the mapping/localization track
   (get_frame via a mediad upstream PR, then `where_am_i`/`go_to`).

## Private context

Details of the author's existing home setup (relevant to path B) live in
`CLAUDE.local.md`, which is untracked. Other private material lives in the
untracked `private/` directory. Neither is ever committed.
