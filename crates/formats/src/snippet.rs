//! Code snippet export.
//!
//! [`to_snippet`] renders a resolved request, like [`to_curl`](crate::curl::to_curl) does, as a short
//! program: Kotlin with OkHttp, Swift with URLSession, JavaScript `fetch` or Python `requests`.

use std::fmt::Write as _;

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use zorvik_engine::{Header, HttpRequest};

/// Languages a request can be exported to as code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum SnippetLanguage {
    /// Kotlin with OkHttp (Android).
    Kotlin,
    /// Swift with URLSession (iOS, macOS).
    Swift,
    /// JavaScript `fetch` (browsers, Node.js 18+).
    #[serde(rename = "javascript")]
    JavaScript,
    /// Python with `requests`.
    Python,
}

/// `req` as ready-to-run code in `lang`.
pub fn to_snippet(req: &HttpRequest, lang: SnippetLanguage) -> String {
    let lines = match lang {
        SnippetLanguage::Kotlin => kotlin(req),
        SnippetLanguage::Swift => swift(req),
        SnippetLanguage::JavaScript => javascript(req),
        SnippetLanguage::Python => python(req),
    };
    let mut code = lines.join("\n");
    code.push('\n');
    code
}

// ---------------------------------------------------------------------------
// What a snippet sends
// ---------------------------------------------------------------------------

/// A request body as a snippet embeds it.
enum Payload<'a> {
    Text(&'a str),
    /// Base64 of a body that is not UTF-8, decoded by the snippet.
    Base64(String),
}

/// Headers and body a snippet sets, leaving out what its HTTP library handles itself.
struct Parts<'a> {
    headers: Vec<&'a Header>,
    body: Option<Payload<'a>>,
    /// Code comments saying what was left out and why.
    notes: Vec<String>,
}

impl<'a> Parts<'a> {
    fn new(req: &'a HttpRequest, lang: SnippetLanguage) -> Self {
        // (library, whether it sets Accept-Encoding itself, whether it can send a body with GET and HEAD)
        let (library, owns_encoding, body_with_get) = match lang {
            SnippetLanguage::Kotlin => ("OkHttp", true, false),
            SnippetLanguage::Swift => ("URLSession", true, false),
            SnippetLanguage::JavaScript => ("fetch", false, false),
            SnippetLanguage::Python => ("requests", false, true),
        };
        let mut notes = Vec::new();
        let mut body = match std::str::from_utf8(&req.body) {
            _ if req.body.is_empty() => None,
            Ok(text) => Some(Payload::Text(text)),
            Err(_) => Some(Payload::Base64(base64::engine::general_purpose::STANDARD.encode(&req.body))),
        };
        if body.is_some() && !body_with_get && matches!(req.method.as_str(), "GET" | "HEAD") {
            notes.push(format!("{library} cannot send a body with {}, so it is left out.", req.method));
            body = None;
        }
        let mut headers = Vec::new();
        let mut dropped_encoding = false;
        for h in &req.headers {
            // Every library derives these from the body and the URL.
            if is(h, "content-length") || is(h, "host") {
                continue;
            }
            if owns_encoding && is(h, "accept-encoding") {
                dropped_encoding = true;
                continue;
            }
            headers.push(h);
        }
        if dropped_encoding {
            notes.push(format!(
                "Accept-Encoding is left out: {library} adds it and then decompresses the response itself."
            ));
        }
        Parts { headers, body, notes }
    }
}

fn is(h: &Header, name: &str) -> bool {
    h.name.eq_ignore_ascii_case(name)
}

/// Values of each header name (ignoring case), in order of first appearance.
fn grouped<'a>(headers: &[&'a Header]) -> Vec<(&'a str, Vec<&'a str>)> {
    let mut groups: Vec<(&str, Vec<&str>)> = Vec::new();
    for h in headers {
        match groups.iter_mut().find(|(name, _)| name.eq_ignore_ascii_case(&h.name)) {
            Some((_, values)) => values.push(&h.value),
            None => groups.push((&h.name, vec![&h.value])),
        }
    }
    groups
}

