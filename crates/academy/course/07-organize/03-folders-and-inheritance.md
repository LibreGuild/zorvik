---
id: folders-and-inheritance
title: Folders that share settings
summary: Set auth and headers once on a folder, and every request inside inherits them.
minutes: 5
lab:
  title: Log in once for a whole folder
  goal: Give a folder a token and a header, let its requests inherit them, and override the auth on one request.
  minutes: 8
  vars:
    adminToken: "{{secret.token}}"
  servers:
    api:
      name: Admin API
      kind: http
      http:
        routes:
          - method: GET
            path: /admin/users
            matchHeaders:
              - key: Authorization
                value: "Bearer {{secret.token}}"
            headers:
              - key: Content-Type
                value: application/json
            body: '[{"id": 1, "name": "Ada", "role": "owner"}, {"id": 2, "name": "Linus", "role": "member"}]'
          - method: GET
            path: /admin/stats
            matchHeaders:
              - key: Authorization
                value: "Bearer {{secret.token}}"
            headers:
              - key: Content-Type
                value: application/json
            body: '{"users": 2, "ordersToday": 17}'
          - method: "*"
            path: /admin/*
            status: 401
            headers:
              - key: Content-Type
                value: application/json
            body: '{"error": "Missing or wrong token"}'
  steps:
    - text: |
        Create a folder named **Admin** (the **+** at the top of the collection → **New folder**). Right-click it → **Folder settings…**. On **Auth**, choose **Bearer token** with the token `{{adminToken}}`. On **Headers**, add `X-Tenant` with the value `acme`. Press **Save**.
      hints:
        - The Lab environment already has the token in a variable called adminToken, so you only type {{adminToken}}.
        - Folder settings has tabs for Auth, Headers, Scripts and Docs. Set both Auth and Headers before you press Save.
        - "Auth → Type: Bearer token, Token: {{adminToken}}. Headers → X-Tenant = acme. Then Save."
      check:
        saved:
          folder: { name: Admin, auth: { type: bearer }, headers: [{ key: X-Tenant, value: acme }] }
      solution:
        - call: { method: folder.create, params: { parent: "", name: Admin } }
        - call:
            method: folder.save
            params:
              path: Admin
              meta:
                name: Admin
                auth: { type: bearer, token: "{{adminToken}}", prefix: Bearer }
                headers: [{ key: X-Tenant, value: acme }]
    - text: Right-click **Admin** → **New HTTP request**, name it **List users**, and set its URL to `{{api}}/admin/users`. Leave its **Auth** tab on **Inherit from parent**, save it and press **Send**.
      hints:
        - A request only inherits from a folder it is saved in. The new request is created inside Admin, so you're set.
        - Don't add any auth or headers to the request. Look at its Auth tab; it says Inherit from parent.
        - "URL {{api}}/admin/users, Auth: Inherit from parent, then Send. The answer is 200 with two users."
      check:
        all:
          - request: { server: api, path: /admin/users, status: 200, headers: { authorization: "Bearer {{secret.token}}", x-tenant: acme } }
          - send: { url: "*/admin/users", auth: inherit, status: 200 }
      solution:
        - call: { method: request.create, params: { parent: Admin, request: { name: List users, method: GET, url: "{{api}}/admin/users" } } }
        - call:
            method: http.send
            params:
              requestId: lab-admin-users
              request: { name: List users, method: GET, url: "{{api}}/admin/users" }
              path: Admin/List users.yaml
    - text: Add a second request in **Admin**, **Stats**, with the URL `{{api}}/admin/stats`. This time set its **Auth** to **No auth** and send it. What does the server say?
      hints:
        - A request's own auth always beats the folder's. No auth means no Authorization header at all.
        - Right-click Admin → New HTTP request, name it Stats, then open its Auth tab and pick No auth.
        - "URL {{api}}/admin/stats, Auth: No auth, then Send. You get 401: the X-Tenant header still came from the folder, the token did not."
      check:
        all:
          - request: { server: api, path: /admin/stats, status: 401, headers: { x-tenant: acme, authorization: "!*" } }
          - send: { url: "*/admin/stats", auth: none, status: 401 }
      solution:
        - call: { method: request.create, params: { parent: Admin, request: { name: Stats, method: GET, url: "{{api}}/admin/stats", auth: { type: none } } } }
        - call:
            method: http.send
            params:
              requestId: lab-admin-stats
              request: { name: Stats, method: GET, url: "{{api}}/admin/stats", auth: { type: none } }
              path: Admin/Stats.yaml
