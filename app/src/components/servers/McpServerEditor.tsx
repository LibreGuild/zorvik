// MCP server settings: what it offers (tools with their input schemas and results, resources,
// prompts), how it introduces itself, and how AI apps and MCP clients reach it.
import { Plus, Trash2 } from "lucide-react";
import type { McpPromptMock } from "../../bindings/McpPromptMock";
import type { McpResourceMock } from "../../bindings/McpResourceMock";
import type { McpServerConfig } from "../../bindings/McpServerConfig";
import type { McpToolMock } from "../../bindings/McpToolMock";
import { copyText } from "../../lib/platform";
import { useWorkspace } from "../../store/workspace";
import { Button, Checkbox, cx, Field, Input, Select, Switch } from "../ui";
import type { ServerEditorProps } from "./kinds";
import { EditorSection } from "./ServerView";

const mono = "font-mono text-[12px]";
const area =
  "w-full resize-y rounded-lg border border-line bg-input px-2.5 py-1.5 font-mono text-[12px] text-fg outline-none placeholder:text-faint hover:border-line-strong focus:border-accent focus:ring-2 focus:ring-accent-soft";

/** A word for a shell command line: quoted when needed, so a name can't run anything when pasted. */
export function shellWord(word: string): string {
  return /^[\w@%+=:,./-]+$/.test(word) ? word : `'${word.replace(/'/g, "'\\''")}'`;
}

function rows(text: string | undefined, min = 2, max = 10) {
  return Math.min(max, Math.max(min, (text ?? "").split("\n").length));
}

function RemoveButton({ label, onClick }: { label: string; onClick: () => void }) {
  return (
    <button aria-label={label} onClick={onClick} className="rounded p-1 text-faint hover:bg-hover hover:text-danger">
      <Trash2 size={13} />
    </button>
  );
}

