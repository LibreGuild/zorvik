---
id: zorvik-command-line
title: The zorvik command line
summary: The zorvik command runs your saved collections, load tests and mock servers from a terminal, with no clicking.
minutes: 6
lab:
  title: Run it like a build server would
  goal: Run a smoke test folder, read its result the way a script reads an exit code, and give it the secret it's missing.
  minutes: 8
  servers:
    api:
      name: Catalog API
      kind: http
      http:
        routes:
          - method: GET
            path: /health
            headers:
              - key: Content-Type
                value: application/json
            body: '{"status": "ok"}'
          - method: GET
            path: /catalog
            matchHeaders:
              - key: X-Api-Key
                value: ci-demo-4242
            headers:
              - key: Content-Type
                value: application/json
            body: '[{"sku": "MUG-1", "name": "Coffee mug"}, {"sku": "POT-2", "name": "Teapot"}]'
          - method: GET
            path: /catalog
            status: 401
            headers:
              - key: Content-Type
                value: application/json
            body: '{"error": "Missing or wrong X-Api-Key header"}'
  files:
    requests/CI smoke tests/_folder.yaml: |
      name: CI smoke tests
      seq: 91
    requests/CI smoke tests/Health.yaml: |
      name: Health
      seq: 1
      method: GET
      url: {{api}}/health
      scripts:
        postResponse: |
          pm.test("Status code is 200", () => {
            pm.response.to.have.status(200);
          });
    requests/CI smoke tests/Catalog.yaml: |
      name: Catalog
      seq: 2
      method: GET
      url: {{api}}/catalog
      headers:
        - key: X-Api-Key
          value: "{{apiKey}}"
      scripts:
        postResponse: |
          pm.test("Status code is 200", () => {
            pm.response.to.have.status(200);
          });
  steps:
    - text: The lab added a folder **CI smoke tests** with two requests and their tests. Right-click it → **Run…** and press **Run**. This is exactly what `zorvik run` does in a terminal.
      hints:
        - The folder is in the Collection section of the sidebar.
        - Right-click CI smoke tests, choose Run…, then press Run or ⌘Enter.
        - "Health passes. Catalog fails with 401: its X-Api-Key header says {{apiKey}}, and no environment defines apiKey yet."
      check:
        run: { name: CI smoke tests, passed: false, failed: 1 }
      solution:
        - call: { method: runner.start, params: { folder: CI smoke tests } }
        - wait: 500
    - text: If a build server had run this folder with `zorvik run`, which **exit code** would the command end with? Type the number.
      hints:
        - An exit code is the number a program hands back when it ends. Scripts and CI servers read it to decide what happens next.
        - 0 means everything passed. Something did fail here.
        - "zorvik run ends with 1 when a request or a test failed (and 2 when the run could not start or was stopped)."
      check:
        answer: ["1", "one", "exit code 1"]
      solution:
        - answer: "1"
    - text: |
        On a build server you'd pass the key with `--var apiKey=…`. In the app, add it to the environment: press **⌘/Ctrl + E**, select **Lab**, add `apiKey` with the value `ci-demo-4242`, click the **lock** to make it secret, and **Save changes**. Then run the folder again.
      hints:
        - The Catalog request already says {{apiKey}}. It only needs a value.
        - In Environments & variables, pick Lab, add a row apiKey = ci-demo-4242, close the lock, Save changes. Then Run again in the runner tab.
        - "Both requests pass now: the run's exit code would be 0."
      check:
        all:
          - saved:
              environment: { name: Lab, variables: [{ key: apiKey, secret: true }] }
          - run: { name: CI smoke tests, passed: true, testsPassed: 2 }
      solution:
        - call:
            method: env.save
            params:
              id: Lab
              environment:
                name: Lab
                variables:
                  - { key: api, value: "{{api}}" }
                  - { key: api_port, value: "{{api_port}}" }
                  - { key: api_host, value: "{{api_host}}" }
                  - { key: apiKey, value: ci-demo-4242, secret: true }
        - call: { method: runner.start, params: { folder: CI smoke tests } }
        - wait: 500
