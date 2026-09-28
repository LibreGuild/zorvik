// The response filter: JSONPath and jq run in Rust over the full body (response.filter);
// XPath runs here, with the web view's own XPath engine, over XML or HTML.
import type { FilterLanguage } from "../../bindings/FilterLanguage";

export type FilterKind = FilterLanguage | "xpath";

export const FILTER_KINDS: { id: FilterKind; label: string; placeholder: string }[] = [
  { id: "jsonPath", label: "JSONPath", placeholder: "$.items[?@.price > 10].name" },
  { id: "jq", label: "jq", placeholder: ".items[] | select(.price > 10) | .name" },
  { id: "xpath", label: "XPath", placeholder: "//item[price > 10]/name" },
];

/** The filters that make sense for a body of this content type. */
export function kindsFor(contentType: string | null): FilterKind[] {
  const ct = (contentType ?? "").toLowerCase();
  if (ct.includes("xml") || ct.includes("html")) return ["xpath"];
  if (ct.includes("json")) return ["jsonPath", "jq"];
  return ["jsonPath", "jq", "xpath"];
}

export interface FilterOutput {
  text: string;
  count: number;
}

/** XPath 1.0 over an XML (or HTML) document; nodes come back as markup, values as text. */
export function xpath(source: string, expression: string, html: boolean): FilterOutput {
  const doc = new DOMParser().parseFromString(source, html ? "text/html" : "application/xml");
  if (!html && doc.getElementsByTagName("parsererror").length) {
    throw new Error("The response isn't valid XML, so it can't be filtered");
  }
  // Prefixes resolve through the document (xmlns declarations on the root element).
  const root = doc.documentElement;
  const resolver = (prefix: string | null) => (prefix ? root.lookupNamespaceURI(prefix) : root.namespaceURI);
  let result: XPathResult;
  try {
    result = doc.evaluate(expression, doc, resolver, XPathResult.ANY_TYPE, null);
  } catch (e) {
    throw new Error(`Not a valid XPath expression: ${e instanceof Error ? e.message : String(e)}`);
  }
  switch (result.resultType) {
    case XPathResult.NUMBER_TYPE:
      return { text: String(result.numberValue), count: 1 };
    case XPathResult.STRING_TYPE:
      return { text: result.stringValue, count: 1 };
    case XPathResult.BOOLEAN_TYPE:
      return { text: String(result.booleanValue), count: 1 };
    default: {
      const serializer = new XMLSerializer();
      const out: string[] = [];
      for (let node = result.iterateNext(); node; node = result.iterateNext()) {
        if (node.nodeType === Node.ATTRIBUTE_NODE || node.nodeType === Node.TEXT_NODE || node.nodeType === Node.CDATA_SECTION_NODE) {
          out.push(node.nodeValue ?? "");
        } else if (html && node instanceof Element) {
          out.push(node.outerHTML);
        } else {
          out.push(serializer.serializeToString(node));
        }
      }
      return { text: out.join("\n"), count: out.length };
    }
  }
}
