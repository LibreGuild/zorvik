//! MQTT client (3.1.1 and 5) for the connection log: connect, subscribe,
//! publish and receive.
//!
//! rumqttc encodes and decodes the packets; the connection is the engine's own
//! (TCP, or TLS with the OS trust store and the TLS settings), so the log shows
//! the same address, timing and certificate details as the TCP client and TLS
//! stays on rustls/ring. Connections are direct (no proxy). QoS 1/2 flows,
//! packet ids and keep alive are handled here; nothing is persisted.

use std::collections::{HashMap, HashSet};
use std::hash::{BuildHasher, Hasher};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use bytes::{Buf, Bytes, BytesMut};
use rumqttc::mqttbytes::{self as mqtt4, v4};
use rumqttc::v5::mqttbytes::{self as mqtt5, v5};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;
use url::Url;

use crate::error::{EngineError, ErrorKind, Result, human_duration};
use crate::framing::display_payload;
use crate::http::{Client, RequestOptions, Timing};
use crate::net::{self, Target};
use crate::socket::{
    SocketConnected, SocketEvent, SocketOpened, SocketOutgoing, SocketSession, decode_base64, ms, now_ms,
};
use crate::tls::Alpn;
use crate::ws::Direction;

/// Largest packet accepted from the broker (announced to MQTT 5 brokers) or sent.
const MAX_PACKET: usize = crate::framing::MAX_MESSAGE;
/// Session expiry asked for when an MQTT 5 session is kept (clean session off).
const KEPT_SESSION_SECS: u32 = 3600;
/// Longest MQTT string (topic, client id, user name): a 2-byte length.
const MAX_STRING: usize = 65_535;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MqttProtocol {
    #[default]
    V311,
    V5,
}

impl MqttProtocol {
    fn label(self) -> &'static str {
        match self {
            MqttProtocol::V311 => "MQTT 3.1.1",
            MqttProtocol::V5 => "MQTT 5",
        }
    }
}

/// How to connect, with variables already substituted.
#[derive(Debug, Clone)]
pub struct MqttConfig {
    /// Empty: a random `zorvik-xxxxxxxx`.
    pub client_id: String,
    pub protocol: MqttProtocol,
    /// Start a new session (MQTT 5: clean start, session ends on disconnect).
    pub clean_session: bool,
    /// Seconds; 0 turns keep alive off.
    pub keep_alive_secs: u16,
    /// Empty: none.
    pub username: String,
    pub password: String,
    /// Topic filters (and QoS) subscribed right after connecting.
    pub subscriptions: Vec<(String, u8)>,
}

impl Default for MqttConfig {
    fn default() -> Self {
        Self {
            client_id: String::new(),
            protocol: MqttProtocol::V311,
            clean_session: true,
            keep_alive_secs: 30,
            username: String::new(),
            password: String::new(),
            subscriptions: Vec::new(),
        }
    }
}

/// `mqtt://host[:1883]` or `mqtts://host[:8883]` (also `tcp://`, `ssl://`, `tls://`).
fn parse_address(raw: &str) -> Result<(String, u16, bool)> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err(EngineError::invalid("Address is empty"));
    }
    let with_scheme = if raw.contains("://") { raw.to_string() } else { format!("mqtt://{raw}") };
    let url = Url::parse(&with_scheme).map_err(|e| EngineError::invalid(format!("Invalid address '{raw}': {e}")))?;
    let tls = match url.scheme() {
        "mqtt" | "tcp" => false,
        "mqtts" | "ssl" | "tls" => true,
        "ws" | "wss" => {
            return Err(EngineError::invalid("MQTT over WebSocket is not supported yet: use mqtt:// or mqtts://"));
        }
        other => return Err(EngineError::invalid(format!("Unsupported scheme '{other}' (use mqtt:// or mqtts://)"))),
    };
    let host = url
        .host_str()
        .filter(|h| !h.is_empty())
        .ok_or_else(|| EngineError::invalid(format!("'{raw}' has no host")))?
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_string();
    let port = url.port().unwrap_or(if tls { 8883 } else { 1883 });
    Ok((host, port, tls))
}

fn random_client_id() -> String {
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    hasher.write_u128(SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0));
    format!("zorvik-{:08x}", hasher.finish() as u32)
}

// ---- packets ------------------------------------------------------------------

