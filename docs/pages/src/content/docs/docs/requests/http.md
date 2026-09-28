---
title: HTTP requests
description: Methods, the URL, query parameters, path variables and headers of an HTTP request, and how Zorvik builds what it sends.
sidebar:
  order: 1
---

An HTTP request in Zorvik is a method, a URL, headers, an optional body, auth and a few settings. This page covers the first four; the other parts have their own pages.

![A request with its Params tab, and the JSON response beside it](/zorvik/shots/request-dark.webp)

## The request editor

Across the top is the **URL bar**: the method, the URL, **Send**, the save button and **More actions** (⋯). Below it are the request's tabs:

| Tab | Holds | See |
|---|---|---|
| **Params** | Query parameters and path variables | [below](#query-parameters) |
| **Headers** | Request headers | [below](#headers) |
| **Body** | The body; the tab shows its type, e.g. `JSON` | [Request bodies](../bodies/) |
| **Auth** | How the request authenticates; the tab shows the type unless it inherits | [Auth](../auth/) |
| **Settings** | Timeout, redirects, TLS verification, HTTP version and decompression for this request | [HTTP versions, timeouts & redirects](../http-versions-timeouts-redirects/#per-request-settings) |
| **Scripts** | Pre-request and post-response JavaScript; a dot shows when there are scripts | [Scripts & tests](../../scripting/overview/) |
| **Docs** | Notes about the request in Markdown, saved with it | |

The **Params** and **Headers** tabs show how many enabled rows they have.

**More actions** (⋯) has **Copy as cURL or code…**, **Load test this request…** and **Save as…** (save a copy under another name or folder).

## Method

Open the method menu on the left of the URL to choose `GET`, `POST`, `PUT`, `PATCH`, `DELETE`, `HEAD` or `OPTIONS`.

For any other method, type it into **Custom method…** at the bottom of the menu and press <kbd>Enter</kbd>, for example `PROPFIND` or `PURGE`. Custom methods may use the letters A–Z, digits, `_` and `-`, and are uppercased as you type.

New requests start as `GET`. Switching the body to GraphQL changes a `GET` to `POST`.

In the sidebar and tab badges, long method names are shortened: `DEL`, `OPT`, `PTCH`, and any method longer than five letters shows its first four followed by `…`.

## URL

Type or paste the full address into the URL field:

```text
https://api.example.com/v2/users/:id/orders?status=open&limit=20
```

- **Scheme**: `http://` or `https://`. A URL without a scheme is sent as `http://`. Other schemes belong to other request kinds (`ws://`, `grpc://`, …).
- **Variables**: `{{name}}` can appear anywhere in the URL, for example `{{baseUrl}}/users`. Known variables are highlighted, undefined ones turn red, and hovering one shows its value and where it comes from. Type `{{` for suggestions. See [Variables & environments](../../variables/variables-and-environments/).
- **Credentials**: `https://user:password@host/` is not sent in the URL. Like curl, Zorvik turns it into an `Authorization: Basic …` header, unless the request already has an `Authorization` header.
- **Fragment**: anything after `#` is never sent.
- **IPv6**: put the address in brackets, `http://[::1]:8080/`.

Press <kbd>Enter</kbd> in the URL field (or <kbd>Mod</kbd>+<kbd>Enter</kbd> anywhere) to send. <kbd>Mod</kbd>+<kbd>L</kbd> jumps to the URL field.

:::caution[Undefined host]
When the host part of the URL (or the start of the URL) is a variable that isn't defined, Zorvik stops before sending. The response pane shows **Undefined variable**, for example `{{baseUrl}} in the URL is not defined`, with an **Open environments** button. Variable names are case-sensitive.

Undefined variables anywhere else (in the path, query, headers or body) don't stop the request: they are sent as written, such as `{{userId}}`, and a warning above the response lists them: **Sent with undefined values: …**
:::

## Query parameters

The **Params** tab shows the URL's query string as a table under **Query parameters**. The table and the URL are two views of the same thing: edit either and the other follows.

- Each row is one `key=value` pair, in URL order. A key without `=` (such as `?verbose`) shows with an empty value.
- The checkbox switches a row off. A switched-off parameter leaves the URL but stays in the table (it is saved as `disabledParams` in the request file), so you can switch it back on later.
- Drag the handle on the left of a row to reorder parameters. The trash icon removes one.
- Values appear exactly as in the URL; they are not decoded. When you edit the table, `&`, `#` and `=` in keys and `&` and `#` in values are percent-encoded so they can't break the query string. Other characters are left as you typed them.
- Keys and values can contain `{{variables}}`.
- Parameters imported with a description (from Postman or OpenAPI) show an **ⓘ** next to the name; hover it to read the description.

:::tip
Parameters that API keys add (API key auth set to **Query parameter**) are not in the table. They are appended when the request is sent. See [Auth](../auth/#api-key).
:::

## Path variables

A path segment that starts with `:` is a **path variable**:

```text
{{baseUrl}}/users/:userId/orders/:orderId
```

Each one gets a row under **Path variables** in the **Params** tab, in the order it appears. Fill in the values there:

| Key | Value |
|---|---|
| `userId` | `42` |
| `orderId` | `{{orderId}}` |

Sends `…/users/42/orders/<value of orderId>`.

- Rows follow the URL: adding `:name` to the path adds a row; removing it removes the row. You can't add, rename or switch off rows by hand.
- Only whole path segments count. `:name` in the host, the port or the query string is left alone, so `http://localhost:8080/` is not a path variable.
- The name is everything after `:` up to the next `/`.
- Values can contain `{{variables}}`. After variables are resolved, the value is percent-encoded as one segment: a space becomes `%20` and `/` becomes `%2F`.
- A path variable with an empty value is sent as written (`:orderId`) and listed in the **Sent with undefined values** warning.
- The same name used twice in one path gets one row, and both places get its value.

Path variables are stored in the request file as `pathParams`.

:::note
Requests imported from OpenAPI use `{{variables}}` instead of `:name` for path parameters (for example `{{petId}}`), with example values in the new environment. See [Import & export](../import-export/#openapi-and-swagger).
:::

## Headers

The **Headers** tab is a table of header names and values.

- Header names are suggested as you type: `Accept`, `Authorization`, `Content-Type`, `X-Request-ID` and other common ones.
- Names and values can contain `{{variables}}`.
- The checkbox switches a header off without deleting it. Rows can be reordered by dragging.
- **Bulk edit** turns the table into text, one `Name: value` per line. A line starting with `//` is a switched-off header. **Table** switches back.

```text title="Bulk edit"
Accept: application/json
X-Request-ID: {{$uuid}}
//X-Debug: true
```

### Headers Zorvik adds

Unless you set them yourself, Zorvik adds:

| Header | Value | When |
|---|---|---|
| `Host` | The URL's host and port | HTTP/1.1 (HTTP/2 and HTTP/3 send it as `:authority`) |
| `User-Agent` | `Zorvik/` and the app version | **Settings → Requests → Default headers** is on (the default) |
| `Accept` | `*/*` | **Default headers** is on |
| `Accept-Encoding` | `gzip, deflate, br, zstd` | **Default headers** and **Decompress responses** are both on |
| `Content-Type` | From the body type | The request has a body. See [Request bodies](../bodies/#content-type). |
| `Content-Length` | The body's size | The request has a body, or its method is `POST`, `PUT` or `PATCH` |
| `Authorization`, or an API key header | From the auth settings | See [Auth](../auth/) |
| `Cookie` | Matching cookies from the jar | The cookie jar is on. See [Cookies](../cookies/). |

The response's **Info** tab lists every header that was really sent, under **Request sent**.

### Rules

- **Inherited headers.** Headers from the workspace (**Workspace settings → Default headers**) and from each folder around the request (**Folder settings → Headers**) are sent too. When two levels set the same name, compared without regard to case, the one closest to the request wins. See [Workspaces](../../getting-started/workspaces/#how-settings-flow-down).
- **`Content-Length` and `Transfer-Encoding`** that you set are ignored. Zorvik always frames the body itself.
- **`Host`**: a `Host` header you set replaces the URL's host. On HTTP/2 and HTTP/3 it becomes the `:authority` pseudo-header.
- **HTTP/2 and HTTP/3** don't allow connection-specific headers. `Connection`, `Keep-Alive`, `Proxy-Connection`, `Transfer-Encoding` and `Upgrade` are left out there, and on HTTP/3 a `TE` header other than `TE: trailers` is left out too.
- **Line breaks** in a header value are not allowed; the request fails with *Invalid value for header*.
- Rows with an empty name, and switched-off rows, are not sent.

## In the request file

Everything on this page is stored in the request's YAML file:

```yaml title="requests/Orders/Get order.yaml"
name: Get order
seq: 1
method: GET
url: "{{baseUrl}}/users/:userId/orders/:orderId?expand=items"
disabledParams:
  - key: debug
    value: "true"
    enabled: false
pathParams:
  - key: userId
    value: "42"
  - key: orderId
    value: "{{orderId}}"
headers:
  - key: Accept
    value: application/json
  - key: X-Debug
    value: "1"
    enabled: false
docs: |
  Returns one order with its items.
```

Enabled query parameters are part of `url`; switched-off ones are in `disabledParams`, and descriptions of enabled ones are in `paramDescriptions`. See [Workspace format](../../reference/workspace-format/).
