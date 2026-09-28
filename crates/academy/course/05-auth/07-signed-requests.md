---
id: signed-requests
title: Signed requests
summary: Instead of sending a secret, some APIs want every request signed with it; Zorvik signs each send for you with JWT, AWS Signature v4, OAuth 1.0 and more.
minutes: 6
added: 0.2.0
lab:
  title: Sign it three ways
  goal: Send requests signed with a JWT, AWS Signature v4 and OAuth 1.0, and look at the signatures the server receives.
  minutes: 8
  vars:
    jwtSecret: "{{secret.jwt}}"
    awsAccessKey: AKIALABEXAMPLE7
    awsSecretKey: "{{secret.aws}}"
    consumerKey: lab-app
    consumerSecret: "{{secret.oauth}}"
  servers:
    api:
      name: Cloud API
      kind: http
      http:
        routes:
          - name: Signed
            method: "*"
            path: /*
            matchHeaders:
              - { key: Authorization, value: "" }
            headers:
              - { key: Content-Type, value: application/json }
            body: '{"ok": true, "path": "{{request.path}}", "note": "A real service recomputes the signature here. This practice server only checks that one is there."}'
          - name: Not signed
            method: "*"
            path: /*
            status: 401
            headers:
              - { key: Content-Type, value: application/json }
            body: '{"error": "unauthorized", "message": "Sign the request: this API wants an Authorization header."}'
  steps:
    - text: |
        Your team's reports service trusts tokens signed with a shared secret. Send `GET {{api}}/reports` with **JWT (signed by Zorvik)** auth: algorithm **HS256**, secret `{{jwtSecret}}`, and this payload:

        ```json
        {"sub": "ada", "exp": {{$timestamp(+1h)}}}
        ```
      hints:
        - Zorvik builds the token from your payload and signs it with the secret, fresh for every send. `{{$timestamp(+1h)}}` makes it expire an hour from now.
        - "Auth tab → Type **JWT (signed by Zorvik)** → Algorithm HS256, Secret {{jwtSecret}}, Payload as shown. Leave Add to on the Authorization header."
        - "GET {{api}}/reports with that auth, then Send. Open Lab · Cloud API under Servers: its Traffic shows Authorization: Bearer eyJ…"
      check:
        all:
          - request: { server: api, method: GET, path: /reports, status: 200, headers: { authorization: "Bearer eyJ*.*.*" } }
          - send: { url: "*/reports", auth: jwt }
      solution:
        - send:
            method: GET
            url: "{{api}}/reports"
            auth:
              type: jwt
              algorithm: HS256
              secret: "{{jwtSecret}}"
              payload: "{\"sub\": \"ada\", \"exp\": {{$timestamp(+1h)}}}"
              prefix: Bearer
              queryParam: token
    - text: |
        The billing API runs behind AWS. Send `GET {{api}}/invoices` with **AWS Signature v4**: access key `{{awsAccessKey}}`, secret key `{{awsSecretKey}}`, region `eu-west-1`, service `execute-api`.
      hints:
        - The signature covers the method, the path, the time and your headers, made with the secret key. The access key only says who signed.
        - "Auth tab → Type **AWS Signature v4** → Access key {{awsAccessKey}}, Secret key {{awsSecretKey}}, Region eu-west-1, Service execute-api."
        - "GET {{api}}/invoices with that auth, then Send. The server receives Authorization: AWS4-HMAC-SHA256 Credential=… and an X-Amz-Date header."
      check:
        all:
          - request:
              server: api
              method: GET
              path: /invoices
              status: 200
              headers:
                authorization: "AWS4-HMAC-SHA256 Credential=AKIALABEXAMPLE7/*/eu-west-1/execute-api/aws4_request*Signature=*"
                x-amz-date: "re:^\\d{8}T\\d{6}Z$"
          - send: { url: "*/invoices", auth: awsSigV4 }
      solution:
        - send:
            method: GET
            url: "{{api}}/invoices"
            auth:
              type: awsSigV4
              accessKey: "{{awsAccessKey}}"
              secretKey: "{{awsSecretKey}}"
              region: eu-west-1
              service: execute-api
    - text: |
        An older partner API still uses OAuth 1.0. Send `GET {{api}}/timeline` with **OAuth 1.0**: consumer key `{{consumerKey}}`, consumer secret `{{consumerSecret}}`, signature method **HMAC-SHA1**. Then send it once more and compare the two `Authorization` headers in the server's **Traffic**.
      hints:
        - OAuth 1.0 signs with a timestamp and a random nonce, so no two sends carry the same signature.
        - "Auth tab → Type **OAuth 1.0** → Signature method HMAC-SHA1, Consumer key {{consumerKey}}, Consumer secret {{consumerSecret}}. Token and the other fields stay empty."
        - "GET {{api}}/timeline with that auth, Send twice. Open Lab · Cloud API under Servers: oauth_nonce and oauth_signature differ between the two."
      check:
        all:
          - request:
              server: api
              method: GET
              path: /timeline
              status: 200
              headers: { authorization: "OAuth *oauth_consumer_key=\"lab-app\"*oauth_signature=*" }
              count: 2
          - send: { url: "*/timeline", auth: oauth1 }
      solution:
        - send: &oauth1
            method: GET
            url: "{{api}}/timeline"
            auth:
              type: oauth1
              consumerKey: "{{consumerKey}}"
              consumerSecret: "{{consumerSecret}}"
              signatureMethod: HMAC-SHA1
        - send: *oauth1
