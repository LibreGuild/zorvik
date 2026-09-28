//! Connection-style clients below HTTP: raw TCP (optionally TLS), UDP and
//! MQTT. Each connect returns a session handle for sending and a stream of
//! events, like the WebSocket client.

use std::collections::VecDeque;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt, ReadHalf, WriteHalf};
use tokio::sync::mpsc;
use ts_rs::TS;
use url::Url;

use crate::error::{EngineError, ErrorKind, Result};
use crate::framing::{Deframer, Framing, LineEnding, display_payload, encode_message};
use crate::http::{Client, RequestOptions, Timing};
use crate::net::{self, Target};
use crate::tls::{Alpn, TlsInfo};
use crate::ws::Direction;

/// How a TCP/UDP connection frames and sends messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SocketConfig {
    pub framing: Framing,
    /// Length prefix size (1, 2 or 4) for [`Framing::LengthPrefixed`].
    pub length_bytes: u8,
    /// Appended to text messages.
    pub line_ending: LineEnding,
    /// UDP: allow sending to broadcast addresses.
    pub broadcast: bool,
}

impl Default for SocketConfig {
    fn default() -> Self {
        Self { framing: Framing::Raw, length_bytes: 2, line_ending: LineEnding::None, broadcast: false }
    }
}

/// Something to send on a socket session.
#[derive(Debug, Clone, Deserialize, TS)]
#[serde(tag = "type", rename_all = "camelCase")]
#[ts(export)]
pub enum SocketOutgoing {
    Text {
        text: String,
    },
    /// Binary payload as base64.
    Binary {
        base64: String,
    },
    /// MQTT: publish `text` (or `base64`) to `topic`.
    #[serde(rename_all = "camelCase")]
    Publish {
        topic: String,
        #[serde(default)]
        text: Option<String>,
        #[serde(default)]
        base64: Option<String>,
        #[serde(default)]
        qos: u8,
        #[serde(default)]
        retain: bool,
    },
    /// MQTT: subscribe to a topic filter.
    #[serde(rename_all = "camelCase")]
    Subscribe {
        topic: String,
        #[serde(default)]
        qos: u8,
    },
    /// MQTT: stop a subscription.
    #[serde(rename_all = "camelCase")]
    Unsubscribe {
        topic: String,
    },
    /// Socket.IO: emit `event` with `args` (JSON: an array for several arguments, else one),
    /// or with one binary argument (`base64`).
    #[serde(rename_all = "camelCase")]
    Emit {
        event: String,
        #[serde(default)]
        args: String,
        #[serde(default)]
        base64: Option<String>,
        /// Ask the server to acknowledge.
        #[serde(default)]
        ack: bool,
    },
}

impl SocketOutgoing {
    /// Payload bytes of a text/binary message (`None` for control messages).
    pub fn payload(&self) -> Result<Option<(Vec<u8>, bool)>> {
        Ok(match self {
            SocketOutgoing::Text { text } => Some((text.as_bytes().to_vec(), true)),
            SocketOutgoing::Binary { base64 } => Some((decode_base64(base64)?, false)),
            SocketOutgoing::Emit { event, args, base64, .. } => {
                if event.trim().is_empty() {
                    return Err(EngineError::invalid("Enter the name of the event to emit"));
                }
                match base64 {
                    Some(data) => {
                        decode_base64(data)?;
                    }
                    None => {
                        crate::socketio::packet::parse_args(args).map_err(EngineError::invalid)?;
                    }
                }
                None
            }
            _ => None,
        })
    }
}

pub(crate) fn decode_base64(data: &str) -> Result<Vec<u8>> {
    base64::engine::general_purpose::STANDARD
        .decode(data)
        .map_err(|e| EngineError::invalid(format!("Binary payload is not valid base64: {e}")))
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(tag = "type", rename_all = "camelCase")]
#[ts(export)]
pub enum SocketEvent {
    #[serde(rename_all = "camelCase")]
    Message {
        direction: Direction,
        /// UTF-8 payload when it decodes.
        text: Option<String>,
        /// Base64 payload otherwise.
        base64: Option<String>,
        #[ts(type = "number")]
        size: u64,
        /// Unix epoch milliseconds.
        timestamp: f64,
        /// UDP: the other side's address.
        peer: Option<String>,
        /// MQTT: the message topic.
        topic: Option<String>,
        /// MQTT: QoS and retain flag, e.g. `QoS 1 · retained`.
        detail: Option<String>,
    },
    /// Something worth showing in the log that is not a message (e.g. "Subscribed to a/b").
    #[serde(rename_all = "camelCase")]
    Info { text: String, timestamp: f64 },
    #[serde(rename_all = "camelCase")]
    Error { message: String },
    #[serde(rename_all = "camelCase")]
    Closed { reason: String, by_client: bool },
}

impl SocketEvent {
    pub fn message(direction: Direction, bytes: &[u8]) -> Self {
        let (text, base64) = display_payload(bytes);
        SocketEvent::Message {
            direction,
            text,
            base64,
            size: bytes.len() as u64,
            timestamp: now_ms(),
            peer: None,
            topic: None,
            detail: None,
        }
    }

