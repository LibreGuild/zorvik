// Lesson code spans: `{{name}}` is a variable chip, `{{=name}}` the lab's value itself
// (for fields that don't take variables, such as a port).
import { cleanup, render } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { TooltipProvider } from "../ui";
import { Inline } from "./Markdown";

afterEach(cleanup);

const codes = (container: HTMLElement) => [...container.querySelectorAll("code")].map((c) => c.textContent);

describe("lesson variables", () => {
  it("shows the value for {{=name}} and keeps {{name}} a variable", () => {
    const vars = { api_port: "51234", docs: "http://127.0.0.1:5000", api: "http://127.0.0.1:5001" };
    const { container } = render(
      <TooltipProvider>
        <Inline text="Port `{{=api_port}}`, file `{{=docs}}/spec.yaml`, URL `{{api}}`" vars={vars} />
      </TooltipProvider>,
    );
    expect(codes(container)).toEqual(["51234", "http://127.0.0.1:5000/spec.yaml", "{{api}}"]);
    expect(container.querySelector("code.zv-var")?.textContent).toBe("{{api}}");
  });

  it("shows the variable while no lab runs", () => {
    const { container } = render(<Inline text="Port `{{=api_port}}`" />);
    expect(container.querySelector("code.zv-var")?.textContent).toBe("{{api_port}}");
  });
});
