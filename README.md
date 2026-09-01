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
  segmentation, half-duplex playback, robotd client on the padd model.
- `wyoming`: registers in Home Assistant and runs the full Assist
  round-trip (wake → STT → intent → TTS).
- `agent`: the neutral WebSocket protocol (`docs/agent-protocol.md`)
  plus the reference bridge in `bridge/` — STT/LLM/TTS as OpenAI-dialect
  url+key endpoints, robot tools behind an exhaustive allowlist, and an
  MCP server exposing them to MCP-native agents.
- `direct`: the self-contained satellite — it calls the three
  OpenAI-dialect endpoints itself, no bridge, and can serve its own MCP
  endpoint so agents drive the robot directly.

## Building

The duck's board is an aarch64 Rockchip RK3566 running Armbian. macOS has no
aarch64-linux sysroot, so cross-build in Docker/Linux (see `scripts/`).
For development without the robot, `robotd --fake` runs the daemon without
hardware, or forward the real socket:

```sh
ssh -L /tmp/robotd.sock:/run/robotd.sock <duck>
```

## License

Apache-2.0 — see [LICENSE](LICENSE) and [NOTICE](NOTICE).
