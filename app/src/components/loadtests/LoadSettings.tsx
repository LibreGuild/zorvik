// The settings side of a load test tab: requests, model, stages, options, thresholds.
import { memo, useCallback } from "react";
import type { HttpVersionPref } from "../../bindings/HttpVersionPref";
import type { LoadModel } from "../../bindings/LoadModel";
import type { LoadStage } from "../../bindings/LoadStage";
import type { LoadTarget } from "../../bindings/LoadTarget";
import type { LoadTest } from "../../bindings/LoadTest";
import type { Threshold } from "../../bindings/Threshold";
import type { TreeNode } from "../../bindings/TreeNode";
import { useWorkspace } from "../../store/workspace";
import { EditorSection } from "../servers/ServerView";
import { Segmented, Switch } from "../ui";
import { MODELS, sendingTargets } from "./model";
import { NumberInput } from "./parts";
import { StagesEditor } from "./StagesEditor";
import { TargetsEditor } from "./TargetsEditor";
import { ThresholdsEditor } from "./ThresholdsEditor";

const NO_NODES: TreeNode[] = [];
/** An hour: think time and timeout. */
const MAX_MS = 3_600_000;
const MAX_IN_FLIGHT = 100_000;

const HTTP_VERSIONS: { id: HttpVersionPref | ""; label: string }[] = [
  { id: "", label: "App setting" },
  { id: "auto", label: "Auto (negotiated)" },
  { id: "http1", label: "HTTP/1.1" },
  { id: "http2", label: "HTTP/2" },
];

const selectClass =
  "h-8 w-full rounded-lg border border-line bg-input px-2 text-[12.5px] text-fg outline-none hover:border-line-strong focus:border-accent focus:ring-2 focus:ring-accent-soft";

export const LoadSettings = memo(function LoadSettings({
  test,
  testId,
  onChange,
  running,
}: {
  test: LoadTest;
  testId: string;
  onChange: (fn: (t: LoadTest) => LoadTest) => void;
  /** This test runs now (edits apply to the next run). */
  running: boolean;
}) {
  const tree = useWorkspace((s) => s.info?.tree ?? NO_NODES);
  const setTargets = useCallback((fn: (l: LoadTarget[]) => LoadTarget[]) => onChange((t) => ({ ...t, targets: fn(t.targets) })), [onChange]);
  const setStages = useCallback((fn: (l: LoadStage[]) => LoadStage[]) => onChange((t) => ({ ...t, stages: fn(t.stages) })), [onChange]);
  const setThresholds = useCallback((fn: (l: Threshold[]) => Threshold[]) => onChange((t) => ({ ...t, thresholds: fn(t.thresholds ?? []) })), [onChange]);
  const model = test.model;
  const sending = sendingTargets(test.targets).length;

  return (
    <div className="@container flex flex-col pb-8" data-testid="load-settings">
      {running && (
        <p className="mx-4 mt-3 rounded-lg bg-panel-2 px-3 py-2 text-[12px] text-muted">This test is running with the settings it started with. Changes apply to the next run.</p>
      )}
      <EditorSection
        title="Requests"
        right={sending < test.targets.length ? <span className="text-[11px] text-faint">{`${sending} of ${test.targets.length} included`}</span> : undefined}
      >
        <TargetsEditor targets={test.targets} tree={tree} onChange={setTargets} />
      </EditorSection>

      <EditorSection title="Load model">
        <div>
          <Segmented<LoadModel>
            items={[
              { id: "virtualUsers", label: MODELS.virtualUsers.label },
              { id: "arrivalRate", label: MODELS.arrivalRate.label },
            ]}
            value={model}
            onChange={(m) => onChange((t) => ({ ...t, model: m }))}
          />
        </div>
        <p className="text-[12px] leading-relaxed text-muted">{MODELS[model].description}</p>
      </EditorSection>

      <EditorSection title="Stages">
        <StagesEditor stages={test.stages} model={model} testId={testId} onChange={setStages} />
      </EditorSection>

      <EditorSection title="Options">
        <div className="grid grid-cols-[repeat(auto-fill,minmax(170px,1fr))] gap-3">
          {model === "virtualUsers" ? (
            <Labeled label="Think time (ms)" hint="Pause after each answer, per user">
              <NumberInput aria-label="Think time in milliseconds" value={test.thinkTimeMs ?? 0} min={0} max={MAX_MS} onChange={(v) => onChange((t) => ({ ...t, thinkTimeMs: Math.min(MAX_MS, v ?? 0) }))} />
            </Labeled>
          ) : (
            <Labeled label="Max in flight" hint="Requests beyond this wait no longer: they count as dropped">
              <NumberInput aria-label="Most requests in flight" value={test.maxInFlight ?? 1000} min={1} max={MAX_IN_FLIGHT} onChange={(v) => onChange((t) => ({ ...t, maxInFlight: Math.min(MAX_IN_FLIGHT, v ?? 1) }))} />
            </Labeled>
          )}
          <Labeled label="Timeout (ms)" hint="Per request; empty uses the app setting, 0 = no limit">
            <NumberInput
              aria-label="Timeout in milliseconds"
              optional
              placeholder="App setting"
              value={test.timeoutMs}
              min={0}
              max={MAX_MS}
              onChange={(v) => onChange((t) => ({ ...t, timeoutMs: v === undefined ? undefined : Math.min(MAX_MS, v) }))}
            />
          </Labeled>
          <Labeled label="HTTP version">
            <select
              aria-label="HTTP version"
              value={test.httpVersion ?? ""}
              onChange={(e) => onChange((t) => ({ ...t, httpVersion: (e.target.value || undefined) as HttpVersionPref | undefined }))}
              className={selectClass}
            >
              {HTTP_VERSIONS.map((v) => (
                <option key={v.id} value={v.id}>
                  {v.label}
                </option>
              ))}
              {test.httpVersion === "http3" && <option value="http3">HTTP/3</option>}
            </select>
          </Labeled>
        </div>
        <div className="flex flex-col gap-1 pt-1">
          <Switch checked={test.keepAlive ?? true} onChange={(keepAlive) => onChange((t) => ({ ...t, keepAlive }))} label="Reuse connections (keep-alive)" />
          <p className="pl-[46px] text-[11.5px] leading-snug text-faint">
            {test.keepAlive ?? true
              ? "Like real clients with connection pools: HTTP/1.1 keep-alive, HTTP/2 multiplexing."
              : "A new connection per request: measures DNS, TCP and TLS setup each time and can run out of local ports (especially on Windows)."}
          </p>
        </div>
      </EditorSection>

      <EditorSection title="Thresholds">
        <ThresholdsEditor thresholds={test.thresholds ?? []} targets={test.targets} tree={tree} onChange={setThresholds} />
      </EditorSection>
    </div>
  );
});

function Labeled({ label, hint, children }: { label: string; hint?: string; children: React.ReactNode }) {
  return (
    <label className="flex min-w-0 flex-col gap-1">
      <span className="text-[11.5px] font-medium text-muted">{label}</span>
      {children}
      {hint && <span className="text-[11px] leading-snug text-faint">{hint}</span>}
    </label>
  );
}
