//! cURL import and export.
//!
//! [`parse_curl`] reads commands copied from browser dev tools ("Copy as cURL"
//! for bash and for cmd), API docs and terminals. [`to_curl`] renders a resolved
//! request for bash, Windows cmd or PowerShell.

use std::fmt::Write as _;

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use zorvik_engine::{HttpRequest, HttpVersionPref};

use crate::import::ImportError;
use crate::model::{
    Auth, Body, BodyType, GraphqlBody, KeyValue, MultipartField, Request, RequestKind, RequestSettings,
};

/// Result of parsing a cURL command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurlImport {
    pub request: Request,
    /// Parts of the command that were ignored or could not be imported faithfully.
    pub warnings: Vec<String>,
}

/// Shell a generated cURL command is meant for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum CurlFlavor {
    /// bash, zsh and other POSIX shells.
    Bash,
    /// Windows `cmd.exe`.
    Cmd,
    /// PowerShell, calling `curl.exe`.
    PowerShell,
}

/// Parse a cURL command line into a request.
pub fn parse_curl(input: &str) -> Result<CurlImport, ImportError> {
    let input = input.trim_start_matches('\u{feff}').trim();
    let mut warnings = Vec::new();
    let mut words = if input.is_empty() { Vec::new() } else { split_words(input, &mut warnings) };
    // A copied shell prompt (`$ curl ...`).
    if words.first().is_some_and(|w| w == "$" || w == "%") {
        words.remove(0);
    }
    let Some(program) = words.first() else {
        return Err(ImportError::new("Paste a cURL command to import"));
    };
    let name = program.rsplit(['/', '\\']).next().unwrap_or_default().to_ascii_lowercase();
    if name != "curl" && name != "curl.exe" {
        return Err(ImportError::new(format!(
            "Not a cURL command: it should start with `curl`, not `{}`",
            clip(program)
        )));
    }
    let mut cmd = Command { warnings, ..Command::default() };
    cmd.parse_args(words.into_iter().skip(1));
    cmd.finish()
}

/// `command` with `notes` as comments before it (`REM` for the Windows Command Prompt).
pub fn noted(command: String, flavor: CurlFlavor, notes: &[String]) -> String {
    let prefix = if flavor == CurlFlavor::Cmd { "REM" } else { "#" };
    let block: String = notes.iter().map(|n| format!("{prefix} {n}\n")).collect();
    format!("{block}{command}")
}

/// Render a fully resolved request as a cURL command line. A binary body is decoded from
/// base64 first, by the command or by a few lines before it.
pub fn to_curl(req: &HttpRequest, flavor: CurlFlavor) -> String {
    let quote = |s: &str| match flavor {
        CurlFlavor::Bash => quote_bash(s),
        CurlFlavor::Cmd => quote_cmd(s),
        CurlFlavor::PowerShell => quote_powershell(s),
    };
    let mut args = vec![quote(&escape_glob(&req.url))];
    // `-X HEAD` makes curl wait for a body that never comes.
    if req.method == "HEAD" && req.body.is_empty() {
        args.push("-I".into());
    } else if req.method != "GET" || !req.body.is_empty() {
        // `--data` implies POST, so a GET with a body needs an explicit method.
        let bare = !req.method.is_empty() && req.method.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-');
        args.push(format!("-X {}", if bare { req.method.clone() } else { quote(&req.method) }));
    }
    for h in &req.headers {
        // `Name:` without a value makes curl drop the header; `Name;` sends it empty.
        let header =
            if h.value.trim().is_empty() { format!("{};", h.name) } else { format!("{}: {}", h.name, h.value) };
        args.push(format!("-H {}", quote(&header)));
    }
    // A command line can't hold a NUL byte (bash cuts the argument short there), and cmd and
    // PowerShell can't pass bytes that aren't text: such a body goes to curl through a pipe
    // or a temporary file, as base64.
    let text = std::str::from_utf8(&req.body).ok().filter(|text| !text.contains('\0'));
    let binary = text.is_none().then(|| base64::engine::general_purpose::STANDARD.encode(&req.body));
    match (text, flavor) {
        (Some(""), _) => {}
        (Some(text), _) => args.push(format!("--data-raw {}", quote(text))),
        (None, CurlFlavor::Bash) => args.push("--data-binary @-".into()),
        (None, CurlFlavor::Cmd) => args.push(format!("--data-binary \"@{CMD_BODY}\"")),
        (None, CurlFlavor::PowerShell) => args.push("--data-binary \"@$body\"".into()),
    }
    let (program, separator) = match flavor {
        CurlFlavor::Bash => ("curl", " \\\n  "),
        CurlFlavor::Cmd => ("curl", " ^\n  "),
        CurlFlavor::PowerShell => ("curl.exe", " `\n  "),
    };
    let command = format!("{program} {}", args.join(separator));
    let Some(base64) = binary else { return command };
    match flavor {
        CurlFlavor::Bash => format!("printf '%s' {} | base64 -d | {command}", quote_bash(&base64)),
        // In short `echo` lines: a cmd line holds at most 8191 characters.
        CurlFlavor::Cmd => {
            let lines: String =
                base64.as_bytes().chunks(76).map(|line| format!("echo {}\n", String::from_utf8_lossy(line))).collect();
            format!(
                "(\n{lines}) > \"{CMD_BODY}.b64\"\ncertutil -f -decode \"{CMD_BODY}.b64\" \"{CMD_BODY}\" > nul\n\
                 {command}\ndel \"{CMD_BODY}.b64\" \"{CMD_BODY}\""
            )
        }
        CurlFlavor::PowerShell => format!(
            "$body = [IO.Path]::GetTempFileName()\n[IO.File]::WriteAllBytes($body, [Convert]::FromBase64String({}))\n\
             {command}\nRemove-Item $body",
            quote_powershell(&base64)
        ),
    }
}

/// Where a cmd command keeps a binary body for curl to read.
const CMD_BODY: &str = "%TEMP%\\zorvik-body";

// ---------------------------------------------------------------------------
// Tokenizing
// ---------------------------------------------------------------------------

/// Words being collected by a tokenizer. Bytes, because `$'\xHH'` can produce any byte.
#[derive(Default)]
struct Words {
    words: Vec<String>,
    cur: Option<Vec<u8>>,
    unterminated: bool,
    invalid_utf8: bool,
}

impl Words {
    fn start(&mut self) -> &mut Vec<u8> {
        self.cur.get_or_insert_with(Vec::new)
    }

    fn push(&mut self, c: char) {
        self.start().extend_from_slice(c.encode_utf8(&mut [0; 4]).as_bytes());
    }

    fn in_word(&self) -> bool {
        self.cur.is_some()
    }

    fn end(&mut self) {
        if let Some(bytes) = self.cur.take() {
            let word = String::from_utf8(bytes).unwrap_or_else(|e| {
                self.invalid_utf8 = true;
                String::from_utf8_lossy(e.as_bytes()).into_owned()
            });
            self.words.push(word);
        }
    }
}

/// Split a command line into words the way the shell it was written for would.
fn split_words(input: &str, warnings: &mut Vec<String>) -> Vec<String> {
    let ends_line = |suffix: &str| input.lines().any(|l| l.trim_end().ends_with(suffix));
    let mut w = Words::default();
    let rest = if input.contains(" ^\"") || ends_line(" ^") {
        split_cmd(input, &mut w)
    } else if ends_line(" `") {
        split_powershell(input, &mut w)
    } else {
        split_posix(input, &mut w)
    };
    w.end();
    if w.unterminated {
        warnings.push("The command has an unterminated quote".into());
    }
    if w.invalid_utf8 {
        warnings.push("Bytes that are not valid UTF-8 were replaced with U+FFFD".into());
    }
    if let Some(rest) = rest.as_deref().map(str::trim).filter(|r| !r.is_empty()) {
        warnings.push(format!("Only the first command was imported; ignored `{}`", clip(rest)));
    }
    w.words
}

/// POSIX shell quoting. Returns the text after a command separator, if any.
fn split_posix(input: &str, w: &mut Words) -> Option<String> {
    let s: Vec<char> = input.chars().collect();
    let mut i = 0;
    while let Some(&c) = s.get(i) {
        i += 1;
        match c {
            ' ' | '\t' | '\n' | '\r' => w.end(),
            '\\' => match s.get(i) {
                Some('\n') => i += 1,
                Some('\r') if s.get(i + 1) == Some(&'\n') => i += 2,
                Some(&n) => {
                    w.push(n);
                    i += 1;
                }
                None => {}
            },
            '\'' => {
                w.start();
                let end = s[i..].iter().position(|&c| c == '\'');
                let stop = end.map_or(s.len(), |n| i + n);
                s[i..stop].iter().for_each(|&c| w.push(c));
                w.unterminated |= end.is_none();
                i = stop + 1;
            }
            '"' => i = double_quoted(&s, i, w),
            '$' if s.get(i) == Some(&'\'') => i = ansi_c(&s, i + 1, w),
            '$' if s.get(i) == Some(&'"') => i = double_quoted(&s, i + 1, w),
            '#' if !w.in_word() => {
                while s.get(i).is_some_and(|&c| c != '\n') {
                    i += 1;
                }
            }
            // Only at the start of a word, so unquoted URLs keep their `&`.
            ';' | '|' | '&' | '<' | '>' if !w.in_word() => return Some(s[i - 1..].iter().collect()),
            c => w.push(c),
        }
    }
    None
}

fn double_quoted(s: &[char], mut i: usize, w: &mut Words) -> usize {
    w.start();
    while let Some(&c) = s.get(i) {
        i += 1;
        match c {
            '"' => return i,
            '\\' => match s.get(i) {
                Some('\n') => i += 1,
                Some('\r') if s.get(i + 1) == Some(&'\n') => i += 2,
                Some(&n @ ('$' | '`' | '"' | '\\')) => {
                    w.push(n);
                    i += 1;
                }
                _ => w.push('\\'),
            },
            c => w.push(c),
        }
    }
    w.unterminated = true;
    i
}

