// Confirm/prompt dialogs and toasts.
import { useState } from "react";
import { CheckCircle2, CircleAlert, Info, X } from "lucide-react";
import { useDialogs } from "../../store/dialogs";
import { dismissToast, useToasts } from "../../store/toasts";
import { Button, cx, Input, Modal } from "../ui";

export function Dialogs() {
  const current = useDialogs((s) => s.current);
  const [value, setValue] = useState("");
  // Reset the field during render, not in an effect, so it mounts with the new value and
  // autoFocus + select() pick it up (an effect ran after focus, leaving nothing selected).
  const [shown, setShown] = useState<typeof current>(null);
  if (shown !== current) {
    setShown(current);
    setValue(current?.kind === "prompt" ? current.value : "");
  }
  if (!current) return null;
  const close = (result: boolean) => {
    useDialogs.setState({ current: null });
    if (current.kind === "confirm") current.resolve(result);
    else current.resolve(result && value.trim() ? value.trim() : null);
  };
  return (
    <Modal
      open
      onClose={() => close(false)}
      title={current.title}
      width={440}
      footer={
        <>
          <Button variant="ghost" onClick={() => close(false)}>
            Cancel
          </Button>
          <Button
            variant={current.kind === "confirm" && current.danger ? "danger" : "primary"}
            onClick={() => close(true)}
            disabled={current.kind === "prompt" && !value.trim()}
            autoFocus={current.kind === "confirm"}
          >
            {current.confirmLabel}
          </Button>
        </>
      }
    >
      {current.kind === "confirm" ? (
        <div className="flex flex-col gap-2.5">
          <p className="text-[13px] text-muted">{current.message}</p>
          {current.details && current.details.length > 0 && (
            <ul className="selectable max-h-48 overflow-auto rounded-lg border border-line bg-panel-2 px-3 py-2 font-mono text-[12px] text-fg" data-testid="confirm-details">
              {current.details.map((d, i) => (
                <li key={i} className="break-all">
                  {d}
                </li>
              ))}
            </ul>
          )}
        </div>
      ) : (
        <div className="flex flex-col gap-2">
          {current.message && <p className="text-[13px] text-muted">{current.message}</p>}
          <Input
            autoFocus
            value={value}
            placeholder={current.placeholder}
            onChange={(e) => setValue(e.target.value)}
            onFocus={(e) => e.target.select()}
            onKeyDown={(e) => {
              if (e.key === "Enter" && !e.nativeEvent.isComposing && e.keyCode !== 229 && value.trim()) close(true);
            }}
          />
        </div>
      )}
    </Modal>
  );
}

export function Toasts() {
  const toasts = useToasts((s) => s.toasts);
  return (
    <div className="pointer-events-none fixed bottom-4 right-4 z-[120] flex w-[360px] flex-col gap-2" role="status" aria-live="polite">
      {toasts.map((t) => (
        <div key={t.id} className="zv-pop pointer-events-auto flex items-start gap-2.5 rounded-lg border border-line bg-elev p-3 shadow-pop">
          {t.tone === "success" ? (
            <CheckCircle2 size={16} className="mt-px shrink-0 text-success" />
          ) : t.tone === "error" ? (
            <CircleAlert size={16} className="mt-px shrink-0 text-danger" />
          ) : (
            <Info size={16} className="mt-px shrink-0 text-info" />
          )}
          <div className="min-w-0 flex-1">
            <div className="text-[12.5px] font-medium text-fg">{t.title}</div>
            {t.detail && <div className={cx("selectable mt-0.5 break-words text-[12px] text-muted")}>{t.detail}</div>}
          </div>
          <button aria-label="Dismiss" onClick={() => dismissToast(t.id)} className="rounded p-0.5 text-faint hover:text-fg">
            <X size={13} />
          </button>
        </div>
      ))}
    </div>
  );
}
