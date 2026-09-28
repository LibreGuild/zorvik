// Socket.IO client: the connection settings (path, transport, auth payload), and the composer's
// event name and acknowledgement switch. Emitting is the composer's Send (store/tabs.ts wsSend).
import type { SocketIoOptions as Options } from "../../bindings/SocketIoOptions";
import type { SocketIoTransport } from "../../bindings/SocketIoTransport";
import { updateDraft } from "../../store/tabs";
import { useVariableNames } from "../../store/workspace";
import { CodeEditor } from "../CodeEditor";
import { VarInput } from "../VarInput";
import { Banner, Checkbox, Field, Input, Select } from "../ui";
import type { KindPaneProps } from "./kinds";

const TRANSPORTS: { id: SocketIoTransport; label: string }[] = [
  { id: "auto", label: "WebSocket, else long-polling" },
  { id: "websocket", label: "WebSocket only" },
  { id: "polling", label: "HTTP long-polling only" },
];

function optionsOf(tab: KindPaneProps["tab"]): Options {
  return tab.draft.socketio ?? {};
}

export function SocketIoOptions({ tab }: KindPaneProps) {
  const { names } = useVariableNames();
  const io = optionsOf(tab);
  const set = (patch: Partial<Options>) => updateDraft(tab.id, (r) => ({ ...r, socketio: { ...(r.socketio ?? {}), ...patch } }));
  const live = tab.stream.status === "open" || tab.stream.status === "connecting";
  return (
    <div className="flex max-w-2xl flex-col gap-4 p-4">
      <p className="text-[12.5px] text-muted">
        Socket.IO 3 and 4 servers. The URL is the server and the namespace to join, e.g. <code className="font-mono">http://localhost:3000/chat</code> (no
        path: the main namespace). Its query parameters (Params tab) and headers go with the handshake; auth from the Auth tab becomes a header.
      </p>
      {live && (
        <div className="-mx-3 -mt-2">
          <Banner tone="info">Connection settings apply the next time you connect.</Banner>
        </div>
      )}
      <div className="grid grid-cols-2 gap-3">
        <Field label="Path" hint="The server's path option (socket.io's default: /socket.io/).">
          <Input aria-label="Socket.IO path" className="font-mono text-[12.5px]" value={io.path ?? "/socket.io/"} onChange={(e) => set({ path: e.target.value })} />
        </Field>
        <Field label="Transport">
          <Select aria-label="Transport" value={io.transport ?? "auto"} onChange={(e) => set({ transport: e.target.value as SocketIoTransport })}>
            {TRANSPORTS.map((t) => (
              <option key={t.id} value={t.id}>
                {t.label}
              </option>
            ))}
          </Select>
        </Field>
      </div>
      <Field label="Auth payload (JSON)" hint="Sent when joining the namespace, like socket.io-client's auth option. Can use {{variables}}.">
        <div className="h-28 overflow-hidden rounded-lg border border-line bg-input focus-within:border-accent" data-testid="socketio-auth">
          <CodeEditor value={io.auth ?? ""} onChange={(auth) => set({ auth })} language="json" variables={names} lineNumbers={false} placeholder={'{"token": "{{token}}"}'} />
        </div>
      </Field>
    </div>
  );
}

/** Composer controls: the event to emit and whether to ask for an acknowledgement. */
export function SocketIoEventField({ tab }: KindPaneProps) {
  const io = optionsOf(tab);
  const set = (patch: Partial<Options>) => updateDraft(tab.id, (r) => ({ ...r, socketio: { ...(r.socketio ?? {}), ...patch } }));
  return (
    <div className="flex min-w-[220px] flex-1 basis-[280px] items-center gap-2">
      <div className="flex h-7 min-w-0 flex-1 items-center rounded-md border border-line bg-panel-2 focus-within:border-accent">
        <span className="shrink-0 pl-2 text-[11px] font-semibold text-faint">Event</span>
        <VarInput value={io.event ?? ""} onChange={(event) => set({ event })} placeholder="message" className="flex-1" ariaLabel="Event" />
      </div>
      <label className="flex shrink-0 cursor-pointer items-center gap-1.5 text-[12px] text-muted" title="The server answers with an acknowledgement">
        <Checkbox checked={io.ack ?? false} onChange={(ack) => set({ ack: ack || undefined })} label="Ask for acknowledgement" />
        Ack
      </label>
    </div>
  );
}
