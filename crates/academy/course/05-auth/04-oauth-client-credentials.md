---
id: oauth-client-credentials
title: OAuth 2.0 client credentials
summary: With OAuth 2.0, an app trades its client id and secret for a short-lived access token at a token endpoint, then uses that token as a bearer token.
minutes: 7
lab:
  title: Get a token, twice
  goal: Fetch an OAuth 2.0 access token by hand, then let Zorvik fetch and attach it for you.
  minutes: 8
  playground: true
  vars:
    clientId: test-client
    clientSecret: test-secret
  steps:
    - text: Send `GET {{playground}}/oauth/protected` with no auth. It's locked.
      hints:
        - A new request inherits the workspace auth, which is none in the Bootcamp.
        - "GET {{playground}}/oauth/protected, then Send. You should get 401."
      check:
        send: { url: "*/oauth/protected", status: 401 }
      solution:
        - send: { method: GET, url: "{{playground}}/oauth/protected" }
    - text: |
        Ask the token endpoint for a token yourself. Send `POST {{playground}}/oauth/token` with a **Form URL-encoded** body:

        | Key | Value |
        |---|---|
        | `grant_type` | `client_credentials` |
        | `client_id` | `{{clientId}}` |
        | `client_secret` | `{{clientSecret}}` |
      hints:
        - Token endpoints take a form, not JSON. It's the **Form URL-encoded** body type from the last unit.
        - Method POST, **Body** tab, **Form URL-encoded**, then three rows. Type the variables with their braces.
        - "POST {{playground}}/oauth/token with grant_type=client_credentials, client_id={{clientId}}, client_secret={{clientSecret}}. The answer holds an access_token."
      check:
        send: { method: POST, url: "*/oauth/token", status: 200, json: { token_type: Bearer, access_token: "*" } }
      solution:
        - send:
            method: POST
            url: "{{playground}}/oauth/token"
            body:
              type: formUrlencoded
              form:
                - { key: grant_type, value: client_credentials }
                - { key: client_id, value: "{{clientId}}" }
                - { key: client_secret, value: "{{clientSecret}}" }
    - text: |
        Now let Zorvik do the dance. Go back to `GET {{playground}}/oauth/protected` (a new tab is fine), choose **OAuth 2.0** in the **Auth** tab and fill it in:

        - **Grant type**: Client credentials
        - **Token URL**: `{{playground}}/oauth/token`
        - **Client ID**: `{{clientId}}`
        - **Client secret**: `{{clientSecret}}`

        Then send.
      hints:
        - Zorvik fetches a token before sending, keeps it until it expires, and adds the `Authorization` header for you.
        - "Leave Client auth on Basic auth header: the practice server accepts both ways."
        - "Auth = OAuth 2.0 with the four fields above, URL {{playground}}/oauth/protected, then Send. You should get 200 {\"ok\": true}."
      check:
        send: { url: "*/oauth/protected", status: 200, auth: oauth2 }
      solution:
        - send:
            method: GET
            url: "{{playground}}/oauth/protected"
            auth:
              type: oauth2
              grantType: clientCredentials
              tokenUrl: "{{playground}}/oauth/token"
              clientId: "{{clientId}}"
              clientSecret: "{{clientSecret}}"
              clientAuth: basicHeader
              headerPrefix: Bearer
quiz:
  - question: In the client credentials flow, who is being authenticated?
    options:
      - A person, typing their password
      - An application, with its client id and secret
      - The token endpoint
    answer: 1
    explain: Client credentials is for machine-to-machine calls. No person signs in; the app proves who it is.
  - question: Your access token has expired. What does the client do?
    options:
      - Asks the token endpoint for a new one
      - Keeps sending it; the server will extend it
      - Changes its client secret
    answer: 0
    explain: Access tokens are short-lived on purpose. Zorvik keeps a token until it expires, then fetches a new one when you send.
  - question: Why is a short-lived access token safer than sending the client secret with every request?
    options:
      - Tokens are shorter, so they are harder to guess
      - Tokens are encrypted, so they can't be copied
      - A leaked token expires soon; the secret only goes to the token endpoint
    answer: 2
    explain: The long-lived secret travels rarely and to one place. What travels often is a token that expires on its own.
---

API keys and passwords are long-lived: if one leaks, it works until somebody notices. **OAuth 2.0** is a standard that fixes this with a middleman. Instead of sending its secret to every API, an app goes to an **authorization server** once, proves who it is, and gets a short-lived **access token**. It then calls the API with that token, as a bearer token.

OAuth 2.0 has several **grant types**, ways of getting a token. This lesson covers the simplest one, **client credentials**, used when one program talks to another with no person involved: a nightly billing job, a backend calling a partner's API.

```sequence
participants: App, Token endpoint, API
App -> Token endpoint: POST /oauth/token (client id + secret)
Note over Token endpoint: checks the credentials
Token endpoint --> App: {"access_token": "…", "expires_in": 3600}
App -> API: GET /data (Authorization: Bearer …)
API --> App: 200 OK
```

## The pieces

| Term | What it is |
|---|---|
| **Client** | the application asking for access |
| **Client ID** | the client's public name, like a username |
| **Client secret** | the client's password: keep it secret |
| **Token endpoint** | the URL that hands out tokens, often `…/oauth/token` |
| **Scope** | optional: which permissions the token should carry |
| **Access token** | the result: a bearer token with an expiry, often a JWT |

The token request is a **Form URL-encoded** POST with `grant_type=client_credentials`. The client id and secret go either in a Basic auth header or in the form itself; each provider says which it accepts. The answer is JSON with the `access_token`, its `token_type` (`Bearer`) and `expires_in` (seconds until it expires).

> [!note] Think of it like…
> A hotel. At reception (the token endpoint) you show your passport once (client id and secret). You get a key card (the access token) that opens your room (the API) until checkout. Lose the card and it stops working at checkout anyway; your passport never left reception.

## Let Zorvik do it

Getting a token by hand is great for understanding, but tedious every hour. In the **Auth** tab choose **OAuth 2.0**, set the **Grant type**, **Token URL**, **Client ID** and **Client secret** (as variables), and send. Zorvik:

1. asks the token endpoint for a token, if it has no valid one yet,
2. keeps the token until it expires,
3. adds `Authorization: Bearer <token>` to your request.

**Fetch token** gets one right away and shows its expiry; **Clear** throws it away. Put the OAuth 2.0 settings on a folder and every request inside shares the token.

> [!tip] Other grant types
> When a *person* signs in, APIs use the **authorization code** grant: a browser window opens for the login, and Zorvik catches the result. Zorvik supports that grant type too; the idea is the same, only the first step differs.

**You'll use this when…** you call a cloud provider's API, a payment service or your company's internal services from a test: they almost all hand out tokens this way. Set it up once on a folder and forget about expiring tokens.
