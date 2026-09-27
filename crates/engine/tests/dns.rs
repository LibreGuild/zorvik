//! DNS client against an in-test responder (UDP + TCP on one port, DNS over
//! TLS, DNS over HTTPS). No internet needed.

use std::net::{Ipv4Addr, SocketAddr};
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use hickory_proto::op::{Edns, Message, ResponseCode};
use hickory_proto::rr::rdata::{A, MX, PTR, SOA, TXT};
use hickory_proto::rr::{Name, RData, Record, RecordType};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, UdpSocket};
use zorvik_engine::{Client, DnsQuery, DnsResult, ErrorKind, RequestOptions, TlsOptions};
use zorvik_testkit::TestCerts;

/// Answers for the test zone. `udp` answers that do not fit are truncated.
fn answer(query: &[u8], udp: bool) -> Option<Vec<u8>> {
    let request = Message::from_vec(query).ok()?;
    let question = request.queries.first()?.clone();
    let qname = question.name().clone();
    let mut response = Message::response(request.metadata.id, request.metadata.op_code);
    response.metadata.recursion_desired = request.metadata.recursion_desired;
    response.metadata.recursion_available = true;
    response.metadata.authoritative = true;
    response.add_query(question.clone());
    if request.edns.is_some() {
        response.set_edns(Edns::new());
    }
    let rec = |data: RData| Record::from_rdata(qname.clone(), 300, data);
    let name = |s: &str| Name::from_str(s).unwrap();
    match (qname.to_ascii().to_lowercase().as_str(), question.query_type()) {
        ("slow.test.", _) => return None,
        ("example.test.", RecordType::A) => {
            response.add_answer(rec(RData::A(A(Ipv4Addr::new(192, 0, 2, 1)))));
        }
        ("example.test.", RecordType::MX) => {
            response.add_answer(rec(RData::MX(MX::new(10, name("mail.example.test.")))));
        }
        ("example.test.", RecordType::TXT) => {
            response.add_answer(rec(RData::TXT(TXT::new(vec!["hello \"world\"".into(), "second".into()]))));
        }
        ("rd.test.", _) => {
            let text = format!("rd={}", request.metadata.recursion_desired);
            response.add_answer(rec(RData::TXT(TXT::new(vec![text]))));
        }
        ("big.test.", RecordType::TXT) if udp => response.metadata.truncation = true,
        ("big.test.", RecordType::TXT) => {
            for i in 0..40 {
                response.add_answer(rec(RData::TXT(TXT::new(vec![format!("{i:02}-{}", "x".repeat(60))]))));
            }
        }
        ("old.test.", _) if request.edns.is_some() => {
            // A server from before EDNS: FORMERR, no OPT record.
            response.metadata.response_code = ResponseCode::FormErr;
            response.edns = None;
        }
        ("old.test.", RecordType::A) => {
            response.add_answer(rec(RData::A(A(Ipv4Addr::new(192, 0, 2, 9)))));
        }
        ("1.2.0.192.in-addr.arpa.", RecordType::PTR) => {
            response.add_answer(rec(RData::PTR(PTR(name("host.example.test.")))));
        }
        _ => {
            response.metadata.response_code = ResponseCode::NXDomain;
            let soa = SOA::new(name("ns.test."), name("admin.test."), 2024, 3600, 600, 86400, 60);
            response.add_authority(Record::from_rdata(name("test."), 60, RData::SOA(soa)));
        }
    }
    response.to_vec().ok()
}

/// Serve length-prefixed DNS messages on one stream (TCP or TLS).
async fn serve_stream<S: AsyncRead + AsyncWrite + Unpin>(mut stream: S) {
    loop {
        let mut len = [0u8; 2];
        if stream.read_exact(&mut len).await.is_err() {
            return;
        }
        let mut query = vec![0u8; usize::from(u16::from_be_bytes(len))];
        if stream.read_exact(&mut query).await.is_err() {
            return;
        }
        let Some(reply) = answer(&query, false) else { continue };
        let mut framed = (reply.len() as u16).to_be_bytes().to_vec();
        framed.extend_from_slice(&reply);
        if stream.write_all(&framed).await.is_err() {
            return;
        }
    }
}

/// UDP and TCP on the same port, like a real server (the TCP retry reuses the port).
async fn start_responder() -> SocketAddr {
    for _ in 0..20 {
        let tcp = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = tcp.local_addr().unwrap();
        let Ok(udp) = UdpSocket::bind(addr).await else { continue };
        tokio::spawn(async move {
            let mut buf = vec![0u8; 65535];
            loop {
                let Ok((n, peer)) = udp.recv_from(&mut buf).await else { return };
                if let Some(reply) = answer(&buf[..n], true) {
                    let _ = udp.send_to(&reply, peer).await;
                }
            }
        });
        tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = tcp.accept().await else { return };
                tokio::spawn(serve_stream(stream));
            }
        });
        return addr;
    }
    panic!("no port free for both UDP and TCP");
}

