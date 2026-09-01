//! Capture: an `arecord` child (same approach as robotd and pet-detect — no
//! in-process ALSA, no C dep) pumped into fixed 16 kHz mono frames.

use std::io::Read;
use std::process::{Child, ChildStdout, Command, Stdio};
use std::sync::mpsc::{SyncSender, TrySendError};

use super::decimate::Decimator;
use super::{FRAME_SAMPLES, HW_CHANNELS, HW_RATE};

const BYTES_PER_HW_FRAME: usize = HW_CHANNELS * 2;

pub fn spawn_arecord(device: &str) -> anyhow::Result<(Child, ChildStdout)> {
    spawn_command(
        "arecord",
        &[
            "-q",
            "-D",
            device,
            "-f",
            "S16_LE",
            "-c",
            &HW_CHANNELS.to_string(),
            "-r",
            &HW_RATE.to_string(),
            "-t",
            "raw",
        ],
    )
}

/// Development hook: any command writing raw S16_LE 2ch 48kHz to stdout.
pub fn spawn_custom(command: &[String]) -> anyhow::Result<(Child, ChildStdout)> {
    let (program, args) = command
        .split_first()
        .ok_or_else(|| anyhow::anyhow!("capture_command is empty"))?;
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    spawn_command(program, &args)
}

fn spawn_command(program: &str, args: &[&str]) -> anyhow::Result<(Child, ChildStdout)> {
    let mut child = Command::new(program)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let stdout = child
        .stdout
        .take()
        .expect("stdout was requested piped above");
    Ok((child, stdout))
}

/// Pump a raw S16_LE interleaved stereo stream into `FRAME_SAMPLES`-sized
/// 16 kHz mono frames on `tx`. Returns `Ok(())` when the receiver hangs up,
/// `Err` when the source ends or fails — the caller decides whether that
/// means restart (a died arecord) or shutdown.
pub fn pump(mut source: impl Read, tx: SyncSender<Vec<i16>>) -> anyhow::Result<()> {
    let mut decimator = Decimator::new();
    let mut dropped: u64 = 0;
    let mut read_buf = [0u8; 8192];
    let mut pending: Vec<u8> = Vec::new();
    let mut frame: Vec<i16> = Vec::with_capacity(FRAME_SAMPLES);

    loop {
        let n = source.read(&mut read_buf)?;
        if n == 0 {
            anyhow::bail!("capture stream ended");
        }
        pending.extend_from_slice(&read_buf[..n]);

        let usable = pending.len() - pending.len() % BYTES_PER_HW_FRAME;
        let right: Vec<i16> = pending[..usable]
            .chunks_exact(BYTES_PER_HW_FRAME)
            // The mic is on the right channel only (Mic3R): take it, never
            // average in the dead left channel.
            .map(|hw_frame| i16::from_le_bytes([hw_frame[2], hw_frame[3]]))
            .collect();
        pending.drain(..usable);

        for sample in decimator.process(&right) {
            frame.push(sample);
            if frame.len() == FRAME_SAMPLES {
                let full = std::mem::replace(&mut frame, Vec::with_capacity(FRAME_SAMPLES));
                // Never block: backpressure would fill the capture child's
                // pipe and wedge it (sox desyncs on CoreAudio overruns and
                // stays deaf until restarted). A full channel means the
                // consumer is busy playing or thinking — those frames were
                // going to be discarded anyway; drop them here and keep
                // the microphone healthy.
                match tx.try_send(full) {
                    Ok(()) => {}
                    Err(TrySendError::Full(_)) => {
                        dropped += 1;
                        if dropped % 64 == 1 {
                            tracing::debug!(dropped, "mic frames dropped (consumer busy)");
                        }
                    }
                    Err(TrySendError::Disconnected(_)) => return Ok(()),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::sync_channel;

    /// Interleave a right-channel signal with a loud junk left channel.
    fn stereo_bytes(right: &[i16]) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(right.len() * BYTES_PER_HW_FRAME);
        for &r in right {
            bytes.extend_from_slice(&20_000i16.to_le_bytes());
            bytes.extend_from_slice(&r.to_le_bytes());
        }
        bytes
    }

    #[test]
    fn extracts_right_channel_and_frames_output() {
        // 1.0 s of DC 1000 on the right channel → 16k samples → 31 full frames.
        let bytes = stereo_bytes(&vec![1000i16; HW_RATE as usize]);
        let (tx, rx) = sync_channel(64);
        let result = pump(std::io::Cursor::new(bytes), tx);
        assert!(result.is_err(), "EOF must be reported as an error");

        let frames: Vec<Vec<i16>> = rx.try_iter().collect();
        assert_eq!(frames.len(), 16_000 / FRAME_SAMPLES);
        assert!(frames.iter().all(|f| f.len() == FRAME_SAMPLES));
        // Settled samples must track the right channel, not the junk left.
        let settled = &frames[2][..];
        assert!(settled.iter().all(|&s| (s - 1000).abs() <= 2));
    }

    #[test]
    fn full_channel_drops_frames_instead_of_blocking() {
        // 4 s of audio into a 2-slot channel with no consumer: the pump
        // must run to EOF (never block) and the two slots must be full.
        let bytes = stereo_bytes(&vec![500i16; 4 * HW_RATE as usize]);
        let (tx, rx) = sync_channel(2);
        let result = pump(std::io::Cursor::new(bytes), tx);
        assert!(result.is_err(), "EOF is reported after draining the input");
        assert_eq!(rx.try_iter().count(), 2);
    }

    #[test]
    fn receiver_hangup_stops_the_pump_cleanly() {
        let bytes = stereo_bytes(&vec![0i16; HW_RATE as usize]);
        let (tx, rx) = sync_channel(1);
        drop(rx);
        assert!(pump(std::io::Cursor::new(bytes), tx).is_ok());
    }
}
