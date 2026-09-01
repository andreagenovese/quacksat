//! Full satellite conversation against a scripted Home Assistant:
//! describe/info handshake, wake → detection + run-pipeline + mic chunks,
//! transcript stops streaming, TTS comes back and is answered with played.

use std::io::BufReader;
use std::os::unix::net::UnixStream;
use std::sync::mpsc::sync_channel;

use quacksat_backend_wyoming::protocol::{Event, read_event, write_event};
use quacksat_backend_wyoming::satellite::{Deps, run_connection};
use quacksat_core::audio::FRAME_SAMPLES;
use quacksat_core::config::Config;
use quacksat_core::playback::Player;
use quacksat_core::wake;
use serde_json::json;

fn loud_frame() -> Vec<i16> {
    (0..FRAME_SAMPLES)
        .map(|i| if i % 2 == 0 { 8000 } else { -8000 })
        .collect()
}

fn fake_aplay(dir: &std::path::Path) -> String {
    let script = dir.join("fake-aplay");
    // Exits immediately (see the agent test for why).
    std::fs::write(&script, "#!/bin/sh\nexit 0\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(&script).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&script, perms).unwrap();
    script.to_str().unwrap().to_string()
}

#[test]
fn full_conversation_flow() {
    let config: Config = toml::from_str("backend = \"wyoming\"").unwrap();
    let dir = tempfile::tempdir().unwrap();
    let aplay = fake_aplay(dir.path());

    let (ha_stream, satellite_stream) = UnixStream::pair().unwrap();
    let (frames_tx, frames_rx) = sync_channel::<Vec<i16>>(64);

    std::thread::scope(|scope| {
        let config_ref = &config;
        let handle = scope.spawn(move || {
            let mut detector = wake::from_config(&config_ref.wake).unwrap();
            let mut player = Player::with_program(&config_ref.audio.playback_device, &aplay);
            let mut control = None;
            run_connection(
                satellite_stream,
                &mut Deps {
                    config: config_ref,
                    frames: &frames_rx,
                    detector: detector.as_mut(),
                    player: &mut player,
                    control: &mut control,
                },
            )
        });

        let mut writer = ha_stream.try_clone().unwrap();
        let mut reader = BufReader::new(ha_stream.try_clone().unwrap());
        let mut next = || read_event(&mut reader).unwrap().expect("stream open");

        // 1. Handshake.
        write_event(
            &mut writer,
            &Event::new("describe", serde_json::Value::Null),
        )
        .unwrap();
        let info = next();
        assert_eq!(info.event_type, "info");
        assert_eq!(info.data["satellite"]["name"], "quacksat");
        assert_eq!(info.data["satellite"]["has_vad"], false);

        // 2. Arm the satellite, then speak: two quiet frames (the room,
        // buffered as pre-roll) followed by three loud ones.
        write_event(
            &mut writer,
            &Event::new("run-satellite", serde_json::Value::Null),
        )
        .unwrap();
        // Ping/pong as a barrier: events are handled in order, so the pong
        // proves run-satellite was processed before any frame arrives.
        write_event(&mut writer, &Event::new("ping", json!({"text": "armed"}))).unwrap();
        assert_eq!(next().event_type, "pong");
        for _ in 0..2 {
            frames_tx.send(vec![10i16; FRAME_SAMPLES]).unwrap();
        }
        for _ in 0..3 {
            frames_tx.send(loud_frame()).unwrap();
        }

        // 3. Wake → detection, run-pipeline, audio-start, chunks. The two
        // pre-roll frames plus the triggering frame are flushed in the wake
        // iteration itself; later frames may be skipped while the local
        // wake ack is still sounding, so only assert the guaranteed three.
        let detection = next();
        assert_eq!(detection.event_type, "detection");
        let pipeline = next();
        assert_eq!(pipeline.event_type, "run-pipeline");
        assert_eq!(pipeline.data["start_stage"], "asr");
        assert_eq!(pipeline.data["end_stage"], "tts");
        let start = next();
        assert_eq!(start.event_type, "audio-start");
        assert_eq!(start.data["rate"], 16_000);
        for _ in 0..3 {
            let chunk = next();
            assert_eq!(chunk.event_type, "audio-chunk");
            assert_eq!(chunk.payload.len(), FRAME_SAMPLES * 2);
        }

        // 4. Transcript ends the streaming; then TTS flows back.
        write_event(
            &mut writer,
            &Event::new("transcript", json!({"text": "accendi la luce"})),
        )
        .unwrap();
        write_event(
            &mut writer,
            &Event::new(
                "audio-start",
                json!({"rate": 22_050, "width": 2, "channels": 1}),
            ),
        )
        .unwrap();
        write_event(
            &mut writer,
            &Event::with_payload("audio-chunk", json!({"rate": 22_050}), vec![0u8; 4410]),
        )
        .unwrap();
        write_event(
            &mut writer,
            &Event::new("audio-stop", serde_json::Value::Null),
        )
        .unwrap();

        // 5. The satellite finishes playback and reports played. Chunks
        // already in flight from step 3 may interleave; skip them.
        let played = loop {
            let event = next();
            if event.event_type != "audio-chunk" {
                break event;
            }
        };
        assert_eq!(played.event_type, "played");

        // 6. Ping → pong.
        write_event(&mut writer, &Event::new("ping", json!({"text": "x"}))).unwrap();
        let pong = loop {
            let event = next();
            if event.event_type != "audio-chunk" {
                break event;
            }
        };
        assert_eq!(pong.event_type, "pong");
        assert_eq!(pong.data["text"], "x");

        // 7. HA disconnects; the connection loop returns cleanly.
        drop(writer);
        drop(reader);
        drop(ha_stream);
        handle.join().unwrap().unwrap();
    });
}

#[test]
fn frames_before_run_satellite_are_ignored() {
    let config: Config = toml::from_str("backend = \"wyoming\"").unwrap();
    let dir = tempfile::tempdir().unwrap();
    let aplay = fake_aplay(dir.path());

    let (ha_stream, satellite_stream) = UnixStream::pair().unwrap();
    let (frames_tx, frames_rx) = sync_channel::<Vec<i16>>(64);

    std::thread::scope(|scope| {
        let config_ref = &config;
        let handle = scope.spawn(move || {
            let mut detector = wake::from_config(&config_ref.wake).unwrap();
            let mut player = Player::with_program(&config_ref.audio.playback_device, &aplay);
            let mut control = None;
            run_connection(
                satellite_stream,
                &mut Deps {
                    config: config_ref,
                    frames: &frames_rx,
                    detector: detector.as_mut(),
                    player: &mut player,
                    control: &mut control,
                },
            )
        });

        // Loud frames while paused must produce no events at all.
        for _ in 0..5 {
            frames_tx.send(loud_frame()).unwrap();
        }
        let mut writer = ha_stream.try_clone().unwrap();
        let mut reader = BufReader::new(ha_stream.try_clone().unwrap());
        write_event(&mut writer, &Event::new("ping", json!({"text": "only"}))).unwrap();
        let event = read_event(&mut reader).unwrap().expect("stream open");
        assert_eq!(event.event_type, "pong", "paused satellite must not stream");

        drop(writer);
        drop(reader);
        drop(ha_stream);
        handle.join().unwrap().unwrap();
    });
}
