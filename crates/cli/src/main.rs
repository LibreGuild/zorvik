//! `zorvik` — run requests from a Zorvik workspace in a terminal or CI,
//! run one of its load tests, serve one of its saved servers (mock API,
//! WebSocket, TCP, …), or let an AI agent control the app (`zorvik mcp`).

use std::io::{IsTerminal, Write as _};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::{Duration, Instant};

use clap::{Parser, Subcommand};
use tokio_util::sync::CancellationToken;
use zorvik_api::runner::{self, RunPlan, RunResult, RunSummary};
use zorvik_api::{ScriptVars, SendContext, values_of};
use zorvik_engine::Client;
use zorvik_load::{DataRow, LoadEvent, PhaseSummary, Plan, PlanTarget, Render, RequestSource, Summary, UserVars};
use zorvik_servers::{Reporter, ServerEvent, StartOptions, TrafficDirection, TrafficEntry, TrafficKind};
use zorvik_workspace::formats::{LoadModel, LoadTest, RequestKind, RequestSettings, Server, ServerKind, Variable};
use zorvik_workspace::oauth2::{TokenCache, ensure_token};
use zorvik_workspace::resolve::{Inheritance, apply_token, check_url_variables, resolve};
use zorvik_workspace::settings::Settings;
use zorvik_workspace::vars::VarContext;
use zorvik_workspace::{EnvironmentEntry, Workspace};

#[derive(Parser)]
#[command(name = "zorvik", version, about = "Run Zorvik workspace requests from the command line")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run the HTTP requests of a workspace or folder (in sidebar order) with their
    /// scripts and tests, once per iteration or data row, and report the results.
    Run(RunArgs),
    /// Run a saved load test and check its thresholds (exit code 1 when one fails).
    Load(LoadArgs),
    /// Start a saved server (mock API, WebSocket, SSE, TCP, UDP, DNS, relay) and
    /// print its traffic until Ctrl+C.
    Serve(ServeArgs),
    /// MCP server for AI agents (Claude Code, Codex, Gemini CLI, Cursor, …) over
    /// stdio: they control the Zorvik app, which is started when needed.
    Mcp(McpArgs),
}

#[derive(Parser)]
struct McpArgs {
    /// The app's data folder (default: where the installed app keeps it).
    #[arg(long, hide = true)]
    data_dir: Option<PathBuf>,
}

#[derive(Parser)]
struct ServeArgs {
    /// Workspace folder (contains zorvik.yaml).
    workspace: PathBuf,
    /// Server name or id (its file name under servers/).
    server: String,
    /// Environment name or id (for {{variables}} in answers).
    #[arg(short, long)]
    env: Option<String>,
    /// Set a variable (highest precedence), e.g. --var token=abc. Repeatable.
    #[arg(long = "var", value_name = "KEY=VALUE")]
    vars: Vec<String>,
    /// Listen on this port instead of the saved one (0 = any free port).
    #[arg(long)]
    port: Option<u16>,
    /// Listen on this address instead of the saved one (0.0.0.0 = other devices too).
    #[arg(long)]
    host: Option<String>,
    /// Skip TLS certificate verification when forwarding (mock fallback, relay).
    #[arg(short = 'k', long)]
    insecure: bool,
    /// Print JSON lines (the start, then one per traffic entry) instead of text.
    #[arg(long)]
    json: bool,
}

#[derive(Parser)]
struct LoadArgs {
    /// Workspace folder (contains zorvik.yaml).
    workspace: PathBuf,
    /// Load test name or id (its file name under loadtests/).
    test: String,
    /// Environment name or id.
    #[arg(short, long)]
    env: Option<String>,
    /// Set a variable (highest precedence), e.g. --var token=abc. Repeatable.
    #[arg(long = "var", value_name = "KEY=VALUE")]
    vars: Vec<String>,
    /// Save the summary as JSON to this file.
    #[arg(long, value_name = "FILE")]
    json: Option<PathBuf>,
    /// Save an HTML report to this file.
    #[arg(long, value_name = "FILE")]
    html: Option<PathBuf>,
    /// Print only the summary (no progress line every second).
    #[arg(short, long)]
    quiet: bool,
    /// Skip TLS certificate verification.
    #[arg(short = 'k', long)]
    insecure: bool,
    /// Let requests send body files, and the data file be, outside the workspace folder.
    #[arg(long)]
    allow_outside_files: bool,
}

#[derive(Parser)]
struct RunArgs {
    /// Workspace folder (contains zorvik.yaml).
    workspace: PathBuf,
    /// Environment name or id.
    #[arg(short, long)]
    env: Option<String>,
    /// Only run requests under this folder (path relative to requests/).
    #[arg(short, long)]
    folder: Option<String>,
    /// Set a variable (highest precedence), e.g. --var token=abc. Repeatable.
    #[arg(long = "var", value_name = "KEY=VALUE")]
    vars: Vec<String>,
    /// Data file: CSV with a header row, or a JSON array of objects. One iteration
    /// per row; columns are variables ({{name}}, pm.iterationData).
    #[arg(short, long, value_name = "FILE")]
    data: Option<PathBuf>,
    /// Iterations (default: one per data row, or 1).
    #[arg(short = 'n', long, value_name = "N", value_parser = clap::value_parser!(u32).range(1..=runner::MAX_ITERATIONS as i64))]
    iterations: Option<u32>,
    /// Pause between requests, in milliseconds.
    #[arg(long, value_name = "MS", default_value_t = 0, value_parser = clap::value_parser!(u64).range(0..=runner::MAX_DELAY_MS))]
    delay: u64,
    /// HTTP 4xx/5xx responses don't fail requests that have no tests.
    #[arg(long)]
    allow_http_errors: bool,
    /// Stop at the first failed request or test.
    #[arg(long)]
    bail: bool,
    /// Request timeout in milliseconds (0 = none).
    #[arg(long)]
    timeout: Option<u64>,
    /// Skip TLS certificate verification.
    #[arg(short = 'k', long)]
    insecure: bool,
    /// Let requests send body files from outside the workspace folder.
    #[arg(long)]
    allow_outside_files: bool,
    /// Print a JSON report (summary and results) instead of text.
    #[arg(long)]
    json: bool,
    /// Save a JUnit XML report to this file.
    #[arg(long, value_name = "FILE")]
    junit: Option<PathBuf>,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build().expect("tokio runtime");
    match cli.command {
        Command::Run(args) => runtime.block_on(run(args)),
        Command::Load(args) => runtime.block_on(load(args)),
        Command::Serve(args) => runtime.block_on(serve(args)),
        Command::Mcp(args) => runtime.block_on(mcp(args)),
    }
}

