// Saved responses of a request ("Save as example"): kept in the request's file, shown in
// its Examples tab, and answered by mocks built from the request.
import type { Example } from "../bindings/Example";
import type { KeyValue } from "../bindings/KeyValue";
import type { SendResult } from "../bindings/SendResult";
import { api } from "../lib/rpc";
import { isDirty, isRequestTab, saveTab, updateDraft, useTabs } from "./tabs";

/** Response headers that don't belong in an example: framing, the date, and cookies (they can hold a session). */
const SKIPPED = ["content-length", "transfer-encoding", "connection", "keep-alive", "date", "set-cookie", "content-encoding"];

export function exampleHeaders(headers: SendResult["meta"]["headers"]): KeyValue[] {
  return headers.filter((h) => !SKIPPED.includes(h.name.toLowerCase())).map((h) => ({ key: h.name, value: h.value, enabled: true }));
}

/** A name that isn't taken yet: "200 OK", "200 OK (2)"… */
export function exampleName(base: string, taken: string[]): string {
  if (!taken.includes(base)) return base;
  let n = 2;
  while (taken.includes(`${base} (${n})`)) n++;
  return `${base} (${n})`;
}

/**
 * Adds the response as an example of the tab's request. A saved request with no other
 * unsaved changes is saved at once; otherwise the example waits with the other changes.
 * Returns whether it was written to the file.
 */
export async function saveAsExample(tabId: string, result: SendResult): Promise<boolean> {
  const tab = useTabs.getState().tabs.find((t) => t.id === tabId);
  if (!isRequestTab(tab)) return false;
  const { text } = await api.responseText(result.responseId);
  const clean = !isDirty(tab) && !!tab.path;
  const status = result.meta.status;
  updateDraft(tabId, (r) => {
    const examples = r.examples ?? [];
    const example: Example = {
      name: exampleName(`${status} ${result.meta.statusText}`.trim(), examples.map((e) => e.name)),
      status,
      headers: exampleHeaders(result.meta.headers),
      body: text,
      // As typed (variables kept); its query tells mocks when to answer with this example.
      url: r.url,
    };
    return { ...r, examples: [...examples, example] };
  });
  return clean ? saveTab(tabId) : false;
}
