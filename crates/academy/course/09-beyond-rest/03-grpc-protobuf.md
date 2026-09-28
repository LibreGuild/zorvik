---
id: grpc-protobuf
title: gRPC and Protocol Buffers
summary: gRPC calls a method on another computer, with messages defined in a shared .proto contract and sent as compact binary.
minutes: 6
lab:
  title: Call a remote method
  goal: Load a gRPC service through server reflection, make a unary call and read a gRPC error status.
  minutes: 8
  playground: { grpc: true }
  steps:
    - text: |
        Create a gRPC request: click **+** at the end of the tab bar and choose **gRPC request**. Set the URL to `{{grpc}}`, then click the method picker left of the URL (it says **Pick method**) so Zorvik loads the server's services.
      hints:
        - "gRPC URLs start with grpc:// (or grpcs:// with TLS). The Lab environment already holds the practice server's address."
        - "The method picker is the button marked gRPC at the left of the URL bar. Opening it asks the server for its services (server reflection)."
        - "Choose gRPC request from the + menu, type {{grpc}} in the URL bar, then click Pick method. A list with zorvik.test.v1.Echo appears."
      check:
        call: { method: grpc.describe, ok: true }
      solution:
        - call: { method: grpc.describe, params: { request: { name: Echo, kind: grpc, url: "{{grpc}}" }, path: null } }
    - text: |
        Pick the method **Unary** of `zorvik.test.v1.Echo`. In the **Message** tab, write `{"message": "Hello, gRPC"}` (or press **Example message** and edit it), then press **Send**.
      hints:
        - "Unary is the method without a badge: one message in, one message out."
        - "The Message tab takes JSON. Zorvik turns it into Protocol Buffers bytes before sending."
        - "In the picker choose Unary, type {\"message\": \"Hello, gRPC\"} in the Message tab and press Send (⌘/Ctrl + Enter). The status badge shows 0 OK."
      check:
        call: { method: grpc.invoke, params: { request: { method: "*Echo/Unary" } }, ok: true, result: { status: { code: 0 } } }
      solution:
        - call: { method: grpc.invoke, params: { requestId: lab-grpc, request: { name: Echo, kind: grpc, url: "{{grpc}}", method: zorvik.test.v1.Echo/Unary, body: { text: '{"message": "Hello, gRPC"}' } }, path: null } }
    - text: |
        Look at the reply in the **Messages** tab. Besides your `message`, it copies your whole request into another field. What is that field called?
      hints:
        - "The reply is shown as JSON. Look at its top-level field names."
        - "Besides message, index and an empty messages list, one field holds a copy of everything you sent."
        - "The field that holds a copy of your request is called request."
      check:
        answer: request
      solution:
        - answer: request
    - text: |
        Now call a method that fails on purpose: pick **Fail**, keep the message as `{}` and press **Send**. Look at the status badge and the **Trailers** tab.
      hints:
        - "Fail always answers with an error status. It's there so you can see what a gRPC error looks like."
        - "Open the method picker again and choose Fail. An empty message {} is fine."
        - "Pick Fail, set the Message to {} and press Send. The status is 5 NOT_FOUND."
      check:
        call: { method: grpc.invoke, params: { request: { method: "*Echo/Fail" } }, result: { status: { name: NOT_FOUND } } }
      solution:
        - call: { method: grpc.invoke, params: { requestId: lab-grpc, request: { name: Echo, kind: grpc, url: "{{grpc}}", method: zorvik.test.v1.Echo/Fail, body: { text: "{}" } }, path: null } }