/// `zorvik mcp`: stdout is the MCP channel, so problems go to stderr only.
async fn mcp(args: McpArgs) -> ExitCode {
    let Some(data_dir) = args.data_dir.or_else(zorvik_mcp::default_data_dir) else {
        eprintln!("zorvik mcp: no data folder for Zorvik on this system");
        return ExitCode::FAILURE;
    };
    let app = std::env::var_os("ZORVIK_APP").filter(|a| !a.is_empty()).map(PathBuf::from);
    match zorvik_mcp::bridge::run(zorvik_mcp::bridge::BridgeOptions { data_dir, app }).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("zorvik mcp: {e}");
            ExitCode::FAILURE
        }
    }
}

fn fail(message: String) -> ExitCode {
    eprintln!("error: {message}");
    ExitCode::from(2)
}

/// `--var KEY=VALUE` values.
fn overrides(values: &[String]) -> Result<Vec<Variable>, String> {
    values
        .iter()
        .map(|v| match v.split_once('=') {
            Some((k, val)) => Ok(Variable { key: k.trim().into(), value: val.into(), enabled: true, secret: false }),
            None => Err(format!("--var expects KEY=VALUE, got '{v}'")),
        })
        .collect()
}

/// An environment by id or name, ignoring case.
fn find_environment(ws: &Workspace, wanted: &str) -> Result<EnvironmentEntry, String> {
    ws.list_environments()
        .unwrap_or_default()
        .into_iter()
        .find(|e| e.id.eq_ignore_ascii_case(wanted) || e.environment.name.eq_ignore_ascii_case(wanted))
        .ok_or_else(|| format!("environment '{wanted}' not found"))
}

/// Secret values live only in the app's data folder (files keep them empty): leave them
/// undefined so they are reported, instead of silently sending "" — pass them with --var.
fn without_blank_secrets(vars: &[Variable]) -> Vec<Variable> {
    vars.iter().filter(|v| !(v.secret && v.value.is_empty())).cloned().collect()
}

/// `--var` overrides, then the environment, then workspace variables.
fn variables(ws: &Workspace, env: Option<&str>, values: &[String]) -> Result<VarContext, String> {
    let set = overrides(values)?;
    let env_vars = match env {
        Some(wanted) => find_environment(ws, wanted)?.environment.variables,
        None => Vec::new(),
    };
    let mut vars = VarContext::new();
    vars.push_layer(&set)
        .push_layer(&without_blank_secrets(&env_vars))
        .push_layer(&without_blank_secrets(&ws.meta().variables));
    Ok(vars)
}

/// Variables for a run (the same scopes as [`variables`], for scripts) and the
/// secrets to hide in reported URLs: values of variables declared secret, also
/// when given with `--var`. Blank ones are listed too: the runner hides what
/// scripts set them to.
fn run_variables(
    ws: &Workspace,
    env: Option<&str>,
    values: &[String],
) -> Result<(ScriptVars, Vec<(String, String)>), String> {
    let set = overrides(values)?;
    let env = env.map(|wanted| find_environment(ws, wanted)).transpose()?;
    let env_vars = env.as_ref().map(|e| e.environment.variables.clone()).unwrap_or_default();
    let mut vars = ScriptVars::default();
    vars.values.overrides = values_of(&set);
    vars.values.environment = values_of(&without_blank_secrets(&env_vars));
    vars.values.collection = values_of(&without_blank_secrets(&ws.meta().variables));
    vars.environment = env.map(|e| (e.id, e.environment.name));
    let secrets = env_vars
        .iter()
        .chain(&ws.meta().variables)
        .filter(|v| v.secret)
        .map(|v| {
            let key = v.key.trim().to_string();
            let value = vars.values.overrides.get(&key).cloned().unwrap_or_else(|| v.value.clone());
            (key, value)
        })
        .collect();
    Ok((vars, secrets))
}

