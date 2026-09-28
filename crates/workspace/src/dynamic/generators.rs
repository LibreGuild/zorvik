//! The generators behind the catalog that need more than a pick from one list.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex};

use base64::Engine as _;
use rand::seq::SliceRandom as _;
use rand::{Rng as _, RngExt as _};
use sha2::Digest as _;
use time::{Date, OffsetDateTime};

use super::data::*;
use super::{Ctx, MAX_DAYS, MAX_LEN, MAX_WORDS};

const LOWER_DIGITS: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
const ALPHANUMERIC: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
const PASSWORD_CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789!@#$%^&*-_=+?";
const SYMBOLS: &[u8] = b"!@#$%^&*-_=+?";
const HEX: &[u8] = b"0123456789abcdef";
const DIGITS: &[u8] = b"0123456789";
const NANOID: &[u8] = b"useandom-26T198340PX75pxJACKVERYMINDBUSHWOLF_GQZbfghjklqvwyzrict";
/// Crockford's base 32, as used by ULIDs.
const CROCKFORD: &[u8] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
const BASE58: &[u8] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
/// Twitter's snowflake epoch (2010-11-04T01:42:54.657Z) in Unix milliseconds.
const SNOWFLAKE_EPOCH_MS: u64 = 1_288_834_974_657;

// ── IDs ──────────────────────────────────────────────────────────────────────

pub(super) fn uuid_v4(_: &mut Ctx) -> Option<String> {
    Some(uuid::Uuid::new_v4().to_string())
}

/// The last UUID v7 and ULID handed out, so ones made in the same millisecond still sort in order.
static LAST_UUID_V7: Mutex<u128> = Mutex::new(0);
static LAST_ULID: Mutex<u128> = Mutex::new(0);

/// `ms` above `random_bits` random bits; when that is not above the last value, the last value plus one.
fn monotonic(last: &Mutex<u128>, c: &mut Ctx, random_bits: u32) -> u128 {
    let random = c.rng.random::<u128>() & ((1 << random_bits) - 1);
    let candidate = (u128::from(unix_ms(c.now)) << random_bits) | random;
    let mut last = last.lock().unwrap_or_else(|e| e.into_inner());
    *last = if candidate > *last { candidate } else { *last + 1 };
    *last
}

fn unix_ms(t: OffsetDateTime) -> u64 {
    u64::try_from(t.unix_timestamp_nanos() / 1_000_000).unwrap_or(0)
}

/// RFC 9562 UUID v7: 48-bit Unix milliseconds, version, 74 bits that count up within a millisecond.
pub(super) fn uuid_v7(c: &mut Ctx) -> Option<String> {
    let v = monotonic(&LAST_UUID_V7, c, 74);
    let (ms, rand_a, rand_b) = (v >> 74, (v >> 62) & 0xfff, v & ((1 << 62) - 1));
    let bits = (ms << 80) | (0x7 << 76) | (rand_a << 64) | (0b10 << 62) | rand_b;
    Some(uuid::Uuid::from_u128(bits).to_string())
}

/// ULID: 48-bit Unix milliseconds and 80 random bits in Crockford base 32 (monotonic).
pub(super) fn ulid(c: &mut Ctx) -> Option<String> {
    let v = monotonic(&LAST_ULID, c, 80);
    Some((0..26).rev().map(|i| CROCKFORD[((v >> (i * 5)) & 31) as usize] as char).collect())
}

pub(super) fn nanoid(c: &mut Ctx) -> Option<String> {
    let n = c.count(0, 21, MAX_LEN)?;
    Some(c.chars(NANOID, n))
}

/// MongoDB ObjectId: creation time in seconds, a per-process random value, a counter.
pub(super) fn object_id(c: &mut Ctx) -> Option<String> {
    static PROCESS: LazyLock<[u8; 5]> = LazyLock::new(|| {
        let mut bytes = [0; 5];
        rand::rng().fill_bytes(&mut bytes);
        bytes
    });
    static COUNTER: LazyLock<AtomicU32> = LazyLock::new(|| AtomicU32::new(rand::random()));
    let seconds = u32::try_from(c.now.unix_timestamp()).unwrap_or(0);
    let count = COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut bytes = [0; 12];
    bytes[..4].copy_from_slice(&seconds.to_be_bytes());
    bytes[4..9].copy_from_slice(&*PROCESS);
    bytes[9..].copy_from_slice(&count.to_be_bytes()[1..]);
    Some(hex(&bytes))
}

/// Twitter-style snowflake: milliseconds since the Twitter epoch, a 10-bit worker, a 12-bit sequence.
pub(super) fn snowflake(c: &mut Ctx) -> Option<String> {
    static WORKER: LazyLock<u64> = LazyLock::new(|| rand::random::<u64>() & 0x3ff);
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let ms = unix_ms(c.now).saturating_sub(SNOWFLAKE_EPOCH_MS);
    let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed) & 0xfff;
    Some(((ms << 22) | (*WORKER << 12) | sequence).to_string())
}

