//! gRPC wire details: length-prefixed messages, status codes, `grpc-timeout`,
//! the percent-encoded `grpc-message` and `grpc-status-details-bin`.

use std::io::Read as _;
use std::time::Duration;

use base64::Engine as _;
use bytes::{Buf, BufMut, Bytes, BytesMut};
use prost::Message as _;
use prost_reflect::{DescriptorPool, DynamicMessage};

use super::GrpcStatus;
use crate::http::Header;

/// Largest message sent or received (the header allows 4 GB).
pub const MAX_MESSAGE_BYTES: usize = 64 << 20;

const STATUS_NAMES: [&str; 17] = [
    "OK",
    "CANCELLED",
    "UNKNOWN",
    "INVALID_ARGUMENT",
    "DEADLINE_EXCEEDED",
    "NOT_FOUND",
    "ALREADY_EXISTS",
    "PERMISSION_DENIED",
    "RESOURCE_EXHAUSTED",
    "FAILED_PRECONDITION",
    "ABORTED",
    "OUT_OF_RANGE",
    "UNIMPLEMENTED",
    "INTERNAL",
    "UNAVAILABLE",
    "DATA_LOSS",
    "UNAUTHENTICATED",
];

pub(crate) const CANCELLED: u32 = 1;
pub(crate) const UNKNOWN: u32 = 2;
pub(crate) const DEADLINE_EXCEEDED: u32 = 4;
pub(crate) const RESOURCE_EXHAUSTED: u32 = 8;
pub(crate) const UNIMPLEMENTED: u32 = 12;
pub(crate) const INTERNAL: u32 = 13;
pub(crate) const UNAVAILABLE: u32 = 14;

/// `NOT_FOUND` for 5; `CODE_42` for codes outside the spec.
pub fn status_name(code: u32) -> String {
    STATUS_NAMES.get(code as usize).map(|s| s.to_string()).unwrap_or_else(|| format!("CODE_{code}"))
}

/// A message with its 5-byte header (flag 0 = not compressed, big-endian length).
pub(crate) fn frame(payload: &[u8]) -> Bytes {
    let mut out = BytesMut::with_capacity(5 + payload.len());
    out.put_u8(0);
    out.put_u32(payload.len() as u32);
    out.extend_from_slice(payload);
    out.freeze()
}

/// Splits received DATA frames into messages.
#[derive(Default)]
pub(crate) struct Deframer {
    buf: BytesMut,
}

impl Deframer {
    pub(crate) fn push(&mut self, data: &[u8]) {
        self.buf.extend_from_slice(data);
    }

    /// The next complete message as (compressed, payload). A header announcing
    /// more than [`MAX_MESSAGE_BYTES`] is an error before anything is buffered;
    /// so is a flag other than 0 and 1 (e.g. a gRPC-Web trailer frame).
    pub(crate) fn next(&mut self) -> Result<Option<(bool, Bytes)>, GrpcStatus> {
        if self.buf.len() < 5 {
            return Ok(None);
        }
        let flag = self.buf[0];
        if flag > 1 {
            let message = format!("Received a message with the invalid compressed flag {flag:#04x}");
            return Err(GrpcStatus::new(INTERNAL, message, true));
        }
        let len = u32::from_be_bytes([self.buf[1], self.buf[2], self.buf[3], self.buf[4]]) as usize;
        if len > MAX_MESSAGE_BYTES {
            let message =
                format!("Received a message of {len} bytes, more than the {} MB limit", MAX_MESSAGE_BYTES >> 20);
            return Err(GrpcStatus::new(RESOURCE_EXHAUSTED, message, true));
        }
        if self.buf.len() < 5 + len {
            return Ok(None);
        }
        let compressed = flag == 1;
        self.buf.advance(5);
        Ok(Some((compressed, self.buf.split_to(len).freeze())))
    }

    /// Bytes of an unfinished message are left over.
    pub(crate) fn has_partial(&self) -> bool {
        !self.buf.is_empty()
    }
}

/// Undo `grpc-encoding` on a message flagged as compressed (only gzip is supported).
pub(crate) fn decompress(payload: &[u8], encoding: Option<&str>) -> Result<Bytes, String> {
    match encoding.map(str::trim) {
        Some("gzip") => {
            let mut out = Vec::new();
            let limit = MAX_MESSAGE_BYTES as u64 + 1;
            flate2::read::GzDecoder::new(payload)
                .take(limit)
                .read_to_end(&mut out)
                .map_err(|e| format!("Could not decompress a gzip message: {e}"))?;
            if out.len() > MAX_MESSAGE_BYTES {
                return Err(format!("A compressed message expands to more than {} MB", MAX_MESSAGE_BYTES >> 20));
            }
            Ok(Bytes::from(out))
        }
        None | Some("") | Some("identity") => {
            Err("The server sent a compressed message without saying how (no grpc-encoding header)".into())
        }
        Some(other) => Err(format!("The server compressed a message with '{other}'; only gzip can be decoded")),
    }
}