/// Bash ANSI-C quoting: `$'...'`, starting after the opening quote.
fn ansi_c(s: &[char], mut i: usize, w: &mut Words) -> usize {
    w.start();
    while let Some(&c) = s.get(i) {
        i += 1;
        if c == '\'' {
            return i;
        }
        if c != '\\' {
            w.push(c);
            continue;
        }
        let Some(&e) = s.get(i) else { break };
        i += 1;
        match e {
            'n' => w.push('\n'),
            't' => w.push('\t'),
            'r' => w.push('\r'),
            'a' => w.push('\x07'),
            'b' => w.push('\x08'),
            'e' | 'E' => w.push('\x1b'),
            'f' => w.push('\x0c'),
            'v' => w.push('\x0b'),
            '\\' | '\'' | '"' | '?' => w.push(e),
            'x' | 'u' | 'U' => {
                let max = match e {
                    'x' => 2,
                    'u' => 4,
                    _ => 8,
                };
                let (value, len) = digits(&s[i..], 16, max);
                i += len;
                if len == 0 {
                    w.push('\\');
                    w.push(e);
                } else if e == 'x' {
                    w.start().push(value as u8);
                } else {
                    w.push(char::from_u32(value).unwrap_or('\u{fffd}'));
                }
            }
            '0'..='7' => {
                let (value, len) = digits(&s[i - 1..], 8, 3);
                i += len - 1;
                w.start().push(value as u8);
            }
            'c' if i < s.len() => {
                w.start().push(s[i] as u8 & 0x1f);
                i += 1;
            }
            _ => {
                w.push('\\');
                w.push(e);
            }
        }
    }
    w.unterminated = true;
    i
}

/// Value and length of up to `max` leading digits in `radix`.
fn digits(s: &[char], radix: u32, max: usize) -> (u32, usize) {
    let len = s.iter().take(max).take_while(|c| c.is_digit(radix)).count();
    let value = s[..len].iter().filter_map(|c| c.to_digit(radix)).fold(0, |v, d| v * radix + d);
    (value, len)
}

/// Windows cmd: cmd.exe strips `^` escapes, then curl.exe splits its command
/// line with the MSVC runtime rules.
fn split_cmd(input: &str, w: &mut Words) -> Option<String> {
    let s: Vec<char> = input.replace("\r\n", "\n").chars().collect();
    let mut line = Vec::with_capacity(s.len());
    let mut rest = None;
    let mut quoted = false;
    let mut i = 0;
    while let Some(&c) = s.get(i) {
        i += 1;
        match c {
            '"' => {
                quoted = !quoted;
                line.push(c);
            }
            _ if quoted => line.push(c),
            '^' => {
                // `^` + newline continues the line and escapes the first character of the next.
                if s.get(i) == Some(&'\n') {
                    i += 1;
                }
                if let Some(&n) = s.get(i) {
                    line.push(n);
                    i += 1;
                }
            }
            '\n' => line.push(' '),
            '&' | '|' | '<' | '>' => {
                rest = Some(s[i - 1..].iter().collect());
                break;
            }
            c => line.push(c),
        }
    }

    // Chrome escapes every backslash as `^\` and doubles it, even where the MSVC
    // rules keep backslashes literal (not before a quote); undo that doubling.
    let chrome = input.contains("^\\");
    let mut quoted = false;
    let mut i = 0;
    while let Some(&c) = line.get(i) {
        match c {
            '\\' => {
                let n = line[i..].iter().take_while(|&&c| c == '\\').count();
                i += n;
                let before_quote = line.get(i) == Some(&'"');
                let keep = if before_quote {
                    n / 2
                } else if chrome {
                    n.div_ceil(2)
                } else {
                    n
                };
                (0..keep).for_each(|_| w.push('\\'));
                if before_quote && n % 2 == 1 {
                    w.push('"');
                    i += 1;
                }
                continue;
            }
            '"' if quoted && line.get(i + 1) == Some(&'"') => {
                w.push('"');
                i += 1;
            }
            '"' => {
                w.start();
                quoted = !quoted;
            }
            ' ' | '\t' if !quoted => w.end(),
            c => w.push(c),
        }
        i += 1;
    }
    w.unterminated |= quoted;
    rest
}

/// Characters that open and close a PowerShell single-quoted string (it accepts typographic quotes too).
pub(crate) const PS_SINGLE_QUOTES: [char; 5] = ['\'', '\u{2018}', '\u{2019}', '\u{201a}', '\u{201b}'];

/// PowerShell quoting with backtick escapes.
fn split_powershell(input: &str, w: &mut Words) -> Option<String> {
    let s: Vec<char> = input.chars().collect();
    let mut rest = None;
    let mut i = 0;
    while let Some(&c) = s.get(i) {
        i += 1;
        match c {
            ' ' | '\t' | '\n' | '\r' => w.end(),
            '`' => match s.get(i) {
                Some('\n') => i += 1,
                Some('\r') if s.get(i + 1) == Some(&'\n') => i += 2,
                Some(&n) => {
                    w.push(n);
                    i += 1;
                }
                None => {}
            },
            c if PS_SINGLE_QUOTES.contains(&c) => {
                w.start();
                loop {
                    match s.get(i) {
                        Some(q)
                            if PS_SINGLE_QUOTES.contains(q)
                                && s.get(i + 1).is_some_and(|n| PS_SINGLE_QUOTES.contains(n)) =>
                        {
                            w.push(s[i + 1]);
                            i += 2;
                        }
                        Some(q) if PS_SINGLE_QUOTES.contains(q) => {
                            i += 1;
                            break;
                        }
                        Some(&c) => {
                            w.push(c);
                            i += 1;
                        }
                        None => {
                            w.unterminated = true;
                            break;
                        }
                    }
                }
            }
            '"' => {
                w.start();
                loop {
                    match s.get(i) {
                        Some('"') if s.get(i + 1) == Some(&'"') => {
                            w.push('"');
                            i += 2;
                        }
                        Some('"') => {
                            i += 1;
                            break;
                        }
                        Some('`') => {
                            match s.get(i + 1) {
                                Some('n') => w.push('\n'),
                                Some('t') => w.push('\t'),
                                Some('r') => w.push('\r'),
                                Some('0') => w.push('\0'),
                                Some('\n') => {}
                                Some(&e) => w.push(e),
                                None => {}
                            }
                            i += 2;
                        }
                        Some(&c) => {
                            w.push(c);
                            i += 1;
                        }
                        None => {
                            w.unterminated = true;
                            break;
                        }
                    }
                }
            }
            '#' if !w.in_word() => {
                while s.get(i).is_some_and(|&c| c != '\n') {
                    i += 1;
                }
            }
            ';' | '|' if !w.in_word() => {
                rest = Some(s[i - 1..].iter().collect());
                break;
            }
            c => w.push(c),
        }
    }
    w.end();
    // Commands written for Windows PowerShell 5.1 escape quotes as `\"` (see `quote_powershell`).
    for word in &mut w.words {
        *word = unescape_crt_quotes(word);
    }
    rest
}

/// `\"` -> `"`, halving the backslashes in front of a quote (MSVC runtime rules).
fn unescape_crt_quotes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut backslashes = 0usize;
    for c in s.chars() {
        if c == '"' {
            out.truncate(out.len() - backslashes.div_ceil(2));
        }
        backslashes = if c == '\\' { backslashes + 1 } else { 0 };
        out.push(c);
    }
    out
}

// ---------------------------------------------------------------------------
// Options
// ---------------------------------------------------------------------------

/// Long options that take a value (all of curl's, so their values are never read as the URL).
#[rustfmt::skip]
const VALUE_OPTIONS: &[&str] = &[
    "abstract-unix-socket", "alt-svc", "aws-sigv4", "cacert", "capath", "cert", "cert-type", "ciphers", "config",
    "connect-timeout", "connect-to", "continue-at", "cookie", "cookie-jar", "create-file-mode", "crlfile", "curves",
    "data", "data-ascii", "data-binary", "data-raw", "data-urlencode", "delegation", "dns-interface",
    "dns-ipv4-addr", "dns-ipv6-addr", "dns-servers", "doh-url", "dump-header", "ech", "egd-file", "engine",
    "etag-compare", "etag-save", "expect100-timeout", "form", "form-string", "ftp-account",
    "ftp-alternative-to-user", "ftp-method", "ftp-port", "ftp-ssl-ccc-mode", "happy-eyeballs-timeout-ms",
    "haproxy-clientip", "header", "hostpubmd5", "hostpubsha256", "hsts", "interface", "ip-tos", "ipfs-gateway",
    "json", "keepalive-cnt", "keepalive-time", "key", "key-type", "krb", "libcurl", "limit-rate", "local-port",
    "login-options", "mail-auth", "mail-from", "mail-rcpt", "max-filesize", "max-redirs", "max-time", "netrc-file",
    "noproxy", "oauth2-bearer", "output", "output-dir", "parallel-max", "pass", "pinnedpubkey", "preproxy", "proto",
    "proto-default", "proto-redir", "proxy", "proxy-cacert", "proxy-capath", "proxy-cert", "proxy-cert-type",
    "proxy-ciphers", "proxy-crlfile", "proxy-header", "proxy-key", "proxy-key-type", "proxy-pass",
    "proxy-pinnedpubkey", "proxy-service-name", "proxy-tls13-ciphers", "proxy-tlsauthtype", "proxy-tlspassword",
    "proxy-tlsuser", "proxy-user", "proxy1.0", "pubkey", "quote", "random-file", "range", "rate", "referer",
    "request", "request-target", "resolve", "retry", "retry-delay", "retry-max-time", "sasl-authzid",
    "service-name", "socks4", "socks4a", "socks5", "socks5-gssapi-service", "socks5-hostname", "speed-limit",
    "speed-time", "stderr", "telnet-option", "tftp-blksize", "time-cond", "tls-max", "tls13-ciphers",
    "tlsauthtype", "tlspassword", "tlsuser", "trace", "trace-ascii", "trace-config", "unix-socket", "upload-file",
    "url", "url-query", "user", "user-agent", "variable", "write-out",
];

/// Boolean options that do not change the request (output, verbosity, ...).
#[rustfmt::skip]
const SILENT_FLAGS: &[&str] = &[
    "anyauth", "ca-native", "compressed", "create-dirs", "disable", "fail", "fail-early",
    "fail-with-body", "help", "include", "ipv4", "ipv6", "junk-session-cookies", "manual", "parallel",
    "parallel-immediate", "path-as-is", "post301", "post302", "post303", "progress-bar", "proxy-insecure",
    "proxytunnel", "raw", "remote-header-name", "remote-name", "remote-name-all", "remote-time",
    "remove-on-error", "retry-all-errors", "retry-connrefused", "show-error", "silent", "ssl-no-revoke",
    "ssl-revoke-best-effort", "styled-output", "suppress-connect-headers", "tcp-fastopen", "tcp-nodelay",
    "tr-encoding", "trace-ids", "trace-time", "verbose", "version", "xattr",
];

