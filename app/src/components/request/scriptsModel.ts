// Scripts (pre-request / post-response): what the editor completes after `pm.` and
// how the response pane sums up test results. The API itself is crates/script/src/prelude.js.
import type { ScriptReport } from "../../bindings/ScriptReport";
import type { Scripts } from "../../bindings/Scripts";

export type ScriptEvent = "preRequest" | "postResponse";

export const hasScripts = (s: Scripts | undefined | null): boolean => !!(s?.preRequest?.trim() || s?.postResponse?.trim());

/** A member offered after a dot: `type` picks the completion icon, `detail` is shown next to it. */
export interface Member {
  type: "function" | "property" | "namespace";
  detail?: string;
  info?: string;
  /** Text inserted instead of the name (e.g. with a call template). */
  apply?: string;
  children?: Record<string, Member>;
}

const fn = (detail: string, info?: string, apply?: string): Member => ({ type: "function", detail, info, apply });
const prop = (info?: string, children?: Record<string, Member>): Member => ({ type: children ? "namespace" : "property", info, children });

const scope = (what: string): Record<string, Member> => ({
  get: fn("(name)", `Value of a ${what} variable`),
  set: fn("(name, value)", `Set a ${what} variable (kept on this computer, never in workspace files)`),
  has: fn("(name)"),
  unset: fn("(name)"),
  clear: fn("()"),
  replaceIn: fn("(text)", "Replace {{variables}} in a text"),
  toObject: fn("()"),
});

const propertyList = (readOnly = false): Record<string, Member> => ({
  get: fn("(name)"),
  has: fn("(name, value?)"),
  toObject: fn("()"),
  all: fn("()"),
  each: fn("(fn)"),
  count: fn("()"),
  ...(readOnly
    ? {}
    : {
        add: fn("({ key, value })"),
        upsert: fn("({ key, value })", "Add, or replace the value of an existing one"),
        remove: fn("(name)"),
        clear: fn("()"),
      }),
});

const cookieList: Record<string, Member> = {
  get: fn("(name)", "A cookie's value"),
  has: fn("(name, value?)"),
  one: fn("(name)", "{ name, value, domain, path, expires, secure, httpOnly }"),
  all: fn("()"),
  toObject: fn("()"),
  count: fn("()"),
};

const responseHave = prop(undefined, {
  status: fn("(code | reason)", "pm.response.to.have.status(200)"),
  header: fn("(name, value?)"),
  jsonBody: fn("(path?, value?)"),
  body: fn("(text | RegExp | object)"),
});
const responseBe = prop(undefined, {
  ok: prop("Status 200"),
  success: prop("Status 2XX"),
  info: prop("Status 1XX"),
  redirection: prop("Status 3XX"),
  clientError: prop("Status 4XX"),
  serverError: prop("Status 5XX"),
  error: prop("Status 4XX or 5XX"),
  json: prop("Body is valid JSON"),
  withBody: prop("Body is not empty"),
  notFound: prop("Status 404"),
  unauthorized: prop("Status 401"),
  forbidden: prop("Status 403"),
  badRequest: prop("Status 400"),
});
const responseTo: Record<string, Member> = {
  have: responseHave,
  be: responseBe,
  not: prop("Negate the next assertion", { have: responseHave, be: responseBe }),
};

