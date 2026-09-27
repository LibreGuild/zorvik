//! Network tools against local servers only: TLS inspector (rustls servers
//! with rcgen certificates), port check, ping and interfaces.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use rcgen::{
    BasicConstraints, CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair, KeyUsagePurpose,
};
use rustls::crypto::ring::cipher_suite;
use rustls::version::{TLS12, TLS13};
use rustls::{SupportedCipherSuite, SupportedProtocolVersion};
use rustls_pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;
use zorvik_engine::proxy::ProxyEndpoint;
use zorvik_engine::tools::{
    PingMode, PingOptions, Pinger, PortErrorKind, PortScan, PortScanOptions, TlsInspectOptions, TlsWarningLevel,
    inspect_tls, list_interfaces, parse_ports,
};
use zorvik_engine::{ErrorKind, ProxySettings};
use zorvik_testkit::TestProxy;

// ---- certificates and servers ---------------------------------------------------

struct Leaf {
    chain: Vec<CertificateDer<'static>>,
    key: PrivateKeyDer<'static>,
    ca_pem: Option<String>,
}

fn days(n: i64) -> time::Duration {
    time::Duration::days(n)
}

/// A CA-issued leaf for localhost / 127.0.0.1, valid from `from` to `until` days from now.
fn ca_issued(from: i64, until: i64, include_ca: bool) -> Leaf {
    let now = time::OffsetDateTime::now_utc();
    let ca_key = KeyPair::generate().unwrap();
    let mut ca = CertificateParams::new(Vec::<String>::new()).unwrap();
    ca.distinguished_name.push(DnType::CommonName, "Tools Test CA");
    ca.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    ca.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign, KeyUsagePurpose::DigitalSignature];
    ca.not_before = now - days(2);
    ca.not_after = now + days(365);
    let ca_cert = ca.self_signed(&ca_key).unwrap();
    let issuer = Issuer::new(ca, ca_key);

    let key = KeyPair::generate().unwrap();
    let mut leaf = CertificateParams::new(vec!["localhost".to_string(), "127.0.0.1".to_string()]).unwrap();
    leaf.distinguished_name.push(DnType::CommonName, "localhost");
    leaf.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    leaf.key_usages = vec![KeyUsagePurpose::DigitalSignature, KeyUsagePurpose::KeyEncipherment];
    leaf.not_before = now + days(from);
    leaf.not_after = now + days(until);
    let cert = leaf.signed_by(&key, &issuer).unwrap();
    let mut chain = vec![cert.der().clone()];
    if include_ca {
        chain.push(ca_cert.der().clone());
    }
    Leaf { chain, key: PrivatePkcs8KeyDer::from(key.serialize_der()).into(), ca_pem: Some(ca_cert.pem()) }
}

fn self_signed(from: i64, until: i64) -> Leaf {
    let now = time::OffsetDateTime::now_utc();
    let key = KeyPair::generate().unwrap();
    let mut params = CertificateParams::new(vec!["localhost".to_string()]).unwrap();
    params.distinguished_name.push(DnType::CommonName, "self.local");
    params.not_before = now + days(from);
    params.not_after = now + days(until);
    let cert = params.self_signed(&key).unwrap();
    Leaf { chain: vec![cert.der().clone()], key: PrivatePkcs8KeyDer::from(key.serialize_der()).into(), ca_pem: None }
}

