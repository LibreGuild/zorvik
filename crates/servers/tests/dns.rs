//! DNS server: records, wildcards, CNAMEs, upstreams, TCP, truncation, bad packets.

mod common;

use std::net::{Ipv4Addr, SocketAddr};
use std::time::Duration;

use common::{Events, start};
use hickory_proto::op::{Edns, Message, Query, ResponseCode};
use hickory_proto::rr::rdata::A;
use hickory_proto::rr::{Name, RData, Record, RecordType};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpStream, UdpSocket};
use zorvik_formats::{DnsRecord, Server, ServerKind};
use zorvik_servers::{OutgoingMessage, RunningServer, TrafficEntry, TrafficKind};

fn record(name: &str, rtype: &str, value: &str) -> DnsRecord {
    DnsRecord { name: name.into(), record_type: rtype.into(), value: value.into(), ttl: 60, enabled: true }
}

fn query(name: &str, qtype: RecordType) -> Message {
    let mut m = Message::query();
    m.metadata.recursion_desired = true;
    m.add_query(Query::query(Name::from_ascii(name).unwrap(), qtype));
    m
}

/// Send a datagram and wait up to `wait` for the answer.
async fn exchange_udp(addr: SocketAddr, bytes: &[u8], wait: Duration) -> Option<Vec<u8>> {
    let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    socket.send_to(bytes, addr).await.unwrap();
    let mut buf = vec![0u8; 65_536];
    let n = tokio::time::timeout(wait, socket.recv(&mut buf)).await.ok()?.unwrap();
    Some(buf[..n].to_vec())
}

const ANSWER: Duration = Duration::from_secs(6);
const NO_ANSWER: Duration = Duration::from_millis(300);

async fn ask(addr: SocketAddr, m: &Message) -> Message {
    let bytes = exchange_udp(addr, &m.to_vec().unwrap(), ANSWER).await.expect("an answer in time");
    let answer = Message::from_vec(&bytes).unwrap();
    assert_eq!(answer.metadata.id, m.metadata.id);
    answer
}

async fn ask_tcp(addr: SocketAddr, m: &Message) -> Message {
    let mut stream = TcpStream::connect(addr).await.unwrap();
    let bytes = m.to_vec().unwrap();
    stream.write_all(&(bytes.len() as u16).to_be_bytes()).await.unwrap();
    stream.write_all(&bytes).await.unwrap();
    let len = tokio::time::timeout(Duration::from_secs(5), stream.read_u16()).await.unwrap().unwrap();
    let mut answer = vec![0u8; len as usize];
    stream.read_exact(&mut answer).await.unwrap();
    Message::from_vec(&answer).unwrap()
}

/// Answer data as text, e.g. `["127.0.0.1"]`.
fn data(m: &Message) -> Vec<String> {
    m.answers.iter().map(|r| r.data.to_string()).collect()
}

async fn dns_server(records: Vec<DnsRecord>, upstream: &str) -> (RunningServer, Events) {
    let mut server = Server::new("DNS", ServerKind::Dns);
    server.dns.records = records;
    server.dns.upstream = upstream.into();
    start(server).await
}

async fn logged(events: &Events, summary: &str) -> TrafficEntry {
    events.wait(summary, |e| e.kind == TrafficKind::Dns && e.summary == summary).await
}

