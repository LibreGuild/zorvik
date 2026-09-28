---
title: Templates in answers
description: "The complete template syntax for mock responses, event streams, greetings and reply rules: request values, the incoming message, variables and dynamic values."
sidebar:
  order: 2
---

Server answers can contain placeholders in double braces. When a server answers, it replaces them with values from the request being answered, the message being replied to, your variables, or generated values such as a fresh UUID:

```json title="A mock route's body"
{
  "id": "{{request.params.id}}",
  "search": "{{request.query.q}}",
  "requestedBy": "{{request.headers.x-user}}",
  "traceId": "{{$uuid}}",
  "region": "{{region}}"
}
```

This page lists everything a template can contain. Templates are plain text substitution: there are no conditions, loops, filters or helper functions.

## Where templates work

| Where | Request values `{{request.…}}` | `{{message}}` | Variables and dynamic values |
|---|---|---|---|
| Mock API: route **body** | Yes | No | Yes |
| Mock API: route **header names and values** | Yes | No | Yes |
| Mock API: route **status**, **method**, **path** | Not templated | | |
| Mock API: **Match only when** values | No | No | Yes |
| Mock API: **Backend URL** (forwarding) | No | No | Yes |
| Event stream server: event **data** and **id** | Yes, from the request that opened the stream | No | Yes |
| Event stream server: event **name** | Not templated | | |
| Event stream server: data of events sent from the traffic panel | Yes, from each client's request | No | Yes |
| WebSocket server: **greeting** | No | No | Yes |
| WebSocket server: rule **replies** | No | Yes | Yes |
| TCP server: **greeting** (text encoding) | No | Yes, always empty | Yes |
| TCP and UDP servers: rule **replies** (text encoding) | No | Yes | Yes |
| TCP and UDP servers: anything in **hex** encoding | Not templated | | |
| Reply rule **patterns** | Not templated | | |
| DNS records, relay target | Not templated | | |

Text you type in a server's traffic panel and send to WebSocket, TCP, UDP or relay clients is different: the app fills in your variables before it sends, like it does for requests.

## Syntax

- A placeholder is a name between `{{` and `}}`. Spaces around the name are allowed: `{{ request.params.id }}` works like `{{request.params.id}}`. The one exception is `{{message}}`, which must be written exactly like that.
- A placeholder can't span lines. `{{}}` and a `{{` without a closing `}}` stay as they are.
- A variable that isn't defined stays as written: `{{token}}` is sent as the text `{{token}}`. (So does a secret variable, see [below](#variables).)
- A request value that isn't there (a missing header, an unknown key) becomes an **empty string**.
- There is no escape syntax. Text that looks like a placeholder but names nothing defined is sent unchanged.
- When `{{` appears inside another pair of braces, only the innermost complete placeholder counts: `{{ a {{host}} }}` becomes `{{ a example.test }}`.

## Request values

Available in mock routes and event stream servers. For an event stream, "the request" is the `GET` that opened the stream.

