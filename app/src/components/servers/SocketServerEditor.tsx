// TCP and UDP server settings: how to answer, message framing, greeting.
import type { Framing } from "../../bindings/Framing";
import type { LineEnding } from "../../bindings/LineEnding";
import type { PayloadEncoding } from "../../bindings/PayloadEncoding";
import type { ReplyMode } from "../../bindings/ReplyMode";
import type { SocketServerConfig } from "../../bindings/SocketServerConfig";
import { Field, Input, Segmented, Select } from "../ui";
import type { ServerEditorProps } from "./kinds";
import { ReplyRulesEditor } from "./ReplyRulesEditor";
import { EditorSection } from "./ServerView";

export const REPLY_MODES: { id: ReplyMode; label: string; hint: string }[] = [
  { id: "echo", label: "Echo", hint: "Every message is sent back." },
  { id: "rules", label: "Rules", hint: "The first matching rule answers; other messages get no reply." },
  { id: "manual", label: "Manual", hint: "Nothing is sent automatically: reply from the traffic panel." },
  { id: "discard", label: "Discard", hint: "Messages are read and dropped." },
];

export function SocketServerEditor({ server, onChange }: ServerEditorProps) {
  const tcp = server.kind === "tcp";
  const socket: SocketServerConfig = server.socket ?? {};
  const set = (patch: Partial<SocketServerConfig>) => onChange((s) => ({ ...s, socket: { ...(s.socket ?? {}), ...patch } }));
  const mode = socket.mode ?? "echo";
  const framing = socket.framing ?? "raw";
  const encoding = socket.encoding ?? "text";

  return (
    <>
      <EditorSection title="Replies">
        <div>
          <Segmented items={REPLY_MODES.map((m) => ({ id: m.id, label: m.label }))} value={mode} onChange={(mode) => set({ mode })} />
        </div>
        <p className="text-[12px] text-muted">{REPLY_MODES.find((m) => m.id === mode)?.hint}</p>
        {mode === "rules" && <ReplyRulesEditor rules={socket.rules ?? []} onChange={(rules) => set({ rules })} encoding={encoding} />}
      </EditorSection>

      <EditorSection title="Messages">
        <div className="grid grid-cols-2 gap-3">
          <Field label={tcp ? "Rules and greeting as" : "Rules and replies as"}>
            <Select value={encoding} onChange={(e) => set({ encoding: e.target.value as PayloadEncoding })}>
              <option value="text">Text</option>
              <option value="hex">Hex bytes</option>
            </Select>
          </Field>
          <Field label="Line ending on replies">
            <Select value={socket.lineEnding ?? "none"} onChange={(e) => set({ lineEnding: e.target.value as LineEnding })}>
              <option value="none">None</option>
              <option value="lf">\n (LF)</option>
              <option value="crLf">\r\n (CRLF)</option>
            </Select>
          </Field>
          {tcp && (
            <Field label="Message framing" hint="How incoming bytes are split into messages; replies use the same framing.">
              <Select value={framing} onChange={(e) => set({ framing: e.target.value as Framing })}>
                <option value="raw">As they arrive</option>
                <option value="line">One message per line</option>
                <option value="lengthPrefixed">Length prefix (big-endian)</option>
              </Select>
            </Field>
          )}
          {tcp && framing === "lengthPrefixed" && (
            <Field label="Prefix size">
              <Select value={String(socket.lengthBytes ?? 2)} onChange={(e) => set({ lengthBytes: Number(e.target.value) })}>
                <option value="1">1 byte</option>
                <option value="2">2 bytes</option>
                <option value="4">4 bytes</option>
              </Select>
            </Field>
          )}
        </div>
        {tcp && (
          <Field label="Greeting" hint="Sent to each client right after it connects (empty: none).">
            <Input
              className="font-mono"
              value={socket.greeting ?? ""}
              placeholder={encoding === "hex" ? "48 45 4c 4c 4f" : "220 Welcome to {{$uuid}}"}
              onChange={(e) => set({ greeting: e.target.value })}
            />
          </Field>
        )}
      </EditorSection>
    </>
  );
}
