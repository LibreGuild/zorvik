---
id: ship-it
title: Ship it
summary: Your final mission, launching the Rocket Pizza API from contract to code, uses everything you've learned.
minutes: 5
lab:
  title: Launch Rocket Pizza
  goal: Take the Rocket Pizza API through the whole release checklist and hand it to the mobile team.
  minutes: 20
  files:
    rocket-pizza.yaml: &contract |
      openapi: 3.0.3
      info:
        title: Rocket Pizza
        version: "1.0.0"
        description: Order pizza from the Rocket Pizza mobile app.
      paths:
        /pizzas:
          get:
            summary: List pizzas
            operationId: listPizzas
            responses:
              "200":
                description: Today's menu
                content:
                  application/json:
                    schema:
                      type: array
                      items:
                        $ref: "#/components/schemas/Pizza"
                    example:
                      - { id: 1, name: Margherita, price: 9.5, vegetarian: true }
                      - { id: 2, name: Pepperoni Blast-off, price: 11, vegetarian: false }
        /pizzas/{pizzaId}:
          get:
            summary: Get a pizza
            operationId: getPizza
            parameters:
              - name: pizzaId
                in: path
                required: true
                schema: { type: integer }
                example: 1
            responses:
              "200":
                description: One pizza
                content:
                  application/json:
                    schema:
                      $ref: "#/components/schemas/Pizza"
                    example: { id: 1, name: Margherita, price: 9.5, vegetarian: true }
              "404":
                description: No pizza with that id
        /orders:
          post:
            summary: Place an order
            operationId: placeOrder
            requestBody:
              required: true
              content:
                application/json:
                  schema:
                    $ref: "#/components/schemas/NewOrder"
                  example: { pizzaId: 1, quantity: 2 }
            responses:
              "201":
                description: Order placed
                content:
                  application/json:
                    schema:
                      $ref: "#/components/schemas/Order"
                    example: { orderId: 501, status: in the oven, etaMinutes: 20 }
      components:
        schemas:
          Pizza:
            type: object
            required: [id, name, price]
            properties:
              id: { type: integer }
              name: { type: string }
              price: { type: number }
              vegetarian: { type: boolean }
          NewOrder:
            type: object
            required: [pizzaId, quantity]
            properties:
              pizzaId: { type: integer }
              quantity: { type: integer, minimum: 1 }
          Order:
            type: object
            required: [orderId, status]
            properties:
              orderId: { type: integer }
              status: { type: string }
              etaMinutes: { type: integer }
  servers:
    staging:
      name: Rocket Pizza staging
      kind: http
      http:
        routes:
          - name: List pizzas
            method: GET
            path: /pizzas
            headers: [{ key: Content-Type, value: application/json }]
            body: '[{"id": 1, "name": "Margherita", "price": 9.5, "vegetarian": true}, {"id": 2, "name": "Pepperoni Blast-off", "price": 11, "vegetarian": false}, {"id": 3, "name": "Veggie Orbit", "price": 10.5, "vegetarian": true}]'
            delayMs: 15
          - name: Get a pizza
            method: GET
            path: /pizzas/:pizzaId
            headers: [{ key: Content-Type, value: application/json }]
            body: '{"id": 1, "name": "Margherita", "price": 9.5, "vegetarian": true}'
          - name: Place an order
            method: POST
            path: /orders
            status: 201
            headers: [{ key: Content-Type, value: application/json }]
            body: '{"orderId": 501, "status": "in the oven", "etaMinutes": 20}'
          - name: The contract
            method: GET
            path: /openapi.yaml
            headers: [{ key: Content-Type, value: application/yaml }]
            body: *contract
  steps:
    - text: |
        **Import the contract.** The back-end team deployed Rocket Pizza to a staging server, and it publishes its contract at `{{staging}}/openapi.yaml`. In the **Collection**, click **＋** → **Import…**, open **OpenAPI URL**, paste that address and press **Import**.

        (The contract is also in the Bootcamp workspace folder as `rocket-pizza.yaml`. Importing the file asks for the **base URL**; give it `{{staging}}`.)
      hints:
        - Importing turns every operation in the contract into a saved request, in a new folder.
        - The ＋ button at the top of the Collection section has Import… just below the New entries. The Lab Guide shows the full staging address.
        - Collection → ＋ → Import… → OpenAPI URL → paste the staging address followed by /openapi.yaml → Import. A Rocket Pizza folder appears.
      check:
        saved:
          folder: { name: Rocket Pizza, openapi: { spec: "*" } }
      solution:
        - call:
            method: import.url
            params: { url: "{{staging}}/openapi.yaml", parent: "" }
    - text: |
        **Check the contract.** The import also made a **Rocket Pizza** environment whose `baseUrl` points at staging. Pick **Rocket Pizza** in the environment menu at the top of the window, open **List pizzas** and press **Send**. In the response, open **Tests**: **Matches the API spec** must pass.
      hints:
        - Imported requests use {{baseUrl}}, which only the Rocket Pizza environment defines.
        - The environment menu is at the top of the window and says Lab right now. Switch it to Rocket Pizza, then open Rocket Pizza → List pizzas in the collection.
        - Environment menu → Rocket Pizza; Collection → Rocket Pizza → List pizzas → Send; response → Tests shows "Matches the API spec" passed.
      check:
        send:
          status: 200
          tests: [{ name: "Matches the API spec*", passed: true }]
      solution:
        - call:
            method: env.setActive
            params: { id: Rocket Pizza }
        - call:
            method: http.send
            params:
              requestId: ship-it-list-pizzas
              path: Rocket Pizza/List pizzas.yaml
              request: &listPizzas
                name: List pizzas
                seq: 1
                method: GET
                url: "{{baseUrl}}/pizzas"
                openapi: { operation: GET /pizzas }
    - text: |
        **Smoke run.** Right-click the **Rocket Pizza** folder, choose **Run…** and press **Run**. Every request is sent once and every answer is checked against the contract. All green?
      hints:
        - A smoke run is a quick pass through every request to make sure nothing is on fire.
        - The run opens in a tab of its own; the Run button is at the top right.
        - Collection → right-click Rocket Pizza → Run… → Run. Three requests, three passed tests.
      check:
        run: { name: Rocket Pizza, passed: true, testsFailed: 0, testsPassed: ">=3" }
      solution:
        - call:
            method: runner.start
            params: { folder: Rocket Pizza }
        - wait: 1500
    - text: |
        **Launch traffic.** Right-click **List pizzas** → **Load test…**. Under **Stages** pick **Constant**, with **Peak (users)** **3** and **Duration (seconds)** **3**. Keep the thresholds (p95 latency under 500 ms, error rate under 1 %) and press **Start**. The run must pass.
      hints:
        - Keep it short and small; you're checking a goal, not trying to break staging.
        - The load test sends the saved List pizzas request, using the Rocket Pizza environment's baseUrl.
        - Right-click List pizzas → Load test… → Constant, Peak 3, Duration 3 → Start. Wait for "passed".
      check:
        all:
          - saved:
              loadTest:
                targets: [{ request: "*List pizzas.yaml" }]
                thresholds: [{ metric: p95 }]
          - load: { passed: true, requests: ">0" }
      solution:
        - call:
            method: load.create
            params:
              test: &launchTest
                name: List pizzas load test
                targets: [{ request: Rocket Pizza/List pizzas.yaml }]
                model: virtualUsers
                stages:
                  - { durationSecs: 0, target: 3 }
                  - { durationSecs: 3, target: 3 }
                thresholds:
                  - { metric: p95, op: "<", value: 500 }
                  - { metric: errorRate, op: "<", value: 1 }
        - call:
            method: load.start
            params: { id: List pizzas load test, test: *launchTest }
        - wait: 3500
    - text: |
        **A mock for the mobile team,** so they can build even when staging is down. In **Servers**, **＋** → **Mock from OpenAPI…** → **URL** `{{staging}}/openapi.yaml`, name it **Rocket Pizza mock**, press **Create mock**, then **Start** it. The lab orders from its `GET /pizzas`.
      hints:
        - A mock made from the contract answers with the contract's examples, so it can't drift from it.
        - Mock from OpenAPI… is at the bottom of the ＋ menu in Servers. After Create mock, the new server opens in a tab with a Start button.
        - Servers → ＋ → Mock from OpenAPI… → URL → staging address + /openapi.yaml → name Rocket Pizza mock → Create mock → Start.
      check:
        probe:
          kind: http
          path: /pizzas
          expect:
            status: 200
            json: [{ name: Margherita }, { name: Pepperoni Blast-off }]
      solution:
        - call:
            method: mock.fromOpenApi
            params: { url: "{{staging}}/openapi.yaml", name: Rocket Pizza mock }
        - call:
            method: server.save
            params:
              id: Rocket Pizza mock
              server: &rocketMock
                name: Rocket Pizza mock
                kind: http
                port: 0
                http:
                  routes:
                    - name: List pizzas
                      method: GET
                      path: /pizzas
                      status: 200
                      headers: [{ key: Content-Type, value: application/json }]
                      body: "[\n  {\n    \"id\": 1,\n    \"name\": \"Margherita\",\n    \"price\": 9.5,\n    \"vegetarian\": true\n  },\n  {\n    \"id\": 2,\n    \"name\": \"Pepperoni Blast-off\",\n    \"price\": 11,\n    \"vegetarian\": false\n  }\n]"
                    - name: Get a pizza
                      method: GET
                      path: /pizzas/:pizzaId
                      status: 200
                      headers: [{ key: Content-Type, value: application/json }]
                      body: "{\n  \"id\": 1,\n  \"name\": \"Margherita\",\n  \"price\": 9.5,\n  \"vegetarian\": true\n}"
                    - name: Place an order
                      method: POST
                      path: /orders
                      status: 201
                      headers: [{ key: Content-Type, value: application/json }]
                      body: "{\n  \"orderId\": 501,\n  \"status\": \"in the oven\",\n  \"etaMinutes\": 20\n}"
        - call:
            method: server.start
            params: { id: Rocket Pizza mock, server: *rocketMock }
    - text: |
        **Code for Android.** Open **List pizzas**, click **⋯** (More actions, next to Save) → **Copy as cURL or code…**, choose **Kotlin (OkHttp, Android)** and press **Copy**. That's the mobile team's first line of networking code, ready to paste.
      hints:
        - Zorvik writes the request as code in cURL, Kotlin, Swift, JavaScript or Python, with the variables filled in.
        - The ⋯ button sits right of the Save button in the request tab. The Format list has a Code group.
        - List pizzas → ⋯ → Copy as cURL or code… → Format Kotlin (OkHttp, Android) → Copy.
      check:
        call:
          method: export.snippet
          ok: true
          params: { language: kotlin, request: { url: "*pizzas*" } }
      solution:
        - call:
            method: export.snippet
            params:
              request: *listPizzas
              path: Rocket Pizza/List pizzas.yaml
              language: kotlin
              resolveVariables: true
    - text: |
        Contract imported, staging checked, smoke run green, load test passed, a mock and code handed over. Everything on the checklist is done.

        Type **ship it** to launch Rocket Pizza.
      hints:
        - You've earned this one.
        - Type the two words in the answer box of this step.
        - Type ship it and press Enter.
      check:
        answer: ["ship it", "ship it!", "shipit", "ship-it"]
      solution:
        - answer: ship it
