// Auth configuration form, shared by requests, folders and the workspace.
import { useEffect, useState } from "react";
import { KeyRound, LogIn, RefreshCw, Trash2 } from "lucide-react";
import type { ApiKeyLocation } from "../../bindings/ApiKeyLocation";
import type { AsapConfig } from "../../bindings/AsapConfig";
import type { Auth } from "../../bindings/Auth";
import type { AwsSigV4Config } from "../../bindings/AwsSigV4Config";
import type { EdgeGridConfig } from "../../bindings/EdgeGridConfig";
import type { HawkConfig } from "../../bindings/HawkConfig";
import type { JwtAlgorithm } from "../../bindings/JwtAlgorithm";
import type { JwtConfig } from "../../bindings/JwtConfig";
import type { OAuth1Config } from "../../bindings/OAuth1Config";
import type { OAuth1Method } from "../../bindings/OAuth1Method";
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
  oauth1: "OAuth 1.0",
  jwt: "JWT (signed by Zorvik)",
  digest: "Digest auth",
  ntlm: "NTLM (Windows)",
  awsSigV4: "AWS Signature v4",
  hawk: "Hawk",
  akamaiEdgeGrid: "Akamai EdgeGrid",
  asap: "Atlassian ASAP",
};

const JWT_ALGORITHMS: JwtAlgorithm[] = ["HS256", "HS384", "HS512", "RS256", "RS384", "RS512", "PS256", "PS384", "PS512", "ES256", "ES384"];
const OAUTH1_METHODS: OAuth1Method[] = ["HMAC-SHA1", "HMAC-SHA256", "HMAC-SHA512", "RSA-SHA1", "RSA-SHA256", "RSA-SHA512", "PLAINTEXT"];
const PEM_PLACEHOLDER = "-----BEGIN PRIVATE KEY-----\n…\n-----END PRIVATE KEY-----";
const usesSecret = (a: JwtAlgorithm) => a.startsWith("HS");

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
    case "digest":
      return { type, username: "", password: "" };
    case "ntlm":
      return { type, username: "", password: "" };
    case "awsSigV4":
      return { type, accessKey: "", secretKey: "", region: "us-east-1", service: "execute-api" };
    case "oauth1":
      return { type, consumerKey: "" };
    case "jwt":
      return { type, algorithm: "HS256", secret: "", payload: '{\n  "sub": "1234567890"\n}', prefix: "Bearer", queryParam: "token" };
    case "hawk":
      return { type, id: "", key: "" };
    case "akamaiEdgeGrid":
      return { type, clientToken: "", clientSecret: "", accessToken: "", maxBody: 131072 };
    case "asap":
      return { type, issuer: "", audience: "", keyId: "", privateKey: "", algorithm: "RS256", expiresIn: 3600 };
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

type FieldProps = { label: string; value: string | undefined; onChange: (v: string) => void; placeholder?: string; hint?: string };

/** A one-line field that takes {{variables}}. */
function Field({ label, value, onChange, placeholder, hint, secret }: FieldProps & { secret?: boolean }) {
  return (
    <Row label={label} hint={hint}>
      <Boxed>
        <VarInput value={value ?? ""} onChange={onChange} placeholder={placeholder} secret={secret} />
      </Boxed>
    </Row>
  );
}

/** Several lines: PEM keys and JSON (variables allowed, substituted when sending). */
function TextBlock({ label, value, onChange, placeholder, hint, rows = 4 }: FieldProps & { rows?: number }) {
  return (
    <Row label={label} hint={hint}>
      <textarea
        value={value ?? ""}
        onChange={(e) => onChange(e.target.value)}
        placeholder={placeholder}
        rows={rows}
        spellCheck={false}
        aria-label={label}
        className="w-full resize-y rounded-md border border-line bg-input px-2.5 py-1.5 font-mono text-[12px] text-fg outline-none placeholder:text-faint focus:border-accent"
      />
    </Row>
  );
}

function NumberField({ label, value, onChange, min, max, hint }: { label: string; value: number; onChange: (v: number) => void; min: number; max?: number; hint?: string }) {
  return (
    <Row label={label} hint={hint}>
      <input
        type="number"
        min={min}
        max={max}
        value={value}
        onChange={(e) => onChange(Math.min(max ?? Infinity, Math.max(min, Number(e.target.value) || min)))}
        aria-label={label}
        className="w-40 rounded-md border border-line bg-input px-2.5 py-1 font-mono text-[12.5px] text-fg outline-none focus:border-accent"
      />
    </Row>
  );
}

function Placement({ value, onChange, query }: { value: ApiKeyLocation | undefined; onChange: (v: ApiKeyLocation) => void; query: string }) {
  return (
    <Row label="Add to">
      <Select value={value ?? "header"} onChange={(e) => onChange(e.target.value as ApiKeyLocation)} className="w-60">
        <option value="header">Authorization header</option>
        <option value="query">{query}</option>
      </Select>
    </Row>
  );
}

