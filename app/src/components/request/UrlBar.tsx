import { useRef, useState } from "react";
import { Activity, ChevronDown, Loader2, MoreHorizontal, Plug, PlugZap, Save, Send, Square, Terminal } from "lucide-react";
import type { Request } from "../../bindings/Request";
import { METHODS, methodColor, methodLabel, STREAM_KINDS } from "../../lib/http";
import { modKey } from "../../lib/platform";
import { syncPathParams } from "../../lib/url";
import { loadTestRequest } from "../../store/loadtests";
import { cancel, disconnect, isDirty, saveTab, send, type Tab, updateDraft } from "../../store/tabs";
import { toast } from "../../store/toasts";
import { openModal } from "../../store/ui";
import { VarInput } from "../VarInput";
import { URL_PLACEHOLDERS, URL_PREFIXES } from "./kinds";
import { Button, cx, IconButton, Menu } from "../ui";
import { DropdownMenu } from "radix-ui";

export function UrlBar({ tab }: { tab: Tab }) {
  const req = tab.draft;
  const kind = req.kind ?? "http";
  const loading = tab.response.status === "loading";
  const streamActive = tab.stream.status === "open" || tab.stream.status === "connecting";
  const oneShot = !STREAM_KINDS.includes(kind);
  const urlRef = useRef<HTMLInputElement>(null);
  const Prefix = URL_PREFIXES[kind];

  const setUrl = (url: string) =>
    updateDraft(tab.id, (r: Request) => ({ ...r, url, pathParams: kind === "http" ? syncPathParams(url, r.pathParams) : r.pathParams }));

  const placeholder = URL_PLACEHOLDERS[kind] ?? "https://api.example.com/users/{{id}}";

  return (
    <div className="flex items-center gap-2 px-3 pb-1 pt-2">
      <div className="flex h-9 min-w-0 flex-1 items-center rounded-xl border border-line bg-input focus-within:border-accent focus-within:ring-2 focus-within:ring-accent-soft">
        {kind === "http" ? (
          <MethodPicker value={req.method} onChange={(method) => updateDraft(tab.id, (r) => ({ ...r, method }))} />
        ) : Prefix ? (
          <Prefix tab={tab} />
        ) : (
          <span className="flex h-full items-center rounded-l-xl border-r border-line px-3 text-[12px] font-bold" style={{ color: methodColor(null, kind) }}>
            {methodLabel(null, kind)}
          </span>
        )}
        <VarInput
          ref={urlRef}
          value={req.url}
          onChange={setUrl}
          placeholder={placeholder}
          onEnter={() => (streamActive ? undefined : send(tab.id))}
          className="flex-1"
          ariaLabel="URL"
          // A new, empty request: start typing the address right away.
          autoFocus={!req.url && !tab.path}
        />
      </div>
      {oneShot ? (
        loading ? (
          <Button variant="secondary" onClick={() => cancel(tab.id)} icon={<Square size={13} />} className="h-9 w-[104px] rounded-xl">
            Cancel
          </Button>
        ) : (
          <Button variant="primary" onClick={() => send(tab.id)} icon={<Send size={14} />} className="h-9 w-[104px] rounded-xl" title={`Send (${modKey}+Enter)`}>
            {kind === "dns" ? "Query" : "Send"}
          </Button>
        )
      ) : streamActive ? (
        <Button
          variant="secondary"
          onClick={() => disconnect(tab.id)}
          icon={tab.stream.status === "connecting" ? <Loader2 size={14} className="zv-spin" /> : <Plug size={14} />}
          className="h-9 w-[124px] rounded-xl"
        >
          Disconnect
        </Button>
      ) : (
        <Button variant="primary" onClick={() => send(tab.id)} icon={<PlugZap size={14} />} className="h-9 w-[124px] rounded-xl">
          Connect
        </Button>
      )}
      <IconButton label={`Save (${modKey}+S)`} onClick={() => saveTab(tab.id)} size={36} className={cx("rounded-xl", isDirty(tab) && "text-accent")}>
        <Save size={16} />
      </IconButton>
      <Menu
        align="end"
        trigger={
          <button aria-label="More actions" className="flex h-9 w-9 items-center justify-center rounded-xl text-muted hover:bg-hover hover:text-fg">
            <MoreHorizontal size={16} />
          </button>
        }
        entries={[
          ...(kind === "http" ? [{ label: "Copy as cURL or code…", icon: <Terminal size={14} />, onSelect: () => openModal({ type: "export", tabId: tab.id }) }] : []),
          ...(kind === "http" ? [{ label: "Load test this request…", icon: <Activity size={14} />, onSelect: () => void loadTestThis(tab) }] : []),
          { label: "Save as…", icon: <Save size={14} />, onSelect: () => openModal({ type: "saveAs", tabId: tab.id }) },
        ]}
      />
    </div>
  );
}

/** A load test sends the saved request, so unsaved requests are saved first. */
async function loadTestThis(tab: Tab) {
  if (!tab.path) {
    toast("info", "Save the request first", "A load test sends saved requests from the collection.");
    return;
  }
  if (isDirty(tab)) toast("info", "Unsaved changes are not load tested", "The load test sends the saved version of this request. Save to include your changes.");
  await loadTestRequest(tab.path, tab.saved?.name ?? tab.draft.name);
}

function MethodPicker({ value, onChange }: { value: string; onChange: (m: string) => void }) {
  const [custom, setCustom] = useState("");
  // Controlled so Enter in the custom-method field can close the menu (it isn't a menu item).
  const [open, setOpen] = useState(false);
  return (
    <DropdownMenu.Root modal={false} open={open} onOpenChange={setOpen}>
      <DropdownMenu.Trigger asChild>
        <button
          aria-label="HTTP method"
          className="flex h-full min-w-[92px] items-center justify-between gap-1 rounded-l-xl border-r border-line pl-3 pr-2 text-[12.5px] font-bold outline-none hover:bg-hover"
          style={{ color: methodColor(value) }}
        >
          <span className="max-w-[90px] truncate">{value || "GET"}</span>
          <ChevronDown size={13} className="text-faint" />
        </button>
      </DropdownMenu.Trigger>
      <DropdownMenu.Portal>
        <DropdownMenu.Content align="start" sideOffset={6} collisionPadding={8} className="zv-pop z-[90] w-44 rounded-xl border border-line bg-elev p-1.5 shadow-pop max-h-[var(--radix-dropdown-menu-content-available-height)] overflow-y-auto overscroll-contain">
          {METHODS.map((m) => (
            <DropdownMenu.Item
              key={m}
              onSelect={() => onChange(m)}
              className="flex h-8 items-center rounded-lg px-2 text-[12.5px] font-bold outline-none data-[highlighted]:bg-hover"
              style={{ color: methodColor(m) }}
            >
              {m}
            </DropdownMenu.Item>
          ))}
          <DropdownMenu.Separator className="my-1 h-px bg-line" />
          <div className="p-1">
            <input
              value={custom}
              onChange={(e) => setCustom(e.target.value.toUpperCase().replace(/[^A-Z0-9_-]/g, ""))}
              onKeyDown={(e) => {
                e.stopPropagation();
                if (e.key === "Enter" && custom) {
                  onChange(custom);
                  setCustom("");
                  setOpen(false);
                }
              }}
              placeholder="Custom method…"
              className="h-7 w-full rounded-md border border-line bg-input px-2 font-mono text-[12px] outline-none focus:border-accent"
            />
          </div>
        </DropdownMenu.Content>
      </DropdownMenu.Portal>
    </DropdownMenu.Root>
  );
}
