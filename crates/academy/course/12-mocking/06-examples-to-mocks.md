---
id: examples-to-mocks
title: Saved examples become mocks
summary: Keep real responses as examples of a request, and a mock built from the folder answers with them, even picking the right one by query.
minutes: 6
added: 0.2.0
lab:
  title: A pet shop that never sleeps
  goal: Save two real answers as examples, turn the folder into a mock, and see the mock pick the right example for each request.
  minutes: 9
  files:
    requests/Pet shop/_folder.yaml: "name: Pet shop\n"
  servers:
    api:
      name: Pet shop API
      kind: http
      http:
        routes:
          - name: Sold pets
            method: GET
            path: /pets
            matchQuery:
              - { key: status, value: sold }
            headers:
              - { key: Content-Type, value: application/json }
            body: &soldPets '[{"id": 3, "name": "Biscuit", "status": "sold"}]'
          - name: All pets
            method: GET
            path: /pets
            headers:
              - { key: Content-Type, value: application/json }
            body: &allPets '[{"id": 1, "name": "Rex", "status": "available"}, {"id": 2, "name": "Luna", "status": "available"}, {"id": 3, "name": "Biscuit", "status": "sold"}]'
  steps:
    - text: |
        The pet shop's test server goes offline every night, so you'll keep its answers. Right-click the **Pet shop** folder in your collection, choose **New HTTP request** and name it **List pets**. Set the URL to `{{api}}/pets`, send it, then press **Save as example** above the response and save the request with **⌘/Ctrl + S**.
      hints:
        - An example is a response kept inside the request's file. It needs a saved request to live in, which is why the request goes into the folder first.
        - "Collection → right-click Pet shop → New HTTP request → List pets → Create. URL {{api}}/pets, then Send."
        - "Save as example is on the status line above the response, next to Mock (a bookmark icon when the pane is narrow). The URL change isn't saved yet, so then press ⌘/Ctrl + S: the Examples tab shows 200 OK."
      check:
        saved:
          request: { path: "Pet shop/*", examples: [{ status: 200, body: "*Rex*" }] }
      solution:
        - send: { method: GET, url: "{{api}}/pets" }
        - call:
            method: request.create
            params:
              parent: Pet shop
              request:
                name: List pets
                seq: 0
                method: GET
                url: "{{api}}/pets"
                examples:
                  - &allExample
                    name: 200 OK
                    status: 200
                    headers: [{ key: Content-Type, value: application/json }]
                    body: *allPets
                    url: "{{api}}/pets"
    - text: |
        The shop's app also asks for sold pets only. Add `?status=sold` to the URL, send it, press **Save as example** again and save with **⌘/Ctrl + S**. The **Examples** tab now lists two.
      hints:
        - Each example remembers the URL it was saved from, query included. That's how a mock knows when to answer with it.
        - "Type ?status=sold at the end of the URL (or add status = sold in the Params tab), then Send. Only Biscuit comes back."
        - "Press Save as example, then ⌘/Ctrl + S. The Examples tab shows 200 OK and 200 OK (2); pick one to see its body."
      check:
        saved:
          request: { path: "Pet shop/*", examples: [{ url: "*status=sold*", body: "*Biscuit*" }] }
      solution:
        - send: { method: GET, url: "{{api}}/pets?status=sold" }
        - call:
            method: request.save
            params:
              path: Pet shop/List pets.yaml
              request:
                name: List pets
                seq: 0
                method: GET
                url: "{{api}}/pets?status=sold"
                examples:
                  - *allExample
                  - name: 200 OK (2)
                    status: 200
                    headers: [{ key: Content-Type, value: application/json }]
                    body: *soldPets
                    url: "{{api}}/pets?status=sold"
    - text: |
        Turn the folder into a mock. Right-click **Pet shop**, choose **Mock this folder…** and press **Create mock**. The request's two examples become two routes.
      hints:
        - A mock from a folder answers with the requests' examples when they have some, and with {} when they don't.
        - Mock this folder… is in the folder's right-click menu, below Run…. The suggested name, Pet shop mock, is fine.
        - "Collection → right-click Pet shop → Mock this folder… → Create mock. The new mock opens with the routes List pets · 200 OK (2) and List pets · 200 OK."
      check:
        call: { method: mock.fromFolder, ok: true, params: { folder: Pet shop }, result: { routes: 2 } }
      solution:
        - call:
            method: mock.fromFolder
            params: { folder: Pet shop, name: Pet shop mock }
    - text: |
        Press **Start** on **Pet shop mock**. The lab then asks it for `GET /pets?status=sold`, just like the shop's app would, and expects Biscuit alone.
      hints:
        - The route saved with ?status=sold only answers requests that carry status=sold. Everything else gets the other example.
        - Press Start at the top right of the Pet shop mock tab. If the port is in use, change Port and press Start again.
        - "Open Pet shop mock under Servers and press Start. GET /pets?status=sold then answers with Biscuit only, and GET /pets with all three pets."
      check:
        probe:
          kind: http
          name: Pet shop mock
          path: /pets?status=sold
          expect:
            status: 200
            json: [{ name: Biscuit }]
            body: "!*Rex*"
      solution:
        - call:
            method: server.save
            params:
              id: Pet shop mock
              server: &petMock
                name: Pet shop mock
                kind: http
                port: 0
                http:
                  routes:
                    - name: List pets · 200 OK (2)
                      method: GET
                      path: /pets
                      status: 200
                      headers: [{ key: Content-Type, value: application/json }]
                      body: *soldPets
                      matchQuery: [{ key: status, value: sold }]
                    - name: List pets · 200 OK
                      method: GET
                      path: /pets
                      status: 200
                      headers: [{ key: Content-Type, value: application/json }]
                      body: *allPets
        - call:
            method: server.start
            params: { id: Pet shop mock, server: *petMock }
