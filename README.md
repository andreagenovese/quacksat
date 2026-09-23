# quacksat

Mobile voice satellite for Home Assistant and AI agents, running on the
Pollen Robotics Microduck.

> **Disclaimer**: quacksat is an independent project. It is not affiliated
> with, endorsed by, or supported by Pollen Robotics or Hugging Face.
> "Microduck" is used only to identify the target hardware.

## What it does

quacksat turns the Microduck into a roaming voice assistant. It captures
audio on the duck, detects a wake word, and hands the conversation to one of
three interchangeable backends selected in `/etc/robot/quacksat.toml`:

- **`wyoming`** — the duck becomes a [Home Assistant Assist](https://www.home-assistant.io/voice_control/)
  satellite over the Wyoming protocol: STT, intent handling, and TTS run in
  your existing HA pipeline.
- **`agent`** — the duck streams audio and events over WebSocket to a bridge
  that runs STT → LLM (with tool calling) → TTS. The protocol is
  agent-neutral; a minimal reference bridge lives in [`bridge/`](bridge/),
  so you can bring your own agent.
- **`direct`** — self-contained: the duck itself calls three
  OpenAI-dialect endpoints (chat completions, transcriptions, speech) —
  any cloud key or local server, no bridge, no home server. It can also
  serve its own MCP endpoint so MCP-capable agents drive the robot
  directly.

In both modes quacksat is an unprivileged client of `robotd`, the Microduck
system daemon: it sends intents and RPCs (move, head, skills) over the
JSON-RPC socket and never touches the hardware bus directly. If quacksat
crashes or hangs, robotd's deadman keeps the robot safe.

## Repository layout

```
quacksat/           the binary: config loading, capture, backend dispatch
quacksat-core/      mic capture, wake word, VAD, speaker, robot tools → robotd
backends/wyoming/   Home Assistant Assist satellite backend
backends/agent/     AI agent backend (WebSocket to a bridge)
backends/direct/    self-contained backend (OpenAI-dialect STT/LLM/TTS, no bridge)
bridge/             minimal reference bridge for the agent backend
systemd/            quacksat.service unit
scripts/            deploy scripts for the Radxa Zero 3 / the duck
docs/study/         study notes on the Microduck software stack
docs/adr/           architecture decision records
```

## Status

Working, validated on a development Mac against real services (a Home
Assistant install, an agent platform, local LLMs). On-robot validation
is pending — December 2026.

- Local wake word (openWakeWord models on the pure-Rust tract runtime;
  custom phrases supported — see `docs/custom-wake-word.md`), VAD turn
  segmentation, half-duplex playback, robotd client on the padd model,
  and a thinking cue: a slow head sway while the answer is computed,
  a low give-up tock on timeout or error (`[thinking]` config).
- `wyoming`: registers in Home Assistant and runs the full Assist
  round-trip (wake → STT → intent → TTS).
- `agent`: the neutral WebSocket protocol (`docs/agent-protocol.md`)
  plus the reference bridge in `bridge/` — STT/LLM/TTS as OpenAI-dialect
  url+key endpoints, robot tools behind an exhaustive allowlist, and an
  MCP server exposing them to MCP-native agents. Multi-duck: several
  satellites on one bridge, wake arbitration by score (the duck that
  heard you best answers), per-duck MCP tool addressing.
- `direct`: the self-contained satellite — it calls the three
  OpenAI-dialect endpoints itself, no bridge, and can serve its own MCP
  endpoint so agents drive the robot directly.
- Navigation, since 2026-09-22, is a daemon of its own (ADR 0006):
  `quack-navd`, in the [quacknav](https://github.com/andreagenovese/quacknav)
  repo, owns the map client, the cliff guard, the planner, the places
  registry, the explorer and the homecoming, and answers for them on
  `/run/quack-nav/nav.sock` (NDJSON JSON-RPC, robotd's own wire). quacksat
  probes that socket at startup: if a daemon answers, its tools are
  announced beside the satellite's own; if none does, the duck listens
  and answers but cannot be sent anywhere, and says so. The two repos
  share no code — only robotd's protocol — so neither has to be cloned
  to install the other. What the navigation does, and three weeks of
  measurements on the MuJoCo twin, are documented in that repo.
- Places, mapping and journeys: the duck learns the name of where it
  stands ("this is the kitchen"), answers "where are you", maps a house
  on its own and walks to a place it knows — all of it through the
  navigation daemon's tools (`robot.where_am_i`, `robot.remember_place`,
  `robot.map_status`, `robot.map_step`, `robot.map_explore`,
  `robot.go_to`, the map library), spliced into the catalog the agent
  sees. Names are attached to map coordinates, never recognized by
  sight.
- Pinned to microduck `daemon-v0.14.4` (2026-09-23): builds and tested
  against the current release. Since 0.14 a robot's skills are its own
  config, so `robot.skill` announces and enforces the list the robot
  reports (`robot.skills`) rather than five names compiled in — re-read
  whenever the request lane is dialled again, which it now is: one
  `robotd::Lane` behind every backend, so robotd restarting under the
  satellite no longer leaves the robot tools answering "robot
  unreachable" — or, on the Home Assistant path, the duck mute and
  still while the pipeline carries on. What that jump cost is written
  up in
  `docs/study/microduck-ipc-and-packaging.md`.

## Getting started

### 1. Build and install on the duck

The duck's board is an aarch64 Rockchip RK3566 running Armbian; macOS
has no aarch64-linux sysroot, so the build runs in a Linux container
(Docker required):

```sh
scripts/build-aarch64.sh          # cross-build the release binary
scripts/deploy.sh <duck-host>     # install everything over ssh
```

The deploy installs the binary (`/usr/local/bin/quacksat`), the systemd
unit and its unprivileged service account, a default config at
`/etc/robot/quacksat.toml` (kept on redeploys — edit it there), and the
wake-word models in `/var/lib/quacksat/models` — including
**"hey Daffy"**, quacksat's own wake word, which ships in this repo
(`models/hey_daffy.onnx`); any extra model in your local `models/`
(e.g. one trained per `docs/custom-wake-word.md`) rides along. Then:

```sh
ssh <duck-host> journalctl -u quacksat -f
```

Pick the backend in the config: `wyoming` needs nothing else on this
list; `agent` needs a running bridge (below); `direct` needs three
OpenAI-dialect endpoint URLs.

### 2. Run the bridge (agent backend)

On any machine with Python 3.11+ (typically your always-on server):

```sh
cd bridge
cp config.example.toml config.toml   # then edit: LLM/STT/TTS urls + keys
python3 -m venv .venv && .venv/bin/pip install websockets "mcp>=2" uvicorn
.venv/bin/python bridge.py --config config.toml
```

Point the satellite at it (`[agent] url = "ws://<bridge-host>:8765"`).
`--fake` instead of `--config` exercises the whole protocol with no AI
services. Details and provider profiles: `bridge/README.md`.

### 3. Run the bridge in Docker

```sh
cd bridge
cp config.example.toml config.toml   # then edit it
docker compose up -d --build
docker compose logs -f bridge
```

Ports: 8765 (satellite WebSocket), 8766 (MCP server when `[mcp]` is
enabled). Protocol smoke test without AI services:
`docker compose run --rm --service-ports bridge python bridge.py --fake`.

### Developing without the robot

`robotd --fake` (from a `pollen-robotics/microduck` checkout) stands in
for the real robot, or forward the real socket:

```sh
ssh -L /tmp/robotd.sock:/run/robotd.sock <duck>
```

On macOS the mic and speakers work through sox — see the
`capture_command` / `playback_program` hooks in `quacksat.example.toml`.

## License

Apache-2.0 — see [LICENSE](LICENSE) and [NOTICE](NOTICE).
