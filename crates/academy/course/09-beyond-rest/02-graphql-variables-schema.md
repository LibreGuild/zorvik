---
id: graphql-variables-schema
title: GraphQL variables and the schema
summary: Write a query once and pass values as variables; the schema tells you, and Zorvik, every field you can ask for.
minutes: 6
lab:
  title: Read the menu, fill in the blanks
  goal: Explore the practice server's schema, then send a query and a mutation that take variables.
  minutes: 8
  playground: true
  steps:
    - text: |
        Create a **GraphQL request** for `{{playground}}/graphql` (or reuse the one from the last lesson). In the **Body** tab, press the **Schema** toggle to open the schema panel, then press **Refresh schema** to read the schema fresh from the server.
      hints:
        - "Zorvik reads the schema by sending an introspection query to the request's URL. It needs the URL first."
        - "The Schema toggle and the Refresh schema button sit in the toolbar above the query editor, next to Prettify."
        - "Set the URL to {{playground}}/graphql, then click Refresh schema in the toolbar above the query. Hover the Schema toggle: it now says Schema loaded."
      check:
        call: { method: graphql.schema, ok: true }
      solution:
        - call: { method: graphql.schema, params: { request: { name: GraphQL, method: POST, url: "{{playground}}/graphql", body: { type: graphql, graphql: { query: "{ hello }" } } }, path: null, refresh: true } }
    - text: |
        In the schema panel, open the **Query** type. One of its fields is marked **deprecated** (it still works, but you shouldn't use it any more). Which field is it?
      hints:
        - "Query is one of the root types at the top of the schema panel. Click it to see its fields."
        - "Deprecated fields are flagged with a Deprecated note that says what to use instead."
        - "Open Query in the schema panel. The field with a Deprecated note (it says to use hello) is the answer."
      check:
        answer: greeting
      solution:
        - answer: greeting
    - text: |
        Now use a variable. Send this query, and put `{"id": "2"}` in the **Variables** box below the editor:

        ```text
        query GetUser($id: ID!) {
          user(id: $id) { name role }
        }
        ```
      hints:
        - "The query declares $id and uses it where the value would go. The value itself goes in the Variables box, as JSON."
        - "The Variables box is under the query editor. Its keys have no $ sign: the variable $id is filled by the key id."
        - "Paste the query as shown, type {\"id\": \"2\"} into Variables and press Send. The user is Alan Turing."
      check:
        send: { request: { body: { graphql: { query: "*$id*" } } }, json: { data: { user: { name: Alan Turing } } } }
      solution:
        - send: { method: POST, url: "{{playground}}/graphql", body: { type: graphql, graphql: { query: "query GetUser($id: ID!) { user(id: $id) { name role } }", variables: '{"id": "2"}' } } }
    - text: |
        Finally, a mutation with a variable. The practice server has one harmless mutation, `echo`, that sends your text back. Send:

        ```text
        mutation Shout($text: String!) {
          echo(text: $text)
        }
        ```

        with Variables such as `{"text": "Hello from Zorvik"}`.
      hints:
        - "A mutation is written like a query, but starts with the word mutation."
        - "Declare $text as String! after the operation name, use it in echo(text: $text), and give it a value in Variables."
        - "Paste the mutation as shown, type {\"text\": \"Hello from Zorvik\"} into Variables and press Send."
      check:
        send: { request: { body: { graphql: { query: "*mutation*$text*" } } }, json: { data: { echo: "*" } } }
      solution:
        - send: { method: POST, url: "{{playground}}/graphql", body: { type: graphql, graphql: { query: "mutation Shout($text: String!) { echo(text: $text) }", variables: '{"text": "Hello from Zorvik"}' } } }
quiz:
  - question: "What does `($id: ID!)` after an operation name declare?"
    options:
      - A variable named id of type ID that must be given a value
      - An optional comment for other developers
      - The ID of the server to send the query to
    answer: 0
    explain: Variables are declared with a $ and a type. The ! means the value is required (non-null).
  - question: "In the schema, a field reads `email: String` with no `!`. What does that tell you?"
    options:
      - The email is always there
      - The email is a list of strings
      - The email may be null
    answer: 2
    explain: "Without ! a field may be null. `name: String!` would promise a value every time."
  - question: How does Zorvik know which fields to suggest while you type a query?
    options:
      - It guesses from earlier responses
      - It sends an introspection query and reads the schema the server returns
      - It downloads the fields from Zorvik's website
    answer: 1
    explain: Introspection asks the server to describe itself. Zorvik uses the answer for autocomplete, red underlines and the schema panel.
---

In the last lesson you typed values straight into queries: `user(id: "1")`. That works, but the query text changes every time the value does. Real apps write each query **once** and pass the changing values separately, like a function with parameters.

## Variables

```anatomy
query GetUser($id: ID!) | operation name GetUser, one variable $id of type ID (! = required)
user(id: $id) { name role } | $id goes where a value would go
{"id": "2"} | the Variables box: the actual value, as JSON
```

The query and its variables travel together in one request: `{"query": "…", "variables": {"id": "2"}}`. Why bother?

- **Reuse.** The same saved request fetches any user; only the Variables change.
- **No gluing strings.** You never build a query by pasting user input into its text (and fighting quotes).
- **Operation names** such as `GetUser` show up in server logs, and they are needed when one document holds several operations (Zorvik then shows an **Operation** menu to pick one).

Variables can use Zorvik variables too: `{"id": "{{userId}}"}` takes the value from your environment.

## The schema: the server's menu

Every GraphQL server has a **schema**, a typed list of everything you can ask for. Part of the practice server's schema, written in GraphQL's schema language, looks like this:

```text
type Query {
  hello(name: String = "world"): String!
  user(id: ID!): User
  users(first: Int): [User!]!
}
type User { id: ID!  name: String!  email: String  role: Role!  posts: [Post!]! }
```

- `String`, `Int`, `ID` and `Boolean` are **scalars**: single values.
- `User` and `Post` are **object types**: they have fields of their own, so you must pick some.
- `!` means **non-null**: always there. `email: String` may be null.
- `[Post!]!` is a list of posts: the list is always there and holds no nulls.

## Introspection

How does a tool learn the schema? It asks. **Introspection** is a special query (`__schema`) that GraphQL servers answer with a description of themselves. Zorvik sends it as soon as your GraphQL request has a URL, and uses the answer for:

- **Autocomplete** while you type (or press Ctrl + Space): only fields that exist.
- **Red underlines** on unknown fields, before you even send.
- The **Schema** panel: types, fields, arguments and descriptions. ⌘/Ctrl-click a field in your query to jump to it.

```sequence
Zorvik -> Server: POST /graphql { __schema { types { name fields … } } }
Server --> Zorvik: every type, field and argument
Note over Zorvik: autocomplete, checks, Schema panel
```

> [!note] Think of it like…
> The schema is a restaurant's menu and introspection is asking the waiter for it. Variables are the blanks on an order form ("how many?"): the form stays the same, only what you write in the blanks changes.

> [!warning] Deprecated fields
> A field marked **deprecated** still works today but is on its way out. The schema usually says what to use instead. Move your queries off it before it disappears.

Some production servers switch introspection off. Then Zorvik can't suggest fields, but your queries still work: ask the API team for their schema.

**You'll use this when…** you test a search screen with a dozen filters. You save one query, point its Variables at environment variables, and replay it against dev, staging and production without touching the query text.