quiz:
  - question: What does `zorvik run ./shop-api --env Staging --folder Orders` do?
    options:
      - It runs the requests of the Orders folder in the workspace ./shop-api, with the Staging environment's variables, scripts and tests included
      - It opens the Zorvik app on the Orders folder
      - It uploads the Orders folder to the Staging server
    answer: 0
    explain: The command uses the same runner as the app. `--env` picks the environment by name, and `--folder` a folder inside `requests/`.
  - question: Your collection needs a secret `apiKey`. How does `zorvik run` on a build server get it?
    options:
      - It reads it from the environment file in Git
      - It asks the Zorvik app on your laptop
      - With `--var apiKey=…`, filled from the CI system's secret store
    answer: 2
    explain: Secret values never enter the workspace files, so the build server doesn't have them. `--var` passes one in, and it beats every other value.
  - question: A load test's thresholds say "p95 under 200 ms", and the run's p95 is 350 ms. What does `zorvik load` do?
    options:
      - It prints a warning and exits with 0
      - It exits with 1, so the pipeline stops
      - It slows down until the threshold passes
    answer: 1
    explain: "A failed threshold is a failed run: exit code 1. That's how a pipeline can refuse to ship something that got slower."
---

Everything you've built so far, the requests, tests, data files and mock servers, lives in plain files in your workspace. The `zorvik` **command line tool** runs those same files without the app. A **command line**, or terminal, is a window where you type commands instead of clicking; build servers only have that.

## Getting it

`zorvik` is installed with the app. On Windows the installer puts it on your **PATH**, the list of places your terminal looks for programs, and the Linux packages put it in `/usr/bin`. On macOS, open **Settings → AI agents** and press **Add zorvik to PATH**. Then check it in a terminal:

```bash
zorvik --version
zorvik run --help
```

## Four commands

```bash
zorvik run ./my-api --env Staging                     # run requests with their scripts and tests
zorvik run ./my-api --folder Users -d users.csv --junit report.xml
zorvik load ./my-api "Checkout smoke" --html report.html
zorvik serve ./my-api "Payments mock" --port 3100     # start a saved mock or server
zorvik mcp                                            # the connection for AI agents (see the last lesson)
```

The first argument is always the **workspace folder**, the one with `zorvik.yaml` in it.

```anatomy
zorvik run ./my-api | run the requests of the workspace in ./my-api
--env Staging | use this environment's variables (by name)
--folder "Smoke tests" | only this folder, its path inside requests/
--var apiKey=$API_KEY | set a variable from outside; it beats every other value
-d users.csv | a data file, relative to where you run the command
--junit report.xml | also save a JUnit XML report for your CI
```

Other useful options: `-n 5` for five iterations, `--bail` to stop at the first failure, `--json` for a machine-readable report. `zorvik <command> --help` lists them all.

> [!warning] Secrets don't travel
> Secret values stay on the computer where you typed them, so a build server doesn't have them. Pass each one with `--var name=value`, taken from your CI system's secret store.

`zorvik load` runs a saved load test and checks its thresholds, and can save an HTML report. `zorvik serve` starts one of your saved servers, for example a mock of a payment provider, and prints its traffic until you press **Ctrl + C**. Run it next to your app's own tests, and they talk to the mock instead of the real thing.

## Exit codes

When a program ends, it hands back a number, its **exit code**. Scripts and CI servers read it to decide what happens next:

| Command | 0 | 1 | 2 |
|---|---|---|---|
| `zorvik run` | every request and test passed | a request or test failed | stopped, or could not start |
| `zorvik load` | every threshold passed | a threshold failed | the run failed |

```flow
zorvik run -> Exit code 0 -> Pipeline goes on
zorvik run -> Exit code 1 -> Pipeline stops, you get a red build
```

> [!note] Think of it like…
> A smoke detector. Nobody reads its logs; it either stays quiet or it beeps. An exit code is your tests beeping at the build server.

The output reads like the runner tab: a ✓ or ✗ per request, its tests below it, then a summary with the number of requests and tests that passed and failed.

**You'll use this when…** you want your API tests to run every night, or on every change, on a machine nobody sits at. One `zorvik run` line, and the red or green result lands where your team already looks.
