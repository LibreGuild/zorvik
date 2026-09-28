//! NTLM client, NTLMv2 only (MS-NLMP): the NEGOTIATE message, reading the server's
//! CHALLENGE and building the AUTHENTICATE message.
//!
//! NTLM authenticates a connection, not a request: the NEGOTIATE and AUTHENTICATE
//! requests must use the same kept-alive connection. No MIC is sent (it is optional)
//! and no session key is exchanged, since HTTP doesn't sign or seal messages.

use base64::Engine as _;
use hmac::{Hmac, KeyInit as _, Mac as _};
use md4::Digest as _;
use time::OffsetDateTime;

use super::{AuthError, b64};

const SIGNATURE: &[u8; 8] = b"NTLMSSP\0";

pub const NEGOTIATE_UNICODE: u32 = 0x0000_0001;
pub const NEGOTIATE_OEM: u32 = 0x0000_0002;
pub const REQUEST_TARGET: u32 = 0x0000_0004;
pub const NEGOTIATE_NTLM: u32 = 0x0000_0200;
pub const NEGOTIATE_ALWAYS_SIGN: u32 = 0x0000_8000;
pub const NEGOTIATE_EXTENDED_SESSIONSECURITY: u32 = 0x0008_0000;
pub const NEGOTIATE_TARGET_INFO: u32 = 0x0080_0000;
pub const NEGOTIATE_128: u32 = 0x2000_0000;
pub const NEGOTIATE_56: u32 = 0x8000_0000;

/// What the client asks for in its NEGOTIATE message.
const CLIENT_FLAGS: u32 = NEGOTIATE_UNICODE
    | NEGOTIATE_OEM
    | REQUEST_TARGET
    | NEGOTIATE_NTLM
    | NEGOTIATE_ALWAYS_SIGN
    | NEGOTIATE_EXTENDED_SESSIONSECURITY
    | NEGOTIATE_128
    | NEGOTIATE_56;

/// AV pair ids (MS-NLMP 2.2.2.1).
pub const AV_EOL: u16 = 0;
pub const AV_NB_COMPUTER_NAME: u16 = 1;
pub const AV_NB_DOMAIN_NAME: u16 = 2;
pub const AV_TIMESTAMP: u16 = 7;

/// Seconds between 1601-01-01 (FILETIME) and 1970-01-01, in 100 ns units.
const FILETIME_UNIX_EPOCH: i128 = 116_444_736_000_000_000;

/// The NEGOTIATE message (type 1), base64 for `Authorization: NTLM <message>`.
pub fn negotiate_message() -> String {
    let mut m = Vec::with_capacity(32);
    m.extend_from_slice(SIGNATURE);
    m.extend_from_slice(&1u32.to_le_bytes());
    m.extend_from_slice(&CLIENT_FLAGS.to_le_bytes());
    m.extend_from_slice(&[0; 16]); // empty domain and workstation fields
    b64(&m)
}

/// The server's CHALLENGE message (type 2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NtlmChallenge {
    pub flags: u32,
    pub server_challenge: [u8; 8],
    pub target_name: String,
    /// The target info (AV pairs) as received; the NTLMv2 response carries it back.
    pub target_info: Vec<u8>,
    /// The pairs of `target_info` as (AvId, value), without the end marker.
    pub av_pairs: Vec<(u16, Vec<u8>)>,
    /// `MsvAvTimestamp`: the server's time as a FILETIME.
    pub timestamp: Option<u64>,
}

