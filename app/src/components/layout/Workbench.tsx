import { useRef } from "react";
import { Columns2, FilePlus2, Import, Radio, Rows2, Zap } from "lucide-react";
import { modKey } from "../../lib/platform";
import { isLive, isLoadTestTab, isRunnerTab, isServerTab, isToolTab, newRequest, type Tab, useTabs } from "../../store/tabs";
import { LoadTestView } from "../loadtests/LoadTestView";
import { RunnerView } from "../runner/RunnerView";
import { openModal, useUi } from "../../store/ui";
import { RequestEditor } from "../request/RequestEditor";
import { UrlBar } from "../request/UrlBar";
import { ResponsePane } from "../response/ResponsePane";
import { ServerView } from "../servers/ServerView";
import { StreamPane } from "../stream/StreamPane";
import { ToolView } from "../tools/ToolView";
import { RESULT_PANES } from "../request/kinds";
import { Splitter } from "../Splitter";
import { Banner, Button, IconButton, Kbd } from "../ui";

export function Workbench() {
  const tab = useTabs((s) => s.tabs.find((t) => t.id === s.activeId));
  if (!tab) return <NoTab />;
  if (isServerTab(tab)) return <ServerView key={tab.id} tab={tab} />;
  if (isToolTab(tab)) return <ToolView key={tab.id} tab={tab} />;
  if (isLoadTestTab(tab)) return <LoadTestView key={tab.id} tab={tab} />;
  if (isRunnerTab(tab)) return <RunnerView key={tab.id} tab={tab} />;
  return <RequestWorkbench key={tab.id} tab={tab} />;
}

function RequestWorkbench({ tab }: { tab: Tab }) {
  const split = useUi((s) => s.split);
  const layout = useUi((s) => s.layout);
  const area = useRef<HTMLDivElement>(null);
  const kind = tab.draft.kind ?? "http";
  const ResultPane = RESULT_PANES[kind];
  const side = layout === "side";

  return (
    <div className="flex h-full min-h-0 flex-col bg-bg">
      {tab.orphaned && (
        <Banner tone="warning">
          {tab.path ? "This request changed on disk and could not be reloaded." : "This request was deleted or moved outside Zorvik."} Save it to keep your
          changes.
        </Banner>
      )}
      <UrlBar tab={tab} />
      <div ref={area} className={side ? "flex min-h-0 flex-1" : "flex min-h-0 flex-1 flex-col"}>
        <div style={side ? { width: `${split * 100}%` } : { height: `${split * 100}%` }} className="min-h-0 min-w-0 shrink-0 overflow-hidden">
          <RequestEditor
            tab={tab}
            toolbar={
              <IconButton
                label={side ? "Stack response below" : "Show response beside"}
                onClick={() => useUi.setState({ layout: side ? "stacked" : "side" })}
              >
                {side ? <Rows2 size={14} /> : <Columns2 size={14} />}
              </IconButton>
            }
          />
        </div>
        <Splitter
          direction={side ? "horizontal" : "vertical"}
          onResize={(delta) => {
            const el = area.current;
            if (!el) return;
            const total = side ? el.clientWidth : el.clientHeight;
            useUi.setState((s) => ({ split: Math.min(0.8, Math.max(0.2, s.split + delta / total)) }));
          }}
        />
        <div className="min-h-0 min-w-0 flex-1 overflow-hidden">
          {ResultPane ? <ResultPane tab={tab} /> : isLive(tab) ? <StreamPane tab={tab} /> : <ResponsePane tab={tab} />}
        </div>
      </div>
    </div>
  );
}

function NoTab() {
  return (
    <div className="flex h-full flex-col items-center justify-center gap-5 bg-bg p-8">
      <img src="/icon.png" alt="" className="h-14 w-14 opacity-90" />
      <div className="text-center">
        <div className="text-[16px] font-semibold text-fg">Start a request</div>
        <div className="mt-1 text-[12.5px] text-muted">Open one from the sidebar, or create a new one.</div>
      </div>
      <div className="grid grid-cols-2 gap-2">
        <Button icon={<FilePlus2 size={14} />} onClick={() => newRequest("http")}>
          HTTP request
        </Button>
        <Button icon={<Zap size={14} />} onClick={() => newRequest("websocket")}>
          WebSocket
        </Button>
        <Button icon={<Radio size={14} />} onClick={() => newRequest("sse")}>
          Event stream
        </Button>
        <Button icon={<Import size={14} />} onClick={() => openModal({ type: "import" })}>
          Import…
        </Button>
      </div>
      <div className="flex flex-col items-center gap-1.5 text-[12px] text-faint">
        <span>
          <Kbd>{modKey}</Kbd> <Kbd>N</Kbd> new request · <Kbd>{modKey}</Kbd> <Kbd>K</Kbd> go to… · <Kbd>{modKey}</Kbd> <Kbd>,</Kbd> settings
        </span>
      </div>
    </div>
  );
}