fn comments<'a>(prefix: &'a str, notes: &'a [String]) -> impl Iterator<Item = String> + 'a {
    notes.iter().map(move |note| format!("{prefix} {note}"))
}

fn lines<const N: usize>(fixed: [&str; N]) -> impl Iterator<Item = String> {
    fixed.into_iter().map(String::from)
}

// ---------------------------------------------------------------------------
// Languages
// ---------------------------------------------------------------------------

/// OkHttp 4, as a Kotlin script.
fn kotlin(req: &HttpRequest) -> Vec<String> {
    let q = kotlin_string;
    let Parts { mut headers, body, notes } = Parts::new(req, SnippetLanguage::Kotlin);
    let method = req.method.as_str();
    // OkHttp sends the body's media type as Content-Type. `String.toRequestBody` would add a
    // charset to it and could re-encode the text, so the body goes as bytes.
    let body = body.map(|body| {
        let content_type = headers.iter().find(|h| is(h, "content-type"));
        let media_type = content_type.map_or("application/octet-stream", |h| h.value.as_str());
        let bytes = match body {
            Payload::Text(text) => format!("{}.toByteArray()", q(text)),
            Payload::Base64(base64) => format!("java.util.Base64.getDecoder().decode({})", q(&base64)),
        };
        format!("{bytes}.toRequestBody({}.toMediaType())", q(media_type))
    });
    let has_media_type = body.is_some();
    if has_media_type {
        headers.retain(|h| !is(h, "content-type"));
    }
    // OkHttp rejects these methods without a body.
    let body = body.or_else(|| {
        matches!(method, "POST" | "PUT" | "PATCH" | "PROPPATCH" | "REPORT")
            .then(|| "ByteArray(0).toRequestBody(null)".to_string())
    });
    let call = match (method, body.is_some()) {
        ("GET", false) => ".get()".to_string(),
        ("HEAD", false) => ".head()".to_string(),
        ("DELETE", false) => ".delete()".to_string(),
        ("POST" | "PUT" | "PATCH" | "DELETE", true) => format!(".{}(body)", method.to_ascii_lowercase()),
        (_, true) => format!(".method({}, body)", q(method)),
        (_, false) => format!(".method({}, null)", q(method)),
    };

    let mut out = Vec::new();
    if has_media_type {
        out.push("import okhttp3.MediaType.Companion.toMediaType".to_string());
    }
    out.extend(lines(["import okhttp3.OkHttpClient", "import okhttp3.Request"]));
    if body.is_some() {
        out.push("import okhttp3.RequestBody.Companion.toRequestBody".to_string());
    }
    out.extend(lines(["", "val client = OkHttpClient()"]));
    if let Some(body) = &body {
        out.push(format!("val body = {body}"));
    }
    out.extend(comments("//", &notes));
    out.push("val request = Request.Builder()".to_string());
    out.push(format!("    .url({})", q(&req.url)));
    out.push(format!("    {call}"));
    out.extend(headers.iter().map(|h| format!("    .addHeader({}, {})", q(&h.name), q(&h.value))));
    out.extend(lines([
        "    .build()",
        "",
        "client.newCall(request).execute().use { response ->",
        "    println(response.code)",
        "    println(response.body?.string())",
        "}",
    ]));
    out
}

