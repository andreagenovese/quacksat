//! End-to-end wake-word tests against the real openWakeWord models.
//!
//! The models are not committed (fetch with scripts/fetch-wake-models.sh);
//! tests skip when they are absent. The positive test synthesizes
//! "hey jarvis" with macOS's `say` and asserts a detection; it skips off
//! macOS or when no English voice is installed.

use std::path::PathBuf;
use std::process::Command;

use quacksat_core::audio::FRAME_SAMPLES;
use quacksat_core::config::{WakeConfig, WakeMode};
use quacksat_core::wake::WakeDetector;
use quacksat_core::wake::oww::OpenWakeWord;

fn models_dir() -> Option<PathBuf> {
    let dir = std::env::var("QUACKSAT_MODELS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../models"));
    dir.join("melspectrogram.onnx").exists().then_some(dir)
}

fn detector(dir: &std::path::Path) -> OpenWakeWord {
    let config = WakeConfig {
        mode: WakeMode::Openwakeword,
        models_dir: dir.to_string_lossy().into_owned(),
        model: "hey_jarvis_v0.1.onnx".to_string(),
        threshold: 0.5,
    };
    OpenWakeWord::load(&config).expect("models must load")
}

fn feed_all(detector: &mut OpenWakeWord, samples: &[i16]) -> bool {
    let mut woke = false;
    for frame in samples.chunks(FRAME_SAMPLES) {
        woke |= detector.feed(frame);
    }
    woke
}

#[test]
fn silence_and_noise_do_not_wake() {
    let Some(dir) = models_dir() else {
        eprintln!("skipped: run scripts/fetch-wake-models.sh first");
        return;
    };
    let mut detector = detector(&dir);

    // 4 s of silence.
    assert!(!feed_all(&mut detector, &vec![0i16; 4 * 16_000]));

    // 4 s of deterministic pseudo-noise at speech-like amplitude.
    let mut state = 0x12345678u32;
    let noise: Vec<i16> = (0..4 * 16_000)
        .map(|_| {
            state = state.wrapping_mul(1664525).wrapping_add(1013904223);
            ((state >> 16) as i16) / 4
        })
        .collect();
    assert!(!feed_all(&mut detector, &noise));
}

/// Synthesize a phrase with macOS `say` into 16 kHz mono i16 samples.
fn synthesize(phrase: &str, voice: &str, dir: &std::path::Path) -> Option<Vec<i16>> {
    let aiff = dir.join("phrase.aiff");
    let wav = dir.join("phrase.wav");
    Command::new("say")
        .args(["-v", voice, "-o"])
        .arg(&aiff)
        .arg(phrase)
        .status()
        .ok()
        .filter(|s| s.success())?;
    Command::new("afconvert")
        .args(["-f", "WAVE", "-d", "LEI16@16000", "-c", "1"])
        .arg(&aiff)
        .arg(&wav)
        .status()
        .ok()
        .filter(|s| s.success())?;
    let mut reader = hound::WavReader::open(&wav).ok()?;
    Some(reader.samples::<i16>().map(|s| s.unwrap()).collect())
}

fn english_voice() -> Option<String> {
    let out = Command::new("say").args(["-v", "?"]).output().ok()?;
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .find(|l| l.contains("en_US"))
        .map(|l| {
            // Voice names may contain spaces; the locale column starts the
            // token containing "en_US".
            let idx = l.find("en_US").unwrap();
            l[..idx].trim().to_string()
        })
}

#[test]
fn synthesized_hey_jarvis_wakes_and_other_speech_does_not() {
    let Some(models) = models_dir() else {
        eprintln!("skipped: run scripts/fetch-wake-models.sh first");
        return;
    };
    let Some(voice) = english_voice() else {
        eprintln!("skipped: no macOS `say` en_US voice available");
        return;
    };
    let tmp = tempfile::tempdir().unwrap();

    let Some(hey_jarvis) = synthesize("hey jarvis", &voice, tmp.path()) else {
        eprintln!("skipped: say/afconvert unavailable");
        return;
    };
    let mut detector = detector(&models);
    // Lead-in silence primes the mel/embedding buffers; trailing silence
    // lets the last chunks flush through.
    let mut audio = vec![0i16; 2 * 16_000];
    audio.extend(&hey_jarvis);
    audio.extend(vec![0i16; 16_000]);
    assert!(
        feed_all(&mut detector, &audio),
        "voice {voice}: 'hey jarvis' must wake"
    );

    let other = synthesize("what time is it right now", &voice, tmp.path()).unwrap();
    let mut detector = self::detector(&models);
    let mut audio = vec![0i16; 2 * 16_000];
    audio.extend(&other);
    audio.extend(vec![0i16; 16_000]);
    assert!(
        !feed_all(&mut detector, &audio),
        "voice {voice}: unrelated speech must not wake"
    );
}