struct TlsServer {
    addr: SocketAddr,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for TlsServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn tls_server(
    leaf: &Leaf,
    versions: &[&'static SupportedProtocolVersion],
    suites: Option<Vec<SupportedCipherSuite>>,
) -> TlsServer {
    tls_server_with_alpn(leaf, versions, suites, vec![b"h2".to_vec(), b"http/1.1".to_vec()]).await
}

async fn tls_server_with_alpn(
    leaf: &Leaf,
    versions: &[&'static SupportedProtocolVersion],
    suites: Option<Vec<SupportedCipherSuite>>,
    alpn: Vec<Vec<u8>>,
) -> TlsServer {
    let mut provider = rustls::crypto::ring::default_provider();
    if let Some(suites) = suites {
        provider.cipher_suites = suites;
    }
    let mut config = rustls::ServerConfig::builder_with_provider(Arc::new(provider))
        .with_protocol_versions(versions)
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(leaf.chain.clone(), leaf.key.clone_key())
        .unwrap();
    config.alpn_protocols = alpn;
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        loop {
            let Ok((tcp, _)) = listener.accept().await else { continue };
            let acceptor = acceptor.clone();
            tokio::spawn(async move {
                if let Ok(mut stream) = acceptor.accept(tcp).await {
                    let _ = stream.shutdown().await;
                }
            });
        }
    });
    TlsServer { addr, task }
}

fn has_warning(report: &zorvik_engine::tools::TlsReport, code: &str) -> bool {
    report.warnings.iter().any(|w| w.code == code)
}

// ---- TLS inspector ----------------------------------------------------------------

#[tokio::test]
async fn inspects_a_modern_server_and_probes_suites() {
    let leaf = ca_issued(-1, 90, true);
    // Only two suites, so the probe results are exact.
    let suites = vec![cipher_suite::TLS13_AES_128_GCM_SHA256, cipher_suite::TLS_ECDHE_ECDSA_WITH_AES_256_GCM_SHA384];
    let server = tls_server(&leaf, &[&TLS13, &TLS12], Some(suites)).await;
    let dir = tempfile::tempdir().unwrap();
    let ca_path = dir.path().join("ca.pem");
    std::fs::write(&ca_path, leaf.ca_pem.as_ref().unwrap()).unwrap();

    let options = TlsInspectOptions { ca_cert_path: Some(ca_path), ..Default::default() };
    let report = inspect_tls("localhost", server.addr.port(), &options).await.unwrap();

    assert_eq!(report.version, "TLS 1.3");
    assert_eq!(report.cipher, "TLS13_AES_128_GCM_SHA256");
    assert_eq!(report.alpn.as_deref(), Some("h2"));
    assert!(report.key_exchange.is_some());
    assert!(report.trusted, "trusted via the extra CA: {:?}", report.trust_error);
    assert!(report.hostname_matches);
    assert!(!report.ocsp_stapled);
    assert!(report.proxy.is_none());

    assert_eq!(report.chain.len(), 2);
    let cert = &report.chain[0];
    assert_eq!(cert.common_name.as_deref(), Some("localhost"));
    assert_eq!(cert.issuer_common_name.as_deref(), Some("Tools Test CA"));
    assert_eq!(cert.key_type, "EC");
    assert_eq!(cert.key_curve.as_deref(), Some("P-256"));
    assert_eq!(cert.key_bits, Some(256));
    assert_eq!(cert.signature_algorithm, "ECDSA with SHA-256");
    assert!(cert.subject_alt_names.contains(&"127.0.0.1".to_string()));
    assert!((88..=90).contains(&cert.days_left), "days left {}", cert.days_left);
    assert!(!cert.is_ca && !cert.self_signed);
    let expected: String = {
        use sha2::Digest as _;
        sha2::Sha256::digest(leaf.chain[0].as_ref()).iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(":")
    };
    assert_eq!(cert.sha256, expected);
    let root = &report.chain[1];
    assert!(root.is_ca && root.self_signed);

    let supported = |v: &str| report.versions.iter().find(|x| x.version == v).unwrap().supported;
    assert_eq!(supported("TLS 1.3"), Some(true));
    assert_eq!(supported("TLS 1.2"), Some(true));
    assert_eq!(supported("TLS 1.1"), None);
    assert_eq!(supported("TLS 1.0"), None);

    let accepted = |name: &str| report.ciphers.iter().find(|c| c.name == name).unwrap().accepted;
    assert_eq!(report.ciphers.len(), rustls::crypto::ring::ALL_CIPHER_SUITES.len());
    assert_eq!(accepted("TLS13_AES_128_GCM_SHA256"), Some(true));
    assert_eq!(accepted("TLS13_AES_256_GCM_SHA384"), Some(false));
    assert_eq!(accepted("TLS_ECDHE_ECDSA_WITH_AES_256_GCM_SHA384"), Some(true));
    assert_eq!(accepted("TLS_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256"), Some(false));
    assert_eq!(accepted("TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256"), Some(false));
    assert!(report.ciphers.iter().all(|c| c.forward_secrecy));
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
}

#[tokio::test]
async fn untrusted_without_the_ca_and_mismatched_sni() {
    let leaf = ca_issued(-1, 90, false);
    let server = tls_server(&leaf, &[&TLS13, &TLS12], None).await;
    let options = TlsInspectOptions { sni: Some("other.example".into()), probe: false, ..Default::default() };
    let report = inspect_tls(&format!("127.0.0.1:{}", server.addr.port()), 0, &options).await.unwrap();
    assert_eq!(report.server_name, "other.example");
    assert!(!report.trusted);
    assert!(report.trust_error.is_some());
    assert!(!report.hostname_matches);
    assert!(has_warning(&report, "untrusted"));
    assert!(has_warning(&report, "hostnameMismatch"));
    assert_eq!(report.warnings[0].level, TlsWarningLevel::Danger);
    // Probes were skipped: versions are listed but not tested.
    assert!(report.ciphers.is_empty());
    assert!(report.versions.iter().all(|v| v.supported.is_none()));
}

#[tokio::test]
async fn tls12_only_server() {
    let leaf = ca_issued(-1, 90, false);
    let server = tls_server(&leaf, &[&TLS12], None).await;
    let report = inspect_tls("127.0.0.1", server.addr.port(), &TlsInspectOptions::default()).await.unwrap();
    assert_eq!(report.version, "TLS 1.2");
    assert!(report.cipher.starts_with("TLS_ECDHE_ECDSA_"));
    let supported = |v: &str| report.versions.iter().find(|x| x.version == v).unwrap().supported;
    assert_eq!(supported("TLS 1.3"), Some(false));
    assert_eq!(supported("TLS 1.2"), Some(true));
    assert!(report.ciphers.iter().filter(|c| c.version == "TLS 1.3").all(|c| c.accepted == Some(false)));
    assert!(report.ciphers.iter().any(|c| c.version == "TLS 1.2" && c.accepted == Some(true)));
    assert!(has_warning(&report, "noTls13"));
    assert!(!has_warning(&report, "noForwardSecrecy"));
}

#[tokio::test]
async fn expired_self_signed_certificate() {
    let leaf = self_signed(-60, -3);
    let server = tls_server(&leaf, &[&TLS13, &TLS12], None).await;
    let options = TlsInspectOptions { probe: false, ..Default::default() };
    let report = inspect_tls("localhost", server.addr.port(), &options).await.unwrap();
    let cert = &report.chain[0];
    assert!(cert.self_signed);
    assert!(cert.days_left <= -3 && cert.days_left >= -4, "days left {}", cert.days_left);
    assert!(!report.trusted);
    assert!(has_warning(&report, "expired"));
    assert!(has_warning(&report, "selfSigned"));
    assert!(has_warning(&report, "untrusted"));
    assert!(!has_warning(&report, "expiresSoon"));
}

#[tokio::test]
async fn soon_expiring_and_not_yet_valid_certificates() {
    let soon = ca_issued(-1, 10, false);
    let server = tls_server(&soon, &[&TLS13], None).await;
    let options = TlsInspectOptions { probe: false, ..Default::default() };
    let report = inspect_tls("localhost", server.addr.port(), &options).await.unwrap();
    assert!(has_warning(&report, "expiresSoon"));
    assert!(!has_warning(&report, "expired"));

    let future = ca_issued(5, 50, false);
    let server = tls_server(&future, &[&TLS13], None).await;
    let report = inspect_tls("localhost", server.addr.port(), &options).await.unwrap();
    assert!(report.chain[0].not_yet_valid);
    assert!(has_warning(&report, "notYetValid"));
}

#[tokio::test]
async fn retries_without_alpn_when_the_server_refuses_ours() {
    let leaf = ca_issued(-1, 90, false);
    let server = tls_server_with_alpn(&leaf, &[&TLS13, &TLS12], None, vec![b"mqtt".to_vec()]).await;
    let report = inspect_tls("127.0.0.1", server.addr.port(), &TlsInspectOptions::default()).await.unwrap();
    assert!(report.alpn.is_none());
    assert_eq!(report.versions[0].supported, Some(true));
    assert!(report.ciphers.iter().any(|c| c.accepted == Some(true)));
}

#[tokio::test]
async fn inspects_through_a_proxy_tunnel() {
    let leaf = ca_issued(-1, 90, false);
    let server = tls_server(&leaf, &[&TLS13, &TLS12], None).await;
    // The proxy sends every tunnel to the server, so a made-up name works.
    let proxy = TestProxy::start_with_upstream(None, Some(server.addr)).await;
    let endpoint = ProxyEndpoint::parse(&proxy.url()).unwrap();
    let options = TlsInspectOptions {
        proxy: ProxySettings { http: Some(endpoint.clone()), https: Some(endpoint), bypass: Vec::new() },
        ..Default::default()
    };
    let report = inspect_tls("https://tls-inspect.test/", 0, &options).await.unwrap();
    assert_eq!(report.port, 443);
    assert_eq!(report.proxy.as_deref(), Some(proxy.addr.to_string().as_str()));
    assert_eq!(report.remote_addr, proxy.addr.to_string());
    assert!(!report.hostname_matches);
    assert!(report.ciphers.iter().any(|c| c.accepted == Some(true)));
    // First handshake + 2 version probes + one per suite.
    let expected = 1 + 2 + rustls::crypto::ring::ALL_CIPHER_SUITES.len() as u64;
    assert_eq!(proxy.hits.load(std::sync::atomic::Ordering::SeqCst), expected);
}

#[tokio::test]
async fn handshake_failures_are_explained() {
    // A plain TCP server that closes right away.
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let task = tokio::spawn(async move {
        while let Ok((mut s, _)) = listener.accept().await {
            let _ = s.write_all(b"HTTP/1.1 400 Bad Request\r\n\r\n").await;
        }
    });
    let options = TlsInspectOptions { probe: false, ..Default::default() };
    let err = inspect_tls("127.0.0.1", port, &options).await.unwrap_err();
    assert_eq!(err.kind, ErrorKind::Tls);
    task.abort();

    let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let err = inspect_tls("127.0.0.1", closed, &options).await.unwrap_err();
    assert_eq!(err.kind, ErrorKind::Connect);
    assert!(inspect_tls("", 443, &options).await.is_err());
}

// ---- port check ---------------------------------------------------------------------

#[tokio::test]
async fn port_check_finds_open_and_closed_ports() {
    let a = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let b = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let (pa, pb) = (a.local_addr().unwrap().port(), b.local_addr().unwrap().port());
    let ports = parse_ports(&format!("{pa} {pb},{closed}")).unwrap();
    let scan = PortScan::prepare("localhost", ports, PortScanOptions::default()).await.unwrap();
    assert!(scan.address().is_loopback());
    // localhost may resolve to ::1 first; the listeners are IPv4-only.
    let scan = if scan.address().is_ipv6() {
        PortScan::prepare("127.0.0.1", parse_ports(&format!("{pa},{pb},{closed}")).unwrap(), PortScanOptions::default())
            .await
            .unwrap()
    } else {
        scan
    };
    let mut results = Vec::new();
    let summary = scan.run(&CancellationToken::new(), |r| results.push(r)).await;
    assert_eq!(results.len(), 3);
    let mut open = vec![pa, pb];
    open.sort_unstable();
    assert_eq!(summary.open, open);
    assert_eq!((summary.refused, summary.checked, summary.total), (1, 3, 3));
    let closed_result = results.iter().find(|r| r.port == closed).unwrap();
    assert!(!closed_result.open);
    assert_eq!(closed_result.error, Some(PortErrorKind::Refused));
    // A refused connection is fast (no SYN retries, also on Windows).
    assert!(closed_result.ms < 1500.0, "refused after {} ms", closed_result.ms);
}

#[tokio::test]
async fn port_check_validates_input() {
    assert!(PortScan::prepare("127.0.0.1:80", vec![80], PortScanOptions::default()).await.is_err());
    assert!(PortScan::prepare("127.0.0.1", vec![], PortScanOptions::default()).await.is_err());
    assert!(PortScan::prepare("127.0.0.1", (1..=1025).collect(), PortScanOptions::default()).await.is_err());
    let err = PortScan::prepare("no-such-host.invalid", vec![80], PortScanOptions::default()).await.unwrap_err();
    assert!(matches!(err.kind, ErrorKind::Dns | ErrorKind::Timeout));
}

/// TEST-NET-1 (RFC 5737) is never routed: SYNs go unanswered (timeout) or
/// fail at once when there is no route (other). Either way nothing is open.
#[tokio::test]
async fn port_check_unanswered_ports_time_out_and_cancel() {
    let options = PortScanOptions { timeout: Duration::from_millis(300), concurrency: 8 };
    let scan = PortScan::prepare("192.0.2.1", vec![81, 82], options).await.unwrap();
    let mut results = Vec::new();
    let summary = scan.run(&CancellationToken::new(), |r| results.push(r)).await;
    assert!(summary.open.is_empty());
    assert!(results.iter().all(|r| matches!(r.error, Some(PortErrorKind::Timeout | PortErrorKind::Other))));

    let options = PortScanOptions { timeout: Duration::from_secs(5), concurrency: 1 };
    let scan = PortScan::prepare("192.0.2.1", (1..=1024).collect(), options).await.unwrap();
    let cancel = CancellationToken::new();
    let trigger = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(150)).await;
        trigger.cancel();
    });
    let started = std::time::Instant::now();
    let summary = scan.run(&cancel, |_| {}).await;
    assert!(summary.cancelled);
    assert!(summary.checked < summary.total);
    assert!(started.elapsed() < Duration::from_secs(3));
}

