---
id: mock-from-spec
title: A mock from a spec or a folder
summary: Zorvik can build a whole mock in seconds from an OpenAPI document or from a folder of saved requests.
minutes: 6
lab:
  title: Coffee from a contract
  goal: Turn an OpenAPI document and a folder of saved requests into mocks, without typing a single route.
  minutes: 8
  files:
    comet-coffee.yaml: &comet |
      openapi: 3.0.3
      info:
        title: Comet Coffee
        version: "1.0.0"
        description: The API behind the Comet Coffee app.
      paths:
        /drinks:
          get:
            summary: List drinks
            responses:
              "200":
                description: The menu
                content:
                  application/json:
                    example:
                      - { id: 1, name: Nebula Latte, price: 4.5 }
                      - { id: 2, name: Stardust Mocha, price: 5 }
        /drinks/{drinkId}:
          get:
            summary: Get a drink
            parameters:
              - { name: drinkId, in: path, required: true, schema: { type: integer }, example: 1 }
            responses:
              "200":
                description: One drink
                content:
                  application/json:
                    example: { id: 1, name: Nebula Latte, price: 4.5 }
              "404":
                description: No drink with that id
        /orders:
          post:
            summary: Place an order
            requestBody:
              content:
                application/json:
                  example: { drinkId: 1, size: large }
            responses:
              "201":
                description: Order placed
                content:
                  application/json:
                    example: { orderId: 101, status: brewing }
    requests/Loyalty/_folder.yaml: "name: Loyalty\n"
    requests/Loyalty/Get points.yaml: "name: Get points\nseq: 0\nmethod: GET\nurl: '{{baseUrl}}/loyalty/points'\n"
    requests/Loyalty/Redeem a reward.yaml: "name: Redeem a reward\nseq: 1\nmethod: POST\nurl: '{{baseUrl}}/loyalty/rewards'\nbody:\n  type: json\n  text: '{\"reward\": \"free-cookie\"}'\n"
  servers:
    docs:
      name: Comet Coffee docs
      kind: http
      http:
        routes:
          - method: GET
            path: /comet-coffee.yaml
            headers: [{ key: Content-Type, value: application/yaml }]
            body: *comet
  steps:
    - text: |
        The Comet Coffee team published their API contract at `{{docs}}/comet-coffee.yaml`. In **Servers**, click **＋** → **Mock from OpenAPI…**, choose **URL**, paste that address, name the server **Comet Coffee mock** and press **Create mock**.

        (The same document is also in the Bootcamp workspace folder as `comet-coffee.yaml`, if you prefer **File**.)
      hints:
        - Mock from OpenAPI is at the bottom of the ＋ menu in the Servers section, below the server kinds.
        - The Lab Guide shows the full address of {{docs}}. Copy it, then add /comet-coffee.yaml at the end.
        - Servers → ＋ → Mock from OpenAPI… → URL → paste the address → Server name Comet Coffee mock → Create mock.
      check:
        call: { method: mock.fromOpenApi, ok: true, result: { routes: ">=3" } }
      solution:
        - call:
            method: mock.fromOpenApi
            params: { url: "{{docs}}/comet-coffee.yaml", name: Comet Coffee mock }
    - text: |
        Look at the routes Zorvik made: one per operation in the document, each answering with the document's example. Press **Start**; the lab then orders a menu from `GET /drinks`.
      hints:
        - The new mock opened in a tab. Starting it works just like the mocks you built by hand.
        - Press Start at the top right of the Comet Coffee mock tab. If the port is in use, change Port and try again.
        - Open Comet Coffee mock in Servers and press Start. GET /drinks then answers with Nebula Latte and Stardust Mocha.
      check:
        probe:
          kind: http
          path: /drinks
          expect:
            status: 200
            json: [{ name: Nebula Latte }]
      solution:
        - call:
            method: server.save
            params:
              id: Comet Coffee mock
              server: &cometMock
                name: Comet Coffee mock
                kind: http
                port: 0
                http:
                  routes:
                    - name: List drinks
                      method: GET
                      path: /drinks
                      status: 200
                      headers: [{ key: Content-Type, value: application/json }]
                      body: "[\n  {\n    \"id\": 1,\n    \"name\": \"Nebula Latte\",\n    \"price\": 4.5\n  },\n  {\n    \"id\": 2,\n    \"name\": \"Stardust Mocha\",\n    \"price\": 5\n  }\n]"
                    - name: Get a drink
                      method: GET
                      path: /drinks/:drinkId
                      status: 200
                      headers: [{ key: Content-Type, value: application/json }]
                      body: "{\n  \"id\": 1,\n  \"name\": \"Nebula Latte\",\n  \"price\": 4.5\n}"
                    - name: Place an order
                      method: POST
                      path: /orders
                      status: 201
                      headers: [{ key: Content-Type, value: application/json }]
                      body: "{\n  \"orderId\": 101,\n  \"status\": \"brewing\"\n}"
        - call:
            method: server.start
            params: { id: Comet Coffee mock, server: *cometMock }
    - text: |
        The loyalty team has no document yet, only saved requests in the **Loyalty** folder of your collection. Right-click that folder, choose **Mock this folder…** and press **Create mock**.
      hints:
        - Folders in the collection have a right-click menu with Run…, Mock this folder… and Load test this folder….
        - Open the Collection section, right-click Loyalty and pick Mock this folder…. The suggested name, Loyalty mock, is fine.
        - Collection → right-click Loyalty → Mock this folder… → Create mock. You get one route per request, answering 200 with {} until you fill in the answers.
      check:
        call: { method: mock.fromFolder, ok: true, params: { folder: Loyalty } }
      solution:
        - call:
            method: mock.fromFolder
            params: { folder: Loyalty, name: Loyalty mock }