/// What the broker sent, the same for both protocol versions.
#[derive(Debug)]
enum In {
    ConnAck {
        refused: Option<String>,
        session_present: bool,
        limits: BrokerLimits,
    },
    Publish {
        topic: String,
        payload: Bytes,
        qos: u8,
        retain: bool,
        dup: bool,
        pkid: u16,
    },
    PubAck {
        pkid: u16,
        error: Option<String>,
    },
    PubRec {
        pkid: u16,
        error: Option<String>,
    },
    PubRel {
        pkid: u16,
    },
    PubComp {
        pkid: u16,
    },
    /// Per filter: granted QoS, or why it was refused.
    SubAck {
        pkid: u16,
        codes: Vec<std::result::Result<u8, String>>,
    },
    /// Per filter (MQTT 5): why it was not unsubscribed, if it was not.
    UnsubAck {
        pkid: u16,
        problems: Vec<Option<String>>,
    },
    PingResp,
    Disconnect {
        reason: String,
    },
    /// A packet only a client sends.
    Unexpected(&'static str),
}

/// What an MQTT 5 broker announced in CONNACK.
#[derive(Debug, Clone)]
struct BrokerLimits {
    max_qos: u8,
    retain_available: bool,
    receive_max: u16,
    max_packet: usize,
    keep_alive: Option<u16>,
    assigned_id: Option<String>,
    reason: Option<String>,
}

impl Default for BrokerLimits {
    fn default() -> Self {
        Self {
            max_qos: 2,
            retain_available: true,
            receive_max: u16::MAX,
            max_packet: MAX_PACKET,
            keep_alive: None,
            assigned_id: None,
            reason: None,
        }
    }
}

/// What the client sends after CONNECT.
enum Out {
    Publish { topic: String, payload: Bytes, qos: u8, retain: bool, pkid: u16 },
    PubAck(u16),
    PubRec(u16),
    PubRel(u16),
    PubComp(u16),
    Subscribe { pkid: u16, filters: Vec<(String, u8)> },
    Unsubscribe { pkid: u16, filters: Vec<String> },
    PingReq,
}

/// DISCONNECT with no reason code: "normal disconnection" in both versions.
const DISCONNECT: [u8; 2] = [0xE0, 0x00];

fn qos4(q: u8) -> mqtt4::QoS {
    match q {
        0 => mqtt4::QoS::AtMostOnce,
        1 => mqtt4::QoS::AtLeastOnce,
        _ => mqtt4::QoS::ExactlyOnce,
    }
}

fn qos5(q: u8) -> mqtt5::QoS {
    match q {
        0 => mqtt5::QoS::AtMostOnce,
        1 => mqtt5::QoS::AtLeastOnce,
        _ => mqtt5::QoS::ExactlyOnce,
    }
}

fn encode_connect(protocol: MqttProtocol, config: &MqttConfig, client_id: &str) -> Result<BytesMut> {
    let mut buf = BytesMut::new();
    let written = match protocol {
        MqttProtocol::V311 => {
            let login = (!config.username.is_empty() || !config.password.is_empty())
                .then(|| v4::Login::new(config.username.clone(), config.password.clone()));
            let connect = v4::Connect {
                protocol: mqtt4::Protocol::V4,
                keep_alive: config.keep_alive_secs,
                client_id: client_id.to_string(),
                clean_session: config.clean_session,
                last_will: None,
                login,
            };
            v4::Packet::Connect(connect).write(&mut buf, MAX_PACKET).map(drop).map_err(|e| e.to_string())
        }
        MqttProtocol::V5 => {
            let login = (!config.username.is_empty() || !config.password.is_empty())
                .then(|| v5::Login::new(config.username.clone(), config.password.clone()));
            let mut properties = v5::ConnectProperties::new();
            properties.max_packet_size = Some(MAX_PACKET as u32);
            properties.session_expiry_interval = (!config.clean_session).then_some(KEPT_SESSION_SECS);
            let connect = v5::Connect {
                keep_alive: config.keep_alive_secs,
                client_id: client_id.to_string(),
                clean_start: config.clean_session,
                properties: Some(properties),
            };
            v5::Packet::Connect(connect, None, login)
                .write(&mut buf, Some(MAX_PACKET as u32))
                .map(drop)
                .map_err(|e| e.to_string())
        }
    };
    written.map_err(|e| EngineError::invalid(format!("Could not build the CONNECT packet: {e}")))?;
    Ok(buf)
}

fn encode(protocol: MqttProtocol, out: Out, max: usize) -> std::result::Result<BytesMut, String> {
    let mut buf = BytesMut::new();
    let written = match protocol {
        MqttProtocol::V311 => {
            let packet = match out {
                Out::Publish { topic, payload, qos, retain, pkid } => {
                    v4::Packet::Publish(v4::Publish { dup: false, qos: qos4(qos), retain, topic, pkid, payload })
                }
                Out::PubAck(pkid) => v4::Packet::PubAck(v4::PubAck::new(pkid)),
                Out::PubRec(pkid) => v4::Packet::PubRec(v4::PubRec::new(pkid)),
                Out::PubRel(pkid) => v4::Packet::PubRel(v4::PubRel::new(pkid)),
                Out::PubComp(pkid) => v4::Packet::PubComp(v4::PubComp::new(pkid)),
                Out::Subscribe { pkid, filters } => v4::Packet::Subscribe(v4::Subscribe {
                    pkid,
                    filters: filters.into_iter().map(|(t, q)| v4::SubscribeFilter::new(t, qos4(q))).collect(),
                }),
                Out::Unsubscribe { pkid, filters } => {
                    v4::Packet::Unsubscribe(v4::Unsubscribe { pkid, topics: filters })
                }
                Out::PingReq => v4::Packet::PingReq,
            };
            packet.write(&mut buf, max).map(drop).map_err(|e| e.to_string())
        }
        MqttProtocol::V5 => {
            let packet = match out {
                Out::Publish { topic, payload, qos, retain, pkid } => v5::Packet::Publish(v5::Publish {
                    dup: false,
                    qos: qos5(qos),
                    retain,
                    topic: Bytes::from(topic.into_bytes()),
                    pkid,
                    payload,
                    properties: None,
                }),
                Out::PubAck(pkid) => v5::Packet::PubAck(v5::PubAck::new(pkid, None)),
                Out::PubRec(pkid) => v5::Packet::PubRec(v5::PubRec::new(pkid, None)),
                Out::PubRel(pkid) => v5::Packet::PubRel(v5::PubRel::new(pkid, None)),
                Out::PubComp(pkid) => v5::Packet::PubComp(v5::PubComp::new(pkid, None)),
                Out::Subscribe { pkid, filters } => v5::Packet::Subscribe(v5::Subscribe {
                    pkid,
                    filters: filters.into_iter().map(|(t, q)| v5::Filter::new(t, qos5(q))).collect(),
                    properties: None,
                }),
                Out::Unsubscribe { pkid, filters } => {
                    v5::Packet::Unsubscribe(v5::Unsubscribe { pkid, filters, properties: None })
                }
                Out::PingReq => v5::Packet::PingReq(v5::PingReq),
            };
            packet.write(&mut buf, Some(max.min(u32::MAX as usize) as u32)).map(drop).map_err(|e| e.to_string())
        }
    };
    written.map(|_| buf)
}

/// The next complete packet in `buf`, `None` when more bytes are needed.
/// Framing is checked first: rumqttc also says "insufficient bytes" for a
/// packet whose body is shorter than its fields, which is malformed, not partial.
fn decode(protocol: MqttProtocol, buf: &mut BytesMut) -> std::result::Result<Option<In>, String> {
    if buf.len() < 2 {
        return Ok(None);
    }
    match protocol {
        MqttProtocol::V311 => {
            match mqtt4::check(buf.iter(), MAX_PACKET) {
                Ok(_) => {}
                Err(mqtt4::Error::InsufficientBytes(_)) => return Ok(None),
                Err(e) => return Err(e.to_string()),
            }
            v4::Packet::read(buf, MAX_PACKET).map(|p| Some(from_v4(p))).map_err(|e| e.to_string())
        }
        MqttProtocol::V5 => {
            // rumqttc refuses an empty DISCONNECT, which MQTT 5 allows (normal disconnection).
            if buf[..2] == DISCONNECT {
                buf.advance(2);
                return Ok(Some(In::Disconnect { reason: "normal disconnection".into() }));
            }
            match v5::check(buf.iter(), Some(MAX_PACKET as u32)) {
                Ok(_) => {}
                Err(mqtt5::Error::InsufficientBytes(_)) => return Ok(None),
                Err(e) => return Err(e.to_string()),
            }
            v5::Packet::read(buf, Some(MAX_PACKET as u32)).map(|p| Some(from_v5(p))).map_err(|e| e.to_string())
        }
    }
}

fn from_v4(packet: v4::Packet) -> In {
    match packet {
        v4::Packet::ConnAck(ack) => In::ConnAck {
            refused: match ack.code {
                v4::ConnectReturnCode::Success => None,
                v4::ConnectReturnCode::RefusedProtocolVersion => Some("unacceptable protocol version".into()),
                v4::ConnectReturnCode::BadClientId => Some("client identifier rejected".into()),
                v4::ConnectReturnCode::ServiceUnavailable => Some("server unavailable".into()),
                v4::ConnectReturnCode::BadUserNamePassword => Some("bad user name or password".into()),
                v4::ConnectReturnCode::NotAuthorized => Some("not authorized".into()),
            },
            session_present: ack.session_present,
            limits: BrokerLimits::default(),
        },
        v4::Packet::Publish(p) => In::Publish {
            topic: p.topic,
            payload: p.payload,
            qos: p.qos as u8,
            retain: p.retain,
            dup: p.dup,
            pkid: p.pkid,
        },
        v4::Packet::PubAck(a) => In::PubAck { pkid: a.pkid, error: None },
        v4::Packet::PubRec(a) => In::PubRec { pkid: a.pkid, error: None },
        v4::Packet::PubRel(a) => In::PubRel { pkid: a.pkid },
        v4::Packet::PubComp(a) => In::PubComp { pkid: a.pkid },
        v4::Packet::SubAck(a) => In::SubAck {
            pkid: a.pkid,
            codes: a
                .return_codes
                .iter()
                .map(|c| match c {
                    v4::SubscribeReasonCode::Success(q) => Ok(*q as u8),
                    v4::SubscribeReasonCode::Failure => Err("refused by the broker".to_string()),
                })
                .collect(),
        },
        v4::Packet::UnsubAck(a) => In::UnsubAck { pkid: a.pkid, problems: Vec::new() },
        v4::Packet::PingResp => In::PingResp,
        v4::Packet::Disconnect => In::Unexpected("DISCONNECT"),
        v4::Packet::Connect(_) => In::Unexpected("CONNECT"),
        v4::Packet::Subscribe(_) => In::Unexpected("SUBSCRIBE"),
        v4::Packet::Unsubscribe(_) => In::Unexpected("UNSUBSCRIBE"),
        v4::Packet::PingReq => In::Unexpected("PINGREQ"),
    }
}

/// `NotAuthorized` -> `not authorized` (rumqttc names MQTT 5 reason codes after the spec).
fn reason_text(code: impl std::fmt::Debug) -> String {
    let debug = format!("{code:?}").replace("QoS", "Qos").replace("UserNamePassword", "UserNameOrPassword");
    let mut out = String::new();
    for (i, c) in debug.chars().enumerate() {
        if c.is_ascii_uppercase() && i > 0 {
            out.push(' ');
        }
        out.push(c.to_ascii_lowercase());
    }
    out.replace("qos", "QoS").replace("pkid", "packet id")
}

fn with_reason_string(text: String, reason: Option<&String>) -> String {
    match reason.filter(|r| !r.trim().is_empty()) {
        Some(r) => format!("{text} ({})", r.trim()),
        None => text,
    }
}

fn from_v5(packet: v5::Packet) -> In {
    match packet {
        v5::Packet::ConnAck(ack) => {
            let p = ack.properties.as_ref();
            let reason = p.and_then(|p| p.reason_string.clone());
            let refused = (ack.code != v5::ConnectReturnCode::Success)
                .then(|| with_reason_string(reason_text(ack.code), reason.as_ref()));
            In::ConnAck {
                refused,
                session_present: ack.session_present,
                limits: BrokerLimits {
                    max_qos: p.and_then(|p| p.max_qos).unwrap_or(2).min(2),
                    retain_available: p.and_then(|p| p.retain_available) != Some(0),
                    // Zero is a protocol error; treat it as "no limit given".
                    receive_max: p.and_then(|p| p.receive_max).filter(|m| *m > 0).unwrap_or(u16::MAX),
                    max_packet: p
                        .and_then(|p| p.max_packet_size)
                        .map_or(MAX_PACKET, |m| usize::try_from(m).unwrap_or(MAX_PACKET).min(MAX_PACKET)),
                    keep_alive: p.and_then(|p| p.server_keep_alive),
                    assigned_id: p.and_then(|p| p.assigned_client_identifier.clone()),
                    reason,
                },
            }
        }
        v5::Packet::Publish(p) => {
            let topic = if p.topic.is_empty() {
                // A topic alias, which we never allow (topic alias maximum 0).
                match p.properties.as_ref().and_then(|x| x.topic_alias) {
                    Some(alias) => format!("(topic alias {alias})"),
                    None => String::new(),
                }
            } else {
                String::from_utf8_lossy(&p.topic).into_owned()
            };
            In::Publish { topic, payload: p.payload, qos: p.qos as u8, retain: p.retain, dup: p.dup, pkid: p.pkid }
        }
        v5::Packet::PubAck(a) => In::PubAck {
            pkid: a.pkid,
            error: (!matches!(a.reason, v5::PubAckReason::Success | v5::PubAckReason::NoMatchingSubscribers)).then(
                || {
                    with_reason_string(
                        reason_text(a.reason),
                        a.properties.as_ref().and_then(|p| p.reason_string.as_ref()),
                    )
                },
            ),
        },
        v5::Packet::PubRec(a) => In::PubRec {
            pkid: a.pkid,
            error: (!matches!(a.reason, v5::PubRecReason::Success | v5::PubRecReason::NoMatchingSubscribers)).then(
                || {
                    with_reason_string(
                        reason_text(a.reason),
                        a.properties.as_ref().and_then(|p| p.reason_string.as_ref()),
                    )
                },
            ),
        },
        v5::Packet::PubRel(a) => In::PubRel { pkid: a.pkid },
        v5::Packet::PubComp(a) => In::PubComp { pkid: a.pkid },
        v5::Packet::SubAck(a) => {
            let why = a.properties.as_ref().and_then(|p| p.reason_string.as_ref());
            In::SubAck {
                pkid: a.pkid,
                codes: a
                    .return_codes
                    .iter()
                    .map(|c| match c {
                        v5::SubscribeReasonCode::Success(q) => Ok(*q as u8),
                        other => Err(with_reason_string(reason_text(other), why)),
                    })
                    .collect(),
            }
        }
        v5::Packet::UnsubAck(a) => In::UnsubAck {
            pkid: a.pkid,
            problems: a
                .reasons
                .iter()
                .map(|r| (!matches!(r, v5::UnsubAckReason::Success)).then(|| reason_text(r)))
                .collect(),
        },
        v5::Packet::PingResp(_) => In::PingResp,
        v5::Packet::Disconnect(d) => In::Disconnect {
            reason: with_reason_string(
                reason_text(d.reason_code),
                d.properties.as_ref().and_then(|p| p.reason_string.as_ref()),
            ),
        },
        v5::Packet::Auth(_) => In::Unexpected("AUTH"),
        v5::Packet::Connect(..) => In::Unexpected("CONNECT"),
        v5::Packet::Subscribe(_) => In::Unexpected("SUBSCRIBE"),
        v5::Packet::Unsubscribe(_) => In::Unexpected("UNSUBSCRIBE"),
        v5::Packet::PingReq(_) => In::Unexpected("PINGREQ"),
    }
}

/// `QoS 1 · retained` for the log.
fn detail(qos: u8, retain: bool, dup: bool) -> String {
    let mut parts = vec![format!("QoS {qos}")];
    if retain {
        parts.push("retained".into());
    }
    if dup {
        parts.push("duplicate".into());
    }
    parts.join(" · ")
}

fn message_event(direction: Direction, topic: &str, payload: &[u8], qos: u8, retain: bool, dup: bool) -> SocketEvent {
    let (text, base64) = display_payload(payload);
    SocketEvent::Message {
        direction,
        text,
        base64,
        size: payload.len() as u64,
        timestamp: now_ms(),
        peer: None,
        topic: Some(topic.to_string()),
        detail: Some(detail(qos, retain, dup)),
    }
}

/// A topic filter to subscribe to: wildcards only as whole levels, `#` last.
fn check_filter(filter: &str) -> std::result::Result<(), String> {
    if filter.is_empty() {
        return Err("Enter a topic filter".into());
    }
    if filter.len() > MAX_STRING || filter.contains('\0') {
        return Err(format!("'{filter}' is not a valid topic filter"));
    }
    if !mqtt4::valid_filter(filter) {
        return Err(format!(
            "'{filter}' is not a valid topic filter: + and # must be whole levels, and # must come last"
        ));
    }
    Ok(())
}

/// A topic to publish to: no wildcards.
fn check_topic(topic: &str) -> std::result::Result<(), String> {
    if topic.is_empty() {
        return Err("Enter a topic".into());
    }
    if topic.len() > MAX_STRING || topic.contains('\0') {
        return Err(format!("'{topic}' is not a valid topic"));
    }
    if topic.contains(['+', '#']) {
        return Err(format!("'{topic}' contains a wildcard (+ or #); those are only for subscriptions"));
    }
    Ok(())
}

// ---- connect ------------------------------------------------------------------

impl Client {
    /// Connect to an MQTT broker: `mqtt://host[:1883]` or `mqtts://host[:8883]`.
    /// Returns once the broker accepted the connection (CONNACK); the
    /// subscriptions in `config` are requested right after.
    pub async fn mqtt(&self, address: &str, opts: &RequestOptions, config: &MqttConfig) -> Result<SocketConnected> {
        let started = Instant::now();
        let (host, port, secure) = parse_address(address)?;
        if let Some(guard) = &opts.host_guard {
            guard.check_host(&host)?;
        }
        let protocol = config.protocol;
        let client_id = match config.client_id.trim() {
            "" => random_client_id(),
            id => id.to_string(),
        };
        if client_id.len() > MAX_STRING || config.username.len() > MAX_STRING || config.password.len() > MAX_STRING {
            return Err(EngineError::invalid("Client ID, user name and password must be shorter than 64 KB"));
        }
        if protocol == MqttProtocol::V311 && config.username.is_empty() && !config.password.is_empty() {
            return Err(EngineError::invalid("MQTT 3.1.1 needs a user name when a password is set"));
        }
        let connect_packet = encode_connect(protocol, config, &client_id)?;

        let conn = net::connect(
            &Target {
                host: &host,
                port,
                tls: secure,
                alpn: Alpn::None,
                tls_options: &opts.tls,
                proxy: None,
                force_tunnel: false,
                connect_timeout: opts.connect_timeout,
            },
            &self.tls,
        )
        .await?;
        let local_addr = match &conn.stream {
            net::Stream::Plain(s) => s.local_addr().ok(),
            net::Stream::Tls(s) => s.get_ref().0.local_addr().ok(),
        };
        let mut stream = conn.stream;

        let sent = Instant::now();
        let wait = opts.connect_timeout;
        let handshake = async {
            stream
                .write_all(&connect_packet)
                .await
                .map_err(|e| EngineError::new(ErrorKind::Io, format!("Could not send CONNECT: {e}")))?;
            let mut buf = BytesMut::with_capacity(4096);
            let ack = read_connack(&mut stream, &mut buf, protocol).await?;
            Ok::<_, EngineError>((ack, buf))
        };
        let ((refused, session_present, limits), leftover) = tokio::time::timeout(wait, handshake)
            .await
            .map_err(|_| EngineError::timeout("Waiting for the broker to accept the connection (CONNACK)", wait))??;
        if let Some(reason) = refused {
            return Err(EngineError::new(ErrorKind::Connect, format!("The broker refused the connection: {reason}")));
        }
        let connack_time = sent.elapsed();

        let opened = SocketOpened {
            protocol: match &conn.tls {
                Some(t) => format!("{} + {}", protocol.label(), t.version),
                None => protocol.label().to_string(),
            },
            remote_addr: Some(conn.remote_addr.to_string()),
            local_addr: local_addr.map(|a| a.to_string()),
            tls: conn.tls.clone(),
            timing: Timing {
                dns_ms: ms(conn.timing.dns),
                connect_ms: ms(conn.timing.connect),
                tls_ms: ms(conn.timing.tls),
                ttfb_ms: ms(connack_time),
                total_ms: ms(started.elapsed()),
                ..Default::default()
            },
        };

        let (out_tx, out_rx) = mpsc::unbounded_channel();
        let (ev_tx, ev_rx) = mpsc::unbounded_channel();
        let keep_alive = limits.keep_alive.unwrap_or(config.keep_alive_secs);
        let mut info = format!("Client ID {}", limits.assigned_id.as_deref().unwrap_or(&client_id));
        if session_present {
            info.push_str(" · resumed the previous session");
        }
        if limits.keep_alive.is_some_and(|k| k != config.keep_alive_secs) {
            info.push_str(&format!(" · the broker set keep alive to {keep_alive} s"));
        }
        if let Some(reason) = limits.reason.as_ref().filter(|r| !r.trim().is_empty()) {
            info.push_str(&format!(" · {}", reason.trim()));
        }
        let _ = ev_tx.send(SocketEvent::info(info));

        let mut session = Session {
            protocol,
            events: ev_tx,
            limits,
            pending: HashMap::new(),
            last_pkid: 0,
            awaiting_release: HashSet::new(),
            outbox: BytesMut::new(),
        };
        for (filter, qos) in &config.subscriptions {
            session.subscribe(filter.trim(), *qos);
        }
        tokio::spawn(run(stream, session, leftover, Duration::from_secs(u64::from(keep_alive)), out_rx));
        Ok(SocketConnected { opened, meta: None, session: SocketSession { tx: out_tx }, events: ev_rx })
    }
}

type ConnAckResult = (Option<String>, bool, BrokerLimits);

async fn read_connack<S: AsyncRead + Unpin>(
    stream: &mut S,
    buf: &mut BytesMut,
    protocol: MqttProtocol,
) -> Result<ConnAckResult> {
    loop {
        // Kept to re-read an MQTT 3.1.1 answer to an MQTT 5 CONNECT (decoding consumes the packet).
        let before = (protocol == MqttProtocol::V5).then(|| buf.clone());
        match decode(protocol, buf) {
            Ok(Some(In::ConnAck { refused, session_present, limits })) => {
                return Ok((refused, session_present, limits));
            }
            Ok(Some(other)) => {
                return Err(EngineError::new(
                    ErrorKind::Protocol,
                    format!("The broker sent {} before accepting the connection", packet_name(&other)),
                ));
            }
            Ok(None) => {}
            Err(e) => {
                // An MQTT 3.1.1 broker answers an MQTT 5 CONNECT with a 3.1.1 CONNACK ("unacceptable version").
                if let Some(mut before) = before
                    && before.first() == Some(&0x20)
                    && let Ok(Some(In::ConnAck { refused: Some(_), .. })) = decode(MqttProtocol::V311, &mut before)
                {
                    return Err(EngineError::new(
                        ErrorKind::Connect,
                        "The broker does not support MQTT 5. Switch the MQTT version to 3.1.1.",
                    ));
                }
                return Err(EngineError::new(ErrorKind::Protocol, format!("Invalid CONNACK from the broker: {e}")));
            }
        }
        buf.reserve(4096);
        let n = stream
            .read_buf(buf)
            .await
            .map_err(|e| EngineError::new(ErrorKind::Io, format!("Connection error while connecting: {e}")))?;
        if n == 0 {
            let hint = match protocol {
                MqttProtocol::V5 => " If it only speaks MQTT 3.1.1, switch the MQTT version.",
                MqttProtocol::V311 => " Check the address, and whether it expects TLS (mqtts://).",
            };
            return Err(EngineError::new(
                ErrorKind::Connect,
                format!("The broker closed the connection without accepting it.{hint}"),
            ));
        }
    }
}

fn packet_name(packet: &In) -> &'static str {
    match packet {
        In::ConnAck { .. } => "CONNACK",
        In::Publish { .. } => "PUBLISH",
        In::PubAck { .. } => "PUBACK",
        In::PubRec { .. } => "PUBREC",
        In::PubRel { .. } => "PUBREL",
        In::PubComp { .. } => "PUBCOMP",
        In::SubAck { .. } => "SUBACK",
        In::UnsubAck { .. } => "UNSUBACK",
        In::PingResp => "PINGRESP",
        In::Disconnect { .. } => "DISCONNECT",
        In::Unexpected(name) => name,
    }
}

