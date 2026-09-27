//! `zorvik-testserver`: run the test endpoints locally for manual QA and E2E tests.
//!
//! Usage: `zorvik-testserver [--port 18787]` → HTTP on PORT, HTTPS on PORT+1,
//! gRPC (plaintext) on PORT+3, gRPC over TLS on PORT+4.
//! The HTTPS CA certificate is written next to the temp dir and printed.

use zorvik_testkit::grpc::Reflection;
use zorvik_testkit::{GrpcTestServer, TestCerts, TestServer};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().with_env_filter("warn").init();
    let args: Vec<String> = std::env::args().collect();
    let port: u16 = args
        .iter()
        .position(|a| a == "--port")
        .and_then(|i| args.get(i + 1))
        .and_then(|p| p.parse().ok())
        .unwrap_or(18787);
    let certs = TestCerts::generate();
    let ca_path = certs.write_ca(&std::env::temp_dir());
    let http = TestServer::bind(([127, 0, 0, 1], port).into(), None).await;
    let https = TestServer::bind(([127, 0, 0, 1], port + 1).into(), Some(&certs)).await;
    let grpc = GrpcTestServer::bind(([127, 0, 0, 1], port + 3).into(), None, Reflection::Both).await;
    let grpcs = GrpcTestServer::bind(([127, 0, 0, 1], port + 4).into(), Some(&certs), Reflection::Both).await;
    println!("HTTP   {}", http.url("/"));
    println!("HTTPS  {}  (CA: {})", https.url("/"), ca_path.display());
    println!("WS     {}", http.ws_url("/ws"));
    println!(
        "gRPC   {}  (TLS: {}, protos: {})",
        grpc.url(),
        grpcs.url(),
        zorvik_testkit::grpc::grpc_proto_dir().display()
    );
    println!("Ready.");
    tokio::signal::ctrl_c().await.ok();
}
