---
id: contract-checks
title: Contract checks with OpenAPI
summary: Import an API's OpenAPI document, and every response is checked against it automatically.
minutes: 6
lab:
  title: Hold the API to its word
  goal: Import a coffee shop's OpenAPI document and find the endpoint that breaks it.
  minutes: 8
  vars:
    baseUrl: "{{api}}"
  files:
    coffee-shop-openapi.yaml: &spec |
      openapi: 3.0.3
      info:
        title: Coffee Shop API
        version: 1.0.0
        description: Drinks and prices of the Bootcamp coffee shop.
      servers:
        - url: {{api}}
      paths:
        /menu:
          get:
            summary: List the menu
            responses:
              "200":
                description: Every drink on the menu
                content:
                  application/json:
                    schema:
                      type: array
                      items:
                        $ref: "#/components/schemas/Drink"
        /specials:
          get:
            summary: List the specials
            responses:
              "200":
                description: The specials of the day
                content:
                  application/json:
                    schema:
                      type: array
                      items:
                        $ref: "#/components/schemas/Drink"
      components:
        schemas:
          Drink:
            type: object
            required: [name, price]
            properties:
              name:
                type: string
              price:
                type: number
                description: Price in euros
              vegan:
                type: boolean
  servers:
    api:
      name: Coffee Shop API
      kind: http
      http:
        routes:
          - method: GET
            path: /openapi.yaml
            headers:
              - key: Content-Type
                value: application/yaml
            body: *spec
          - method: GET
            path: /menu
            headers:
              - key: Content-Type
                value: application/json
            body: '[{"name": "Flat white", "price": 3.2, "vegan": false}, {"name": "Oat latte", "price": 3.6, "vegan": true}]'
          - method: GET
            path: /specials
            headers:
              - key: Content-Type
                value: application/json
            body: '[{"name": "Pumpkin spice latte", "price": "4.50", "vegan": false}]'
  steps:
    - text: |
        The coffee shop publishes its OpenAPI document at `{{api}}/openapi.yaml`. Open the collection's **+** menu → **Import…**, pick the **OpenAPI URL** tab, enter that address and press **Import**.
      hints:
        - Type the address itself (it starts with http://127.0.0.1:). The lab also saved the same document as coffee-shop-openapi.yaml in the workspace folder, if you prefer the File tab.
        - Collection + → Import… → OpenAPI URL, paste the address, keep "Import into" at the top of the collection, then Import.
        - "The summary says: Imported \"Coffee Shop API\", 2 requests. A new folder with that name appears in the sidebar."
      check:
        call: { method: 're:^import\.(url|file)$', ok: true, result: { name: Coffee Shop API, requests: 2 } }
      solution:
        - call: { method: import.url, params: { url: "{{api}}/openapi.yaml", parent: "" } }
    - text: |
        Open **Coffee Shop API → List the menu** and press **Send**. Its URL uses the `baseUrl` variable, which the Lab environment already has, so it works right away. Open the **Tests** tab: you never wrote a test, yet there is one.
      hints:
        - The imported folder is in the sidebar. Requests are named after the operations in the document.
        - Click List the menu, press Send, then open the Tests tab of the response.
        - "The test is called Matches the API spec (GET /menu → 200), and it passes: both drinks have a name and a numeric price."
      check:
        send: { url: "*/menu", tests: [{ name: "Matches the API spec*", passed: true }] }
      solution:
        - call:
            method: http.send
            params:
              requestId: lab-menu
              request: { name: List the menu, method: GET, url: "{{baseUrl}}/menu", openapi: { operation: GET /menu } }
              path: Coffee Shop API/List the menu.yaml
    - text: Now open **List the specials** from the same folder and send it. Does it keep its promise?
      hints:
        - It's the request right below List the menu.
        - Send it and open the Tests tab. The status is 200, but look at the test.
        - "Matches the API spec fails. Its message names the field that doesn't fit the document."
      check:
        send: { url: "*/specials", tests: [{ name: "Matches the API spec*", passed: false }] }
      solution:
        - call:
            method: http.send
            params:
              requestId: lab-specials
              request: { name: List the specials, method: GET, url: "{{baseUrl}}/specials", openapi: { operation: GET /specials } }
              path: Coffee Shop API/List the specials.yaml
    - text: Which field of the specials breaks the contract? Type its name.
      hints:
        - The failed test's message points at the problem, like $[0].something.
        - "$[0] is the first drink in the list. Compare its value with the document: which type does it expect?"
        - "The document says price is a number; the server sent the text \"4.50\". Type price."
      check:
        answer: ["price", "*price*"]
      solution:
        - answer: price
quiz:
  - question: What does the **Matches the API spec** test check?
    options:
      - That the server is fast enough
      - That the status is documented, and a JSON body fits the documented schema
      - That the request's URL is spelled right
    answer: 1
    explain: The document describes each operation's responses. Zorvik checks the status against them, and the body's fields, types and required fields against the schema.
  - question: The document says `price` is a number, and the server sends `"4.50"`. What does the failing test tell you?
    options:
      - The server and the document disagree; fix the server, or change the document on purpose
      - Zorvik has a bug
      - Nothing, the price looks right
    answer: 0
    explain: A contract only helps if both sides keep it. Apps written from the document would try to do maths with a piece of text.
  - question: Where does Zorvik keep the OpenAPI document after importing it?
    options:
      - Nowhere, it only reads it once
      - In your browser
      - In the workspace's `specs/` folder, next to the requests, so it's in Git too
    answer: 2
    explain: The imported folder links to it, and every send and every run checks responses against it. After an API change, Update from API spec… brings in the new version.
---

When two teams build two halves of one product, like an app and its API, they need to agree on what the API sends: which endpoints exist, which fields each answer has, and what type each field is. That agreement is the API's **contract**. When the API quietly breaks it, the app breaks too, often in front of users.

## OpenAPI

Most teams write the contract down as an **OpenAPI document** (older versions are called **Swagger**). It's a YAML or JSON file that describes every operation, and a **schema** for each answer: the shape the data must have.

```anatomy
GET /specials | an operation: method and path
responses: "200" | the statuses it may answer with
type: array, items: Drink | the answer is a list of drinks
Drink: required [name, price] | every drink must have these fields
price: type number | and price must be a number, not text
```

> [!note] Think of it like…
> A rental contract. It says what you get: two bedrooms, a balcony, heating that works. If you move in and the balcony is missing, you don't argue about how the flat feels; you point at the contract.

## Contract checks in Zorvik

Import an OpenAPI document (collection **+** → **Import…**, from a file or a URL) and Zorvik does three things:

1. It creates a **folder with one request per operation**, with example bodies filled in, and an **environment** named after the API with its `baseUrl`.
2. It **keeps the document** in your workspace's `specs/` folder, so it travels with the requests in Git.
3. From then on, **every response** to those requests gets a test: **Matches the API spec**.

```flow
OpenAPI document -[Import]-> Folder of requests
Response -[checked against the schema]-> Matches the API spec
```

The test checks that the status is one the document lists, and that a JSON body fits the schema: types, required fields, allowed values and more. Formats and patterns, like "must look like an email", are not checked. The test counts like one you wrote yourself: in the Tests tab, in collection runs, on the command line and in CI reports.

> [!tip] Contract checks for free
> You get this test without writing a line of JavaScript. Add your own tests for the business rules, like "the total is the sum of the items", and let the contract check guard the shape of the data.

## When the API changes

APIs evolve. When a new version of the document is out, right-click the imported folder → **Update from API spec…**: new operations come in, and your own edits to the requests stay. You can switch the checks off for a folder in **Folder settings… → API spec**, but think twice: that test is what tells you the API changed under your feet.

**You'll use this when…** the backend team changes a field from a number to text "just for one endpoint". Your contract check turns red on the next run, days before the mobile app starts showing prices of *NaN €*.
