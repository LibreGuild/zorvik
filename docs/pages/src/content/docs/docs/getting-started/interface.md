---
title: The interface
description: A tour of the Zorvik window, including the title bar, the left rail, tabs, the command palette, keyboard shortcuts, layout, themes, zoom and fonts.
sidebar:
  order: 4
---

![The Zorvik window: the left rail and the collection, a request with its tabs, and the JSON response beside it](/zorvik/shots/request-dark.webp)

With a workspace open, the window has five parts:

1. **The title bar** at the top: the workspace, the active environment and app-wide controls.
2. **The left rail**: icons that switch what the sidebar shows.
3. **The sidebar**: the collection, load tests, servers, tools, history, AI agents or docs.
4. **The tab bar**: everything you have open.
5. **The work area**: for a request, the request editor and its response.

In this documentation, <kbd>Mod</kbd> means <kbd>⌘</kbd> on macOS and <kbd>Ctrl</kbd> on Windows and Linux.

## Title bar

Zorvik draws its own title bar. Drag any empty part of it to move the window. On macOS the traffic-light buttons sit on its left; on Windows and Linux, **Minimize**, **Maximize** and **Close** are on its right.

**On the left:**

- **The workspace menu**: the name of the open workspace. It opens the Training Bootcamp, **Workspace settings…**, **Open workspace…**, recent workspaces and **Close workspace**. See [Workspaces](../workspaces/#open-a-workspace).
- **Workbench | Academy**: only in the Training Bootcamp workspace. It switches between the course and the normal app. See [Training Bootcamp](../../bootcamp/training-bootcamp/).

**On the right**, some items appear only while something is going on:

| Item | When | Does |
|---|---|---|
| Agent name, e.g. **Claude Code** | An AI agent is connected | Shows the agent's activity in the **AI agents** sidebar; a number shows how many questions wait for you. The square button disconnects the agent. See [AI agents](../../agents/setup/). |
| **Load test** and a timer | A load test runs | Opens the load test; the square button stops it |
| **N running** | Servers are running | Lists every running server, in any workspace, to open or stop them; **Stop all** stops them all |
| Environment picker | Always | Shows the active environment (or **No environment**). Choose another one, or **Manage environments…**. See [Variables & environments](../../variables/variables-and-environments/). |
| Cookie icon | Always | The workspace's cookie jar. See [Cookies](../../requests/cookies/). |
| Theme icon | Always | Cycles the theme: match system → dark → light |
| Keyboard icon | Always | The list of keyboard shortcuts |
| Gear icon | Always | **Settings** (<kbd>Mod</kbd>+<kbd>,</kbd>) |

## Left rail and sidebar

The rail on the far left switches the sidebar between seven sections. Hover an icon to see its name.

| Section | Shows |
|---|---|
| **Collection** | The requests and folders of the workspace, with **Filter** and **+** (new request, new folder, import, run, mock, load test). See [Workspaces](../workspaces/#requests). |
| **Load tests** | The workspace's load tests. A pulsing dot on the icon means one is running. See [Load testing](../../load-testing/overview/). |
| **Servers** | Mock APIs and other servers. A badge on the icon counts the running ones. See [Mock servers](../../servers/mock-api/). |
| **Tools** | Network tools that open in their own tab: **TLS inspector**, **DNS lookup**, **Port check**, **Ping**, **Network interfaces**, **Encoders** and **HTTP/3 check** |
| **History** | Every request you sent in this workspace. See [Responses](../../requests/responses/#history). |
| **AI agents** | What connected agents do, and how to connect one. A green dot on the icon means an agent is connected. See [AI agents](../../agents/setup/). |
| **Docs** | Short in-app docs for every feature |

Drag the border between the sidebar and the work area to resize the sidebar (200 to 560 pixels). The section you chose and the sidebar width are remembered.

## Tabs

Every request, server, load test, collection run and tool opens in a tab.

- **New tab**: the **+** at the right end of the tab bar offers every kind of request. <kbd>Mod</kbd>+<kbd>N</kbd> opens a new HTTP request.
- **Badges**: a request tab shows its method (or `GQL`, `WS`, `SSE`, `TCP`, `UDP`, `DNS`, `MQTT`, `gRPC`, `SIO`), servers and load tests their kind. Hover a tab to see its file.
- **Unsaved**: a tab that was never saved shows its name in italics. A tab with unsaved changes shows a dot where the close button is; hover it to close.
- **Live**: a green dot means a connection is open or a request is running; a pulsing dot means a load test or collection run is going.
- **Close**: <kbd>Mod</kbd>+<kbd>W</kbd>, the **×**, or a middle click. Right-click a tab for **Close** and **Close others**. Closing a tab with unsaved changes asks **Discard unsaved changes?** A new tab you haven't typed anything into closes without asking.
- **Switch**: click a tab, or press <kbd>Ctrl</kbd>+<kbd>Tab</kbd> (next) and <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>Tab</kbd> (previous). This is <kbd>Ctrl</kbd> on every system, also on macOS.

Opening a request that already has a tab switches to that tab. Tabs are remembered per workspace: when you reopen a workspace, its tabs come back as you left them, including unsaved changes.

## Command palette

Press <kbd>Mod</kbd>+<kbd>K</kbd> (or <kbd>Mod</kbd>+<kbd>P</kbd>) to open **Go to**. Type to search requests, load tests, servers and tools; it also has **Run collection** and **Open the Academy**.

- Every word you type must appear in the item's name, its folder path or its kind. For example, `users post` finds POST requests in a `Users` folder, and `server` lists all servers.
- <kbd>↑</kbd> and <kbd>↓</kbd> move, <kbd>Enter</kbd> opens, <kbd>Esc</kbd> closes.
- It shows up to 100 results.

<kbd>Mod</kbd>+<kbd>K</kbd> works even while another dialog is open.

## Keyboard shortcuts

| Shortcut | Does |
|---|---|
| <kbd>Mod</kbd>+<kbd>Enter</kbd> | Send the request, connect, start the server, start the load test, or start and stop a collection run |
| <kbd>Mod</kbd>+<kbd>S</kbd> | Save |
| <kbd>Mod</kbd>+<kbd>N</kbd> | New HTTP request |
| <kbd>Mod</kbd>+<kbd>W</kbd> | Close the tab |
| <kbd>Mod</kbd>+<kbd>K</kbd> or <kbd>Mod</kbd>+<kbd>P</kbd> | Command palette |
| <kbd>Mod</kbd>+<kbd>L</kbd> | Jump to the URL field |
| <kbd>Mod</kbd>+<kbd>E</kbd> | Environments & variables |
| <kbd>Mod</kbd>+<kbd>,</kbd> | Settings |
| <kbd>Ctrl</kbd>+<kbd>Tab</kbd> / <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>Tab</kbd> | Next / previous tab |
| <kbd>Mod</kbd>+<kbd>F</kbd> | Search in an editor or a response body |
| <kbd>Mod</kbd>+<kbd>+</kbd> / <kbd>Mod</kbd>+<kbd>−</kbd> | Zoom in / out |
| <kbd>Mod</kbd>+<kbd>0</kbd> | Reset the zoom to 100 % |

- Most shortcuts work only with a workspace open, and not while a dialog is open (the command palette shortcut is the exception). Zoom works everywhere.
- In the URL field, and in the rows of the query parameter, header and form tables, <kbd>Enter</kbd> sends the request too.
- The keyboard icon in the title bar shows this list in the app.

See also the [keyboard shortcuts reference](../../reference/keyboard-shortcuts/).

## Layout

The response can be **beside** the request (the default) or **below** it:

- Choose the layout icon at the right end of the request's tab strip (**Stack response below** or **Show response beside**).
- Or set **Settings → General → Response position**: **Beside the request** or **Below the request**.

Drag the border between the request and the response to share the space between them; each side keeps at least 20 %. The layout, the split and the sidebar width are stored on this computer, not in the workspace.

## Themes

Zorvik has a light and a dark theme.

- **Settings → General → Theme**: **Match system** (the default), **Dark** or **Light**. **Match system** follows your operating system, also when it changes during the day.
- The theme icon in the title bar cycles through match system, dark and light.

## Zoom

Zoom makes the whole interface bigger or smaller: text, icons and layout together.

- <kbd>Mod</kbd>+<kbd>+</kbd> and <kbd>Mod</kbd>+<kbd>−</kbd> step through 50, 67, 75, 80, 90, 100, 110, 125, 150, 175 and 200 %. <kbd>Mod</kbd>+<kbd>0</kbd> goes back to 100 %.
- **Settings → General → Zoom** has the same steps, with **Zoom out**, **Zoom in** and **Reset** buttons.

Zoom applies at once and is saved right away. It works on the welcome screen and in dialogs too.

## Fonts

**Settings → General** has three font settings. Font changes preview while the Settings dialog is open; choose **Save** to keep them.

| Setting | Applies to | Default |
|---|---|---|
| **Interface font** | Menus, lists, forms and tabs | The system font |
| **Code font** | Editors, response bodies, headers and other code | The system's monospace font |
| **Code text size** | Editors and response bodies, on top of the zoom: 10 to 24 px | 13 px |
| **Ligatures** | Draw `=>`, `!=` and `>=` as single symbols with code fonts that have them (Fira Code, JetBrains Mono, Cascadia Code) | Off |

Type any installed font's name. The suggestions list the common fonts installed on this computer. A font that isn't installed says **Not installed on this computer: the default font is used.** Choose **Default** to go back to the system font.

Theme, zoom and fonts are stored in the app's settings on this computer. See [Settings](../../reference/settings/).
