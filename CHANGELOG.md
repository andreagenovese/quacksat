# Changelog

All notable changes to quacksat. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow
[Semantic Versioning](https://semver.org/) (pre-1.0: a minor version may
break things). Italian copy: [CHANGELOG.it.md](CHANGELOG.it.md).

quacksat has run on development Macs against real services, against a
real robotd (`--fake`, or the robot's socket forwarded) and with the
MuJoCo twin of the Microduck — never yet on a physical duck.

## [Unreleased]

## [0.1.0-rc1] - 2026-10-03

The first release candidate: the voice satellite as five weeks of history
left it, packaged to install on the duck. Release notes:
[docs/release-notes-v0.1.0-rc1.md](docs/release-notes-v0.1.0-rc1.md).

### Added

- **The voice pipeline** (`quacksat-core`). Continuous capture through an
  `arecord` child on the duck's codec (the microphone's right channel,
  48 kHz resampled to 16 kHz mono; ADR 0003), an energy VAD with an
  adaptive noise floor segmenting the turns, half-duplex playback through
  one `aplay` child at a time, and 320 ms of pre-roll flushed on wake so
  the transcriber hears the first phoneme. The `capture_command` and
  `playback_program` hooks (a sox shim for macOS) run the whole pipeline
  off the robot.
- **A local wake word**: openWakeWord-style ONNX models (the shared mel
  and embedding feature models plus a wake head) on tract, the pure-Rust
  ONNX runtime — no C dependency. "Hey Daffy", quacksat's own phrase,
  trained with openWakeWord's pipeline and kept in the repository, is the
  default; any openWakeWord model drops in (`docs/custom-wake-word.md`).
  The detector is reset when listening resumes, and the microphone is
  gated while the wake acknowledgement sounds.
- **The duck stops listening to itself**: one microphone, no echo
  cancellation, a speaker beside it. After its own voice the satellite
  throws away a tail (`listen::TAIL_FRAMES`, ~320 ms, a laptop's number
  until it is measured on the board) and keeps the microphone open for at
  least three seconds; speech is waited out to its end. `direct` and
  `agent` share it; the `wyoming` path applies the same tail.
- **Three interchangeable backends** (ADR 0002), picked by `backend` in
  `/etc/robot/quacksat.toml`, plus `none` for bring-up (wake and chirp):
  - **`wyoming`**: a Home Assistant Assist satellite over the Wyoming
    protocol — STT, intents and TTS stay in Home Assistant's pipeline.
  - **`agent`**: audio and events over a WebSocket to a bridge running
    STT → LLM (tool calling) → TTS, on a neutral protocol
    (`docs/agent-protocol.md`, ADR 0004).
  - **`direct`**: self-contained — the duck calls three OpenAI-dialect
    endpoints itself (chat completions, transcriptions, speech), a cloud
    key or local servers, no bridge. Reasoning models' think blocks are
    stripped from what is spoken.
- **The reference bridge** (`bridge/`, Python): the agent protocol's
  server side, LLM/STT/TTS as OpenAI-dialect URL + key endpoints, robot
  tools behind an exhaustive allowlist, `--fake` to exercise the whole
  protocol with no AI services, a Dockerfile and a compose file.
  **Several ducks on one bridge**: each announces its `[agent] name`,
  wakes within a 250 ms window compete and the highest score answers.
- **Agent protocol v1.1** (2026-10-03): `say`, a sentence of the duck's
  own spoken by the bridge as an ordinary TTS clip, sent only between
  turns and only to a bridge that lists `say` in `session.ready`'s
  `features`; an older bridge never receives one.
- **MCP**: the bridge serves the satellite's tool catalog as an MCP
  server (Streamable HTTP, port 8766), its tools addressed per duck when
  several are connected; the `direct` backend serves its own
  (`[direct.mcp]`, port 8767, a bearer token mandatory), so an
  MCP-capable agent drives the body with no bridge.
- **Robot tools through robotd**, on the padd model: an unprivileged
  client of robotd's JSON-RPC socket that sends intents and requests and
  never touches the bus (`robot.move`, `robot.head`, `robot.look`,
  `robot.sound`, `robot.skill`, `robot.state`, `robot.get_frame`, …);
  robotd's deadman keeps the robot safe if the satellite stalls. Pinned
  to microduck `daemon-v0.14.4`: `robot.skill` enforces the skills the
  robot itself lists. One request lane (`robotd::Lane`) behind every
  backend dials again when robotd restarts, instead of leaving the tools
  "robot unreachable" (or, on the Home Assistant path, the duck mute)
  until the satellite restarts. `[gait]` trims for a walk that veers.
- **The thinking cue**: after `[thinking] delay_s` of waiting for an
  answer the head sways slowly, and resumes after tools that leave the
  body alone; at `timeout_s` a low tock says the duck gave up.
- **Navigation through quack-navd** (`[nav] socket`,
  `/run/quack-nav/nav.sock`). The map, the places, the planner, the
  explorer and the homecoming moved to a daemon of their own in the
  quacknav repository on 2026-09-22 (ADR 0006); the satellite probes its
  socket at startup and announces its tools (`robot.where_am_i`,
  `robot.remember_place`, `robot.go_to`, `robot.map_explore`, …) beside
  its own. Nothing listening is not an error: the duck answers but cannot
  be sent anywhere, and says so. The two repositories share no code.
- **What the duck says on its own** (`[announce]`, 2026-10-03): the
  satellite follows quack-navd's `robot.map_status` (every 2 s while
  something moves, every 10 s otherwise) and says, in a short fixed
  phrase in Italian or English, how a journey or an exploration it
  started ended ("Sono arrivata in cucina", "Mi sono fermata", "Ho
  esplorato per 10 minuti, la casa è mappata al 72 per cento") and why
  the duck walks when nobody asked (the homecoming's search, the
  relocalization before a journey). Between turns, each once: `direct`
  in its own voice, `agent` through the bridge's `say`.
- **The systemd unit**: an unprivileged `quacksat` account
  (`SupplementaryGroups=robot audio`), `After`/`Wants` robotd and
  quack-navd rather than requiring them, a resource fence (Nice 10,
  CPUWeight 50, MemoryHigh 192M, MemoryMax 256M) so the satellite never
  costs robotd its 50 Hz loop, and padd's hardening block. The logs
  carry no colour escapes when stderr is not a terminal (the journal).
- **The release**: `scripts/cross-build.sh` builds the board's binary
  with cargo-zigbuild (glibc floor 2.31, no Docker); `scripts/package.sh`
  packs `quacksat-<version>-aarch64-linux.tar.gz` with the unit, the
  account, the example config, the installer and a step-by-step
  `README-install.md` (and `.it.md`); `scripts/install-on-duck.sh`
  installs or upgrades over ssh, from the package or from a checkout,
  with `--dry-run`; it keeps an existing config and installs a new one
  0640 root:quacksat. The wake models are not packaged (openWakeWord
  publishes its pre-trained models under CC BY-NC-SA 4.0): the installer
  downloads them on the duck, each checked against its sha256. CI runs
  the tests and the cross-build on every push and attaches the package,
  the bare binary and their checksums to the release of a `v*` tag.

[Unreleased]: https://github.com/andreagenovese/quacksat/compare/v0.1.0-rc1...HEAD
[0.1.0-rc1]: https://github.com/andreagenovese/quacksat/releases/tag/v0.1.0-rc1
