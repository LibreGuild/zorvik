---
id: fake-data
title: Fake data on every send
summary: Built-in dynamic variables such as {{$randomEmail}} and {{$isoDate(+7d)}} make fresh, realistic test data every time you send.
minutes: 6
added: 0.2.0
lab:
  title: A crowd of test users
  goal: Sign up users and place orders with fresh fake data on every send, shape values with arguments, and use one value in two places.
  minutes: 8
  servers:
    api:
      name: Sign-up API
      kind: http
      http:
        routes:
          - name: Sign up
            method: POST
            path: /users
            status: 201
            headers:
              - { key: Content-Type, value: application/json }
            body: '{"id": "{{$uuid}}", "status": "created", "received": {{request.body}}}'
          - name: Place order
            method: POST
            path: /orders
            status: 201
            headers:
              - { key: Content-Type, value: application/json }
            body: '{"status": "received", "order": {{request.body}}}'
          - name: Update order
            method: PUT
            path: /orders/:id
            headers:
              - { key: Content-Type, value: application/json }
            body: '{"id": "{{request.params.id}}", "status": "updated", "received": {{request.body}}}'
  steps:
    - text: |
        Sign up a made-up person. Send `POST {{api}}/users` with this **JSON** body:

        ```json
        {"name": "{{$randomFullName}}", "email": "{{$randomEmail}}"}
        ```

        Type `{{$` inside the quotes and pick from the suggestions, or paste it. Send it twice: every answer shows someone new.
      hints:
        - Names that start with `$` are dynamic variables. You don't define them anywhere; Zorvik makes the value as the request goes out.
        - "Method POST, **Body** tab, type **JSON**. Inside the quotes, type `{{$rand` and the list shows $randomFullName, $randomEmail and many more, each with an example value."
        - "POST {{api}}/users, JSON body {\"name\": \"{{$randomFullName}}\", \"email\": \"{{$randomEmail}}\"}, then Send. The answer's received part shows the name and email that went out."
      check:
        all:
          - request:
              server: api
              method: POST
              path: /users
              json:
                name: "re:^[^{}]+ [^{}]+$"
                email: "re:^[^@{}\\s]+@example\\.(com|net|org)$"
          - send: { url: "*/users", request: { body: { text: "*{{$random*" } } }
      solution:
        - send:
            method: POST
            url: "{{api}}/users"
            body: { type: json, text: "{\"name\": \"{{$randomFullName}}\", \"email\": \"{{$randomEmail}}\"}" }
    - text: |
        Arguments shape a value. Order a random number of T-shirts, from 1 to 5, in a random size, delivered a week from today. Send `POST {{api}}/orders` with:

        ```json
        {
          "quantity": {{$randomInt(1, 5)}},
          "size": "{{$randomFrom(S, M, L)}}",
          "deliverBy": "{{$isoDate(+7d)}}"
        }
        ```
      hints:
        - Arguments go in parentheses after the name. `$randomInt(1, 5)` is a whole number from 1 to 5, both included; `+7d` moves today's date seven days on.
        - "`quantity` has no quotes, so the server gets a number. `size` and `deliverBy` are text, so they keep theirs."
        - "POST {{api}}/orders, Body JSON as shown, then Send. The answer echoes a quantity from 1 to 5, a size S, M or L, and a date like 2026-10-05."
      check:
        all:
          - request:
              server: api
              method: POST
              path: /orders
              json:
                quantity: "re:^[1-5]$"
                size: "re:^[SML]$"
                deliverBy: "re:^\\d{4}-\\d{2}-\\d{2}$"
          - send: { url: "*/orders", request: { body: { text: "*{{$isoDate(*" } } }
      solution:
        - send:
            method: POST
            url: "{{api}}/orders"
            body:
              type: json
              text: "{\n  \"quantity\": {{$randomInt(1, 5)}},\n  \"size\": \"{{$randomFrom(S, M, L)}}\",\n  \"deliverBy\": \"{{$isoDate(+7d)}}\"\n}"
    - text: |
        Every `{{$uuid}}` is a new value, even twice in one request. To use one id in two places, make it once in a **Pre-request** script:

        ```js
        pm.variables.set("orderId", pm.variables.replaceIn("{{$uuid}}"));
        ```

        Then send `PUT {{api}}/orders/{{orderId}}` with the JSON body `{"id": "{{orderId}}", "status": "paid"}`. The id in the address and the one in the body are the same.
      hints:
        - "`pm.variables.replaceIn` fills in the variables of a text, dynamic ones included. `pm.variables.set` keeps the result for this one send."
        - "Method PUT, URL {{api}}/orders/{{orderId}}. Scripts tab → Pre-request: paste the line. Body tab → JSON."
        - "PUT {{api}}/orders/{{orderId}}, the pre-request line above, body {\"id\": \"{{orderId}}\", \"status\": \"paid\"}, then Send. The answer shows one id twice."
      check:
        all:
          - request:
              server: api
              method: PUT
              path: "re:^/orders/[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$"
              json: { id: "re:^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$", status: paid }
          - send: { method: PUT, request: { url: "*orderId*", scripts: { preRequest: "*replaceIn*" } } }
      solution:
        - send:
            method: PUT
            url: "{{api}}/orders/{{orderId}}"
            body: { type: json, text: "{\"id\": \"{{orderId}}\", \"status\": \"paid\"}" }
            scripts:
              preRequest: pm.variables.set("orderId", pm.variables.replaceIn("{{$uuid}}"));
