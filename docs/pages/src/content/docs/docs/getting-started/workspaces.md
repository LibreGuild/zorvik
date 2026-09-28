---
title: Workspaces
description: How a Zorvik workspace is laid out on disk, and how to create, open, organize and share one.
sidebar:
  order: 3
---

A **workspace** is a folder of small, readable YAML files: your requests and folders, environments, mock servers, load tests and imported API specs. Zorvik reads and writes these files directly, so you can commit the folder to Git, review changes in pull requests and pull your teammates' work.

What belongs to one computer only, such as secret values, cookies, OAuth tokens and history, is kept in the app data folder instead, never in the workspace. See [Secrets](../../variables/secrets/).

## What's in a workspace folder

```text
my-api/
  zorvik.yaml              # name, id, workspace variables, default auth and headers, scripts
  environments/
    Local.yaml             # one file per environment
    Staging.yaml
  requests/
    Get status.yaml        # one file per request
    Users/                 # folders are folders
      _folder.yaml         # optional: the folder's order, auth, headers, scripts and docs
      List users.yaml
  servers/
    Payments mock.yaml     # mock APIs and other servers
  loadtests/
    Checkout smoke.yaml    # load test plans
  specs/
    Payments API.yaml      # OpenAPI documents kept by imports
```

`requests/` and `environments/` are created with the workspace; `servers/`, `loadtests/` and `specs/` appear when you first need them. A folder is a workspace as long as it contains `zorvik.yaml`.

A saved request looks like this:

```yaml title="requests/Users/Create user.yaml"
name: Create user
seq: 2
method: POST
url: "{{baseUrl}}/users?notify=true"
headers:
  - key: X-Request-ID
    value: "{{$uuid}}"
body:
  type: json
  text: |
    { "name": "{{name}}" }
```

Default values are left out, so files stay small and diffs stay clean. [Workspace format](../../reference/workspace-format/) documents every field.

## Create a workspace

On the welcome screen, choose **New workspace**:

| Field | Meaning |
|---|---|
| **Name** | The workspace's display name (default `My Workspace`). You can change it later. |
| **Folder** | Where the files go. Type a path or choose **Browse…**. The folder may already exist (an empty folder, or a project or Git repository), but must not already contain a `zorvik.yaml`. |

Choose **Create workspace**. Zorvik writes `zorvik.yaml` and creates `requests/` and `environments/`. Other files in the folder are left alone.

## Open a workspace

- **Welcome screen → Open workspace**, then pick the folder that contains `zorvik.yaml`.
- **Welcome screen → Recent**: workspaces you opened before, newest first. Hover one and choose **×** to remove it from the list (the folder is not touched).
- **Workspace menu** (the workspace name at the top left of the title bar) → **Open workspace…**, or one of the recent workspaces listed there.

If the folder you pick has no `zorvik.yaml` yet, Zorvik asks **Create a workspace here?** Choose **Create workspace** to make it one; existing files are left untouched.

One workspace is open at a time. Switching closes the current one's tabs and brings back the tabs you had open in the other one. Zorvik also reopens the last workspace when it starts.

The workspace menu contains:

| Item | Does |
|---|---|
| **Training Bootcamp** | Opens the built-in course and its own workspace (see [Training Bootcamp](../../bootcamp/training-bootcamp/)) |
| **Workspace settings…** | Name, default auth, default headers and scripts (below) |
| **Open workspace…** | Opens another folder |
| *Recent workspaces* | Up to six other workspaces you opened recently |
| **Close workspace** | Back to the welcome screen |

## Workspace settings

**Workspace menu → Workspace settings…** has four tabs:

| Tab | What it holds |
|---|---|
| **General** | **Name**, and the **Location** of the folder (read-only) |
| **Default auth** | Auth for every request left on **Inherit from parent** whose folders don't set one. The default is **No auth**. See [Auth](../../requests/auth/#inheritance). |
| **Default headers** | Headers sent with every request in the workspace unless a folder or the request sets the same header |
| **Scripts** | Pre-request and post-response scripts that run around every request. See [Scripts & tests](../../scripting/overview/). |

