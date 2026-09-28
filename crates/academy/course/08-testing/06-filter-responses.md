---
id: filter-responses
title: Filter big responses
summary: The funnel above the response body shows just the part you need, picked out with JSONPath or jq (XPath for XML and HTML).
minutes: 6
added: 0.2.0
lab:
  title: Find it in the haystack
  goal: Pick names out of an order list with JSONPath, filter them with a condition, and add up totals with jq.
  minutes: 7
  servers:
    api:
      name: Order history API
      kind: http
      http:
        routes:
          - name: Orders
            method: GET
            path: /orders
            headers:
              - { key: Content-Type, value: application/json }
            body: &orders |
              {
                "page": 1,
                "orders": [
                  {"id": "A-1001", "customer": "Ada Lovelace", "total": 42.5, "status": "shipped", "items": [{"sku": "kb-01", "qty": 1}]},
                  {"id": "A-1002", "customer": "Grace Hopper", "total": 129.5, "status": "pending", "items": [{"sku": "ms-02", "qty": 2}, {"sku": "pad-01", "qty": 1}]},
                  {"id": "A-1003", "customer": "Alan Turing", "total": 18, "status": "shipped", "items": [{"sku": "cab-05", "qty": 3}]},
                  {"id": "A-1004", "customer": "Katherine Johnson", "total": 240.25, "status": "pending", "items": [{"sku": "mon-27", "qty": 1}]},
                  {"id": "A-1005", "customer": "Linus Torvalds", "total": 99.5, "status": "cancelled", "items": [{"sku": "kb-02", "qty": 1}]},
                  {"id": "A-1006", "customer": "Margaret Hamilton", "total": 310, "status": "shipped", "items": [{"sku": "lap-13", "qty": 1}]}
                ]
              }
  steps:
    - text: |
        Send `GET {{api}}/orders`. Then click **Filter** (the funnel icon above the body), keep **JSONPath** and type:

        ```text
        $.orders[*].customer
        ```

        The body now shows only the six customers' names.
      hints:
        - "Read it from left to right: $ is the whole answer, .orders its orders list, [*] every item of it, .customer that key of each item."
        - The funnel is in the body toolbar, next to Wrap lines and Copy body. A row opens above the body with the language and an expression box.
        - "GET {{api}}/orders, Send, click the funnel, JSONPath, type $.orders[*].customer. The row says 6 matches."
      check:
        all:
          - request: { server: api, method: GET, path: /orders }
          - call:
              method: response.filter
              ok: true
              params: { language: jsonPath }
              result: { count: 6, text: "*Ada Lovelace*Margaret Hamilton*" }
      solution:
        - send: { method: GET, url: "{{api}}/orders" }
        - call:
            method: response.filter
            params: { text: *orders, language: jsonPath, expression: "$.orders[*].customer" }
    - text: |
        Only the big spenders now: change the expression to show the customers of orders **over 100**. A filter `[?…]` keeps the items where its condition is true, and `@` is the item being tested.
      hints:
        - "The condition goes where [*] was: [?@.total > 100]."
        - Keep .customer at the end, so you get names, not whole orders.
        - "Type $.orders[?@.total > 100].customer. It shows 3 matches: Grace Hopper, Katherine Johnson and Margaret Hamilton."
      check:
        call:
          method: response.filter
          ok: true
          params: { language: jsonPath }
          result: { count: 3, text: "*Grace Hopper*Katherine Johnson*Margaret Hamilton*" }
      solution:
        - call:
            method: response.filter
            params: { text: *orders, language: jsonPath, expression: "$.orders[?@.total > 100].customer" }
    - text: |
        JSONPath picks values; **jq** can also calculate with them. How much money is waiting in **pending** orders? Switch the filter to **jq** and type:

        ```text
        [.orders[] | select(.status == "pending") | .total] | add
        ```
      hints:
        - "Read it through the pipes (|): take every order, keep the pending ones, take their totals, put them in a list [ ], then add the list up."
        - The language switch is at the left of the filter row. The expression box keeps working as you type.
        - "Choose jq and type [.orders[] | select(.status == \"pending\") | .total] | add. The body shows one number: 369.75."
      check:
        call:
          method: response.filter
          ok: true
          params: { language: jq }
          result: { text: "369.75" }
      solution:
        - call:
            method: response.filter
            params: { text: *orders, language: jq, expression: '[.orders[] | select(.status == "pending") | .total] | add' }
