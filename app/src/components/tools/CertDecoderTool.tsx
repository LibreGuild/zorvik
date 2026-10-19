// Certificate decoder: paste PEM (a certificate, a chain, a CSR, keys) or open a file and
// see what it holds. Decoded in Rust as you type; private keys are only used to find the
// certificate they belong to and are never shown.
import { Copy, FileBadge, FileText, FolderOpen, KeyRound, X } from "lucide-react";
import type { CertDecodeReport } from "../../bindings/CertDecodeReport";
import type { DecodedItem } from "../../bindings/DecodedItem";
import { copyText, pickFile } from "../../lib/platform";
import { api } from "../../lib/rpc";
import { type ToolTab } from "../../store/tabs";
import { toast } from "../../store/toasts";
import { toolState } from "../../store/tools";
import { Badge, Button, cx, EmptyState, IconButton, Spinner } from "../ui";
import { editInputs, useLiveResult } from "./live";
import { itemRole, itemTitle, keyDescription, keyOwners } from "./models";
import { ErrorNote, KV, Section, textareaClass } from "./parts";
import { CertificateCard, WarningRow } from "./TlsInspectorTool";

interface CertState {
  text: string;
  /** A file to read instead of the text. */
  path: string | null;
}

const DEFAULTS: CertState = { text: "", path: null };

const FILE_TYPES = ["pem", "crt", "cer", "cert", "der", "csr", "key", "pub"];

export function CertDecoderTool({ tab }: { tab: ToolTab }) {
  const s = toolState(tab, DEFAULTS);
  const edit = (patch: Partial<CertState>) => editInputs(tab.id, () => patch);
  const { text, path } = s;
  const run = path ? () => api.certDecode({ path }) : text.trim() ? () => api.certDecode({ text }) : null;
  const live = useLiveResult(tab, run, 200);
  const report = live.value;
  const fileName = path?.split(/[\\/]/).pop() ?? null;

  const openFile = async () => {
    const picked = await pickFile("Open a certificate, request or key", FILE_TYPES);
    if (picked) edit({ path: picked });
  };

  return (
    <div className="flex min-h-full flex-col pb-6" data-testid="cert-decoder">
      <div className="flex flex-col gap-1.5 px-4">
        <div className="flex h-7 items-center gap-1">
          <span className="flex-1 text-[12px] font-medium text-muted">{path ? "File" : "PEM text"}</span>
          {live.busy && run && <Spinner size={12} />}
          <Button size="sm" variant="ghost" icon={<FolderOpen size={13} />} onClick={() => void openFile()}>
            Open file…
          </Button>
          {(text || path) && (
            <IconButton label="Clear" onClick={() => edit({ text: "", path: null })}>
              <X size={13} />
            </IconButton>
          )}
        </div>
        {path ? (
          <div className="flex items-center gap-2 rounded-lg border border-line bg-panel-2 px-3 py-2 text-[12.5px]">
            <FileText size={14} className="shrink-0 text-muted" />
            <span className="selectable min-w-0 flex-1 truncate font-mono text-[12px] text-fg" title={path}>
              {fileName}
            </span>
            <Button size="sm" variant="ghost" onClick={() => edit({ path: null })}>
              Paste text instead
            </Button>
          </div>
        ) : (
          <textarea
            autoFocus
            spellCheck={false}
            aria-label="PEM text"
            className={cx(textareaClass, "h-[150px] resize-y")}
            placeholder={"-----BEGIN CERTIFICATE-----\nMIID…\n-----END CERTIFICATE-----\n\nA certificate, a whole chain, a certificate request (CSR) or keys"}
            value={s.text}
            onChange={(e) => edit({ text: e.target.value })}
          />
        )}
        <p className="text-[11.5px] text-faint">Private keys are only read to find the certificate they belong to: they're never shown or kept.</p>
      </div>

      {!run ? (
        <EmptyState icon={<FileBadge size={28} />} title="Decode a certificate">
          See its names, issuer, validity, key and fingerprints, check that a chain is in order, and which certificate a private key belongs to. PEM, DER and base64
          work; PKCS#12 (.p12, .pfx) files don't.
        </EmptyState>
      ) : live.error ? (
        <div className="pt-3">
          <ErrorNote>{live.error}</ErrorNote>
        </div>
      ) : report ? (
        <Report report={report} stale={live.stale} />
      ) : null}
    </div>
  );
}

