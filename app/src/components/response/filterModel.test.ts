import { describe, expect, it } from "vitest";
import { kindsFor, xpath } from "./filterModel";

const XML = `<?xml version="1.0"?>
<catalog xmlns:p="urn:price">
  <book id="1"><title>Dune</title><p:price>9.5</p:price></book>
  <book id="2"><title>Emma</title><p:price>12</p:price></book>
</catalog>`;

describe("response filter", () => {
  it("offers the filters that fit the content type", () => {
    expect(kindsFor("application/json; charset=utf-8")).toEqual(["jsonPath", "jq"]);
    expect(kindsFor("text/xml")).toEqual(["xpath"]);
    expect(kindsFor("text/html")).toEqual(["xpath"]);
    expect(kindsFor(null)).toEqual(["jsonPath", "jq", "xpath"]);
  });

  it("selects XML nodes, attributes, text and values", () => {
    expect(xpath(XML, "//book[@id='2']/title", false)).toEqual({ text: "<title>Emma</title>", count: 1 });
    expect(xpath(XML, "//book/@id", false)).toEqual({ text: "1\n2", count: 2 });
    expect(xpath(XML, "//title/text()", false)).toEqual({ text: "Dune\nEmma", count: 2 });
    expect(xpath(XML, "count(//book)", false)).toEqual({ text: "2", count: 1 });
    expect(xpath(XML, "sum(//p:price)", false)).toEqual({ text: "21.5", count: 1 });
    expect(xpath(XML, "//nothing", false)).toEqual({ text: "", count: 0 });
  });

  it("works on HTML", () => {
    const html = "<html><body><ul><li class='a'>One</li><li>Two</li></ul></body></html>";
    expect(xpath(html, "//li[@class='a']", true).text).toBe('<li class="a">One</li>');
    expect(xpath(html, "count(//li)", true).text).toBe("2");
  });

  it("explains bad input", () => {
    expect(() => xpath("<a>", "//a", false)).toThrow(/isn't valid XML/);
    expect(() => xpath(XML, "//book[", false)).toThrow(/Not a valid XPath expression/);
  });
});
