//! MQTT client against a minimal in-test broker (3.1.1 and 5, plain and TLS)
//! built on rumqttc's packet codec. No internet needed.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bytes::{Bytes, BytesMut};
use rumqttc::mqttbytes::{self as m4, v4};
use rumqttc::v5::mqttbytes::{self as m5, v5};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use zorvik_engine::mqtt::{MqttConfig, MqttProtocol};
use zorvik_engine::{Client, Direction, RequestOptions, SocketConnected, SocketEvent, SocketOutgoing, TlsOptions};
use zorvik_testkit::TestCerts;

// ---- broker -------------------------------------------------------------------

/// A client packet, whatever the protocol version.
enum Pkt {
    Subscribe { pkid: u16, filters: Vec<(String, u8)> },
    Unsubscribe { pkid: u16 },
    Publish { topic: String, payload: Bytes, qos: u8, retain: bool, pkid: u16 },
    PubRec { pkid: u16 },
    PubRel { pkid: u16 },
    PingReq,
    Disconnect,
    Other,
}

struct Peer {
    version: u8,
    filters: Vec<(String, u8)>,
    tx: mpsc::UnboundedSender<Vec<u8>>,
    next_pkid: u16,
}

#[derive(Default)]
struct Broker {
    peers: Mutex<Vec<(u64, Peer)>>,
    next_id: AtomicUsize,
    disconnects: AtomicUsize,
    /// Only speak MQTT 3.1.1 (answer an MQTT 5 CONNECT with "unacceptable version").
    v311_only: bool,
}

fn qos4(q: u8) -> m4::QoS {
    [m4::QoS::AtMostOnce, m4::QoS::AtLeastOnce, m4::QoS::ExactlyOnce][usize::from(q.min(2))]
}

fn qos5(q: u8) -> m5::QoS {
    [m5::QoS::AtMostOnce, m5::QoS::AtLeastOnce, m5::QoS::ExactlyOnce][usize::from(q.min(2))]
}

fn write4(p: v4::Packet) -> Vec<u8> {
    let mut b = BytesMut::new();
    p.write(&mut b, 1 << 24).unwrap();
    b.to_vec()
}

fn write5(p: v5::Packet) -> Vec<u8> {
    let mut b = BytesMut::new();
    p.write(&mut b, None).unwrap();
    b.to_vec()
}

fn publish(version: u8, topic: &str, payload: Bytes, qos: u8, retain: bool, pkid: u16) -> Vec<u8> {
    if version == 5 {
        let mut p = v5::Publish::new(topic, qos5(qos), payload, None);
        (p.retain, p.pkid) = (retain, pkid);
        write5(v5::Packet::Publish(p))
    } else {
        let mut p = v4::Publish::from_bytes(topic, qos4(qos), payload);
        (p.retain, p.pkid) = (retain, pkid);
        write4(v4::Packet::Publish(p))
    }
}

fn read_packet(version: u8, buf: &mut BytesMut) -> Option<Pkt> {
    Some(if version == 5 {
        match v5::Packet::read(buf, None).ok()? {
            v5::Packet::Subscribe(s) => {
                Pkt::Subscribe { pkid: s.pkid, filters: s.filters.into_iter().map(|f| (f.path, f.qos as u8)).collect() }
            }
            v5::Packet::Unsubscribe(u) => Pkt::Unsubscribe { pkid: u.pkid },
            v5::Packet::Publish(p) => Pkt::Publish {
                topic: String::from_utf8_lossy(&p.topic).into_owned(),
                payload: p.payload,
                qos: p.qos as u8,
                retain: p.retain,
                pkid: p.pkid,
            },
            v5::Packet::PubRec(p) => Pkt::PubRec { pkid: p.pkid },
            v5::Packet::PubRel(p) => Pkt::PubRel { pkid: p.pkid },
            v5::Packet::PingReq(_) => Pkt::PingReq,
            v5::Packet::Disconnect(_) => Pkt::Disconnect,
            _ => Pkt::Other,
        }
    } else {
        match v4::Packet::read(buf, 1 << 24).ok()? {
            v4::Packet::Subscribe(s) => {
                Pkt::Subscribe { pkid: s.pkid, filters: s.filters.into_iter().map(|f| (f.path, f.qos as u8)).collect() }
            }
            v4::Packet::Unsubscribe(u) => Pkt::Unsubscribe { pkid: u.pkid },
            v4::Packet::Publish(p) => {
                Pkt::Publish { topic: p.topic, payload: p.payload, qos: p.qos as u8, retain: p.retain, pkid: p.pkid }
            }
            v4::Packet::PubRec(p) => Pkt::PubRec { pkid: p.pkid },
            v4::Packet::PubRel(p) => Pkt::PubRel { pkid: p.pkid },
            v4::Packet::PingReq => Pkt::PingReq,
            v4::Packet::Disconnect => Pkt::Disconnect,
            _ => Pkt::Other,
        }
    })
}