function Report({ report, stale }: { report: CertDecodeReport; stale: boolean }) {
  const { items } = report;
  const counts = items.length === 1 ? "1 item" : `${items.length} items`;
  return (
    <div className={cx("transition-opacity", stale && "opacity-50")} data-testid="cert-report">
      {report.warnings.length > 0 && (
        <Section title="Findings">
          <div className="flex flex-col gap-1.5 px-4">
            {report.warnings.map((w, i) => (
              <WarningRow key={i} warning={w} />
            ))}
          </div>
        </Section>
      )}
      <Section title={counts}>
        <div className="flex flex-col gap-2 px-4">
          {items.map((item, i) => (
            <ItemCard key={i} items={items} index={i} item={item} />
          ))}
        </div>
      </Section>
    </div>
  );
}

function ItemCard({ items, index, item }: { items: DecodedItem[]; index: number; item: DecodedItem }) {
  const caption = `${index + 1} · ${itemRole(items, index)}`;
  const cert = item.certificate;
  if (cert) {
    return (
      <CertificateCard cert={cert.details} index={index} caption={caption}>
        {cert.keyUsage.length > 0 && <KV label="Key usage" value={cert.keyUsage.join(", ")} mono={false} />}
        {cert.extendedKeyUsage.length > 0 && <KV label="Extended key usage" value={cert.extendedKeyUsage.join(", ")} mono={false} />}
        <KV label="Version" value={`X.509 v${cert.version}`} mono={false} />
        <div className="flex px-3 pt-1">
          <Button
            size="sm"
            variant="ghost"
            icon={<Copy size={12} />}
            onClick={() => {
              void copyText(cert.pem);
              toast("success", "Copied to clipboard");
            }}
          >
            Copy PEM
          </Button>
        </div>
      </CertificateCard>
    );
  }
  const request = item.request;
  const key = item.key;
  return (
    <div className="rounded-lg border border-line py-2" data-testid="cert-item" data-kind={item.kind}>
      <div className="flex flex-wrap items-center gap-2 px-4 pb-1">
        <span className="text-[11px] font-semibold uppercase tracking-wide text-faint">{caption}</span>
        {key && <KeyRound size={13} className="text-muted" />}
        <span className="selectable truncate text-[13px] font-semibold text-fg">{itemTitle(item)}</span>
        {key?.private && !key.encrypted && <Badge className="bg-hover text-muted">not shown</Badge>}
      </div>
      {item.error && <p className="selectable px-4 py-1 text-[12.5px] text-danger">{item.error}</p>}
      {item.note && <p className="px-4 py-1 text-[12.5px] text-muted">{item.note}</p>}
      {request && (
        <>
          <KV label="Subject" value={request.subject || "–"} />
          {request.subjectAltNames.length > 0 && <KV label={`Names (${request.subjectAltNames.length})`} value={request.subjectAltNames.join(", ")} />}
          <KV label="Public key" value={keyDescription(request.keyType, request.keyBits, request.keyCurve) || "–"} mono={false} />
          <KV label="Signature" value={request.signatureAlgorithm} mono={false} />
        </>
      )}
      {key && !key.encrypted && key.keyType && (
        <KV
          label="Belongs to"
          mono={false}
          value={
            key.matches.length > 0 ? (
              <span className="text-success">{keyOwners(items, key.matches)}</span>
            ) : items.some((i) => i.kind === "certificate" || i.kind === "request") ? (
              <span className="text-danger">None of the certificates here</span>
            ) : (
              <span className="text-faint">Paste its certificate too to check</span>
            )
          }
        />
      )}
    </div>
  );
}
