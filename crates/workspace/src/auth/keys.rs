//! Private keys pasted as PEM text: RSA (PKCS#1 or PKCS#8) and EC P-256/P-384
//! (SEC1 or PKCS#8), and the signatures made with them.
//!
//! Signing runs on ring, except RSA PKCS#1 v1.5 with SHA-1 (OAuth 1.0a `RSA-SHA1`),
//! which ring only verifies: that one is computed here with CRT and checked
//! against the public key before it is used.

use num_bigint::BigUint;
use ring::rand::SystemRandom;
use ring::signature::{self, EcdsaKeyPair, RsaKeyPair};

use super::AuthError;

const SEQUENCE: u8 = 0x30;
const INTEGER: u8 = 0x02;
const BIT_STRING: u8 = 0x03;
const OCTET_STRING: u8 = 0x04;
const OID: u8 = 0x06;
const CONTEXT_0: u8 = 0xa0;
const CONTEXT_1: u8 = 0xa1;

const OID_RSA: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x01];
const OID_RSA_PSS: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x0a];
const OID_EC: &[u8] = &[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x02, 0x01];
const OID_P256: &[u8] = &[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x03, 0x01, 0x07];
const OID_P384: &[u8] = &[0x2b, 0x81, 0x04, 0x00, 0x22];
const OID_ED25519: &[u8] = &[0x2b, 0x65, 0x70];

/// DigestInfo prefix for SHA-1 (RFC 8017 section 9.2, note 1).
const SHA1_DIGEST_INFO: &[u8] =
    &[0x30, 0x21, 0x30, 0x09, 0x06, 0x05, 0x2b, 0x0e, 0x03, 0x02, 0x1a, 0x05, 0x00, 0x04, 0x14];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RsaScheme {
    Pkcs1Sha1,
    Pkcs1Sha256,
    Pkcs1Sha384,
    Pkcs1Sha512,
    PssSha256,
    PssSha384,
    PssSha512,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Curve {
    P256,
    P384,
}

enum PrivateKey {
    /// PKCS#1 `RSAPrivateKey` DER.
    Rsa(Vec<u8>),
    /// SEC1 `ECPrivateKey` DER, and the curve named by the PKCS#8 wrapper (if any).
    Ec { curve: Option<Vec<u8>>, sec1: Vec<u8> },
}

/// Signs `message` with an RSA key in PEM form.
pub(crate) fn rsa_sign(pem_text: &str, scheme: RsaScheme, message: &[u8]) -> Result<Vec<u8>, AuthError> {
    let PrivateKey::Rsa(pkcs1) = read_private_key(pem_text)? else {
        return Err(AuthError::key(
            "The private key is an EC key, but this signature needs an RSA key (BEGIN RSA PRIVATE KEY or BEGIN PRIVATE KEY).",
        ));
    };
    let padding: &'static dyn signature::RsaEncoding = match scheme {
        RsaScheme::Pkcs1Sha1 => return rsa_pkcs1_sha1(&pkcs1, message),
        RsaScheme::Pkcs1Sha256 => &signature::RSA_PKCS1_SHA256,
        RsaScheme::Pkcs1Sha384 => &signature::RSA_PKCS1_SHA384,
        RsaScheme::Pkcs1Sha512 => &signature::RSA_PKCS1_SHA512,
        RsaScheme::PssSha256 => &signature::RSA_PSS_SHA256,
        RsaScheme::PssSha384 => &signature::RSA_PSS_SHA384,
        RsaScheme::PssSha512 => &signature::RSA_PSS_SHA512,
    };
    // ring's rejection reasons are stable codes such as "TooSmall".
    let pair = RsaKeyPair::from_der(&pkcs1).map_err(|e| match e.to_string().as_str() {
        "TooSmall" | "TooLarge" | "PrivateModulusLenNotMultipleOf512Bits" => {
            AuthError::key("This RSA key size isn't supported: use a 2048, 3072 or 4096-bit key.")
        }
        _ => AuthError::key("The RSA private key is damaged or in a form Zorvik can't read."),
    })?;
    let mut sig = vec![0; pair.public().modulus_len()];
    pair.sign(padding, &SystemRandom::new(), message, &mut sig)
        .map_err(|_| AuthError::key("Signing with the RSA key failed."))?;
    Ok(sig)
}

