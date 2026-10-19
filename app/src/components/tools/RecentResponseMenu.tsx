// "From history": put the body of a response sent from the app (one of the last ones, which
// are kept in memory) into a tool's input.
import { useState } from "react";
import { DropdownMenu } from "radix-ui";
import { History } from "lucide-react";
import type { RecentResponseInfo } from "../../bindings/RecentResponseInfo";
import { formatRelative, statusTone, toneText } from "../../lib/format";
import { api, errorMessage } from "../../lib/rpc";
import { toast } from "../../store/toasts";
import { Button, cx, Spinner } from "../ui";

/** Shorten a URL for a menu row: drop the scheme. */
export function shortUrl(url: string): string {
  return url.replace(/^[a-z][a-z0-9+.-]*:\/\//i, "");
}

export function RecentResponseMenu({ onPick, label = "From history" }: { onPick: (text: string) => void; label?: string }) {
  const [list, setList] = useState<RecentResponseInfo[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = () => {
    setList(null);
    setError(null);
    api.recentResponses().then(setList, (e) => setError(errorMessage(e)));
  };

  const pick = (r: RecentResponseInfo) => {
    api.recentResponse(r.historyId).then(
      (body) => {
        onPick(body.text);
        if (body.cut) toast("info", "Only the start of this response is kept", "Zorvik keeps the first 64,000 characters of recent responses.");
      },
      (e) => toast("error", "Could not use that response", errorMessage(e)),
    );
  };

  return (
    <DropdownMenu.Root modal={false} onOpenChange={(open) => open && load()}>
      <DropdownMenu.Trigger asChild>
        <Button size="sm" variant="ghost" icon={<History size={13} />}>
          {label}
        </Button>
      </DropdownMenu.Trigger>
      <DropdownMenu.Portal>
        <DropdownMenu.Content
          align="end"
          sideOffset={4}
          collisionPadding={8}
          aria-label="Recent responses"
          className="zv-pop z-[90] max-h-[min(420px,var(--radix-dropdown-menu-content-available-height))] w-[400px] max-w-[calc(100vw-16px)] overflow-y-auto overscroll-contain rounded-xl border border-line bg-elev p-1.5 text-[12.5px] shadow-pop"
        >
          {error ? (
            <div className="px-2 py-1.5 text-danger">{error}</div>
          ) : list === null ? (
            <div className="flex items-center gap-2 px-2 py-1.5 text-muted">
              <Spinner size={12} />
              Loading…
            </div>
          ) : list.length === 0 ? (
            <div className="px-2 py-1.5 text-muted">No responses yet. Send a request, then pick its response here.</div>
          ) : (
            <>
              <div className="px-2 pb-1 pt-0.5 text-[11px] text-faint">Responses sent from Zorvik, newest first</div>
              {list.map((r) => (
                <DropdownMenu.Item
                  key={r.historyId}
                  onSelect={() => pick(r)}
                  className="flex h-8 cursor-default items-center gap-2 rounded-lg px-2 outline-none data-[highlighted]:bg-hover"
                >
                  <span className={cx("w-8 shrink-0 font-mono text-[11.5px] font-semibold", toneText[statusTone(r.status)])}>{r.status}</span>
                  <span className="shrink-0 font-mono text-[11px] font-semibold text-muted">{r.method}</span>
                  <span className="min-w-0 flex-1 truncate font-mono text-[11.5px] text-fg" title={r.url}>
                    {shortUrl(r.url)}
                  </span>
                  <span className="shrink-0 text-[11px] text-faint">{formatRelative(r.createdAt)}</span>
                </DropdownMenu.Item>
              ))}
            </>
          )}
        </DropdownMenu.Content>
      </DropdownMenu.Portal>
    </DropdownMenu.Root>
  );
}
