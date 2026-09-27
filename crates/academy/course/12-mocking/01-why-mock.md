---
id: why-mock
title: Why mock an API?
summary: A mock is a stand-in server that answers like the real API, so you can build and test without waiting for it.
minutes: 5
quiz:
  - question: The back-end team needs three more weeks for the `/orders` API. What lets the mobile team start today?
    options:
      - Wait until the real API is finished
      - Build against a mock that answers like the agreed API
      - Read the data straight from the production database
    answer: 1
    explain: A mock gives the app realistic answers right now. When the real API is ready, one variable points the app at it instead.
  - question: Why is a mock great for testing how an app handles errors?
    options:
      - It can be slow, answer 503 or drop the connection on command, without breaking a real server
      - It fixes the errors in your app for you
      - It hides every error from the app
    answer: 0
    explain: Breaking a real server on purpose is risky and hard to arrange. A mock misbehaves exactly when and how you tell it to.
  - question: What is the biggest risk of working with mocks?
    options:
      - Mocks are always slower than real servers
      - Mocks only run on Linux
      - The real API changes and the mock doesn't, so the app breaks for real users
    answer: 2
    explain: That's called drift. Generate mocks from the API's contract and check the real API against the same contract, so both stay in step.
---

Imagine the mobile team has to build the **Order history** screen this week, but the back-end team will only finish the `/orders` API next month. Waiting a month is not an option. So they build against a **mock**.

A **mock API** (or *mock server*) is a small stand-in server that answers requests the way the real API will: the same paths, the same status codes, the same shape of JSON. There is no database and no business logic behind it. It simply returns answers you prepared. You may also hear the words *stub* or *fake*; for this course they mean the same idea.

```sequence
participants: App, Mock, Real API
App -> Mock: GET /orders
Mock --> App: 200 OK [{"id": 1, "total": 18.5}]
Note over Real API: still being built
```

> [!note] Think of it like…
> A film set. From the camera's side the street looks real: shop windows, doors, signs. Open a door and there is nothing behind it. A mock is the same: from the app's side it looks like the real API, and that's all the app needs to get its work done.

## Why teams mock

1. **Front end before back end.** Once both teams agree on the shape of the API (the *contract*, often written as an OpenAPI document), both can start on the same day. The app talks to the mock; the back end is built in parallel.
2. **Testing failures safely.** What does your app do when the server is slow, answers `503 Service Unavailable` or drops the connection? With a real server you would have to break it to find out. A mock breaks on command, and only for you.
3. **Stable, fast tests.** Tests that call someone else's service (payments, maps, email) fail whenever that service has a bad day. A mock on your own computer answers the same way every time, in a few milliseconds.
4. **Demos and offline work.** A mock runs on your laptop at `127.0.0.1`. No Wi-Fi, no VPN and no test account needed.

## Swapping the mock for the real thing

Your requests use a variable such as `{{baseUrl}}` for the server address. Today it points at the mock; next month you change that one value and the same requests go to the real API.

```flow
App -[today]-> Mock API
App -[next month]-> Real API
```

## Mocks in Zorvik

Mocks live in the **Servers** section on the left rail. A mock is a list of **routes**: "when `GET /orders` comes in, answer `200` with this body". In this unit you will:

- build a mock by hand, one route at a time;
- make answers dynamic with templates such as `{{request.params.id}}`;
- add delays, errors, dropped connections and CORS for browser apps;
- generate a whole mock from an OpenAPI document or a folder of saved requests.

Mocks are saved in the workspace as plain files, like requests, so your whole team can run exactly the same one.

> [!warning] Mocks can drift
> A mock only knows what you told it. If the real API changes and the mock doesn't, the app keeps working against the mock and then breaks in production. Keep mocks close to the contract: generate them from the OpenAPI document, and check the real API against that same document (the capstone shows how).

**You'll use this when…** the back end isn't ready yet, a partner's test system is down again, or someone asks "what does the app do when payments time out?" and you want to show them in two minutes instead of two days.
