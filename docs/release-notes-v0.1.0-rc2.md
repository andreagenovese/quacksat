# quacksat v0.1.0-rc2 — release notes

2026-10-10. Italian copy: [release-notes-v0.1.0-rc2.it.md](release-notes-v0.1.0-rc2.it.md).
The full list of what changed is in [CHANGELOG.md](../CHANGELOG.md); what
v0.1.0-rc1 brought, and the limits it still has, in
[release-notes-v0.1.0-rc1.md](release-notes-v0.1.0-rc1.md).

## What is new

**The duck's body in Home Assistant.** With `[mqtt]` on, on any backend,
quacksat publishes the duck as a Home Assistant device through MQTT
discovery ([ADR 0007](adr/0007-home-assistant-entities-over-mqtt.md)):

- buttons to walk forward, turn left and right, stop, center the head,
  run each skill the robot lists, play each sound (disabled by default)
  and, with the navigation, go to each known place;
- the head as three sliders (pitch, yaw, roll);
- sensors for the battery, the robot's health, its mode, the last
  command's answer and, with the navigation, where it is and how its
  journey is going.

On the `wyoming` path this is what makes the duck a whole robot: Assist
gives it the voice, these entities give it the body. Example sentence
automations ("papera cammina", "duck turn left", "duck go to the
kitchen"), Speech-to-Phrase sentences on the areas and a blueprint for
the places, in Italian and English, let Home Assistant's own agent
drive it by voice with no LLM, and answer with what the duck actually
did.

Every motion is an impulse — a press walks for a few seconds and stops —
and one walk runs at a time in the whole satellite, whoever asked (a
voice turn, an MCP agent, a button); **stop** always wins.

**Guides.** [Trying quacksat without a duck](try-without-a-duck.md): all
three modes on a laptop, with a real `robotd --fake` as the body. And
[the Home Assistant setup guide](home-assistant/setup-guide.md): the
Wyoming satellite and the MQTT device configured together, step by
step, with screenshots.

## Status

- **Not yet on a physical duck.** It arrives in December 2026.
- **The Home Assistant device** was verified against Home Assistant
  2026.10 and Speech-to-Phrase 1.4.3 in containers (discovery, every
  command, the refusals, availability, the sentence automations by text
  and by audio), then end to end against a Home Assistant install in
  everyday use, with quacksat on a Mac: "hey Daffy" on the satellite,
  "papera cammina" transcribed by the house's pipeline, the automation
  pressing the button, the walk, "Fatto." spoken back.
- **No regression on the other paths**, checked live on a Mac against a
  real robotd `--fake` before the tag: `direct` driven through its MCP
  server with MQTT on beside it (a stop from Home Assistant ends a walk
  an MCP agent started, and the next walk runs whole), `agent` against
  the bridge's fake mode with and without MQTT, the bring-up `none`.
  96 automated tests, clippy clean, the board's cross-build.

## Install

As for rc1: the release carries `quacksat-0.1.0-rc2-aarch64-linux.tar.gz`
with its `.sha256`, and the bare binary `quacksat-aarch64-linux` with its
own. See [Installing from a release](../README.md#installing-from-a-release)
and the package's [README-install.md](../scripts/package/README-install.md),
which now has the `[mqtt]` section. Upgrading keeps
`/etc/robot/quacksat.toml`: `[mqtt]` is off until you add it.

## Configuration

```toml
[mqtt]
enabled = true
host = "homeassistant.local"   # the MQTT broker
port = 1883
username = "quacksat"          # mandatory: the topics drive a robot that walks
password = "…"
# node = "quacksat"            # the id in topics and entity ids; empty: the backend's name
# device_name = ""             # empty: "Papera" or "Duck", by [announce] language
step_s = 2.0                   # one forward press, seconds (at most 3)
turn_s = 2.0                   # one turn press
```

The broker user, an ACL, the example automations and the topics are in
[docs/home-assistant/README.md](home-assistant/README.md).

## Known limitations

New with this release:

- **The manual steps have no cliff guard.** The guard rides with the
  navigation's journeys; a forward press does not look down. Keep the
  duck away from the edge of a table.
- **Plain MQTT only**: no TLS to the broker, QoS 0. Meant for a broker
  on the home network, with a user of its own for the duck.
- **Speech-to-Phrase as a Home Assistant add-on**: where it reads its
  own sentence files from, and whether it retrains on its own when an
  area or a blueprint sentence is added, are still to be checked; until
  then, restart the add-on after adding one.
- **Home Assistant's own agent does not press buttons by name**: the
  voice goes through the example automations; an LLM agent in Home
  Assistant sees the buttons as they are.

Still true from rc1: no control socket for quack-control yet; the
`wyoming` backend says nothing on its own (the journey sensor now gives
Home Assistant what it needs to announce, not yet wired); Italian puts
"in" before every place; CPU and memory on the RK3566 unmeasured, the
echo tail and the `[gait]` trims to measure again on the duck; "hey
Daffy" and an Italian accent; robotd pinned to `daemon-v0.14.4`.

## What v0.1.0 final needs

A first session on the real duck: the codec's capture and playback, the
wake word's CPU on the RK3566, the echo tail measured there, a
conversation that walks it to the kitchen — and, now, a button in Home
Assistant that does the same.
