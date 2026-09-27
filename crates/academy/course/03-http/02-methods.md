---
id: http-methods
title: "Methods: GET, POST, PUT, PATCH, DELETE"
summary: The method says what to do with a thing; "safe" and "idempotent" tell you what's harmless to repeat.
minutes: 6
lab:
  title: A note's life
  goal: Create a note, change part of it and delete it, using the right method for each.
  minutes: 7
  servers:
    api:
      name: Notes API
      kind: http
      http:
        routes:
          - method: GET
            path: /notes
            headers:
              - key: Content-Type
                value: application/json
            body: '[{"id": 1, "text": "Water the plants", "done": true}, {"id": 2, "text": "Call the bakery", "done": false}]'
          - method: POST
            path: /notes
            status: 201
            headers:
              - key: Content-Type
                value: application/json
              - key: Location
                value: /notes/3
            body: '{"id": 3, "created": {{request.body}}}'
          - method: PUT
            path: /notes/:id
            headers:
              - key: Content-Type
                value: application/json
            body: '{"id": "{{request.params.id}}", "replacedWith": {{request.body}}}'
          - method: PATCH
            path: /notes/:id
            headers:
              - key: Content-Type
                value: application/json
            body: '{"id": "{{request.params.id}}", "changed": {{request.body}}}'
          - method: DELETE
            path: /notes/:id
            status: 204
  steps:
    - text: |
        Create a note: send `POST {{api}}/notes` with a JSON body such as `{"text": "Buy milk"}`. The server answers **201 Created**.
      hints:
        - Pick POST in the method list left of the URL, then open the request's Body tab.
        - In the Body tab choose JSON and type the note, with a "text" field.
        - 'Method POST, URL `{{api}}/notes`, Body → JSON: {"text": "Buy milk"}, then Send.'
      check:
        request: { server: api, method: POST, path: /notes, json: { text: "*" } }
      solution:
        - send: { method: POST, url: "{{api}}/notes", body: { type: json, text: '{"text": "Buy milk"}' } }
    - text: |
        The new note got the id 3. Mark it done without touching its text: send `PATCH {{api}}/notes/3` with the body `{"done": true}`.
      hints:
        - PATCH changes only the fields you send. PUT would replace the whole note.
        - Switch the method to PATCH, change the URL to end in /notes/3, and replace the body.
        - 'Method PATCH, URL `{{api}}/notes/3`, Body → JSON: {"done": true}, then Send.'
      check:
        request: { server: api, method: PATCH, path: /notes/3, json: { done: true } }
      solution:
        - send: { method: PATCH, url: "{{api}}/notes/3", body: { type: json, text: '{"done": true}' } }
    - text: Delete the note with `DELETE {{api}}/notes/3`. The answer is **204 No Content**, which means "done, nothing more to say".
      hints:
        - DELETE needs no body. The URL says which note.
        - "Method DELETE, URL `{{api}}/notes/3`, then Send."
      check:
        request: { server: api, method: DELETE, path: /notes/3 }
      solution:
        - send: { method: DELETE, url: "{{api}}/notes/3" }
    - text: DELETE is idempotent, so sending it again is harmless. Send the same DELETE **once more**.
      hints:
        - Just press Send again on the same request.
        - ⌘↩ (Ctrl+Enter on Windows) sends the current tab.
      check:
        request: { server: api, method: DELETE, path: /notes/3, count: 2 }
      solution:
        - send: { method: DELETE, url: "{{api}}/notes/3" }
quiz:
  - question: You want to change only a note's title and leave the rest as it is. Which method fits best?
    options:
      - PUT
      - PATCH
      - POST
      - GET
    answer: 1
    explain: PATCH sends just the changes. PUT replaces the whole thing, so you'd have to send every field.
  - question: Which method is "safe", meaning it never changes anything on the server?
    options:
      - GET
      - POST
      - DELETE
      - PATCH
    answer: 0
    explain: GET only reads. Browsers, caches and search engines send GETs freely, so a GET must never change data.
  - question: A "create order" POST times out. Why is simply retrying it risky?
    options:
      - The retry is sent as a GET
      - Servers block every retry
      - A POST can only be sent once per URL
      - The first one may have worked, so the retry could create a second order
    answer: 3
    explain: POST isn't idempotent. Unless the API offers a way to spot duplicates (often an Idempotency-Key header), two POSTs make two orders.
---

The first word of every request is the **method**: the verb. The URL says *which* thing, and the method says *what to do* with it. Most APIs follow a style called **REST**, where the same URLs are used with different methods:

| Method | Means | Example | Body |
|---|---|---|---|
| **GET** | read it | `GET /notes/3` | none |
| **POST** | create something new, or "do this" | `POST /notes` | the new thing |
| **PUT** | replace it completely | `PUT /notes/3` | the whole new version |
| **PATCH** | change part of it | `PATCH /notes/3` | just the changes |
| **DELETE** | remove it | `DELETE /notes/3` | usually none |

Here's a note's whole life:

```sequence
participants: You, Notes API
You -> Notes API: POST /notes with a new note
Notes API --> You: 201 Created, the note has id 3
You -> Notes API: PATCH /notes/3 with done = true
Notes API --> You: 200 OK
You -> Notes API: DELETE /notes/3
Notes API --> You: 204 No Content
```

## Safe and idempotent

Two words you'll hear in every API design review:

- **Safe** means the request changes nothing on the server. GET is safe (so are HEAD and OPTIONS, two less common methods). Browsers, caches and search engines send GETs all the time, so a GET must never delete or buy anything.
- **Idempotent** means that sending it twice has the same effect as sending it once. PUT and DELETE are: replacing a note with the same content twice leaves the same note, and deleting it twice leaves it deleted. (The second DELETE may answer 404, but nothing more changes.) POST is **not**: two POSTs usually create two notes. PATCH depends on the change: "set done to true" is idempotent, "add 1 to the likes" isn't.

| Method | Safe | Idempotent |
|---|---|---|
| GET | yes | yes |
| PUT | no | yes |
| DELETE | no | yes |
| PATCH | no | not always |
| POST | no | no |

Why care? Networks fail. When a request times out, you don't know whether the server got it. An idempotent request can simply be sent again. A POST can't, unless the API gives you a way to avoid duplicates, often an `Idempotency-Key` header. That's how payment APIs avoid charging a card twice.

> [!note] Think of it like…
> A lamp. "Turn it on" (PUT) is idempotent: press it five times and the lamp is on. "Toggle it" (a POST-style action) isn't: five presses leave it on or off depending on where you started. And just looking at the lamp (GET) is safe.

## In Zorvik

Pick the method in the list left of the URL. To send a body, open the request's **Body** tab and choose **JSON**: Zorvik adds the `Content-Type: application/json` header for you.

> [!tip] Don't trust a method name alone
> Some APIs use POST for everything, or change data on a GET. When you test an API, check that GETs really change nothing and that retried PUTs and DELETEs do no harm.

**You'll use this when…** you design or test an API: checking that reading never changes data, that retrying a PUT is harmless, and that a double-clicked "Pay" button doesn't create two orders.