quiz:
  - question: The "Matches the API spec" test fails on staging after a back-end update. What does that most likely mean?
    options:
      - The staging API no longer answers the way the contract promises
      - Zorvik is broken
      - The mobile app has a bug
    answer: 0
    explain: The test compares real answers with the contract. When it turns red after an update, the API drifted, and every app built on the contract could break.
  - question: Why did the load test in this mission use only 3 users for 3 seconds?
    options:
      - Because Zorvik can't run more users
      - Because thresholds only work with small tests
      - To check the performance goal without hammering a shared staging server
    answer: 2
    explain: A release check asks "do we meet the goal?", not "when does it break?". Find the breaking point separately, on a system set aside for it.
  - question: Next release, what's the fastest way to repeat all of these checks?
    options:
      - Start over from a blank workspace
      - Keep the workspace in Git and run the same collection, load test and mock again
      - Ask the back-end team whether anything changed
    answer: 1
    explain: Everything you built is plain files in the workspace. Next time, the whole checklist takes minutes.
---

This is it: your final mission. Everything from the Bootcamp comes together in one launch.

**The situation.** Rocket Pizza is launching its mobile app. The design team wrote the API **contract**, an OpenAPI document. The back-end team deployed a first version to a **staging** server. The Android team starts on Monday and is counting on you. Before anyone says "launch", you'll take the API through the whole release checklist.