function Note({ children }: { children: React.ReactNode }) {
  return <p className="text-[12.5px] text-muted">{children}</p>;
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
      {auth.type === "digest" && (
        <>
          <Note>Zorvik sends the request, answers the server's Digest challenge (MD5 or SHA-256, qop auth or auth-int) and sends it again.</Note>
          <Field label="Username" value={auth.username} onChange={(username) => onChange({ ...auth, username })} placeholder="username" />
          <Field label="Password" value={auth.password} onChange={(password) => onChange({ ...auth, password })} placeholder="password" secret />
        </>
      )}
      {auth.type === "ntlm" && (
        <>
          <Note>Windows authentication (NTLMv2), for IIS and other Windows servers. The handshake runs over HTTP/1.1 on one connection.</Note>
          <Field label="Username" value={auth.username} onChange={(username) => onChange({ ...auth, username })} placeholder="user, DOMAIN\user or user@domain" />
          <Field label="Password" value={auth.password} onChange={(password) => onChange({ ...auth, password })} placeholder="password" secret />
          <Field label="Domain" value={auth.domain} onChange={(domain) => onChange({ ...auth, domain })} placeholder="optional" />
          <Field label="Workstation" value={auth.workstation} onChange={(workstation) => onChange({ ...auth, workstation })} placeholder="optional" />
        </>
      )}
      {auth.type === "awsSigV4" && <AwsForm auth={auth} onChange={onChange} />}
      {auth.type === "oauth1" && <OAuth1Form auth={auth} onChange={onChange} />}
      {auth.type === "jwt" && <JwtForm auth={auth} onChange={onChange} />}
      {auth.type === "hawk" && <HawkForm auth={auth} onChange={onChange} />}
      {auth.type === "akamaiEdgeGrid" && <EdgeGridForm auth={auth} onChange={onChange} />}
      {auth.type === "asap" && <AsapForm auth={auth} onChange={onChange} />}
    </div>
  );
}

function AwsForm({ auth, onChange }: { auth: Extract<Auth, { type: "awsSigV4" }>; onChange: (a: Auth) => void }) {
  const set = (patch: Partial<AwsSigV4Config>) => onChange({ ...auth, ...patch });
  return (
    <>
      <Note>Signs each request with AWS Signature Version 4: API Gateway, S3, Lambda function URLs and other AWS services.</Note>
      <Field label="Access key" value={auth.accessKey} onChange={(accessKey) => set({ accessKey })} placeholder="{{awsAccessKeyId}}" />
      <Field label="Secret key" value={auth.secretKey} onChange={(secretKey) => set({ secretKey })} placeholder="{{awsSecretAccessKey}}" secret />
      <Field label="Session token" value={auth.sessionToken} onChange={(sessionToken) => set({ sessionToken })} placeholder="optional, for temporary credentials" secret />
      <Field label="Region" value={auth.region} onChange={(region) => set({ region })} placeholder="us-east-1" />
      <Field label="Service" value={auth.service} onChange={(service) => set({ service })} placeholder="execute-api, s3, lambda…" />
      <Placement value={auth.location} onChange={(location) => set({ location })} query="Query string (presigned URL)" />
    </>
  );
}

function OAuth1Form({ auth, onChange }: { auth: Extract<Auth, { type: "oauth1" }>; onChange: (a: Auth) => void }) {
  const set = (patch: Partial<OAuth1Config>) => onChange({ ...auth, ...patch });
  const method = auth.signatureMethod ?? "HMAC-SHA1";
  const rsa = method.startsWith("RSA");
  return (
    <>
      <Row label="Signature method">
        <Select value={method} onChange={(e) => set({ signatureMethod: e.target.value as OAuth1Method })} className="w-60">
          {OAUTH1_METHODS.map((m) => (
            <option key={m} value={m}>
              {m}
            </option>
          ))}
        </Select>
      </Row>
      <Field label="Consumer key" value={auth.consumerKey} onChange={(consumerKey) => set({ consumerKey })} placeholder="{{consumerKey}}" />
      {rsa ? (
        <TextBlock label="Private key" value={auth.privateKey} onChange={(privateKey) => set({ privateKey })} placeholder={PEM_PLACEHOLDER} hint="PEM (PKCS#1 or PKCS#8). Keep it in a secret variable." />
      ) : (
        <Field label="Consumer secret" value={auth.consumerSecret} onChange={(consumerSecret) => set({ consumerSecret })} placeholder="{{consumerSecret}}" secret />
      )}
      <Field label="Token" value={auth.token} onChange={(token) => set({ token })} placeholder="access token (optional)" />
      <Field label="Token secret" value={auth.tokenSecret} onChange={(tokenSecret) => set({ tokenSecret })} placeholder="optional" secret />
      <Field label="Callback URL" value={auth.callback} onChange={(callback) => set({ callback })} placeholder="optional" />
      <Field label="Verifier" value={auth.verifier} onChange={(verifier) => set({ verifier })} placeholder="optional" />
      <Field label="Realm" value={auth.realm} onChange={(realm) => set({ realm })} placeholder="optional" />
      <Row label="Options">
        <div className="flex flex-col gap-2">
          <Switch checked={auth.includeVersion ?? true} onChange={(includeVersion) => set({ includeVersion })} label="Send oauth_version=1.0" />
          <Switch checked={auth.includeBodyHash ?? false} onChange={(includeBodyHash) => set({ includeBodyHash })} label="Sign the body (oauth_body_hash)" />
        </div>
      </Row>
      <Placement value={auth.location} onChange={(location) => set({ location })} query="Query string" />
    </>
  );
}