quiz:
  - question: What does **Save as example** keep?
    options:
      - Only the status code
      - The status, the headers and the body of that response, in the request's file
      - A link to the response in History
    answer: 1
    explain: The example is named after the status (200 OK, 200 OK (2)…) and lives in the request's file, so it's shared through Git like the request itself.
  - question: A request has two examples, saved from `/pets` and from `/pets?status=sold`. The mock built from its folder gets `GET /pets`. What answers?
    options:
      - The example saved without a query
      - The example saved with status=sold
      - Nothing, the mock answers 404
    answer: 0
    explain: Examples saved with a query only answer requests that carry it. The first example saved without a query answers everything else.
  - question: Why does Save as example leave out `Set-Cookie` headers?
    options:
      - Mocks can't send cookies
      - Cookies are too large to keep
      - Cookies can hold a login session, and examples are saved in the workspace and end up in Git
    answer: 2
    explain: Examples are files your team shares. Framing headers and the Date are left out too, because the mock sets its own.
---

You send a request and the answer is exactly right: the data the app needs, in the right shape. Tomorrow the test server is down, or the data has changed. Keep that answer: press **Save as example** above the response.

An **example** is a response kept inside the request's file. It documents what the request answers, for you and your team, and it's what a mock answers with.

```flow
Send -> Response -[Save as example]-> The request's Examples tab
Folder of requests -[Mock this folder…]-> A mock that answers with the examples
```

## What an example keeps

- The **status**, which is also its name: `200 OK`, then `200 OK (2)` for the next one.
- The response **headers**, without the framing ones (`Content-Length` and friends), the `Date` and `Set-Cookie`: cookies can hold a login session, and examples end up in Git.
- The **body**, up to 1 MB of text. Binary answers can't be examples; use **Save to file** for those.
- The **URL** it was sent to, as you typed it, variables and query included.

A saved request with no other changes is saved at once. If you changed something, such as the URL, the example waits with your other changes until you save.

The request's **Examples** tab lists them. Pick one to see its headers and body, rename it, or edit its status and body: turn a copy into a `404` with an error body, and your mock can answer failures too.

```yaml
examples:
  - name: 200 OK
    status: 200
    headers:
      - { key: Content-Type, value: application/json }
    body: '[{"id": 1, "name": "Rex"}]'
    url: "{{baseUrl}}/pets"
```

## From examples to a mock

Right-click a folder and choose **Mock this folder…**. A request without examples becomes a route answering `200` with `{}`. A request **with** examples becomes one route per example:

| Route | Match query | Answers |
|---|---|---|
| List pets · 200 OK (2) | `status=sold` | the example saved from `/pets?status=sold` |
| List pets · 200 OK | | every other request to `/pets` |

Examples saved with a query go first and only answer requests that carry it. Then the first example saved without a query answers everything else.

```sequence
participants: Shop app, Pet shop mock
Shop app -> Pet shop mock: GET /pets?status=sold
Note over Pet shop mock: the status=sold route matches
Pet shop mock --> Shop app: 200 [Biscuit]
Shop app -> Pet shop mock: GET /pets
Pet shop mock --> Shop app: 200 [Rex, Luna, Biscuit]
```

> [!note] Think of it like…
> The plastic dishes in a restaurant window. Each one was made from the real meal, so it shows exactly what you'll get, and it never goes cold. Examples are made from real answers, and the mock serves them any time, even when the kitchen is closed.

> [!tip] Example or Mock button?
> **Mock**, next to **Save as example**, adds one route to a mock server right away. An example stays with the request, so every mock you build from its folder later answers with it, and your teammates see it in the file.

**You'll use this when…** the test server is flaky, the back end changes under you, or a new teammate needs to see what an endpoint returns. Save the good answers once, and a realistic mock is one right-click away.