fn query(name: &str, record_type: &str, server: &str) -> DnsQuery {
    DnsQuery {
        name: name.into(),
        record_type: record_type.into(),
        server: server.into(),
        recursion: true,
        timeout: Duration::from_secs(5),
    }
}

async fn run(q: DnsQuery) -> DnsResult {
    Client::new().dns_query(&q, &RequestOptions::default()).await.unwrap_or_else(|e| panic!("{}", e.message))
}

#[tokio::test]
async fn udp_answer_with_sections_flags_and_zone_text() {
    let addr = start_responder().await;
    let server = addr.to_string();

    let r = run(query("example.test", "A", &server)).await;
    assert_eq!((r.rcode.as_str(), r.rcode_value), ("NOERROR", 0));
    assert_eq!((r.question.name.as_str(), r.question.record_type.as_str()), ("example.test.", "A"));
    assert_eq!(r.answers.len(), 1);
    let a = &r.answers[0];
    assert_eq!((a.name.as_str(), a.record_type.as_str(), a.class.as_str(), a.ttl), ("example.test.", "A", "IN", 300));
    assert_eq!(a.data, "192.0.2.1");
    assert!(r.flags.aa && r.flags.rd && r.flags.ra && !r.flags.tc);
    assert_eq!((r.protocol.as_str(), r.server.as_str(), r.system), ("UDP", server.as_str(), false));
    assert!(r.size > 12 && r.duration_ms > 0.0);

    let mx = run(query("https://Example.test:8443/path", "mx", &server)).await;
    assert_eq!(mx.answers[0].data, "10 mail.example.test.");
    let txt = run(query("example.test", "TXT", &server)).await;
    assert_eq!(txt.answers[0].data, r#""hello \"world\"" "second""#);
    // An IP address becomes its reverse name for PTR.
    let ptr = run(query("192.0.2.1", "PTR", &format!("udp://{server}"))).await;
    assert_eq!(ptr.question.name, "1.2.0.192.in-addr.arpa.");
    assert_eq!(ptr.answers[0].data, "host.example.test.");
}

#[tokio::test]
async fn recursion_flag_is_sent_as_asked() {
    let server = start_responder().await.to_string();
    let mut q = query("rd.test", "TXT", &server);
    q.recursion = false;
    let r = run(q).await;
    assert_eq!(r.answers[0].data, "\"rd=false\"");
    assert!(!r.flags.rd);
    let r = run(query("rd.test", "TXT", &server)).await;
    assert_eq!(r.answers[0].data, "\"rd=true\"");
}

#[tokio::test]
async fn nxdomain_is_a_result_with_the_soa() {
    let server = start_responder().await.to_string();
    let r = run(query("missing.test", "AAAA", &server)).await;
    assert_eq!((r.rcode.as_str(), r.rcode_value), ("NXDOMAIN", 3));
    assert!(r.answers.is_empty());
    assert_eq!(r.authority[0].record_type, "SOA");
    assert_eq!(r.authority[0].data, "ns.test. admin.test. 2024 3600 600 86400 60");
}

#[tokio::test]
async fn truncated_udp_answer_is_fetched_over_tcp() {
    let server = start_responder().await.to_string();
    let r = run(query("big.test", "TXT", &server)).await;
    assert_eq!(r.protocol, "TCP");
    assert_eq!(r.answers.len(), 40);
    assert!(!r.flags.tc);
    assert!(r.size > 1232, "{}", r.size);
    assert!(r.notes.iter().any(|n| n.contains("over TCP")), "{:?}", r.notes);
}

#[tokio::test]
async fn truncated_answer_is_kept_when_the_tcp_retry_never_answers() {
    // UDP answers (truncated); TCP on the same port accepts but stays silent.
    let (tcp, udp) = loop {
        let tcp = TcpListener::bind("127.0.0.1:0").await.unwrap();
        if let Ok(udp) = UdpSocket::bind(tcp.local_addr().unwrap()).await {
            break (tcp, udp);
        }
    };
    let addr = udp.local_addr().unwrap();
    tokio::spawn(async move {
        let mut buf = vec![0u8; 65535];
        while let Ok((n, peer)) = udp.recv_from(&mut buf).await {
            if let Some(reply) = answer(&buf[..n], true) {
                let _ = udp.send_to(&reply, peer).await;
            }
        }
    });
    tokio::spawn(async move {
        let mut open = Vec::new();
        while let Ok((stream, _)) = tcp.accept().await {
            open.push(stream);
        }
    });
    let mut q = query("big.test", "TXT", &addr.to_string());
    q.timeout = Duration::from_millis(800);
    let r = run(q).await;
    assert_eq!(r.protocol, "UDP");
    assert!(r.flags.tc);
    assert!(r.notes.iter().any(|n| n.contains("TCP retry failed")), "{:?}", r.notes);
}

#[tokio::test]
async fn tcp_resolver_and_edns_fallback() {
    let addr = start_responder().await;
    let r = run(query("example.test", "A", &format!("tcp://{addr}"))).await;
    assert_eq!((r.protocol.as_str(), r.answers[0].data.as_str()), ("TCP", "192.0.2.1"));
    // FORMERR to EDNS: asked again without it.
    let r = run(query("old.test", "A", &addr.to_string())).await;
    assert_eq!(r.answers[0].data, "192.0.2.9");
    assert!(r.notes.iter().any(|n| n.contains("EDNS")), "{:?}", r.notes);
}

#[tokio::test]
async fn silent_server_times_out() {
    let server = start_responder().await.to_string();
    let mut q = query("slow.test", "A", &server);
    q.timeout = Duration::from_millis(400);
    let started = Instant::now();
    let err = Client::new().dns_query(&q, &RequestOptions::default()).await.unwrap_err();
    assert_eq!(err.kind, ErrorKind::Timeout, "{}", err.message);
    assert!(started.elapsed() < Duration::from_secs(3));
}

#[tokio::test]
async fn nothing_listening_is_an_error() {
    let port = std::net::UdpSocket::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let mut q = query("example.test", "A", &format!("127.0.0.1:{port}"));
    q.timeout = Duration::from_millis(1500);
    let err = Client::new().dns_query(&q, &RequestOptions::default()).await.unwrap_err();
    // ICMP "port unreachable" is reported right away where the OS passes it on.
    assert!(matches!(err.kind, ErrorKind::Connect | ErrorKind::Timeout), "{:?}: {}", err.kind, err.message);
}

#[tokio::test]
async fn invalid_input_is_refused_before_sending() {
    let client = Client::new();
    let opts = RequestOptions::default();
    for (name, rtype, server, needle) in [
        ("1.2.3.4", "A", "", "PTR"),
        ("", "A", "", "Enter a name"),
        ("example.test", "BOGUS", "", "record type"),
        ("example.test", "AXFR", "", "Zone transfers"),
        ("example.test", "A", "ftp://x", "scheme"),
    ] {
        let err = client.dns_query(&query(name, rtype, server), &opts).await.unwrap_err();
        assert_eq!(err.kind, ErrorKind::InvalidRequest);
        assert!(err.message.contains(needle), "{name}/{rtype}/{server}: {}", err.message);
    }
}

#[tokio::test]
async fn wrong_ids_are_ignored_and_garbage_is_a_protocol_error() {
    let udp = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let addr = udp.local_addr().unwrap();
    tokio::spawn(async move {
        let mut buf = vec![0u8; 65535];
        loop {
            let Ok((n, peer)) = udp.recv_from(&mut buf).await else { return };
            let query = buf[..n].to_vec();
            let is_garbage = Message::from_vec(&query).unwrap().queries[0].name().to_ascii().starts_with("garbage");
            // First a spoofed answer with another ID, then the real one.
            let mut spoof = answer(&query, true).unwrap();
            spoof[0] ^= 0xff;
            let _ = udp.send_to(&spoof, peer).await;
            let reply = if is_garbage {
                // Our ID with the QR bit, then claims 200 answers it does not have.
                let mut bad = vec![query[0], query[1], 0x81, 0x80, 0, 1, 0, 200, 0, 0, 0, 0];
                bad.extend_from_slice(&[0xc0; 40]);
                bad
            } else {
                answer(&query, true).unwrap()
            };
            let _ = udp.send_to(&reply, peer).await;
        }
    });
    let r = run(query("example.test", "A", &addr.to_string())).await;
    assert_eq!(r.answers[0].data, "192.0.2.1");
    let err = Client::new()
        .dns_query(&query("garbage.test", "A", &addr.to_string()), &RequestOptions::default())
        .await
        .unwrap_err();
    assert_eq!(err.kind, ErrorKind::Protocol, "{}", err.message);
}

fn tls_acceptor(certs: &TestCerts) -> tokio_rustls::TlsAcceptor {
    use rustls_pki_types::pem::PemObject;
    use rustls_pki_types::{CertificateDer, PrivateKeyDer};
    let chain = CertificateDer::pem_slice_iter(certs.cert_pem.as_bytes()).collect::<Result<Vec<_>, _>>().unwrap();
    let key = PrivateKeyDer::from_pem_slice(certs.key_pem.as_bytes()).unwrap();
    let config = rustls::ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(chain, key)
        .unwrap();
    tokio_rustls::TlsAcceptor::from(Arc::new(config))
}

#[tokio::test]
async fn dns_over_tls_verifies_the_server() {
    let certs = TestCerts::generate();
    let acceptor = tls_acceptor(&certs);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((tcp, _)) = listener.accept().await else { return };
            let acceptor = acceptor.clone();
            tokio::spawn(async move {
                if let Ok(tls) = acceptor.accept(tcp).await {
                    serve_stream(tls).await;
                }
            });
        }
    });
    let dir = tempfile::tempdir().unwrap();
    let trusted = RequestOptions {
        tls: TlsOptions { ca_cert_path: Some(certs.write_ca(dir.path())), ..TlsOptions::default() },
        ..RequestOptions::default()
    };
    let q = query("example.test", "A", &format!("tls://{addr}"));
    let r = Client::new().dns_query(&q, &trusted).await.unwrap();
    assert_eq!((r.protocol.as_str(), r.answers[0].data.as_str()), ("DoT", "192.0.2.1"));
    assert!(r.tls.as_ref().is_some_and(|t| t.version.starts_with("TLS")));
    // The test CA is not in the OS trust store.
    let err = Client::new().dns_query(&q, &RequestOptions::default()).await.unwrap_err();
    assert_eq!(err.kind, ErrorKind::Tls, "{}", err.message);
}