fn short_option(c: char) -> Option<&'static str> {
    Some(match c {
        '0' => "http1.0",
        '1' => "tlsv1",
        '2' => "sslv2",
        '3' => "sslv3",
        '4' => "ipv4",
        '6' => "ipv6",
        '#' => "progress-bar",
        ':' => "next",
        'a' => "append",
        'A' => "user-agent",
        'b' => "cookie",
        'B' => "use-ascii",
        'c' => "cookie-jar",
        'C' => "continue-at",
        'd' => "data",
        'D' => "dump-header",
        'e' => "referer",
        'E' => "cert",
        'f' => "fail",
        'F' => "form",
        'g' => "globoff",
        'G' => "get",
        'h' => "help",
        'H' => "header",
        'i' => "include",
        'I' => "head",
        'j' => "junk-session-cookies",
        'J' => "remote-header-name",
        'k' => "insecure",
        'K' => "config",
        'l' => "list-only",
        'L' => "location",
        'm' => "max-time",
        'M' => "manual",
        'n' => "netrc",
        'N' => "no-buffer",
        'o' => "output",
        'O' => "remote-name",
        'p' => "proxytunnel",
        'P' => "ftp-port",
        'q' => "disable",
        'Q' => "quote",
        'r' => "range",
        'R' => "remote-time",
        's' => "silent",
        'S' => "show-error",
        't' => "telnet-option",
        'T' => "upload-file",
        'u' => "user",
        'U' => "proxy-user",
        'v' => "verbose",
        'V' => "version",
        'w' => "write-out",
        'x' => "proxy",
        'X' => "request",
        'y' => "speed-time",
        'Y' => "speed-limit",
        'z' => "time-cond",
        'Z' => "parallel",
        _ => return None,
    })
}

enum Data {
    Text(String),
    /// `@file`, sent as it is.
    File(String),
    /// `--data-urlencode [name]@file`, sent URL-encoded.
    EncodedFile(String),
}

/// Request under construction while reading curl's arguments.
#[derive(Default)]
struct Command {
    method: Option<String>,
    url: Option<String>,
    headers: Vec<KeyValue>,
    cookie_header: Option<usize>,
    data: Vec<Data>,
    json: bool,
    form: Vec<MultipartField>,
    upload: Option<String>,
    get: bool,
    head: bool,
    globoff: bool,
    url_query: Vec<String>,
    auth: Auth,
    /// How `-u` credentials are sent: `--basic`, `--digest` or `--ntlm` (the last one wins).
    user_auth: UserAuth,
    settings: RequestSettings,
    warnings: Vec<String>,
}

#[derive(Default, Clone, Copy)]
enum UserAuth {
    #[default]
    Basic,
    Digest,
    Ntlm,
}

impl Command {
    fn parse_args(&mut self, mut args: impl Iterator<Item = String>) {
        let mut options_done = false;
        while let Some(arg) = args.next() {
            if options_done || arg == "-" || !arg.starts_with('-') {
                self.positional(arg);
            } else if arg == "--" {
                options_done = true;
            } else if let Some(long) = arg.strip_prefix("--") {
                let (name, inline) = match long.split_once('=') {
                    Some((name, value)) => (name, Some(value.to_string())),
                    None => (long, None),
                };
                // curl 8.3 `--expand-*` variants expand `{{variables}}` in the value.
                let name = name.strip_prefix("expand-").unwrap_or(name);
                if VALUE_OPTIONS.contains(&name) {
                    match inline.or_else(|| args.next()) {
                        Some(value) => self.option(name, value),
                        None => self.warnings.push(format!("Option --{name} is missing its value")),
                    }
                } else {
                    self.flag(name, &format!("--{name}"));
                }
            } else {
                // Short options combine (`-sSL`); one that takes a value ends the group (`-XPOST`).
                let group = &arg[1..];
                for (pos, c) in group.char_indices() {
                    let display = format!("-{c}");
                    let Some(name) = short_option(c) else {
                        self.flag("", &display);
                        continue;
                    };
                    if VALUE_OPTIONS.contains(&name) {
                        let rest = &group[pos + c.len_utf8()..];
                        match if rest.is_empty() { args.next() } else { Some(rest.to_string()) } {
                            Some(value) => self.option(name, value),
                            None => self.warnings.push(format!("Option {display} is missing its value")),
                        }
                        break;
                    }
                    self.flag(name, &display);
                }
            }
        }
    }

    fn positional(&mut self, arg: String) {
        if self.url.is_none() {
            self.url = Some(arg);
        } else {
            self.warnings.push(format!("Ignored extra argument `{}`", clip(&arg)));
        }
    }

    fn flag(&mut self, name: &str, display: &str) {
        match name {
            "get" => self.get = true,
            "head" => self.head = true,
            "insecure" => self.settings.verify_tls = Some(false),
            "location" | "location-trusted" => self.settings.follow_redirects = Some(true),
            "http1.0" | "http1.1" => self.settings.http_version = Some(HttpVersionPref::Http1),
            "http2" | "http2-prior-knowledge" => self.settings.http_version = Some(HttpVersionPref::Http2),
            "http3" | "http3-only" => self.settings.http_version = Some(HttpVersionPref::Http3),
            "globoff" => self.globoff = true,
            "basic" => self.user_auth = UserAuth::Basic,
            "digest" => self.user_auth = UserAuth::Digest,
            "ntlm" => self.user_auth = UserAuth::Ntlm,
            _ if SILENT_FLAGS.contains(&name) || name.starts_with("no-") => {}
            _ => self.warnings.push(format!("Ignored unsupported option {display}")),
        }
    }

    fn option(&mut self, name: &str, value: String) {
        match name {
            "request" => self.method = Some(value),
            "url" => self.positional(value),
            "header" => self.header(&value),
            "user-agent" if !value.is_empty() => self.headers.push(KeyValue::new("User-Agent", value)),
            "referer" if !value.is_empty() => {
                let referer = value.strip_suffix(";auto").unwrap_or(&value);
                self.headers.push(KeyValue::new("Referer", referer));
            }
            "cookie" => self.cookie(value),
            "user" => {
                let (username, password) = value.split_once(':').unwrap_or((&value, ""));
                self.auth = Auth::Basic { username: username.into(), password: password.into() };
            }
            "oauth2-bearer" => self.auth = Auth::Bearer { token: value, prefix: "Bearer".into() },
            "data" | "data-ascii" | "data-binary" | "json" => {
                self.json |= name == "json";
                self.data.push(match value.strip_prefix('@') {
                    Some(path) => Data::File(path.into()),
                    None => Data::Text(value),
                });
            }
            "data-raw" => self.data.push(Data::Text(value)),
            "data-urlencode" => self.data.push(data_urlencode(&value)),
            "url-query" => match value.strip_prefix('+') {
                Some(raw) => self.url_query.push(raw.to_string()),
                None => match data_urlencode(&value) {
                    Data::Text(query) => self.url_query.push(query),
                    Data::File(path) | Data::EncodedFile(path) => {
                        self.warnings.push(format!("Could not read query file `{path}`"));
                    }
                },
            },
            "form" | "form-string" => self.form_field(&value, name == "form-string"),
            "upload-file" => self.upload = Some(value),
            "max-time" => match value.trim().parse::<f64>() {
                Ok(secs) if secs.is_finite() && secs >= 0.0 => {
                    self.settings.timeout_ms = Some((secs * 1000.0).round() as u64);
                }
                _ => self.warnings.push(format!("Ignored invalid --max-time `{}`", clip(&value))),
            },
            "max-redirs" => match value.trim().parse::<i64>() {
                Ok(n) if n >= 0 => self.settings.max_redirects = Some(u32::try_from(n).unwrap_or(u32::MAX)),
                Ok(_) => {} // -1 means unlimited
                Err(_) => self.warnings.push(format!("Ignored invalid --max-redirs `{}`", clip(&value))),
            },
            "proxy" | "preproxy" | "proxy1.0" | "socks4" | "socks4a" | "socks5" | "socks5-hostname" => {
                self.warnings.push(format!("Ignored proxy `{}`; set a proxy in Settings", clip(&value)));
            }
            "cacert" | "capath" | "cert" | "key" | "crlfile" | "pinnedpubkey" => {
                self.warnings.push(format!("Ignored --{name} `{}`; configure certificates in Settings", clip(&value)));
            }
            "aws-sigv4"
            | "config"
            | "connect-to"
            | "netrc-file"
            | "request-target"
            | "resolve"
            | "unix-socket"
            | "abstract-unix-socket"
            | "variable" => {
                self.warnings.push(format!("Ignored unsupported option --{name}"));
            }
            // Output files, timeouts, retries and similar do not affect the request itself.
            _ => {}
        }
    }

    fn header(&mut self, raw: &str) {
        if let Some(path) = raw.strip_prefix('@') {
            self.warnings.push(format!("Could not read headers file `{path}`"));
            return;
        }
        if raw.starts_with(':') {
            return; // HTTP/2 pseudo-header, derived from the URL
        }
        let (name, value) = match raw.split_once(':') {
            // `Name:` tells curl to remove a header it would add by default.
            Some((_, value)) if value.trim().is_empty() => return,
            Some((name, value)) => (name.trim(), value.trim_start()),
            // `Name;` sends the header with an empty value.
            None => (raw.trim_end().strip_suffix(';').unwrap_or_default().trim(), ""),
        };
        if name.is_empty() || name.contains(char::is_whitespace) {
            self.warnings.push(format!("Skipped malformed header `{}`", clip(raw)));
        } else {
            self.headers.push(KeyValue::new(name, value));
        }
    }

    fn cookie(&mut self, value: String) {
        if !value.contains('=') {
            // An empty value only turns on curl's cookie engine.
            if !value.is_empty() {
                self.warnings.push(format!("Ignored cookie file `{}`; only inline cookies are imported", clip(&value)));
            }
            return;
        }
        match self.cookie_header.and_then(|i| self.headers.get_mut(i)) {
            Some(header) => {
                header.value.push_str("; ");
                header.value.push_str(&value);
            }
            None => {
                self.cookie_header = Some(self.headers.len());
                self.headers.push(KeyValue::new("Cookie", value));
            }
        }
    }

    fn form_field(&mut self, spec: &str, literal: bool) {
        let Some((name, content)) = spec.split_once('=') else {
            self.warnings.push(format!("Skipped malformed form field `{}`", clip(spec)));
            return;
        };
        let (value, file, content_type) = if literal {
            (content.to_string(), false, None)
        } else if let Some(path) = content.strip_prefix('@') {
            let (path, content_type) = form_params(path);
            (path, true, content_type)
        } else if let Some(path) = content.strip_prefix('<') {
            let (path, _) = form_params(path);
            self.warnings.push(format!("Form field `{name}` reads its value from file `{path}`; enter it manually"));
            (String::new(), false, None)
        } else {
            let (value, content_type) = form_params(content);
            (value, false, content_type)
        };
        self.form.push(MultipartField { key: name.to_string(), value, file, content_type, enabled: true });
    }

