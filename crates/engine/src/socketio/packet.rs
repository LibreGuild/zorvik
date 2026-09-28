//! Socket.IO 3/4 packets inside Engine.IO 4 packets, shared by the client and Zorvik's
//! Socket.IO server.
//!
//! Engine.IO: one character for the type (`0` open, `1` close, `2` ping, `3` pong,
//! `4` message, `5` upgrade, `6` noop), then the data. Long-polling joins packets with the
//! record separator (`\x1e`) and sends binary ones as `b` + base64.
//!
//! Socket.IO (inside an Engine.IO message):
//! `<type>[<attachments>-][<namespace>,][<ack id>][<JSON data>]`, e.g. `2["chat","hi"]`,
//! `2/admin,7["save",{"id":1}]`, `51-["file",{"_placeholder":true,"num":0}]` (the attachment
//! follows as a binary packet).

use base64::Engine as _;
use serde_json::{Value, json};

/// Engine.IO packet types.
pub const OPEN: char = '0';
pub const CLOSE: char = '1';
pub const PING: char = '2';
pub const PONG: char = '3';
pub const MESSAGE: char = '4';
pub const UPGRADE: char = '5';
pub const NOOP: char = '6';

/// Separates packets in a long-polling payload.
pub const SEPARATOR: char = '\u{1e}';

/// Socket.IO packet types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Connect,
    Disconnect,
    Event,
    Ack,
    ConnectError,
    BinaryEvent,
    BinaryAck,
}

impl Kind {
    fn code(self) -> char {
        match self {
            Kind::Connect => '0',
            Kind::Disconnect => '1',
            Kind::Event => '2',
            Kind::Ack => '3',
            Kind::ConnectError => '4',
            Kind::BinaryEvent => '5',
            Kind::BinaryAck => '6',
        }
    }

    fn from_code(c: char) -> Option<Self> {
        Some(match c {
            '0' => Kind::Connect,
            '1' => Kind::Disconnect,
            '2' => Kind::Event,
            '3' => Kind::Ack,
            '4' => Kind::ConnectError,
            '5' => Kind::BinaryEvent,
            '6' => Kind::BinaryAck,
            _ => return None,
        })
    }

    pub fn is_binary(self) -> bool {
        matches!(self, Kind::BinaryEvent | Kind::BinaryAck)
    }
}

/// A Socket.IO packet.
#[derive(Debug, Clone, PartialEq)]
pub struct Packet {
    pub kind: Kind,
    /// `/` for the main namespace.
    pub namespace: String,
    pub id: Option<u64>,
    pub data: Option<Value>,
    /// Binary attachments that follow (binary kinds only).
    pub attachments: usize,
}

impl Packet {
    pub fn new(kind: Kind, namespace: &str, data: Option<Value>) -> Self {
        Self { kind, namespace: namespace.to_string(), id: None, data, attachments: 0 }
    }

    /// An event: `[event, ...args]`.
    pub fn event(namespace: &str, event: &str, args: Vec<Value>, id: Option<u64>) -> Self {
        let mut data = vec![Value::String(event.to_string())];
        data.extend(args);
        Self { id, ..Self::new(Kind::Event, namespace, Some(Value::Array(data))) }
    }

    /// The packet as Engine.IO frames: an event or acknowledgement with binary arguments
    /// (`{"base64": …}`) becomes a binary one followed by its attachments.
    pub fn frames(mut self) -> Vec<Frame> {
        let mut attachments = Vec::new();
        if matches!(self.kind, Kind::Event | Kind::Ack)
            && let Some(data) = self.data.as_mut()
        {
            take_binary(data, &mut attachments);
        }
        if !attachments.is_empty() {
            self.kind = if self.kind == Kind::Event { Kind::BinaryEvent } else { Kind::BinaryAck };
            self.attachments = attachments.len();
        }
        let mut frames = vec![Frame::Text(format!("{MESSAGE}{}", self.encode()))];
        frames.extend(attachments.into_iter().map(Frame::Binary));
        frames
    }

    /// The event's name and arguments.
    pub fn event_parts(&self) -> Option<(&str, &[Value])> {
        match (&self.kind, &self.data) {
            (Kind::Event | Kind::BinaryEvent, Some(Value::Array(items))) => {
                let (name, args) = items.split_first()?;
                Some((name.as_str()?, args))
            }
            _ => None,
        }
    }

