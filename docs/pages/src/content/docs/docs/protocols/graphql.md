---
title: GraphQL
description: Send GraphQL queries, mutations and subscriptions with a schema-aware editor, variables, operation picking and introspection.
sidebar:
  order: 1
---

A GraphQL request in Zorvik is an ordinary [HTTP request](../../requests/http/) whose body type is **GraphQL**. Everything that works for HTTP requests works here too: params, headers, auth (including OAuth 2.0), folder and workspace inheritance, settings, the cookie jar, scripts and tests, history, the collection runner and load tests. What the GraphQL body adds is a query editor that knows your schema.

## Create a GraphQL request

- In the **Collection** sidebar, open the **New** menu (**+**, or right-click a folder) and choose **New GraphQL request**. You get a `POST` request with an empty GraphQL body, opened on its **Body** tab.
- Or, in any HTTP request, open the **Body** tab and pick **GraphQL** as the body type. A `GET` request (or one without a method) becomes `POST`. If the body was JSON shaped like `{"query": …, "variables": …, "operationName": …}`, its query, variables and operation name are taken over.

## The editor

The **Body** tab of a GraphQL request has three parts:

| Part | What it does |
|---|---|
| **Query** (top) | The GraphQL document: syntax highlighting, completion of fields, arguments and types from the schema, validation against the schema (errors are underlined), documentation on hover. ⌘-click (Ctrl-click on Windows and Linux) on a field opens its type in the schema panel. `{{variables}}` are highlighted like everywhere else. |
| **Variables** (bottom) | The GraphQL variables as JSON. `{{variables}}` may be used inside. Drag the divider to resize. |
| **Schema** panel (right) | The schema explorer, shown with the **Schema** button. |

The toolbar above the editor has:

- **Operation picker**: shown when the document has more than one named operation. Pick the one to run (**Choose operation…** sends no `operationName`). When you rename or delete the chosen operation, the choice is cleared.
- **Prettify**: reformats the query (and beautifies the Variables JSON). Prettifying removes comments; a message tells you when that happened, and the editor's undo brings them back.
- **Refresh schema** (circular arrow): introspects the server again.
- **Schema**: opens or closes the schema panel. Its dot shows the schema state: grey (not loaded), amber (loading), green (**Schema loaded · N types**) or red (the error, on hover).

## What is sent

Zorvik sends the operation as a JSON body:

```json
{"query":"query User($id: ID!) { user(id: $id) { name } }","variables":{"id": "42"},"operationName":"User"}
```

| Key | Sent when | Value |
|---|---|---|
| `query` | always | The query text with `{{variables}}` replaced. |
| `variables` | the Variables editor is not empty | The Variables text with `{{variables}}` replaced, inserted **as typed** (so large integers and decimals keep their precision). |
| `operationName` | an operation is chosen | The name (with `{{variables}}` replaced). A blank name is left out. |

- **Content-Type** is `application/json`, unless the request has its own `Content-Type` header (for example `application/graphql+json`); then yours is sent instead.
- **Method** is the request's method, `POST` for new GraphQL requests.
- The Variables must be valid JSON once `{{variables}}` are replaced, or the request stops with **GraphQL variables are not valid JSON: …**. Placeholders for variables that are not defined are sent as written (and listed in the **Sent with undefined values** warning), as long as the text would be valid JSON with them.

Placeholders are replaced as text, so put quotes around them when the value is a string:

```json title="Variables"
{
  "id": "{{userId}}",
  "limit": {{pageSize}},
  "filter": {"status": "open"}
}
```

## Subscriptions

When the operation that runs is a `subscription`, the request becomes a live one: **Send** turns into **Subscribe**, and the response pane shows the results as they arrive, like a [WebSocket](../websocket/) log. The operation is sent first (marked **subscribe**), then each result appears as a received message; **Unsubscribe** ends it, and the log says when the server completes the subscription or rejects it.

Below the query, the **Variables** pane gets a **Subscription** tab next to it:

| Setting | Default | What it does |
|---|---|---|
| Transport | **WebSocket** | **WebSocket**: the `graphql-transport-ws` protocol (the graphql-ws library, Apollo Server 4 and later, Hasura, most servers). **WebSocket (legacy)**: `subscriptions-transport-ws` (subprotocol `graphql-ws`), Apollo Server 2 and 3. **SSE**: graphql-sse over Server-Sent Events (GraphQL Yoga and others). |
| URL | the request URL | Where subscriptions connect when it isn't the request URL, for example `{{baseUrl}}/subscriptions`. For WebSocket, `http://` and `https://` become `ws://` and `wss://`. |
| Connection params | none | WebSocket only: JSON sent in `connection_init`, where servers often expect the auth token, for example `{"authToken": "{{token}}"}`. |

