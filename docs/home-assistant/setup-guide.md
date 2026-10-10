# Setting up the duck in Home Assistant: Wyoming + MQTT

A step-by-step guide, with screenshots, to connect quacksat to Home
Assistant in both ways at once:

- **Wyoming**: quacksat is an Assist voice satellite — the duck listens
  and speaks.
- **MQTT**: the duck's body appears as a device, with buttons to walk,
  turn, stop and run skills, the head as sliders, and sensors.

Italian copy: [setup-guide.it.md](setup-guide.it.md). The details behind
each step are in [README.md](README.md); the decision and its
measurements in [ADR 0007](../adr/0007-home-assistant-entities-over-mqtt.md).

The screenshots come from a run on 2026-10-10 against a Home Assistant
in everyday use, **without the duck**: quacksat ran on a Mac, with the
Mac's microphone and speakers and a real `robotd --fake` standing in for
the robot ([try-without-a-duck.md](../try-without-a-duck.md)). On the
duck only the address and the audio change; the steps in Home Assistant
are the same. Home Assistant's interface in the screenshots is in
Italian; the labels are translated in the text. Private details
(addresses, passwords, other devices) are blurred.

## Before you start

- In Home Assistant: the **Mosquitto broker** add-on and the **MQTT**
  integration, already set up.
- A **broker user for the duck**: a Home Assistant user works
  (Settings → People → Users), e.g. `quacksat`. quacksat refuses to
  start MQTT without a username and a password.
- An **Assist pipeline** in your language (here: Speech-to-Phrase,
  Piper and Home Assistant's own agent, in Italian).
- On a laptop without the duck: `robotd --fake` running and quacksat
  built ([try-without-a-duck.md](../try-without-a-duck.md)).

## 1. quacksat's config

![quacksat's config](img/setup/01-config.jpg)

What matters:

```toml
backend = "wyoming"
robotd_socket = "/tmp/qsm/robotd.sock"

[audio]   # the Mac's microphone and speakers; leave out on the duck
capture_command = ["sox", "-q", "-d", "-t", "raw", "-r", "48000", "-c", "2", "-e", "signed-integer", "-b", "16", "-"]
playback_program = "/path/to/quacksat/scripts/aplay-shim-macos.sh"

[wake]
mode = "openwakeword"
models_dir = "/path/to/quacksat/models"
model = "hey_daffy.onnx"
threshold = 0.5

[wyoming]
bind = "0.0.0.0:10700"   # reachable from the network: Home Assistant connects here
name = "quacksat"

[announce]
language = "it"          # or "en"

[mqtt]
enabled = true
host = "<Home Assistant's address>"
port = 1883
username = "quacksat"
password = "…"
```

Start it (on a laptop, from the checkout):

```sh
cargo build -p quacksat && target/debug/quacksat quacksat-dev.toml
```

The log must show `mqtt: connected to the broker`, `mqtt: discovery
published entities=28` and `wyoming satellite listening
bind=0.0.0.0:10700`. On first start macOS may ask whether to accept
incoming connections: **Allow**.

## 2. The voice satellite (Wyoming)

### 2.1 Add the integration

Settings → Devices & services → **Wyoming Protocol** → Add service
("Aggiungi servizio"). Host = the address of the laptop (or of the
duck), port `10700`.

![Wyoming Protocol: host and port](img/setup/02-wyoming-host.jpg)

### 2.2 The satellite is ready

Home Assistant finds the satellite and asks for the last settings:
the device's name, the assistant and the voice.

![Voice Satellite setup](img/setup/03-satellite-setup.jpg)

### 2.3 The assistant

The button next to the assistant opens the pipeline. Here:

- Conversation agent: **Home Assistant**, Italian;
- Speech-to-text: **speech-to-phrase**, Italian;
- Text-to-speech: **piper**, `it_IT`, voice **paola (medium)**.

![The assistant's pipeline](img/setup/04-pipeline.jpg)

Home Assistant's own agent is enough: the duck's sentences come from
the automations of step 4, no LLM needed.

## 3. The body (MQTT)

### 3.1 The device appears by itself

As soon as quacksat connects to the broker, Settings → Devices &
services → **MQTT** lists the device **Papera** (Microduck) with its 28
entities ("Duck" when `[announce] language` is English). Nothing is
added by hand.

![The Papera device in the MQTT integration](img/setup/05-mqtt-device.jpg)

### 3.2 The device page

Controls: forward ("avanti"), turn right/left, the skills, stop, the
three head sliders, head to the center, and one "go to: …" button
("vai: …") per place the navigation knows. The activity on the right
shows the sensors: where, problem, journey, mode, last answer.

![The Papera device page](img/setup/06-papera-device.jpg)

With the fake robot on a Mac the **problem** sensor is on: the reason is
"policy unavailable: ONNX Runtime not loadable", the walking policy
missing on the Mac. That is expected; on the duck it goes off.

### 3.3 Try the buttons

Every press reaches the robot and the answer shows in **last answer**:
"ok" for forward and for the head, "the policy is not driving — press
Start on the pad" for a skill (the fake robot answering, which proves
the line works).

![Activity after a few presses](img/setup/07-papera-activity.jpg)

The six sound buttons are disabled on purpose ("+6 disabled
entities"): enable them on the device page if you want them.

## 4. Spoken sentences (automations)

Home Assistant's agent does not press buttons by name: for "papera
cammina", "papera gira a sinistra", "papera quanta batteria hai" (or
"duck walk forward", "duck turn left", "duck how much battery do you
have") you need the example automations. Copy
[automations.it.yaml](automations.it.yaml) or
[automations.yaml](automations.yaml) to the end of your own
`automations.yaml` (here with the Studio Code Server add-on), then
Developer tools → YAML → **Automations** (reload).

![The automations pasted into automations.yaml](img/setup/08-automations.jpg)

If the duck's node is not `quacksat`, change it in the topics
(`quacksat/quacksat/...`) and in the entity ids (`button.quacksat_...`).

With Speech-to-Phrase, new sentences are learned when the add-on
retrains: if the duck first answers "No text recognized", restart the
Speech-to-Phrase add-on.

## 5. On a dashboard

The duck's entities go on a dashboard like any other: the buttons on
one card, battery, where, mode, problem, last answer and journey on
another.

![The duck on a dashboard](img/setup/09-dashboard.jpg)

## 6. Try it by voice

"**hey Daffy**" … "**papera cammina**". In quacksat's log:

![The log of a spoken command](img/setup/10-voice-log.jpg)

```
wake                                   ← "hey Daffy" heard
pipeline error "No text recognized"    ← twice: Speech-to-Phrase had not learned the sentence yet
mqtt: command forward                  ← the automation pressed the button
transcript "papera cammina"
thinking pose: swaying …               ← the head sways while it waits
command answered {"ok":true,"walked_s":2.0}
tts playback starting → tts played     ← "Fatto." from the speakers
```

Bursts of "home assistant connected / disconnected" earlier in the log
are Home Assistant probing the satellite, mostly while its settings are
being changed: they are normal.

## Cleaning up after a test

Stopping quacksat leaves the device in Home Assistant, unavailable. To
remove it: the Papera device page → ⋮ → **Delete**, then clear the
retained messages left on the broker:

```sh
for t in homeassistant/device/quacksat/config quacksat/quacksat/state \
         quacksat/quacksat/availability quacksat/quacksat/robot; do
  mosquitto_pub -h <Home Assistant's address> -u quacksat -P '…' -r -n -t "$t"
done
```