async fn run(args: RunArgs) -> ExitCode {
    let color = !args.json && std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none();
    let paint = move |code: &str, s: &str| if color { format!("\x1b[{code}m{s}\x1b[0m") } else { s.to_string() };

    let ws = match Workspace::open(&args.workspace) {
        Ok(ws) => ws,
        Err(e) => return fail(e.message),
    };
    let (name, items) = match runner::collect(&ws, args.folder.as_deref().unwrap_or_default(), None) {
        Ok(found) => found,
        Err(e) => return fail(e),
    };
    let (mut vars, secrets) = match run_variables(&ws, args.env.as_deref(), &args.vars) {
        Ok(found) => found,
        Err(e) => return fail(e),
    };
    let data = match &args.data {
        Some(path) => match read_data(path) {
            Ok(data) => data.rows,
            Err(e) => return fail(format!("data file {}: {e}", path.display())),
        },
        None => Vec::new(),
    };
    let plan = RunPlan {
        name,
        items,
        iterations: runner::iteration_count(args.iterations, data.len()),
        delay: Duration::from_millis(args.delay),
        data,
        stop_on_failure: args.bail,
        allow_http_errors: args.allow_http_errors,
        secrets,
    };

    let mut settings = Settings::default();
    if let Some(t) = args.timeout {
        settings.request.timeout_ms = t;
    }
    if args.insecure {
        settings.request.verify_tls = false;
    }
    settings.files_outside_workspace = args.allow_outside_files;
    let client = std::sync::Arc::new(Client::new());
    let tokens = TokenCache::in_memory();
    let jar = std::sync::Arc::new(zorvik_engine::CookieJar::new());
    let specs = zorvik_api::specs::SpecCache::default();
    let cx = SendContext {
        ws: &ws,
        meta: ws.meta(),
        client: &client,
        settings: &settings,
        tokens: &tokens,
        jar: Some(&jar),
        guard: None,
        specs: Some(&specs),
    };

    // Ctrl+C stops the run; the summary so far is still printed (and saved).
    let cancel = CancellationToken::new();
    let stop = cancel.clone();
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            stop.cancel();
        }
    });
    let mut printed_iteration = None;
    let report = Box::pin(runner::run(&cx, &plan, &mut vars, &cancel, |result, vars| {
        // Values scripts set apply for the rest of the run; nothing is kept after it.
        vars.changes.clear();
        if args.json {
            return;
        }
        if plan.iterations > 1 && printed_iteration != Some(result.iteration) {
            if printed_iteration.is_some() {
                out("");
            }
            printed_iteration = Some(result.iteration);
            out(&paint("1", &format!("Iteration {} of {}", result.iteration + 1, plan.iterations)));
        }
        print_result(result, &paint);
    }))
    .await;

    let mut code = run_exit_code(&report.summary);
    if args.json {
        out(&serde_json::to_string_pretty(&report).unwrap_or_default());
    } else {
        print_run_summary(&report.summary, &paint);
    }
    if let Some(path) = &args.junit
        && let Err(e) = std::fs::write(path, runner::junit_xml(&report))
    {
        eprintln!("error: could not save {}: {e}", path.display());
        code = 2;
    }
    ExitCode::from(code)
}

/// A data file given on the command line (relative to the current folder).
fn read_data(path: &std::path::Path) -> Result<runner::DataFile, String> {
    let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
    if !meta.is_file() {
        return Err("not a regular file".into());
    }
    if meta.len() > runner::MAX_DATA_FILE {
        return Err("larger than 50 MB".into());
    }
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    runner::parse_data(&name, &bytes)
}

/// `0` everything passed, `1` a request or test failed, `2` the run was stopped (Ctrl+C).
fn run_exit_code(summary: &RunSummary) -> u8 {
    if summary.stopped {
        2
    } else if summary.passed {
        0
    } else {
        1
    }
}

fn kind_name(kind: RequestKind) -> &'static str {
    match kind {
        RequestKind::Http => "HTTP",
        RequestKind::Websocket => "WS",
        RequestKind::Sse => "SSE",
        RequestKind::Tcp => "TCP",
        RequestKind::Udp => "UDP",
        RequestKind::Dns => "DNS",
        RequestKind::Mqtt => "MQTT",
        RequestKind::Grpc => "gRPC",
    }
}

/// One line per request, then its tests and problems (indented).
fn print_result(r: &RunResult, paint: &dyn Fn(&str, &str) -> String) {
    let name = printable(&r.name);
    if r.skipped {
        let reason = r.skip_reason.as_deref().unwrap_or("not a kind the runner sends");
        let note = paint("2", &format!("(skipped: {reason})"));
        out(&format!("{} {:<7} {name}  {note}", paint("33", "–"), kind_name(r.kind)));
        return;
    }
    let mark = if r.passed { paint("32", "✓") } else { paint("31", "✗") };
    let detail = match (&r.status, &r.error) {
        (_, Some(e)) => paint("31", &printable(e)),
        (Some(s), None) => {
            let code = if *s < 400 { "32" } else { "31" };
            let timing = format!("{:.0} ms, {} B", r.duration_ms.unwrap_or_default(), r.size.unwrap_or_default());
            format!("{} {}", paint(code, &s.to_string()), paint("2", &timing))
        }
        _ => String::new(),
    };
    out(&format!("{mark} {:<7} {name}  {}  {detail}", printable(&r.method), paint("2", &printable(&r.url))));
    for t in &r.tests {
        let test = printable(&t.name);
        if t.skipped {
            out(&format!("    {} {test} {}", paint("33", "–"), paint("2", "(skipped)")));
        } else if t.passed {
            out(&format!("    {} {test}", paint("32", "✓")));
        } else {
            let error = t.error.as_deref().map(|e| format!(" — {}", printable(e))).unwrap_or_default();
            out(&format!("    {} {test}{}", paint("31", "✗"), paint("31", &error)));
        }
    }
    for e in &r.script_errors {
        out(&format!("    {} {}", paint("31", "!"), paint("31", &printable(e))));
    }
    if !r.unresolved.is_empty() {
        out(&format!("    {} undefined variables: {}", paint("33", "!"), printable(&r.unresolved.join(", "))));
    }
}

