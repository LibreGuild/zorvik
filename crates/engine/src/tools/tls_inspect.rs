//! TLS inspector: what a server's TLS looks like from here.
//!
//! One full handshake records the certificate chain with a verifier that
//! accepts any chain, while the OS verifier (as used for requests) runs on the
//! side to report whether the chain is trusted and why not. Then extra
//! handshakes probe the protocol versions and each cipher suite of the ring
//! provider, a few at a time. rustls implements TLS 1.2 and 1.3 only, so
//! TLS 1.0/1.1 support can't be tested.

use std::io;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::CryptoProvider;
use rustls::version::{TLS12, TLS13};
use rustls::{
    CertificateError, ClientConfig, DigitallySignedStruct, SignatureScheme, SupportedCipherSuite,
    SupportedProtocolVersion,
};
use rustls_pki_types::{CertificateDer, ServerName, UnixTime};
use serde::Serialize;
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use tokio::net::TcpStream;
use ts_rs::TS;

use super::ms;
use crate::error::{EngineError, ErrorKind, Result};
use crate::net::{self, Stream, Target};
use crate::proxy::{ProxyEndpoint, ProxySettings};
use crate::tls::{self, Alpn, TlsConfigCache, TlsOptions};

#[derive(Debug, Clone)]
pub struct TlsInspectOptions {
    /// Name sent in SNI and checked against the certificate (default: the host).
    pub sni: Option<String>,
    /// HTTP proxy settings; a CONNECT tunnel is used when one applies to the host.
    pub proxy: ProxySettings,
    /// Extra trusted root CA certificates (PEM), like Settings > Certificates.
    pub ca_cert_path: Option<PathBuf>,
    /// TCP connect + first handshake.
    pub connect_timeout: Duration,
    /// Each version / cipher suite probe.
    pub probe_timeout: Duration,
    /// The whole inspection; probes not started in time are reported as skipped.
    pub total_timeout: Duration,
    /// Probe handshakes in flight at once.
    pub probe_concurrency: usize,
    /// Probe protocol versions and cipher suites (one connection each).
    pub probe: bool,
}