quiz:
  - question: What is a `.proto` file?
    options:
      - A log of the calls you made
      - The contract that defines a service's methods and messages
      - The server's TLS certificate
    answer: 1
    explain: Both sides build on the same .proto file, so they agree on every method, field and type.
  - question: A gRPC call fails because the item doesn't exist. Where do you see that?
    options:
      - In the gRPC status (such as 5 NOT_FOUND), sent at the end of the call
      - In the HTTP status line, as 404
      - Nowhere, gRPC calls can't fail
    answer: 0
    explain: The HTTP status of a gRPC call is almost always 200. The real verdict is the gRPC status in the trailers.
  - question: What does server reflection do?
    options:
      - It sends every call to a second server as a backup
      - It makes calls faster by caching answers
      - It lets a client ask the server which services and messages it has
    answer: 2
    explain: With reflection, a tool like Zorvik can list the methods and build messages without the .proto files.
---

An **RPC**, a *remote procedure call*, means calling a function that runs on another computer as if it were in your own program: `getUser(42)` instead of "build a URL, send a GET, parse the JSON". **gRPC** is a popular open-source RPC framework, created at Google. You'll meet it mostly *between* services inside a company, and in apps that need to be fast.

## Three big ideas

1. **A contract comes first.** The methods and messages are written in a `.proto` file. Both sides generate code from it, so they agree on every field and type.
2. **Protocol Buffers** (protobuf) is the message format: compact *binary* instead of text. Each field is sent as its number plus its value, which is smaller and faster to read than JSON, but not human-readable. That's where Zorvik helps: you type JSON, it converts.
3. **HTTP/2** carries the calls. One connection handles many calls at once, and data can flow both ways at the same time (you'll use that for streaming next).

```anatomy
service Echo { | a service: a group of methods
rpc Unary(EchoRequest) returns (EchoReply); | a method: one message in, one message out
message EchoRequest { | a message: a typed record
string message = 1; | a field: its type, its name and its number
int32 count = 2; | the numbers identify fields on the wire, so names can change safely
```

This is part of the practice server's own `.proto`. Each field gets a **type** (`string`, `int32`, another message…) and a **field number** that must never be reused.

```layers
gRPC | services and methods, e.g. zorvik.test.v1.Echo/Unary
Protocol Buffers | messages encoded as compact binary
HTTP/2 | many calls over one connection, streams both ways
TLS (optional) | encryption, when the URL starts with grpcs://
TCP | a reliable stream of bytes
```

> [!note] Think of it like…
> Ordering from a catalogue by item number. The catalogue (the `.proto`) is printed in advance and both sides have a copy, so your order can be as short as "item 12, size 3". Nobody has to spell out what item 12 is.

## Server reflection

To turn your JSON into protobuf bytes, Zorvik needs the `.proto` definitions. It gets them one of two ways:

- **Proto files:** add the team's `.proto` files in the request's **Proto files** tab.
- **Server reflection:** many servers can describe themselves. Zorvik asks, and the server sends its definitions. The practice server supports this.

```sequence
Zorvik -> Server: reflection: which services do you have?
Server --> Zorvik: zorvik.test.v1.Echo: Unary, ServerStream, …
Zorvik -> Server: Echo/Unary {"message": "Hello, gRPC"}
Server --> Zorvik: {"message": "Hello, gRPC", …} + status 0 OK
```

## Status codes and metadata

gRPC has its own **status codes**, sent at the very end of each call in the **trailers** (headers that come after the data): `0 OK`, `3 INVALID_ARGUMENT`, `5 NOT_FOUND`, `7 PERMISSION_DENIED`, `14 UNAVAILABLE` and more. The HTTP status underneath is almost always `200`, so the gRPC status is the one to read. Headers are called **metadata** in gRPC; auth tokens travel there.

In Zorvik a gRPC request has the tabs **Message**, **Metadata**, **Proto files**, **Auth**, **Settings** and **Docs**. **Send** makes the call, and the result pane shows **Messages**, **Headers**, **Trailers**, **Request metadata** and **Timing**.

**You'll use this when…** your backend services talk gRPC to each other and you need to call one directly: to reproduce a bug, check what a service returns for one customer, or see how it reports an error, without writing a client program first.