fn print_run_summary(s: &RunSummary, paint: &dyn Fn(&str, &str) -> String) {
    let passed = s.requests - s.failed;
    out("");
    if s.stopped {
        out(&paint("33", "Stopped (Ctrl+C)."));
    } else if s.bailed {
        out(&paint("31", "Stopped at the first failure (--bail)."));
    }
    if let Some(e) = &s.error {
        out(&paint("31", &printable(e)));
    }
    out(&format!(
        "Requests  {} passed, {} failed, {} skipped ({} total)",
        paint("32", &passed.to_string()),
        paint(if s.failed > 0 { "31" } else { "32" }, &s.failed.to_string()),
        s.skipped,
        s.requests + s.skipped
    ));
    if s.tests_passed + s.tests_failed + s.tests_skipped > 0 {
        let skipped = if s.tests_skipped > 0 { format!(", {} skipped", s.tests_skipped) } else { String::new() };
        out(&format!(
            "Tests     {} passed, {} failed{skipped}",
            paint("32", &s.tests_passed.to_string()),
            paint(if s.tests_failed > 0 { "31" } else { "32" }, &s.tests_failed.to_string()),
        ));
    }
    let iterations = if s.iterations > 1 { format!(", {} iterations", s.per_iteration.len()) } else { String::new() };
    out(&format!("Time      {} s{iterations}", fixed1(s.duration_ms / 1000.0)));
}

// ---- load -------------------------------------------------------------------------------

/// A saved load test by id (file name) or name, ignoring case.
fn find_load_test(ws: &Workspace, wanted: &str) -> Result<LoadTest, String> {
    let nodes = ws.list_load_tests().map_err(|e| e.message)?;
    let node = nodes
        .iter()
        .find(|n| n.id == wanted)
        .or_else(|| nodes.iter().find(|n| n.id.eq_ignore_ascii_case(wanted) || n.name.eq_ignore_ascii_case(wanted)))
        .ok_or_else(|| {
            let names: Vec<&str> = nodes.iter().map(|n| n.name.as_str()).collect();
            if names.is_empty() {
                format!("load test '{wanted}' not found (the workspace has no load tests)")
            } else {
                format!("load test '{wanted}' not found (load tests: {})", names.join(", "))
            }
        })?;
    if let Some(e) = &node.error {
        return Err(format!("load test '{}' could not be read: {e}", node.name));
    }
    ws.read_load_test(&node.id).map_err(|e| e.message)
}

/// `--var` values (above a user's data row and captured values) and the
/// environment and workspace variables below them, as one layer each.
fn load_variables(
    ws: &Workspace,
    env: Option<&str>,
    values: &[String],
) -> Result<(Vec<Variable>, Vec<Variable>), String> {
    let set = overrides(values)?;
    let env_vars = match env {
        Some(wanted) => find_environment(ws, wanted)?.environment.variables,
        None => Vec::new(),
    };
    let mut seen = std::collections::HashSet::new();
    let base = without_blank_secrets(&env_vars)
        .into_iter()
        .chain(without_blank_secrets(&ws.meta().variables))
        .filter(|v| v.enabled && !v.key.trim().is_empty() && seen.insert(v.key.trim().to_string()))
        .collect();
    Ok((set, base))
}

/// Variables for one user's request: `--var`, then the user's (captured
/// values, its data row), then the environment and workspace variables.
fn user_context(above: &[Variable], user: &UserVars, base: &[Variable]) -> VarContext {
    let mut ctx = VarContext::new();
    ctx.push_layer(above);
    if !user.is_empty() {
        let vars: Vec<Variable> = user
            .iter()
            .map(|(key, value)| Variable { key: key.into(), value: value.into(), enabled: true, secret: false })
            .collect();
        ctx.push_layer(&vars);
    }
    ctx.push_layer(base);
    ctx
}