quiz:
  - question: Why can't someone copy the signature from yesterday's AWS request and use it today?
    options:
      - The signature covers the time and the request itself, so it only fits that request, for a few minutes
      - AWS changes everyone's keys every night
      - Zorvik deletes signatures after sending
    answer: 0
    explain: Change the path, a header, the body or the time and the signature no longer matches. That's the point of signing.
  - question: What does "JWT (signed by Zorvik)" do that "Bearer token" doesn't?
    options:
      - It sends the token in the body instead of a header
      - It makes and signs a fresh token for every send, from your claims and key
      - It encrypts the token so nobody can read it
    answer: 1
    explain: With Bearer you paste a token someone gave you. With JWT (signed by Zorvik) you hold the key, and Zorvik signs a new token each time, with claims such as an exp an hour from now.
  - question: Where should an AWS secret key or a signing key live?
    options:
      - In the Secret key field, since it shows dots
      - In the request's URL, so it's easy to change
      - In a secret variable, with `{{name}}` in the field
    answer: 2
    explain: Auth fields are saved in the request, folder or workspace file, and so end up in Git. A secret variable's value stays on your computer.
---

A token or a password is like a house key: whoever copies it can use it, as often as they like. **Signing** works differently. Your secret never leaves your computer. Instead, every request gets a **signature**: a short code calculated from the request itself (its method, path, time and body) and your secret. The server does the same calculation with its copy of the secret and compares.

```sequence
participants: Zorvik, API
Note over Zorvik: signature = hash(secret, method + path + time + body)
Zorvik -> API: GET /invoices, Authorization: AWS4-HMAC-SHA256 … Signature=9c1f…
Note over API: recalculates it with its copy of the secret
API --> Zorvik: 200 OK, the signatures match
```

Change one letter of the request and the signature no longer matches. Send the same request again tomorrow and it's refused, because the time is part of the signature.

> [!note] Think of it like…
> Signing a cheque. Your signature on it only goes with that amount, that payee and that date. Nobody can raise the amount or cash it again next year, and you never had to hand over your pen.

## The signing types in Zorvik

All of them are in the request's **Auth** tab (and in **Folder settings…** and **Workspace settings…**, so a whole folder can share one):

| Type | Where you meet it |
|---|---|
| **JWT (signed by Zorvik)** | Your own services: Zorvik builds a token from the claims you write and signs it (HS, RS, PS or ES algorithms) |
| **AWS Signature v4** | API Gateway, S3, Lambda function URLs and other AWS services |
| **OAuth 1.0** | Twitter/X, older Atlassian and many enterprise APIs |
| **Hawk** | Services built on Mozilla's Hawk scheme |
| **Akamai EdgeGrid** | Akamai's APIs, with the values from your `.edgerc` |
| **Atlassian ASAP** | Service-to-service calls: a short-lived JWT signed with your service's key |

Zorvik signs **every send**, after pre-request scripts have run, over the final method, URL, headers and body, with a fresh timestamp and a fresh random value (a **nonce**). In a load test, each request is signed on its own.

## What arrives at the server

A signature travels in the `Authorization` header. AWS's shows the parts nicely:

```anatomy
AWS4-HMAC-SHA256 | the scheme and the hash used
Credential=AKIA…/20260928/eu-west-1/execute-api/aws4_request | who signed, the day, the region and the service
SignedHeaders=host;x-amz-date | which headers the signature covers
Signature=9c1f… | the proof, made with the secret key
```

A JWT from **JWT (signed by Zorvik)** goes out as `Authorization: Bearer eyJ…`. You can paste one into **Tools → Encoders → JWT** to read its claims. OAuth 1.0 sends `Authorization: OAuth oauth_consumer_key="…", oauth_nonce="…", oauth_signature="…", …`.

The lab's practice server can't check signatures: it doesn't have your secrets. It only checks that an `Authorization` header is there, and its **Traffic** shows what arrived. A real service recalculates every signature and answers `401` or `403` when one doesn't match.

> [!tip] Keys belong in secret variables
> Access keys, secret keys and private keys are saved in the file like every auth field. Put them in secret variables and type `{{awsSecretKey}}` in the field, so they stay on your computer and out of Git.

**You'll use this when…** you call an AWS API Gateway, an S3 bucket or a partner's OAuth 1.0 API, or a service that trusts tokens signed with a shared key. Set the auth once on a folder, and every request inside is signed fresh on every send.