/// W3C Trace Context: version 00, trace ID, parent span ID, sampled.
pub(super) fn traceparent(c: &mut Ctx) -> Option<String> {
    let trace = c.rng.random_range(1..=u128::MAX);
    let span = c.rng.random_range(1..=u64::MAX);
    Some(format!("00-{trace:032x}-{span:016x}-01"))
}

pub(super) fn random_hex(c: &mut Ctx) -> Option<String> {
    let n = c.count(0, 32, MAX_LEN)?;
    Some(c.chars(HEX, n))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

// ── Numbers ──────────────────────────────────────────────────────────────────

/// A random integer in `min..=max` (arguments 0 and 1), or the defaults.
fn int_range(c: &mut Ctx, min: i64, max: i64) -> Option<i64> {
    let (lo, hi) = (c.int(0, min)?, c.int(1, max)?);
    (lo <= hi).then(|| c.between(lo, hi))
}

pub(super) fn random_int(c: &mut Ctx) -> Option<String> {
    int_range(c, 0, 1000).map(|n| n.to_string())
}

pub(super) fn random_float(c: &mut Ctx) -> Option<String> {
    let (lo, hi) = (c.float(0, 0.0)?, c.float(1, 1000.0)?);
    let decimals = c.int_in(2, 2, 0, 10)? as usize;
    if lo > hi || !(hi - lo).is_finite() {
        return None;
    }
    let x = if lo == hi { lo } else { c.rng.random_range(lo..=hi) };
    Some(format!("{x:.decimals$}"))
}

pub(super) fn digits(c: &mut Ctx) -> Option<String> {
    let n = c.count(0, 6, MAX_LEN)?;
    Some(c.chars(DIGITS, n))
}

// ── Text ─────────────────────────────────────────────────────────────────────

pub(super) fn alphanumeric(c: &mut Ctx) -> Option<String> {
    let n = c.count(0, 1, MAX_LEN)?;
    Some(c.chars(LOWER_DIGITS, n))
}

pub(super) fn string(c: &mut Ctx) -> Option<String> {
    let n = c.count(0, 16, MAX_LEN)?;
    Some(c.chars(ALPHANUMERIC, n))
}

pub(super) fn password(c: &mut Ctx) -> Option<String> {
    let n = c.count(0, 15, MAX_LEN)?;
    Some(c.chars(ALPHANUMERIC, n))
}

/// At least one lower-case letter, upper-case letter, digit and symbol.
pub(super) fn strong_password(c: &mut Ctx) -> Option<String> {
    let n = c.count(0, 16, MAX_LEN).filter(|n| *n >= 4)?;
    let classes: [&[u8]; 4] = [&ALPHANUMERIC[26..52], &ALPHANUMERIC[..26], DIGITS, SYMBOLS];
    let mut password: Vec<u8> = classes.iter().map(|class| c.pick(class)).collect();
    password.extend((4..n).map(|_| c.pick(PASSWORD_CHARS)));
    password.shuffle(&mut c.rng);
    String::from_utf8(password).ok()
}

pub(super) fn random_base64(c: &mut Ctx) -> Option<String> {
    let mut bytes = vec![0; c.count(0, 16, MAX_LEN)?];
    c.rng.fill_bytes(&mut bytes);
    Some(base64::engine::general_purpose::STANDARD.encode(bytes))
}

/// Characters from every pool (accents, Greek and Cyrillic, CJK, right-to-left,
/// Indic and Thai, emoji), each pool at least once when the length allows.
pub(super) fn unicode_string(c: &mut Ctx) -> Option<String> {
    static POOLS: LazyLock<Vec<Vec<char>>> =
        LazyLock::new(|| UNICODE_POOLS.iter().map(|p| p.chars().collect()).collect());
    let n = c.count(0, 16, MAX_LEN)?;
    let mut chars: Vec<char> = (0..n)
        .map(|i| {
            let pool = &POOLS[if i < POOLS.len() { i } else { c.below(POOLS.len()) }];
            pool[c.below(pool.len())]
        })
        .collect();
    chars.shuffle(&mut c.rng);
    Some(chars.into_iter().collect())
}

pub(super) fn random_from(c: &mut Ctx) -> Option<String> {
    if c.args.is_empty() {
        return None;
    }
    let i = c.below(c.args.len());
    Some(c.args[i].clone())
}

pub(super) fn slug(c: &mut Ctx) -> Option<String> {
    let n = c.count(0, 3, MAX_WORDS)?;
    Some((0..n).map(|_| c.pick(&WORDS).to_lowercase()).collect::<Vec<_>>().join("-"))
}

// ── Words and lorem ipsum ────────────────────────────────────────────────────

/// Single words for `$randomWord` and friends.
static WORDS: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
    [NOUNS, ADJECTIVES, VERBS, BS_NOUNS, BS_BUZZ, CATCH_NOUNS, PRODUCTS, ABBREVIATIONS]
        .concat()
        .into_iter()
        .filter(|w| w.chars().all(|ch| ch.is_ascii_alphanumeric()))
        .collect()
});

pub(super) fn word(c: &mut Ctx) -> Option<String> {
    Some(c.pick(&WORDS).to_string())
}

