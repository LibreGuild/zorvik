// Settings → Updates: the version, where the update stands, and the buttons that move it on.
import { useState } from "react";
import { errorMessage, isTauri } from "../../lib/rpc";
import { openExternal } from "../../lib/platform";
import { toast } from "../../store/toasts";
import { useSettings } from "../../store/settings";
import { checkForUpdates, downloadUpdate, installsByHand, installUpdate, releaseNotesUrl, useUpdates, type UpdateInfo } from "../../store/updates";
import { Button, Spinner } from "../ui";

function describe(info: UpdateInfo): string {
  const s = info.status;
  switch (s.state) {
    case "idle":
      return "";
    case "checking":
      return "Checking GitHub for a new version…";
    case "upToDate":
      return `Up to date (checked ${new Date(s.checkedAt).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}).`;
    case "available":
      if (!info.canInstall) return `Version ${s.version} is out. This copy (${info.whyNot}) doesn't update itself.`;
      return s.byHand ? `Version ${s.version} is out. It couldn't be installed automatically: download it from GitHub.` : `Version ${s.version} is out.`;
    case "downloading":
      return `Downloading ${s.version}${s.percent != null ? `: ${s.percent}%` : "…"}`;
    case "ready":
      return `Version ${s.version} is downloaded. It installs when you quit Zorvik, or restart now.`;
    case "installing":
      return "Installing…";
    case "failed":
      return s.message;
  }
}

export function UpdateStatusRow({ version, build }: { version: string; build: string | null }) {
  const info = useUpdates((s) => s.info);
  const channel = useSettings((s) => s.settings?.updates.channel ?? "stable");
  const [busy, setBusy] = useState(false);
  const run = async (f: () => Promise<unknown>) => {
    setBusy(true);
    try {
      await f();
    } catch (e) {
      toast("error", "Could not update", errorMessage(e));
    } finally {
      setBusy(false);
    }
  };
  const state = info?.status.state;
  const out = info?.status.state === "available" ? info.status : null;
  const working = state === "checking" || state === "downloading" || state === "installing";
  return (
    <div className="flex flex-col gap-2" data-testid="update-status">
      <div className="text-[13px] text-fg">
        Zorvik {version}
        {build && <span className="text-faint"> · {build}</span>}
      </div>
      {info && (
        <div className={state === "failed" ? "text-[12.5px] text-danger" : "text-[12.5px] text-muted"}>
          {working && <Spinner size={12} />} {describe(info)}
        </div>
      )}
      {isTauri && info && (
        <div className="flex flex-wrap gap-2">
          {state === "ready" ? (
            <Button size="sm" variant="primary" loading={busy} onClick={() => void run(installUpdate)}>
              Restart to update
            </Button>
          ) : out && !installsByHand(info) ? (
            <Button size="sm" variant="primary" loading={busy} onClick={() => void run(downloadUpdate)}>
              Download and install
            </Button>
          ) : out ? (
            <Button size="sm" variant="primary" onClick={() => void openExternal(releaseNotesUrl(info, out.version, channel))}>
              Open downloads
            </Button>
          ) : null}
          <Button size="sm" disabled={working || busy} onClick={() => void run(checkForUpdates)}>
            Check for updates
          </Button>
        </div>
      )}
    </div>
  );
}
