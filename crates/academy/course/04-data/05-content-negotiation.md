---
id: content-negotiation
title: Content-Type and Accept
summary: "`Content-Type` says what format your body is in; `Accept` says which formats you'd like back."
minutes: 5
lab:
  title: Speak the server's format
  goal: Ask for CSV with Accept, get a 415 for a mislabeled upload, then fix its Content-Type.
  minutes: 6
  servers:
    api:
      name: Sales API
      kind: http
      http:
        routes:
          - name: CSV report
            method: GET
            path: /report
            matchHeaders:
              - { key: Accept, value: text/csv }
            headers:
              - { key: Content-Type, value: text/csv }
            body: "month,sales\nJuly,120\nAugust,135\n"
          - name: JSON report
            method: GET
            path: /report
            headers:
              - { key: Content-Type, value: application/json }
            body: '{"report": [{"month": "July", "sales": 120}, {"month": "August", "sales": 135}]}'
          - name: Import CSV
            method: POST
            path: /import
            status: 202
            matchHeaders:
              - { key: Content-Type, value: text/csv }
            headers:
              - { key: Content-Type, value: application/json }
            body: '{"accepted": true, "message": "Import started."}'
          - name: Import CSV (charset)
            method: POST
            path: /import
            status: 202
            matchHeaders:
              - { key: Content-Type, value: text/csv; charset=utf-8 }
            headers:
              - { key: Content-Type, value: application/json }
            body: '{"accepted": true, "message": "Import started."}'
          - name: Unsupported type
            method: POST
            path: /import
            status: 415
            headers:
              - { key: Content-Type, value: application/json }
            body: '{"error": "unsupported_media_type", "message": "This endpoint only reads CSV. Send it with Content-Type: text/csv."}'
  steps:
    - text: |
        Send `GET {{api}}/report`: you get JSON. Now add the header `Accept: text/csv` and send again to get the same report as CSV.
      hints:
        - Headers go in the **Headers** tab, one per row.
        - "Add a row with the name Accept and the value text/csv. Zorvik then leaves out its default Accept: */*."
        - "Headers tab: Accept = text/csv. URL {{api}}/report, method GET, then Send."
      check:
        request: { server: api, method: GET, path: /report, headers: { accept: text/csv }, route: CSV report }
      solution:
        - send: { method: GET, url: "{{api}}/report" }
        - send: { method: GET, url: "{{api}}/report", headers: [{ key: Accept, value: text/csv }] }
    - text: |
        Upload two rows of CSV to `POST {{api}}/import`. Use the **Text** body type and leave its content type at `text/plain` for now:

        ```text
        month,sales
        September,142
        ```

        The server refuses it. Look at the status.
      hints:
        - Remove the Accept header from the last step if it's still there; it doesn't hurt, but it isn't needed.
        - Method POST, **Body** tab, choose **Text**. The list next to it is the content type; keep `text/plain`.
        - "Body type Text (text/plain) with the two CSV lines, URL {{api}}/import, then Send. You should get 415."
      check:
        request: { server: api, method: POST, path: /import, status: 415 }
      solution:
        - send:
            method: POST
            url: "{{api}}/import"
            body: { type: text, text: "month,sales\nSeptember,142\n" }
    - text: Fix it. Label the body as CSV (`text/csv`) and send again, until the server answers `202 Accepted`.
      hints:
        - A 415 means "I don't read that format". Your data was fine; its label wasn't.
        - The content type list next to the **Text** body type has `text/csv`.
        - "Body type Text, content type text/csv, same two lines, then Send."
      check:
        request: { server: api, method: POST, path: /import, status: 202, headers: { content-type: "text/csv*" } }
      solution:
        - send:
            method: POST
            url: "{{api}}/import"
            body: { type: text, contentType: text/csv, text: "month,sales\nSeptember,142\n" }
quiz:
  - question: Which header describes the body of the request you are sending?
    options:
      - Accept
      - Content-Type
      - Content-Length
    answer: 1
    explain: Content-Type labels the body. Accept is a wish list for the response.
  - question: "You send JSON, but with `Content-Type: text/plain`. What might the server answer?"
    options:
      - 415 Unsupported Media Type
      - 201 Created, labels don't matter
      - 301 Moved Permanently
    answer: 0
    explain: Many servers pick a parser by Content-Type. With the wrong label they can't, or won't, read the body.
  - question: "Your request says `Accept: application/json`, but the server can only produce XML. What's the polite answer?"
    options:
      - 404 Not Found
      - 500 Internal Server Error
      - 406 Not Acceptable, or XML anyway
    answer: 2
    explain: Accept is a preference. A strict server answers 406; many just send what they have, so always check the response's Content-Type.
---

A body is just bytes. The same bytes could be JSON, CSV, an image or a zip file, and the server can't always tell by looking. Two headers settle it, one for each direction:

- **`Content-Type`** labels the body of *this* message: "what I'm sending you is JSON".
- **`Accept`** is a request header that says which formats you'd like *back*: "I can read CSV, please".

Asking for a format and getting one is called **content negotiation**.

```sequence
You -> Sales API: GET /report (Accept: text/csv)
Note over Sales API: can I make CSV? yes
Sales API --> You: 200 OK (Content-Type: text/csv)
You -> Sales API: POST /import (Content-Type: text/plain)
Sales API --> You: 415 Unsupported Media Type
```

## Media types

The values are **media types** (also called MIME types): a family and a format, separated by a slash.

| Media type | What it is |
|---|---|
| `application/json` | JSON |
| `application/x-www-form-urlencoded` | a simple form (last lesson) |
| `multipart/form-data` | a form with files |
| `text/plain` | plain text |
| `text/csv` | comma-separated values, like a spreadsheet |
| `image/png` | a PNG picture |

A media type can carry parameters after a `;`, such as `text/csv; charset=utf-8` (the character encoding) or the multipart `boundary`.

> [!note] Think of it like…
> Labels on parcels. `Content-Type` is the label on the parcel you hand over ("fragile: glass"). `Accept` is the note you leave for deliveries ("letters or small parcels only, please"). A parcel with the wrong label may get sent back.

## When labels go wrong

| Status | Meaning |
|---|---|
| `415 Unsupported Media Type` | "I can't read the format your body is labeled as." Check your Content-Type. |
| `406 Not Acceptable` | "I can't produce any format your Accept allows." Loosen your Accept. |

A `415` is a good example of an error message pointing at the fix: the data can be perfect and still be refused because its label is wrong.

## In Zorvik

- Picking a body type sets `Content-Type` for you: JSON, XML, forms and multipart each get the right one. The **Text** body type has a list to choose the content type, from `text/plain` to `text/csv`.
- A `Content-Type` you add yourself in the **Headers** tab wins over the automatic one.
- Zorvik sends `Accept: */*` ("anything is fine") by default. Add your own `Accept` header and the default is left out. You can switch default headers off in **Settings → Requests**.
- The response's `Content-Type` decides how Zorvik shows the body: JSON is formatted and images get a preview.

> [!tip] Check what came back
> Don't assume the server honored your Accept. Look at the response's `Content-Type` in its **Headers** tab.

**You'll use this when…** an endpoint answers 415 to a body that looks right, an export endpoint can return CSV or JSON, or a client library sends the wrong label and you need to prove it.
