//! Message boundaries on byte streams (TCP): raw chunks, lines, or
//! length-prefixed frames. Shared by the TCP client and the TCP servers.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// How received bytes on a TCP/UDP connection are split into messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum Framing {
    /// As they arrive (TCP chunks, one message per UDP datagram).
    #[default]
    Raw,
    /// One message per line (`\n`, a trailing `\r` is dropped).
    Line,
    /// A big-endian length prefix (`lengthBytes` long) before each message.
    LengthPrefixed,
}

/// Appended to each text message sent on a TCP/UDP connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum LineEnding {
    #[default]
    None,
    Lf,
    CrLf,
}

impl LineEnding {
    pub fn as_str(self) -> &'static str {
        match self {
            LineEnding::None => "",
            LineEnding::Lf => "\n",
            LineEnding::CrLf => "\r\n",
        }
    }
}

/// How payloads typed in the UI or stored in files are written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum PayloadEncoding {
    #[default]
    Text,
    /// Hex bytes, e.g. `48 65 6c 6c 6f`.
    Hex,
}

/// Longest message kept: a peer that never sends a line break (or announces a
/// huge length) cannot exhaust memory.
pub const MAX_MESSAGE: usize = 16 << 20;

/// Splits a byte stream into messages.
pub struct Deframer {
    framing: Framing,
    length_bytes: usize,
    buf: Vec<u8>,
}

impl Deframer {
    pub fn new(framing: Framing, length_bytes: u8) -> Self {
        let length_bytes = match length_bytes {
            1 | 2 | 4 => length_bytes as usize,
            _ => 2,
        };
        Self { framing, length_bytes, buf: Vec::new() }
    }

    /// Feed received bytes; returns the complete messages. A line longer than
    /// [`MAX_MESSAGE`] is cut into pieces; a length prefix above it is an error.
    pub fn push(&mut self, data: &[u8]) -> Result<Vec<Vec<u8>>, String> {
        match self.framing {
            Framing::Raw => Ok(if data.is_empty() { Vec::new() } else { vec![data.to_vec()] }),
            Framing::Line => {
                let mut out = Vec::new();
                let mut rest = data;
                while let Some(i) = rest.iter().position(|&b| b == b'\n') {
                    self.buf.extend_from_slice(&rest[..i]);
                    let mut line = std::mem::take(&mut self.buf);
                    if line.last() == Some(&b'\r') {
                        line.pop();
                    }
                    out.push(line);
                    rest = &rest[i + 1..];
                }
                self.buf.extend_from_slice(rest);
                while self.buf.len() >= MAX_MESSAGE {
                    out.push(self.buf.drain(..MAX_MESSAGE).collect());
                }
                Ok(out)
            }
            Framing::LengthPrefixed => {
                self.buf.extend_from_slice(data);
                let mut out = Vec::new();
                // Consumed bytes are dropped once at the end: draining per message
                // would move the rest of the buffer for every tiny frame.
                let mut at = 0;
                loop {
                    let rest = &self.buf[at..];
                    if rest.len() < self.length_bytes {
                        break;
                    }
                    let len = rest[..self.length_bytes].iter().fold(0usize, |acc, &b| (acc << 8) | usize::from(b));
                    if len > MAX_MESSAGE {
                        return Err(format!("Message length {len} is larger than the 16 MB limit"));
                    }
                    if rest.len() < self.length_bytes + len {
                        break;
                    }
                    out.push(rest[self.length_bytes..self.length_bytes + len].to_vec());
                    at += self.length_bytes + len;
                }
                self.buf.drain(..at);
                Ok(out)
            }
        }
    }

    /// Bytes of an unfinished message (reported when the connection closes).
    pub fn remainder(&mut self) -> Option<Vec<u8>> {
        (!self.buf.is_empty()).then(|| std::mem::take(&mut self.buf))
    }
}

/// Wrap an outgoing message: add the length prefix, or the line ending to text.
pub fn encode_message(
    framing: Framing,
    length_bytes: u8,
    line_ending: LineEnding,
    mut payload: Vec<u8>,
    is_text: bool,
) -> Result<Vec<u8>, String> {
    if is_text {
        payload.extend_from_slice(line_ending.as_str().as_bytes());
    }
    if framing != Framing::LengthPrefixed {
        return Ok(payload);
    }
    let n = match length_bytes {
        1 | 2 | 4 => length_bytes as usize,
        _ => 2,
    };
    let max = if n == 4 { u32::MAX as usize } else { (1usize << (8 * n)) - 1 };
    if payload.len() > max {
        return Err(format!("Message of {} bytes does not fit a {n}-byte length prefix", payload.len()));
    }
    let mut out = Vec::with_capacity(n + payload.len());
    out.extend_from_slice(&(payload.len() as u32).to_be_bytes()[4 - n..]);
    out.extend_from_slice(&payload);
    Ok(out)
}