pub(super) fn words(c: &mut Ctx) -> Option<String> {
    let default = c.between(2, 5) as usize;
    let n = c.count(0, default, MAX_WORDS)?;
    Some((0..n).map(|_| c.pick(&WORDS)).collect::<Vec<_>>().join(" "))
}

pub(super) fn phrase(c: &mut Ctx) -> Option<String> {
    let template = c.pick(PHRASES);
    let mut out = String::with_capacity(template.len() + 32);
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let key = rest.as_bytes()[start + 1];
        out.push_str(&match key {
            b'a' => c.pick(ABBREVIATIONS).to_string(),
            b'j' => c.pick(ADJECTIVES).to_string(),
            b'n' => c.pick(NOUNS).to_string(),
            b'v' => c.pick(VERBS).to_string(),
            b'g' => c.pick(ING_VERBS).to_string(),
            _ => capitalize(c.pick(ING_VERBS)),
        });
        rest = &rest[start + 3..];
    }
    out.push_str(rest);
    Some(out)
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    chars.next().map(|first| first.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

fn lorem_words(c: &mut Ctx, n: usize) -> Vec<&'static str> {
    (0..n).map(|_| c.pick(LOREM_WORDS)).collect()
}

fn lorem_sentence(c: &mut Ctx, words: usize) -> String {
    format!("{}.", capitalize(&lorem_words(c, words).join(" ")))
}

