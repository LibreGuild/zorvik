//! TLS client configuration and connection metadata.
//!
//! Certificates are verified with the operating system trust store
//! (`rustls-platform-verifier`), so corporate root CAs installed on Windows or
//! in the macOS keychain are trusted the same way browsers trust them.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::CryptoProvider;
use rustls::{ClientConfig, DigitallySignedStruct, SignatureScheme};
use rustls_pki_types::pem::PemObject;
use rustls_pki_types::{CertificateDer, PrivateKeyDer, ServerName, UnixTime};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::error::{EngineError, ErrorKind, Result};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TlsOptions {
    /// Verify the server certificate chain and host name.
    pub verify: bool,
    /// Extra trusted root CA certificates (PEM file), added to the OS trust store.
    pub ca_cert_path: Option<PathBuf>,
    /// Client certificate for mutual TLS (PEM file with one or more certificates).
    pub client_cert_path: Option<PathBuf>,
    /// Private key for the client certificate (PEM file).
    pub client_key_path: Option<PathBuf>,
}

impl Default for TlsOptions {
    fn default() -> Self {
        Self { verify: true, ca_cert_path: None, client_cert_path: None, client_key_path: None }
    }
}

/// What was negotiated on a TLS connection, shown in the response "Info" tab.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TlsInfo {
    pub version: String,
    pub cipher: String,
    pub alpn: Option<String>,
    pub certificate: Option<CertificateInfo>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CertificateInfo {
    pub subject: String,
    pub issuer: String,
    pub not_before: String,
    pub not_after: String,
    pub subject_alt_names: Vec<String>,
    pub serial: String,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Alpn {
    /// Offer h2 and http/1.1.
    Auto,
    Http1,
    Http2,
    /// No ALPN (raw TLS sockets).
    None,
    /// QUIC for HTTP/3: ALPN `h3` and TLS 1.3 only (see [`TlsConfigCache::quic`]).
    Http3,
}

impl Alpn {
    fn protocols(self) -> Vec<Vec<u8>> {
        match self {
            Alpn::Auto => vec![b"h2".to_vec(), b"http/1.1".to_vec()],
            Alpn::Http1 => vec![b"http/1.1".to_vec()],
            Alpn::Http2 => vec![b"h2".to_vec()],
            Alpn::None => Vec::new(),
            Alpn::Http3 => vec![b"h3".to_vec()],
        }
    }
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct CacheKey {
    options: TlsOptions,
    alpn: Alpn,
    /// Modification times of referenced files so edits on disk are picked up.
    mtimes: Vec<Option<SystemTime>>,
}

/// Builds and caches rustls client configs. Building the platform verifier
/// can be expensive (it may load the whole OS trust store), so configs are
/// reused across requests.
#[derive(Default)]
pub(crate) struct TlsConfigCache {
    configs: Mutex<HashMap<CacheKey, Arc<ClientConfig>>>,
}

impl TlsConfigCache {
    pub(crate) fn get(&self, options: &TlsOptions, alpn: Alpn) -> Result<Arc<ClientConfig>> {
        let mtimes = [&options.ca_cert_path, &options.client_cert_path, &options.client_key_path]
            .iter()
            .map(|p| p.as_ref().and_then(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok()))
            .collect();
        let key = CacheKey { options: options.clone(), alpn, mtimes };
        let mut configs = self.configs.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(config) = configs.get(&key) {
            return Ok(config.clone());
        }
        let config = Arc::new(build_config(options, alpn)?);
        if configs.len() > 32 {
            configs.clear();
        }
        configs.insert(key, config.clone());
        Ok(config)
    }

    /// Client config for QUIC (HTTP/3): TLS 1.3 only, ALPN `h3`, and the same
    /// verification, extra CA and client certificate rules as TCP connections.
    pub(crate) fn quic(&self, options: &TlsOptions) -> Result<Arc<ClientConfig>> {
        self.get(options, Alpn::Http3)
    }
}

fn provider() -> Arc<CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

fn tls_err(message: impl Into<String>) -> EngineError {
    EngineError::new(ErrorKind::Tls, message)
}

fn build_config(options: &TlsOptions, alpn: Alpn) -> Result<ClientConfig> {
    let provider = provider();
    let builder = ClientConfig::builder_with_provider(provider.clone());
    // QUIC is built on TLS 1.3 (RFC 9001); rustls refuses a QUIC config that allows less.
    let builder = if alpn == Alpn::Http3 {
        builder.with_protocol_versions(&[&rustls::version::TLS13])
    } else {
        builder.with_safe_default_protocol_versions()
    }
    .map_err(|e| tls_err(format!("TLS setup failed: {e}")))?;

    let verifier: Arc<dyn ServerCertVerifier> = if options.verify {
        let extra = match &options.ca_cert_path {
            Some(path) => read_certs(path, "CA certificate")?,
            None => Vec::new(),
        };
        let verifier = if extra.is_empty() {
            rustls_platform_verifier::Verifier::new(provider.clone())
        } else {
            rustls_platform_verifier::Verifier::new_with_extra_roots(extra, provider.clone())
        }
        .map_err(|e| tls_err(format!("Could not load trusted certificates: {e}")))?;
        Arc::new(verifier)
    } else {
        Arc::new(NoVerification(provider.clone()))
    };

    let builder = builder.dangerous().with_custom_certificate_verifier(verifier);
    let mut config = match (&options.client_cert_path, &options.client_key_path) {
        (Some(cert_path), Some(key_path)) => {
            let certs = read_certs(cert_path, "client certificate")?;
            let key = read_key(key_path)?;
            builder
                .with_client_auth_cert(certs, key)
                .map_err(|e| tls_err(format!("Invalid client certificate or key: {e}")))?
        }
        (None, None) => builder.with_no_client_auth(),
        _ => {
            return Err(EngineError::invalid("Client certificate and client key must both be set for mutual TLS"));
        }
    };
    config.alpn_protocols = alpn.protocols();
    Ok(config)
}

fn read_file(path: &Path, what: &str) -> Result<Vec<u8>> {
    std::fs::read(path)
        .map_err(|e| EngineError::invalid(format!("Could not read {what} file '{}': {e}", path.display())))
}

pub(crate) fn read_certs(path: &Path, what: &str) -> Result<Vec<CertificateDer<'static>>> {
    let data = read_file(path, what)?;
    let certs = CertificateDer::pem_slice_iter(&data)
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| EngineError::invalid(format!("Invalid PEM in {what} '{}': {e}", path.display())))?;
    if certs.is_empty() {
        return Err(EngineError::invalid(format!(
            "No certificates found in {what} file '{}' (expected PEM)",
            path.display()
        )));
    }
    Ok(certs)
}