// ---- ping -------------------------------------------------------------------------------

#[tokio::test]
async fn tcp_ping_measures_connects() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let task = tokio::spawn(async move {
        while let Ok((s, _)) = listener.accept().await {
            drop(s);
        }
    });
    let options =
        PingOptions { count: 3, interval: Duration::from_millis(200), mode: PingMode::Tcp, port, ..Default::default() };
    let pinger = Pinger::prepare("127.0.0.1", options).await.unwrap();
    assert_eq!(pinger.started().mode, PingMode::Tcp);
    assert_eq!(pinger.started().port, Some(port));
    let mut replies = Vec::new();
    let summary = pinger.run(&CancellationToken::new(), |r| replies.push(r)).await;
    assert_eq!(replies.iter().map(|r| r.seq).collect::<Vec<_>>(), [1, 2, 3]);
    assert!(replies.iter().all(|r| r.ms.is_some() && r.error.is_none()));
    assert_eq!((summary.sent, summary.received), (3, 3));
    assert_eq!(summary.loss_percent, 0.0);
    assert!(summary.min_ms.unwrap() <= summary.avg_ms.unwrap() && summary.avg_ms.unwrap() <= summary.max_ms.unwrap());
    task.abort();

    // A port in the host overrides the option; refused counts as lost.
    let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let options =
        PingOptions { count: 2, interval: Duration::from_millis(200), mode: PingMode::Tcp, ..Default::default() };
    let pinger = Pinger::prepare(&format!("127.0.0.1:{closed}"), options).await.unwrap();
    let mut replies = Vec::new();
    let summary = pinger.run(&CancellationToken::new(), |r| replies.push(r)).await;
    assert_eq!((summary.sent, summary.received), (2, 0));
    assert_eq!(summary.loss_percent, 100.0);
    assert!(replies[0].error.as_deref().unwrap().contains("refused"));
    assert!(summary.avg_ms.is_none());
}

