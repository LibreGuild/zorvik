---
title: Auth
description: Basic, Bearer, API key and OAuth 2.0 authentication, how tokens are fetched and cached, and how auth is inherited from folders and the workspace.
sidebar:
  order: 3
---

Auth adds credentials to a request: an `Authorization` header, an API key, or an OAuth 2.0 access token that Zorvik fetches for you. You can set it in three places:

| Where | How |
|---|---|
| A request | The request's **Auth** tab |
| A folder | Right-click the folder → **Folder settings…** → **Auth** |
| The workspace | Workspace menu → **Workspace settings…** → **Default auth** |

Set auth once on a folder or the workspace, and leave its requests on **Inherit from parent**. See [Inheritance](#inheritance).

## Auth types

| Type (menu) | In the file (`auth.type`) | Sends |
|---|---|---|
| **Inherit from parent** | `inherit` (the default; not written to the file) | Whatever the folder or workspace uses |
| **No auth** | `none` | Nothing |
| **Basic auth** | `basic` | `Authorization: Basic <base64 of username:password>` |
| **Bearer token** | `bearer` | `Authorization: Bearer <token>` |
| **API key** | `apiKey` | A header or a query parameter with your key |
| **OAuth 2.0** | `oauth2` | `Authorization: Bearer <access token>`, with the token fetched, cached and refreshed |

When the request's auth is not inherited, the **Auth** tab shows its type, for example **Auth** `Bearer`.

Every text field accepts `{{variables}}`. Keep real credentials in [secret variables](../../variables/secrets/), not in the fields themselves:

:::caution[Auth fields are saved in the file]
Password and secret fields show dots, but what you type into them is stored as plain text in the request, folder or workspace file, and so ends up in Git. Put a variable in the field instead, for example `{{clientSecret}}`, and mark that variable **secret**: its value then stays on your computer.
:::

## No auth and inherit

- **Inherit from parent** uses the auth of the nearest folder that sets one, else the workspace's **Default auth**. New requests start here.
- **No auth** sends no credentials and stops inheritance: the folder's and workspace's auth are not used for this request.

The workspace's **Default auth** can't inherit; its default is **No auth**.

## Basic auth

| Field | Example |
|---|---|
| **Username** | `{{username}}` |
| **Password** | `{{password}}` |

Sends `Authorization: Basic` followed by the Base64 of `username:password` (after variables are resolved).

```yaml
auth:
  type: basic
  username: "{{username}}"
  password: "{{password}}"
```

:::note
A URL like `https://user:pass@api.example.com/` also becomes Basic auth when the request has no `Authorization` header. See [HTTP requests](../http/#url).
:::

## Bearer token

| Field | Default | Meaning |
|---|---|---|
| **Token** | | The token, e.g. `{{token}}` |
| **Prefix** | `Bearer` | Sent as `Authorization: <prefix> <token>`. Leave it empty to send the bare token, or use another scheme such as `Token`. |

```yaml
auth:
  type: bearer
  token: "{{token}}"
  prefix: Bearer
```

## API key

| Field | Default | Meaning |
|---|---|---|
| **Key** | `X-API-Key` | The header name, or the query parameter name |
| **Value** | | The key, e.g. `{{apiKey}}` |
| **Add to** | **Header** | **Header** or **Query parameter** |

- **Header**: sends `Key: Value`. If the request already has a header with that name (set by you, a folder or the workspace), the API key is not added.
- **Query parameter**: appends `key=value` to the URL, URL-encoded, after any existing query parameters. It isn't shown in the **Params** table.
- With an empty **Key**, nothing is added.

```yaml
auth:
  type: apiKey
  key: api_key
  value: "{{apiKey}}"
  location: query   # header (default) or query
```

## OAuth 2.0

Zorvik gets an access token from your identity provider and sends it as `Authorization: Bearer <token>`. It supports three grant types:

| Grant type (menu) | In the file (`grantType`) | How the token is obtained |
|---|---|---|
| **Client credentials** | `clientCredentials` | Automatically when you send |
| **Password** | `password` | Automatically when you send, with a username and password |
| **Authorization code** | `authorizationCode` | You sign in once in your browser with **Get token**; PKCE by default |

Other grants, such as the implicit grant, are not supported.

### Fields

| Field | Grant types | Default | Meaning |
|---|---|---|---|
| **Grant type** | all | Client credentials | See above |
| **Authorization URL** | Authorization code | | The provider's authorize endpoint, e.g. `https://id.example.com/oauth/authorize` |
| **Token URL** | all | | The provider's token endpoint |
| **Client ID** | all | | |
| **Client secret** | all | | Leave empty for public clients |
| **Username**, **Password** | Password | | The resource owner's credentials |
| **Redirect URI** | Authorization code | `http://127.0.0.1:53682/callback` | Where the provider sends you back; Zorvik listens there while you sign in |
| **Scope** | all | | Space-separated scopes, e.g. `read write` |
| **Audience** | all | | Sent as `audience` when set (used by some providers) |
| **Client auth** | all | Basic auth header | How the client ID and secret reach the token endpoint: **Basic auth header** or **In request body** |
| **PKCE** | Authorization code | On | **Use PKCE (S256)** |
| **Header prefix** | all | `Bearer` | Sent as `Authorization: <prefix> <token>`; empty sends the bare token |

```yaml
auth:
  type: oauth2
  grantType: clientCredentials   # clientCredentials | password | authorizationCode
  tokenUrl: https://id.example.com/oauth/token
  clientId: "{{clientId}}"
  clientSecret: "{{clientSecret}}"
  scope: orders:read
  clientAuth: basicHeader        # basicHeader | body
  headerPrefix: Bearer
  redirectUri: http://127.0.0.1:53682/callback
  pkce: true
```

### The token request

Zorvik asks the **Token URL** for a token with a `POST`:

- `Content-Type: application/x-www-form-urlencoded` and `Accept: application/json`
- `grant_type`: `client_credentials`, `password` (plus `username` and `password`), or `authorization_code` (plus `code`, `redirect_uri` and, with PKCE, `code_verifier`)
- `scope` and `audience`, when set
- The client credentials: with **Basic auth header**, an `Authorization: Basic` header made of the URL-encoded client ID and secret; with **In request body**, `client_id` and (when not empty) `client_secret` as form fields

The response may be JSON or form-encoded. Zorvik reads `access_token` (required), `expires_in`, `refresh_token` and `scope`. An `error` in the response fails the send with *Token request failed*, the status, the error and its `error_description`.

Token requests go through the same proxy and certificate settings as your requests.

### Token status

Below the fields, a box shows the token Zorvik has for this configuration:

- **Token** and the start and end of the token (for example `eyJhbG…9xQw`), **Expires** or **Expired** with the time, or **No expiry**, and **refresh token available** when there is one.
- Without a token: *No token yet. Sign in to get one.* (authorization code) or *No cached token. One is requested automatically when you send.*

| Button | Does |
|---|---|
| **Get token** | Authorization code: starts the browser sign-in (below) |
| **Fetch token** | Client credentials and password: requests a new token now, even if one is cached |
| **Clear** | Forgets the cached token |

### How tokens are used and refreshed

When you send a request with OAuth 2.0:

1. If a cached token is still valid for at least 30 more seconds, it is used. A token without `expires_in` is used until you clear it.
2. If it expired and there is a refresh token, Zorvik refreshes it (`grant_type=refresh_token`). A new refresh token from the provider replaces the old one; otherwise the old one is kept.
3. Otherwise, client credentials and password grants fetch a new token. The authorization code grant can't sign in by itself: the send fails with *No valid OAuth 2.0 token. Open the Auth tab and click "Get token" to sign in.*

The token is cached **per workspace on this computer**, and per identity: the grant type, token URL, authorization URL, client ID, client secret, scope, audience, username and password, after variables are resolved. So:

- Every request with the same OAuth configuration (typically, all requests inheriting it from one folder) shares one token.
- Switching to an environment with other credentials uses, and fetches, a separate token.
- Tokens are never shared between workspaces.

Tokens are stored in the app data folder (`oauth-tokens.json`), readable only by your user on macOS and Linux, and survive restarts. They are never written to the workspace.

### Authorization code sign-in

1. Register the **Redirect URI** with your identity provider. The default is `http://127.0.0.1:53682/callback`. It must be `http://` with the host `127.0.0.1`, `localhost` or `[::1]`, and include a port.
2. Choose **Get token**. Zorvik starts listening on that address and opens the **Authorization URL** in your default browser, with `response_type=code`, `client_id`, `redirect_uri`, a random `state`, `scope` and `audience` when set, and, with PKCE, an S256 `code_challenge`.
3. Sign in and approve. The provider redirects your browser back to Zorvik, which shows *Signed in. You can close this tab and return to Zorvik.*
4. Zorvik exchanges the code for a token and shows **Token received**.

Details:

- You have 5 minutes to finish signing in; after that, *Timed out waiting for the sign-in to finish in the browser*.
- A redirect whose `state` doesn't match is answered but ignored, so another web page can't complete or cancel your sign-in. Other paths on that port get `404`.
- If the provider redirects with an `error`, the sign-in fails with that error and its description.
- If the port is taken, *Could not listen on 127.0.0.1:53682 for the OAuth redirect*. Choose another port and register that URI instead.
- The Authorization URL must start with `http://` or `https://`.

:::tip
Set OAuth 2.0 on a folder and choose **Get token** there, in **Folder settings → Auth**. Every request inside that inherits it uses the same token.
:::

## Inheritance

A request left on **Inherit from parent** uses the first auth it finds, looking:

1. at its own folder, then each folder above it, up to the top of the collection, skipping folders that inherit too;
2. then at the workspace's **Default auth**.

```text
Workspace                 Default auth: No auth
└── Payments/             Auth: OAuth 2.0 (client credentials)
    ├── Refunds/          Auth: Inherit from parent
    │   └── Create refund Auth: Inherit from parent   → OAuth 2.0 from Payments/
    └── Health check      Auth: No auth               → nothing
```

A request that isn't saved yet only inherits from the workspace, because it isn't in a folder yet.

### When you set an Authorization header yourself

An `Authorization` header in the **Headers** of the request, a folder or the workspace wins over **Basic auth**, **Bearer token** and **OAuth 2.0**: they add nothing (and no OAuth token is fetched). An **API key** header is skipped when a header with the same name exists.

**No auth** stops inherited *auth*, but not inherited *headers*: an `Authorization` header set in a folder's or the workspace's headers is still sent.

## Other request kinds

WebSocket, event stream (SSE) and gRPC requests have the same **Auth** tab. MQTT clients log in with the **Basic auth** username and password.

## Importing auth

Imports bring auth along: Postman's basic, bearer, API key, OAuth 2.0, no-auth and inherit settings, and OpenAPI security schemes (with placeholders such as `{{bearerToken}}` and `{{clientId}}` in the new environment). See [Import & export](../import-export/).
