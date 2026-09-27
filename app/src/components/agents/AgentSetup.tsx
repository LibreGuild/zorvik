// How to connect an AI agent: the commands for each one, with this computer's `zorvik` path.
import { useState } from "react";
import { Check, Copy } from "lucide-react";
import { copyText } from "../../lib/platform";
import { useWorkspace } from "../../store/workspace";
import { cx } from "../ui";

function quote(path: string) {
  return /[\s'"]/.test(path) ? `"${path}"` : path;
}

export function agentSetups(cli: string): { agent: string; command: string; note?: string }[] {
  const q = quote(cli);
  return [
    { agent: "Claude Code", command: `claude mcp add --scope user zorvik -- ${q} mcp` },
    { agent: "Codex", command: `codex mcp add zorvik -- ${q} mcp` },
    { agent: "Gemini CLI", command: `gemini mcp add --scope user zorvik ${q} mcp` },
    {
      agent: "Cursor, Antigravity, Windsurf, VS Code…",
      command: JSON.stringify({ mcpServers: { zorvik: { command: cli, args: ["mcp"] } } }, null, 2),
      note: "Add to the agent's MCP settings file (VS Code: under \"servers\" in .vscode/mcp.json).",
    },
  ];
}

function CopyBlock({ text }: { text: string }) {
  const [copied, setCopied] = useState(false);
  return (
    <div className="group relative">
      <pre className="selectable whitespace-pre-wrap break-all rounded-md border border-line bg-panel-2 py-1.5 pl-2.5 pr-9 font-mono text-[11.5px] leading-relaxed text-fg">{text}</pre>
      <button
        aria-label="Copy"
        onClick={async () => {
          await copyText(text);
          setCopied(true);
          setTimeout(() => setCopied(false), 1500);
        }}
        className={cx("absolute right-1 top-1 rounded p-1 text-faint hover:bg-hover hover:text-fg", copied && "text-success")}
      >
        {copied ? <Check size={13} /> : <Copy size={13} />}
      </button>
    </div>
  );
}

/** Setup commands for the agents people use. */
export function AgentSetup({ compact = false }: { compact?: boolean }) {
  const cliPath = useWorkspace((s) => s.appInfo?.cliPath ?? null);
  const setups = agentSetups(cliPath ?? "zorvik");
  return (
    <div className="flex flex-col gap-3" data-testid="agent-setup">
      {!cliPath && (
        <p className="text-[11.5px] text-faint">The zorvik command-line tool was not found next to the app; the commands assume it is on your PATH.</p>
      )}
      {setups.slice(0, compact ? 3 : setups.length).map((s) => (
        <div key={s.agent} className="flex flex-col gap-1">
          <div className="text-[12px] font-medium text-muted">{s.agent}</div>
          <CopyBlock text={s.command} />
          {s.note && <div className="text-[11px] text-faint">{s.note}</div>}
        </div>
      ))}
      <p className="text-[11.5px] leading-snug text-faint">
        Then ask the agent, e.g. “map the APIs in this repo to Zorvik” or “run the Users folder in Zorvik”. Prompts: /mcp__zorvik__map_apis,
        /mcp__zorvik__test_apis.
      </p>
    </div>
  );
}
