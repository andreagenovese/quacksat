# Study: a live satellite on a Mac, with no duck in the room

What it takes to run the real thing — a real `robotd`, the real
`quacksat` binary, a real microphone and real speakers — months before
the hardware arrives, and what each answer proves. Written from the
session of 2026-09-23, which is how the 0.14.4 alignment and the
listening window were validated.

## robotd, for real, on macOS

`robotd` builds and runs on a Mac: the daemon says so itself ("no bus on
this platform; use `--fake`"). From a worktree of `pollen-robotics/microduck`
at the release tag:

```sh
cargo build --release -p robotd        # ~20 s
robotd --fake --socket /tmp/qsl/robotd.sock --params /tmp/qsl/robotd.toml
```

- **The socket path must be short.** macOS caps a unix socket path at
  about a hundred characters, and a scratch directory buried under
  `/private/tmp/...` blows it: the daemon refuses with "path must be
  shorter than SUN_LEN". `/tmp/<short>/robotd.sock` is the whole fix.
- `--sim host:port` runs the same daemon against the MuJoCo twin instead,
  when the body has to move for real.

**What that daemon cannot do on a laptop, and why that is useful.** It
has no ONNX runtime, so no policy drives and `robot.do` answers "the
policy is not driving — press Start on the pad". It has no voice bank, so
`robot.sound` answers "this robot has no voice". Both are *answers*: a
refusal that comes back over the wire proves the call reached the robot,
which makes them the cheapest witness that a lane is alive. `robot.head`,
`robot.look` and `robot.state` succeed normally.

**The skill table is config**, so a `[[policy.skill]]` entry in the
`--params` file shows up in `robot.skills` — which is how a satellite that
asks the robot what it can do (since daemon 0.14) can be tested against a
skill no release ever shipped. A stock `--fake` robotd lists six.

## The satellite's mic and speakers on a Mac

`[audio] capture_command` takes any command writing raw S16_LE 2ch 48 kHz
to stdout, and `playback_program` any aplay-style program:

```toml
capture_command = ["sox", "-q", "-d", "-t", "raw", "-r", "48000", "-c", "2", "-e", "signed-integer", "-b", "16", "-"]
playback_program = "scripts/aplay-shim-macos.sh"
```

- For a room that needs no human, a python generator alternating two
  seconds of silence with one of tone drives `wake.mode = "energy"`, which
  wants an onset after silence rather than constant noise.
- **`pkill` on the satellite orphans its capture child**, which keeps the
  microphone. The next run then reads a device somebody else holds and
  hears nothing at all — with no error anywhere. Kill the capture command
  too, and check with `pgrep` before blaming the audio.
- Input gain is a real variable: at 27/100 a quiet room reads RMS 0.001,
  at 75 it reads 0.018. Raising it does not only help — it lifts the
  duck's own voice over the VAD's floor too.

## Driving a turn without STT, LLM or TTS

The `direct` backend serves its own MCP endpoint, which is a complete tool
path that needs no voice services at all:

```sh
curl -s -X POST http://127.0.0.1:8767/mcp -H "Authorization: Bearer <token>" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/list"}'
```

`tools/list` shows exactly what the agent would see (including the skill
enum the robot itself reported), and `tools/call` executes against the
live daemon. That is enough to test the whole tool surface, a robotd
restart mid-session, and the lane coming back.

For the Home Assistant path, about sixty lines of python speaking Wyoming
(describe → info, run-satellite, ping/pong, then detection) make the duck
wake and close a turn. The satellite listens; Home Assistant connects.

## What only a human and a real voice can find

Everything above is automatable, and none of it found the two faults that
an evening of actually talking to the duck did:

- **The duck hears its own acknowledgement.** Every turn closed two
  seconds after the wake, before anyone had spoken, and the transcript
  came back as whisper's hallucination on silence rather than as an error.
  See ADR 0003 §4 — and note that the automated tests could not have
  caught it, because burst-fed frames do not model a speaker that keeps
  sounding after its program exits.
- **A wake word trained on English voices does not answer to an Italian
  mouth** (`docs/custom-wake-word.md`).

The rule that came out of it: a fault that only appears at the speed of a
real conversation needs a real conversation. Budget an evening of talking
to the duck before believing a voice path works.
