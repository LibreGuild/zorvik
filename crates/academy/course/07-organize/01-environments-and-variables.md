---
id: environments-and-variables
title: Environments and variables
summary: Write a request once, then point it at Dev or Staging just by switching the environment.
minutes: 5
lab:
  title: One request, two servers
  goal: Create Dev and Staging environments and send the same request to each of them.
  minutes: 8
  servers:
    dev:
      name: Dev API
      kind: http
      http:
        routes:
          - method: GET
            path: /status
            headers:
              - key: Content-Type
                value: application/json
            body: '{"environment": "dev", "version": "2.1.0-beta", "healthy": true}'
    staging:
      name: Staging API
      kind: http
      http:
        routes:
          - method: GET
            path: /status
            headers:
              - key: Content-Type
                value: application/json
            body: '{"environment": "staging", "version": "2.0.3", "healthy": true}'
  steps:
    - text: |
        Press **⌘/Ctrl + E** to open **Environments & variables**. Click **+** next to *Environments*, rename the new environment to **Dev**, and add one variable: `baseUrl`, with the Dev server's address `{{=dev}}` as its value. Press **Save changes**.
      hints:
        - An environment is a named list of variables. Dev needs just one, called baseUrl.
        - In the dialog, click + next to "Environments", type Dev in the name field at the top, then type baseUrl in the Variable column and the address in the Value column.
        - "Value: the address itself, starting with http://127.0.0.1: and no slash at the end. Not a variable: once Dev is active, the Lab environment is switched off. Then press Save changes."
      check:
        saved:
          environment: { name: Dev, variables: [{ key: baseUrl, value: "{{dev}}*" }] }
      solution:
        - call: { method: env.create, params: { environment: { name: Dev, variables: [{ key: baseUrl, value: "{{dev}}" }] } } }
        - call: { method: env.save, params: { id: Dev, environment: { name: Dev, variables: [{ key: baseUrl, value: "{{dev}}" }] } } }
    - text: With **Dev** selected in the dialog, click **Duplicate**. Rename the copy to **Staging**, change its `baseUrl` to the Staging server's address, `{{=staging}}`, and save.
      hints:
        - Duplicate copies every variable, so you only change what is different.
        - The Duplicate button is at the top right of the dialog, next to Delete. The copy is called "Dev copy".
        - "Rename \"Dev copy\" to Staging, set baseUrl to the Staging server's address from the step above (same host, different port), then Save changes."
      check:
        saved:
          environment: { name: Staging, variables: [{ key: baseUrl, value: "{{staging}}*" }] }
      solution:
        - call: { method: env.create, params: { environment: { name: Staging, variables: [{ key: baseUrl, value: "{{staging}}" }] } } }
        - call: { method: env.save, params: { id: Staging, environment: { name: Staging, variables: [{ key: baseUrl, value: "{{staging}}" }] } } }
    - text: Close the dialog. Pick **Dev** in the environment menu at the top right of the window, then send `GET {{baseUrl}}/status` from a new request (**⌘/Ctrl + N**).
      hints:
        - The environment menu shows the active environment's name (right now it says Lab).
        - Click it, choose Dev, then press ⌘N and type {{baseUrl}}/status in the URL bar. Hover {{baseUrl}} to see its value.
        - "Environment: Dev. Method GET, URL {{baseUrl}}/status, then Send. The answer says \"environment\": \"dev\"."
      check:
        send: { request: { url: "{{baseUrl}}/status*" }, json: { environment: dev } }
      solution:
        - call: { method: env.setActive, params: { id: Dev } }
        - send: { method: GET, url: "{{baseUrl}}/status" }
    - text: Now switch the environment menu to **Staging** and send the very same request again, without changing it.
      hints:
        - Only the environment changes. The URL still says {{baseUrl}}/status.
        - Click the environment menu at the top right, choose Staging, then press Send.
        - "The answer should now say \"environment\": \"staging\". Same request, different server."
      check:
        send: { request: { url: "{{baseUrl}}/status*" }, json: { environment: staging } }
      solution:
        - call: { method: env.setActive, params: { id: Staging } }
        - send: { method: GET, url: "{{baseUrl}}/status" }
quiz:
  - question: "`baseUrl` is set in the workspace variables and in the active environment, with different values. Which one does Zorvik use?"
    options:
      - The workspace variable, because it was there first
      - The active environment's value
      - "Neither: Zorvik reports a conflict"
    answer: 1
    explain: The more specific place wins. The active environment beats workspace variables, which beat globals.
  - question: Why put the server address in a `baseUrl` variable instead of typing it into every request?
    options:
      - Variables make requests faster
      - Servers refuse addresses that are typed out
      - To move every request to another server by changing one value or switching the environment
    answer: 2
    explain: Your requests stay the same; the environment decides where they go. Changing servers is one click instead of fifty edits.
  - question: You send `{{baseUrl}}/users`, but the active environment has no `baseUrl`. What happens?
    options:
      - Zorvik stops and tells you the variable is not defined
      - The request goes to a random server
      - Zorvik sends it to your own computer
    answer: 0
    explain: An undefined variable in the URL stops the send with a clear message, instead of a confusing network error. Hover the variable to see where its value would come from.
---

Most APIs live in more than one place. There is a copy on your laptop, a **Dev** server your team shares, a **Staging** server that looks just like the real thing, and **Production**, the one real customers use. The requests are the same everywhere. Only the address changes.

## Variables

A **variable** is a name for a value. You write it in double curly braces, like `{{baseUrl}}`, and Zorvik swaps in the value just before the request leaves. Variables work in the URL, headers, body and auth.

```anatomy
GET {{baseUrl}}/status | what you write: a variable, then the path
GET http://127.0.0.1:5001/status | what is sent, with Dev active
GET http://127.0.0.1:5002/status | what is sent, with Staging active
```

## Environments

An **environment** is a named set of variables. *Dev* says `baseUrl` is one address, *Staging* says it is another. One environment is **active** at a time: you pick it in the environment menu at the top right of the window. Switch it, and every request follows.

```flow
GET baseUrl/status -[Dev active]-> Dev server
GET baseUrl/status -[Staging active]-> Staging server
```

> [!note] Think of it like…
> The contacts on your phone. You tap *Pizza place*, not its number. If you keep one contact list at home and another on holiday, the same button calls a different pizza place, and you never type a number again.

## Where variables live

Open **Environments & variables** with **⌘/Ctrl + E**:

- **Environments** hold values that differ per server, like `baseUrl` or a test account's id.
- **Workspace variables** are shared by every environment, like an API version.
- **Globals** are values that scripts set, available in every workspace.

Environments are small files in your workspace's `environments/` folder, so your team gets them through Git too. There are also **dynamic values** that are new on every send: `{{$uuid}}`, `{{$timestamp}}`, `{{$randomEmail}}` and a few more.

## Which value wins?

The same name can exist in several places. The most specific one wins, from top to bottom:

```layers
pm.variables | set by a script, for one send or one run
Data file row | a row of a data file, in a collection run
Active environment | Dev, Staging, Production…
Workspace variables | shared by every environment
Globals | set by scripts, for every workspace
```

On the command line, a `--var name=value` beats all of them. You'll meet scripts, data files and the command line later in the course.

> [!tip] Hover to check
> Hover a variable in the URL bar to see its value and where it comes from. A red one is not defined anywhere, and Zorvik won't send a URL with an undefined variable.

The lab gives you two practice servers that answer differently, so you can see exactly which one got your request.

**You'll use this when…** you test a new feature on Staging in the morning and check a bug on Production after lunch, with the same saved requests and a single click in between.
