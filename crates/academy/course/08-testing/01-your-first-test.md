---
id: your-first-test
title: Your first test
summary: A test is a few lines of JavaScript that check every response for you, so you don't have to read it each time.
minutes: 5
lab:
  title: Catch a bug with a test
  goal: Write two tests for a product API and watch one of them catch a real bug.
  minutes: 8
  servers:
    api:
      name: Shop API
      kind: http
      http:
        routes:
          - method: GET
            path: /products/1
            headers:
              - key: Content-Type
                value: application/json
            body: '{"id": 1, "name": "Coffee mug", "price": 12.5, "inStock": true}'
          - method: GET
            path: /products/2
            headers:
              - key: Content-Type
                value: application/json
            body: '{"id": 2, "name": "Teapot", "price": "24.00", "inStock": true}'
  steps:
    - text: |
        Open a new request for `{{api}}/products/1`. In its **Scripts** tab, choose **Post-response** and add this test (or pick **Snippets → Status code is 200**):

        ```js
        pm.test("Status code is 200", () => {
          pm.response.to.have.status(200);
        });
        ```

        Press **Send** and open the response's **Tests** tab.
      hints:
        - The Scripts tab is next to Settings and Docs, under the URL bar. Post-response scripts run after the answer arrives.
        - Press ⌘N, type {{api}}/products/1, open Scripts → Post-response, paste the test, then Send.
        - "The response gets a Tests tab that says 1/1 in green. That's your first passing test."
      check:
        send: { url: "*/products/1", testsPassed: ">=1", testsFailed: 0 }
      solution:
        - send:
            method: GET
            url: "{{api}}/products/1"
            scripts:
              postResponse: |
                pm.test("Status code is 200", () => {
                  pm.response.to.have.status(200);
                });
    - text: |
        A 200 doesn't mean the data is right. Below the first test, add a second one that checks the price, then send again:

        ```js
        pm.test("Price is a number", () => {
          const product = pm.response.json();
          pm.expect(product.price).to.be.a("number");
        });
        ```
      hints:
        - pm.response.json() reads the body as JSON, so product.price is the price field.
        - Keep the first test and paste the second one under it, in the same Post-response script.
        - "Both tests in Post-response, then Send. The Tests tab should say 2/2."
      check:
        send: { url: "*/products/1", testsPassed: ">=2", testsFailed: 0 }
      solution:
        - send:
            method: GET
            url: "{{api}}/products/1"
            scripts:
              postResponse: |
                pm.test("Status code is 200", () => {
                  pm.response.to.have.status(200);
                });
                pm.test("Price is a number", () => {
                  const product = pm.response.json();
                  pm.expect(product.price).to.be.a("number");
                });
    - text: Now change the URL to `{{api}}/products/2`, keep the same tests, and send. Read the **Tests** tab. What did your test catch?
      hints:
        - The status is still 200, so at first glance everything looks fine.
        - Only the number at the end of the URL changes. Then Send and open the Tests tab.
        - "\"Price is a number\" fails: product 2's price is the text \"24.00\", not a number. An app doing maths with it would break."
      check:
        send: { url: "*/products/2", testsFailed: ">=1" }
      solution:
        - send:
            method: GET
            url: "{{api}}/products/2"
            scripts:
              postResponse: |
                pm.test("Status code is 200", () => {
                  pm.response.to.have.status(200);
                });
                pm.test("Price is a number", () => {
                  const product = pm.response.json();
                  pm.expect(product.price).to.be.a("number");
                });
quiz:
  - question: Where does a test like `pm.test("Status code is 200", …)` go?
    options:
      - In the request's post-response script, which runs after the answer arrives
      - In the URL
      - In the pre-request script, which runs before sending
    answer: 0
    explain: A test checks the response, so it must run after the response is there. Pre-request scripts prepare the request instead.
  - question: The server answers 200, but a test says the price is not a number. What should you believe?
    options:
      - The 200, because the server said everything is OK
      - Neither, send it again until both agree
      - The test, because the status only says the request was handled, not that the data is right
    answer: 2
    explain: A 200 with broken data is still a bug. That's why good tests check the body, not only the status.
  - question: Which line checks that the response came back in under half a second?
    options:
      - "`pm.response.to.have.status(500)`"
      - "`pm.expect(pm.response.responseTime).to.be.below(500)`"
      - "`pm.expect(pm.response.json().time).to.equal(0.5)`"
    answer: 1
    explain: "`pm.response.responseTime` is in milliseconds. `status(500)` would check for a server error instead."
---

So far you've checked every response with your own eyes. That works for one request. It doesn't work for fifty requests, every day, after every change to the code. A **test** does the looking for you.

## What a test is

A test is a small piece of JavaScript with a name and a check. Zorvik runs it right after the response arrives, and marks it **passed** or **failed**. Tests live in the request's **post-response script**: open the request's **Scripts** tab and choose **Post-response**.

```anatomy
pm.test("Price is a number", () => { | a test: a name and what to check
  const product = pm.response.json(); | read the response body as JSON
  pm.expect(product.price).to.be.a("number"); | the check; if it's false, the test fails
}); | end of the test
```

- `pm` is Zorvik's scripting toolbox. It works like Postman's, so tests you find online usually work as they are.
- `pm.test(name, function)` defines a test. The name is what you read in the results, so make it a sentence.
- `pm.expect(value)` starts a check, and reads almost like English: `.to.equal(3)`, `.to.be.a("string")`, `.to.include("Coffee")`, `.to.be.below(500)`.

```sequence
You -> Server: GET /products/1
Server --> You: 200 {"price": 12.5}
Note over You: post-response script runs the tests
Note over You: Tests 2/2 passed
```

> [!note] Think of it like…
> A checklist at the airport gate. Nobody reads your whole passport again; they tick three boxes: right name, right flight, still valid. A test ticks the boxes that matter for a response.

## Checks you'll write all the time

| You want to know… | Write |
|---|---|
| did it work? | `pm.response.to.have.status(200);` |
| is a field right? | `pm.expect(pm.response.json().name).to.equal("Coffee mug");` |
| is a field the right type? | `pm.expect(pm.response.json().price).to.be.a("number");` |
| was it fast? | `pm.expect(pm.response.responseTime).to.be.below(500);` |
| is a header there? | `pm.response.to.have.header("Content-Type");` |

Each line goes inside a `pm.test(…)`. You don't have to remember them: the **Snippets** menu in the script editor inserts the common ones, and typing `pm.` suggests the rest.

## Reading the results

After you send, the response gets a **Tests** tab showing, say, **2/2** in green. A failed test turns it red and says why, for example *expected '24.00' to be a number*. Anything you `console.log()` shows up in the **Console** tab.

> [!tip] Test the data, not just the status
> A 200 means the server handled the request. It doesn't mean the answer is right. The best tests check the fields your app actually uses.

**You'll use this when…** the backend team ships a change. Instead of clicking through twenty requests and squinting at JSON, you look at a column of green ticks, and the one red cross points straight at what broke.
