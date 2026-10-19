// Number fields: the text can be cleared and retyped; only allowed values are reported while
// typing, anything else becomes the nearest allowed value when the field is left or Enter pressed.
import { useState } from "react";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { NumberInput, typedNumber, type NumberBounds } from "./ui";

afterEach(cleanup);

function Field({ initial, report, ...bounds }: NumberBounds & { initial: number | undefined; report: (v: number | undefined) => void }) {
  const [value, setValue] = useState(initial);
  return (
    <NumberInput
      aria-label="Every (ms)"
      value={value}
      onChange={(v) => {
        report(v);
        setValue(v);
      }}
      {...bounds}
    />
  );
}

const field = () => screen.getByLabelText("Every (ms)") as HTMLInputElement;

describe("typedNumber", () => {
  it("counts only allowed values while typing", () => {
    const b = { min: 10, max: 5000 };
    expect(typedNumber("", b, true)).toBeNull();
    expect(typedNumber("5", b, true)).toBeNull();
    expect(typedNumber("50", b, true)).toBe(50);
    expect(typedNumber("1.5e3", b, true)).toBe(1500);
    expect(typedNumber("12.5", b, true)).toBeNull();
    expect(typedNumber("9000", b, true)).toBeNull();
    expect(typedNumber("abc", b, true)).toBeNull();
    expect(typedNumber("", { ...b, optional: true }, true)).toBeUndefined();
    expect(typedNumber("0.5", { integer: false }, true)).toBe(0.5);
  });

  it("takes the nearest allowed value when done", () => {
    const b = { min: 10, max: 5000 };
    expect(typedNumber("5", b, false)).toBe(10);
    expect(typedNumber("9000", b, false)).toBe(5000);
    expect(typedNumber("12.9", b, false)).toBe(12);
    expect(typedNumber("-", b, false)).toBeNull();
    // Whole numbers stop at what the files hold; a soft maximum is kept (and shown as invalid).
    expect(typedNumber("1e20", {}, false)).toBe(4_294_967_295);
    expect(typedNumber("9000", { ...b, softMax: true }, false)).toBe(9000);
  });
});

describe("NumberInput", () => {
  it("lets the field be cleared and retyped, and settles on leaving it", () => {
    const report = vi.fn();
    render(<Field initial={500} min={10} report={report} />);
    fireEvent.focus(field());
    fireEvent.change(field(), { target: { value: "" } });
    expect(field().value).toBe("");
    fireEvent.change(field(), { target: { value: "2" } });
    expect(report).not.toHaveBeenCalled();
    fireEvent.change(field(), { target: { value: "25" } });
    expect(report).toHaveBeenLastCalledWith(25);
    fireEvent.change(field(), { target: { value: "2" } });
    fireEvent.blur(field());
    expect(report).toHaveBeenLastCalledWith(10);
    expect(field().value).toBe("10");
  });

  it("shows the value again when left empty, or reports empty when that's allowed", () => {
    const report = vi.fn();
    render(<Field initial={500} min={10} report={report} />);
    fireEvent.change(field(), { target: { value: "" } });
    fireEvent.blur(field());
    expect(report).not.toHaveBeenCalled();
    expect(field().value).toBe("500");
    cleanup();

    render(<Field initial={500} optional report={report} />);
    fireEvent.change(field(), { target: { value: "" } });
    expect(report).toHaveBeenLastCalledWith(undefined);
  });

  it("settles on Enter and steps with the arrow keys", () => {
    const report = vi.fn();
    render(<Field initial={20} min={10} max={100} report={report} />);
    fireEvent.change(field(), { target: { value: "1000" } });
    fireEvent.keyDown(field(), { key: "Enter" });
    expect(report).toHaveBeenLastCalledWith(100);
    expect(field().value).toBe("100");
    fireEvent.keyDown(field(), { key: "ArrowDown" });
    expect(field().value).toBe("99");
    fireEvent.keyDown(field(), { key: "ArrowUp" });
    fireEvent.keyDown(field(), { key: "ArrowUp" });
    expect(field().value).toBe("100");
  });

  it("marks a value above a soft maximum as invalid", () => {
    render(<Field initial={20} max={100} softMax report={() => {}} />);
    fireEvent.change(field(), { target: { value: "150" } });
    expect(field().getAttribute("aria-invalid")).toBe("true");
  });
});
