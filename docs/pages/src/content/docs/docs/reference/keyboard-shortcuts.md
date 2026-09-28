---
title: Keyboard shortcuts
description: Every keyboard shortcut in Zorvik, on macOS, Windows and Linux, and when each one works.
sidebar:
  order: 3
---

In this table, **Mod** is <kbd>⌘</kbd> (Command) on macOS and <kbd>Ctrl</kbd> on Windows and Linux. The **keyboard** button in the title bar shows the main shortcuts inside the app.

## App shortcuts

| Shortcut | Does |
|---|---|
| <kbd>Mod</kbd> <kbd>Enter</kbd> | The main action of the tab: send the request (or connect a WebSocket, Socket.IO, SSE, TCP, MQTT … session, or subscribe), start or restart a server, start or stop a load test, start or stop a collection run. |
| <kbd>Mod</kbd> <kbd>S</kbd> | Save the tab (request, load test, server …). |
| <kbd>Mod</kbd> <kbd>N</kbd> | New HTTP request. |
| <kbd>Mod</kbd> <kbd>W</kbd> | Close the tab (asks first when it has unsaved changes). |
| <kbd>Mod</kbd> <kbd>K</kbd> or <kbd>Mod</kbd> <kbd>P</kbd> | Command palette: go to a request, load test, server or tool. |
| <kbd>Mod</kbd> <kbd>L</kbd> | Put the cursor in the URL. |
| <kbd>Ctrl</kbd> <kbd>Tab</kbd> | Next tab (<kbd>Ctrl</kbd> on every system, also on macOS). |
| <kbd>Ctrl</kbd> <kbd>Shift</kbd> <kbd>Tab</kbd> | Previous tab. |
| <kbd>Mod</kbd> <kbd>E</kbd> | Environments. |
| <kbd>Mod</kbd> <kbd>,</kbd> | Settings. |
| <kbd>Mod</kbd> <kbd>+</kbd> (or <kbd>Mod</kbd> <kbd>=</kbd>) | Zoom in. |
| <kbd>Mod</kbd> <kbd>−</kbd> | Zoom out. |
| <kbd>Mod</kbd> <kbd>0</kbd> | Reset the zoom to 100 %. |
| <kbd>⌘</kbd> <kbd>Q</kbd> (macOS) | Quit Zorvik. Asks first while servers or a load test run. |

## In editors and responses

Body, script, GraphQL and response editors are code editors:

| Shortcut | Does |
|---|---|
| <kbd>Mod</kbd> <kbd>F</kbd> | Search in the editor or response (with replace in editable editors). |
| <kbd>Mod</kbd> <kbd>Enter</kbd> | In an editor that has its own action (for example a message composer), that action; otherwise the tab's main action above. |
| <kbd>Mod</kbd> <kbd>Z</kbd> / <kbd>Mod</kbd> <kbd>Shift</kbd> <kbd>Z</kbd> | Undo / redo. |
| <kbd>Tab</kbd> / <kbd>Shift</kbd> <kbd>Tab</kbd> | Indent / outdent. To leave an editor with the keyboard, press <kbd>Esc</kbd>, then <kbd>Tab</kbd>. |

Editors also close brackets and quotes as you type, fold blocks, and complete `{{variables}}`.

## Dialogs

| Key | Does |
|---|---|
| <kbd>Esc</kbd> | Close the dialog. With unsaved edits it asks first; an open autocomplete list closes before the dialog. In an AI agent's approval dialog, <kbd>Esc</kbd> means **Deny**. |
| <kbd>Enter</kbd> | Confirm the focused button. In an agent's approval dialog, the focus starts on **Deny**. |
| <kbd>↑</kbd> <kbd>↓</kbd> <kbd>Enter</kbd> | Move through and pick in the command palette and in the request picker of a load test. |

## When shortcuts work

- The app shortcuts work when a workspace is open, in the workbench. They are off in the Training Bootcamp's **Academy** view.
- While a dialog is open, only the zoom keys work; the other keys belong to the dialog.
- In the tab bar, <kbd>←</kbd> and <kbd>→</kbd> move between tabs (<kbd>Home</kbd> and <kbd>End</kbd> to the first and last) and <kbd>Enter</kbd> or <kbd>Space</kbd> opens one.
- The zoom keys work everywhere, also on the welcome screen and in dialogs.
- Combinations with <kbd>Alt</kbd> / <kbd>Option</kbd> are left to the system.
- On macOS, Zorvik uses <kbd>⌘</kbd> so that <kbd>Ctrl</kbd> keeps its text-editing meaning in fields.
- A held <kbd>Mod</kbd> <kbd>Enter</kbd> on a runner tab doesn't stop the run it just started.