/// Signs `message` with an EC key in PEM form; the signature is `r || s` (JWS form).
pub(crate) fn ec_sign(pem_text: &str, curve: Curve, message: &[u8]) -> Result<Vec<u8>, AuthError> {
    let (algorithm, want, name) = match curve {
        Curve::P256 => (&signature::ECDSA_P256_SHA256_FIXED_SIGNING, OID_P256, "P-256"),
        Curve::P384 => (&signature::ECDSA_P384_SHA384_FIXED_SIGNING, OID_P384, "P-384"),
    };
    let PrivateKey::Ec { curve: outer_curve, sec1 } = read_private_key(pem_text)? else {
        return Err(AuthError::key(
            "The private key is an RSA key, but this signature needs an EC key (BEGIN EC PRIVATE KEY or BEGIN PRIVATE KEY).",
        ));
    };
    let bad = || AuthError::key("The EC private key is damaged or in a form Zorvik can't read.");
    // ECPrivateKey ::= SEQUENCE { version 1, privateKey OCTET STRING, [0] curve OPTIONAL, [1] publicKey OPTIONAL }
    let mut seq = Der(Der(&sec1).expect(SEQUENCE).ok_or_else(bad)?);
    seq.expect(INTEGER).ok_or_else(bad)?;
    let private = seq.expect(OCTET_STRING).ok_or_else(bad)?;
    let mut inner_curve = None;
    let mut public = None;
    while let Some((tag, value)) = seq.next() {
        match tag {
            CONTEXT_0 => inner_curve = Der(value).expect(OID),
            CONTEXT_1 => public = Der(value).expect(BIT_STRING).and_then(|b| b.split_first()).map(|(_, point)| point),
            _ => {}
        }
    }
    let key_curve = outer_curve.as_deref().or(inner_curve);
    if key_curve.is_some_and(|oid| oid != want) {
        return Err(AuthError::key(format!("The EC key is on another curve; this signature needs a {name} key.")));
    }
    let public = public.ok_or_else(|| {
        AuthError::key(
            "The EC private key has no public key part. Convert it with `openssl ec -in key.pem -out full.pem` and paste the result.",
        )
    })?;
    let rng = SystemRandom::new();
    let pair = EcdsaKeyPair::from_private_key_and_public_key(algorithm, private, public, &rng).map_err(|_| bad())?;
    let sig = pair.sign(&rng, message).map_err(|_| AuthError::key("Signing with the EC key failed."))?;
    Ok(sig.as_ref().to_vec())
}

fn read_private_key(text: &str) -> Result<PrivateKey, AuthError> {
    let not_pem = || {
        AuthError::key(
            "The private key isn't PEM text. Paste the whole key, from -----BEGIN … PRIVATE KEY----- to -----END … PRIVATE KEY-----.",
        )
    };
    // Keys copied out of JSON files often carry `\n` escapes instead of line breaks.
    let text = if !text.contains('\n') && text.contains("\\n") { text.replace("\\n", "\n") } else { text.to_string() };
    let blocks = pem::parse_many(text.trim()).map_err(|_| not_pem())?;
    let Some(block) = blocks.iter().find(|b| b.tag().ends_with("PRIVATE KEY")) else {
        if blocks.iter().any(|b| b.tag().contains("PUBLIC KEY") || b.tag().contains("CERTIFICATE")) {
            return Err(AuthError::key("This is a public key or a certificate. Paste the private key instead."));
        }
        return Err(not_pem());
    };
    if block.tag() == "ENCRYPTED PRIVATE KEY"
        || block.headers().get("Proc-Type").is_some_and(|v| v.contains("ENCRYPTED"))
    {
        return Err(AuthError::key(
            "The private key is protected with a passphrase. Export it without one (for example `openssl pkey -in key.pem -out plain.pem`).",
        ));
    }
    match block.tag() {
        "RSA PRIVATE KEY" => Ok(PrivateKey::Rsa(block.contents().to_vec())),
        "EC PRIVATE KEY" => Ok(PrivateKey::Ec { curve: None, sec1: block.contents().to_vec() }),
        "PRIVATE KEY" => read_pkcs8(block.contents()),
        "OPENSSH PRIVATE KEY" => Err(AuthError::key(
            "OpenSSH keys need converting to PEM first: `ssh-keygen -p -m PEM -f key` (on a copy of the key).",
        )),
        _ => Err(AuthError::key(
            "Zorvik reads RSA and EC private keys (BEGIN RSA PRIVATE KEY, BEGIN EC PRIVATE KEY or BEGIN PRIVATE KEY).",
        )),
    }
}

