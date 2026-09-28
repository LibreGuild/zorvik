// A quiet card when a new version is ready (or out, in notify-only mode): what's new,
// restart now, or later (it installs on quit). Never covers the work; one card per version.
import { useState } from "react";
import { ArrowUpCircle, X } from "lucide-react";
import { openExternal } from "../lib/platform";
import { errorMessage } from "../lib/rpc";
import { useSettings } from "../store/settings";
import { toast } from "../store/toasts";
import { dismissNotice, downloadUpdate, installsByHand, installUpdate, releaseNotesUrl, useUpdates } from "../store/updates";
import { Button } from "./ui";

export function UpdateNotice() {
  const info = useUpdates((s) => s.info);
  const dismissed = useUpdates((s) => s.dismissed);
  const channel = useSettings((s) => s.settings?.updates.channel ?? "stable");
  const mode = useSettings((s) => s.settings?.updates.mode ?? "automatic");
  const [busy, setBusy] = useState(false);
  const status = info?.status;
  if (!info || !status || mode === "off") return null;
  const ready = status.state === "ready";
  // Automatic mode shows the card once the download is done; notify-only (and a version to
  // download by hand) as soon as it is out.
  const byHand = installsByHand(info);
  const out = status.state === "available" && (mode === "notify" || byHand);
  if (!ready && !out) return null;
  const version = status.version;
  if (dismissed === version) return null;

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

  return (
    <div role="status" aria-live="polite" className="zv-pop fixed bottom-4 right-4 z-[85] w-[340px] rounded-2xl border border-accent/40 bg-elev p-4 shadow-pop" data-testid="update-notice">
      <div className="flex items-start gap-3">
        <ArrowUpCircle size={20} className="mt-0.5 shrink-0 text-accent" />
        <div className="min-w-0 flex-1">
          <div className="text-[13.5px] font-semibold text-fg">{ready ? `Zorvik ${version} is ready` : `Zorvik ${version} is out`}</div>
          <div className="mt-0.5 text-[12.5px] leading-snug text-muted">
            {ready
              ? "It installs when you quit Zorvik, or restart now."
              : !info.canInstall
                ? `This copy (${info.whyNot}) doesn't update itself: download the new version from GitHub.`
                : byHand
                  ? "It couldn't be installed automatically this time: download the new version from GitHub."
                  : "Download it now; it installs when you restart or quit."}
          </div>
          <div className="mt-3 flex flex-wrap items-center gap-2">
            {ready && (
              <Button size="sm" variant="primary" loading={busy} onClick={() => void run(installUpdate)}>
                Restart now
              </Button>
            )}
            {out && !byHand && (
              <Button size="sm" variant="primary" loading={busy} onClick={() => void run(downloadUpdate)}>
                Download
              </Button>
            )}
            {out && byHand && (
              <Button size="sm" variant="primary" onClick={() => void openExternal(releaseNotesUrl(info, version, channel))}>
                Open downloads
              </Button>
            )}
            <Button size="sm" variant="ghost" onClick={() => void openExternal(releaseNotesUrl(info, version, channel))}>
              What's new
            </Button>
          </div>
        </div>
        <button aria-label="Later" title="Later" onClick={() => dismissNotice(version)} className="rounded-md p-1 text-faint hover:bg-hover hover:text-fg">
          <X size={14} />
        </button>
      </div>
    </div>
  );
}
