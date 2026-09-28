---
id: bearer-and-jwt
title: Bearer tokens and JWT
summary: After you log in, you carry a token instead of your password; a JWT is a token you can read, but not change.
minutes: 6
lab:
  title: Read your token, then use it
  goal: Log in, decode the JWT you get back, and call a protected endpoint with a Bearer token.
  minutes: 7
  vars:
    token: eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiJ1LTEwMjQiLCJuYW1lIjoiQWRhIExvdmVsYWNlIiwicm9sZSI6ImF1ZGl0b3IiLCJzY29wZSI6InJlcG9ydHM6cmVhZCIsImlzcyI6Imh0dHBzOi8vbG9naW4ubGFiLnRlc3QiLCJpYXQiOjE3NjcyMjU2MDAsImV4cCI6NDEwMjQ0NDgwMH0.VxHJbhbaSOClPgIMxg0BB4Bm556QllkS7HKtOxXER1I
  servers:
    api:
      name: Accounts API
      kind: http
      http:
        routes:
          - name: Log in
            method: POST
            path: /login
            headers:
              - { key: Content-Type, value: application/json }
            body: '{"access_token": "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiJ1LTEwMjQiLCJuYW1lIjoiQWRhIExvdmVsYWNlIiwicm9sZSI6ImF1ZGl0b3IiLCJzY29wZSI6InJlcG9ydHM6cmVhZCIsImlzcyI6Imh0dHBzOi8vbG9naW4ubGFiLnRlc3QiLCJpYXQiOjE3NjcyMjU2MDAsImV4cCI6NDEwMjQ0NDgwMH0.VxHJbhbaSOClPgIMxg0BB4Bm556QllkS7HKtOxXER1I", "token_type": "Bearer", "expires_in": 3600}'
          - name: Me
            method: GET
            path: /me
            matchHeaders:
              - { key: Authorization, value: Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiJ1LTEwMjQiLCJuYW1lIjoiQWRhIExvdmVsYWNlIiwicm9sZSI6ImF1ZGl0b3IiLCJzY29wZSI6InJlcG9ydHM6cmVhZCIsImlzcyI6Imh0dHBzOi8vbG9naW4ubGFiLnRlc3QiLCJpYXQiOjE3NjcyMjU2MDAsImV4cCI6NDEwMjQ0NDgwMH0.VxHJbhbaSOClPgIMxg0BB4Bm556QllkS7HKtOxXER1I }
            headers:
              - { key: Content-Type, value: application/json }
            body: '{"id": "u-1024", "name": "Ada Lovelace", "role": "auditor"}'
          - name: No token
            method: GET
            path: /me
            status: 401
            headers:
              - { key: Content-Type, value: application/json }
              - { key: WWW-Authenticate, value: Bearer }
            body: '{"error": "unauthorized", "message": "Send Authorization: Bearer <token>."}'
  steps:
    - text: |
        Log in: send `POST {{api}}/login` with the JSON body `{"username": "ada", "password": "lovelace"}`. The answer holds an `access_token`.
      hints:
        - Method POST, **Body** tab, type **JSON**.
        - "The practice server accepts any username and password; real ones don't."
        - "POST {{api}}/login, JSON body {\"username\": \"ada\", \"password\": \"lovelace\"}, then Send."
      check:
        request: { server: api, method: POST, path: /login }
      solution:
        - send:
            method: POST
            url: "{{api}}/login"
            body: { type: json, text: "{\"username\": \"ada\", \"password\": \"lovelace\"}" }
    - text: |
        Copy the `access_token` value and decode it: **Tools → Encoders → JWT**. Which `role` does the token give you?
      hints:
        - Copy the long text between the quotes after `"access_token":`. It has two dots in it.
        - In the sidebar open **Tools**, then **Encoders**, pick **JWT** and paste. The **Payload** box lists the claims.
        - "Find \"role\" in the Payload and type its value."
      check:
        answer: auditor
      solution:
        - answer: auditor
    - text: |
        Call `GET {{api}}/me` as that user. The lab saved the same token in the variable `{{token}}`: in the **Auth** tab choose **Bearer token** and put `{{token}}` in **Token**.
      hints:
        - Without a token, `/me` answers 401.
        - "Auth tab: Type = Bearer token, Token = {{token}}, Prefix = Bearer (the default)."
        - "GET {{api}}/me with that auth, then Send. You should get 200 and Ada's profile."
      check:
        all:
          - request: { server: api, method: GET, path: /me, status: 200, headers: { authorization: "Bearer {{token}}" } }
          - send: { auth: bearer, status: 200 }
      solution:
        - send: { method: GET, url: "{{api}}/me", auth: { type: bearer, token: "{{token}}", prefix: Bearer } }