export function McpServerEditor({ server, onChange, running }: ServerEditorProps) {
  const mcp: McpServerConfig = server.mcp ?? {};
  const set = (patch: Partial<McpServerConfig>) => onChange((s) => ({ ...s, mcp: { ...(s.mcp ?? {}), ...patch } }));
  const tools = mcp.tools ?? [];
  const resources = mcp.resources ?? [];
  const prompts = mcp.prompts ?? [];
  const workspace = useWorkspace((s) => s.info?.path);
  const stdio = ["zorvik", "serve", workspace ?? "<workspace>", server.name, "--stdio"].map(shellWord).join(" ");

  return (
    <>
      <EditorSection title="Tools" right={<AddButton label="Add tool" onClick={() => set({ tools: [...tools, { name: "", description: "", inputSchema: '{\n  "type": "object",\n  "properties": {}\n}', result: "" }] })} />}>
        <p className="text-[12px] text-muted">
          What AI apps can call. The description and input schema are what a model reads to decide when and how to call a tool. Results can use{" "}
          <code className="font-mono">{"{{args.name}}"}</code>, <code className="font-mono">{"{{args}}"}</code> (all arguments as JSON), dynamic and environment
          variables.
        </p>
        {tools.map((tool, i) => (
          <ToolEditor
            key={i}
            tool={tool}
            onChange={(patch) => set({ tools: tools.map((t, j) => (j === i ? { ...t, ...patch } : t)) })}
            onRemove={() => set({ tools: tools.filter((_, j) => j !== i) })}
          />
        ))}
      </EditorSection>

      <EditorSection title="Resources" right={<AddButton label="Add resource" onClick={() => set({ resources: [...resources, { uri: "", name: "", text: "" }] })} />}>
        <p className="text-[12px] text-muted">
          Documents clients can read. A URI with <code className="font-mono">{"{parts}"}</code> is a template (
          <code className="font-mono">{"users://{id}"}</code>) whose text can use <code className="font-mono">{"{{params.id}}"}</code>.
        </p>
        {resources.map((resource, i) => (
          <ResourceEditor
            key={i}
            resource={resource}
            onChange={(patch) => set({ resources: resources.map((r, j) => (j === i ? { ...r, ...patch } : r)) })}
            onRemove={() => set({ resources: resources.filter((_, j) => j !== i) })}
          />
        ))}
      </EditorSection>

      <EditorSection title="Prompts" right={<AddButton label="Add prompt" onClick={() => set({ prompts: [...prompts, { name: "", messages: [{ role: "user", text: "" }] }] })} />}>
        <p className="text-[12px] text-muted">
          Message templates users pick in AI apps. Their text can use <code className="font-mono">{"{{args.name}}"}</code>.
        </p>
        {prompts.map((prompt, i) => (
          <PromptEditor
            key={i}
            prompt={prompt}
            onChange={(patch) => set({ prompts: prompts.map((p, j) => (j === i ? { ...p, ...patch } : p)) })}
            onRemove={() => set({ prompts: prompts.filter((_, j) => j !== i) })}
          />
        ))}
      </EditorSection>

      <EditorSection title="Introduction">
        <div className="grid grid-cols-2 gap-3">
          <Field label="Name clients see" hint="Empty: the server's name.">
            <Input aria-label="Server name" className={mono} value={mcp.serverName ?? ""} placeholder={server.name} onChange={(e) => set({ serverName: e.target.value })} />
          </Field>
          <Field label="Version">
            <Input aria-label="Version" className={mono} value={mcp.version ?? ""} placeholder="1.0.0" onChange={(e) => set({ version: e.target.value })} />
          </Field>
        </div>
        <Field label="Instructions" hint="Sent when a client connects: how to use this server (AI apps pass it to the model).">
          <textarea
            aria-label="Instructions"
            value={mcp.instructions ?? ""}
            spellCheck={false}
            rows={rows(mcp.instructions)}
            onChange={(e) => set({ instructions: e.target.value })}
            className={area}
          />
        </Field>
      </EditorSection>

      <EditorSection title="Clients">
        <div className="grid grid-cols-2 gap-3">
          <Field label="Endpoint path" hint="Streamable HTTP. Clients of the older HTTP+SSE transport use /sse.">
            <Input aria-label="Endpoint path" className={mono} value={mcp.path ?? "/mcp"} placeholder="/mcp" onChange={(e) => set({ path: e.target.value })} />
          </Field>
          <div className="pt-6">
            <Switch checked={mcp.cors ?? false} onChange={(cors) => set({ cors })} label="Allow browser-based clients (CORS)" />
          </div>
        </div>
        <div className="flex flex-col gap-1.5 text-[12px] text-muted">
          <div>
            Connect an AI app or MCP client to{" "}
            {running ? <CopyCode text={running.url} /> : <span>its address once it runs</span>}, or let it start the server as a program over stdio:
          </div>
          <CopyCode text={stdio} />
          <div>Every request and answer shows in the traffic panel. Changes reach running servers at once, and connected clients are told.</div>
        </div>
      </EditorSection>
    </>
  );
}

function AddButton({ label, onClick }: { label: string; onClick: () => void }) {
  return (
    <Button size="sm" icon={<Plus size={13} />} onClick={onClick}>
      {label}
    </Button>
  );
}

function CopyCode({ text }: { text: string }) {
  return (
    <button type="button" title="Copy" onClick={() => void copyText(text)} className="selectable break-all rounded bg-panel-2 px-1.5 py-0.5 text-left font-mono text-[12px] text-fg hover:bg-hover">
      {text}
    </button>
  );
}

function Card({ enabled, onEnable, onRemove, label, children }: { enabled: boolean; onEnable: (v: boolean) => void; onRemove: () => void; label: string; children: React.ReactNode }) {
  return (
    <div className={cx("flex gap-2 rounded-lg border border-line p-2.5 text-[12.5px]", !enabled && "opacity-50")} data-testid="mcp-mock-item">
      <div className="pt-1.5">
        <Checkbox checked={enabled} onChange={onEnable} title={`Offer this ${label}`} />
      </div>
      <div className="flex min-w-0 flex-1 flex-col gap-2">{children}</div>
      <div>
        <RemoveButton label={`Remove ${label}`} onClick={onRemove} />
      </div>
    </div>
  );
}