// ---- session ------------------------------------------------------------------

/// Something sent that waits for the broker's acknowledgement.
enum Pending {
    Publish { topic: String },
    Subscribe(Vec<(String, u8)>),
    Unsubscribe(Vec<String>),
}

/// Protocol state of a live connection. Handlers queue packets in `outbox`;
/// the loop writes them.
struct Session {
    protocol: MqttProtocol,
    events: mpsc::UnboundedSender<SocketEvent>,
    limits: BrokerLimits,
    /// By packet id: our publishes (QoS 1/2), subscribes and unsubscribes.
    pending: HashMap<u16, Pending>,
    last_pkid: u16,
    /// Incoming QoS 2 messages delivered but not yet released (PUBREL).
    awaiting_release: HashSet<u16>,
    outbox: BytesMut,
}

impl Session {
    fn emit(&self, event: SocketEvent) {
        let _ = self.events.send(event);
    }

    fn error(&self, message: impl Into<String>) {
        self.emit(SocketEvent::Error { message: message.into() });
    }

    fn queue(&mut self, out: Out) -> std::result::Result<(), String> {
        let bytes = encode(self.protocol, out, self.limits.max_packet)?;
        self.outbox.extend_from_slice(&bytes);
        Ok(())
    }

    /// A free packet id (1..=65535), or `None` when all are waiting for acknowledgements.
    fn next_pkid(&mut self) -> Option<u16> {
        for _ in 0..u16::MAX {
            self.last_pkid = self.last_pkid.checked_add(1).unwrap_or(1);
            if !self.pending.contains_key(&self.last_pkid) {
                return Some(self.last_pkid);
            }
        }
        None
    }

