---
id: save-your-work
title: Save your work
summary: Saved requests live in your collection; history remembers everything you sent.
minutes: 3
lab:
  title: Build your first collection
  goal: Save a request, then find it again in your collection and in history.
  minutes: 6
  servers:
    api:
      name: Greetings API
      kind: http
      http:
        routes:
          - method: GET
            path: /greetings
            headers:
              - key: Content-Type
                value: application/json
            body: '[{"lang": "en", "text": "Hello"}, {"lang": "es", "text": "Hola"}, {"lang": "hi", "text": "Namaste"}]'
  steps:
    - text: Create a request for `{{api}}/greetings` and **save it** (⌘/Ctrl + S) with the name **Greetings**.
      hints:
        - Saving asks for a name and a folder. The top of the collection is fine.
        - Press ⌘N, type {{api}}/greetings, then ⌘S and name it Greetings.
      check:
        saved:
          request: { name: Greetings, method: GET, url: "*/greetings" }
      solution:
        - save: { name: Greetings, method: GET, url: "{{api}}/greetings" }
    - text: Open **Greetings** from the collection in the sidebar and send it.
      hints:
        - The Collection section is the first icon on the left rail.
        - Click Greetings in the sidebar, then press Send.
      check:
        request: { server: api, method: GET, path: /greetings }
      solution:
        - send: { name: Greetings, method: GET, url: "{{api}}/greetings" }
    - text: How many greetings did the server send back? Type the number.
      hints:
        - Count the items between the square brackets.
      check:
        answer: ["3", "three"]
      solution:
        - answer: "3"
quiz:
  - question: Where does a saved request go?
    options:
      - Into the collection, as a file in the workspace folder
      - Only into history
      - Nowhere until you export it
    answer: 0
    explain: A workspace is a folder of plain files you can keep in Git and share with your team.
  - question: You sent a request an hour ago but never saved it. Where can you find it?
    options:
      - It's gone
      - In History
      - In the Servers section
    answer: 1
    explain: History keeps every request you send, saved or not.
  - question: Which shortcut saves the request in the current tab?
    options:
      - ⌘/Ctrl + S
      - ⌘/Ctrl + N
      - ⌘/Ctrl + Enter
    answer: 0
    explain: ⌘/Ctrl + N opens a new request and ⌘/Ctrl + Enter sends it.
---

Typing the same request again and again gets old fast. Zorvik gives you two memories:

- **Collection:** the requests you **save**, organized in folders. Each saved request is a small file in the workspace folder, so you can keep it in Git and share it.
- **History:** every request you **send**, saved or not, with its status and time.

```flow
New tab -> Send -> Save (⌘S) -> Collection
Send -> History
```

## Tabs

Each request opens in a **tab**, like a web browser. A dot on a tab means it has changes you haven't saved yet. Close a tab with **⌘/Ctrl + W**.

## Folders

Right-click the collection to create folders, for example one folder per API or per feature. Folders can also hold settings every request inside shares, like a login. You'll use that in the auth unit.

> [!tip] Find anything
> Press **⌘/Ctrl + K** and start typing a request's name to jump to it.

**You'll use this when…** a teammate asks "which request did you use to reproduce that bug?" You send them the saved request, not a screenshot.
