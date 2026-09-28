---
id: authn-vs-authz
title: Authentication vs authorization
summary: Authentication proves who you are (401 when it fails); authorization decides what you may do (403 when it says no).
minutes: 5
lab:
  title: Three visitors, three answers
  goal: Call the same endpoint as nobody, as a regular user and as an admin, and read what each status means.
  minutes: 6
  vars:
    userToken: tok-user-7f3a
    adminToken: tok-admin-91c2
  servers:
    api:
      name: Reports API
      kind: http
      http:
        routes:
          - name: Admin
            method: GET
            path: /reports
            matchHeaders:
              - { key: Authorization, value: Bearer tok-admin-91c2 }
            headers:
              - { key: Content-Type, value: application/json }
            body: '{"reports": [{"id": 1, "title": "Q3 revenue"}, {"id": 2, "title": "Payroll"}]}'
          - name: Regular user
            method: GET
            path: /reports
            status: 403
            matchHeaders:
              - { key: Authorization, value: Bearer tok-user-7f3a }
            headers:
              - { key: Content-Type, value: application/json }
            body: '{"error": "forbidden", "message": "We know who you are, but your account may not see reports."}'
          - name: Nobody
            method: GET
            path: /reports
            status: 401
            headers:
              - { key: Content-Type, value: application/json }
              - { key: WWW-Authenticate, value: 'Bearer realm="reports"' }
            body: '{"error": "unauthorized", "message": "Who are you? Send a token in the Authorization header."}'
  steps:
    - text: Send `GET {{api}}/reports` without logging in. Read the status.
      hints:
        - A new request has no credentials unless a folder or the workspace gives it some.
        - Open a new request (⌘/Ctrl + N), type the URL and press Send.
        - "Method GET, URL {{api}}/reports, then Send. You should get 401."
      check:
        request: { server: api, method: GET, path: /reports, status: 401 }
      solution:
        - send: { method: GET, url: "{{api}}/reports" }
    - text: The 401 response has a header that tells a client *how* to log in. What is its name?
      hints:
        - Open the response's **Headers** tab.
        - It's the header whose value starts with `Bearer`.
      check:
        answer: ["WWW-Authenticate", "WWW-Authenticate:*"]
      solution:
        - answer: WWW-Authenticate
    - text: |
        Now send the same request as a regular user. In the **Auth** tab choose **Bearer token** and put `{{userToken}}` in **Token**.
      hints:
        - The lab put two tokens in the Lab environment. Type the variable, braces included; Zorvik fills in its value.
        - "Auth tab: Type = Bearer token, Token = {{userToken}}. Leave Prefix as Bearer."
        - "Then Send: the server should answer 403."
      check:
        request: { server: api, method: GET, path: /reports, status: 403, headers: { authorization: "Bearer {{userToken}}" } }
      solution:
        - send: { method: GET, url: "{{api}}/reports", auth: { type: bearer, token: "{{userToken}}", prefix: Bearer } }
    - text: Switch the token to `{{adminToken}}` and send again.
      hints:
        - Only the token changes; the request stays the same.
        - "Auth tab: Token = {{adminToken}}, then Send. You should get 200 and a list of reports."
      check:
        request: { server: api, method: GET, path: /reports, status: 200, headers: { authorization: "Bearer {{adminToken}}" } }
      solution:
        - send: { method: GET, url: "{{api}}/reports", auth: { type: bearer, token: "{{adminToken}}", prefix: Bearer } }
quiz:
  - question: You send a request with an expired token. Which status fits best?
    options:
      - 401 Unauthorized
      - 403 Forbidden
      - 404 Not Found
    answer: 0
    explain: The server can't tell who you are anymore, so authentication failed. Log in again, then retry.
  - question: You are logged in correctly, but your account may not delete invoices. Which status do you expect?
    options:
      - 401 Unauthorized
      - 500 Internal Server Error
      - 403 Forbidden
    answer: 2
    explain: The server knows who you are and says no. Sending the same credentials again won't help; you need different permissions.
  - question: Why do some APIs answer 404 instead of 403 for things you may not see?
    options:
      - Because 404 is faster for the server to send
      - To avoid revealing that the thing exists at all
      - Because 403 only works over HTTPS
    answer: 1
    explain: "\"Forbidden\" confirms something is there. For private data, some APIs prefer to say \"nothing here\" to strangers."
---

Almost every real API wants to know who is calling. Behind that question are two separate checks, and mixing them up is the most common source of confusion when a request is refused.

- **Authentication** (often shortened to **authn**): *who are you?* You prove your identity with a password, a key or a token. The proof you send is called a **credential**.
- **Authorization** (**authz**): *what may you do?* Once the server knows who you are, it checks your permissions: may this user read reports, delete invoices, see other people's data?

```flow
Request -> Who are you? -[unknown]-> 401 Unauthorized
Who are you? -[known]-> May you do this? -[no]-> 403 Forbidden
May you do this? -[yes]-> 200 OK
```

> [!note] Think of it like…
> An office building. At the front desk you show your badge: that's authentication. Your badge opens some doors but not others: that's authorization. No badge, and you don't get past the desk (401). A valid badge at the server-room door that isn't on your list: the door stays shut (403).

## 401 Unauthorized: "who are you?"

Despite its name, **401** is about *authentication*. The server doesn't know who you are: you sent no credentials, or they are wrong or expired. A 401 response usually includes a **`WWW-Authenticate`** header that names the method the server expects, such as `Bearer` or `Basic`. That's a big hint when the documentation is thin.

**What to do:** check that credentials are attached at all, that they aren't expired, and that they're for this server (a staging token won't work in production).

## 403 Forbidden: "I know you, and no"

**403** means authentication worked but authorization didn't. Sending the same credentials again won't change anything. You need an account with more permissions, or the operation isn't meant for you.

**What to do:** check which user or token you're using, and what role or scopes it has. Scopes are permissions written into a token; you'll decode one later in this unit.

| Status | Question that failed | Retry with the same credentials? |
|---|---|---|
| 401 | Who are you? | No: fix or refresh them first |
| 403 | May you do this? | No: use an account that is allowed |

> [!tip] 404 can mean "not for you"
> Some APIs answer 404 for things you may not see, so strangers can't even learn they exist. If a colleague sees an item and you get a 404, suspect permissions.

## Credentials in Zorvik

The **Auth** tab of a request holds its credentials. By default a request says **Inherit from parent**: it uses the auth of its folder, or of the workspace. That way you set a login once and every request inside uses it. In the lab you'll set a **Bearer token** on one request; the next lessons cover each kind of auth in turn.

**You'll use this when…** a request that worked yesterday fails today. A 401 sends you to check the token; a 403 sends you to check the user's role. Knowing which question failed saves an hour of guessing.
