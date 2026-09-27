//! Server-Sent Events parser (WHATWG HTML spec, "event stream interpretation").

use serde::Serialize;
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SseEvent {
    /// `message` when the server sent no `event:` field.
    pub event: String,
    pub data: String,
    /// Last event ID in effect when this event was dispatched.
    pub id: Option<String>,
    /// Reconnection time requested by the server, in milliseconds.
    pub retry: Option<u32>,
}

/// Longest line and longest event data kept in memory. Anything beyond is
/// dropped, so a server that never sends a line break cannot exhaust memory.
const MAX_EVENT_BYTES: usize = 16 << 20;

/// Incremental parser: feed arbitrary byte chunks, get complete events.
#[derive(Default)]
pub struct SseParser {
    pending: Vec<u8>,
    /// A chunk ended with CR; a following LF belongs to the same line break.
    skip_lf: bool,
    bom_checked: bool,
    data: String,
    has_data: bool,
    event: String,
    last_id: Option<String>,
    retry: Option<u32>,
}

impl SseParser {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn feed(&mut self, chunk: &[u8]) -> Vec<SseEvent> {
        let mut events = Vec::new();
        let mut bytes = chunk;
        if !self.bom_checked {
            self.pending.extend_from_slice(bytes);
            if self.pending.len() < 3 && b"\xEF\xBB\xBF".starts_with(&self.pending) {
                return events;
            }
            self.bom_checked = true;
            if self.pending.starts_with(b"\xEF\xBB\xBF") {
                self.pending.drain(..3);
            }
            let buffered = std::mem::take(&mut self.pending);
            return self.feed_lines(&buffered, events);
        }
        // An empty chunk says nothing about whether the CR is followed by LF.
        if self.skip_lf && !bytes.is_empty() {
            self.skip_lf = false;
            if bytes[0] == b'\n' {
                bytes = &bytes[1..];
            }
        }
        self.feed_lines(bytes, std::mem::take(&mut events))
    }

    fn feed_lines(&mut self, bytes: &[u8], mut events: Vec<SseEvent>) -> Vec<SseEvent> {
        let mut start = 0;
        let mut i = 0;
        while i < bytes.len() {
            match bytes[i] {
                b'\n' | b'\r' => {
                    let mut line = std::mem::take(&mut self.pending);
                    extend_capped(&mut line, &bytes[start..i]);
                    if bytes[i] == b'\r' {
                        if i + 1 < bytes.len() {
                            if bytes[i + 1] == b'\n' {
                                i += 1;
                            }
                        } else {
                            self.skip_lf = true;
                        }
                    }
                    if let Some(event) = self.process_line(&line) {
                        events.push(event);
                    }
                    i += 1;
                    start = i;
                }
                _ => i += 1,
            }
        }
        extend_capped(&mut self.pending, &bytes[start..]);
        events
    }

    fn process_line(&mut self, line: &[u8]) -> Option<SseEvent> {
        let line = String::from_utf8_lossy(line);
        if line.is_empty() {
            return self.dispatch();
        }
        if line.starts_with(':') {
            return None;
        }
        let (field, value) = match line.find(':') {
            Some(pos) => {
                let value = &line[pos + 1..];
                (&line[..pos], value.strip_prefix(' ').unwrap_or(value))
            }
            None => (line.as_ref(), ""),
        };
        match field {
            "event" => self.event = value.to_string(),
            "data" => {
                if self.has_data {
                    push_capped(&mut self.data, "\n");
                }
                push_capped(&mut self.data, value);
                self.has_data = true;
            }
            "id" if !value.contains('\0') => self.last_id = Some(value.to_string()),
            "retry" if !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit()) => {
                // Out-of-range values are ignored like any other invalid retry.
                if let Ok(ms) = value.parse() {
                    self.retry = Some(ms);
                }
            }
            _ => {}
        }
        None
    }

    fn dispatch(&mut self) -> Option<SseEvent> {
        let event_type = std::mem::take(&mut self.event);
        if !self.has_data {
            return None;
        }
        self.has_data = false;
        Some(SseEvent {
            event: if event_type.is_empty() { "message".into() } else { event_type },
            data: std::mem::take(&mut self.data),
            id: self.last_id.clone(),
            retry: self.retry,
        })
    }
}

