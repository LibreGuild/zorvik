---
id: repeat-until
title: Waiting for results
summary: Some answers aren't ready yet. Repeat a request in a run until a condition holds, instead of guessing how long to wait.
minutes: 5
lab:
  title: Wait for the report
  goal: Make a collection run ask again and again until a report is ready, so its test passes every time.
  minutes: 7
  servers:
    api:
      name: Reports API
      kind: http
      http:
        routes:
          - method: GET
            path: /reports/:id
            headers:
              - key: Content-Type
                value: application/json
            body: '{"id": "{{request.params.id}}", "ready": {{$randomBoolean}}, "checkedAt": "{{$isoTimestamp}}"}'
  files:
    requests/Report jobs/_folder.yaml: |
      name: Report jobs
      seq: 90
    requests/Report jobs/Report status.yaml: |
      name: Report status
      seq: 1
      method: GET
      url: {{api}}/reports/42
      scripts:
        postResponse: |
          pm.test("Report is ready", () => {
            pm.expect(pm.response.json().ready).to.be.true;
          });
  steps:
    - text: The lab added a folder **Report jobs** to your collection. Open **Report status** in it and press **Send** three or four times. Watch `ready` in the answer, and the **Tests** tab.
      hints:
        - The Report jobs folder is in the Collection section of the sidebar. Click the request to open it.
        - This practice server flips a coin each time you ask, a stand-in for a real job that finishes when it finishes.
        - "Send it a few times. Sometimes ready is true and the test passes, sometimes false and it fails."
      check:
        request: { server: api, method: GET, path: /reports/42, count: 2 }
      solution:
        - send: &status
            method: GET
            url: "{{api}}/reports/42"
            scripts:
              postResponse: |
                pm.test("Report is ready", () => {
                  pm.expect(pm.response.json().ready).to.be.true;
                });
        - send: *status
    - text: |
        In **Report status**, open the **Settings** tab. Under **Repeat until**, switch **Repeat in collection runs** on, set **Condition** to `pm.response.json().ready === true` and **Every** to `250`. Save (**⌘/Ctrl + S**).
      hints:
        - Settings is one of the request tabs, next to Scripts. Repeat until is near the bottom.
        - "The condition is JavaScript that must become true: pm.response.json().ready === true"
        - "Repeat in collection runs: On, Condition as above, Every: 250 (milliseconds), Give up after: leave 30000. Then ⌘S."
      check:
        saved:
          request: { name: Report status, path: "Report jobs/*", settings: { repeat: { condition: "*ready*" } } }
      solution:
        - call:
            method: request.save
            params:
              path: Report jobs/Report status.yaml
              request:
                name: Report status
                seq: 1
                method: GET
                url: "{{api}}/reports/42"
                settings: { repeat: { condition: "pm.response.json().ready === true", intervalMs: 200, timeoutMs: 30000 } }
                scripts:
                  postResponse: |
                    pm.test("Report is ready", () => {
                      pm.expect(pm.response.json().ready).to.be.true;
                    });
    - text: Right-click the **Report jobs** folder → **Run…**, then press **Run**. The test passes. If the runner had to ask more than once, the result shows how many sends it took, like **3×**. Run it again if you like; it passes every time now.
      hints:
        - Repeat until only works in collection runs. A single Send sends once.
        - Right-click Report jobs in the sidebar, choose Run…, and press Run or ⌘Enter.
        - "The result row shows a number like 2× or 4×: the runner asked that many times before ready was true."
      check:
        run: { name: Report jobs, passed: true, testsPassed: ">=1" }
      solution:
        - call: { method: runner.start, params: { folder: Report jobs } }
        - wait: 1000
quiz:
  - question: Why not just wait 10 seconds before checking a slow job?
    options:
      - Zorvik can send each request only once
      - Some days the job takes 11 seconds and the test fails; other days you waste 9 seconds
      - Servers don't allow waiting
    answer: 1
    explain: A fixed wait is either too short or too long. Asking until it's ready is as fast as the job and as patient as it needs to be.
  - question: A request repeats with an empty **Condition**. When does it stop?
    options:
      - After exactly three sends
      - Never
      - When its tests pass (or, without tests, when the status is below 400), or when time runs out
    answer: 2
    explain: An empty condition means "until this request's tests pass". The Give up after limit always ends it, and then the request fails.
  - question: Where does **Repeat until** take effect?
    options:
      - In collection runs, in the app and with `zorvik run`
      - Every time you press Send
      - Only in load tests
    answer: 0
    explain: A single Send sends once so you can see each answer. Collection runs, including runs on the command line, repeat.
---

Not every answer is ready right away. You ask a server to build a report, convert a video or process a payment, and it says *"got it, working on it"*. The result shows up later. This is called an **asynchronous job**, and it's everywhere: exports, payments, AI requests, anything that takes more than a moment.

## The job pattern

Usually one request starts the job, and another asks about it. You ask again, and again, until it says *done*. Asking repeatedly like this is called **polling**.

```sequence
participants: Zorvik, Server
Zorvik -> Server: GET /reports/42
Server --> Zorvik: {"ready": false}
Note over Zorvik: wait 250 ms
Zorvik -> Server: GET /reports/42
Server --> Zorvik: {"ready": false}
Note over Zorvik: wait 250 ms
Zorvik -> Server: GET /reports/42
Server --> Zorvik: {"ready": true}
Note over Zorvik: condition holds, tests run
```

A test that checks the report right away would pass on lucky days and fail on others. A test like that is called **flaky**, and flaky tests are worse than no tests: people learn to ignore them.

## Repeat until

Every HTTP request has a **Settings** tab with a **Repeat until** section. Switch **Repeat in collection runs** on, and fill in:

- **Condition:** JavaScript that becomes true when the answer is the one you're waiting for, like `pm.response.json().status === "done"`. Leave it empty to repeat until the request's own tests pass.
- **Every:** how many milliseconds to wait between sends.
- **Give up after:** a time limit in milliseconds. If the condition still doesn't hold, the request fails with a message saying so.

The condition runs after the post-response scripts, so it can use anything they did. The runner shows each result with the number of sends it took, like **3×**, and only the last answer's tests count.

> [!note] Think of it like…
> Waiting for a parcel with tracking. You don't stand at the door all day, and you don't give up after one look. You check the tracking page every so often until it says *delivered*, and if it's still not there after a week, you call the shop.

## When it applies

Repeating happens in **collection runs**, in the app and with `zorvik run` on the command line. A single **Send** always sends once, so you can look at each answer yourself.

> [!warning] Always keep a limit
> Pick a **Give up after** that fits the job, a few seconds for a report, maybe minutes for a video. A job that never finishes should fail your run, not hang it forever.

In the lab, the practice server flips a coin each time it's asked. A real server would say `ready` once the work is done; your request can't tell the difference, and that's the point: it doesn't need to know how long the job takes.

**You'll use this when…** you test an export or payment flow in CI. The run asks until the job is done, passes as soon as it is, and fails clearly if the job gets stuck, instead of failing at random every third night.