/** Everything scripts can reach from the global scope. */
export const SCRIPT_GLOBALS: Record<string, Member> = {
  pm: prop("The script API (Postman-compatible)", {
    test: fn("(name, fn)", "A named test: it fails when fn throws", "test(\"\", () => {\n  \n})"),
    expect: fn("(value)", "Chai-style assertion: pm.expect(x).to.equal(y)"),
    variables: prop("Variables for this send (or run); get() looks through all scopes", scope("request")),
    environment: prop("The active environment", { ...scope("environment"), name: prop("Name of the active environment") }),
    collectionVariables: prop("Workspace variables", scope("workspace")),
    globals: prop("Global variables", scope("global")),
    iterationData: prop("The runner's current data row", { get: fn("(name)"), has: fn("(name)"), toObject: fn("()") }),
    info: prop("About this run", {
      eventName: prop("prerequest or test"),
      iteration: prop("0-based"),
      iterationCount: prop(),
      requestName: prop(),
      requestId: prop("The request's path"),
    }),
    request: prop("The request (before variables are resolved in pre-request scripts)", {
      url: prop("URL (toString() for the text)", {
        toString: fn("()"),
        update: fn("(url)"),
        getHost: fn("()"),
        getPath: fn("()"),
        getQueryString: fn("()"),
        query: prop("Query parameters", propertyList()),
      }),
      method: prop(),
      headers: prop("Request headers", propertyList()),
      body: prop("Body", { raw: prop("Body text"), mode: prop(), update: fn("(text)") }),
    }),
    response: prop("The response (post-response scripts)", {
      code: prop("Status code"),
      status: prop("Reason phrase"),
      headers: prop("Response headers", propertyList(true)),
      text: fn("()", "Body as text"),
      json: fn("()", "Body parsed as JSON"),
      responseTime: prop("Milliseconds"),
      responseSize: prop("Body size in bytes"),
      cookies: prop("Cookies the response set", cookieList),
      to: prop("Response assertions", responseTo),
    }),
    sendRequest: fn(
      "(request, callback?)",
      "Send another request: a URL or { url, method, header, body }. Call back with (err, res), or await it.",
      'sendRequest("", (err, res) => {\n  \n})'
    ),
    cookies: prop("Cookies the cookie jar sends to this request's URL", {
      ...cookieList,
      jar: fn("()", "The cookie jar: get, getAll, set, unset, clear (this request's site only)"),
    }),
    visualizer: prop("A view of the response in the Visualize tab", {
      set: fn("(template, data)", "Render a Handlebars template with data: pm.visualizer.set('<b>{{name}}</b>', json)"),
      clear: fn("()"),
    }),
    execution: prop("Control the run", {
      setNextRequest: fn("(name | null)", "In a collection run, go on with this request (null ends the iteration)"),
      skipRequest: fn("()", "Pre-request scripts: don't send this request"),
    }),
    require: fn("(\"npm:name@version\")", "A built-in library, e.g. pm.require('npm:lodash@4')"),
  }),
  require: fn("(name)", "A built-in library: lodash, crypto-js, moment, ajv, uuid, tv4, chai, csv-parse, xml2js, cheerio, handlebars…"),
  setTimeout: fn("(fn, ms)", "Run fn later; the script waits for it"),
  setInterval: fn("(fn, ms)"),
  clearTimeout: fn("(id)"),
  clearInterval: fn("(id)"),
  console: prop("Output shown in the response's Console tab", {
    log: fn("(...values)"),
    info: fn("(...values)"),
    warn: fn("(...values)"),
    error: fn("(...values)"),
    debug: fn("(...values)"),
  }),
};

/** Chains and assertions offered after `pm.expect(…).` */
export const EXPECT_WORDS: Record<string, Member> = Object.fromEntries([
  ...["to", "be", "been", "is", "that", "which", "and", "has", "have", "with", "at", "of", "same", "not", "deep", "nested", "own", "any", "all"].map(
    (w) => [w, prop("chain")] as const,
  ),
  ...["ok", "true", "false", "null", "undefined", "NaN", "exist", "empty"].map((w) => [w, prop("assertion")] as const),
  ...(
    [
      ["equal", "(value)"],
      ["eql", "(value)"],
      ["a", "(type)"],
      ["an", "(type)"],
      ["include", "(value)"],
      ["contain", "(value)"],
      ["above", "(n)"],
      ["below", "(n)"],
      ["least", "(n)"],
      ["most", "(n)"],
      ["within", "(start, finish)"],
      ["property", "(name, value?)"],
      ["lengthOf", "(n)"],
      ["match", "(RegExp)"],
      ["oneOf", "(list)"],
      ["keys", "(...names)"],
      ["members", "(list)"],
      ["closeTo", "(n, delta)"],
      ["string", "(text)"],
      ["instanceOf", "(constructor)"],
      ["throw", "(error?)"],
      ["satisfy", "(fn)"],
    ] as const
  ).map(([w, d]) => [w, fn(d)] as const),
]);

export interface MemberCompletion {
  /** Offset (in `before`) where the completed word starts. */
  from: number;
  options: { label: string; type: string; detail?: string; info?: string; apply?: string }[];
}

/**
 * Members to offer for the text before the cursor: `pm.environment.s` → the
 * environment scope's members starting at `s`; `pm.expect(x).to.` → chai words.
 */