quiz:
  - question: What does "Mock from OpenAPI…" put in each route's answer?
    options:
      - The operation's first success response, using the document's example or one made from its schema
      - An empty body, always
      - A copy of the real server's answer, downloaded live
    answer: 0
    explain: Zorvik reads each operation and answers with its first success response. A good example in the document means a realistic mock for free.
  - question: Your team has saved requests for a new feature but no OpenAPI document. What's the quickest way to a mock?
    options:
      - Write an OpenAPI document first
      - Copy each request into a route by hand
      - Right-click the folder and choose "Mock this folder…"
    answer: 2
    explain: You get one route per request, answering 200 with {} until you fill in real answers.
  - question: Half of the new API already runs on a test server; the other half doesn't exist yet. How can one mock cover both?
    options:
      - It can't, you need two apps
      - Mock the missing routes and set "Requests no route matches" to "Forward to a backend"
      - Stop the test server
    answer: 1
    explain: Requests without a matching route are passed to the real back end, so you only mock what's missing.
---

Building routes by hand is great for learning, but real APIs have dozens of endpoints. Typing them all would take a day, and you'd make mistakes. Luckily, the shape of an API is usually written down somewhere already. Zorvik can turn that into a mock in seconds.

## From an OpenAPI document

An **OpenAPI document** (older versions are called *Swagger*) is a file, in YAML or JSON, that describes an API: its paths, the methods on each path, the parameters, and what every answer looks like. Teams often agree on this document first; it's the **contract** between the front end and the back end.

```anatomy
/drinks: | a path of the API
get: | an operation: a method on that path
responses: "200": | the success answer
example: [{"name": "Nebula Latte"}] | what the mock will send back
```

In **Servers**, **＋** → **Mock from OpenAPI…** reads a document from a **File**, a **URL** or text you **Paste**. You get one route per operation, answering with its first success response: the document's **example** if it has one, otherwise an answer made up from the **schema** (the description of each field and its type). Paths like `/drinks/{drinkId}` become routes like `/drinks/:drinkId`.

> [!note] Think of it like…
> An architect's model. From the blueprint, a model maker builds a cardboard house in an afternoon, so the buyers can plan where the sofa goes long before the real walls are up. The OpenAPI document is the blueprint; the mock is the model.

## From saved requests

No document yet? If you already have saved requests, right-click a folder and choose **Mock this folder…** (or **Mock the collection…** from the collection's **＋** menu). Every HTTP request becomes a route with the same method and path, answering `200` with `{}` until you fill in real answers.

And when you get a response you'd like to keep, the **Mock** button above the response adds a route that answers exactly like it.

```flow
OpenAPI document -[Mock from OpenAPI…]-> Mock API
Folder of requests -[Mock this folder…]-> Mock API
A response you got -[Mock]-> One more route
```

## Mock only what's missing

Often part of an API already runs on a test server and part doesn't exist yet. Mock the missing routes, then under **Requests no route matches** choose **Forward to a backend** and enter the real server's address. Requests the mock knows get the mock's answer; everything else goes to the real server, and its answer comes back unchanged.

> [!tip] Regenerate, don't hand-edit
> When the document changes, make a fresh mock from it rather than patching routes by hand. That's how the mock stays true to the contract.

**You'll use this when…** the API team sends you a document on Monday and you want the app talking to something realistic on Monday afternoon, not after the back end ships.
