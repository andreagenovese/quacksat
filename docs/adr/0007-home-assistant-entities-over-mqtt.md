# ADR 0007: The duck's body as Home Assistant entities, over MQTT

- Status: accepted
- Date: 2026-10-09, revised 2026-10-10 (the base case is Home
  Assistant's own agent, no LLM; §7 rewritten on what was measured)
- Inputs: ADR 0002 (interchangeable backends), ADR 0004 §4 (the tool
  surface), ADR 0006 §4 (one driver at a time),
  `docs/study/quacksat-ha-vs-agent.md` ("robot control: absent" on
  path A), the user's question of 2026-10-09
- Italian copy: [0007-home-assistant-entities-over-mqtt.it.md](0007-home-assistant-entities-over-mqtt.it.md)

## Context

On the `wyoming` path the duck is an Assist satellite and nothing else.
The Wyoming protocol carries audio and pipeline events; it has no way
to declare an entity or receive a command. Home Assistant hears "duck,
walk forward", runs it through its intents or its conversation agent,
and finds nothing to call. Even with Arkimede as the conversation agent
the answer is the same: Arkimede drives Home Assistant over MCP, and
Home Assistant knows nothing of the duck. The robotd lane the wyoming
backend holds carries the duck's own cues (the chirp, the thinking
sway, the sad tock) and no tool.

So path A is half a robot: it has a voice and no body. The study of
2026-08 wrote it down as "absent (or via HA automations → RPC)" and
left it there.

The case this ADR is designed for is the plainest one: Home
Assistant's own conversation agent, with no LLM behind it. That agent
does not reason; it matches the transcribed text against a list of
sentences — the built-in ones applied to the names of the exposed
entities, and the ones the user adds — and runs what matches, or says
it did not understand. Whatever the duck can do by voice on path A has
to be reachable that way. An LLM agent, where there is one, is an
addition on top (§7), never a requirement.

The body's commands already exist as a table: `quacksat_core::tools`,
one allowlist with satellite-side clamps, executed for the agent
bridge, for the direct backend's LLM and for its MCP server. What is
missing is a door into Home Assistant.

## Decision

### 1. MQTT discovery, inside quacksat

quacksat connects to the home's MQTT broker (in an HAOS install, the
Mosquitto add-on) and publishes a discovery payload: Home Assistant
creates a device, "the duck", with the entities in §3, and from then on
pressing one of them is a message on a topic that quacksat executes.

Considered and set aside:

- **ESPHome's native API** instead of Wyoming. It is what the Voice PE
  speaks and would give one device that is both satellite and entities,
  but it means a protobuf-over-TCP server with Noise encryption in Rust:
  in practice a fourth backend, for the result §3 gives with a client.
- **A custom Home Assistant integration** (Python, HACS). A second
  codebase in a second language, released on Home Assistant's calendar,
  to say what discovery says in a JSON payload.
- **An MCP server on the wyoming path.** Small (the direct backend's
  `mcp.rs` moved to the core), but it reaches only an MCP agent such
  as Arkimede; Home Assistant's own intents, Speech-to-Phrase,
  automations and dashboards stay without the body. It complements
  this ADR and is not replaced by it.

The client is `rumqttc` with its default features off: plain TCP, sync
API (a `Client` plus a `Connection` iterator on a thread of its own),
no tokio runtime in a backend that has none, and no `ring`, which is
what the TLS feature would bring and is C and assembly. TLS waits for a
pure-Rust provider or a real need; on a home LAN the broker's password
and ACL are the fence (§5).

### 2. Opt-in, every backend, its own thread

`[mqtt] enabled = true` turns it on. It is written for path A but not
fenced to it: the entities are as useful on `agent` and `direct` for
automations and dashboards, and the code does not care which backend
holds the microphone.