    pub fn info(text: impl Into<String>) -> Self {
        SocketEvent::Info { text: text.into(), timestamp: now_ms() }
    }
}

/// What a socket connect established (shown when the connection opens).
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SocketOpened {
    /// `TCP`, `TLS`, `UDP`, `MQTT 3.1.1`, …
    pub protocol: String,
    pub remote_addr: Option<String>,
    pub local_addr: Option<String>,
    pub tls: Option<TlsInfo>,
    pub timing: Timing,
}

/// Handle to a live socket session. Dropping it closes the connection.
pub struct SocketSession {
    pub(crate) tx: mpsc::UnboundedSender<SocketOutgoing>,
}

impl SocketSession {
    pub fn send(&self, msg: SocketOutgoing) -> Result<()> {
        msg.payload()?;
        self.tx.send(msg).map_err(|_| EngineError::new(ErrorKind::Io, "The connection is closed"))
    }
}

pub struct SocketConnected {
    pub opened: SocketOpened,
    /// The HTTP answer that started it, for sessions that begin with one (WebSocket, SSE).
    pub meta: Option<crate::http::ResponseMeta>,
    pub session: SocketSession,
    pub events: mpsc::UnboundedReceiver<SocketEvent>,
}

pub(crate) fn now_ms() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as f64).unwrap_or(0.0)
}

pub(crate) fn ms(d: std::time::Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

/// Parse `scheme://host:port` (or `host:port`) for a socket client. Returns the
/// URL with `default_scheme` filled in; the port is required.
pub fn parse_socket_url(raw: &str, default_scheme: &str, schemes: &[&str]) -> Result<(Url, String, u16)> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err(EngineError::invalid("Address is empty"));
    }
    let with_scheme = if raw.contains("://") { raw.to_string() } else { format!("{default_scheme}://{raw}") };
    let url = Url::parse(&with_scheme).map_err(|e| EngineError::invalid(format!("Invalid address '{raw}': {e}")))?;
    if !schemes.contains(&url.scheme()) {
        return Err(EngineError::invalid(format!(
            "Unsupported scheme '{}' (use {})",
            url.scheme(),
            schemes.join(", ")
        )));
    }
    let host =
        url.host_str().filter(|h| !h.is_empty()).ok_or_else(|| EngineError::invalid(format!("'{raw}' has no host")))?;
    let host = host.trim_start_matches('[').trim_end_matches(']').to_string();
    let port = url
        .port_or_known_default()
        .ok_or_else(|| EngineError::invalid(format!("'{raw}' has no port (e.g. {default_scheme}://{host}:9000)")))?;
    Ok((url, host, port))
}

impl Client {
    /// Open a raw TCP connection: `tcp://host:port`, or `tls://host:port` for TLS.
    /// Goes through the HTTP proxy (CONNECT) when one applies to the host.
    pub async fn tcp(&self, address: &str, opts: &RequestOptions, config: SocketConfig) -> Result<SocketConnected> {
        let started = Instant::now();
        let (url, host, port) = parse_socket_url(address, "tcp", &["tcp", "tls", "ssl"])?;
        let secure = url.scheme() != "tcp";
        let proxy = opts.proxy.for_target(&host, true);
        let conn = net::connect(
            &Target {
                host: &host,
                port,
                tls: secure,
                alpn: Alpn::None,
                tls_options: &opts.tls,
                proxy,
                force_tunnel: true,
                connect_timeout: opts.connect_timeout,
            },
            &self.tls,
        )
        .await?;
        let local_addr = match &conn.stream {
            net::Stream::Plain(s) => s.local_addr().ok(),
            net::Stream::Tls(s) => s.get_ref().0.local_addr().ok(),
        };
        let opened = SocketOpened {
            protocol: match &conn.tls {
                Some(t) => format!("TCP + {}", t.version),
                None => "TCP".into(),
            },
            remote_addr: Some(conn.remote_addr.to_string()),
            local_addr: local_addr.map(|a| a.to_string()),
            tls: conn.tls.clone(),
            timing: Timing {
                dns_ms: ms(conn.timing.dns),
                connect_ms: ms(conn.timing.connect),
                tls_ms: ms(conn.timing.tls),
                total_ms: ms(started.elapsed()),
                ..Default::default()
            },
        };
        let (out_tx, out_rx) = mpsc::unbounded_channel();
        let (ev_tx, ev_rx) = mpsc::unbounded_channel();
        tokio::spawn(run_tcp(conn.stream, config, out_rx, ev_tx));
        Ok(SocketConnected { opened, meta: None, session: SocketSession { tx: out_tx }, events: ev_rx })
    }
}

