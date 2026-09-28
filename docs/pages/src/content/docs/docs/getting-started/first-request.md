---
title: Your first request
description: Create a workspace, send a request, read the response, save it and switch environments, step by step.
sidebar:
  order: 2
---

This walkthrough takes about five minutes. You'll create a workspace, send an HTTP request, save it into your collection and make it work against more than one server with an environment.

In the shortcuts below, <kbd>Mod</kbd> means <kbd>⌘</kbd> on macOS and <kbd>Ctrl</kbd> on Windows and Linux.

## 1. Create a workspace

A workspace is a folder of YAML files that holds your requests (see [Workspaces](../workspaces/)).

1. On the welcome screen, choose **New workspace**.
2. Give it a **Name** (the default is `My Workspace`).
3. Under **Folder**, type a path or choose **Browse…**. Pick an empty folder, or a folder inside your project's Git repository.
4. Choose **Create workspace**.

Zorvik writes `zorvik.yaml` plus empty `requests/` and `environments/` folders, and opens the workspace.

:::tip
Already have a folder with a `zorvik.yaml`, for example from a teammate's Git repository? Choose **Open workspace** instead.
:::

## 2. Create a request

Press <kbd>Mod</kbd>+<kbd>N</kbd>. A new tab opens with an HTTP `GET` request named "New request". Its name is in italics because it isn't saved yet, and the cursor is already in the URL field.

Type or paste a URL, for example:

```text
https://jsonplaceholder.typicode.com/todos/1
```

A URL without a scheme is sent as `http://`. To change the method, open the method menu on the left of the URL (see [HTTP requests](../../requests/http/)).

## 3. Send it

Press <kbd>Mod</kbd>+<kbd>Enter</kbd>, press <kbd>Enter</kbd> in the URL field, or choose **Send**.

While the request runs, the response pane shows a timer, and **Send** turns into **Cancel**.

## 4. Read the response

The line above the response shows:

- the status, such as `200 OK`, colored by class (2xx, 3xx, 4xx, 5xx)
- the total time and the body size (hover the size to see how many bytes came over the wire)
- the protocol (`HTTP/1.1`, `HTTP/2` or `HTTP/3`), and a lock when the connection used TLS

Below it are the response tabs:

| Tab | Shows |
|---|---|
| **Body** | The body as **Pretty** JSON, **Raw** text or a **Preview** of HTML and images |
| **Headers** | Every response header |
| **Cookies** | Cookies the response set |
| **Timing** | Where the time went: DNS lookup, TCP connect, TLS handshake, waiting for the first byte, download |
| **Info** | The connection, the TLS certificate, redirects and the exact request that was sent |

See [Responses](../../requests/responses/) for everything they can do.

## 5. Save it

Press <kbd>Mod</kbd>+<kbd>S</kbd>. Because the request isn't saved yet, the **Save request** dialog opens:

1. Enter a **Name**, for example `Get todo`.
2. Leave **Folder** on **Workspace root**, or pick a folder.
3. Choose **Save**.

The request is written to `requests/Get todo.yaml` and appears in the **Collection** sidebar. From now on, <kbd>Mod</kbd>+<kbd>S</kbd> saves it in place. The save button next to **Send** is highlighted whenever the tab has unsaved changes.

:::note
Sending doesn't save. What you send is always what is in the tab, saved or not.
:::

## 6. Organize your collection

In the **Collection** sidebar:

- Choose **+** (next to **Filter**) for **New folder**, or for a new request of any kind: HTTP, GraphQL, gRPC, WebSocket, Socket.IO, SSE, TCP, UDP, DNS or MQTT. Requests created this way ask for a name and are saved right away.
- Drag requests and folders to reorder them, or drop them onto a folder to move them there.
- Right-click an item for **Rename…**, **Duplicate**, **Move to…** and **Delete**. Deleted items go to your system's trash.
- Type in **Filter** to show only matching requests.

Folders can hold auth, headers and scripts that apply to every request inside them. See [Workspaces](../workspaces/#folders).

## 7. Add an environment

Environments let one request run against different servers, for example your laptop and a staging server.

1. Press <kbd>Mod</kbd>+<kbd>E</kbd> to open **Environments & variables**.
2. Next to **Environments**, choose **+** (**New environment**). Rename it `Local` in the name field.
3. Add a variable: `baseUrl` with the value `https://jsonplaceholder.typicode.com`.
4. Choose **Save changes**, then **Close**.
5. In the title bar, open the environment picker (it says **No environment**) and choose **Local**.
6. Change the request's URL to:

   ```text
   {{baseUrl}}/todos/1
   ```

`{{baseUrl}}` is highlighted as a known variable. Hover it to see its value and where it comes from. Send the request again: it goes to the same address.

Add a `Staging` environment with a different `baseUrl`, and switching environments in the title bar is all it takes to send the same request to another server. See [Variables & environments](../../variables/variables-and-environments/).

:::caution
If the URL's host is a variable that isn't defined (for example because no environment is selected), Zorvik doesn't send the request. It says which variable is missing and offers **Open environments**.
:::

## Next steps

- [HTTP requests](../../requests/http/): methods, query and path parameters, headers
- [Request bodies](../../requests/bodies/) and [Auth](../../requests/auth/)
- [Secrets](../../variables/secrets/): keep tokens out of your Git repository
- [Import](../../requests/import-export/) from Postman, OpenAPI or cURL
- [Scripts & tests](../../scripting/overview/) and the [collection runner](../../testing/collection-runner/)
- [The interface](../interface/): tabs, the command palette, shortcuts, themes and zoom
