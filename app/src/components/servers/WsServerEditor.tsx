// WebSocket server settings: how to answer messages, and a greeting.
import type { WsServerConfig } from "../../bindings/WsServerConfig";
import { Field, Segmented } from "../ui";
import type { ServerEditorProps } from "./kinds";
import { ReplyRulesEditor } from "./ReplyRulesEditor";
import { EditorSection } from "./ServerView";
import { REPLY_MODES } from "./SocketServerEditor";

export function WsServerEditor({ server, onChange, running }: ServerEditorProps) {
  const ws: WsServerConfig = server.websocket ?? {};
  const set = (patch: Partial<WsServerConfig>) => onChange((s) => ({ ...s, websocket: { ...(s.websocket ?? {}), ...patch } }));
  const mode = ws.mode ?? "echo";

  return (
    <>
      <EditorSection title="Replies">
        <div>
          <Segmented items={REPLY_MODES.map((m) => ({ id: m.id, label: m.label }))} value={mode} onChange={(mode) => set({ mode })} />
        </div>
        <p className="text-[12px] text-muted">
          {REPLY_MODES.find((m) => m.id === mode)?.hint}
          {mode === "echo" && " Binary messages come back as binary."}
          {mode === "rules" && " Text and binary messages are matched as text; replies are text."}
        </p>
        {mode === "rules" && <ReplyRulesEditor rules={ws.rules ?? []} onChange={(rules) => set({ rules })} />}
      </EditorSection>

      <EditorSection title="Greeting">
        <Field label="Sent to each client when it connects" hint="Empty: none. Can use {{$uuid}}, {{$timestamp}} and environment variables.">
          <textarea
            aria-label="Greeting"
            value={ws.greeting ?? ""}
            spellCheck={false}
            rows={Math.min(8, Math.max(2, (ws.greeting ?? "").split("\n").length))}
            placeholder={'{"type": "welcome", "session": "{{$uuid}}"}'}
            onChange={(e) => set({ greeting: e.target.value })}
            className="w-full resize-y rounded-lg border border-line bg-input px-2.5 py-1.5 font-mono text-[12.5px] text-fg outline-none placeholder:text-faint hover:border-line-strong focus:border-accent focus:ring-2 focus:ring-accent-soft"
          />
        </Field>
      </EditorSection>

      <EditorSection title="Clients">
        <p className="text-[12px] text-muted">
          Clients can connect on any path{running ? (
            <>
              , e.g. <span className="selectable font-mono text-fg">{running.url}/</span>
            </>
          ) : null}
          . Messages up to 16 MB; pings are answered. Send to one client or all of them from the traffic panel.
        </p>
      </EditorSection>
    </>
  );
}