    /// The packet as a Socket.IO string (without the Engine.IO `4`).
    pub fn encode(&self) -> String {
        let mut out = String::new();
        out.push(self.kind.code());
        if self.kind.is_binary() {
            out.push_str(&format!("{}-", self.attachments));
        }
        if self.namespace != "/" && !self.namespace.is_empty() {
            out.push_str(&self.namespace);
            out.push(',');
        }
        if let Some(id) = self.id {
            out.push_str(&id.to_string());
        }
        if let Some(data) = &self.data {
            out.push_str(&data.to_string());
        }
        out
    }

    /// Read a Socket.IO string (without the Engine.IO `4`).
    pub fn decode(text: &str) -> Result<Self, String> {
        let mut rest = text;
        let first = rest.chars().next().ok_or("an empty packet")?;
        let kind = Kind::from_code(first).ok_or_else(|| format!("unknown packet type '{first}'"))?;
        rest = &rest[1..];
        let mut attachments = 0;
        if kind.is_binary() {
            let (count, after) = rest.split_once('-').ok_or("a binary packet without an attachment count")?;
            attachments = count.parse().map_err(|_| format!("a bad attachment count '{count}'"))?;
            rest = after;
        }
        let mut namespace = "/".to_string();
        if rest.starts_with('/') {
            match rest.find(',') {
                Some(i) => {
                    namespace = rest[..i].to_string();
                    rest = &rest[i + 1..];
                }
                None => {
                    namespace = rest.to_string();
                    rest = "";
                }
            }
        }
        let digits = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len());
        let id = match digits {
            0 => None,
            _ => Some(rest[..digits].parse::<u64>().map_err(|_| "an acknowledgement id that is too large")?),
        };
        rest = &rest[digits..];
        let data = match rest.trim() {
            "" => None,
            json => Some(serde_json::from_str::<Value>(json).map_err(|e| format!("data that isn't JSON ({e})"))?),
        };
        let valid = match kind {
            Kind::Connect => data.as_ref().is_none_or(Value::is_object),
            Kind::Disconnect => data.is_none(),
            Kind::ConnectError => data.as_ref().is_none_or(|d| d.is_object() || d.is_string()),
            Kind::Event | Kind::BinaryEvent => {
                data.as_ref().and_then(Value::as_array).and_then(|a| a.first()).is_some_and(Value::is_string)
            }
            Kind::Ack | Kind::BinaryAck => data.as_ref().is_some_and(Value::is_array) && id.is_some(),
        };
        if !valid {
            return Err(format!("a malformed packet: {}", preview(text)));
        }
        Ok(Self { kind, namespace, id, data, attachments })
    }
}

/// Arguments typed as JSON: an array is several arguments, anything else one; blank is none.
pub fn parse_args(text: &str) -> Result<Vec<Value>, String> {
    match text.trim() {
        "" => Ok(Vec::new()),
        text => match serde_json::from_str::<Value>(text) {
            Ok(Value::Array(items)) => Ok(items),
            Ok(value) => Ok(vec![value]),
            Err(e) => Err(format!("The arguments are not valid JSON: {e}")),
        },
    }
}

/// Put binary attachments where their placeholders are, as `{"base64": …, "bytes": n}`.
pub fn fill_placeholders(value: &mut Value, attachments: &[Vec<u8>]) {
    match value {
        Value::Object(map) if map.get("_placeholder") == Some(&Value::Bool(true)) => {
            if let Some(bytes) = map.get("num").and_then(Value::as_u64).and_then(|n| attachments.get(n as usize)) {
                *value =
                    json!({ "base64": base64::engine::general_purpose::STANDARD.encode(bytes), "bytes": bytes.len() });
            }
        }
        Value::Object(map) => map.values_mut().for_each(|v| fill_placeholders(v, attachments)),
        Value::Array(items) => items.iter_mut().for_each(|v| fill_placeholders(v, attachments)),
        _ => {}
    }
}

/// The placeholder of attachment `num`.
pub fn placeholder(num: usize) -> Value {
    json!({ "_placeholder": true, "num": num })
}