/// URLSession with async/await (macOS 12, iOS 15).
fn swift(req: &HttpRequest) -> Vec<String> {
    let q = swift_string;
    let Parts { headers, body, notes } = Parts::new(req, SnippetLanguage::Swift);
    let mut setup = Vec::new();
    if req.method != "GET" {
        setup.push(format!("request.httpMethod = {}", q(&req.method)));
    }
    for (i, h) in headers.iter().enumerate() {
        // `setValue` replaces a header, `addValue` appends to it.
        let repeated = headers[..i].iter().any(|prev| prev.name.eq_ignore_ascii_case(&h.name));
        let set = if repeated { "addValue" } else { "setValue" };
        setup.push(format!("request.{set}({}, forHTTPHeaderField: {})", q(&h.value), q(&h.name)));
    }
    match body {
        Some(Payload::Text(text)) => setup.push(format!("request.httpBody = Data({}.utf8)", q(text))),
        Some(Payload::Base64(base64)) => {
            setup.push(format!("request.httpBody = Data(base64Encoded: {})!", q(&base64)));
        }
        None => {}
    }

    let mut out: Vec<String> = lines(["import Foundation", ""]).collect();
    out.extend(comments("//", &notes));
    let binding = if setup.is_empty() { "let" } else { "var" };
    out.push(format!("{binding} request = URLRequest(url: URL(string: {})!)", q(&req.url)));
    out.extend(setup);
    out.extend(lines([
        "",
        "let (data, response) = try await URLSession.shared.data(for: request)",
        "if let response = response as? HTTPURLResponse {",
        "    print(response.statusCode)",
        "}",
        "print(String(decoding: data, as: UTF8.self))",
    ]));
    out
}

/// `fetch` with top-level await (an ES module, or a browser console).
fn javascript(req: &HttpRequest) -> Vec<String> {
    let q = json_string;
    let Parts { headers, body, notes } = Parts::new(req, SnippetLanguage::JavaScript);
    let mut options = Vec::new();
    if req.method != "GET" {
        options.push(format!("  method: {},", q(&req.method)));
    }
    if grouped(&headers).len() < headers.len() {
        // An object holds one value per name; `fetch` appends pairs.
        options.push("  headers: [".to_string());
        options.extend(headers.iter().map(|h| format!("    [{}, {}],", q(&h.name), q(&h.value))));
        options.push("  ],".to_string());
    } else if !headers.is_empty() {
        options.push("  headers: {".to_string());
        options.extend(headers.iter().map(|h| format!("    {}: {},", q(&h.name), q(&h.value))));
        options.push("  },".to_string());
    }
    match body {
        Some(Payload::Text(text)) => options.push(format!("  body: {},", q(text))),
        Some(Payload::Base64(base64)) => {
            options.push(format!("  body: Uint8Array.from(atob({}), c => c.charCodeAt(0)),", q(&base64)));
        }
        None => {}
    }

    let mut out: Vec<String> = comments("//", &notes).collect();
    if options.is_empty() {
        out.push(format!("const response = await fetch({});", q(&req.url)));
    } else {
        out.push(format!("const response = await fetch({}, {{", q(&req.url)));
        out.extend(options);
        out.push("});".to_string());
    }
    out.push("console.log(response.status, await response.text());".to_string());
    out
}

/// `requests`, Python 3.
fn python(req: &HttpRequest) -> Vec<String> {
    let q = json_string;
    let Parts { headers, body, notes } = Parts::new(req, SnippetLanguage::Python);
    let mut out = Vec::new();
    if matches!(body, Some(Payload::Base64(_))) {
        out.extend(lines(["import base64", ""]));
    }
    out.extend(lines(["import requests", ""]));
    out.push(format!("url = {}", q(&req.url)));
    let mut args = String::from("url");
    if !headers.is_empty() {
        let groups = grouped(&headers);
        let repeated: Vec<_> = groups.iter().filter(|(_, values)| values.len() > 1).map(|(name, _)| q(name)).collect();
        if !repeated.is_empty() {
            out.push(format!(
                "# A dict holds one value per header, so repeated headers are joined: {}.",
                repeated.join(", ")
            ));
        }
        out.push("headers = {".to_string());
        for (name, values) in groups {
            let separator = if name.eq_ignore_ascii_case("cookie") { "; " } else { ", " };
            out.push(format!("    {}: {},", q(name), q(&values.join(separator))));
        }
        out.push("}".to_string());
        args.push_str(", headers=headers");
    }
    if let Some(body) = body {
        out.push(match body {
            Payload::Text(text) if text.is_ascii() => format!("data = {}", q(text)),
            // Older urllib3 versions send `str` bodies as Latin-1.
            Payload::Text(text) => format!("data = {}.encode()", q(text)),
            Payload::Base64(base64) => format!("data = base64.b64decode({})", q(&base64)),
        });
        args.push_str(", data=data");
    }
    out.push(String::new());
    out.extend(comments("#", &notes));
    out.push(format!("response = requests.request({}, {args})", q(&req.method)));
    out.push("print(response.status_code, response.text)".to_string());
    out
}

