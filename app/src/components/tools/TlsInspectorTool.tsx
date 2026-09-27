// TLS inspector: trust verdict, negotiated parameters, the certificate chain,
// and which protocol versions and cipher suites the server accepts.
import { CircleAlert, Info, Lock, ShieldAlert, ShieldCheck, Square, TriangleAlert, X } from "lucide-react";
import type { TlsCertificate } from "../../bindings/TlsCertificate";
import type { TlsReport } from "../../bindings/TlsReport";
import type { TlsWarning } from "../../bindings/TlsWarning";
import { formatMs } from "../../lib/format";
import { type ToolTab, updateToolState } from "../../store/tabs";
import { inspectTls, stopRun, TLS_DEFAULTS, toolState } from "../../store/tools";
import { Badge, Button, cx, EmptyState, Input, Spinner, Tooltip } from "../ui";
import { ErrorNote, KV, LabeledField, Section, ToolBar, YesNo } from "./parts";

export function TlsInspectorTool({ tab }: { tab: ToolTab }) {
  const s = toolState(tab, TLS_DEFAULTS);
  const running = s.status === "running";
  const set = (patch: Partial<typeof s>) => updateToolState(tab.id, (st) => ({ ...st, ...patch }));

  return (
    <div className="pb-6" data-testid="tls-inspector">
      <ToolBar onSubmit={() => void inspectTls(tab.id)}>
        <LabeledField label="Server" className="min-w-[220px] flex-[3]">
          <Input
            autoFocus
            className="font-mono"
            value={s.host}
            placeholder="example.com, example.com:8443 or https://…"
            aria-label="Server"
            onChange={(e) => set({ host: e.target.value })}
          />
        </LabeledField>
        <LabeledField label="Server name (SNI)" className="min-w-[150px] flex-1">
          <Input className="font-mono" value={s.sni} placeholder="same as server" aria-label="Server name (SNI)" onChange={(e) => set({ sni: e.target.value })} />
        </LabeledField>
        {running ? (
          <Button icon={<Square size={12} />} onClick={() => void stopRun(tab.id)}>
            Cancel
          </Button>
        ) : (
          <Button variant="primary" icon={<Lock size={13} />} onClick={() => void inspectTls(tab.id)} className="w-[104px]" data-testid="tls-inspect">
            Inspect
          </Button>
        )}
      </ToolBar>

      {running && (
        <div className="flex items-center gap-2 px-4 pb-3 text-[12.5px] text-muted">
          <Spinner />
          Connecting, then trying each protocol version and cipher suite…
        </div>
      )}
      {s.status === "error" && s.error && <ErrorNote>{s.error}</ErrorNote>}
      {s.report ? (
        <Report report={s.report} stale={running} />
      ) : (
        !running &&
        s.status !== "error" && (
          <EmptyState icon={<Lock size={28} />} title="Inspect a server's TLS">
            See whether its certificate is trusted, when it expires, the full chain, and which TLS versions and cipher suites it accepts. Uses the proxy and
            extra CA from Settings.
          </EmptyState>
        )
      )}
    </div>
  );
}

