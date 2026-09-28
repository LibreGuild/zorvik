---
title: OpenAPI contract checks
description: The automatic "Matches the API spec" test for requests imported from an OpenAPI document, what it checks, how to turn it off, and how to update from a new spec version.
sidebar:
  order: 4
---

When you import an OpenAPI (or Swagger 2) document, Zorvik keeps a copy of it in the workspace and remembers which operation each request came from. After every send of such a request, it checks the response against the document and adds the result as a test named **Matches the API spec**:

- Is the status code documented for this operation?
- Does a JSON body match the documented schema: types, required fields, allowed values, lengths and ranges?

You get contract tests without writing any script. The check runs everywhere a request is sent with its tests: single sends in the app, collection runs, `zorvik run`, and requests AI agents send.

## Where the check comes from

Importing an OpenAPI document (see [Import and export](../../requests/import-export/)) creates a folder of requests and:

- saves the document under `specs/` in the workspace folder,
- links the folder to it in `_folder.yaml`,
- names each request's operation in its file.

```yaml title="requests/Petstore/_folder.yaml"
name: Petstore
openapi:
  spec: specs/petstore.json
  source: https://petstore3.swagger.io/api/v3/openapi.json
```

```yaml title="requests/Petstore/pet/Get pet by id.yaml"
name: Get pet by id
method: GET
url: "{{baseUrl}}/pet/{{petId}}"
openapi:
  operation: GET /pet/{petId}
```

`spec` is the kept copy (a path inside the workspace), `source` is where it was imported from (used by **Update from API spec…**), and `operation` is the method and path template as the document writes them.

## The test

The test is named after the operation and the status, and passes or fails like any other test:

```text
✓ Matches the API spec (GET /pet/{petId} → 200)
✗ Matches the API spec (GET /pet/{petId} → 200) — `$.id`: expected integer, got a string ("7")
```

When it fails, its message lists the problems, one per line, each naming the place in the body as a path: `$` is the whole body, `$.owner.name` a nested field, `$[0].id` the `id` of the first item of an array.

