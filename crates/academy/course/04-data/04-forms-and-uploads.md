---
id: forms-and-uploads
title: Forms and file uploads
summary: Web forms send `key=value` pairs; files travel in multipart bodies, split into parts by a boundary.
minutes: 6
lab:
  title: Contact us, then upload a receipt
  goal: Send a URL-encoded form and a multipart upload with a file, and see how each arrives at the server.
  minutes: 7
  files:
    uploads/receipt.txt: |
      Coffee Corner, Lisbon
      Receipt {{secret.receipt}}
      2 x espresso beans 500 g ... 18.40 EUR
      Paid by card
  servers:
    api:
      name: Help Desk API
      kind: http
      http:
        routes:
          - name: Contact form
            method: POST
            path: /contact
            headers:
              - { key: Content-Type, value: application/json }
            body: '{"ok": true, "message": "Thanks, we will be in touch."}'
          - name: Upload
            method: POST
            path: /upload
            status: 201
            headers:
              - { key: Content-Type, value: application/json }
            body: '{"ok": true, "stored": true}'
  steps:
    - text: |
        Send `POST {{api}}/contact` with a **Form URL-encoded** body of two fields: `name` = `Ada` and `message` = `Hello there`.
      hints:
        - In the **Body** tab, the body type list has **Form URL-encoded**. It gives you a key/value table.
        - Method POST, body type **Form URL-encoded**, then one row per field. Zorvik encodes the space in `Hello there` for you.
        - "Rows: name = Ada, message = Hello there. URL {{api}}/contact, then Send."
      check:
        all:
          - request:
              server: api
              method: POST
              path: /contact
              headers: { content-type: "application/x-www-form-urlencoded*" }
              body: "re:(^|&)name=Ada(&|$)"
          - request: { server: api, method: POST, path: /contact, body: "re:(^|&)message=Hello(\\+|%20)there(&|$)" }
      solution:
        - send:
            method: POST
            url: "{{api}}/contact"
            body:
              type: formUrlencoded
              form: [{ key: name, value: Ada }, { key: message, value: Hello there }]
    - text: |
        Upload the receipt the lab put in your workspace. Send `POST {{api}}/upload` with a **Multipart form** body:

        - a text field `title` = `Coffee receipt`
        - a **File** field `file` pointing to `uploads/receipt.txt`
      hints:
        - Each multipart row has a small **Text / File** switch in front of its value.
        - Choose **Multipart form**, add `title` as a Text row, then add `file`, switch it to **File** and press **Browse**. The receipt is in the `uploads` folder of the Training Bootcamp workspace.
        - "You can also type the path uploads/receipt.txt: file paths are relative to the workspace folder. Then Send."
      check:
        all:
          - request:
              server: api
              method: POST
              path: /upload
              headers: { content-type: "multipart/form-data; boundary=*" }
              body: '*name="title"*Coffee receipt*'
          - request: { server: api, method: POST, path: /upload, body: '*filename="receipt.txt"*{{secret.receipt}}*' }
      solution:
        - send:
            method: POST
            url: "{{api}}/upload"
            body:
              type: multipart
              multipart:
                - { key: title, value: Coffee receipt }
                - { key: file, value: uploads/receipt.txt, file: true }
    - text: |
        Open **Servers → Lab · Help Desk API** and click your upload in its traffic. The file part carries the receipt: type its receipt code.
      hints:
        - Every request a lab server receives shows up in its traffic list. Click one to see its headers and body.
        - In the request body, find the part with `filename="receipt.txt"`. The file's text follows its small headers.
        - "Copy the value after the word Receipt on the second line of the file."
      check:
        answer: "{{secret.receipt}}"
      solution:
        - answer: "{{secret.receipt}}"
quiz:
  - question: What does the body of a Form URL-encoded request look like?
    options:
      - "name=Ada&message=Hello+there"
      - "{\"name\": \"Ada\"}"
      - Several parts separated by a boundary line
    answer: 0
    explain: It's the same key=value format as a query string, just in the body instead of the URL.
  - question: You need to send a photo along with a caption. Which body type fits?
    options:
      - Form URL-encoded
      - Multipart form
      - JSON
    answer: 1
    explain: Multipart bodies carry several parts, each with its own content type, so text fields and files can travel together.
  - question: In a multipart body, what is the boundary for?
    options:
      - It encrypts each part so proxies can't read it
      - It tells the server the maximum upload size
      - It marks where one part ends and the next begins
    answer: 2
    explain: The boundary is a random line that never appears in the data. It is announced in the Content-Type header so the server can split the body.
---

Before JSON took over, the web already had a way to send data: the **HTML form**. Every login page, search box and contact form still uses it, and many APIs accept it too. There are two flavors, and Zorvik has a body type for each.

## Form URL-encoded: simple fields

A **URL-encoded form** is the query-string format from the query parameters lesson, moved into the body: `key=value` pairs joined by `&`, with special characters percent-encoded.

```http
POST /contact HTTP/1.1
Content-Type: application/x-www-form-urlencoded

name=Ada&message=Hello+there
```

It is compact and every server understands it, but it can only carry text. In Zorvik choose **Form URL-encoded** in the **Body** tab and fill the table; Zorvik encodes the values and sets the `Content-Type` header.

## Multipart: fields and files together

To send a **file**, a body needs room for bytes and a little description of each piece. A **multipart form** body is split into **parts**. Each part has its own small headers (its field name, a file name, its own `Content-Type`) followed by its data. A **boundary**, a random line that appears nowhere in the data, separates the parts. The request's `Content-Type` header announces it: `multipart/form-data; boundary=----ZorvikBoundary…`.

```anatomy
------ZorvikBoundary7f3a | the boundary: a new part starts
Content-Disposition: form-data; name="title" | a text field called title
Coffee receipt | its value
------ZorvikBoundary7f3a | the next part
Content-Disposition: form-data; name="file"; filename="receipt.txt" | a file field, with the file's name
Content-Type: text/plain | what kind of file it is
Coffee Corner, Lisbon… | the file's bytes
------ZorvikBoundary7f3a-- | two extra dashes: the end
```

> [!note] Think of it like…
> Posting a parcel with several items. A URL-encoded form is a postcard: short text only. A multipart body is a box with dividers: each compartment has its own label saying what's inside, and there's room for a photo next to the letter.

In Zorvik, choose **Multipart form**. Each row has a **Text / File** switch: text rows hold a value, file rows hold a path. Zorvik reads the file when it sends, guesses the file's content type from its extension, and builds the parts and the boundary for you.

> [!warning] Files stay inside the workspace
> File paths can be relative to the workspace folder, and by default Zorvik only sends files from inside it. A shared workspace can't quietly upload something private like your SSH keys. To allow other files, switch on **Files outside the workspace** in **Settings → Data & privacy**.

## Which one to pick?

| You send… | Use |
|---|---|
| structured data to a modern API | JSON |
| a few simple fields, like a login form | Form URL-encoded |
| one or more files, maybe with fields | Multipart form |
| just the raw bytes of one file | Binary file |

The API's documentation decides, not you: if it says `multipart/form-data`, a JSON body won't work, however correct it looks.

**You'll use this when…** you test a profile-picture upload, an import endpoint that takes a CSV file, or an old login form that expects `username=…&password=…`.