/// The reverse of [`fill_placeholders`]: `{"base64": …}` objects (optionally with `bytes`)
/// become attachments, placeholders take their place.
pub fn take_binary(value: &mut Value, attachments: &mut Vec<Vec<u8>>) {
    let decoded = match value {
        Value::Object(map) if map.keys().all(|k| k == "base64" || k == "bytes") => map
            .get("base64")
            .and_then(Value::as_str)
            .and_then(|b| base64::engine::general_purpose::STANDARD.decode(b).ok()),
        _ => None,
    };
    match (decoded, value) {
        (Some(bytes), value) => {
            *value = placeholder(attachments.len());
            attachments.push(bytes);
        }
        (None, Value::Object(map)) => map.values_mut().for_each(|v| take_binary(v, attachments)),
        (None, Value::Array(items)) => items.iter_mut().for_each(|v| take_binary(v, attachments)),
        _ => {}
    }
}

/// Engine.IO packets in a long-polling payload: text, or binary (`b` + base64).
pub fn split_payload(body: &str) -> Vec<Frame> {
    body.split(SEPARATOR)
        .filter(|p| !p.is_empty())
        .map(|p| match p.strip_prefix('b') {
            Some(b64) => match base64::engine::general_purpose::STANDARD.decode(b64) {
                Ok(bytes) => Frame::Binary(bytes),
                Err(_) => Frame::Text(p.to_string()),
            },
            None => Frame::Text(p.to_string()),
        })
        .collect()
}

/// Engine.IO packets as one long-polling payload.
pub fn join_payload(frames: &[Frame]) -> String {
    let parts: Vec<String> = frames
        .iter()
        .map(|f| match f {
            Frame::Text(t) => t.clone(),
            Frame::Binary(b) => format!("b{}", base64::engine::general_purpose::STANDARD.encode(b)),
        })
        .collect();
    parts.join(&SEPARATOR.to_string())
}

/// One Engine.IO packet: text (type character + data) or binary (a message's attachment).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Frame {
    Text(String),
    Binary(Vec<u8>),
}

/// The Engine.IO open packet's data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Handshake {
    pub sid: String,
    pub upgrades: Vec<String>,
    pub ping_interval: u64,
    pub ping_timeout: u64,
    pub max_payload: u64,
}

impl Handshake {
    pub fn parse(data: &str) -> Result<Self, String> {
        let v: Value = serde_json::from_str(data).map_err(|_| format!("a bad open packet: {}", preview(data)))?;
        let sid = v["sid"].as_str().filter(|s| !s.is_empty()).ok_or("an open packet without a session id")?;
        Ok(Self {
            sid: sid.to_string(),
            upgrades: v["upgrades"]
                .as_array()
                .map(|a| a.iter().filter_map(|u| u.as_str().map(String::from)).collect())
                .unwrap_or_default(),
            ping_interval: v["pingInterval"].as_u64().unwrap_or(25_000),
            ping_timeout: v["pingTimeout"].as_u64().unwrap_or(20_000),
            max_payload: v["maxPayload"].as_u64().unwrap_or(1_000_000),
        })
    }

    pub fn to_json(&self) -> String {
        json!({
            "sid": self.sid,
            "upgrades": self.upgrades,
            "pingInterval": self.ping_interval,
            "pingTimeout": self.ping_timeout,
            "maxPayload": self.max_payload,
        })
        .to_string()
    }
}