function JwtForm({ auth, onChange }: { auth: Extract<Auth, { type: "jwt" }>; onChange: (a: Auth) => void }) {
  const set = (patch: Partial<JwtConfig>) => onChange({ ...auth, ...patch });
  const secret = usesSecret(auth.algorithm);
  return (
    <>
      <Note>Zorvik builds and signs a fresh token for every request from the claims below.</Note>
      <Row label="Algorithm">
        <Select value={auth.algorithm} onChange={(e) => set({ algorithm: e.target.value as JwtAlgorithm })} className="w-60">
          {JWT_ALGORITHMS.map((a) => (
            <option key={a} value={a}>
              {a}
            </option>
          ))}
        </Select>
      </Row>
      {secret ? (
        <>
          <Field label="Secret" value={auth.secret} onChange={(v) => set({ secret: v })} placeholder="{{jwtSecret}}" secret />
          <Row label="">
            <Switch checked={auth.secretBase64 ?? false} onChange={(secretBase64) => set({ secretBase64 })} label="The secret is base64" />
          </Row>
        </>
      ) : (
        <TextBlock label="Private key" value={auth.secret} onChange={(v) => set({ secret: v })} placeholder={PEM_PLACEHOLDER} hint="PEM private key (RSA or EC). Keep it in a secret variable." />
      )}
      <TextBlock
        label="Payload"
        value={auth.payload}
        onChange={(payload) => set({ payload })}
        rows={5}
        placeholder='{ "sub": "{{userId}}", "iat": {{$timestamp}}, "exp": {{$timestamp(+1h)}} }'
        hint="JSON claims. Variables work, e.g. {{$timestamp}} for iat and {{$timestamp(+1h)}} for exp."
      />
      <TextBlock label="Header" value={auth.header} onChange={(header) => set({ header })} rows={2} placeholder='{ "kid": "key-1" }' hint="Optional extra header fields (alg and typ are set)." />
      <Placement value={auth.location} onChange={(location) => set({ location })} query="Query parameter" />
      {auth.location === "query" ? (
        <Field label="Parameter" value={auth.queryParam} onChange={(queryParam) => set({ queryParam })} placeholder="token" />
      ) : (
        <Field label="Prefix" value={auth.prefix} onChange={(prefix) => set({ prefix })} placeholder="Bearer" hint="Sent as “Authorization: <prefix> <token>”. Leave empty to send the bare token." />
      )}
    </>
  );
}

function HawkForm({ auth, onChange }: { auth: Extract<Auth, { type: "hawk" }>; onChange: (a: Auth) => void }) {
  const set = (patch: Partial<HawkConfig>) => onChange({ ...auth, ...patch });
  return (
    <>
      <Field label="Hawk ID" value={auth.id} onChange={(id) => set({ id })} placeholder="{{hawkId}}" />
      <Field label="Hawk key" value={auth.key} onChange={(key) => set({ key })} placeholder="{{hawkKey}}" secret />
      <Row label="Algorithm">
        <Select value={auth.algorithm ?? "sha256"} onChange={(e) => set({ algorithm: e.target.value as HawkConfig["algorithm"] })} className="w-60">
          <option value="sha256">SHA-256</option>
          <option value="sha1">SHA-1</option>
        </Select>
      </Row>
      <Field label="Extra data (ext)" value={auth.ext} onChange={(ext) => set({ ext })} placeholder="optional" />
      <Field label="App ID" value={auth.app} onChange={(app) => set({ app })} placeholder="optional" />
      <Field label="Delegation (dlg)" value={auth.dlg} onChange={(dlg) => set({ dlg })} placeholder="optional" />
      <Row label="Options">
        <Switch checked={auth.includePayloadHash ?? false} onChange={(includePayloadHash) => set({ includePayloadHash })} label="Sign the body (payload hash)" />
      </Row>
    </>
  );
}

