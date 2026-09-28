---
id: secret-variables
title: Secrets stay secret
summary: Mark API keys and passwords as secret, and they stay on your computer instead of in the workspace files you share.
minutes: 5
lab:
  title: Keep the key out of Git
  goal: Get an API key, store it as a secret variable, use it, and share the request without leaking the key.
  minutes: 8
  servers:
    api:
      name: Payments API
      kind: http
      http:
        routes:
          - method: POST
            path: /keys
            status: 201
            headers:
              - key: Content-Type
                value: application/json
            body: '{"apiKey": "sk_test_{{secret.key}}", "note": "This key is shown only once. Store it somewhere safe."}'
          - method: GET
            path: /balance
            matchHeaders:
              - key: X-Api-Key
                value: "sk_test_{{secret.key}}"
            headers:
              - key: Content-Type
                value: application/json
            body: '{"currency": "EUR", "available": 1250.40, "pending": 80.00}'
          - method: GET
            path: /balance
            status: 401
            headers:
              - key: Content-Type
                value: application/json
            body: '{"error": "Missing or wrong X-Api-Key header"}'
  steps:
    - text: Send `POST {{api}}/keys`. Like a developer portal, the server creates an API key for you and shows it once.
      hints:
        - Change the method from GET to POST in the menu left of the URL bar.
        - Press ⌘N, pick POST, type {{api}}/keys and press Send.
        - "Method POST, URL {{api}}/keys, then Send. The answer is 201 Created with an apiKey that starts with sk_test_."
      check:
        request: { server: api, method: POST, path: /keys }
      solution:
        - send: { method: POST, url: "{{api}}/keys" }
    - text: |
        Copy the `apiKey` value. Press **⌘/Ctrl + E**, select the **Lab** environment and add a variable `apiKey` with the key as its value. Click the **lock** at the end of the row to make it a secret, then press **Save changes**.
      hints:
        - Copy only the key itself, without the quotes around it.
        - The lock icon sits at the right end of the value field. Open means plain, closed means secret.
        - "Variable apiKey, value sk_test_… (your key), lock closed, then Save changes. The value now shows as dots."
      check:
        saved:
          environment: { name: Lab, variables: [{ key: apiKey, secret: true, value: "" }] }
      solution:
        - call:
            method: env.save
            params:
              id: Lab
              environment:
                name: Lab
                variables:
                  - { key: api, value: "{{api}}" }
                  - { key: api_port, value: "{{api_port}}" }
                  - { key: api_host, value: "{{api_host}}" }
                  - { key: apiKey, value: "sk_test_{{secret.key}}", secret: true }
    - text: Send `GET {{api}}/balance` with a header `X-Api-Key` whose value is `{{apiKey}}`. Use the variable, not the key itself.
      hints:
        - Headers are in the Headers tab below the URL bar.
        - Add a row with X-Api-Key as the name and {{apiKey}} as the value. Hover {{apiKey}}; it shows as a secret.
        - "GET {{api}}/balance, header X-Api-Key: {{apiKey}}, then Send. Without the right key the server answers 401."
      check:
        all:
          - request: { server: api, method: GET, path: /balance, headers: { x-api-key: "sk_test_{{secret.key}}" } }
          - any:
              - send: { url: "*/balance", request: { headers: [{ value: "*apiKey*" }] } }
              - send: { url: "*/balance", request: { auth: { value: "*apiKey*" } } }
      solution:
        - send: { method: GET, url: "{{api}}/balance", headers: [{ key: X-Api-Key, value: "{{apiKey}}" }] }
    - text: Now share it safely. Open **More actions** (the **⋯** next to Save), choose **Copy as cURL or code…** and switch **Substitute variables** off. The command keeps `{{apiKey}}` instead of your key.
      hints:
        - With Substitute variables on, the command would contain the real key. That is fine on your own computer, not in a chat.
        - The ⋯ button is at the right end of the URL bar. The switch is next to the format menu.
        - "⋯ → Copy as cURL or code… → turn Substitute variables off. Read the command: the header says X-Api-Key: {{apiKey}}."
      check:
        call: { method: export.curl, ok: true, params: { resolveVariables: false } }
      solution:
        - call:
            method: export.curl
            params:
              request: { name: Balance, method: GET, url: "{{api}}/balance", headers: [{ key: X-Api-Key, value: "{{apiKey}}" }] }
              path: null
              flavor: bash
              resolveVariables: false