quiz:
  - question: "`$.orders[*].id` is written in which filter language?"
    options:
      - JSONPath
      - jq
      - XPath
    answer: 0
    explain: JSONPath expressions start with $, the whole document. jq starts with a dot, and XPath with slashes.
  - question: The filter says 3 matches. What happened to the rest of the response?
    options:
      - It was deleted on the server
      - Zorvik sent a new request that only asked for 3 items
      - Nothing. The filter only changes what you see; close it to see the whole body again
    answer: 2
    explain: Filters work on the response you already have. Nothing is sent, and closing the filter (or Esc) brings the whole body back.
  - question: You want the sum of every order's total, as one number. Which fits?
    options:
      - "JSONPath `$.orders[*].total`"
      - "jq `[.orders[].total] | add`"
      - "XPath `//total`"
    answer: 1
    explain: JSONPath and XPath pick values out; jq can also calculate with them, such as adding, counting or sorting.
---

Real API answers are big. A list of 300 orders, each with 40 fields, is thousands of lines of JSON. When you only want to know "which customers are waiting?", scrolling and searching by eye is slow and easy to get wrong.

**Filter** (the funnel icon in the body toolbar) opens a row above the body. Pick a language, type an **expression**, and the body shows only what matches, with the number of matches next to it.

```flow
Whole response -[JSONPath: every customer]-> 6 names
Whole response -[JSONPath: orders over 100]-> 3 names
Whole response -[jq: add up the totals]-> 1 number
```

Close the filter, or press **Esc** in the box, to see the whole body again. The expression stays when you send again, so you can keep an eye on the same part of the response while you work.

## JSONPath: point at the values

**JSONPath** walks through JSON like a path through folders:

```anatomy
$ | the whole document (the root)
.orders | the value under the key orders
[*] | every item of a list ([0] is the first one)
[?@.total > 100] | only items whose condition is true; @ is the item being tested
.customer | that key of each item left
```

JSONPath shows its matches as a JSON list, even when there is only one.

## jq: point, then calculate

**jq** is a small language for reshaping JSON. You chain steps with pipes `|`, each one working on what the step before produced:

- Every customer: `.orders[].customer`
- Pending orders only: `.orders[] | select(.status == "pending")`
- How many orders? `.orders | length`
- The sum of all totals: `[.orders[].total] | add`

jq shows each result on its own, like the `jq` command line tool, so a sum is just one number.

> [!note] Think of it like…
> A spreadsheet. JSONPath is the filter button on a column: the rows are all still there, you just see the ones you asked for. jq is a formula cell: it can also add, count and rearrange what it finds.

## XPath for XML and HTML

For XML and HTML answers the filter offers **XPath** instead, for example `//item[price > 10]/name` or `count(//li)`. It runs on the part of the body the pane shows. Plain text answers offer all three languages.

## Good to know

- JSONPath and jq run on the **whole** body, even when the pane only shows the first 10 MB.
- At most 10,000 results are shown. A mistake in the expression is shown in red, saying what's wrong.
- **Copy body** copies what the filter shows, handy for pasting a short list into a bug report.
- AI agents use the same filters when they send requests through Zorvik, so they read only what they need.

**You'll use this when…** an endpoint returns hundreds of records and you need one answer fast: which items failed, which ids are missing, how much is in the basket. Type the question as an expression instead of scrolling for it.
