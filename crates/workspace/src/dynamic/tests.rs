use std::collections::HashSet;
use std::net::{Ipv4Addr, Ipv6Addr};

use regex::Regex;
use sha2::Digest as _;
use time::format_description::BorrowedFormatItem;
use time::format_description::well_known::Rfc3339;
use time::macros::format_description;
use time::{Date, OffsetDateTime, PrimitiveDateTime};

use super::catalog::GROUPS;
use super::data::{CARD_BRANDS, FIRST_NAMES, IBAN_FORMATS, LAST_NAMES, PHONE_FORMATS, UNICODE_POOLS};
use super::generators::ascii;
use super::*;

/// Every dynamic variable on Postman's list
/// (learning.postman.com/docs/tests-and-scripts/write-scripts/variables-list).
const POSTMAN: &[&str] = &[
    "$guid",
    "$timestamp",
    "$isoTimestamp",
    "$randomUUID",
    "$randomAlphaNumeric",
    "$randomBoolean",
    "$randomInt",
    "$randomColor",
    "$randomHexColor",
    "$randomAbbreviation",
    "$randomIP",
    "$randomIPV6",
    "$randomMACAddress",
    "$randomPassword",
    "$randomLocale",
    "$randomUserAgent",
    "$randomProtocol",
    "$randomSemver",
    "$randomFirstName",
    "$randomLastName",
    "$randomFullName",
    "$randomNamePrefix",
    "$randomNameSuffix",
    "$randomJobArea",
    "$randomJobDescriptor",
    "$randomJobTitle",
    "$randomJobType",
    "$randomPhoneNumber",
    "$randomPhoneNumberExt",
    "$randomCity",
    "$randomStreetName",
    "$randomStreetAddress",
    "$randomCountry",
    "$randomCountryCode",
    "$randomLatitude",
    "$randomLongitude",
    "$randomAvatarImage",
    "$randomImageUrl",
    "$randomAbstractImage",
    "$randomAnimalsImage",
    "$randomBusinessImage",
    "$randomCatsImage",
    "$randomCityImage",
    "$randomFoodImage",
    "$randomNightlifeImage",
    "$randomFashionImage",
    "$randomPeopleImage",
    "$randomNatureImage",
    "$randomSportsImage",
    "$randomTransportImage",
    "$randomImageDataUri",
    "$randomBankAccount",
    "$randomBankAccountName",
    "$randomCreditCardMask",
    "$randomBankAccountBic",
    "$randomBankAccountIban",
    "$randomTransactionType",
    "$randomCurrencyCode",
    "$randomCurrencyName",
    "$randomCurrencySymbol",
    "$randomBitcoin",
    "$randomCompanyName",
    "$randomCompanySuffix",
    "$randomBs",
    "$randomBsAdjective",
    "$randomBsBuzz",
    "$randomBsNoun",
    "$randomCatchPhrase",
    "$randomCatchPhraseAdjective",
    "$randomCatchPhraseDescriptor",
    "$randomCatchPhraseNoun",
    "$randomDatabaseColumn",
    "$randomDatabaseType",
    "$randomDatabaseCollation",
    "$randomDatabaseEngine",
    "$randomDateFuture",
    "$randomDatePast",
    "$randomDateRecent",
    "$randomWeekday",
    "$randomMonth",
    "$randomDomainName",
    "$randomDomainSuffix",
    "$randomDomainWord",
    "$randomEmail",
    "$randomExampleEmail",
    "$randomUserName",
    "$randomUrl",
    "$randomFileName",
    "$randomFileType",
    "$randomFileExt",
    "$randomCommonFileName",
    "$randomCommonFileType",
    "$randomCommonFileExt",
    "$randomFilePath",
    "$randomDirectoryPath",
    "$randomMimeType",
    "$randomPrice",
    "$randomProduct",
    "$randomProductAdjective",
    "$randomProductMaterial",
    "$randomProductName",
    "$randomDepartment",
    "$randomNoun",
    "$randomVerb",
    "$randomIngverb",
    "$randomAdjective",
    "$randomWord",
    "$randomWords",
    "$randomPhrase",
    "$randomLoremWord",
    "$randomLoremWords",
    "$randomLoremSentence",
    "$randomLoremSentences",
    "$randomLoremParagraph",
    "$randomLoremParagraphs",
    "$randomLoremText",
    "$randomLoremSlug",
    "$randomLoremLines",
];

fn make(expr: &str) -> String {
    generate(expr).unwrap_or_else(|| panic!("{expr} gave no value"))
}

