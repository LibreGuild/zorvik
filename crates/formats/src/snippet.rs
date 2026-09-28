//! Code snippet export.
//!
//! [`to_snippet`] renders a resolved request, like [`to_curl`](crate::curl::to_curl) does, as a short
//! program or command: Kotlin with OkHttp, Swift with URLSession, JavaScript `fetch` or axios, Python
//! `requests` or HTTPX, Go `net/http`, Java and C# `HttpClient`, PHP with curl, Ruby `Net::HTTP`,
//! Rust `reqwest`, Dart `package:http`, C with libcurl, PowerShell `Invoke-WebRequest`, HTTPie or Wget.

use std::fmt::Write as _;

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use zorvik_engine::{Header, HttpRequest};

use crate::curl::{PS_SINGLE_QUOTES, ansi_c_quote, quote_bash};

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
    /// JavaScript with axios (Node.js).
    #[serde(rename = "javascriptAxios")]
    JavaScriptAxios,
    /// Python with `requests`.
    Python,
    /// Python with HTTPX.
    PythonHttpx,
    /// Go with `net/http`.
    Go,
    /// Java with `java.net.http.HttpClient` (Java 11+).
    Java,
    /// C# with `HttpClient` (.NET 6+).
    #[serde(rename = "csharp")]
    CSharp,
    /// PHP with the curl extension.
    Php,
    /// Ruby with `Net::HTTP`.
    Ruby,
    /// Rust with `reqwest` and Tokio.
    Rust,
    /// Dart with `package:http` (Flutter).
    Dart,
    /// C with libcurl.
    C,
    /// PowerShell 7 `Invoke-WebRequest`.
    PowerShell,
    /// The HTTPie command line.
    Httpie,
    /// The Wget command line.
    Wget,
}

/// `req` as ready-to-run code in `lang`.
pub fn to_snippet(req: &HttpRequest, lang: SnippetLanguage) -> String {
    let lines = match lang {
        SnippetLanguage::Kotlin => kotlin(req),
        SnippetLanguage::Swift => swift(req),
        SnippetLanguage::JavaScript => javascript(req),
        SnippetLanguage::JavaScriptAxios => axios(req),
        SnippetLanguage::Python => python(req),
        SnippetLanguage::PythonHttpx => httpx(req),
        SnippetLanguage::Go => go(req),
        SnippetLanguage::Java => java(req),
        SnippetLanguage::CSharp => csharp(req),
        SnippetLanguage::Php => php(req),
        SnippetLanguage::Ruby => ruby(req),
        SnippetLanguage::Rust => rust(req),
        SnippetLanguage::Dart => dart(req),
        SnippetLanguage::C => c(req),
        SnippetLanguage::PowerShell => powershell(req),
        SnippetLanguage::Httpie => httpie(req),
        SnippetLanguage::Wget => wget(req),
    };
    let mut code = lines.join("\n");
    code.push('\n');
    code
}

/// `code` with `notes` as comments at the top (after PHP's opening tag), e.g. what the code
/// can't do that Zorvik does when sending.
pub fn noted(code: String, lang: SnippetLanguage, notes: &[String]) -> String {
    if notes.is_empty() {
        return code;
    }
    let prefix = match lang {
        SnippetLanguage::Python
        | SnippetLanguage::PythonHttpx
        | SnippetLanguage::Ruby
        | SnippetLanguage::PowerShell
        | SnippetLanguage::Httpie
        | SnippetLanguage::Wget => "#",
        _ => "//",
    };
    let block: String = notes.iter().map(|n| format!("{prefix} {n}\n")).collect();
    match code.strip_prefix("<?php\n") {
        Some(rest) => format!("<?php\n{block}{rest}"),
        None => format!("{block}{code}"),
    }
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
            SnippetLanguage::JavaScriptAxios => ("axios", true, true),
            SnippetLanguage::Python => ("requests", false, true),
            SnippetLanguage::PythonHttpx => ("HTTPX", false, true),
            SnippetLanguage::Go => ("net/http", false, true),
            SnippetLanguage::Java => ("HttpClient", false, true),
            SnippetLanguage::CSharp => ("HttpClient", false, true),
            SnippetLanguage::Php => ("curl", false, true),
            SnippetLanguage::Ruby => ("Net::HTTP", false, true),
            SnippetLanguage::Rust => ("reqwest", false, true),
            SnippetLanguage::Dart => ("package:http", true, true),
            SnippetLanguage::C => ("libcurl", false, true),
            SnippetLanguage::PowerShell => ("Invoke-WebRequest", true, false),
            SnippetLanguage::Httpie => ("HTTPie", false, true),
            SnippetLanguage::Wget => ("Wget", false, true),
        };
        // Headers the library refuses to set.
        let refused: &[&str] = match lang {
            SnippetLanguage::Java => &["connection", "expect", "upgrade"],
            _ => &[],
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
        let mut dropped: Vec<&str> = Vec::new();
        for h in &req.headers {
            // Every library derives these from the body and the URL.
            if is(h, "content-length") || is(h, "host") {
                continue;
            }
            if owns_encoding && is(h, "accept-encoding") {
                dropped_encoding = true;
                continue;
            }
            if refused.iter().any(|name| is(h, name)) {
                if !dropped.iter().any(|name| name.eq_ignore_ascii_case(&h.name)) {
                    dropped.push(&h.name);
                }
                continue;
            }
            headers.push(h);
        }
        if dropped_encoding {
            notes.push(format!(
                "Accept-Encoding is left out: {library} adds it and then decompresses the response itself."
            ));
        }
        if !dropped.is_empty() {
            notes.push(format!("{library} does not allow setting {}, so {}.", and_list(&dropped), left_out(&dropped)));
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

/// One value per header name, for libraries that keep headers in a map: repeated headers are
/// joined with `, ` (`; ` for Cookie), and a note says so, starting with `why`.
fn joined<'a>(headers: &[&'a Header], why: &str, q: fn(&str) -> String) -> (Vec<(&'a str, String)>, Option<String>) {
    let groups = grouped(headers);
    let repeated: Vec<_> = groups.iter().filter(|(_, values)| values.len() > 1).map(|(name, _)| q(name)).collect();
    let note =
        (!repeated.is_empty()).then(|| format!("{why}, so repeated headers are joined: {}.", repeated.join(", ")));
    let pairs = groups
        .into_iter()
        .map(|(name, values)| {
            let separator = if name.eq_ignore_ascii_case("cookie") { "; " } else { ", " };
            (name, values.join(separator))
        })
        .collect();
    (pairs, note)
}

/// Names in a sentence: `A`, `A and B`, `A, B and C`.
fn and_list(names: &[&str]) -> String {
    match names {
        [rest @ .., last] if !rest.is_empty() => format!("{} and {last}", rest.join(", ")),
        _ => names.concat(),
    }
}

fn left_out(names: &[&str]) -> &'static str {
    if names.len() == 1 { "it is left out" } else { "they are left out" }
}

fn comments<'a>(prefix: &'a str, notes: &'a [String]) -> impl Iterator<Item = String> + 'a {
    notes.iter().map(move |note| format!("{prefix} {note}"))
}

fn lines<const N: usize>(fixed: [&str; N]) -> impl Iterator<Item = String> {
    fixed.into_iter().map(String::from)
}

/// A header line for curl: `Name:` without a value makes curl drop the header; `Name;` sends it empty.
fn curl_header(h: &Header) -> String {
    if h.value.trim().is_empty() { format!("{};", h.name) } else { format!("{}: {}", h.name, h.value) }
}

/// Whether libcurl needs a custom method and whether it needs `NOBODY`: a body makes it send POST,
/// and a custom HEAD alone makes it wait for a response body that never comes.
fn curl_method(method: &str, has_body: bool) -> (bool, bool) {
    (has_body || !matches!(method, "GET" | "HEAD"), method == "HEAD")
}