quiz:
  - question: Anyone can decode a JWT's payload. What stops them from changing their role to "admin"?
    options:
      - The payload is encrypted, so only the server can read it
      - The signature no longer matches, so the server rejects it
      - A JWT becomes read-only once it is issued
    answer: 1
    explain: The signature is made with a key only the issuer has. Change one character of the payload and verification fails.
  - question: Where does a bearer token usually go?
    options:
      - "In the Authorization header: `Bearer <token>`"
      - In the URL, as `?token=`
      - In the JSON body of every request
    answer: 0
    explain: The Authorization header is the standard place. Tokens in URLs leak into logs and browser history.
  - question: A JWT's `exp` claim is in the past. What will the API most likely answer?
    options:
      - 403 Forbidden
      - 200 OK, expiry is only a hint
      - 401 Unauthorized, so you need a new token
    answer: 2
    explain: An expired token no longer proves who you are. Log in again (or use a refresh token) to get a fresh one.
---

Sending your password with every request is risky: the more often it travels, the more places it can leak. So most modern APIs have you **log in once** and hand you a **token**, a long string that stands for "this user, logged in, until a certain time". You then send the token with every request instead.

```sequence
You -> API: POST /login {"username": "ada", "password": "…"}
Note over API: checks the password once
API --> You: 200 {"access_token": "eyJhbGci…"}
You -> API: GET /me (Authorization: Bearer eyJhbGci…)
API --> You: 200 {"name": "Ada Lovelace"}
```

## Bearer tokens

A **bearer token** works like cash: whoever *bears* (holds) it can use it, no questions asked. It goes in the `Authorization` header with the word `Bearer` in front:

```http
GET /me HTTP/1.1
Authorization: Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9…
```

In Zorvik, choose **Bearer token** in the **Auth** tab and put the token, or better a variable like `{{token}}`, in **Token**. **Prefix** stays `Bearer` unless an API asks for something else.

## JWT: a token you can read

Many tokens are **JWTs** (JSON Web Tokens, often said "jot"). A JWT is three Base64 pieces joined by dots:

```anatomy
eyJhbGciOiJIUzI1NiJ9 | header: which algorithm signed it
.eyJzdWIiOiJ1LTEwMjQiLCJyb2xlIjoi… | payload: the claims, facts about you
.VxHJbhbaSOClPgIMxg0BB4Bm… | signature: proves nobody changed the first two parts
```

The payload holds **claims**, small facts in JSON:

| Claim | Means |
|---|---|
| `sub` | subject: who the token is about, usually a user id |
| `iss` | issuer: who created the token |
| `exp` | expires: a time as seconds since 1 January 1970 |
| `iat` | issued at: when it was created |
| `scope`, `role` | what the holder may do (names vary by API) |

> [!note] Think of it like…
> A festival wristband with your details printed on it. Anyone can read the print, but the wristband has a tamper-proof seal. Cut it open to change "day ticket" to "backstage" and the seal breaks, and security at the gate sees it at once.

## Decoding is not verifying

**Tools → Encoders → JWT** shows the header, the payload and friendly dates for `exp` and `iat`, and warns when a token has expired. It does *not* check the signature: only the server has the key for that. So decoding is great for debugging ("is this token expired? which user is it for?"), but never trust a token just because it decodes.

> [!warning] Treat tokens like passwords
> A JWT's payload is readable by anyone, so never put secrets in one, and don't paste real production tokens into websites you don't trust. Keep them in variables marked secret.

**You'll use this when…** an API suddenly answers 401. Decode the token: if `exp` is in the past you need a fresh one; if `scope` or `role` lacks a permission, you know why you got a 403.