/// `grpc-timeout` value: at most 8 digits and a unit.
pub(crate) fn timeout_value(d: Duration) -> String {
    let ms = d.as_millis().max(1);
    if ms < 100_000_000 {
        return format!("{ms}m");
    }
    let secs = d.as_secs();
    if secs < 100_000_000 { format!("{secs}S") } else { format!("{}H", (secs / 3600).min(99_999_999)) }
}

/// `application/grpc`, optionally with `+proto`, `+json`, … or parameters
/// (not `application/grpc-web`, whose framing differs).
pub(crate) fn is_grpc_content_type(value: &str) -> bool {
    let value = value.trim();
    let Some(rest) = value.get(..16).filter(|p| p.eq_ignore_ascii_case("application/grpc")).map(|_| &value[16..])
    else {
        return false;
    };
    rest.is_empty() || rest.starts_with('+') || rest.starts_with(';')
}

/// gRPC code for a non-200 HTTP response (gRPC's HTTP to gRPC status mapping).
pub(crate) fn code_for_http_status(status: u16) -> u32 {
    match status {
        400 => INTERNAL,
        401 => 16,
        403 => 7,
        404 => UNIMPLEMENTED,
        429 | 502 | 503 | 504 => UNAVAILABLE,
        _ => UNKNOWN,
    }
}

/// The status in `grpc-status` / `grpc-message` / `grpc-status-details-bin`, if any.
pub(crate) fn status_from(headers: &http::HeaderMap, pool: Option<&DescriptorPool>) -> Option<GrpcStatus> {
    let raw = headers.get("grpc-status")?;
    let code = std::str::from_utf8(raw.as_bytes()).ok().and_then(|s| s.trim().parse::<u32>().ok());
    let message = headers
        .get("grpc-message")
        .map(|v| percent_encoding::percent_decode(v.as_bytes()).decode_utf8_lossy().into_owned())
        .unwrap_or_default();
    let details =
        headers.get("grpc-status-details-bin").map(|v| describe_details(&String::from_utf8_lossy(v.as_bytes()), pool));
    Some(match code {
        Some(code) => GrpcStatus::new(code, message, false).with_details(details),
        None => GrpcStatus::new(
            UNKNOWN,
            format!("Invalid grpc-status '{}' from the server", String::from_utf8_lossy(raw.as_bytes())),
            true,
        ),
    })
}

/// Response metadata for display. Header names are already lowercase on HTTP/2.
pub(crate) fn metadata(headers: &http::HeaderMap) -> Vec<Header> {
    headers.iter().map(|(n, v)| Header::new(n.as_str(), String::from_utf8_lossy(v.as_bytes()))).collect()
}

/// A `-bin` metadata value: base64 input is sent as given (unpadded), other
/// text is base64-encoded (like grpcurl).
pub(crate) fn binary_value(value: &str) -> String {
    let engine = base64::engine::general_purpose::STANDARD_NO_PAD;
    match decode_base64(value.trim()) {
        Some(bytes) => engine.encode(bytes),
        None => engine.encode(value.as_bytes()),
    }
}

/// Standard or URL-safe base64, padded or not.
fn decode_base64(value: &str) -> Option<Vec<u8>> {
    use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD};
    if value.is_empty() {
        return None;
    }
    [STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD].iter().find_map(|e| e.decode(value).ok())
}

/// `google.rpc.Status` as sent in `grpc-status-details-bin`.
#[derive(Clone, PartialEq, prost::Message)]
struct RpcStatus {
    #[prost(int32, tag = "1")]
    code: i32,
    #[prost(string, tag = "2")]
    message: String,
    #[prost(message, repeated, tag = "3")]
    details: Vec<RpcAny>,
}

#[derive(Clone, PartialEq, prost::Message)]
struct RpcAny {
    #[prost(string, tag = "1")]
    type_url: String,
    #[prost(bytes = "vec", tag = "2")]
    value: Vec<u8>,
}