The request's headers, auth (including OAuth 2.0 tokens), cookies, proxy and TLS settings apply as for other requests. Over SSE the operation is a `POST` with `Accept: text/event-stream`.

**When the server says no.** A server that closes the connection explains why with a close code, which the log puts in words: `4401` (credentials needed), `4403` (forbidden: check the connection params and auth), `4406` (it doesn't speak this protocol: try the other WebSocket transport), `4408` (the connection took too long to start). Validation errors come back as the server's GraphQL errors.

**In collection runs, the CLI and for AI agents**, a subscription is read like an [event stream](../../testing/repeat-and-streams/): until a number of results or a time limit (the request's **Settings** tab, **In collection runs**). Post-response scripts get the results as a JSON array in `pm.response.json()` and as events named `next` in `pm.response.events`. Load tests don't send subscriptions.

## Schema introspection

The schema powers completion, validation, hover docs and the schema panel.

**When it loads.** It loads by itself about 0.7 seconds after the URL stops changing, as long as the URL is `http://` or `https://` (or has no scheme) and its host contains no undefined variables. It is loaded again when you press **Refresh schema**. Each workspace, environment and URL has its own schema, so switching environments may load another one.

**How it is fetched.** Zorvik takes the request as it is (URL, headers, auth including OAuth 2.0 tokens, settings, proxy, TLS settings and cookie jar), replaces the body with the standard introspection query (the one graphql-js and GraphiQL send, with descriptions and deprecated fields) and sends it as `POST`. Introspection requests are not recorded in history.

**Older servers.** When a server rejects the query with GraphQL errors (and not with `401`, `403` or `407`), Zorvik asks once more with the older form of the query, without `specifiedByURL` and `isRepeatable`, which servers from before the October 2021 spec don't know.

**Caching.** Schemas stay in memory for the session, keyed by the resolved URL, headers, OAuth 2.0 client and TLS settings. At most 16 schemas (64 MB of responses) are kept; the least recently loaded go first.

**Limits and errors.**

| Message | Meaning |
|---|---|
| **The schema is larger than 20 MB** | The introspection response is over the 20 MB limit. |
| **The response is not JSON. Is this a GraphQL endpoint?** | A 2xx answer that isn't JSON. |
| **The schema request failed: …** | The server answered with GraphQL errors (for example, introspection is disabled). |
| **The server answered HTTP 401 …** | A non-2xx answer without GraphQL errors. |
| **The response has no schema (`data.__schema`). Is this a GraphQL endpoint?** | JSON without a schema in it. |

When introspection is not available (many production servers turn it off), you can still write and send queries; you just don't get completion and validation.

## The schema panel

The panel starts at the root types (`Query`, `Mutation`, `Subscription`) and lists each type's fields with their arguments, types, descriptions and deprecation reasons. Click a type name to open that type; use back and home to navigate. The search box finds types and fields (up to 100 results).

## Saved format

A GraphQL request is saved like any HTTP request, with `body.type: graphql`:

```yaml title="requests/Users/Get user.yaml"
name: Get user
method: POST
url: "{{baseUrl}}/graphql"
headers:
  - key: Authorization
    value: Bearer {{token}}
body:
  type: graphql
  graphql:
    query: |
      query User($id: ID!) {
        user(id: $id) { id name email }
      }
    variables: '{"id": "{{userId}}"}'
    operationName: User
```

| Field | Default | Description |
|---|---|---|
| `body.graphql.query` | empty | The GraphQL document. |
| `body.graphql.variables` | empty | Variables as JSON text (may contain `{{variables}}`). |
| `body.graphql.operationName` | none | The operation to run. |
| `body.graphql.transport` | `websocket` | Subscriptions: `websocket`, `websocketLegacy` or `sse`. |
| `body.graphql.subscriptionUrl` | the request URL | Subscriptions: where they connect. |
| `body.graphql.connectionParams` | none | WebSocket subscriptions: the `connection_init` payload as JSON text. |

Switching the body to another type keeps the GraphQL fields in the file, so switching back loses nothing. See [Workspace format](../../reference/workspace-format/).

## Not supported

- Subscriptions over HTTP multipart responses (Apollo Router's `multipart/mixed`) are not built in.
- Other request shapes, such as batched arrays of operations or persisted-query hashes, are not built in; send them with a JSON body instead.

:::tip[AI agents]
Agents connected to Zorvik can read an endpoint's schema as SDL with the `graphql_schema` tool, and write queries against it. See [Agent tools](../../agents/tools/).
:::