| Placeholder | Value | Example: `POST /users/42?q=a%20b&tag=x` |
|---|---|---|
| `{{request.method}}` | The method | `POST` |
| `{{request.path}}` | The path as sent, without the query (not decoded) | `/users/42` |
| `{{request.url}}` | Path and query as sent (not decoded) | `/users/42?q=a%20b&tag=x` |
| `{{request.body}}` | The request body as text (bytes that aren't UTF-8 become `�`) | the body |
| `{{request.params.NAME}}` | The path parameter `:NAME` or `{NAME}` of the route, decoded | `{{request.params.id}}` → `42` |
| `{{request.params.*}}` | What a trailing `*` in the route path matched, decoded | `/files/*` on `/files/a/b.txt` → `a/b.txt` |
| `{{request.query.NAME}}` | The query parameter, decoded (`%XX` and `+`). The first one when it repeats. The name is case-sensitive. | `{{request.query.q}}` → `a b` |
| `{{request.headers.NAME}}` | The request header, name in any case. The first one when it repeats. | `{{request.headers.content-type}}` |

In event stream servers the body is always empty and there are no path parameters.

There is no way to reach into the body: `{{request.body.user.name}}` is an unknown key and becomes empty. To echo a field, echo the whole body, or add a route per case with **Body contains** conditions.

:::caution[Values are inserted raw]
Request values are inserted exactly as they are, without JSON or HTML escaping. `{"q": "{{request.query.q}}"}` becomes invalid JSON when `q` contains a `"`. Keep that in mind when a test sends unusual input.
:::

Request values are never expanded again: a client that sends `{{token}}` in its body and gets it back through `{{request.body}}` receives the text `{{token}}`, never the value of your `token` variable.

## `{{message}}`

In WebSocket, TCP and UDP [reply rules](../websocket-and-sse-servers/#reply-rules), `{{message}}` is the message being answered, as text:

- Trailing line breaks (`\r`, `\n`) are removed, so line protocols echo cleanly.
- Bytes that aren't UTF-8 become `�`.
- Like request values, the message is never expanded: a client sending `{{token}}` gets `{{token}}` back.

```text title="A rule: Matches regex ^GET  →  reply"
value of {{message}} at {{$isoTimestamp}}
```

A TCP greeting is rendered like a reply to an empty message, so `{{message}}` is empty there. In a WebSocket greeting, `{{message}}` is not special: it stays as written unless you have a variable named `message`.

## Variables

Every other name is a variable. Servers use:

1. the **active environment's** variables (including values that scripts set on this computer), then
2. the **workspace** variables (their saved values).

The first one that defines a name wins. [Global variables](../../variables/variables-and-environments/) (the ones scripts set) are not used.

:::note[Secret variables are never inserted]
A server answers whoever connects to it, so variables marked **secret** are left out: `{{apiKey}}` stays `{{apiKey}}` in the answer. Use a normal variable for values a mock may give away.
:::

A variable's value may itself contain placeholders for other variables or dynamic values (up to 10 levels deep), but not request values or `{{message}}`.

A running server reads the variables when it starts, and again whenever its settings change while it runs. After you switch the environment or change a variable, restart the server (or make any edit to it) to use the new values. With [`zorvik serve`](../../cli/serve/), `--env` chooses the environment and `--var key=value` sets variables.

## Dynamic values

Names starting with `$` generate a value each time they appear:

| Placeholder | Value | Example |
|---|---|---|
| `{{$uuid}}` | A random UUID (version 4) | `3b241101-e2bb-4255-8caf-4136c566a962` |
| `{{$guid}}`, `{{$randomUUID}}` | Same as `{{$uuid}}` | |
| `{{$timestamp}}` | Unix time in seconds | `1790000000` |
| `{{$timestampMs}}` | Unix time in milliseconds | `1790000000123` |
| `{{$isoTimestamp}}` | The current time in UTC, RFC 3339 with fractional seconds | `2026-09-28T09:15:42.482913Z` |
| `{{$randomInt}}` | A whole number from 0 to 1000 | `417` |
| `{{$randomBoolean}}` | `true` or `false` | `true` |
| `{{$randomAlphaNumeric}}` | One character, `a`–`z` or `0`–`9` | `k` |
| `{{$randomEmail}}` | `user` + four digits + `@example.com` | `user4821@example.com` |

Two `{{$uuid}}` in one answer are two different UUIDs. A variable you define with the same name (for example `$timestamp`) is used instead of the generated value. Other `$` names are not defined and stay as written.

## Limits

A rendered mock response, event or WebSocket greeting can grow to 32 MB; placeholders past that point are left as written. One variable placeholder can expand to at most 16 MB, and variables nested more than 10 levels deep (for example two that refer to each other) are left partly unexpanded.

## Examples

**Echo the request** (a mock route, `ANY /echo`):

```text title="Response body"
{{request.method}} {{request.url}}
Content-Type: {{request.headers.content-type}}

{{request.body}}
```

**Pass a header through** (a response header row):

| Header | Value |
|---|---|
| `X-Request-Id` | `{{request.headers.x-request-id}}` |
| `Location` | `/users/{{request.params.id}}` |

**Personalized event stream** (an event stream server, opened with `GET /events?user=ann`):

| Event | Data | Id |
|---|---|---|
| `hello` | `{"user": "{{request.query.user}}", "env": "{{envName}}"}` | `1` |
| `tick` | `{"at": "{{$isoTimestamp}}"}` | |

**Reply rules** (a TCP server with line framing):

| When | Message | Reply |
|---|---|---|
| Is exactly | `PING` | `PONG` |
| Matches regex | `^GET ` | `value of {{message}}` |
| Contains | `id` | `{{$uuid}}` |