// ---------------------------------------------------------------------------
// String literals
// ---------------------------------------------------------------------------

/// A Kotlin string literal. `$` would start a string template.
fn kotlin_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '$' => out.push_str("\\$"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => {
                let _ = write!(out, "\\u{:04x}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A Swift string literal. Every backslash is doubled, so `\(` cannot start an interpolation.
fn swift_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => {
                let _ = write!(out, "\\u{{{:x}}}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A JSON string, which is also a valid JavaScript and Python string literal.
fn json_string(s: &str) -> String {
    serde_json::Value::from(s).to_string()
}

#[cfg(test)]
mod tests {
    use super::SnippetLanguage::{JavaScript, Kotlin, Python, Swift};
    use super::*;

    const ALL: [SnippetLanguage; 4] = [Kotlin, Swift, JavaScript, Python];

    fn http(method: &str, url: &str, headers: &[(&str, &str)], body: &str) -> HttpRequest {
        HttpRequest {
            method: method.into(),
            url: url.into(),
            headers: headers.iter().map(|(n, v)| Header::new(*n, *v)).collect(),
            body: body.as_bytes().to_vec().into(),
        }
    }

    fn get_request() -> HttpRequest {
        http(
            "GET",
            "https://api.example.com/users?page=2&sort=name",
            &[("Accept", "application/json"), ("Authorization", "Bearer abc123")],
            "",
        )
    }

    /// Pretty JSON with quotes, backslashes, newlines, `$`, `\(` and non-ASCII text.
    const JSON_BODY: &str = r#"{
  "say": "hi \"there\"",
  "path": "C:\\tmp",
  "price": "$5 ${total}",
  "swift": "\(name)",
  "unicode": "héllo 世界 🎉"
}"#;

    fn post_request() -> HttpRequest {
        http(
            "POST",
            "https://api.example.com/items",
            &[("Content-Type", "application/json"), ("Authorization", "Bearer abc123")],
            JSON_BODY,
        )
    }

    fn binary_request() -> HttpRequest {
        HttpRequest { body: vec![0xff, 0, b'a', 0xfe].into(), ..http("PUT", "https://x.test/upload", &[], "") }
    }

    fn duplicates_request() -> HttpRequest {
        http(
            "GET",
            "https://x.test/",
            &[("X-Tag", "a"), ("Accept", "*/*"), ("x-tag", "b"), ("Cookie", "a=1"), ("cookie", "b=2")],
            "",
        )
    }

    fn managed_request() -> HttpRequest {
        http(
            "POST",
            "https://api.example.com/",
            &[("Host", "api.example.com"), ("Content-Length", "2"), ("Accept-Encoding", "gzip"), ("X-Keep", "1")],
            "{}",
        )
    }

    #[test]
    fn snippet_language_wire_names() {
        assert_eq!(serde_json::to_string(&ALL).unwrap(), r#"["kotlin","swift","javascript","python"]"#);
        let parsed: Vec<SnippetLanguage> = serde_json::from_str(r#"["kotlin","swift","javascript","python"]"#).unwrap();
        assert_eq!(parsed, ALL);
    }

    #[test]
    fn snippet_get_kotlin() {
        assert_eq!(
            to_snippet(&get_request(), Kotlin),
            r#"import okhttp3.OkHttpClient
import okhttp3.Request

val client = OkHttpClient()
val request = Request.Builder()
    .url("https://api.example.com/users?page=2&sort=name")
    .get()
    .addHeader("Accept", "application/json")
    .addHeader("Authorization", "Bearer abc123")
    .build()

client.newCall(request).execute().use { response ->
    println(response.code)
    println(response.body?.string())
}
"#
        );
    }

    #[test]
    fn snippet_get_swift() {
        assert_eq!(
            to_snippet(&get_request(), Swift),
            r#"import Foundation

var request = URLRequest(url: URL(string: "https://api.example.com/users?page=2&sort=name")!)
request.setValue("application/json", forHTTPHeaderField: "Accept")
request.setValue("Bearer abc123", forHTTPHeaderField: "Authorization")

let (data, response) = try await URLSession.shared.data(for: request)
if let response = response as? HTTPURLResponse {
    print(response.statusCode)
}
print(String(decoding: data, as: UTF8.self))
"#
        );
        // Nothing to set: the request stays immutable.
        let bare = to_snippet(&http("GET", "https://x.test/", &[], ""), Swift);
        assert!(bare.contains("\nlet request = URLRequest(url: URL(string: \"https://x.test/\")!)\n"), "{bare}");
    }

    #[test]
    fn snippet_get_javascript() {
        assert_eq!(
            to_snippet(&get_request(), JavaScript),
            r#"const response = await fetch("https://api.example.com/users?page=2&sort=name", {
  headers: {
    "Accept": "application/json",
    "Authorization": "Bearer abc123",
  },
});
console.log(response.status, await response.text());
"#
        );
        assert_eq!(
            to_snippet(&http("GET", "https://x.test/", &[], ""), JavaScript),
            "const response = await fetch(\"https://x.test/\");\n\
             console.log(response.status, await response.text());\n"
        );
    }

    #[test]
    fn snippet_get_python() {
        assert_eq!(
            to_snippet(&get_request(), Python),
            r#"import requests

url = "https://api.example.com/users?page=2&sort=name"
headers = {
    "Accept": "application/json",
    "Authorization": "Bearer abc123",
}

response = requests.request("GET", url, headers=headers)
print(response.status_code, response.text)
"#
        );
    }

    #[test]
    fn snippet_post_json_kotlin() {
        let code = to_snippet(&post_request(), Kotlin);
        assert!(code.contains(r#"\"price\": \"\$5 \${total}\""#), "{code}");
        assert_eq!(
            code,
            r#"import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody

val client = OkHttpClient()
val body = "{\n  \"say\": \"hi \\\"there\\\"\",\n  \"path\": \"C:\\\\tmp\",\n  \"price\": \"\$5 \${total}\",\n  \"swift\": \"\\(name)\",\n  \"unicode\": \"héllo 世界 🎉\"\n}".toByteArray().toRequestBody("application/json".toMediaType())
val request = Request.Builder()
    .url("https://api.example.com/items")
    .post(body)
    .addHeader("Authorization", "Bearer abc123")
    .build()

client.newCall(request).execute().use { response ->
    println(response.code)
    println(response.body?.string())
}
"#
        );
    }

    #[test]
    fn snippet_post_json_swift() {
        let code = to_snippet(&post_request(), Swift);
        assert!(code.contains(r#"\"swift\": \"\\(name)\""#), "{code}");
        assert_eq!(
            code,
            r#"import Foundation

var request = URLRequest(url: URL(string: "https://api.example.com/items")!)
request.httpMethod = "POST"
request.setValue("application/json", forHTTPHeaderField: "Content-Type")
request.setValue("Bearer abc123", forHTTPHeaderField: "Authorization")
request.httpBody = Data("{\n  \"say\": \"hi \\\"there\\\"\",\n  \"path\": \"C:\\\\tmp\",\n  \"price\": \"$5 ${total}\",\n  \"swift\": \"\\(name)\",\n  \"unicode\": \"héllo 世界 🎉\"\n}".utf8)

let (data, response) = try await URLSession.shared.data(for: request)
if let response = response as? HTTPURLResponse {
    print(response.statusCode)
}
print(String(decoding: data, as: UTF8.self))
"#
        );
    }

    #[test]
    fn snippet_post_json_javascript() {
        assert_eq!(
            to_snippet(&post_request(), JavaScript),
            r#"const response = await fetch("https://api.example.com/items", {
  method: "POST",
  headers: {
    "Content-Type": "application/json",
    "Authorization": "Bearer abc123",
  },
  body: "{\n  \"say\": \"hi \\\"there\\\"\",\n  \"path\": \"C:\\\\tmp\",\n  \"price\": \"$5 ${total}\",\n  \"swift\": \"\\(name)\",\n  \"unicode\": \"héllo 世界 🎉\"\n}",
});
console.log(response.status, await response.text());
"#
        );
    }

    #[test]
    fn snippet_post_json_python() {
        assert_eq!(
            to_snippet(&post_request(), Python),
            r#"import requests

url = "https://api.example.com/items"
headers = {
    "Content-Type": "application/json",
    "Authorization": "Bearer abc123",
}
data = "{\n  \"say\": \"hi \\\"there\\\"\",\n  \"path\": \"C:\\\\tmp\",\n  \"price\": \"$5 ${total}\",\n  \"swift\": \"\\(name)\",\n  \"unicode\": \"héllo 世界 🎉\"\n}".encode()

response = requests.request("POST", url, headers=headers, data=data)
print(response.status_code, response.text)
"#
        );
        // ASCII text goes as a plain string.
        let form = to_snippet(&http("POST", "https://x.test/", &[], "a=1&b=2"), Python);
        assert!(form.contains("\ndata = \"a=1&b=2\"\n"), "{form}");
    }

    #[test]
    fn snippet_binary_body_as_base64() {
        let req = binary_request();
        let kotlin = to_snippet(&req, Kotlin);
        assert!(
            kotlin.contains(
                "val body = java.util.Base64.getDecoder().decode(\"/wBh/g==\")\
                 .toRequestBody(\"application/octet-stream\".toMediaType())\n"
            ),
            "{kotlin}"
        );
        assert!(kotlin.contains("    .put(body)\n"), "{kotlin}");
        let swift = to_snippet(&req, Swift);
        assert!(swift.contains("\nrequest.httpBody = Data(base64Encoded: \"/wBh/g==\")!\n"), "{swift}");
        let js = to_snippet(&req, JavaScript);
        assert!(js.contains("\n  body: Uint8Array.from(atob(\"/wBh/g==\"), c => c.charCodeAt(0)),\n"), "{js}");
        let python = to_snippet(&req, Python);
        assert!(python.starts_with("import base64\n\nimport requests\n\n"), "{python}");
        assert!(python.contains("\ndata = base64.b64decode(\"/wBh/g==\")\n"), "{python}");
        assert!(python.contains("requests.request(\"PUT\", url, data=data)"), "{python}");
    }

    #[test]
    fn snippet_duplicate_headers() {
        let req = duplicates_request();
        assert_eq!(
            to_snippet(&req, JavaScript),
            r#"const response = await fetch("https://x.test/", {
  headers: [
    ["X-Tag", "a"],
    ["Accept", "*/*"],
    ["x-tag", "b"],
    ["Cookie", "a=1"],
    ["cookie", "b=2"],
  ],
});
console.log(response.status, await response.text());
"#
        );
        assert_eq!(
            to_snippet(&req, Python),
            r#"import requests

url = "https://x.test/"
# A dict holds one value per header, so repeated headers are joined: "X-Tag", "Cookie".
headers = {
    "X-Tag": "a, b",
    "Accept": "*/*",
    "Cookie": "a=1; b=2",
}

response = requests.request("GET", url, headers=headers)
print(response.status_code, response.text)
"#
        );
        let swift = to_snippet(&req, Swift);
        assert!(swift.contains("request.setValue(\"a\", forHTTPHeaderField: \"X-Tag\")\n"), "{swift}");
        assert!(swift.contains("request.addValue(\"b\", forHTTPHeaderField: \"x-tag\")\n"), "{swift}");
        let kotlin = to_snippet(&req, Kotlin);
        assert!(kotlin.contains(".addHeader(\"X-Tag\", \"a\")\n    .addHeader(\"Accept\""), "{kotlin}");
        assert!(kotlin.contains(".addHeader(\"x-tag\", \"b\")\n"), "{kotlin}");
    }

    #[test]
    fn snippet_kotlin_methods() {
        assert_eq!(
            to_snippet(&http("DELETE", "https://api.example.com/items/1", &[], ""), Kotlin),
            r#"import okhttp3.OkHttpClient
import okhttp3.Request

val client = OkHttpClient()
val request = Request.Builder()
    .url("https://api.example.com/items/1")
    .delete()
    .build()

client.newCall(request).execute().use { response ->
    println(response.code)
    println(response.body?.string())
}
"#
        );

        // OkHttp needs a body for POST; with none there is no media type, so Content-Type stays a header.
        let post = to_snippet(&http("POST", "https://x.test/", &[("Content-Type", "application/json")], ""), Kotlin);
        assert!(post.contains("\nval body = ByteArray(0).toRequestBody(null)\n"), "{post}");
        assert!(post.contains("    .post(body)\n    .addHeader(\"Content-Type\", \"application/json\")\n"), "{post}");
        assert!(post.contains("import okhttp3.RequestBody.Companion.toRequestBody\n"), "{post}");
        assert!(!post.contains("toMediaType"), "{post}");

        let cases = [
            (http("HEAD", "https://x.test/", &[], ""), "    .head()\n"),
            (http("OPTIONS", "https://x.test/", &[], ""), "    .method(\"OPTIONS\", null)\n"),
            (http("REPORT", "https://x.test/", &[], ""), "    .method(\"REPORT\", body)\n"),
            (http("PROPFIND", "https://x.test/", &[], "<a/>"), "    .method(\"PROPFIND\", body)\n"),
            (http("DELETE", "https://x.test/", &[], "x"), "    .delete(body)\n"),
            (http("PATCH", "https://x.test/", &[], "x"), "    .patch(body)\n"),
        ];
        for (req, call) in cases {
            let code = to_snippet(&req, Kotlin);
            assert!(code.contains(call), "{code}");
        }
        let options = to_snippet(&http("OPTIONS", "https://x.test/", &[], ""), Kotlin);
        assert!(!options.contains("toRequestBody"), "{options}");
        // Without a Content-Type header the body is sent as octet-stream.
        let put = to_snippet(&http("PUT", "https://x.test/", &[], "x"), Kotlin);
        assert!(
            put.contains("val body = \"x\".toByteArray().toRequestBody(\"application/octet-stream\".toMediaType())\n"),
            "{put}"
        );
    }

    #[test]
    fn snippet_drops_managed_headers() {
        let req = managed_request();
        for lang in ALL {
            let code = to_snippet(&req, lang);
            assert!(!code.contains("\"Host\""), "{lang:?}\n{code}");
            assert!(!code.contains("\"Content-Length\""), "{lang:?}\n{code}");
            assert!(code.contains("\"X-Keep\""), "{lang:?}\n{code}");
        }
        let kotlin = to_snippet(&req, Kotlin);
        assert!(!kotlin.contains("\"Accept-Encoding\""), "{kotlin}");
        assert!(
            kotlin.contains(
                "\n// Accept-Encoding is left out: OkHttp adds it and then decompresses the response itself.\n\
                 val request = "
            ),
            "{kotlin}"
        );
        let swift = to_snippet(&req, Swift);
        assert!(!swift.contains("\"Accept-Encoding\""), "{swift}");
        assert!(
            swift.contains(
                "\n// Accept-Encoding is left out: URLSession adds it and then decompresses the response itself.\n"
            ),
            "{swift}"
        );
        assert!(to_snippet(&req, JavaScript).contains("\n    \"Accept-Encoding\": \"gzip\",\n"));
        assert!(to_snippet(&req, Python).contains("\n    \"Accept-Encoding\": \"gzip\",\n"));
    }

    #[test]
    fn snippet_get_with_body() {
        let req = http("GET", "https://x.test/", &[], "q=1");
        let kotlin = to_snippet(&req, Kotlin);
        assert!(kotlin.contains("\n// OkHttp cannot send a body with GET, so it is left out.\n"), "{kotlin}");
        assert!(kotlin.contains("    .get()\n") && !kotlin.contains("toRequestBody"), "{kotlin}");
        let swift = to_snippet(&req, Swift);
        assert!(swift.contains("\n// URLSession cannot send a body with GET, so it is left out.\n"), "{swift}");
        assert!(!swift.contains("httpBody"), "{swift}");
        assert_eq!(
            to_snippet(&req, JavaScript),
            "// fetch cannot send a body with GET, so it is left out.\n\
             const response = await fetch(\"https://x.test/\");\n\
             console.log(response.status, await response.text());\n"
        );
        let python = to_snippet(&req, Python);
        assert!(python.contains("\ndata = \"q=1\"\n"), "{python}");
        assert!(python.contains("requests.request(\"GET\", url, data=data)"), "{python}");
    }

    #[test]
    fn snippet_string_escapes() {
        let s = "a\u{7}\u{0}\u{7f}\u{85}$\\\"\r\n\t\u{2028}é";
        assert_eq!(kotlin_string(s), "\"a\\u0007\\u0000\\u007f\\u0085\\$\\\\\\\"\\r\\n\\t\u{2028}é\"");
        assert_eq!(swift_string(s), "\"a\\u{7}\\u{0}\\u{7f}\\u{85}$\\\\\\\"\\r\\n\\t\u{2028}é\"");
        assert_eq!(swift_string("\\(x)"), r#""\\(x)""#);
        assert_eq!(json_string("\u{8}\u{c}\u{1}\"\\"), r#""\b\f\u0001\"\\""#);
    }

    /// Requests covering every branch, for the toolchain check.
    fn syntax_cases() -> Vec<HttpRequest> {
        vec![
            get_request(),
            post_request(),
            binary_request(),
            duplicates_request(),
            managed_request(),
            http("GET", "https://x.test/", &[], "q=1"),
            http("DELETE", "https://x.test/items/1", &[], ""),
            http("POST", "https://x.test/", &[("Content-Type", "text/plain")], ""),
            http("OPTIONS", "https://x.test/", &[], ""),
            http("PROPFIND", "https://x.test/", &[("Depth", "1")], "<a/>"),
            http(
                "PATCH",
                "https://x.test/p?x=$y&z=\\(w)",
                &[("X-Template", "$HOME ${x} \\(y) `z`")],
                "nul\u{0} bell\u{7} del\u{7f} ls\u{2028} bom\u{feff} é\r\n\ttab \"q\" 'a' \\ $x ${y} \\(z) `t` {{a}}",
            ),
        ]
    }

    /// Checks the syntax of the generated code with the toolchains that are installed (`node`,
    /// `python3`, `swiftc`). Kotlin needs OkHttp on the classpath, so the unit tests above cover it.
    /// The files stay in the temporary directory for inspection.
    #[test]
    #[ignore = "runs node, python3 and swiftc when installed"]
    fn snippet_syntax_with_toolchains() {
        use std::process::Command;

        let dir = std::env::temp_dir().join("zorvik-snippets");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // `swiftc -parse` needs no SDK (URLSession is in FoundationNetworking on Linux).
        let tools: [(SnippetLanguage, &str, &str, &[&str]); 3] = [
            (JavaScript, "mjs", "node", &["--check"]),
            (Python, "py", "python3", &["-m", "py_compile"]),
            (Swift, "swift", "swiftc", &["-parse"]),
        ];
        let mut checked = 0;
        for (lang, ext, tool, args) in tools {
            if !Command::new(tool).arg("--version").output().is_ok_and(|o| o.status.success()) {
                eprintln!("{tool} is not installed; skipped {lang:?}");
                continue;
            }
            for (i, req) in syntax_cases().iter().enumerate() {
                let code = to_snippet(req, lang);
                let path = dir.join(format!("case{i}.{ext}"));
                std::fs::write(&path, &code).unwrap();
                let output = Command::new(tool).args(args).arg(&path).output().unwrap();
                assert!(
                    output.status.success(),
                    "{tool} rejected {}:\n{}\n{code}",
                    path.display(),
                    String::from_utf8_lossy(&output.stderr)
                );
                checked += 1;
            }
            eprintln!("{tool}: {} snippets OK", syntax_cases().len());
        }
        eprintln!("checked {checked} snippets in {}", dir.display());
    }
}