/// Resolve every enabled target once (variables, inherited headers/auth,
/// OAuth2 token) and read the data file, like the app does; requests using
/// dynamic variables (`{{$uuid}}`…), data file columns or captured values are
/// rendered again for every iteration. Returns warnings too.
async fn load_plan(
    ws: &Workspace,
    test: &LoadTest,
    (above, base): (Vec<Variable>, Vec<Variable>),
    settings: &Settings,
    outside_files: bool,
) -> Result<(Plan, Vec<String>), String> {
    let overrides =
        RequestSettings { timeout_ms: test.timeout_ms, http_version: test.http_version, ..Default::default() };
    let options = settings.request_options(&overrides).map_err(|e| e.message)?;
    let client = Client::new();
    let tokens = TokenCache::in_memory();
    let (rows, columns): (Vec<DataRow>, Vec<String>) =
        match test.data_file.as_deref().map(str::trim).filter(|f| !f.is_empty()) {
            Some(file) => {
                let (data, _) = runner::read_data_file(file, ws.root(), outside_files)?;
                let text = |v: &serde_json::Value| match v {
                    serde_json::Value::String(s) => s.clone(),
                    serde_json::Value::Null => String::new(),
                    other => other.to_string(),
                };
                let rows = data
                    .rows
                    .iter()
                    .map(|row| row.iter().map(|(k, v)| (k.clone(), text(v))).collect::<Vec<_>>().into())
                    .collect();
                (rows, data.columns)
            }
            None => (Vec::new(), Vec::new()),
        };
    let captured = test.targets.iter().flat_map(|t| &t.captures).map(|c| c.variable.trim().to_string());
    let names: Vec<String> = columns.into_iter().chain(captured).filter(|n| !n.is_empty()).collect();
    let (above, base) = (Arc::new(above), Arc::new(base));
    let mut targets = Vec::new();
    let mut warnings = Vec::new();
    for target in test.targets.iter().filter(|t| t.enabled && t.weight > 0) {
        let request = ws
            .read_request(&target.request)
            .map_err(|e| format!("request '{}' can't be loaded: {}", target.request, e.message))?;
        if request.kind != RequestKind::Http {
            return Err(format!("'{}' is not an HTTP request: only HTTP requests can be load tested", request.name));
        }
        let folders = ws.ancestors(&target.request);
        let inherit = Inheritance { workspace: ws.meta(), folders: &folders, base_dir: ws.root(), outside_files };
        let problem = |message: String| format!("{}: {message}", request.name);
        // Checked with every user variable defined: a URL may use them.
        let probe = user_context(&above, &UserVars::probe(&names), &base);
        let mut resolved = resolve(&request, &inherit, &probe).map_err(|e| problem(e.message))?;
        check_url_variables(&resolved).map_err(|e| problem(e.message))?;
        if resolved.challenge.is_some() {
            return Err(problem(zorvik_workspace::signing::LOAD_TEST_CHALLENGE.into()));
        }
        let signed = zorvik_workspace::signing::signs_each_send(zorvik_workspace::resolve::effective_auth(
            &request.auth,
            &inherit,
        ));
        if !resolved.unresolved.is_empty() {
            warnings.push(format!("{}: undefined variables: {}", request.name, resolved.unresolved.join(", ")));
        }
        let token = match resolved.oauth2.clone() {
            Some(config) => {
                let token = ensure_token(&client, &options, &config, &tokens, &ws.local_key())
                    .await
                    .map_err(|e| problem(e.message))?
                    .access_token;
                apply_token(&mut resolved, &token);
                Some(token)
            }
            None => None,
        };
        let dynamic = signed || serde_json::to_string(&request).is_ok_and(|json| json.contains("{{$"));
        let name = request.name.clone();
        let (meta, base_dir) = (ws.meta().clone(), ws.root().to_path_buf());
        let (above, base) = (above.clone(), base.clone());
        // Without user variables (no data file, nothing captured) the context is built once.
        let plain = user_context(&above, &UserVars::default(), &base);
        let render: Render = Arc::new(move |user: &UserVars| {
            let inherit = Inheritance { workspace: &meta, folders: &folders, base_dir: &base_dir, outside_files };
            let vars = if user.is_empty() { None } else { Some(user_context(&above, user, &base)) };
            let mut resolved = resolve(&request, &inherit, vars.as_ref().unwrap_or(&plain)).map_err(|e| e.message)?;
            if let Some(token) = &token {
                apply_token(&mut resolved, token);
            }
            Ok(resolved.request)
        });
        let source = RequestSource::from_render(render, &names, dynamic).map_err(|e| format!("{name}: {e}"))?;
        targets.push(PlanTarget {
            name,
            request: target.request.clone(),
            source,
            weight: target.weight,
            captures: target.captures.clone(),
        });
    }
    let plan = Plan {
        targets,
        model: test.model,
        stages: test.stages.clone(),
        think_time: Duration::from_millis(test.think_time_ms),
        max_in_flight: test.max_in_flight,
        keep_alive: test.keep_alive,
        options,
        thresholds: test.thresholds.clone(),
        rows,
    };
    zorvik_load::validate(&plan)?;
    Ok((plan, warnings))
}