/// Acks as `(v4 packet, v5 packet)`, picked by version.
fn ack(version: u8, v4p: v4::Packet, v5p: v5::Packet) -> Vec<u8> {
    if version == 5 { write5(v5p) } else { write4(v4p) }
}

impl Broker {
    async fn start(v311_only: bool) -> (Arc<Broker>, SocketAddr) {
        let broker = Arc::new(Broker { v311_only, ..Default::default() });
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let b = broker.clone();
        tokio::spawn(async move {
            loop {
                let Ok((tcp, _)) = listener.accept().await else { return };
                tokio::spawn(b.clone().serve(tcp));
            }
        });
        (broker, addr)
    }

    async fn start_tls(certs: &TestCerts) -> (Arc<Broker>, SocketAddr) {
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
        let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
        let broker = Arc::new(Broker::default());
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let b = broker.clone();
        tokio::spawn(async move {
            loop {
                let Ok((tcp, _)) = listener.accept().await else { return };
                let (b, acceptor) = (b.clone(), acceptor.clone());
                tokio::spawn(async move {
                    if let Ok(tls) = acceptor.accept(tcp).await {
                        b.serve(tls).await;
                    }
                });
            }
        });
        (broker, addr)
    }

    fn route(&self, topic: &str, payload: &Bytes, qos: u8, retain: bool) {
        for (_, peer) in self.peers.lock().unwrap().iter_mut() {
            let Some(sub_qos) = peer.filters.iter().filter(|(f, _)| m4::matches(topic, f)).map(|(_, q)| *q).max()
            else {
                continue;
            };
            let out_qos = qos.min(sub_qos);
            peer.next_pkid = peer.next_pkid % 60000 + 1;
            let pkid = if out_qos == 0 { 0 } else { peer.next_pkid };
            let _ = peer.tx.send(publish(peer.version, topic, payload.clone(), out_qos, retain, pkid));
        }
    }

    async fn serve<S: AsyncRead + AsyncWrite + Unpin + Send + 'static>(self: Arc<Self>, stream: S) {
        let (mut rd, mut wr) = tokio::io::split(stream);
        let mut buf = BytesMut::new();
        // CONNECT: the protocol level follows the "MQTT" protocol name.
        let (version, client_id, username) = loop {
            if m4::check(buf.iter(), 1 << 20).is_ok() {
                let level = buf.windows(4).position(|w| w == b"MQTT").map(|i| buf[i + 4]).unwrap_or(0);
                if level == 5 && self.v311_only {
                    let _ = wr.write_all(&[0x20, 0x02, 0x00, 0x01]).await;
                    return;
                }
                if level == 5 {
                    let v5::Packet::Connect(c, _, login) = v5::Packet::read(&mut buf, None).unwrap() else { return };
                    break (5u8, c.client_id, login.map(|l| l.username).unwrap_or_default());
                }
                let v4::Packet::Connect(c) = v4::Packet::read(&mut buf, 1 << 20).unwrap() else { return };
                break (4u8, c.client_id, c.login.map(|l| l.username).unwrap_or_default());
            }
            if rd.read_buf(&mut buf).await.unwrap_or(0) == 0 {
                return;
            }
        };
        if username == "bad" {
            let refused = ack(
                version,
                v4::Packet::ConnAck(v4::ConnAck::new(v4::ConnectReturnCode::BadUserNamePassword, false)),
                v5::Packet::ConnAck(v5::ConnAck {
                    session_present: false,
                    code: v5::ConnectReturnCode::BadUserNamePassword,
                    properties: None,
                }),
            );
            let _ = wr.write_all(&refused).await;
            return;
        }
        let accepted = ack(
            version,
            v4::Packet::ConnAck(v4::ConnAck::new(v4::ConnectReturnCode::Success, false)),
            v5::Packet::ConnAck(v5::ConnAck {
                session_present: false,
                code: v5::ConnectReturnCode::Success,
                properties: None,
            }),
        );
        wr.write_all(&accepted).await.unwrap();
        if client_id == "garbage" {
            let _ = wr.write_all(&[0x00, 0x00, 0xff, 0xff]).await;
        }

