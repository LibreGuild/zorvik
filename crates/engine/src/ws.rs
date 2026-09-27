//! WebSocket client. Uses the same connection path as HTTP (DNS, proxy
//! tunnel, OS-trusted TLS) and reports the upgrade handshake like a response.

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::protocol::{CloseFrame, WebSocketConfig};
use tokio_tungstenite::tungstenite::{self, Message};
use ts_rs::TS;
use url::{Position, Url};

use crate::cookies::CookieJar;
use crate::error::{EngineError, ErrorKind, Result};
use crate::http::{Client, Header, HttpRequest, RequestOptions, ResponseMeta, SentRequest, Timing, canonical_name};
use crate::net::{self, Target};
use crate::tls::Alpn;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum Direction {
    Sent,
    Received,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum WsMessageKind {
    Text,
    Binary,
    Ping,
    Pong,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(tag = "type", rename_all = "camelCase")]
#[ts(export)]
pub enum WsEvent {
    #[serde(rename_all = "camelCase")]
    Message {
        direction: Direction,
        kind: WsMessageKind,
        /// UTF-8 payload for text frames.
        text: Option<String>,
        /// Base64 payload for binary/ping/pong frames.
        base64: Option<String>,
        #[ts(type = "number")]
        size: u64,
        /// Unix epoch milliseconds.
        timestamp: f64,
    },
    #[serde(rename_all = "camelCase")]
    Closed { code: Option<u16>, reason: String, by_client: bool },
    #[serde(rename_all = "camelCase")]
    Error { message: String },
}

#[derive(Debug, Clone, Deserialize, TS)]
#[serde(tag = "type", rename_all = "camelCase")]
#[ts(export)]
pub enum WsOutgoing {
    Text {
        text: String,
    },
    /// Binary payload as base64.
    Binary {
        base64: String,
    },
    Ping,
    Close {
        code: Option<u16>,
        reason: Option<String>,
    },
}

/// Handle to a live WebSocket. Dropping it closes the connection.
pub struct WsSession {
    tx: mpsc::UnboundedSender<WsOutgoing>,
}

impl WsSession {
    pub fn send(&self, msg: WsOutgoing) -> Result<()> {
        if let WsOutgoing::Binary { base64 } = &msg {
            base64::engine::general_purpose::STANDARD
                .decode(base64)
                .map_err(|e| EngineError::invalid(format!("Binary payload is not valid base64: {e}")))?;
        }
        self.tx.send(msg).map_err(|_| EngineError::new(ErrorKind::Io, "WebSocket is closed"))
    }
}

pub struct WsConnected {
    pub meta: ResponseMeta,
    pub timing: Timing,
    pub session: WsSession,
    pub events: mpsc::UnboundedReceiver<WsEvent>,
}

fn now_ms() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as f64).unwrap_or(0.0)
}

/// Accept ws://, wss://, http:// and https:// URLs (no scheme = ws://).
pub fn normalize_ws_url(raw: &str) -> Result<Url> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err(EngineError::invalid("URL is empty"));
    }
    let with_scheme = if crate::http::has_scheme(raw) { raw.to_string() } else { format!("ws://{raw}") };
    let mut url = Url::parse(&with_scheme).map_err(|e| EngineError::invalid(format!("Invalid URL '{raw}': {e}")))?;
    let scheme = match url.scheme() {
        "ws" | "http" => "ws",
        "wss" | "https" => "wss",
        other => return Err(EngineError::invalid(format!("Unsupported WebSocket scheme '{other}' (use ws or wss)"))),
    };
    url.set_scheme(scheme).map_err(|_| EngineError::invalid("Invalid WebSocket URL"))?;
    if url.host_str().is_none_or(str::is_empty) {
        return Err(EngineError::invalid(format!("URL '{raw}' has no host")));
    }
    url.set_fragment(None);
    Ok(url)
}