async fn load(args: LoadArgs) -> ExitCode {
    let color = std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none();
    let paint = |code: &str, s: &str| if color { format!("\x1b[{code}m{s}\x1b[0m") } else { s.to_string() };
    let ws = match Workspace::open(&args.workspace) {
        Ok(ws) => ws,
        Err(e) => return fail(e.message),
    };
    let test = match find_load_test(&ws, &args.test) {
        Ok(test) => test,
        Err(e) => return fail(e),
    };
    let vars = match load_variables(&ws, args.env.as_deref(), &args.vars) {
        Ok(vars) => vars,
        Err(e) => return fail(e),
    };
    let mut settings = Settings::default();
    if args.insecure {
        settings.request.verify_tls = false;
    }
    let (plan, warnings) = match load_plan(&ws, &test, vars, &settings, args.allow_outside_files).await {
        Ok(found) => found,
        Err(e) => return fail(printable(&e)),
    };
    for warning in &warnings {
        eprintln!("{} {}", paint("33", "warning:"), printable(warning));
    }

    let peak = test.stages.iter().map(|s| s.target).max().unwrap_or(0);
    let shape = match test.model {
        LoadModel::VirtualUsers => format!("up to {peak} virtual users"),
        LoadModel::ArrivalRate => format!("up to {peak} requests/s"),
    };
    let requests = plan.targets.len();
    out(&format!(
        "Load test \"{}\": {shape}, {} s, {requests} request{}",
        printable(&test.name),
        test.duration_secs(),
        if requests == 1 { "" } else { "s" }
    ));

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let run = match zorvik_load::start(
        plan,
        Arc::new(move |event| {
            let _ = tx.send(event);
        }),
    ) {
        Ok(run) => run,
        Err(e) => return fail(printable(&e)),
    };
    let active_label = if test.model == LoadModel::VirtualUsers { "users" } else { "in flight" };
    // Ctrl+C presses, from one listener for the whole run: a fresh `ctrl_c()` per
    // event would miss a press between two events (and a listener that fails to
    // start must not count as one).
    let (interrupt_tx, mut interrupts) = tokio::sync::mpsc::unbounded_channel();
    tokio::spawn(async move { while tokio::signal::ctrl_c().await.is_ok() && interrupt_tx.send(()).is_ok() {} });
    let mut stopping = false;
    let summary = loop {
        tokio::select! {
            event = rx.recv() => match event {
                // One line per finished second of the run.
                Some(LoadEvent::Snapshot { snapshot }) if !args.quiet => {
                    for point in &snapshot.points {
                        let mut line = format!(
                            "{:>6}  {:>9} req/s  p95 {:>10}  p99 {:>10}  errors {:<6}  {active_label} {}",
                            format!("{}s", point.second + 1),
                            fixed1(point.rps),
                            ms(point.p95),
                            ms(point.p99),
                            point.errors,
                            point.active
                        );
                        if snapshot.totals.dropped > 0 {
                            line.push_str(&format!("  dropped so far {}", thousands(snapshot.totals.dropped)));
                        }
                        out(&paint("2", &line));
                    }
                }
                Some(LoadEvent::Snapshot { .. }) => {}
                Some(LoadEvent::Finished { summary }) => break summary,
                None => return fail("the load generator stopped without a result".into()),
            },
            Some(()) = interrupts.recv() => {
                if stopping {
                    eprintln!("Quit before the run finished.");
                    return ExitCode::from(2);
                }
                stopping = true;
                eprintln!("Stopping (waiting for requests in flight; Ctrl+C again to quit now)…");
                run.stop();
            }
        }
    };

    print_load_summary(&test.name, &summary, &paint);
    let mut code = ExitCode::from(load_exit_code(&summary));
    if let Some(path) = &args.json {
        let data = serde_json::to_vec_pretty(&summary).unwrap_or_default();
        if let Err(e) = std::fs::write(path, data) {
            eprintln!("error: could not save {}: {e}", path.display());
            code = ExitCode::from(2);
        }
    }
    if let Some(path) = &args.html
        && let Err(e) = std::fs::write(path, zorvik_load::html_report(&test.name, &summary))
    {
        eprintln!("error: could not save {}: {e}", path.display());
        code = ExitCode::from(2);
    }
    code
}

/// `0` every threshold passed, `1` a threshold failed, `2` the run failed (no usable result).
fn load_exit_code(summary: &Summary) -> u8 {
    match (&summary.error, summary.passed) {
        (Some(_), _) => 2,
        (None, true) => 0,
        (None, false) => 1,
    }
}

fn print_load_summary(name: &str, s: &Summary, paint: &dyn Fn(&str, &str) -> String) {
    let t = &s.totals;
    let l = &t.latency;
    out("");
    let mut duration = format!("{} s", fixed1(s.duration_ms as f64 / 1000.0));
    if s.stopped_early {
        duration.push_str(" (stopped early)");
    }
    out(&format!("\"{}\" ran {duration}", printable(name)));
    if let Some(error) = &s.error {
        out(&paint("31", &format!("  error: {}", printable(error))));
    }
    out(&format!("  Requests   {} ({} req/s)", thousands(t.requests), fixed1(t.rps)));
    out(&format!("  Errors     {} ({} %)", thousands(t.errors), fixed2(t.error_rate)));
    if t.dropped > 0 {
        out(&format!("  Dropped    {} (in-flight limit reached)", thousands(t.dropped)));
    }
    out(&format!(
        "  Latency    min {}  avg {}  p50 {}  p90 {}  p95 {}  p99 {}  p99.9 {}  max {}",
        ms(l.min),
        ms(l.avg),
        ms(l.p50),
        ms(l.p90),
        ms(l.p95),
        ms(l.p99),
        ms(l.p999),
        ms(l.max)
    ));
    let tm = &t.timing;
    if tm.ttfb.count > 0 {
        out(&format!("  First byte {}  (request sent to first byte: server + network)", phase(&tm.ttfb)));
    }
    if tm.connect.count > 0 {
        let share = if t.requests == 0 { 0.0 } else { t.connections as f64 * 100.0 / t.requests as f64 };
        out(&format!("  Connect    {}  (new connections, {} % of requests)", phase(&tm.connect), fixed2(share)));
    }
    if tm.server.count > 0 {
        out(&format!("  Server     {}  (reported in Server-Timing)", phase(&tm.server)));
    }
    if t.capture_misses > 0 {
        out(&paint(
            "33",
            &format!(
                "  Captures   {} missed (found nothing; the variable kept its value)",
                thousands(t.capture_misses)
            ),
        ));
    }
    let codes: Vec<String> = t.status_codes.iter().map(|(code, n)| format!("{code} × {}", thousands(*n))).collect();
    if !codes.is_empty() {
        out(&format!("  Status     {}", codes.join(", ")));
    }
    let kinds: Vec<String> = t.error_kinds.iter().map(|(kind, n)| format!("{kind} × {}", thousands(*n))).collect();
    if !kinds.is_empty() {
        out(&format!("  Network    {}", kinds.join(", ")));
    }
    out(&format!(
        "  Data       {} received, {} sent, {} connection{}",
        bytes(t.bytes_in),
        bytes(t.bytes_out),
        thousands(t.connections),
        if t.connections == 1 { "" } else { "s" }
    ));
    if let Some(cpu) = s.peak_cpu_percent {
        out(&format!("  CPU        {cpu:.0} % peak (100 % = one core)"));
    }
    let misses = s.targets.iter().any(|t| t.metrics.capture_misses > 0);
    if s.targets.len() > 1 || misses {
        let width = s.targets.iter().map(|t| printable(&t.name).chars().count()).max().unwrap_or(0).clamp(7, 40);
        out("");
        let mut head = format!(
            "  {:<width$}  {:>10}  {:>9}  {:>8}  {:>10}  {:>10}  {:>13}",
            "Request", "Requests", "req/s", "Errors", "p95", "p99", "1st byte p95"
        );
        if misses {
            head.push_str(&format!("  {:>8}", "Missed"));
        }
        out(&head);
        for target in &s.targets {
            let m = &target.metrics;
            let name: String = printable(&target.name).chars().take(width).collect();
            let ttfb = if m.timing.ttfb.count > 0 { ms(m.timing.ttfb.p95) } else { "–".into() };
            let mut line = format!(
                "  {name:<width$}  {:>10}  {:>9}  {:>6} %  {:>10}  {:>10}  {:>13}",
                thousands(m.requests),
                fixed1(m.rps),
                fixed2(m.error_rate),
                ms(m.latency.p95),
                ms(m.latency.p99),
                ttfb
            );
            if misses {
                line.push_str(&format!("  {:>8}", thousands(m.capture_misses)));
            }
            out(&line);
        }
    }
    if !s.thresholds.is_empty() {
        out("");
        out("  Thresholds");
        for th in &s.thresholds {
            let actual = th.actual.map_or_else(|| "no data".to_string(), fixed2);
            let (mark, code) = if th.passed { ("✓", "32") } else { ("✗", "31") };
            out(&format!(
                "  {} {}  {}",
                paint(code, mark),
                printable(&th.label),
                paint("2", &format!("(actual {actual})"))
            ));
        }
    }
    out("");
    out(&if s.passed { paint("32", "PASSED") } else { paint("31", "FAILED") });
}

