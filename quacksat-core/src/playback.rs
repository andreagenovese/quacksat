//! Speaker output (ADR 0003): one `aplay` child at a time, quacksat-side
//! serialized — a new utterance kills the old, mirroring robotd's sound.rs.
//! The codec PCM is exclusive: a spawn that dies immediately means robotd
//! holds it, so retry briefly instead of failing the utterance.

use std::io::Write;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use crate::audio::PIPELINE_RATE;

const OPEN_RETRIES: u32 = 5;
const RETRY_DELAY: Duration = Duration::from_millis(150);
/// How long after spawn we check whether aplay died on open. A real
/// playback outlives this unless the clip is shorter than the check.
const SETTLE: Duration = Duration::from_millis(120);

pub struct Player {
    device: String,
    program: String,
    child: Option<Child>,
}

impl Player {
    pub fn new(device: &str) -> Self {
        Self::with_program(device, "aplay")
    }

    /// Test hook: substitute the spawned binary.
    pub fn with_program(device: &str, program: &str) -> Self {
        Self {
            device: device.to_string(),
            program: program.to_string(),
            child: None,
        }
    }

    /// Play a wav file, killing whatever quacksat was playing before.
    pub fn play_wav(&mut self, path: &std::path::Path) -> anyhow::Result<()> {
        self.stop();
        self.spawn_with_retry(|program, device| {
            Command::new(program)
                .args(["-q", "-D", device])
                .arg(path)
                .stdin(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
        })
    }

    /// Play raw 16 kHz mono PCM (TTS output), killing the previous sound.
    /// The samples are handed to a writer thread; playback is asynchronous.
    pub fn play_pcm(&mut self, samples: Vec<i16>) -> anyhow::Result<()> {
        self.stop();
        self.spawn_with_retry(|program, device| {
            Command::new(program)
                .args([
                    "-q",
                    "-D",
                    device,
                    "-t",
                    "raw",
                    "-f",
                    "S16_LE",
                    "-c",
                    "1",
                    "-r",
                    &PIPELINE_RATE.to_string(),
                ])
                .stdin(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
        })?;
        if let Some(child) = &mut self.child
            && let Some(mut stdin) = child.stdin.take()
        {
            std::thread::spawn(move || {
                let bytes: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
                // A killed child closes the pipe mid-write; that is normal.
                let _ = stdin.write_all(&bytes);
            });
        }
        Ok(())
    }

    /// Start a streaming raw-PCM playback (TTS from a backend): chunks are
    /// written as they arrive with [`Self::write_stream`], and
    /// [`Self::end_stream`] closes the input and waits for the sound to
    /// finish — the synchronous completion point half-duplex needs.
    pub fn begin_stream(&mut self, rate: u32, channels: u16) -> anyhow::Result<()> {
        self.stop();
        self.spawn_with_retry(|program, device| {
            Command::new(program)
                .args([
                    "-q",
                    "-D",
                    device,
                    "-t",
                    "raw",
                    "-f",
                    "S16_LE",
                    "-c",
                    &channels.to_string(),
                    "-r",
                    &rate.to_string(),
                ])
                .stdin(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
        })
    }

    /// Write one chunk of the stream started by [`Self::begin_stream`].
    /// A killed or finished child makes this a no-op rather than an error.
    pub fn write_stream(&mut self, bytes: &[u8]) {
        if let Some(child) = &mut self.child
            && let Some(stdin) = &mut child.stdin
            && stdin.write_all(bytes).is_err()
        {
            child.stdin = None;
        }
    }

    /// Close the stream and block until playback drains.
    pub fn end_stream(&mut self) {
        if let Some(mut child) = self.child.take() {
            drop(child.stdin.take());
            let _ = child.wait();
        }
    }

    /// Kill and reap the current playback, if any.
    pub fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    pub fn is_playing(&mut self) -> bool {
        match &mut self.child {
            Some(child) => match child.try_wait() {
                Ok(None) => true,
                _ => {
                    self.child = None;
                    false
                }
            },
            None => false,
        }
    }

    fn spawn_with_retry(
        &mut self,
        spawn: impl Fn(&str, &str) -> std::io::Result<Child>,
    ) -> anyhow::Result<()> {
        for attempt in 0..OPEN_RETRIES {
            if attempt > 0 {
                std::thread::sleep(RETRY_DELAY * attempt);
            }
            let mut child = spawn(&self.program, &self.device).map_err(|e| {
                anyhow::anyhow!("spawning playback program `{}`: {e}", self.program)
            })?;
            std::thread::sleep(SETTLE);
            match child.try_wait()? {
                // Still running, or already finished cleanly (short clip).
                None => {
                    self.child = Some(child);
                    return Ok(());
                }
                Some(status) if status.success() => return Ok(()),
                // Died on open: the device is busy (robotd is quacking, or
                // a theremin holds the PCM). Retry with backoff.
                Some(status) => {
                    tracing::debug!(%status, attempt, "playback child died on open, retrying");
                }
            }
        }
        anyhow::bail!(
            "could not open playback device {} after {OPEN_RETRIES} attempts (busy?)",
            self.device
        )
    }
}

/// A short synthesized duck-ish double chirp (16 kHz mono), the built-in
/// wake acknowledgement for setups where robotd cannot play one (dev
/// machines, no sound bank). Two ~120 ms bursts with a falling glide and
/// a touch of harmonics.
pub fn wake_ack_pcm() -> Vec<i16> {
    let rate = crate::audio::PIPELINE_RATE as f32;
    let burst = (0.12 * rate) as usize;
    let gap = (0.06 * rate) as usize;
    let mut pcm = Vec::with_capacity(2 * burst + gap);
    for b in 0..2 {
        for i in 0..burst {
            let t = i as f32 / rate;
            let progress = i as f32 / burst as f32;
            let f = 520.0 - 140.0 * progress - b as f32 * 60.0;
            let envelope = (progress * std::f32::consts::PI).sin();
            let s = (2.0 * std::f32::consts::PI * f * t).sin()
                + 0.5 * (4.0 * std::f32::consts::PI * f * t).sin();
            pcm.push((s * envelope * 9000.0) as i16);
        }
        if b == 0 {
            pcm.extend(std::iter::repeat_n(0i16, gap));
        }
    }
    pcm
}

impl Drop for Player {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_successful_playback_is_ok() {
        // `true` exits 0 immediately: treated as a clip shorter than the
        // settle window, not as a busy device.
        let mut player = Player::with_program("ignored", "true");
        assert!(player.play_wav(std::path::Path::new("/dev/null")).is_ok());
    }

    #[test]
    fn busy_device_is_retried_then_reported() {
        // `false` exits non-zero immediately: looks like EBUSY every time.
        let mut player = Player::with_program("ignored", "false");
        let err = player
            .play_wav(std::path::Path::new("/dev/null"))
            .unwrap_err();
        assert!(err.to_string().contains("busy"), "unexpected error: {err}");
    }

    #[test]
    fn streaming_playback_completes_on_end_stream() {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("fake-aplay");
        std::fs::write(&script, "#!/bin/sh\nexec cat > /dev/null\n").unwrap();
        let mut perms = std::fs::metadata(&script).unwrap().permissions();
        use std::os::unix::fs::PermissionsExt;
        perms.set_mode(0o755);
        std::fs::set_permissions(&script, perms).unwrap();

        let mut player = Player::with_program("ignored", script.to_str().unwrap());
        player.begin_stream(22_050, 1).unwrap();
        assert!(player.is_playing());
        player.write_stream(&[0u8; 4096]);
        player.end_stream();
        assert!(!player.is_playing());
        // Writing after the stream ended is a no-op, not a panic.
        player.write_stream(&[0u8; 16]);
    }

    #[test]
    fn long_playback_is_tracked_and_killed_by_stop() {
        // A stand-in for aplay that ignores its flags, drains stdin, and
        // then lingers the way a real playback outlives its input.
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("fake-aplay");
        std::fs::write(&script, "#!/bin/sh\ncat > /dev/null\nexec sleep 30\n").unwrap();
        let mut perms = std::fs::metadata(&script).unwrap().permissions();
        use std::os::unix::fs::PermissionsExt;
        perms.set_mode(0o755);
        std::fs::set_permissions(&script, perms).unwrap();

        let mut player = Player::with_program("ignored", script.to_str().unwrap());
        player.play_pcm(vec![0i16; 32_000]).unwrap();
        assert!(player.is_playing());
        player.stop();
        assert!(!player.is_playing());
        // stop() on an already-dead child must be a no-op.
        player.stop();
    }
}
