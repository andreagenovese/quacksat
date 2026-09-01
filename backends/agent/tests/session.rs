//! Full agent-protocol conversation against a scripted bridge over a
//! real localhost WebSocket: session.start handshake, wake → audio
//! streaming → utterance.end, tool calls (unsupported / unknown /
//! no-robot), TTS round with follow-up listen.start, clean close.

use std::net::TcpListener;
use std::sync::mpsc::sync_channel;

use quacksat_backend_agent::session::{Deps, run_session};
use quacksat_core::audio::FRAME_SAMPLES;
use quacksat_core::config::Config;
use quacksat_core::playback::Player;
use quacksat_core::wake;
use serde_json::{Value, json};
use tungstenite::Message;

fn loud_frame() -> Vec<i16> {
    (0..FRAME_SAMPLES)
        .map(|i| if i % 2 == 0 { 8000 } else { -8000 })
        .collect()
}

fn quiet_frame() -> Vec<i16> {
    vec![10i16; FRAME_SAMPLES]
}

fn fake_aplay(dir: &std::path::Path) -> String {
    let script = dir.join("fake-aplay");
    // Exits immediately: tests feed frames in bursts, and a lingering
    // ack player would gate them all away in one drain.
    std::fs::write(&script, "#!/bin/sh\nexit 0\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(&script).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&script, perms).unwrap();
    script.to_str().unwrap().to_string()
}

#[test]
fn full_conversation_flow() {
    let config: Config = toml::from_str("backend = \"agent\"").unwrap();
    let dir = tempfile::tempdir().unwrap();
    let aplay = fake_aplay(dir.path());

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (frames_tx, frames_rx) = sync_channel::<Vec<i16>>(256);

    std::thread::scope(|scope| {
        let config_ref = &config;
        // The satellite side under test.
        let satellite = scope.spawn(move || {
            let (ws, _) = tungstenite::connect(format!("ws://127.0.0.1:{port}")).expect("connect");
            let mut detector = wake::from_config(&config_ref.wake).unwrap();
            let mut player = Player::with_program("ignored", &aplay);
            let mut control = None;
            run_session(
                ws,
                &mut Deps {
                    config: config_ref,
                    frames: &frames_rx,
                    detector: detector.as_mut(),
                    player: &mut player,
                    control: &mut control,
                },
            )
        });

        // The scripted bridge.
        let (stream, _) = listener.accept().unwrap();
        let mut bridge = tungstenite::accept(stream).unwrap();
        let next_json = |bridge: &mut tungstenite::WebSocket<std::net::TcpStream>| -> Value {
            loop {
                match bridge.read().unwrap() {
                    Message::Text(text) => return serde_json::from_str(&text).unwrap(),
                    Message::Binary(_) => continue,
                    other => panic!("unexpected message: {other:?}"),
                }
            }
        };
        let send = |bridge: &mut tungstenite::WebSocket<std::net::TcpStream>, event: Value| {
            bridge.send(Message::text(event.to_string())).unwrap();
        };

        // 1. Handshake: session.start with the tool catalog.
        let start = next_json(&mut bridge);
        assert_eq!(start["type"], "session.start");
        assert_eq!(start["audio"]["rate"], 16_000);
        let tools: Vec<&str> = start["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert!(tools.contains(&"robot.move"));
        assert!(tools.contains(&"robot.get_frame"));
        send(&mut bridge, json!({"type": "session.ready", "version": 1}));

        // 2. Tool calls while idle: unsupported, unknown, and no-robot.
        send(
            &mut bridge,
            json!({"type": "tool.call", "id": "t1", "name": "robot.get_frame", "args": {}}),
        );
        let result = next_json(&mut bridge);
        assert_eq!(result["type"], "tool.result");
        assert_eq!(result["id"], "t1");
        assert_eq!(result["ok"], false);
        assert_eq!(result["error"], "unsupported");

        send(
            &mut bridge,
            json!({"type": "tool.call", "id": "t2", "name": "robot.fly", "args": {}}),
        );
        let result = next_json(&mut bridge);
        assert_eq!(result["ok"], false);
        assert_eq!(result["error"], "unknown tool `robot.fly`");

        send(
            &mut bridge,
            json!({"type": "tool.call", "id": "t3", "name": "robot.state", "args": {}}),
        );
        let result = next_json(&mut bridge);
        assert_eq!(result["ok"], false);
        assert_eq!(result["error"], "robot unreachable");

        // 3. Wake: quiet pre-roll then speech; expect wake, binary audio,
        // and utterance.end once silence returns.
        for _ in 0..2 {
            frames_tx.send(quiet_frame()).unwrap();
        }
        for _ in 0..2 {
            frames_tx.send(loud_frame()).unwrap();
        }
        let wake = next_json(&mut bridge);
        assert_eq!(wake["type"], "wake");
        // Let the local wake ack finish before feeding the utterance:
        // burst-fed frames arrive faster than real time, and the
        // half-duplex gate would otherwise drain them all away while
        // the ack child lingers.
        std::thread::sleep(std::time::Duration::from_millis(500));
        for _ in 0..9 {
            frames_tx.send(loud_frame()).unwrap();
        }
        // Silence long enough for the VAD hangover (25 frames) to
        // close the utterance.
        for _ in 0..30 {
            frames_tx.send(quiet_frame()).unwrap();
        }

        let mut audio_frames = 0;
        loop {
            match bridge.read().unwrap() {
                Message::Binary(bytes) => {
                    assert_eq!(bytes.len(), FRAME_SAMPLES * 2);
                    audio_frames += 1;
                }
                Message::Text(text) => {
                    let event: Value = serde_json::from_str(&text).unwrap();
                    assert_eq!(event["type"], "utterance.end");
                    break;
                }
                other => panic!("unexpected message: {other:?}"),
            }
        }
        assert!(audio_frames >= 12, "pre-roll + burst must be streamed");

        // 4. TTS round, then a follow-up turn without a wake word.
        send(
            &mut bridge,
            json!({"type": "tts.start", "rate": 22_050, "channels": 1}),
        );
        bridge.send(Message::binary(vec![0u8; 4410])).unwrap();
        send(&mut bridge, json!({"type": "tts.end"}));
        send(&mut bridge, json!({"type": "listen.start"}));
        send(&mut bridge, json!({"type": "ping", "text": "alive"}));
        let pong = next_json(&mut bridge);
        assert_eq!(pong["type"], "pong");
        assert_eq!(pong["text"], "alive");

        // The follow-up turn streams without any wake event.
        for _ in 0..6 {
            frames_tx.send(loud_frame()).unwrap();
        }
        let mut followup_frames = 0;
        while followup_frames < 6 {
            match bridge.read().unwrap() {
                Message::Binary(_) => followup_frames += 1,
                Message::Text(text) => {
                    let event: Value = serde_json::from_str(&text).unwrap();
                    panic!("no events expected during follow-up, got {event}");
                }
                other => panic!("unexpected message: {other:?}"),
            }
        }

        // 5. Bridge closes; the session ends cleanly.
        bridge.close(None).unwrap();
        let _ = bridge.read(); // flush the close handshake
        satellite.join().unwrap().unwrap();
    });
}

/// A bridge that never answers: after `utterance.end` the satellite must
/// give up at `[thinking] timeout_s` and play the sad ack locally (the
/// counting fake aplay sees wake ack + sad ack = two runs).
#[test]
fn reply_timeout_ends_with_sad_ack() {
    let config: Config =
        toml::from_str("backend = \"agent\"\n[thinking]\ndelay_s = 0.05\ntimeout_s = 0.2\n")
            .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let plays = dir.path().join("plays");
    let script = dir.path().join("counting-aplay");
    std::fs::write(
        &script,
        format!("#!/bin/sh\necho run >> {}\nexit 0\n", plays.display()),
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(&script).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&script, perms).unwrap();
    let aplay = script.to_str().unwrap().to_string();

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (frames_tx, frames_rx) = sync_channel::<Vec<i16>>(256);

    std::thread::scope(|scope| {
        let config_ref = &config;
        let satellite = scope.spawn(move || {
            let (ws, _) = tungstenite::connect(format!("ws://127.0.0.1:{port}")).expect("connect");
            let mut detector = wake::from_config(&config_ref.wake).unwrap();
            let mut player = Player::with_program("ignored", &aplay);
            let mut control = None;
            run_session(
                ws,
                &mut Deps {
                    config: config_ref,
                    frames: &frames_rx,
                    detector: detector.as_mut(),
                    player: &mut player,
                    control: &mut control,
                },
            )
        });

        let (stream, _) = listener.accept().unwrap();
        let mut bridge = tungstenite::accept(stream).unwrap();
        let start: Value = match bridge.read().unwrap() {
            Message::Text(text) => serde_json::from_str(&text).unwrap(),
            other => panic!("unexpected message: {other:?}"),
        };
        assert_eq!(start["type"], "session.start");

        // Wake on a short burst.
        for _ in 0..2 {
            frames_tx.send(quiet_frame()).unwrap();
        }
        for _ in 0..2 {
            frames_tx.send(loud_frame()).unwrap();
        }
        loop {
            match bridge.read().unwrap() {
                Message::Binary(_) => continue,
                Message::Text(text) => {
                    let event: Value = serde_json::from_str(&text).unwrap();
                    assert_eq!(event["type"], "wake");
                    break;
                }
                other => panic!("unexpected message: {other:?}"),
            }
        }
        // Let the local wake ack finish before feeding the utterance:
        // burst-fed frames arrive faster than real time, and the
        // half-duplex gate would otherwise drain them all away while
        // the ack child lingers.
        std::thread::sleep(std::time::Duration::from_millis(500));
        for _ in 0..8 {
            frames_tx.send(loud_frame()).unwrap();
        }
        for _ in 0..30 {
            frames_tx.send(quiet_frame()).unwrap();
        }
        loop {
            match bridge.read().unwrap() {
                Message::Binary(_) => continue,
                Message::Text(text) => {
                    let event: Value = serde_json::from_str(&text).unwrap();
                    assert_eq!(event["type"], "utterance.end");
                    break;
                }
                other => panic!("unexpected message: {other:?}"),
            }
        }

        // The bridge stays silent past timeout_s; the satellite keeps
        // polling on its own and must give up with the sad ack.
        std::thread::sleep(std::time::Duration::from_millis(800));
        let log = std::fs::read_to_string(&plays).unwrap_or_default();
        assert_eq!(
            log.lines().count(),
            2,
            "expected wake ack + sad ack, got: {log:?}"
        );

        bridge.close(None).unwrap();
        let _ = bridge.read(); // flush the close handshake
        satellite.join().unwrap().unwrap();
    });
}