    fn finish(mut self) -> Result<CurlImport, ImportError> {
        let Some(mut url) = self.url.take() else {
            return Err(ImportError::new("No URL found in the cURL command"));
        };
        if !self.globoff {
            url = unescape_glob(&url);
        }
        let has_data = !self.data.is_empty();
        let single = self.data.len() == 1;
        let mut texts = Vec::new();
        let mut body_file = None;
        for data in std::mem::take(&mut self.data) {
            match data {
                Data::Text(text) => texts.push(text),
                Data::File(path) if single && !self.get && path != "-" => body_file = Some(path),
                Data::File(path) | Data::EncodedFile(path) => {
                    self.warnings.push(format!("Could not read body file `{path}`; add its content manually"));
                }
            }
        }
        let mut text = texts.join("&");
        if let Some(path) = self.upload.take() {
            body_file = Some(path);
        }
        if let Some(path) = &body_file {
            self.warnings.push(format!("The body is read from file `{path}`; check that the path is correct"));
        }

        let mut query = std::mem::take(&mut self.url_query);
        if self.get && !text.is_empty() {
            query.push(std::mem::take(&mut text));
        }
        if !query.is_empty() {
            url.push(if url.contains('?') { '&' } else { '?' });
            url.push_str(&query.join("&"));
        }

        let method = self.method.take().unwrap_or_else(|| {
            let method = if self.head {
                "HEAD"
            } else if body_file.is_some() && !has_data {
                "PUT" // -T
            } else if (has_data && !self.get) || !self.form.is_empty() {
                "POST"
            } else {
                "GET"
            };
            method.to_string()
        });

        let content_type =
            self.headers.iter().find(|h| h.key.eq_ignore_ascii_case("content-type")).map(|h| h.value.clone());
        let body = if !self.form.is_empty() {
            if has_data {
                self.warnings.push("Ignored --data because -F form fields were also given".into());
            }
            Body { body_type: BodyType::Multipart, multipart: std::mem::take(&mut self.form), ..Body::default() }
        } else if let Some(file) = body_file {
            Body { body_type: BodyType::Binary, file, ..Body::default() }
        } else if !text.is_empty() {
            let body = text_body(text, content_type.as_deref(), self.json, &mut self.warnings);
            graphql_body(&body, &url).unwrap_or(body)
        } else {
            Body::default()
        };
        if self.json && !self.headers.iter().any(|h| h.key.eq_ignore_ascii_case("accept")) {
            self.headers.push(KeyValue::new("Accept", "application/json"));
        }

        let auth = match (self.auth, self.user_auth) {
            (Auth::Basic { username, password }, UserAuth::Digest) => Auth::Digest { username, password },
            (Auth::Basic { username, password }, UserAuth::Ntlm) => {
                Auth::Ntlm { username, password, domain: String::new(), workstation: String::new() }
            }
            (auth, _) => auth,
        };
        let name = request_name(&method, &url);
        let request = Request {
            method,
            url,
            headers: self.headers,
            body,
            auth,
            settings: self.settings,
            ..Request::new(name, RequestKind::Http)
        };
        Ok(CurlImport { request, warnings: self.warnings })
    }
}

/// Body for `--data` text: the type follows the Content-Type header, as curl sends it.
fn text_body(text: String, content_type: Option<&str>, json: bool, warnings: &mut Vec<String>) -> Body {
    let ct = content_type.unwrap_or_default().to_ascii_lowercase();
    let looks_json = text.trim_start().starts_with(['{', '[']);
    let mut body = Body::default();
    if ct.contains("json") || (ct.is_empty() && json) {
        body.body_type = BodyType::Json;
    } else if ct.contains("xml") {
        body.body_type = BodyType::Xml;
    } else if ct.is_empty() && looks_json {
        warnings.push(
            "Without a Content-Type header curl sends this JSON as application/x-www-form-urlencoded; \
             imported it as JSON"
                .into(),
        );
        body.body_type = BodyType::Json;
    } else if let Some(form) =
        parse_form(&text).filter(|_| ct.is_empty() || ct.starts_with("application/x-www-form-urlencoded"))
    {
        body.body_type = BodyType::FormUrlencoded;
        body.form = form;
    } else {
        body.body_type = BodyType::Text;
        body.content_type = Some(content_type.unwrap_or("application/x-www-form-urlencoded").to_string());
    }
    body.text = text;
    body
}

/// A JSON body that is exactly a GraphQL request (`query`, optional `variables` and
/// `operationName`, nothing else) as a GraphQL body, when it goes to a `…/graphql`
/// URL or its query starts like a GraphQL document. Anything else stays JSON.
fn graphql_body(body: &Body, url: &str) -> Option<Body> {
    use serde_json::Value;
    if body.body_type != BodyType::Json {
        return None;
    }
    let Ok(Value::Object(op)) = serde_json::from_str::<Value>(&body.text) else { return None };
    if op.keys().any(|k| !matches!(k.as_str(), "query" | "variables" | "operationName")) {
        return None;
    }
    let query = op.get("query")?.as_str()?.to_string();
    let variables = match op.get("variables") {
        None | Some(Value::Null) => String::new(),
        Some(Value::Object(o)) if o.is_empty() => String::new(),
        Some(v @ Value::Object(_)) => serde_json::to_string_pretty(v).ok()?,
        Some(_) => return None,
    };
    let operation_name = match op.get("operationName") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => Some(s.clone()).filter(|s| !s.trim().is_empty()),
        Some(_) => return None,
    };
    let path = url.split(['?', '#']).next().unwrap_or_default().trim_end_matches('/');
    if !(path.to_ascii_lowercase().ends_with("/graphql") || starts_like_graphql(&query)) {
        return None;
    }
    let graphql = GraphqlBody { query, variables, operation_name, ..Default::default() };
    Some(Body { body_type: BodyType::Graphql, graphql, ..Body::default() })
}

/// Whether `query` starts like a GraphQL document: a selection set (`{ field …`), an
/// operation (`query`/`mutation`/`subscription`, an optional name, then `(`, `@` or `{`)
/// or a fragment (`fragment Name on Type {`). Search text ("query builder", "fragment shader")
/// and JSON in a string (`{"match": …}`) are not.
fn starts_like_graphql(query: &str) -> bool {
    let name_len = |s: &str| s.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).unwrap_or(s.len());
    let mut rest = query.trim_start();
    while let Some(comment) = rest.strip_prefix('#') {
        rest = comment.split_once('\n').map_or("", |(_, next)| next).trim_start();
    }
    if let Some(selection) = rest.strip_prefix('{') {
        return selection.trim_start().starts_with(|c: char| c.is_ascii_alphabetic() || c == '_' || c == '.');
    }
    let (keyword, after) = rest.split_at(name_len(rest));
    let after = after.trim_start();
    let (name, after_name) = after.split_at(name_len(after));
    let after_name = after_name.trim_start();
    match keyword {
        "query" | "mutation" | "subscription" => after_name.starts_with(['(', '@', '{']),
        // `fragment Name on Type {`
        "fragment" => after_name.strip_prefix("on").is_some_and(|after_on| {
            let condition = after_on.trim_start();
            let (type_name, rest) = condition.split_at(name_len(condition));
            let spaced = condition.len() < after_on.len();
            !name.is_empty() && spaced && !type_name.is_empty() && rest.trim_start().starts_with(['@', '{'])
        }),
        _ => false,
    }
}

/// Decoded fields of `a=1&b=2`, or `None` if the text is not plain form data.
fn parse_form(data: &str) -> Option<Vec<KeyValue>> {
    let clean = |c: char| c == ' ' || !(c.is_whitespace() || c.is_control() || "\"<>\\^`{|}".contains(c));
    if !data.chars().all(clean) {
        return None;
    }
    data.split('&')
        .map(|pair| {
            let (key, value) = pair.split_once('=')?;
            if key.is_empty() {
                return None;
            }
            Some(KeyValue::new(form_decode(key)?, form_decode(value)?))
        })
        .collect()
}

fn form_decode(s: &str) -> Option<String> {
    let mut out = Vec::with_capacity(s.len());
    let mut bytes = s.bytes();
    while let Some(b) = bytes.next() {
        out.push(match b {
            b'+' => b' ',
            b'%' => {
                let hex = [bytes.next()?, bytes.next()?];
                let hex = std::str::from_utf8(&hex).ok().filter(|h| h.bytes().all(|b| b.is_ascii_hexdigit()))?;
                u8::from_str_radix(hex, 16).ok()?
            }
            b => b,
        });
    }
    String::from_utf8(out).ok()
}

/// URL-encode like curl's `--data-urlencode` (space as `+`).
fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => out.push(b as char),
            b' ' => out.push('+'),
            _ => {
                let _ = write!(out, "%{b:02x}");
            }
        }
    }
    out
}

/// `content`, `=content`, `name=content`, `@file` or `name@file`. As in curl, an `=` anywhere
/// makes it `name=content` (`a@b=c` is the name `a@b`); only without one does `@` name a file.
fn data_urlencode(arg: &str) -> Data {
    match arg.find('=') {
        Some(0) => Data::Text(urlencode(&arg[1..])),
        Some(i) => Data::Text(format!("{}={}", &arg[..i], urlencode(&arg[i + 1..]))),
        None => match arg.find('@') {
            Some(i) => Data::EncodedFile(arg[i + 1..].to_string()),
            None => Data::Text(urlencode(arg)),
        },
    }
}

/// Split a `-F` value (`value;type=...;filename=...`, value optionally quoted) into value and content type.
fn form_params(s: &str) -> (String, Option<String>) {
    let (value, params) = match s.strip_prefix('"') {
        Some(quoted) => {
            let mut value = String::new();
            let mut end = quoted.len();
            let mut chars = quoted.char_indices();
            while let Some((i, c)) = chars.next() {
                match c {
                    '\\' => value.extend(chars.next().map(|(_, c)| c)),
                    '"' => {
                        end = i + 1;
                        break;
                    }
                    c => value.push(c),
                }
            }
            (value, &quoted[end..])
        }
        None => {
            let cut = [";type=", ";filename=", ";headers=", ";encoder="]
                .iter()
                .filter_map(|p| s.find(p))
                .min()
                .unwrap_or(s.len());
            (s[..cut].to_string(), &s[cut..])
        }
    };
    let content_type =
        params.split(';').find_map(|p| p.trim().strip_prefix("type=")).map(|t| t.trim_matches('"').to_string());
    (value, content_type)
}