    fn unacked_publishes(&self) -> usize {
        self.pending.values().filter(|p| matches!(p, Pending::Publish { .. })).count()
    }

    fn subscribe(&mut self, filter: &str, qos: u8) {
        if let Err(e) = check_filter(filter) {
            return self.error(e);
        }
        let Some(pkid) = self.next_pkid() else {
            return self.error("Too many requests are waiting for the broker to answer");
        };
        let filters = vec![(filter.to_string(), qos.min(2))];
        match self.queue(Out::Subscribe { pkid, filters: filters.clone() }) {
            Ok(()) => {
                self.pending.insert(pkid, Pending::Subscribe(filters));
            }
            Err(e) => self.error(format!("Could not subscribe to {filter}: {e}")),
        }
    }

    fn unsubscribe(&mut self, filter: &str) {
        if let Err(e) = check_filter(filter) {
            return self.error(e);
        }
        let Some(pkid) = self.next_pkid() else {
            return self.error("Too many requests are waiting for the broker to answer");
        };
        match self.queue(Out::Unsubscribe { pkid, filters: vec![filter.to_string()] }) {
            Ok(()) => {
                self.pending.insert(pkid, Pending::Unsubscribe(vec![filter.to_string()]));
            }
            Err(e) => self.error(format!("Could not unsubscribe from {filter}: {e}")),
        }
    }