/// PrivateKeyInfo ::= SEQUENCE { version, AlgorithmIdentifier { OID, params }, privateKey OCTET STRING, … }
fn read_pkcs8(der: &[u8]) -> Result<PrivateKey, AuthError> {
    let bad = || AuthError::key("The private key (BEGIN PRIVATE KEY) isn't a valid PKCS#8 key.");
    let mut seq = Der(Der(der).expect(SEQUENCE).ok_or_else(bad)?);
    seq.expect(INTEGER).ok_or_else(bad)?;
    let mut algorithm = Der(seq.expect(SEQUENCE).ok_or_else(bad)?);
    let oid = algorithm.expect(OID).ok_or_else(bad)?;
    let key = seq.expect(OCTET_STRING).ok_or_else(bad)?.to_vec();
    match oid {
        OID_RSA | OID_RSA_PSS => Ok(PrivateKey::Rsa(key)),
        OID_EC => Ok(PrivateKey::Ec { curve: algorithm.expect(OID).map(<[u8]>::to_vec), sec1: key }),
        OID_ED25519 => {
            Err(AuthError::key("Ed25519 keys aren't supported here. Use an RSA key or an EC P-256/P-384 key."))
        }
        _ => Err(AuthError::key("The private key uses an algorithm Zorvik doesn't support. Use an RSA or EC key.")),
    }
}

/// RSASSA-PKCS1-v1_5 with SHA-1 (RFC 8017 section 8.2).
fn rsa_pkcs1_sha1(pkcs1: &[u8], message: &[u8]) -> Result<Vec<u8>, AuthError> {
    let bad = || AuthError::key("The RSA private key is damaged or in a form Zorvik can't read.");
    // RSAPrivateKey ::= SEQUENCE { version, n, e, d, p, q, dp, dq, qinv }
    let mut seq = Der(Der(pkcs1).expect(SEQUENCE).ok_or_else(bad)?);
    let mut next = || seq.expect(INTEGER).map(BigUint::from_bytes_be).ok_or_else(bad);
    let (_version, n, e, _d) = (next()?, next()?, next()?, next()?);
    let (p, q, dp, dq, qinv) = (next()?, next()?, next()?, next()?, next()?);
    match n.bits() {
        0..1024 => return Err(AuthError::key("The RSA key is too short: use a key of at least 1024 bits.")),
        1024..=16384 => {}
        _ => return Err(AuthError::key("The RSA key is too long: use a key of at most 16384 bits.")),
    }
    let k = n.bits().div_ceil(8) as usize;
    let hash = ring::digest::digest(&ring::digest::SHA1_FOR_LEGACY_USE_ONLY, message);
    let t = [SHA1_DIGEST_INFO, hash.as_ref()].concat();
    // EM = 0x00 || 0x01 || PS (0xff…) || 0x00 || T
    let mut em = vec![0xff; k];
    em[0] = 0;
    em[1] = 1;
    em[k - t.len() - 1] = 0;
    em[k - t.len()..].copy_from_slice(&t);
    let m = BigUint::from_bytes_be(&em);
    // Components no larger than n (a crafted key could make the arithmetic run for ages).
    if p.bits() == 0 || q.bits() == 0 || [&p, &q, &dp, &dq, &qinv, &e].iter().any(|x| x.bits() > n.bits()) {
        return Err(bad());
    }
    let m1 = m.modpow(&dp, &p);
    let m2 = m.modpow(&dq, &q);
    let h = (&qinv * (&m1 + &p - (&m2 % &p))) % &p;
    let s = m2 + h * &q;
    // A wrong component (or a fault) would leak the key through a bad signature.
    if s >= n || s.modpow(&e, &n) != m {
        return Err(bad());
    }
    let bytes = s.to_bytes_be();
    let mut sig = vec![0; k - bytes.len()];
    sig.extend_from_slice(&bytes);
    Ok(sig)
}

/// A minimal DER reader: definite lengths up to 4 bytes, which covers keys.
struct Der<'a>(&'a [u8]);

impl<'a> Der<'a> {
    fn next(&mut self) -> Option<(u8, &'a [u8])> {
        let (&tag, rest) = self.0.split_first()?;
        let (&first, rest) = rest.split_first()?;
        let (len, rest) = if first < 0x80 {
            (first as usize, rest)
        } else {
            let n = (first & 0x7f) as usize;
            if n == 0 || n > 4 || rest.len() < n {
                return None;
            }
            (rest[..n].iter().fold(0usize, |acc, &b| (acc << 8) | b as usize), &rest[n..])
        };
        if rest.len() < len {
            return None;
        }
        let (value, rest) = rest.split_at(len);
        self.0 = rest;
        Some((tag, value))
    }