quiz:
  - question: You mark `apiKey` as secret. Where is its value kept?
    options:
      - In the environment file in your workspace, encrypted
      - On a Zorvik cloud server
      - On this computer, in Zorvik's own data folder, never in the workspace files
    answer: 2
    explain: The environment file keeps the name with an empty value. The value itself stays in the app's data folder on your computer.
  - question: A teammate pulls your workspace from Git. What do they get for `apiKey`?
    options:
      - The variable with an empty value, so they add their own key
      - Your key, ready to use
      - Nothing at all, the variable is missing
    answer: 0
    explain: Everyone sees that the request needs an apiKey, and each person fills in their own. Nobody's key ends up in the repository.
  - question: You copy a request as cURL with **Substitute variables** on, to paste in a team chat. What is the risk?
    options:
      - None, secrets are always hidden
      - The command contains the real values, secrets included
      - The command won't run anywhere else
    answer: 1
    explain: Substituted means real values, including secret ones, and Zorvik warns you about it. Turn the switch off to share `{{apiKey}}` instead.
---

Some values must never leak: API keys, passwords, tokens. A leaked key can cost real money, and keys pushed to a public Git repository are found by bots within minutes.

Zorvik workspaces are made to be shared through Git, so a normal variable is written into a file in the workspace. That's great for `baseUrl`, and dangerous for a password. That's what **secret variables** are for.

## Plain or secret

Every variable row in **Environments & variables** (**⌘/Ctrl + E**) has a **lock** at its right end:

- **Open lock: plain.** The value is saved in the workspace file. Use it for addresses, ids and settings.
- **Closed lock: secret.** The file keeps only the name, with an empty value. The real value lives on your computer, in Zorvik's own data folder, and shows as dots.

```flow
apiKey (secret) -[name only]-> Workspace file -> Git
apiKey (secret) -[value]-> This computer only
```

Secrets work like any other variable: `{{apiKey}}` in a header, a URL or the Auth tab is filled in when the request is sent.

> [!note] Think of it like…
> A shared recipe book. The recipe says "use the key to the spice cabinet", and everyone knows a key is needed. But the key itself stays on your own keyring, not taped inside the book.

## Where secrets stay hidden

- **Workspace files:** only the name, never the value.
- **History:** URLs are saved with secret values hidden.
- **Collection run reports:** a secret in a URL is shown as `{{apiKey}}`.
- **AI agents:** secret values are masked in everything an agent sees.

There is one place where you decide: **More actions** (the **⋯** next to the Save button) → **Copy as cURL or code…**. With **Substitute variables** on, the code contains the real values, secrets too, ready to run on your computer. Switch it off before you share it.

> [!warning] Secrets are per computer
> A secret you typed in is on this computer only. On a new laptop, or on a teammate's, fill it in again. On a build server, give it to the `zorvik` command with `--var apiKey=…`.

## Where the key goes in the request

Most APIs want the key in a header such as `X-Api-Key` or `Authorization`. You can add the header yourself, or use the request's **Auth** tab: **API key** puts it in a header or the query, **Bearer token** sends `Authorization: Bearer …`. Either way, type `{{apiKey}}`, never the key itself, so the saved request stays safe to commit.

```sequence
You -> Server: GET /balance, X-Api-Key sk_test_…
Note over You: the file says {{apiKey}}
Server --> You: 200 OK {"available": 1250.40}
```

**You'll use this when…** you commit a collection to your team's repository. Every request that needs a key says `{{apiKey}}`, and nobody's key is in the history of the project.
