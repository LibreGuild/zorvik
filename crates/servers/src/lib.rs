//! Servers Zorvik runs for the user: one listener per saved [`Server`],
//! reporting its traffic through a [`Reporter`]. `zorvik-api` owns the running
//! servers (start/stop, logs, events); this crate only knows sockets.
//!
//! Every kind follows the same contract:
//! - `run(listener, ctx)` serves until `ctx.cancel` fires, then returns `Ok(())`.
//!   Tasks it spawns (one per connection) must stop on `ctx.cancel` too.
//! - Configuration changes arrive through `ctx.live` and apply to the next
//!   message/request; host, port, TLS and kind changes need a restart.
//! - `ctx.control` carries sends/disconnects from the UI.

mod connections;
mod dns;
mod http;
mod relay;
pub mod report;
pub mod rules;
mod sse;
mod tcp;
mod template;
pub mod tls;
mod udp;
mod ws;

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::panic::AssertUnwindSafe;
use std::path::PathBuf;
use std::sync::Arc;

use futures_util::FutureExt as _;
use futures_util::future::BoxFuture;
use tokio::sync::{mpsc, oneshot, watch};
use tokio_util::sync::CancellationToken;
use zorvik_engine::tools::{Protocol, port_owner};
use zorvik_engine::{Client, RequestOptions};
use zorvik_formats::{Server, ServerKind};
use zorvik_workspace::vars::VarContext;