        let (tx, mut rx) = mpsc::unbounded_channel::<Vec<u8>>();
        let id = self.next_id.fetch_add(1, Ordering::SeqCst) as u64;
        self.peers.lock().unwrap().push((id, Peer { version, filters: Vec::new(), tx: tx.clone(), next_pkid: 0 }));
        loop {
            while let Some(packet) = read_packet(version, &mut buf) {
                let reply = match packet {
                    Pkt::Subscribe { pkid, filters } => {
                        // "forbidden/…" is refused; "grant1/…" gets at most QoS 1.
                        let codes: Vec<Result<u8, ()>> = filters
                            .iter()
                            .map(|(f, q)| {
                                if f.starts_with("forbidden") {
                                    Err(())
                                } else if f.starts_with("grant1") {
                                    Ok((*q).min(1))
                                } else {
                                    Ok(*q)
                                }
                            })
                            .collect();
                        if let Some((_, peer)) = self.peers.lock().unwrap().iter_mut().find(|(i, _)| *i == id) {
                            for ((f, _), code) in filters.iter().zip(&codes) {
                                if let Ok(q) = code {
                                    peer.filters.push((f.clone(), *q));
                                }
                            }
                        }
                        ack(
                            version,
                            v4::Packet::SubAck(v4::SubAck::new(
                                pkid,
                                codes
                                    .iter()
                                    .map(|c| match c {
                                        Ok(q) => v4::SubscribeReasonCode::Success(qos4(*q)),
                                        Err(()) => v4::SubscribeReasonCode::Failure,
                                    })
                                    .collect(),
                            )),
                            v5::Packet::SubAck(v5::SubAck {
                                pkid,
                                return_codes: codes
                                    .iter()
                                    .map(|c| match c {
                                        Ok(q) => v5::SubscribeReasonCode::Success(qos5(*q)),
                                        Err(()) => v5::SubscribeReasonCode::NotAuthorized,
                                    })
                                    .collect(),
                                properties: None,
                            }),
                        )
                    }
                    Pkt::Unsubscribe { pkid } => {
                        if let Some((_, peer)) = self.peers.lock().unwrap().iter_mut().find(|(i, _)| *i == id) {
                            peer.filters.clear();
                        }
                        ack(
                            version,
                            v4::Packet::UnsubAck(v4::UnsubAck::new(pkid)),
                            v5::Packet::UnsubAck(v5::UnsubAck {
                                pkid,
                                reasons: vec![v5::UnsubAckReason::Success],
                                properties: None,
                            }),
                        )
                    }
                    Pkt::Publish { topic, payload, qos, retain, pkid } => {
                        if topic == "close/now" {
                            self.peers.lock().unwrap().retain(|(i, _)| *i != id);
                            return;
                        }
                        self.route(&topic, &payload, qos, retain);
                        match qos {
                            1 => ack(
                                version,
                                v4::Packet::PubAck(v4::PubAck::new(pkid)),
                                v5::Packet::PubAck(v5::PubAck::new(pkid, None)),
                            ),
                            2 => ack(
                                version,
                                v4::Packet::PubRec(v4::PubRec::new(pkid)),
                                v5::Packet::PubRec(v5::PubRec::new(pkid, None)),
                            ),
                            _ => Vec::new(),
                        }
                    }
                    Pkt::PubRec { pkid } => ack(
                        version,
                        v4::Packet::PubRel(v4::PubRel::new(pkid)),
                        v5::Packet::PubRel(v5::PubRel::new(pkid, None)),
                    ),
                    Pkt::PubRel { pkid } => ack(
                        version,
                        v4::Packet::PubComp(v4::PubComp::new(pkid)),
                        v5::Packet::PubComp(v5::PubComp::new(pkid, None)),
                    ),
                    Pkt::PingReq if client_id == "noping" => Vec::new(),
                    Pkt::PingReq => ack(version, v4::Packet::PingResp, v5::Packet::PingResp(v5::PingResp)),
                    Pkt::Disconnect => {
                        self.disconnects.fetch_add(1, Ordering::SeqCst);
                        self.peers.lock().unwrap().retain(|(i, _)| *i != id);
                        return;
                    }
                    Pkt::Other => Vec::new(),
                };
                if !reply.is_empty() {
                    let _ = tx.send(reply);
                }
            }
            tokio::select! {
                read = rd.read_buf(&mut buf) => {
                    if read.unwrap_or(0) == 0 {
                        self.peers.lock().unwrap().retain(|(i, _)| *i != id);
                        return;
                    }
                }
                Some(bytes) = rx.recv() => {
                    if wr.write_all(&bytes).await.is_err() {
                        return;
                    }
                }
            }
        }
    }
}

