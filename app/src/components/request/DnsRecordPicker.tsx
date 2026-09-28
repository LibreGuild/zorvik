// DNS record type picker: in the URL bar of a DNS request (in place of the HTTP
// method) and in the DNS lookup tool.
import { useState } from "react";
import { ChevronDown } from "lucide-react";
import { DropdownMenu } from "radix-ui";
import { updateDraft } from "../../store/tabs";
import { cx } from "../ui";
import { DNS_RECORD_TYPES, DNS_TYPE_HINTS } from "./dnsModel";
import type { KindPaneProps } from "./kinds";

const COLOR = "var(--m-dns)";

export function RecordTypePicker({
  value,
  onChange,
  boxed,
}: {
  value: string;
  onChange: (type: string) => void;
  /** Standalone field with its own border (the URL bar draws its own). */
  boxed?: boolean;
}) {
  const [custom, setCustom] = useState("");
  // Controlled so Enter in the custom-type field can close the menu (it isn't a menu item).
  const [open, setOpen] = useState(false);
  const shown = value.trim().toUpperCase() || "A";
  return (
    <DropdownMenu.Root modal={false} open={open} onOpenChange={setOpen}>
      <DropdownMenu.Trigger asChild>
        <button
          aria-label="Record type"
          title={DNS_TYPE_HINTS[shown]}
          className={cx(
            "flex items-center justify-between gap-1 pl-3 pr-2 font-mono text-[12.5px] font-bold outline-none hover:bg-hover",
            boxed ? "h-8 min-w-[96px] rounded-lg border border-line bg-input hover:border-line-strong" : "h-full min-w-[92px] rounded-l-xl border-r border-line",
          )}
          style={{ color: COLOR }}
        >
          <span className="max-w-[90px] truncate">{shown}</span>
          <ChevronDown size={13} className="text-faint" />
        </button>
      </DropdownMenu.Trigger>
      <DropdownMenu.Portal>
        <DropdownMenu.Content align="start" sideOffset={6} collisionPadding={8} className="zv-pop z-[90] w-72 rounded-xl border border-line bg-elev p-1.5 shadow-pop max-h-[var(--radix-dropdown-menu-content-available-height)] overflow-y-auto overscroll-contain">
          {DNS_RECORD_TYPES.map((t) => (
            <DropdownMenu.Item
              key={t}
              onSelect={() => onChange(t)}
              className={cx("flex h-8 items-center gap-3 rounded-lg px-2 outline-none data-[highlighted]:bg-hover", t === shown && "bg-hover/60")}
            >
              <span className="w-12 shrink-0 font-mono text-[12.5px] font-bold" style={{ color: COLOR }}>
                {t}
              </span>
              <span className="truncate text-[11.5px] text-muted">{DNS_TYPE_HINTS[t]}</span>
            </DropdownMenu.Item>
          ))}
          <DropdownMenu.Separator className="my-1 h-px bg-line" />
          <div className="p-1">
            <input
              value={custom}
              onChange={(e) => setCustom(e.target.value.toUpperCase().replace(/[^A-Z0-9]/g, "").slice(0, 12))}
              onKeyDown={(e) => {
                e.stopPropagation();
                if (e.key === "Enter" && custom) {
                  onChange(custom);
                  setCustom("");
                  setOpen(false);
                }
              }}
              aria-label="Other record type"
              placeholder="Other type… (HTTPS, DS, TYPE65)"
              className="h-7 w-full rounded-md border border-line bg-input px-2 font-mono text-[12px] outline-none focus:border-accent"
            />
          </div>
        </DropdownMenu.Content>
      </DropdownMenu.Portal>
    </DropdownMenu.Root>
  );
}

/** URL bar prefix of DNS requests: the record type is stored as the request method. */
export function DnsRecordPrefix({ tab }: KindPaneProps) {
  return <RecordTypePicker value={tab.draft.method} onChange={(method) => updateDraft(tab.id, (r) => ({ ...r, method }))} />;
}
