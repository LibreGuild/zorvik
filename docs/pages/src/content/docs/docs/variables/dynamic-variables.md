---
title: Dynamic variables
description: The built-in {{$...}} variables that generate a fresh UUID, timestamp, random number or email on every send, with their exact formats.
sidebar:
  order: 2
---

Dynamic variables are built in. They start with `$` and produce a new value every time a request is sent, so you don't have to define them anywhere. They use Postman's names, so imported collections work unchanged.

```http
POST {{baseUrl}}/orders
X-Request-ID: {{$uuid}}
Content-Type: application/json

{
  "reference": "test-{{$timestamp}}",
  "customerEmail": "{{$randomEmail}}",
  "quantity": {{$randomInt}}
}
```

## All dynamic variables

| Variable | Value | Example |
|---|---|---|
| `{{$uuid}}` | A random UUID (version 4), lowercase | `3b241101-e2bb-4255-8caf-4136c566a962` |
| `{{$guid}}` | Same as `{{$uuid}}` | `9f1c2b7e-0d4a-4c3e-b8a1-5e6f7a8b9c0d` |
| `{{$randomUUID}}` | Same as `{{$uuid}}` | `c0a8e3f2-7b1d-4e9a-a2c4-6d8e0f1a3b5c` |
| `{{$timestamp}}` | The current time as Unix seconds | `1790588467` |
| `{{$timestampMs}}` | The current time as Unix milliseconds | `1790588467482` |
| `{{$isoTimestamp}}` | The current time in UTC, RFC 3339 (ISO 8601). Fractional seconds are included when not zero. | `2026-09-28T09:41:07.482913Z` |
| `{{$randomInt}}` | A whole number from 0 to 1000, both included | `742` |
| `{{$randomBoolean}}` | `true` or `false` | `true` |
| `{{$randomAlphaNumeric}}` | One character: a lowercase letter `a`–`z` or a digit `0`–`9` | `k` |
| `{{$randomEmail}}` | `user`, four digits (1000–9999) and `@example.com` | `user4821@example.com` |

Type `{{$` in any field to get them as suggestions. Hovering one says *Dynamic value, generated on every send*.

## How they behave

- **A new value for every use.** Two `{{$uuid}}` in the same request get two different UUIDs. Every send, and every iteration of a collection run, gets new values.
- **They work wherever variables work**: the URL, headers, bodies, auth fields and so on. See [Where variables work](../variables-and-environments/#where-variables-work).
- **They are the lowest scope.** A variable you define with the same name, for example `$timestamp` in an environment, replaces the dynamic value. That's handy to pin a value while debugging.
- **Only the names above exist.** Postman has more (such as `$randomFirstName` or `$randomCity`); Zorvik doesn't. An unknown `{{$name}}` is an [undefined variable](../variables-and-environments/#undefined-variables): it is sent as written and listed in the *Sent with undefined values* warning.
- **The command line** generates them the same way (`zorvik run`, `zorvik load`). In load tests, requests that use them are rendered anew for every request. See [Load testing](../../load-testing/overview/).

## Using one value in several places

Because every use gets a new value, generate the value once in a **pre-request script** and use an ordinary variable instead:

```js title="Pre-request script"
pm.variables.set("orderId", pm.variables.replaceIn("{{$uuid}}"));
```

Then use `{{orderId}}` in the URL, headers and body: they all get the same UUID for this send. `pm.variables` values last for this send only (or the whole collection run). To keep a value for later requests, use `pm.environment.set` instead. See [Values set by scripts](../variables-and-environments/#values-set-by-scripts) and the [pm API reference](../../scripting/pm-reference/).

:::note
In scripts, `pm.variables.replaceIn("{{$isoTimestamp}}")` gives the time with milliseconds, such as `2026-09-28T09:41:07.482Z`.
:::

## Copy as cURL or code

When you [copy a request as cURL or code](../../requests/import-export/#copy-as-curl-or-code), dynamic variables are replaced with a freshly generated value, also when **Substitute variables** is off.
