import { FileUp, Wand2 } from "lucide-react";
import type { Body } from "../../bindings/Body";
import type { BodyType } from "../../bindings/BodyType";
import type { KeyValue } from "../../bindings/KeyValue";
import type { MultipartField } from "../../bindings/MultipartField";
import { pickFile } from "../../lib/platform";
import { type Tab, updateDraft } from "../../store/tabs";
import { toast } from "../../store/toasts";
import { useVariableNames } from "../../store/workspace";
import { CodeEditor, type EditorLanguage } from "../CodeEditor";
import { KeyValueEditor } from "../KeyValueEditor";
import { VarInput } from "../VarInput";
import { Button, EmptyState, Select } from "../ui";
import { GraphqlEditor, GraphqlToolbar } from "./GraphqlEditor";
import { beautifyJson } from "./jsonFormat";

const TYPES: { id: BodyType; label: string }[] = [
  { id: "none", label: "None" },
  { id: "json", label: "JSON" },
  { id: "text", label: "Text" },
  { id: "xml", label: "XML" },
  { id: "formUrlencoded", label: "Form URL-encoded" },
  { id: "multipart", label: "Multipart form" },
  { id: "binary", label: "Binary file" },
  { id: "graphql", label: "GraphQL" },
];

const TEXT_TYPES = ["text/plain", "text/html", "text/csv", "application/javascript", "application/graphql", "application/yaml"];

function textLanguage(body: Body): EditorLanguage {
  if (body.type === "json") return "json";
  if (body.type === "xml") return "xml";
  const ct = body.contentType ?? "";
  if (ct.includes("html")) return "html";
  if (ct.includes("javascript")) return "javascript";
  return "text";
}

/** A GraphQL body from a JSON body shaped like `{"query", "variables", "operationName"}` (else empty). */
function graphqlFromJson(body: Body): Body["graphql"] {
  try {
    const op = JSON.parse(body.text ?? "") as { query?: unknown; variables?: unknown; operationName?: unknown };
    if (typeof op?.query !== "string") return { query: "" };
    const variables = op.variables && typeof op.variables === "object" ? JSON.stringify(op.variables, null, 2) : undefined;
    return { query: op.query, variables, operationName: typeof op.operationName === "string" ? op.operationName : undefined };
  } catch {
    return { query: "" };
  }
}

/** `tab` is needed for GraphQL bodies (the schema comes from the request's URL, headers and auth). */
export function BodyEditor({ body, onChange, onSubmit, tab }: { body: Body; onChange: (b: Body) => void; onSubmit: () => void; tab?: Tab }) {
  const { names } = useVariableNames();
  const type = body.type;
  const textual = type === "json" || type === "text" || type === "xml";
  const changeType = (next: BodyType) => {
    if (next !== "graphql" || !tab) return onChange({ ...body, type: next });
    // A GraphQL body is sent as POST; start from a JSON body that already holds an operation.
    const graphql = body.graphql?.query ? body.graphql : graphqlFromJson(body);
    updateDraft(tab.id, (r) => ({ ...r, method: r.method === "GET" || !r.method ? "POST" : r.method, body: { ...body, type: next, graphql } }));
  };

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex h-10 shrink-0 items-center gap-2 px-3">
        <Select value={type} onChange={(e) => changeType(e.target.value as BodyType)} className="w-48" aria-label="Body type">
          {TYPES.filter((t) => t.id !== "graphql" || tab || type === "graphql").map((t) => (
            <option key={t.id} value={t.id}>
              {t.label}
            </option>
          ))}
        </Select>
        {type === "text" && (
          <Select
            value={body.contentType ?? "text/plain"}
            onChange={(e) => onChange({ ...body, contentType: e.target.value })}
            className="w-52"
            aria-label="Content type"
          >
            {/* Keep an imported/hand-edited type visible instead of silently showing text/plain. */}
            {[...TEXT_TYPES, ...(body.contentType && !TEXT_TYPES.includes(body.contentType) ? [body.contentType] : [])].map((t) => (
              <option key={t}>{t}</option>
            ))}
          </Select>
        )}
        {type === "graphql" && tab ? <GraphqlToolbar tab={tab} body={body} onChange={onChange} /> : <div className="flex-1" />}
        {type === "json" && (
          <Button
            size="sm"
            variant="ghost"
            icon={<Wand2 size={13} />}
            onClick={() => {
              const pretty = beautifyJson(body.text ?? "");
              if (pretty === null) toast("error", "Body is not valid JSON");
              else onChange({ ...body, text: pretty });
            }}
          >
            Beautify
          </Button>
        )}
      </div>
      <div className="min-h-0 flex-1 overflow-auto">
        {type === "none" && <EmptyState title="This request has no body">Choose a body type above to add one.</EmptyState>}
        {textual && (
          <CodeEditor
            value={body.text ?? ""}
            onChange={(text) => onChange({ ...body, text })}
            language={textLanguage(body)}
            variables={names}
            onSubmit={onSubmit}
            placeholder={type === "json" ? '{\n  "name": "{{name}}"\n}' : "Request body"}
          />
        )}
        {type === "graphql" && tab && <GraphqlEditor tab={tab} body={body} onChange={onChange} onSubmit={onSubmit} />}
        {type === "formUrlencoded" && (
          <KeyValueEditor rows={body.form ?? []} onChange={(form: KeyValue[]) => onChange({ ...body, form })} onEnter={onSubmit} />
        )}
        {type === "multipart" && <MultipartEditor fields={body.multipart ?? []} onChange={(multipart) => onChange({ ...body, multipart })} />}
        {type === "binary" && (
          <div className="flex items-center gap-2 p-4">
            <div className="min-w-0 flex-1 rounded-md border border-line bg-input">
              <VarInput value={body.file ?? ""} onChange={(file) => onChange({ ...body, file })} placeholder="Path to file (absolute, or relative to the workspace)" />
            </div>
            <Button
              icon={<FileUp size={14} />}
              onClick={async () => {
                const file = await pickFile("Choose body file");
                if (file) onChange({ ...body, file });
              }}
            >
              Browse…
            </Button>
          </div>
        )}
      </div>
    </div>
  );
}

function MultipartEditor({ fields, onChange }: { fields: MultipartField[]; onChange: (f: MultipartField[]) => void }) {
  // Rows carry the multipart-only props (file, contentType) through the table unchanged.
  return (
    <KeyValueEditor
      rows={fields as unknown as KeyValue[]}
      onChange={(next) => onChange(next as unknown as MultipartField[])}
      renderValue={(row, _i, update) => {
        const isFile = (row as unknown as MultipartField).file ?? false;
        return (
          <div className="flex items-center">
            <select
              value={isFile ? "file" : "text"}
              onChange={(e) => update({ file: e.target.value === "file", value: "" } as Partial<KeyValue>)}
              className="h-7 border-r border-line bg-transparent px-1.5 text-[11.5px] text-muted outline-none"
              aria-label="Field type"
            >
              <option value="text">Text</option>
              <option value="file">File</option>
            </select>
            <VarInput value={row.value} onChange={(value) => update({ value })} placeholder={isFile ? "File path" : "Value"} className="flex-1" />
            {isFile && (
              <button
                className="mr-1 rounded px-1.5 py-0.5 text-[11.5px] text-muted hover:bg-hover hover:text-fg"
                onClick={async () => {
                  const file = await pickFile("Choose file");
                  if (file) update({ value: file });
                }}
              >
                Browse
              </button>
            )}
          </div>
        );
      }}
    />
  );
}
