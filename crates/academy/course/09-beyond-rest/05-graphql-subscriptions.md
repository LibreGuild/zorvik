---
id: graphql-subscriptions
title: "GraphQL subscriptions: results that keep coming"
summary: A subscription is a GraphQL operation that stays open. The server sends a new result each time something happens, over WebSocket or Server-Sent Events.
minutes: 5
added: 0.2.0
lab:
  title: Tick, tick, tick
  goal: Subscribe over WebSocket, change what the subscription asks for, then subscribe over Server-Sent Events.
  minutes: 6
  playground: true
  steps:
    - text: |
        Create a GraphQL request (**+** at the end of the tab bar → **GraphQL request**) with the URL `{{playground}}/graphql-ws` and the query `subscription { tick }`. The **Send** button turns into **Subscribe**: press it, and watch the results arrive one by one until the server completes the subscription.
      hints:
        - "A subscription is written like a query, with the keyword subscription instead of query."
        - "Type subscription { tick } in the query editor. As soon as Zorvik sees the subscription keyword, the main button says Subscribe."
        - "URL {{playground}}/graphql-ws, query subscription { tick }, press Subscribe. Three results ({\"data\":{\"tick\":1}} and so on) appear in the log, then The server completed the subscription."
      check:
        call: { method: socket.connect, ok: true, params: { request: { body: { graphql: { query: "*subscription*" } } } } }
      solution:
        - call:
            method: socket.connect
            params:
              connId: lab-subscription
              path: null
              request: { name: Ticks, method: POST, url: "{{playground}}/graphql-ws", body: { type: graphql, graphql: { query: "subscription { tick }" } } }
        - wait: 300
    - text: |
        Subscriptions take variables like queries do. This practice server sends as many ticks as the variable `count` asks for. Set the **Variables** to `{"count": 5, "intervalMs": 300}` and subscribe again. What is the number of the last tick?
      hints:
        - "The Variables editor is below the query, on its own tab."
        - "Replace the Variables with {\"count\": 5, \"intervalMs\": 300} and press Subscribe. The ticks now come every 0.3 seconds."
        - "The last result reads {\"data\":{\"tick\":5}}: type 5."
      check:
        all:
          - call: { method: socket.connect, ok: true, params: { request: { body: { graphql: { variables: "*count*" } } } } }
          - answer: "5"
      solution:
        - call:
            method: socket.connect
            params:
              connId: lab-subscription
              path: null
              request:
                name: Ticks
                method: POST
                url: "{{playground}}/graphql-ws"
                body: { type: graphql, graphql: { query: "subscription { tick }", variables: '{"count": 5, "intervalMs": 10}' } }
        - wait: 300
        - answer: "5"
    - text: |
        Some servers stream subscriptions over **Server-Sent Events** instead of WebSocket. Open the **Subscription** tab next to **Variables**, choose **SSE**, change the URL to `{{playground}}/graphql-sse`, and subscribe once more.
      hints:
        - "The Subscription tab appears only while the operation is a subscription. It holds the transport, a separate URL and connection params."
        - "Click Subscription (next to Variables), then SSE. Put {{playground}}/graphql-sse in the URL bar and press Subscribe."
        - "Subscription tab → SSE, URL {{playground}}/graphql-sse, Subscribe. The log's Connected line now says GraphQL over SSE."
      check:
        call: { method: socket.connect, ok: true, params: { request: { body: { graphql: { transport: sse } } } } }
      solution:
        - call:
            method: socket.connect
            params:
              connId: lab-subscription
              path: null
              request:
                name: Ticks
                method: POST
                url: "{{playground}}/graphql-sse"
                body: { type: graphql, graphql: { query: "subscription { tick }", transport: sse } }
        - wait: 300
quiz:
  - question: What makes a subscription different from a query?
    options:
      - It stays open, and the server sends a new result whenever something happens
      - It changes data on the server, like a mutation
      - It returns the schema instead of data
    answer: 0
    explain: A query answers once. A subscription keeps the line open and pushes a result for every new event.
  - question: A server closes your subscription's WebSocket with code 4403. What should you check first?
    options:
      - The query's spelling
      - The credentials, usually in the connection params
      - Whether the server supports HTTP/2
    answer: 1
    explain: "4403 means forbidden. Servers usually read the auth token from the connection params sent in connection_init, or from a header."
  - question: Your team's server runs Apollo Server 3. Which transport fits?
    options:
      - WebSocket (legacy), the subscriptions-transport-ws protocol
      - WebSocket, the graphql-transport-ws protocol
      - "Neither: Apollo Server 3 has no subscriptions"
    answer: 0
    explain: Apollo Server 2 and 3 speak the older subscriptions-transport-ws protocol. Newer servers (Apollo Server 4, graphql-ws, Hasura) speak graphql-transport-ws.
---

Queries ask once and get one answer. Mutations change something and answer once. But some screens need to know **the moment** something changes: a new chat message, a price update, a finished build. For that, GraphQL has a third kind of operation: the **subscription**.

```graphql
subscription OnMessage($room: ID!) {
  messageAdded(room: $room) {
    author
    text
  }
}
```

It looks like a query, but instead of one answer you get a stream of them, each shaped exactly like the subscription asked.

## How it travels

Plain HTTP answers once, so subscriptions need a connection that stays open. Most servers use a **WebSocket** with a small protocol on top:

```sequence
Client -> Server: connection_init (with the auth token in the params)
Server --> Client: connection_ack
Client -> Server: subscribe (the operation and its variables)
Server --> Client: next (a result)
Server --> Client: next (another result)
Server --> Client: complete
```

There are two such protocols, and a server speaks one of them:

| Transport | Protocol | Typical servers |
|---|---|---|
| **WebSocket** | `graphql-transport-ws` | the graphql-ws library, Apollo Server 4+, Hasura |
| **WebSocket (legacy)** | `subscriptions-transport-ws` | Apollo Server 2 and 3 |
| **SSE** | graphql-sse | GraphQL Yoga and others |

The SSE way is simpler: the client sends the operation as a normal POST and the server answers with an event stream, one `next` event per result.

> [!note] Think of it like…
> A query is a phone call: you ask, they answer, you hang up. A subscription is a newsletter: you sign up once, and every issue arrives by itself until you unsubscribe.

## Auth for subscriptions

Browsers can't set headers on a WebSocket, so many servers read the token from the **connection params** instead: a JSON object sent in `connection_init`, like `{"authToken": "…"}`. In Zorvik it sits in the **Subscription** tab, next to the transport and an optional separate URL (some servers serve subscriptions on their own path, like `/subscriptions`).

When a server refuses, the close code tells you why: `4401` means it wants credentials, `4403` means the ones you sent aren't allowed.

## Testing subscriptions

In the app, a subscription is live: results arrive as they happen. In a **collection run** (and with `zorvik run` in CI), Zorvik reads a subscription until a number of results or a time limit (the request's **Settings**, **In collection runs**), and then its tests see all the results at once:

```js
pm.test("the first three ticks arrive in order", () => {
  const ticks = pm.response.json().map((r) => r.data.tick);
  pm.expect(ticks).to.eql([1, 2, 3]);
});
```
