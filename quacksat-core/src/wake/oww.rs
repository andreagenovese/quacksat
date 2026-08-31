//! openWakeWord detector on the tract ONNX runtime — pure Rust, no C deps.
//!
//! Faithful port of openWakeWord's streaming `AudioFeatures` pipeline
//! (github.com/dscripka/openWakeWord, Apache-2.0): audio is consumed in
//! 1280-sample (80 ms) chunks with 480 samples of lookback; each chunk
//! yields 8 mel frames (32 bands, normalized `x/10 + 2`); each chunk's
//! embedding is computed over the last 76 mel frames; the wake model
//! scores the last 16 embeddings (~1.3 s of audio).

use std::collections::VecDeque;
use std::path::Path;

use tract_onnx::prelude::*;

use crate::config::WakeConfig;

use super::WakeDetector;

/// One openWakeWord chunk: 80 ms at 16 kHz.
const CHUNK: usize = 1280;
/// Lookback fed to the melspectrogram alongside each chunk.
const LOOKBACK: usize = 480;
/// Mel frames per chunk: (1280 + 480) / 160 - 3.
const MEL_FRAMES_PER_CHUNK: usize = 8;
const MEL_BANDS: usize = 32;
/// Mel frames per embedding window.
const EMB_WINDOW: usize = 76;
const EMB_DIM: usize = 96;
/// Embeddings per wake-model input.
const WAKE_WINDOW: usize = 16;
/// Chunks to ignore after a detection (~2 s), so one utterance is one wake.
const REFRACTORY_CHUNKS: u32 = 25;

type Model = TypedRunnableModel<TypedModel>;

pub struct OpenWakeWord {
    mel_model: Model,
    emb_model: Model,
    wake_model: Model,
    name: String,
    threshold: f32,
    pending: Vec<i16>,
    lookback: [f32; LOOKBACK],
    mels: VecDeque<[f32; MEL_BANDS]>,
    feats: VecDeque<[f32; EMB_DIM]>,
    refractory: u32,
}

