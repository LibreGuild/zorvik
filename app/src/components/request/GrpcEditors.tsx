// Editor tabs of gRPC requests: the JSON message (with the method's example and, while a
// client stream is open, Send message / End stream), metadata (the request headers) and the
// .proto files / import folders that replace server reflection.
import { FilePlus2, FolderPlus, Info, Send, Square, Trash2, Wand2 } from "lucide-react";
import type { GrpcOptions } from "../../bindings/GrpcOptions";
import { modKey, pickFile, pickFolder } from "../../lib/platform";
import { confirm } from "../../store/dialogs";
import { endStream, findMethod, loadServices, openCall, sendMessage, sourceKey, useGrpc, usesProtoFiles, workspaceRelative } from "../../store/grpc";
import { send, updateDraft } from "../../store/tabs";
import { toast } from "../../store/toasts";
import { useVariableNames, useWorkspace } from "../../store/workspace";
import { CodeEditor } from "../CodeEditor";
import { KeyValueEditor } from "../KeyValueEditor";
import { Button, cx, IconButton, Spinner } from "../ui";
import { StreamBadge } from "./GrpcMethodPicker";
import { beautifyJson } from "./jsonFormat";
import type { KindPaneProps } from "./kinds";

function Hint({ children }: { children: React.ReactNode }) {
  return (
    <div className="flex items-start gap-2 px-3 py-3 text-[12px] text-faint">
      <Info size={13} className="mt-0.5 shrink-0" />
      <div>{children}</div>
    </div>
  );
}

export function GrpcMessageTab({ tab }: KindPaneProps) {
  const { names } = useVariableNames();
  const environment = useWorkspace((s) => s.info?.activeEnvironment);
  const entry = useGrpc((s) => s.services[sourceKey(tab.draft, environment)]);
  const call = useGrpc((s) => s.calls[tab.id]);
  const method = entry ? findMethod(entry.services, tab.draft.method) : null;
  const text = tab.draft.body?.text ?? "";
  // A client stream is open and still takes messages.
  const composing = !!call && call.streaming && call.state !== "ended" && !call.clientEnded && !!call.method?.clientStreaming;
  const setText = (t: string) => updateDraft(tab.id, (r) => ({ ...r, body: { ...(r.body ?? { type: "json" }), type: "json", text: t } }));
  const submit = () => {
    if (openCall(tab.id)) {
      if (composing) void sendMessage(tab.id, text);
      return;
    }
    void send(tab.id);
  };
  const example = async () => {
    if (!method) {
      toast("info", "Pick a method first", "The example is built from the method's input message.");
      return;
    }
    const current = text.trim();
    if (current && current !== "{}" && current !== method.example.trim()) {
      const ok = await confirm({ title: "Replace the message?", message: `The message is replaced with an example ${method.inputType}.`, confirmLabel: "Replace" });
      if (!ok) return;
    }
    setText(method.example);
  };

  return (
    <div className="flex h-full min-h-0 flex-col">
      {/* Wraps in a narrow pane (the stream buttons appear while a stream is open). */}
      <div className="flex min-h-10 shrink-0 flex-wrap items-center gap-x-2 gap-y-1 px-3 py-1">
        {method ? (
          <span className="flex min-w-0 items-center gap-2 text-[12px] text-muted">
            <span className="truncate font-mono" title={method.inputType}>
              {method.inputType}
            </span>
            <span className="shrink-0">
              <StreamBadge method={method} />
            </span>
          </span>
        ) : (
          <span className="truncate text-[12px] text-faint">{tab.draft.method.trim() ? tab.draft.method : "No method picked"}</span>
        )}
        <div className="flex-1" />
        <Button size="sm" variant="ghost" icon={<Wand2 size={13} />} onClick={() => void example()}>
          Example message
        </Button>
        <Button
          size="sm"
          variant="ghost"
          onClick={() => {
            const pretty = beautifyJson(text);
            if (pretty === null) toast("error", "The message is not valid JSON");
            else setText(pretty);
          }}
        >
          Beautify
        </Button>
        {composing && (
          <>
            <Button size="sm" variant="primary" icon={<Send size={13} />} onClick={() => void sendMessage(tab.id, text)} title={`Send this message (${modKey}+Enter)`}>
              Send message
            </Button>
            <Button size="sm" icon={<Square size={12} />} onClick={() => void endStream(tab.id)} title="Tell the server no more messages follow">
              End stream
            </Button>
          </>
        )}
      </div>
      <div className="min-h-0 flex-1 overflow-auto" data-testid="grpc-message-editor">
        <CodeEditor value={text} onChange={setText} language="json" variables={names} onSubmit={submit} placeholder='{"field": "value"}' />
      </div>
      {method?.clientStreaming && !composing && (
        <div className="shrink-0 border-t border-line/60 px-3 py-2 text-[11.5px] text-faint">
          Send opens the stream. Then each <span className="text-muted">Send message</span> ({modKey}+Enter) sends the JSON above, and{" "}
          <span className="text-muted">End stream</span> tells the server you are done.
        </div>
      )}
    </div>
  );
}

const METADATA_KEYS = ["authorization", "x-api-key", "x-request-id", "grpc-trace-bin", "user-agent"];

