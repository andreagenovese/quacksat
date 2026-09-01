//! Minimal OpenAI-dialect HTTP clients (sync, ureq): chat completions,
//! audio transcriptions (multipart), audio speech. Mirrors the reference
//! bridge's clients, in Rust.

use serde_json::{Value, json};

use quacksat_core::config::{LlmService, SttService, TtsService};

fn agent(call_timeout_s: u64) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(call_timeout_s)))
        .build()
        .into()
}

pub fn transcribe(stt: &SttService, wav: Vec<u8>) -> anyhow::Result<String> {
    let boundary = format!("quacksat{:x}", std::process::id());
    let mut body = Vec::new();
    for (name, value) in [
        ("model", stt.model.as_str()),
        ("language", stt.language.as_str()),
        ("response_format", "json"),
    ] {
        if value.is_empty() {
            continue;
        }
        body.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"
            )
            .as_bytes(),
        );
    }
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; \
             filename=\"utterance.wav\"\r\nContent-Type: audio/wav\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(&wav);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());

    let url = format!(
        "{}/audio/transcriptions",
        stt.base_url.trim_end_matches('/')
    );
    let mut request = agent(120).post(&url).header(
        "Content-Type",
        &format!("multipart/form-data; boundary={boundary}"),
    );
    if !stt.api_key.is_empty() {
        request = request.header("Authorization", &format!("Bearer {}", stt.api_key));
    }
    let text = request.send(&body[..])?.body_mut().read_to_string()?;
    let parsed: Value = serde_json::from_str(&text)?;
    Ok(parsed
        .get("text")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string())
}

/// One chat-completions round. Returns (content, tool_calls).
pub fn complete(
    llm: &LlmService,
    messages: &[Value],
    tools: Option<&Value>,
) -> anyhow::Result<(Option<String>, Vec<Value>)> {
    let mut payload = json!({"model": llm.model, "messages": messages});
    if let Some(tools) = tools {
        payload["tools"] = tools.clone();
    }
    let url = format!("{}/chat/completions", llm.base_url.trim_end_matches('/'));
    let mut request = agent(180)
        .post(&url)
        .header("Content-Type", "application/json");
    if !llm.api_key.is_empty() {
        request = request.header("Authorization", &format!("Bearer {}", llm.api_key));
    }
    let text = request
        .send(payload.to_string().as_bytes())?
        .body_mut()
        .read_to_string()?;
    let parsed: Value = serde_json::from_str(&text)?;
    let message = parsed
        .pointer("/choices/0/message")
        .ok_or_else(|| anyhow::anyhow!("malformed completion: {}", &text[..text.len().min(200)]))?;
    let content = message
        .get("content")
        .and_then(Value::as_str)
        .map(str::to_string);
    let tool_calls = message
        .get("tool_calls")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    Ok((content, tool_calls))
}

/// Synthesize speech; returns (pcm, rate, channels) decoded from the WAV.
pub fn synthesize(tts: &TtsService, text: &str) -> anyhow::Result<(Vec<i16>, u32, u16)> {
    let mut payload = json!({"input": text, "response_format": "wav"});
    if !tts.model.is_empty() {
        payload["model"] = json!(tts.model);
    }
    if !tts.voice.is_empty() {
        payload["voice"] = json!(tts.voice);
    }
    let url = format!("{}/audio/speech", tts.base_url.trim_end_matches('/'));
    let mut request = agent(120)
        .post(&url)
        .header("Content-Type", "application/json");
    if !tts.api_key.is_empty() {
        request = request.header("Authorization", &format!("Bearer {}", tts.api_key));
    }
    let mut response = request.send(payload.to_string().as_bytes())?;
    let bytes = response
        .body_mut()
        .with_config()
        .limit(64 * 1024 * 1024)
        .read_to_vec()?;
    let mut reader = hound::WavReader::new(std::io::Cursor::new(bytes))?;
    let spec = reader.spec();
    anyhow::ensure!(
        spec.bits_per_sample == 16,
        "tts wav is {} bits, expected 16",
        spec.bits_per_sample
    );
    let pcm: Result<Vec<i16>, _> = reader.samples::<i16>().collect();
    Ok((pcm?, spec.sample_rate, spec.channels))
}

/// Wrap 16 kHz mono PCM into a WAV container for the STT upload.
pub fn pcm_to_wav(pcm: &[i16], rate: u32) -> Vec<u8> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut cursor = std::io::Cursor::new(Vec::new());
    {
        let mut writer = hound::WavWriter::new(&mut cursor, spec).expect("in-memory wav");
        for &s in pcm {
            writer.write_sample(s).expect("in-memory wav");
        }
        writer.finalize().expect("in-memory wav");
    }
    cursor.into_inner()
}

/// Project the satellite tool catalog into OpenAI `tools` format. Dots
/// become underscores (many providers reject dots in function names); the
/// executor maps them back.
pub fn openai_tools(catalog: &Value) -> Value {
    let tools: Vec<Value> = catalog
        .as_array()
        .map(|tools| {
            tools
                .iter()
                .map(|tool| {
                    json!({"type": "function", "function": {
                        "name": tool["name"].as_str().unwrap_or_default().replace('.', "_"),
                        "description": tool.get("description").cloned().unwrap_or(json!("")),
                        "parameters": tool.get("parameters").cloned()
                            .unwrap_or(json!({"type": "object", "properties": {}})),
                    }})
                })
                .collect()
        })
        .unwrap_or_default();
    Value::Array(tools)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tools_projection_replaces_dots() {
        let catalog = json!([{"name": "robot.get_frame", "description": "d", "parameters": {"type": "object"}}]);
        let tools = openai_tools(&catalog);
        assert_eq!(tools[0]["function"]["name"], "robot_get_frame");
        assert_eq!(tools[0]["type"], "function");
    }

    #[test]
    fn wav_round_trip() {
        let pcm: Vec<i16> = (0..1600).map(|i| (i % 100) as i16).collect();
        let wav = pcm_to_wav(&pcm, 16_000);
        let mut reader = hound::WavReader::new(std::io::Cursor::new(wav)).unwrap();
        assert_eq!(reader.spec().sample_rate, 16_000);
        let back: Vec<i16> = reader.samples::<i16>().map(Result::unwrap).collect();
        assert_eq!(back, pcm);
    }
}