fn extend_capped(buf: &mut Vec<u8>, bytes: &[u8]) {
    let room = MAX_EVENT_BYTES.saturating_sub(buf.len());
    buf.extend_from_slice(&bytes[..bytes.len().min(room)]);
}

fn push_capped(buf: &mut String, s: &str) {
    let mut end = s.len().min(MAX_EVENT_BYTES.saturating_sub(buf.len()));
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    buf.push_str(&s[..end]);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_all(chunks: &[&[u8]]) -> Vec<SseEvent> {
        let mut p = SseParser::new();
        chunks.iter().flat_map(|c| p.feed(c)).collect()
    }

    #[test]
    fn basic_events() {
        let ev = parse_all(&[b"data: hello\n\nevent: update\ndata: a\ndata: b\nid: 7\n\n"]);
        assert_eq!(ev.len(), 2);
        assert_eq!(ev[0].event, "message");
        assert_eq!(ev[0].data, "hello");
        assert_eq!(ev[1].event, "update");
        assert_eq!(ev[1].data, "a\nb");
        assert_eq!(ev[1].id.as_deref(), Some("7"));
    }

    #[test]
    fn split_chunks_and_line_endings() {
        let ev = parse_all(&[b"\xEF\xBB", b"\xBFdata: x", b"y\r", b"\n\r", b"\n", b"data:z\r\r"]);
        assert_eq!(ev.iter().map(|e| e.data.as_str()).collect::<Vec<_>>(), vec!["xy", "z"]);
    }

    #[test]
    fn comments_retry_and_empty_events() {
        let ev = parse_all(&[b": keepalive\n\nretry: 3000\nretry: x\nevent: ping\n\ndata\n\n"]);
        // "event: ping" without data dispatches nothing; bare "data" is an empty data line.
        assert_eq!(ev.len(), 1);
        assert_eq!(ev[0].data, "");
        assert_eq!(ev[0].retry, Some(3000));
    }

    #[test]
    fn empty_chunk_keeps_crlf_together() {
        // "\r" + "" + "\n" is one line break, so no event is dispatched yet.
        let ev = parse_all(&[b"data: a\r", b"", b"\n", b"data: b\n\n"]);
        assert_eq!(ev.len(), 1);
        assert_eq!(ev[0].data, "a\nb");
    }

    #[test]
    fn retry_overflow_keeps_previous_value() {
        let ev = parse_all(&[b"retry: 5\nretry: 99999999999999\ndata: x\n\n"]);
        assert_eq!(ev[0].retry, Some(5));
    }

    #[test]
    fn endless_line_and_data_are_bounded() {
        let mut p = SseParser::new();
        let chunk = vec![b'x'; 1 << 20];
        p.feed(b"data: ");
        for _ in 0..40 {
            assert!(p.feed(&chunk).is_empty());
        }
        assert!(p.pending.len() <= MAX_EVENT_BYTES);
        let ev = p.feed(b"\n\n");
        assert_eq!(ev[0].data.len(), MAX_EVENT_BYTES - "data: ".len());

        // Many empty data lines cannot grow the event without bound either.
        let mut p = SseParser::new();
        p.feed(b"data: ");
        p.feed(&vec![b'y'; MAX_EVENT_BYTES]);
        for _ in 0..1000 {
            p.feed(b"\ndata:");
        }
        assert!(p.data.len() <= MAX_EVENT_BYTES);
    }

    #[test]
    fn utf8_split_across_chunks() {
        let text = "data: héllo\n\n".as_bytes();
        let (a, b) = text.split_at(8);
        let ev = parse_all(&[a, b]);
        assert_eq!(ev[0].data, "héllo");
    }
}
