---
title: Encoders
description: Base64, URL and hex encoding, JWT decoding, SHA hashes and Unix timestamps, all inside the app.
sidebar:
  order: 2
---

The **Encoders** tool converts the small things you meet while testing APIs: Base64 and URL encoding, hex bytes, JWTs, hashes and timestamps. Open it from the **Tools** sidebar. Everything runs inside the app: nothing you paste is sent anywhere.

Pick a mode at the top: **Base64**, **URL**, **Hex**, **JWT**, **Hash** or **Timestamp**. Each mode keeps its own input while you switch between them. Text is always treated as UTF-8.

## Base64, URL and hex

These three work the same way: choose **Encode** or **Decode**, type or paste on the left, and the result appears on the right as you type, with a copy button. The ⇄ button between them (**Use the result as input and switch direction**) turns the result into the input and flips the direction, handy for checking a round trip. Errors are shown in place of the result.

### Base64

| Direction | Input | Result |
|---|---|---|
| **Encode** | Text | Standard Base64 with `=` padding. With **URL-safe** checked: `-` and `_` instead of `+` and `/`, and no padding (Base64URL, as used in JWTs). |
| **Decode** | Base64 | The text. Standard and URL-safe alphabets are both accepted, padding is optional and whitespace is ignored. When the bytes aren't UTF-8 text, they are shown as hex, with a note such as **Not UTF-8 text: showing the 32 bytes as hex**. |

Errors: **Not valid Base64: unexpected character '…'**, **misplaced '=' padding**, or **the length is wrong (a character is missing or extra)**.

### URL

| Direction | Input | Result |
|---|---|---|
| **Encode** | Text | Percent-encoded for use inside a URL component, such as a query value or a path segment (JavaScript's `encodeURIComponent`): everything except letters, digits and `- _ . ! ~ * ' ( )` is encoded, including `/`, `?`, `&`, `=` and spaces (`%20`). |
| **Decode** | Percent-encoded text | The text. `+` is read as a space, as in form data. |

Errors: **Not valid URL encoding: '%' at position … is not followed by two hex digits**, or **the escapes are not UTF-8 text**.

### Hex

| Direction | Input | Result |
|---|---|---|
| **Encode** | Text | Its UTF-8 bytes as lowercase hex, separated by spaces: `Hello` → `48 65 6c 6c 6f`. |
| **Decode** | Hex bytes | The text. Accepted forms: `48656c6c6f`, `48 65 6c`, `48:65:6c`, `48-65-6c`, `48,65,6c`, `0x48 0x65` and `\x48\x65`. |

Decoding fails with **Not valid hex: unexpected character '…'** or **odd number of digits**, and with **The N bytes are not UTF-8 text** when the bytes don't form text (use Base64 decode to see binary as hex).

## JWT

Paste a JSON Web Token (with or without a leading `Bearer `). The tool shows:

- The **algorithm** from the header (`alg`), as a badge.
- **Expired** (the `exp` time has passed), **Not valid yet** (the `nbf` time is still ahead) or **Not expired**.
- **Times**: **Issued at (iat)**, **Not before (nbf)** and **Expires (exp)** in your local time, relative to now ("in 2 hours", "3 days ago") and as the raw number.
- **Header** and **Payload** as formatted JSON, each with a copy button.

:::caution[The signature is not verified]
The tool decodes a token; it doesn't check its signature. Anyone can create a token that decodes like this. Verify tokens with the issuer's keys in your application.
:::

A token must have three parts separated by dots. A five-part token is an encrypted JWE, which can't be read without the key; the tool says so. Parts that aren't Base64URL, UTF-8 or JSON are reported by name, for example **The payload is not JSON**.

## Hash

Type or paste text to see its **SHA-1**, **SHA-256** and **SHA-512** digests (of the UTF-8 bytes) as lowercase hex, each with a copy button. They update as you type.

SHA-1 is for checksums and legacy systems only; don't rely on it for security. MD5 and keyed hashes (HMAC) are not offered.

## Timestamp

Enter a Unix timestamp or a date; **Now** fills in the current Unix time in seconds.

A number is read as seconds, milliseconds, microseconds or nanoseconds, guessed from its size (as jwt.io and epochconverter do):

| Absolute value | Read as |
|---|---|
| below 100,000,000,000 | seconds |
| below 100,000,000,000,000 | milliseconds |
| below 100,000,000,000,000,000 | microseconds |
| larger | nanoseconds |

Negative numbers and decimals are allowed. Anything else is read as a date, for example `2024-01-31T12:00:00Z` (ISO 8601) or `Wed, 31 Jan 2024 12:00:00 GMT` (RFC 2822).

The tool says how it read the input (**Read as seconds.**) and shows:

| Row | Example for `1700000000` |
|---|---|
| **Unix seconds** | `1700000000` |
| **Unix milliseconds** | `1700000000000` |
| **ISO 8601 (UTC)** | `2023-11-14T22:13:20.000Z` |
| **Local time** | The full date and time in your time zone |
| **Relative** | For example "2 years ago" |

Dates must lie within 100,000,000 days (about 273,000 years) of 1970, or you get **That date is out of range**.

:::tip
To put the current time into a request or a mock answer, you don't need this tool: use the dynamic variables `{{$timestamp}}`, `{{$timestampMs}}` and `{{$isoTimestamp}}`. See [Templates](../../servers/templates/#dynamic-values).
:::
