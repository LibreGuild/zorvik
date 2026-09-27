// A tool tab.
import type { ToolTab } from "../../store/tabs";
import { EmptyState } from "../ui";
import { toolInfo } from "./registry";

export function ToolView({ tab }: { tab: ToolTab }) {
  const tool = toolInfo(tab.tool);
  if (!tool) return <EmptyState title="Unknown tool">This tool is not available in this version.</EmptyState>;
  if (tool.bare) return <tool.View tab={tab} />;
  return (
    <div className="flex h-full min-h-0 flex-col bg-bg" data-testid={`tool-${tool.id}`}>
      <div className="flex shrink-0 items-center gap-2.5 px-4 pb-2 pt-3">
        <span className="flex h-8 w-8 items-center justify-center rounded-lg bg-panel-2 text-muted">{tool.icon(16)}</span>
        <div className="min-w-0">
          <div className="text-[14px] font-semibold text-fg">{tool.label}</div>
          <div className="truncate text-[12px] text-muted">{tool.description}</div>
        </div>
      </div>
      <div className="min-h-0 flex-1 overflow-auto">
        <tool.View tab={tab} />
      </div>
    </div>
  );
}
