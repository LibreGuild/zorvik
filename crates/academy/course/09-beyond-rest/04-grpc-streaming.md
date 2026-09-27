---
id: grpc-streaming
title: gRPC streaming
summary: A gRPC call can carry many messages, from the server, from the client, or from both at once.
minutes: 6
lab:
  title: Three kinds of streams
  goal: Receive a server stream, send a client stream and play ping-pong over a bidirectional stream.
  minutes: 10
  playground: { grpc: true }
  steps:
    - text: |
        In a gRPC request for `{{grpc}}`, pick **ServerStream** (it has a **server stream** badge). Set the Message to `{"message": "tick", "count": 4}` and press **Send**. Watch the replies arrive one by one in **Messages**.
      hints:
        - "ServerStream answers count times (3 if you leave count out). You send one message, then only listen."
        - "Open the method picker left of the URL, choose ServerStream, and type the JSON in the Message tab."
        - "Method ServerStream, Message {\"message\": \"tick\", \"count\": 4}, then Send. Four replies arrive, then the status 0 OK."
      check:
        call: { method: grpc.start, params: { request: { method: "*Echo/ServerStream", body: { text: 're:"count"\s*:\s*4\b' } } }, ok: true }
      solution:
        - call: { method: grpc.start, params: { sessionId: lab-grpc-server, request: { name: Echo, kind: grpc, url: "{{grpc}}", method: zorvik.test.v1.Echo/ServerStream, body: { text: '{"message": "tick", "count": 4}' } }, path: null } }
        - wait: 300
    - text: What does the `message` field of the **last** reply say? Type it exactly.
      hints:
        - "The server numbers its replies. Look at the fourth message you received."
        - "Received messages have a blue arrow in the Messages tab. Open the last one."
        - "The server adds #1, #2, … to your text, so the last one is tick #4."
      check:
        answer: ["tick #4", "tick#4"]
      solution:
        - answer: "tick #4"
    - text: |
        Now a client stream. Pick **ClientStream** and press **Send** to open the call: nothing comes back yet. Then send three messages with **Send message**, one after another: `{"message": "red"}`, `{"message": "green"}` and `{"message": "blue"}`. Finally press **End stream** and read the single reply.
      hints:
        - "In a client stream the server waits until you say you're done. End stream is how you say it."
        - "Send opens the call. After that the Message tab shows Send message and End stream buttons: edit the JSON, press Send message, repeat."
        - "Pick ClientStream, press Send, then send {\"message\": \"red\"}, {\"message\": \"green\"} and {\"message\": \"blue\"} with Send message, then press End stream. The reply is red,green,blue."
      check:
        all:
          - call: { method: grpc.start, params: { request: { method: "*Echo/ClientStream" } }, ok: true }
          - call: { method: grpc.send, params: { message: "*blue*" }, ok: true }
          - call: { method: grpc.end, ok: true }
      solution:
        - call: { method: grpc.start, params: { sessionId: lab-grpc-client, request: { name: Echo, kind: grpc, url: "{{grpc}}", method: zorvik.test.v1.Echo/ClientStream }, path: null } }
        - call: { method: grpc.send, params: { sessionId: lab-grpc-client, message: '{"message": "red"}' } }
        - call: { method: grpc.send, params: { sessionId: lab-grpc-client, message: '{"message": "green"}' } }
        - call: { method: grpc.send, params: { sessionId: lab-grpc-client, message: '{"message": "blue"}' } }
        - call: { method: grpc.end, params: { sessionId: lab-grpc-client } }
        - wait: 300
    - text: |
        Last, both directions at once. Pick **Bidi**, press **Send**, then send `{"message": "marco"}` with **Send message**. The answer arrives right away, while the call stays open. Send one more if you like, then press **End stream**.
      hints:
        - "In a bidirectional stream the server doesn't wait for End stream: it answers each message as it arrives."
        - "Same buttons as the client stream: Send opens the call, Send message sends, End stream finishes."
        - "Pick Bidi, press Send, type {\"message\": \"marco\"}, press Send message, watch the reply, then press End stream."
      check:
        all:
          - call: { method: grpc.start, params: { request: { method: "*Echo/Bidi" } }, ok: true }
          - call: { method: grpc.send, params: { message: "*marco*" }, ok: true }
      solution:
        - call: { method: grpc.start, params: { sessionId: lab-grpc-bidi, request: { name: Echo, kind: grpc, url: "{{grpc}}", method: zorvik.test.v1.Echo/Bidi }, path: null } }
        - call: { method: grpc.send, params: { sessionId: lab-grpc-bidi, message: '{"message": "marco"}' } }
        - wait: 200
        - call: { method: grpc.end, params: { sessionId: lab-grpc-bidi } }
        - wait: 200
