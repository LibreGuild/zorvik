---
title: Training Bootcamp
description: Learn networks and APIs inside Zorvik with the Academy's lessons, hands-on labs, the Lab Guide, hints, XP, badges and a graduation certificate.
sidebar:
  order: 1
---

The **Training Bootcamp** is a course built into Zorvik, from "what is a network?" to testing, mocking and load testing APIs. Each lesson is a short reading with diagrams, a hands-on **lab** you do in the real workbench against practice servers on your own computer, and a quick check. You need no account and no internet connection for it.

![The Welcome unit's artwork](/zorvik/art/unit-welcome.webp)

## Open the Academy

Choose **Training Bootcamp** at the top of the workspace menu, or on the welcome screen. It opens the Bootcamp workspace in **Academy** mode.

The Bootcamp workspace is the only one with a **Workbench | Academy** switch in the title bar:

- **Academy**: the course: your level, the course map, lessons, badges.
- **Workbench**: the normal Zorvik app, where you do the labs.

The Academy's own menu (**…** at the top right) has **Badges and certificate**, **Open the workbench** and **Reset Bootcamp workspace…**.

## The course

The course has 16 units and 65 lessons; 60 lessons have a lab.

| # | Unit | Lessons | Badge |
|---|---|---|---|
| 0 | Welcome to Bootcamp | 3 | First Contact |
| 1 | How networks talk | 4 | Packet Pioneer |
| 2 | DNS: the internet's address book | 4 | DNS Detective |
| 3 | HTTP basics | 5 | Status Sage |
| 4 | Sending data | 5 | Payload Pro |
| 5 | Identity & auth | 5 | Token Tamer |
| 6 | Secure transport | 4 | TLS Guardian |
| 7 | Organized like a pro | 4 | Workspace Architect |
| 8 | Testing APIs | 5 | Test Pilot |
| 9 | Beyond REST: GraphQL & gRPC | 4 | Query Crafter |
| 10 | Real-time: WebSocket, SSE, MQTT | 4 | Stream Surfer |
| 11 | Raw sockets: TCP & UDP | 4 | Socket Smith |
| 12 | Mocking APIs | 5 | Mock Master |
| 13 | Performance & load testing | 4 | Load Legend |
| 14 | Automate: CLI, CI and AI agents | 3 | Automation Ace |
| 15 | Capstone: Ship it | 2 | Graduate |

The **course map** lists the units; open one to see its lessons, each with its reading time, lab time and number of questions, and a tick when done. A card at the top says where to go next: **Start the Bootcamp** at first, then **Continue** with the next lesson. You can open any lesson in any order.

## A lesson

A lesson page has three parts:

1. **The reading**: short sections in plain words, with sequence, flow, layer and anatomy diagrams, callouts and code. Variables such as `{{api}}` show their value while a lab runs.
2. **The lab** (most lessons): a card with the lab's title, goal and time, and **Start lab**. After you finish it once, the button says **Practice again**, and "Lab done ✓" shows.
3. **The quick check**: usually three multiple-choice questions. Each answer says **Right!** or **Not quite.** with a short explanation. **Try again** lets you answer once more; your best score counts.

A lesson is **complete** when its lab is done (if it has one) **and** its quick check is passed with at least 60 % right answers, rounded up (2 of 3) (if it has one). A lesson with neither has a **Mark as done** button. When a lesson is complete, **Next lesson** takes you on.

## Labs

**Start lab** switches to the workbench and prepares everything the lab needs:

- **Lab servers**: mock APIs and other servers the lab needs are saved into the Bootcamp workspace as **Lab · *name*** and started on free ports. Open them in the Servers sidebar to see their routes and live traffic.
- **The Lab environment**: an environment named **Lab** is filled in and made active. It holds the servers' addresses (`{{api}}`, and `{{api_host}}`, `{{api_port}}` for a server named `api`), the practice playground (`{{playground}}`), and anything else the lab needs.
- **Practice servers**: a local playground (echo, status codes, redirects, delays, cookies, auth, JSON, SSE, WebSocket, GraphQL, OAuth 2.0), an HTTPS version with a certificate from a private authority, and a gRPC server. They start the first time a lab needs them and stay up.
- **Files** some labs need, such as a CSV data file, written into the workspace.
- **Secret words**: some labs generate a random value when they start (such as `falcon-4821`), so answers can't be copied from someone else.

Starting a lab stops the servers you ran in earlier labs (they stay saved), removes the previous lab's servers, and clears values scripts saved in the Lab environment, so every lab starts clean. Cookies are shared with the rest of the workspace.

### The Lab Guide

While a lab runs, the **Lab Guide** is docked on the right of the workbench:

- The lab's title and goal, "Step 2 of 5", and a progress bar.
- The steps, in order. The current one says what to do and "Waiting for you to do it in the workbench…".
- **The Lab environment** at the bottom: its variables with a copy button, and the lab's servers (click one to open it and its traffic).
- The menu (**…**): **Back to the lesson**, **Check again** and **Stop lab**.
- The fold button collapses the guide to a thin bar ("Lab 2/5"); click it to open the guide again.

You just work in the app: send the request, save the environment, start the mock, run the load test. Zorvik notes what you do and **checks the current step after every action and once a second**, then ticks it off the moment you get it right. Some steps look at what a lab server received, some at a request you sent or saved, a finished collection run or load test, or a server you started. Steps that ask a question have an answer box: type the answer and press **Check** ("Not that one. Look again, or open a hint." when it's wrong). Steps that call a server you run yourself have a **Check now** button.

Going back to the Academy doesn't stop the lab: the course map shows "Lab in progress" with **Back to the lab**, and the lesson shows **Continue the lab**.

### Hints and "Do it for me"

Nobody gets stuck:

- **Hint** opens the next hint of the current step, up to three. They go from a nudge, to where to click, to exactly what to do; the last one is labelled **Show me how**.
- **Do it for me** runs the step for you (it sends the request, saves the file, or types the answer) and ticks it off. That step earns no XP.

## XP, levels and ranks

| You earn | XP |
|---|---|
| A lab step you did yourself (each step once) | 10 |
| A lesson completed | 50 |
| A quick-check answer right on the first attempt (the first attempt only) | 5 each |
| Every answer of a quick check right on the first attempt (3 questions or more) | +20 |
| A unit completed, or tested out of | 100 |
| Graduating (finishing the capstone unit) | 250 |

Your **level** follows your XP: level *L* starts at 20 × (*L* − 1) × *L* XP.

| Level | Starts at | Level | Starts at |
|---|---|---|---|
| 2 | 40 XP | 6 | 600 XP |
| 3 | 120 XP | 10 | 1,800 XP |
| 4 | 240 XP | 14 | 3,640 XP |
| 5 | 400 XP | 18 | 6,120 XP |

**Ranks** come with levels: Newbie Node (level 1), Packet Pusher (3), Header Hacker (6), Protocol Pro (10), Network Ninja (14) and Wire Wizard (18). The Academy's header shows your rank, a level ring and the XP to the next level. A celebration pops up for a completed lesson, a new level, a new badge and graduation.

## Streaks

Your **streak** counts the days in a row on which you earned XP, by your computer's local date. Miss a day and it starts again at 1; the header says "learn today to keep it" when you haven't earned XP yet today. Your best streak is kept too.

## Badges

There are 24 badges: one per unit (see [the course](#the-course)) and eight for how you learn.

| Badge | How to earn it |
|---|---|
| No Hints Needed | Finish a lab without a hint (and without "Do it for me"). |
| Perfect Score | Answer every question of a quick check right the first time. |
| Speed Runner | Finish a lab of three steps or more in under two minutes, without "Do it for me". |
| Night Owl | Complete a lesson between midnight and 5 am. |
| Early Bird | Complete a lesson between 5 and 8 am. |
| On Fire | Learn three days in a row. |
| Unstoppable | Learn seven days in a row. |
| Halfway There | Complete half of the lessons. |

**Badges and certificate** (the badge counter in the header, or the Academy menu) shows every badge, when you earned it, and how to earn the others.

## Test out of a unit

Already know a topic? A unit card offers **Test out** ("Know this already? Pass a short quiz to earn the *badge* badge."). The test has up to eight questions taken from the unit's lessons. Get **80 %** right to earn the unit's badge and its 100 XP. The unit's lessons stay open to read and practise; they are not marked done. You can try again as often as you like. The capstone can't be tested out of: it is a project.

## Graduate

Finish the capstone unit to graduate: 250 XP, the **Graduate** badge, and a **Zorvik Bootcamp Graduate** certificate. The course map then shows "You graduated from the Bootcamp!" with **View certificate**.

The certificate, on the **Badges and certificate** page, states that you completed all units and lessons of the Zorvik Training Bootcamp, with your level and rank, your XP, your badge count and the graduation date. **Save as image** saves it as a PNG file.

## The Bootcamp workspace

The Bootcamp has its own workspace, kept in the app data folder (`bootcamp/`, see [Data locations](../../reference/data-locations/)). It is always listed first, it can't be removed, and it never mixes with your own workspaces.

**Reset Bootcamp workspace…** in the Academy menu empties it: its requests, environments and servers are removed and a running lab stops. **Your progress, XP and badges stay**: they are kept separately, in `academy-progress.json` in the app data folder.

## Good to know

- Every lab can be done again (**Practice again**). Steps already done earn no XP a second time.
- Labs use practice servers on `127.0.0.1`; your own workspaces and servers are not touched (servers you started in earlier *labs* are stopped when a new lab starts).
- Keyboard shortcuts of the workbench are off while the Academy view is shown.
- Want to write a lesson? See [Writing lessons](../writing-lessons/).