// ---- helpers ------------------------------------------------------------------

async fn next(conn: &mut SocketConnected) -> SocketEvent {
    tokio::time::timeout(Duration::from_secs(5), conn.events.recv()).await.expect("event in time").expect("event")
}

/// The next event that is not the "Client ID …" line.
async fn next_event(conn: &mut SocketConnected) -> SocketEvent {
    loop {
        match next(conn).await {
            SocketEvent::Info { text, .. } if text.starts_with("Client ID") => continue,
            other => return other,
        }
    }
}

fn info(event: &SocketEvent) -> String {
    match event {
        SocketEvent::Info { text, .. } => text.clone(),
        other => panic!("not info: {other:?}"),
    }
}

fn error(event: &SocketEvent) -> String {
    match event {
        SocketEvent::Error { message } => message.clone(),
        other => panic!("not an error: {other:?}"),
    }
}

/// (direction, topic, detail, text)
fn message(event: &SocketEvent) -> (Direction, String, String, String) {
    match event {
        SocketEvent::Message { direction, topic, detail, text, .. } => (
            *direction,
            topic.clone().unwrap_or_default(),
            detail.clone().unwrap_or_default(),
            text.clone().unwrap_or_default(),
        ),
        other => panic!("not a message: {other:?}"),
    }
}

fn config(protocol: MqttProtocol, subscriptions: &[(&str, u8)]) -> MqttConfig {
    MqttConfig {
        protocol,
        subscriptions: subscriptions.iter().map(|(t, q)| (t.to_string(), *q)).collect(),
        ..MqttConfig::default()
    }
}

fn publish_msg(topic: &str, text: &str, qos: u8, retain: bool) -> SocketOutgoing {
    SocketOutgoing::Publish { topic: topic.into(), text: Some(text.into()), base64: None, qos, retain }
}

async fn connect(addr: SocketAddr, cfg: &MqttConfig) -> SocketConnected {
    Client::new().mqtt(&format!("mqtt://{addr}"), &RequestOptions::default(), cfg).await.unwrap()
}

// ---- tests --------------------------------------------------------------------

#[tokio::test]
async fn v311_subscribe_publish_and_receive() {
    let (_broker, addr) = Broker::start(false).await;
    let mut conn = connect(addr, &config(MqttProtocol::V311, &[("test/#", 1)])).await;
    assert_eq!(conn.opened.protocol, "MQTT 3.1.1");
    assert_eq!(conn.opened.remote_addr.as_deref(), Some(addr.to_string().as_str()));
    assert!(conn.opened.timing.total_ms >= conn.opened.timing.ttfb_ms);
    match next(&mut conn).await {
        SocketEvent::Info { text, .. } => assert!(text.starts_with("Client ID zorvik-"), "{text}"),
        other => panic!("{other:?}"),
    }
    assert_eq!(info(&next_event(&mut conn).await), "Subscribed to test/# (QoS 1)");

    conn.session.send(publish_msg("test/a", "hello", 1, true)).unwrap();
    assert_eq!(
        message(&next_event(&mut conn).await),
        (Direction::Sent, "test/a".into(), "QoS 1 · retained".into(), "hello".into())
    );
    assert_eq!(
        message(&next_event(&mut conn).await),
        (Direction::Received, "test/a".into(), "QoS 1 · retained".into(), "hello".into())
    );

    // Binary payloads arrive as base64; not matching the subscription: nothing comes back.
    conn.session
        .send(SocketOutgoing::Publish {
            topic: "other".into(),
            text: None,
            base64: Some("/w==".into()),
            qos: 0,
            retain: false,
        })
        .unwrap();
    match next_event(&mut conn).await {
        SocketEvent::Message { direction: Direction::Sent, base64, size: 1, .. } => {
            assert_eq!(base64.as_deref(), Some("/w=="))
        }
        other => panic!("{other:?}"),
    }
    conn.session.send(SocketOutgoing::Unsubscribe { topic: "test/#".into() }).unwrap();
    assert_eq!(info(&next_event(&mut conn).await), "Unsubscribed from test/#");
    conn.session.send(publish_msg("test/a", "again", 0, false)).unwrap();
    assert_eq!(message(&next_event(&mut conn).await).0, Direction::Sent);
    assert!(tokio::time::timeout(Duration::from_millis(300), conn.events.recv()).await.is_err(), "no echo");
}

