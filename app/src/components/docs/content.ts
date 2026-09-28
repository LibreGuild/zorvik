// The in-app docs: what Zorvik can do, one topic per feature. Text in `[...]` is a
// keyboard shortcut ("mod" is ⌘ on macOS, Ctrl elsewhere); `code` spans are monospace.

export interface DocTopic {
  id: string;
  /** Short group label shown above the title. */
  group: string;
  title: string;
  /** One line: why it matters. */
  tagline: string;
  /** Illustration (assets/docs/<image>.webp). */
  image: string;
  intro: string;
  features: string[];
  tryIt: string[];
  tip?: string;
}

export const QUICK_START = [
  "Create a workspace, or open a folder you already have. It's a folder of YAML files you can commit to Git.",
  "Press [mod+N] for a new request, paste a URL, and press [mod+Enter] to send it.",
  "Press [mod+S] to save it into your collection, and [mod+E] to add environments such as Local or Staging.",
];

export const SHORTCUTS: [string, string][] = [
  ["mod+N", "New request"],
  ["mod+Enter", "Send / connect / start"],
  ["mod+S", "Save"],
  ["mod+W", "Close tab"],
  ["mod+K", "Find anything"],
  ["mod+E", "Environments"],
  ["mod+L", "Jump to the URL"],
  ["mod+,", "Settings"],
  ["Ctrl+Tab", "Next tab"],
  ["mod++", "Zoom in"],
  ["mod+-", "Zoom out"],
  ["mod+0", "Reset zoom"],
];

