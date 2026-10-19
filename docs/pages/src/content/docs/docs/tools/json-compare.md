---
title: JSON compare
description: See what was added, removed or changed between two JSON documents or two responses, path by path.
sidebar:
  order: 4
---

**JSON compare** lists the differences between two JSON documents by path: what the right one adds, what it removes and which values changed. Use it to compare a response with the one from yesterday, staging with production, or a response with the example in your docs. Open it from the **Tools** sidebar.

## Two documents

Paste JSON into **Left** and **Right**, or click **From history** above either side to use the body of a response you sent from Zorvik. The comparison runs as you type, once both sides hold JSON.

- **From history** lists the last 50 responses sent from the app (newest first, with status, method and URL). Their bodies are kept in memory while the app runs; only the first 64,000 characters of each are kept, and the app says so when you pick one that was longer.
- **Swap sides** exchanges the two documents.
- **✕** clears one side.

Each document may be up to 20 MB. A side that isn't JSON is reported by name, for example *The left document isn't valid JSON: expected value at line 1 column 1*.

## What counts as a difference

- **Key order** and **formatting** never count.
- Numbers compare by value: `1`, `1.0` and `1e0` are the same. Whole numbers too large for a double (above 2^53) are compared exactly.
- **Arrays** are aligned like lines in a text diff: an item inserted at the start is one addition, not every item changed, and an item that changed in place is compared field by field (`$.items[3].price`).
- **Ignore array order** matches array items in any order, for lists whose order isn't guaranteed. An item counts as unchanged when the other array has an equal one anywhere; the rest are added or removed.

## The result

**The documents are the same** when nothing differs. Otherwise the tool shows how many differences there are, and buttons to see **All**, or only **Changed**, **Added** or **Removed** ones (each with its count). Every row has:

| Column | Shows |
|---|---|
| ~ / + / − | Changed, added or removed |
| **Path** | Where, as a [JSONPath](https://www.rfc-editor.org/rfc/rfc9535) such as `$.items[2].name` or `$['odd key']`, with a copy button. It works in the [Filter playground](../filter-playground/), the response filter and load test captures. |
| **Left** / **Right** | The value on each side, as compact JSON (long values are shortened) |

Array indexes in paths are positions in the left document, except for added items, which are positions in the right one.

**Copy as text** copies the differences shown, one per line:

```text
~ $.id: 7 → "7"
- $.meta: {"version":"1.0"}
+ $.tags[2]: "new"
```

The first 1,000 differences are listed; the counts include all of them.
