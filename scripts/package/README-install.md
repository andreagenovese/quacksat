# Installing quacksat on the duck

This package installs `quacksat`, the Microduck's voice satellite, on the
duck's board (Radxa Zero 3, aarch64 Linux, Debian 13) from your own
computer, over ssh. No checkout of the repository and no build are needed.
Italian copy: [README-install.it.md](README-install.it.md).

What you need:

- a duck provisioned by microduck: robotd running, the `robot` group its
  socket belongs to, the `audio` group, and `arecord`/`aplay` (alsa-utils);
- the internet on the duck once, during the install: the wake-word models
  are downloaded there (see section 3, "Wake word");
- ssh access to the duck with an account that has `sudo` (`microduck` on
  the board image; older images had `radxa`);
- on your computer: `bash`, `ssh`, `scp`, `tar`, and `shasum` or
  `sha256sum` (macOS and Linux have them all).

Optional, beside it: `quack-navd` (the navigation daemon,
<https://github.com/andreagenovese/quacknav>) so the duck can be sent
somewhere; without it the duck listens and answers, and says it cannot
move around the house.

## 1. Download and verify

From <https://github.com/andreagenovese/quacksat/releases>, the package
and its checksum. This package is `@VERSION@`; for a newer one set `V` to
its version (the tag without its `v`):

```sh
V=@VERSION@
gh release download "v$V" --repo andreagenovese/quacksat \
    --pattern "quacksat-$V-aarch64-linux.tar.gz*"
# or, without gh:
curl -LO "https://github.com/andreagenovese/quacksat/releases/download/v$V/quacksat-$V-aarch64-linux.tar.gz"
curl -LO "https://github.com/andreagenovese/quacksat/releases/download/v$V/quacksat-$V-aarch64-linux.tar.gz.sha256"

shasum -a 256 -c "quacksat-$V-aarch64-linux.tar.gz.sha256"   # or sha256sum -c
```

It must print `OK`. Then unpack:

```sh
tar xzf "quacksat-$V-aarch64-linux.tar.gz"
cd "quacksat-$V"
```

## 2. Install

```sh
./install-on-duck.sh --dry-run microduck@192.168.1.42   # optional: print what it would do
./install-on-duck.sh microduck@192.168.1.42
```

It copies the files to the duck, then with `sudo` there:

| on the duck | from this package |
|---|---|
| `/usr/local/bin/quacksat` | `bin/quacksat` |
| `/etc/systemd/system/quacksat.service` | `systemd/quacksat.service` |
| `/etc/sysusers.d/quacksat.conf` (user `quacksat`) | `systemd/sysusers.d/quacksat.conf` |
| `/etc/robot/quacksat.toml`, mode 0640 root:quacksat — **only when there is none** | `quacksat.example.toml` |
| `/var/lib/quacksat/models/` — files already there are kept | downloaded by `fetch-wake-models.sh` |

Then it enables and restarts the service, printing every command.
`SSH_OPTS="-p 2222"` passes options to ssh and scp;
`WAKE_MODELS="hey_daffy.onnx hey_jarvis_v0.1.onnx"` fetches more wake
models than the default `hey_daffy.onnx`.

The example config starts the satellite in **bring-up mode**
(`backend = "none"`): it listens, wakes on "hey Daffy" and chirps, and
talks to nobody. Choose a backend next.

## 3. The config: /etc/robot/quacksat.toml

Edit it on the duck (`sudo nano /etc/robot/quacksat.toml`), then
`sudo systemctl restart quacksat`. Every key left out takes its default;
an unknown key stops the satellite with a message naming it.

**It will hold API keys.** Keep it readable by root and the service
only, as the installer leaves it:

```sh
sudo chown root:quacksat /etc/robot/quacksat.toml
sudo chmod 640 /etc/robot/quacksat.toml
```

(Not 600 owned by root: the service runs as `quacksat` and could no
longer read it.)

### The backend

`backend` picks who the duck talks to:

| backend | what runs where | what to set |
|---|---|---|
| `"none"` | bring-up: wake word and chirp only | nothing |
| `"direct"` | the duck itself calls an LLM, a speech-to-text and a text-to-speech endpoint, OpenAI dialect (a cloud key or local servers) | `[direct.llm]`, `[direct.stt]`, `[direct.tts]` |
| `"agent"` | the duck streams to a bridge on a server, which runs STT → LLM → TTS | `[agent] url` (and the bridge, below) |
| `"wyoming"` | the duck is a Home Assistant Assist satellite | `[wyoming] bind`, then add it in Home Assistant |

`direct`, the simplest without a server:

```toml
backend = "direct"

[direct.llm]
base_url = "https://api.example.com/v1"   # any /chat/completions endpoint
api_key = "YOUR-LLM-KEY"
model = "your-model"
tool_calling = true                        # the robot tools need it

[direct.stt]
base_url = "https://api.example.com/v1"   # any /audio/transcriptions endpoint
api_key = "YOUR-STT-KEY"
language = "it"                            # also picks the duck's own phrases

[direct.tts]
base_url = "https://api.example.com/v1"   # any /audio/speech endpoint returning WAV
api_key = "YOUR-TTS-KEY"
voice = "your-voice"
```

`[direct.mcp]` (off by default) serves the robot's tools to MCP-capable
agents on port 8767; its `token` is mandatory.

`agent`:

```toml
backend = "agent"

[agent]
url = "ws://<bridge-host>:8765"
# token = "..."           # if the bridge asks for one
name = "duck-kitchen"     # unique per duck when several share a bridge
```

The **bridge** is not in this package: it runs on a server (Python 3.11+
or Docker), not on the duck. Take it from the same tag, either the
release's "Source code" archive or:

```sh
git clone --branch v@VERSION@ --depth 1 https://github.com/andreagenovese/quacksat
cd quacksat/bridge
cp config.example.toml config.toml   # then edit: LLM/STT/TTS urls + keys
docker compose up -d --build         # or the venv steps in bridge/README.md
```

`wyoming`: point Home Assistant's Wyoming integration at
`<duck>:10700` (Settings → Devices & Services → Add integration →
Wyoming Protocol). STT, intents and TTS are Home Assistant's.

### Wake word

```toml
[wake]
mode = "openwakeword"
models_dir = "/var/lib/quacksat/models"
model = "hey_daffy.onnx"
threshold = 0.5
```

The models are **not** in this package: openWakeWord publishes its
pre-trained models under CC BY-NC-SA 4.0 (non-commercial), so the
installer downloads them on the duck, from their own homes, each checked
against its sha256: the two shared feature models (`melspectrogram.onnx`,
`embedding_model.onnx`) from openWakeWord's release, and "hey Daffy",
quacksat's own wake word, from this repository at the package's commit.
Without the internet on the duck, fetch them anywhere with
`fetch-wake-models.sh <dir> hey_daffy.onnx` and copy them to
`/var/lib/quacksat/models/`. Another phrase: drop its `.onnx` there and
set `model`. If it misses you, lower `threshold` (0.4, 0.35); if it fires
on the TV, raise it.

### Audio

```toml
[audio]
playback_device = "plughw:aic3104"
capture_device = "plughw:aic3104,0"
```

The duck's own codec, as microduck names it. The microphone must be
free: robotd's `audio.pet_detect` must stay `false` in
`/etc/robot/robotd.toml`, the factory default; `audio.greet = false`
there is recommended too (the boot quack would race the first
utterance). The speaker is shared with robotd's own sounds: the duck's
chirps are robotd's, the voice is quacksat's, never both at once. There
is no echo cancellation: the duck does not listen while it speaks.

### Navigation and what the duck says on its own

```toml
[nav]
enabled = true
socket = "/run/quack-nav/nav.sock"   # quack-navd's socket

[announce]
enabled = true
language = ""      # "it" or "en"; empty: [direct.stt] language, else the bridge's, else English
journeys = true    # "Sono arrivata in cucina", "Mi sono fermata", ...
own_motion = true  # why the duck walks when nobody asked
```

With quack-navd answering on `socket`, its tools (where am I, remember
this place, go to the kitchen, map the house) join the satellite's. The
duck says how a journey it was sent on ended, between turns: with
`direct` in its own voice, with `agent` through the bridge. The
`wyoming` backend says nothing on its own yet.

## 4. Check

On the duck:

```sh
systemctl status quacksat
journalctl -u quacksat -f
```

A healthy start logs `quacksat starting`, `listening` on the capture
device, the robotd connection and, when quack-navd answers, its tools.
Without robotd or quack-navd it still starts, says so in the log and
retries. Say "hey Daffy": the duck chirps.

## 5. Upgrade

Download the newer package, verify it, unpack it, and run its
`./install-on-duck.sh` the same way. The binary, the unit and the account
are replaced; your `/etc/robot/quacksat.toml` and the models in
`/var/lib/quacksat/models` are kept. A new release may add config keys
(they take their defaults) — compare with the package's
`quacksat.example.toml`.

## 6. Uninstall

On the duck:

```sh
sudo systemctl disable --now quacksat
sudo rm /usr/local/bin/quacksat /etc/systemd/system/quacksat.service /etc/sysusers.d/quacksat.conf
sudo systemctl daemon-reload
# kept on purpose: /etc/robot/quacksat.toml (your keys) and
# /var/lib/quacksat/ (the models). Remove them, and `sudo userdel quacksat`,
# to leave nothing behind.
```

More: the project's README, <https://github.com/andreagenovese/quacksat>.