/// A minimal HTTP/1.1 server for DoH: POST with a DNS message body.
async fn serve_doh<S: AsyncRead + AsyncWrite + Unpin>(mut stream: S) {
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        if stream.read_exact(&mut byte).await.is_err() || head.len() > 16 * 1024 {
            return;
        }
        head.push(byte[0]);
    }
    let head = String::from_utf8_lossy(&head).to_string();
    let length = head
        .lines()
        .find_map(|l| {
            l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap())
        })
        .unwrap_or(0);
    let mut body = vec![0u8; length];
    let _ = stream.read_exact(&mut body).await;
    let typed = head.to_ascii_lowercase().contains("content-type: application/dns-message");
    let (status, content_type, reply) = if head.starts_with("POST /dns-query ") && typed {
        ("200 OK", "application/dns-message", answer(&body, false).unwrap_or_default())
    } else if head.starts_with("POST /html ") {
        ("200 OK", "text/html", b"<html></html>".to_vec())
    } else {
        ("404 Not Found", "text/plain", b"not here".to_vec())
    };
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        reply.len()
    );
    let _ = stream.write_all(response.as_bytes()).await;
    let _ = stream.write_all(&reply).await;
    let _ = stream.shutdown().await;
}

#[tokio::test]
async fn dns_over_https_posts_a_dns_message() {
    let certs = TestCerts::generate();
    let acceptor = tls_acceptor(&certs);
    let plain = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let secure = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (plain_addr, secure_addr) = (plain.local_addr().unwrap(), secure.local_addr().unwrap());
    tokio::spawn(async move {
        loop {
            let Ok((tcp, _)) = plain.accept().await else { return };
            tokio::spawn(serve_doh(tcp));
        }
    });
    tokio::spawn(async move {
        loop {
            let Ok((tcp, _)) = secure.accept().await else { return };
            let acceptor = acceptor.clone();
            tokio::spawn(async move {
                if let Ok(tls) = acceptor.accept(tcp).await {
                    serve_doh(tls).await;
                }
            });
        }
    });
    let client = Client::new();

    // A bare origin gets the usual /dns-query path.
    let r = run(query("example.test", "MX", &format!("http://{plain_addr}"))).await;
    assert_eq!((r.protocol.as_str(), r.answers[0].data.as_str()), ("DoH", "10 mail.example.test."));
    assert_eq!(r.server, format!("http://{plain_addr}/dns-query"));

    let dir = tempfile::tempdir().unwrap();
    let trusted = RequestOptions {
        tls: TlsOptions { ca_cert_path: Some(certs.write_ca(dir.path())), ..TlsOptions::default() },
        ..RequestOptions::default()
    };
    let q = query("missing.test", "A", &format!("https://127.0.0.1:{}/dns-query", secure_addr.port()));
    let r = client.dns_query(&q, &trusted).await.unwrap();
    assert_eq!((r.protocol.as_str(), r.rcode.as_str()), ("DoH", "NXDOMAIN"));
    assert!(r.tls.is_some());

    let err = client.dns_query(&query("example.test", "A", &format!("http://{plain_addr}/nope")), &trusted).await;
    assert!(err.unwrap_err().message.contains("404"));
    let err = client.dns_query(&query("example.test", "A", &format!("http://{plain_addr}/html")), &trusted).await;
    assert!(err.unwrap_err().message.contains("text/html"));
}