    fn publish(&mut self, topic: &str, payload: Vec<u8>, qos: u8, retain: bool) {
        if let Err(e) = check_topic(topic) {
            return self.error(e);
        }
        if qos > 2 {
            return self.error(format!("QoS {qos} does not exist (use 0, 1 or 2)"));
        }
        if qos > self.limits.max_qos {
            return self.error(format!("The broker accepts messages up to QoS {}", self.limits.max_qos));
        }
        if retain && !self.limits.retain_available {
            return self.error("The broker does not support retained messages");
        }
        let pkid = if qos == 0 {
            0
        } else {
            if self.unacked_publishes() >= usize::from(self.limits.receive_max) {
                return self.error(format!(
                    "{} messages are waiting for the broker's acknowledgement (its limit); try again shortly",
                    self.limits.receive_max
                ));
            }
            match self.next_pkid() {
                Some(id) => id,
                None => return self.error("Too many messages are waiting for the broker's acknowledgement"),
            }
        };
        let event = message_event(Direction::Sent, topic, &payload, qos, retain, false);
        let out = Out::Publish { topic: topic.to_string(), payload: Bytes::from(payload), qos, retain, pkid };
        match self.queue(out) {
            Ok(()) => {
                if qos > 0 {
                    self.pending.insert(pkid, Pending::Publish { topic: topic.to_string() });
                }
                self.emit(event);
            }
            Err(e) => self.error(format!("Could not publish to {topic}: {e}")),
        }
    }