fn lorem_sentences(c: &mut Ctx, n: usize) -> String {
    (0..n)
        .map(|_| {
            let words = c.between(3, 10) as usize;
            lorem_sentence(c, words)
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub(super) fn lorem_word(c: &mut Ctx) -> Option<String> {
    c.one(LOREM_WORDS)
}

pub(super) fn lorem_words_var(c: &mut Ctx) -> Option<String> {
    let n = c.count(0, 3, MAX_WORDS)?;
    Some(lorem_words(c, n).join(" "))
}

pub(super) fn lorem_sentence_var(c: &mut Ctx) -> Option<String> {
    let default = c.between(3, 10) as usize;
    let n = c.count(0, default, MAX_WORDS)?;
    Some(lorem_sentence(c, n))
}

pub(super) fn lorem_sentences_var(c: &mut Ctx) -> Option<String> {
    let default = c.between(2, 6) as usize;
    let n = c.count(0, default, MAX_WORDS)?;
    Some(lorem_sentences(c, n))
}

pub(super) fn lorem_paragraph(c: &mut Ctx) -> Option<String> {
    let default = c.between(3, 6) as usize;
    let n = c.count(0, default, MAX_WORDS)?;
    Some(lorem_sentences(c, n))
}

pub(super) fn lorem_paragraphs(c: &mut Ctx) -> Option<String> {
    let n = c.count(0, 3, MAX_WORDS / 10)?;
    Some(
        (0..n)
            .map(|_| {
                let sentences = c.between(3, 6) as usize;
                lorem_sentences(c, sentences)
            })
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

pub(super) fn lorem_text(c: &mut Ctx) -> Option<String> {
    let n = c.between(1, 5) as usize;
    Some(lorem_sentences(c, n))
}

pub(super) fn lorem_slug(c: &mut Ctx) -> Option<String> {
    let n = c.count(0, 3, MAX_WORDS)?;
    Some(lorem_words(c, n).join("-"))
}

pub(super) fn lorem_lines(c: &mut Ctx) -> Option<String> {
    let default = c.between(1, 5) as usize;
    let n = c.count(0, default, MAX_WORDS)?;
    Some(
        (0..n)
            .map(|_| {
                let words = c.between(3, 10) as usize;
                lorem_sentence(c, words)
            })
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

// ── Dates and times ──────────────────────────────────────────────────────────

impl Ctx {
    /// Now moved by `seconds`, within years 1 to 9999.
    fn at(&self, seconds: i64) -> Option<OffsetDateTime> {
        self.now.checked_add(time::Duration::seconds(seconds)).filter(|t| (1..=9999).contains(&t.year()))
    }

    /// Now moved by argument `i` (an offset), or by `default` seconds.
    fn at_arg(&self, i: usize, default: i64) -> Option<OffsetDateTime> {
        self.at(self.offset(i, default)?)
    }

    /// A random moment between now + `from` and now + `to` seconds, to the millisecond.
    fn random_moment(&mut self, from: i64, to: i64) -> Option<OffsetDateTime> {
        let (lo, hi) = (self.at(from)?, self.at(to)?);
        let (lo, hi) = (lo.unix_timestamp_nanos() / 1_000_000, hi.unix_timestamp_nanos() / 1_000_000);
        if lo > hi {
            return None;
        }
        let ms = self.rng.random_range(lo..=hi);
        OffsetDateTime::from_unix_timestamp_nanos(ms * 1_000_000).ok()
    }

    /// A random moment within the last (`past`) or next `days` days (argument 0).
    fn random_days(&mut self, default: usize, past: bool) -> Option<OffsetDateTime> {
        let days = self.count(0, default, MAX_DAYS)? as i64 * 86_400;
        if past { self.random_moment(-days, 0) } else { self.random_moment(0, days) }
    }
}

/// `2026-09-28T09:41:07.123Z`, like JavaScript's `toISOString`.
fn iso(t: OffsetDateTime) -> String {
    format!("{}T{:02}:{:02}:{:02}.{:03}Z", iso_date(t.date()), t.hour(), t.minute(), t.second(), t.millisecond())
}

fn iso_date(d: Date) -> String {
    format!("{:04}-{:02}-{:02}", d.year(), d.month() as u8, d.day())
}

fn weekday(t: OffsetDateTime) -> &'static str {
    WEEKDAYS[t.weekday().number_days_from_monday() as usize]
}

fn month(t: OffsetDateTime) -> &'static str {
    MONTHS[t.month() as usize - 1]
}

/// `Tue Mar 17 2026 13:11:50 GMT+0000`, like JavaScript's `Date.toString` (UTC).
fn js_date(t: OffsetDateTime) -> String {
    format!(
        "{} {} {:02} {:04} {:02}:{:02}:{:02} GMT+0000",
        &weekday(t)[..3],
        &month(t)[..3],
        t.day(),
        t.year(),
        t.hour(),
        t.minute(),
        t.second()
    )
}

pub(super) fn timestamp(c: &mut Ctx) -> Option<String> {
    Some(c.at_arg(0, 0)?.unix_timestamp().to_string())
}

pub(super) fn timestamp_ms(c: &mut Ctx) -> Option<String> {
    Some((c.at_arg(0, 0)?.unix_timestamp_nanos() / 1_000_000).to_string())
}

pub(super) fn iso_timestamp(c: &mut Ctx) -> Option<String> {
    Some(iso(c.at_arg(0, 0)?))
}

pub(super) fn date_at(c: &mut Ctx) -> Option<String> {
    Some(iso_date(c.at_arg(0, 0)?.date()))
}

pub(super) fn days_from_today(c: &Ctx, days: i64) -> Option<String> {
    Some(iso_date(c.at(days * 86_400)?.date()))
}

/// RFC 9110 HTTP date: `Mon, 28 Sep 2026 09:41:07 GMT`.
pub(super) fn http_date(c: &mut Ctx) -> Option<String> {
    let t = c.at_arg(0, 0)?;
    Some(format!(
        "{}, {:02} {} {:04} {:02}:{:02}:{:02} GMT",
        &weekday(t)[..3],
        t.day(),
        &month(t)[..3],
        t.year(),
        t.hour(),
        t.minute(),
        t.second()
    ))
}

const YEAR: i64 = 365 * 86_400;

pub(super) fn date_time(c: &mut Ctx) -> Option<String> {
    let (from, to) = (c.offset(0, -YEAR)?, c.offset(1, YEAR)?);
    Some(iso(c.random_moment(from, to)?))
}

pub(super) fn random_date(c: &mut Ctx) -> Option<String> {
    let (from, to) = (c.offset(0, -YEAR)?, c.offset(1, YEAR)?);
    Some(iso_date(c.random_moment(from, to)?.date()))
}

pub(super) fn random_time(c: &mut Ctx) -> Option<String> {
    let seconds = c.between(0, 86_399);
    Some(format!("{:02}:{:02}:{:02}", seconds / 3600, seconds / 60 % 60, seconds % 60))
}

pub(super) fn date_past(c: &mut Ctx) -> Option<String> {
    Some(js_date(c.random_days(365, true)?))
}

pub(super) fn date_future(c: &mut Ctx) -> Option<String> {
    Some(js_date(c.random_days(365, false)?))
}

pub(super) fn date_recent(c: &mut Ctx) -> Option<String> {
    Some(js_date(c.random_days(1, true)?))
}

// ── People ───────────────────────────────────────────────────────────────────

pub(super) fn full_name(c: &mut Ctx) -> Option<String> {
    Some(format!("{} {}", c.pick(FIRST_NAMES), c.pick(LAST_NAMES)))
}

/// Someone aged `min..=max` today (arguments 0 and 1, default 18 to 80).
pub(super) fn birthdate(c: &mut Ctx) -> Option<String> {
    let (min, max) = (c.int_in(0, 18, 0, 150)?, c.int_in(1, 80, 0, 150)?);
    if min > max {
        return None;
    }
    let today = c.now.date();
    let latest = years_before(today, min as i32)?;
    let earliest = years_before(today, max as i32 + 1)?.next_day()?;
    let day = c.between(earliest.to_julian_day().into(), latest.to_julian_day().into());
    Some(iso_date(Date::from_julian_day(day as i32).ok()?))
}

/// The same day `years` earlier (February 29 becomes February 28).
fn years_before(d: Date, years: i32) -> Option<Date> {
    let year = d.year() - years;
    Date::from_calendar_date(year, d.month(), d.day().min(d.month().length(year))).ok()
}

pub(super) fn age(c: &mut Ctx) -> Option<String> {
    let (min, max) = (c.int_in(0, 18, 0, 150)?, c.int_in(1, 80, 0, 150)?);
    (min <= max).then(|| c.between(min, max).to_string())
}

pub(super) fn e164_phone(c: &mut Ctx) -> Option<String> {
    let (_, code, patterns) = match c.arg(0) {
        Some(country) => *PHONE_FORMATS.iter().find(|(iso, _, _)| iso.eq_ignore_ascii_case(country))?,
        None => c.pick(PHONE_FORMATS),
    };
    let pattern = c.pick(patterns);
    Some(format!("+{code}{}", c.pattern(pattern)))
}

pub(super) fn job_title(c: &mut Ctx) -> Option<String> {
    Some(format!("{} {} {}", c.pick(JOB_DESCRIPTORS), c.pick(JOB_AREAS), c.pick(JOB_TYPES)))
}

/// `name` in ASCII letters and digits only: accents dropped, `O'Brien` → `OBrien`.
pub(super) fn ascii(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
            continue;
        }
        let plain = match ch.to_lowercase().next().unwrap_or(ch) {
            'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' => "a",
            'æ' => "ae",
            'ç' | 'ć' | 'č' => "c",
            'è' | 'é' | 'ê' | 'ë' | 'ē' => "e",
            'ì' | 'í' | 'î' | 'ï' | 'ı' | 'ī' => "i",
            'ñ' | 'ń' => "n",
            'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' | 'ő' => "o",
            'ù' | 'ú' | 'û' | 'ü' | 'ū' | 'ű' => "u",
            'ý' | 'ÿ' => "y",
            'ß' => "ss",
            'ł' => "l",
            'ś' | 'š' | 'ş' => "s",
            'ź' | 'ż' | 'ž' => "z",
            'ğ' => "g",
            'œ' => "oe",
            _ => continue,
        };
        if ch.is_uppercase() { out.push_str(&plain.to_uppercase()) } else { out.push_str(plain) }
    }
    out
}

// ── Internet ─────────────────────────────────────────────────────────────────

/// `Lottie.Smith24`, `Pablo_Garcia`, `Aoife71` …
pub(super) fn user_name(c: &mut Ctx) -> Option<String> {
    let (first, last) = (ascii(c.pick(FIRST_NAMES)), ascii(c.pick(LAST_NAMES)));
    let n = c.between(1, 99);
    Some(match c.below(4) {
        0 => format!("{first}.{last}"),
        1 => format!("{first}.{last}{n}"),
        2 => format!("{first}_{last}"),
        _ => format!("{first}{n}"),
    })
}

pub(super) fn email(c: &mut Ctx) -> Option<String> {
    let (first, last) = (ascii(c.pick(FIRST_NAMES)), ascii(c.pick(LAST_NAMES)));
    let n = c.between(1, 99);
    let local = match c.below(3) {
        0 => format!("{first}.{last}"),
        1 => format!("{first}.{last}{n}"),
        _ => format!("{first}{n}"),
    };
    Some(format!("{}@{}", local.to_lowercase(), c.pick(EMAIL_DOMAINS)))
}

pub(super) fn example_email(c: &mut Ctx) -> Option<String> {
    let user = user_name(c)?;
    Some(format!("{user}@{}", c.pick(EMAIL_DOMAINS)))
}

pub(super) fn domain_word(c: &mut Ctx) -> Option<String> {
    let word = ascii(c.pick(LAST_NAMES)).to_lowercase();
    Some(if word.is_empty() { "example".into() } else { word })
}

pub(super) fn domain_name(c: &mut Ctx) -> Option<String> {
    Some(format!("{}.{}", domain_word(c)?, c.pick(DOMAIN_SUFFIXES)))
}

pub(super) fn url(c: &mut Ctx) -> Option<String> {
    Some(format!("https://{}", domain_name(c)?))
}

pub(super) fn ipv4(c: &mut Ctx) -> Option<String> {
    let first = loop {
        let n = c.between(1, 223);
        if n != 10 && n != 127 {
            break n;
        }
    };
    Some(format!("{first}.{}.{}.{}", c.between(0, 255), c.between(0, 255), c.between(1, 254)))
}

pub(super) fn ipv6(c: &mut Ctx) -> Option<String> {
    Some((0..8).map(|_| format!("{:04x}", c.rng.random::<u16>())).collect::<Vec<_>>().join(":"))
}

/// RFC 1918: 10.0.0.0/8, 172.16.0.0/12 or 192.168.0.0/16.
pub(super) fn private_ipv4(c: &mut Ctx) -> Option<String> {
    let host = format!("{}.{}", c.between(0, 255), c.between(1, 254));
    Some(match c.below(3) {
        0 => format!("10.{}.{host}", c.between(0, 255)),
        1 => format!("172.{}.{host}", c.between(16, 31)),
        _ => format!("192.168.{host}"),
    })
}

/// A unicast MAC address.
pub(super) fn mac(c: &mut Ctx) -> Option<String> {
    let mut bytes = [0u8; 6];
    c.rng.fill_bytes(&mut bytes);
    bytes[0] &= 0xfe;
    Some(bytes.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(":"))
}

pub(super) fn semver(c: &mut Ctx) -> Option<String> {
    Some(format!("{}.{}.{}", c.between(0, 9), c.between(0, 20), c.between(0, 30)))
}

// ── Location ─────────────────────────────────────────────────────────────────

pub(super) fn street_name(c: &mut Ctx) -> Option<String> {
    Some(format!("{} {}", c.pick(STREET_BASES), c.pick(STREET_SUFFIXES)))
}

pub(super) fn street_address(c: &mut Ctx) -> Option<String> {
    let number = c.between(1, 9999);
    Some(format!("{number} {}", street_name(c)?))
}

/// A coordinate in `min..=max` (arguments 0 and 1) within `-limit..=limit`, 4 decimals.
fn coordinate(c: &mut Ctx, limit: f64) -> Option<f64> {
    let (lo, hi) = (c.float(0, -limit)?, c.float(1, limit)?);
    if lo > hi || lo < -limit || hi > limit {
        return None;
    }
    let x = if lo == hi { lo } else { c.rng.random_range(lo..=hi) };
    Some((x * 10_000.0).round() / 10_000.0)
}

pub(super) fn latitude(c: &mut Ctx) -> Option<String> {
    Some(format!("{:.4}", coordinate(c, 90.0)?))
}

pub(super) fn longitude(c: &mut Ctx) -> Option<String> {
    Some(format!("{:.4}", coordinate(c, 180.0)?))
}

pub(super) fn coordinates(c: &mut Ctx) -> Option<String> {
    let (lat, lon) = (c.rng.random_range(-90.0..=90.0f64), c.rng.random_range(-180.0..=180.0f64));
    Some(format!("{lat:.4},{lon:.4}"))
}

// ── Finance ──────────────────────────────────────────────────────────────────

/// An IBAN with valid check digits (and national check digits where the country has them).
pub(super) fn iban(c: &mut Ctx) -> Option<String> {
    let (country, pattern) = match c.arg(0) {
        Some(code) => *IBAN_FORMATS.iter().find(|(cc, _)| cc.eq_ignore_ascii_case(code))?,
        None => c.pick(IBAN_FORMATS),
    };
    let bban = loop {
        let mut bban: Vec<u8> = pattern
            .bytes()
            .map(|p| if p == b'a' { b'A' + c.rng.random_range(0..26u8) } else { b'0' + c.rng.random_range(0..10u8) })
            .collect();
        if national_check(country, &mut bban) {
            break String::from_utf8(bban).ok()?;
        }
    };
    let check = 98 - mod97(&format!("{bban}{country}00"));
    Some(format!("{country}{check:02}{bban}"))
}

/// The remainder of the number an IBAN's characters stand for (A = 10 … Z = 35) divided by 97.
pub(super) fn mod97(s: &str) -> u32 {
    s.bytes().fold(0, |r, b| match b {
        b'0'..=b'9' => (r * 10 + u32::from(b - b'0')) % 97,
        _ => (r * 100 + u32::from(b.to_ascii_uppercase() - b'A' + 10)) % 97,
    })
}

fn number(digits: &[u8]) -> u64 {
    digits.iter().fold(0, |n, d| n * 10 + u64::from(d - b'0'))
}

fn set_digits(dst: &mut [u8], value: u64) {
    let text = format!("{value:0width$}", width = dst.len());
    dst.copy_from_slice(text.as_bytes());
}

/// Fill in the national check digits of `bban`; `false` when these digits can't have one.
fn national_check(country: &str, bban: &mut [u8]) -> bool {
    match country {
        // Bank and account (10 digits) mod 97, where 0 becomes 97.
        "BE" => {
            let check = number(&bban[..10]) % 97;
            set_digits(&mut bban[10..], if check == 0 { 97 } else { check });
        }
        // French RIB key over bank (5), branch (5) and account (11 digits).
        "FR" => {
            let sum = 89 * number(&bban[..5]) + 15 * number(&bban[5..10]) + 3 * number(&bban[10..21]);
            set_digits(&mut bban[21..], 97 - sum % 97);
        }
        // Spanish control digits: one over "00" + bank + branch, one over the account.
        "ES" => {
            let mut first = [b'0'; 10];
            first[2..].copy_from_slice(&bban[..8]);
            let (d1, d2) = (spanish_digit(&first), spanish_digit(&bban[10..20]));
            bban[8] = b'0' + d1;
            bban[9] = b'0' + d2;
        }
        // Dutch account numbers pass the 11-test.
        "NL" => {
            let account = &mut bban[4..];
            let sum: u32 = account[..9].iter().zip((2..=10).rev()).map(|(d, w)| u32::from(d - b'0') * w).sum();
            let last = (11 - sum % 11) % 11;
            if last == 10 {
                return false;
            }
            account[9] = b'0' + last as u8;
        }
        _ => {}
    }
    true
}

fn spanish_digit(digits: &[u8]) -> u8 {
    const WEIGHTS: [u32; 10] = [1, 2, 4, 8, 5, 10, 9, 7, 3, 6];
    let sum: u32 = digits.iter().zip(WEIGHTS).map(|(d, w)| u32::from(d - b'0') * w).sum();
    match 11 - sum % 11 {
        11 => 0,
        10 => 1,
        d => d as u8,
    }
}

/// SWIFT/BIC: bank (4 letters), country, location (2 characters).
pub(super) fn bic(c: &mut Ctx) -> Option<String> {
    const LOCATION: &[u8] = b"ABCDEFGHIJKLMNPQRSTUVWXYZ23456789";
    let bank = c.chars(&ALPHANUMERIC[..26], 4);
    let (country, _, _) = c.pick(COUNTRIES);
    Some(format!("{bank}{country}{}", c.chars(LOCATION, 2)))
}

pub(super) fn card_number(c: &mut Ctx) -> Option<String> {
    let (_, prefixes, len) = match c.arg(0) {
        Some(brand) => *CARD_BRANDS.iter().find(|(name, _, _)| name.eq_ignore_ascii_case(brand))?,
        None => c.pick(CARD_BRANDS),
    };
    let mut digits: Vec<u8> = c.pick(prefixes).bytes().map(|b| b - b'0').collect();
    while digits.len() < len - 1 {
        digits.push(c.rng.random_range(0..10));
    }
    digits.push(luhn_digit(&digits));
    Some(digits.iter().map(|d| char::from(b'0' + d)).collect())
}

/// The Luhn check digit to append to `body` (digit values, not ASCII).
pub(super) fn luhn_digit(body: &[u8]) -> u8 {
    let sum: u32 = body
        .iter()
        .rev()
        .enumerate()
        .map(|(i, &d)| {
            let d = u32::from(d);
            if i % 2 == 0 { if d * 2 > 9 { d * 2 - 9 } else { d * 2 } } else { d }
        })
        .sum();
    ((10 - sum % 10) % 10) as u8
}

/// `MM/YY`, 1 to 60 months ahead.
pub(super) fn card_expiry(c: &mut Ctx) -> Option<String> {
    let months = i64::from(c.now.year()) * 12 + i64::from(c.now.month() as u8 - 1) + c.between(1, 60);
    Some(format!("{:02}/{:02}", months % 12 + 1, (months / 12) % 100))
}

/// A legacy (P2PKH, `1…`) or script (P2SH, `3…`) address with a valid Base58Check checksum.
pub(super) fn bitcoin(c: &mut Ctx) -> Option<String> {
    let mut payload = [0u8; 25];
    payload[0] = if c.rng.random_bool(0.5) { 0x00 } else { 0x05 };
    c.rng.fill_bytes(&mut payload[1..21]);
    let check = sha2::Sha256::digest(sha2::Sha256::digest(&payload[..21]));
    payload[21..].copy_from_slice(&check[..4]);
    Some(base58(&payload))
}

fn base58(bytes: &[u8]) -> String {
    let zeros = bytes.iter().take_while(|b| **b == 0).count();
    // Base-58 digits, least significant first.
    let mut digits: Vec<u8> = Vec::new();
    for &byte in bytes {
        let mut carry = u32::from(byte);
        for d in &mut digits {
            carry += u32::from(*d) << 8;
            *d = (carry % 58) as u8;
            carry /= 58;
        }
        while carry > 0 {
            digits.push((carry % 58) as u8);
            carry /= 58;
        }
    }
    let mut out = "1".repeat(zeros);
    out.extend(digits.iter().rev().map(|d| BASE58[*d as usize] as char));
    out
}

// ── Business and commerce ────────────────────────────────────────────────────

pub(super) fn company_name(c: &mut Ctx) -> Option<String> {
    let (a, b, d) = (c.pick(LAST_NAMES), c.pick(LAST_NAMES), c.pick(LAST_NAMES));
    Some(match c.below(3) {
        0 => format!("{a} {}", c.pick(COMPANY_SUFFIXES)),
        1 => format!("{a} - {b}"),
        _ => format!("{a}, {b} and {d}"),
    })
}

pub(super) fn bs(c: &mut Ctx) -> Option<String> {
    Some(format!("{} {} {}", c.pick(BS_BUZZ), c.pick(BS_ADJECTIVES), c.pick(BS_NOUNS)))
}

pub(super) fn catch_phrase(c: &mut Ctx) -> Option<String> {
    Some(format!("{} {} {}", c.pick(CATCH_ADJECTIVES), c.pick(CATCH_DESCRIPTORS), c.pick(CATCH_NOUNS)))
}

pub(super) fn product_name(c: &mut Ctx) -> Option<String> {
    Some(format!("{} {} {}", c.pick(PRODUCT_ADJECTIVES), c.pick(PRODUCT_MATERIALS), c.pick(PRODUCTS)))
}

pub(super) fn price(c: &mut Ctx) -> Option<String> {
    let (lo, hi) = (c.float(0, 0.0)?, c.float(1, 1000.0)?);
    if lo < 0.0 || hi > 1e15 {
        return None;
    }
    let (lo, hi) = ((lo * 100.0).ceil() as i64, (hi * 100.0).floor() as i64);
    if lo > hi {
        return None;
    }
    let cents = c.between(lo, hi);
    Some(format!("{}.{:02}", cents / 100, cents % 100))
}

fn random_digits(c: &mut Ctx, n: usize) -> Vec<u8> {
    (0..n).map(|_| c.rng.random_range(0..10)).collect()
}

fn digits_text(digits: &[u8]) -> String {
    digits.iter().map(|d| char::from(b'0' + d)).collect()
}

/// GS1 check digit (EAN-13, UPC-A, ISBN-13) for `body` digit values.
pub(super) fn gs1_digit(body: &[u8]) -> u8 {
    let sum: u32 = body.iter().rev().enumerate().map(|(i, &d)| u32::from(d) * if i % 2 == 0 { 3 } else { 1 }).sum();
    ((10 - sum % 10) % 10) as u8
}

fn with_gs1_digit(mut body: Vec<u8>) -> String {
    body.push(gs1_digit(&body));
    digits_text(&body)
}

pub(super) fn ean13(c: &mut Ctx) -> Option<String> {
    Some(with_gs1_digit(random_digits(c, 12)))
}

pub(super) fn upc(c: &mut Ctx) -> Option<String> {
    Some(with_gs1_digit(random_digits(c, 11)))
}

pub(super) fn isbn13(c: &mut Ctx) -> Option<String> {
    let mut body = vec![9, 7, 8];
    body.extend(random_digits(c, 9));
    Some(with_gs1_digit(body))
}

pub(super) fn isbn10(c: &mut Ctx) -> Option<String> {
    let body = random_digits(c, 9);
    let sum: u32 = body.iter().zip((2..=10).rev()).map(|(d, w)| u32::from(*d) * w).sum();
    let check = match (11 - sum % 11) % 11 {
        10 => 'X',
        d => char::from(b'0' + d as u8),
    };
    Some(format!("{}{check}", digits_text(&body)))
}

// ── Files ────────────────────────────────────────────────────────────────────

fn mime_entry(c: &mut Ctx, common: bool) -> (&'static str, &'static str) {
    static COMMON: LazyLock<Vec<(&str, &str, bool)>> =
        LazyLock::new(|| MIME_TYPES.iter().copied().filter(|(_, _, common)| *common).collect());
    let (mime, ext, _) = if common { c.pick(&COMMON) } else { c.pick(MIME_TYPES) };
    (mime, ext)
}

pub(super) fn mime_type(c: &mut Ctx) -> Option<String> {
    Some(mime_entry(c, false).0.to_string())
}

pub(super) fn file_type(c: &mut Ctx, common: bool) -> Option<String> {
    Some(mime_entry(c, common).0.split('/').next()?.to_string())
}

pub(super) fn file_ext(c: &mut Ctx, common: bool) -> Option<String> {
    Some(mime_entry(c, common).1.to_string())
}

pub(super) fn file_name(c: &mut Ctx, common: bool) -> Option<String> {
    let stem = format!("{}_{}", c.pick(CATCH_DESCRIPTORS), c.pick(NOUNS)).replace([' ', '-', '/'], "_");
    Some(format!("{stem}.{}", mime_entry(c, common).1))
}

pub(super) fn file_path(c: &mut Ctx) -> Option<String> {
    let dir = c.pick(DIRECTORIES);
    Some(format!("{dir}/{}", file_name(c, false)?))
}

// ── Images and colors ────────────────────────────────────────────────────────

fn image_size(c: &Ctx) -> Option<(usize, usize)> {
    Some((c.count(0, 640, 10_000)?, c.count(1, 480, 10_000)?))
}

pub(super) fn image_url(c: &mut Ctx) -> Option<String> {
    let (w, h) = image_size(c)?;
    Some(format!("https://picsum.photos/seed/{}/{w}/{h}", c.chars(LOWER_DIGITS, 8)))
}

pub(super) fn category_image(c: &mut Ctx, category: &str) -> Option<String> {
    let (w, h) = image_size(c)?;
    Some(format!("https://loremflickr.com/{w}/{h}/{category}?lock={}", c.between(1, 99_999)))
}

/// An SVG of one random color with its size written in the middle, as a data URI.
pub(super) fn image_data_uri(c: &mut Ctx) -> Option<String> {
    let (w, h) = image_size(c)?;
    let color = hex_color(c)?;
    let svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" version=\"1.1\" baseProfile=\"full\" width=\"{w}\" height=\"{h}\">\
         <rect width=\"100%\" height=\"100%\" fill=\"{color}\"/><text x=\"{}\" y=\"{}\" font-size=\"20\" \
         alignment-baseline=\"middle\" text-anchor=\"middle\" fill=\"white\">{w}x{h}</text></svg>",
        w / 2,
        h / 2
    );
    let mut encoded = String::with_capacity(svg.len() * 2);
    for b in svg.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            encoded.push(b as char);
        } else {
            encoded.push_str(&format!("%{b:02X}"));
        }
    }
    Some(format!("data:image/svg+xml;charset=UTF-8,{encoded}"))
}

pub(super) fn hex_color(c: &mut Ctx) -> Option<String> {
    Some(format!("#{:06x}", c.rng.random_range(0..0x100_0000u32)))
}

pub(super) fn rgb_color(c: &mut Ctx) -> Option<String> {
    Some(format!("rgb({}, {}, {})", c.between(0, 255), c.between(0, 255), c.between(0, 255)))
}

pub(super) fn hsl_color(c: &mut Ctx) -> Option<String> {
    Some(format!("hsl({}, {}%, {}%)", c.between(0, 359), c.between(0, 100), c.between(0, 100)))
}