/// curl treats `[]{}` in URLs as glob patterns unless backslash-escaped.
fn unescape_glob(url: &str) -> String {
    let mut out = String::with_capacity(url.len());
    let mut chars = url.chars().peekable();
    while let Some(c) = chars.next() {
        if !(c == '\\' && matches!(chars.peek(), Some('[' | ']' | '{' | '}'))) {
            out.push(c);
        }
    }
    out
}

fn escape_glob(url: &str) -> String {
    let mut out = String::with_capacity(url.len());
    for c in url.chars() {
        if matches!(c, '[' | ']' | '{' | '}') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// `POST /api/users`, or just the host for the root path.
fn request_name(method: &str, url: &str) -> String {
    let rest = match url.split_once("://") {
        Some((scheme, rest)) if !scheme.is_empty() && scheme.chars().all(|c| c.is_ascii_alphanumeric()) => rest,
        _ => url,
    };
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let host = rest[..end].rsplit('@').next().unwrap_or_default();
    let path = rest[end..].split(['?', '#']).next().unwrap_or_default();
    match (path, host) {
        ("" | "/", "") => method.to_string(),
        ("" | "/", host) => host.to_string(),
        (path, _) => format!("{method} {path}"),
    }
}

/// Shorten user input quoted in messages.
fn clip(s: &str) -> String {
    match s.char_indices().nth(60) {
        Some((i, _)) => format!("{}…", &s[..i]),
        None => s.to_string(),
    }
}

// ---------------------------------------------------------------------------
// Quoting for to_curl
// ---------------------------------------------------------------------------

pub(crate) fn quote_bash(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

pub(crate) fn ansi_c_quote(bytes: &[u8]) -> String {
    let mut out = String::from("$'");
    for &b in bytes {
        match b {
            b'\\' => out.push_str("\\\\"),
            b'\'' => out.push_str("\\'"),
            b'\n' => out.push_str("\\n"),
            b'\r' => out.push_str("\\r"),
            b'\t' => out.push_str("\\t"),
            b' '..=b'~' => out.push(b as char),
            _ => {
                let _ = write!(out, "\\x{b:02x}");
            }
        }
    }
    out.push('\'');
    out
}

/// Escape `"` for the MSVC runtime's argument splitting: backslashes are
/// literal except in front of a quote. `quoted` means a closing quote follows.
fn crt_escape(s: &str, quoted: bool) -> String {
    let mut out = String::with_capacity(s.len());
    let mut backslashes = 0;
    for c in s.chars() {
        if c == '"' {
            out.extend(std::iter::repeat_n('\\', backslashes + 1));
        }
        backslashes = if c == '\\' { backslashes + 1 } else { 0 };
        out.push(c);
    }
    if quoted {
        out.extend(std::iter::repeat_n('\\', backslashes));
    }
    out
}

/// Chrome's "Copy as cURL (cmd)" quoting: `^"...^"` so cmd.exe never enters quote
/// mode, every metacharacter caret-escaped, and `%` followed by `^` so variables
/// cannot expand. A newline is `^` + two newlines: the first continues the line,
/// the second is the escaped (literal) character.
fn quote_cmd(s: &str) -> String {
    let s = crt_escape(s, true);
    let mut out = String::from("^\"");
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\r' if chars.peek() == Some(&'\n') => {}
            '\r' | '\n' => out.push_str("^\n\n"),
            '%' => {
                out.push_str("^%");
                if chars.peek().is_some_and(|&n| n.is_ascii_alphanumeric() || n == '_') {
                    out.push('^');
                }
            }
            c if c.is_ascii_alphanumeric() || !c.is_ascii() || " \t_-:=+~'/.,?;*\\".contains(c) => out.push(c),
            c => {
                out.push('^');
                out.push(c);
            }
        }
    }
    out.push_str("^\"");
    out
}

/// Windows PowerShell 5.1 passes arguments to native programs without escaping
/// embedded double quotes, so curl.exe would lose them; `\"` survives. (PowerShell
/// 7.3+ escapes quotes itself and passes the backslashes through.) Typographic single
/// quotes also end the string, so they are doubled like `'`.
fn quote_powershell(s: &str) -> String {
    let mut out = String::from("'");
    for c in crt_escape(s, false).chars() {
        if PS_SINGLE_QUOTES.contains(&c) {
            out.push(c);
        }
        out.push(c);
    }
    out.push('\'');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use zorvik_engine::Header;

    fn parse(input: &str) -> CurlImport {
        parse_curl(input).unwrap_or_else(|e| panic!("{e}: {input}"))
    }

    fn headers(r: &Request) -> Vec<(&str, &str)> {
        r.headers.iter().map(|h| (h.key.as_str(), h.value.as_str())).collect()
    }

    fn http(method: &str, url: &str, headers: &[(&str, &str)], body: &str) -> HttpRequest {
        HttpRequest {
            method: method.into(),
            url: url.into(),
            headers: headers.iter().map(|(n, v)| Header::new(*n, *v)).collect(),
            body: body.as_bytes().to_vec().into(),
        }
    }

    // Produced by Chrome DevTools' generateCurlCommand (current version, `--url` first).
    const CHROME_BASH: &str = r#"curl --url 'https://api.example.com/v1/search?q=a&b=%5B1%5D&arr\[\]=x' \
  -X 'PUT' \
  -H 'accept: application/json, text/plain, */*' \
  -H 'content-type: application/json' \
  -b 'sid=a%20b; theme=dark' \
  -H 'x-empty;' \
  -H 'user-agent: Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/128.0.0.0 Safari/537.36' \
  --data-raw $'{"q":"rock & roll","note":"say \\"hi\\" 100%","path":"C:\\\\tmp","tags":["a|b","<c>"],"n":"line1\nline2\u0021"}' \
  --compressed"#;

    const CHROME_CMD: &str = "curl --url ^\"https://api.example.com/v1/search?q=a^&b=^%^5B1^%^5D^&arr^\\[^\\]=x^\" ^\r
  -X ^\"PUT^\" ^\r
  -H ^\"accept: application/json, text/plain, */*^\" ^\r
  -H ^\"content-type: application/json^\" ^\r
  -b ^\"sid=a^%^20b; theme=dark^\" ^\r
  -H ^\"x-empty;^\" ^\r
  -H ^\"user-agent: Mozilla/5.0 ^(Windows NT 10.0; Win64; x64^) AppleWebKit/537.36 ^(KHTML, like Gecko^) Chrome/128.0.0.0 Safari/537.36^\" ^\r
  --data-raw ^\"^{^\\^\"q^\\^\":^\\^\"rock ^& roll^\\^\",^\\^\"note^\\^\":^\\^\"say ^\\^\\^\\^\"hi^\\^\\^\\^\" 100^%^\\^\",^\\^\"path^\\^\":^\\^\"C:^\\^\\^\\^\\tmp^\\^\",^\\^\"tags^\\^\":^[^\\^\"a^|b^\\^\",^\\^\"^<c^>^\\^\"^],^\\^\"n^\\^\":^\\^\"line1^\r
\r
line2^!^\\^\"^}^\"";

    const CHROME_BODY: &str = concat!(
        r#"{"q":"rock & roll","note":"say \"hi\" 100%","path":"C:\\tmp","tags":["a|b","<c>"],"n":"line1"#,
        "\n",
        r#"line2!"}"#
    );

    fn assert_chrome_request(import: &CurlImport) {
        let r = &import.request;
        assert_eq!(import.warnings, Vec::<String>::new());
        assert_eq!(r.method, "PUT");
        assert_eq!(r.url, "https://api.example.com/v1/search?q=a&b=%5B1%5D&arr[]=x");
        assert_eq!(r.name, "PUT /v1/search");
        assert_eq!(
            headers(r),
            [
                ("accept", "application/json, text/plain, */*"),
                ("content-type", "application/json"),
                ("Cookie", "sid=a%20b; theme=dark"),
                ("x-empty", ""),
                (
                    "user-agent",
                    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) \
                     Chrome/128.0.0.0 Safari/537.36"
                ),
            ]
        );
        assert_eq!(r.body.body_type, BodyType::Json);
        assert_eq!(r.body.text, CHROME_BODY);
    }

    #[test]
    fn chrome_bash_copy() {
        assert_chrome_request(&parse(CHROME_BASH));
    }

    #[test]
    fn chrome_cmd_copy() {
        let cmd = parse(CHROME_CMD);
        assert_chrome_request(&cmd);
        assert_eq!(cmd, parse(CHROME_BASH));
    }

    #[test]
    fn chrome_bash_copy_classic() {
        let input = r#"curl 'https://www.example.com/api/v2/orders?page=1&per_page=20' \
  -H 'authority: www.example.com' \
  -H 'accept: application/json' \
  -H 'accept-language: en-GB,en;q=0.9' \
  -H 'cache-control: no-cache' \
  -H 'content-type: application/json;charset=UTF-8' \
  -H 'cookie: _ga=GA1.2.3; session=eyJhbGciOi' \
  -H 'origin: https://www.example.com' \
  -H 'pragma: no-cache' \
  -H 'referer: https://www.example.com/orders' \
  -H 'sec-ch-ua: "Not_A Brand";v="8", "Chromium";v="120", "Google Chrome";v="120"' \
  -H 'sec-ch-ua-mobile: ?0' \
  -H 'sec-ch-ua-platform: "macOS"' \
  -H 'sec-fetch-dest: empty' \
  -H 'sec-fetch-mode: cors' \
  -H 'sec-fetch-site: same-origin' \
  -H 'user-agent: Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36' \
  --data-raw $'{"note":"Don\'t ship before 9am\\nThanks","qty":2}' \
  --compressed"#;
        let import = parse(input);
        let r = &import.request;
        assert!(import.warnings.is_empty(), "{:?}", import.warnings);
        assert_eq!(r.method, "POST");
        assert_eq!(r.name, "POST /api/v2/orders");
        assert_eq!(r.url, "https://www.example.com/api/v2/orders?page=1&per_page=20");
        assert_eq!(r.headers.len(), 16);
        assert_eq!(headers(r)[9], ("sec-ch-ua", r#""Not_A Brand";v="8", "Chromium";v="120", "Google Chrome";v="120""#));
        assert_eq!(r.body.body_type, BodyType::Json);
        assert_eq!(r.body.text, r#"{"note":"Don't ship before 9am\nThanks","qty":2}"#);
        assert_eq!((r.kind, r.seq, &r.auth), (RequestKind::Http, 0, &Auth::Inherit));
    }

    #[test]
    fn firefox_copy() {
        let input = "curl 'https://example.org/graphql' -X POST -H 'User-Agent: Mozilla/5.0 (X11; Linux x86_64; \
                     rv:128.0) Gecko/20100101 Firefox/128.0' -H 'Accept: */*' -H 'Accept-Language: en-US,en;q=0.5' \
                     -H 'Accept-Encoding: gzip, deflate, br, zstd' -H 'Content-Type: application/json' -H 'Origin: \
                     https://example.org' -H 'Connection: keep-alive' -H 'Cookie: sid=1; theme=dark' -H \
                     'Sec-Fetch-Dest: empty' -H 'Sec-Fetch-Mode: cors' -H 'Sec-Fetch-Site: same-origin' -H \
                     'Priority: u=4' -H 'TE: trailers' --data-raw '{\"query\":\"{ me { id } }\"}'";
        let import = parse(input);
        let r = &import.request;
        assert!(import.warnings.is_empty(), "{:?}", import.warnings);
        assert_eq!((r.method.as_str(), r.url.as_str()), ("POST", "https://example.org/graphql"));
        assert_eq!(r.headers.len(), 13);
        assert_eq!(headers(r)[7], ("Cookie", "sid=1; theme=dark"));
        assert_eq!(r.body.body_type, BodyType::Graphql);
        assert_eq!(r.body.graphql, GraphqlBody { query: "{ me { id } }".into(), ..Default::default() });
        assert!(r.body.text.is_empty());
    }

    #[test]
    fn graphql_bodies() {
        let r = parse(
            r#"curl https://api.example.com/v1/graphql/ -H 'Content-Type: application/json' -d '{"query":"query U($id: ID!) { user(id: $id) { name } }","variables":{"id":"7"},"operationName":"U"}'"#,
        )
        .request;
        assert_eq!(r.method, "POST");
        assert_eq!(r.body.body_type, BodyType::Graphql);
        assert_eq!(
            r.body.graphql,
            GraphqlBody {
                query: "query U($id: ID!) { user(id: $id) { name } }".into(),
                variables: "{\n  \"id\": \"7\"\n}".into(),
                operation_name: Some("U".into()),
                ..Default::default()
            }
        );
        // Any URL, when the query reads like GraphQL; null variables and operation name are dropped.
        let r = parse(
            r#"curl https://x.test/api --json '{"query":"mutation { ping }","variables":null,"operationName":null}'"#,
        )
        .request;
        assert_eq!(r.body.graphql, GraphqlBody { query: "mutation { ping }".into(), ..Default::default() });

        // Everything else stays JSON: other keys, a non-GraphQL "query", a text body.
        for input in [
            r#"curl https://x.test/graphql -H 'Content-Type: application/json' -d '{"query":"{ a }","extensions":{}}'"#,
            r#"curl https://x.test/search -H 'Content-Type: application/json' -d '{"query":"shoes"}'"#,
            r#"curl https://x.test/graphql -H 'Content-Type: application/json' -d '{"query":"{ a }","variables":[1]}'"#,
            r#"curl https://x.test/graphql -H 'Content-Type: application/json' -d '[{"query":"{ a }"}]'"#,
            r#"curl https://x.test/graphql -H 'Content-Type: application/json' -d '{"query":{"a":1}}'"#,
            r#"curl https://x.test/querying -H 'Content-Type: application/json' -d '{"query":"queryAll"}'"#,
            // A search API whose text starts with a GraphQL keyword, or JSON inside the string.
            r#"curl https://x.test/search --json '{"query":"query builder"}'"#,
            r#"curl https://x.test/search --json '{"query":"mutation testing tools"}'"#,
            r#"curl https://x.test/search --json '{"query":"fragment shader on gpu"}'"#,
            r#"curl https://x.test/search --json '{"query":"{\"match\":{\"title\":\"x\"}}"}'"#,
            r#"curl https://x.test/search --json '{"query":"{}"}'"#,
        ] {
            assert_eq!(parse(input).request.body.body_type, BodyType::Json, "{input}");
        }
        for query in [
            "query($id: ID!) { user(id: $id) { id } }",
            "query Users @cached { users { id } }",
            "subscription OnPost { post { id } }",
            "# all users\n{ users { id } }",
            "{ ...Fields } fragment Fields on Query { a }",
            "fragment F on User { id } query { me { ...F } }",
        ] {
            let body = serde_json::json!({ "query": query }).to_string();
            let r = parse(&format!("curl https://x.test/api --json '{body}'")).request;
            assert_eq!(r.body.body_type, BodyType::Graphql, "{query}");
        }
        assert!(!starts_like_graphql("query") && !starts_like_graphql("# only a comment"));
        let r = parse(r#"curl https://x.test/graphql -H 'Content-Type: application/graphql' -d '{ a }'"#).request;
        assert_eq!(r.body.body_type, BodyType::Text);
    }

    #[test]
    fn handwritten_cmd_with_caret_continuations() {
        let input = "curl -X POST \"https://api.example.com/items?a=1&b=2\" ^\n  -H \"Content-Type: application/json\" \
                     ^\n  -d \"{\\\"name\\\":\\\"Widget\\\",\\\"path\\\":\\\"C:\\\\dir\\\"}\"";
        let r = parse(input).request;
        assert_eq!(r.url, "https://api.example.com/items?a=1&b=2");
        assert_eq!(headers(&r), [("Content-Type", "application/json")]);
        assert_eq!(r.body.text, r#"{"name":"Widget","path":"C:\\dir"}"#);
    }

    #[test]
    fn powershell_backtick_continuations() {
        let input = "curl.exe -X POST `\n  -H 'Content-Type: application/json' `\n  -H \"X-Trace: `\"abc`\"\" `\n  \
                     -d '{\\\"a\\\":\\\"it''s\\\"}' `\n  https://api.example.com/items";
        let r = parse(input).request;
        assert_eq!((r.method.as_str(), r.url.as_str()), ("POST", "https://api.example.com/items"));
        assert_eq!(headers(&r), [("Content-Type", "application/json"), ("X-Trace", "\"abc\"")]);
        assert_eq!(r.body.text, r#"{"a":"it's"}"#);
    }

    #[test]
    fn posix_quoting() {
        let input = "$ curl \"https://x.test/p?q=\\$HOME\" \\\n    -H \"X-A: say \\\"hi\\\" \\\\ \\z\" \\\n    \
                     -H $'X-B: \\x41\\u00e9\\101\\t!' \\\n    -d 'a'\"b\"'c' # trailing comment";
        let import = parse(input);
        let r = &import.request;
        assert!(import.warnings.is_empty(), "{:?}", import.warnings);
        assert_eq!(r.url, "https://x.test/p?q=$HOME");
        assert_eq!(headers(r), [("X-A", r#"say "hi" \ \z"#), ("X-B", "AéA\t!")]);
        assert_eq!(r.body.text, "abc");
        assert_eq!(r.body.body_type, BodyType::Text);
        assert_eq!(r.body.content_type.as_deref(), Some("application/x-www-form-urlencoded"));
    }

    #[test]
    fn crlf_input_and_program_names() {
        let r = parse("curl 'https://x.test/' \\\r\n  -H 'A: 1' \\\r\n  -d 'k=v'\r\n").request;
        assert_eq!(headers(&r), [("A", "1")]);
        assert_eq!(r.body.form, [KeyValue::new("k", "v")]);
        for program in ["curl.exe", "CURL.EXE", "/usr/bin/curl"] {
            assert_eq!(parse(&format!("{program} https://x.test")).request.url, "https://x.test");
        }
    }

    #[test]
    fn only_first_command_is_imported() {
        let import = parse("curl -s https://x.test/api | jq .");
        assert_eq!(import.request.url, "https://x.test/api");
        assert_eq!(import.warnings, ["Only the first command was imported; ignored `| jq .`"]);
        let import = parse("curl ^\"https://x.test/a^\" &\r\ncurl ^\"https://x.test/b^\"");
        assert_eq!(import.request.url, "https://x.test/a");
        assert_eq!(import.warnings.len(), 1);
        // An unquoted `&` inside a URL is kept.
        assert_eq!(parse("curl https://x.test/?a=1&b=2").request.url, "https://x.test/?a=1&b=2");
    }

    #[test]
    fn basic_and_bearer_auth() {
        let auth = |input: &str| parse(input).request.auth;
        let basic = |u: &str, p: &str| Auth::Basic { username: u.into(), password: p.into() };
        assert_eq!(auth("curl -u alice:s3cr:et https://x.test"), basic("alice", "s3cr:et"));
        assert_eq!(auth("curl --user bob https://x.test"), basic("bob", ""));
        assert_eq!(auth("curl --user=carol:pw https://x.test"), basic("carol", "pw"));
        assert_eq!(
            auth("curl --oauth2-bearer tok https://x.test"),
            Auth::Bearer { token: "tok".into(), prefix: "Bearer".into() }
        );
    }

    #[test]
    fn digest_and_ntlm_auth() {
        let import = parse("curl --digest -u alice:pw https://x.test");
        assert!(import.warnings.is_empty(), "{:?}", import.warnings);
        assert_eq!(import.request.auth, Auth::Digest { username: "alice".into(), password: "pw".into() });
        let ntlm = |username: &str| Auth::Ntlm {
            username: username.into(),
            password: "pw".into(),
            domain: String::new(),
            workstation: String::new(),
        };
        assert_eq!(parse(r"curl -u 'CORP\bob:pw' --ntlm https://x.test").request.auth, ntlm(r"CORP\bob"));
        // The last of --basic, --digest and --ntlm wins.
        assert_eq!(
            parse("curl --ntlm --basic -u carol:pw https://x.test").request.auth,
            Auth::Basic { username: "carol".into(), password: "pw".into() }
        );
        assert_eq!(parse("curl --digest --ntlm -u bob:pw https://x.test").request.auth, ntlm("bob"));
        // Without -u there are no credentials to send.
        assert_eq!(parse("curl --digest https://x.test").request.auth, Auth::Inherit);
    }

    #[test]
    fn multipart_form() {
        let input = "curl https://x.test/upload -F 'name=John Doe' -F 'avatar=@/tmp/me.png;type=image/png' \
                     -F 'doc=@\"my;file.txt\";filename=x.txt' --form-string 'raw=@literal;type=x' -F 'bio=<bio.txt'";
        let import = parse(input);
        let r = &import.request;
        assert_eq!(r.method, "POST");
        assert_eq!(r.body.body_type, BodyType::Multipart);
        let field = |key: &str, value: &str, file: bool, content_type: Option<&str>| MultipartField {
            key: key.into(),
            value: value.into(),
            file,
            content_type: content_type.map(Into::into),
            enabled: true,
        };
        assert_eq!(
            r.body.multipart,
            [
                field("name", "John Doe", false, None),
                field("avatar", "/tmp/me.png", true, Some("image/png")),
                field("doc", "my;file.txt", true, None),
                field("raw", "@literal;type=x", false, None),
                field("bio", "", false, None),
            ]
        );
        assert_eq!(import.warnings, ["Form field `bio` reads its value from file `bio.txt`; enter it manually"]);
    }

    #[test]
    fn get_moves_data_to_query() {
        let r = parse("curl -G https://x.test/search?lang=en -d q=rust -d page=2 --data-urlencode 'tag=a b&c'").request;
        assert_eq!(r.method, "GET");
        assert_eq!(r.url, "https://x.test/search?lang=en&q=rust&page=2&tag=a+b%26c");
        assert_eq!(r.body, Body::default());
        assert_eq!(r.name, "GET /search");
        let r = parse("curl --get --url-query 'x=1 2' --url-query '+raw=a b' https://x.test/").request;
        assert_eq!(r.url, "https://x.test/?x=1+2&raw=a b");
    }

    #[test]
    fn data_urlencode_forms() {
        let r = parse("curl -G https://x.test --data-urlencode 'a b' --data-urlencode '=c&d' --data-urlencode 'n=é/?'")
            .request;
        assert_eq!(r.url, "https://x.test?a+b&c%26d&n=%c3%a9%2f%3f");
        let import = parse("curl https://x.test --data-urlencode 'msg=hello world' --data-urlencode 'n=é/?'");
        let body = &import.request.body;
        assert_eq!(body.text, "msg=hello+world&n=%c3%a9%2f%3f");
        assert_eq!(body.body_type, BodyType::FormUrlencoded);
        assert_eq!(body.form, [KeyValue::new("msg", "hello world"), KeyValue::new("n", "é/?")]);
        let import = parse("curl https://x.test -d a=1 --data-urlencode @notes.txt");
        assert_eq!(import.warnings, ["Could not read body file `notes.txt`; add its content manually"]);
        assert_eq!(import.request.body.text, "a=1");
        // The file's content would be sent URL-encoded, so it is not a binary body.
        let import = parse("curl https://x.test --data-urlencode msg@notes.txt");
        assert_eq!((import.request.method.as_str(), &import.request.body), ("POST", &Body::default()));
        assert_eq!(import.warnings, ["Could not read body file `notes.txt`; add its content manually"]);
        // An `=` anywhere makes it `name=content`, even after an `@` (as in curl).
        let import = parse("curl https://x.test --data-urlencode 'a@b=c d' --data-urlencode 'to=me@x.test'");
        assert!(import.warnings.is_empty(), "{:?}", import.warnings);
        assert_eq!(import.request.body.text, "a@b=c+d&to=me%40x.test");
        let r = parse("curl -G https://x.test --url-query 'mail@home=a@b'").request;
        assert_eq!(r.url, "https://x.test?mail@home=a%40b");
    }

    #[test]
    fn settings_flags() {
        let s = parse("curl -k -L --max-redirs 3 -m 2.5 --http2 --connect-timeout 5 https://x.test").request.settings;
        assert_eq!(s.verify_tls, Some(false));
        assert_eq!(s.follow_redirects, Some(true));
        assert_eq!(s.max_redirects, Some(3));
        assert_eq!(s.timeout_ms, Some(2500));
        assert_eq!(s.http_version, Some(HttpVersionPref::Http2));
        let s = parse("curl --http1.1 --max-time=0.25 https://x.test").request.settings;
        assert_eq!((s.http_version, s.timeout_ms), (Some(HttpVersionPref::Http1), Some(250)));
        let s = parse("curl --http2-prior-knowledge http://x.test").request.settings;
        assert_eq!(s.http_version, Some(HttpVersionPref::Http2));
        let s = parse("curl --http3-only https://x.test").request.settings;
        assert_eq!(s.http_version, Some(HttpVersionPref::Http3));
        assert_eq!(parse("curl https://x.test").request.settings, RequestSettings::default());
    }

    #[test]
    fn combined_short_flags() {
        let import = parse("curl -sSLk -XPOST -HAccept:text/plain -dfoo=bar -m5 https://x.test/api");
        let r = &import.request;
        assert!(import.warnings.is_empty(), "{:?}", import.warnings);
        assert_eq!(r.method, "POST");
        assert_eq!((r.settings.follow_redirects, r.settings.verify_tls), (Some(true), Some(false)));
        assert_eq!(r.settings.timeout_ms, Some(5000));
        assert_eq!(headers(r), [("Accept", "text/plain")]);
        assert_eq!(r.body.form, [KeyValue::new("foo", "bar")]);
    }

    #[test]
    fn unknown_and_ignored_flags() {
        let import = parse(
            "curl --frobnicate -9 --tlsv1.2 --foo=bar -x http://proxy:8080 -o out.json --retry 3 -w '%{http_code}' \
             --cacert ca.pem https://x.test",
        );
        assert_eq!(import.request.url, "https://x.test");
        assert_eq!(
            import.warnings,
            [
                "Ignored unsupported option --frobnicate",
                "Ignored unsupported option -9",
                "Ignored unsupported option --tlsv1.2",
                "Ignored unsupported option --foo",
                "Ignored proxy `http://proxy:8080`; set a proxy in Settings",
                "Ignored --cacert `ca.pem`; configure certificates in Settings",
            ]
        );
        let import = parse("curl https://x.test https://y.test -H");
        assert_eq!(import.warnings, ["Ignored extra argument `https://y.test`", "Option -H is missing its value"]);
    }

    #[test]
    fn errors() {
        for (input, message) in [
            ("", "Paste a cURL command to import"),
            ("  \n\t ", "Paste a cURL command to import"),
            ("$", "Paste a cURL command to import"),
            ("wget https://x.test", "Not a cURL command: it should start with `curl`, not `wget`"),
            ("https://x.test", "Not a cURL command: it should start with `curl`, not `https://x.test`"),
            ("curl -X POST -H 'A: b'", "No URL found in the cURL command"),
            ("curl", "No URL found in the cURL command"),
        ] {
            assert_eq!(parse_curl(input).unwrap_err().to_string(), message, "{input:?}");
        }
    }

    #[test]
    fn method_rules() {
        let method = |input: &str| parse(input).request.method;
        assert_eq!(method("curl https://x.test"), "GET");
        assert_eq!(method("curl -I https://x.test"), "HEAD");
        assert_eq!(method("curl -d x=1 https://x.test"), "POST");
        assert_eq!(method("curl -F a=1 https://x.test"), "POST");
        assert_eq!(method("curl -X PATCH -d x=1 https://x.test"), "PATCH");
        assert_eq!(method("curl -X GET -d x=1 https://x.test"), "GET");
        let import = parse("curl -T ./file.bin https://x.test/up");
        assert_eq!(import.request.method, "PUT");
        assert_eq!(
            (import.request.body.body_type, import.request.body.file.as_str()),
            (BodyType::Binary, "./file.bin")
        );
        assert_eq!(import.warnings.len(), 1);
    }

    #[test]
    fn body_types() {
        let import = parse(r#"curl https://x.test -d '{"a":1}'"#);
        assert_eq!(import.request.body.body_type, BodyType::Json);
        assert!(import.request.headers.is_empty());
        assert_eq!(import.warnings.len(), 1);

        let import = parse(r#"curl https://x.test -H 'Content-Type: application/json' -d '[1]'"#);
        assert_eq!((import.request.body.body_type, import.request.headers.len()), (BodyType::Json, 1));
        assert!(import.warnings.is_empty());

        let body = |input: &str| parse(input).request.body;
        assert_eq!(body("curl https://x.test -H 'Content-Type: application/xml' -d '<a/>'").body_type, BodyType::Xml);
        let text = body("curl https://x.test -H 'content-type: text/plain' -d 'hi there'");
        assert_eq!((text.body_type, text.content_type.as_deref()), (BodyType::Text, Some("text/plain")));
        let form = body("curl https://x.test -H 'Content-Type: application/x-www-form-urlencoded' -d 'a=1&b=x%20y+z'");
        assert_eq!(form.form, [KeyValue::new("a", "1"), KeyValue::new("b", "x y z")]);
        for data in ["not form data", "a=%zz", "a=%ff", "=1", "a=1&&b=2", "a=\"q\""] {
            let b = body(&format!("curl https://x.test --data-raw '{data}'"));
            assert_eq!(b.body_type, BodyType::Text, "{data}");
            assert_eq!(b.text, data);
        }
        let json = parse(r#"curl https://x.test --json '{"a":1}'"#).request;
        assert_eq!(json.body.body_type, BodyType::Json);
        assert_eq!(headers(&json), [("Accept", "application/json")]);
        assert_eq!(body("curl https://x.test -d a=1 -d b=2").form, [KeyValue::new("a", "1"), KeyValue::new("b", "2")]);
    }

    #[test]
    fn data_file_references() {
        let import = parse("curl https://x.test -H 'Content-Type: application/json' -d @body.json");
        assert_eq!(import.request.method, "POST");
        assert_eq!((import.request.body.body_type, import.request.body.file.as_str()), (BodyType::Binary, "body.json"));
        assert_eq!(import.warnings, ["The body is read from file `body.json`; check that the path is correct"]);
        let import = parse("curl https://x.test --data-raw @literal");
        assert_eq!(import.request.body.text, "@literal");
        assert!(import.warnings.is_empty());
    }

    #[test]
    fn header_forms() {
        let import = parse(
            "curl https://x.test -H 'X-Empty;' -H 'Accept:' -H 'bogus' -H ':authority: x.test' -H 'X-Spaced:   v  ' \
             -A 'agent/1.0' -e 'https://ref/;auto' -b 'a=1' -b 'b=2' -b cookies.txt",
        );
        assert_eq!(
            headers(&import.request),
            [
                ("X-Empty", ""),
                ("X-Spaced", "v  "),
                ("User-Agent", "agent/1.0"),
                ("Referer", "https://ref/"),
                ("Cookie", "a=1; b=2"),
            ]
        );
        assert_eq!(
            import.warnings,
            ["Skipped malformed header `bogus`", "Ignored cookie file `cookies.txt`; only inline cookies are imported"]
        );
    }

    #[test]
    fn url_globbing() {
        assert_eq!(parse(r"curl 'https://x.test/?a\[0\]=1&b=\{x\}'").request.url, "https://x.test/?a[0]=1&b={x}");
        assert_eq!(parse(r"curl -g 'https://x.test/?a\[0\]=1'").request.url, r"https://x.test/?a\[0\]=1");
    }

    #[test]
    fn request_names() {
        let name = |input: &str| parse(input).request.name;
        assert_eq!(name("curl -X POST 'https://api.x.test/api/users?id=1'"), "POST /api/users");
        assert_eq!(name("curl https://api.x.test/"), "api.x.test");
        assert_eq!(name("curl https://user:pw@api.x.test:8443"), "api.x.test:8443");
        assert_eq!(name("curl 'https://x.test?next=http://y.test/z'"), "x.test");
        let r = parse("curl localhost:3000/health").request;
        assert_eq!((r.name.as_str(), r.url.as_str()), ("GET /health", "localhost:3000/health"));
    }

    #[test]
    fn never_panics() {
        let samples = [
            CHROME_BASH,
            CHROME_CMD,
            "curl.exe -X POST `\n -H \"a: `\"b`\"\" `\n -d 'x''y' https://x",
            "curl -F 'a=@\"x\\\";type=y' -F 'b=<\"' $'\\x\\u\\U\\c\\777\\q\\",
        ];
        for sample in samples {
            for (i, _) in sample.char_indices() {
                let _ = parse_curl(&sample[..i]);
            }
        }
        for input in [
            "curl ^",
            "curl `",
            "curl -",
            "curl --",
            "curl -H",
            "curl '",
            "curl \"\\",
            "curl $'\\c",
            "curl -F =",
            "curl -é",
            "curl -Hé",
            "curl x -d '%'",
            "curl x -d a=%e",
            "curl ^\"\\^\"",
            "curl \"\"\"",
            "curl -- -x",
            "curl x -m nan --max-redirs -1 -m -2",
            "curl x --max-redirs 99999999999",
        ] {
            let _ = parse_curl(input);
        }
    }

    fn round_trip_cases() -> Vec<HttpRequest> {
        let json = [("Content-Type", "application/json")];
        vec![
            http("GET", "https://example.com/", &[], ""),
            http(
                "GET",
                "https://example.com/a%20b/?filter[name]=x&q={y}&bang=!&pct=%26&dollar=$HOME",
                &[
                    ("X-Quote", r#"it's "quoted""#),
                    ("X-Empty", ""),
                    ("X-Caret", "a^b|c<d>e&f (g)"),
                    ("X-Percent", "%PATH% 100% %% %^"),
                    ("X-Name", "José 世界"),
                    // PowerShell ends single-quoted strings at these too.
                    ("X-Smart", "it\u{2019}s \u{2018}q\u{2019} \u{201a}low\u{201b}; echo pwned"),
                ],
                "",
            ),
            http(
                "POST",
                "https://example.com/api?x=1&y=2",
                &json,
                "{\"msg\":\"héllo 世界 🎉\",\"q\":\"a & b | c\",\"say\":\"\\\"hi\\\"\",\"esc\":\"C:\\\\dir\\\\\",\
                 \"pct\":\"100% %TEMP%\",\"bang\":\"wow!\",\"dollar\":\"$HOME `id` $(x)\"}\n",
            ),
            http(
                "PUT",
                "https://example.com/t",
                &[("Content-Type", "text/plain")],
                r#"back\\slash\\\"quote\" trailing\"#,
            ),
            http("GET", "https://example.com/search", &[], "x"),
            http("HEAD", "https://example.com/", &[("Accept", "*/*")], ""),
            http("DELETE", "https://example.com/items/1", &[], ""),
            http("POST", "https://example.com/form", &[], "a=1&b=two%20words"),
            http("POST", "https://example.com/at", &[], "@not-a-file"),
            http("PATCH", "https://example.com/p", &[("Content-Type", "text/plain")], "line1\n\n\tline3\n"),
            http("M-SEARCH", "http://239.255.255.250:1900/", &[("MAN", "\"ssdp:discover\"")], ""),
        ]
    }

    #[test]
    fn round_trip() {
        for flavor in [CurlFlavor::Bash, CurlFlavor::Cmd, CurlFlavor::PowerShell] {
            for req in round_trip_cases() {
                let command = to_curl(&req, flavor);
                let r = parse_curl(&command).unwrap_or_else(|e| panic!("{e}\n{command}")).request;
                let context = format!("{flavor:?}\n{command}");
                assert_eq!(r.method, req.method, "{context}");
                assert_eq!(r.url, req.url, "{context}");
                let expected: Vec<_> = req.headers.iter().map(|h| (h.name.as_str(), h.value.as_str())).collect();
                assert_eq!(headers(&r), expected, "{context}");
                assert_eq!(r.body.text.as_bytes(), &req.body[..], "{context}");
            }
        }
    }

    #[test]
    fn bash_output() {
        let req = http(
            "POST",
            "https://api.example.com/items?a=1&b=2",
            &[("Content-Type", "application/json"), ("X-Empty", "")],
            r#"{"name":"it's"}"#,
        );
        assert_eq!(
            to_curl(&req, CurlFlavor::Bash),
            r#"curl 'https://api.example.com/items?a=1&b=2' \
  -X POST \
  -H 'Content-Type: application/json' \
  -H 'X-Empty;' \
  --data-raw '{"name":"it'\''s"}'"#
        );
        assert_eq!(
            to_curl(&http("GET", "https://x.test/?a[0]=1", &[], ""), CurlFlavor::Bash),
            r"curl 'https://x.test/?a\[0\]=1'"
        );
        // `-X HEAD` would make curl wait for a body.
        assert_eq!(
            to_curl(&http("HEAD", "https://x.test/", &[], ""), CurlFlavor::Bash),
            "curl 'https://x.test/' \\\n  -I"
        );
        // Bytes go through base64: bash would cut an argument short at the NUL byte.
        let binary =
            HttpRequest { body: vec![0xff, 0, b'a', b'\'', b'\n'].into(), ..http("PUT", "https://x.test", &[], "") };
        assert_eq!(
            to_curl(&binary, CurlFlavor::Bash),
            "printf '%s' '/wBhJwo=' | base64 -d | curl 'https://x.test' \\\n  -X PUT \\\n  --data-binary @-"
        );
        // So does text with a NUL byte.
        assert_eq!(
            to_curl(&http("POST", "https://x.test", &[], "a\0b"), CurlFlavor::Bash),
            "printf '%s' 'YQBi' | base64 -d | curl 'https://x.test' \\\n  -X POST \\\n  --data-binary @-"
        );
    }

    #[test]
    fn binary_bodies_keep_every_byte() {
        let body: Vec<u8> = (0..=255).chain(0..=255).collect();
        let req = HttpRequest { body: body.clone().into(), ..http("PUT", "https://x.test/up", &[], "") };
        let base64 = base64::engine::general_purpose::STANDARD.encode(&body);

        let bash = to_curl(&req, CurlFlavor::Bash);
        let piped = bash.strip_prefix("printf '%s' '").and_then(|rest| rest.split_once("' | base64 -d | curl "));
        assert_eq!(piped.map(|(sent, _)| sent), Some(base64.as_str()), "{bash}");

        // cmd writes the base64 to a file with `echo` (a line holds at most 8191 characters).
        let cmd = to_curl(&req, CurlFlavor::Cmd);
        let lines: Vec<&str> = cmd.lines().collect();
        assert_eq!(lines[0], "(");
        let echoed: String = lines.iter().filter_map(|l| l.strip_prefix("echo ")).collect();
        assert_eq!(echoed, base64);
        assert!(lines.iter().all(|l| l.len() < 100), "{cmd}");
        assert!(
            cmd.ends_with(
                ") > \"%TEMP%\\zorvik-body.b64\"\n\
                 certutil -f -decode \"%TEMP%\\zorvik-body.b64\" \"%TEMP%\\zorvik-body\" > nul\n\
                 curl ^\"https://x.test/up^\" ^\n  -X PUT ^\n  --data-binary \"@%TEMP%\\zorvik-body\"\n\
                 del \"%TEMP%\\zorvik-body.b64\" \"%TEMP%\\zorvik-body\""
            ),
            "{cmd}"
        );

        let small = HttpRequest { body: vec![0xff, 0].into(), ..http("PUT", "https://x.test/up", &[], "") };
        assert_eq!(
            to_curl(&small, CurlFlavor::PowerShell),
            "$body = [IO.Path]::GetTempFileName()\n\
             [IO.File]::WriteAllBytes($body, [Convert]::FromBase64String('/wA='))\n\
             curl.exe 'https://x.test/up' `\n  -X PUT `\n  --data-binary \"@$body\"\nRemove-Item $body"
        );
    }

    #[test]
    fn cmd_output() {
        let req = http(
            "POST",
            "https://api.example.com/items?a=1&b=2",
            &[("Content-Type", "application/json"), ("X-Env", "%PATH% 5%")],
            "{\"name\":\"it's\"}\n!",
        );
        assert_eq!(
            to_curl(&req, CurlFlavor::Cmd),
            "curl ^\"https://api.example.com/items?a=1^&b=2^\" ^\n  -X POST ^\n  -H ^\"Content-Type: application/json^\" \
             ^\n  -H ^\"X-Env: ^%^PATH^% 5^%^\" ^\n  --data-raw ^\"^{\\^\"name\\^\":\\^\"it's\\^\"^}^\n\n^!^\""
        );
    }

    #[test]
    fn powershell_output() {
        let req = http(
            "POST",
            "https://api.example.com/items?a=1&b=2",
            &[("Content-Type", "application/json"), ("X-Empty", "")],
            r#"{"name":"it's","path":"C:\\"}"#,
        );
        assert_eq!(
            to_curl(&req, CurlFlavor::PowerShell),
            r#"curl.exe 'https://api.example.com/items?a=1&b=2' `
  -X POST `
  -H 'Content-Type: application/json' `
  -H 'X-Empty;' `
  --data-raw '{\"name\":\"it''s\",\"path\":\"C:\\\\\"}'"#
        );
        let req = http("GET", "https://x.test/", &[("X-Note", "it\u{2019}s")], "");
        assert_eq!(
            to_curl(&req, CurlFlavor::PowerShell),
            "curl.exe 'https://x.test/' `\n  -H 'X-Note: it\u{2019}\u{2019}s'"
        );
    }
}
