//! TLS for listeners: the user's certificate and key (PEM), or a self-signed
//! certificate for localhost generated once per run of the app.

use std::path::Path;
use std::sync::{Arc, OnceLock};

use rustls::ServerConfig;
use rustls_pki_types::pem::PemObject;
use rustls_pki_types::{CertificateDer, PrivateKeyDer};
use zorvik_formats::ServerTls;

use crate::ServerError;

struct SelfSigned {
    cert: CertificateDer<'static>,
    key: Vec<u8>,
}

fn self_signed() -> Result<&'static SelfSigned, ServerError> {
    static CERT: OnceLock<Result<SelfSigned, String>> = OnceLock::new();
    CERT.get_or_init(|| {
        let names = vec!["localhost".to_string(), "127.0.0.1".to_string(), "::1".to_string()];
        let key = rcgen::KeyPair::generate().map_err(|e| e.to_string())?;
        let mut params = rcgen::CertificateParams::new(names).map_err(|e| e.to_string())?;
        params.distinguished_name.push(rcgen::DnType::CommonName, "Zorvik local server");
        let cert = params.self_signed(&key).map_err(|e| e.to_string())?;
        Ok(SelfSigned { cert: cert.der().clone(), key: key.serialize_der() })
    })
    .as_ref()
    .map_err(|e| ServerError::new(format!("Could not create a self-signed certificate: {e}")))
}

fn resolve(base_dir: &Path, path: &str) -> std::path::PathBuf {
    let p = Path::new(path.trim());
    if p.is_absolute() { p.to_path_buf() } else { base_dir.join(p) }
}

/// A PEM file of reasonable size. The path comes from a (possibly shared)
/// server file: a device such as /dev/zero or a pipe must not hang the start.
fn read_pem(path: &Path, what: &str) -> Result<Vec<u8>, ServerError> {
    use std::io::Read as _;
    const MAX_PEM: u64 = 1 << 20;
    let failed = |e: std::io::Error| ServerError::new(format!("Could not read {what} '{}': {e}", path.display()));
    let meta = std::fs::metadata(path).map_err(failed)?;
    if !meta.is_file() {
        return Err(ServerError::new(format!("The {what} '{}' is not a regular file", path.display())));
    }
    let mut data = Vec::new();
    std::fs::File::open(path).and_then(|f| f.take(MAX_PEM + 1).read_to_end(&mut data)).map_err(failed)?;
    if data.len() as u64 > MAX_PEM {
        return Err(ServerError::new(format!("The {what} '{}' is larger than 1 MB", path.display())));
    }
    Ok(data)
}

/// Server-side TLS config with the given ALPN protocols (e.g. `h2`, `http/1.1`).
pub fn server_config(tls: &ServerTls, base_dir: &Path, alpn: &[&[u8]]) -> Result<Arc<ServerConfig>, ServerError> {
    let (certs, key) = if tls.cert_path.trim().is_empty() && tls.key_path.trim().is_empty() {
        let s = self_signed()?;
        (vec![s.cert.clone()], PrivateKeyDer::try_from(s.key.clone()).map_err(ServerError::new)?)
    } else {
        if tls.cert_path.trim().is_empty() || tls.key_path.trim().is_empty() {
            return Err(ServerError::new(
                "Set both the certificate and the key file (or neither for a self-signed one)",
            ));
        }
        let cert_file = resolve(base_dir, &tls.cert_path);
        let data = read_pem(&cert_file, "certificate")?;
        let certs = CertificateDer::pem_slice_iter(&data)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| ServerError::new(format!("Invalid PEM in '{}': {e}", cert_file.display())))?;
        if certs.is_empty() {
            return Err(ServerError::new(format!("No certificate in '{}' (expected PEM)", cert_file.display())));
        }
        let key_file = resolve(base_dir, &tls.key_path);
        let data = read_pem(&key_file, "key")?;
        let key = PrivateKeyDer::from_pem_slice(&data)
            .map_err(|e| ServerError::new(format!("No usable private key in '{}': {e}", key_file.display())))?;
        (certs, key)
    };
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let mut config = ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| ServerError::new(format!("TLS setup failed: {e}")))?
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|e| ServerError::new(format!("Certificate and key don't match: {e}")))?;
    config.alpn_protocols = alpn.iter().map(|p| p.to_vec()).collect();
    Ok(Arc::new(config))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn self_signed_and_missing_files() {
        let tls = ServerTls { enabled: true, ..Default::default() };
        let config = server_config(&tls, Path::new("."), &[b"h2"]).unwrap();
        assert_eq!(config.alpn_protocols, vec![b"h2".to_vec()]);
        let only_cert = ServerTls { enabled: true, cert_path: "c.pem".into(), key_path: String::new() };
        assert!(server_config(&only_cert, Path::new("."), &[]).is_err());
        let missing = ServerTls { enabled: true, cert_path: "nope.pem".into(), key_path: "nope.key".into() };
        let err = server_config(&missing, Path::new("/tmp"), &[]).unwrap_err();
        assert!(err.message.contains("nope.pem"), "{}", err.message);
    }
}
