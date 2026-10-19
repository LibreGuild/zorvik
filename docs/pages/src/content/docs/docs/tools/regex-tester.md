---
title: Regex tester
description: Test a regular expression with the syntax Zorvik's reply rules, wait steps and load test captures use, and see its matches and groups.
sidebar:
  order: 6
---

The **Regex tester** runs a regular expression on a text and highlights what it matches, with its groups. It uses the same regex engine as Zorvik's server [reply rules](../../servers/websocket-and-sse-servers/#reply-rules), wait steps and [load test captures](../../load-testing/data-and-captures/), so a pattern that works here works there. Open it from the **Tools** sidebar.

## Use it

Type a **Pattern** and paste a **Test text** (a response body, a message, a log line). Matches are highlighted in the text as you type, in alternating colors so neighbouring matches stay apart, and the number of matches shows next to the flags.

The flags apply to the whole pattern:

| Flag | Name | Effect |
|---|---|---|
| **i** | Ignore case | Letters match upper and lower case. |
| **m** | Multi-line | `^` and `$` match at the start and end of each line, not only of the text. |
| **s** | Dot matches line breaks | `.` matches `\n` too. |
| **x** | Ignore whitespace | Spaces and `# comments` in the pattern are ignored, for long patterns over several lines. |

Rules and captures have no flag settings: write the flags into the pattern. With a flag on, the tester shows the pattern with its flags written in, for example `(?i)token=(\w+)`, with a copy button.

## The result

- **A reply rule or wait step with this pattern matches this text**: the pattern matches somewhere in it, which is all a rule asks.
- **A load test capture takes …**: the value a capture with this pattern would store, that is the first group of the first match, or the whole first match when the pattern has no group.
- **Matches**: each match with its position in the text and its groups. Named groups (`(?P<token>\w+)` or `(?<token>\w+)`) show their name; a group that took no part in a match says **no part**. Line breaks and tabs in values are shown as `\n` and `\t`.

The first 1,000 matches are highlighted and the first 200 listed; counting stops at 100,000. The text may be up to 10 MB.

## Syntax

The syntax is that of Rust's [`regex` crate](https://docs.rs/regex/latest/regex/#syntax), like RE2. It always runs in time proportional to the text, so no pattern can hang a server or a load test. In exchange there is no look-around (`(?=…)`, `(?<=…)`) and no backreferences (`\1`); the tester explains what is wrong with a pattern, with a pointer to the spot, for example:

```text
regex parse error:
    (?=token)
    ^^^
error: look-around, including look-ahead and look-behind, is not supported
```

Some differences from JavaScript regular expressions:

| JavaScript | Rust `regex` |
|---|---|
| `/abc/i` | `(?i)abc` |
| `(?<name>…)` | `(?<name>…)` or `(?P<name>…)` |
| `\d` matches ASCII digits | `\d` matches any Unicode digit; use `[0-9]` for ASCII only |
| Look-around and `\1` | Not available |

The tester works on text. Reply rules on TCP and UDP servers in hex mode match raw bytes instead; see [TCP, UDP and DNS servers](../../servers/tcp-udp-dns-servers/).