#[tokio::test]
async fn answers_from_records() {
    let long = "x".repeat(300);
    let (running, events) = dns_server(
        vec![
            record("api.example.test", "A", "127.0.0.1"),
            record("api.example.test", "A", "127.0.0.2"),
            record("api.example.test", "AAAA", "::1"),
            record("*.example.test", "A", "10.0.0.1"),
            record("www.example.test", "CNAME", "api.example.test."),
            record("example.test", "MX", "10 mail.example.test"),
            record("example.test", "TXT", &long),
            record("_sip._tcp.example.test", "SRV", "10 5 5060 sip.example.test"),
            record("example.test", "CAA", "0 issue letsencrypt.org"),
            record("127.0.0.1", "PTR", "api.example.test"),
            DnsRecord { enabled: false, ..record("off.example.test", "A", "1.1.1.1") },
        ],
        "",
    )
    .await;
    assert!(running.url.starts_with("dns://127.0.0.1:"), "{}", running.url);
    let addr = running.addr;

    let a = ask(addr, &query("API.Example.Test.", RecordType::A)).await;
    assert_eq!(a.metadata.response_code, ResponseCode::NoError);
    assert!(a.metadata.authoritative && a.metadata.recursion_desired && !a.metadata.recursion_available);
    assert_eq!(data(&a), ["127.0.0.1", "127.0.0.2"]);
    assert_eq!(a.answers[0].ttl, 60);
    // Logged with the question and the answer records in zone-file format.
    let entry = logged(&events, "A API.Example.Test → 127.0.0.1, 127.0.0.2").await;
    let details = entry.text.unwrap();
    assert!(details.starts_with(";; NOERROR"), "{details}");
    assert!(details.contains("API.Example.Test.\t60\tIN\tA\t127.0.0.1"), "{details}");
    assert!(entry.size > 0);

    assert_eq!(data(&ask(addr, &query("api.example.test", RecordType::AAAA)).await), ["::1"]);
    // Wildcards match one or more labels; answers carry the asked name.
    let w = ask(addr, &query("a.b.example.test", RecordType::A)).await;
    assert_eq!(data(&w), ["10.0.0.1"]);
    assert_eq!(w.answers[0].name.to_ascii(), "a.b.example.test.");
    // CNAMEs are followed within the records.
    let c = ask(addr, &query("www.example.test", RecordType::A)).await;
    assert_eq!(data(&c), ["api.example.test.", "127.0.0.1", "127.0.0.2"]);
    logged(&events, "A www.example.test → CNAME api.example.test, 127.0.0.1, 127.0.0.2").await;

    assert_eq!(data(&ask(addr, &query("example.test", RecordType::MX)).await), ["10 mail.example.test."]);
    let txt = ask(addr, &query("example.test", RecordType::TXT)).await;
    let RData::TXT(t) = &txt.answers[0].data else { panic!("{txt:?}") };
    assert_eq!(t.txt_data.iter().map(|s| s.len()).collect::<Vec<_>>(), [255, 45]);
    assert_eq!(
        data(&ask(addr, &query("_sip._tcp.example.test", RecordType::SRV)).await),
        ["10 5 5060 sip.example.test."]
    );
    assert_eq!(data(&ask(addr, &query("example.test", RecordType::CAA)).await), ["0 issue \"letsencrypt.org\""]);
    assert_eq!(data(&ask(addr, &query("1.0.0.127.in-addr.arpa", RecordType::PTR)).await), ["api.example.test."]);

    // No such name, a name without that type, a name above records, a disabled record.
    let nx = ask(addr, &query("nope.test", RecordType::MX)).await;
    assert_eq!((nx.metadata.response_code, nx.answers.len()), (ResponseCode::NXDomain, 0));
    logged(&events, "MX nope.test → NXDOMAIN").await;
    let nodata = ask(addr, &query("www2.example.test", RecordType::TXT)).await;
    assert_eq!((nodata.metadata.response_code, nodata.answers.len()), (ResponseCode::NoError, 0));
    assert_eq!(ask(addr, &query("test", RecordType::A)).await.metadata.response_code, ResponseCode::NoError);
    assert_eq!(data(&ask(addr, &query("off.example.test", RecordType::A)).await), ["10.0.0.1"]);

    // DNS servers have no connections to send to.
    assert!(running.send(None, OutgoingMessage::Text { text: "x".into() }).await.is_err());
}