pub use report::{
    HttpExchange, OutgoingMessage, Reporter, ServerEvent, ServerStats, TrafficDirection, TrafficEntry, TrafficKind,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerError {
    pub message: String,
}

impl ServerError {
    pub fn new(message: impl Into<String>) -> Self {
        Self { message: message.into() }
    }
}

impl std::fmt::Display for ServerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

/// Things servers need from the app besides their own configuration.
#[derive(Clone)]
pub struct StartOptions {
    /// Workspace root: relative certificate paths are resolved against it.
    pub base_dir: PathBuf,
    /// HTTP client for proxy fallbacks and upstream lookups.
    pub client: Arc<Client>,
    /// Proxy/TLS/timeout settings for that client.
    pub request_options: RequestOptions,
}

/// The configuration a server currently runs with.
pub struct Live {
    pub server: Server,
    /// Environment and workspace variables for templates (`{{name}}`).
    pub vars: VarContext,
}

/// From the UI to a running server.
pub enum Control {
    /// Send to one connection, or to all when `conn` is `None`. Answers with the
    /// number of connections it went to.
    Send { conn: Option<u64>, message: OutgoingMessage, reply: oneshot::Sender<Result<usize, String>> },
    /// Close one connection.
    Disconnect { conn: u64 },
}

/// What a server kind's `run` gets.
pub(crate) struct Ctx {
    pub live: watch::Receiver<Arc<Live>>,
    pub control: mpsc::UnboundedReceiver<Control>,
    pub cancel: CancellationToken,
    pub reporter: Reporter,
    /// Set when the server listens with TLS (ALPN chosen per kind).
    pub tls: Option<Arc<rustls::ServerConfig>>,
    #[allow(dead_code)] // used by the kinds that talk to other servers (proxy fallback, DNS upstream, relay)
    pub options: Arc<StartOptions>,
}

/// Handle to a running server. Dropping it stops the server.
pub struct RunningServer {
    pub addr: SocketAddr,
    pub url: String,
    live: watch::Sender<Arc<Live>>,
    control: mpsc::UnboundedSender<Control>,
    cancel: CancellationToken,
}

impl RunningServer {
    /// Apply a changed configuration. Returns `false` (and changes nothing) when
    /// the kind, address, port or TLS changed: those need a restart.
    pub fn update(&self, server: Server, vars: VarContext) -> bool {
        let same_listener = {
            let current = &self.live.borrow().server;
            current.kind == server.kind
                && current.host == server.host
                && current.port == server.port
                && current.tls == server.tls
        };
        if same_listener {
            self.live.send_replace(Arc::new(Live { server, vars }));
        }
        same_listener
    }

    /// Send a message to one client or all of them; returns how many got it.
    pub async fn send(&self, conn: Option<u64>, message: OutgoingMessage) -> Result<usize, ServerError> {
        let (reply, answer) = oneshot::channel();
        self.control
            .send(Control::Send { conn, message, reply })
            .map_err(|_| ServerError::new("The server is not running"))?;
        answer.await.map_err(|_| ServerError::new("The server is not running"))?.map_err(ServerError::new)
    }

    pub fn disconnect(&self, conn: u64) {
        let _ = self.control.send(Control::Disconnect { conn });
    }

    pub fn stop(&self) {
        self.cancel.cancel();
    }
}

impl Drop for RunningServer {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

/// ALPN protocols offered by a TLS listener of this kind.
fn alpn(kind: ServerKind) -> &'static [&'static [u8]] {
    match kind {
        ServerKind::Http | ServerKind::Sse => &[b"h2", b"http/1.1"],
        ServerKind::Websocket => &[b"http/1.1"],
        _ => &[],
    }
}

/// The address clients use (`0.0.0.0` becomes `127.0.0.1` for display).
pub fn display_url(kind: ServerKind, tls: bool, addr: SocketAddr) -> String {
    let ip = match addr.ip() {
        IpAddr::V4(ip) if ip.is_unspecified() => IpAddr::V4(Ipv4Addr::LOCALHOST),
        IpAddr::V6(ip) if ip.is_unspecified() => IpAddr::V6(Ipv6Addr::LOCALHOST),
        ip => ip,
    };
    let host = SocketAddr::new(ip, addr.port());
    let scheme = match (kind, tls) {
        (ServerKind::Http | ServerKind::Sse, false) => "http",
        (ServerKind::Http | ServerKind::Sse, true) => "https",
        (ServerKind::Websocket, false) => "ws",
        (ServerKind::Websocket, true) => "wss",
        (ServerKind::Tcp | ServerKind::TcpProxy, false) => "tcp",
        (ServerKind::Tcp | ServerKind::TcpProxy, true) => "tls",
        (ServerKind::Udp, _) => "udp",
        (ServerKind::Dns, _) => "dns",
    };
    format!("{scheme}://{host}")
}

/// Why a listener couldn't bind, naming the program that holds a taken port when the
/// OS says which one.
async fn bind_error(e: std::io::Error, host: &str, port: u16, protocol: Protocol) -> ServerError {
    use std::io::ErrorKind as K;
    let owner = match e.kind() {
        K::AddrInUse => tokio::task::spawn_blocking(move || port_owner(port, protocol)).await.ok().flatten(),
        _ => None,
    };
    ServerError::new(match e.kind() {
        K::AddrInUse => match owner {
            Some(owner) if owner.pid == std::process::id() => {
                format!("Port {port} is already in use by another server in Zorvik. Stop it or pick another port.")
            }
            Some(owner) => format!("Port {port} is already in use by {owner}. Stop it or pick another port."),
            None => format!("Port {port} is already in use by another server or app. Stop it or pick another port."),
        },
        K::PermissionDenied => {
            format!("Not allowed to listen on port {port} (ports below 1024 may need administrator rights).")
        }
        K::AddrNotAvailable => format!("This computer has no address {host}. Use 127.0.0.1 or 0.0.0.0."),
        _ => format!("Could not listen on {host}:{port}: {e}"),
    })
}

/// Start a server. Its traffic goes to `reporter`; when it ends (stopped or
/// failed) the reporter gets [`ServerEvent::Stopped`].
pub async fn start(
    server: Server,
    vars: VarContext,
    options: StartOptions,
    reporter: Reporter,
) -> Result<RunningServer, ServerError> {
    let host = server.host.trim().to_string();
    if host.is_empty() {
        return Err(ServerError::new("Enter an address to listen on (127.0.0.1 or 0.0.0.0)"));
    }
    let port = server.port;
    let kind = server.kind;
    // Settings a server can't run without fail here, before anything listens.
    if kind == ServerKind::TcpProxy {
        relay::parse_target(&server.proxy).map_err(ServerError::new)?;
    }
    let tls = if server.tls.enabled {
        if !kind.supports_tls() {
            return Err(ServerError::new("TLS is not available for this kind of server"));
        }
        Some(tls::server_config(&server.tls, &options.base_dir, alpn(kind))?)
    } else {
        None
    };
    let (live_tx, live_rx) = watch::channel(Arc::new(Live { server, vars }));
    let (control_tx, control_rx) = mpsc::unbounded_channel();
    let cancel = CancellationToken::new();
    let ctx = Ctx {
        live: live_rx,
        control: control_rx,
        cancel: cancel.clone(),
        reporter: reporter.clone(),
        tls: tls.clone(),
        options: Arc::new(options),
    };
    let (addr, run): (SocketAddr, BoxFuture<'static, Result<(), String>>) = match kind {
        ServerKind::Udp | ServerKind::Dns => {
            // DNS on any free port: one free for TCP too (DNS over TCP uses the same number).
            let bound = if kind == ServerKind::Dns && port == 0 {
                dns::bind_both(&host).await.map(|(udp, tcp)| (udp, Some(tcp)))
            } else {
                tokio::net::UdpSocket::bind((host.as_str(), port)).await.map(|udp| (udp, None))
            };
            let (socket, tcp) = match bound {
                Ok(bound) => bound,
                Err(e) => return Err(bind_error(e, &host, port, Protocol::Udp).await),
            };
            let addr = socket.local_addr().map_err(|e| ServerError::new(e.to_string()))?;
            let run = match kind {
                ServerKind::Udp => Box::pin(udp::run(socket, ctx)) as BoxFuture<'static, _>,
                _ => Box::pin(dns::run(socket, tcp, ctx)),
            };
            (addr, run)
        }
        _ => {
            let listener = match tokio::net::TcpListener::bind((host.as_str(), port)).await {
                Ok(listener) => listener,
                Err(e) => return Err(bind_error(e, &host, port, Protocol::Tcp).await),
            };
            let addr = listener.local_addr().map_err(|e| ServerError::new(e.to_string()))?;
            let run = match kind {
                ServerKind::Http => Box::pin(http::run(listener, ctx)) as BoxFuture<'static, _>,
                ServerKind::Websocket => Box::pin(ws::run(listener, ctx)),
                ServerKind::Sse => Box::pin(sse::run(listener, ctx)),
                ServerKind::Tcp => Box::pin(tcp::run(listener, ctx)),
                _ => Box::pin(relay::run(listener, ctx)),
            };
            (addr, run)
        }
    };
    let url = display_url(kind, tls.is_some(), addr);
    let token = cancel.clone();
    tokio::spawn(async move {
        // A bug must not leave the server looking alive: a panic stops it like an error.
        let error = tokio::select! {
            result = AssertUnwindSafe(run).catch_unwind() => match result {
                Ok(result) => result.err(),
                Err(panic) => Some(format!("internal error ({})", panic_message(panic.as_ref()))),
            },
            _ = token.cancelled() => None,
        };
        // Tell every connection task to stop too (also when `run` failed).
        token.cancel();
        if let Some(e) = &error {
            reporter.error(None, None, format!("Server stopped: {e}"));
        }
        reporter.stopped(error);
    });
    Ok(RunningServer { addr, url, live: live_tx, control: control_tx, cancel })
}

fn panic_message(panic: &(dyn std::any::Any + Send)) -> &str {
    panic
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| panic.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("panic")
}

/// Bytes of an outgoing text/binary message (`None` with an error for SSE events).
pub(crate) fn message_bytes(message: &OutgoingMessage) -> Result<(Vec<u8>, bool), String> {
    use base64::Engine as _;
    match message {
        OutgoingMessage::Text { text } => Ok((text.as_bytes().to_vec(), true)),
        OutgoingMessage::Binary { base64 } => base64::engine::general_purpose::STANDARD
            .decode(base64)
            .map(|b| (b, false))
            .map_err(|e| format!("Binary payload is not valid base64: {e}")),
        OutgoingMessage::Event { data, .. } => Ok((data.as_bytes().to_vec(), true)),
    }
}
