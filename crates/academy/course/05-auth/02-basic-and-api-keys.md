---
id: basic-and-api-keys
title: Basic auth and API keys
summary: The simplest credentials are a username and password (Basic auth) or a long secret key, sent with every request.
minutes: 6
lab:
  title: Log in twice, then share the key
  goal: Use Basic auth and an API key, then put the key on a folder so every request inside inherits it.
  minutes: 8
  playground: true
  vars:
    user: ada
    password: "{{secret.pw}}"
    apiKey: "{{secret.apikey}}"
  servers:
    api:
      name: Weather API
      kind: http
      http:
        routes:
          - name: Weather
            method: GET
            path: /weather
            matchHeaders:
              - { key: X-API-Key, value: "{{secret.apikey}}" }
            headers:
              - { key: Content-Type, value: application/json }
            body: '{"city": "Lisbon", "tempC": 21, "sky": "sunny"}'
          - name: Forecast
            method: GET
            path: /forecast
            matchHeaders:
              - { key: X-API-Key, value: "{{secret.apikey}}" }
            headers:
              - { key: Content-Type, value: application/json }
            body: '{"city": "Lisbon", "days": [{"day": "Mon", "tempC": 22}, {"day": "Tue", "tempC": 19}]}'
          - name: Missing or wrong key
            method: "*"
            path: /*
            status: 401
            headers:
              - { key: Content-Type, value: application/json }
            body: '{"error": "unauthorized", "message": "Send your API key in the X-API-Key header."}'
  steps:
    - text: |
        The practice server has a door that opens for one user. Send `GET {{playground}}/basic-auth/{{user}}/{{password}}` with **Basic auth**: username `{{user}}`, password `{{password}}`.
      hints:
        - Without credentials this address answers 401. The **Auth** tab has a **Basic auth** type with two fields.
        - "Auth tab: Type = Basic auth, Username = {{user}}, Password = {{password}}. Type the variables with their braces."
        - "URL {{playground}}/basic-auth/{{user}}/{{password}}, Basic auth as above, then Send. The answer says \"authenticated\": true."
      check:
        send: { url: "*/basic-auth/ada/{{secret.pw}}", status: 200, auth: basic }
      solution:
        - send:
            method: GET
            url: "{{playground}}/basic-auth/{{user}}/{{password}}"
            auth: { type: basic, username: "{{user}}", password: "{{password}}" }
    - text: |
        The Weather API wants an API key instead. Send `GET {{api}}/weather` with **API key** auth: key name `X-API-Key`, value `{{apiKey}}`, added to the header.
      hints:
        - Try it without a key first if you like; the 401 body says which header it wants.
        - "Auth tab: Type = API key, Key = X-API-Key, Value = {{apiKey}}, Add to = Header."
        - "URL {{api}}/weather with that auth, then Send. You should get the weather and a 200."
      check:
        all:
          - request: { server: api, method: GET, path: /weather, status: 200, headers: { x-api-key: "{{secret.apikey}}" } }
          - send: { auth: apiKey }
      solution:
        - send:
            method: GET
            url: "{{api}}/weather"
            auth: { type: apiKey, key: X-API-Key, value: "{{apiKey}}", location: header }
    - text: |
        Set the key once for a whole folder. Create a folder **Weather**, give it the same API key in its **Folder settings…**, then create a request inside it for `{{api}}/forecast`. Leave that request's auth on **Inherit from parent** and send it.
      hints:
        - Folders can hold auth that every request inside them uses. Requests say **Inherit from parent** by default.
        - "Collection: + → New folder → Weather. Right-click Weather → Folder settings… → Auth: API key, X-API-Key, {{apiKey}}, Header → Save."
        - "Right-click Weather → New HTTP request, name it Forecast, URL {{api}}/forecast, keep Auth on Inherit from parent, then Send."
      check:
        all:
          - saved: { folder: { auth: { type: apiKey, key: X-API-Key } } }
          - request: { server: api, method: GET, path: /forecast, status: 200 }
          - send: { url: "*/forecast", status: 200, auth: inherit }
      solution:
        - call: { method: folder.create, params: { parent: "", name: Weather } }
        - call:
            method: folder.save
            params:
              path: Weather
              meta:
                name: Weather
                auth: { type: apiKey, key: X-API-Key, value: "{{apiKey}}", location: header }
        - call:
            method: http.send
            params:
              requestId: lab-forecast
              path: Weather/Forecast.yaml
              request: { name: Forecast, seq: 0, method: GET, url: "{{api}}/forecast" }