/// Parse hex bytes (`48 65 6c`, `0x48,0x65`, `48:65`).
pub fn parse_hex(text: &str) -> Result<Vec<u8>, String> {
    let clean: String = text
        .replace("0x", "")
        .replace("0X", "")
        .chars()
        .filter(|c| !matches!(c, ' ' | ',' | ':' | '\n' | '\r' | '\t'))
        .collect();
    // Checked first: slicing below needs ASCII, and `from_str_radix` would accept a sign.
    if let Some(c) = clean.chars().find(|c| !c.is_ascii_hexdigit()) {
        return Err(format!("'{c}' is not a hex digit"));
    }
    if !clean.len().is_multiple_of(2) {
        return Err("Hex needs two digits per byte".into());
    }
    (0..clean.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&clean[i..i + 2], 16).map_err(|_| format!("'{}' is not hex", &clean[i..i + 2])))
        .collect()
}

/// Payload for display: UTF-8 text when it decodes, otherwise base64.
pub fn display_payload(bytes: &[u8]) -> (Option<String>, Option<String>) {
    use base64::Engine as _;
    match std::str::from_utf8(bytes) {
        Ok(text) => (Some(text.to_string()), None),
        Err(_) => (None, Some(base64::engine::general_purpose::STANDARD.encode(bytes))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_across_chunks() {
        let mut d = Deframer::new(Framing::Line, 2);
        assert_eq!(d.push(b"hel").unwrap(), Vec::<Vec<u8>>::new());
        assert_eq!(d.push(b"lo\r\nwor").unwrap(), vec![b"hello".to_vec()]);
        assert_eq!(d.push(b"ld\n\n").unwrap(), vec![b"world".to_vec(), Vec::new()]);
        assert_eq!(d.remainder(), None);
        d.push(b"tail").unwrap();
        assert_eq!(d.remainder(), Some(b"tail".to_vec()));
    }

    #[test]
    fn length_prefixed_round_trip_and_limits() {
        for n in [1u8, 2, 4] {
            let a = encode_message(Framing::LengthPrefixed, n, LineEnding::None, b"abc".to_vec(), true).unwrap();
            let b = encode_message(Framing::LengthPrefixed, n, LineEnding::None, vec![], false).unwrap();
            let mut d = Deframer::new(Framing::LengthPrefixed, n);
            let mut stream = [a, b].concat();
            let last = stream.split_off(2);
            let mut got = d.push(&stream).unwrap();
            got.extend(d.push(&last).unwrap());
            assert_eq!(got, vec![b"abc".to_vec(), Vec::new()], "{n} bytes");
        }
        assert!(encode_message(Framing::LengthPrefixed, 1, LineEnding::None, vec![0; 256], false).is_err());
        let mut d = Deframer::new(Framing::LengthPrefixed, 4);
        assert!(d.push(&u32::MAX.to_be_bytes()).is_err());
    }

    #[test]
    fn many_frames_in_one_chunk_keep_the_partial_tail() {
        let mut d = Deframer::new(Framing::LengthPrefixed, 1);
        let mut chunk = [1u8, b'x'].repeat(100_000);
        chunk.extend_from_slice(&[3, b'a', b'b']);
        let got = d.push(&chunk).unwrap();
        assert_eq!(got.len(), 100_000);
        assert!(got.iter().all(|m| m == b"x"));
        assert_eq!(d.push(b"c").unwrap(), vec![b"abc".to_vec()]);
        assert_eq!(d.remainder(), None);
    }

    #[test]
    fn endless_line_is_cut() {
        let mut d = Deframer::new(Framing::Line, 2);
        let out = d.push(&vec![b'x'; MAX_MESSAGE + 10]).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(d.remainder().unwrap().len(), 10);
    }

    #[test]
    fn text_gets_line_ending_and_hex_parses() {
        assert_eq!(encode_message(Framing::Raw, 2, LineEnding::CrLf, b"hi".to_vec(), true).unwrap(), b"hi\r\n");
        assert_eq!(encode_message(Framing::Raw, 2, LineEnding::CrLf, b"hi".to_vec(), false).unwrap(), b"hi");
        assert_eq!(parse_hex("48 65:6c,0x6c 6F").unwrap(), b"Hello");
        assert!(parse_hex("4").is_err());
        assert!(parse_hex("zz").is_err());
        // Non-ASCII input used to panic when slicing inside a character; a sign is not a digit.
        assert!(parse_hex("aéb").unwrap_err().contains('é'));
        assert!(parse_hex("+1").is_err());
        assert_eq!(display_payload(&[0xff]).1.as_deref(), Some("/w=="));
    }
}