function ToolEditor({ tool, onChange, onRemove }: { tool: McpToolMock; onChange: (p: Partial<McpToolMock>) => void; onRemove: () => void }) {
  return (
    <Card enabled={tool.enabled !== false} onEnable={(enabled) => onChange({ enabled })} onRemove={onRemove} label="tool">
      <div className="grid grid-cols-[minmax(0,1fr)_minmax(0,1.6fr)] gap-2">
        <Input aria-label="Tool name" className={mono} value={tool.name} placeholder="get_weather" onChange={(e) => onChange({ name: e.target.value })} />
        <Input aria-label="Description" value={tool.description ?? ""} placeholder="What it does and when to use it" onChange={(e) => onChange({ description: e.target.value })} />
      </div>
      <div className="flex flex-col gap-2">
        <Field label="Input schema (JSON Schema)" hint="Missing required arguments answer as a failed call.">
          <textarea aria-label="Input schema" value={tool.inputSchema ?? ""} spellCheck={false} wrap="off" rows={rows(tool.inputSchema, 3, 12)} onChange={(e) => onChange({ inputSchema: e.target.value })} className={area} />
        </Field>
        <Field label="Result" hint={tool.outputSchema ? "JSON matching the output schema (also sent as structuredContent)." : "Text; JSON text is fine."}>
          <textarea
            aria-label="Result"
            value={tool.result ?? ""}
            spellCheck={false}
            wrap="off"
            rows={rows(tool.result, 3, 12)}
            placeholder={'{"city": "{{args.city}}", "forecast": "sunny"}'}
            onChange={(e) => onChange({ result: e.target.value })}
            className={area}
          />
        </Field>
      </div>
      <div className="flex flex-wrap items-center gap-x-4 gap-y-1.5 text-muted">
        <label className="flex cursor-pointer items-center gap-1.5">
          <Checkbox checked={tool.isError ?? false} onChange={(isError) => onChange({ isError: isError || undefined })} label="Answer as a failed call" />
          answer as a failed call (isError)
        </label>
        <label className="flex items-center gap-1.5">
          after
          <Input
            aria-label="Delay in milliseconds"
            type="number"
            min={0}
            className={cx(mono, "w-20")}
            value={tool.delayMs ?? 0}
            onChange={(e) => onChange({ delayMs: Math.max(0, Math.trunc(Number(e.target.value)) || 0) })}
          />
          ms
        </label>
        <Input aria-label="Title" className="w-48" value={tool.title ?? ""} placeholder="Title (optional)" onChange={(e) => onChange({ title: e.target.value })} />
      </div>
      <details open={!!tool.outputSchema}>
        <summary className="cursor-pointer select-none text-[12px] text-faint">Output schema (structured results)</summary>
        <textarea
          aria-label="Output schema"
          value={tool.outputSchema ?? ""}
          spellCheck={false}
          wrap="off"
          rows={rows(tool.outputSchema, 3, 12)}
          placeholder={'{"type": "object", "properties": {"forecast": {"type": "string"}}}'}
          onChange={(e) => onChange({ outputSchema: e.target.value })}
          className={cx(area, "mt-1.5")}
        />
      </details>
    </Card>
  );
}

function ResourceEditor({ resource, onChange, onRemove }: { resource: McpResourceMock; onChange: (p: Partial<McpResourceMock>) => void; onRemove: () => void }) {
  return (
    <Card enabled={resource.enabled !== false} onEnable={(enabled) => onChange({ enabled })} onRemove={onRemove} label="resource">
      <div className="grid grid-cols-[minmax(0,1.6fr)_minmax(0,1fr)_minmax(0,1fr)] gap-2">
        <Input aria-label="URI" className={mono} value={resource.uri} placeholder="docs://readme or users://{id}" onChange={(e) => onChange({ uri: e.target.value })} />
        <Input aria-label="Resource name" value={resource.name} placeholder="Name" onChange={(e) => onChange({ name: e.target.value })} />
        <Input aria-label="MIME type" className={mono} value={resource.mimeType ?? ""} placeholder="text/plain" onChange={(e) => onChange({ mimeType: e.target.value })} />
      </div>
      <Input aria-label="Resource description" value={resource.description ?? ""} placeholder="Description (optional)" onChange={(e) => onChange({ description: e.target.value })} />
      <textarea aria-label="Text" value={resource.text ?? ""} spellCheck={false} rows={rows(resource.text, 2, 12)} placeholder="The content" onChange={(e) => onChange({ text: e.target.value })} className={area} />
    </Card>
  );
}

