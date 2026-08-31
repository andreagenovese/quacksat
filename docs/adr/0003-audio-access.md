# ADR 0003: Audio access on the duck (mic and speaker)

- Status: accepted
- Date: 2026-08-31
- Inputs: `docs/study/microduck-speaker-path.md`,
  `docs/study/microduck-mic-path.md`,
  `docs/study/microduck-client-pattern.md`,
  `docs/study/microduck-ipc-and-packaging.md`

## Context

quacksat needs continuous microphone capture (wake word + STT streaming)
and speaker output (TTS) on hardware whose single audio codec
(TLV320AIC3104, card `aic3104`) is used by robotd under an explicit
exclusivity premise:

- **Playback**: `robotd/src/sound.rs` spawns one `aplay` child per sound on
  `plughw:aic3104` ("one playing child, a new sound kills it"). The device
  is free between sounds; `robot.theremin`/`robot.chorale` can hold it
  indefinitely. If `aplay` cannot open the device robotd logs a debug line
  and moves on — failure is soft in both directions.
- **Capture**: `pet-detect` (a thread inside robotd) holds
  `arecord -D plughw:aic3104,0` open continuously — but **only when
  `audio.pet_detect = true`, which is off by default**. On a stock robot
  the capture PCM is unused.
- There is **no dsnoop/dmix/asound.conf anywhere** in the microduck deploy:
  both directions are genuinely single-client.
- The `robot.sound` RPC cannot carry TTS: it takes a closed 7-tag enum that
  selects a random wav from a root-owned bank directory. There is no
  file/stream/volume API.
- The single mic is wired to **Mic3R → right PGA only**; a mono capture
  through `plug` averages a dead left channel into the signal (~half
  amplitude). Native rate is 48 kHz.
- There is **no echo cancellation anywhere** (and no hardware AEC, unlike
  Voice PE): the mic hears every sound the duck plays.

## Decision

### 1. Direct, exclusive device access, via ALSA CLI children

quacksat opens the codec directly, spawning `arecord`/`aplay` subprocesses
exactly like robotd and pet-detect do — no in-process ALSA binding, no C
dependency, same failure characteristics as the rest of the stack.

- **Capture**: `arecord -D plughw:aic3104,0 -f S16_LE -c 2 -r 48000 -t raw`,
  held open continuously. quacksat takes the **right channel** and
  resamples in-process to what the wake-word engine and backend need
  (typically 16 kHz mono). We do not copy pet-detect's `-c 1 -r 16000`,
  which halves the mic signal and discards quality STT wants.
- **Playback**: TTS goes to `plughw:aic3104` through a single `aplay`
  child, quacksat-side serialized (one child, a new utterance kills the
  old — same policy as sound.rs). On `-EBUSY` (robotd is quacking),
  retry with short backoff; if the state stream shows an active
  theremin/chorale, wait for it to end instead of spinning.

### 2. Coexistence with pet-detect by policy, not plumbing

quacksat requires `audio.pet_detect = false` — **the factory default**. This
is documented, and quacksat detects the conflict at startup (capture open
fails / robotd config) and says so in one clear log line rather than
entering a silent retry duel. Petting detection and a voice satellite are
mutually exclusive on this hardware today.

### 3. Duck cues via robotd, voice via quacksat

Expressive non-speech sounds (acknowledgement chirp on wake word, inquire,
alarm) are requested through the `robot.sound` RPC and stay robotd's job —
zero contention, serialized by robotd, consistent duck personality. TTS is
exclusively quacksat's aplay. While speaking, quacksat may animate the beak
with `robot.mouth` notifications.

### 4. Half-duplex audio as the echo answer (v0)

With no AEC, quacksat is **half-duplex**: while its TTS child is playing
(and, best effort, while a robotd sound it requested is playing), wake-word
and STT processing are suppressed; capture stays open but frames are
dropped. No barge-in in v0. Recommended robot config for satellite duty:
`audio.greet = false` (avoids the boot quack racing the first utterance).

### 5. Privileges and unit ordering

quacksat runs as its own user with `SupplementaryGroups=robot audio`
(`/dev/snd/*` is root:audio 0660; nothing in microduck manages the audio
group — we join the distro's). The unit orders
`After=aic3104-init.service robotd.service` with `Wants=robotd.service`
(After, not Requires/BindsTo: a down robotd must not keep the satellite
from starting, retrying, or serving as a plain speaker).

## Alternatives considered

- **Shared ALSA plumbing (dsnoop + dmix via an additive
  `/etc/asound.conf`)**: real concurrency, but requires repointing
  pet-detect too (capture device is *derived* from the playback param —
  needs an upstream `audio.capture_device` PR, ~10 lines), forces fixed
  slave params, adds latency to robotd's tightly tuned synth path, and
  buys little while pet_detect defaults off. **Deferred**: this is the
  documented evolution path if coexistence is ever needed.
- **Route TTS through robotd**: impossible today (closed enum, no stream
  API). An upstream "play arbitrary PCM" RPC is a bigger design
  conversation (deliberately out of scope for a safety daemon).
- **In-process ALSA (`alsa` crate)**: lower latency and finer control, but
  it is a C binding — costs the cross-build and diverges from house style.
  Can be revisited if subprocess latency proves inadequate for barge-in.
- **Software AEC (e.g. speexdsp/webrtc-audio-processing)**: the only road
  to full-duplex barge-in; all candidates are C deps and unproven on
  1 GB RAM next to inference. Deferred to a dedicated ADR when half-duplex
  proves too limiting.

## Consequences

- Zero changes to microduck are required for v0. Collisions with robotd
  sounds fail soft in both directions and are bounded (~hundreds of ms)
  except theremin/chorale, which quacksat must detect and yield to.
- A robot with petting detection enabled cannot run the satellite mic;
  the tradeoff is explicit and logged.
- Half-duplex means the duck cannot be interrupted mid-sentence; barge-in
  is consciously traded away until AEC exists.
- Mixer state is boot-set by `aic3104-init.service`; if TTS needs a
  different PGA/volume, quacksat must set it itself (`amixer -c aic3104`)
  and accept that it also changes robotd's loudness — avoided in v0.
- The speaker rolls off below ~300 Hz: pick/high-pass the TTS voice
  accordingly.
