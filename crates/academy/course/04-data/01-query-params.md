---
id: query-params
title: Query parameters
summary: The part of a URL after the `?` tells the server exactly which slice of the data you want.
minutes: 5
lab:
  title: Find the cheapest book
  goal: Filter, sort and limit a product list with query parameters, and watch a mock pick its answer by them.
  minutes: 6
  servers:
    api:
      name: Bookshop API
      kind: http
      http:
        routes:
          - name: Cheapest book
            method: GET
            path: /products
            matchQuery:
              - { key: category, value: books }
              - { key: sort, value: price }
              - { key: limit, value: "1" }
            headers:
              - { key: Content-Type, value: application/json }
            body: '{"category": "books", "sort": "price", "limit": 1, "items": [{"title": "Refactoring on a Budget", "price": 4.99, "code": "{{secret.code}}"}]}'
          - name: Books
            method: GET
            path: /products
            matchQuery:
              - { key: category, value: books }
            headers:
              - { key: Content-Type, value: application/json }
            body: '{"category": "books", "items": [{"title": "The Pragmatic Programmer", "price": 39.90}, {"title": "Clean Code", "price": 31.50}, {"title": "Refactoring on a Budget", "price": 4.99}]}'
          - name: Everything
            method: GET
            path: /products
            headers:
              - { key: Content-Type, value: application/json }
            body: '{"hint": "That is the whole shop. Try ?category=books", "items": [{"title": "Clean Code", "price": 31.50}, {"title": "USB cable", "price": 7.00}, {"title": "Desk lamp", "price": 24.00}, {"title": "The Pragmatic Programmer", "price": 39.90}]}'
  steps:
    - text: Send `GET {{api}}/products?category=books`. Only books come back.
      hints:
        - Everything after the `?` is the query string. You can type it straight into the URL bar.
        - Open a new request (⌘/Ctrl + N), type the URL and press Send. Then open the **Params** tab below the URL bar; your parameter is already in the table.
        - "Method GET, URL {{api}}/products?category=books, then Send."
      check:
        request: { server: api, method: GET, path: /products, query: { category: books } }
      solution:
        - send: { method: GET, url: "{{api}}/products?category=books" }
    - text: |
        In the **Params** tab, add two rows: `sort` = `price` and `limit` = `1`. Watch the URL change while you type, then send.
      hints:
        - The table and the URL are two views of the same query string. Edit either one.
        - In **Params**, click the empty row under `category`, type `sort` as the parameter and `price` as the value. Add `limit` = `1` the same way.
        - "The URL should end with ?category=books&sort=price&limit=1. Press Send."
      check:
        request: { server: api, method: GET, path: /products, query: { category: books, sort: price, limit: "1" }, route: Cheapest book }
      solution:
        - send: { method: GET, url: "{{api}}/products?category=books&sort=price&limit=1" }
    - text: Only one book came back, the cheapest. Type its `code`.
      hints:
        - Look at the response body. The one item in `items` has a `code` field.
        - Copy the value after `"code":`, without the quotes.
      check:
        answer: "{{secret.code}}"
      solution:
        - answer: "{{secret.code}}"
quiz:
  - question: In `/orders?status=open&page=2`, what is `page`?
    options:
      - Part of the path
      - A query parameter with the value 2
      - A request header
    answer: 1
    explain: Everything after the `?` is the query string. `status=open` and `page=2` are two parameters, joined by `&`.
  - question: You untick a row in the Params table. What happens to that parameter?
    options:
      - It is deleted for good
      - It is sent with an empty value
      - It stays in the table but is not sent
    answer: 2
    explain: Unticked rows are kept with the request, so you can switch a filter on and off without retyping it.
  - question: You want to search for `rock & roll`. Why can't the `&` go into the URL as it is?
    options:
      - A raw `&` starts a new parameter, so your value would be cut in two
      - URLs may only contain letters, digits and dashes
      - The `&` would turn the request into a POST
    answer: 0
    explain: "`&` separates parameters, so inside a value it must be written as `%26`. The Params table does that for you."
---

A URL can carry more than an address. After a `?` comes the **query string**: small `key=value` pairs, joined by `&`, that tell the server *which* data you want. Each pair is a **query parameter**.

```anatomy
https://shop.example/products | where: the host and the path
?category=books | the query string starts at the first ?
&sort=price | more parameters, joined with &
&limit=1 | the value is always text: the server decides it means a number
```

The path says *what* you are asking for (the products). The query parameters say *how much of it, and in what order*.

> [!note] Think of it like…
> Ordering at a deli counter. The path is the counter ("products, please"). The query parameters are the details you add: "just the books, cheapest first, and only one".

## What query parameters are used for

| Job | Typical parameters |
|---|---|
| Filtering | `?status=open`, `?category=books` |
| Sorting | `?sort=price`, `?order=desc` |
| Paging through long lists | `?page=2&limit=50`, `?offset=100` |
| Searching | `?q=keyboard` |

Their names are not standard: every API chooses its own, so check its documentation. The order of the pairs usually doesn't matter to the server.

## The Params tab

In Zorvik, the **Params** tab under the URL bar shows the query string as a table. The table and the URL are two views of the same thing: type in one and the other follows. Untick a row to stop sending it without losing it, which is handy for switching a filter on and off.

## Special characters

Some characters mean something in a URL: `?` starts the query, `&` separates pairs, `=` joins a key to its value, and `#` starts a fragment the server never sees. To use one of them *inside* a value, it is **percent-encoded**: written as `%` and two hex digits.

| Character | Encoded |
|---|---|
| space | `%20` (web forms often use `+`) |
| `&` | `%26` |
| `#` | `%23` |

When you type a value in the Params table, Zorvik escapes `&` and `#` for you, and spaces are encoded when the request is sent. The server decodes everything again, so `q=rock%20%26%20roll` arrives as `rock & roll`.

## How a server picks an answer

The lab's practice server is a **mock**: a small fake API that answers from routes you describe. One route can say "only answer when the query has `category=books`" (in a mock route, that's **Match only when → Query parameters**). Routes are tried from top to bottom and the first match answers:

```flow
?category=books&sort=price&limit=1 -> Cheapest book
?category=books -> Books
no parameters -> Everything
```

Real servers do the same kind of thing in code: read the parameters, filter the data, send back only what matched.

> [!tip] Parameters are part of the URL
> They show up in browser history, server logs and screenshots. Never put passwords or tokens in a query string.

**You'll use this when…** you page through a long list of orders, search for one customer, or reproduce a bug a user hit with a specific filter. Paste their URL and every filter comes with it.
