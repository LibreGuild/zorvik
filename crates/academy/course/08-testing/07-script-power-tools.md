---
id: script-power-tools
title: Script power tools
summary: Scripts can send their own requests, use built-in libraries, check a response against a JSON Schema, draw it as a table, and skip a request altogether.
minutes: 7
added: 0.2.0
lab:
  title: The self-serving report
  goal: Make a request fetch its own token, hold its answer to a schema, show it as a table, and keep a dangerous request from running.
  minutes: 10
  vars:
    stage: production
  servers:
    api:
      name: Reports API
      kind: http
      http:
        routes:
          - name: Token
            method: POST
            path: /token
            headers:
              - { key: Content-Type, value: application/json }
            body: '{"token": "{{secret.token}}", "expiresIn": 300}'
          - name: Weekly report
            method: GET
            path: /reports
            matchHeaders:
              - { key: Authorization, value: "Bearer {{secret.token}}" }
            headers:
              - { key: Content-Type, value: application/json }
            body: '{"title": "Weekly sales", "week": 39, "items": [{"name": "Tomato soup", "sales": 128, "price": 4.5}, {"name": "Grilled cheese", "sales": 96, "price": 6}, {"name": "Lemonade", "sales": 210, "price": 2.5}]}'
          - name: No token
            method: GET
            path: /reports
            status: 401
            headers:
              - { key: Content-Type, value: application/json }
            body: '{"error": "unauthorized", "message": "Get a token from POST /token first."}'
          - name: Delete test data
            method: DELETE
            path: /test-data
            status: 204
  steps:
    - text: |
        `GET {{api}}/reports` needs a fresh token from `POST {{api}}/token`. Let the request fetch its own: give it this **Pre-request** script, then send it.

        ```js
        const res = await pm.sendRequest({
          url: pm.variables.replaceIn("{{api}}/token"),
          method: "POST",
        });
        pm.request.headers.upsert({ key: "Authorization", value: "Bearer " + res.json().token });
        ```
      hints:
        - "`pm.sendRequest` doesn't fill in `{{variables}}` itself, so the URL goes through `pm.variables.replaceIn` first. `await` waits for the answer."
        - "New request, GET {{api}}/reports. Scripts tab → Pre-request → paste the script → Send."
        - "You get 200 and the weekly report. The response's Console tab lists the extra request: pm.sendRequest POST …/token → 200 OK."
      check:
        all:
          - request: { server: api, method: POST, path: /token }
          - request: { server: api, method: GET, path: /reports, status: 200, headers: { authorization: "Bearer {{secret.token}}" } }
          - send: { url: "*/reports", status: 200, request: { scripts: { preRequest: "*sendRequest*" } } }
      solution:
        - send:
            method: GET
            url: "{{api}}/reports"
            scripts:
              preRequest: &fetchToken |
                const res = await pm.sendRequest({
                  url: pm.variables.replaceIn("{{api}}/token"),
                  method: "POST",
                });
                pm.request.headers.upsert({ key: "Authorization", value: "Bearer " + res.json().token });
    - text: |
        Hold the report to a shape. Add a **Post-response** script with a JSON Schema test, keep the pre-request script, and send again:

        ```js
        const schema = {
          type: "object",
          required: ["title", "items"],
          properties: {
            items: {
              type: "array",
              items: {
                type: "object",
                required: ["name", "sales"],
                properties: { sales: { type: "integer" } },
              },
            },
          },
        };
        pm.test("Report has the right shape", () => pm.response.to.have.jsonSchema(schema));
        ```
      hints:
        - A JSON Schema describes the shape of a JSON value. jsonSchema checks the whole body against it and lists every field that doesn't fit.
        - "Same request: Scripts tab → Post-response → paste the script. Leave the Pre-request script as it is, or the request gets no token."
        - "Send again. The response's Tests tab shows Report has the right shape, passed."
      check:
        send:
          url: "*/reports"
          status: 200
          testsPassed: ">=1"
          testsFailed: 0
          request: { scripts: { postResponse: "*jsonSchema*" } }
      solution:
        - send:
            method: GET
            url: "{{api}}/reports"
            scripts:
              preRequest: *fetchToken
              postResponse: &schemaTest |
                const schema = {
                  type: "object",
                  required: ["title", "items"],
                  properties: {
                    items: {
                      type: "array",
                      items: {
                        type: "object",
                        required: ["name", "sales"],
                        properties: { sales: { type: "integer" } },
                      },
                    },
                  },
                };
                pm.test("Report has the right shape", () => pm.response.to.have.jsonSchema(schema));
    - text: |
        Now make it readable. Add these lines at the end of the **Post-response** script: the built-in **lodash** library sorts the dishes, best seller first, and `pm.visualizer.set` draws them as a table. Send again and open the response's new **Visualize** tab.

        ```js
        const _ = require("lodash");
        const items = _.orderBy(pm.response.json().items, "sales", "desc");
        pm.visualizer.set(`
          <table>
            <tr><th>Dish</th><th>Sales</th></tr>
            {{#each items}}<tr><td>{{name}}</td><td>{{sales}}</td></tr>{{/each}}
          </table>`, { items });
        ```
      hints:
        - "The template is Handlebars: {{#each items}} repeats a row for every dish, and {{name}} is a field of that dish."
        - "Libraries are part of Zorvik, so require(\"lodash\") works offline. Paste the lines below the jsonSchema test in Post-response."
        - "Send again. Next to Body, a Visualize tab shows the table: Lemonade 210, Tomato soup 128, Grilled cheese 96."
      check:
        send:
          url: "*/reports"
          status: 200
          visualized: true
          request: { scripts: { postResponse: "re:lodash|_\\." } }
      solution:
        - send:
            method: GET
            url: "{{api}}/reports"
            scripts:
              preRequest: *fetchToken
              postResponse: |
                const schema = {
                  type: "object",
                  required: ["title", "items"],
                  properties: {
                    items: {
                      type: "array",
                      items: {
                        type: "object",
                        required: ["name", "sales"],
                        properties: { sales: { type: "integer" } },
                      },
                    },
                  },
                };
                pm.test("Report has the right shape", () => pm.response.to.have.jsonSchema(schema));

                const _ = require("lodash");
                const items = _.orderBy(pm.response.json().items, "sales", "desc");
                pm.visualizer.set(`
                  <table>
                    <tr><th>Dish</th><th>Sales</th></tr>
                    {{#each items}}<tr><td>{{name}}</td><td>{{sales}}</td></tr>{{/each}}
                  </table>`, { items });
    - text: |
        Last, a safety catch. `DELETE {{api}}/test-data` wipes test data, which must never happen on production. Create that request with this **Pre-request** script and send it. The Lab environment says `stage` is `production`, so it's not sent.

        ```js
        if (pm.environment.get("stage") === "production") {
          pm.execution.skipRequest();
        }
        ```
      hints:
        - "pm.execution.skipRequest() in a pre-request script stops the send before anything leaves your computer."
        - "New request, method DELETE, URL {{api}}/test-data. Scripts tab → Pre-request → paste the script → Send."
        - "The response pane says Not sent. In a collection run, the request is reported as skipped and the run goes on."
      check:
        send: { method: DELETE, url: "*/test-data", errorCode: skipped }
      solution:
        - send:
            method: DELETE
            url: "{{api}}/test-data"
            scripts:
              preRequest: |
                if (pm.environment.get("stage") === "production") {
                  pm.execution.skipRequest();
                }
quiz:
  - question: "In a script, `pm.sendRequest({ url: \"{{api}}/token\" })` fails. Why?"
    options:
      - The token endpoint is down
      - "pm.sendRequest doesn't fill in variables: wrap the URL in pm.variables.replaceIn"
      - Scripts can only send GET requests
    answer: 1
    explain: As in Postman, requests a script sends are taken literally. pm.variables.replaceIn fills in {{api}} (and dynamic variables) first.
  - question: Where does `pm.visualizer.set(template, data)` show its result?
    options:
      - In the Console tab
      - In a new browser window
      - In a Visualize tab of the response
    answer: 2
    explain: The template is filled with your data and shown in the response's Visualize tab, in a sandbox where its own scripts don't run.
  - question: A pre-request script calls `pm.execution.skipRequest()` during a collection run. What happens?
    options:
      - The request isn't sent; the run reports it as skipped and goes on
      - The whole run stops with an error
      - The request is sent anyway, because skipping only works for single sends
    answer: 0
    explain: A skipped request isn't a failure. A single send shows Not sent; a run lists it as skipped, with the reason, and moves to the next request.
---

You already use scripts to save tokens and write tests. They can do a lot more. This lesson is a tour of the power tools, each one a line or two of JavaScript.

| Tool | What it does |
|---|---|
| `pm.sendRequest(…)` | sends another request from the script: get a token, create test data, clean up |
| `require("lodash")`, `CryptoJS`, `moment`, `ajv`… | libraries built into Zorvik, so they work offline |
| `pm.response.to.have.jsonSchema(schema)` | checks the whole body against a JSON Schema |
| `pm.visualizer.set(template, data)` | draws the response your way, in a **Visualize** tab |
| `pm.execution.skipRequest()` | stops a request from being sent at all |
| `pm.cookies` | reads the cookies of the request's site |
| `setTimeout`, `setInterval` | timers; the script waits for them before it ends |

## Requests from a script

`pm.sendRequest` sends a request of its own. With `await` at the top of a script, the script waits for the answer:

```sequence
participants: Pre-request script, Zorvik, Reports API
Pre-request script -> Reports API: pm.sendRequest POST /token
Reports API --> Pre-request script: {"token": "a1b2…"}
Note over Pre-request script: sets Authorization: Bearer a1b2…
Zorvik -> Reports API: GET /reports, with the token
Reports API --> Zorvik: 200 weekly report
```

Two things to know: it doesn't fill in `{{variables}}` by itself (use `pm.variables.replaceIn`), and every call shows up in the response's **Console** tab. You can also pass a callback, `pm.sendRequest(request, (err, res) => …)`, as older Postman scripts do.

> [!note] Think of it like…
> A hotel concierge. You ask for a table at a restaurant, and before you even get there the concierge has phoned ahead, got the booking number and written it on your card. The pre-request script is the concierge; the token is the booking number.

## Libraries, built in

Scripts can `require` the libraries Postman's sandbox has: **lodash** (also the global `_`) for sorting and grouping, **CryptoJS** for hashes and signatures, **moment** for dates, **Ajv** for JSON Schema, and a few more. They are part of Zorvik, so nothing is downloaded.

```js
const _ = require("lodash");
const hash = CryptoJS.SHA256(pm.response.text()).toString();
```

## Check the shape, all at once

Writing a test per field gets long. A **JSON Schema** describes the whole shape: which fields are required and what type each one has. `pm.response.to.have.jsonSchema(schema)` checks the body against it, and a failure lists every field that doesn't fit, such as `body/items/0/sales must be integer`.

## Show it your way

`pm.visualizer.set(template, data)` fills a **Handlebars** template with your data and shows the result in the response's **Visualize** tab: a table of dishes instead of a wall of JSON. The page runs in a sandbox: plain HTML and CSS, no scripts, nothing loaded from the internet.

## Don't send it

`pm.execution.skipRequest()` in a pre-request script means "not this time". A single send shows **Not sent**; a collection run reports the request as skipped and carries on. Use it for requests that only make sense in some environments, or when an earlier request found there's nothing to do.

**You'll use this when…** a request needs a token from another service, a report is easier to read as a table than as JSON, or a clean-up request must never touch production. A few lines of script, and the collection takes care of itself.