export function completeMembers(before: string): MemberCompletion | null {
  const m = /(?:^|[^\w$.])((?:[A-Za-z_$][\w$]*\.)+)([\w$]*)$/.exec(before);
  const toOptions = (members: Record<string, Member>, from: number): MemberCompletion => ({
    from,
    options: Object.entries(members).map(([label, x]) => ({
      label,
      type: x.type === "function" ? "method" : x.type === "namespace" ? "namespace" : "property",
      detail: x.detail,
      info: x.info,
      apply: x.apply,
    })),
  });
  // `pm.expect(…)` / `pm.response.to…` chains after a call.
  const chain = /\)((?:\.[\w$]+)*)\.([\w$]*)$/.exec(before);
  if (chain && /\bexpect\s*\(/.test(before)) {
    return toOptions(EXPECT_WORDS, before.length - chain[2].length);
  }
  if (!m) {
    const word = /(?:^|[^\w$.])([\w$]*)$/.exec(before);
    if (!word || !word[1]) return null;
    return toOptions(SCRIPT_GLOBALS, before.length - word[1].length);
  }
  let members: Record<string, Member> | undefined = SCRIPT_GLOBALS;
  for (const part of m[1].split(".").filter(Boolean)) {
    members = members?.[part]?.children;
    if (!members) return null;
  }
  return toOptions(members, before.length - m[2].length);
}

/** Tests tab label: passed/total (skipped tests don't count). */
export function testSummary(report: ScriptReport | undefined | null): { passed: number; failed: number; skipped: number; total: number } {
  const tests = report?.tests ?? [];
  const skipped = tests.filter((t) => t.skipped).length;
  const passed = tests.filter((t) => t.passed).length;
  return { passed, skipped, failed: tests.length - passed - skipped, total: tests.length - skipped };
}

/** Snippets offered by the editor's menu. */
export const SNIPPETS: Record<ScriptEvent, { label: string; code: string }[]> = {
  preRequest: [
    { label: "Set a variable for this send", code: 'pm.variables.set("name", "value");' },
    { label: "Set an environment variable", code: 'pm.environment.set("name", "value");' },
    { label: "Add a header", code: 'pm.request.headers.upsert({ key: "X-Request-Id", value: pm.variables.replaceIn("{{$guid}}") });' },
    { label: "Log the request", code: "console.log(pm.request.method, pm.request.url.toString());" },
    {
      label: "Get a token first",
      code: 'const res = await pm.sendRequest({\n  url: pm.variables.replaceIn("{{baseUrl}}/login"),\n  method: "POST",\n  header: { "Content-Type": "application/json" },\n  body: { mode: "raw", raw: JSON.stringify({ user: pm.environment.get("user") }) },\n});\npm.request.headers.upsert({ key: "Authorization", value: "Bearer " + res.json().token });',
    },
    {
      label: "Sign the body (HMAC)",
      code: 'const signature = CryptoJS.HmacSHA256(pm.request.body.raw, pm.environment.get("secret")).toString();\npm.request.headers.upsert({ key: "X-Signature", value: signature });',
    },
  ],
  postResponse: [
    { label: "Status code is 200", code: 'pm.test("Status code is 200", () => {\n  pm.response.to.have.status(200);\n});' },
    {
      label: "Response time is below 500 ms",
      code: 'pm.test("Response time is below 500 ms", () => {\n  pm.expect(pm.response.responseTime).to.be.below(500);\n});',
    },
    {
      label: "JSON value check",
      code: 'pm.test("Body has an id", () => {\n  const json = pm.response.json();\n  pm.expect(json).to.have.property("id");\n});',
    },
    { label: "Save a value from the body", code: 'const json = pm.response.json();\npm.environment.set("token", json.token);' },
    { label: "Header is present", code: 'pm.test("Content-Type is set", () => {\n  pm.response.to.have.header("Content-Type");\n});' },
    {
      label: "Matches a JSON Schema",
      code: 'const schema = { type: "object", required: ["id"], properties: { id: { type: "integer" } } };\npm.test("Body matches the schema", () => {\n  pm.response.to.have.jsonSchema(schema);\n});',
    },
    {
      label: "Show a table (Visualize)",
      code: 'const template = `<table>{{#each items}}<tr><td>{{id}}</td><td>{{name}}</td></tr>{{/each}}</table>`;\npm.visualizer.set(template, { items: pm.response.json() });',
    },
  ],
};
