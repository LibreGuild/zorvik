// Auth configuration form, shared by requests, folders and the workspace.
import { useEffect, useState } from "react";
import { KeyRound, LogIn, RefreshCw, Trash2 } from "lucide-react";
import type { Auth } from "../../bindings/Auth";
import type { OAuth2Config } from "../../bindings/OAuth2Config";
import type { TokenStatus } from "../../bindings/TokenStatus";
import { formatRelative } from "../../lib/format";
import { api, errorMessage } from "../../lib/rpc";
import { toast } from "../../store/toasts";
import { VarInput } from "../VarInput";
import { Button, Select, Switch } from "../ui";

type AuthType = Auth["type"];

const LABELS: Record<AuthType, string> = {
  inherit: "Inherit from parent",
  none: "No auth",
  basic: "Basic auth",
  bearer: "Bearer token",
  apiKey: "API key",
  oauth2: "OAuth 2.0",
};

const DEFAULT_OAUTH: OAuth2Config = {
  grantType: "clientCredentials",
  tokenUrl: "",
  redirectUri: "http://127.0.0.1:53682/callback",
  clientId: "",
  clientAuth: "basicHeader",
  pkce: true,
  headerPrefix: "Bearer",
};

function defaultAuth(type: AuthType): Auth {
  switch (type) {
    case "basic":
      return { type, username: "", password: "" };
    case "bearer":
      return { type, token: "", prefix: "Bearer" };
    case "apiKey":
      return { type, key: "X-API-Key", value: "", location: "header" };
    case "oauth2":
      return { type, ...DEFAULT_OAUTH };
    default:
      return { type } as Auth;
  }
}

function Row({ label, children, hint }: { label: string; children: React.ReactNode; hint?: string }) {
  return (
    <div className="grid grid-cols-[150px_1fr] items-start gap-3">
      <div className="pt-1.5 text-[12px] font-medium text-muted">{label}</div>
      <div className="min-w-0">
        {children}
        {hint && <div className="mt-1 text-[11.5px] text-faint">{hint}</div>}
      </div>
    </div>
  );
}

function Boxed({ children }: { children: React.ReactNode }) {
  return <div className="rounded-md border border-line bg-input focus-within:border-accent">{children}</div>;
}

export function AuthEditor({
  auth,
  onChange,
  allowInherit = true,
  path,
}: {
  auth: Auth;
  onChange: (a: Auth) => void;
  allowInherit?: boolean;
  /** Request/folder path, used to resolve OAuth tokens. */
  path?: string | null;
}) {
  const types = (Object.keys(LABELS) as AuthType[]).filter((t) => allowInherit || t !== "inherit");
  return (
    <div className="flex max-w-3xl flex-col gap-4 p-4">
      <Row label="Type">
        <Select value={auth.type} onChange={(e) => onChange(defaultAuth(e.target.value as AuthType))} className="w-60">
          {types.map((t) => (
            <option key={t} value={t}>
              {LABELS[t]}
            </option>
          ))}
        </Select>
      </Row>
      {auth.type === "inherit" && (
        <p className="text-[12.5px] text-muted">Uses the auth configured on the parent folder, or on the workspace.</p>
      )}
      {auth.type === "none" && <p className="text-[12.5px] text-muted">No Authorization header is added.</p>}
      {auth.type === "basic" && (
        <>
          <Row label="Username">
            <Boxed>
              <VarInput value={auth.username} onChange={(username) => onChange({ ...auth, username })} placeholder="username" />
            </Boxed>
          </Row>
          <Row label="Password">
            <Boxed>
              <VarInput value={auth.password} onChange={(password) => onChange({ ...auth, password })} placeholder="password" secret />
            </Boxed>
          </Row>
        </>
      )}
      {auth.type === "bearer" && (
        <>
          <Row label="Token">
            <Boxed>
              <VarInput value={auth.token} onChange={(token) => onChange({ ...auth, token })} placeholder="{{token}}" />
            </Boxed>
          </Row>
          <Row label="Prefix" hint="Sent as “Authorization: <prefix> <token>”. Leave empty to send the bare token.">
            <Boxed>
              <VarInput value={auth.prefix} onChange={(prefix) => onChange({ ...auth, prefix })} placeholder="Bearer" />
            </Boxed>
          </Row>
        </>
      )}
      {auth.type === "apiKey" && (
        <>
          <Row label="Key">
            <Boxed>
              <VarInput value={auth.key} onChange={(key) => onChange({ ...auth, key })} placeholder="X-API-Key" />
            </Boxed>
          </Row>
          <Row label="Value">
            <Boxed>
              <VarInput value={auth.value} onChange={(value) => onChange({ ...auth, value })} placeholder="{{apiKey}}" />
            </Boxed>
          </Row>
          <Row label="Add to">
            <Select value={auth.location} onChange={(e) => onChange({ ...auth, location: e.target.value as "header" | "query" })} className="w-60">
              <option value="header">Header</option>
              <option value="query">Query parameter</option>
            </Select>
          </Row>
        </>
      )}
      {auth.type === "oauth2" && <OAuthForm auth={auth} onChange={onChange} path={path ?? null} />}
    </div>
  );
}