quiz:
  - question: "What does a Basic auth header like `Authorization: Basic YWRhOnNlY3JldA==` protect against?"
    options:
      - Nothing on its own; it's only Base64, which anyone can decode
      - Eavesdroppers, because Base64 hides the password
      - Replay attacks, because the header changes every time
    answer: 0
    explain: Base64 is an encoding, not encryption. Basic auth is only safe over HTTPS, which encrypts the whole request.
  - question: Why do many APIs prefer an API key in a header over one in the query string?
    options:
      - Headers are sent faster
      - Query strings can't hold long values
      - URLs end up in logs, browser history and screenshots
    answer: 2
    explain: A key in the URL leaks wherever the URL is written down. Headers are not usually logged.
  - question: A request's auth says "Inherit from parent". Where does its credential come from?
    options:
      - "Nowhere: the request is sent without any auth"
      - From the nearest folder above it that sets auth, else the workspace
      - From the request you sent most recently to that host
    answer: 1
    explain: Set a login once on a folder or the workspace and every request inside uses it. Change it in one place when it expires.
---

Now that you know the difference between *who you are* and *what you may do*, let's send some credentials. The two simplest kinds are still everywhere.

## Basic auth: a username and a password

With **Basic auth** the client joins the username and password with a colon, encodes them in **Base64** (a way to write any bytes as plain letters and digits) and sends the result in the `Authorization` header on every request:

```anatomy
Authorization: Basic YWRhOnNlY3JldA== | the header, the scheme, then the credentials
YWRhOnNlY3JldA== | Base64 of ada:secret
```

> [!warning] Encoded is not encrypted
> Anyone who sees that header can decode it in a second (try **Tools → Encoders → Base64**). Basic auth is only acceptable over **HTTPS**, which encrypts the whole request on its way. The next unit shows why.

In Zorvik, choose **Basic auth** in the **Auth** tab and fill **Username** and **Password**; Zorvik builds the header.

## API keys: one long secret

An **API key** is a long random string the provider gives you, often from a developer dashboard. You send it with every request, usually in a header the API chooses, such as `X-API-Key: 3f9c…`, and sometimes as a query parameter like `?api_key=3f9c…`.

```sequence
You -> Weather API: GET /weather (X-API-Key: 3f9c…)
Note over Weather API: looks up the key: whose is it, what may it do?
Weather API --> You: 200 OK {"tempC": 21}
```

Keys identify an *application* or *account* rather than a person, so they're common for server-to-server calls. In Zorvik choose **API key** in the **Auth** tab, then set **Key** (the header or parameter name), **Value** and **Add to** (Header or Query parameter).

> [!note] Think of it like…
> Basic auth is telling the guard your name and password every time you walk in. An API key is a membership card: whoever holds it gets in, so you guard it like a house key.

## Keep secrets out of your requests

Type `{{apiKey}}` rather than the key itself. The value then lives in an **environment**, not in the saved request, so you can share the request without sharing the key, and use a different key per environment. A variable can also be marked secret so its value stays on your computer.

## Set it once: folder auth

Most requests to one API use the same credentials. Instead of repeating them, put the auth on a **folder** (right-click it → **Folder settings…** → **Auth**) or on the workspace. Every request inside whose auth is **Inherit from parent** uses it:

```flow
Workspace -> Folder: Weather (API key) -> Forecast (inherit)
```

When the key changes, you update one place.

**You'll use this when…** you connect to a partner's API with the key from their dashboard, or test an internal service behind a username and password. Put the credential on the folder once, and every request you add just works.
