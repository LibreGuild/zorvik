import { beforeEach, describe, expect, it, vi } from "vitest";
import type { RunningServerInfo } from "../bindings/RunningServerInfo";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(() => Promise.resolve(null)) }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));
vi.mock("../lib/events", () => ({ onEvent: () => () => {} }));

const { invoke } = await import("@tauri-apps/api/core");
const { useServers } = await import("./servers");
const { useLoadTests } = await import("./loadtests");
const { useAgents } = await import("./agents");
const { useDialogs } = await import("./dialogs");
const { dismissNotice, installsByHand, installUpdate, newVersion, releaseNotesUrl, runningWork, useUpdates } = await import("./updates");
type Info = NonNullable<ReturnType<typeof useUpdates.getState>["info"]>;

const info = (status: Info["status"], canInstall = true): Info => ({
  current: "0.1.2",
  status,
  canInstall,
  whyNot: canInstall ? null : "the portable version",
  downloadPage: "https://github.com/LibreGuild/zorvik/releases",
});

beforeEach(() => {
  vi.mocked(invoke).mockClear();
  useServers.setState({ running: [] });
  useAgents.setState({ sessions: [] });
  useDialogs.setState({ current: null });
});

describe("updates", () => {
  it("knows when a newer version is out", () => {
    expect(newVersion(info({ state: "upToDate", checkedAt: 1 }))).toBeNull();
    expect(newVersion(info({ state: "available", version: "0.1.3", notes: null, date: null, byHand: false }))).toBe("0.1.3");
    expect(newVersion(info({ state: "ready", version: "0.1.3", notes: null }))).toBe("0.1.3");
    expect(newVersion(null)).toBeNull();
  });

  it("sends people to GitHub when this copy can't install the update", () => {
    const out = (byHand: boolean) => ({ state: "available" as const, version: "0.1.3", notes: null, date: null, byHand });
    expect(installsByHand(info(out(false)))).toBe(false);
    // The download or its signature check failed: a link, not an error.
    expect(installsByHand(info(out(true)))).toBe(true);
    // The portable zip, .deb and .rpm never install themselves.
    expect(installsByHand(info(out(false), false))).toBe(true);
  });

  it("links to the release notes of the channel", () => {
    const i = info({ state: "idle" });
    expect(releaseNotesUrl(i, "0.1.3", "stable")).toBe("https://github.com/LibreGuild/zorvik/releases/tag/v0.1.3");
    expect(releaseNotesUrl(i, "0.1.3", "nightly")).toBe("https://github.com/LibreGuild/zorvik/releases/tag/nightly");
  });

  it("remembers a closed notice per version", () => {
    dismissNotice("0.1.3");
    expect(useUpdates.getState().dismissed).toBe("0.1.3");
  });

  it("restarts at once when nothing would be interrupted", async () => {
    expect(runningWork()).toEqual([]);
    await expect(installUpdate()).resolves.toBe(true);
    expect(invoke).toHaveBeenCalledWith("update_install");
  });

  it("asks first when servers run or an agent is connected", async () => {
    useServers.setState({ running: [{ runId: "r" } as RunningServerInfo, { runId: "s" } as RunningServerInfo] });
    useAgents.setState({ sessions: [{ id: "a" }] as never });
    expect(runningWork()).toEqual(["2 servers are running", "an AI agent is connected"]);
    const pending = installUpdate();
    await vi.waitFor(() => expect(useDialogs.getState().current).not.toBeNull());
    const dialog = useDialogs.getState().current!;
    expect(dialog.kind === "confirm" && dialog.message).toContain("2 servers are running, an AI agent is connected");
    if (dialog.kind === "confirm") dialog.resolve(false);
    await expect(pending).resolves.toBe(false);
    expect(invoke).not.toHaveBeenCalled();
    expect(useLoadTests.getState().active).toBeFalsy();
  });
});