function OAuthForm({ auth, onChange, path }: { auth: Extract<Auth, { type: "oauth2" }>; onChange: (a: Auth) => void; path: string | null }) {
  const set = (patch: Partial<OAuth2Config>) => onChange({ ...auth, ...patch });
  const [status, setStatus] = useState<TokenStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const code = auth.grantType === "authorizationCode";

  useEffect(() => {
    let alive = true;
    api
      .oauthStatus(auth, path)
      .then((s) => alive && setStatus(s))
      .catch(() => alive && setStatus(null));
    return () => {
      alive = false;
    };
  }, [auth, path]);

  const getToken = async () => {
    setBusy(true);
    try {
      setStatus(await api.oauthGetToken(auth, path));
      toast("success", "Token received");
    } catch (e) {
      toast("error", "Could not get a token", errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  const field = (label: string, key: keyof OAuth2Config, placeholder: string, opts: { secret?: boolean; hint?: string } = {}) => (
    <Row label={label} hint={opts.hint}>
      <Boxed>
        <VarInput value={(auth[key] as string) ?? ""} onChange={(v) => set({ [key]: v } as Partial<OAuth2Config>)} placeholder={placeholder} secret={opts.secret} />
      </Boxed>
    </Row>
  );

  return (
    <>
      <Row label="Grant type">
        <Select value={auth.grantType} onChange={(e) => set({ grantType: e.target.value as OAuth2Config["grantType"] })} className="w-60">
          <option value="clientCredentials">Client credentials</option>
          <option value="authorizationCode">Authorization code</option>
          <option value="password">Password</option>
        </Select>
      </Row>
      {code && field("Authorization URL", "authUrl", "https://id.example.com/oauth/authorize")}
      {field("Token URL", "tokenUrl", "https://id.example.com/oauth/token")}
      {field("Client ID", "clientId", "{{clientId}}")}
      {field("Client secret", "clientSecret", "{{clientSecret}}", { secret: true })}
      {auth.grantType === "password" && field("Username", "username", "user@example.com")}
      {auth.grantType === "password" && field("Password", "password", "password", { secret: true })}
      {code &&
        field("Redirect URI", "redirectUri", "http://127.0.0.1:53682/callback", {
          hint: "Register this loopback URL with your identity provider. Zorvik listens on it while you sign in.",
        })}
      {field("Scope", "scope", "read write")}
      {field("Audience", "audience", "optional")}
      <Row label="Client auth">
        <Select value={auth.clientAuth} onChange={(e) => set({ clientAuth: e.target.value as OAuth2Config["clientAuth"] })} className="w-60">
          <option value="basicHeader">Basic auth header</option>
          <option value="body">In request body</option>
        </Select>
      </Row>
      {code && (
        <Row label="PKCE">
          <Switch checked={auth.pkce} onChange={(pkce) => set({ pkce })} label="Use PKCE (S256)" />
        </Row>
      )}
      {field("Header prefix", "headerPrefix", "Bearer")}
      <div className="rounded-lg border border-line bg-panel-2 p-3">
        <div className="flex items-center gap-3">
          <KeyRound size={16} className={status?.hasToken ? "text-success" : "text-faint"} />
          <div className="min-w-0 flex-1 text-[12.5px]">
            {status?.hasToken ? (
              <>
                <div className="font-medium text-fg">
                  Token <span className="font-mono text-muted">{status.tokenPreview}</span>
                </div>
                <div className="text-muted">
                  {status.expiresAt ? (status.expiresAt > Date.now() ? `Expires ${new Date(status.expiresAt).toLocaleString()}` : `Expired ${formatRelative(status.expiresAt)}`) : "No expiry"}
                  {status.hasRefreshToken && " · refresh token available"}
                </div>
              </>
            ) : (
              <div className="text-muted">
                {code ? "No token yet. Sign in to get one." : "No cached token. One is requested automatically when you send."}
              </div>
            )}
          </div>
          {status?.hasToken && (
            <Button
              size="sm"
              variant="ghost"
              icon={<Trash2 size={13} />}
              onClick={async () => {
                await api.oauthClear(auth, path).catch(() => {});
                setStatus(await api.oauthStatus(auth, path).catch(() => null));
              }}
            >
              Clear
            </Button>
          )}
          <Button size="sm" variant="primary" loading={busy} icon={code ? <LogIn size={13} /> : <RefreshCw size={13} />} onClick={getToken}>
            {code ? "Get token" : "Fetch token"}
          </Button>
        </div>
      </div>
    </>
  );
}