async fn run_tcp(
    stream: net::Stream,
    config: SocketConfig,
    outgoing: mpsc::UnboundedReceiver<SocketOutgoing>,
    events: mpsc::UnboundedSender<SocketEvent>,
) {
    let (reader, writer) = tokio::io::split(stream);
    // Reading goes on while a message is being written: a peer that answers
    // before it has read everything (an echo server) would otherwise stall
    // both sides once the socket buffers are full. Whichever side ends first
    // ends the connection.
    tokio::select! {
        () = read_tcp(reader, config, &events) => {}
        () = write_tcp(writer, config, outgoing, &events) => {}
    }
}

/// How long a disconnect waits for queued messages to go out (like WebSocket's close).
const CLOSE_GRACE: Duration = Duration::from_secs(5);

fn closed(reason: String, by_client: bool) -> SocketEvent {
    SocketEvent::Closed { reason, by_client }
}

async fn write_tcp(
    mut writer: WriteHalf<net::Stream>,
    config: SocketConfig,
    mut outgoing: mpsc::UnboundedReceiver<SocketOutgoing>,
    events: &mpsc::UnboundedSender<SocketEvent>,
) {
    // Taken from the channel while an earlier message was still being written.
    let mut waiting = VecDeque::new();
    // Once the session handle is dropped, what is queued still goes out, but a
    // peer that stopped reading can't keep the connection open past this.
    let mut closing: Option<tokio::time::Instant> = None;
    loop {
        let msg = match waiting.pop_front() {
            Some(msg) => msg,
            None if closing.is_some() => break,
            None => match outgoing.recv().await {
                Some(msg) => msg,
                None => break,
            },
        };
        let payload = match msg.payload() {
            Ok(Some(p)) => p,
            Ok(None) => {
                let _ = events.send(SocketEvent::Error { message: "Not supported on a TCP connection".into() });
                continue;
            }
            Err(e) => {
                let _ = events.send(SocketEvent::Error { message: e.message });
                continue;
            }
        };
        let (bytes, is_text) = payload;
        // The log shows the message as typed (plus its line ending), not the length prefix.
        let mut shown = bytes.clone();
        if is_text {
            shown.extend_from_slice(config.line_ending.as_str().as_bytes());
        }
        let framed = match encode_message(config.framing, config.length_bytes, config.line_ending, bytes, is_text) {
            Ok(f) => f,
            Err(message) => {
                let _ = events.send(SocketEvent::Error { message });
                continue;
            }
        };
        // Flushed so TLS records still buffered after a blocked write go out too.
        let write = async {
            writer.write_all(&framed).await?;
            writer.flush().await
        };
        tokio::pin!(write);
        let written = loop {
            tokio::select! {
                result = &mut write => break Some(result),
                next = outgoing.recv(), if closing.is_none() => match next {
                    Some(msg) => waiting.push_back(msg),
                    None => closing = Some(tokio::time::Instant::now() + CLOSE_GRACE),
                },
                _ = tokio::time::sleep_until(closing.unwrap_or_else(tokio::time::Instant::now)), if closing.is_some() => {
                    break None;
                }
            }
        };
        match written {
            Some(Ok(())) => {
                let _ = events.send(SocketEvent::message(Direction::Sent, &shown));
            }
            Some(Err(e)) => {
                let _ = events.send(closed(format!("Send failed: {e}"), false));
                return;
            }
            None => break,
        }
    }
    // Session handle dropped: close our side.
    let until = closing.unwrap_or_else(|| tokio::time::Instant::now() + CLOSE_GRACE);
    let _ = tokio::time::timeout_at(until, writer.shutdown()).await;
    let _ = events.send(closed("Disconnected".into(), true));
}

async fn read_tcp(
    mut reader: ReadHalf<net::Stream>,
    config: SocketConfig,
    events: &mpsc::UnboundedSender<SocketEvent>,
) {
    let mut deframer = Deframer::new(config.framing, config.length_bytes);
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        match reader.read(&mut buf).await {
            Ok(0) => {
                if let Some(rest) = deframer.remainder() {
                    let _ = events.send(SocketEvent::message(Direction::Received, &rest));
                }
                let _ = events.send(closed("Closed by server".into(), false));
                return;
            }
            Ok(n) => match deframer.push(&buf[..n]) {
                Ok(messages) => {
                    for m in messages {
                        let _ = events.send(SocketEvent::message(Direction::Received, &m));
                    }
                }
                Err(message) => {
                    let _ = events.send(closed(message, true));
                    return;
                }
            },
            Err(e) => {
                let _ = events.send(closed(format!("Connection error: {e}"), false));
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn socket_urls() {
        let (_, host, port) = parse_socket_url("localhost:9000", "tcp", &["tcp", "tls"]).unwrap();
        assert_eq!((host.as_str(), port), ("localhost", 9000));
        let (url, host, _) = parse_socket_url("tls://[::1]:993", "tcp", &["tcp", "tls"]).unwrap();
        assert_eq!((url.scheme(), host.as_str()), ("tls", "::1"));
        assert!(parse_socket_url("tcp://host", "tcp", &["tcp"]).is_err());
        assert!(parse_socket_url("http://host:1", "tcp", &["tcp"]).is_err());
        assert!(parse_socket_url(" ", "tcp", &["tcp"]).is_err());
    }
}