export function GrpcMetadataTab({ tab }: KindPaneProps) {
  return (
    <div>
      <KeyValueEditor
        rows={tab.draft.headers ?? []}
        onChange={(headers) => updateDraft(tab.id, (r) => ({ ...r, headers }))}
        keyPlaceholder="Key"
        keySuggestions={METADATA_KEYS}
        onEnter={() => send(tab.id)}
      />
      <Hint>
        Sent as gRPC metadata (request headers). Keys ending in <code className="font-mono">-bin</code> carry binary values as base64. Folder and
        workspace headers are inherited, and Auth adds <code className="font-mono">authorization</code>.
      </Hint>
    </div>
  );
}

export function GrpcProtoFilesTab({ tab }: KindPaneProps) {
  const root = useWorkspace((s) => s.info?.path);
  const environment = useWorkspace((s) => s.info?.activeEnvironment);
  const entry = useGrpc((s) => s.services[sourceKey(tab.draft, environment)]);
  const grpc: GrpcOptions = tab.draft.grpc ?? {};
  const files = grpc.protoFiles ?? [];
  const imports = grpc.importPaths ?? [];
  const update = (patch: Partial<GrpcOptions>) => updateDraft(tab.id, (r) => ({ ...r, grpc: { ...(r.grpc ?? {}), ...patch } }));
  const addFile = async () => {
    const file = await pickFile("Add .proto file", ["proto"]);
    if (!file) return;
    const rel = workspaceRelative(file, root);
    if (!files.includes(rel)) update({ protoFiles: [...files, rel] });
  };
  const addFolder = async () => {
    const folder = await pickFolder("Add import folder");
    if (!folder) return;
    const rel = workspaceRelative(folder, root);
    if (!imports.includes(rel)) update({ importPaths: [...imports, rel] });
  };
  const fromFiles = usesProtoFiles(tab.draft);

  return (
    <div className="flex max-w-3xl flex-col gap-1 pb-4">
      <PathList
        title="Proto files"
        items={files}
        empty="None: services come from server reflection."
        onRemove={(i) => update({ protoFiles: files.filter((_, j) => j !== i) })}
        action={
          <Button size="sm" variant="ghost" icon={<FilePlus2 size={13} />} onClick={() => void addFile()}>
            Add .proto file…
          </Button>
        }
        testId="grpc-proto-files"
      />
      <PathList
        title="Import folders"
        items={imports}
        empty="The folder of each .proto file is searched for imports; add the root of your proto tree if imports start there."
        onRemove={(i) => update({ importPaths: imports.filter((_, j) => j !== i) })}
        action={
          <Button size="sm" variant="ghost" icon={<FolderPlus size={13} />} onClick={() => void addFolder()}>
            Add folder…
          </Button>
        }
      />
      <div className="mx-3 mt-2 flex flex-wrap items-center gap-2 text-[12px]">
        <Button size="sm" onClick={() => void loadServices(tab, true)} disabled={!fromFiles && !tab.draft.url.trim()}>
          {fromFiles ? "Load services from the files" : "Load services from the server"}
        </Button>
        {entry?.status === "loading" && <Spinner />}
        {entry?.status === "loaded" && (
          <span className="text-muted" data-testid="grpc-services-status">
            {entry.services.length} {entry.services.length === 1 ? "service" : "services"}, {entry.services.reduce((n, s) => n + s.methods.length, 0)} methods from {entry.source}
          </span>
        )}
      </div>
      {entry?.status === "error" && (
        <div className="selectable mx-3 mt-2 whitespace-pre-wrap break-words rounded-lg border border-danger/30 bg-danger/5 px-3 py-2 font-mono text-[12px] text-fg">
          {entry.error}
        </div>
      )}
      <Hint>
        Paths inside the workspace are saved relative to it, so the request works for everyone who opens the workspace. Files outside the workspace
        need Settings → Data &amp; privacy → allow files outside the workspace. Well-known types (<code className="font-mono">google/protobuf/*.proto</code>)
        are built in.
      </Hint>
    </div>
  );
}

function PathList({
  title,
  items,
  empty,
  onRemove,
  action,
  testId,
}: {
  title: string;
  items: string[];
  empty: string;
  onRemove: (index: number) => void;
  action: React.ReactNode;
  testId?: string;
}) {
  return (
    <section data-testid={testId}>
      <div className="flex items-center justify-between px-3 pb-1 pt-3">
        <div className="text-[11px] font-semibold uppercase tracking-wide text-faint">{title}</div>
        {action}
      </div>
      {items.length === 0 ? (
        <div className="px-3 py-1 text-[12px] text-faint">{empty}</div>
      ) : (
        <div className="mx-3 overflow-hidden rounded-lg border border-line">
          {items.map((item, i) => (
            <div key={item} className={cx("group flex items-center gap-2 px-3 py-1.5", i > 0 && "border-t border-line/60")}>
              <span className="selectable min-w-0 flex-1 truncate font-mono text-[12px] text-fg" title={item}>
                {item}
              </span>
              <IconButton label={`Remove ${item}`} onClick={() => onRemove(i)} size={24}>
                <Trash2 size={12} />
              </IconButton>
            </div>
          ))}
        </div>
      )}
    </section>
  );
}
