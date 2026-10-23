---
title: gRPC
description: Call gRPC services with server reflection or .proto files, unary and streaming (with saved messages and steps), with metadata, auth and TLS, and test them in collection runs.
sidebar:
  order: 2
---

A gRPC request calls one method of a gRPC service. Messages are written and shown as JSON; Zorvik converts them to and from Protocol Buffers with the service definitions it gets from **server reflection** or from your **`.proto` files**. Unary calls, server streaming, client streaming and bidirectional streaming are all supported.

## Create a gRPC request

In the **Collection** sidebar, open the **New** menu (**+**) and choose **New gRPC request**. The request editor then shows:

- **URL bar**: the method picker (in place of the HTTP method), the server address and **Send**.
- Tabs: **Message**, **Metadata**, **Saved messages**, **On connect**, **Proto files**, **Auth**, **Settings**, **Scripts** and **Docs**.

## Server address

| URL | Connection |
|---|---|
| `grpc://host:port` | Plaintext HTTP/2 ("prior knowledge", also called h2c). |
| `grpcs://host:port` | HTTP/2 over TLS (ALPN `h2`). |
| `http://…`, `https://…` | Same as `grpc://` and `grpcs://`. |
| `host:port` | No scheme means `grpc://`. |

- Always include the port. Without one, Zorvik uses 80 for `grpc://` and 443 for `grpcs://`, not the usual 50051.
- A path in the URL (`grpcs://gateway.example.com/api`) is put in front of every method path, for gateways that route by prefix.
- `{{variables}}` work in the URL, for example `grpc://{{grpcHost}}`.
- When you use `grpcs://` with a server that doesn't speak TLS, the TLS error ends with **The server may not use TLS: try grpc:// instead of grpcs://.**

## Service definitions: reflection or .proto files

Zorvik needs the service definitions to list methods and to convert JSON. It uses one of two sources:

| | Server reflection | `.proto` files |
|---|---|---|
| Used when | The **Proto files** tab lists no file (the default) | At least one `.proto` file is listed |
| Needs | The server to run the reflection service (`grpc.reflection.v1`, or `v1alpha` for older servers) | The files, in the workspace (see below) |
| Auth | Metadata and auth are sent with the reflection calls too | None |
| Cached | Per URL, for the session | Until one of the files changes on disk |
| Reload | **Refresh** in the method picker, or **Load services from the server** in the **Proto files** tab | **Load services from the files** in the **Proto files** tab |

A method that is not in the cached definitions (because the server or the files changed) makes Zorvik reload them once before it gives up. The reflection service itself is not listed.

Reflection is limited by the request's timeout, or by 2 minutes when requests have no timeout.

### Using .proto files

1. Open the **Proto files** tab.
2. Click **Add .proto file…** for each file that defines a service you want to call.
3. When your files import others from a common root (`import "acme/common/money.proto";`), click **Add folder…** under **Import folders** and add that root.
4. Click **Load services from the files**. The tab shows how many services and methods were found, or the compiler error.

How imports are found: first in the import folders, in order, then in the folder of each listed file. The Protocol Buffers well-known types (`google/protobuf/*.proto`, such as `timestamp.proto` and `empty.proto`) are built in. You don't need `protoc` installed; Zorvik compiles the files itself.

Files inside the workspace are saved relative to it, so the request works for everyone who opens the workspace. Files outside the workspace (including files an import reaches through a link) are refused unless you allow them in **Settings → Data & privacy** (allow files outside the workspace).

## Pick a method

Click the method picker left of the URL. It loads the services the first time you open it, groups methods by service, and has a search box. Streaming methods carry a badge: **server stream**, **client stream** or **bidi**.

The method is saved as `package.Service/Method`, for example `helloworld.Greeter/SayHello`. You can also type or paste it into the file as `package.Service.Method` or with a leading `/`.

## The message

The **Message** tab holds the request message as JSON, in the standard proto3 JSON mapping (field names in `lowerCamelCase` or as in the `.proto` file, 64-bit integers as strings or numbers, enums by name, `bytes` as base64, well-known types such as `Timestamp` as strings).

- **Example message** fills in an example of the method's input type. It asks before replacing a message you wrote.
- **Beautify** reformats the JSON.
- `{{variables}}` are replaced before the message is encoded; undefined ones are reported.
- An empty message is sent as `{}`. Fields the message type doesn't have are an error: **The message is not a valid helloworld.HelloRequest: …**

Response messages are shown as JSON too, including fields that have their default values.