    fn expect(&mut self, tag: u8) -> Option<&'a [u8]> {
        self.next().filter(|(t, _)| *t == tag).map(|(_, v)| v)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use ring::signature::UnparsedPublicKey;

    fn der(tag: u8, body: &[u8]) -> Vec<u8> {
        let mut out = vec![tag];
        match body.len() {
            n @ 0..128 => out.push(n as u8),
            n => {
                let len = (n as u32).to_be_bytes();
                let skip = len.iter().take_while(|b| **b == 0).count();
                out.push(0x80 | (4 - skip) as u8);
                out.extend_from_slice(&len[skip..]);
            }
        }
        out.extend_from_slice(body);
        out
    }

    fn der_int(v: &BigUint) -> Vec<u8> {
        let mut bytes = v.to_bytes_be();
        if bytes[0] & 0x80 != 0 {
            bytes.insert(0, 0);
        }
        der(INTEGER, &bytes)
    }

    #[test]
    fn crafted_rsa_keys_are_refused_not_a_crash() {
        // e = 1 and q = n make s = m + h·n pass the s^e mod n check while s is larger than n.
        let n = (BigUint::from(1u8) << 1100u32) + BigUint::from(1u8);
        let one = BigUint::from(1u8);
        let parts = [
            BigUint::from(0u8),
            n.clone(),
            one.clone(),
            one.clone(),
            BigUint::from(3u8),
            n,
            BigUint::from(2u8),
            one.clone(),
            one,
        ];
        let body: Vec<u8> = parts.iter().flat_map(der_int).collect();
        let key = der(SEQUENCE, &body);
        for i in 0..24u8 {
            if let Ok(signature) = rsa_pkcs1_sha1(&key, &[i]) {
                assert_eq!(signature.len(), 1101usize.div_ceil(8));
            }
        }
        // A component far larger than n is refused before any arithmetic.
        let mut parts = parts.to_vec();
        parts[6] = BigUint::from(1u8) << 100_000u32;
        let body: Vec<u8> = parts.iter().flat_map(der_int).collect();
        assert!(rsa_pkcs1_sha1(&der(SEQUENCE, &body), b"x").is_err());
    }

    /// A 2048-bit RSA key made with `openssl genrsa 2048` for these tests only.
    pub(crate) const RSA_PKCS1: &str = include_str!("testdata/rsa2048-pkcs1.pem");
    /// The same key as PKCS#8 (`openssl pkcs8 -topk8 -nocrypt`).
    pub(crate) const RSA_PKCS8: &str = include_str!("testdata/rsa2048-pkcs8.pem");
    /// A P-256 key from `openssl ecparam -name prime256v1 -genkey` (SEC1, with the parameters block).
    pub(crate) const EC_P256_SEC1: &str = include_str!("testdata/p256-sec1.pem");
    /// A P-384 key as PKCS#8.
    pub(crate) const EC_P384_PKCS8: &str = include_str!("testdata/p384-pkcs8.pem");

    /// The public key (PKCS#1 `RSAPublicKey` DER) of an RSA private key, for verifying.
    pub(crate) fn rsa_public_der(pem_text: &str) -> Vec<u8> {
        let PrivateKey::Rsa(der) = read_private_key(pem_text).unwrap() else { panic!("not RSA") };
        RsaKeyPair::from_der(&der).unwrap().public().as_ref().to_vec()
    }

    /// The public point of an EC private key, for verifying.
    pub(crate) fn ec_public_point(pem_text: &str, curve: Curve) -> Vec<u8> {
        let PrivateKey::Ec { sec1, .. } = read_private_key(pem_text).unwrap() else { panic!("not EC") };
        let alg = match curve {
            Curve::P256 => &signature::ECDSA_P256_SHA256_FIXED_SIGNING,
            Curve::P384 => &signature::ECDSA_P384_SHA384_FIXED_SIGNING,
        };
        let mut seq = Der(Der(&sec1).expect(SEQUENCE).unwrap());
        seq.expect(INTEGER).unwrap();
        let private = seq.expect(OCTET_STRING).unwrap();
        let mut public = None;
        while let Some((tag, value)) = seq.next() {
            if tag == CONTEXT_1 {
                public = Der(value).expect(BIT_STRING).map(|b| b[1..].to_vec());
            }
        }
        let public = public.unwrap();
        let rng = SystemRandom::new();
        use ring::signature::KeyPair as _;
        EcdsaKeyPair::from_private_key_and_public_key(alg, private, &public, &rng)
            .unwrap()
            .public_key()
            .as_ref()
            .to_vec()
    }