/// A link-local IPv6 address only works with its scope (`fe80::1%lo0`); it
/// must survive into every probe. macOS gives the loopback such an address;
/// elsewhere the test has nothing to connect to and stops early.
#[tokio::test]
async fn link_local_targets_keep_their_scope() {
    let Some(scoped) = tokio::net::lookup_host("fe80::1%lo0:0").await.ok().and_then(|mut a| a.next()) else { return };
    let Ok(listener) = TcpListener::bind(scoped).await else { return };
    let port = listener.local_addr().unwrap().port();
    let task = tokio::spawn(async move {
        while let Ok((s, _)) = listener.accept().await {
            drop(s);
        }
    });
    let options = PingOptions { count: 1, mode: PingMode::Tcp, port, ..Default::default() };
    let pinger = Pinger::prepare("fe80::1%lo0", options).await.unwrap();
    let mut replies = Vec::new();
    pinger.run(&CancellationToken::new(), |r| replies.push(r)).await;
    assert!(replies[0].ms.is_some(), "{replies:?}");

    let scan = PortScan::prepare("fe80::1%lo0", vec![port], PortScanOptions::default()).await.unwrap();
    let summary = scan.run(&CancellationToken::new(), |_| {}).await;
    assert_eq!(summary.open, vec![port]);

    let options = PingOptions { count: 1, mode: PingMode::Icmp, ..Default::default() };
    if let Ok(pinger) = Pinger::prepare("fe80::1%lo0", options).await {
        let mut replies = Vec::new();
        pinger.run(&CancellationToken::new(), |r| replies.push(r)).await;
        assert!(replies[0].ms.is_some(), "{replies:?}");
    }
    task.abort();
}

