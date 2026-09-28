import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { createElement } from "react";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import type { Settings } from "../../bindings/Settings";
import { useSettings } from "../../store/settings";
import { useUi } from "../../store/ui";
import { TooltipProvider } from "../ui";
import { rebase, SettingsModal } from "./SettingsModal";

afterEach(cleanup);

describe("rebase", () => {
  it("keeps the user's edits and takes other changes", () => {
    const base = { theme: "system", agents: { enabled: false, follow: true }, historyLimit: 500 };
    const edited = { ...base, historyLimit: 900 };
    const latest = { ...base, agents: { enabled: true, follow: true } };
    expect(rebase(base, edited, latest)).toEqual({ theme: "system", agents: { enabled: true, follow: true }, historyLimit: 900 });
  });

  it("a field changed on both sides keeps the user's value", () => {
    const base = { agents: { enabled: false, follow: true } };
    const edited = { agents: { enabled: false, follow: false } };
    const latest = { agents: { enabled: true, follow: true } };
    expect(rebase(base, edited, latest)).toEqual({ agents: { enabled: true, follow: false } });
  });
});

const SETTINGS: Settings = {
  theme: "system",
  appearance: { zoom: 1, uiFont: "", codeFont: "", codeFontSize: 13, ligatures: false },
  request: {
    timeoutMs: 30000,
    connectTimeoutMs: 10000,
    followRedirects: true,
    maxRedirects: 10,
    verifyTls: true,
    httpVersion: "auto",
    decompress: true,
    maxResponseMb: 50,
    sendDefaultHeaders: true,
  },
  proxy: { mode: "none" },
  tls: { caCertPath: "", clientCertPath: "", clientKeyPath: "" },
  historyLimit: 500,
  cookieJar: true,
  filesOutsideWorkspace: false,
  scriptTimeoutMs: 5000,
  agents: { enabled: false, changes: "ask", traffic: "askOutside", follow: true, headless: false },
  updates: { mode: "off", channel: "stable" },
};

describe("the Settings dialog", () => {
  // Font checks measure text on a canvas, which jsdom doesn't have.
  beforeAll(() => void vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(null));
  const open = () => {
    useSettings.setState({ settings: SETTINGS });
    useUi.setState({ modal: { type: "settings" } });
    render(createElement(TooltipProvider, null, createElement(SettingsModal)));
    return screen.getByRole("dialog", { name: "Settings" });
  };

  it("closes on Escape when nothing changed", () => {
    const dialog = open();
    fireEvent.keyDown(dialog, { key: "Escape" });
    expect(useUi.getState().modal.type).toBe("none");
  });

  it("asks before Escape discards changes", () => {
    const dialog = open();
    fireEvent.change(screen.getByDisplayValue("Match system"), { target: { value: "dark" } });
    fireEvent.keyDown(dialog, { key: "Escape" });
    expect(useUi.getState().modal.type).toBe("settings");
    expect(screen.getByText("Discard unsaved changes?")).toBeTruthy();
  });
});