/// A body for a command line: an argument cannot hold a NUL byte, so such text goes as base64 too.
fn shell_payload(body: Option<Payload<'_>>) -> Option<Payload<'_>> {
    body.map(|body| match body {
        Payload::Text(text) if text.contains('\0') => {
            Payload::Base64(base64::engine::general_purpose::STANDARD.encode(text))
        }
        body => body,
    })
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

/// axios in Node.js, as an ES module with top-level await.
fn axios(req: &HttpRequest) -> Vec<String> {
    let q = json_string;
    let Parts { headers, body, notes } = Parts::new(req, SnippetLanguage::JavaScriptAxios);
    // axios trims a string body sent as JSON, and quotes it when it does not parse.
    let json =
        headers.iter().any(|h| is(h, "content-type") && h.value.to_ascii_lowercase().contains("application/json"));
    let mut out: Vec<String> = lines(["import axios from \"axios\";", ""]).collect();
    out.extend(comments("//", &notes));
    out.push("const response = await axios({".to_string());
    out.push(format!("  method: {},", q(&req.method)));
    out.push(format!("  url: {},", q(&req.url)));
    if !headers.is_empty() {
        let (headers, note) = joined(&headers, "An object holds one value per header", q);
        out.extend(note.map(|note| format!("  // {note}")));
        out.push("  headers: {".to_string());
        out.extend(headers.iter().map(|(name, value)| format!("    {}: {},", q(name), q(value))));
        out.push("  },".to_string());
    }
    match body {
        Some(Payload::Text(text)) => {
            out.push(format!("  data: {},", q(text)));
            if json {
                out.extend(lines([
                    "  // Send the text as it is: axios would trim JSON, or quote it when it does not parse.",
                    "  transformRequest: (data) => data,",
                ]));
            }
        }
        Some(Payload::Base64(base64)) => out.push(format!("  data: Buffer.from({}, \"base64\"),", q(&base64))),
        None => {}
    }
    out.extend(lines([
        "  responseType: \"text\",",
        "  validateStatus: () => true,",
        "});",
        "console.log(response.status, response.data);",
    ]));
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
        let (headers, note) = joined(&headers, "A dict holds one value per header", q);
        out.extend(note.map(|note| format!("# {note}")));
        out.push("headers = {".to_string());
        out.extend(headers.iter().map(|(name, value)| format!("    {}: {},", q(name), q(value))));
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

/// HTTPX, Python 3.
fn httpx(req: &HttpRequest) -> Vec<String> {
    let q = json_string;
    let Parts { headers, body, notes } = Parts::new(req, SnippetLanguage::PythonHttpx);
    let mut out = Vec::new();
    if matches!(body, Some(Payload::Base64(_))) {
        out.extend(lines(["import base64", ""]));
    }
    out.extend(lines(["import httpx", ""]));
    out.push(format!("url = {}", q(&req.url)));
    let mut args = String::from("url");
    if grouped(&headers).len() < headers.len() {
        // A dict holds one value per name; a list of pairs keeps them all.
        out.push("headers = [".to_string());
        out.extend(headers.iter().map(|h| format!("    ({}, {}),", q(&h.name), q(&h.value))));
        out.push("]".to_string());
    } else if !headers.is_empty() {
        out.push("headers = {".to_string());
        out.extend(headers.iter().map(|h| format!("    {}: {},", q(&h.name), q(&h.value))));
        out.push("}".to_string());
    }
    if !headers.is_empty() {
        args.push_str(", headers=headers");
    }
    if let Some(body) = body {
        out.push(match body {
            // HTTPX sends `str` content as UTF-8.
            Payload::Text(text) => format!("content = {}", q(text)),
            Payload::Base64(base64) => format!("content = base64.b64decode({})", q(&base64)),
        });
        args.push_str(", content=content");
    }
    out.push(String::new());
    out.extend(comments("#", &notes));
    out.push(format!("response = httpx.request({}, {args})", q(&req.method)));
    out.push("print(response.status_code, response.text)".to_string());
    out
}

/// `net/http`, as a Go program that uses only the standard library.
fn go(req: &HttpRequest) -> Vec<String> {
    let q = go_string;
    let Parts { headers, body, notes } = Parts::new(req, SnippetLanguage::Go);
    let check = ["\tif err != nil {", "\t\tpanic(err)", "\t}"];
    // Go refuses to compile an import that is not used.
    let mut imports = vec!["fmt", "io", "net/http"];
    let mut setup = Vec::new();
    let reader = match body {
        Some(Payload::Text(text)) => {
            imports.push("strings");
            setup.push(format!("\tbody := strings.NewReader({})", q(text)));
            "body"
        }
        Some(Payload::Base64(base64)) => {
            imports.extend(["bytes", "encoding/base64"]);
            setup.push(format!("\tbody, err := base64.StdEncoding.DecodeString({})", q(&base64)));
            setup.extend(lines(check));
            "bytes.NewReader(body)"
        }
        None => "nil",
    };
    imports.sort_unstable();

    let mut out: Vec<String> = lines(["package main", "", "import ("]).collect();
    out.extend(imports.iter().map(|path| format!("\t\"{path}\"")));
    out.extend(lines([")", "", "func main() {"]));
    out.extend(setup);
    out.extend(comments("\t//", &notes));
    out.push(format!("\treq, err := http.NewRequest({}, {}, {reader})", q(&req.method), q(&req.url)));
    out.extend(lines(check));
    // `Add` keeps repeated headers; `Set` would replace them.
    out.extend(headers.iter().map(|h| format!("\treq.Header.Add({}, {})", q(&h.name), q(&h.value))));
    out.extend(lines(["", "\tresp, err := http.DefaultClient.Do(req)"]));
    out.extend(lines(check));
    out.extend(lines(["\tdefer resp.Body.Close()", "\tdata, err := io.ReadAll(resp.Body)"]));
    out.extend(lines(check));
    out.extend(lines(["\tfmt.Println(resp.StatusCode, string(data))", "}"]));
    out
}

/// `java.net.http.HttpClient` (Java 11+), as a single-file program: `java Main.java`.
fn java(req: &HttpRequest) -> Vec<String> {
    let q = java_string;
    let Parts { headers, body, notes } = Parts::new(req, SnippetLanguage::Java);
    let method = req.method.as_str();
    let publisher = body.as_ref().map(|body| match body {
        Payload::Text(text) => format!("BodyPublishers.ofString({})", q(text)),
        Payload::Base64(base64) => {
            format!("BodyPublishers.ofByteArray(Base64.getDecoder().decode({}))", q(base64))
        }
    });
    let call = match (method, publisher) {
        ("GET", None) => ".GET()".to_string(),
        ("DELETE", None) => ".DELETE()".to_string(),
        ("POST" | "PUT", publisher) => {
            format!(".{method}({})", publisher.as_deref().unwrap_or("BodyPublishers.noBody()"))
        }
        (_, publisher) => {
            format!(".method({}, {})", q(method), publisher.as_deref().unwrap_or("BodyPublishers.noBody()"))
        }
    };
    let mut imports = vec!["java.net.URI", "java.net.http.HttpClient", "java.net.http.HttpRequest"];
    if call.contains("BodyPublishers") {
        imports.push("java.net.http.HttpRequest.BodyPublishers");
    }
    imports.extend(["java.net.http.HttpResponse", "java.net.http.HttpResponse.BodyHandlers"]);
    if matches!(body, Some(Payload::Base64(_))) {
        imports.push("java.util.Base64");
    }

    let mut out: Vec<String> = imports.iter().map(|path| format!("import {path};")).collect();
    out.extend(lines([
        "",
        "public class Main {",
        "    public static void main(String[] args) throws Exception {",
        "        HttpClient client = HttpClient.newHttpClient();",
    ]));
    out.extend(comments("        //", &notes));
    out.push("        HttpRequest request = HttpRequest.newBuilder()".to_string());
    out.push(format!("                .uri(URI.create({}))", q(&req.url)));
    out.push(format!("                {call}"));
    out.extend(headers.iter().map(|h| format!("                .header({}, {})", q(&h.name), q(&h.value))));
    out.extend(lines([
        "                .build();",
        "",
        "        HttpResponse<String> response = client.send(request, BodyHandlers.ofString());",
        "        System.out.println(response.statusCode());",
        "        System.out.println(response.body());",
        "    }",
        "}",
    ]));
    out
}

/// Headers `HttpClient` keeps on the body (`HttpContent`) and refuses on the request.
const CONTENT_HEADERS: [&str; 10] = [
    "allow",
    "content-disposition",
    "content-encoding",
    "content-language",
    "content-location",
    "content-md5",
    "content-range",
    "content-type",
    "expires",
    "last-modified",
];

/// `HttpClient`, as a C# program with top-level statements (.NET 6+).
fn csharp(req: &HttpRequest) -> Vec<String> {
    let q = csharp_string;
    let Parts { headers, body, mut notes } = Parts::new(req, SnippetLanguage::CSharp);
    let (content_headers, headers): (Vec<&Header>, Vec<&Header>) =
        headers.into_iter().partition(|h| CONTENT_HEADERS.iter().any(|name| is(h, name)));
    if body.is_none() && !content_headers.is_empty() {
        let names: Vec<&str> = grouped(&content_headers).into_iter().map(|(name, _)| name).collect();
        notes.push(format!("HttpClient sends {} only with a body, so {}.", and_list(&names), left_out(&names)));
    }

    let mut out = Vec::new();
    if matches!(body, Some(Payload::Text(_))) {
        out.extend(lines(["using System.Text;", ""]));
    }
    out.push("using var client = new HttpClient();".to_string());
    out.extend(comments("//", &notes));
    out.push(format!(
        "using var request = new HttpRequestMessage(new HttpMethod({}), {});",
        q(&req.method),
        q(&req.url)
    ));
    out.extend(
        headers.iter().map(|h| format!("request.Headers.TryAddWithoutValidation({}, {});", q(&h.name), q(&h.value))),
    );
    if let Some(body) = body {
        // Bytes, so HttpClient adds no Content-Type or charset of its own.
        let bytes = match body {
            Payload::Text(text) => format!("Encoding.UTF8.GetBytes({})", q(text)),
            Payload::Base64(base64) => format!("Convert.FromBase64String({})", q(&base64)),
        };
        out.push(format!("request.Content = new ByteArrayContent({bytes});"));
        out.extend(
            content_headers
                .iter()
                .map(|h| format!("request.Content.Headers.TryAddWithoutValidation({}, {});", q(&h.name), q(&h.value))),
        );
    }
    out.extend(lines([
        "",
        "using var response = await client.SendAsync(request);",
        "Console.WriteLine((int)response.StatusCode);",
        "Console.WriteLine(await response.Content.ReadAsStringAsync());",
    ]));
    out
}

/// PHP with the curl extension.
fn php(req: &HttpRequest) -> Vec<String> {
    let q = php_string;
    let Parts { headers, body, notes } = Parts::new(req, SnippetLanguage::Php);
    let (custom, nobody) = curl_method(&req.method, body.is_some());
    let mut out: Vec<String> = lines(["<?php", ""]).collect();
    out.extend(comments("//", &notes));
    out.extend(lines(["$curl = curl_init();", "curl_setopt_array($curl, ["]));
    out.push(format!("    CURLOPT_URL => {},", q(&req.url)));
    if custom {
        out.push(format!("    CURLOPT_CUSTOMREQUEST => {},", q(&req.method)));
    }
    if nobody {
        out.push("    CURLOPT_NOBODY => true,".to_string());
    }
    if !headers.is_empty() {
        out.push("    CURLOPT_HTTPHEADER => [".to_string());
        out.extend(headers.iter().map(|h| format!("        {},", q(&curl_header(h)))));
        out.push("    ],".to_string());
    }
    match body {
        Some(Payload::Text(text)) => out.push(format!("    CURLOPT_POSTFIELDS => {},", q(text))),
        Some(Payload::Base64(base64)) => {
            out.push(format!("    CURLOPT_POSTFIELDS => base64_decode({}),", q(&base64)));
        }
        None => {}
    }
    out.extend(lines([
        "    CURLOPT_RETURNTRANSFER => true,",
        "]);",
        "",
        "$response = curl_exec($curl);",
        "if ($response === false) {",
        "    exit(curl_error($curl) . \"\\n\");",
        "}",
        "echo curl_getinfo($curl, CURLINFO_RESPONSE_CODE), \"\\n\";",
        "echo $response, \"\\n\";",
    ]));
    out
}

/// `Net::HTTP`, from Ruby's standard library.
fn ruby(req: &HttpRequest) -> Vec<String> {
    let q = ruby_string;
    let Parts { headers, body, notes } = Parts::new(req, SnippetLanguage::Ruby);
    let mut out: Vec<String> = lines(["require \"net/http\"", ""]).collect();
    out.push(format!("uri = URI({})", q(&req.url)));
    out.extend(comments("#", &notes));
    // A generic request takes any method: (method, has a body, has a response body, path).
    out.push(format!(
        "request = Net::HTTPGenericRequest.new({}, {}, {}, uri.request_uri)",
        q(&req.method),
        body.is_some(),
        req.method != "HEAD"
    ));
    // `[]=` replaces the Accept and Accept-Encoding that Net::HTTP sets; `add_field` would append
    // to them. Net::HTTP joins repeated headers with commas, which would be wrong for Cookie.
    let (headers, note) = joined(&headers, "Net::HTTP sends one line per header name", q);
    out.extend(note.map(|note| format!("# {note}")));
    out.extend(headers.iter().map(|(name, value)| format!("request[{}] = {}", q(name), q(value))));
    match body {
        Some(Payload::Text(text)) => out.push(format!("request.body = {}", q(text))),
        Some(Payload::Base64(base64)) => out.push(format!("request.body = {}.unpack1(\"m\")", q(&base64))),
        None => {}
    }
    out.extend(lines([
        "",
        "response = Net::HTTP.start(uri.host, uri.port, use_ssl: uri.scheme == \"https\") do |http|",
        "  http.request(request)",
        "end",
        "puts response.code",
        "puts response.body",
    ]));
    out
}

/// `reqwest` with Tokio (the `macros` and `rt-multi-thread` features).
fn rust(req: &HttpRequest) -> Vec<String> {
    let q = rust_string;
    let Parts { headers, body, notes } = Parts::new(req, SnippetLanguage::Rust);
    let method = req.method.as_str();
    let call = match method {
        "GET" | "POST" | "PUT" | "PATCH" | "DELETE" | "HEAD" => {
            format!(".{}({})", method.to_ascii_lowercase(), q(&req.url))
        }
        _ => format!(".request(reqwest::Method::from_bytes({})?, {})", rust_bytes(method.as_bytes()), q(&req.url)),
    };
    let mut out: Vec<String> =
        lines(["#[tokio::main]", "async fn main() -> Result<(), Box<dyn std::error::Error>> {"]).collect();
    out.extend(comments("    //", &notes));
    out.push("    let response = reqwest::Client::new()".to_string());
    out.push(format!("        {call}"));
    // `header` appends, so repeated headers are kept.
    out.extend(headers.iter().map(|h| format!("        .header({}, {})", q(&h.name), q(&h.value))));
    match body {
        Some(Payload::Text(text)) => out.push(format!("        .body({})", q(text))),
        Some(Payload::Base64(_)) => out.push(format!("        .body({}.to_vec())", rust_bytes(&req.body))),
        None => {}
    }
    out.extend(lines([
        "        .send()",
        "        .await?;",
        "    println!(\"{}\", response.status().as_u16());",
        "    println!(\"{}\", response.text().await?);",
        "    Ok(())",
        "}",
    ]));
    out
}

/// `package:http` (Dart, Flutter).
fn dart(req: &HttpRequest) -> Vec<String> {
    let q = dart_string;
    let Parts { headers, body, notes } = Parts::new(req, SnippetLanguage::Dart);
    let mut out = Vec::new();
    if body.is_some() {
        out.extend(lines(["import 'dart:convert';", ""]));
    }
    out.extend(lines(["import 'package:http/http.dart' as http;", "", "Future<void> main() async {"]));
    out.extend(comments("  //", &notes));
    out.push(format!("  final request = http.Request({}, Uri.parse({}));", q(&req.method), q(&req.url)));
    let (headers, note) = joined(&headers, "A Map holds one value per header", q);
    out.extend(note.map(|note| format!("  // {note}")));
    out.extend(headers.iter().map(|(name, value)| format!("  request.headers[{}] = {};", q(name), q(value))));
    // `body` would add a charset to Content-Type; `bodyBytes` sends the bytes as they are.
    match body {
        Some(Payload::Text(text)) => out.push(format!("  request.bodyBytes = utf8.encode({});", q(text))),
        Some(Payload::Base64(base64)) => out.push(format!("  request.bodyBytes = base64Decode({});", q(&base64))),
        None => {}
    }
    out.extend(lines([
        "",
        "  final response = await request.send();",
        "  print(response.statusCode);",
        "  print(await response.stream.bytesToString());",
        "}",
    ]));
    out
}

/// libcurl's easy interface, in C.
fn c(req: &HttpRequest) -> Vec<String> {
    let q = c_string;
    let Parts { headers, body, notes } = Parts::new(req, SnippetLanguage::C);
    let (custom, nobody) = curl_method(&req.method, body.is_some());
    let mut out: Vec<String> = lines([
        "#include <stdio.h>",
        "#include <curl/curl.h>",
        "",
        "int main(void) {",
        "    CURL *curl = curl_easy_init();",
        "    if (!curl) {",
        "        return 1;",
        "    }",
    ])
    .collect();
    out.extend(comments("    //", &notes));
    match &body {
        Some(Payload::Text(text)) => {
            // One line of code per line of text; the compiler joins the pieces.
            let pieces: Vec<&str> = text.split_inclusive('\n').collect();
            if let [piece] = pieces[..] {
                out.push(format!("    static const char body[] = {};", q(piece)));
            } else {
                out.push("    static const char body[] =".to_string());
                let last = pieces.len() - 1;
                out.extend(
                    pieces
                        .iter()
                        .enumerate()
                        .map(|(i, piece)| format!("        {}{}", q(piece), if i == last { ";" } else { "" })),
                );
            }
        }
        Some(Payload::Base64(_)) => out.push(format!("    static const char body[] = {};", c_bytes(&req.body))),
        None => {}
    }
    if !headers.is_empty() {
        out.push("    struct curl_slist *headers = NULL;".to_string());
        out.extend(
            headers.iter().map(|h| format!("    headers = curl_slist_append(headers, {});", q(&curl_header(h)))),
        );
    }
    out.push(format!("    curl_easy_setopt(curl, CURLOPT_URL, {});", q(&req.url)));
    if custom {
        out.push(format!("    curl_easy_setopt(curl, CURLOPT_CUSTOMREQUEST, {});", q(&req.method)));
    }
    if nobody {
        out.push("    curl_easy_setopt(curl, CURLOPT_NOBODY, 1L);".to_string());
    }
    if !headers.is_empty() {
        out.push("    curl_easy_setopt(curl, CURLOPT_HTTPHEADER, headers);".to_string());
    }
    if body.is_some() {
        // With the size, the body can hold NUL bytes.
        out.extend(lines([
            "    curl_easy_setopt(curl, CURLOPT_POSTFIELDSIZE, (long)(sizeof body - 1));",
            "    curl_easy_setopt(curl, CURLOPT_POSTFIELDS, body);",
        ]));
    }
    out.extend(lines([
        "",
        "    // libcurl writes the response body to standard output.",
        "    CURLcode result = curl_easy_perform(curl);",
        "    if (result == CURLE_OK) {",
        "        long status = 0;",
        "        curl_easy_getinfo(curl, CURLINFO_RESPONSE_CODE, &status);",
        "        printf(\"\\n%ld\\n\", status);",
        "    } else {",
        "        fprintf(stderr, \"%s\\n\", curl_easy_strerror(result));",
        "    }",
    ]));
    if !headers.is_empty() {
        out.push("    curl_slist_free_all(headers);".to_string());
    }
    out.extend(lines(["    curl_easy_cleanup(curl);", "    return result == CURLE_OK ? 0 : 1;", "}"]));
    out
}

/// PowerShell 7 `Invoke-WebRequest`, with its parameters splatted from a hashtable.
fn powershell(req: &HttpRequest) -> Vec<String> {
    let q = powershell_string;
    let Parts { mut headers, body, notes } = Parts::new(req, SnippetLanguage::PowerShell);
    // Content-Type is a parameter of its own.
    let content_type: Vec<&str> = headers.iter().filter(|h| is(h, "content-type")).map(|h| h.value.as_str()).collect();
    headers.retain(|h| !is(h, "content-type"));
    let method = match req.method.as_str() {
        "GET" => Some("Get"),
        "HEAD" => Some("Head"),
        "POST" => Some("Post"),
        "PUT" => Some("Put"),
        "DELETE" => Some("Delete"),
        "TRACE" => Some("Trace"),
        "OPTIONS" => Some("Options"),
        "MERGE" => Some("Merge"),
        "PATCH" => Some("Patch"),
        _ => None,
    };

    let mut out: Vec<String> = comments("#", &notes).collect();
    out.push("$params = @{".to_string());
    out.push(format!("    Uri = {}", q(&req.url)));
    out.push(match method {
        Some(method) => format!("    Method = '{method}'"),
        None => format!("    CustomMethod = {}", q(&req.method)),
    });
    if !headers.is_empty() {
        let (headers, note) = joined(&headers, "A hashtable holds one value per header", q);
        out.push("    Headers = @{".to_string());
        out.extend(note.map(|note| format!("        # {note}")));
        out.extend(headers.iter().map(|(name, value)| format!("        {} = {}", q(name), q(value))));
        out.push("    }".to_string());
    }
    if !content_type.is_empty() {
        out.push(format!("    ContentType = {}", q(&content_type.join(", "))));
    }
    match body {
        Some(Payload::Text(text)) if text.is_ascii() => out.push(format!("    Body = {}", q(text))),
        // Older versions send a string body as Latin-1 unless Content-Type names a charset.
        Some(Payload::Text(text)) => out.push(format!("    Body = [System.Text.Encoding]::UTF8.GetBytes({})", q(text))),
        Some(Payload::Base64(base64)) => out.push(format!("    Body = [Convert]::FromBase64String({})", q(&base64))),
        None => {}
    }
    out.extend(lines([
        "    SkipHttpErrorCheck = $true",
        "    SkipHeaderValidation = $true",
        "}",
        "$response = Invoke-WebRequest @params",
        "$response.StatusCode",
        "$response.Content",
    ]));
    out
}

/// HTTPie 3, as a bash command line.
fn httpie(req: &HttpRequest) -> Vec<String> {
    let Parts { headers, body, notes } = Parts::new(req, SnippetLanguage::Httpie);
    let mut args = vec![format!("{} {}", shell_word(&req.method), shell(&req.url))];
    args.extend(headers.iter().map(|h| {
        shell(&match h.value.as_str() {
            // `Name:` without a value would remove the header.
            value if value.trim().is_empty() => format!("{};", h.name),
            // `Name:=value` would be a JSON field.
            value if value.starts_with('=') => format!("{}:\\{value}", h.name),
            value => format!("{}:{value}", h.name),
        })
    }));
    // HTTPie reads a body from standard input, and waits for one unless told not to.
    let command = match shell_payload(body) {
        Some(Payload::Base64(base64)) => format!("printf '%s' {} | base64 -d | http", shell(&base64)),
        Some(Payload::Text(text)) => {
            args.push(format!("--raw {}", shell(text)));
            "http --ignore-stdin".to_string()
        }
        None => "http --ignore-stdin".to_string(),
    };
    let mut out: Vec<String> = comments("#", &notes).collect();
    out.push(format!("{command} {}", args.join(" \\\n  ")));
    out
}

/// Wget, as a bash command line.
fn wget(req: &HttpRequest) -> Vec<String> {
    let Parts { headers, body, notes } = Parts::new(req, SnippetLanguage::Wget);
    let mut out: Vec<String> = comments("#", &notes).collect();
    let mut args = vec![
        "--quiet --server-response --content-on-error --output-document=-".to_string(),
        format!("--method={}", shell_word(&req.method)),
    ];
    args.extend(headers.iter().map(|h| format!("--header={}", shell(&format!("{}: {}", h.name, h.value)))));
    let file = match shell_payload(body) {
        Some(Payload::Text(text)) => {
            args.push(format!("--body-data={}", shell(text)));
            false
        }
        // Wget needs the size of the body up front, so it cannot read one from a pipe.
        Some(Payload::Base64(base64)) => {
            out.push("body=$(mktemp)".to_string());
            out.push(format!("printf '%s' {} | base64 -d > \"$body\"", shell(&base64)));
            args.push("--body-file=\"$body\"".to_string());
            true
        }
        None => false,
    };
    args.push(shell(&req.url));
    out.push(format!("wget {}", args.join(" \\\n  ")));
    if file {
        out.push("rm \"$body\"".to_string());
    }
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

/// A Go string literal. Go rejects NUL and byte order marks in source files, so they are escaped
/// like the control characters.
fn go_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_ascii_control() => {
                let _ = write!(out, "\\x{:02x}", u32::from(c));
            }
            // `\x` is a byte, so the characters after ASCII take `\u`.
            c if c.is_control() || c == '\u{feff}' => {
                let _ = write!(out, "\\u{:04x}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A Java string literal. Java turns `\u` escapes into characters before it reads string literals
/// (so `\u000a` would end the line), and control characters take octal escapes instead.
fn java_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            // Three digits, so a digit after the escape cannot join it. Control characters end at \237.
            c if c.is_control() => {
                let _ = write!(out, "\\{:03o}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A C# string literal. `\x` takes up to four hex digits, so control characters use `\u`; U+2028
/// and U+2029 end a line in C#, so they are escaped too.
fn csharp_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\0' => out.push_str("\\0"),
            '\u{7}' => out.push_str("\\a"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{b}' => out.push_str("\\v"),
            c if c.is_control() || c == '\u{2028}' || c == '\u{2029}' => {
                let _ = write!(out, "\\u{:04x}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A double-quoted PHP string. `$` would start an interpolation.
fn php_string(s: &str) -> String {
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
            '\u{b}' => out.push_str("\\v"),
            '\u{c}' => out.push_str("\\f"),
            '\u{1b}' => out.push_str("\\e"),
            c if c.is_ascii_control() => {
                let _ = write!(out, "\\x{:02x}", u32::from(c));
            }
            // `\x` is a byte, so the characters after ASCII take `\u`.
            c if c.is_control() => {
                let _ = write!(out, "\\u{{{:x}}}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A double-quoted Ruby string. `#` is escaped too, so `#{`, `#@` and `#$` cannot interpolate.
fn ruby_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '#' => out.push_str("\\#"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{1b}' => out.push_str("\\e"),
            c if c.is_control() => {
                let _ = write!(out, "\\u{{{:x}}}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A Rust string literal. The compiler rejects characters that change the direction of text in
/// literals, so they are escaped like the control characters.
fn rust_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\0' => out.push_str("\\0"),
            c if c.is_control() || matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}') => {
                let _ = write!(out, "\\u{{{:x}}}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A Rust byte string literal.
fn rust_bytes(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() + 3);
    out.push_str("b\"");
    for &b in bytes {
        match b {
            b'\\' => out.push_str("\\\\"),
            b'"' => out.push_str("\\\""),
            b'\n' => out.push_str("\\n"),
            b'\r' => out.push_str("\\r"),
            b'\t' => out.push_str("\\t"),
            b'\0' => out.push_str("\\0"),
            b' '..=b'~' => out.push(char::from(b)),
            _ => {
                let _ = write!(out, "\\x{b:02x}");
            }
        }
    }
    out.push('"');
    out
}

/// A single-quoted Dart string. `$` would start an interpolation.
fn dart_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\'' => out.push_str("\\'"),
            '$' => out.push_str("\\$"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => {
                let _ = write!(out, "\\u{{{:x}}}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out.push('\'');
    out
}

/// A C string literal. A `?` after another is escaped so that `??` cannot start a trigraph, and
/// control characters take octal escapes of three digits, so a digit after one cannot join it.
fn c_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '?' if out.ends_with('?') => out.push_str("\\?"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => {
                for b in c.encode_utf8(&mut [0; 4]).bytes() {
                    let _ = write!(out, "\\{b:03o}");
                }
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A C string literal of any bytes.
fn c_bytes(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() + 2);
    out.push('"');
    for &b in bytes {
        match b {
            b'\\' => out.push_str("\\\\"),
            b'"' => out.push_str("\\\""),
            b'?' if out.ends_with('?') => out.push_str("\\?"),
            b'\n' => out.push_str("\\n"),
            b'\r' => out.push_str("\\r"),
            b'\t' => out.push_str("\\t"),
            b' '..=b'~' => out.push(char::from(b)),
            _ => {
                let _ = write!(out, "\\{b:03o}");
            }
        }
    }
    out.push('"');
    out
}

/// A PowerShell string: single-quoted (lines and all) when it has no other control characters,
/// else double-quoted with backtick escapes. Typographic quotes also end a string, so they are
/// escaped like `'` and `"`.
fn powershell_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    if !s.chars().any(|c| c.is_control() && c != '\n') {
        out.push('\'');
        for c in s.chars() {
            if PS_SINGLE_QUOTES.contains(&c) {
                out.push(c);
            }
            out.push(c);
        }
        out.push('\'');
        return out;
    }
    out.push('"');
    for c in s.chars() {
        match c {
            '`' | '$' | '"' | '\u{201c}' | '\u{201d}' | '\u{201e}' => {
                out.push('`');
                out.push(c);
            }
            '\0' => out.push_str("`0"),
            '\u{7}' => out.push_str("`a"),
            '\u{8}' => out.push_str("`b"),
            '\u{c}' => out.push_str("`f"),
            '\n' => out.push_str("`n"),
            '\r' => out.push_str("`r"),
            '\t' => out.push_str("`t"),
            '\u{b}' => out.push_str("`v"),
            '\u{1b}' => out.push_str("`e"),
            c if c.is_control() => {
                let _ = write!(out, "`u{{{:x}}}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A bash word: single-quoted, which keeps newlines readable, or ANSI-C quoted (`$'...'`) when
/// it holds other control characters.
fn shell(s: &str) -> String {
    if s.chars().any(|c| c.is_control() && c != '\n') { ansi_c_quote(s.as_bytes()) } else { quote_bash(s) }
}

/// A bash word, bare when it needs no quotes (a method such as `POST`).
fn shell_word(s: &str) -> String {
    let bare = !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-');
    if bare { s.to_string() } else { shell(s) }
}

#[cfg(test)]
mod tests {
    use super::SnippetLanguage::{
        C, CSharp, Dart, Go, Httpie, Java, JavaScript, JavaScriptAxios, Kotlin, Php, PowerShell, Python, PythonHttpx,
        Ruby, Rust, Swift, Wget,
    };
    use super::*;

    const ALL: [SnippetLanguage; 17] = [
        Kotlin,
        Swift,
        JavaScript,
        JavaScriptAxios,
        Python,
        PythonHttpx,
        Go,
        Java,
        CSharp,
        Php,
        Ruby,
        Rust,
        Dart,
        C,
        PowerShell,
        Httpie,
        Wget,
    ];

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
        let names = r#"["kotlin","swift","javascript","javascriptAxios","python","pythonHttpx","go","java","csharp","php","ruby","rust","dart","c","powerShell","httpie","wget"]"#;
        assert_eq!(serde_json::to_string(&ALL).unwrap(), names);
        let parsed: Vec<SnippetLanguage> = serde_json::from_str(names).unwrap();
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
    fn snippet_get_axios() {
        assert_eq!(
            to_snippet(&get_request(), JavaScriptAxios),
            r#"import axios from "axios";

const response = await axios({
  method: "GET",
  url: "https://api.example.com/users?page=2&sort=name",
  headers: {
    "Accept": "application/json",
    "Authorization": "Bearer abc123",
  },
  responseType: "text",
  validateStatus: () => true,
});
console.log(response.status, response.data);
"#
        );
    }

    #[test]
    fn snippet_post_json_axios() {
        assert_eq!(
            to_snippet(&post_request(), JavaScriptAxios),
            r#"import axios from "axios";

const response = await axios({
  method: "POST",
  url: "https://api.example.com/items",
  headers: {
    "Content-Type": "application/json",
    "Authorization": "Bearer abc123",
  },
  data: "{\n  \"say\": \"hi \\\"there\\\"\",\n  \"path\": \"C:\\\\tmp\",\n  \"price\": \"$5 ${total}\",\n  \"swift\": \"\\(name)\",\n  \"unicode\": \"héllo 世界 🎉\"\n}",
  // Send the text as it is: axios would trim JSON, or quote it when it does not parse.
  transformRequest: (data) => data,
  responseType: "text",
  validateStatus: () => true,
});
console.log(response.status, response.data);
"#
        );
    }

    #[test]
    fn snippet_get_httpx() {
        assert_eq!(
            to_snippet(&get_request(), PythonHttpx),
            r#"import httpx

url = "https://api.example.com/users?page=2&sort=name"
headers = {
    "Accept": "application/json",
    "Authorization": "Bearer abc123",
}

response = httpx.request("GET", url, headers=headers)
print(response.status_code, response.text)
"#
        );
    }

    #[test]
    fn snippet_post_json_httpx() {
        assert_eq!(
            to_snippet(&post_request(), PythonHttpx),
            r#"import httpx

url = "https://api.example.com/items"
headers = {
    "Content-Type": "application/json",
    "Authorization": "Bearer abc123",
}
content = "{\n  \"say\": \"hi \\\"there\\\"\",\n  \"path\": \"C:\\\\tmp\",\n  \"price\": \"$5 ${total}\",\n  \"swift\": \"\\(name)\",\n  \"unicode\": \"héllo 世界 🎉\"\n}"

response = httpx.request("POST", url, headers=headers, content=content)
print(response.status_code, response.text)
"#
        );
    }

    #[test]
    fn snippet_get_go() {
        assert_eq!(
            to_snippet(&get_request(), Go).replace('\t', "    "),
            r#"package main

import (
    "fmt"
    "io"
    "net/http"
)

func main() {
    req, err := http.NewRequest("GET", "https://api.example.com/users?page=2&sort=name", nil)
    if err != nil {
        panic(err)
    }
    req.Header.Add("Accept", "application/json")
    req.Header.Add("Authorization", "Bearer abc123")

    resp, err := http.DefaultClient.Do(req)
    if err != nil {
        panic(err)
    }
    defer resp.Body.Close()
    data, err := io.ReadAll(resp.Body)
    if err != nil {
        panic(err)
    }
    fmt.Println(resp.StatusCode, string(data))
}
"#
        );
    }

    #[test]
    fn snippet_post_json_go() {
        assert_eq!(
            to_snippet(&post_request(), Go).replace('\t', "    "),
            r#"package main

import (
    "fmt"
    "io"
    "net/http"
    "strings"
)

func main() {
    body := strings.NewReader("{\n  \"say\": \"hi \\\"there\\\"\",\n  \"path\": \"C:\\\\tmp\",\n  \"price\": \"$5 ${total}\",\n  \"swift\": \"\\(name)\",\n  \"unicode\": \"héllo 世界 🎉\"\n}")
    req, err := http.NewRequest("POST", "https://api.example.com/items", body)
    if err != nil {
        panic(err)
    }
    req.Header.Add("Content-Type", "application/json")
    req.Header.Add("Authorization", "Bearer abc123")

    resp, err := http.DefaultClient.Do(req)
    if err != nil {
        panic(err)
    }
    defer resp.Body.Close()
    data, err := io.ReadAll(resp.Body)
    if err != nil {
        panic(err)
    }
    fmt.Println(resp.StatusCode, string(data))
}
"#
        );
    }

    #[test]
    fn snippet_get_java() {
        assert_eq!(
            to_snippet(&get_request(), Java),
            r#"import java.net.URI;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.net.http.HttpResponse.BodyHandlers;

public class Main {
    public static void main(String[] args) throws Exception {
        HttpClient client = HttpClient.newHttpClient();
        HttpRequest request = HttpRequest.newBuilder()
                .uri(URI.create("https://api.example.com/users?page=2&sort=name"))
                .GET()
                .header("Accept", "application/json")
                .header("Authorization", "Bearer abc123")
                .build();

        HttpResponse<String> response = client.send(request, BodyHandlers.ofString());
        System.out.println(response.statusCode());
        System.out.println(response.body());
    }
}
"#
        );
    }

    #[test]
    fn snippet_post_json_java() {
        assert_eq!(
            to_snippet(&post_request(), Java),
            r#"import java.net.URI;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpRequest.BodyPublishers;
import java.net.http.HttpResponse;
import java.net.http.HttpResponse.BodyHandlers;

public class Main {
    public static void main(String[] args) throws Exception {
        HttpClient client = HttpClient.newHttpClient();
        HttpRequest request = HttpRequest.newBuilder()
                .uri(URI.create("https://api.example.com/items"))
                .POST(BodyPublishers.ofString("{\n  \"say\": \"hi \\\"there\\\"\",\n  \"path\": \"C:\\\\tmp\",\n  \"price\": \"$5 ${total}\",\n  \"swift\": \"\\(name)\",\n  \"unicode\": \"héllo 世界 🎉\"\n}"))
                .header("Content-Type", "application/json")
                .header("Authorization", "Bearer abc123")
                .build();

        HttpResponse<String> response = client.send(request, BodyHandlers.ofString());
        System.out.println(response.statusCode());
        System.out.println(response.body());
    }
}
"#
        );
    }

    #[test]
    fn snippet_get_csharp() {
        assert_eq!(
            to_snippet(&get_request(), CSharp),
            r#"using var client = new HttpClient();
using var request = new HttpRequestMessage(new HttpMethod("GET"), "https://api.example.com/users?page=2&sort=name");
request.Headers.TryAddWithoutValidation("Accept", "application/json");
request.Headers.TryAddWithoutValidation("Authorization", "Bearer abc123");

using var response = await client.SendAsync(request);
Console.WriteLine((int)response.StatusCode);
Console.WriteLine(await response.Content.ReadAsStringAsync());
"#
        );
    }

    #[test]
    fn snippet_post_json_csharp() {
        assert_eq!(
            to_snippet(&post_request(), CSharp),
            r#"using System.Text;

using var client = new HttpClient();
using var request = new HttpRequestMessage(new HttpMethod("POST"), "https://api.example.com/items");
request.Headers.TryAddWithoutValidation("Authorization", "Bearer abc123");
request.Content = new ByteArrayContent(Encoding.UTF8.GetBytes("{\n  \"say\": \"hi \\\"there\\\"\",\n  \"path\": \"C:\\\\tmp\",\n  \"price\": \"$5 ${total}\",\n  \"swift\": \"\\(name)\",\n  \"unicode\": \"héllo 世界 🎉\"\n}"));
request.Content.Headers.TryAddWithoutValidation("Content-Type", "application/json");

using var response = await client.SendAsync(request);
Console.WriteLine((int)response.StatusCode);
Console.WriteLine(await response.Content.ReadAsStringAsync());
"#
        );
    }

    #[test]
    fn snippet_get_php() {
        assert_eq!(
            to_snippet(&get_request(), Php),
            r#"<?php

$curl = curl_init();
curl_setopt_array($curl, [
    CURLOPT_URL => "https://api.example.com/users?page=2&sort=name",
    CURLOPT_HTTPHEADER => [
        "Accept: application/json",
        "Authorization: Bearer abc123",
    ],
    CURLOPT_RETURNTRANSFER => true,
]);

$response = curl_exec($curl);
if ($response === false) {
    exit(curl_error($curl) . "\n");
}
echo curl_getinfo($curl, CURLINFO_RESPONSE_CODE), "\n";
echo $response, "\n";
"#
        );
    }

    #[test]
    fn snippet_post_json_php() {
        assert_eq!(
            to_snippet(&post_request(), Php),
            r#"<?php

$curl = curl_init();
curl_setopt_array($curl, [
    CURLOPT_URL => "https://api.example.com/items",
    CURLOPT_CUSTOMREQUEST => "POST",
    CURLOPT_HTTPHEADER => [
        "Content-Type: application/json",
        "Authorization: Bearer abc123",
    ],
    CURLOPT_POSTFIELDS => "{\n  \"say\": \"hi \\\"there\\\"\",\n  \"path\": \"C:\\\\tmp\",\n  \"price\": \"\$5 \${total}\",\n  \"swift\": \"\\(name)\",\n  \"unicode\": \"héllo 世界 🎉\"\n}",
    CURLOPT_RETURNTRANSFER => true,
]);

$response = curl_exec($curl);
if ($response === false) {
    exit(curl_error($curl) . "\n");
}
echo curl_getinfo($curl, CURLINFO_RESPONSE_CODE), "\n";
echo $response, "\n";
"#
        );
    }

    #[test]
    fn snippet_get_ruby() {
        assert_eq!(
            to_snippet(&get_request(), Ruby),
            r#"require "net/http"

uri = URI("https://api.example.com/users?page=2&sort=name")
request = Net::HTTPGenericRequest.new("GET", false, true, uri.request_uri)
request["Accept"] = "application/json"
request["Authorization"] = "Bearer abc123"

response = Net::HTTP.start(uri.host, uri.port, use_ssl: uri.scheme == "https") do |http|
  http.request(request)
end
puts response.code
puts response.body
"#
        );
    }

    #[test]
    fn snippet_post_json_ruby() {
        assert_eq!(
            to_snippet(&post_request(), Ruby),
            r#"require "net/http"

uri = URI("https://api.example.com/items")
request = Net::HTTPGenericRequest.new("POST", true, true, uri.request_uri)
request["Content-Type"] = "application/json"
request["Authorization"] = "Bearer abc123"
request.body = "{\n  \"say\": \"hi \\\"there\\\"\",\n  \"path\": \"C:\\\\tmp\",\n  \"price\": \"$5 ${total}\",\n  \"swift\": \"\\(name)\",\n  \"unicode\": \"héllo 世界 🎉\"\n}"

response = Net::HTTP.start(uri.host, uri.port, use_ssl: uri.scheme == "https") do |http|
  http.request(request)
end
puts response.code
puts response.body
"#
        );
    }

    #[test]
    fn snippet_get_rust() {
        assert_eq!(
            to_snippet(&get_request(), Rust),
            r#"#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let response = reqwest::Client::new()
        .get("https://api.example.com/users?page=2&sort=name")
        .header("Accept", "application/json")
        .header("Authorization", "Bearer abc123")
        .send()
        .await?;
    println!("{}", response.status().as_u16());
    println!("{}", response.text().await?);
    Ok(())
}
"#
        );
    }

    #[test]
    fn snippet_post_json_rust() {
        assert_eq!(
            to_snippet(&post_request(), Rust),
            r#"#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let response = reqwest::Client::new()
        .post("https://api.example.com/items")
        .header("Content-Type", "application/json")
        .header("Authorization", "Bearer abc123")
        .body("{\n  \"say\": \"hi \\\"there\\\"\",\n  \"path\": \"C:\\\\tmp\",\n  \"price\": \"$5 ${total}\",\n  \"swift\": \"\\(name)\",\n  \"unicode\": \"héllo 世界 🎉\"\n}")
        .send()
        .await?;
    println!("{}", response.status().as_u16());
    println!("{}", response.text().await?);
    Ok(())
}
"#
        );
    }

    #[test]
    fn snippet_get_dart() {
        assert_eq!(
            to_snippet(&get_request(), Dart),
            r#"import 'package:http/http.dart' as http;

Future<void> main() async {
  final request = http.Request('GET', Uri.parse('https://api.example.com/users?page=2&sort=name'));
  request.headers['Accept'] = 'application/json';
  request.headers['Authorization'] = 'Bearer abc123';

  final response = await request.send();
  print(response.statusCode);
  print(await response.stream.bytesToString());
}
"#
        );
    }

    #[test]
    fn snippet_post_json_dart() {
        assert_eq!(
            to_snippet(&post_request(), Dart),
            r#"import 'dart:convert';

import 'package:http/http.dart' as http;

Future<void> main() async {
  final request = http.Request('POST', Uri.parse('https://api.example.com/items'));
  request.headers['Content-Type'] = 'application/json';
  request.headers['Authorization'] = 'Bearer abc123';
  request.bodyBytes = utf8.encode('{\n  "say": "hi \\"there\\"",\n  "path": "C:\\\\tmp",\n  "price": "\$5 \${total}",\n  "swift": "\\(name)",\n  "unicode": "héllo 世界 🎉"\n}');

  final response = await request.send();
  print(response.statusCode);
  print(await response.stream.bytesToString());
}
"#
        );
    }

    #[test]
    fn snippet_get_c() {
        assert_eq!(
            to_snippet(&get_request(), C),
            r#"#include <stdio.h>
#include <curl/curl.h>

int main(void) {
    CURL *curl = curl_easy_init();
    if (!curl) {
        return 1;
    }
    struct curl_slist *headers = NULL;
    headers = curl_slist_append(headers, "Accept: application/json");
    headers = curl_slist_append(headers, "Authorization: Bearer abc123");
    curl_easy_setopt(curl, CURLOPT_URL, "https://api.example.com/users?page=2&sort=name");
    curl_easy_setopt(curl, CURLOPT_HTTPHEADER, headers);

    // libcurl writes the response body to standard output.
    CURLcode result = curl_easy_perform(curl);
    if (result == CURLE_OK) {
        long status = 0;
        curl_easy_getinfo(curl, CURLINFO_RESPONSE_CODE, &status);
        printf("\n%ld\n", status);
    } else {
        fprintf(stderr, "%s\n", curl_easy_strerror(result));
    }
    curl_slist_free_all(headers);
    curl_easy_cleanup(curl);
    return result == CURLE_OK ? 0 : 1;
}
"#
        );
    }

    #[test]
    fn snippet_post_json_c() {
        assert_eq!(
            to_snippet(&post_request(), C),
            r#"#include <stdio.h>
#include <curl/curl.h>

int main(void) {
    CURL *curl = curl_easy_init();
    if (!curl) {
        return 1;
    }
    static const char body[] =
        "{\n"
        "  \"say\": \"hi \\\"there\\\"\",\n"
        "  \"path\": \"C:\\\\tmp\",\n"
        "  \"price\": \"$5 ${total}\",\n"
        "  \"swift\": \"\\(name)\",\n"
        "  \"unicode\": \"héllo 世界 🎉\"\n"
        "}";
    struct curl_slist *headers = NULL;
    headers = curl_slist_append(headers, "Content-Type: application/json");
    headers = curl_slist_append(headers, "Authorization: Bearer abc123");
    curl_easy_setopt(curl, CURLOPT_URL, "https://api.example.com/items");
    curl_easy_setopt(curl, CURLOPT_CUSTOMREQUEST, "POST");
    curl_easy_setopt(curl, CURLOPT_HTTPHEADER, headers);
    curl_easy_setopt(curl, CURLOPT_POSTFIELDSIZE, (long)(sizeof body - 1));
    curl_easy_setopt(curl, CURLOPT_POSTFIELDS, body);

    // libcurl writes the response body to standard output.
    CURLcode result = curl_easy_perform(curl);
    if (result == CURLE_OK) {
        long status = 0;
        curl_easy_getinfo(curl, CURLINFO_RESPONSE_CODE, &status);
        printf("\n%ld\n", status);
    } else {
        fprintf(stderr, "%s\n", curl_easy_strerror(result));
    }
    curl_slist_free_all(headers);
    curl_easy_cleanup(curl);
    return result == CURLE_OK ? 0 : 1;
}
"#
        );
    }

    #[test]
    fn snippet_get_powershell() {
        assert_eq!(
            to_snippet(&get_request(), PowerShell),
            r#"$params = @{
    Uri = 'https://api.example.com/users?page=2&sort=name'
    Method = 'Get'
    Headers = @{
        'Accept' = 'application/json'
        'Authorization' = 'Bearer abc123'
    }
    SkipHttpErrorCheck = $true
    SkipHeaderValidation = $true
}
$response = Invoke-WebRequest @params
$response.StatusCode
$response.Content
"#
        );
    }

    #[test]
    fn snippet_post_json_powershell() {
        assert_eq!(
            to_snippet(&post_request(), PowerShell),
            r#"$params = @{
    Uri = 'https://api.example.com/items'
    Method = 'Post'
    Headers = @{
        'Authorization' = 'Bearer abc123'
    }
    ContentType = 'application/json'
    Body = [System.Text.Encoding]::UTF8.GetBytes('{
  "say": "hi \"there\"",
  "path": "C:\\tmp",
  "price": "$5 ${total}",
  "swift": "\(name)",
  "unicode": "héllo 世界 🎉"
}')
    SkipHttpErrorCheck = $true
    SkipHeaderValidation = $true
}
$response = Invoke-WebRequest @params
$response.StatusCode
$response.Content
"#
        );
    }

    #[test]
    fn snippet_get_httpie() {
        assert_eq!(
            to_snippet(&get_request(), Httpie),
            r#"http --ignore-stdin GET 'https://api.example.com/users?page=2&sort=name' \
  'Accept:application/json' \
  'Authorization:Bearer abc123'
"#
        );
    }

    #[test]
    fn snippet_post_json_httpie() {
        assert_eq!(
            to_snippet(&post_request(), Httpie),
            r#"http --ignore-stdin POST 'https://api.example.com/items' \
  'Content-Type:application/json' \
  'Authorization:Bearer abc123' \
  --raw '{
  "say": "hi \"there\"",
  "path": "C:\\tmp",
  "price": "$5 ${total}",
  "swift": "\(name)",
  "unicode": "héllo 世界 🎉"
}'
"#
        );
    }

    #[test]
    fn snippet_get_wget() {
        assert_eq!(
            to_snippet(&get_request(), Wget),
            r#"wget --quiet --server-response --content-on-error --output-document=- \
  --method=GET \
  --header='Accept: application/json' \
  --header='Authorization: Bearer abc123' \
  'https://api.example.com/users?page=2&sort=name'
"#
        );
    }

    #[test]
    fn snippet_post_json_wget() {
        assert_eq!(
            to_snippet(&post_request(), Wget),
            r#"wget --quiet --server-response --content-on-error --output-document=- \
  --method=POST \
  --header='Content-Type: application/json' \
  --header='Authorization: Bearer abc123' \
  --body-data='{
  "say": "hi \"there\"",
  "path": "C:\\tmp",
  "price": "$5 ${total}",
  "swift": "\(name)",
  "unicode": "héllo 世界 🎉"
}' \
  'https://api.example.com/items'
"#
        );
    }

    #[test]
    fn snippet_go_uses_tabs() {
        let code = to_snippet(&get_request(), Go);
        assert!(code.contains("\n\treq, err := http.NewRequest(") && !code.contains("    "), "{code}");
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

        let axios = to_snippet(&req, JavaScriptAxios);
        assert!(axios.contains("\n  data: Buffer.from(\"/wBh/g==\", \"base64\"),\n"), "{axios}");
        assert!(!axios.contains("transformRequest"), "{axios}");
        let httpx = to_snippet(&req, PythonHttpx);
        assert!(httpx.starts_with("import base64\n\nimport httpx\n\n"), "{httpx}");
        assert!(httpx.contains("\ncontent = base64.b64decode(\"/wBh/g==\")\n"), "{httpx}");
        assert!(httpx.contains("httpx.request(\"PUT\", url, content=content)"), "{httpx}");
        let go = to_snippet(&req, Go);
        assert!(go.contains("import (\n\t\"bytes\"\n\t\"encoding/base64\"\n\t\"fmt\"\n"), "{go}");
        assert!(go.contains("\n\tbody, err := base64.StdEncoding.DecodeString(\"/wBh/g==\")\n"), "{go}");
        assert!(go.contains("http.NewRequest(\"PUT\", \"https://x.test/upload\", bytes.NewReader(body))"), "{go}");
        assert!(!go.contains("\"strings\""), "{go}");
        let java = to_snippet(&req, Java);
        assert!(java.contains("\nimport java.util.Base64;\n"), "{java}");
        assert!(java.contains(".PUT(BodyPublishers.ofByteArray(Base64.getDecoder().decode(\"/wBh/g==\")))"), "{java}");
        let csharp = to_snippet(&req, CSharp);
        assert!(!csharp.contains("using System.Text;"), "{csharp}");
        assert!(
            csharp.contains("\nrequest.Content = new ByteArrayContent(Convert.FromBase64String(\"/wBh/g==\"));\n"),
            "{csharp}"
        );
        let php = to_snippet(&req, Php);
        assert!(php.contains("\n    CURLOPT_POSTFIELDS => base64_decode(\"/wBh/g==\"),\n"), "{php}");
        let ruby = to_snippet(&req, Ruby);
        assert!(ruby.contains("\nrequest.body = \"/wBh/g==\".unpack1(\"m\")\n"), "{ruby}");
        assert!(ruby.starts_with("require \"net/http\"\n\n"), "{ruby}");
        let rust = to_snippet(&req, Rust);
        assert!(rust.contains("\n        .body(b\"\\xff\\0a\\xfe\".to_vec())\n"), "{rust}");
        let dart = to_snippet(&req, Dart);
        assert!(dart.contains("\n  request.bodyBytes = base64Decode('/wBh/g==');\n"), "{dart}");
        let c = to_snippet(&req, C);
        assert!(c.contains("\n    static const char body[] = \"\\377\\000a\\376\";\n"), "{c}");
        let powershell = to_snippet(&req, PowerShell);
        assert!(powershell.contains("\n    Body = [Convert]::FromBase64String('/wBh/g==')\n"), "{powershell}");
        assert_eq!(to_snippet(&req, Httpie), "printf '%s' '/wBh/g==' | base64 -d | http PUT 'https://x.test/upload'\n");
        assert_eq!(
            to_snippet(&req, Wget),
            r#"body=$(mktemp)
printf '%s' '/wBh/g==' | base64 -d > "$body"
wget --quiet --server-response --content-on-error --output-document=- \
  --method=PUT \
  --body-file="$body" \
  'https://x.test/upload'
rm "$body"
"#
        );
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
        assert_eq!(
            to_snippet(&req, PythonHttpx),
            r#"import httpx

url = "https://x.test/"
headers = [
    ("X-Tag", "a"),
    ("Accept", "*/*"),
    ("x-tag", "b"),
    ("Cookie", "a=1"),
    ("cookie", "b=2"),
]

response = httpx.request("GET", url, headers=headers)
print(response.status_code, response.text)
"#
        );
        let swift = to_snippet(&req, Swift);
        assert!(swift.contains("request.setValue(\"a\", forHTTPHeaderField: \"X-Tag\")\n"), "{swift}");
        assert!(swift.contains("request.addValue(\"b\", forHTTPHeaderField: \"x-tag\")\n"), "{swift}");
        let kotlin = to_snippet(&req, Kotlin);
        assert!(kotlin.contains(".addHeader(\"X-Tag\", \"a\")\n    .addHeader(\"Accept\""), "{kotlin}");
        assert!(kotlin.contains(".addHeader(\"x-tag\", \"b\")\n"), "{kotlin}");

        // Libraries that keep headers in a map get them joined.
        let axios = to_snippet(&req, JavaScriptAxios);
        assert!(
            axios.contains(
                "\n  // An object holds one value per header, so repeated headers are joined: \"X-Tag\", \"Cookie\".\n  \
                 headers: {\n    \"X-Tag\": \"a, b\",\n    \"Accept\": \"*/*\",\n    \"Cookie\": \"a=1; b=2\",\n  },\n"
            ),
            "{axios}"
        );
        let ruby = to_snippet(&req, Ruby);
        assert!(
            ruby.contains(
                "\n# Net::HTTP sends one line per header name, so repeated headers are joined: \"X-Tag\", \"Cookie\".\n\
                 request[\"X-Tag\"] = \"a, b\"\nrequest[\"Accept\"] = \"*/*\"\nrequest[\"Cookie\"] = \"a=1; b=2\"\n"
            ),
            "{ruby}"
        );
        let dart = to_snippet(&req, Dart);
        assert!(
            dart.contains(
                "\n  // A Map holds one value per header, so repeated headers are joined: 'X-Tag', 'Cookie'.\n  \
                 request.headers['X-Tag'] = 'a, b';\n  request.headers['Accept'] = '*/*';\n  \
                 request.headers['Cookie'] = 'a=1; b=2';\n"
            ),
            "{dart}"
        );
        let powershell = to_snippet(&req, PowerShell);
        assert!(
            powershell.contains(
                "\n    Headers = @{\n        \
                 # A hashtable holds one value per header, so repeated headers are joined: 'X-Tag', 'Cookie'.\n        \
                 'X-Tag' = 'a, b'\n        'Accept' = '*/*'\n        'Cookie' = 'a=1; b=2'\n    }\n"
            ),
            "{powershell}"
        );
        // The others send every header as it is.
        let kept = [
            (Go, "\treq.Header.Add(\"X-Tag\", \"a\")\n", "\treq.Header.Add(\"x-tag\", \"b\")\n"),
            (Java, ".header(\"X-Tag\", \"a\")\n", ".header(\"x-tag\", \"b\")\n"),
            (
                CSharp,
                "request.Headers.TryAddWithoutValidation(\"X-Tag\", \"a\");\n",
                "request.Headers.TryAddWithoutValidation(\"x-tag\", \"b\");\n",
            ),
            (Php, "        \"X-Tag: a\",\n", "        \"x-tag: b\",\n"),
            (Rust, ".header(\"X-Tag\", \"a\")\n", ".header(\"x-tag\", \"b\")\n"),
            (C, "curl_slist_append(headers, \"X-Tag: a\");\n", "curl_slist_append(headers, \"x-tag: b\");\n"),
            (Httpie, "  'X-Tag:a' \\\n", "  'x-tag:b' \\\n"),
            (Wget, "  --header='X-Tag: a' \\\n", "  --header='x-tag: b' \\\n"),
        ];
        for (lang, first, second) in kept {
            let code = to_snippet(&req, lang);
            assert!(code.contains(first) && code.contains(second), "{lang:?}\n{code}");
            assert!(code.contains("a=1") && code.contains("b=2") && !code.contains("a, b"), "{lang:?}\n{code}");
        }
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
    fn snippet_methods() {
        let head = http("HEAD", "https://x.test/", &[], "");
        // A custom HEAD makes curl wait for a body; NOBODY sends a real HEAD request.
        let php = to_snippet(&head, Php);
        assert!(php.contains("\n    CURLOPT_NOBODY => true,\n") && !php.contains("CUSTOMREQUEST"), "{php}");
        let c = to_snippet(&head, C);
        assert!(
            c.contains("\n    curl_easy_setopt(curl, CURLOPT_NOBODY, 1L);\n") && !c.contains("CUSTOMREQUEST"),
            "{c}"
        );
        // Net::HTTP must not wait for a response body after HEAD.
        let ruby = to_snippet(&head, Ruby);
        assert!(ruby.contains("Net::HTTPGenericRequest.new(\"HEAD\", false, false, uri.request_uri)"), "{ruby}");
        assert!(to_snippet(&head, Rust).contains("\n        .head(\"https://x.test/\")\n"));
        assert!(to_snippet(&head, Java).contains("\n                .method(\"HEAD\", BodyPublishers.noBody())\n"));
        assert!(to_snippet(&head, PowerShell).contains("\n    Method = 'Head'\n"));
        assert!(to_snippet(&head, Wget).contains("\n  --method=HEAD \\\n"));

        let propfind = http("PROPFIND", "https://x.test/", &[], "<a/>");
        let rust = to_snippet(&propfind, Rust);
        assert!(
            rust.contains("\n        .request(reqwest::Method::from_bytes(b\"PROPFIND\")?, \"https://x.test/\")\n")
        );
        assert!(to_snippet(&propfind, Java).contains(".method(\"PROPFIND\", BodyPublishers.ofString(\"<a/>\"))"));
        assert!(to_snippet(&propfind, PowerShell).contains("\n    CustomMethod = 'PROPFIND'\n"));
        assert!(to_snippet(&propfind, CSharp).contains("new HttpRequestMessage(new HttpMethod(\"PROPFIND\"), "));
        assert!(to_snippet(&propfind, Php).contains("\n    CURLOPT_CUSTOMREQUEST => \"PROPFIND\",\n"));
        assert!(to_snippet(&propfind, Httpie).starts_with("http --ignore-stdin PROPFIND 'https://x.test/' \\\n"));

        let delete = to_snippet(&http("DELETE", "https://x.test/", &[], ""), Java);
        assert!(delete.contains("\n                .DELETE()\n") && !delete.contains("BodyPublishers"), "{delete}");
        let post = to_snippet(&http("POST", "https://x.test/", &[], ""), Java);
        assert!(post.contains("\n                .POST(BodyPublishers.noBody())\n"), "{post}");
        let ruby = to_snippet(&http("POST", "https://x.test/", &[], ""), Ruby);
        assert!(ruby.contains("Net::HTTPGenericRequest.new(\"POST\", false, true, uri.request_uri)"), "{ruby}");
        // Methods that are not plain words are quoted for the shell.
        let odd = http("GET ME", "https://x.test/", &[], "");
        assert!(to_snippet(&odd, Httpie).starts_with("http --ignore-stdin 'GET ME' 'https://x.test/'\n"));
        assert!(to_snippet(&odd, Wget).contains("\n  --method='GET ME' \\\n"));
    }

    #[test]
    fn snippet_drops_managed_headers() {
        let req = managed_request();
        for lang in ALL {
            let code = to_snippet(&req, lang);
            assert!(!code.contains("Host"), "{lang:?}\n{code}");
            assert!(!code.contains("Content-Length"), "{lang:?}\n{code}");
            assert!(code.contains("X-Keep"), "{lang:?}\n{code}");
        }
        // Libraries that ask for compressed responses and decompress them themselves.
        let decompress = [
            (Kotlin, "OkHttp"),
            (Swift, "URLSession"),
            (JavaScriptAxios, "axios"),
            (Dart, "package:http"),
            (PowerShell, "Invoke-WebRequest"),
        ];
        for lang in ALL {
            let code = to_snippet(&req, lang);
            match decompress.iter().find(|(l, _)| *l == lang) {
                Some((_, library)) => {
                    let note = format!(
                        " Accept-Encoding is left out: {library} adds it and then decompresses the response itself.\n"
                    );
                    assert!(code.contains(&note), "{lang:?}\n{code}");
                    assert!(!code.contains("gzip"), "{lang:?}\n{code}");
                }
                None => assert!(code.contains("Accept-Encoding") && code.contains("gzip"), "{lang:?}\n{code}"),
            }
        }
        let kotlin = to_snippet(&req, Kotlin);
        assert!(
            kotlin.contains(
                "\n// Accept-Encoding is left out: OkHttp adds it and then decompresses the response itself.\n\
                 val request = "
            ),
            "{kotlin}"
        );
        assert!(to_snippet(&req, JavaScript).contains("\n    \"Accept-Encoding\": \"gzip\",\n"));
        assert!(to_snippet(&req, Python).contains("\n    \"Accept-Encoding\": \"gzip\",\n"));
        let dart = to_snippet(&req, Dart);
        assert!(
            dart.contains(
                "\n  // Accept-Encoding is left out: package:http adds it and then decompresses the response itself.\n  \
                 final request = "
            ),
            "{dart}"
        );
        let powershell = to_snippet(&req, PowerShell);
        assert!(powershell.starts_with("# Accept-Encoding is left out: Invoke-WebRequest adds it"), "{powershell}");
    }

    #[test]
    fn snippet_java_restricted_headers() {
        let req = http(
            "GET",
            "https://x.test/",
            &[
                ("Connection", "close"),
                ("Expect", "100-continue"),
                ("connection", "x"),
                ("Upgrade", "h2c"),
                ("X-Keep", "1"),
            ],
            "",
        );
        let java = to_snippet(&req, Java);
        assert!(
            java.contains(
                "\n        // HttpClient does not allow setting Connection, Expect and Upgrade, so they are left out.\n        \
                 HttpRequest request = "
            ),
            "{java}"
        );
        assert!(!java.contains(".header(\"Connection\"") && !java.contains(".header(\"connection\""), "{java}");
        assert!(!java.contains(".header(\"Expect\"") && !java.contains(".header(\"Upgrade\""), "{java}");
        assert!(java.contains(".header(\"X-Keep\", \"1\")"), "{java}");
        let one = to_snippet(&http("GET", "https://x.test/", &[("upgrade", "h2c")], ""), Java);
        assert!(one.contains("// HttpClient does not allow setting upgrade, so it is left out.\n"), "{one}");
        // The other libraries send them.
        assert!(to_snippet(&req, Go).contains("\treq.Header.Add(\"Connection\", \"close\")\n"));
        assert!(to_snippet(&req, CSharp).contains("request.Headers.TryAddWithoutValidation(\"Upgrade\", \"h2c\");"));
    }

    #[test]
    fn snippet_csharp_content_headers() {
        let headers = [("Content-Type", "text/plain"), ("X-A", "1"), ("content-language", "en"), ("Expires", "0")];
        let post = to_snippet(&http("POST", "https://x.test/", &headers, "hi"), CSharp);
        assert!(
            post.contains(
                "\nrequest.Headers.TryAddWithoutValidation(\"X-A\", \"1\");\n\
                 request.Content = new ByteArrayContent(Encoding.UTF8.GetBytes(\"hi\"));\n\
                 request.Content.Headers.TryAddWithoutValidation(\"Content-Type\", \"text/plain\");\n\
                 request.Content.Headers.TryAddWithoutValidation(\"content-language\", \"en\");\n\
                 request.Content.Headers.TryAddWithoutValidation(\"Expires\", \"0\");\n"
            ),
            "{post}"
        );
        // Without a body there is nothing to carry them.
        let get = to_snippet(&http("GET", "https://x.test/", &headers, ""), CSharp);
        assert!(
            get.contains(
                "\n// HttpClient sends Content-Type, content-language and Expires only with a body, so they are left out.\n\
                 using var request = "
            ),
            "{get}"
        );
        assert!(get.contains("\nrequest.Headers.TryAddWithoutValidation(\"X-A\", \"1\");\n"), "{get}");
        assert!(!get.contains("text/plain") && !get.contains("request.Content"), "{get}");
    }

    #[test]
    fn snippet_axios_json_body() {
        // axios would trim a JSON body, and quote one that does not parse.
        let json = to_snippet(
            &http("POST", "https://x.test/", &[("content-type", "application/json")], " {{a}} "),
            JavaScriptAxios,
        );
        assert!(json.contains("\n  data: \" {{a}} \",\n  // Send the text as it is: "), "{json}");
        assert!(json.contains("\n  transformRequest: (data) => data,\n"), "{json}");
        // Other text goes as it is.
        let form = to_snippet(&http("POST", "https://x.test/", &[], "a=1"), JavaScriptAxios);
        assert!(form.contains("\n  data: \"a=1\",\n  responseType: \"text\",\n"), "{form}");
    }

    #[test]
    fn snippet_shell_bodies_and_headers() {
        // Lines stay readable in single quotes; other control characters need ANSI-C quotes.
        let quote = http("POST", "https://x.test/", &[], "it's\nok");
        assert!(to_snippet(&quote, Httpie).ends_with(" \\\n  --raw 'it'\\''s\nok'\n"));
        assert!(to_snippet(&quote, Wget).contains(" \\\n  --body-data='it'\\''s\nok' \\\n"));
        let tab = http("POST", "https://x.test/", &[], "a\tb");
        assert!(to_snippet(&tab, Httpie).ends_with(" \\\n  --raw $'a\\tb'\n"));
        assert!(to_snippet(&tab, Wget).contains(" \\\n  --body-data=$'a\\tb' \\\n"));
        // An argument cannot hold a NUL byte, so the body goes through base64.
        let nul = http("POST", "https://x.test/", &[], "a\u{0}b");
        assert_eq!(to_snippet(&nul, Httpie), "printf '%s' 'YQBi' | base64 -d | http POST 'https://x.test/'\n");
        let wget = to_snippet(&nul, Wget);
        assert!(wget.contains("\nprintf '%s' 'YQBi' | base64 -d > \"$body\"\n"), "{wget}");
        assert!(wget.contains(" \\\n  --body-file=\"$body\" \\\n"), "{wget}");

        // `Name:` removes a header in HTTPie, and `Name:=value` is a JSON field.
        let req = http("GET", "https://x.test/", &[("X-Empty", ""), ("X-Eq", "=1"), ("X-Q", "it's")], "");
        let httpie = to_snippet(&req, Httpie);
        assert!(httpie.ends_with(" \\\n  'X-Empty;' \\\n  'X-Eq:\\=1' \\\n  'X-Q:it'\\''s'\n"), "{httpie}");
        let wget = to_snippet(&req, Wget);
        assert!(wget.contains("\n  --header='X-Empty: ' \\\n  --header='X-Eq: =1' \\\n"), "{wget}");
        // curl sends a header with an empty value as `Name;`.
        assert!(to_snippet(&req, Php).contains("\n        \"X-Empty;\",\n        \"X-Eq: =1\",\n"));
        assert!(to_snippet(&req, C).contains("curl_slist_append(headers, \"X-Empty;\");\n"));
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
        let powershell = to_snippet(&req, PowerShell);
        assert!(
            powershell
                .starts_with("# Invoke-WebRequest cannot send a body with GET, so it is left out.\n$params = @{\n"),
            "{powershell}"
        );
        assert!(!powershell.contains("q=1"), "{powershell}");

        // The other libraries send it.
        for lang in [JavaScriptAxios, PythonHttpx, Go, Java, CSharp, Php, Ruby, Rust, Dart, C, Httpie, Wget] {
            let code = to_snippet(&req, lang);
            assert!(code.contains("q=1") && !code.contains("left out"), "{lang:?}\n{code}");
        }
        // A body alone would turn curl's request into a POST.
        assert!(
            to_snippet(&req, Php)
                .contains("\n    CURLOPT_CUSTOMREQUEST => \"GET\",\n    CURLOPT_POSTFIELDS => \"q=1\",\n")
        );
        assert!(to_snippet(&req, C).contains("\n    curl_easy_setopt(curl, CURLOPT_CUSTOMREQUEST, \"GET\");\n"));
        assert!(to_snippet(&req, Ruby).contains("Net::HTTPGenericRequest.new(\"GET\", true, true, uri.request_uri)"));
        assert!(
            to_snippet(&req, Java).contains("\n                .method(\"GET\", BodyPublishers.ofString(\"q=1\"))\n")
        );
        assert!(to_snippet(&req, Wget).contains("\n  --method=GET \\\n  --body-data='q=1' \\\n"));
    }

    #[test]
    fn snippet_c_body_lines() {
        let one = to_snippet(&http("POST", "https://x.test/", &[], "a=1"), C);
        assert!(one.contains("\n    static const char body[] = \"a=1\";\n"), "{one}");
        let lines = to_snippet(&http("POST", "https://x.test/", &[], "a\r\nb\n"), C);
        assert!(
            lines.contains("\n    static const char body[] =\n        \"a\\r\\n\"\n        \"b\\n\";\n"),
            "{lines}"
        );
    }

    #[test]
    fn snippet_string_escapes() {
        let s = "a\u{7}\u{0}\u{7f}\u{85}$\\\"\r\n\t\u{2028}é";
        assert_eq!(kotlin_string(s), "\"a\\u0007\\u0000\\u007f\\u0085\\$\\\\\\\"\\r\\n\\t\u{2028}é\"");
        assert_eq!(swift_string(s), "\"a\\u{7}\\u{0}\\u{7f}\\u{85}$\\\\\\\"\\r\\n\\t\u{2028}é\"");
        assert_eq!(swift_string("\\(x)"), r#""\\(x)""#);
        assert_eq!(json_string("\u{8}\u{c}\u{1}\"\\"), r#""\b\f\u0001\"\\""#);

        assert_eq!(go_string(s), "\"a\\x07\\x00\\x7f\\u0085$\\\\\\\"\\r\\n\\t\u{2028}é\"");
        assert_eq!(go_string("\u{feff}`"), "\"\\ufeff`\"");
        assert_eq!(java_string(s), "\"a\\007\\000\\177\\205$\\\\\\\"\\r\\n\\t\u{2028}é\"");
        // Never `\u`, which Java reads before the string; octal escapes keep three digits.
        assert_eq!(java_string("\u{8}\u{c}\u{1}1\\u000a"), r#""\b\f\0011\\u000a""#);
        assert_eq!(csharp_string(s), "\"a\\a\\0\\u007f\\u0085$\\\\\\\"\\r\\n\\t\\u2028é\"");
        assert_eq!(csharp_string("\u{0}1\u{b}\u{2029}"), r#""\01\v\u2029""#);
        assert_eq!(php_string(s), "\"a\\x07\\x00\\x7f\\u{85}\\$\\\\\\\"\\r\\n\\t\u{2028}é\"");
        assert_eq!(php_string("{$x}\u{1b}\u{b}\u{c}"), r#""{\$x}\e\v\f""#);
        assert_eq!(ruby_string(s), "\"a\\u{7}\\u{0}\\u{7f}\\u{85}$\\\\\\\"\\r\\n\\t\u{2028}é\"");
        assert_eq!(ruby_string("#{x} #@y #$z \u{1b}"), r#""\#{x} \#@y \#$z \e""#);
        assert_eq!(rust_string(s), "\"a\\u{7}\\0\\u{7f}\\u{85}$\\\\\\\"\\r\\n\\t\u{2028}é\"");
        assert_eq!(rust_string("\u{202e}{}"), r#""\u{202e}{}""#);
        assert_eq!(rust_bytes(b"\xff\x00a\"\\\n"), r#"b"\xff\0a\"\\\n""#);
        assert_eq!(dart_string(s), "'a\\u{7}\\u{0}\\u{7f}\\u{85}\\$\\\\\"\\r\\n\\t\u{2028}é'");
        assert_eq!(dart_string("it's ${x}"), r#"'it\'s \${x}'"#);
        assert_eq!(c_string(s), "\"a\\007\\000\\177\\302\\205$\\\\\\\"\\r\\n\\t\u{2028}é\"");
        // `??=` would be a trigraph; octal escapes keep three digits.
        assert_eq!(c_string("a??=b?\u{1}2"), r#""a?\?=b?\0012""#);
        assert_eq!(c_bytes(b"\xff\x00a??"), r#""\377\000a?\?""#);
        assert_eq!(powershell_string(s), "\"a`a`0`u{7f}`u{85}`$\\`\"`r`n`t\u{2028}é\"");
        assert_eq!(powershell_string("\u{1b}`\u{201c}"), "\"`e```\u{201c}\"");
        // Single quotes where they can be used keep `$`, backticks and lines as they are.
        assert_eq!(powershell_string("it's $x `y`\n\u{2019}"), "'it''s $x `y`\n\u{2019}\u{2019}'");
        assert_eq!(shell("a\nb 'c'"), "'a\nb '\\''c'\\'''");
        assert_eq!(shell("a\tb"), "$'a\\tb'");
        assert_eq!(shell_word("POST"), "POST");
        assert_eq!(shell_word("A B"), "'A B'");
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
                &[("X-Template", "$HOME ${x} \\(y) `z` #{a} ??= 'q' \"d\"")],
                "nul\u{0} bell\u{7} del\u{7f} ls\u{2028} bom\u{feff} é\r\n\ttab \"q\" 'a' \\ $x ${y} \\(z) `t` {{a}} #{b} ??= \u{201c}p\u{201d}",
            ),
            http("HEAD", "https://x.test/", &[("Connection", "close"), ("X-Empty", "")], ""),
            http(
                "POST",
                "https://x.test/",
                &[("Content-Type", "application/json"), ("Cookie", "a=1")],
                "{\n  \"a\": \"it's\"\n}\n",
            ),
        ]
    }

    /// Checks the generated code with the toolchains that are installed, each case in a directory
    /// of its own under the temporary directory, kept for inspection. Kotlin needs OkHttp on the
    /// classpath, so the unit tests above cover it; Rust and Dart are parsed only, since their
    /// libraries are not at hand, and the shell commands too.
    #[test]
    #[ignore = "runs the language toolchains that are installed"]
    fn snippet_syntax_with_toolchains() {
        use std::process::Command;

        let dir = std::env::temp_dir().join("zorvik-snippets");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // An empty config keeps rustfmt's defaults.
        std::fs::write(dir.join("rustfmt.toml"), "").unwrap();
        std::fs::write(dir.join("curl.c"), "#include <curl/curl.h>\n").unwrap();
        let parse_ps1 = dir.join("parse.ps1");
        std::fs::write(
            &parse_ps1,
            "$errors = $null\n\
             $null = [System.Management.Automation.Language.Parser]::ParseFile((Resolve-Path $args[0]).Path, [ref]$null, [ref]$errors)\n\
             if ($errors) {\n    $errors | ForEach-Object { \"$($_.Extent.StartLineNumber): $($_.Message)\" }\n    exit 1\n}\n",
        )
        .unwrap();
        let parse_ps1 = parse_ps1.display().to_string();
        let command = |args: &[&str]| {
            let mut command = Command::new(args[0]);
            command.args(&args[1..]).env("DOTNET_CLI_TELEMETRY_OPTOUT", "1").env("DOTNET_NOLOGO", "1");
            command
        };
        let works = |args: &[&str]| command(args).current_dir(&dir).output().is_ok_and(|o| o.status.success());
        // The project builds for the newest .NET the SDK knows.
        let dotnet = command(&["dotnet", "--version"]).output().ok().filter(|o| o.status.success()).map(|o| {
            let version = String::from_utf8_lossy(&o.stdout).to_string();
            let major = version.split('.').next().unwrap_or_default().trim().to_string();
            format!(
                "<Project Sdk=\"Microsoft.NET.Sdk\">\n  <PropertyGroup>\n    <OutputType>Exe</OutputType>\n    \
                 <TargetFramework>net{major}.0</TargetFramework>\n    <ImplicitUsings>enable</ImplicitUsings>\n  \
                 </PropertyGroup>\n</Project>\n"
            )
        });

        // A language, its file, a command that works when the toolchain is installed, and the checks.
        type Check<'a> = (SnippetLanguage, &'a str, Vec<&'a str>, Vec<Vec<&'a str>>);
        let checks: Vec<Check<'_>> = vec![
            (JavaScript, "snippet.mjs", vec!["node", "--version"], vec![vec!["node", "--check", "snippet.mjs"]]),
            (JavaScriptAxios, "snippet.mjs", vec!["node", "--version"], vec![vec!["node", "--check", "snippet.mjs"]]),
            (
                Python,
                "snippet.py",
                vec!["python3", "--version"],
                vec![vec!["python3", "-m", "py_compile", "snippet.py"]],
            ),
            (
                PythonHttpx,
                "snippet.py",
                vec!["python3", "--version"],
                vec![vec!["python3", "-m", "py_compile", "snippet.py"]],
            ),
            // `swiftc -parse` needs no SDK (URLSession is in FoundationNetworking on Linux).
            (Swift, "snippet.swift", vec!["swiftc", "--version"], vec![vec!["swiftc", "-parse", "snippet.swift"]]),
            // gofmt lists the file when its formatting differs; vet compiles the program.
            (
                Go,
                "main.go",
                vec!["go", "version"],
                vec![vec!["gofmt", "-e", "-l", "main.go"], vec!["go", "vet", "main.go"]],
            ),
            // java.net.http is part of the JDK, so the program compiles.
            (
                Java,
                "Main.java",
                vec!["javac", "-version"],
                vec![vec!["javac", "-encoding", "UTF-8", "-d", "classes", "Main.java"]],
            ),
            (CSharp, "Program.cs", vec!["dotnet", "--version"], vec![vec!["dotnet", "build", "--nologo"]]),
            (Php, "snippet.php", vec!["php", "--version"], vec![vec!["php", "-l", "snippet.php"]]),
            (Ruby, "snippet.rb", vec!["ruby", "--version"], vec![vec!["ruby", "-c", "snippet.rb"]]),
            (
                Rust,
                "main.rs",
                vec!["rustfmt", "--version"],
                vec![vec!["rustfmt", "--edition", "2024", "--emit", "stdout", "main.rs"]],
            ),
            (Dart, "main.dart", vec!["dart", "--version"], vec![vec!["dart", "format", "--output=none", "main.dart"]]),
            // Needs the libcurl headers.
            (
                C,
                "main.c",
                vec!["cc", "-fsyntax-only", "curl.c"],
                vec![vec!["cc", "-fsyntax-only", "-Wall", "-Wextra", "-Werror", "main.c"]],
            ),
            (
                PowerShell,
                "snippet.ps1",
                vec!["pwsh", "-Version"],
                vec![vec!["pwsh", "-NoProfile", "-NonInteractive", "-File", parse_ps1.as_str(), "snippet.ps1"]],
            ),
            (Httpie, "snippet.sh", vec!["bash", "--version"], vec![vec!["bash", "-n", "snippet.sh"]]),
            (Wget, "snippet.sh", vec!["bash", "--version"], vec![vec!["bash", "-n", "snippet.sh"]]),
        ];
        let mut checked = 0;
        for (lang, file, probe, commands) in checks {
            if !works(&probe) {
                eprintln!("`{}` failed (not installed?); skipped {lang:?}", probe.join(" "));
                continue;
            }
            for (i, req) in syntax_cases().iter().enumerate() {
                let code = to_snippet(req, lang);
                let case = dir.join(format!("{lang:?}/case{i}"));
                std::fs::create_dir_all(&case).unwrap();
                std::fs::write(case.join(file), &code).unwrap();
                if let (CSharp, Some(project)) = (lang, &dotnet) {
                    std::fs::write(case.join("snippet.csproj"), project).unwrap();
                }
                for args in &commands {
                    let output = command(args).current_dir(&case).output().unwrap();
                    // gofmt succeeds on a file it would reformat, and names it.
                    let quiet = args[0] != "gofmt" || output.stdout.is_empty();
                    assert!(
                        output.status.success() && quiet,
                        "{} rejected {}:\n{}{}\n{code}",
                        args.join(" "),
                        case.join(file).display(),
                        String::from_utf8_lossy(&output.stdout),
                        String::from_utf8_lossy(&output.stderr)
                    );
                }
                checked += 1;
            }
            eprintln!("{lang:?}: {} snippets OK", syntax_cases().len());
        }
        eprintln!("checked {checked} snippets in {}", dir.display());
    }
}
