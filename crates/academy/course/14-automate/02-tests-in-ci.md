---
id: tests-in-ci
title: Tests in CI pipelines
summary: Run your API tests on every change with a CI pipeline, and let the result decide whether the change ships.
minutes: 6
lab:
  title: Fail fast
  goal: Run a release gate twice, once to the end and once stopping at the first failure, and see why stopping early helps.
  minutes: 6
  servers:
    api:
      name: Orders API
      kind: http
      http:
        routes:
          - method: GET
            path: /health
            headers:
              - key: Content-Type
                value: application/json
            body: '{"status": "ok"}'
          - method: POST
            path: /orders
            status: 500
            headers:
              - key: Content-Type
                value: application/json
            body: '{"error": "The orders database is read-only"}'
          - method: POST
            path: /orders/latest/pay
            status: 404
            headers:
              - key: Content-Type
                value: application/json
            body: '{"error": "No such order"}'
  files:
    requests/Release gate/_folder.yaml: |
      name: Release gate
      seq: 92
    requests/Release gate/Health.yaml: |
      name: Health
      seq: 1
      method: GET
      url: {{api}}/health
      scripts:
        postResponse: |
          pm.test("Service is up", () => {
            pm.response.to.have.status(200);
          });
    requests/Release gate/Create order.yaml: |
      name: Create order
      seq: 2
      method: POST
      url: {{api}}/orders
      body:
        type: json
        text: '{"item": "MUG-1", "qty": 2}'
      scripts:
        postResponse: |
          pm.test("Order created", () => {
            pm.response.to.have.status(201);
          });
    requests/Release gate/Pay order.yaml: |
      name: Pay order
      seq: 3
      method: POST
      url: {{api}}/orders/latest/pay
      scripts:
        postResponse: |
          pm.test("Order paid", () => {
            pm.response.to.have.status(200);
          });
  steps:
    - text: The lab added a folder **Release gate**. Right-click it → **Run…** and press **Run**. How many requests fail, and do both failures tell you something new?
      hints:
        - Create order fails first. Look at what that does to Pay order.
        - Right-click Release gate in the sidebar, choose Run…, then press Run or ⌘Enter.
        - "Two failures: Create order gets a 500, then Pay order gets a 404 because there is no order to pay. Only the first one is the real problem."
      check:
        run: { name: Release gate, requests: 3, failed: 2 }
      solution:
        - call: { method: runner.start, params: { folder: Release gate } }
        - wait: 500
    - text: In the runner tab, switch on **Stop on the first failure** and press **Run** again.
      hints:
        - The switch is in the run settings, below the data file.
        - Turn on Stop on the first failure, then Run.
        - "The run stops right after Create order: one failure, the real one, and Pay order never runs."
      check:
        run: { name: Release gate, bailed: true, requests: 2, failed: 1 }
      solution:
        - call: { method: runner.start, params: { folder: Release gate, stopOnFailure: true } }
        - wait: 500
    - text: Which `zorvik run` option does the same thing in a pipeline? Type it.
      hints:
        - It's in the list of options from the previous lesson, and in zorvik run --help.
        - Bailing out means giving up early.
        - "It's --bail."
      check:
        answer: ["--bail", "bail", "zorvik run --bail"]
      solution:
        - answer: "--bail"
quiz:
  - question: Why save a JUnit report in the pipeline?
    options:
      - JUnit reports make the tests run faster
      - Zorvik can't run without one
      - The CI system reads it and shows every test by name on the build page, failures first
    answer: 2
    explain: JUnit XML is the format most CI systems understand. The exit code says pass or fail; the report says which test and why.
  - question: A release gate creates an order, then pays for it. Why run it with `--bail`?
    options:
      - So the run stops at the first failure, and the real cause isn't buried under failures it caused
      - So failed tests are ignored
      - So the run repeats until it passes
    answer: 0
    explain: If creating the order fails, paying for it must fail too. Stopping early is faster, and the build shows the one failure that matters.
  - question: The pipeline needs your `apiKey`. Where should it come from?
    options:
      - Typed into the workflow file
      - The CI system's secret store, handed to `zorvik run` with `--var`
      - A plain variable in the environment file
    answer: 1
    explain: The workflow file and the environment files are in Git, where anyone with access can read them. CI secret stores keep the value hidden, even in logs.
---

A **CI pipeline** (continuous integration) is a list of steps that a server runs by itself every time someone changes the code: build it, test it, maybe deploy it. If a step fails, the pipeline stops and the change is marked red. API tests are a perfect step: they catch a broken endpoint before anyone merges it.

## The recipe

Every pipeline that runs Zorvik tests does four things:

1. **Get the workspace.** It's in Git with your code, so the pipeline checks out the repository.
2. **Install `zorvik`.** On Linux build machines, install the `.deb` from the releases page.
3. **Run the tests** with the right environment, and secrets from the CI system's **secret store**.
4. **Keep the report,** so everyone can see which test failed and why.

```flow
Push a change -> CI server -> zorvik run -> Exit code 0 -> Merge allowed
zorvik run -> Exit code 1 -> Build turns red
```

Here is the whole thing on **GitHub Actions**, in a file like `.github/workflows/api-tests.yml`:

```yaml
name: API tests
on: [push, pull_request]

jobs:
  api-tests:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Install Zorvik
        run: |
          curl -fsSLO https://github.com/LibreGuild/zorvik/releases/latest/download/Zorvik-Linux-amd64.deb
          sudo apt-get update
          sudo apt-get install -y ./Zorvik-Linux-amd64.deb
      - name: Run the API tests
        env:
          API_KEY: ${{ secrets.STAGING_API_KEY }}
        run: zorvik run ./api-tests --env Staging --var apiKey="$API_KEY" --bail --junit report.xml
      - name: Keep the report
        if: always()
        uses: actions/upload-artifact@v4
        with:
          name: api-test-report
          path: report.xml
```

GitLab CI, Jenkins, Azure Pipelines and others follow the same four steps with their own syntax.

> [!note] Think of it like…
> The safety check at the end of a car assembly line. Every car goes through it, no exceptions, and a car that fails doesn't leave the factory, however much of a hurry anyone is in.

## Reports and exit codes

- The **exit code** decides: `0` lets the pipeline go on, `1` stops it.
- The **JUnit XML** report explains: most CI systems read it and list every test by name. Zorvik writes one test suite per request and one test case per `pm.test`. You can also export any run from the app's runner tab (**Export → JUnit XML…**).

## Good habits

- **Fail fast** with `--bail` when later requests depend on earlier ones, like creating an order before paying for it.
- **No fixed waits.** Use **Repeat until** for slow jobs, so the pipeline isn't flaky.
- **Mock what you don't own.** Start a saved mock with `zorvik serve` in the pipeline, instead of calling a real payment provider on every push.
- **Pick the target carefully.** Test against a staging or test server; tests that create and delete data don't belong on production.

**You'll use this when…** a teammate opens a pull request that renames a field. The pipeline runs your collection, the contract check turns red, and the problem is fixed before it's merged, not after customers find it.
