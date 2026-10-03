# quacksat v0.1.0-rc1 — release notes

2026-10-03. Italian copy: [release-notes-v0.1.0-rc1.it.md](release-notes-v0.1.0-rc1.it.md).
The full list of what is in it is in [CHANGELOG.md](../CHANGELOG.md).

## What this is

quacksat is the Microduck's voice satellite: it runs on the duck, listens
for its wake word ("hey Daffy"), and hands the conversation to Home
Assistant, to an AI agent behind a bridge, or straight to OpenAI-dialect
speech and language endpoints — and gives that conversation the duck's
body: its moves, its head, its sounds and skills through robotd, and,
through the navigation daemon quack-navd, the places of the house.

This is the **first release candidate**: what five weeks of work left,
packaged so it installs on the duck from a download.

## Status

- **Not yet on a physical duck.** The duck arrives in December 2026;
  until a first session on it, nothing here has run on the RK3566.
- **On development Macs, against real services**: a Home Assistant
  install (the `wyoming` path, the full Assist round-trip), a self-hosted
  agent platform and local LLMs (the `agent` and `direct` paths), a real
  robotd — `--fake`, or a robot's socket forwarded over ssh — answering
  the robot tools.
- **On the MuJoCo twin** of the Microduck, with quack-navd: "vai in
  cucina" walked to the kitchen and the duck said so when it arrived; a
  STOP mid-way was said; the homecoming's search at boot and the
  relocalization before a journey were announced, with `direct` (its own
  voice) and with `agent` (through the bridge's `say`).
- **The install** was tried in a Debian 13 arm64 container booted with
  systemd: from the unpacked package and again from a checkout, the
  service starting under its unit's sandbox and logging what it lacks
  (no sound card, no robotd, no quack-navd). Not yet on a board.

## What works

- **Wake word, locally**: openWakeWord-style ONNX models on tract (pure
  Rust); "hey Daffy" by default, any openWakeWord model by name
  ([custom-wake-word.md](custom-wake-word.md)).
- **Turn-taking without echo cancellation**: half-duplex playback, a
  tail of the duck's own voice thrown away, a minimum listening window,
  speech waited out to its end.
- **Four backends**: `none` (bring-up: wake and chirp), `wyoming` (a
  Home Assistant Assist satellite), `agent` (a WebSocket to a bridge,
  [agent-protocol.md](agent-protocol.md) v1.1), `direct` (the duck calls
  the endpoints itself, no server).
- **The reference bridge** in `bridge/`, several ducks on one bridge
  (the one that heard you best answers), and **MCP**: the bridge's server
  for MCP-native agents, and the duck's own in `direct`.
- **The body**: robot tools through robotd's socket as an unprivileged
  client (move, head, look, sound, skills, state, a camera frame),
  surviving a robotd restart; a thinking pose while the answer is
  computed.
- **The house**: quack-navd's tools spliced in when its socket answers —
  where am I, remember this place, go to the kitchen, map the house — and
  the duck says on its own how a journey ended and why it walks when
  nobody asked (`[announce]`, Italian and English).

## Install

The release carries `quacksat-0.1.0-rc1-aarch64-linux.tar.gz` with its
`.sha256`, and the bare binary `quacksat-aarch64-linux` with its own.
Download the package, verify it, unpack it and run
`./install-on-duck.sh microduck@<duck>` from your computer: see
[Installing from a release](../README.md#installing-from-a-release) and
the package's step-by-step
[README-install.md](../scripts/package/README-install.md). The duck needs
robotd, the `robot` and `audio` groups, alsa-utils, and the internet once
(the wake models are downloaded there: openWakeWord's pre-trained models
are CC BY-NC-SA 4.0, so none is packaged). The bridge is not in the
package: it runs on a server, from a checkout of the tag.

## Configuration highlights

`/etc/robot/quacksat.toml`, installed from `quacksat.example.toml` only
when there is none, mode 0640 root:quacksat — it will hold API keys.

- `backend`: `none` in the example; `direct` needs `[direct.llm]`,
  `[direct.stt]`, `[direct.tts]` (`base_url`, `api_key`, model or voice);
  `agent` needs `[agent] url` (and a unique `name` per duck on a shared
  bridge); `wyoming` listens on `[wyoming] bind` (`0.0.0.0:10700`).
- `[wake]`: `models_dir = "/var/lib/quacksat/models"`,
  `model = "hey_daffy.onnx"`, `threshold = 0.5`.
- `[audio]`: the duck's codec, `plughw:aic3104`; robotd's
  `audio.pet_detect` must stay `false` (the factory default).
- `[nav] socket = "/run/quack-nav/nav.sock"`: quack-navd's; nothing
  listening is not an error.
- `[announce]`: on, `language` `"it"` or `"en"` (empty: the STT's, then
  the bridge's, then English), `journeys` and `own_motion`.
- `[direct.mcp]`: off; when on, a `token` is mandatory.
- Every key left out takes its default; an unknown key stops the
  satellite with a message naming it.

## Known limitations

- **No control socket yet**: quack-control, the local page for the
  duck's daemons, shows quacksat as "not available" until it exposes its
  status, its configuration and later a chat
  ([todo.md](todo.md)).
- **Announcements wait for a pause, not for the story's order**: a
  sentence of the duck's own is said only between turns, so it can come
  after a late answer from the model about something that happened
  later, or before it.
- **Italian puts "in" before every place**: "Sono arrivata in cucina" is
  right, "in divano" is not.
- **An untrusted pose alone is not said**: the duck may have been moved,
  but until something moves the satellite says nothing about it.
- **The `wyoming` backend says nothing on its own**: no announcements on
  the Home Assistant path yet (HA's announce action is not wired).
- **Fixed phrases**: the duck's own sentences are fixed Italian or
  English phrases, not worded by the agent.
- **CPU on the RK3566 is unmeasured**: the wake word's budget, and the
  unit's memory caps (192M high, 256M max), are first guesses for a 1 GB
  board. The 320 ms echo tail and the `[gait]` trims were tuned on a
  laptop and on the twin; both are to be measured again on the duck.
- **"Hey Daffy" and an Italian accent**: an Italian speaker scored
  0.20–0.37 against the 0.5 threshold, where the stock `hey_jarvis` woke
  at once; lower the threshold or train the phrase you would say
  ([custom-wake-word.md](custom-wake-word.md)).
- **robotd is pinned to `daemon-v0.14.4`**: a newer robotd on the board
  is untested from here.

## What v0.1.0 final needs

A first session on the real duck: the codec's capture and playback, the
wake word's CPU on the RK3566, the echo tail measured there, and a
conversation that walks it to the kitchen.
