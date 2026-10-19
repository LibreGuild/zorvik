---
title: Filter playground
description: Try JSONPath, jq and XPath expressions on JSON or XML you paste or pick from history, as you type.
sidebar:
  order: 5
---

The **Filter playground** runs a JSONPath, jq or XPath expression on a document you paste and shows the result as you type. Use it to work out an expression before you use it in the [response filter](../../requests/responses/#filter-the-body), a load test capture or an agent's `send_request`, or to pull data out of JSON copied from somewhere else. Open it from the **Tools** sidebar.

## Use it

1. Pick the language: **JSONPath**, **jq** or **XPath**.
2. Paste the document on the left (**JSON**, or **XML or HTML** for XPath), or click **From history** to use the body of a response you sent from Zorvik.
3. Type an **Expression**, or click one of the examples under it (**Try:**) to start from.

The **Result** shows what matches, with the number of matches next to the expression and a copy button. A mistake shows what is wrong in red, for example *Not a valid jq expression: expected a closing bracket*, or *The text isn't valid JSON (…)*.

| Language | Runs | Shows |
|---|---|---|
| **JSONPath** ([RFC 9535](https://www.rfc-editor.org/rfc/rfc9535)) | In Rust, exactly like the response filter | The matches as a JSON array |
| **jq** | In Rust, exactly like the response filter | Each result on its own, like the `jq` command |
| **XPath** 1.0 | In the app's web view | Nodes as markup, numbers, strings and booleans as text. Text that starts with `<!doctype html` or `<html` is read as HTML (leniently); other text must be well-formed XML. |

At most 10,000 results are shown, as in the response filter.

## Examples

| Language | Expression | Gives |
|---|---|---|
| JSONPath | `$..id` | Every `id`, at any depth |
| JSONPath | `$.items[-1]` | The last item |
| JSONPath | `$.items[?@.price > 10].name` | Names of items that cost more than 10 |
| JSONPath | `$.items[?length(@.tags) > 0]` | Items with at least one tag |
| jq | `.items \| length` | The number of items |
| jq | `.items \| map({id, name})` | Each item with only `id` and `name` |
| jq | `[.items[].price] \| add` | The sum of the prices |
| XPath | `count(//item)` | The number of `item` elements |
| XPath | `//*[@id]` | Every element with an `id` attribute |

:::tip
Load test captures take a simpler path (`$.items[0].id`, `$.items[-1].id`, `$['odd key']`, no wildcards or filters): a path that gives one value here gives the same value there. See [Data and captures](../../load-testing/data-and-captures/).
:::