impl Default for TlsInspectOptions {
    fn default() -> Self {
        Self {
            sni: None,
            proxy: ProxySettings::default(),
            ca_cert_path: None,
            connect_timeout: Duration::from_secs(10),
            probe_timeout: Duration::from_secs(5),
            total_timeout: Duration::from_secs(45),
            probe_concurrency: 4,
            probe: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TlsReport {
    pub host: String,
    pub port: u16,
    /// Name sent in SNI (none for IP addresses) and checked against the certificate.
    pub server_name: String,
    /// Address connected to (the proxy's when tunnelling).
    pub remote_addr: String,
    /// `host:port` of the HTTP proxy used, if any.
    pub proxy: Option<String>,
    pub version: String,
    pub cipher: String,
    /// Key exchange group, e.g. `X25519`.
    pub key_exchange: Option<String>,
    pub alpn: Option<String>,
    /// The OS trust store (plus the CA from Settings) accepts the chain for this name.
    pub trusted: bool,
    /// Why the chain is not trusted.
    pub trust_error: Option<String>,
    pub hostname_matches: bool,
    /// The server stapled an OCSP response.
    pub ocsp_stapled: bool,
    /// Certificates as sent by the server, the server's own first.
    pub chain: Vec<TlsCertificate>,
    /// TLS 1.3 … TLS 1.0; `supported` is `None` when not tested.
    pub versions: Vec<TlsVersionSupport>,
    pub ciphers: Vec<TlsCipherSupport>,
    /// Most severe first.
    pub warnings: Vec<TlsWarning>,
    /// TCP connect + TLS handshake of the first connection.
    pub handshake_ms: f64,
    pub duration_ms: f64,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TlsCertificate {
    pub subject: String,
    pub common_name: Option<String>,
    pub issuer: String,
    pub issuer_common_name: Option<String>,
    /// RFC 3339.
    pub not_before: String,
    pub not_after: String,
    /// Whole days until `not_after` (negative once expired).
    #[ts(type = "number")]
    pub days_left: i64,
    pub not_yet_valid: bool,
    pub subject_alt_names: Vec<String>,
    /// `RSA`, `EC`, `Ed25519`, …
    pub key_type: String,
    pub key_bits: Option<u32>,
    /// EC curve, e.g. `P-256`.
    pub key_curve: Option<String>,
    pub signature_algorithm: String,
    /// SHA-256 of the DER certificate, `AB:CD:…`.
    pub sha256: String,
    pub serial: String,
    pub is_ca: bool,
    pub self_signed: bool,
    /// Set when the certificate could not be parsed (other fields are then empty).
    pub parse_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TlsVersionSupport {
    /// `TLS 1.3`, `TLS 1.2`, `TLS 1.1`, `TLS 1.0`.
    pub version: String,
    /// `None`: not tested or no answer.
    pub supported: Option<bool>,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TlsCipherSupport {
    /// rustls name, e.g. `TLS13_AES_128_GCM_SHA256`.
    pub name: String,
    pub version: String,
    /// `None`: no answer (timeout, connection error, time limit).
    pub accepted: Option<bool>,
    pub forward_secrecy: bool,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum TlsWarningLevel {
    Danger,
    Warning,
    Info,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TlsWarning {
    pub level: TlsWarningLevel,
    /// Stable id: `untrusted`, `expired`, `notYetValid`, `hostnameMismatch`,
    /// `chainExpired`, `expiresSoon`, `selfSigned`, `weakKey`, `weakSignature`,
    /// `noForwardSecrecy`, `noTls13`.
    pub code: String,
    pub message: String,
}

/// Inspect the TLS server at `host` (which may include a port or be a URL)
/// and `port` (used when the host has none; 0 = 443).
pub async fn inspect_tls(host: &str, port: u16, options: &TlsInspectOptions) -> Result<TlsReport> {
    let started = Instant::now();
    let deadline = started + options.total_timeout;
    let (host, host_port) = super::parse_target(host)?;
    let port = host_port.or((port != 0).then_some(port)).unwrap_or(443);
    let name = options.sni.as_deref().map(str::trim).filter(|s| !s.is_empty()).unwrap_or(&host).to_string();
    let server_name = tls::server_name(&name)?;
    let proxy = options.proxy.for_target(&host, true).cloned();
    let provider = Arc::new(rustls::crypto::ring::default_provider());

    // 1. Full handshake: chain, negotiated parameters, trust. ALPN is offered
    //    like a browser does; a server that refuses both protocols (e.g. one
    //    configured for `mqtt` only) is asked again without ALPN.
    let platform = platform_verifier(&provider, options.ca_cert_path.as_ref())?;
    let recorder = Arc::new(Recorder { provider: provider.clone(), platform: Some(platform), seen: Mutex::new(None) });
    let connect_timeout = options.connect_timeout.min(options.total_timeout).max(Duration::from_millis(100));
    let dial = Dial { host: &host, port, proxy: proxy.as_ref(), pinned: None };
    let mut alpn = true;
    let (stream, remote_addr) = loop {
        let config = client_config(provider.clone(), &[&TLS13, &TLS12], recorder.clone(), alpn)?;
        let (tcp, remote_addr) = dial.connect(connect_timeout).await?;
        let left = connect_timeout.saturating_sub(started.elapsed()).max(Duration::from_millis(1));
        match tokio::time::timeout(left, tokio_rustls::TlsConnector::from(config).connect(server_name.clone(), tcp))
            .await
        {
            Err(_) => return Err(EngineError::timeout("TLS handshake", connect_timeout)),
            Ok(Ok(stream)) => break (stream, remote_addr),
            Ok(Err(e)) if alpn && is_alert(&e, rustls::AlertDescription::NoApplicationProtocol) => alpn = false,
            Ok(Err(e)) => return Err(main_handshake_error(e, &host, port)),
        }
    };
    let handshake_ms = ms(started.elapsed());
    let session = stream.get_ref().1;
    let info = tls::connection_info(session);
    let key_exchange = session.negotiated_key_exchange_group().map(|g| format!("{:?}", g.name()));
    let negotiated = session.negotiated_cipher_suite();
    drop(stream);
    let recorded =
        recorder.take().ok_or_else(|| EngineError::new(ErrorKind::Tls, "The server did not present a certificate"))?;

    let now = OffsetDateTime::now_utc();
    let chain: Vec<TlsCertificate> = recorded.chain.iter().map(|c| describe(c, now)).collect();
    let hostname_matches = recorded.chain.first().is_some_and(|leaf| {
        rustls::server::ParsedCertificate::try_from(leaf)
            .and_then(|parsed| rustls::client::verify_server_name(&parsed, &server_name))
            .is_ok()
    });
    let trust_error = match &recorded.trust {
        Ok(()) => None,
        Err(e) => Some(trust_message(e, &name)),
    };

    // 2. Probes: versions and cipher suites, pinned to the same server address
    //    (unless tunnelling, where the proxy resolves the name).
    let (versions, ciphers) = if options.probe {
        let pinned = match (&proxy, remote_addr) {
            (None, SocketAddr::V4(_)) => Some(remote_addr),
            (None, SocketAddr::V6(v6)) if v6.scope_id() == 0 => Some(remote_addr),
            _ => None,
        };
        let dial = Dial { host: &host, port, proxy: proxy.as_ref(), pinned };
        let probe =
            ProbeContext { dial: &dial, server_name: &server_name, provider: &provider, alpn, options, deadline };
        probe_all(&probe).await
    } else {
        (untested_versions(), Vec::new())
    };

    let mut report = TlsReport {
        host: host.clone(),
        port,
        server_name: name,
        remote_addr: remote_addr.to_string(),
        proxy: proxy.as_ref().map(|p| format!("{}:{}", p.host, p.port)),
        version: info.version,
        cipher: info.cipher,
        key_exchange,
        alpn: info.alpn,
        trusted: trust_error.is_none(),
        trust_error,
        hostname_matches,
        ocsp_stapled: !recorded.ocsp.is_empty(),
        chain,
        versions,
        ciphers,
        warnings: Vec::new(),
        handshake_ms,
        duration_ms: 0.0,
    };
    report.warnings = warnings(&report, negotiated);
    report.duration_ms = ms(started.elapsed());
    Ok(report)
}

// ---- connections ------------------------------------------------------------

struct Dial<'a> {
    host: &'a str,
    port: u16,
    proxy: Option<&'a ProxyEndpoint>,
    /// Connect straight to this address (no DNS) when there is no proxy.
    pinned: Option<SocketAddr>,
}

impl Dial<'_> {
    async fn connect(&self, timeout: Duration) -> Result<(TcpStream, SocketAddr)> {
        if let (None, Some(addr)) = (self.proxy, self.pinned) {
            return match tokio::time::timeout(timeout, TcpStream::connect(addr)).await {
                Ok(Ok(tcp)) => Ok((tcp, addr)),
                Ok(Err(e)) => Err(EngineError::new(ErrorKind::Connect, format!("Could not connect to {addr}: {e}"))),
                Err(_) => Err(EngineError::timeout("Connection", timeout)),
            };
        }
        let options = TlsOptions::default();
        let target = Target {
            host: self.host,
            port: self.port,
            tls: false,
            alpn: Alpn::None,
            tls_options: &options,
            proxy: self.proxy,
            force_tunnel: true,
            connect_timeout: timeout,
        };
        let conn = net::connect(&target, &TlsConfigCache::default()).await?;
        match conn.stream {
            Stream::Plain(tcp) => {
                let _ = tcp.set_nodelay(true);
                Ok((tcp, conn.remote_addr))
            }
            Stream::Tls(_) => Err(EngineError::new(ErrorKind::Io, "Unexpected TLS stream")),
        }
    }
}

fn main_handshake_error(err: io::Error, host: &str, port: u16) -> EngineError {
    if let Some(tls_err) = err.get_ref().and_then(|e| e.downcast_ref::<rustls::Error>()) {
        let hint = match tls_err {
            rustls::Error::AlertReceived(_) | rustls::Error::PeerIncompatible(_) => {
                " The server may only support TLS 1.0/1.1 (not testable here) or need a client certificate."
            }
            rustls::Error::InvalidMessage(_) => " Is this port speaking TLS?",
            _ => "",
        };
        return EngineError::new(ErrorKind::Tls, format!("TLS handshake with {host}:{port} failed: {tls_err}.{hint}"));
    }
    match err.kind() {
        io::ErrorKind::UnexpectedEof | io::ErrorKind::ConnectionReset | io::ErrorKind::ConnectionAborted => {
            EngineError::new(
                ErrorKind::Tls,
                format!(
                    "{host}:{port} closed the connection during the TLS handshake. It may not speak TLS on this port, or only support TLS 1.0/1.1."
                ),
            )
        }
        _ => EngineError::new(ErrorKind::Tls, format!("TLS handshake with {host}:{port} failed: {err}")),
    }
}

fn is_alert(err: &io::Error, alert: rustls::AlertDescription) -> bool {
    matches!(
        err.get_ref().and_then(|e| e.downcast_ref::<rustls::Error>()),
        Some(rustls::Error::AlertReceived(a)) if *a == alert
    )
}

fn client_config(
    provider: Arc<CryptoProvider>,
    versions: &[&'static SupportedProtocolVersion],
    verifier: Arc<dyn ServerCertVerifier>,
    alpn: bool,
) -> Result<Arc<ClientConfig>> {
    let mut config = ClientConfig::builder_with_provider(provider)
        .with_protocol_versions(versions)
        .map_err(|e| EngineError::new(ErrorKind::Tls, format!("TLS setup failed: {e}")))?
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth();
    if alpn {
        config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    }
    // Every probe is a fresh, full handshake.
    config.resumption = rustls::client::Resumption::disabled();
    Ok(Arc::new(config))
}

fn platform_verifier(provider: &Arc<CryptoProvider>, ca: Option<&PathBuf>) -> Result<Arc<dyn ServerCertVerifier>> {
    let extra = match ca {
        Some(path) => tls::read_certs(path, "CA certificate")?,
        None => Vec::new(),
    };
    let verifier = if extra.is_empty() {
        rustls_platform_verifier::Verifier::new(provider.clone())
    } else {
        rustls_platform_verifier::Verifier::new_with_extra_roots(extra, provider.clone())
    }
    .map_err(|e| EngineError::new(ErrorKind::Tls, format!("Could not load trusted certificates: {e}")))?;
    Ok(Arc::new(verifier))
}

struct Recorded {
    chain: Vec<CertificateDer<'static>>,
    ocsp: Vec<u8>,
    trust: std::result::Result<(), rustls::Error>,
}

impl std::fmt::Debug for Recorded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Recorded({} certificates)", self.chain.len())
    }
}

/// Accepts any chain (so broken servers can be inspected) but records it and,
/// when `platform` is set, the OS verifier's verdict. Handshake signatures are
/// still checked, so the peer really holds the certificate's key.
#[derive(Debug)]
struct Recorder {
    provider: Arc<CryptoProvider>,
    platform: Option<Arc<dyn ServerCertVerifier>>,
    seen: Mutex<Option<Recorded>>,
}

impl Recorder {
    fn take(&self) -> Option<Recorded> {
        self.seen.lock().unwrap_or_else(|e| e.into_inner()).take()
    }
}

impl ServerCertVerifier for Recorder {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        ocsp_response: &[u8],
        now: UnixTime,
    ) -> std::result::Result<ServerCertVerified, rustls::Error> {
        if let Some(platform) = &self.platform {
            let trust =
                platform.verify_server_cert(end_entity, intermediates, server_name, ocsp_response, now).map(|_| ());
            let mut chain = Vec::with_capacity(intermediates.len() + 1);
            chain.push(end_entity.clone().into_owned());
            // Bounded: a chain longer than this is nonsense and would bloat the report.
            chain.extend(intermediates.iter().take(16).map(|c| c.clone().into_owned()));
            let recorded = Recorded { chain, ocsp: ocsp_response.to_vec(), trust };
            *self.seen.lock().unwrap_or_else(|e| e.into_inner()) = Some(recorded);
        }
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider.signature_verification_algorithms.supported_schemes()
    }
}

fn trust_message(err: &rustls::Error, name: &str) -> String {
    match err {
        rustls::Error::InvalidCertificate(e) => match e {
            CertificateError::UnknownIssuer => {
                "Not issued by a trusted certificate authority (unknown issuer or incomplete chain)".into()
            }
            CertificateError::Expired | CertificateError::ExpiredContext { .. } => "The certificate has expired".into(),
            CertificateError::NotValidYet | CertificateError::NotValidYetContext { .. } => {
                "The certificate is not valid yet".into()
            }
            CertificateError::NotValidForName | CertificateError::NotValidForNameContext { .. } => {
                format!("The certificate is not valid for {name}")
            }
            CertificateError::Revoked => "The certificate has been revoked".into(),
            CertificateError::BadSignature => "A certificate signature is invalid".into(),
            CertificateError::InvalidPurpose | CertificateError::InvalidPurposeContext { .. } => {
                "The certificate is not meant for TLS servers".into()
            }
            other => format!("{other:?}"),
        },
        other => other.to_string(),
    }
}

// ---- probes -----------------------------------------------------------------

enum ProbeOutcome {
    Accepted,
    /// The server refused (alert, or closed the connection).
    Rejected(String),
    /// No verdict (connection failed, timeout, time limit).
    Unknown(String),
}

#[derive(Clone, Copy)]
enum Probe {
    Version(&'static SupportedProtocolVersion),
    Suite(SupportedCipherSuite),
}

fn version_name(v: &SupportedProtocolVersion) -> &'static str {
    if v.version == rustls::ProtocolVersion::TLSv1_3 { "TLS 1.3" } else { "TLS 1.2" }
}

fn untested_versions() -> Vec<TlsVersionSupport> {
    let mut list: Vec<TlsVersionSupport> = ["TLS 1.3", "TLS 1.2"]
        .iter()
        .map(|v| TlsVersionSupport { version: v.to_string(), supported: None, detail: Some("Not tested".into()) })
        .collect();
    list.extend(legacy_versions());
    list
}

fn legacy_versions() -> Vec<TlsVersionSupport> {
    ["TLS 1.1", "TLS 1.0"]
        .iter()
        .map(|v| TlsVersionSupport {
            version: v.to_string(),
            supported: None,
            detail: Some("Can't be tested: the TLS library (rustls) doesn't implement TLS 1.0/1.1".into()),
        })
        .collect()
}

/// What every probe shares.
struct ProbeContext<'a> {
    dial: &'a Dial<'a>,
    server_name: &'a ServerName<'static>,
    provider: &'a Arc<CryptoProvider>,
    /// Offer ALPN (off when the server refused ours).
    alpn: bool,
    options: &'a TlsInspectOptions,
    deadline: Instant,
}

async fn probe_all(cx: &ProbeContext<'_>) -> (Vec<TlsVersionSupport>, Vec<TlsCipherSupport>) {
    let suites = rustls::crypto::ring::ALL_CIPHER_SUITES;
    let mut probes: Vec<Probe> = vec![Probe::Version(&TLS13), Probe::Version(&TLS12)];
    probes.extend(suites.iter().map(|s| Probe::Suite(*s)));

    let mut outcomes: Vec<(usize, ProbeOutcome)> = futures_util::stream::iter(probes.iter().copied().enumerate())
        .map(|(i, probe)| async move { (i, run_probe(cx, probe).await) })
        .buffer_unordered(cx.options.probe_concurrency.max(1))
        .collect()
        .await;
    outcomes.sort_by_key(|(i, _)| *i);

    let mut versions = Vec::new();
    let mut ciphers = Vec::new();
    for ((_, outcome), probe) in outcomes.into_iter().zip(&probes) {
        let (supported, detail) = match outcome {
            ProbeOutcome::Accepted => (Some(true), None),
            ProbeOutcome::Rejected(d) => (Some(false), Some(d)),
            ProbeOutcome::Unknown(d) => (None, Some(d)),
        };
        match probe {
            Probe::Version(v) => {
                versions.push(TlsVersionSupport { version: version_name(v).into(), supported, detail })
            }
            Probe::Suite(s) => {
                let name = format!("{:?}", s.suite());
                let tls13 = s.version().version == rustls::ProtocolVersion::TLSv1_3;
                ciphers.push(TlsCipherSupport {
                    forward_secrecy: tls13 || name.contains("DHE_"),
                    version: version_name(s.version()).into(),
                    name,
                    accepted: supported,
                    detail,
                });
            }
        }
    }
    versions.extend(legacy_versions());
    (versions, ciphers)
}

async fn run_probe(cx: &ProbeContext<'_>, probe: Probe) -> ProbeOutcome {
    let remaining = cx.deadline.saturating_duration_since(Instant::now());
    if remaining < Duration::from_millis(50) {
        return ProbeOutcome::Unknown("Skipped: the time limit was reached".into());
    }
    let timeout = cx.options.probe_timeout.min(remaining);
    let (provider, versions): (Arc<CryptoProvider>, Vec<&'static SupportedProtocolVersion>) = match probe {
        Probe::Version(v) => (cx.provider.clone(), vec![v]),
        Probe::Suite(s) => {
            let only = CryptoProvider { cipher_suites: vec![s], ..rustls::crypto::ring::default_provider() };
            (Arc::new(only), vec![s.version()])
        }
    };
    let recorder = Arc::new(Recorder { provider: provider.clone(), platform: None, seen: Mutex::new(None) });
    let config = match client_config(provider, &versions, recorder, cx.alpn) {
        Ok(c) => c,
        Err(e) => return ProbeOutcome::Unknown(e.message),
    };
    let attempt = async {
        let (tcp, _) = cx.dial.connect(timeout).await.map_err(|e| ProbeOutcome::Unknown(e.message))?;
        tokio_rustls::TlsConnector::from(config)
            .connect(cx.server_name.clone(), tcp)
            .await
            .map_err(|e| classify_probe_error(&e))?;
        Ok::<_, ProbeOutcome>(())
    };
    match tokio::time::timeout(timeout, attempt).await {
        Ok(Ok(())) => ProbeOutcome::Accepted,
        Ok(Err(outcome)) => outcome,
        Err(_) => ProbeOutcome::Unknown(format!("No answer within {}", crate::error::human_duration(timeout))),
    }
}

fn classify_probe_error(err: &io::Error) -> ProbeOutcome {
    if let Some(tls_err) = err.get_ref().and_then(|e| e.downcast_ref::<rustls::Error>()) {
        return ProbeOutcome::Rejected(match tls_err {
            rustls::Error::AlertReceived(alert) => format!("Refused by the server ({alert:?})"),
            other => other.to_string(),
        });
    }
    match err.kind() {
        io::ErrorKind::UnexpectedEof | io::ErrorKind::ConnectionReset | io::ErrorKind::ConnectionAborted => {
            ProbeOutcome::Rejected("The server closed the connection".into())
        }
        _ => ProbeOutcome::Unknown(err.to_string()),
    }
}

// ---- certificates -------------------------------------------------------------

fn describe(der: &CertificateDer<'_>, now: OffsetDateTime) -> TlsCertificate {
    use x509_parser::prelude::*;
    let sha256 = colon_hex(&Sha256::digest(der.as_ref()));
    let cert = match X509Certificate::from_der(der.as_ref()) {
        Ok((_, cert)) => cert,
        Err(e) => {
            return TlsCertificate {
                subject: String::new(),
                common_name: None,
                issuer: String::new(),
                issuer_common_name: None,
                not_before: String::new(),
                not_after: String::new(),
                days_left: 0,
                not_yet_valid: false,
                subject_alt_names: Vec::new(),
                key_type: String::new(),
                key_bits: None,
                key_curve: None,
                signature_algorithm: String::new(),
                sha256,
                serial: String::new(),
                is_ca: false,
                self_signed: false,
                parse_error: Some(format!("Could not parse the certificate: {e}")),
            };
        }
    };
    let fmt_time = |t: &ASN1Time| {
        t.to_datetime().format(&::time::format_description::well_known::Rfc3339).unwrap_or_else(|_| t.to_string())
    };
    let common_name =
        |name: &X509Name<'_>| name.iter_common_name().next().and_then(|cn| cn.as_str().ok()).map(str::to_string);
    let validity = cert.validity();
    let not_after = validity.not_after.to_datetime();
    let days_left = (not_after - now).whole_seconds().div_euclid(86_400);
    let (key_type, key_bits, key_curve) = key_info(cert.public_key());
    TlsCertificate {
        subject: cert.subject().to_string(),
        common_name: common_name(cert.subject()),
        issuer: cert.issuer().to_string(),
        issuer_common_name: common_name(cert.issuer()),
        not_before: fmt_time(&validity.not_before),
        not_after: fmt_time(&validity.not_after),
        days_left,
        not_yet_valid: validity.not_before.to_datetime() > now,
        subject_alt_names: tls::subject_alt_names(&cert),
        key_type,
        key_bits,
        key_curve,
        signature_algorithm: signature_name(&cert.signature_algorithm.algorithm.to_id_string()),
        sha256,
        serial: cert.raw_serial_as_string(),
        is_ca: cert.is_ca(),
        self_signed: is_self_signed(&cert),
        parse_error: None,
    }
}

fn is_self_signed(cert: &x509_parser::certificate::X509Certificate<'_>) -> bool {
    use x509_parser::extensions::ParsedExtension;
    if cert.subject().as_raw() != cert.issuer().as_raw() {
        return false;
    }
    // Same name: also require matching key identifiers when both are present.
    let mut ski = None;
    let mut aki = None;
    for ext in cert.extensions() {
        match ext.parsed_extension() {
            ParsedExtension::SubjectKeyIdentifier(id) => ski = Some(id.0),
            ParsedExtension::AuthorityKeyIdentifier(a) => aki = a.key_identifier.as_ref().map(|id| id.0),
            _ => {}
        }
    }
    match (ski, aki) {
        (Some(s), Some(a)) => s == a,
        _ => true,
    }
}

fn key_info(spki: &x509_parser::x509::SubjectPublicKeyInfo<'_>) -> (String, Option<u32>, Option<String>) {
    use x509_parser::public_key::PublicKey;
    let algorithm = spki.algorithm.algorithm.to_id_string();
    match spki.parsed() {
        Ok(PublicKey::RSA(rsa)) => ("RSA".into(), rsa_bits(rsa.modulus), None),
        Ok(PublicKey::EC(ec)) => {
            let curve = spki
                .algorithm
                .parameters
                .as_ref()
                .and_then(|p| p.as_oid().ok())
                .map(|oid| curve_name(&oid.to_id_string()));
            let bits = match curve.as_deref() {
                Some("P-256") => Some(256),
                Some("P-384") => Some(384),
                Some("P-521") => Some(521),
                _ => (ec.key_size() > 0).then(|| ec.key_size() as u32),
            };
            ("EC".into(), bits, curve)
        }
        Ok(PublicKey::DSA(y)) => ("DSA".into(), Some((y.len() * 8) as u32), None),
        _ => match algorithm.as_str() {
            "1.3.101.112" => ("Ed25519".into(), Some(256), None),
            "1.3.101.113" => ("Ed448".into(), Some(456), None),
            other => (other.to_string(), None, None),
        },
    }
}

/// Bit length of an RSA modulus given as DER integer bytes. x509-parser's
/// `key_size` rounds down to whole bytes and says 0 when there is no leading
/// zero byte, which would be reported as a "0-bit" weak key.
fn rsa_bits(modulus: &[u8]) -> Option<u32> {
    let first = modulus.iter().position(|&b| b != 0)?;
    let bits = (modulus.len() - first) * 8 - modulus[first].leading_zeros() as usize;
    u32::try_from(bits).ok()
}

fn curve_name(oid: &str) -> String {
    match oid {
        "1.2.840.10045.3.1.7" => "P-256".into(),
        "1.3.132.0.34" => "P-384".into(),
        "1.3.132.0.35" => "P-521".into(),
        "1.3.132.0.10" => "secp256k1".into(),
        other => other.into(),
    }
}

fn signature_name(oid: &str) -> String {
    match oid {
        "1.2.840.113549.1.1.4" => "MD5 with RSA",
        "1.2.840.113549.1.1.5" => "SHA-1 with RSA",
        "1.2.840.113549.1.1.11" => "SHA-256 with RSA",
        "1.2.840.113549.1.1.12" => "SHA-384 with RSA",
        "1.2.840.113549.1.1.13" => "SHA-512 with RSA",
        "1.2.840.113549.1.1.10" => "RSA-PSS",
        "1.2.840.10045.4.1" => "ECDSA with SHA-1",
        "1.2.840.10045.4.3.2" => "ECDSA with SHA-256",
        "1.2.840.10045.4.3.3" => "ECDSA with SHA-384",
        "1.2.840.10045.4.3.4" => "ECDSA with SHA-512",
        "1.3.101.112" => "Ed25519",
        "1.3.101.113" => "Ed448",
        other => return other.to_string(),
    }
    .to_string()
}

fn colon_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(":")
}

// ---- warnings -------------------------------------------------------------------

fn warnings(report: &TlsReport, negotiated: Option<SupportedCipherSuite>) -> Vec<TlsWarning> {
    let mut out = Vec::new();
    let mut add = |level, code: &str, message: String| out.push(TlsWarning { level, code: code.into(), message });
    let label = |c: &TlsCertificate| c.common_name.clone().unwrap_or_else(|| c.subject.clone());
    let date = |rfc3339: &str| rfc3339.split('T').next().unwrap_or(rfc3339).to_string();

    if let Some(reason) = &report.trust_error {
        add(TlsWarningLevel::Danger, "untrusted", format!("Not trusted. {reason}."));
    }
    if let Some(leaf) = report.chain.first().filter(|c| c.parse_error.is_none()) {
        if leaf.days_left < 0 {
            add(TlsWarningLevel::Danger, "expired", format!("The certificate expired on {}.", date(&leaf.not_after)));
        } else if leaf.not_yet_valid {
            add(
                TlsWarningLevel::Danger,
                "notYetValid",
                format!("The certificate is not valid until {}.", date(&leaf.not_before)),
            );
        } else if leaf.days_left < 30 {
            add(
                TlsWarningLevel::Warning,
                "expiresSoon",
                format!(
                    "The certificate expires in {} day{} ({}).",
                    leaf.days_left,
                    plural(leaf.days_left),
                    date(&leaf.not_after)
                ),
            );
        }
        if !report.hostname_matches {
            let names = if leaf.subject_alt_names.is_empty() {
                "it lists no names".to_string()
            } else {
                let mut shown: Vec<&str> = leaf.subject_alt_names.iter().take(5).map(String::as_str).collect();
                if leaf.subject_alt_names.len() > 5 {
                    shown.push("…");
                }
                format!("it covers {}", shown.join(", "))
            };
            add(
                TlsWarningLevel::Danger,
                "hostnameMismatch",
                format!("The certificate is not valid for {} ({names}).", report.server_name),
            );
        }
        if leaf.self_signed {
            add(TlsWarningLevel::Warning, "selfSigned", "The certificate is self-signed.".into());
        }
    }
    for cert in report.chain.iter().skip(1).filter(|c| c.parse_error.is_none() && !c.self_signed) {
        if cert.days_left < 0 {
            add(
                TlsWarningLevel::Danger,
                "chainExpired",
                format!("Chain certificate '{}' expired on {}.", label(cert), date(&cert.not_after)),
            );
        } else if cert.days_left < 30 {
            add(
                TlsWarningLevel::Warning,
                "expiresSoon",
                format!(
                    "Chain certificate '{}' expires in {} day{}.",
                    label(cert),
                    cert.days_left,
                    plural(cert.days_left)
                ),
            );
        }
    }
    for cert in report.chain.iter().filter(|c| c.parse_error.is_none()) {
        let weak = match (cert.key_type.as_str(), cert.key_bits) {
            ("RSA" | "DSA", Some(bits)) if bits < 2048 => Some(format!("a {bits}-bit {} key", cert.key_type)),
            ("EC", Some(bits)) if bits < 256 => Some(format!("a {bits}-bit EC key")),
            _ => None,
        };
        if let Some(weak) = weak {
            add(
                TlsWarningLevel::Warning,
                "weakKey",
                format!("'{}' uses {weak} (use RSA 2048+ or ECDSA P-256+).", label(cert)),
            );
        }
        // A root's own signature is never checked, so only flag the others.
        let root = cert.self_signed && cert.is_ca;
        if !root && (cert.signature_algorithm.contains("SHA-1") || cert.signature_algorithm.contains("MD5")) {
            add(
                TlsWarningLevel::Warning,
                "weakSignature",
                format!("'{}' is signed with {}, which is no longer secure.", label(cert), cert.signature_algorithm),
            );
        }
    }
    if let Some(suite) = negotiated {
        let name = format!("{:?}", suite.suite());
        if suite.version().version == rustls::ProtocolVersion::TLSv1_2 && !name.contains("DHE_") {
            add(
                TlsWarningLevel::Warning,
                "noForwardSecrecy",
                format!("The connection uses {name}, which has no forward secrecy."),
            );
        }
    }
    let tls13 = report.version == "TLS 1.3"
        || report.versions.iter().any(|v| v.version == "TLS 1.3" && v.supported == Some(true));
    let no_tls13 = report.versions.iter().any(|v| v.version == "TLS 1.3" && v.supported == Some(false));
    if !tls13 && no_tls13 {
        add(TlsWarningLevel::Info, "noTls13", "The server doesn't support TLS 1.3.".into());
    }
    out.sort_by_key(|w| w.level);
    out
}

fn plural(n: i64) -> &'static str {
    if n == 1 { "" } else { "s" }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_signature_algorithms_and_curves() {
        assert_eq!(signature_name("1.2.840.113549.1.1.11"), "SHA-256 with RSA");
        assert_eq!(signature_name("1.2.3"), "1.2.3");
        assert_eq!(curve_name("1.2.840.10045.3.1.7"), "P-256");
        assert_eq!(colon_hex(&[0xab, 0x01]), "AB:01");
    }

    #[test]
    fn rsa_key_bits_from_the_modulus() {
        // 2048 bits: a leading zero byte keeps the DER integer positive.
        let mut m2048 = vec![0u8, 0x80];
        m2048.extend_from_slice(&[0xff; 255]);
        assert_eq!(rsa_bits(&m2048), Some(2048));
        // 2047 bits needs no leading zero; x509-parser would say 2040.
        let mut m2047 = vec![0x7f];
        m2047.extend_from_slice(&[0xff; 255]);
        assert_eq!(rsa_bits(&m2047), Some(2047));
        // Top bit set without a leading zero (x509-parser: 0).
        assert_eq!(rsa_bits(&[0xff; 128]), Some(1024));
        assert_eq!(rsa_bits(&[0, 0]), None);
        assert_eq!(rsa_bits(&[]), None);
    }

    fn cert(key_type: &str, bits: u32, signature: &str, days_left: i64) -> TlsCertificate {
        TlsCertificate {
            subject: "CN=example.com".into(),
            common_name: Some("example.com".into()),
            issuer: "CN=Some CA".into(),
            issuer_common_name: Some("Some CA".into()),
            not_before: "2020-01-01T00:00:00Z".into(),
            not_after: "2030-01-01T00:00:00Z".into(),
            days_left,
            not_yet_valid: false,
            subject_alt_names: vec!["example.com".into()],
            key_type: key_type.into(),
            key_bits: Some(bits),
            key_curve: None,
            signature_algorithm: signature.into(),
            sha256: String::new(),
            serial: String::new(),
            is_ca: false,
            self_signed: false,
            parse_error: None,
        }
    }

    fn report(chain: Vec<TlsCertificate>, versions: Vec<TlsVersionSupport>) -> TlsReport {
        TlsReport {
            host: "example.com".into(),
            port: 443,
            server_name: "example.com".into(),
            remote_addr: "192.0.2.1:443".into(),
            proxy: None,
            version: "TLS 1.2".into(),
            cipher: String::new(),
            key_exchange: None,
            alpn: None,
            trusted: true,
            trust_error: None,
            hostname_matches: true,
            ocsp_stapled: false,
            chain,
            versions,
            ciphers: Vec::new(),
            warnings: Vec::new(),
            handshake_ms: 0.0,
            duration_ms: 0.0,
        }
    }

    #[test]
    fn warns_about_weak_keys_signatures_and_missing_tls13() {
        let mut intermediate = cert("RSA", 2048, "SHA-1 with RSA", 12);
        intermediate.common_name = Some("Old CA".into());
        let mut root = cert("RSA", 1024, "SHA-1 with RSA", 900);
        root.is_ca = true;
        root.self_signed = true;
        let versions = vec![TlsVersionSupport { version: "TLS 1.3".into(), supported: Some(false), detail: None }];
        let r = report(vec![cert("RSA", 1024, "SHA-256 with RSA", 200), intermediate, root], versions);
        let w = warnings(&r, None);
        let codes: Vec<&str> = w.iter().map(|w| w.code.as_str()).collect();
        // Weak key on the leaf and the root; SHA-1 only flagged where it is checked (not the root).
        assert_eq!(codes.iter().filter(|c| **c == "weakKey").count(), 2);
        assert_eq!(codes.iter().filter(|c| **c == "weakSignature").count(), 1);
        assert!(codes.contains(&"expiresSoon"));
        assert_eq!(codes.last(), Some(&"noTls13"));
        assert!(w.windows(2).all(|p| p[0].level <= p[1].level));
    }

    #[test]
    fn untrusted_and_mismatched_are_dangers_first() {
        let mut r = report(vec![cert("EC", 256, "ECDSA with SHA-256", 3)], Vec::new());
        r.trusted = false;
        r.trust_error = Some("unknown issuer".into());
        r.hostname_matches = false;
        let w = warnings(&r, None);
        assert_eq!(w[0].code, "untrusted");
        assert_eq!(w[1].code, "hostnameMismatch");
        assert!(w[1].message.contains("example.com"));
        assert_eq!(w[2].level, TlsWarningLevel::Warning);
        // Not tested is not "unsupported".
        assert!(!w.iter().any(|w| w.code == "noTls13"));
    }

    #[test]
    fn unparsable_certificates_are_reported_not_fatal() {
        let garbage = CertificateDer::from(vec![0x30, 0x03, 0x02, 0x01]);
        let c = describe(&garbage, OffsetDateTime::now_utc());
        assert!(c.parse_error.is_some());
        assert_eq!(c.sha256.len(), 32 * 3 - 1);
    }
}
