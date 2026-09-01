//! Full direct-backend conversation against a scripted OpenAI-dialect
//! HTTP server: wake → utterance → STT → LLM tool round (robot.get_frame,
//! robot-independent) → final reply → TTS → playback.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{Sender, channel, sync_channel};

use quacksat_core::audio::FRAME_SAMPLES;
use quacksat_core::config::Config;

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
    std::fs::write(&script, "#!/bin/sh\nexec cat > /dev/null\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(&script).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&script, perms).unwrap();
    script.to_str().unwrap().to_string()
}

/// Read one HTTP request (headers + content-length body); None on EOF.
fn read_request(stream: &mut TcpStream) -> Option<(String, Vec<u8>)> {
    let mut buf = Vec::new();
    let mut byte = [0u8; 1];
    while !buf.ends_with(b"\r\n\r\n") {
        match stream.read(&mut byte) {
            Ok(1) => buf.push(byte[0]),
            _ => return None,
        }
    }
    let head = String::from_utf8_lossy(&buf).to_string();
    let length: usize = head
        .lines()
        .find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.eq_ignore_ascii_case("content-length")
                .then(|| v.trim().parse().ok())?
        })
        .unwrap_or(0);
    let mut body = vec![0u8; length];
    stream.read_exact(&mut body).ok()?;
    let path = head.lines().next()?.split_whitespace().nth(1)?.to_string();
    Some((path, body))
}

fn respond(stream: &mut TcpStream, content_type: &str, body: &[u8]) {
    let head = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes()).unwrap();
    stream.write_all(body).unwrap();
    stream.flush().unwrap();
}

fn tts_wav() -> Vec<u8> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 22_050,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut cursor = std::io::Cursor::new(Vec::new());
    let mut writer = hound::WavWriter::new(&mut cursor, spec).unwrap();
    for i in 0..2_205 {
        writer.write_sample((i % 100) as i16).unwrap();
    }
    writer.finalize().unwrap();
    cursor.into_inner()
}

/// The scripted service: answers every endpoint, reports (path, body).
fn serve_ai(listener: TcpListener, seen: Sender<(String, String)>) {
    let mut completions = 0usize;
    for stream in listener.incoming() {
        let Ok(mut stream) = stream else { continue };
        while let Some((path, body)) = read_request(&mut stream) {
            let _ = seen.send((path.clone(), String::from_utf8_lossy(&body).to_string()));
            if path.ends_with("/audio/transcriptions") {
                respond(
                    &mut stream,
                    "application/json",
                    br#"{"text": "fai un verso"}"#,
                );
            } else if path.ends_with("/chat/completions") {
                completions += 1;
                let message = if completions == 1 {
                    r#"{"content": null, "tool_calls": [{"id": "t1", "type": "function",
                        "function": {"name": "robot_get_frame", "arguments": "{}"}}]}"#
                        .to_string()
                } else {
                    r#"{"content": "**Fatto**, quack! 🦆"}"#.to_string()
                };
                let body = format!(r#"{{"choices": [{{"message": {message}}}]}}"#);
                respond(&mut stream, "application/json", body.as_bytes());
            } else if path.ends_with("/audio/speech") {
                respond(&mut stream, "audio/wav", &tts_wav());
            } else {
                respond(&mut stream, "application/json", b"{}");
            }
        }
    }
}

#[test]
fn full_conversation_flow() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (seen_tx, seen_rx) = channel();
    std::thread::spawn(move || serve_ai(listener, seen_tx));

    let dir = tempfile::tempdir().unwrap();
    let aplay = fake_aplay(dir.path());
    let base = format!("http://127.0.0.1:{port}/v1");
    let config: Config = toml::from_str(&format!(
        "backend = \"direct\"\n\
         [audio]\nplayback_program = \"{aplay}\"\n\
         [wake]\nmode = \"energy\"\n\
         [direct.llm]\nbase_url = \"{base}\"\nmodel = \"test\"\n\
         [direct.stt]\nbase_url = \"{base}\"\n\
         [direct.tts]\nbase_url = \"{base}\"\n"
    ))
    .unwrap();

    let (frames_tx, frames_rx) = sync_channel::<Vec<i16>>(512);
    let satellite = std::thread::spawn(move || quacksat_backend_direct::run(&config, frames_rx));

    // Silence, a burst (wake + speech), then enough silence for the
    // 25-frame hangover to close the utterance.
    for _ in 0..3 {
        frames_tx.send(quiet_frame()).unwrap();
    }
    for _ in 0..12 {
        frames_tx.send(loud_frame()).unwrap();
    }
    for _ in 0..30 {
        frames_tx.send(quiet_frame()).unwrap();
    }

    let timeout = std::time::Duration::from_secs(20);
    let (stt_path, _) = seen_rx.recv_timeout(timeout).expect("stt request");
    assert!(stt_path.ends_with("/audio/transcriptions"));

    let (llm1_path, llm1_body) = seen_rx.recv_timeout(timeout).expect("first llm request");
    assert!(llm1_path.ends_with("/chat/completions"));
    assert!(llm1_body.contains("fai un verso"));
    assert!(
        llm1_body.contains("robot_get_frame"),
        "tools must be declared"
    );

    let (llm2_path, llm2_body) = seen_rx.recv_timeout(timeout).expect("second llm request");
    assert!(llm2_path.ends_with("/chat/completions"));
    assert!(
        llm2_body.contains("unsupported"),
        "tool result must be fed back"
    );
    assert!(llm2_body.contains("\"tool_call_id\":\"t1\""));

    let (tts_path, tts_body) = seen_rx.recv_timeout(timeout).expect("tts request");
    assert!(tts_path.ends_with("/audio/speech"));
    assert!(
        tts_body.contains("Fatto, quack!"),
        "reply must be sanitized: {tts_body}"
    );
    assert!(!tts_body.contains("**"));

    // Closing the mic ends the backend cleanly.
    drop(frames_tx);
    let err = satellite.join().unwrap().unwrap_err();
    assert!(err.to_string().contains("capture channel closed"));
}
