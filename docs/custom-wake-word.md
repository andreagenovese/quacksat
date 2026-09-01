# Training a custom wake word (e.g. "hey Daffy")

quacksat's wake detector runs any openWakeWord model: the phrase is just
an `.onnx` file dropped into the models directory. Pretrained models
(hey jarvis, alexa, …) come from `scripts/fetch-wake-models.sh`; a custom
phrase is trained once with openWakeWord's official pipeline — free on
Google Colab, no local GPU needed. (Paid sites offering the same training
wrap this exact notebook.)

## Procedure (~1 hour, mostly unattended)

1. Open the official training notebook on Colab (Google account needed):

   <https://colab.research.google.com/github/dscripka/openWakeWord/blob/main/notebooks/automatic_model_training.ipynb>

2. Runtime → Change runtime type → **GPU** (the free T4 is fine).

3. In the configuration cell set:
   - `target_word` / `target_phrase`: `"hey daffy"` (English spelling —
     the synthetic voices are English; that also matches how a non-native
     speaker pronounces it closely enough)
   - model name: `hey_daffy`
   Leave the defaults for sample counts and training steps on a first run.

4. Run all cells. The pipeline synthesizes thousands of pronunciations of
   the phrase with many TTS voices, augments them with noise and room
   reverb, mixes in negative data (speech that is *not* the phrase), and
   trains the classifier head. The mel + embedding stages are the shared
   pretrained models quacksat already has.

5. Download the resulting `hey_daffy.onnx` and place it in the models
   directory (`/var/lib/quacksat/models` on the robot, `models/` in dev).

6. Point the config at it:

   ```toml
   [wake]
   mode = "openwakeword"
   model = "hey_daffy.onnx"
   threshold = 0.5
   ```

## Tuning

- Synthetic-only models are a bit less robust than the curated pretrained
  ones. If it misses you, lower `threshold` (0.4, then 0.35); if it fires
  on TV or conversation, raise it (0.6+). Watch the scores with
  `RUST_LOG=quacksat_core=debug` — every detection logs its score.
- If your pronunciation differs from the synthetic voices (accent), a
  retrain with extra phrase spellings (e.g. `"hey daffy"`, `"ei daffy"`)
  in the target list usually helps.
- If a *similar* phrase also triggers (in practice: a first "hey Daffy"
  model fired on "hey Jarvis" too), retrain adding the confusable
  phrases to the custom **adversarial negatives** — that separation is
  not fixable with the threshold alone.
- Quick sanity check without hardware, on macOS:

  ```sh
  say -v Samantha -o /tmp/daffy.aiff "hey daffy"
  afconvert -f WAVE -d LEI16@16000 -c 1 /tmp/daffy.aiff /tmp/daffy.wav
  ```

  then feed `/tmp/daffy.wav` through the detector (the pattern in
  `quacksat-core/tests/wake_oww.rs` does exactly this for hey jarvis).