impl OpenWakeWord {
    pub fn load(config: &WakeConfig) -> anyhow::Result<Self> {
        let dir = Path::new(&config.models_dir);
        let wake_path = dir.join(&config.model);
        let mel_model = load_model(&dir.join("melspectrogram.onnx"), &[1, CHUNK + LOOKBACK])?;
        let emb_model = load_model(
            &dir.join("embedding_model.onnx"),
            &[1, EMB_WINDOW, MEL_BANDS, 1],
        )?;
        let wake_model = load_model(&wake_path, &[1, WAKE_WINDOW, EMB_DIM])?;
        let name = wake_path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "openwakeword".to_string());
        tracing::info!(model = %name, threshold = config.threshold, "wake word loaded");
        Ok(Self {
            mel_model,
            emb_model,
            wake_model,
            name,
            threshold: config.threshold,
            pending: Vec::with_capacity(2 * CHUNK),
            lookback: [0.0; LOOKBACK],
            mels: VecDeque::with_capacity(EMB_WINDOW + MEL_FRAMES_PER_CHUNK),
            feats: VecDeque::with_capacity(WAKE_WINDOW),
            refractory: 0,
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    fn process_chunk(&mut self, chunk: &[i16]) -> anyhow::Result<bool> {
        // openWakeWord feeds raw int16 amplitudes as f32 — no rescaling.
        let mut audio = Vec::with_capacity(LOOKBACK + CHUNK);
        audio.extend_from_slice(&self.lookback);
        audio.extend(chunk.iter().map(|&s| s as f32));
        self.lookback
            .copy_from_slice(&audio[audio.len() - LOOKBACK..]);

        let input = Tensor::from_shape(&[1, LOOKBACK + CHUNK], &audio)?;
        let mel_out = self.mel_model.run(tvec!(input.into()))?;
        let mel = mel_out[0].as_slice::<f32>()?;
        anyhow::ensure!(
            mel.len() == MEL_FRAMES_PER_CHUNK * MEL_BANDS,
            "unexpected melspectrogram output size {}",
            mel.len()
        );
        for frame in mel.chunks_exact(MEL_BANDS) {
            let mut row = [0.0; MEL_BANDS];
            for (dst, &src) in row.iter_mut().zip(frame) {
                *dst = src / 10.0 + 2.0;
            }
            if self.mels.len() == EMB_WINDOW + MEL_FRAMES_PER_CHUNK {
                self.mels.pop_front();
            }
            self.mels.push_back(row);
        }

        if self.mels.len() < EMB_WINDOW {
            return Ok(false);
        }
        let mut window = Vec::with_capacity(EMB_WINDOW * MEL_BANDS);
        for row in self.mels.iter().skip(self.mels.len() - EMB_WINDOW) {
            window.extend_from_slice(row);
        }
        let input = Tensor::from_shape(&[1, EMB_WINDOW, MEL_BANDS, 1], &window)?;
        let emb_out = self.emb_model.run(tvec!(input.into()))?;
        let emb = emb_out[0].as_slice::<f32>()?;
        anyhow::ensure!(
            emb.len() == EMB_DIM,
            "unexpected embedding size {}",
            emb.len()
        );
        if self.feats.len() == WAKE_WINDOW {
            self.feats.pop_front();
        }
        let mut feat = [0.0; EMB_DIM];
        feat.copy_from_slice(emb);
        self.feats.push_back(feat);

        if self.feats.len() < WAKE_WINDOW {
            return Ok(false);
        }
        let mut features = Vec::with_capacity(WAKE_WINDOW * EMB_DIM);
        for row in &self.feats {
            features.extend_from_slice(row);
        }
        let input = Tensor::from_shape(&[1, WAKE_WINDOW, EMB_DIM], &features)?;
        let wake_out = self.wake_model.run(tvec!(input.into()))?;
        let score = wake_out[0].as_slice::<f32>()?[0];

        // Tuning aid: anything the model found even vaguely interesting is
        // visible at debug, so a threshold can be picked from real voices.
        if score >= 0.1 {
            tracing::debug!(score = format!("{score:.2}").as_str(), model = %self.name, "wake score");
        }

        if self.refractory > 0 {
            self.refractory -= 1;
            return Ok(false);
        }
        if score >= self.threshold {
            self.refractory = REFRACTORY_CHUNKS;
            return Ok(true);
        }
        Ok(false)
    }
}

impl WakeDetector for OpenWakeWord {
    fn reset(&mut self) {
        self.pending.clear();
        self.lookback = [0.0; LOOKBACK];
        self.mels.clear();
        self.feats.clear();
        self.refractory = 0;
    }

    fn feed(&mut self, frame: &[i16]) -> bool {
        self.pending.extend_from_slice(frame);
        let mut woke = false;
        while self.pending.len() >= CHUNK {
            let chunk: Vec<i16> = self.pending.drain(..CHUNK).collect();
            match self.process_chunk(&chunk) {
                Ok(hit) => woke |= hit,
                Err(e) => {
                    // A model that fails once will fail forever; say so
                    // loudly but keep the pipeline alive.
                    tracing::error!(error = %e, "wake inference failed");
                }
            }
        }
        woke
    }
}

fn load_model(path: &Path, shape: &[usize]) -> anyhow::Result<Model> {
    // Decluttered but not optimized: tract 0.21's optimizer panics on the
    // melspectrogram graph (PushSplitDown); declutter alone is correct and
    // fast enough (~ms per chunk).
    let model = tract_onnx::onnx()
        .model_for_path(path)
        .map_err(|e| anyhow::anyhow!("loading {}: {e}", path.display()))?
        .with_input_fact(0, InferenceFact::dt_shape(f32::datum_type(), shape))?
        .into_typed()?
        .into_decluttered()?;
    model.into_runnable()
}
