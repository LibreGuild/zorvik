// "Set by scripts": values of variables marked secret stay hidden until shown.
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { LocalValue } from "../../bindings/LocalValue";

vi.mock("../../lib/events", () => ({ onEvent: () => () => {} }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("../../lib/rpc", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../lib/rpc")>();
  const values: LocalValue[] = [
    { scope: "environment", environmentId: "dev", key: "token", value: "tok-s3cret" },
    { scope: "environment", environmentId: "dev", key: "userId", value: "42" },
    { scope: "environment", environmentId: "prod", key: "other", value: "x" },
  ];
  return { ...actual, api: { localValues: vi.fn(() => Promise.resolve(values)), clearLocalValues: vi.fn(() => Promise.resolve(null)) } };
});

const { TooltipProvider } = await import("../ui");
const { LocalValues } = await import("./LocalValues");

afterEach(cleanup);

describe("values set by scripts", () => {
  it("hides secret values until shown", async () => {
    render(
      <TooltipProvider>
        <LocalValues scope="environment" environmentId="dev" secretKeys={["token"]} />
      </TooltipProvider>,
    );
    expect(await screen.findByText("42")).toBeTruthy();
    expect(screen.queryByText("x")).toBeNull();
    expect(screen.queryByText("tok-s3cret")).toBeNull();
    expect(screen.getByText("••••••••")).toBeTruthy();
    fireEvent.click(screen.getByLabelText("Show token"));
    expect(screen.getByText("tok-s3cret")).toBeTruthy();
  });
});
