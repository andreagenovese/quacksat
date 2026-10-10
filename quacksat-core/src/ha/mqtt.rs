//! A minimal MQTT 3.1.1 client: what Home Assistant's discovery needs
//! and nothing more. QoS 0 only, a clean session, a last will, username
//! and password. Hand-rolled, like the direct backend's MCP server: the
//! crates that do this carry an async runtime and, for TLS, a C
//! dependency, for a protocol whose QoS 0 subset is a few framed
//! packets (ADR 0007 §1).
//!
//! One thread owns the socket: it reads with a short timeout and
//! publishes between reads, so nothing here is shared.

use std::io::{self, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

const CONNECT: u8 = 0x10;
const CONNACK: u8 = 0x20;
const PUBLISH: u8 = 0x30;
const SUBSCRIBE: u8 = 0x82;
const SUBACK: u8 = 0x90;
const PINGREQ: u8 = 0xC0;
const PINGRESP: u8 = 0xD0;

pub struct Will {
    pub topic: String,
    pub payload: String,
    pub retain: bool,
}

pub struct Options {
    pub host: String,
    pub port: u16,
    pub client_id: String,
    pub username: String,
    pub password: String,
    pub keep_alive_s: u16,
    pub will: Will,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Packet {
    Publish { topic: String, payload: Vec<u8>, retain: bool },
    SubAck,
    PingResp,
    /// Anything a QoS 0 client does not act on.
    Other(u8),
}

pub struct Client {
    stream: TcpStream,
    buffer: Vec<u8>,
    next_packet_id: u16,
}

/// How long a read waits before handing control back to the caller.
pub const READ_TIMEOUT: Duration = Duration::from_millis(200);

impl Client {
    /// Open the socket, send CONNECT and wait for the broker's CONNACK.
    pub fn connect(options: &Options) -> io::Result<Self> {
        let address = (options.host.as_str(), options.port)
            .to_socket_addrs()?
            .next()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "the broker's name resolves to nothing"))?;
        let stream = TcpStream::connect_timeout(&address, Duration::from_secs(5))?;
        stream.set_nodelay(true)?;
        stream.set_read_timeout(Some(Duration::from_secs(5)))?;
        stream.set_write_timeout(Some(Duration::from_secs(5)))?;
        let mut client = Client { stream, buffer: Vec::new(), next_packet_id: 1 };
        client.stream.write_all(&connect_packet(options))?;
        match client.read_packet_blocking()? {
            (CONNACK, body) if body.len() >= 2 => match body[1] {
                0 => {}
                4 => return Err(io::Error::new(io::ErrorKind::PermissionDenied, "the broker refused the username or password")),
                5 => return Err(io::Error::new(io::ErrorKind::PermissionDenied, "the broker says this client is not authorized")),
                code => return Err(io::Error::other(format!("the broker refused the connection (code {code})"))),
            },
            (kind, _) => return Err(io::Error::other(format!("expected CONNACK, got packet type {kind:#x}"))),
        }
        client.stream.set_read_timeout(Some(READ_TIMEOUT))?;
        Ok(client)
    }

    pub fn subscribe(&mut self, filter: &str) -> io::Result<()> {
        let id = self.packet_id();
        let mut body = id.to_be_bytes().to_vec();
        put_str(&mut body, filter);
        body.push(0); // requested QoS 0
        self.stream.write_all(&frame(SUBSCRIBE, &body))
    }

    pub fn publish(&mut self, topic: &str, payload: &[u8], retain: bool) -> io::Result<()> {
        self.stream.write_all(&publish_packet(topic, payload, retain))
    }

    pub fn ping(&mut self) -> io::Result<()> {
        self.stream.write_all(&[PINGREQ, 0])
    }

    /// The next packet, or `None` when nothing arrived within
    /// [`READ_TIMEOUT`]. A closed socket is an error.
    pub fn read(&mut self) -> io::Result<Option<Packet>> {
        loop {
            if let Some((kind, body, used)) = split_packet(&self.buffer)? {
                self.buffer.drain(..used);
                return Ok(Some(decode(kind, &body)?));
            }
            let mut chunk = [0u8; 4096];
            match self.stream.read(&mut chunk) {
                Ok(0) => return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "the broker closed the connection")),
                Ok(n) => self.buffer.extend_from_slice(&chunk[..n]),
                Err(e) if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) => return Ok(None),
                Err(e) => return Err(e),
            }
        }
    }

    fn read_packet_blocking(&mut self) -> io::Result<(u8, Vec<u8>)> {
        loop {
            if let Some((kind, body, used)) = split_packet(&self.buffer)? {
                self.buffer.drain(..used);
                return Ok((kind, body));
            }
            let mut chunk = [0u8; 512];
            let n = self.stream.read(&mut chunk)?;
            if n == 0 {
                return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "the broker closed the connection"));
            }
            self.buffer.extend_from_slice(&chunk[..n]);
        }
    }

    fn packet_id(&mut self) -> u16 {
        let id = self.next_packet_id;
        self.next_packet_id = self.next_packet_id.checked_add(1).unwrap_or(1);
        id
    }
}

fn connect_packet(options: &Options) -> Vec<u8> {
    let mut body = Vec::new();
    put_str(&mut body, "MQTT");
    body.push(4); // protocol level 3.1.1
    let mut flags = 0x02; // clean session
    flags |= 0x04; // will
    if options.will.retain {
        flags |= 0x20;
    }
    if !options.username.is_empty() {
        flags |= 0x80;
        if !options.password.is_empty() {
            flags |= 0x40;
        }
    }
    body.push(flags);
    body.extend_from_slice(&options.keep_alive_s.to_be_bytes());
    put_str(&mut body, &options.client_id);
    put_str(&mut body, &options.will.topic);
    put_str(&mut body, &options.will.payload);
    if !options.username.is_empty() {
        put_str(&mut body, &options.username);
        if !options.password.is_empty() {
            put_str(&mut body, &options.password);
        }
    }
    frame(CONNECT, &body)
}

