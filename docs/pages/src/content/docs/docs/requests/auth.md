---
title: Auth
description: Basic, Bearer, API key, OAuth 2.0 and 1.0, JWT, Digest, NTLM, AWS Signature v4, Hawk, Akamai EdgeGrid and Atlassian ASAP, how tokens are fetched and signatures made, and how auth is inherited from folders and the workspace.
sidebar:
  order: 3
---

Auth adds credentials to a request: an `Authorization` header, an API key, an OAuth 2.0 access token that Zorvik fetches for you, a signature made for every send, or the answer to the server's challenge. You can set it in three places:

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
| **OAuth 1.0** | `oauth1` | A signed `Authorization: OAuth …` header (or query parameters) |
| **JWT (signed by Zorvik)** | `jwt` | A token Zorvik signs from your claims, as `Authorization: Bearer <token>` or a query parameter |
| **Digest auth** | `digest` | The answer to the server's Digest challenge |
| **NTLM (Windows)** | `ntlm` | The NTLMv2 handshake with the server |
| **AWS Signature v4** | `awsSigV4` | A signed `Authorization: AWS4-HMAC-SHA256 …` header, or a presigned URL |
| **Hawk** | `hawk` | A signed `Authorization: Hawk …` header |
| **Akamai EdgeGrid** | `akamaiEdgeGrid` | A signed `Authorization: EG1-HMAC-SHA256 …` header |
| **Atlassian ASAP** | `asap` | A short-lived JWT signed with your service's key, as `Authorization: Bearer <token>` |

The signing types (OAuth 1.0, JWT, AWS, Hawk, EdgeGrid, ASAP) are signed **for every send**, after pre-request scripts ran, over the final method, URL, headers and body, with a fresh timestamp and nonce. In [load tests](../../load-testing/overview/) each request is signed separately. Digest and NTLM can't be load tested: they answer a challenge on the connection that received it.

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

Zorvik gets an access token from your identity provider and sends it as `Authorization: Bearer <token>`. It supports four grant types:

| Grant type (menu) | In the file (`grantType`) | How the token is obtained |
|---|---|---|
| **Client credentials** | `clientCredentials` | Automatically when you send |
| **Password** | `password` | Automatically when you send, with a username and password |
| **Authorization code** | `authorizationCode` | You sign in once in your browser with **Get token**; PKCE by default |
| **Implicit (legacy)** | `implicit` | You sign in with **Get token**; the token comes back in the redirect itself, without a token URL. Older single-page apps use it; prefer the authorization code with PKCE when the provider offers it. |

### Fields

| Field | Grant types | Default | Meaning |
|---|---|---|---|
| **Grant type** | all | Client credentials | See above |
| **Authorization URL** | Authorization code, implicit | | The provider's authorize endpoint, e.g. `https://id.example.com/oauth/authorize` |
| **Token URL** | all but implicit | | The provider's token endpoint |
| **Client ID** | all | | |
| **Client secret** | all but implicit | | Leave empty for public clients |
| **Username**, **Password** | Password | | The resource owner's credentials |
| **Redirect URI** | Authorization code, implicit | `http://127.0.0.1:53682/callback` | Where the provider sends you back; Zorvik listens there while you sign in |
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
- **Implicit grant**: the same steps, but the authorization URL asks for `response_type=token` and the provider puts the token in the redirect's `#fragment`, which browsers never send to a server. The page Zorvik shows at the redirect reads the fragment and hands it to Zorvik, then says *Signed in*. The token has no refresh token: sign in again when it expires.

:::tip
Set OAuth 2.0 on a folder and choose **Get token** there, in **Folder settings → Auth**. Every request inside that inherits it uses the same token.
:::

## OAuth 1.0

Signs each request with OAuth 1.0a (RFC 5849), as Twitter/X, older Atlassian and many enterprise APIs expect.

