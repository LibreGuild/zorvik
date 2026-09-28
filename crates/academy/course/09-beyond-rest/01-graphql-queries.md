---
id: graphql-queries
title: "GraphQL: ask for exactly what you need"
summary: One endpoint, one query that lists the fields you want, and an answer shaped exactly like your question.
minutes: 5
lab:
  title: Order exactly what you want
  goal: Send GraphQL queries to the practice server and get back only the fields you asked for.
  minutes: 8
  playground: true
  steps:
    - text: |
        Create a GraphQL request: click **+** at the end of the tab bar and choose **GraphQL request**. Set the URL to `{{playground}}/graphql`, type `{ hello }` in the query editor and press **Send**.
      hints:
        - "A GraphQL request is an HTTP POST whose body is a query. Zorvik sets that up for you when you pick GraphQL request."
        - "Click the + at the right end of the tab bar and pick GraphQL request. Paste {{playground}}/graphql into the URL bar, then type { hello } in the big editor of the Body tab."
        - "URL {{playground}}/graphql, query { hello }, then Send (⌘/Ctrl + Enter). The answer is data.hello: Hello, world!"
      check:
        send: { url: "*/graphql*", status: 200, json: { data: { hello: "Hello, world!" } } }
      solution:
        - send: { method: POST, url: "{{playground}}/graphql", body: { type: graphql, graphql: { query: "{ hello }" } } }
    - text: |
        Ask for the user with id `1`, but only their `name` and `email`. Nothing else should come back.
      hints:
        - "The user field takes an argument: user(id: 1). Inside its curly braces, list only the fields you want."
        - "Start typing inside the braces: the editor suggests the fields that exist. Leave out id, role and posts."
        - "Replace the query with { user(id: 1) { name email } } and press Send."
      check:
        send: { json: { data: { user: { name: Ada Lovelace, email: ada@example.test, id: "!*", role: "!*", posts: "!*" } } } }
      solution:
        - send: { method: POST, url: "{{playground}}/graphql", body: { type: graphql, graphql: { query: '{ user(id: "1") { name email } }' } } }
    - text: |
        Fields can nest. Ask for the same user's `posts`, with each post's `title`, in the same query. Then type the title of **Ada's second post** here.
      hints:
        - "posts is a list of Post objects, so it needs its own curly braces with the fields you want from each post."
        - "Add posts { title } inside the user's braces, send, and look at the second item of the posts list in the response."
        - "Send { user(id: 1) { name posts { title } } } and copy the second title from the response."
      check:
        all:
          - send: { json: { data: { user: { posts: [{ title: "*" }] } } } }
          - answer: On Bernoulli numbers
      solution:
        - send: { method: POST, url: "{{playground}}/graphql", body: { type: graphql, graphql: { query: '{ user(id: "1") { name posts { title } } }' } } }
        - answer: On Bernoulli numbers
    - text: |
        Now make a mistake on purpose: ask the user for a field that does not exist, such as `age`, and send anyway. Read what comes back instead of `data`.
      hints:
        - "The editor already underlines age in red: it knows the schema. Send it anyway to see the server's side."
        - "Try { user(id: 1) { name age } } and look for an errors list in the response."
        - "Send { user(id: 1) { age } }. The server answers 400 with errors: Cannot query field age on type User."
      check:
        send: { url: "*/graphql*", json: { errors: [{ message: "Cannot query field*" }] } }
      solution:
        - send: { method: POST, url: "{{playground}}/graphql", body: { type: graphql, graphql: { query: '{ user(id: "1") { age } }' } } }
quiz:
  - question: You only need a user's name. How do you get just that from a GraphQL API?
    options:
      - Call a special URL such as /users/1/name
      - List only the field name inside the user's curly braces in your query
      - 'Send a header that says "Fields: name"'
    answer: 1
    explain: In GraphQL the query itself says which fields you want. Fields you don't list are never sent.
  - question: A GraphQL server answers with status 200. Did your query work?
    options:
      - Yes, 200 always means everything worked
      - No, GraphQL answers 201 when a query works
      - Not necessarily. Check the body for an errors list
    answer: 2
    explain: Many GraphQL servers answer 200 even when a query failed and report the problem in an errors list, so always read the body.
  - question: How many endpoints does a typical GraphQL API have?
    options:
      - Usually just one, such as /graphql
      - One per type of data
      - One per field
    answer: 0
    explain: One endpoint serves every query. What you get back depends on the query you send, not on the URL.
---

Most APIs you have met so far are **REST** APIs: many URLs, one per kind of thing (`/users/1`, `/users/1/posts`), and each URL answers with a shape the server chose. That leads to two classic annoyances:

- **Over-fetching:** you need a user's name, but `/users/1` sends forty fields.
- **Under-fetching:** you need a user *and* their posts, so you make two or three requests.

**GraphQL**, a query language for APIs first built at Facebook, turns this around. There is usually **one endpoint**, often `/graphql`, and *you* write down exactly which fields you want. The answer comes back in the same shape as your question.

```sequence
App -> Server: POST /graphql { user(id: "1") { name } }
Note over Server: finds user 1, keeps only "name"
Server --> App: 200 {"data": {"user": {"name": "Ada Lovelace"}}}
```

## Reading a query

```anatomy
query { | the operation: a query reads data (you may leave the word out)
user(id: "1") { | a field with an argument: which user
name | a field you want back
email | another one; fields you don't list never arrive
} } | closing braces end the user, then the query
```

- A **field** is one piece of data, such as `name`.
- An **argument** narrows a field down, such as `id: "1"`.
- The curly braces after a field hold its **selection set**: the fields you want from inside it. Selections nest as deep as the data goes, so `user { posts { title } }` fetches a user and their post titles in one trip.

The server's answer mirrors that shape under a top-level `data` key:

```json
{ "data": { "user": { "name": "Ada Lovelace", "email": "ada@example.test" } } }
```

> [!note] Think of it like…
> A set menu versus a buffet. A REST endpoint is a set menu: you get the whole plate. GraphQL is a buffet with a single counter: you point at exactly what you want, and only that lands on your plate.

## How it travels

GraphQL is not a new network protocol. It rides on plain HTTP: a query is usually sent as a `POST` with a JSON body such as `{"query": "{ hello }"}`. In Zorvik you don't write that JSON yourself. A **GraphQL request** has a query editor in its **Body** tab and wraps your query for you. It even suggests field names as you type, because it has read the server's list of fields (more on that in the next lesson).

## When things go wrong

A GraphQL server checks your query against the fields it knows before running it. Ask for something that doesn't exist and you get an `errors` list instead of `data`. Each error has a `message`, and usually the `locations` (line and column) of the mistake in your query:

```json
{ "errors": [{ "message": "Cannot query field \"age\" on type \"User\"." }] }
```

> [!warning] Don't trust the status alone
> Many GraphQL servers answer `200 OK` even when the query failed, and put the problem in `errors`. The practice server answers `400` for a broken query, but always read the body.

> [!tip] Reading and writing
> Reading data is a **query**. Changing data (create, update, delete) is a **mutation**, written the same way: `mutation { echo(text: "hi") }`. You'll send one in the next lesson.

**You'll use this when…** a mobile screen needs a user's name, avatar and last three orders. With GraphQL that is one request that returns exactly those fields: less data over a slow phone connection and fewer round trips. When the screen looks wrong, you paste its query into Zorvik and see exactly what the server returned.
