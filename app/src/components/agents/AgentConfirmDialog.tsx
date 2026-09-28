// An AI agent's action waiting for the user: allow once, for the session, or deny.
import { useEffect, useState } from "react";
import { Bot, TriangleAlert } from "lucide-react";
import type { AgentConfirm } from "../../bindings/AgentConfirm";
import { answer, useAgents } from "../../store/agents";
import { Button, cx, Modal } from "../ui";

/** Answer buttons wait this long after a question appears: a key or click meant for
 *  something else (typing, a double click on the last question) must not approve it. */
const ARM_DELAY_MS = 700;

function useSecondsLeft(until: number) {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const t = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(t);
  }, []);
  return Math.max(0, Math.ceil((until - now) / 1000));
}

export function AgentConfirmDialog() {
  const request = useAgents((s) => s.pending[0] ?? null);
  const waiting = useAgents((s) => s.pending.length);
  if (!request) return null;
  return <Question key={request.id} request={request} more={waiting - 1} />;
}

function Question({ request, more }: { request: AgentConfirm; more: number }) {
  const left = useSecondsLeft(request.expiresAt);
  const [armed, setArmed] = useState(false);
  useEffect(() => {
    const t = setTimeout(() => setArmed(true), ARM_DELAY_MS);
    return () => clearTimeout(t);
  }, []);
  const deny = () => void answer(request.id, false);
  return (
    <Modal
      open
      onClose={deny}
      dismissOnOutside={false}
      title={
        <span className="flex items-center gap-2">
          <Bot size={15} className="shrink-0 text-accent" />
          {request.title}
        </span>
      }
      description={request.message}
      width={500}
      focusFirstField={false}
      footer={
        <>
          <span className="mr-auto text-[11.5px] tabular-nums text-faint">
            {left > 0 ? `Denied in ${Math.floor(left / 60)}:${String(left % 60).padStart(2, "0")}` : "Expired"}
            {more > 0 && ` · ${more} more waiting`}
          </span>
          {/* Focus starts on Deny: Enter or Space meant for something else says no. */}
          <Button variant="ghost" onClick={deny} autoFocus data-testid="agent-deny">
            Deny
          </Button>
          {request.sessionOption && (
            <Button disabled={!armed} onClick={() => void answer(request.id, true, true)} data-testid="agent-allow-session">
              Allow for this session
            </Button>
          )}
          <Button
            variant={request.danger ? "danger" : "primary"}
            disabled={!armed}
            onClick={() => void answer(request.id, true)}
            data-testid="agent-allow"
          >
            {request.confirmLabel}
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-3" data-testid="agent-confirm">
        <div className="text-[11.5px] text-faint">Asked by {request.client}, an AI agent</div>
        {request.items.length > 0 && (
          <ul className="selectable max-h-60 overflow-auto rounded-md border border-line bg-panel-2 px-3 py-2 font-mono text-[12px] leading-relaxed text-fg">
            {request.items.map((item, i) => (
              <li key={i} className="break-all">
                {item}
              </li>
            ))}
          </ul>
        )}
        {request.danger && (
          <div className={cx("flex items-start gap-2 rounded-md bg-danger/10 px-2.5 py-2 text-[12px] text-danger")}>
            <TriangleAlert size={14} className="mt-px shrink-0" />
            {request.kind === "delete"
              ? "They go to the trash; the run history of a load test is deleted."
              : request.kind === "server"
                ? "Other devices on your network will be able to reach it."
                : request.kind === "program"
                  ? "The program runs on your computer with your permissions: it can read your files and reach the network."
                  : "This reaches systems outside this computer and your private network."}
          </div>
        )}
      </div>
    </Modal>
  );
}