/// Reads a CHALLENGE message: the base64 token of `WWW-Authenticate: NTLM <token>`
/// (a leading `NTLM ` or `Negotiate ` is accepted too).
pub fn parse_challenge(b64: &str) -> Result<NtlmChallenge, AuthError> {
    let text = b64.trim();
    let text = text.strip_prefix("NTLM ").or_else(|| text.strip_prefix("Negotiate ")).unwrap_or(text).trim();
    let bad = || AuthError::challenge("The server's NTLM challenge is malformed.");
    if text.is_empty() {
        return Err(AuthError::challenge(
            "The server answered without an NTLM challenge. It may not support NTLM, or the connection was not kept alive.",
        ));
    }
    let data = base64::engine::general_purpose::STANDARD.decode(text).map_err(|_| bad())?;
    if data.len() < 32 || &data[..8] != SIGNATURE {
        return Err(bad());
    }
    if u32_at(&data, 8) != 2 {
        return Err(AuthError::challenge("The server sent an NTLM message that isn't a challenge."));
    }
    let flags = u32_at(&data, 20);
    let mut server_challenge = [0; 8];
    server_challenge.copy_from_slice(&data[24..32]);
    let target_name = field(&data, 12).ok_or_else(bad)?;
    let target_name = if flags & NEGOTIATE_UNICODE != 0 {
        String::from_utf16_lossy(
            &target_name.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect::<Vec<_>>(),
        )
    } else {
        String::from_utf8_lossy(target_name).into_owned()
    };
    let target_info = if flags & NEGOTIATE_TARGET_INFO != 0 && data.len() >= 48 {
        field(&data, 40).ok_or_else(bad)?.to_vec()
    } else {
        Vec::new()
    };
    let mut av_pairs = Vec::new();
    let mut i = 0;
    while i + 4 <= target_info.len() {
        let id = u16::from_le_bytes([target_info[i], target_info[i + 1]]);
        let len = u16::from_le_bytes([target_info[i + 2], target_info[i + 3]]) as usize;
        if id == AV_EOL {
            break;
        }
        av_pairs.push((id, target_info.get(i + 4..i + 4 + len).ok_or_else(bad)?.to_vec()));
        i += 4 + len;
    }
    let timestamp = av_pairs
        .iter()
        .find(|(id, v)| *id == AV_TIMESTAMP && v.len() == 8)
        .map(|(_, v)| u64::from_le_bytes(v[..8].try_into().expect("8 bytes")));
    Ok(NtlmChallenge { flags, server_challenge, target_name, target_info, av_pairs, timestamp })
}

/// The AUTHENTICATE message (type 3) with an NTLMv2 response, base64.
///
/// `user` may be `DOMAIN\user` or `user@domain` when `domain` is empty.
/// `client_challenge` must be 8 fresh random bytes. `timestamp` is the client's
/// time as a FILETIME (see [`filetime`]); the server's `MsvAvTimestamp` wins when
/// present, `None` takes the clock. With a server timestamp the LMv2 response is
/// zeros, as MS-NLMP 3.1.5.1.2 says.
pub fn authenticate_message(
    challenge: &NtlmChallenge,
    user: &str,
    password: &str,
    domain: &str,
    workstation: &str,
    client_challenge: [u8; 8],
    timestamp: Option<u64>,
) -> Result<String, AuthError> {
    let (user, domain) = split_user(user, domain);
    if user.is_empty() {
        return Err(AuthError::input("NTLM needs a username."));
    }
    let key = ntowf_v2(password, user, domain);
    let time = challenge.timestamp.or(timestamp).unwrap_or_else(|| filetime(OffsetDateTime::now_utc()));
    let nt_response =
        ntlmv2_response(&key, &challenge.server_challenge, &client_challenge, time, &challenge.target_info);
    let lm_response = if challenge.timestamp.is_some() {
        vec![0; 24]
    } else {
        lmv2_response(&key, &challenge.server_challenge, &client_challenge)
    };

    let unicode = challenge.flags & NEGOTIATE_UNICODE != 0;
    let echoed = NEGOTIATE_NTLM
        | REQUEST_TARGET
        | NEGOTIATE_ALWAYS_SIGN
        | NEGOTIATE_EXTENDED_SESSIONSECURITY
        | NEGOTIATE_TARGET_INFO
        | NEGOTIATE_128
        | NEGOTIATE_56;
    let flags = (challenge.flags & echoed) | if unicode { NEGOTIATE_UNICODE } else { NEGOTIATE_OEM };
    let text = |s: &str| if unicode { utf16le(s) } else { s.as_bytes().to_vec() };
    // Payload order as Windows sends it: domain, user, workstation, LM, NT, session key.
    let payload = [text(domain), text(user), text(workstation), lm_response, nt_response];

    let mut m = vec![0; 64];
    m[..8].copy_from_slice(SIGNATURE);
    m[8..12].copy_from_slice(&3u32.to_le_bytes());
    // Header positions of each payload's (len, max len, offset) field.
    let positions = [28, 36, 44, 12, 20];
    for (bytes, at) in payload.iter().zip(positions) {
        let offset = m.len();
        put_field(&mut m, at, bytes.len(), offset)?;
        m.extend_from_slice(bytes);
    }
    let end = m.len();
    put_field(&mut m, 52, 0, end)?; // no session key
    m[60..64].copy_from_slice(&flags.to_le_bytes());
    Ok(b64(&m))
}