#[tokio::test]
async fn qos2_both_ways_and_granted_qos() {
    let (_broker, addr) = Broker::start(false).await;
    let mut conn = connect(addr, &config(MqttProtocol::V311, &[("q2/#", 2), ("grant1/#", 2)])).await;
    assert_eq!(info(&next_event(&mut conn).await), "Subscribed to q2/# (QoS 2)");
    assert_eq!(info(&next_event(&mut conn).await), "Subscribed to grant1/# (QoS 1; asked for 2)");
    for round in 0..3 {
        conn.session.send(publish_msg("q2/x", &format!("exactly once {round}"), 2, false)).unwrap();
        assert_eq!(message(&next_event(&mut conn).await).0, Direction::Sent);
        let (direction, topic, detail, text) = message(&next_event(&mut conn).await);
        assert_eq!((direction, topic.as_str(), detail.as_str()), (Direction::Received, "q2/x", "QoS 2"));
        assert_eq!(text, format!("exactly once {round}"));
    }
}

#[tokio::test]
async fn refused_subscription_and_bad_input_are_reported() {
    let (_broker, addr) = Broker::start(false).await;
    let mut conn = connect(addr, &config(MqttProtocol::V311, &[])).await;
    conn.session.send(SocketOutgoing::Subscribe { topic: "forbidden/#".into(), qos: 1 }).unwrap();
    assert!(error(&next_event(&mut conn).await).contains("Subscription to forbidden/# failed"));
    conn.session.send(SocketOutgoing::Text { text: "no topic".into() }).unwrap();
    assert_eq!(error(&next_event(&mut conn).await), "Enter a topic");
    conn.session.send(publish_msg("a/+", "x", 0, false)).unwrap();
    assert!(error(&next_event(&mut conn).await).contains("wildcard"));
    conn.session.send(SocketOutgoing::Subscribe { topic: "a/#/b".into(), qos: 0 }).unwrap();
    assert!(error(&next_event(&mut conn).await).contains("not a valid topic filter"));
    conn.session.send(publish_msg("a", "x", 3, false)).unwrap();
    assert!(error(&next_event(&mut conn).await).contains("QoS 3"));
}

#[tokio::test]
async fn bad_credentials_are_a_connect_error() {
    let (_broker, addr) = Broker::start(false).await;
    for protocol in [MqttProtocol::V311, MqttProtocol::V5] {
        let cfg = MqttConfig { username: "bad".into(), password: "secret".into(), ..config(protocol, &[]) };
        let err = Client::new().mqtt(&addr.to_string(), &RequestOptions::default(), &cfg).await.err().unwrap();
        assert!(err.message.contains("bad user name or password"), "{protocol:?}: {}", err.message);
    }
    // A password without a user name cannot be sent with MQTT 3.1.1.
    let cfg = MqttConfig { password: "secret".into(), ..config(MqttProtocol::V311, &[]) };
    let err = Client::new().mqtt(&addr.to_string(), &RequestOptions::default(), &cfg).await.err().unwrap();
    assert!(err.message.contains("needs a user name"), "{}", err.message);
}