quiz:
  - question: A request has `{{$uuid}}` in its URL and again in a header. What do they get when you send?
    options:
      - The same UUID, made once per send
      - Two different UUIDs
      - The text `{{$uuid}}`, because it isn't defined in any environment
    answer: 1
    explain: Every use gets a new value. To share one, make it once in a pre-request script with pm.variables.replaceIn and use an ordinary variable.
  - question: You need a whole number from 10 to 20. Which one gives it?
    options:
      - "`{{$randomInt(10, 20)}}`"
      - "`{{$randomFrom(10, 20)}}`"
      - "`{{$randomInt}}`"
    answer: 0
    explain: "$randomInt takes a minimum and a maximum, both included. $randomFrom picks one of the values you list, so only 10 or 20; $randomInt alone goes from 0 to 1000."
  - question: Why do addresses from `{{$randomEmail}}` end in example.com, example.net or example.org?
    options:
      - Zorvik owns those domains and reads the mail
      - They are the only domains APIs accept
      - Those domains are reserved for examples, so no real person ever gets your test mail
    answer: 2
    explain: The example domains belong to no one's inbox. A sign-up test that sends a welcome email can't spam a stranger.
---

Try testing a sign-up API with the same `ada@example.com` twice and the second attempt fails: *email already taken*. Typing a new name and email before every send gets old fast, and made-up values like `test123` don't look like real data, so they hide bugs that real data would find.

**Dynamic variables** fix this. They look like normal variables with a `$` in front, and Zorvik makes a fresh value every time you send.

```sequence
participants: You, Zorvik, API
You -> Zorvik: Send {"email": "{{$randomEmail}}"}
Note over Zorvik: makes a new value
Zorvik -> API: {"email": "lottie.smith24@example.org"}
API --> Zorvik: 201 Created
```

## 170 of them, built in

You don't define dynamic variables anywhere. There are 170: ids, dates, names, addresses, prices, card numbers and more. A few of them:

| Variable | Makes |
|---|---|
| `{{$uuid}}` | a unique id, such as `3f1c9b0e-7d2a-4c8e-9a51-2b6f0d4e8c17` |
| `{{$timestamp}}` | the time in seconds since 1970, such as `1790588467` |
| `{{$randomFullName}}` | a first and last name, such as `Priya Fernández` |
| `{{$randomEmail}}` | an email address, such as `pablo.garcia62@example.com` |
| `{{$randomCity}}` | a city, such as `Lisbon` |
| `{{$randomPrice(5, 50)}}` | a price from 5 to 50, such as `24.99` |

You don't have to remember them: type `{{$` in any field, the URL, a header or a body, and a list of suggestions opens with an example value next to each. Hover one in the list to see what it makes. They work wherever variables work, even in a mock server's answers.

## Arguments shape the value

Many take **arguments** in parentheses:

```anatomy
{{$randomInt(1, 100)}} | a whole number from 1 to 100, both included
{{$randomFrom(S, M, L)}} | one of the values you list
{{$isoDate(+7d)}} | the date a week from today (s, m, h, d, w)
{{$timestamp(-1h)}} | the time an hour ago
```

Without arguments they use sensible defaults: `{{$randomInt}}` goes from 0 to 1000. A wrong argument, such as a minimum above the maximum, leaves the variable as written, and the response warns about *undefined values*.

> [!note] Think of it like…
> A film studio's props department. It hands out passports, bank notes and letters that look real on camera, but none of them works in a real shop. Dynamic variables are props for your API: realistic enough to test with, useless for anything else.

## Safe by design

The values are for testing only. Emails use the reserved `example.com`, `example.net` and `example.org` domains, so no real inbox ever gets your test mail. Card numbers come from the card brands' test ranges. IBANs and ISBNs have valid check digits but belong to no one.

## One value, several places

Every use gets a new value. Two `{{$uuid}}` in one request are two different ids. When the URL and the body must carry the *same* id, make it once in a **pre-request script** and use an ordinary variable:

```js
pm.variables.set("orderId", pm.variables.replaceIn("{{$uuid}}"));
```

Then write `{{orderId}}` everywhere. `pm.variables` values last for one send (or one collection run), so the next send gets a new id.

> [!tip] Pin a value while debugging
> A variable you define wins over a dynamic one with the same name. Add `$timestamp` to an environment and every request uses that fixed time until you remove it.

**You'll use this when…** a test creates users, orders or bookings. Each run gets fresh names, emails and ids, no "already exists" errors, and data that looks like what real users send.
