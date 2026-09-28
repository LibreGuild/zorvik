//! Zorvik protocol engine.
//!
//! Pure networking: no persistence, no UI. Callers pass fully resolved
//! requests and options; see `docs/architecture.md`.

pub mod cookies;
mod decode;
pub mod dns;
pub mod error;
pub mod framing;
pub mod grpc;
mod h3;
pub mod http;
pub mod mqtt;
mod net;
pub mod pool;
pub mod proxy;
pub mod relay_connect;
pub mod socket;
pub mod sse;
pub mod tls;
pub mod tools;
pub mod udp;
pub mod ws;

pub use cookies::{CookieInfo, CookieJar};
pub use decode::pretty_json;
pub use dns::{DnsFlags, DnsQuery, DnsQuestion, DnsResolver, DnsResult, DnsResultRecord};
pub use error::{EngineError, ErrorKind, Result};
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
