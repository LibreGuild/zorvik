---
id: import-and-export
title: Import and export
summary: Bring requests in from cURL, Postman and OpenAPI, and hand them out as cURL or ready-to-run code.
minutes: 5
lab:
  title: From a cURL command to Python
  goal: Import a cURL command, send and save it, then copy it as code for a teammate.
  minutes: 7
  servers:
    api:
      name: Weather API
      kind: http
      http:
        routes:
          - method: GET
            path: /forecast
            headers:
              - key: Content-Type
                value: application/json
            body: '{"city": "{{request.query.city}}", "tempC": 21, "sky": "sunny", "wind": "light breeze"}'
  steps:
    - text: |
        A colleague sent you this command:

        ```bash
        curl '{{api}}/forecast?city=Lisbon' -H 'Accept: application/json' -H 'X-Client: bootcamp'
        ```

        Open the collection's **+** menu → **Import…**, pick the **cURL** tab, paste it and press **Open as new request**. Then **Send** it.
      hints:
        - Import turns the command into a normal request tab, with its URL, query and headers filled in.
        - Click + at the top of the collection, choose Import…, then the cURL tab. Paste the whole command, starting with curl.
        - "Paste, press Open as new request, check the Headers tab (X-Client: bootcamp is there), then Send."
      check:
        all:
          - call: { method: import.curl, ok: true }
          - request: { server: api, method: GET, path: /forecast, query: { city: Lisbon }, headers: { x-client: bootcamp } }
      solution:
        - call: { method: import.curl, params: { text: "curl '{{api}}/forecast?city=Lisbon' -H 'Accept: application/json' -H 'X-Client: bootcamp'" } }
        - send:
            method: GET
            url: "{{api}}/forecast?city=Lisbon"
            headers: [{ key: Accept, value: application/json }, { key: X-Client, value: bootcamp }]
    - text: Save the imported request (**⌘/Ctrl + S**) with the name **Lisbon forecast**.
      hints:
        - An imported request is a new tab. It's not in the collection until you save it.
        - Press ⌘S (Ctrl+S on Windows), type the name and keep the folder at the top of the collection.
        - "Name: Lisbon forecast, then Save. It appears in the sidebar."
      check:
        saved:
          request: { name: Lisbon forecast, url: "*forecast?city=Lisbon*", headers: [{ key: X-Client, value: bootcamp }] }
      solution:
        - save:
            name: Lisbon forecast
            method: GET
            url: "{{api}}/forecast?city=Lisbon"
            headers: [{ key: Accept, value: application/json }, { key: X-Client, value: bootcamp }]
    - text: Your teammate writes Python. Open **More actions** (the **⋯** next to Save) → **Copy as cURL or code…** and choose **Python** in the list of languages. (Any other language works too.)
      hints:
        - The same dialog makes cURL for bash, the Windows Command Prompt and PowerShell, and code in 16 languages.
        - The languages are listed on the left of the dialog, under "Code". Type in the search box to find one.
        - "Pick Python (requests is the default library; HTTPX is next to it). The code appears right away; Copy puts it on the clipboard."
      check:
        call: { method: export.snippet, ok: true }
      solution:
        - call:
            method: export.snippet
            params:
              request:
                name: Lisbon forecast
                method: GET
                url: "{{api}}/forecast?city=Lisbon"
                headers: [{ key: Accept, value: application/json }, { key: X-Client, value: bootcamp }]
              path: null
              language: python
              resolveVariables: true
quiz:
  - question: A page in your web app shows the wrong data. How do you replay the exact request it made in Zorvik?
    options:
      - Retype the URL and guess the headers
      - Take a screenshot of the Network tab and import it
      - In the browser's developer tools, use Copy as cURL, then Import → cURL in Zorvik
    answer: 2
    explain: Browsers copy a request with every header and cookie as a cURL command, and Zorvik turns that into a request you can change and send again.
  - question: Which of these can Zorvik import?
    options:
      - Postman collections and environments, OpenAPI and Swagger documents, and cURL commands
      - Only files made by Zorvik
      - Any web page that documents an API
    answer: 0
    explain: The Import dialog detects the kind by itself. Postman scripts and variables come along, and an OpenAPI document gives you one request per operation.
  - question: You copy a request as **Python (requests)** with **Substitute variables** on. What do you get?
    options:
      - A Zorvik file that only Zorvik can read
      - Python code that sends the same request, with the variables' values filled in
      - A screenshot of the request
    answer: 1
    explain: The code is ready to run. With the switch on it holds real values, secrets too, so turn it off when you share code that should say `{{apiKey}}`.
---

You rarely start from nothing. The API docs show examples as `curl` commands, a colleague sends you a Postman collection, the backend team publishes an OpenAPI document. And when you find a bug, the developer fixing it wants the request in *their* language. Zorvik moves requests in both directions.

## Importing

Open the collection's **+** menu (or right-click a folder) → **Import…**. Zorvik understands:

- **cURL commands**, copied from docs, a terminal, or a browser (bash, Windows Command Prompt and PowerShell styles).
- **Postman** collections and environments. Scripts, tests and variables come along.
- **OpenAPI 3 and Swagger 2** documents, as JSON or YAML, from a file or a URL. Each operation becomes a request with a realistic example body. More about that in the testing unit.

**cURL** is a small command-line program that sends HTTP requests. It's everywhere, so its commands have become a common language for sharing a request:

```anatomy
curl 'https://api.example.com/forecast?city=Lisbon' | the URL, query included
-X POST | the method (GET when it's left out)
-H 'Accept: application/json' | a header; -H can repeat
-d '{"city": "Lisbon"}' | a body to send
```

> [!tip] Replay what your browser did
> In Chrome, Edge or Firefox, open the developer tools, right-click a request in the **Network** tab and choose **Copy → Copy as cURL**. Import it, and you have the exact request your web page made, headers and cookies included, ready to change and send again.

## Exporting

In a request tab, open **More actions** (the **⋯** next to Save) → **Copy as cURL or code…**. You can pick:

- **cURL** for bash and zsh (macOS, Linux), the Windows Command Prompt or PowerShell.
- **HTTPie, Wget and PowerShell** commands.
- **Code** in JavaScript (fetch or axios), Python (requests or HTTPX), Go, Java, Kotlin (OkHttp, Android), Swift (URLSession), C#, PHP, Ruby, Rust, Dart (Flutter) and C (libcurl).

A comment at the top of the code tells you when it can't do something Zorvik does for you, like answering a Digest login or signing each request.

**Substitute variables** decides what the code says: on, it holds real values, ready to run; off, it keeps `{{baseUrl}}` and `{{apiKey}}`, safe to paste in a chat.

```flow
API docs, browser -[cURL]-> Import -> Request in Zorvik
Request in Zorvik -[Copy as cURL or code]-> Terminal, Python, JavaScript, Go, Java, Swift, Kotlin, …
```

> [!note] Think of it like…
> A good translator. Someone hands you a note in cURL, and you read it in Zorvik. You answer in Zorvik, and your colleague receives it in Python.

## And whole collections?

A whole collection needs no export: the workspace is already a folder of small, readable files. Put it in Git, and your teammates open the same folder in their Zorvik.

**You'll use this when…** a bug report arrives as a pasted cURL command. You import it, reproduce the bug in a minute, save it next to your other requests, and send the developer the fix-ready request as code in their language.