| Field | Default | Meaning |
|---|---|---|
| **Signature method** | HMAC-SHA1 | HMAC-SHA1, HMAC-SHA256, HMAC-SHA512, RSA-SHA1, RSA-SHA256, RSA-SHA512 or PLAINTEXT |
| **Consumer key**, **Consumer secret** | | Your app's credentials (the secret for HMAC and PLAINTEXT) |
| **Private key** | | RSA methods: a PEM private key (PKCS#1 or PKCS#8) instead of the consumer secret |
| **Token**, **Token secret** | | The user's access token and its secret, when you have them |
| **Callback URL**, **Verifier**, **Realm** | | Sent when set |
| **Send oauth_version=1.0** | On | |
| **Sign the body** | Off | Adds `oauth_body_hash` (for JSON and other bodies that aren't form data) |
| **Add to** | Authorization header | Or the query string |

The query parameters and, for `application/x-www-form-urlencoded` bodies, the form fields are part of the signature. Every send gets a new timestamp and nonce.

```yaml
auth:
  type: oauth1
  consumerKey: "{{consumerKey}}"
  consumerSecret: "{{consumerSecret}}"
  token: "{{accessToken}}"
  tokenSecret: "{{tokenSecret}}"
  signatureMethod: HMAC-SHA256
```

## JWT (signed by Zorvik)

Zorvik builds and signs a fresh JSON Web Token for every send, from claims you write.

| Field | Default | Meaning |
|---|---|---|
| **Algorithm** | HS256 | HS256/384/512 (a shared secret), RS256/384/512 and PS256/384/512 (an RSA key), ES256/384 (an EC key) |
| **Secret** / **Private key** | | HS: the secret (**The secret is base64** to decode it first). Others: a PEM private key; RSA keys of 2048, 3072 or 4096 bits (PKCS#1 or PKCS#8), EC keys as PKCS#8 or SEC1 with their public part |
| **Payload** | `{"sub": "1234567890"}` | The claims, as JSON. Variables work: `{"sub": "{{userId}}", "iat": {{$timestamp}}, "exp": {{$timestamp(+1h)}}}` |
| **Header** | | Extra header fields as JSON, such as `{"kid": "key-1"}` (`alg` and `typ` are set for you) |
| **Add to** | Authorization header | With **Prefix** (`Bearer`; empty sends the bare token), or a query parameter named **Parameter** (`token`) |

A payload that isn't a JSON object, or a key that doesn't fit the algorithm, stops the send with an error saying what to change.

## Digest auth

HTTP Digest (RFC 7616 and RFC 2617). Zorvik sends the request, reads the server's `401` challenge and sends it again with the answer, on the same connection.

| Field | Meaning |
|---|---|
| **Username**, **Password** | Your credentials |

The rest comes from the server's challenge: algorithms MD5, MD5-sess, SHA-256, SHA-256-sess and SHA-512-256, `qop` `auth` (preferred) or `auth-int` (the body is signed too), `opaque`, `userhash`, and non-ASCII usernames (`username*`). A wrong password gets the server's `401` as the response. **Request sent** in the response's **Info** tab shows the `Authorization: Digest …` header of the second attempt; the time includes both round trips.

## NTLM (Windows)

NTLMv2, for IIS, SharePoint, Exchange and other Windows servers.

| Field | Meaning |
|---|---|
| **Username** | `user`, `DOMAIN\user` or `user@domain` |
| **Password** | |
| **Domain**, **Workstation** | Optional; the domain is taken from the username when it has one |

Zorvik sends the negotiate message, answers the server's challenge and sends the request again, all on one connection, so NTLM always uses HTTP/1.1 (choosing HTTP/2 for the request is refused). The server's timestamp is used when it sends one. Servers that require Extended Protection (channel binding over TLS) or Kerberos refuse it.

## AWS Signature v4

Signs each request for AWS: API Gateway, S3, Lambda function URLs, OpenSearch and the other services.

| Field | Meaning |
|---|---|
| **Access key**, **Secret key** | Your credentials, e.g. `{{awsAccessKeyId}}` and `{{awsSecretAccessKey}}` |
| **Session token** | For temporary credentials (STS, SSO): sent as `X-Amz-Security-Token` |
| **Region** | e.g. `eu-west-1` |
| **Service** | e.g. `execute-api`, `s3`, `lambda`, `es` |
| **Add to** | **Authorization header** (with `X-Amz-Date`), or **Query string (presigned URL)**, valid for an hour |

The signature covers the method, the path (encoded as the service expects; S3 is different), the query, the `Host` header, your headers (not `User-Agent`, `Authorization` and a few connection headers) and a hash of the body. For S3, `X-Amz-Content-Sha256` is added; set it yourself (for example `UNSIGNED-PAYLOAD`) to sign differently.

## Hawk

| Field | Meaning |
|---|---|
| **Hawk ID**, **Hawk key** | Your credentials |
| **Algorithm** | SHA-256 (default) or SHA-1 |
| **Extra data (ext)**, **App ID**, **Delegation (dlg)** | Sent when set |
| **Sign the body** | Adds the payload hash (Content-Type and body); the server must check it |

## Akamai EdgeGrid

For Akamai's APIs. Put the host from your `.edgerc` (`akab-….luna.akamaiapis.net`) in the URL.

| Field | Meaning |
|---|---|
| **Client token**, **Client secret**, **Access token** | From your `.edgerc` or API client |
| **Headers to sign** | Usually empty; comma-separated names |
| **Max body** | Bytes of a POST body in the signature (131072, Akamai's default) |

## Atlassian ASAP

Atlassian's service-to-service auth: a JWT your service signs, valid for a short time.

| Field | Default | Meaning |
|---|---|---|
| **Issuer** | | Your service |
| **Audience** | | The receiving service (comma-separated for several) |
| **Key ID** | | The `kid` the receiver finds your public key by, e.g. `my-service/key-1` |
| **Private key** | | PEM |
| **Algorithm** | RS256 | RS, PS or ES |
| **Subject** | | Optional |
| **Expires in** | 3600 | Seconds (at most 3600) |
| **Extra claims** | | Optional JSON |

Each send gets a new token with `iat`, `exp` and a unique `jti`.

:::tip[Keep keys and secrets out of the files]
Private keys, secret keys and passwords are saved in the request, folder or workspace file like every field. Put them in [secret variables](../../variables/secrets/) and use `{{name}}` in the field.
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

An `Authorization` header in the **Headers** of the request, a folder or the workspace wins over every auth type that uses it: Basic, Bearer, OAuth 2.0 and 1.0, JWT, Digest, NTLM, AWS, Hawk, EdgeGrid and ASAP add nothing (and no OAuth token is fetched). An **API key** header is skipped when a header with the same name exists.

**No auth** stops inherited *auth*, but not inherited *headers*: an `Authorization` header set in a folder's or the workspace's headers is still sent.

## Other request kinds

WebSocket, Socket.IO, event stream (SSE) and gRPC requests have the same **Auth** tab (for Socket.IO it becomes a header of the handshake). MQTT clients log in with the **Basic auth** username and password.

## Importing auth

Imports bring auth along: every Postman auth type listed above, with its settings, and OpenAPI security schemes (Basic, Bearer, Digest, API keys and every OAuth 2.0 flow, with placeholders such as `{{bearerToken}}` and `{{clientId}}` in the new environment). See [Import & export](../import-export/).