Workspace **variables** are edited under **Environments & variables** (<kbd>Mod</kbd>+<kbd>E</kbd>), not here. See [Variables & environments](../../variables/variables-and-environments/#workspace-variables).

All of this is stored in `zorvik.yaml`.

## Requests

The **Collection** section of the sidebar shows the requests of `requests/` as a tree.

**Create a request:**

- Press <kbd>Mod</kbd>+<kbd>N</kbd> for a new, unsaved HTTP request in a tab. <kbd>Mod</kbd>+<kbd>S</kbd> then asks for a name and a folder.
- Or choose **+** at the top of the sidebar (or right-click an empty part of the tree, or right-click a folder) and pick a kind: **HTTP request**, **GraphQL request**, **gRPC request**, **WebSocket**, **Event stream (SSE)**, **TCP connection**, **UDP socket**, **DNS query**, **MQTT client**, **Socket.IO client** or **MCP call (AI tools)**. Zorvik asks for a name and saves the request right away.

**Right-click a request** (or hover it and choose **⋯**) for:

| Item | Does |
|---|---|
| **Open** | Opens it in a tab (a single click does too) |
| **Copy as cURL or code…** | See [Copy as cURL or code](../../requests/import-export/#copy-as-curl-or-code) |
| **Load test…** | Starts a load test of this HTTP request. See [Load testing](../../load-testing/overview/). |
| **Rename…** | Also renames its file |
| **Duplicate** | Adds "*name* copy" to the same folder |
| **Move to…** | Moves it to another folder |
| **Delete** | Moves the file to your system's trash |

Keyboard shortcuts in the tree: <kbd>Enter</kbd> opens a request (or expands a folder), <kbd>F2</kbd> renames, <kbd>Delete</kbd> (or <kbd>⌘</kbd>+<kbd>Backspace</kbd> on macOS) deletes.

**Filter** at the top of the tree shows requests whose name contains the text, or whose method is exactly the text (type `post` to see every POST request). A folder whose name contains the text is shown as well.

## Folders

Folders are real folders under `requests/`. Each can have a `_folder.yaml` with its settings.

**Right-click a folder** for:

| Item | Does |
|---|---|
| **New …** | A request of any kind, or **New folder**, inside this folder |
| **Folder settings…** | Auth, headers, scripts and docs for everything inside (below) |
| **Import into folder…** | Imports Postman, OpenAPI or cURL into this folder. See [Import & export](../../requests/import-export/). |
| **Run…** | Runs the folder's requests in order. See [Collection runner](../../testing/collection-runner/). |
| **Mock this folder…** | Creates a mock API from its requests. See [Mock servers](../../servers/mock-api/). |
| **Update from API spec…** | Only on folders imported from an OpenAPI document. See [Update from the spec](../../requests/import-export/#update-from-the-api-spec). |
| **Load test this folder…** | A load test of its HTTP requests |
| **Rename…**, **Duplicate**, **Move to…**, **Delete** | As for requests; a folder moves or goes to the trash with everything in it |

The collection itself has the same **Import…**, **Run…**, **Mock the collection…** and **Load test the collection…** items in the **+** menu.

### Folder settings

**Folder settings…** opens a dialog titled *Folder: name* with these tabs:

| Tab | Applies to |
|---|---|
| **Auth** | Requests inside that are left on **Inherit from parent**. The folder itself can inherit from its parent folder or the workspace. |
| **Headers** | Every request inside. A request, or a folder deeper down, that sets the same header replaces it. |
| **Scripts** | Pre-request and post-response scripts that run for every request inside |
| **Docs** | Notes about the folder, in Markdown |
| **API spec** | Only for folders imported from an OpenAPI document: where the kept document is, and **Check responses against the spec** |

### How settings flow down

A request gets its settings from the workspace, then each folder around it from the outside in, then its own:

- **Headers** add up. When two levels set a header with the same name (compared without regard to case), the inner one wins.
- **Auth** comes from the nearest level that doesn't inherit. See [Auth](../../requests/auth/#inheritance).
- **Scripts** all run, outermost first: workspace, folders, then the request.

:::note
A new request that isn't saved yet only inherits from the workspace. It picks up folder settings once it is saved into a folder.
:::

## Order and names

- Items are shown in the order you gave them by dragging. The position is stored as `seq` in each file (for a folder, in its `_folder.yaml`). Items with the same position are sorted folders first, then by name.
- File names come from item names, made safe for Windows, macOS and Linux: `< > : " / \ | ? *` and control characters become `-`, leading dots and trailing dots or spaces are removed, and names are cut to 80 characters. When a name is taken (ignoring case), a number is added: `Get user 2.yaml`.
- The display name is stored inside the file, so it can contain any character. Names can't be empty and have at most 200 characters.
- Renaming an item renames its file or folder.

## Working with Git

- Commit the whole workspace folder. It contains no secret values, cookies or tokens.
- Zorvik watches the folder. When files change on disk (a `git pull`, a branch switch, an edit in another editor), the sidebar, environments and open tabs update by themselves. A tab with unsaved changes keeps your edits. When a request's file is deleted, moved away or can no longer be read, its tab shows a warning; save it to write your version again.
- A file that isn't valid YAML shows in the sidebar with a warning icon (hover it for the error) instead of breaking the workspace.
- `zorvik.yaml` carries `version: 1`. A workspace written by a newer Zorvik with a newer format is refused with a message asking you to update the app.

Each teammate has their own:

| Per computer | Where |
|---|---|
| Secret variable values | App data folder, see [Secrets](../../variables/secrets/) |
| Values set by scripts | App data folder, see [Values set by scripts](../../variables/variables-and-environments/#values-set-by-scripts) |
| The active environment | App data folder |
| Cookies and OAuth tokens | App data folder |
| History | App data folder |
| Open tabs and sidebar layout | The app's local storage |

Secret values, cookies and tokens are keyed by the workspace's `id` (in `zorvik.yaml`) **and** its folder on this computer, so a copy of the workspace in another folder, or another workspace that reuses the same `id`, doesn't get them.

## Limits and safety

Workspace files come from Git and are treated as untrusted input:

- Symbolic links inside the workspace are not followed.
- Folder nesting deeper than 32 levels is not shown.
- YAML files larger than 50 MB are refused.
- Request bodies can only upload files from inside the workspace folder, unless you allow otherwise in **Settings → Data & privacy → Files outside the workspace**. See [Request bodies](../../requests/bodies/#files).
