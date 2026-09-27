// This computer's network interfaces: the addresses other devices can use to
// reach servers running here.
import { useEffect } from "react";
import { Info, Network, RefreshCw } from "lucide-react";
import type { NetAddress } from "../../bindings/NetAddress";
import type { NetInterface } from "../../bindings/NetInterface";
import { type ToolTab } from "../../store/tabs";
import { bestAddress, INTERFACES_DEFAULTS, loadInterfaces, toolState } from "../../store/tools";
import { Badge, Button, cx, EmptyState, Spinner } from "../ui";
import { CopyButton, ErrorNote, Hint } from "./parts";

export function InterfacesTool({ tab }: { tab: ToolTab }) {
  const s = toolState(tab, INTERFACES_DEFAULTS);
  const tabId = tab.id;
  const idle = s.status === "idle";
  useEffect(() => {
    if (idle) void loadInterfaces(tabId);
  }, [idle, tabId]);
  const best = bestAddress(s.list);

  return (
    <div className="pb-6" data-testid="interfaces-tool">
      <Hint icon={<Info size={13} />}>
        Use one of these instead of <span className="font-mono">127.0.0.1</span> to reach your servers from other devices (start the server on{" "}
        <span className="font-mono">0.0.0.0</span>).
      </Hint>
      <div className="flex items-center gap-2 px-4 pb-2">
        {best && (
          <span className="text-[12.5px] text-muted">
            Best for other devices: <span className="selectable font-mono text-fg">{best}</span>
          </span>
        )}
        {best && <CopyButton text={best} label="Copy address" />}
        <span className="flex-1" />
        <Button size="sm" variant="ghost" icon={s.status === "loading" ? <Spinner size={13} /> : <RefreshCw size={13} />} onClick={() => void loadInterfaces(tabId)} disabled={s.status === "loading"}>
          Refresh
        </Button>
      </div>
      {s.status === "error" && s.error && <ErrorNote>{s.error}</ErrorNote>}
      {s.list.length === 0 && s.status === "done" && (
        <EmptyState icon={<Network size={28} />} title="No network interfaces">
          This computer reports no network addresses.
        </EmptyState>
      )}
      {s.list.length > 0 && (
        <table className="mx-4 w-[calc(100%-2rem)] text-[12.5px]" data-testid="interfaces-table">
          <thead>
            <tr className="border-b border-line text-left text-[11px] text-faint">
              <th className="w-[30%] pb-1.5 font-medium">Interface</th>
              <th className="pb-1.5 font-medium">Address</th>
              <th className="w-16 pb-1.5 font-medium">Type</th>
              <th className="w-10 pb-1.5" />
            </tr>
          </thead>
          <tbody>
            {s.list.map((i) =>
              i.addresses.map((a, n) => <AddressRow key={`${i.name}-${a.ip}`} iface={i} address={a} first={n === 0} rows={i.addresses.length} />),
            )}
          </tbody>
        </table>
      )}
    </div>
  );
}

function AddressRow({ iface, address, first, rows }: { iface: NetInterface; address: NetAddress; first: boolean; rows: number }) {
  const dim = iface.loopback || !iface.up || address.linkLocal;
  const withPrefix = address.prefix != null ? `${address.ip}/${address.prefix}` : address.ip;
  return (
    <tr className={cx("group", first && "border-t border-line/60")}>
      {first && (
        <td rowSpan={rows} className="py-1.5 pr-3 align-top">
          <div className="flex flex-wrap items-center gap-1.5">
            <span className="selectable font-medium text-fg">{iface.name}</span>
            {iface.loopback && <Badge className="bg-hover text-muted">loopback</Badge>}
            {!iface.up && <Badge className="bg-hover text-faint">down</Badge>}
          </div>
        </td>
      )}
      <td className="py-1 pr-3">
        <span className={cx("selectable font-mono text-[12px]", dim ? "text-muted" : "text-fg")} title={withPrefix}>
          {address.ip}
        </span>
        {address.prefix != null && <span className="font-mono text-[12px] text-faint">/{address.prefix}</span>}
        {address.linkLocal && <span className="ml-2 text-[11px] text-faint">link-local</span>}
      </td>
      <td className="py-1 text-muted">{address.family === "ipv4" ? "IPv4" : "IPv6"}</td>
      <td className="py-1 text-right">
        <CopyButton text={address.ip} label="Copy address" className="opacity-0 group-hover:opacity-100 focus-visible:opacity-100" />
      </td>
    </tr>
  );
}