#[tokio::test]
async fn invalid_records_are_reported_per_change() {
    let mut server = Server::new("DNS", ServerKind::Dns);
    server.port = 0;
    server.dns.records = vec![record("ok.test", "A", "127.0.0.1"), record("bad.test", "A", "localhost")];
    let (running, events) = start(server.clone()).await;
    let problems = |events: &Events| events.traffic().into_iter().filter(|e| e.kind == TrafficKind::Error).count();
    let first = events.wait_kind(TrafficKind::Error).await;
    assert!(first.summary.contains("DNS record 2 (A bad.test)"), "{}", first.summary);
    assert_eq!(data(&ask(running.addr, &query("ok.test", RecordType::A)).await), ["127.0.0.1"]);
    assert_eq!(problems(&events), 1);

    // Unrelated edits do not report again; record edits do.
    server.name = "Renamed".into();
    assert!(running.update(server.clone(), Default::default()));
    ask(running.addr, &query("ok.test", RecordType::A)).await;
    assert_eq!(problems(&events), 1);
    server.dns.records.push(record("mx.test", "MX", "mail.test"));
    server.dns.upstream = "not a server".into();
    assert!(running.update(server, Default::default()));
    // The broken upstream answers SERVFAIL for names without records.
    let fail = ask(running.addr, &query("elsewhere.test", RecordType::A)).await;
    assert_eq!(fail.metadata.response_code, ResponseCode::ServFail);
    assert_eq!(problems(&events), 4, "{:#?}", events.traffic());
}

/// A stand-in upstream DNS server: answers A queries with `ip`, first sending
/// an answer with the wrong id (which must be ignored).
async fn fake_upstream(ip: Ipv4Addr) -> SocketAddr {
    let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let addr = socket.local_addr().unwrap();
    tokio::spawn(async move {
        let mut buf = vec![0u8; 4096];
        while let Ok((n, peer)) = socket.recv_from(&mut buf).await {
            let Ok(q) = Message::from_vec(&buf[..n]) else { continue };
            let mut answer = Message::response(q.metadata.id, q.metadata.op_code);
            answer.metadata.recursion_available = true;
            answer.metadata.recursion_desired = q.metadata.recursion_desired;
            answer.add_query(q.queries[0].clone());
            answer.add_answer(Record::from_rdata(q.queries[0].name().clone(), 300, RData::A(A(ip))));
            let mut wrong = answer.clone();
            wrong.metadata.id = q.metadata.id.wrapping_add(1);
            let _ = socket.send_to(&wrong.to_vec().unwrap(), peer).await;
            let _ = socket.send_to(&answer.to_vec().unwrap(), peer).await;
        }
    });
    addr
}

#[tokio::test]
async fn forwards_unknown_names_and_follows_cnames_out() {
    let upstream = fake_upstream(Ipv4Addr::new(192, 0, 2, 7)).await;
    let (running, events) = dns_server(
        vec![record("mine.test", "A", "127.0.0.1"), record("out.mine.test", "CNAME", "far.away.test")],
        &upstream.to_string(),
    )
    .await;
    let addr = running.addr;

    let own = ask(addr, &query("mine.test", RecordType::A)).await;
    assert!(own.metadata.authoritative && own.metadata.recursion_available);

    let fwd = ask(addr, &query("elsewhere.test", RecordType::A)).await;
    assert_eq!(data(&fwd), ["192.0.2.7"]);
    assert!(!fwd.metadata.authoritative && fwd.metadata.recursion_available);
    let entry = logged(&events, &format!("A elsewhere.test → forwarded to {upstream} · NOERROR · 192.0.2.7")).await;
    assert!(entry.text.as_deref().unwrap().contains("elsewhere.test.\t300\tIN\tA\t192.0.2.7"), "{:?}", entry.text);

    // A CNAME to a name outside the records is resolved upstream.
    let chased = ask(addr, &query("out.mine.test", RecordType::A)).await;
    assert_eq!(data(&chased), ["far.away.test.", "192.0.2.7"]);

    // Over TCP too.
    assert_eq!(data(&ask_tcp(addr, &query("tcp.elsewhere.test", RecordType::A)).await), ["192.0.2.7"]);
    logged(&events, &format!("A tcp.elsewhere.test → forwarded to {upstream} · NOERROR · 192.0.2.7 · TCP")).await;
}

#[tokio::test]
async fn silent_upstream_is_servfail() {
    // Bound but never answering.
    let silent = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let (running, events) = dns_server(vec![], &silent.local_addr().unwrap().to_string()).await;
    let answer = ask(running.addr, &query("slow.test", RecordType::A)).await;
    assert_eq!(answer.metadata.response_code, ResponseCode::ServFail);
    let entry = events.wait("servfail", |e| e.kind == TrafficKind::Dns && e.summary.contains("SERVFAIL")).await;
    assert!(entry.summary.contains("did not answer within 3 s"), "{}", entry.summary);
}