impl Client {
    pub async fn websocket(
        &self,
        req: HttpRequest,
        opts: &RequestOptions,
        jar: Option<&CookieJar>,
    ) -> Result<WsConnected> {
        let started = Instant::now();
        let url = normalize_ws_url(&req.url)?;
        let secure = url.scheme() == "wss";
        let host = url.host_str().unwrap_or_default();
        let bare_host = host.trim_start_matches('[').trim_end_matches(']');
        let port = url.port_or_known_default().unwrap_or(if secure { 443 } else { 80 });
        let http_url = {
            let mut u = url.clone();
            let _ = u.set_scheme(if secure { "https" } else { "http" });
            u
        };
        // Proxies cannot forward an upgrade reliably, so ws:// also uses a CONNECT tunnel.
        let proxy = opts.proxy.for_target(bare_host, true);
        let conn = net::connect(
            &Target {
                host: bare_host,
                port,
                tls: secure,
                alpn: Alpn::Http1,
                tls_options: &opts.tls,
                proxy,
                force_tunnel: true,
                connect_timeout: opts.connect_timeout,
            },
            &self.tls,
        )
        .await?;

        let mut builder = http::Request::builder().method("GET").uri(url.as_str());
        let mut sent = Vec::new();
        let user_host = req.headers.iter().find(|h| h.name.eq_ignore_ascii_case("host")).map(|h| h.value.clone());
        let host_value = user_host.unwrap_or_else(|| match url.port() {
            Some(p) => format!("{host}:{p}"),
            None => host.to_string(),
        });
        let key = tungstenite::handshake::client::generate_key();
        let mut headers = vec![
            Header::new("Host", host_value),
            Header::new("Connection", "Upgrade"),
            Header::new("Upgrade", "websocket"),
            Header::new("Sec-WebSocket-Version", "13"),
            Header::new("Sec-WebSocket-Key", key),
        ];
        let reserved =
            ["host", "connection", "upgrade", "sec-websocket-version", "sec-websocket-key", "content-length"];
        for h in req.headers.iter().filter(|h| !h.name.trim().is_empty()) {
            if !reserved.iter().any(|r| h.name.trim().eq_ignore_ascii_case(r)) {
                headers.push(Header::new(h.name.trim(), h.value.clone()));
            }
        }
        if opts.default_headers && !headers.iter().any(|h| h.name.eq_ignore_ascii_case("user-agent")) {
            headers.push(Header::new("User-Agent", crate::http::USER_AGENT));
        }
        if let Some(cookies) = jar.and_then(|j| j.header_for(&http_url))
            && !headers.iter().any(|h| h.name.eq_ignore_ascii_case("cookie"))
        {
            headers.push(Header::new("Cookie", cookies));
        }
        for h in headers {
            let name = http::HeaderName::from_bytes(h.name.as_bytes())
                .map_err(|_| EngineError::invalid(format!("Invalid header name '{}'", h.name)))?;
            let value = http::HeaderValue::from_bytes(h.value.as_bytes())
                .map_err(|_| EngineError::invalid(format!("Invalid value for header '{}'", h.name)))?;
            builder = builder.header(name, value);
            sent.push(h);
        }
        let request = builder.body(()).map_err(|e| EngineError::invalid(format!("Invalid request: {e}")))?;
        let config = WebSocketConfig::default().max_message_size(Some(64 << 20)).max_frame_size(Some(16 << 20));

        let remote_addr = conn.remote_addr.to_string();
        let tls = conn.tls.clone();
        let conn_timing = conn.timing;
        let handshake_started = Instant::now();
        let handshake = tokio_tungstenite::client_async_with_config(request, conn.stream, Some(config));
        let handshake = match opts.timeout {
            Some(limit) => tokio::time::timeout(limit, handshake)
                .await
                .map_err(|_| EngineError::timeout("WebSocket handshake", limit))?,
            None => handshake.await,
        };
        let (stream, response) = handshake.map_err(handshake_error)?;
        let ttfb = handshake_started.elapsed();

        let resp_headers: Vec<Header> = response
            .headers()
            .iter()
            .map(|(n, v)| Header::new(canonical_name(n.as_str()), String::from_utf8_lossy(v.as_bytes())))
            .collect();
        if let Some(jar) = jar {
            let set: Vec<String> = response
                .headers()
                .get_all("set-cookie")
                .iter()
                .map(|v| String::from_utf8_lossy(v.as_bytes()).into_owned())
                .collect();
            jar.store(&http_url, &set);
        }
        let meta = ResponseMeta {
            status: response.status().as_u16(),
            status_text: response.status().canonical_reason().unwrap_or_default().to_string(),
            http_version: "HTTP/1.1".into(),
            headers_size: resp_headers.iter().map(|h| (h.name.len() + h.value.len() + 4) as u64).sum(),
            headers: resp_headers,
            url: url.to_string(),
            remote_addr: Some(remote_addr),
            tls,
            redirects: Vec::new(),
            request: SentRequest {
                method: "GET".into(),
                url: url[..Position::AfterQuery].to_string(),
                http_version: "HTTP/1.1".into(),
                headers: sent,
                body_size: 0,
                proxy: proxy.map(|p| format!("{}:{}", p.host, p.port)),
            },
            cookies: Vec::new(),
        };
        let timing = Timing {
            dns_ms: conn_timing.dns.as_secs_f64() * 1000.0,
            connect_ms: conn_timing.connect.as_secs_f64() * 1000.0,
            tls_ms: conn_timing.tls.as_secs_f64() * 1000.0,
            ttfb_ms: ttfb.as_secs_f64() * 1000.0,
            total_ms: started.elapsed().as_secs_f64() * 1000.0,
            ..Default::default()
        };

        let (out_tx, out_rx) = mpsc::unbounded_channel();
        let (ev_tx, ev_rx) = mpsc::unbounded_channel();
        tokio::spawn(run_session(stream, out_rx, ev_tx, CLOSE_TIMEOUT));
        Ok(WsConnected { meta, timing, session: WsSession { tx: out_tx }, events: ev_rx })
    }
}

