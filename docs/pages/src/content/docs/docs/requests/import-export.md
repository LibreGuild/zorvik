---
title: Import & export
description: Import Postman collections and environments, OpenAPI 3 and Swagger 2 documents and cURL commands, update imported folders from a new API spec, and copy requests as cURL or code.
sidebar:
  order: 9
---

Bring your requests from Postman, an OpenAPI document or a cURL command, and take any request out again as cURL or as code. Imports run entirely on your computer.

## The Import dialog

Open it from:

- the **Collection** sidebar: **+** → **Import…** (or right-click an empty part of the tree);
- a folder: right-click → **Import into folder…**;
- the start screen of the work area, or the empty collection: **Import…**.

It has three tabs:

| Tab | Imports |
|---|---|
| **File** | Choose a file, or drop it on the box. Postman collections and environments, OpenAPI and Swagger documents (JSON or YAML) and saved cURL commands are recognized automatically. |
| **cURL** | A pasted cURL command, opened as a new request |
| **OpenAPI URL** | An OpenAPI or Swagger document downloaded from a URL |

**Import into** chooses the folder the import goes into (**Workspace root** by default).

When an import finishes, the dialog shows *Imported "name"* with the number of requests, folders, environments and workspace variables it created, and **notes** for anything that couldn't be brought over exactly. Choose **Done**.

| Limit | |
|---|---|
| File or download size | 50 MB |
| Unrecognized files | Refused: *Supported: Postman collection v2/v2.1, Postman environment, OpenAPI 3.x / Swagger 2.0 (JSON or YAML), cURL command.* |

## Postman collections

Export the collection from Postman as **Collection v2.1** (v2.0 works too) and import the file. Exports downloaded from the Postman API, which wrap the collection in `{"collection": …}`, work as well.

What you get:

- A new folder named after the collection, holding its folders and requests in the same order.
- The collection's auth and scripts on that folder; each folder's and request's auth, scripts and description on the folder or request.
- Collection variables added to the [workspace variables](../../variables/variables-and-environments/#workspace-variables). A variable the workspace already has keeps its current value, and a note lists it.

| Postman | In Zorvik |
|---|---|
| Pre-request script | Pre-request script |
| Tests script | Post-response script |
| Other script events | Skipped, with a note |
| Auth: basic, bearer, API key, OAuth 1.0, OAuth 2.0 (every grant, implicit too), JWT Bearer, Digest, NTLM, AWS Signature, Hawk, Akamai EdgeGrid, ASAP, no auth, inherit | The same [auth type](../auth/), with its settings |
| Other auth types | **No auth**, with a note |
| Saved responses | [Examples](../responses/#examples) of the request (bodies up to 1 MB) |
| OAuth 2.0 redirect URL on `oauth.pstmn.io` (or none) | `http://127.0.0.1:53682/callback`, with a note to register it |
| Body: raw (JSON, XML, HTML, JavaScript, text), URL-encoded, form-data, file, GraphQL | The matching [body type](../bodies/) |
| Path variables (`:id`) with values | [Path variables](../http/#path-variables) |
| Disabled query parameters, parameter descriptions | Switched-off parameters, with descriptions |
| Settings: follow redirects, max redirects, SSL verification | The request's **Settings** |
| Folder variables | Skipped, with a note |
| Collection v1 files | Refused; export as v2.1 again |

Scripts are imported as they are. Scripts that use APIs Zorvik doesn't have (`pm.sendRequest`, `setTimeout`, `setInterval`, `pm.cookies`, `pm.visualizer`) are listed in a note; they fail when run. See [pm API reference](../../scripting/pm-reference/).

Files for form-data and binary bodies aren't part of a Postman export: choose them again after importing (a note says which).

## Postman environments

Import an environment export file (Postman: **Export** on the environment) to create a new [environment](../../variables/variables-and-environments/#environments) with its variables. Variables of Postman's **secret** type become [secret variables](../../variables/secrets/): their values go to this computer's secret store, never into the workspace file.

A Postman **globals** export imports the same way, as an environment (named "Globals" when the file has no name). Zorvik's own globals are only set by scripts.

## OpenAPI and Swagger

Import OpenAPI 3.0 and 3.1, and Swagger 2.0, as JSON or YAML, from a file or from the **OpenAPI URL** tab.

### What you get

- **A folder** named after the document's `info.title`, with a subfolder per tag (in the order of the document's `tags`), holding the operations with that first tag. Operations without a tag go directly into the folder.
- **One request per operation**, named after its `summary`, else its `operationId`, else `METHOD /path`. The `description` goes into the request's **Docs**; deprecated operations say so there.
- **An environment** named after the document, with `baseUrl` (the first server URL), placeholders for auth, and the path parameters with example values.

| Document | Request |
|---|---|
| Server URL | `{{baseUrl}}` at the start of every URL. An operation or path with its own absolute server gets that URL instead. |
| Path parameter `{session_id}` | `{{sessionId}}` in the URL (camelCase). Generic names get the resource in front: `/pets/{id}` becomes `{{petId}}`. Example values go into the new environment. |
| Required query parameter | In the URL, with its example value when the document gives one |
| Optional query parameter | A switched-off parameter in the **Params** table, with its description |
| Header parameter | A header, switched on if required (`Accept`, `Content-Type` and `Authorization` parameters are ignored, as OpenAPI says) |
| Cookie parameters | One `Cookie` header |
| Request body | An example body (JSON, XML, text, form, multipart or a binary file), from the document's examples, else generated from the schema: enums, formats and property names give realistic values |
| Security schemes | Auth (below) |

| Security scheme | Auth | Environment placeholders |
|---|---|---|
| HTTP Basic | Basic auth | `username`, `password` (secret) |
| HTTP Digest | Digest auth | `username`, `password` (secret) |
| HTTP Bearer | Bearer token | `bearerToken` (secret) |
| API key in a header or query | API key | `apiKey` (secret) |
| OAuth 2.0 client credentials, authorization code, password or implicit flow | OAuth 2.0 with the token and authorization URLs and all scopes | `clientId`, `clientSecret` (secret), and for the password flow `username`, `password` |
| API key in a cookie, other HTTP schemes, OpenID Connect | Not imported, with a note | |

The document-wide `security` becomes the imported folder's auth; operations with different security get their own. When a requirement combines several schemes, only one is imported (with a note). Fill in the placeholders in the new environment. The ones that hold secrets (`password`, `bearerToken`, `apiKey`, `clientSecret`) are already marked secret, so their values stay on your computer.

External `$ref`s (to other files or URLs) aren't followed; a note lists them. Very large documents are cut short with a note rather than slowing the app down.

### The base URL

When the document doesn't say where the API runs (no `servers`, or no `host` in Swagger 2.0), or gives only a relative server URL such as `/v1`, the Import dialog asks for it:

> This API document doesn't say where the API runs. Give its base URL (for example https://api.example.com) and import again.

Type the base URL, such as `https://api.example.com`, and choose **Import**. *It goes into the new environment as baseUrl; you can change it there later.* A relative server path is added after the host you give (`https://api.example.com` + `/v1`), unless your URL has a path of its own.

When you import from the **OpenAPI URL** tab, the scheme and host the document was downloaded from are used as the base URL, so there's nothing to ask.

### The document is kept

The imported document is saved in the workspace's `specs/` folder, and the imported folder remembers it (in its `_folder.yaml`, as `openapi: {spec, source, validate}`). Each request remembers its operation, such as `GET /pets/{petId}`. This enables two things:

- **Contract checks**: every response to an imported request is checked against the documented status codes and schema, as a test named **Matches the API spec**. Switch it off in **Folder settings → API spec → Check responses against the spec**. See [OpenAPI contract checks](../../testing/openapi-contract-checks/).
- **Updating** the folder from a new version of the document (below).

`source` is the URL it was imported from, or its path when the file was inside the workspace folder (a path elsewhere on your computer would mean nothing to teammates).

## Update from the API spec

When the API changes, update the imported folder instead of importing it again:

1. Right-click the imported folder → **Update from API spec…** (only folders imported from OpenAPI have it).
2. Pick the new version: **URL**, **File** or **Paste**. The URL or file it was imported from is filled in when Zorvik knows it.
3. Choose **Preview changes**. Nothing is written yet. The preview lists:
   - **Added**: operations new in the document;
   - **Changed**: operations that differ, with the fields that will be updated and the fields where your edits are kept;
   - **No longer in the spec (kept, marked)**: operations the document dropped;
   - new environment variables for new path parameters.
4. Choose **Update** to apply it, then **Done**.

How it merges, field by field:

| | |
|---|---|
| Fields that can be updated | Method, URL, headers, body, path variables, switched-off parameters and parameter descriptions, auth, docs |
| Your edits win | A field is updated only where the saved request still has the old document's value. A field you changed stays yours and is listed as kept. |
| Never touched | Request names, scripts and settings |
| Removed operations | Kept, so nothing you built is lost, and shown crossed out in the sidebar (*no longer in the API spec*). If a later version brings the operation back, it is restored. |
| New operations | Added to the folder (and the tag subfolder) |
| New path parameters | Added to the folder's environment |
| The kept document | Replaced by the new version, so the next update compares against it |
| Old document missing | When the old version is no longer in `specs/`, edits can't be told apart: URLs, parameters, headers and bodies take the new version, docs and auth stay. The preview warns about this. |

If the document says nothing new, the preview says *Nothing to change: the folder already matches this version.*

## cURL

**Import → cURL**: paste a command and choose **Open as new request**. It opens in a new, unsaved tab; save it with <kbd>Mod</kbd>+<kbd>S</kbd>. If anything couldn't be imported exactly, a message lists it. A cURL command saved in a file imports from the **File** tab too, straight into the collection.

Commands copied from browser dev tools ("Copy as cURL"), API docs or a terminal work, written for:

- bash, zsh and other POSIX shells (`\` line continuations, `'…'`, `"…"` and `$'…'` quoting);
- Windows Command Prompt (`^` continuations and escapes);
- PowerShell (`` ` `` continuations, `curl.exe`).

A leading `$ ` prompt is ignored. Only the first command is imported.

| Option | Becomes |
|---|---|
| `-X`, `--request` | The method |
| `-H`, `--header` | A header (`Name:` with no value, which removes a header in curl, is skipped) |
| `-d`, `--data`, `--data-ascii`, `--data-binary`, `--data-raw`, `--data-urlencode` | The body. JSON, XML or form data by the `Content-Type` header; JSON-looking data without a `Content-Type` becomes JSON (with a note). A JSON body that is exactly a GraphQL request, sent to a `…/graphql` URL or starting like a GraphQL document, becomes a GraphQL body. |
| `-d @file` (one data argument) | A binary file body |
| `--json` | A JSON body, plus `Accept: application/json` |
| `-F`, `--form`, `--form-string` | A multipart body; `name=@path` becomes a file field (with `;type=` as its content type) |
| `-T`, `--upload-file` | A binary file body; the method becomes `PUT` |
| `-G`, `--get` | The data goes into the query string, with `GET` |
| `-I`, `--head` | `HEAD` |
| `-u`, `--user` | Basic auth; Digest with `--digest`, NTLM with `--ntlm` (`DOMAIN\user` stays the user name) |
| `--oauth2-bearer` | Bearer token |
| `-b`, `--cookie` with `name=value` | A `Cookie` header (cookie files are ignored, with a note) |
| `-A`, `--user-agent`; `-e`, `--referer` | `User-Agent`; `Referer` |
| `-k`, `--insecure` | **Verify TLS certificates**: Off |
| `-L`, `--location` | **Follow redirects**: On |
| `--max-redirs`, `-m`/`--max-time` | **Max redirects**, **Timeout** |
| `-0`/`--http1.0`, `--http1.1`, `--http2`, `--http3` | **HTTP version** |
| `--url`, `--url-query`, `-g`/`--globoff` | The URL, extra query parameters, literal `[]{}` |

Without `-X`, the method follows curl: `POST` with data or form fields, `PUT` with `-T`, `HEAD` with `-I`, else `GET`.

Proxy options (`-x`, `--socks5`, …) and certificate options (`--cacert`, `--cert`, `--key`, …) are ignored with a note: set those in [Proxies](../proxies/) and [TLS & certificates](../tls-and-certificates/). Options that only change curl's output (`-s`, `-v`, `-o`, …) are ignored silently; other unsupported options are listed in the note.

## Copy as cURL or code

Copy any HTTP request as a command or as a short program:

- the request's **More actions** (⋯) → **Copy as cURL or code…**;
- or right-click a request in the sidebar → **Copy as cURL or code…**.

The dialog lists the languages on the left (type in the search box to find one by name, library or platform: `axios`, `android`, `flutter`, `.net`; <kbd>↑</kbd> and <kbd>↓</kbd> move through the list), shows the code with syntax highlighting, and **Copy** copies it. Where a language has several libraries (or shells), pick one above the code. Zorvik remembers your last choice.

| Language | Libraries | Notes |
|---|---|---|
| **cURL** | bash / zsh, Windows cmd, PowerShell (`curl.exe`) | bash / zsh is the default on macOS and Linux, Windows cmd on Windows. Binary bodies go through base64 (bash) or a temporary file (cmd, PowerShell), so every byte arrives |
| **HTTPie** | HTTPie 3 | A shell command |
| **Wget** | Wget 1.15+ | A shell command; binary bodies go through a temporary file |
| **PowerShell** | `Invoke-WebRequest` | PowerShell 7 |
| **JavaScript** | `fetch`, axios | `fetch`: browsers and Node.js 18+; axios: Node.js (an ES module) |
| **Python** | requests, HTTPX | |
| **Go** | `net/http` | A complete program, standard library only |
| **Java** | `java.net.http.HttpClient` | Java 11+; runs with `java Main.java` |
| **Kotlin** | OkHttp | OkHttp 4 (Android) |
| **Swift** | URLSession | iOS and macOS |
| **C#** | `HttpClient` | .NET 6+ top-level program |
| **PHP** | the curl extension | |
| **Ruby** | `Net::HTTP` | |
| **Rust** | reqwest | With Tokio |
| **Dart** | `package:http` | Flutter and Dart |
| **C** | libcurl | |

What's in it:

- The method, the URL, the headers and the body **as Zorvik would send them**: inherited headers, auth (including the cached OAuth 2.0 token, or `<access-token>` when there is none), `Content-Type` and the encoded body.
- **Substitute variables** (on by default) resolves `{{variables}}` with the active environment. Switch it off to keep them as `{{name}}`. Dynamic variables such as `{{$uuid}}` get a value either way.
- Not included: headers Zorvik adds at send time (`User-Agent`, `Accept`, `Accept-Encoding`), cookies from the jar, pre-request scripts, and the request's settings (timeout, redirects, TLS verification, HTTP version).
- Each snippet leaves out what its library does by itself, with a comment saying why: `Content-Length` and `Host` everywhere; `Accept-Encoding` where the library adds it and decompresses (OkHttp, URLSession, axios, Dart, PowerShell); a body with `GET` or `HEAD` where the library can't send one (OkHttp, URLSession, `fetch`, PowerShell). Java's `HttpClient` refuses `Connection`, `Expect` and `Upgrade`, so those are left out too. A header sent twice is kept twice where the library allows it, and joined (`, `, or `; ` for `Cookie`) where it holds one value per name. Bodies that aren't text are embedded as Base64 (C and Rust: as bytes).
- **Comments at the top say what the code can't do** that Zorvik does: answer a Digest or NTLM challenge (the code sends no credentials), and sign each request for AWS Signature V4, OAuth 1.0, JWT, Hawk, Akamai EdgeGrid and ASAP (the copied signature soon expires). `<access-token>` is explained when there is no OAuth 2.0 token yet.

:::caution
With **Substitute variables** on, the result contains the real values, secrets included. Be careful where you paste it.
:::

AI agents can export requests the same way. See [AI agents](../../agents/setup/).