## Metadata

The **Metadata** tab is the request's headers, sent as gRPC metadata. Folder and workspace headers are inherited, and the **Auth** tab adds an `authorization` entry (Basic, Bearer, API key in a header, or OAuth 2.0, with tokens fetched when needed).

| Rule | Details |
|---|---|
| Keys | Sent in lowercase. |
| Text values | Printable ASCII only (no line breaks, tabs or accents). Anything else is an error that tells you to use a `-bin` key. |
| `-bin` keys | Binary metadata. A value that is valid base64 (standard or URL-safe, with or without padding) is sent as those bytes; any other text is base64-encoded for you. |
| `host` or `:authority` | Sets the HTTP/2 `:authority` of the call. |
| Other pseudo-headers (`:path`, …) | Refused. |
| Reserved names | `content-type`, `te`, `content-length`, `grpc-timeout`, `grpc-encoding`, `grpc-accept-encoding`, `connection`, `keep-alive`, `proxy-connection`, `transfer-encoding` and `upgrade` are ignored: the protocol sets them. |

Before your metadata, every call sends `content-type: application/grpc`, `te: trailers`, a `user-agent` (unless you set one, or default headers are off in Settings), `grpc-accept-encoding: gzip` and, for unary calls with a timeout, `grpc-timeout`. The **Request metadata** result tab shows exactly what was sent.

## Calls

Press **Send** (or <kbd>Mod</kbd>+<kbd>Enter</kbd>). What happens depends on the method:

| Method kind | What **Send** does |
|---|---|
| **Unary** | Sends the message and waits for the answer. The request's timeout is the call's deadline, sent as `grpc-timeout`. |
| **Server stream** | Opens the call, sends the message and closes the sending side. Response messages appear as they arrive. |
| **Client stream** and **Bidi stream** | Opens the call without sending anything. Then **Send message** (<kbd>Mod</kbd>+<kbd>Enter</kbd> in the message editor) sends the current JSON, as often as you like, and **End stream** tells the server that no more messages follow. The server may keep answering after that. |

Streams have no deadline: they run until the server ends them or you press **Cancel**, which ends the call with `CANCELLED`. Connecting is still limited by the request's timeout. Each call uses a fresh connection, like HTTP requests, so the timing is honest.

### Stream a file or the microphone

Client and bidirectional streams can send audio or any other bytes the way a speech-to-text service expects them: each chunk as a message with its bytes (base64, as proto3 JSON writes `bytes`) in one field. Below the message editor, choose **File** or **Microphone**:

1. Set **Chunk field** to the input message's field for the bytes, for example `audioContent` (Google Speech-to-Text). A dotted path fills a nested field (`audio.content`). It is saved with the request (`grpc.chunkField`).
2. **Send** opens the stream; **Send message** sends the first message from the editor (a config, for example).
3. **Start streaming** sends the file at its pace (a WAV file is set up to play in real time: 20 ms of samples per message, the header skipped), or **Start recording** sends the microphone (PCM 16-bit, 16 kHz, mono for most speech services). The settings are those of WebSocket's [file](../websocket/#binary-stream-a-file) and [microphone](../websocket/#binary-stream-the-microphone) sources.
4. With **End the stream when done** (on by default), the stream's sending side ends when the file has been sent or the recording stops, so the service sends its final results. Answers show in **Messages** as they arrive: interim transcripts, then final ones.

Speech services expect audio close to real time: Google's closes a stream that gets no audio for about 10 seconds (`OUT_OF_RANGE: Audio Timeout Error`), so start the audio soon after the config.

### Saved messages and On connect steps