/// ICMP needs no privileges on macOS and Windows; on Linux it depends on
/// `net.ipv4.ping_group_range`, so "not permitted" is accepted there.
#[tokio::test]
async fn icmp_ping_to_loopback() {
    let options =
        PingOptions { count: 2, interval: Duration::from_millis(200), mode: PingMode::Icmp, ..Default::default() };
    let pinger = match Pinger::prepare("127.0.0.1", options).await {
        Ok(p) => p,
        Err(e) => {
            if !cfg!(target_os = "linux") {
                panic!("ICMP should be available here: {}", e.message);
            }
            return;
        }
    };
    assert_eq!(pinger.started().mode, PingMode::Icmp);
    let mut replies = Vec::new();
    let summary = pinger.run(&CancellationToken::new(), |r| replies.push(r)).await;
    assert_eq!(summary.sent, 2);
    assert!(summary.received >= 1, "{replies:?}");
    let ok = replies.iter().find(|r| r.ms.is_some()).unwrap();
    assert!(ok.ms.unwrap() < 1000.0);
    #[cfg(any(target_os = "macos", windows))]
    assert!(ok.ttl.is_some());
}

#[tokio::test]
async fn auto_mode_picks_icmp_or_explains_the_fallback() {
    let options = PingOptions { count: 1, mode: PingMode::Auto, ..Default::default() };
    let pinger = Pinger::prepare("127.0.0.1", options).await.unwrap();
    let started = pinger.started();
    match started.mode {
        PingMode::Icmp => assert!(started.note.is_none()),
        PingMode::Tcp => assert!(started.note.as_deref().unwrap().contains("TCP")),
        PingMode::Auto => panic!("auto must resolve to a concrete mode"),
    }
}

#[tokio::test]
async fn endless_ping_stops_on_cancel() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let options =
        PingOptions { count: 0, interval: Duration::from_millis(200), mode: PingMode::Tcp, port, ..Default::default() };
    let pinger = Pinger::prepare("127.0.0.1", options).await.unwrap();
    let cancel = CancellationToken::new();
    let trigger = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(500)).await;
        trigger.cancel();
    });
    let mut count = 0;
    let summary = pinger.run(&cancel, |_| count += 1).await;
    assert!(summary.cancelled);
    assert!((2..=4).contains(&count), "{count} replies");
    assert_eq!(summary.sent, count);
    drop(listener);
}

// ---- interfaces ------------------------------------------------------------------------

#[test]
fn lists_interfaces_with_loopback_last() {
    let list = list_interfaces().unwrap();
    assert!(!list.is_empty());
    assert!(list.iter().any(|i| i.loopback));
    assert!(list.iter().all(|i| !i.addresses.is_empty()));
    let first_loopback = list.iter().position(|i| i.loopback).unwrap();
    assert!(list[first_loopback..].iter().all(|i| i.loopback));
}
