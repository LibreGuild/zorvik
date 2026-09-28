// The update notice: a ready update offers a restart; one this copy can't install (or that
// failed to download or verify) offers the GitHub release page instead of an error.
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Settings } from "../bindings/Settings";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(() => Promise.resolve(null)) }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));
vi.mock("../lib/events", () => ({ onEvent: () => () => {} }));
vi.mock("../lib/platform", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/platform")>()),
  openExternal: vi.fn(() => Promise.resolve()),
}));

const { openExternal } = await import("../lib/platform");
const { useSettings } = await import("../store/settings");
const { useUpdates } = await import("../store/updates");
const { UpdateNotice } = await import("./UpdateNotice");
type Info = NonNullable<ReturnType<typeof useUpdates.getState>["info"]>;

const show = (status: Info["status"], canInstall = true) =>
  useUpdates.setState({
    dismissed: null,
    info: {
      current: "0.1.2",
      status,
      canInstall,
      whyNot: canInstall ? null : "the portable version",
      downloadPage: "https://github.com/LibreGuild/zorvik/releases",
    },
  });

beforeEach(() => {
  vi.mocked(openExternal).mockClear();
  useSettings.setState({ settings: { updates: { mode: "automatic", channel: "stable" } } as Settings });
});
afterEach(cleanup);

describe("UpdateNotice", () => {
  it("offers a restart when the update is ready", () => {
    show({ state: "ready", version: "0.1.3", notes: null });
    render(<UpdateNotice />);
    expect(screen.getByText("Zorvik 0.1.3 is ready")).toBeTruthy();
    expect(screen.getByRole("button", { name: /Restart now/ })).toBeTruthy();
  });

  it("stays quiet while an automatic download is on its way", () => {
    show({ state: "available", version: "0.1.3", notes: null, date: null, byHand: false });
    render(<UpdateNotice />);
    expect(screen.queryByTestId("update-notice")).toBeNull();
  });

  it("links to the release when the automatic install didn't work", () => {
    show({ state: "available", version: "0.1.3", notes: null, date: null, byHand: true });
    render(<UpdateNotice />);
    expect(screen.getByText("Zorvik 0.1.3 is out")).toBeTruthy();
    expect(screen.getByText(/couldn't be installed automatically this time/)).toBeTruthy();
    expect(screen.queryByRole("button", { name: /^Download$/ })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: /Open downloads/ }));
    expect(openExternal).toHaveBeenCalledWith("https://github.com/LibreGuild/zorvik/releases/tag/v0.1.3");
  });

  it("links to the release for copies that never update themselves", () => {
    show({ state: "available", version: "0.1.3", notes: null, date: null, byHand: false }, false);
    render(<UpdateNotice />);
    expect(screen.getByText(/the portable version/)).toBeTruthy();
    expect(screen.getByRole("button", { name: /Open downloads/ })).toBeTruthy();
  });
});
