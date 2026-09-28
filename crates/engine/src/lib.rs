//! Zorvik protocol engine.
//!
//! Pure networking: no persistence, no UI. Callers pass fully resolved
//! requests and options; see `docs/architecture.md`.

pub mod cookies;
mod decode;
pub mod dns;
pub mod error;
pub mod framing;
pub mod graphql;
pub mod grpc;
mod h3;
pub mod http;
pub mod mcp;
pub mod mqtt;
mod net;
pub mod pool;
pub mod proxy;
pub mod relay_connect;
pub mod socket;
pub mod socketio;
pub mod sse;
pub mod tls;
pub mod tools;
pub mod udp;
pub mod ws;

pub use cookies::{CookieInfo, CookieJar};
pub use decode::pretty_json;
pub use dns::{DnsFlags, DnsQuery, DnsQuestion, DnsResolver, DnsResult, DnsResultRecord};
pub use error::{EngineError, ErrorKind, Result};
pub use graphql::GraphqlWsProtocol;
pub use grpc::{GrpcDescriptors, GrpcEvent, GrpcResponse, GrpcSession, GrpcStatus, GrpcTarget};
pub use http::{
    BodyStream, ChallengeAuth, ChallengeAuthRef, Client, Header, HostGuard, HttpRequest, HttpResponse, HttpVersionPref,
    RedirectHop, RequestOptions, ResponseMeta, SentRequest, StreamingResponse, Timing,
};
pub use mqtt::{MqttConfig, MqttProtocol};
pub use proxy::{ProxyMode, ProxySettings};
pub use relay_connect::{RawConnection, RawStream};
pub use socket::{SocketConfig, SocketConnected, SocketEvent, SocketOpened, SocketOutgoing, SocketSession};
pub use sse::{SseEvent, SseParser};
pub use tls::{CertificateInfo, TlsInfo, TlsOptions};
pub use ws::{Direction, WsConnected, WsEvent, WsMessageKind, WsOutgoing, WsSession};

/// Allow this process more open files than a GUI app starts with on macOS (256), for servers
/// and load tests with many connections. Best effort; call once at startup.
pub fn raise_open_file_limit() {
    #[cfg(unix)]
    {
        let mut limit = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
        // SAFETY: getrlimit and setrlimit only read and write the struct passed to them.
        unsafe {
            if libc::getrlimit(libc::RLIMIT_NOFILE, &mut limit) != 0 {
                return;
            }
            // macOS refuses more than its per-process maximum: try smaller values.
            for want in [65_536, 24_576, 10_240, 4_096] {
                if limit.rlim_cur >= want {
                    return;
                }
                let next = libc::rlimit { rlim_cur: want.min(limit.rlim_max), rlim_max: limit.rlim_max };
                if libc::setrlimit(libc::RLIMIT_NOFILE, &next) == 0 {
                    return;
                }
            }
        }
    }
}