fn handshake_error(err: tungstenite::Error) -> EngineError {
    match err {
        tungstenite::Error::Http(resp) => {
            let body = resp.body().as_deref().map(|b| String::from_utf8_lossy(b).chars().take(500).collect::<String>());
            let mut msg = format!(
                "Server rejected the WebSocket upgrade: {} {}",
                resp.status().as_u16(),
                resp.status().canonical_reason().unwrap_or_default()
            );
            if let Some(body) = body.filter(|b| !b.trim().is_empty()) {
                msg.push_str(&format!(" — {}", body.trim()));
            }
            EngineError::new(ErrorKind::Protocol, msg)
        }
        tungstenite::Error::Io(e) => EngineError::new(ErrorKind::Io, format!("WebSocket I/O error: {e}")),
        other => EngineError::new(ErrorKind::Protocol, format!("WebSocket handshake failed: {other}")),
    }
}

fn message_event(direction: Direction, msg: &Message) -> Option<WsEvent> {
    let b64 = |b: &[u8]| Some(base64::engine::general_purpose::STANDARD.encode(b));
    let (kind, text, base64, size) = match msg {
        Message::Text(t) => (WsMessageKind::Text, Some(t.to_string()), None, t.len()),
        Message::Binary(b) => (WsMessageKind::Binary, None, b64(b), b.len()),
        Message::Ping(b) => (WsMessageKind::Ping, None, b64(b), b.len()),
        Message::Pong(b) => (WsMessageKind::Pong, None, b64(b), b.len()),
        _ => return None,
    };
    Some(WsEvent::Message { direction, kind, text, base64, size: size as u64, timestamp: now_ms() })
}

/// How long to wait for the server to answer our close frame before dropping
/// the connection (otherwise a silent server keeps the socket and task alive).
const CLOSE_TIMEOUT: Duration = Duration::from_secs(5);

