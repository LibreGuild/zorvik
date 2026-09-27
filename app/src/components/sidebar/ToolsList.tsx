// Sidebar: network tools.
import { isToolTab, openTool, useTabs } from "../../store/tabs";
import { cx } from "../ui";
import { TOOLS } from "../tools/registry";

export function ToolsList() {
  const activeTool = useTabs((s) => {
    const t = s.tabs.find((x) => x.id === s.activeId);
    return isToolTab(t) ? t.tool : null;
  });
  return (
    <div className="min-h-0 flex-1 overflow-auto px-1.5 py-2" data-testid="tools-list">
      {TOOLS.map((tool) => (
        <button
          key={tool.id}
          onClick={() => openTool(tool.id)}
          className={cx(
            "flex w-full items-start gap-2.5 rounded-lg px-2 py-2 text-left outline-none focus-visible:ring-1 focus-visible:ring-accent",
            activeTool === tool.id ? "bg-elev" : "hover:bg-hover/70",
          )}
        >
          <span className="mt-0.5 shrink-0 text-muted">{tool.icon(15)}</span>
          <span className="min-w-0">
            <span className="block text-[12.5px] font-medium text-fg">{tool.label}</span>
            <span className="block text-[11.5px] leading-snug text-faint">{tool.description}</span>
          </span>
        </button>
      ))}
    </div>
  );
}
