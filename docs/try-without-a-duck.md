# Trying quacksat without a duck

Every backend runs on a laptop today, with no Microduck in the room: a
real `robotd --fake` stands in for the body, the laptop's microphone and
speakers stand in for the duck's. This guide walks through the three
modes on a Mac (Linux works the same, with `arecord`/`aplay` instead of
sox). Italian copy: [try-without-a-duck.it.md](try-without-a-duck.it.md).
The why of each step, and what each answer proves, is in
[study/live-without-a-duck.md](study/live-without-a-duck.md).

What a laptop cannot give you: real legs (the fake robot answers, it
does not walk), a policy (skills are refused with "the policy is not
driving", which is the robot answering), and the duck's own audio path.

## 1. Common to every mode

**Tools:** Rust (1.89 or later), `sox` (`brew install sox`), git.

**The robot, faked.** Build robotd from Pollen's repo at the tag
quacksat is pinned to, and start it on a short socket path (macOS caps
a unix socket path at about a hundred characters):

```sh
git clone --branch daemon-v0.14.4 https://github.com/pollen-robotics/microduck.git
cd microduck && cargo build --release -p robotd
mkdir -p /tmp/qs && target/release/robotd --fake --socket /tmp/qs/robotd.sock
```

**The satellite.** In this checkout:

```sh
scripts/fetch-wake-models.sh models hey_daffy.onnx   # shared feature models + "hey Daffy"
cargo build -p quacksat
```

**The config every mode starts from**, in a file of your own
(`quacksat-dev.toml`, outside git if it will hold keys):

```toml
robotd_socket = "/tmp/qs/robotd.sock"

[audio]
capture_command = ["sox", "-q", "-d", "-t", "raw", "-r", "48000", "-c", "2", "-e", "signed-integer", "-b", "16", "-"]
playback_program = "scripts/aplay-shim-macos.sh"

[wake]
mode = "openwakeword"
models_dir = "models"
model = "hey_daffy.onnx"
threshold = 0.5

[announce]
language = "en"          # or "it"
```

Run with `target/debug/quacksat quacksat-dev.toml`; the log says what
answered (the robot's skills, the navigation, the backend). macOS asks
once for microphone access for the terminal.

Without the navigation daemon (`quack-navd`, the
[quacknav](https://github.com/andreagenovese/quacknav) repo) the duck
answers and moves but cannot be sent to a place, and says so; set
`[nav] enabled = false` to stop it asking.

## 2. wyoming — the duck in Home Assistant Assist

What you need: a Home Assistant with an Assist pipeline (any STT, TTS
and agent), reachable from the laptop.

```toml
backend = "wyoming"

[wyoming]
bind = "0.0.0.0:10700"
name = "quacksat"
```

In Home Assistant: Settings → Devices & services → Add integration →
**Wyoming Protocol**, host = the laptop's IP, port 10700. Pick the
pipeline on the new satellite device, then say "hey Daffy" and a
question. The log shows `wake`, the transcript and `tts played`.

**The body, too** (ADR 0007): add `[mqtt]` with the broker and a user
of its own, and the duck appears as a device with buttons, sliders and
sensors; the example automations make it walk by voice. All of it is in
[home-assistant/README.md](home-assistant/README.md); the section
"Trying it without the duck" there has the Home Assistant side.

```toml
[mqtt]
enabled = true
host = "homeassistant.local"
username = "quacksat"
password = "…"
```

## 3. agent — through the reference bridge

What you need: Python 3.11+. Start with the bridge's fake mode, which
needs no AI service at all:

```sh
cd bridge
python3 -m venv .venv && .venv/bin/pip install websockets
.venv/bin/python bridge.py --fake
```

```toml
backend = "agent"

[agent]
url = "ws://127.0.0.1:8765"
name = "quacksat"
```

Say "hey Daffy" and anything: the fake bridge answers with a canned
reply and a tone, and calls `robot.sound` once per turn — the robot's
"this robot has no voice" is the proof the tool path reached robotd.
For a real conversation, `bridge.py --config config.toml` with your
STT, LLM and TTS endpoints (`bridge/README.md`).

## 4. direct — the satellite calls the services itself

What you need: three OpenAI-dialect endpoints — a `/chat/completions`
LLM (a local ollama works), an `/audio/transcriptions` STT (e.g.
speaches / faster-whisper-server) and an `/audio/speech` TTS returning
WAV (e.g. openedai-speech with Piper).

```toml
backend = "direct"

[direct.llm]
base_url = "http://localhost:11434/v1"
model = "qwen3:8b"
tool_calling = true

[direct.stt]
base_url = "http://localhost:9000/v1"
language = "en"

[direct.tts]
base_url = "http://localhost:9100/v1"
voice = "en_US-amy-medium"
```

Say "hey Daffy, look to the left": the LLM calls `robot.head` and the
fake robot answers. To drive the tools with no voice services at all,
enable the duck's own MCP server and call it with curl:

```toml
[direct.mcp]
enabled = true
port = 8767
token = "change-me"
```

```sh
curl -s -X POST http://127.0.0.1:8767/mcp -H "Authorization: Bearer change-me" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/list"}'
```

## When something is silent

- **Nothing wakes:** check that no older `sox` holds the microphone
  (`pgrep -fl sox`): killing the satellite with `pkill` can orphan it.
  Raise the input gain, or lower `[wake] threshold`.
- **The duck hears its own answer:** lower the speaker volume; the
  laptop's speaker and microphone sit closer than the duck's.
- **"the policy is not driving", "this robot has no voice":** not
  errors. The fake robot answered, so the line works.
