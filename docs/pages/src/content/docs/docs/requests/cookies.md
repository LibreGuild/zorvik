---
title: Cookies
description: How the per-workspace cookie jar stores cookies from responses and sends them on later requests, and how to view, delete or turn it off.
sidebar:
  order: 4
---

Zorvik has a **cookie jar** for each workspace, like a browser. Cookies that responses set are stored in it and sent automatically on later requests to matching URLs, so a login request followed by other requests just works.

## How it works

1. A response arrives with `Set-Cookie` headers. Zorvik stores those cookies in the workspace's jar, following the usual cookie rules for domain, path, expiry and `Secure`. Cookies from every redirect hop are stored too.
2. On every later request, the cookies that match its URL are added as a `Cookie` header. On a redirect, the cookies are matched again for each new URL.

A `Domain` attribute without a dot, such as `Domain=com`, is rejected unless it names the request's own host or `localhost`, so a server can't set a cookie for a whole top-level domain.

### Your own Cookie header

You can still set a `Cookie` header yourself. Zorvik then adds the jar's matching cookies to it, except those whose name you already set: your values win.

When a redirect goes to another origin (scheme, host or port), the `Cookie` header you set is dropped, as are `Authorization`, `Proxy-Authorization` and `Host`. Cookies from the jar are still matched for the new URL.

## Seeing cookies

**For one response:** the response's **Cookies** tab lists the cookies that response set, with **Name**, **Value**, **Domain**, **Path**, **Expires** (or **Session**) and **Flags** (`Secure`, `HttpOnly`, `SameSite`). It shows them even when the jar is off.

**For the workspace:** choose the cookie icon in the title bar. The **Cookies** dialog lists every cookie in the jar, grouped by domain, with its value and its expiry date (or **session**).

- Hover a cookie and choose the trash icon to delete it.
- **Clear all** deletes every cookie of this workspace after you confirm. This can't be undone.

The **Info** tab of a response shows the `Cookie` header that was really sent, under **Request sent**.

## Where cookies are kept

| Cookie | Kept |
|---|---|
| With `Expires` or `Max-Age` | Saved in the app data folder (`cookies/`) after each send, until they expire |
| Session cookies (no expiry) | In memory, until Zorvik quits |

Cookies never go into the workspace files. Each workspace has its own jar, keyed by the workspace and its folder on this computer, so a copy of the workspace in another folder starts with an empty jar.

## Turning the jar off

**Settings → Data & privacy → Cookie jar** (on by default). When it's off, Zorvik neither stores cookies from responses nor sends cookies from the jar. `Cookie` headers you set yourself are still sent.

## Where the jar is used

| Sends from | Uses the jar |
|---|---|
| Requests sent from the app | Yes (when the jar is on) |
| Collection runs in the app | Yes, the same workspace jar |
| `zorvik run` on the command line | A fresh, empty jar for each run. See [Command line](../../cli/overview/). |
| The network tools (Tools rail) | No |

## Limits

- Scripts can't read or change the jar: `pm.cookies` and `pm.response.cookies` are not supported. Read `Set-Cookie` from `pm.response.headers` instead. See [pm API reference](../../scripting/pm-reference/).
- AI agents can't read your cookies.
