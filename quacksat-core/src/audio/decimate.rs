//! 48 kHz → 16 kHz decimator: windowed-sinc low-pass FIR, then keep every
//! third sample. Pure Rust on purpose — a resampling C dep would cost the
//! cross-build for a fixed 3:1 ratio.

use super::{HW_RATE, PIPELINE_RATE};

const FACTOR: usize = (HW_RATE / PIPELINE_RATE) as usize;
const TAPS: usize = 63;
/// Cutoff at 90% of the target Nyquist (7.2 kHz), normalized to the input
/// rate, leaving a transition band before aliasing folds in.
const CUTOFF: f32 = 0.9 * (PIPELINE_RATE as f32 / 2.0) / HW_RATE as f32;

pub struct Decimator {
    taps: [f32; TAPS],
    buf: Vec<f32>,
}

impl Decimator {
    pub fn new() -> Self {
        let mut taps = [0.0f32; TAPS];
        let mid = (TAPS - 1) as f32 / 2.0;
        let mut sum = 0.0;
        for (i, tap) in taps.iter_mut().enumerate() {
            let x = i as f32 - mid;
            let sinc = if x == 0.0 {
                2.0 * CUTOFF
            } else {
                (2.0 * std::f32::consts::PI * CUTOFF * x).sin() / (std::f32::consts::PI * x)
            };
            let hamming =
                0.54 - 0.46 * (2.0 * std::f32::consts::PI * i as f32 / (TAPS - 1) as f32).cos();
            *tap = sinc * hamming;
            sum += *tap;
        }
        // Unity DC gain, so amplitude is preserved through the filter.
        for tap in &mut taps {
            *tap /= sum;
        }
        Self {
            taps,
            buf: Vec::new(),
        }
    }

    /// Feed input samples, get whatever output samples are ready.
    pub fn process(&mut self, input: &[i16]) -> Vec<i16> {
        self.buf.extend(input.iter().map(|&s| s as f32));
        let mut out = Vec::with_capacity(self.buf.len() / FACTOR);
        let mut pos = 0;
        while pos + TAPS <= self.buf.len() {
            let acc: f32 = self
                .taps
                .iter()
                .zip(&self.buf[pos..pos + TAPS])
                .map(|(t, s)| t * s)
                .sum();
            out.push(acc.clamp(i16::MIN as f32, i16::MAX as f32) as i16);
            pos += FACTOR;
        }
        self.buf.drain(..pos);
        out
    }
}

impl Default for Decimator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rms(samples: &[i16]) -> f64 {
        let sum: f64 = samples.iter().map(|&s| (s as f64) * (s as f64)).sum();
        (sum / samples.len() as f64).sqrt()
    }

    fn sine(freq: f64, len: usize) -> Vec<i16> {
        (0..len)
            .map(|i| {
                (10_000.0 * (2.0 * std::f64::consts::PI * freq * i as f64 / HW_RATE as f64).sin())
                    as i16
            })
            .collect()
    }

    #[test]
    fn output_is_one_third_of_input() {
        let mut d = Decimator::new();
        let out = d.process(&vec![0i16; 48_000]);
        let expected = (48_000 - TAPS) / FACTOR + 1;
        assert!((out.len() as i64 - expected as i64).abs() <= 1);
    }

    #[test]
    fn preserves_dc_level() {
        let mut d = Decimator::new();
        let out = d.process(&vec![1000i16; 4800]);
        let settled = &out[TAPS..];
        assert!(settled.iter().all(|&s| (s - 1000).abs() <= 2));
    }

    #[test]
    fn passes_voice_band_rejects_aliasing_band() {
        let mut d = Decimator::new();
        let low = d.process(&sine(400.0, 48_000));
        let mut d = Decimator::new();
        let high = d.process(&sine(20_000.0, 48_000));
        let low_rms = rms(&low[100..]);
        let high_rms = rms(&high[100..]);
        assert!(low_rms > 6_000.0, "voice band attenuated: rms {low_rms}");
        assert!(high_rms < 500.0, "aliasing band passed: rms {high_rms}");
    }

    #[test]
    fn chunked_input_matches_single_shot() {
        let signal = sine(1000.0, 9600);
        let mut whole = Decimator::new();
        let expected = whole.process(&signal);
        let mut chunked = Decimator::new();
        let mut got = Vec::new();
        for chunk in signal.chunks(517) {
            got.extend(chunked.process(chunk));
        }
        assert_eq!(expected, got);
    }
}
