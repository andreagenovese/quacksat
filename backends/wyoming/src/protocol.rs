//! Wyoming wire format (rhasspy/wyoming `event.py`): a JSON header line
//! `{"type", "version", "data_length"?, "payload_length"?}` followed by
//! `data_length` bytes of JSON (the event data) and `payload_length` bytes
//! of binary payload (PCM audio). Reads also accept `data` inline in the
//! header, which older peers emit.

use std::io::{BufRead, Write};

use serde_json::{Map, Value, json};

/// Protocol version stamped on outgoing headers; peers do not gate on it.
const VERSION: &str = "1.0.0";

#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    pub event_type: String,
    pub data: Value,
    pub payload: Vec<u8>,
}

impl Event {
    pub fn new(event_type: &str, data: Value) -> Self {
        Self {
            event_type: event_type.to_string(),
            data,
            payload: Vec::new(),
        }
    }

    pub fn with_payload(event_type: &str, data: Value, payload: Vec<u8>) -> Self {
        Self {
            event_type: event_type.to_string(),
            data,
            payload,
        }
    }
}

pub fn write_event(writer: &mut impl Write, event: &Event) -> std::io::Result<()> {
    let data_bytes = if event.data.is_null() {
        Vec::new()
    } else {
        serde_json::to_vec(&event.data)?
    };
    let mut header = json!({ "type": event.event_type, "version": VERSION });
    if !data_bytes.is_empty() {
        header["data_length"] = json!(data_bytes.len());
    }
    if !event.payload.is_empty() {
        header["payload_length"] = json!(event.payload.len());
    }
    let mut line = serde_json::to_vec(&header)?;
    line.push(b'\n');
    writer.write_all(&line)?;
    writer.write_all(&data_bytes)?;
    writer.write_all(&event.payload)?;
    writer.flush()
}

/// Read one event; `Ok(None)` on a clean end of stream.
pub fn read_event(reader: &mut impl BufRead) -> anyhow::Result<Option<Event>> {
    let mut line = String::new();
    if reader.read_line(&mut line)? == 0 {
        return Ok(None);
    }
    let header: Value = serde_json::from_str(&line)
        .map_err(|e| anyhow::anyhow!("bad wyoming header {:?}: {e}", line.trim()))?;
    let event_type = header
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("wyoming header without a type: {}", line.trim()))?
        .to_string();

    // Inline data first, then the separate data section merged over it.
    let mut data = match header.get("data") {
        Some(Value::Object(map)) => Value::Object(map.clone()),
        _ => Value::Object(Map::new()),
    };
    if let Some(data_length) = header.get("data_length").and_then(Value::as_u64) {
        let mut bytes = vec![0u8; data_length as usize];
        reader.read_exact(&mut bytes)?;
        if let (Value::Object(target), Ok(Value::Object(read))) =
            (&mut data, serde_json::from_slice::<Value>(&bytes))
        {
            target.extend(read);
        }
    }

    let mut payload = Vec::new();
    if let Some(payload_length) = header.get("payload_length").and_then(Value::as_u64) {
        payload = vec![0u8; payload_length as usize];
        reader.read_exact(&mut payload)?;
    }

    Ok(Some(Event {
        event_type,
        data,
        payload,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn round_trips_data_and_payload() {
        let event = Event::with_payload(
            "audio-chunk",
            json!({"rate": 16000, "width": 2, "channels": 1}),
            vec![1, 2, 3, 4],
        );
        let mut wire = Vec::new();
        write_event(&mut wire, &event).unwrap();
        // Header, then data bytes, then payload — no payload in the header line.
        let header_line = wire.split(|&b| b == b'\n').next().unwrap();
        let header: Value = serde_json::from_slice(header_line).unwrap();
        assert_eq!(header["type"], "audio-chunk");
        assert_eq!(header["payload_length"], 4);
        assert!(header.get("data").is_none());

        let got = read_event(&mut Cursor::new(wire)).unwrap().unwrap();
        assert_eq!(got, event);
    }

    #[test]
    fn reads_inline_header_data_from_older_peers() {
        let wire = b"{\"type\": \"describe\", \"data\": {\"a\": 1}}\n".to_vec();
        let got = read_event(&mut Cursor::new(wire)).unwrap().unwrap();
        assert_eq!(got.event_type, "describe");
        assert_eq!(got.data["a"], 1);
    }

    #[test]
    fn separate_data_section_wins_over_inline() {
        let data = b"{\"a\": 2}";
        let mut wire = format!(
            "{{\"type\": \"t\", \"data\": {{\"a\": 1, \"b\": 3}}, \"data_length\": {}}}\n",
            data.len()
        )
        .into_bytes();
        wire.extend_from_slice(data);
        let got = read_event(&mut Cursor::new(wire)).unwrap().unwrap();
        assert_eq!(got.data["a"], 2);
        assert_eq!(got.data["b"], 3);
    }

    #[test]
    fn end_of_stream_is_none() {
        assert!(read_event(&mut Cursor::new(Vec::new())).unwrap().is_none());
    }

    #[test]
    fn back_to_back_events_parse_in_order() {
        let mut wire = Vec::new();
        write_event(&mut wire, &Event::new("ping", Value::Null)).unwrap();
        write_event(
            &mut wire,
            &Event::with_payload("audio-chunk", json!({"rate": 16000}), vec![9; 32]),
        )
        .unwrap();
        let mut cursor = Cursor::new(wire);
        assert_eq!(read_event(&mut cursor).unwrap().unwrap().event_type, "ping");
        let second = read_event(&mut cursor).unwrap().unwrap();
        assert_eq!(second.payload.len(), 32);
        assert!(read_event(&mut cursor).unwrap().is_none());
    }
}