    #[test]
    fn rsa_sha1_matches_openssl_and_verifies() {
        // `printf 'zorvik' | openssl dgst -sha1 -sign rsa2048-pkcs1.pem | base64`
        let expected = include_str!("testdata/rsa2048-sha1-zorvik.b64").trim();
        for pem_text in [RSA_PKCS1, RSA_PKCS8] {
            let sig = rsa_sign(pem_text, RsaScheme::Pkcs1Sha1, b"zorvik").unwrap();
            assert_eq!(super::super::b64(&sig), expected);
            UnparsedPublicKey::new(&signature::RSA_PKCS1_2048_8192_SHA1_FOR_LEGACY_USE_ONLY, rsa_public_der(pem_text))
                .verify(b"zorvik", &sig)
                .unwrap();
        }
    }

    #[test]
    fn rsa_sha256_is_deterministic_and_verifies() {
        let a = rsa_sign(RSA_PKCS1, RsaScheme::Pkcs1Sha256, b"message").unwrap();
        let b = rsa_sign(RSA_PKCS8, RsaScheme::Pkcs1Sha256, b"message").unwrap();
        assert_eq!(a, b);
        UnparsedPublicKey::new(&signature::RSA_PKCS1_2048_8192_SHA256, rsa_public_der(RSA_PKCS1))
            .verify(b"message", &a)
            .unwrap();
    }

    #[test]
    fn keys_in_json_escapes_and_crlf_are_read() {
        let escaped = RSA_PKCS8.trim().replace('\n', "\\n");
        assert!(rsa_sign(&escaped, RsaScheme::Pkcs1Sha256, b"x").is_ok());
        let crlf = RSA_PKCS1.replace('\n', "\r\n");
        assert!(rsa_sign(&crlf, RsaScheme::Pkcs1Sha256, b"x").is_ok());
    }

    #[test]
    fn wrong_keys_say_what_is_wrong() {
        let err = |r: Result<Vec<u8>, AuthError>| r.unwrap_err().to_string();
        assert!(err(rsa_sign("not a key", RsaScheme::Pkcs1Sha256, b"x")).contains("isn't PEM text"));
        assert!(err(rsa_sign(EC_P256_SEC1, RsaScheme::Pkcs1Sha256, b"x")).contains("needs an RSA key"));
        assert!(err(ec_sign(RSA_PKCS1, Curve::P256, b"x")).contains("needs an EC key"));
        assert!(err(ec_sign(EC_P256_SEC1, Curve::P384, b"x")).contains("needs a P-384 key"));
        assert!(err(ec_sign(EC_P384_PKCS8, Curve::P256, b"x")).contains("needs a P-256 key"));
        let encrypted = "-----BEGIN ENCRYPTED PRIVATE KEY-----\nMIIBAA==\n-----END ENCRYPTED PRIVATE KEY-----\n";
        assert!(err(rsa_sign(encrypted, RsaScheme::Pkcs1Sha256, b"x")).contains("passphrase"));
        let public = "-----BEGIN PUBLIC KEY-----\nMIIBAA==\n-----END PUBLIC KEY-----\n";
        assert!(err(rsa_sign(public, RsaScheme::Pkcs1Sha256, b"x")).contains("public key"));
        // Messages never quote the key material.
        assert!(!err(rsa_sign(EC_P256_SEC1, RsaScheme::Pkcs1Sha256, b"x")).contains("MHc"));
    }

    #[test]
    fn ec_keys_sign_and_verify() {
        let sig = ec_sign(EC_P256_SEC1, Curve::P256, b"message").unwrap();
        assert_eq!(sig.len(), 64);
        UnparsedPublicKey::new(&signature::ECDSA_P256_SHA256_FIXED, ec_public_point(EC_P256_SEC1, Curve::P256))
            .verify(b"message", &sig)
            .unwrap();
        let sig = ec_sign(EC_P384_PKCS8, Curve::P384, b"message").unwrap();
        assert_eq!(sig.len(), 96);
        UnparsedPublicKey::new(&signature::ECDSA_P384_SHA384_FIXED, ec_public_point(EC_P384_PKCS8, Curve::P384))
            .verify(b"message", &sig)
            .unwrap();
    }
}