/// The details of a `google.rpc.Status` as pretty JSON: messages whose type is
/// known (from reflection or the .proto files) are decoded, others stay base64.
/// Anything that isn't a `google.rpc.Status` is shown as the raw value.
fn describe_details(raw: &str, pool: Option<&DescriptorPool>) -> String {
    let Some(status) = decode_base64(raw.trim()).and_then(|b| RpcStatus::decode(b.as_slice()).ok()) else {
        return raw.to_string();
    };
    let details: Vec<serde_json::Value> = status
        .details
        .iter()
        .map(|any| {
            let name = any.type_url.rsplit('/').next().unwrap_or_default();
            let decoded = pool
                .and_then(|p| p.get_message_by_name(name))
                .and_then(|desc| DynamicMessage::decode(desc, any.value.as_slice()).ok())
                .and_then(|msg| super::schema::message_value(&msg).ok());
            match decoded {
                Some(serde_json::Value::Object(fields)) => {
                    let mut out = serde_json::Map::new();
                    out.insert("@type".into(), any.type_url.clone().into());
                    out.extend(fields);
                    serde_json::Value::Object(out)
                }
                _ => serde_json::json!({
                    "@type": any.type_url,
                    "value": base64::engine::general_purpose::STANDARD.encode(&any.value),
                }),
            }
        })
        .collect();
    serde_json::to_string_pretty(&details).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_round_trip_and_split_across_chunks() {
        let mut d = Deframer::default();
        let mut data = frame(b"hello").to_vec();
        data.extend_from_slice(&frame(b""));
        data.extend_from_slice(&frame(b"world")[..4]);
        d.push(&data[..3]);
        assert_eq!(d.next().unwrap(), None);
        d.push(&data[3..]);
        assert_eq!(d.next().unwrap(), Some((false, Bytes::from_static(b"hello"))));
        assert_eq!(d.next().unwrap(), Some((false, Bytes::new())));
        assert_eq!(d.next().unwrap(), None);
        assert!(d.has_partial());
        d.push(&frame(b"world")[4..]);
        assert_eq!(d.next().unwrap(), Some((false, Bytes::from_static(b"world"))));
        assert!(!d.has_partial());
    }

    #[test]
    fn oversized_and_compressed_messages() {
        let mut d = Deframer::default();
        d.push(&[0, 0xff, 0xff, 0xff, 0xff]);
        let err = d.next().unwrap_err();
        assert!(err.message.contains("64 MB") && err.name == "RESOURCE_EXHAUSTED", "{err:?}");
        // gRPC-Web's trailer frame (0x80) is not a message.
        let mut d = Deframer::default();
        d.push(&[0x80, 0, 0, 0, 0]);
        assert_eq!(d.next().unwrap_err().name, "INTERNAL");

        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        std::io::Write::write_all(&mut gz, b"payload").unwrap();
        let gz = gz.finish().unwrap();
        assert_eq!(&decompress(&gz, Some("gzip")).unwrap()[..], b"payload");
        assert!(decompress(&gz, Some("snappy")).unwrap_err().contains("snappy"));
        assert!(decompress(&gz, None).unwrap_err().contains("grpc-encoding"));
    }

    #[test]
    fn grpc_content_types() {
        for ok in ["application/grpc", "application/grpc+proto", "Application/GRPC+json", "application/grpc; x=1"] {
            assert!(is_grpc_content_type(ok), "{ok}");
        }
        for bad in ["application/grpc-web", "application/grpc-web+proto", "application/grpcx", "text/html", "", "ü"] {
            assert!(!is_grpc_content_type(bad), "{bad}");
        }
    }

    #[test]
    fn timeouts_and_http_status_mapping() {
        assert_eq!(timeout_value(Duration::from_millis(1500)), "1500m");
        assert_eq!(timeout_value(Duration::ZERO), "1m");
        assert_eq!(timeout_value(Duration::from_secs(200_000)), "200000S");
        assert_eq!(code_for_http_status(404), UNIMPLEMENTED);
        assert_eq!(code_for_http_status(503), UNAVAILABLE);
        assert_eq!(code_for_http_status(500), UNKNOWN);
        assert_eq!(status_name(16), "UNAUTHENTICATED");
        assert_eq!(status_name(99), "CODE_99");
    }

    #[test]
    fn status_trailers_are_decoded() {
        let mut h = http::HeaderMap::new();
        h.insert("grpc-status", "5".parse().unwrap());
        h.insert("grpc-message", "no%20such%20item%3A%20%E2%9C%93".parse().unwrap());
        let details = RpcStatus {
            code: 5,
            message: "x".into(),
            details: vec![RpcAny { type_url: "type.googleapis.com/test.Unknown".into(), value: vec![1, 2] }],
        };
        let b64 = base64::engine::general_purpose::STANDARD_NO_PAD.encode(details.encode_to_vec());
        h.insert("grpc-status-details-bin", b64.parse().unwrap());
        let s = status_from(&h, None).unwrap();
        assert_eq!((s.code, s.name.as_str(), s.message.as_str()), (5, "NOT_FOUND", "no such item: ✓"));
        let d = s.details.unwrap();
        assert!(d.contains("type.googleapis.com/test.Unknown") && d.contains("AQI="), "{d}");
        h.insert("grpc-status", "abc".parse().unwrap());
        assert_eq!(status_from(&h, None).unwrap().code, UNKNOWN);
        assert!(status_from(&http::HeaderMap::new(), None).is_none());
    }

    #[test]
    fn binary_metadata_values() {
        assert_eq!(binary_value("AQID"), "AQID");
        assert_eq!(binary_value("AQI="), "AQI");
        assert_eq!(binary_value("not base64!"), "bm90IGJhc2U2NCE");
    }
}