    fn outgoing(&mut self, msg: SocketOutgoing) {
        match msg {
            SocketOutgoing::Publish { topic, text, base64, qos, retain } => {
                let payload = match (text, base64) {
                    (Some(text), _) => text.into_bytes(),
                    (None, Some(b64)) => match decode_base64(&b64) {
                        Ok(bytes) => bytes,
                        Err(e) => return self.error(e.message),
                    },
                    (None, None) => Vec::new(),
                };
                self.publish(topic.trim(), payload, qos, retain);
            }
            SocketOutgoing::Subscribe { topic, qos } => self.subscribe(topic.trim(), qos),
            SocketOutgoing::Unsubscribe { topic } => self.unsubscribe(topic.trim()),
            SocketOutgoing::Text { .. } | SocketOutgoing::Binary { .. } => self.error("Enter a topic"),
            SocketOutgoing::Emit { .. } => self.error("MQTT publishes to topics; it has no events"),
        }
    }

    /// Handle a packet from the broker. `Some(reason)` closes the connection.
    fn incoming(&mut self, packet: In) -> Option<String> {
        match packet {
            In::Publish { topic, payload, qos, retain, dup, pkid } => {
                match qos {
                    1 => {
                        self.emit(message_event(Direction::Received, &topic, &payload, qos, retain, dup));
                        self.ack(Out::PubAck(pkid));
                    }
                    2 => {
                        // Deliver once: a resent PUBLISH before PUBREL is the same message.
                        if self.awaiting_release.insert(pkid) {
                            self.emit(message_event(Direction::Received, &topic, &payload, qos, retain, dup));
                        }
                        self.ack(Out::PubRec(pkid));
                    }
                    _ => self.emit(message_event(Direction::Received, &topic, &payload, qos, retain, dup)),
                }
            }
            In::PubAck { pkid, error } => {
                if let Some(Pending::Publish { topic }) =
                    self.take_pending(pkid, |p| matches!(p, Pending::Publish { .. }))
                    && let Some(error) = error
                {
                    self.error(format!("The broker rejected the message to {topic}: {error}"));
                }
            }
            In::PubRec { pkid, error } => {
                if !matches!(self.pending.get(&pkid), Some(Pending::Publish { .. })) {
                    // Not ours (any more): release it so the broker can forget it.
                    self.ack(Out::PubRel(pkid));
                } else if let Some(error) = error {
                    if let Some(Pending::Publish { topic }) = self.pending.remove(&pkid) {
                        self.error(format!("The broker rejected the message to {topic}: {error}"));
                    }
                } else {
                    self.ack(Out::PubRel(pkid));
                }
            }
            In::PubRel { pkid } => {
                self.awaiting_release.remove(&pkid);
                self.ack(Out::PubComp(pkid));
            }
            In::PubComp { pkid } => {
                self.take_pending(pkid, |p| matches!(p, Pending::Publish { .. }));
            }
            In::SubAck { pkid, codes } => {
                let Some(Pending::Subscribe(filters)) = self.take_pending(pkid, |p| matches!(p, Pending::Subscribe(_)))
                else {
                    return None;
                };
                for (i, (filter, asked)) in filters.iter().enumerate() {
                    match codes.get(i) {
                        Some(Ok(granted)) if granted < asked => self.emit(SocketEvent::info(format!(
                            "Subscribed to {filter} (QoS {granted}; asked for {asked})"
                        ))),
                        Some(Ok(granted)) => {
                            self.emit(SocketEvent::info(format!("Subscribed to {filter} (QoS {granted})")))
                        }
                        Some(Err(why)) => self.error(format!("Subscription to {filter} failed: {why}")),
                        None => self.error(format!("The broker did not answer the subscription to {filter}")),
                    }
                }
            }
            In::UnsubAck { pkid, problems } => {
                let Some(Pending::Unsubscribe(filters)) =
                    self.take_pending(pkid, |p| matches!(p, Pending::Unsubscribe(_)))
                else {
                    return None;
                };
                for (i, filter) in filters.iter().enumerate() {
                    match problems.get(i).cloned().flatten() {
                        Some(why) => self.emit(SocketEvent::info(format!("Unsubscribed from {filter} ({why})"))),
                        None => self.emit(SocketEvent::info(format!("Unsubscribed from {filter}"))),
                    }
                }
            }
            In::PingResp => {}
            In::Disconnect { reason } => return Some(format!("The broker disconnected: {reason}")),
            In::ConnAck { .. } => self.error("The broker sent a second CONNACK"),
            In::Unexpected(name) => self.error(format!("The broker sent {name}, which only clients send")),
        }
        None
    }