export const TOPICS: DocTopic[] = [
  {
    id: "academy",
    group: "Learn",
    title: "Training Bootcamp",
    tagline: "Learn networks and APIs hands-on, from zero to load testing.",
    image: "docs-academy",
    intro:
      "A course built into Zorvik. Each lesson is a short reading with diagrams, a lab you do in the real workbench against practice servers on your own computer, and a quick check. The Lab Guide ticks steps off the moment you get them right.",
    features: [
      "16 units: networks, DNS, HTTP, sending data, auth and JWT, TLS, environments, testing, GraphQL and gRPC, WebSocket and SSE, TCP and UDP, mocking, load testing, automation, and a capstone",
      "Labs start their own servers and fill in a Lab environment; nothing leaves your machine",
      "Hints from a nudge to the exact clicks, and Do it for me when you're stuck",
      "XP, levels, ranks, streaks and badges; finish the capstone for the Zorvik Bootcamp Graduate certificate",
      "The Training Bootcamp workspace is always there and can't be deleted; Reset it from the Academy menu (progress stays)",
    ],
    tryIt: ["Choose Training Bootcamp at the top of the workspace menu (or on the welcome screen), then Start the Bootcamp.", "Stuck in a lab? Open a hint in the Lab Guide on the right."],
    tip: "Already know a topic? Take the unit's Test out quiz to earn its badge right away.",
  },
  {
    id: "requests",
    group: "Build & send",
    title: "HTTP requests",
    tagline: "Craft any call and see exactly what happened on the wire.",
    image: "docs-requests",
    intro:
      "Every request shows its status, time and size. It also shows how that time was spent, which certificate answered, where redirects went, and the headers that actually left your machine.",
    features: [
      "Any method (custom ones too), a params table kept in sync with the URL, and `:path` variables",
      "Bodies: JSON, text, XML, form, multipart with files, or a binary file",
      "HTTP/1.1, HTTP/2 and HTTP/3, per-request timeouts, redirects and TLS checks",
      "Pretty JSON, raw, HTML and image previews, and a hex view",
      "A timing waterfall: DNS → connect → TLS → first byte → download",
      "Save a response to a file, or copy any request as cURL or code (Kotlin, Swift, JavaScript, Python)",
      "Query parameters keep their descriptions; hover the ⓘ next to a name",
    ],
    tryIt: ["Press [mod+N], paste a URL, then [mod+Enter].", "Open Timing and Info on the response to see where the time went."],
    tip: "Hover any `{{variable}}` to see its value and where it comes from. Undefined ones turn red before you send.",
  },
  {
    id: "graphql",
    group: "Build & send",
    title: "GraphQL",
    tagline: "Queries that know your schema.",
    image: "docs-graphql",
    intro:
      "Point a GraphQL request at an endpoint and Zorvik fetches its schema by itself, with the same auth and headers the request uses. From then on the editor completes fields, checks your query and explains types.",
    features: [
      "Autocomplete and inline errors from the live schema",
      "Variables as JSON, and an operation picker when a document has several",
      "A schema explorer to browse types, fields and deprecations",
      "Prettify with one click; auth, headers and environments work as for HTTP",
    ],
    tryIt: ["In the collection, choose New → GraphQL request.", "Type `{` in the query and let autocomplete suggest the fields."],
  },
  {
    id: "grpc",
    group: "Build & send",
    title: "gRPC",
    tagline: "Call services by reflection or `.proto` files: unary and streaming.",
    image: "docs-grpc",
    intro:
      "Use `grpc://` or `grpcs://` and pick a method. Services come from server reflection, or from your `.proto` files and their imports. Messages are plain JSON with a template to start from.",
    features: [
      "Unary, server, client and bidirectional streaming calls",
      "Metadata, headers and trailers, and statuses with their details",
      "TLS, custom certificates and OAuth 2.0, the same as HTTP requests",
    ],
    tryIt: ["Choose New → gRPC request, enter `grpc://host:port`, and pick a method from the list."],
  },
  {
    id: "realtime",
    group: "Build & send",
    title: "WebSocket, SSE, TCP, UDP & MQTT",
    tagline: "Talk to live connections and watch every message.",
    image: "docs-realtime",
    intro:
      "Connection requests keep a live log of everything sent and received, so you can follow a conversation and filter it. DNS queries are a request type too.",
    features: [
      "WebSocket: text, JSON or binary (hex) messages",
      "Server-Sent Events: every event as it arrives",
      "Raw TCP (plain or TLS) and UDP, with message framing",
      "MQTT: subscriptions, QoS and retained messages",
      "DNS: any record type over UDP, TCP, TLS or HTTPS",
    ],
    tryIt: ["Choose New → WebSocket, enter a `ws://` or `wss://` URL, and press [mod+Enter] to connect."],
  },
  {
    id: "variables",
    group: "Organize",
    title: "Environments, variables & secrets",
    tagline: "One request, many environments, without leaking secrets into Git.",
    image: "docs-variables",
    intro:
      "Write `{{baseUrl}}/users/{{id}}` once and switch between Local, Staging and Production in the title bar. Secret values stay on this computer and never go into your workspace files.",
    features: [
      "Environments ([mod+E]) and collection-wide variables",
      "Dynamic values such as `{{$uuid}}` and `{{$timestamp}}`",
      "Secret variables: stored locally, shown as ••••••",
      "Values set by scripts are kept on this computer, never written to your files, and shown under Set by scripts",
    ],
    tryIt: ["Press [mod+E], add an environment with `baseUrl`, and select it in the title bar."],
    tip: "An undefined host stops the send with a clear message instead of a confusing network error.",
  },
  {
    id: "auth",
    group: "Organize",
    title: "Auth",
    tagline: "Set it once on a folder; every request inside inherits it.",
    image: "docs-auth",
    intro:
      "Auth and headers flow down from the collection to its folders and requests, and any level can override them. OAuth 2.0 tokens are fetched, cached and refreshed for you.",
    features: [
      "Basic, Bearer and API key (header or query)",
      "OAuth 2.0: client credentials, password, and authorization code with PKCE",
      "Custom CA certificates, client certificates (mTLS) and proxies in Settings",
    ],
    tryIt: ["Right-click a folder → Folder settings… → Auth, and leave its requests on Inherit."],
  },
  {
    id: "scripts",
    group: "Test",
    title: "Scripts & tests",
    tagline: "Postman-compatible JavaScript before and after every request.",
    image: "docs-scripts",
    intro:
      "Pre-request scripts can prepare a request; post-response scripts check what came back. Scripts run on the collection, its folders and each request, outermost first, in a sandbox with time and memory limits.",
    features: [
      "The `pm` API: `pm.environment`, `pm.variables`, `pm.request`, `pm.response` and more",
      "`pm.test` with `pm.expect` assertions; results show in the Tests tab",
      "Console output next to the response",
      "Postman collections keep their scripts when imported",
    ],
    tryIt: ["In a request's Scripts tab, add: `pm.test('ok', () => pm.response.to.have.status(200));`"],
  },
  {
    id: "runner",
    group: "Test",
    title: "Collection runner",
    tagline: "Run a whole folder end to end, with data files and reports.",
    image: "docs-runner",
    intro:
      "Run a folder, or the whole collection, in order with all its scripts and tests. Feed it a CSV or JSON file and it runs once per row.",
    features: [
      "Choose and reorder requests, set iterations and a delay",
      "Data files: each column becomes a `{{variable}}`",
      "Repeat until: send a request again until a condition holds, e.g. `pm.response.json().status === \"done\"`, to wait for a job",
      "Event streams (SSE) run too: they stop at a named event, a number of events or a time limit, and tests see `pm.response.events`",
      "Stop at the first failure, and watch the results arrive live; skipped requests say why",
      "Export JSON or JUnit XML for your CI",
    ],
    tryIt: [
      "Right-click a folder → Run…, then press [mod+Enter].",
      "To poll, open a request's Settings → Repeat until, and give a condition and a time limit.",
    ],
  },
  {
    id: "load",
    group: "Test",
    title: "Load testing",
    tagline: "Find the breaking point before your users do.",
    image: "docs-load",
    intro:
      "Turn saved requests into a load test, watch it live, and get honest numbers. The arrival-rate model measures from the scheduled start, so a slow server can't hide its queueing.",
    features: [
      "Virtual users or arrival rate, with ramp, hold and spike stages",
      "Weighted targets, think time and keep-alive options",
      "Live throughput, latency percentiles, errors, status codes and the generator's CPU",
      "Thresholds such as p95 < 300 ms that pass or fail the run",
      "A data file gives each virtual user its own row, so the test doesn't hit the same id every time",
      "Captures save a value from a response, e.g. `$.id` of a new order, for that user's next requests",
      "Timing: time to first byte, transfer and connect, plus the server's own time when it sends a Server-Timing header",
      "Run history with HTML and JSON reports, and Compare to see how a run differs from an earlier one",
    ],
    tryIt: [
      "Right-click a request → Load test…, pick a shape, and press [mod+Enter].",
      "After a second run, open Compare in the results and pick the earlier run.",
    ],
    tip: "Hosts outside this computer and your network need your confirmation first. Only load test systems you're allowed to.",
  },
  {
    id: "servers",
    group: "Simulate",
    title: "Mock servers & servers",
    tagline: "Stand in for backends that don't exist yet.",
    image: "docs-servers",
    intro:
      "Start a mock API in seconds from a folder, an OpenAPI file or a response. You can also run WebSocket, SSE, TCP, UDP and DNS servers, or a relay to watch the traffic between two sides.",
    features: [
      "Mock routes with templates, delays, faults, CORS, and a fallback or forward to the real backend",
      "Live traffic for every server, and replies to one client or all of them",
      "Servers keep running in the background; the title bar shows what's running",
      "Saved in the workspace, so your team gets the same mocks",
      "A port that's taken says which program holds it",
    ],
    tryIt: ["Right-click a folder → Mock this folder…, then start it from the Servers rail."],
    tip: "Mock bodies can echo the request: `{{request.params.id}}`, `{{request.query.q}}`, `{{request.headers.name}}` and `{{request.body}}`.",
  },
  {
    id: "tools",
    group: "Inspect",
    title: "Network tools",
    tagline: "A pocket toolkit for when something's off.",
    image: "docs-tools",
    intro: "Answer quick questions about a host without leaving the app. Each tool opens in its own tab.",
    features: [
      "TLS inspector: certificate chain, expiry, protocols and ciphers",
      "DNS lookup with any resolver; port check; ping over ICMP or TCP",
      "Your network interfaces, so other devices can reach your servers",
      "Encoders: Base64, URL, hex, JWT, hashes and timestamps",
      "HTTP/3 check",
    ],
    tryIt: ["Open the Tools rail and pick TLS inspector; enter a host such as `example.com`."],
  },
  {
    id: "import",
    group: "Organize",
    title: "Import & export",
    tagline: "Bring your collections; take your requests anywhere.",
    image: "docs-import",
    intro: "Move from other tools in a minute. Scripts and variables come along, and nothing leaves your machine.",
    features: [
      "Postman collections and environments",
      "OpenAPI 3 and Swagger 2, from a file, by drag and drop, or from a URL, with realistic example bodies",
      "OpenAPI path parameters become variables: `{session_id}` turns into `{{sessionId}}` in the new environment",
      "Responses to imported requests are checked against the spec: a test named “Matches the API spec” fails when a field or type drifts",
      "Update from API spec: a new version adds and updates operations and keeps your edits; removed ones stay, crossed out",
      "cURL commands (bash, cmd and PowerShell) by pasting",
      "Copy any request as cURL, or as Kotlin, Swift, JavaScript or Python code",
    ],
    tryIt: [
      "Open the collection menu → Import…, then drop a file, paste a cURL command, or enter a URL.",
      "After the API changes, right-click the imported folder → Update from API spec… and preview the changes.",
    ],
    tip: "The imported document is kept in `specs/` in your workspace. Switch the checks off in Folder settings → API spec.",
  },
  {
    id: "workspace",
    group: "Organize",
    title: "Workspaces, Git & history",
    tagline: "Your collection is a folder of YAML: review it, diff it, share it.",
    image: "docs-workspace",
    intro:
      "Requests, environments, load tests and servers are small readable YAML files. Commit them, review them in pull requests, and pull your teammates' changes; the app reloads as files change.",
    features: [
      "`requests/`, `environments/`, `loadtests/` and `servers/` folders",
      "Secret values stay out of the files",
      "Searchable history of every send, and a cookie jar per workspace",
      "Tabs come back as you left them",
    ],
    tryIt: ["Press [mod+K] to find any request, server, load test or tool."],
  },
  {
    id: "cli",
    group: "Automate",
    title: "Command line & CI",
    tagline: "The same engine in your terminal and your pipelines.",
    image: "docs-cli",
    intro:
      "`zorvik` is installed with the app. Run your collection's tests in CI, gate a deployment on load-test thresholds, or start a mock server for integration tests.",
    features: [
      "`zorvik run <workspace>`: environments, folders, data files, JUnit and JSON reports",
      "`zorvik load <workspace> <test>`: exits with 1 when a threshold fails",
      "`zorvik serve <workspace> <server>`: runs a saved mock or server",
      "`zorvik mcp`: the connection for AI agents",
    ],
    tryIt: ["In a terminal: `zorvik run ./my-api --env Staging --junit report.xml`"],
    tip: "On macOS, Settings → AI agents → Add zorvik to PATH makes the command available everywhere.",
  },
  {
    id: "agents",
    group: "Automate",
    title: "AI agents",
    tagline: "Let Claude Code, Codex or Gemini drive Zorvik while you watch.",
    image: "docs-agents",
    intro:
      "Connect your coding agent once, then ask it to map the APIs in your code to a collection, send requests, run tests or start a load test. Every action shows up live in the app, and risky ones wait for your OK.",
    features: [
      "One command to connect (Settings → AI agents shows it for your agent)",
      "Agents can build and run mock servers, read their traffic, read streams, variables and history, and export code",
      "Deletes, load tests, servers and requests to outside hosts ask you first",
      "The AI agents panel lists every action; follow mode opens what the agent touches",
      "Secret values are masked in everything the agent sees",
    ],
    tryIt: ["Ask your agent: “Map the APIs in this repo to Zorvik and run them.”"],
    tip: "In Claude Code, `/mcp__zorvik__map_apis` starts a guided mapping of your codebase.",
  },
];