function Report({ report, stale }: { report: TlsReport; stale: boolean }) {
  const leaf = report.chain[0];
  return (
    <div className={cx("transition-opacity", stale && "opacity-50")}>
      <Summary report={report} leaf={leaf} />
      {report.warnings.length > 0 && (
        <Section title="Findings">
          <div className="flex flex-col gap-1.5 px-4">
            {report.warnings.map((w, i) => (
              <WarningRow key={i} warning={w} />
            ))}
          </div>
        </Section>
      )}
      <Section title={`Certificate chain · ${report.chain.length} sent by the server`}>
        <div className="flex flex-col gap-2 px-4">
          {report.chain.map((c, i) => (
            <CertificateCard key={i} cert={c} index={i} />
          ))}
        </div>
      </Section>
      <div className="grid gap-x-4 xl:grid-cols-[minmax(0,1fr)_minmax(0,1.6fr)]">
        <Section title="Protocol versions">
          <table className="mx-4 w-[calc(100%-2rem)] text-[12.5px]">
            <tbody>
              {report.versions.map((v) => (
                <tr key={v.version} className="border-b border-line/50 last:border-0">
                  <td className="py-1.5 pr-3 font-mono text-[12px] text-fg">{v.version}</td>
                  <td className="py-1.5">
                    <Tooltip content={v.detail}>
                      <span>
                        <YesNo value={v.supported} yes="Supported" no="Not supported" unknown={v.version === "TLS 1.0" || v.version === "TLS 1.1" ? "Can't be tested" : "Unknown"} />
                      </span>
                    </Tooltip>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          <p className="px-4 pt-2 text-[11.5px] text-faint">TLS 1.0 and 1.1 can't be tested: the TLS library used here doesn't implement them.</p>
        </Section>
        <Section title="Cipher suites">
          {report.ciphers.length === 0 ? (
            <p className="px-4 text-[12px] text-faint">Not tested.</p>
          ) : (
            <table className="mx-4 w-[calc(100%-2rem)] text-[12.5px]">
              <thead>
                <tr className="text-left text-[11px] text-faint">
                  <th className="pb-1 font-medium">Suite</th>
                  <th className="pb-1 font-medium">Version</th>
                  <th className="pb-1 font-medium">Accepted</th>
                </tr>
              </thead>
              <tbody>
                {report.ciphers.map((c) => (
                  <tr key={c.name} className={cx("border-b border-line/50 last:border-0", c.accepted === false && "text-muted")}>
                    <td className={cx("selectable py-1.5 pr-3 font-mono text-[11.5px]", c.accepted ? "text-fg" : "text-muted")}>{c.name}</td>
                    <td className="whitespace-nowrap py-1.5 pr-3 text-[12px]">{c.version}</td>
                    <td className="py-1.5">
                      <Tooltip content={c.detail}>
                        <span>
                          <YesNo value={c.accepted} yes="Yes" no="No" unknown="No answer" />
                        </span>
                      </Tooltip>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
          <p className="px-4 pt-2 text-[11.5px] text-faint">Suites this app can offer (all with forward secrecy). Servers may accept others too.</p>
        </Section>
      </div>
    </div>
  );
}

function Summary({ report, leaf }: { report: TlsReport; leaf: TlsCertificate | undefined }) {
  return (
    <div className="mx-4 mb-1 rounded-xl border border-line bg-panel-2/50 p-3" data-testid="tls-summary">
      <div className="flex flex-wrap items-center gap-2 pb-2">
        {report.trusted ? (
          <span className="inline-flex items-center gap-1.5 rounded-lg bg-success/12 px-2 py-1 text-[12.5px] font-semibold text-success">
            <ShieldCheck size={15} />
            Trusted
          </span>
        ) : (
          <span className="inline-flex items-center gap-1.5 rounded-lg bg-danger/12 px-2 py-1 text-[12.5px] font-semibold text-danger">
            <ShieldAlert size={15} />
            Not trusted
          </span>
        )}
        <Badge className="bg-hover text-fg">{report.version}</Badge>
        {report.alpn && <Badge className="bg-hover text-muted">ALPN {report.alpn}</Badge>}
        {leaf && <DaysLeft cert={leaf} />}
        <span className="flex-1" />
        <span className="text-[11.5px] text-faint">
          Handshake {formatMs(report.handshakeMs)} · all checks {formatMs(report.durationMs)}
        </span>
      </div>
      {!report.trusted && report.trustError && <p className="selectable pb-2 text-[12.5px] text-danger">{report.trustError}</p>}
      <div className="-mx-4">
        <KV label="Server name" value={report.serverName} />
        <KV label="Connected to" value={report.proxy ? `${report.remoteAddr} (proxy ${report.proxy})` : report.remoteAddr} />
        <KV label="Cipher" value={report.cipher} />
        <KV label="Key exchange" value={report.keyExchange ?? "–"} />
        <KV label="Name matches" value={<YesNo value={report.hostnameMatches} no="No" />} mono={false} />
        <KV label="OCSP stapling" value={<YesNo value={report.ocspStapled} no="No" />} mono={false} />
      </div>
    </div>
  );
}

function DaysLeft({ cert }: { cert: TlsCertificate }) {
  if (cert.parseError) return null;
  const expired = cert.daysLeft < 0;
  const tone = expired || cert.notYetValid ? "bg-danger/12 text-danger" : cert.daysLeft < 30 ? "bg-warning/14 text-warning" : "bg-hover text-muted";
  const text = cert.notYetValid
    ? `Valid from ${formatDate(cert.notBefore)}`
    : expired
      ? `Expired ${formatDate(cert.notAfter)}`
      : `Expires in ${cert.daysLeft} day${cert.daysLeft === 1 ? "" : "s"}`;
  return (
    <Tooltip content={`Valid ${formatDate(cert.notBefore)} → ${formatDate(cert.notAfter)}`}>
      <span>
        <Badge className={tone}>{text}</Badge>
      </span>
    </Tooltip>
  );
}

function WarningRow({ warning }: { warning: TlsWarning }) {
  const style =
    warning.level === "danger"
      ? { cls: "border-danger/30 bg-danger/8 text-danger", icon: <CircleAlert size={14} /> }
      : warning.level === "warning"
        ? { cls: "border-warning/30 bg-warning/8 text-warning", icon: <TriangleAlert size={14} /> }
        : { cls: "border-info/30 bg-info/8 text-info", icon: <Info size={14} /> };
  return (
    <div className={cx("flex items-start gap-2 rounded-lg border px-3 py-1.5 text-[12.5px]", style.cls)} data-warning={warning.code}>
      <span className="mt-0.5 shrink-0">{style.icon}</span>
      <span className="selectable min-w-0">{warning.message}</span>
    </div>
  );
}

function role(cert: TlsCertificate, index: number): string {
  if (index === 0) return "Server certificate";
  return cert.selfSigned && cert.isCa ? "Root" : "Intermediate";
}

function CertificateCard({ cert, index }: { cert: TlsCertificate; index: number }) {
  if (cert.parseError) {
    return (
      <div className="rounded-lg border border-line p-3 text-[12.5px]">
        <div className="flex items-center gap-2 text-danger">
          <X size={14} />
          {cert.parseError}
        </div>
        <div className="-mx-4 pt-1">
          <KV label="SHA-256" value={cert.sha256} copy={cert.sha256} />
        </div>
      </div>
    );
  }
  const key = [cert.keyType, cert.keyCurve, cert.keyBits ? `${cert.keyBits} bits` : null].filter(Boolean).join(" · ");
  const names = cert.subjectAltNames;
  return (
    <div className="rounded-lg border border-line py-2" data-testid="tls-certificate">
      <div className="flex flex-wrap items-center gap-2 px-4 pb-1">
        <span className="text-[11px] font-semibold uppercase tracking-wide text-faint">{role(cert, index)}</span>
        <span className="selectable truncate text-[13px] font-semibold text-fg">{cert.commonName ?? cert.subject}</span>
        {cert.selfSigned && <Badge className="bg-hover text-muted">self-signed</Badge>}
        {cert.isCa && <Badge className="bg-hover text-muted">CA</Badge>}
        <span className="flex-1" />
        <DaysLeft cert={cert} />
      </div>
      <KV label="Subject" value={cert.subject} />
      <KV label="Issuer" value={cert.issuer} />
      <KV label="Valid" value={`${formatDate(cert.notBefore)} → ${formatDate(cert.notAfter)}`} mono={false} />
      {names.length > 0 && (
        <KV
          label={`Names (${names.length})`}
          mono={false}
          value={
            <span className="flex flex-wrap gap-1">
              {names.slice(0, 50).map((n) => (
                <span key={n} className="rounded bg-hover px-1.5 font-mono text-[11.5px]">
                  {n}
                </span>
              ))}
              {names.length > 50 && <span className="text-faint">+{names.length - 50} more</span>}
            </span>
          }
        />
      )}
      <KV label="Public key" value={key || "–"} mono={false} />
      <KV label="Signature" value={cert.signatureAlgorithm} mono={false} />
      <KV label="SHA-256" value={cert.sha256} copy={cert.sha256} />
      <KV label="Serial" value={cert.serial} copy={cert.serial} />
    </div>
  );
}

function formatDate(rfc3339: string): string {
  const d = new Date(rfc3339);
  return Number.isNaN(d.getTime()) ? rfc3339 : d.toLocaleDateString(undefined, { year: "numeric", month: "short", day: "numeric" });
}
