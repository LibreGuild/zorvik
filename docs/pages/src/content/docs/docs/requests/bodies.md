---
title: Request bodies
description: Every body type an HTTP request can send (JSON, text, XML, forms, multipart, binary files and GraphQL), with the Content-Type Zorvik sets and the limits on files.
sidebar:
  order: 2
---

The **Body** tab of an HTTP request chooses what the request sends. Pick the type in the menu at the top of the tab; the tab label then shows it, for example **Body** `JSON`.

Zorvik keeps what you typed for every type. Switching from JSON to Form and back loses nothing; only the selected type is sent.

## Body types

| Type (menu) | In the file (`body.type`) | Sends | `Content-Type` added |
|---|---|---|---|
| **None** | `none` | No body | — |
| **JSON** | `json` | The text as typed | `application/json` |
| **Text** | `text` | The text as typed | The type chosen next to the menu (default `text/plain`) |
| **XML** | `xml` | The text as typed | `application/xml` |
| **Form URL-encoded** | `formUrlencoded` | `key=value&…`, URL-encoded | `application/x-www-form-urlencoded` |
| **Multipart form** | `multipart` | Text fields and files | `multipart/form-data; boundary=…` |
| **Binary file** | `binary` | The bytes of one file | Guessed from the file name |
| **GraphQL** | `graphql` | `{"query", "variables", "operationName"}` as JSON | `application/json` |

Bodies are sent with any method, `GET` included, as long as the type isn't **None**.

### Content-Type

Zorvik adds the `Content-Type` above only when no `Content-Type` header is set, whether on the request, a folder or the workspace. To send JSON as `application/vnd.api+json`, keep the body type **JSON** and add that `Content-Type` header yourself.

## JSON, Text and XML

These types have a code editor with syntax highlighting, search (<kbd>Mod</kbd>+<kbd>F</kbd>) and `{{variable}}` highlighting and suggestions.

```json
{
  "name": "{{name}}",
  "email": "{{$randomEmail}}",
  "age": {{age}}
}
```

- Variables are replaced as text, anywhere in the body. Put quotes around them for JSON strings (`"{{name}}"`); leave the quotes out for numbers, booleans and objects (`{{age}}`).
- Zorvik doesn't check that a JSON body is valid before sending it. **Beautify** (JSON only) re-indents the body with two spaces and keeps numbers, string escapes and `{{variables}}` exactly as typed. When the body isn't valid JSON, it says **Body is not valid JSON** and changes nothing.
- **Text** has a second menu for its `Content-Type`: `text/plain`, `text/html`, `text/csv`, `application/javascript`, `application/graphql` or `application/yaml`. A different type set in the file (for example by an import) is kept and shown there too.

## Form URL-encoded

A table of keys and values, like the query parameter table:

| Key | Value |
|---|---|
| `grant_type` | `password` |
| `username` | `{{user}}` |
| `note` | `a & b` |

With `user` set to `alice`, this sends `grant_type=password&username=alice&note=a+%26+b`.

- Keys and values can contain `{{variables}}`; they are resolved first, then URL-encoded (a space becomes `+`).
- Switched-off rows and rows without a key are not sent.
- Rows can be reordered by dragging.

## Multipart form

Each row is one part. The menu before the value chooses **Text** or **File**:

- **Text**: the value is sent as the part's content.
- **File**: the value is a file path. Choose **Browse** to pick one. The part gets the file's name as `filename` and a `Content-Type` guessed from its extension (`application/octet-stream` when unknown).

```text
------ZorvikBoundary3f2a…
Content-Disposition: form-data; name="description"

Profile photo
------ZorvikBoundary3f2a…
Content-Disposition: form-data; name="photo"; filename="avatar.png"
Content-Type: image/png

<the file's bytes>
------ZorvikBoundary3f2a…--
```

- Part names, text values and file paths can contain `{{variables}}`.
- A part can carry its own `contentType` in the request file (imports from Postman and cURL set it). On a file part it replaces the guessed type; on a text part it adds a `Content-Type` line. There is no field for it in the table.
- The boundary is `----ZorvikBoundary` followed by a random id, new for every send.

See [Files](#files) for which files may be sent.

## Binary file

Sends the bytes of one file as the whole body. Type the path or choose **Browse…**. The path can be absolute, or relative to the workspace folder, and can contain `{{variables}}`.

`Content-Type` is guessed from the file name, for example `application/pdf` for `report.pdf`, and is `application/octet-stream` when the extension is unknown. Set a `Content-Type` header to send something else.

## Files

Multipart file parts and binary bodies read files from disk when the request is sent. The same rules apply to both:

| Rule | Detail |
|---|---|
| Relative paths | Resolved against the workspace folder, so they work for everyone who clones it. Prefer these for files you commit. |
| Only files inside the workspace | By default, a path that points outside the workspace folder, through `..` or a symbolic link included, is refused: *Body file '…' is outside the workspace folder.* This stops a shared workspace from uploading private files such as `~/.ssh/id_rsa`. Turn on **Settings → Data & privacy → Files outside the workspace** to allow any file on this computer. |
| Size | At most 100 MB per file. |
| Regular files only | Devices, pipes and folders are refused. |
| Missing file | The request fails before sending: *Body file '…'* and the reason. A binary body with no path says *No file selected for the request body*. |

## GraphQL

**GraphQL** sends a GraphQL operation as JSON:

```json
{"query": "query User($id: ID!) { user(id: $id) { name } }", "variables": {"id": "42"}, "operationName": "User"}
```

- The query, the variables (JSON text) and the operation name can all contain `{{variables}}`.
- After variables are resolved, the variables must be valid JSON, or the request fails with *GraphQL variables are not valid JSON*. Undefined `{{variables}}` are sent as written and reported, like in other bodies.
- Variables are sent exactly as typed, so large numbers keep their precision. Empty variables and an empty operation name are left out.
- Choosing **GraphQL** for a `GET` request switches the method to `POST`. When the current body is JSON shaped like `{"query": …}`, its query, variables and operation name are carried over.

The GraphQL editor fetches the endpoint's schema for autocomplete and checks, using the request's own URL, headers and auth. New GraphQL requests (**New → GraphQL request**) start with this body type.

## In the request file

The request file keeps the data of every type you filled in; `type` decides what is sent.

```yaml
body:
  type: multipart
  multipart:
    - key: description
      value: Profile photo
    - key: photo
      value: fixtures/avatar.png
      file: true
      contentType: image/png
  text: |
    { "draft": true }
```

| Field | Used by |
|---|---|
| `type` | Which of the fields below is sent |
| `text` | `json`, `text`, `xml` |
| `contentType` | `text` (default `text/plain`) |
| `form` | `formUrlencoded`: rows of `key`, `value`, `enabled`, `description` |
| `multipart` | `multipart`: rows of `key`, `value`, `file`, `contentType`, `enabled` |
| `file` | `binary`: the file path |
| `graphql` | `graphql`: `query`, `variables` (JSON text), `operationName` |

See [Workspace format](../../reference/workspace-format/).