function PromptEditor({ prompt, onChange, onRemove }: { prompt: McpPromptMock; onChange: (p: Partial<McpPromptMock>) => void; onRemove: () => void }) {
  const args = prompt.arguments ?? [];
  const messages = prompt.messages ?? [];
  return (
    <Card enabled={prompt.enabled !== false} onEnable={(enabled) => onChange({ enabled })} onRemove={onRemove} label="prompt">
      <div className="grid grid-cols-[minmax(0,1fr)_minmax(0,1.6fr)] gap-2">
        <Input aria-label="Prompt name" className={mono} value={prompt.name} placeholder="code_review" onChange={(e) => onChange({ name: e.target.value })} />
        <Input aria-label="Prompt description" value={prompt.description ?? ""} placeholder="Description" onChange={(e) => onChange({ description: e.target.value })} />
      </div>
      <div className="flex flex-col gap-1.5">
        <div className="text-[11.5px] text-faint">Arguments</div>
        {args.map((a, i) => (
          <div key={i} className="flex items-center gap-2">
            <Input aria-label="Argument name" className={cx(mono, "w-40")} value={a.name} placeholder="name" onChange={(e) => onChange({ arguments: args.map((x, j) => (j === i ? { ...x, name: e.target.value } : x)) })} />
            <Input aria-label="Argument description" className="flex-1" value={a.description ?? ""} placeholder="Description" onChange={(e) => onChange({ arguments: args.map((x, j) => (j === i ? { ...x, description: e.target.value } : x)) })} />
            <label className="flex items-center gap-1.5 text-muted">
              <Checkbox checked={a.required ?? false} onChange={(required) => onChange({ arguments: args.map((x, j) => (j === i ? { ...x, required: required || undefined } : x)) })} label="Required" />
              required
            </label>
            <RemoveButton label="Remove argument" onClick={() => onChange({ arguments: args.filter((_, j) => j !== i) })} />
          </div>
        ))}
        <div>
          <Button size="sm" variant="ghost" icon={<Plus size={13} />} onClick={() => onChange({ arguments: [...args, { name: "" }] })}>
            Add argument
          </Button>
        </div>
      </div>
      <div className="flex flex-col gap-1.5">
        <div className="text-[11.5px] text-faint">Messages</div>
        {messages.map((m, i) => (
          <div key={i} className="flex items-start gap-2">
            <Select aria-label="Role" className="w-28" value={m.role || "user"} onChange={(e) => onChange({ messages: messages.map((x, j) => (j === i ? { ...x, role: e.target.value } : x)) })}>
              <option value="user">user</option>
              <option value="assistant">assistant</option>
            </Select>
            <textarea
              aria-label="Message"
              value={m.text}
              spellCheck={false}
              rows={rows(m.text, 2, 8)}
              placeholder="Review this code: {{args.code}}"
              onChange={(e) => onChange({ messages: messages.map((x, j) => (j === i ? { ...x, text: e.target.value } : x)) })}
              className={cx(area, "flex-1")}
            />
            <RemoveButton label="Remove message" onClick={() => onChange({ messages: messages.filter((_, j) => j !== i) })} />
          </div>
        ))}
        <div>
          <Button size="sm" variant="ghost" icon={<Plus size={13} />} onClick={() => onChange({ messages: [...messages, { role: "user", text: "" }] })}>
            Add message
          </Button>
        </div>
      </div>
    </Card>
  );
}
