//! Response body decoding (Content-Encoding) and display helpers.

use std::io::Read;

/// Result of decoding a response body.
pub(crate) struct Decoded {
    pub body: Vec<u8>,
    /// Output hit `limit` and was cut.
    pub truncated: bool,
    /// Decoding failed; `body` holds the raw bytes.
    pub error: Option<String>,
}

/// Decode `raw` according to a Content-Encoding header value such as
/// `gzip` or `deflate, br`. Codings are undone in reverse order.
/// Output is capped at `limit` bytes to defuse decompression bombs.
pub(crate) fn decode_content(raw: Vec<u8>, content_encoding: &str, limit: usize) -> Decoded {
    let codings: Vec<String> = content_encoding
        .split(',')
        .map(|c| c.trim().to_ascii_lowercase())
        .filter(|c| !c.is_empty() && c != "identity")
        .collect();
    if codings.is_empty() {
        return Decoded { body: raw, truncated: false, error: None };
    }
    let mut current: Option<Vec<u8>> = None;
    let mut truncated = false;
    for coding in codings.iter().rev() {
        let input = current.as_deref().unwrap_or(&raw);
        match decode_one(input, coding, limit) {
            Ok((out, cut)) => {
                current = Some(out);
                truncated |= cut;
            }
            Err(e) => {
                return Decoded {
                    body: raw,
                    truncated: false,
                    error: Some(format!("Could not decode '{coding}' body: {e}. Showing raw bytes.")),
                };
            }
        }
    }
    Decoded { body: current.unwrap_or(raw), truncated, error: None }
}

fn decode_one(input: &[u8], coding: &str, limit: usize) -> Result<(Vec<u8>, bool), String> {
    match coding {
        "gzip" | "x-gzip" => read_limited(flate2::read::MultiGzDecoder::new(input), limit),
        "deflate" => {
            // Servers disagree on whether "deflate" means zlib-wrapped or raw.
            read_limited(flate2::read::ZlibDecoder::new(input), limit)
                .or_else(|_| read_limited(flate2::read::DeflateDecoder::new(input), limit))
        }
        "br" => read_limited(brotli_decompressor::Decompressor::new(input, 64 * 1024), limit),
        "zstd" => {
            let decoder = ruzstd::decoding::StreamingDecoder::new(input).map_err(|e| e.to_string())?;
            read_limited(decoder, limit)
        }
        other => Err(format!("unsupported encoding '{other}'")),
    }
}

fn read_limited(reader: impl Read, limit: usize) -> Result<(Vec<u8>, bool), String> {
    let mut out = Vec::new();
    let mut limited = reader.take((limit as u64).saturating_add(1));
    limited.read_to_end(&mut out).map_err(|e| e.to_string())?;
    let truncated = out.len() > limit;
    out.truncate(limit);
    Ok((out, truncated))
}

/// Pretty-print JSON without re-parsing numbers or reordering keys: only
/// whitespace changes, so the output is byte-faithful to the server's values.
/// Returns `None` when `input` is not valid JSON.
pub fn pretty_json(input: &str) -> Option<String> {
    serde_json::from_str::<serde::de::IgnoredAny>(input).ok()?;
    let mut out = String::with_capacity(input.len() + input.len() / 4);
    let mut indent = 0usize;
    let mut chars = input.chars().peekable();
    let newline = |out: &mut String, indent: usize| {
        out.push('\n');
        for _ in 0..indent {
            out.push_str("  ");
        }
    };
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                out.push('"');
                let mut escaped = false;
                for s in chars.by_ref() {
                    out.push(s);
                    if escaped {
                        escaped = false;
                    } else if s == '\\' {
                        escaped = true;
                    } else if s == '"' {
                        break;
                    }
                }
            }
            '{' | '[' => {
                out.push(c);
                // Keep empty containers compact: {} and [].
                while chars.peek().is_some_and(|n| n.is_whitespace()) {
                    chars.next();
                }
                let close = if c == '{' { '}' } else { ']' };
                if chars.peek() == Some(&close) {
                    out.push(close);
                    chars.next();
                } else {
                    indent += 1;
                    newline(&mut out, indent);
                }
            }
            '}' | ']' => {
                indent = indent.saturating_sub(1);
                newline(&mut out, indent);
                out.push(c);
            }
            ',' => {
                out.push(',');
                newline(&mut out, indent);
            }
            ':' => out.push_str(": "),
            c if c.is_whitespace() => {}
            c => out.push(c),
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn gzip_and_stacked_encodings() {
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gz.write_all(b"hello world").unwrap();
        let gz = gz.finish().unwrap();
        let d = decode_content(gz.clone(), "gzip", 1024);
        assert_eq!(d.body, b"hello world");
        assert!(d.error.is_none());

        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        z.write_all(&gz).unwrap();
        let stacked = z.finish().unwrap();
        let d = decode_content(stacked, "gzip, deflate", 1024);
        assert_eq!(d.body, b"hello world");
    }

    #[test]
    fn raw_deflate_fallback() {
        let mut e = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
        e.write_all(b"raw deflate").unwrap();
        let d = decode_content(e.finish().unwrap(), "deflate", 1024);
        assert_eq!(d.body, b"raw deflate");
    }

    #[test]
    fn bomb_is_capped() {
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
        gz.write_all(&vec![0u8; 10_000_000]).unwrap();
        let d = decode_content(gz.finish().unwrap(), "gzip", 1000);
        assert_eq!(d.body.len(), 1000);
        assert!(d.truncated);
    }

    #[test]
    fn bad_data_falls_back_to_raw() {
        let d = decode_content(b"not gzip".to_vec(), "gzip", 1024);
        assert_eq!(d.body, b"not gzip");
        assert!(d.error.is_some());
        let d = decode_content(b"x".to_vec(), "snappy", 1024);
        assert!(d.error.unwrap().contains("unsupported"));
    }

    #[test]
    fn pretty_json_preserves_values() {
        let input = r#"{"b":1.50,"a":[1,2,{}],"big":123456789012345678901234567890,"s":"x,\"y\":{"}"#;
        let out = pretty_json(input).unwrap();
        assert!(out.contains("1.50"));
        assert!(out.contains("123456789012345678901234567890"));
        assert!(out.contains(r#""s": "x,\"y\":{""#));
        assert!(out.find("\"b\"").unwrap() < out.find("\"a\"").unwrap());
        assert!(out.contains("{}"));
        assert!(pretty_json("{nope").is_none());
        assert_eq!(pretty_json("[]").unwrap(), "[]");
        assert_eq!(pretty_json(" { } ").unwrap(), "{}");
    }
}