    fn ack(&mut self, out: Out) {
        if let Err(e) = self.queue(out) {
            self.error(format!("Could not acknowledge a message: {e}"));
        }
    }

    /// Remove the pending entry for `pkid` when it is of the expected kind.
    fn take_pending(&mut self, pkid: u16, is: impl Fn(&Pending) -> bool) -> Option<Pending> {
        if self.pending.get(&pkid).is_some_and(is) { self.pending.remove(&pkid) } else { None }
    }
}

async fn run(
    stream: net::Stream,
    mut session: Session,
    mut buf: BytesMut,
    keep_alive: Duration,
    mut outgoing: mpsc::UnboundedReceiver<SocketOutgoing>,
) {
    let (mut reader, mut writer) = tokio::io::split(stream);
    let closed = |reason: String, by_client: bool| SocketEvent::Closed { reason, by_client };
    let mut last_sent = tokio::time::Instant::now();
    // When the PINGREQ that is still unanswered was sent.
    let mut ping_sent: Option<tokio::time::Instant> = None;
    // Packets that arrived with CONNACK are handled before reading more.
    let mut have_bytes = !buf.is_empty();
    loop {
        if !session.outbox.is_empty() {
            let bytes = std::mem::take(&mut session.outbox);
            // Flushed so TLS records still buffered after a blocked write go out too.
            let written = async {
                writer.write_all(&bytes).await?;
                writer.flush().await
            };
            // A broker that stopped reading can't hold the session (and this task) forever.
            let result = match tokio::time::timeout(crate::socket::WRITE_TIMEOUT, written).await {
                Ok(result) => result.map_err(|e| format!("Send failed: {e}")),
                Err(_) => Err(format!(
                    "Send failed: the broker stopped reading (nothing went out for {} s)",
                    crate::socket::WRITE_TIMEOUT.as_secs()
                )),
            };
            if let Err(reason) = result {
                session.emit(closed(reason, false));
                return;
            }
            last_sent = tokio::time::Instant::now();
        }
        if have_bytes {
            have_bytes = false;
            loop {
                match decode(session.protocol, &mut buf) {
                    Ok(Some(packet)) => {
                        if matches!(packet, In::PingResp) {
                            ping_sent = None;
                        }
                        if let Some(reason) = session.incoming(packet) {
                            let _ = writer.shutdown().await;
                            session.emit(closed(reason, false));
                            return;
                        }
                    }
                    Ok(None) => break,
                    Err(e) => {
                        let _ = writer.write_all(&DISCONNECT).await;
                        session.emit(closed(format!("Invalid packet from the broker: {e}"), true));
                        return;
                    }
                }
            }
            continue;
        }
        let ping_at = match ping_sent {
            Some(at) => at + keep_alive,
            None => last_sent + keep_alive,
        };
        buf.reserve(16 * 1024);
        tokio::select! {
            out = outgoing.recv() => {
                let Some(msg) = out else {
                    // Session handle dropped: say goodbye so the broker drops no will message.
                    let _ = tokio::time::timeout(Duration::from_secs(1), async {
                        let _ = writer.write_all(&DISCONNECT).await;
                        let _ = writer.shutdown().await;
                    })
                    .await;
                    session.emit(closed("Disconnected".into(), true));
                    return;
                };
                session.outgoing(msg);
            }
            read = reader.read_buf(&mut buf) => match read {
                Ok(0) => {
                    session.emit(closed("Closed by the broker".into(), false));
                    return;
                }
                Ok(_) => have_bytes = true,
                Err(e) => {
                    session.emit(closed(format!("Connection error: {e}"), false));
                    return;
                }
            },
            _ = tokio::time::sleep_until(ping_at), if !keep_alive.is_zero() => {
                if ping_sent.is_some() {
                    let _ = writer.write_all(&DISCONNECT).await;
                    session.emit(closed(
                        format!("The broker stopped answering (no PINGRESP within {})", human_duration(keep_alive)),
                        true,
                    ));
                    return;
                }
                if let Err(e) = session.queue(Out::PingReq) {
                    session.error(e);
                }
                ping_sent = Some(tokio::time::Instant::now());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses() {
        assert_eq!(parse_address("localhost").unwrap(), ("localhost".into(), 1883, false));
        assert_eq!(parse_address("mqtts://broker.test").unwrap(), ("broker.test".into(), 8883, true));
        assert_eq!(parse_address("mqtt://[::1]:1999").unwrap(), ("::1".into(), 1999, false));
        assert!(parse_address("ws://x").unwrap_err().message.contains("WebSocket"));
        assert!(parse_address("http://x").is_err());
        assert!(parse_address("").is_err());
        let id = random_client_id();
        assert!(id.starts_with("zorvik-") && id.len() == 15, "{id}");
    }

    #[test]
    fn topics_and_filters() {
        assert!(check_topic("a/b").is_ok());
        assert!(check_topic("").is_err());
        assert!(check_topic("a/+").is_err());
        assert!(check_filter("a/+/c").is_ok());
        assert!(check_filter("a/#").is_ok());
        assert!(check_filter("a/#/c").is_err());
        assert!(check_filter("a+").is_err());
        assert_eq!(detail(1, true, false), "QoS 1 · retained");
        assert_eq!(reason_text(v5::ConnectReturnCode::BadUserNamePassword), "bad user name or password");
        assert_eq!(reason_text(v5::SubscribeReasonCode::QuotaExceeded), "quota exceeded");
        assert_eq!(reason_text(v5::ConnectReturnCode::QoSNotSupported), "QoS not supported");
    }

    #[test]
    fn packets_round_trip_and_garbage_is_an_error() {
        for protocol in [MqttProtocol::V311, MqttProtocol::V5] {
            let mut buf = encode(
                protocol,
                Out::Publish { topic: "a/b".into(), payload: Bytes::from_static(b"hi"), qos: 1, retain: true, pkid: 7 },
                MAX_PACKET,
            )
            .unwrap();
            let tail = buf.split_off(3);
            assert!(decode(protocol, &mut buf).unwrap().is_none(), "incomplete");
            buf.unsplit(tail);
            match decode(protocol, &mut buf).unwrap() {
                Some(In::Publish { topic, payload, qos: 1, retain: true, pkid: 7, .. }) => {
                    assert_eq!((topic.as_str(), &payload[..]), ("a/b", &b"hi"[..]));
                }
                other => panic!("{other:?}"),
            }
            assert!(buf.is_empty());
        }
        let mut empty_disconnect = BytesMut::from(&DISCONNECT[..]);
        assert!(matches!(decode(MqttProtocol::V5, &mut empty_disconnect), Ok(Some(In::Disconnect { .. }))));
        // Arbitrary bytes never panic.
        let mut seed = 0x9e37_79b9_7f4a_7c15u64;
        for len in 0..400 {
            for protocol in [MqttProtocol::V311, MqttProtocol::V5] {
                let mut buf: BytesMut = (0..len)
                    .map(|_| {
                        seed ^= seed << 13;
                        seed ^= seed >> 7;
                        seed ^= seed << 17;
                        seed as u8
                    })
                    .collect::<Vec<u8>>()
                    .as_slice()
                    .into();
                while let Ok(Some(_)) = decode(protocol, &mut buf) {}
            }
        }
    }
}