```flow
Contract -[Import…]-> Collection
Collection -[Send + Run…]-> Staging
Collection -[Load test…]-> Staging
Contract -[Mock from OpenAPI…]-> Mock for mobile
Collection -[Copy as code…]-> Kotlin snippet
```

## Your mission, step by step

| Step | What you do | You learned it in… |
|---|---|---|
| 1 | Import the contract into your collection | OpenAPI and collections |
| 2 | Switch environment and pass **Matches the API spec** | environments and tests |
| 3 | Run the whole folder once: a smoke run | running collections |
| 4 | A short load test whose thresholds must pass | performance and load testing |
| 5 | A mock of the API for the mobile team | mocking APIs |
| 6 | Kotlin code for the Android developers | sharing requests as code |

Take your time. The hints are there if you need them, and nothing you do here can break anything real: the staging server, the mock and the load all stay on your own computer.

> [!note] Think of it like…
> A rocket launch countdown. Fuel: check. Guidance: check. Weather: check. Nobody launches on "probably fine"; every system says **go** first. Your checklist is the countdown, and the last step is yours to call.

## Why these checks, in this order

First you prove that staging keeps the contract's promises, one request, then all of them. Only then does it make sense to measure speed; a fast wrong answer is still wrong. Last comes the hand-off, so the mobile team gets a mock and code that match an API you've just verified.

> [!tip] Look around as you go
> After the smoke run, open the Tests of each result. After the load test, compare p50 with p95. Everything you notice now is something you'll notice at work.

## Graduation

When the last step is done and you've answered the questions below, you graduate: the **Graduate** badge, 250 bonus XP and your **Zorvik Bootcamp Graduate** certificate. You started with "what is a network?" and you're finishing with a full API release. That's a real skill set; be proud of it.

**You'll use this when…** you ship your first API at work. The names will change, but the checklist won't: contract, collection, checks, smoke run, load test, hand-off. Ship it!