/// A time as a FILETIME: 100 ns intervals since 1601-01-01 UTC.
pub fn filetime(t: OffsetDateTime) -> u64 {
    (t.unix_timestamp_nanos() / 100 + FILETIME_UNIX_EPOCH).max(0) as u64
}

/// NTOWFv2 (MS-NLMP 3.3.2): HMAC-MD5 of the uppercased user and the domain, keyed with the NT hash.
fn ntowf_v2(password: &str, user: &str, domain: &str) -> [u8; 16] {
    let nt_hash = md4::Md4::digest(utf16le(password));
    hmac_md5(&nt_hash, &[&utf16le(&format!("{}{domain}", uppercase(user)))])
}

/// NTProofStr followed by the blob ("temp" in MS-NLMP 3.3.2).
fn ntlmv2_response(key: &[u8; 16], server: &[u8; 8], client: &[u8; 8], time: u64, target_info: &[u8]) -> Vec<u8> {
    let mut temp = vec![1, 1, 0, 0, 0, 0, 0, 0];
    temp.extend_from_slice(&time.to_le_bytes());
    temp.extend_from_slice(client);
    temp.extend_from_slice(&[0; 4]);
    temp.extend_from_slice(target_info);
    temp.extend_from_slice(&[0; 4]);
    let proof = hmac_md5(key, &[server, &temp]);
    [proof.as_slice(), &temp].concat()
}

fn lmv2_response(key: &[u8; 16], server: &[u8; 8], client: &[u8; 8]) -> Vec<u8> {
    [hmac_md5(key, &[server, client]).as_slice(), client].concat()
}

fn hmac_md5(key: &[u8], parts: &[&[u8]]) -> [u8; 16] {
    let mut mac = Hmac::<md5::Md5>::new_from_slice(key).expect("HMAC takes any key length");
    for part in parts {
        mac.update(part);
    }
    mac.finalize().into_bytes().into()
}

/// `DOMAIN\user` and `user@domain` forms, used when no domain is given.
fn split_user<'a>(user: &'a str, domain: &'a str) -> (&'a str, &'a str) {
    if domain.is_empty() {
        if let Some((d, u)) = user.split_once('\\') {
            return (u, d);
        }
        if let Some((u, d)) = user.rsplit_once('@') {
            return (u, d);
        }
    }
    (user, domain)
}

/// Uppercase one character for one, like Windows (`ß` stays `ß`).
fn uppercase(s: &str) -> String {
    s.chars()
        .map(|c| {
            let mut upper = c.to_uppercase();
            match (upper.next(), upper.next()) {
                (Some(u), None) => u,
                _ => c,
            }
        })
        .collect()
}