quiz:
  - question: A dashboard opens one call and then receives price updates for minutes. Which style is that?
    options:
      - Unary
      - Server streaming
      - Client streaming
    answer: 1
    explain: One request from the client, many replies from the server over the same call.
  - question: What does End stream do in a client-streaming call?
    options:
      - It cancels the call and throws the answer away
      - It restarts the call from the beginning
      - It tells the server no more messages follow, and you still get its answer
    answer: 2
    explain: Ending your side is a "half-close". The server then sends its reply and the final status.
  - question: What is special about a bidirectional stream?
    options:
      - Both sides can send messages whenever they like, over the same open call
      - The client must send everything before the server may answer
      - Only the server can send messages
    answer: 0
    explain: The two directions are independent, which suits chats, games and live collaboration.
---

So far every gRPC call was **unary**: one message in, one message out. But gRPC runs on HTTP/2, where a single call is a **stream**: a channel that can carry any number of messages in each direction until one side ends it. The method's definition in the `.proto` file says which directions stream:

```text
rpc Unary(EchoRequest) returns (EchoReply);
rpc ServerStream(EchoRequest) returns (stream EchoReply);
rpc ClientStream(stream EchoRequest) returns (EchoReply);
rpc Bidi(stream EchoRequest) returns (stream EchoReply);
```

The word `stream` in front of a message type means "many of these".

| Style | Client sends | Server sends | Real-world example |
|---|---|---|---|
| Unary | one | one | fetch a user |
| Server streaming | one | many | live prices, a long report sent in pieces |
| Client streaming | many | one | upload a file in chunks, send a batch of readings |
| Bidirectional | many | many | chat, multiplayer games, live translation |

## Server streaming

You send one request, then listen. Replies arrive one by one, and the call ends with a status in the trailers.

## Client streaming and "End stream"

You send many messages; the server usually waits until you say you're done. Saying so is called a **half-close**: you close *your* direction but keep listening. In Zorvik that's the **End stream** button.

```sequence
Client -> Server: {"message": "red"}
Client -> Server: {"message": "green"}
Client -> Server: {"message": "blue"}
Client -> Server: End stream (half-close)
Server --> Client: {"message": "red,green,blue"}
Server --> Client: status 0 OK
```

## Bidirectional streaming

Both sides send whenever they like. The practice server's **Bidi** method answers each message as soon as it arrives:

```sequence
Client -> Server: {"message": "marco"}
Server --> Client: {"message": "marco"}
Client -> Server: {"message": "polo"}
Server --> Client: {"message": "polo"}
Note over Client, Server: the call stays open until both sides end it
```

> [!note] Think of it like…
> Unary is sending a letter and getting one back. Server streaming is subscribing to a newspaper. Client streaming is dictating a long message, then hearing "got it". Bidirectional is a live conversation: both talk, both listen, at the same time.

## In Zorvik

Streaming methods carry a badge in the method picker: **server stream**, **client stream** or **bidi**. **Send** starts the call. For a server stream your message goes out at once. For client and bidi streams, the **Message** tab gets **Send message** and **End stream** buttons, and the result pane counts messages as they flow ("N sent · M received"). Sent messages have a green ↗ arrow, received ones a blue ↙. **Cancel** stops a call early; its status then says it was cancelled.

> [!tip] Streams end with a status too
> A stream that sent ten good messages can still end with an error status. Always check the final status and trailers, not only the messages.

**You'll use this when…** you test a service that streams order updates, uploads telemetry in batches or powers a chat. You open the stream in Zorvik, send exactly the messages you want, and watch every reply and the final status arrive in order.
