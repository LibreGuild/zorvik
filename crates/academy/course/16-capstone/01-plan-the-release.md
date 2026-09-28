---
id: release-checklist
title: Plan the release
summary: Before an API goes live, a short checklist proves it matches its contract, works end to end, holds up under load and is ready for the teams that use it.
minutes: 5
quiz:
  - question: What is the API contract?
    options:
      - The legal terms of use on the company website
      - The list of servers the API runs on
      - A document, often OpenAPI, describing the paths, requests and answers that both sides agree on
    answer: 2
    explain: The contract is the shared promise. The back end must keep it, and the apps, mocks and tests are all built from it.
  - question: Why is a load test with thresholds part of the release checklist, not just "nice to have"?
    options:
      - It makes the API faster
      - It turns "is it fast enough?" into a clear pass or fail before real users arrive
      - It replaces all functional tests
    answer: 1
    explain: A threshold like "p95 under 500 ms" is a performance promise. Checking it before launch means you find problems on a test server, not in production.
  - question: The mobile team starts on Monday but the API launches in three weeks. What do you hand them?
    options:
      - A mock made from the contract, plus ready-made code snippets for their requests
      - Nothing; they should wait for the launch
      - The production database password
    answer: 0
    explain: A mock lets them build against realistic answers today, and snippets save them from retyping every request in their own language.
---

You've learned a lot: requests and responses, environments, tests, collections, mocks and load tests. In the real world, all of it comes together on one day: **release day**, when an API goes live and real apps start calling it.

Good teams don't hope a release goes well. They go through a **checklist**, and each item on it is something you can now do in Zorvik.

## The release checklist

```flow
Contract -> Collection -> Checks -> Smoke run -> Load test -> Hand-off
```

| # | Item | In Zorvik | Done when… |
|---|---|---|---|
| 1 | **Contract** | an OpenAPI document everyone agreed on | it's in the repository, reviewed |
| 2 | **Collection** | **Import…** the contract; an environment holds `baseUrl` | every operation is a saved request |
| 3 | **Contract checks** | the **Matches the API spec** test on every response | the real API gives the documented answers |
| 4 | **Smoke run** | **Run…** the folder | every request passes, in one click |
| 5 | **Performance** | a load test with thresholds | the run passes: p95 and error rate within goals |
| 6 | **Hand-off** | a mock from the contract, and **Copy as cURL or code…** | the app teams can start without asking you |

A few words on each:

- **Contract.** The OpenAPI document describes every path, parameter and answer. It's the promise the back end makes to the apps.
- **Collection and environments.** Importing the contract turns each operation into a saved request. The server address lives in a `baseUrl` variable, so the same requests work against a laptop, a *staging* server (a copy of production used for testing) or production.
- **Contract checks.** Requests imported from a document get a free test on every answer: **Matches the API spec**. It checks that the status is documented and that a JSON body has the documented shape. If the back end drifts from the contract, this test turns red.
- **Smoke run.** A *smoke test* is a quick run through the main features to see that nothing is on fire. Running the whole folder once is exactly that.
- **Performance.** A short load test with thresholds turns "is it fast enough?" into a green or red answer.
- **Hand-off.** App teams get a mock to build against and code snippets in their own language: Kotlin for Android, Swift for iOS, JavaScript for the web, Python for scripts.

> [!note] Think of it like…
> A pilot's pre-flight checklist. The pilot has flown a thousand times and still checks the fuel, the flaps and the instruments, every single flight, in the same order. Not because they forget, but because a checklist makes "I think it's fine" into "I checked it's fine".

## Keep it repeatable

The checklist isn't a one-time ceremony. Keep the contract, the collection, the load test and the mock in the workspace, and keep the workspace in Git. Next release, you run the same checks again in minutes, and anything that got worse shows up in red.

> [!tip] Staging first, production last
> Run every check against a staging server. Point the checklist at production only for the smoke run after launch, and never load test production without a plan and permission.

In the next lesson you'll go through this whole checklist for a real (well, almost real) launch. It's your final mission.

**You'll use this when…** your team says "we launch on Thursday" and you're the one who can say, with evidence, "it's ready".