quiz:
  - question: A request's Auth is **Inherit from parent**. Its folder has a Bearer token and the workspace has Basic auth. What is sent?
    options:
      - Both, one after the other
      - The folder's Bearer token, because it is the nearest parent
      - The workspace's Basic auth, because it is the default
    answer: 1
    explain: "Zorvik looks upward and uses the first auth that isn't Inherit from parent: the request's, then its folders' from the inside out, then the workspace's."
  - question: "The folder sends `X-Tenant: acme`, and one request inside it sets `X-Tenant: globex`. Which value goes out with that request?"
    options:
      - globex, because the request's own header replaces the inherited one
      - acme, because folders win
      - Both headers are sent
    answer: 0
    explain: Headers add up from the workspace to the folders to the request, and a header with the same name replaces the one inherited from above.
  - question: You made a new request in a tab but never saved it into the Admin folder. Does it get the folder's token?
    options:
      - Yes, every request in the workspace does
      - Yes, if the URL matches the folder's name
      - No, a request inherits only from the folders it is saved in
    answer: 2
    explain: Inheritance follows where the request lives. Save it into the folder (or create it there) and it picks up the folder's auth, headers and scripts.
---

As a collection grows, the same settings show up again and again: the same token on twenty admin requests, the same `X-Tenant` header on all of them. Copy it twenty times and one day you'll update nineteen. **Folders** fix that.

## Folders hold settings too

A folder in the collection is more than a box for requests. Right-click one and choose **Folder settings…**, and you'll find four tabs (plus **API spec** on folders imported from an OpenAPI document):

- **Auth:** how every request inside logs in, for example a Bearer token.
- **Headers:** headers sent with every request inside.
- **Scripts:** JavaScript that runs before or after every request inside. You'll write scripts in the testing unit.
- **Docs:** notes for your team.

A request whose **Auth** tab says **Inherit from parent** (the default for new requests) uses its folder's auth. **Inherit** means "take it from the level above".

## The chain

Settings flow down from the workspace to folders to requests. Each level can add to what it inherits, or override it:

```layers
Workspace | default auth and headers for everything (workspace menu → Workspace settings…)
Folder "Admin" | Bearer {{adminToken}}, X-Tenant: acme
Folder "Admin/Reports" | adds X-Report-Format: csv
Request "Monthly sales" | Auth: Inherit from parent, gets all of the above
```

- **Auth:** the request's own auth wins. If it says *Inherit*, Zorvik uses the nearest folder's, then the workspace's.
- **Headers:** they add up. A header with the same name lower down replaces the inherited one.
- **Scripts:** they all run, the workspace's first, then the folders' from the outside in, then the request's own.

> [!note] Think of it like…
> A school's rules. The school says "no phones in class", your classroom adds "bring a calculator", and a teacher can make an exception for one lesson. You follow all of it without anyone repeating the school rules every morning.

## Overriding

Sometimes one request must be different. The login request itself has no token yet, and a public health check needs none. Set that request's **Auth** to **No auth** or to its own method: the request's choice beats anything inherited, while inherited headers still arrive.

```sequence
participants: Zorvik, Server
Note over Zorvik: Stats says No auth; the folder adds X-Tenant
Zorvik -> Server: GET /admin/stats with X-Tenant, no token
Server --> Zorvik: 401 Missing or wrong token
```

> [!tip] Inheritance needs a saved request
> A request inherits from the folders it is **saved** in. An unsaved tab doesn't belong to any folder yet, so it gets only the workspace's settings.

The token lives in a variable, `{{adminToken}}`. When it changes, you update one variable, and a whole folder of requests keeps working.

**You'll use this when…** your team's API has a public part and an admin part. One folder per part, the admin token set once on its folder, and a new teammate only has to fill in one variable to use all of it.