const UUID_V4: &str = r"^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$";
const ISO_TIME: &str = r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$";
const ISO_DATE: &str = r"^\d{4}-(0[1-9]|1[0-2])-(0[1-9]|[12]\d|3[01])$";
const JS_DATE: &str = r"^(Mon|Tue|Wed|Thu|Fri|Sat|Sun) (Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec) \d{2} \d{4} \d{2}:\d{2}:\d{2} GMT\+0000$";
const JS_DATE_FORMAT: &[BorrowedFormatItem<'_>] =
    format_description!("[weekday repr:short] [month repr:short] [day] [year] [hour]:[minute]:[second] GMT+0000");
const HTTP_DATE_FORMAT: &[BorrowedFormatItem<'_>] =
    format_description!("[weekday repr:short], [day] [month repr:short] [year] [hour]:[minute]:[second] GMT");
/// One line of text without surrounding whitespace.
const LINE: &str = r"^\S(.*\S)?$";

/// The expected shape of a variable's values (and of its example).
fn shape(v: &DynamicVar) -> &'static str {
    let specific = match v.name {
        "$guid" | "$uuid" | "$randomUUID" => UUID_V4,
        "$uuidv7" => r"^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$",
        "$ulid" => r"^[0-7][0-9A-HJKMNP-TV-Z]{25}$",
        "$nanoid" => r"^[A-Za-z0-9_-]{21}$",
        "$objectId" => r"^[0-9a-f]{24}$",
        "$snowflake" => r"^[1-9]\d{17,18}$",
        "$traceparent" => r"^00-[0-9a-f]{32}-[0-9a-f]{16}-01$",
        "$randomHex" => r"^[0-9a-f]{32}$",
        "$randomInt" => r"^(\d{1,3}|1000)$",
        "$randomFloat" => r"^\d{1,4}\.\d{2}$",
        "$randomDigits" => r"^\d{6}$",
        "$randomInt64" => r"^-?\d{1,19}$",
        "$randomBoolean" => r"^(true|false)$",
        "$randomAlphaNumeric" => r"^[a-z0-9]$",
        "$randomString" => r"^[A-Za-z0-9]{16}$",
        "$randomPassword" => r"^[A-Za-z0-9]{15}$",
        "$randomStrongPassword" => r"^[A-Za-z0-9!@#$%^&*_=+?-]{16}$",
        "$randomBase64" => r"^[A-Za-z0-9+/]{22}==$",
        "$randomUnicodeString" => r"^.{16}$",
        "$randomEmoji" => r"^[\p{Emoji}\p{Emoji_Component}]+$",
        "$randomFrom" => r"^(red|green|blue)$",
        "$randomSlug" => r"^[a-z0-9]+(-[a-z0-9]+){2}$",
        "$randomAbbreviation" => r"^[A-Z0-9]+$",
        "$randomLoremWord" => r"^[a-z]+$",
        "$randomLoremWords" => r"^[a-z]+( [a-z]+){2}$",
        "$randomLoremSlug" => r"^[a-z]+(-[a-z]+){2}$",
        "$randomLoremSentence" => r"^[A-Z][a-z]*( [a-z]+){2,9}\.$",
        "$randomLoremSentences" | "$randomLoremParagraph" | "$randomLoremText" => r"^[A-Z][a-z ]+\.( [A-Z][a-z ]+\.)*$",
        "$randomLoremParagraphs" | "$randomLoremLines" => r"^[A-Z][A-Za-z .]+(\n[A-Z][A-Za-z .]+)*$",
        "$timestamp" => r"^1\d{9}$",
        "$timestampMs" => r"^1\d{12}$",
        "$isoTimestamp" | "$randomDateTime" => ISO_TIME,
        "$isoDate" | "$today" | "$tomorrow" | "$yesterday" | "$randomDate" | "$randomBirthdate" => ISO_DATE,
        "$httpDate" => {
            r"^(Mon|Tue|Wed|Thu|Fri|Sat|Sun), \d{2} (Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec) \d{4} \d{2}:\d{2}:\d{2} GMT$"
        }
        "$randomTime" => r"^([01]\d|2[0-3]):[0-5]\d:[0-5]\d$",
        "$randomDatePast" | "$randomDateFuture" | "$randomDateRecent" => JS_DATE,
        "$randomWeekday" => r"^(Mon|Tues|Wednes|Thurs|Fri|Satur|Sun)day$",
        "$randomTimezone" => r"^(UTC|[A-Z][A-Za-z]+(/[A-Z][A-Za-z_]+)+)$",
        "$randomGender" => r"^(female|male|non-binary)$",
        "$randomAge" => r"^\d{2}$",
        "$randomPhoneNumber" => r"^[2-9]\d{2}-[2-9]\d{2}-\d{4}$",
        "$randomPhoneNumberExt" => r"^[1-9]{2}-[2-9]\d{2}-[2-9]\d{2}-\d{4}$",
        "$randomE164Phone" => r"^\+[1-9]\d{7,14}$",
        "$randomEmail" => r"^[a-z0-9]+(\.[a-z0-9]+)*@example\.(com|net|org)$",
        "$randomExampleEmail" => r"^[A-Za-z0-9]+([._][A-Za-z0-9]+)*@example\.(com|net|org)$",
        "$randomUserName" => r"^[A-Za-z0-9]+([._][A-Za-z0-9]+)*$",
        "$randomDomainName" => r"^[a-z0-9]+\.[a-z]{2,4}$",
        "$randomDomainSuffix" => r"^[a-z]{2,4}$",
        "$randomDomainWord" => r"^[a-z0-9]+$",
        "$randomUrl" => r"^https://[a-z0-9]+\.[a-z]{2,4}$",
        "$randomIP" | "$randomPrivateIP" => r"^\d{1,3}(\.\d{1,3}){3}$",
        "$randomIPV6" => r"^[0-9a-f]{4}(:[0-9a-f]{4}){7}$",
        "$randomMACAddress" => r"^[0-9a-f][02468ace](:[0-9a-f]{2}){5}$",
        "$randomPort" => r"^\d{4,5}$",
        "$randomProtocol" => r"^https?$",
        "$randomHttpMethod" => r"^(GET|POST|PUT|PATCH|DELETE|HEAD|OPTIONS)$",
        "$randomHttpStatus" => r"^[1-5]\d{2}$",
        "$randomUserAgent" => r"^Mozilla/5\.0 \([^)]+\) \S.*\S$",
        "$randomSemver" => r"^\d+\.\d+\.\d+$",
        "$randomCountryCode" | "$randomStateAbbr" => r"^[A-Z]{2}$",
        "$randomCountryCodeAlpha3" | "$randomCurrencyCode" => r"^[A-Z]{3}$",
        "$randomStreetAddress" => r"^\d{1,4} [A-Z][a-z]+ [A-Z][a-z]+$",
        "$randomPostalCode" => r"^\d{5}$",
        "$randomLatitude" | "$randomLongitude" => r"^-?\d{1,3}\.\d{4}$",
        "$randomCoordinates" => r"^-?\d{1,2}\.\d{4},-?\d{1,3}\.\d{4}$",
        "$randomLocale" | "$randomLanguageCode" => r"^[a-z]{2}$",
        "$randomLanguageTag" => r"^[a-z]{2,3}-[A-Z]{2}$",
        "$randomBankAccount" => r"^\d{8}$",
        "$randomBankAccountName" => r"^[A-Z][A-Za-z ]+ Account$",
        "$randomBankAccountIban" => r"^[A-Z]{2}\d{2}[A-Z0-9]{12,30}$",
        "$randomBankAccountBic" => r"^[A-Z]{6}[A-Z0-9]{2}$",
        "$randomCreditCardMask" => r"^\d{4}$",
        "$randomCreditCardNumber" => r"^\d{14,16}$",
        "$randomCreditCardCvv" => r"^\d{3}$",
        "$randomCreditCardExpiry" => r"^(0[1-9]|1[0-2])/\d{2}$",
        "$randomTransactionType" => r"^[a-z]+$",
        "$randomBitcoin" => r"^[13][1-9A-HJ-NP-Za-km-z]{25,34}$",
        "$randomPrice" => r"^\d{1,4}\.\d{2}$",
        "$randomIsbn10" => r"^\d{9}[\dX]$",
        "$randomIsbn13" => r"^97[89]\d{10}$",
        "$randomEan13" => r"^\d{13}$",
        "$randomUpc" => r"^\d{12}$",
        "$randomFileName" | "$randomCommonFileName" => r"^[a-z0-9]+(_[a-z0-9]+)+\.[a-z0-9]+$",
        "$randomFileType" | "$randomCommonFileType" => r"^(application|audio|font|image|model|text|video)$",
        "$randomFileExt" | "$randomCommonFileExt" => r"^[a-z0-9]+$",
        "$randomFilePath" => r"^(/[A-Za-z0-9]+)+/[a-z0-9_]+\.[a-z0-9]+$",
        "$randomDirectoryPath" => r"^(/[A-Za-z0-9]+)+$",
        "$randomMimeType" => r"^[a-z]+/[a-z0-9.+-]+$",
        "$randomAvatarImage" => r"^https://avatars\.githubusercontent\.com/u/\d+$",
        "$randomImageUrl" => r"^https://picsum\.photos/seed/[a-z0-9]+/640/480$",
        "$randomImageDataUri" => r"^data:image/svg\+xml;charset=UTF-8,%3Csvg[A-Za-z0-9%._~-]+%3C%2Fsvg%3E$",
        "$randomHexColor" => r"^#[0-9a-f]{6}$",
        "$randomRgbColor" => r"^rgb\(\d{1,3}, \d{1,3}, \d{1,3}\)$",
        "$randomHslColor" => r"^hsl\(\d{1,3}, \d{1,3}%, \d{1,3}%\)$",
        _ => "",
    };
    if !specific.is_empty() {
        return specific;
    }
    match v.group {
        "Images" => r"^https://loremflickr\.com/640/480/[a-z]+\?lock=\d+$",
        "Databases" => r"^[A-Za-z0-9_]+$",
        "Words" | "People" | "Location" | "Business" | "Commerce" | "Dates & times" | "Finance" | "Colors" => LINE,
        // IDs, numbers, text, internet values and files have a format: list it above.
        group => panic!("{} ({group}) needs a shape", v.name),
    }
}

/// What to generate for a variable in the shape test (some need arguments).
fn sample_expr(v: &DynamicVar) -> String {
    match v.name {
        "$randomFrom" => "$randomFrom(red, green, blue)".into(),
        name => name.into(),
    }
}

#[test]
fn every_postman_variable_exists() {
    assert_eq!(POSTMAN.len(), 118);
    for name in POSTMAN {
        let var = find(name).unwrap_or_else(|| panic!("{name} is missing"));
        assert_eq!(var.name, *name, "exact name");
    }
}

#[test]
fn catalog_is_well_formed() {
    let all = catalog();
    assert!((150..=250).contains(&all.len()), "{} variables", all.len());
    let mut seen = HashSet::new();
    for v in all {
        assert!(v.name.starts_with('$') && v.name[1..].chars().all(|c| c.is_ascii_alphanumeric()), "{}", v.name);
        // Lookups ignore case as a fallback, so names must differ in more than case.
        assert!(seen.insert(v.name.to_ascii_lowercase()), "duplicate {}", v.name);
        assert!(GROUPS.contains(&v.group), "{}: group {}", v.name, v.group);
        assert!(v.description.ends_with('.') && v.description.len() > 8, "{}: {}", v.name, v.description);
        assert!(v.args.is_empty() || (v.args.starts_with('(') && v.args.ends_with(')')), "{}", v.name);
        assert!(!v.example.is_empty(), "{}", v.name);
    }
    // Each group is one run, in display order.
    let mut groups: Vec<&str> = all.iter().map(|v| v.group).collect();
    groups.dedup();
    assert_eq!(groups, GROUPS);

    let info = catalog_info();
    assert_eq!(info.len(), all.len());
    assert_eq!(info[0].name, all[0].name);
    assert_eq!(info.iter().find(|v| v.name == "$randomInt").unwrap().args, "(min, max)");
    assert_eq!(crate::vars::DYNAMIC_VARIABLES.len(), all.len());
}

#[test]
fn every_variable_generates_values_shaped_like_its_example() {
    for v in catalog() {
        let re = Regex::new(shape(v)).unwrap();
        assert!(re.is_match(v.example), "example of {}: {:?}", v.name, v.example);
        let expr = sample_expr(v);
        for _ in 0..40 {
            let value = make(&expr);
            assert!(re.is_match(&value), "{}: {value:?}", v.name);
        }
    }
}

#[test]
fn uuids_have_their_version_and_variant() {
    for _ in 0..200 {
        for (expr, version) in [("$uuid", 4), ("$guid", 4), ("$randomUUID", 4), ("$uuidv7", 7)] {
            let id = uuid::Uuid::parse_str(&make(expr)).unwrap();
            assert_eq!(id.get_version_num(), version, "{expr}");
            assert_eq!(id.get_variant(), uuid::Variant::RFC4122, "{expr}");
        }
    }
}

fn now_ms() -> i128 {
    OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000
}

#[test]
fn time_ordered_ids_sort_by_creation_and_carry_the_time() {
    for expr in ["$uuidv7", "$ulid"] {
        let ids: Vec<String> = (0..2000).map(|_| make(expr)).collect();
        assert!(ids.windows(2).all(|w| w[0] < w[1]), "{expr} values are not strictly increasing");
    }
    // UUID v7: the first 48 bits are Unix milliseconds.
    let v7 = make("$uuidv7").replace('-', "");
    let ms = i128::from_str_radix(&v7[..12], 16).unwrap();
    assert!((now_ms() - ms).abs() < 5_000);
    // ULID: 10 base-32 characters of Unix milliseconds, then 16 of randomness.
    const CROCKFORD: &str = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    let ulid = make("$ulid");
    assert_eq!(ulid.len(), 26);
    assert!(ulid.chars().all(|c| CROCKFORD.contains(c)));
    let ms = ulid[..10].chars().fold(0i128, |n, c| n * 32 + CROCKFORD.find(c).unwrap() as i128);
    assert!((now_ms() - ms).abs() < 5_000);
    // ObjectId: 4 bytes of Unix seconds; snowflake: milliseconds since the Twitter epoch above 22 bits.
    let seconds = i128::from_str_radix(&make("$objectId")[..8], 16).unwrap();
    assert!((now_ms() / 1000 - seconds).abs() < 5);
    let snowflake: i128 = make("$snowflake").parse().unwrap();
    assert!((now_ms() - ((snowflake >> 22) + 1_288_834_974_657)).abs() < 5_000);
    assert_ne!(make("$objectId"), make("$objectId"));
    assert_ne!(make("$snowflake"), make("$snowflake"));
}

fn luhn_ok(number: &str) -> bool {
    let sum: u32 = number
        .bytes()
        .rev()
        .enumerate()
        .map(|(i, b)| {
            let d = u32::from(b - b'0');
            if i % 2 == 1 { (d * 2) / 10 + (d * 2) % 10 } else { d }
        })
        .sum();
    sum.is_multiple_of(10)
}

#[test]
fn card_numbers_pass_luhn_with_the_brand_prefix_and_length() {
    assert!(luhn_ok("4111111111111111") && !luhn_ok("4111111111111112"));
    for (brand, prefixes, len) in CARD_BRANDS {
        for _ in 0..100 {
            let n = make(&format!("$randomCreditCardNumber({brand})"));
            assert_eq!(n.len(), *len, "{brand}: {n}");
            assert!(prefixes.iter().any(|p| n.starts_with(p)), "{brand}: {n}");
            assert!(luhn_ok(&n), "{brand}: {n}");
        }
    }
    for _ in 0..200 {
        assert!(luhn_ok(&make("$randomCreditCardNumber")));
    }
    assert_eq!(make("$randomCreditCardNumber(AMEX)").len(), 15);
}

/// ISO 13616: the rearranged IBAN as a number, mod 97, is 1.
fn iban_ok(iban: &str) -> bool {
    let rearranged = format!("{}{}", &iban[4..], &iban[..4]);
    let digits: String = rearranged.chars().map(|c| c.to_digit(36).unwrap().to_string()).collect();
    digits.as_bytes().chunks(9).fold(0u64, |r, chunk| {
        let part: u64 = std::str::from_utf8(chunk).unwrap().parse().unwrap();
        (r * 10u64.pow(chunk.len() as u32) + part) % 97
    }) == 1
}

fn num(s: &str) -> u64 {
    s.parse().unwrap()
}

#[test]
fn ibans_have_valid_check_digits() {
    assert!(iban_ok("DE89370400440532013000") && iban_ok("GB82WEST12345698765432"));
    assert!(!iban_ok("DE89370400440532013001"));
    let lengths = [
        ("AT", 20),
        ("BE", 16),
        ("CH", 21),
        ("DE", 22),
        ("DK", 18),
        ("ES", 24),
        ("FR", 27),
        ("GB", 22),
        ("IE", 22),
        ("NL", 18),
        ("SE", 24),
    ];
    assert_eq!(lengths.len(), IBAN_FORMATS.len());
    for (country, len) in lengths {
        for _ in 0..100 {
            let iban = make(&format!("$randomBankAccountIban({})", country.to_lowercase()));
            assert!(iban.starts_with(country) && iban.len() == len, "{iban}");
            assert!(iban_ok(&iban), "{iban}");
            let bban = &iban[4..];
            match country {
                "BE" => {
                    let check = num(&bban[..10]) % 97;
                    assert_eq!(num(&bban[10..]), if check == 0 { 97 } else { check }, "{iban}");
                }
                "FR" => {
                    let sum = 89 * num(&bban[..5]) + 15 * num(&bban[5..10]) + 3 * num(&bban[10..21]) + num(&bban[21..]);
                    assert_eq!(sum % 97, 0, "{iban}");
                }
                "ES" => {
                    let digit = |s: &str| {
                        let weights = [1, 2, 4, 8, 5, 10, 9, 7, 3, 6];
                        let sum: u64 = s.bytes().zip(weights).map(|(b, w)| u64::from(b - b'0') * w).sum();
                        match 11 - sum % 11 {
                            11 => 0,
                            10 => 1,
                            d => d,
                        }
                    };
                    assert_eq!(digit(&format!("00{}", &bban[..8])), num(&bban[8..9]), "{iban}");
                    assert_eq!(digit(&bban[10..]), num(&bban[9..10]), "{iban}");
                }
                "NL" => {
                    let sum: u64 =
                        bban[4..].bytes().enumerate().map(|(i, b)| u64::from(b - b'0') * (10 - i as u64)).sum();
                    assert_eq!(sum % 11, 0, "{iban}");
                }
                _ => {}
            }
        }
    }
    for _ in 0..200 {
        assert!(iban_ok(&make("$randomBankAccountIban")));
    }
}

/// GS1 (EAN-13, UPC-A, ISBN-13): weights 1 and 3 from the right, total a multiple of 10.
fn gs1_ok(code: &str) -> bool {
    let sum: u32 =
        code.bytes().rev().enumerate().map(|(i, b)| u32::from(b - b'0') * if i % 2 == 1 { 3 } else { 1 }).sum();
    sum.is_multiple_of(10)
}

fn isbn10_ok(isbn: &str) -> bool {
    let sum: u32 = isbn
        .chars()
        .enumerate()
        .map(|(i, c)| if c == 'X' { 10 } else { c.to_digit(10).unwrap() } * (10 - i as u32))
        .sum();
    sum.is_multiple_of(11)
}

#[test]
fn barcodes_and_isbns_have_valid_check_digits() {
    assert!(gs1_ok("4006381333931") && !gs1_ok("4006381333932"));
    assert!(isbn10_ok("0306406152") && isbn10_ok("080442957X") && !isbn10_ok("0306406153"));
    for _ in 0..300 {
        let (isbn13, ean, upc, isbn10) =
            (make("$randomIsbn13"), make("$randomEan13"), make("$randomUpc"), make("$randomIsbn10"));
        assert!(gs1_ok(&isbn13) && isbn13.starts_with("978"), "{isbn13}");
        assert!(gs1_ok(&ean), "{ean}");
        assert!(gs1_ok(&upc), "{upc}");
        assert!(isbn10_ok(&isbn10), "{isbn10}");
    }
}

#[test]
fn bitcoin_addresses_have_a_valid_checksum() {
    const BASE58: &[u8] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
    let decode = |s: &str| {
        let mut bytes: Vec<u8> = Vec::new(); // least significant first
        for c in s.bytes() {
            let mut carry = BASE58.iter().position(|b| *b == c).unwrap() as u32;
            for b in &mut bytes {
                carry += u32::from(*b) * 58;
                *b = (carry & 0xff) as u8;
                carry >>= 8;
            }
            while carry > 0 {
                bytes.push((carry & 0xff) as u8);
                carry >>= 8;
            }
        }
        let mut out = vec![0u8; s.bytes().take_while(|b| *b == b'1').count()];
        out.extend(bytes.iter().rev());
        out
    };
    let valid = |s: &str| {
        let raw = decode(s);
        let check = sha2::Sha256::digest(sha2::Sha256::digest(&raw[..21]));
        raw.len() == 25 && (raw[0] == 0 || raw[0] == 5) && raw[21..] == check[..4]
    };
    assert!(valid("1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa"));
    for _ in 0..200 {
        let address = make("$randomBitcoin");
        assert!(valid(&address), "{address}");
    }
}

#[test]
fn network_values_parse() {
    for _ in 0..300 {
        make("$randomIP").parse::<Ipv4Addr>().unwrap();
        make("$randomIPV6").parse::<Ipv6Addr>().unwrap();
        assert!(make("$randomPrivateIP").parse::<Ipv4Addr>().unwrap().is_private());
        let mac = make("$randomMACAddress");
        let bytes: Vec<u8> = mac.split(':').map(|b| u8::from_str_radix(b, 16).unwrap()).collect();
        assert!(bytes.len() == 6 && bytes[0] & 1 == 0, "{mac}");
        let port: u16 = make("$randomPort").parse().unwrap();
        assert!(port >= 1024);
        let traceparent = make("$traceparent");
        assert!(!traceparent[3..35].chars().all(|c| c == '0') && !traceparent[36..52].chars().all(|c| c == '0'));
    }
}

#[test]
fn emails_look_valid() {
    for expr in ["$randomEmail", "$randomExampleEmail"] {
        for _ in 0..300 {
            let email = make(expr);
            let (local, domain) = email.split_once('@').unwrap();
            assert!(!local.is_empty() && !local.starts_with('.') && !local.ends_with('.'), "{email}");
            assert!(!domain.contains('@') && domain.contains('.'), "{email}");
        }
    }
}

#[test]
fn names_fold_to_ascii_for_emails_and_usernames() {
    for name in FIRST_NAMES.iter().chain(LAST_NAMES) {
        let plain = ascii(name);
        assert!(!plain.is_empty() && plain.chars().all(|c| c.is_ascii_alphanumeric()), "{name} -> {plain}");
    }
    assert_eq!(ascii("Élodie"), "Elodie");
    assert_eq!(ascii("O'Brien"), "OBrien");
    assert_eq!(ascii("Wiśniewski"), "Wisniewski");
    assert_eq!(ascii("Müller"), "Muller");
}

fn seconds_between(a: OffsetDateTime, b: OffsetDateTime) -> i64 {
    (a - b).whole_seconds()
}

fn iso(s: &str) -> OffsetDateTime {
    OffsetDateTime::parse(s, &Rfc3339).unwrap_or_else(|e| panic!("{s}: {e}"))
}

/// Parse `s` with `format`; formatting the result must give `s` back, so the weekday matches the date.
fn parse_with(s: &str, format: &[BorrowedFormatItem<'_>]) -> OffsetDateTime {
    let t = PrimitiveDateTime::parse(s, format).unwrap_or_else(|e| panic!("{s}: {e}"));
    assert_eq!(t.format(format).unwrap(), s, "weekday or padding differs");
    t.assume_utc()
}

fn day(s: &str) -> Date {
    Date::parse(s, format_description!("[year]-[month]-[day]")).unwrap_or_else(|e| panic!("{s}: {e}"))
}

#[test]
fn dates_parse_and_offsets_move_them() {
    let now = OffsetDateTime::now_utc();
    let close = |t: OffsetDateTime, expected: i64| (seconds_between(t, now) - expected).abs() <= 5;
    assert!(close(iso(&make("$isoTimestamp")), 0));
    assert!(close(iso(&make("$isoTimestamp(+1d)")), 86_400));
    assert!(close(iso(&make("$isoTimestamp(-7d)")), -7 * 86_400));
    assert!(close(iso(&make("$isoTimestamp(1w2d3h4m5s)")), 9 * 86_400 + 3 * 3600 + 4 * 60 + 5));
    assert!(close(iso(&make("$isoTimestamp(now)")), 0));
    let ts: i64 = make("$timestamp(-1h)").parse().unwrap();
    assert!((now.unix_timestamp() - 3600 - ts).abs() <= 5);
    let ms: i128 = make("$timestampMs(+90s)").parse().unwrap();
    assert!((now_ms() + 90_000 - ms).abs() <= 5_000);
    assert!(close(parse_with(&make("$httpDate(+1h)"), HTTP_DATE_FORMAT), 3600));

    let today = OffsetDateTime::now_utc().date();
    if today == now.date() {
        assert_eq!(day(&make("$today")), today);
        assert_eq!(day(&make("$isoDate")), today);
        assert_eq!(day(&make("$tomorrow")), today.next_day().unwrap());
        assert_eq!(day(&make("$yesterday")), today.previous_day().unwrap());
        assert_eq!(make("$isoDate(+1d)"), make("$tomorrow"));
        assert_eq!(make("$isoDate(-24h)"), make("$yesterday"));
    }

    for _ in 0..200 {
        let t = iso(&make("$randomDateTime(-7d, now)"));
        assert!((-7 * 86_400 - 5..=5).contains(&seconds_between(t, now)), "{t}");
        let t = iso(&make("$randomDateTime(+1h, +2h)"));
        assert!((3600 - 5..=7200 + 5).contains(&seconds_between(t, now)), "{t}");
        let t = iso(&make("$randomDateTime"));
        assert!(seconds_between(t, now).abs() <= 366 * 86_400);
        let d = day(&make("$randomDate(now, +30d)"));
        assert!((0..=31).contains(&(d - today).whole_days()), "{d}");

        let past = parse_with(&make("$randomDatePast(30)"), JS_DATE_FORMAT);
        assert!((-30 * 86_400 - 5..=5).contains(&seconds_between(past, now)), "{past}");
        let future = parse_with(&make("$randomDateFuture(10)"), JS_DATE_FORMAT);
        assert!((-5..=10 * 86_400 + 5).contains(&seconds_between(future, now)), "{future}");
        let recent = parse_with(&make("$randomDateRecent"), JS_DATE_FORMAT);
        assert!((-86_400 - 5..=5).contains(&seconds_between(recent, now)), "{recent}");
        let past = parse_with(&make("$randomDatePast"), JS_DATE_FORMAT);
        assert!((-366 * 86_400..=5).contains(&seconds_between(past, now)), "{past}");
    }
    // The examples are real dates too.
    for name in ["$randomDatePast", "$randomDateFuture", "$randomDateRecent"] {
        parse_with(find(name).unwrap().example, JS_DATE_FORMAT);
    }
    parse_with(find("$httpDate").unwrap().example, HTTP_DATE_FORMAT);
    iso(find("$isoTimestamp").unwrap().example);
}

#[test]
fn birthdates_match_the_age_range() {
    let today = OffsetDateTime::now_utc().date();
    let age = |birth: Date| {
        let mut years = today.year() - birth.year();
        if (today.month() as u8, today.day()) < (birth.month() as u8, birth.day()) {
            years -= 1;
        }
        years
    };
    for _ in 0..300 {
        assert_eq!(age(day(&make("$randomBirthdate(30, 30)"))), 30);
        assert!((18..=80).contains(&age(day(&make("$randomBirthdate")))));
        assert!((0..=1).contains(&age(day(&make("$randomBirthdate(0, 1)")))));
    }
    assert_eq!(make("$randomAge(5, 5)"), "5");
}

#[test]
fn parameterized_forms() {
    assert_eq!(make("$randomInt(5,5)"), "5");
    assert_eq!(make("  $randomInt( 7 , 7 )  "), "7");
    assert_eq!(make("$randomInt(, 0)"), "0", "an empty argument takes the default");
    let three_decimals = Regex::new(r"^[12]\.\d{3}$").unwrap();
    for _ in 0..200 {
        let n: i64 = make("$randomInt(-3, -1)").parse().unwrap();
        assert!((-3..=-1).contains(&n));
        let n: i64 = make("$randomInt(1)").parse().unwrap();
        assert!((1..=1000).contains(&n));
        let f = make("$randomFloat(1, 2, 3)");
        assert!(three_decimals.is_match(&f), "{f}");
        assert!(["a", "b"].contains(&make("$randomFrom(a,b)").as_str()));
        assert!(["x, y", "z)"].contains(&make(r#"$randomFrom("x, y", 'z)')"#).as_str()));
        assert!(["a", ""].contains(&make("$randomFrom(a,)").as_str()));
        let lat: f64 = make("$randomLatitude(40, 41)").parse().unwrap();
        assert!((40.0..=41.0).contains(&lat));
        let price: f64 = make("$randomPrice(10, 20)").parse().unwrap();
        assert!((10.0..=20.0).contains(&price));
    }
    assert_eq!(make("$randomFrom(only)"), "only");
    assert_eq!(make("$randomFloat(0, 0, 0)"), "0");
    assert_eq!(make("$randomLatitude(10, 10)"), "10.0000");
    assert_eq!(make("$randomPrice(5, 5)"), "5.00");
    assert_eq!(make("$uuid()").len(), 36);

    assert_eq!(make("$randomString(24)").len(), 24);
    assert!(make("$randomAlphaNumeric(12)").chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()));
    assert_eq!(make("$randomAlphaNumeric(12)").len(), 12);
    assert_eq!(make("$randomHex(64)").len(), 64);
    assert_eq!(make("$nanoid(10)").len(), 10);
    assert_eq!(make("$randomDigits(4)").len(), 4);
    assert_eq!(make("$randomPassword(32)").len(), 32);
    assert_eq!(make("$randomBase64(3)").len(), 4);
    assert_eq!(make("$randomLoremWords(5)").split(' ').count(), 5);
    assert_eq!(make("$randomWords(4)").split(' ').count(), 4);
    assert_eq!(make("$randomLoremSentence(4)").split(' ').count(), 4);
    assert_eq!(make("$randomLoremParagraphs(2)").lines().count(), 2);
    assert_eq!(make("$randomLoremLines(3)").lines().count(), 3);
    assert_eq!(make("$randomSlug(5)").split('-').count(), 5);
    assert_eq!(make("$randomLoremSlug(2)").split('-').count(), 2);
    assert!(make("$randomImageUrl(100, 200)").ends_with("/100/200"));
    assert!(make("$randomCatsImage(300, 300)").starts_with("https://loremflickr.com/300/300/cats?lock="));
    assert!(make("$randomImageDataUri(32, 16)").contains("width%3D%2232%22%20height%3D%2216%22"));
    assert!(make("$randomBankAccountIban(gb)").starts_with("GB"));
    for (country, code, _) in PHONE_FORMATS {
        assert!(make(&format!("$randomE164Phone({country})")).starts_with(&format!("+{code}")));
    }

    for _ in 0..100 {
        let password = make("$randomStrongPassword(4)");
        assert!(password.chars().any(|c| c.is_ascii_lowercase()), "{password}");
        assert!(password.chars().any(|c| c.is_ascii_uppercase()), "{password}");
        assert!(password.chars().any(|c| c.is_ascii_digit()), "{password}");
        assert!(password.chars().any(|c| !c.is_ascii_alphanumeric()), "{password}");
        let text = make("$randomUnicodeString(40)");
        assert_eq!(text.chars().count(), 40);
        for pool in UNICODE_POOLS {
            assert!(text.chars().any(|c| pool.contains(c)), "{text} lacks one of {pool}");
        }
    }
}

#[test]
fn unknown_names_and_bad_arguments_give_nothing() {
    for expr in [
        "$nope",
        "uuid",
        "$",
        "$randomInt(10, 1)",
        "$randomInt(a)",
        "$randomInt(1.5)",
        "$randomInt(1, 2, 3)",
        "$randomInt(1",
        "$randomInt 5",
        "$uuid(1)",
        "$randomFrom()",
        r#"$randomFrom("a)"#,
        r#"$randomFrom("a" b)"#,
        "$randomString(0)",
        "$randomString(-1)",
        "$randomString(100001)",
        "$randomStrongPassword(3)",
        "$randomFloat(0, 1, 11)",
        "$randomFloat(-1e308, 1e308)",
        "$randomFloat(inf)",
        "$timestamp(+1x)",
        "$timestamp(1h+)",
        "$timestamp(+)",
        "$timestamp(99999999999999999999d)",
        "$isoTimestamp(+999999w)",
        "$randomDateTime(+1d, -1d)",
        "$randomDatePast(0)",
        "$randomBirthdate(80, 18)",
        "$randomLatitude(-91, 0)",
        "$randomPrice(-1, 5)",
        "$randomPrice(0.001, 0.009)",
        "$randomImageUrl(0, 10)",
        "$randomBankAccountIban(XX)",
        "$randomCreditCardNumber(foo)",
        "$randomE164Phone(ZZ)",
    ] {
        assert_eq!(generate(expr), None, "{expr}");
    }
}

#[test]
fn names_are_matched_exactly_then_ignoring_case() {
    assert_eq!(find("$randomIPV6").unwrap().name, "$randomIPV6");
    assert_eq!(find("$randomIpv6").unwrap().name, "$randomIPV6");
    assert_eq!(find("$RANDOMUUID").unwrap().name, "$randomUUID");
    assert!(make("$randomusername").chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_'));
}

/// The website's reference (docs/pages/…/variables/dynamic-variables.md) lists the catalog:
/// the section between its markers is written from it, like the TypeScript bindings (CI
/// checks it is committed up to date).
#[test]
fn docs_list_every_variable() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/pages/src/content/docs/docs/variables/dynamic-variables.md");
    let page = std::fs::read_to_string(&path).expect("the dynamic variables page");
    let start = page.find("<!-- catalog:start").expect("catalog:start marker");
    let start = start + page[start..].find("-->").unwrap() + 3;
    let end = page.find("<!-- catalog:end -->").expect("catalog:end marker");
    let cell = |s: &str| s.replace('|', "\\|").replace('\n', " ");
    let mut out = String::from("\n");
    let mut group = "";
    for v in catalog() {
        if v.group != group {
            group = v.group;
            out.push_str(&format!("\n### {group}\n\n| Variable | Value | Example |\n|---|---|---|\n"));
        }
        let name = if v.args.is_empty() {
            format!("`{{{{{}}}}}`", v.name)
        } else {
            format!("`{{{{{}}}}}`<br/>`{{{{{}{}}}}}`", v.name, v.name, v.args)
        };
        out.push_str(&format!("| {name} | {} | `{}` |\n", cell(v.description), cell(v.example)));
    }
    out.push('\n');
    let updated = format!("{}{}{}", &page[..start], out, &page[end..]);
    if updated != page {
        std::fs::write(&path, updated).expect("write the page");
    }
    for v in catalog() {
        assert!(std::fs::read_to_string(&path).unwrap().contains(&format!("`{{{{{}", v.name)));
    }
}