fn utf16le(s: &str) -> Vec<u8> {
    s.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

fn u32_at(data: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(data[at..at + 4].try_into().expect("4 bytes"))
}

/// The payload a (len, max len, offset) field points to.
fn field(data: &[u8], at: usize) -> Option<&[u8]> {
    let len = u16::from_le_bytes([*data.get(at)?, *data.get(at + 1)?]) as usize;
    let offset = u32::from_le_bytes(data.get(at + 4..at + 8)?.try_into().ok()?) as usize;
    data.get(offset..offset.checked_add(len)?)
}

fn put_field(m: &mut [u8], at: usize, len: usize, offset: usize) -> Result<(), AuthError> {
    let too_long = || AuthError::input("The NTLM username, domain or workstation is too long.");
    let len = u16::try_from(len).map_err(|_| too_long())?;
    let offset = u32::try_from(offset).map_err(|_| too_long())?;
    m[at..at + 2].copy_from_slice(&len.to_le_bytes());
    m[at + 2..at + 4].copy_from_slice(&len.to_le_bytes());
    m[at + 4..at + 8].copy_from_slice(&offset.to_le_bytes());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::hex;

    /// MS-NLMP 4.2.4.3 CHALLENGE_MESSAGE:
    /// https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-nlmp/bc612491-fb0b-4829-91bc-7c6b95ff67fe
    const SPEC_CHALLENGE: &str = "4e544c4d53535000020000000c000c003800000033828ae20123456789abcdef00000000000000002400240044000000060070170000000f53006500720076006500720002000c0044006f006d00610069006e0001000c0053006500720076006500720000000000";
    /// The NTLMv2 blob ("temp") of MS-NLMP 4.2.4.1.3:
    /// https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-nlmp/946f54bd-76b5-4b18-ace8-6e8c992d5847
    const SPEC_TEMP: &str = "01010000000000000000000000000000aaaaaaaaaaaaaaaa0000000002000c0044006f006d00610069006e0001000c005300650072007600650072000000000000000000";

    fn unhex(s: &str) -> Vec<u8> {
        (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
    }

    fn spec_challenge() -> NtlmChallenge {
        parse_challenge(&b64(&unhex(SPEC_CHALLENGE))).unwrap()
    }

    /// (len, offset) payload of an AUTHENTICATE message field.
    fn payload(message: &[u8], at: usize) -> Vec<u8> {
        field(message, at).unwrap().to_vec()
    }

    #[test]
    fn spec_values() {
        // MS-NLMP 4.2.4.1.1 NTOWFv2:
        // https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-nlmp/7795bd0e-fd5e-43ec-bd9c-994704d8ee26
        let key = ntowf_v2("Password", "User", "Domain");
        assert_eq!(hex(&key), "0c868a403bfd7a93a3001ef22ef02e3f");
        let server = [0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef];
        let client = [0xaa; 8];
        // 4.2.4.2.1 LMv2 response:
        // https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-nlmp/7e2b35f9-fe90-49fb-8c9d-30639a899160
        assert_eq!(hex(&lmv2_response(&key, &server, &client)), "86c35097ac9cec102554764a57cccc19aaaaaaaaaaaaaaaa");
        // 4.2.4.2.2 NTLMv2 response (NTProofStr), then the blob:
        // https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-nlmp/fa2bc0f0-9efa-40d7-a165-adfccd7f6da7
        let challenge = spec_challenge();
        let response = ntlmv2_response(&key, &server, &client, 0, &challenge.target_info);
        assert_eq!(hex(&response[..16]), "68cd0ab851e51c96aabc927bebef6a1c");
        assert_eq!(hex(&response[16..]), SPEC_TEMP);
    }

    #[test]
    fn reads_the_spec_challenge() {
        let c = spec_challenge();
        assert_eq!(c.flags, 0xe28a_8233);
        assert_eq!(hex(&c.server_challenge), "0123456789abcdef");
        assert_eq!(c.target_name, "Server");
        assert_eq!(c.av_pairs, [(AV_NB_DOMAIN_NAME, utf16le("Domain")), (AV_NB_COMPUTER_NAME, utf16le("Server"))]);
        assert_eq!(c.timestamp, None);
    }

    #[test]
    fn authenticate_message_carries_the_spec_responses() {
        let challenge = spec_challenge();
        let message =
            authenticate_message(&challenge, "User", "Password", "Domain", "COMPUTER", [0xaa; 8], Some(0)).unwrap();
        let m = base64::engine::general_purpose::STANDARD.decode(message).unwrap();
        assert_eq!(&m[..12], b"NTLMSSP\0\x03\0\0\0");
        // The LM and NT responses of the AUTHENTICATE_MESSAGE in MS-NLMP 4.2.4.3.
        assert_eq!(hex(&payload(&m, 12)), "86c35097ac9cec102554764a57cccc19aaaaaaaaaaaaaaaa");
        assert_eq!(hex(&payload(&m, 20)), format!("68cd0ab851e51c96aabc927bebef6a1c{SPEC_TEMP}"));
        assert_eq!(payload(&m, 28), utf16le("Domain"));
        assert_eq!(payload(&m, 36), utf16le("User"));
        assert_eq!(payload(&m, 44), utf16le("COMPUTER"));
        assert_eq!(payload(&m, 52), b"");
        let flags = u32_at(&m, 60);
        assert_ne!(flags & NEGOTIATE_UNICODE, 0);
        assert_ne!(flags & NEGOTIATE_EXTENDED_SESSIONSECURITY, 0);
        assert_eq!(flags & 0x4000_0000, 0, "no key exchange");
    }

    #[test]
    fn server_timestamp_wins_and_zeroes_the_lm_response() {
        let mut info = Vec::new();
        for (id, value) in
            [(AV_NB_DOMAIN_NAME, utf16le("CORP")), (AV_TIMESTAMP, 0x01d0_0000_0000_0042u64.to_le_bytes().to_vec())]
        {
            info.extend_from_slice(&id.to_le_bytes());
            info.extend_from_slice(&(value.len() as u16).to_le_bytes());
            info.extend_from_slice(&value);
        }
        info.extend_from_slice(&[0; 4]);
        let reparsed = {
            // The spec's message with this target info instead.
            let mut m = unhex(SPEC_CHALLENGE)[..0x44].to_vec();
            m[40..42].copy_from_slice(&(info.len() as u16).to_le_bytes());
            m[42..44].copy_from_slice(&(info.len() as u16).to_le_bytes());
            m.extend_from_slice(&info);
            parse_challenge(&format!("NTLM {}", b64(&m))).unwrap()
        };
        assert_eq!(reparsed.timestamp, Some(0x01d0_0000_0000_0042));
        let message = authenticate_message(&reparsed, "CORP\\alice", "pw", "", "", [7; 8], Some(1)).unwrap();
        let m = base64::engine::general_purpose::STANDARD.decode(message).unwrap();
        assert_eq!(payload(&m, 12), [0; 24]);
        let nt = payload(&m, 20);
        assert_eq!(&nt[24..32], &0x01d0_0000_0000_0042u64.to_le_bytes(), "the blob carries the server's time");
        assert_eq!(payload(&m, 28), utf16le("CORP"));
        assert_eq!(payload(&m, 36), utf16le("alice"));
        // The proof verifies with the same inputs.
        let key = ntowf_v2("pw", "alice", "CORP");
        assert_eq!(nt[..16], hmac_md5(&key, &[&reparsed.server_challenge, &nt[16..]]));
    }

    #[test]
    fn usernames_with_the_domain_inside() {
        assert_eq!(split_user("CORP\\alice", ""), ("alice", "CORP"));
        assert_eq!(split_user("alice@corp.example", ""), ("alice", "corp.example"));
        assert_eq!(split_user("CORP\\alice", "OTHER"), ("CORP\\alice", "OTHER"));
        assert_eq!(uppercase("straße ünï"), "STRAßE ÜNÏ");
        // Unicode credentials hash as UTF-16.
        let a = ntowf_v2("pässwörd ✓", "jürgen", "Dömäin");
        let b = ntowf_v2("pässwörd ✓", "JÜRGEN", "Dömäin");
        assert_eq!(a, b, "the user name is case-insensitive");
        assert_ne!(a, ntowf_v2("pässwörd ✓", "jürgen", "DÖMÄIN"), "the domain is used as given");
    }

    #[test]
    fn negotiate_and_bad_challenges() {
        let m = base64::engine::general_purpose::STANDARD.decode(negotiate_message()).unwrap();
        assert_eq!(m.len(), 32);
        assert_eq!(&m[..12], b"NTLMSSP\0\x01\0\0\0");
        assert_eq!(u32_at(&m, 12), 0xa008_8207);
        let err = |s: &str| parse_challenge(s).unwrap_err().to_string();
        assert!(err("").contains("without an NTLM challenge"));
        assert!(err("NTLM !!!").contains("malformed"));
        assert!(err(&negotiate_message()).contains("isn't a challenge"));
        let mut truncated = unhex(SPEC_CHALLENGE);
        truncated.truncate(0x50);
        assert!(err(&b64(&truncated)).contains("malformed"));
        assert!(authenticate_message(&spec_challenge(), "", "pw", "", "", [0; 8], Some(0)).is_err());
    }

    #[test]
    fn filetime_epoch() {
        assert_eq!(filetime(OffsetDateTime::UNIX_EPOCH), 116_444_736_000_000_000);
        assert_eq!(filetime(OffsetDateTime::UNIX_EPOCH + time::Duration::seconds(1)), 116_444_736_010_000_000);
    }
}