fn thousands(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn fixed1(v: f64) -> String {
    format!("{v:.1}")
}

/// `p50 1.20 ms  p95 3.40 ms  p99 8.00 ms` of a timing phase.
fn phase(p: &PhaseSummary) -> String {
    format!("p50 {}  p95 {}  p99 {}", ms(p.p50), ms(p.p95), ms(p.p99))
}

fn fixed2(v: f64) -> String {
    format!("{v:.2}")
}

fn ms(v: f64) -> String {
    if v >= 100.0 { format!("{v:.0} ms") } else { format!("{v:.2} ms") }
}

fn bytes(n: u64) -> String {
    let units = ["B", "KB", "MB", "GB", "TB"];
    let mut v = n as f64;
    let mut unit = 0;
    while v >= 1000.0 && unit < units.len() - 1 {
        v /= 1000.0;
        unit += 1;
    }
    if unit == 0 { format!("{n} B") } else { format!("{v:.1} {}", units[unit]) }
}

// ---- serve ------------------------------------------------------------------------------

fn kind_label(kind: ServerKind) -> &'static str {
    match kind {
        ServerKind::Http => "Mock API",
        ServerKind::Websocket => "WebSocket server",
        ServerKind::Sse => "Event stream server",
        ServerKind::Tcp => "TCP server",
        ServerKind::Udp => "UDP server",
        ServerKind::Dns => "DNS server",
        ServerKind::TcpProxy => "TCP relay",
    }
}

/// A saved server by id (file name) or name, ignoring case.
fn find_server(ws: &Workspace, wanted: &str) -> Result<(String, Server), String> {
    let nodes = ws.list_servers().map_err(|e| e.message)?;
    let node = nodes
        .iter()
        .find(|n| n.id == wanted)
        .or_else(|| nodes.iter().find(|n| n.id.eq_ignore_ascii_case(wanted) || n.name.eq_ignore_ascii_case(wanted)))
        .ok_or_else(|| {
            let names: Vec<&str> = nodes.iter().map(|n| n.name.as_str()).collect();
            if names.is_empty() {
                format!("server '{wanted}' not found (the workspace has no servers)")
            } else {
                format!("server '{wanted}' not found (servers: {})", names.join(", "))
            }
        })?;
    if let Some(e) = &node.error {
        return Err(format!("server '{}' could not be read: {e}", node.name));
    }
    let server = ws.read_server(&node.id).map_err(|e| e.message)?;
    Ok((node.id.clone(), server))
}

/// Write a line, ignoring a closed stdout (`zorvik serve … | head`).
fn out(line: &str) {
    let _ = writeln!(std::io::stdout().lock(), "{line}");
}

/// Text from clients (or from workspace files) for the terminal: control
/// characters are shown escaped, so escape sequences in the traffic can't
/// restyle or rewrite the terminal (or set its title or clipboard).
fn printable(s: &str) -> String {
    if !s.chars().any(|c| c.is_control() && c != '\t') {
        return s.to_string();
    }
    s.chars()
        .map(|c| if c.is_control() && c != '\t' { c.escape_default().to_string() } else { c.to_string() })
        .collect()
}