In a collection run the check counts like any test: a failing check fails the request, and a request with a passing check is judged by its tests instead of its status (see [When a request passes or fails](../collection-runner/#when-a-request-passes-or-fails)). It appears in JUnit reports like other tests.

### When no check is added

The check is silently left out (no test at all) when:

- the request is not an HTTP request, or has no `openapi.operation`;
- the request is marked as no longer in the spec (see [below](#updating-from-a-new-version-of-the-spec));
- no folder above the request links a document, or the link has `validate: false`;
- the document can't be read: missing, larger than 50 MB, outside the workspace folder (an absolute path, `..`, or a symbolic link on the way), or not a valid OpenAPI or Swagger document;
- the document doesn't have the operation, or documents no responses for it.

When several folders above a request link documents, the innermost one is used.

## What is checked

### Status

The response status must be documented. Zorvik looks for, in order:

1. the exact code, such as `200`;
2. its range, such as `2XX` (in any case);
3. `default`.

If none is there, the test fails: `Status 500 is not documented (documented: 200, 4XX)`.

### Body

The body is checked when the documented response has a schema for a JSON body:

- **Swagger 2**: the response's `schema`.
- **OpenAPI 3.0 and 3.1**: the `content` entry for the response's `Content-Type` (parameters like `charset` ignored); if there's none, the first JSON-like entry (`application/json`, anything ending in `+json`, `application/*` or `*/*`).

The body is **not** checked (only the status is) when the response documents no body schema, or when the response's `Content-Type` is not JSON (for example an HTML error page).

| Problem | Message |
|---|---|
| A schema is documented but the body is empty | `The body is empty, but the spec documents one` |
| The body isn't valid JSON | `The body is not valid JSON (…)` |

### Schema keywords

| Keyword | Checked |
|---|---|
| `type` | `string`, `number`, `integer`, `boolean`, `object`, `array`, `null`; a list of types (OpenAPI 3.1). `integer` accepts whole numbers such as `3.0`. Unknown types pass. |
| `nullable` (OpenAPI 3.0) | `null` is allowed when `nullable: true`, when `type` includes `"null"`, or when the schema has neither `type` nor `enum` |
| `enum` | The value must be one of the listed values (numbers compared by value) |
| `const` | The value must equal it |
| `required` | Each required field must be present, except properties marked `writeOnly` (they never come back in responses) |
| `properties` | Each field present is checked against its schema |
| `additionalProperties` | `false`: fields not in `properties` are reported (`` `$.extra` is not in the spec ``). A schema: extra fields are checked against it. |
| `items` | Each array item is checked (the first 1,000 items) |
| `minItems`, `maxItems` | Array length |
| `minLength`, `maxLength` | String length in characters |
| `minimum`, `maximum` | Number bounds, with `exclusiveMinimum` / `exclusiveMaximum` as booleans (OpenAPI 3.0) or as numbers (OpenAPI 3.1) |
| `allOf` | The value must match every schema |
| `anyOf`, `oneOf` | The value must match at least one of the schemas |
| `$ref` | Local references (`#/components/schemas/Pet`, `#/definitions/Pet`) are followed, also recursively |

**Not checked**: `format` (dates, emails, UUIDs), `pattern`, `multipleOf`, `uniqueItems`, `minProperties`, `maxProperties`, `not`, `discriminator`, `patternProperties`, dependent keywords, references to other files or URLs (they're skipped), response headers, and requests themselves (parameters and request bodies).

`oneOf` is checked like `anyOf`: a value matching more than one of the schemas passes.

### Messages

| Message | Meaning |
|---|---|
| `` `$.id`: expected integer, got a string ("7") `` | Wrong type |
| `` `$` is missing the required field `name` `` | A required field is missing |
| `` `$.status`: "lost" is not one of the documented values `` | Not in `enum` |
| `` `$.extra` is not in the spec `` | An extra field where `additionalProperties: false` |
| `` `$.tag` is null, but the spec doesn't allow null `` | `null` where it isn't allowed |
| `` `$.owner` matches none of one of the oneOf schemas `` | No `oneOf` option matches |
| `` `$.owner` matches none of any of the anyOf schemas `` | No `anyOf` option matches |
| `` `$` has 4 items, more than the 3 allowed `` | `maxItems` |
| `` `$.name` is longer than the documented maximum length `` | `maxLength` |
| `` `$[0].id`: 0 is below the documented minimum 1 `` | `minimum` |
| `` `$.n`: 0 must be above 0 `` | `exclusiveMinimum` (3.1) |

At most 20 problems are listed per response, followed by `… and N more`. Nested schemas are followed up to 64 levels deep.

## Turning it off

For a whole imported folder: right-click the folder → **Folder settings…** → **API spec** tab → turn off **Check responses against the spec**, then **Save**. The **API spec** tab is only there for folders imported from a document. In the file, this is `validate: false`:

```yaml title="requests/Petstore/_folder.yaml"
name: Petstore
openapi:
  spec: specs/petstore.json
  source: https://petstore3.swagger.io/api/v3/openapi.json
  validate: false
```

The document stays linked, so **Update from API spec…** keeps working. Turn the switch back on to check again.

There's no switch for a single request. If one operation is known to differ from its documentation, test it with your own script and turn the check off for the folder, or move that request out of the imported folder.

## Updating from a new version of the spec

When the API changes, bring the folder up to date instead of importing again:

1. Right-click the imported folder → **Update from API spec…** (only shown for folders imported from a document).
2. Choose the new version: **URL**, **File** or **Paste**. The URL or file it was imported from is filled in.
3. Press **Preview changes**. Zorvik lists what would change: **Added**, **Changed** (with the fields it updates and the ones where your edits stay), **No longer in the spec (kept, marked)**, and **New environment variables**.
4. Press **Update** to apply it.

What an update does:

- **New operations** are added as new requests.
- **Changed operations** are updated field by field (method, URL, headers, body, parameters, auth, docs), but only where the saved request still has the value from the old version of the document. Where you edited a field, your version stays.
- **Scripts, request settings and names are never changed.**
- **Operations no longer in the document** are kept and marked: they're crossed out in the sidebar, their tooltip says *(no longer in the API spec)*, and they're no longer checked against the document. If an operation comes back in a later version, the mark is removed.
- **New path variables** are added to the environment named like the folder (created if needed).
- The new version **replaces the kept copy** under `specs/`, so contract checks use it from now on, and it's the version the next update compares against. The folder's `source` is updated when you used a different URL (or a file inside the workspace).

If the old copy is missing from the workspace, Zorvik can't tell your edits from the old document. The preview then warns that URLs, parameters, headers and bodies take the new version while docs and auth stay.

## Keeping it in Git

The kept document (`specs/…`), the folder link and the operation names are ordinary workspace files. Commit them with the requests: then CI runs with `zorvik run` check responses against the same version of the spec as everyone else.