fn read_key(path: &Path) -> Result<PrivateKeyDer<'static>> {
    let data = read_file(path, "client key")?;
    PrivateKeyDer::from_pem_slice(&data)
        .map_err(|e| EngineError::invalid(format!("No usable private key in '{}': {e}", path.display())))
}

pub(crate) fn server_name(host: &str) -> Result<ServerName<'static>> {
    ServerName::try_from(host.to_string())
        .map_err(|_| EngineError::invalid(format!("Invalid TLS server name '{host}'")))
}

/// Turn a rustls handshake failure into a message that tells the user what to do.
pub(crate) fn handshake_error(err: std::io::Error, host: &str) -> EngineError {
    let detail = err.get_ref().map(|inner| inner.to_string()).unwrap_or_else(|| err.to_string());
    handshake_failure(&detail, host)
}

/// A TLS handshake failure described by rustls' error text (also used for QUIC).
pub(crate) fn handshake_failure(detail: &str, host: &str) -> EngineError {
    let hint = if detail.contains("UnknownIssuer") || detail.contains("unknown issuer") {
        " The certificate is not trusted. If this is a corporate or self-signed certificate, add its CA in Settings > TLS or turn off certificate verification."
    } else if detail.contains("NotValidForName") || detail.contains("not valid for name") {
        " The certificate does not match the host name."
    } else if detail.contains("Expired") || detail.contains("expired") {
        " The certificate has expired."
    } else {
        ""
    };
    tls_err(format!("TLS handshake with {host} failed: {detail}.{hint}"))
}

pub(crate) fn connection_info(conn: &rustls::ClientConnection) -> TlsInfo {
    let version = match conn.protocol_version() {
        Some(rustls::ProtocolVersion::TLSv1_3) => "TLS 1.3".to_string(),
        Some(rustls::ProtocolVersion::TLSv1_2) => "TLS 1.2".to_string(),
        Some(other) => format!("{other:?}"),
        None => "unknown".to_string(),
    };
    let cipher = conn.negotiated_cipher_suite().map(|s| format!("{:?}", s.suite())).unwrap_or_default();
    let alpn = conn.alpn_protocol().map(|p| String::from_utf8_lossy(p).into_owned());
    let certificate =
        conn.peer_certificates().and_then(|certs| certs.first()).and_then(|cert| describe_certificate(cert.as_ref()));
    TlsInfo { version, cipher, alpn, certificate }
}

pub(crate) fn describe_certificate(der: &[u8]) -> Option<CertificateInfo> {
    use x509_parser::prelude::*;
    let (_, cert) = X509Certificate::from_der(der).ok()?;
    let fmt_time = |t: &ASN1Time| {
        t.to_datetime().format(&::time::format_description::well_known::Rfc3339).unwrap_or_else(|_| t.to_string())
    };
    let subject_alt_names = subject_alt_names(&cert);
    Some(CertificateInfo {
        subject: cert.subject().to_string(),
        issuer: cert.issuer().to_string(),
        not_before: fmt_time(&cert.validity().not_before),
        not_after: fmt_time(&cert.validity().not_after),
        subject_alt_names,
        serial: cert.raw_serial_as_string(),
    })
}

/// Subject alternative names as text (DNS names, IP addresses, others as printed).
pub(crate) fn subject_alt_names(cert: &x509_parser::certificate::X509Certificate<'_>) -> Vec<String> {
    use x509_parser::extensions::GeneralName;
    cert.subject_alternative_name()
        .ok()
        .flatten()
        .map(|ext| {
            ext.value
                .general_names
                .iter()
                .map(|name| match name {
                    GeneralName::DNSName(d) => d.to_string(),
                    GeneralName::IPAddress(ip) => match ip.len() {
                        4 => std::net::Ipv4Addr::new(ip[0], ip[1], ip[2], ip[3]).to_string(),
                        16 => {
                            let mut b = [0u8; 16];
                            b.copy_from_slice(ip);
                            std::net::Ipv6Addr::from(b).to_string()
                        }
                        _ => format!("{ip:?}"),
                    },
                    other => other.to_string(),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Accepts any server certificate. Only used when the user explicitly turns
/// verification off; signatures are still checked so the handshake is sane.
#[derive(Debug)]
struct NoVerification(Arc<CryptoProvider>);

impl ServerCertVerifier for NoVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> std::result::Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(message, cert, dss, &self.0.signature_verification_algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.0.signature_verification_algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}
