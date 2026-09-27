import { describe, expect, it } from "vitest";
import { confirm, prompt, useDialogs } from "./dialogs";

describe("dialogs", () => {
  it("cancels a dialog that another one replaces", async () => {
    const first = confirm({ title: "Discard?", message: "" });
    const second = prompt({ title: "Name" });
    expect(await first).toBe(false);
    expect(useDialogs.getState().current?.title).toBe("Name");
    const current = useDialogs.getState().current;
    if (current?.kind === "prompt") current.resolve("x");
    expect(await second).toBe("x");
  });
});