function EdgeGridForm({ auth, onChange }: { auth: Extract<Auth, { type: "akamaiEdgeGrid" }>; onChange: (a: Auth) => void }) {
  const set = (patch: Partial<EdgeGridConfig>) => onChange({ ...auth, ...patch });
  return (
    <>
      <Note>For Akamai APIs. Use the host from your .edgerc (akab-….luna.akamaiapis.net) in the URL.</Note>
      <Field label="Client token" value={auth.clientToken} onChange={(clientToken) => set({ clientToken })} placeholder="{{client_token}}" />
      <Field label="Client secret" value={auth.clientSecret} onChange={(clientSecret) => set({ clientSecret })} placeholder="{{client_secret}}" secret />
      <Field label="Access token" value={auth.accessToken} onChange={(accessToken) => set({ accessToken })} placeholder="{{access_token}}" />
      <Field label="Headers to sign" value={auth.headersToSign} onChange={(headersToSign) => set({ headersToSign })} placeholder="optional, comma-separated" />
      <NumberField label="Max body" value={auth.maxBody} min={0} onChange={(maxBody) => set({ maxBody })} hint="Bytes of a POST body included in the signature (Akamai's default: 131072)." />
    </>
  );
}

function AsapForm({ auth, onChange }: { auth: Extract<Auth, { type: "asap" }>; onChange: (a: Auth) => void }) {
  const set = (patch: Partial<AsapConfig>) => onChange({ ...auth, ...patch });
  return (
    <>
      <Note>Atlassian service-to-service auth: a short-lived JWT signed with your service's private key, sent as a Bearer token.</Note>
      <Field label="Issuer" value={auth.issuer} onChange={(issuer) => set({ issuer })} placeholder="my-service" />
      <Field label="Audience" value={auth.audience} onChange={(audience) => set({ audience })} placeholder="target-service (comma-separated for several)" />
      <Field label="Key ID" value={auth.keyId} onChange={(keyId) => set({ keyId })} placeholder="my-service/key-1" />
      <TextBlock label="Private key" value={auth.privateKey} onChange={(privateKey) => set({ privateKey })} placeholder={PEM_PLACEHOLDER} hint="PEM. Keep it in a secret variable." />
      <Row label="Algorithm">
        <Select value={auth.algorithm} onChange={(e) => set({ algorithm: e.target.value as JwtAlgorithm })} className="w-60">
          {JWT_ALGORITHMS.filter((a) => !usesSecret(a)).map((a) => (
            <option key={a} value={a}>
              {a}
            </option>
          ))}
        </Select>
      </Row>
      <Field label="Subject" value={auth.subject} onChange={(subject) => set({ subject })} placeholder="optional" />
      <NumberField label="Expires in" value={auth.expiresIn} min={1} max={3600} onChange={(expiresIn) => set({ expiresIn })} hint="Seconds each token is valid (at most 3600)." />
      <TextBlock label="Extra claims" value={auth.claims} onChange={(claims) => set({ claims })} rows={2} placeholder='{ "scope": "read" }' hint="Optional JSON." />
    </>
  );
}

function OAuthForm({ auth, onChange, path }: { auth: Extract<Auth, { type: "oauth2" }>; onChange: (a: Auth) => void; path: string | null }) {
  const set = (patch: Partial<OAuth2Config>) => onChange({ ...auth, ...patch });
  const [status, setStatus] = useState<TokenStatus | null>(null);
  const [busy, setBusy] = useState(false);
  // Grants that sign in through the browser.
  const code = auth.grantType === "authorizationCode" || auth.grantType === "implicit";
  const implicit = auth.grantType === "implicit";

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
          <option value="implicit">Implicit (legacy)</option>
        </Select>
      </Row>
      {code && field("Authorization URL", "authUrl", "https://id.example.com/oauth/authorize")}
      {!implicit && field("Token URL", "tokenUrl", "https://id.example.com/oauth/token")}
      {field("Client ID", "clientId", "{{clientId}}")}
      {!implicit && field("Client secret", "clientSecret", "{{clientSecret}}", { secret: true })}
      {auth.grantType === "password" && field("Username", "username", "user@example.com")}
      {auth.grantType === "password" && field("Password", "password", "password", { secret: true })}
      {code &&
        field("Redirect URI", "redirectUri", "http://127.0.0.1:53682/callback", {
          hint: "Register this loopback URL with your identity provider. Zorvik listens on it while you sign in.",
        })}
      {field("Scope", "scope", "read write")}
      {field("Audience", "audience", "optional")}
      {!implicit && (
        <Row label="Client auth">
          <Select value={auth.clientAuth} onChange={(e) => set({ clientAuth: e.target.value as OAuth2Config["clientAuth"] })} className="w-60">
            <option value="basicHeader">Basic auth header</option>
            <option value="body">In request body</option>
          </Select>
        </Row>
      )}
      {code && !implicit && (
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