async fn run_session<S>(
    stream: tokio_tungstenite::WebSocketStream<S>,
    mut outgoing: mpsc::UnboundedReceiver<WsOutgoing>,
    events: mpsc::UnboundedSender<WsEvent>,
    close_timeout: Duration,
) where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let (mut sink, mut source) = stream.split();
    let mut closing_by_client = false;
    let close_deadline = tokio::time::sleep(Duration::ZERO);
    tokio::pin!(close_deadline);
    loop {
        tokio::select! {
            _ = &mut close_deadline, if closing_by_client => {
                let _ = events.send(WsEvent::Closed {
                    code: None,
                    reason: "Server did not answer the close request".into(),
                    by_client: true,
                });
                return;
            }
            out = outgoing.recv() => {
                let msg = match out {
                    Some(WsOutgoing::Text { text }) => Message::text(text),
                    Some(WsOutgoing::Binary { base64 }) => {
                        let bytes = base64::engine::general_purpose::STANDARD.decode(base64).unwrap_or_default();
                        Message::binary(bytes)
                    }
                    Some(WsOutgoing::Ping) => Message::Ping(bytes::Bytes::from_static(b"zorvik")),
                    Some(WsOutgoing::Close { code, reason }) => {
                        closing_by_client = true;
                        close_deadline.as_mut().reset(tokio::time::Instant::now() + close_timeout);
                        Message::Close(Some(CloseFrame {
                            code: CloseCode::from(code.unwrap_or(1000)),
                            reason: reason.unwrap_or_default().into(),
                        }))
                    }
                    None => {
                        // Session handle dropped: close politely (unless the peer stopped reading) and stop.
                        let _ = tokio::time::timeout(close_timeout, sink.send(Message::Close(None))).await;
                        return;
                    }
                };
                let event = message_event(Direction::Sent, &msg);
                if let Err(e) = sink.send(msg).await {
                    let _ = events.send(WsEvent::Error { message: format!("Send failed: {e}") });
                    let _ = events.send(WsEvent::Closed { code: None, reason: "Connection lost".into(), by_client: false });
                    return;
                }
                if let Some(event) = event {
                    let _ = events.send(event);
                }
            }
            incoming = source.next() => match incoming {
                Some(Ok(Message::Close(frame))) => {
                    let (code, reason) = frame.map(|f| (Some(u16::from(f.code)), f.reason.to_string())).unwrap_or((None, String::new()));
                    // tungstenite replies to the close handshake while flushing.
                    let _ = sink.flush().await;
                    let _ = events.send(WsEvent::Closed { code, reason, by_client: closing_by_client });
                    return;
                }
                Some(Ok(msg)) => {
                    if let Some(event) = message_event(Direction::Received, &msg) {
                        let _ = events.send(event);
                    }
                }
                Some(Err(e)) => {
                    let reason = match &e {
                        tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed => "Connection closed".to_string(),
                        // After we sent Close, the peer dropping the socket is the expected end.
                        _ if closing_by_client => "Connection closed".to_string(),
                        tungstenite::Error::Protocol(tungstenite::error::ProtocolError::ResetWithoutClosingHandshake) => {
                            "Server closed the connection without a close frame".to_string()
                        }
                        other => {
                            let _ = events.send(WsEvent::Error { message: other.to_string() });
                            "Connection lost".to_string()
                        }
                    };
                    let _ = events.send(WsEvent::Closed { code: None, reason, by_client: closing_by_client });
                    return;
                }
                None => {
                    let _ = events.send(WsEvent::Closed { code: None, reason: "Connection closed".into(), by_client: closing_by_client });
                    return;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ws_url_normalization() {
        assert_eq!(normalize_ws_url("localhost:8080/ws").unwrap().as_str(), "ws://localhost:8080/ws");
        assert_eq!(normalize_ws_url("https://h/x").unwrap().scheme(), "wss");
        assert!(normalize_ws_url("ftp://h").is_err());
    }

    #[tokio::test]
    async fn client_close_ends_session_when_server_never_answers() {
        // The server end stays open but never reads, so no close reply arrives.
        let (client, _server) = tokio::io::duplex(4096);
        let stream =
            tokio_tungstenite::WebSocketStream::from_raw_socket(client, tungstenite::protocol::Role::Client, None)
                .await;
        let (out_tx, out_rx) = mpsc::unbounded_channel();
        let (ev_tx, mut ev_rx) = mpsc::unbounded_channel();
        let task = tokio::spawn(run_session(stream, out_rx, ev_tx, Duration::from_millis(100)));
        out_tx.send(WsOutgoing::Close { code: Some(1000), reason: None }).unwrap();
        let event = tokio::time::timeout(Duration::from_secs(5), ev_rx.recv()).await.expect("closed in time");
        assert!(matches!(event, Some(WsEvent::Closed { by_client: true, .. })), "{event:?}");
        tokio::time::timeout(Duration::from_secs(5), task).await.expect("session task ends").unwrap();
    }
}
