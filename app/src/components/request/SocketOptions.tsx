// TCP/UDP request options: message framing and what is appended to text messages.
import type { Framing } from "../../bindings/Framing";
import type { LineEnding } from "../../bindings/LineEnding";
import type { SocketOptions as Options } from "../../bindings/SocketOptions";
import { updateDraft } from "../../store/tabs";
import { Field, Select, Switch } from "../ui";
import type { KindPaneProps } from "./kinds";

export function SocketOptions({ tab }: KindPaneProps) {
  const tcp = tab.draft.kind === "tcp";
  const socket: Options = tab.draft.socket ?? {};
  const set = (patch: Partial<Options>) => updateDraft(tab.id, (r) => ({ ...r, socket: { ...(r.socket ?? {}), ...patch } }));
  const framing = socket.framing ?? "raw";
  return (
    <div className="flex max-w-2xl flex-col gap-4 p-4">
      <p className="text-[12.5px] text-muted">
        {tcp ? (
          <>
            Address: <code className="font-mono">tcp://host:port</code>, or <code className="font-mono">tls://host:port</code> for TLS. Messages you
            send go out as typed; incoming bytes are split into messages as set below.
          </>
        ) : (
          <>
            Address: <code className="font-mono">udp://host:port</code>. Each message is one datagram; replies from the host show up in the log.
          </>
        )}
      </p>
      <div className="grid grid-cols-2 gap-3">
        {tcp && (
          <Field label="Message framing" hint="How incoming bytes are split into messages.">
            <Select value={framing} onChange={(e) => set({ framing: e.target.value as Framing })}>
              <option value="raw">As they arrive</option>
              <option value="line">One message per line</option>
              <option value="lengthPrefixed">Length prefix (big-endian)</option>
            </Select>
          </Field>
        )}
        {tcp && framing === "lengthPrefixed" && (
          <Field label="Prefix size" hint="Sent messages get the same prefix.">
            <Select value={String(socket.lengthBytes ?? 2)} onChange={(e) => set({ lengthBytes: Number(e.target.value) })}>
              <option value="1">1 byte</option>
              <option value="2">2 bytes</option>
              <option value="4">4 bytes</option>
            </Select>
          </Field>
        )}
        <Field label="Add to each text message">
          <Select value={socket.lineEnding ?? "none"} onChange={(e) => set({ lineEnding: e.target.value as LineEnding })}>
            <option value="none">Nothing</option>
            <option value="lf">\n (LF)</option>
            <option value="crLf">\r\n (CRLF)</option>
          </Select>
        </Field>
      </div>
      {!tcp && <Switch checked={socket.broadcast ?? false} onChange={(broadcast) => set({ broadcast })} label="Allow broadcast addresses (e.g. 255.255.255.255)" />}
    </div>
  );
}