#[tokio::test]
async fn broker_close_and_client_disconnect() {
    let (broker, addr) = Broker::start(false).await;
    let mut conn = connect(addr, &config(MqttProtocol::V311, &[])).await;
    conn.session.send(publish_msg("close/now", "bye", 0, false)).unwrap();
    assert_eq!(message(&next_event(&mut conn).await).0, Direction::Sent);
    match next_event(&mut conn).await {
        SocketEvent::Closed { by_client: false, reason } => assert_eq!(reason, "Closed by the broker"),
        other => panic!("{other:?}"),
    }

    // Dropping the session sends DISCONNECT.
    let SocketConnected { session, mut events, .. } = connect(addr, &config(MqttProtocol::V311, &[])).await;
    drop(session);
    loop {
        match tokio::time::timeout(Duration::from_secs(5), events.recv()).await.unwrap().unwrap() {
            SocketEvent::Closed { by_client: true, .. } => break,
            SocketEvent::Info { .. } => {}
            other => panic!("{other:?}"),
        }
    }
    for _ in 0..100 {
        if broker.disconnects.load(Ordering::SeqCst) == 1 {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("the broker did not get DISCONNECT");
}

#[tokio::test]
async fn keep_alive_detects_a_dead_broker() {
    let (_broker, addr) = Broker::start(false).await;
    let cfg = MqttConfig { client_id: "noping".into(), keep_alive_secs: 1, ..config(MqttProtocol::V311, &[]) };
    let mut conn = connect(addr, &cfg).await;
    match tokio::time::timeout(Duration::from_secs(6), next_event(&mut conn)).await.unwrap() {
        SocketEvent::Closed { reason, .. } => assert!(reason.contains("PINGRESP"), "{reason}"),
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn invalid_packets_close_the_connection() {
    let (_broker, addr) = Broker::start(false).await;
    for protocol in [MqttProtocol::V311, MqttProtocol::V5] {
        let cfg = MqttConfig { client_id: "garbage".into(), ..config(protocol, &[]) };
        let mut conn = connect(addr, &cfg).await;
        match next_event(&mut conn).await {
            SocketEvent::Closed { reason, .. } => assert!(reason.starts_with("Invalid packet"), "{reason}"),
            other => panic!("{other:?}"),
        }
    }
}

#[tokio::test]
async fn v5_subscribe_publish_and_receive() {
    let (_broker, addr) = Broker::start(false).await;
    let cfg =
        MqttConfig { username: "user".into(), password: "pw".into(), ..config(MqttProtocol::V5, &[("five/+", 1)]) };
    let mut conn = connect(addr, &cfg).await;
    assert_eq!(conn.opened.protocol, "MQTT 5");
    assert_eq!(info(&next_event(&mut conn).await), "Subscribed to five/+ (QoS 1)");
    conn.session.send(publish_msg("five/a", "{\"v\":5}", 1, false)).unwrap();
    assert_eq!(message(&next_event(&mut conn).await).0, Direction::Sent);
    assert_eq!(
        message(&next_event(&mut conn).await),
        (Direction::Received, "five/a".into(), "QoS 1".into(), "{\"v\":5}".into())
    );
    conn.session.send(SocketOutgoing::Subscribe { topic: "forbidden/x".into(), qos: 0 }).unwrap();
    assert!(error(&next_event(&mut conn).await).contains("not authorized"));
}

#[tokio::test]
async fn v5_against_a_v311_broker_says_so() {
    let (_broker, addr) = Broker::start(true).await;
    let err = Client::new()
        .mqtt(&addr.to_string(), &RequestOptions::default(), &config(MqttProtocol::V5, &[]))
        .await
        .err()
        .unwrap();
    assert!(err.message.contains("does not support MQTT 5"), "{}", err.message);
}

#[tokio::test]
async fn mqtts_uses_the_tls_settings() {
    let certs = TestCerts::generate();
    let (_broker, addr) = Broker::start_tls(&certs).await;
    let dir = tempfile::tempdir().unwrap();
    let opts = RequestOptions {
        tls: TlsOptions { ca_cert_path: Some(certs.write_ca(dir.path())), ..TlsOptions::default() },
        ..RequestOptions::default()
    };
    let url = format!("mqtts://localhost:{}", addr.port());
    let mut conn = Client::new().mqtt(&url, &opts, &config(MqttProtocol::V311, &[("t", 0)])).await.unwrap();
    assert!(conn.opened.protocol.starts_with("MQTT 3.1.1 + TLS"), "{}", conn.opened.protocol);
    assert!(conn.opened.tls.is_some());
    assert_eq!(info(&next_event(&mut conn).await), "Subscribed to t (QoS 0)");
    // Without the test CA the certificate is not trusted.
    let err =
        Client::new().mqtt(&url, &RequestOptions::default(), &config(MqttProtocol::V311, &[])).await.err().unwrap();
    assert_eq!(err.kind, zorvik_engine::ErrorKind::Tls, "{}", err.message);
}

#[tokio::test]
async fn nothing_listening_is_a_connect_error() {
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let err = Client::new()
        .mqtt(&format!("mqtt://127.0.0.1:{port}"), &RequestOptions::default(), &MqttConfig::default())
        .await
        .err()
        .unwrap();
    assert_eq!(err.kind, zorvik_engine::ErrorKind::Connect, "{}", err.message);
}