fn publish_packet(topic: &str, payload: &[u8], retain: bool) -> Vec<u8> {
    let mut body = Vec::new();
    put_str(&mut body, topic);
    body.extend_from_slice(payload);
    frame(PUBLISH | u8::from(retain), &body)
}

fn decode(kind: u8, body: &[u8]) -> io::Result<Packet> {
    Ok(match kind & 0xF0 {
        PUBLISH => {
            let qos = (kind >> 1) & 0x03;
            let retain = kind & 0x01 == 1;
            let invalid = || io::Error::new(io::ErrorKind::InvalidData, "a malformed PUBLISH");
            let len = usize::from(u16::from_be_bytes([*body.first().ok_or_else(invalid)?, *body.get(1).ok_or_else(invalid)?]));
            let topic = body.get(2..2 + len).ok_or_else(invalid)?;
            let topic = String::from_utf8(topic.to_vec()).map_err(|_| invalid())?;
            // A QoS 1/2 message carries a packet id; this client asked for
            // QoS 0, so a broker that sends one anyway is read past.
            let start = 2 + len + if qos > 0 { 2 } else { 0 };
            let payload = body.get(start..).ok_or_else(invalid)?.to_vec();
            Packet::Publish { topic, payload, retain }
        }
        SUBACK => Packet::SubAck,
        PINGRESP => Packet::PingResp,
        other => Packet::Other(other),
    })
}

/// A whole packet at the front of `buffer`: (first byte, body, bytes
/// used). `None` while it is still arriving.
fn split_packet(buffer: &[u8]) -> io::Result<Option<(u8, Vec<u8>, usize)>> {
    if buffer.is_empty() {
        return Ok(None);
    }
    let mut length = 0usize;
    let mut shift = 0;
    let mut index = 1;
    loop {
        let Some(&byte) = buffer.get(index) else { return Ok(None) };
        length |= usize::from(byte & 0x7F) << shift;
        index += 1;
        if byte & 0x80 == 0 {
            break;
        }
        shift += 7;
        if shift > 21 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "a malformed remaining length"));
        }
    }
    if buffer.len() < index + length {
        return Ok(None);
    }
    Ok(Some((buffer[0], buffer[index..index + length].to_vec(), index + length)))
}

fn frame(first: u8, body: &[u8]) -> Vec<u8> {
    let mut packet = vec![first];
    let mut length = body.len();
    loop {
        let mut byte = (length % 128) as u8;
        length /= 128;
        if length > 0 {
            byte |= 0x80;
        }
        packet.push(byte);
        if length == 0 {
            break;
        }
    }
    packet.extend_from_slice(body);
    packet
}

fn put_str(out: &mut Vec<u8>, text: &str) {
    let bytes = text.as_bytes();
    out.extend_from_slice(&(bytes.len() as u16).to_be_bytes());
    out.extend_from_slice(bytes);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remaining_length_round_trips_across_the_varint_boundaries() {
        for size in [0, 1, 127, 128, 300, 16_383, 16_384, 70_000] {
            let body = vec![7u8; size];
            let packet = frame(PUBLISH, &body);
            let (kind, back, used) = split_packet(&packet).unwrap().unwrap();
            assert_eq!((kind, back.len(), used), (PUBLISH, size, packet.len()));
        }
    }

    #[test]
    fn a_packet_still_arriving_is_not_a_packet() {
        let packet = publish_packet("a/b", b"hello", false);
        assert!(split_packet(&packet[..packet.len() - 1]).unwrap().is_none());
        assert!(split_packet(&packet[..1]).unwrap().is_none());
    }

    #[test]
    fn a_publish_decodes_with_its_retain_flag() {
        let packet = publish_packet("quacksat/duck/cmd/forward", b"PRESS", true);
        let (kind, body, _) = split_packet(&packet).unwrap().unwrap();
        assert_eq!(
            decode(kind, &body).unwrap(),
            Packet::Publish { topic: "quacksat/duck/cmd/forward".into(), payload: b"PRESS".to_vec(), retain: true }
        );
    }

    #[test]
    fn a_qos1_publish_is_read_past_its_packet_id() {
        let mut body = Vec::new();
        put_str(&mut body, "t");
        body.extend_from_slice(&[0, 9]); // packet id
        body.extend_from_slice(b"x");
        let packet = frame(PUBLISH | 0x02, &body);
        let (kind, body, _) = split_packet(&packet).unwrap().unwrap();
        assert_eq!(decode(kind, &body).unwrap(), Packet::Publish { topic: "t".into(), payload: b"x".to_vec(), retain: false });
    }

    #[test]
    fn connect_carries_the_will_and_the_credentials() {
        let packet = connect_packet(&Options {
            host: String::new(),
            port: 1883,
            client_id: "quacksat-duck".into(),
            username: "duck".into(),
            password: "secret".into(),
            keep_alive_s: 30,
            will: Will { topic: "quacksat/duck/availability".into(), payload: "offline".into(), retain: true },
        });
        let (kind, body, _) = split_packet(&packet).unwrap().unwrap();
        assert_eq!(kind, CONNECT);
        // "MQTT", level 4, flags: user, password, will retain, will, clean.
        assert_eq!(&body[..8], &[0, 4, b'M', b'Q', b'T', b'T', 4, 0xE6]);
        let text = String::from_utf8_lossy(&body);
        for part in ["quacksat-duck", "quacksat/duck/availability", "offline", "duck", "secret"] {
            assert!(text.contains(part), "{part} missing");
        }
    }
}