async fn serve(args: ServeArgs) -> ExitCode {
    let color = !args.json && std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none();
    let ws = match Workspace::open(&args.workspace) {
        Ok(ws) => ws,
        Err(e) => return fail(e.message),
    };
    let (_, mut server) = match find_server(&ws, &args.server) {
        Ok(found) => found,
        Err(e) => return fail(e),
    };
    let vars = match variables(&ws, args.env.as_deref(), &args.vars) {
        Ok(vars) => vars,
        Err(e) => return fail(e),
    };
    if let Some(port) = args.port {
        server.port = port;
    }
    if let Some(host) = &args.host {
        server.host = host.clone();
    }
    let mut settings = Settings::default();
    if args.insecure {
        settings.request.verify_tls = false;
    }
    let request_options = match settings.request_options(&Default::default()) {
        Ok(o) => o,
        Err(e) => return fail(e.message),
    };
    let options = StartOptions { base_dir: ws.root().to_path_buf(), client: Arc::new(Client::new()), request_options };

    let (stopped_tx, mut stopped_rx) = tokio::sync::mpsc::unbounded_channel();
    let started = Instant::now();
    let json = args.json;
    let reporter = Reporter::new(move |event| match event {
        ServerEvent::Traffic { entry } if json => {
            out(&serde_json::json!({ "type": "traffic", "entry": entry }).to_string());
        }
        ServerEvent::Traffic { entry } => out(&traffic_line(&entry, started, color)),
        ServerEvent::Stopped { error } => {
            let _ = stopped_tx.send(error);
        }
        ServerEvent::Stats { .. } => {}
    });
    let (name, kind) = (server.name.clone(), server.kind);
    let routes: Vec<String> = match kind {
        ServerKind::Http => server
            .http
            .routes
            .iter()
            .filter(|r| r.enabled)
            .map(|r| {
                let method = if r.method.trim() == "*" { "ANY".to_string() } else { r.method.to_ascii_uppercase() };
                format!("  {:<7} {}  → {}", printable(&method), printable(&r.path), r.status)
            })
            .collect(),
        _ => Vec::new(),
    };
    let running = match zorvik_servers::start(server, vars, options, reporter).await {
        Ok(r) => r,
        Err(e) => return fail(e.message),
    };
    if json {
        out(&serde_json::json!({ "type": "started", "name": name, "kind": kind, "url": running.url }).to_string());
    } else {
        out(&format!("{} \"{}\" is running at {} (Ctrl+C to stop)", kind_label(kind), printable(&name), running.url));
        for route in &routes {
            out(route);
        }
    }

    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            running.stop();
            // Give connections a moment to close (and the log its last lines).
            let _ = tokio::time::timeout(std::time::Duration::from_secs(3), stopped_rx.recv()).await;
            if json {
                out(&serde_json::json!({ "type": "stopped" }).to_string());
            } else {
                out("Stopped.");
            }
            ExitCode::SUCCESS
        }
        error = stopped_rx.recv() => match error.flatten() {
            Some(e) => fail(format!("the server stopped: {e}")),
            None => ExitCode::SUCCESS,
        },
    }
}

/// One traffic entry as a line: time since start, connection, what happened.
fn traffic_line(e: &TrafficEntry, started: Instant, color: bool) -> String {
    let paint = |code: &str, s: &str| if color { format!("\x1b[{code}m{s}\x1b[0m") } else { s.to_string() };
    let time = paint("2", &format!("{:>9.3}s", started.elapsed().as_secs_f64()));
    // "#3 connected from …" names the connection itself.
    let conn = match e.conn {
        Some(c) if e.kind != TrafficKind::Open => format!("#{c} "),
        _ => String::new(),
    };
    let body = match e.kind {
        TrafficKind::Http => {
            let status = e.http.as_ref().map_or(0, |h| h.status);
            let code = match status {
                0 | 500.. => "31",
                400..=499 => "33",
                _ => "32",
            };
            paint(code, &printable(&e.summary))
        }
        TrafficKind::Data => {
            let arrow = match e.direction {
                Some(TrafficDirection::In) => "in ",
                Some(TrafficDirection::Out) => "out",
                Some(TrafficDirection::ToTarget) => "→ target",
                Some(TrafficDirection::FromTarget) => "← target",
                None => "",
            };
            let payload = match (&e.text, &e.base64) {
                (Some(text), _) => {
                    let flat = printable(text);
                    match flat.char_indices().nth(160) {
                        Some((i, _)) => format!("{}…", &flat[..i]),
                        None => flat,
                    }
                }
                (None, Some(_)) => format!("({} bytes of binary data)", e.size),
                (None, None) => String::new(),
            };
            let label = if e.summary.is_empty() { String::new() } else { format!("[{}] ", printable(&e.summary)) };
            format!("{arrow} {label}{payload}")
        }
        TrafficKind::Error => paint("31", &printable(&e.summary)),
        TrafficKind::Open | TrafficKind::Close | TrafficKind::Info => paint("2", &printable(&e.summary)),
        TrafficKind::Dns => printable(&e.summary),
    };
    format!("{time}  {conn}{body}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_exit_codes() {
        let summary = |passed: bool, error: Option<&str>| Summary {
            started_at: 0.0,
            duration_ms: 1000,
            totals: Default::default(),
            targets: Vec::new(),
            points: Vec::new(),
            thresholds: Vec::new(),
            passed,
            stopped_early: false,
            error: error.map(str::to_string),
            peak_cpu_percent: None,
        };
        assert_eq!(load_exit_code(&summary(true, None)), 0);
        assert_eq!(load_exit_code(&summary(false, None)), 1);
        // A run that could not start or broke off is an error, not a failed threshold.
        assert_eq!(load_exit_code(&summary(false, Some("Could not start the load generator"))), 2);
    }

    #[test]
    fn run_exit_codes() {
        let summary = |passed: bool, stopped: bool| RunSummary { passed, stopped, ..Default::default() };
        assert_eq!(run_exit_code(&summary(true, false)), 0);
        assert_eq!(run_exit_code(&summary(false, false)), 1);
        // Ctrl+C: not every request ran.
        assert_eq!(run_exit_code(&summary(true, true)), 2);
    }
}
