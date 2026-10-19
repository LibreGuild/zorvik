//! Certificate decoder: what a PEM file (or a DER certificate) holds. Certificates are
//! described like the TLS inspector's chain, plus key usages and the certificate alone as
//! PEM; requests (CSRs) by subject, names and key; keys by type and size. A private key is
//! never echoed back: only its type, and which certificates carry its public key.
//! Findings cover expiry, weak keys and signatures, the order of a chain, and a private
//! key that matches none of the certificates.

use base64::Engine as _;
use rustls_pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs1KeyDer, PrivatePkcs8KeyDer, PrivateSec1KeyDer};
use serde::Serialize;
use time::OffsetDateTime;
use ts_rs::TS;
use x509_parser::certification_request::X509CertificationRequest;
use x509_parser::extensions::{GeneralName, ParsedExtension};
use x509_parser::pem::Pem;
use x509_parser::prelude::{FromDer, X509Certificate};
use x509_parser::x509::SubjectPublicKeyInfo;

use super::tls_inspect::{TlsCertificate, TlsWarning, TlsWarningLevel, describe, key_info, signature_name};
use crate::error::{EngineError, Result};

/// Size of the input.
pub const MAX_INPUT: usize = 1024 * 1024;
/// PEM blocks read.
const MAX_ITEMS: usize = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum DecodedKind {
    Certificate,
    /// A certificate signing request (CSR).
    Request,
    PrivateKey,
    PublicKey,
    /// Anything else (a CRL, EC parameters…): only its label is shown.
    Other,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DecodedItem {
    pub kind: DecodedKind,
    /// The PEM label (`CERTIFICATE`, `PRIVATE KEY`, …), or `DER` for binary input.
    pub label: String,
    pub certificate: Option<DecodedCertificate>,
    pub request: Option<DecodedRequest>,
    pub key: Option<DecodedKey>,
    /// Something to know about this item.
    pub note: Option<String>,
    /// Why the item could not be read.
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DecodedCertificate {
    pub details: TlsCertificate,
    /// X.509 version (3 for nearly every certificate in use).
    pub version: u32,
    /// `Digital Signature`, `Key Encipherment`, …
    pub key_usage: Vec<String>,
    /// `Server authentication`, `Client authentication`, …
    pub extended_key_usage: Vec<String>,
    /// This certificate alone, as PEM.
    pub pem: String,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DecodedRequest {
    pub subject: String,
    pub common_name: Option<String>,
    /// Names asked for in the request's extensions.
    pub subject_alt_names: Vec<String>,
    pub key_type: String,
    pub key_bits: Option<u32>,
    pub key_curve: Option<String>,
    pub signature_algorithm: String,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DecodedKey {
    /// `RSA`, `EC`, `Ed25519`, …; empty when unknown.
    pub key_type: String,
    pub key_bits: Option<u32>,
    pub key_curve: Option<String>,
    pub private: bool,
    /// Protected with a password, so it can't be read.
    pub encrypted: bool,
    /// Items (indexes) with the same public key: the certificates and requests this key
    /// belongs to.
    pub matches: Vec<u32>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CertDecodeReport {
    /// In input order.
    pub items: Vec<DecodedItem>,
    /// Most severe first. Codes: `expired`, `notYetValid`, `expiresSoon`, `weakKey`,
    /// `weakSignature`, `chainOrder`, `chainReversed`, `keyMismatch`.
    pub warnings: Vec<TlsWarning>,
}

/// An item and what the checks across items need (not sent).
struct Decoded {
    item: DecodedItem,
    /// The subject public key's bits.
    public_key: Option<Vec<u8>>,
    /// Raw names of a certificate.
    names: Option<Names>,
}

struct Names {
    subject: Vec<u8>,
    issuer: Vec<u8>,
}

/// Decode PEM text (any number of blocks) or one DER certificate or request (binary, or
/// base64 without PEM lines).
pub fn decode_certificates(input: &[u8]) -> Result<CertDecodeReport> {
    if input.len() > MAX_INPUT {
        return Err(EngineError::invalid(format!("That is more than {} MB", MAX_INPUT / (1024 * 1024))));
    }
    let now = OffsetDateTime::now_utc();
    let mut decoded = Vec::new();
    if contains(input, b"-----BEGIN ") {
        for block in Pem::iter_from_buffer(input).take(MAX_ITEMS) {
            decoded.push(match block {
                Ok(pem) => decode_block(&pem.label, &pem.contents, now),
                Err(e) => failed(DecodedKind::Other, "", format!("A PEM block could not be read ({e})")),
            });
        }
    } else {
        let der = match base64_text(input) {
            Some(der) => der,
            None if looks_like_text(input) => return Err(not_recognized()),
            None => input.to_vec(),
        };
        decoded.push(decode_der(&der, now)?);
    }
    if decoded.is_empty() {
        return Err(not_recognized());
    }
    let warnings = findings(&mut decoded);
    Ok(CertDecodeReport { items: decoded.into_iter().map(|d| d.item).collect(), warnings })
}

fn not_recognized() -> EngineError {
    EngineError::invalid(
        "Paste a certificate, a chain, a certificate request or a key in PEM format \
         (-----BEGIN CERTIFICATE----- …), or open a .pem, .crt, .cer or .der file",
    )
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

fn looks_like_text(input: &[u8]) -> bool {
    std::str::from_utf8(input).is_ok_and(|s| s.chars().all(|c| !c.is_control() || c.is_whitespace()))
}

/// Base64 without PEM lines (whitespace allowed, padding optional).
fn base64_text(input: &[u8]) -> Option<Vec<u8>> {
    let text = std::str::from_utf8(input).ok()?;
    let compact: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    if compact.len() < 16 || !compact.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'=')) {
        return None;
    }
    let engine = base64::engine::GeneralPurpose::new(
        &base64::alphabet::STANDARD,
        base64::engine::GeneralPurposeConfig::new()
            .with_decode_padding_mode(base64::engine::DecodePaddingMode::Indifferent),
    );
    engine.decode(compact).ok()
}

/// A binary certificate or request; other DER content is refused.
fn decode_der(der: &[u8], now: OffsetDateTime) -> Result<Decoded> {
    if X509Certificate::from_der(der).is_ok() {
        return Ok(decode_block("CERTIFICATE", der, now)).map(|mut d| {
            d.item.label = "DER".into();
            d
        });
    }
    if X509CertificationRequest::from_der(der).is_ok() {
        return Ok(decode_block("CERTIFICATE REQUEST", der, now)).map(|mut d| {
            d.item.label = "DER".into();
            d
        });
    }
    // PKCS#12: SEQUENCE { INTEGER 3, … }.
    let pkcs12 = der.first() == Some(&0x30) && der.windows(3).take(8).any(|w| w == [0x02, 0x01, 0x03]);
    if pkcs12 {
        return Err(EngineError::invalid(
            "This looks like a PKCS#12 file (.p12, .pfx), which can't be read here. Export its certificate as PEM \
             first, e.g. openssl pkcs12 -in file.p12 -nokeys",
        ));
    }
    Err(not_recognized())
}

fn item(kind: DecodedKind, label: &str) -> DecodedItem {
    DecodedItem { kind, label: label.to_string(), certificate: None, request: None, key: None, note: None, error: None }
}

fn failed(kind: DecodedKind, label: &str, error: String) -> Decoded {
    Decoded { item: DecodedItem { error: Some(error), ..item(kind, label) }, public_key: None, names: None }
}

fn decode_block(label: &str, der: &[u8], now: OffsetDateTime) -> Decoded {
    match label {
        "CERTIFICATE" | "X509 CERTIFICATE" | "TRUSTED CERTIFICATE" => certificate(label, der, now),
        "CERTIFICATE REQUEST" | "NEW CERTIFICATE REQUEST" => request(label, der),
        "PRIVATE KEY" => private_key(label, PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(der.to_vec()))),
        "RSA PRIVATE KEY" => private_key(label, PrivateKeyDer::Pkcs1(PrivatePkcs1KeyDer::from(der.to_vec()))),
        "EC PRIVATE KEY" => private_key(label, PrivateKeyDer::Sec1(PrivateSec1KeyDer::from(der.to_vec()))),
        "ENCRYPTED PRIVATE KEY" => Decoded {
            item: DecodedItem {
                key: Some(DecodedKey {
                    key_type: String::new(),
                    key_bits: None,
                    key_curve: None,
                    private: true,
                    encrypted: true,
                    matches: Vec::new(),
                }),
                note: Some("Protected with a password, so its type and certificate can't be checked.".into()),
                ..item(DecodedKind::PrivateKey, label)
            },
            public_key: None,
            names: None,
        },
        "PUBLIC KEY" => public_key(label, der),
        "RSA PUBLIC KEY" => Decoded {
            item: DecodedItem {
                key: Some(DecodedKey {
                    key_type: "RSA".into(),
                    key_bits: None,
                    key_curve: None,
                    private: false,
                    encrypted: false,
                    matches: Vec::new(),
                }),
                ..item(DecodedKind::PublicKey, label)
            },
            public_key: None,
            names: None,
        },
        "X509 CRL" => Decoded {
            item: DecodedItem {
                note: Some("A certificate revocation list. Its contents aren't shown here.".into()),
                ..item(DecodedKind::Other, label)
            },
            public_key: None,
            names: None,
        },
        "EC PARAMETERS" => Decoded {
            item: DecodedItem {
                note: Some("The curve of an EC key (written by openssl ecparam); the key follows.".into()),
                ..item(DecodedKind::Other, label)
            },
            public_key: None,
            names: None,
        },
        other => Decoded {
            item: DecodedItem {
                note: Some(format!("'{other}' blocks aren't decoded here.")),
                ..item(DecodedKind::Other, label)
            },
            public_key: None,
            names: None,
        },
    }
}

fn certificate(label: &str, der: &[u8], now: OffsetDateTime) -> Decoded {
    let cert = match X509Certificate::from_der(der) {
        Ok((_, cert)) => cert,
        Err(e) => return failed(DecodedKind::Certificate, label, format!("Could not parse the certificate: {e}")),
    };
    let key_usage = cert
        .key_usage()
        .ok()
        .flatten()
        .map(|ku| ku.value.to_string().split(", ").filter(|s| !s.is_empty()).map(str::to_string).collect())
        .unwrap_or_default();
    let extended_key_usage = cert
        .extended_key_usage()
        .ok()
        .flatten()
        .map(|eku| {
            let e = eku.value;
            let mut out: Vec<String> = [
                (e.any, "Any purpose"),
                (e.server_auth, "Server authentication"),
                (e.client_auth, "Client authentication"),
                (e.code_signing, "Code signing"),
                (e.email_protection, "Email protection"),
                (e.time_stamping, "Time stamping"),
                (e.ocsp_signing, "OCSP signing"),
            ]
            .into_iter()
            .filter(|(on, _)| *on)
            .map(|(_, name)| name.to_string())
            .collect();
            out.extend(e.other.iter().map(|oid| oid.to_id_string()));
            out
        })
        .unwrap_or_default();
    let details = describe(&CertificateDer::from(der.to_vec()), now);
    Decoded {
        public_key: Some(cert.public_key().subject_public_key.data.to_vec()),
        names: Some(Names { subject: cert.subject().as_raw().to_vec(), issuer: cert.issuer().as_raw().to_vec() }),
        item: DecodedItem {
            certificate: Some(DecodedCertificate {
                details,
                version: cert.version().0 + 1,
                key_usage,
                extended_key_usage,
                pem: pem("CERTIFICATE", der),
            }),
            ..item(DecodedKind::Certificate, label)
        },
    }
}

fn request(label: &str, der: &[u8]) -> Decoded {
    let csr = match X509CertificationRequest::from_der(der) {
        Ok((_, csr)) => csr,
        Err(e) => return failed(DecodedKind::Request, label, format!("Could not parse the certificate request: {e}")),
    };
    let info = &csr.certification_request_info;
    let mut names = Vec::new();
    for ext in csr.requested_extensions().into_iter().flatten() {
        if let ParsedExtension::SubjectAlternativeName(san) = ext {
            names.extend(san.general_names.iter().map(general_name));
        }
    }
    let (key_type, key_bits, key_curve) = key_info(&info.subject_pki);
    Decoded {
        public_key: Some(info.subject_pki.subject_public_key.data.to_vec()),
        names: None,
        item: DecodedItem {
            request: Some(DecodedRequest {
                subject: info.subject.to_string(),
                common_name: info.subject.iter_common_name().next().and_then(|cn| cn.as_str().ok()).map(str::to_string),
                subject_alt_names: names,
                key_type,
                key_bits,
                key_curve,
                signature_algorithm: signature_name(&csr.signature_algorithm.algorithm.to_id_string()),
            }),
            ..item(DecodedKind::Request, label)
        },
    }
}

fn private_key(label: &str, der: PrivateKeyDer<'static>) -> Decoded {
    // The public half, derived from the key, says what it is and which certificate it belongs to.
    let spki =
        rustls::crypto::ring::sign::any_supported_type(&der).ok().and_then(|k| k.public_key().map(|p| p.to_vec()));
    let parsed = spki.as_deref().and_then(|spki| SubjectPublicKeyInfo::from_der(spki).ok().map(|(_, info)| info));
    let (key, public_key, note) = match parsed {
        Some(info) => {
            let (key_type, key_bits, key_curve) = key_info(&info);
            let key =
                DecodedKey { key_type, key_bits, key_curve, private: true, encrypted: false, matches: Vec::new() };
            (key, Some(info.subject_public_key.data.to_vec()), None)
        }
        None => {
            let (key_type, key_curve) = key_algorithm(label, der.secret_der());
            let key = DecodedKey {
                key_type,
                key_bits: None,
                key_curve,
                private: true,
                encrypted: false,
                matches: Vec::new(),
            };
            let note = "This kind of key can't be read here, so its size and certificate can't be checked.";
            (key, None, Some(note.to_string()))
        }
    };
    Decoded {
        item: DecodedItem { key: Some(key), note, ..item(DecodedKind::PrivateKey, label) },
        public_key,
        names: None,
    }
}

fn public_key(label: &str, der: &[u8]) -> Decoded {
    match SubjectPublicKeyInfo::from_der(der) {
        Ok((_, info)) => {
            let (key_type, key_bits, key_curve) = key_info(&info);
            Decoded {
                public_key: Some(info.subject_public_key.data.to_vec()),
                names: None,
                item: DecodedItem {
                    key: Some(DecodedKey {
                        key_type,
                        key_bits,
                        key_curve,
                        private: false,
                        encrypted: false,
                        matches: Vec::new(),
                    }),
                    ..item(DecodedKind::PublicKey, label)
                },
            }
        }
        Err(e) => failed(DecodedKind::PublicKey, label, format!("Could not parse the public key: {e}")),
    }
}

/// The algorithm of a key the TLS library can't load, from its label or the algorithm
/// identifier near the start of a PKCS#8 key.
fn key_algorithm(label: &str, der: &[u8]) -> (String, Option<String>) {
    if label.starts_with("RSA") {
        return ("RSA".into(), None);
    }
    let head = &der[..der.len().min(48)];
    let has = |oid: &[u8]| contains(head, oid);
    let curve = if has(&[0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x03, 0x01, 0x07]) {
        Some("P-256".to_string())
    } else if has(&[0x2B, 0x81, 0x04, 0x00, 0x22]) {
        Some("P-384".to_string())
    } else if has(&[0x2B, 0x81, 0x04, 0x00, 0x23]) {
        Some("P-521".to_string())
    } else if has(&[0x2B, 0x81, 0x04, 0x00, 0x0A]) {
        Some("secp256k1".to_string())
    } else {
        None
    };
    if label.starts_with("EC") || has(&[0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x02, 0x01]) {
        ("EC".into(), curve)
    } else if has(&[0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, 0x01]) {
        ("RSA".into(), None)
    } else if has(&[0x06, 0x03, 0x2B, 0x65, 0x70]) {
        ("Ed25519".into(), None)
    } else if has(&[0x06, 0x03, 0x2B, 0x65, 0x71]) {
        ("Ed448".into(), None)
    } else {
        (String::new(), None)
    }
}

fn general_name(name: &GeneralName<'_>) -> String {
    match name {
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
    }
}

fn pem(label: &str, der: &[u8]) -> String {
    let b64 = base64::engine::general_purpose::STANDARD.encode(der);
    let mut out = format!("-----BEGIN {label}-----\n");
    for line in b64.as_bytes().chunks(64) {
        out.push_str(std::str::from_utf8(line).unwrap_or_default());
        out.push('\n');
    }
    out.push_str(&format!("-----END {label}-----\n"));
    out
}

// ---- findings across items ------------------------------------------------------

fn findings(decoded: &mut [Decoded]) -> Vec<TlsWarning> {
    let mut out = Vec::new();
    let mut add = |level, code: &str, message: String| out.push(TlsWarning { level, code: code.into(), message });
    let date = |rfc3339: &str| rfc3339.split('T').next().unwrap_or(rfc3339).to_string();
    let plural = |n: i64| if n == 1 { "" } else { "s" };

    // Each certificate on its own.
    for d in decoded.iter() {
        let Some(cert) = d.item.certificate.as_ref().map(|c| &c.details).filter(|c| c.parse_error.is_none()) else {
            continue;
        };
        let name = label(cert);
        if cert.days_left < 0 {
            add(TlsWarningLevel::Danger, "expired", format!("'{name}' expired on {}.", date(&cert.not_after)));
        } else if cert.not_yet_valid {
            add(
                TlsWarningLevel::Danger,
                "notYetValid",
                format!("'{name}' is not valid until {}.", date(&cert.not_before)),
            );
        } else if cert.days_left < 30 {
            add(
                TlsWarningLevel::Warning,
                "expiresSoon",
                format!(
                    "'{name}' expires in {} day{} ({}).",
                    cert.days_left,
                    plural(cert.days_left),
                    date(&cert.not_after)
                ),
            );
        }
        if let Some(weak) = weak_key(&cert.key_type, cert.key_bits) {
            add(TlsWarningLevel::Warning, "weakKey", format!("'{name}' uses {weak} (use RSA 2048+ or ECDSA P-256+)."));
        }
        // A root's own signature is never checked.
        let root = cert.self_signed && cert.is_ca;
        if !root && (cert.signature_algorithm.contains("SHA-1") || cert.signature_algorithm.contains("MD5")) {
            add(
                TlsWarningLevel::Warning,
                "weakSignature",
                format!("'{name}' is signed with {}, which is no longer secure.", cert.signature_algorithm),
            );
        }
    }
    for d in decoded.iter() {
        if let Some(r) = &d.item.request
            && let Some(weak) = weak_key(&r.key_type, r.key_bits)
        {
            let name = r.common_name.clone().unwrap_or_else(|| r.subject.clone());
            add(TlsWarningLevel::Warning, "weakKey", format!("The request for '{name}' uses {weak}."));
        }
    }

    // The order of a chain: each certificate's issuer right after it. A self-signed
    // certificate may end a chain (a bundle of root certificates is fine).
    let certs: Vec<(&TlsCertificate, &Names)> =
        decoded.iter().filter_map(|d| Some((&d.item.certificate.as_ref()?.details, d.names.as_ref()?))).collect();
    if certs.len() >= 2 {
        let (subject, issuer) = (|i: usize| &certs[i].1.subject, |i: usize| &certs[i].1.issuer);
        let issued_by_next =
            |i: usize| issuer(i) == subject(i + 1) || (certs[i].0.self_signed && issuer(i + 1) != subject(i));
        let issued_by_previous = |i: usize| issuer(i + 1) == subject(i);
        let pairs = 0..certs.len() - 1;
        if !pairs.clone().all(issued_by_next) {
            if pairs.clone().all(issued_by_previous) {
                add(
                    TlsWarningLevel::Info,
                    "chainReversed",
                    "The certificates are in reverse order. A server sends its own certificate first, then the \
                     one that issued it, and so on."
                        .into(),
                );
            } else {
                for i in pairs.filter(|&i| !issued_by_next(i)) {
                    add(
                        TlsWarningLevel::Warning,
                        "chainOrder",
                        format!(
                            "'{}' did not issue '{}', the certificate before it. A chain lists each certificate's \
                             issuer right after it (is one missing?).",
                            label(certs[i + 1].0),
                            label(certs[i].0)
                        ),
                    );
                }
            }
        }
    }

    // Which certificates and requests each key belongs to.
    let holders: Vec<(usize, Vec<u8>)> = decoded
        .iter()
        .enumerate()
        .filter(|(_, d)| matches!(d.item.kind, DecodedKind::Certificate | DecodedKind::Request))
        .filter_map(|(i, d)| Some((i, d.public_key.clone()?)))
        .collect();
    let any_certificate = decoded.iter().any(|d| d.item.certificate.is_some());
    for d in decoded.iter_mut() {
        let (Some(key), Some(bits)) = (d.item.key.as_mut(), d.public_key.as_ref()) else { continue };
        key.matches = holders.iter().filter(|(_, b)| b == bits).map(|(i, _)| *i as u32).collect();
        if key.private && any_certificate && key.matches.is_empty() {
            add(TlsWarningLevel::Danger, "keyMismatch", "The private key doesn't match any certificate here.".into());
        }
    }
    out.sort_by_key(|w| w.level);
    out
}

fn label(cert: &TlsCertificate) -> String {
    cert.common_name.clone().unwrap_or_else(|| cert.subject.clone())
}

fn weak_key(key_type: &str, bits: Option<u32>) -> Option<String> {
    match (key_type, bits) {
        ("RSA" | "DSA", Some(bits)) if bits < 2048 => Some(format!("a {bits}-bit {key_type} key")),
        ("EC", Some(bits)) if bits < 256 => Some(format!("a {bits}-bit EC key")),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use rcgen::{CertificateParams, DistinguishedName, DnType, IsCa, Issuer, KeyPair};

    use super::*;

    struct Issued {
        cert_pem: String,
        key_pem: String,
    }

    struct Ca {
        cert: rcgen::Certificate,
        issuer: Issuer<'static, KeyPair>,
    }

    fn ca(name: &str) -> Ca {
        let key = KeyPair::generate().unwrap();
        let mut params = CertificateParams::new(Vec::<String>::new()).unwrap();
        params.distinguished_name = DistinguishedName::new();
        params.distinguished_name.push(DnType::CommonName, name);
        params.is_ca = IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
        let cert = params.self_signed(&key).unwrap();
        Ca { cert, issuer: Issuer::new(params, key) }
    }

    fn leaf(name: &str, ca: &Ca) -> Issued {
        let key = KeyPair::generate().unwrap();
        let mut params = CertificateParams::new(vec![name.to_string(), "127.0.0.1".to_string()]).unwrap();
        params.distinguished_name.push(DnType::CommonName, name);
        params.extended_key_usages = vec![rcgen::ExtendedKeyUsagePurpose::ServerAuth];
        let cert = params.signed_by(&key, &ca.issuer).unwrap();
        Issued { cert_pem: cert.pem(), key_pem: key.serialize_pem() }
    }

    fn decode(text: &str) -> CertDecodeReport {
        decode_certificates(text.as_bytes()).unwrap()
    }

    #[test]
    fn decodes_a_chain_with_its_key() {
        let root = ca("Test Root");
        let server = leaf("api.example.test", &root);
        let text = format!("# comment\n{}{}\n{}", server.cert_pem, root.cert.pem(), server.key_pem);
        let r = decode(&text);
        let kinds: Vec<DecodedKind> = r.items.iter().map(|i| i.kind).collect();
        assert_eq!(kinds, [DecodedKind::Certificate, DecodedKind::Certificate, DecodedKind::PrivateKey]);
        let c = r.items[0].certificate.as_ref().unwrap();
        assert_eq!(c.details.common_name.as_deref(), Some("api.example.test"));
        assert_eq!(c.details.subject_alt_names, ["api.example.test", "127.0.0.1"]);
        assert_eq!(c.details.issuer_common_name.as_deref(), Some("Test Root"));
        assert_eq!(c.extended_key_usage, ["Server authentication"]);
        assert_eq!(c.version, 3);
        assert_eq!(c.pem.trim(), server.cert_pem.trim());
        assert!(r.items[1].certificate.as_ref().unwrap().details.is_ca);
        // The key: its type, and that it belongs to the server certificate. Never echoed.
        let key = r.items[2].key.as_ref().unwrap();
        assert_eq!((key.key_type.as_str(), key.key_curve.as_deref(), key.private), ("EC", Some("P-256"), true));
        assert_eq!(key.matches, [0]);
        assert!(!serde_json::to_string(&r).unwrap().contains("PRIVATE KEY-----"));
        assert!(r.warnings.is_empty(), "{:?}", r.warnings);
    }

    #[test]
    fn finds_chain_problems_and_a_key_of_another_certificate() {
        let root = ca("Test Root");
        let other_root = ca("Other Root");
        let a = leaf("a.example.test", &root);
        let b = leaf("b.example.test", &root);
        // Reversed: root first.
        let r = decode(&format!("{}{}", root.cert.pem(), a.cert_pem));
        assert_eq!(r.warnings.iter().map(|w| w.code.as_str()).collect::<Vec<_>>(), ["chainReversed"]);
        // The issuer is missing (another root follows).
        let r = decode(&format!("{}{}", a.cert_pem, other_root.cert.pem()));
        assert_eq!(r.warnings[0].code, "chainOrder");
        assert!(
            r.warnings[0].message.contains("'Other Root' did not issue 'a.example.test'"),
            "{}",
            r.warnings[0].message
        );
        // A bundle of roots is not a chain problem.
        assert!(decode(&format!("{}{}", root.cert.pem(), other_root.cert.pem())).warnings.is_empty());
        // A key that belongs to none of the certificates.
        let r = decode(&format!("{}{}", a.cert_pem, b.key_pem));
        assert_eq!(r.warnings[0].code, "keyMismatch");
        assert_eq!(r.warnings[0].level, TlsWarningLevel::Danger);
    }

    #[test]
    fn decodes_requests_public_keys_and_der() {
        let key = KeyPair::generate().unwrap();
        let mut params = CertificateParams::new(vec!["req.example.test".to_string()]).unwrap();
        params.distinguished_name.push(DnType::CommonName, "req.example.test");
        let csr = params.serialize_request(&key).unwrap();
        let r = decode(&format!("{}{}", csr.pem().unwrap(), key.public_key_pem()));
        let req = r.items[0].request.as_ref().unwrap();
        assert_eq!(req.common_name.as_deref(), Some("req.example.test"));
        assert_eq!(req.subject_alt_names, ["req.example.test"]);
        assert_eq!((req.key_type.as_str(), req.key_bits), ("EC", Some(256)));
        assert_eq!(req.signature_algorithm, "ECDSA with SHA-256");
        // The public key belongs to the request.
        let public = r.items[1].key.as_ref().unwrap();
        assert!(!public.private);
        assert_eq!(public.matches, [0]);

        // A binary certificate, and the same as base64 without PEM lines.
        let root = ca("DER Root");
        let der = root.cert.der().to_vec();
        let r = decode_certificates(&der).unwrap();
        assert_eq!(r.items[0].label, "DER");
        assert_eq!(r.items[0].certificate.as_ref().unwrap().details.common_name.as_deref(), Some("DER Root"));
        let b64 = base64::engine::general_purpose::STANDARD.encode(&der);
        let wrapped: String =
            b64.as_bytes().chunks(60).map(|c| format!("{}\n", std::str::from_utf8(c).unwrap())).collect();
        assert_eq!(decode(&wrapped).items[0].kind, DecodedKind::Certificate);
    }

    #[test]
    fn explains_what_it_cant_read() {
        assert!(decode_certificates(b"hello").unwrap_err().message.contains("PEM format"));
        assert!(decode_certificates(b"").is_err());
        let p12 = [0x30, 0x82, 0x01, 0x00, 0x02, 0x01, 0x03, 0x30, 0x80];
        assert!(decode_certificates(&p12).unwrap_err().message.contains("PKCS#12"));
        let r = decode("-----BEGIN CERTIFICATE-----\nAAAA\n-----END CERTIFICATE-----\n");
        assert!(r.items[0].error.as_deref().unwrap().starts_with("Could not parse the certificate"));
        let r = decode("-----BEGIN ENCRYPTED PRIVATE KEY-----\nAAAA\n-----END ENCRYPTED PRIVATE KEY-----\n");
        assert!(r.items[0].key.as_ref().unwrap().encrypted);
        let r = decode("-----BEGIN X509 CRL-----\nAAAA\n-----END X509 CRL-----\n");
        assert_eq!(r.items[0].kind, DecodedKind::Other);
        assert!(r.items[0].note.is_some());
        let r = decode("-----BEGIN CERTIFICATE-----\n!!!\n-----END CERTIFICATE-----\n");
        assert!(r.items[0].error.as_deref().unwrap().contains("could not be read"));
    }

    #[test]
    fn names_keys_it_cant_load() {
        // A PKCS#8 header for an RSA key (the key itself is not needed to name it).
        let rsa = [
            0x30, 0x82, 0x04, 0xBE, 0x02, 0x01, 0x00, 0x30, 0x0D, 0x06, 0x09, 0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01,
            0x01, 0x01,
        ];
        assert_eq!(key_algorithm("PRIVATE KEY", &rsa).0, "RSA");
        assert_eq!(key_algorithm("EC PRIVATE KEY", &[]).0, "EC");
        let p384 = [
            0x30, 0x13, 0x06, 0x07, 0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x02, 0x01, 0x06, 0x05, 0x2B, 0x81, 0x04, 0x00, 0x22,
        ];
        assert_eq!(key_algorithm("PRIVATE KEY", &p384), ("EC".to_string(), Some("P-384".to_string())));
        let item = private_key("PRIVATE KEY", PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(rsa.to_vec())));
        assert_eq!(item.item.key.as_ref().unwrap().key_type, "RSA");
        assert!(item.item.note.is_some() && item.public_key.is_none());
    }
}
