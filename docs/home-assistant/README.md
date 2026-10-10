# The duck in Home Assistant

quacksat can publish the duck's body to Home Assistant as a device:
buttons that walk it, turn it, stop it and run its skills, sliders for
the head, and sensors for the battery, its health and, with the
navigation, where it is and how its journey is going. On the `wyoming`
path this is what makes the duck a whole robot: Assist gives it the
voice, these entities give it the body. It works on every backend.
Italian copy: [README.it.md](README.it.md). The decision and the
measurements behind it: [ADR 0007](../adr/0007-home-assistant-entities-over-mqtt.md).

## What you need

- An MQTT broker. In Home Assistant OS: the **Mosquitto broker** add-on,
  and the **MQTT** integration pointed at it.
- A broker user for the duck. The topics drive a robot that walks, so
  quacksat refuses to start MQTT without a username and a password.

Mosquitto ACL that lets only the duck and Home Assistant write under the
duck's prefix (the add-on's `customize` folder, or your own
`acl_file`):

```text
user quacksat
topic readwrite quacksat/#
topic write homeassistant/#

user homeassistant
topic readwrite #
```

## quacksat's side

In `/etc/robot/quacksat.toml`:

```toml
[mqtt]
enabled = true
host = "homeassistant.local"
port = 1883
username = "quacksat"
password = "…"
# node = "quacksat"        # empty: the backend's name ([wyoming] name)
# device_name = "Papera"   # empty: "Papera" or "Duck", by [announce] language
step_s = 2.0               # one forward press, seconds (at most 3)
turn_s = 2.0               # one turn press, about 90°
```

Restart quacksat: the device appears in Home Assistant by itself (MQTT
discovery), in the same area as the satellite if `[wyoming] area` is set.

## What appears

With the node `quacksat` and the Italian names:

| Entity | What it does |
|---|---|
| `button.quacksat_forward` | walks forward for `step_s` (≈ 20 cm in 2 s) |
| `button.quacksat_turn_left` / `_turn_right` | walks a turn for `turn_s` (the duck cannot turn in place) |
| `button.quacksat_stop` | ends the walk at once |
| `number.quacksat_head_pitch` / `_yaw` / `_roll` | the head, in radians, within the tool's limits |
| `button.quacksat_head_center` | head back to the center |
| `button.quacksat_skill_<name>` | one per skill the robot lists |
| `button.quacksat_sound_<tag>` | the sounds, disabled until you enable them |
| `button.quacksat_place_<name>` | one per place, with the navigation |
| `sensor.quacksat_battery`, `binary_sensor.quacksat_problem`, `sensor.quacksat_mode` | battery %, health (the reason as an attribute), mode |
| `sensor.quacksat_where`, `sensor.quacksat_journey` | with the navigation: the nearest place, the journey's state |
| `sensor.quacksat_last_answer` | the last command's answer, as the duck would say it |

Every motion is an impulse: a press walks for a few seconds and stops.
A press while the duck is walking, or while a journey runs, is refused
("I'm already walking"), and **stop** always wins. The manual steps have
no cliff guard (the guard rides with the navigation's journeys): keep
the duck away from the edge of a table when you press forward.

## By voice

Home Assistant's own agent does not press buttons by name, and
Speech-to-Phrase only learns sentences from automations. Measured on
2026-10-10, see ADR 0007 §7. So the voice comes from three files in this
folder:

1. **[automations.yaml](automations.yaml)** (Italian:
   [automations.it.yaml](automations.it.yaml)). One automation per
   command family, with a sentence trigger: "duck walk forward", "duck
   turn left", "duck stop", "duck do a somersault", "duck look left",
   "duck how much battery do you have", "duck go to the kitchen". Each
   waits for quacksat's answer and says what actually happened. Paste
   them into `automations.yaml` (change `quacksat` in topics and ids if
   your node is different) and reload the automations.
2. **[speech-to-phrase/](speech-to-phrase/)**, only if you use
   Speech-to-Phrase. A sentence file that teaches it "duck go to <area>"
   for every Home Assistant area. It goes in the add-on's custom
   sentences folder, under the language (`it/` or `en/`). The exact path
   in the add-on is still to be checked; the add-on may need a restart to
   retrain after you add an area. **Give the duck's places the names of
   your areas** and every room works with no list to keep.
3. **[blueprints/quacksat_go_to.yaml](blueprints/quacksat_go_to.yaml)**
   (Italian: [quacksat_go_to.it.yaml](blueprints/quacksat_go_to.it.yaml)).
   For the places that are not rooms ("the sofa"): one sentence per place,
   which Speech-to-Phrase learns from the trigger. Copy it to
   `config/blueprints/automation/quacksat/` and create an automation from
   it.

With faster-whisper the first file alone is enough, places included: the
"go to" automation takes any name and quacksat says whether it knows it.

The errors quacksat sends back are in the duck's language
(`[announce] language`), because the automations read them aloud.

## The topics, for your own automations

With `base_topic = "quacksat"` and the node `quacksat`:

| Topic | Direction | Payload |
|---|---|---|
| `quacksat/quacksat/cmd/<command>` | to the duck | `forward`, `turn_left`, `turn_right`, `stop`, `head_center`: anything non-empty; `head_pitch`/`_yaw`/`_roll`: a number; `skill`: a skill name; `sound`: a tag; `go_to`: a place as spoken |
| `quacksat/quacksat/result/<command>` | from the duck | `{"ok": true, …}` or `{"ok": false, "error": "…", "detail": "…"}`, not retained |
| `quacksat/quacksat/state` | from the duck | one JSON document, retained: `battery`, `volts`, `healthy`, `reason`, `mode`, `walking`, and with the navigation `place`, `at_place`, `journey`, `journey_reason` |
| `quacksat/quacksat/availability`, `…/robot` | from the duck | `online` / `offline`: quacksat itself, and its line to the robot |

Commands are never acted on when retained, nor when empty (that is a
retained message being cleared), and presses do not queue.

## Trying it without the duck

quacksat on a laptop with a real robotd `--fake` as the body is enough;
[docs/try-without-a-duck.md](../try-without-a-duck.md) sets up that
side. For Home Assistant, either a container (with a Mosquitto
container beside it) or the one you already have: on 2026-10-10 the
whole loop ran against a Home Assistant install in everyday use, with
quacksat on a Mac — the device by discovery, the buttons and sliders,
and "hey Daffy, papera cammina" through the Wyoming satellite, the
automation, the walk and the spoken "Fatto." (ADR 0007 §7).

On your Home Assistant, with the Mosquitto add-on and the MQTT
integration already set up:

1. Create a broker user for the duck (a Home Assistant user works: the
   add-on accepts them).
2. Add `[mqtt]` to the laptop's config, with `host` = Home Assistant's
   address, and start quacksat: the device appears by itself.
3. For the voice, set `[wyoming] bind = "0.0.0.0:10700"` and add the
   Wyoming Protocol integration with the laptop's IP; then paste the
   automations.

A refusal like "the policy is not driving" on a skill is the robot
answering, which proves the line works.

**Cleaning up afterwards.** Stopping quacksat leaves the device in Home
Assistant, unavailable. Delete it from its device page, then clear the
retained messages it left on the broker (`-r -n` publishes an empty
retained message, which removes it):

```sh
for t in homeassistant/device/quacksat/config quacksat/quacksat/state \
         quacksat/quacksat/availability quacksat/quacksat/robot; do
  mosquitto_pub -h homeassistant.local -u quacksat -P '…' -r -n -t "$t"
done
```