It runs on a thread of its own, beside the voice loop, and shares the
`Robot` with it — the robotd lane, the skill list, the navigation
lane — behind the same mutex the direct backend's MCP server already
uses (`SharedRobot`, which moves from `backends/direct` to the core).
The wyoming backend, which today holds a bare `Lane`, holds the shared
`Robot` instead.

### 3. The entity catalog

MQTT is a fourth caller of the tool table, not a new surface: every
command entity becomes one `tools::execute` call with fixed or
clamped arguments. Nothing reaches robotd that the LLM tools could not
already send, and the clamps are the same constants.

| Entity | Kind | Becomes | Notes |
|---|---|---|---|
| Walk forward | `button` | `robot.move {vx: 0.3, duration_s: step_s}` | `step_s` default 2 s, capped at 3 (the tool's own cap); ≈ 20 cm |
| Turn left / right | `button` ×2 | `robot.move {vx: 0.3, vyaw: ±0.7, duration_s: turn_s}` | the duck cannot turn in place; `turn_s` default 2 s ≈ 90° |
| Stop | `button` | ends the walk this thread is pumping | see §4 |
| Head pitch / yaw / roll | `number` ×3 | `robot.head` | min/max are the tool's clamps; state is the last value sent |
| Center head | `button` | `robot.head {}` | |
| One per skill | `button` | `robot.skill {name}` | built from `robot.skills` at connect; re-published when the robot comes back with a different list |
| One per sound tag | `button` | `robot.sound {tag}` | `enabled_by_default: false`, six of them would crowd the device page |
| One per place | `button` | `robot.go_to {place}` | only when quack-navd answers; built from `robot.list_places`, re-published when a place is remembered or forgotten |
| Battery | `sensor` | `robot.state` → `battery` | as robotd reports it |
| Healthy | `binary_sensor` (`problem`) | `robot.state` → `healthy`, `reason` as attribute | |
| Mode | `sensor` | `robot.state` → `mode` | |
| Where | `sensor` | `robot.where_am_i` / `robot.map_status` | navigation only: the place name, or the pose as attributes |
| Journey | `sensor` | `robot.map_status` | navigation only: idle, walking, arrived, failed, with the reason |

**Places: buttons for the hand, a named command for the voice.** One
button per place serves dashboards and automations, built from
`robot.list_places` and re-published when a place is remembered or
forgotten. The voice does not go through them: as measured (§7), no
built-in sentence of Home Assistant presses a button, in Italian or in
English, and Speech-to-Phrase does not learn entity names outside the
on/off sentences for lights, switches and fans. So a place can also be
asked for by name: `cmd/go_to` takes the place as spoken, quacksat
matches it against `robot.list_places` ignoring case and a leading
article (il, lo, la, l', i, gli, le), and answers on `result/go_to`
with `{ok: true, place}` or `{ok: false, error, places}` — "no place
called garage", and the ones it knows. It is the one command not tied
to an entity; a sentence automation is its caller (§7).

Not a `select`: whether Home Assistant's own agent sets an option by
voice was not verified, and the named command makes the question moot.

Not scenes, although they were the surprise of the measurements: Home
Assistant's agent activates a scene by its bare name ("papera vai in
salotto" → activated), so MQTT scenes would give place commands with no
file at all. Two things weigh against: Speech-to-Phrase's Italian
sentences leave scenes out (the same phrase, spoken, came back empty),
and the reply is Home Assistant's fixed "Ho attivato la scena…", which
says nothing when quacksat refuses.

**Names in the user's language.** Friendly names follow `[announce]
language` ("Papera avanti", "Papera vai in cucina"): they are what a
dashboard shows and what an LLM agent reads. Entity ids stay English
and stable (§6), so an automation survives a change of language.

Left out of v1, on purpose:

- **Walking backward.** The twin's gait does not back straight (it
  backs only with some yaw); a button that promises "back" and walks a
  curve is worse than no button. It comes back after the December
  measurements on the real gait.
- **`robot.look`.** It wants a point in metres in the duck's frame,
  which nobody types into a dashboard.
- **`robot.map_explore`.** A long autonomous job is not a button to
  press by mistake from a phone. It can come later, disabled by default.
- **`robot.get_frame`.** Unsupported on the robot until mediad.

Sensors are polled on the cadence the announcer already uses: every
2 s while the duck moves or a journey runs, every 10 s otherwise.

### 4. Every motion is an impulse

There is no "walk until I say stop" entity. A press is one bounded
`timed_move` — the same pump the tools use, robotd's deadman behind it
— and when it ends the duck stands. Consequences for the topics:

- **Command topics are never retained, and a retained message on one is
  dropped.** MQTT hands a subscriber the retained message on every
  (re)connect: a retained "forward" would walk the duck after every
  broker restart.
- **QoS 0 and a clean session.** A command sent while quacksat was
  offline is lost, not replayed hours later.
- **A press while walking is dropped**, and logged. Presses do not
  queue: five impatient taps are one step, not a metre.
- **Stop always wins.** It sets a flag the pump checks every tick
  (40 ms), so the walk ends at the next tick and the deadman does the
  rest. It does not call robotd's `robot.stop`, which is a mode change
  and not "stand still".

### 5. One driver, and who may press

ADR 0006 §4 holds: one thing drives the legs at a time.

- A walk from MQTT is refused while a navigation journey runs, while a
  voice turn is executing a tool call, or while another walk runs; the
  `Robot` mutex serialises the rest. The refusal is published on the
  command's result topic (§6) and logged.
- A navigation job started from a place button is followed by the
  announcer like one started by voice, so on `agent` and `direct` the
  duck still says how it ended.
- The topic tree drives a robot that can walk off a table: the manual
  steps carry no cliff guard (the guard rides with the navigation's
  journeys, exactly as for the `robot.move` tool). quacksat refuses to
  start the MQTT thread without `username` and `password`, and the
  install docs give the Mosquitto ACL that lets that user, and Home
  Assistant's, alone write under the duck's prefix.

### 6. Topics and availability

With `node = "quacksat"` (default: the active backend's name, `[wyoming] name`
or `[agent] name`, else `quacksat`; `[wyoming] area` becomes the
device's suggested area, so the duck and its satellite land in the same
room):

- discovery: one device payload at
  `homeassistant/device/<node>/config` (device-based discovery,
  Home Assistant 2024.11+), retained;
- commands: `quacksat/<node>/cmd/<entity>`;
- the one command without an entity: `quacksat/<node>/cmd/go_to`, the
  place name as payload (§3);
- results: `quacksat/<node>/result/<entity>` (and `result/go_to`),
  `{ok, error?}`, not retained — what an automation can wait on;
- state: `quacksat/<node>/state`, one JSON document, retained, read by
  every sensor through a value template;
- availability, two topics with `availability_mode: all`: quacksat's
  own (`online`, and `offline` as the MQTT last will) and the robot's
  (`offline` while the robotd lane is down). Home Assistant greys the
  duck out in either case instead of accepting presses that cannot
  land.

### 7. How a voice command reaches the legs

quacksat never understands speech: on path A the duck sends audio,
Home Assistant transcribes it and decides. MQTT only gives Home
Assistant something to press.

**What was measured.** On 2026-10-10, against Home Assistant 2026.10.0
in a container and Speech-to-Phrase 1.4.3 trained on it, with Italian
audio from macOS's Alice voice sent over Wyoming, and the transcripts
then given to Home Assistant's agent:

| Way | Home Assistant's agent (text) | Speech-to-Phrase (audio) |
|---|---|---|
| built-in sentence on a button ("premi / accendi papera avanti") | no: no Italian or English sentence covers buttons; "accendi {name}" is for light, switch, fan, media_player, input_boolean, climate | no |
| built-in sentence on a sensor ("qual è lo stato di batteria papera") | yes, that phrasing only | no: its Italian sensor sentences are disabled |
| bare name of a scene or a script | yes | no: its Italian scene and script sentences are disabled |
| `custom_sentences/` + `intent_script` | yes | no: it does not read Home Assistant's sentence files |
| automation with a `conversation` trigger | yes | yes: it asks Home Assistant for the trigger sentences |
| the same with a free slot (`{posto}`) | yes, any name | only names it knows from elsewhere (below) |
| its own sentence file, with `{area}` | — | yes: every Home Assistant area, a new one after retraining |
| its own sentence file, with `{name}` | — | yes, bare entity names; the domain filter is ignored |

The form that works for both is the automation with a sentence
trigger. The examples in the repo are automations, never
`intent_script`.

**The base case: Home Assistant's agent, a closed vocabulary.** The
repo ships the examples (`docs/home-assistant/`, Italian and English),
one automation per command family —

```yaml
triggers:
  - trigger: conversation
    command:
      - "papera (avanza|vai avanti|cammina)"
actions:
  - action: button.press
    target: {entity_id: button.quacksat_forward}
  - wait_for_trigger:
      - trigger: mqtt
        topic: quacksat/quacksat/result/forward
    timeout: 3
  - set_conversation_response: >-
      {% if not wait.trigger %}Il robot non risponde.
      {% elif wait.trigger.payload_json.ok %}Vado!
      {% else %}Non posso: {{ wait.trigger.payload_json.error }}{% endif %}
```

— so "papera avanza", "papera gira a destra", "papera fermati", "papera
fai la capriola", "papera alza la testa", "papera quanta batteria hai"
are answered with what actually happened, not with a "Vado!" said
before quacksat refused. That is what the result topics (§6) are for.

Rules the examples follow, because of who listens:

- **Closed lists only**, written out in the trigger (skills,
  directions, head poses): a free slot reaches Speech-to-Phrase only
  for the names it already knows.
- **Head angles are words, not numbers.** "Alza / abbassa / gira a
  sinistra la testa" map to fixed poses.
- **Sensors get their own sentence.** The built-in sensor question
  does not survive Speech-to-Phrase in Italian, so "papera quanta
  batteria hai" is an automation that reads the sensor and says it.
- **The fast lane stays fast.** These sentences are matched locally;
  with an LLM fallback configured, Home Assistant still tries them
  first.

**Places by voice: three pieces.** Places are the one open list, and
they change as the duck learns the house.

1. **One automation with a free slot** catches every "papera vai in
   …" and hands the name to quacksat, which says whether it knows it:

   ```yaml
   triggers:
     - trigger: conversation
       command:
         - "papera (vai|va) [in|nel|nella|al|alla] {posto}"
   actions:
     - action: mqtt.publish
       data:
         topic: quacksat/quacksat/cmd/go_to
         payload: "{{ trigger.slots.posto }}"
     - wait_for_trigger:
         - trigger: mqtt
           topic: quacksat/quacksat/result/go_to
       timeout: 3
     - set_conversation_response: >-
         {% if not wait.trigger %}Il robot non risponde.
         {% elif wait.trigger.payload_json.ok %}Vado in {{ trigger.slots.posto }}.
         {% else %}{{ wait.trigger.payload_json.error }}{% endif %}
   ```

   With faster-whisper this alone covers any place. Measured: the
   transcripts "papera va in Cucina", "papera vai nella Camera da
   letto", "papera vai in garage" all reached the automation with the
   right name.
2. **A Speech-to-Phrase sentence file on the areas**, shipped as
   `docs/home-assistant/speech-to-phrase/it/quacksat.yaml`:

   ```yaml
   language: it
   intents:
     QuacksatGoArea:
       data:
         - sentences:
             - "papera (vai|va) (in|nel|nella|al|alla) {area}"
   ```

   Every room of Home Assistant becomes a place Speech-to-Phrase can
   hear, with no list to keep: an area created during the test
   ("Studio") was heard after retraining. Hence the advice the install
   docs give: **name the duck's places as Home Assistant's areas.**
3. **A blueprint for the places that are not rooms** ("Papera, vai al
   posto", a list of sentences such as "papera vai in terrazza").
   Speech-to-Phrase learns them through the trigger sentences, with no
   file of its own; the list is kept by hand when the duck learns or
   forgets a place.

What this case cannot do: compose ("vai in cucina e poi siediti"),
understand a phrasing nobody wrote down, or answer anything outside the
list. It gives a fixed vocabulary — some twenty sentences, plus the
places — that is fast, local, and covers the short commands that voice
is for.

**An addition: an LLM agent.** Where Home Assistant's agent is an LLM,
or Arkimede reaches Home Assistant over its MCP server, the exposed
buttons are visible to it as they are, and free requests become
possible. For anything with a parameter the examples add Home
Assistant scripts (`script.papera_vai_al_posto` with its place,
`script.papera_testa` with its angles): an exposed script is a tool
with a name, a description and fields, which an LLM calls reliably and
which waits on the result topic like the automations. Nothing in
quacksat changes for this.

**Without voice: automations and dashboards.** The same entities are
ordinary buttons and sensors: a card on the phone, "at eight go to the
kitchen", "battery under 20 %, tell me", and the spoken outcomes of
path A (Open questions).

### 8. Config

```toml
[mqtt]
enabled = false
host = "homeassistant.local"
port = 1883
username = ""            # required when enabled
password = ""            # required when enabled
node = ""                # default: the backend's name, else "quacksat"
discovery_prefix = "homeassistant"
base_topic = "quacksat"
step_s = 2.0             # one forward press, capped at 3
turn_s = 2.0             # one turn press, capped at 3
```

## Consequences

- Path A becomes a whole robot: the voice through Assist, the body
  through entities, and the same body reachable by automations,
  dashboards and any agent that already drives Home Assistant.
- One more network surface on a machine that walks. It is opt-in,
  authenticated, ACL-fenced, impulse-only, and cannot send anything the
  tool table could not; it is still a door, and the install docs say
  so.
- A broker becomes a dependency for whoever turns it on. HAOS users
  have one a click away; the satellite runs exactly as before without
  it.
- The discovery payload is a contract with Home Assistant: entity ids
  are derived from `node` and the entity key and must not change once
  published, or automations break. Place buttons are the exception by
  nature: forgetting a place removes its button, and an automation that
  pressed it.
- Path A by voice is a closed vocabulary, except for places, which
  are open with faster-whisper and as wide as Home Assistant's areas
  with Speech-to-Phrase. It is enough for the short commands and it is
  all local; free requests need an LLM agent, which this ADR does not
  require and does not prevent.
- quack-control (the browser page) and this ADR answer different
  people: the page is the duck's own control plane on the LAN, MQTT is
  the door into Home Assistant. Both go through the one-driver rule.
- Testable without the duck: robotd `--fake`, a local Mosquitto,
  `mosquitto_sub`/`mosquitto_pub` for the topics, and a Home Assistant
  container for discovery; the refusals that prove the lane is alive
  are the ones in `docs/study/live-without-a-duck.md`.

## Open questions

- **Spoken outcomes on path A.** The wyoming backend says nothing on
  its own (`docs/todo.md`). The journey sensor gives Home Assistant
  what it needs to announce "I made it to the kitchen" through the
  satellite; whether a Wyoming satellite accepts `assist_satellite.announce`
  is to verify before choosing between that and an example automation.
- **Speech-to-Phrase as an add-on.** Two things the container could
  not answer and the home install will: where the add-on reads its own
  sentence files from (`/share/speech-to-phrase/custom_sentences/` is
  remembered, not checked), and when it retrains. The program retrains
  at start, on an interval or on every connection, as it is launched;
  if the add-on does only the first, a new area or a new blueprint
  sentence is heard after restarting it.