For streaming calls, the **Saved messages** tab keeps JSON messages with the request (each with a name), and the **On connect** tab holds steps that run in order once a stream is open, as for [WebSocket requests](../websocket/#on-connect):

| Step | What it does |
|---|---|
| **Send** | Sends a saved message (its `{{variables}}` filled in). |
| **Wait for** | Waits for a received message that matches, within a time limit: contains, is exactly, matches a regex, a JSON condition on the message (`$.status == READY`), or any message. |
| **Keep sending** | Sends a saved message every so often while the call is open. |

A server stream sends the **Message** tab's message first; the steps then wait for its answers. **End the stream after the steps** closes the sending side of a client or bidirectional stream once the steps are done (not while a step keeps sending). What the steps do shows above the messages, for example **On connect: got a JSON message where $.message == hi after 3 ms**. While a stream is open, the send button on a saved message sends it.

### In collection runs

gRPC requests run in the Runner tab, in [`zorvik run`](../../cli/run/), in runs an AI agent starts, and when an agent sends one with `send_request`. Each call runs to its end:

- **Unary** calls send the message. **Server streams** send it and read the answers. **Client and bidirectional streams** run their **On connect** steps (or send the message when there are none), then end the sending side.
- Each **Wait for** step is a test; after the first failure the rest are skipped.
- Reading a stream stops after the request's **Settings → In collection runs**: **Stop after** received messages and **Time limit** (100 messages or 10 seconds by default, as for [event streams](../../testing/repeat-and-streams/#event-streams-in-runs)); stopping there is not a failure.
- The result's status is `200` for `OK`, otherwise the HTTP status gRPC maps the code to (`NOT_FOUND` is `404`, `UNAVAILABLE` `503`, …), so a call that doesn't end `OK` fails. The status line reads `NOT_FOUND · user 42 not found`, and the headers include the trailers.
- Scripts run around the call: `pm.response.json()` is the answer message (unary) or the received messages as an array (streams).

```js
pm.test("Greets Ada", () => pm.expect(pm.response.json().message).to.include("Ada"));
```

### Results

The result pane shows the final status (`OK`, `NOT_FOUND`, …) with its message, the total time, and these tabs:

| Tab | Contents |
|---|---|
| **Messages** | Received messages (and, for streams, sent ones) with time and size. Streams keep the newest 5,000. |
| **Headers** | Response headers (initial metadata). Empty for a trailers-only answer. |
| **Trailers** | Trailing metadata, including `grpc-status` and `grpc-message`. |
| **Request metadata** | The headers as sent: the gRPC ones, then yours. |
| **Timing** | DNS, connect, TLS and the call. |

`grpc-status-details-bin` is decoded as `google.rpc.Status` (with known detail types) when possible. A status Zorvik set itself (cancelled, deadline exceeded, connection lost, an HTTP answer that isn't gRPC) is marked as such rather than coming from the server. Problems that don't end the call, such as a message that could not be decoded, are listed as warnings.

Unary calls are recorded in history: an `OK` status counts as success, any other status as an error such as `NOT_FOUND: user 42 not found`.

## TLS, proxies and limits

- **TLS**: `grpcs://` checks the server certificate against this computer's trusted certificates plus the ones in **Settings → Certificates**; a client certificate set there is used for mutual TLS. **Verify TLS certificates** in the request's **Settings** tab (or the app default) turns the check off. See [TLS & certificates](../../requests/tls-and-certificates/).
- **Proxy**: when an HTTP proxy applies to the host, both `grpc://` and `grpcs://` go through a `CONNECT` tunnel.
- **Compression**: gzip-compressed responses are accepted; requests are sent uncompressed.
- **Message size**: at most 64 MB per message, sent or received (a compressed message counts at its decompressed size).
- **gRPC-Web** is not supported, and Zorvik has no gRPC server or mock.

## Saved format

```yaml title="requests/Greeter/Say hello.yaml"
name: Say hello
kind: grpc
method: helloworld.Greeter/SayHello
url: grpc://localhost:50051
headers:
  - key: x-request-id
    value: "{{$uuid}}"
body:
  type: json
  text: '{"name": "Ada"}'
grpc:
  protoFiles:
    - protos/helloworld.proto
  importPaths:
    - protos
  messages:
    - name: Hello
      format: json
      text: '{"name": "{{user}}"}'
  onConnect:
    - type: send
      message: Hello
    - type: wait
      match: json
      pattern: $.message == Hello Ada
  endAfterSteps: true
```

| Field | Description |
|---|---|
| `kind` | `grpc`. |
| `method` | `package.Service/Method`. |
| `url` | The server address. |
| `headers` | Metadata. |
| `body.text` | The JSON message. |
| `grpc.protoFiles` | `.proto` files, relative to the workspace or absolute. Empty or missing: server reflection. |
| `grpc.importPaths` | Folders searched for imports (each proto file's folder is always searched). |
| `grpc.messages` | Saved messages for streams. |
| `grpc.onConnect` | Steps once a stream is open. |
| `grpc.endAfterSteps` | End the sending side after the steps, in the app. |

:::tip[AI agents]
Agents can list a server's services and methods with the `grpc_describe` tool, and make any call with `send_request`: streams run to their end as in collection runs, and the answer lists the received and sent messages, the steps and why reading stopped. See [Agent tools](../../agents/tools/).
:::