fn preview(text: &str) -> String {
    let start: String = text.chars().take(120).collect();
    if start.len() < text.len() { format!("{start}…") } else { start }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(text: &str) -> Packet {
        let packet = Packet::decode(text).unwrap_or_else(|e| panic!("{text}: {e}"));
        assert_eq!(packet.encode(), text);
        packet
    }

    #[test]
    fn packets_as_socket_io_writes_them() {
        let p = round_trip("0");
        assert_eq!((p.kind, p.namespace.as_str(), p.data), (Kind::Connect, "/", None));
        let p = round_trip(r#"0/admin,{"token":"x"}"#);
        assert_eq!((p.namespace.as_str(), p.data), ("/admin", Some(json!({ "token": "x" }))));
        let p = round_trip(r#"2["chat","hi",{"n":1}]"#);
        assert_eq!(p.event_parts(), Some(("chat", &[json!("hi"), json!({ "n": 1 })][..])));
        let p = round_trip(r#"2/admin,12["save",1]"#);
        assert_eq!((p.namespace.as_str(), p.id), ("/admin", Some(12)));
        let p = round_trip(r#"312["ok"]"#);
        assert_eq!((p.kind, p.id, p.data), (Kind::Ack, Some(12), Some(json!(["ok"]))));
        let p = round_trip(r#"51-["file",{"_placeholder":true,"num":0}]"#);
        assert_eq!((p.kind, p.attachments), (Kind::BinaryEvent, 1));
        let p = round_trip(r#"4{"message":"Not authorized"}"#);
        assert_eq!(p.kind, Kind::ConnectError);
        round_trip("1/admin,");
        // A namespace alone (no comma) is accepted when reading.
        assert_eq!(Packet::decode("1/admin").unwrap().namespace, "/admin");
        assert_eq!(Packet::event("/", "a", vec![], Some(3)).encode(), r#"23["a"]"#);
    }

    #[test]
    fn malformed_packets_are_refused() {
        for bad in ["", "9", "2", "2{}", "2[1]", "3[]", "0[]", "1{}", "5[\"a\"]", "2[\"a\""] {
            assert!(Packet::decode(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn arguments_and_attachments() {
        assert_eq!(parse_args(" ").unwrap(), Vec::<Value>::new());
        assert_eq!(parse_args("[1, \"a\"]").unwrap(), [json!(1), json!("a")]);
        assert_eq!(parse_args("{\"a\": 1}").unwrap(), [json!({ "a": 1 })]);
        assert_eq!(parse_args("\"text\"").unwrap(), [json!("text")]);
        assert!(parse_args("hello").unwrap_err().starts_with("The arguments are not valid JSON"));

        let mut v = json!(["file", { "name": "a", "data": placeholder(0) }, [placeholder(1)]]);
        fill_placeholders(&mut v, &[vec![1, 2, 3], vec![]]);
        assert_eq!(
            v,
            json!(["file", { "name": "a", "data": { "base64": "AQID", "bytes": 3 } }, [{ "base64": "", "bytes": 0 }]])
        );
        // And back.
        let mut attachments = Vec::new();
        take_binary(&mut v, &mut attachments);
        assert_eq!(v, json!(["file", { "name": "a", "data": placeholder(0) }, [placeholder(1)]]));
        assert_eq!(attachments, [vec![1, 2, 3], vec![]]);
        let mut not_binary = json!([{ "base64": "!!" }, { "base64": "AQID", "name": "x" }]);
        take_binary(&mut not_binary, &mut attachments);
        assert_eq!(not_binary, json!([{ "base64": "!!" }, { "base64": "AQID", "name": "x" }]));

        let frames = Packet::event("/", "up", vec![json!({ "base64": "AQI=" })], Some(2)).frames();
        assert_eq!(
            frames,
            [Frame::Text(r#"451-2["up",{"_placeholder":true,"num":0}]"#.into()), Frame::Binary(vec![1, 2])]
        );
        let frames = Packet::event("/", "up", vec![json!(1)], None).frames();
        assert_eq!(frames, [Frame::Text(r#"42["up",1]"#.into())]);
    }

    #[test]
    fn polling_payloads() {
        let frames = split_payload("40\u{1e}42[\"a\"]\u{1e}bAQI=");
        assert_eq!(frames, [Frame::Text("40".into()), Frame::Text("42[\"a\"]".into()), Frame::Binary(vec![1, 2])]);
        assert_eq!(join_payload(&frames), "40\u{1e}42[\"a\"]\u{1e}bAQI=");
        let h = Handshake::parse(
            r#"{"sid":"abc","upgrades":["websocket"],"pingInterval":300,"pingTimeout":200,"maxPayload":1000}"#,
        )
        .unwrap();
        assert_eq!(
            (h.sid.as_str(), h.upgrades.as_slice(), h.ping_interval, h.ping_timeout),
            ("abc", &["websocket".to_string()][..], 300, 200)
        );
        assert_eq!(Handshake::parse(&h.to_json()).unwrap(), h);
        assert!(Handshake::parse("{}").is_err());
    }
}