#[tokio::test]
async fn system_resolver_for_addresses_only() {
    let (running, _events) = dns_server(vec![record("mine.test", "TXT", "hello")], "system").await;
    let addr = running.addr;
    let local = ask(addr, &query("localhost", RecordType::A)).await;
    assert_eq!(local.metadata.response_code, ResponseCode::NoError);
    assert!(data(&local).contains(&"127.0.0.1".to_string()), "{local:?}");
    assert!(local.metadata.recursion_available);
    // Other types: the resolver cannot answer them.
    assert_eq!(ask(addr, &query("localhost", RecordType::MX)).await.metadata.response_code, ResponseCode::NotImp);
    // A name with records of other types: no data rather than "not implemented".
    let nodata = ask(addr, &query("mine.test", RecordType::MX)).await;
    assert_eq!((nodata.metadata.response_code, nodata.answers.len()), (ResponseCode::NoError, 0));
}

#[tokio::test]
async fn big_answers_are_truncated_over_udp_but_not_tcp() {
    let records =
        (0..60).map(|i| record("many.test", "TXT", &format!("record number {i} {}", "y".repeat(20)))).collect();
    let (running, events) = dns_server(records, "").await;
    let addr = running.addr;

    let udp = ask(addr, &query("many.test", RecordType::TXT)).await;
    assert!(udp.metadata.truncation && udp.answers.is_empty());
    assert_eq!(udp.queries.len(), 1);
    events.wait("truncated", |e| e.summary.contains("truncated")).await;

    // EDNS lets the client take more (up to 4096 bytes).
    let mut with_edns = query("many.test", RecordType::TXT);
    let mut edns = Edns::new();
    edns.set_max_payload(4096);
    with_edns.set_edns(edns);
    let bigger = ask(addr, &with_edns).await;
    assert!(!bigger.metadata.truncation && bigger.answers.len() == 60);
    assert!(bigger.edns.is_some());

    let tcp = ask_tcp(addr, &query("many.test", RecordType::TXT)).await;
    assert!(!tcp.metadata.truncation && tcp.answers.len() == 60);
}

#[tokio::test]
async fn malformed_packets() {
    let (running, events) = dns_server(vec![record("a.test", "A", "127.0.0.1")], "").await;
    let addr = running.addr;

    // Too short for a header: dropped (and reported).
    assert!(exchange_udp(addr, &[1, 2, 3], NO_ANSWER).await.is_none());
    events.wait("short", |e| e.kind == TrafficKind::Error && e.summary.contains("3-byte")).await;

    // A readable header with garbage after it: FORMERR with the same id.
    let mut garbage = vec![0xab, 0xcd, 0x01, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
    garbage.extend_from_slice(&[0xff; 20]);
    let answer = exchange_udp(addr, &garbage, ANSWER).await.expect("FORMERR");
    assert_eq!((answer[0], answer[1], answer[2] & 0x80, answer[3] & 0x0f), (0xab, 0xcd, 0x80, 1));
    logged(&events, "Malformed query → FORMERR").await;

    // Claiming 65535 questions is refused without parsing.
    let mut many = garbage.clone();
    many[4] = 0xff;
    many[5] = 0xff;
    assert_eq!(exchange_udp(addr, &many, ANSWER).await.expect("FORMERR")[3] & 0x0f, 1);

    // Other opcodes are not implemented.
    let mut notify = query("a.test", RecordType::A).to_vec().unwrap();
    notify[2] |= 4 << 3;
    assert_eq!(exchange_udp(addr, &notify, ANSWER).await.expect("NOTIMP")[3] & 0x0f, 4);

    // Responses sent to the server are not answered.
    let mut response = query("a.test", RecordType::A).to_vec().unwrap();
    response[2] |= 0x80;
    assert!(exchange_udp(addr, &response, NO_ANSWER).await.is_none());

    // Still serving.
    assert_eq!(data(&ask(addr, &query("a.test", RecordType::A)).await), ["127.0.0.1"]);
    assert!(events.stopped().is_none());
}
