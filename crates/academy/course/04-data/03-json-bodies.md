---
id: json-bodies
title: JSON bodies
summary: Requests that create or change something carry data in their body, and most APIs want it as JSON with the right types.
minutes: 6
lab:
  title: Place two orders
  goal: Send JSON bodies with numbers, true/false, nested objects, and read what the server sends back.
  minutes: 7
  servers:
    api:
      name: Orders API
      kind: http
      http:
        routes:
          - name: Create order
            method: POST
            path: /orders
            status: 201
            headers:
              - { key: Content-Type, value: application/json }
              - { key: Location, value: "/orders/{{secret.order}}" }
            body: '{"orderId": "{{secret.order}}", "status": "received"}'
  steps:
    - text: |
        Send `POST {{api}}/orders` with a **JSON** body ordering two keyboards:

        ```json
        {"product": "keyboard", "quantity": 2}
        ```

        `quantity` must be a number, not text.
      hints:
        - Change the method to POST, open the **Body** tab and choose **JSON**.
        - Numbers in JSON have no quotes. `2` is a number; `"2"` is text.
        - "Method POST, URL {{api}}/orders, Body: JSON with {\"product\": \"keyboard\", \"quantity\": 2}, then Send."
      check:
        request:
          server: api
          method: POST
          path: /orders
          headers: { content-type: "application/json*" }
          json: { product: keyboard, quantity: 2 }
          body: 're:"quantity"\s*:\s*2\s*[,}]'
      solution:
        - send:
            method: POST
            url: "{{api}}/orders"
            body: { type: json, text: "{\"product\": \"keyboard\", \"quantity\": 2}" }
    - text: |
        Order one mouse as a gift, shipped to Lisbon. Add a true/false field and a nested object:

        ```json
        {
          "product": "mouse",
          "quantity": 1,
          "giftWrap": true,
          "shipTo": {"name": "Ada", "city": "Lisbon"}
        }
        ```
      hints:
        - "`true` and `false` are written without quotes, like numbers. An object inside an object is just another pair of braces."
        - Edit the body of the same request. **Beautify** (next to the body type) lays it out neatly and tells you if the JSON is broken.
        - "Body: {\"product\": \"mouse\", \"quantity\": 1, \"giftWrap\": true, \"shipTo\": {\"name\": \"Ada\", \"city\": \"Lisbon\"}}, then Send."
      check:
        request:
          server: api
          method: POST
          path: /orders
          json: { product: mouse, giftWrap: true, shipTo: { city: Lisbon } }
          body: 're:"giftWrap"\s*:\s*true'
      solution:
        - send:
            method: POST
            url: "{{api}}/orders"
            body:
              type: json
              text: "{\"product\": \"mouse\", \"quantity\": 1, \"giftWrap\": true, \"shipTo\": {\"name\": \"Ada\", \"city\": \"Lisbon\"}}"
    - text: The server answered `201 Created` and gave your order an id. Type the `orderId`.
      hints:
        - It's in the response body. The `Location` response header points to the new order too.
        - Copy the value after `"orderId":`, without the quotes.
      check:
        answer: "{{secret.order}}"
      solution:
        - answer: "{{secret.order}}"
quiz:
  - question: Which of these is valid JSON?
    options:
      - "{'name': 'Ada'}"
      - "{\"name\": \"Ada\",}"
      - "{\"name\": \"Ada\", \"admin\": false}"
    answer: 2
    explain: JSON needs double quotes around names and text, and no comma after the last item. `false` has no quotes.
  - question: "An API expects `\"quantity\": 2` but you send `\"quantity\": \"2\"`. What's the difference?"
    options:
      - You sent text instead of a number, which a strict API may reject with 400
      - "None: JSON reads `\"2\"` and `2` as the same value"
      - The second one is not valid JSON at all
    answer: 0
    explain: Both are valid JSON, but one is a number and the other a string. Many APIs check types and answer 400 Bad Request.
  - question: You choose the JSON body type in Zorvik. Which header does it add for you?
    options:
      - "Accept: application/json"
      - "Content-Type: application/json"
      - "Authorization: JSON"
    answer: 1
    explain: Content-Type describes the body you send. Unless you set that header yourself, Zorvik adds it to match the body type.
---

So far you have *read* data. To **create** or **change** something, a request carries data of its own in its **body**: the part that comes after the headers. `GET` requests normally have no body; `POST` (create), `PUT` (replace) and `PATCH` (change part of something) usually do.

```sequence
You -> Orders API: POST /orders {"product": "keyboard", "quantity": 2}
Note over Orders API: saves the order
Orders API --> You: 201 Created {"orderId": "…", "status": "received"}
```

## JSON in five rules

**JSON** (JavaScript Object Notation) is the format most APIs speak. It is plain text with a few strict rules:

1. An **object** is a set of `"name": value` pairs in curly braces, separated by commas.
2. Names and text (**strings**) use *double* quotes: `"Lisbon"`.
3. **Numbers** have no quotes: `2`, `4.99`, `-1`.
4. `true`, `false` and `null` (nothing) have no quotes either.
5. A **list** (array) goes in square brackets: `["api", "testing"]`. Objects and lists can sit inside each other.

```anatomy
{ | an object starts
"product": "mouse", | a name and a string value
"quantity": 1, | a number: no quotes
"giftWrap": true, | true/false: no quotes
"shipTo": {"city": "Lisbon"} | an object inside the object
} | no comma after the last pair
```

> [!note] Think of it like…
> A form you fill in for the server, with strict handwriting rules. The server's code reads it field by field; if a box holds the wrong *kind* of thing (the word "two" where a number should be), it gives the form back.

## Types matter

`"quantity": 2` and `"quantity": "2"` are both valid JSON, but the first is a number and the second is text. A strict API rejects the wrong one with `400 Bad Request`, and a sloppy one might store it and break something later. The same goes for `true` versus `"true"`.

## The most common mistakes

| Mistake | Wrong | Right |
|---|---|---|
| Single quotes | `{'city': 'Lisbon'}` | `{"city": "Lisbon"}` |
| Comma after the last item | `{"a": 1,}` | `{"a": 1}` |
| Comments | `{"a": 1 // one}` | JSON has no comments |
| Unquoted names | `{city: "Lisbon"}` | `{"city": "Lisbon"}` |

## JSON bodies in Zorvik

In the **Body** tab choose **JSON** and type. The editor colors the syntax, and **Beautify** lays the body out neatly or tells you it isn't valid JSON. You can use variables in a body too: `{"email": "{{email}}"}`.

When the body type is JSON, Zorvik sends a `Content-Type: application/json` header unless you set one yourself. That header tells the server how to read the body; you'll look at it closely two lessons from now.

> [!tip] Read the answer too
> A successful create is often `201 Created`, with the new item's id in the body and its address in a `Location` header. Tests you write later will check exactly those.

**You'll use this when…** you create a customer, place a test order or update a setting through an API. Most "it doesn't work" moments come down to a missing field or a number sent as text.
