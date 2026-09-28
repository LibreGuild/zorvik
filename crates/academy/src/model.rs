//! The course as authored: units (`course/<nn>-<unit>/unit.yaml`) holding lessons
//! (`<nn>-<lesson>.md`: YAML front matter, then the reading in Markdown). See `docs/academy.md`.

use indexmap::IndexMap;
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Clone)]
pub struct Course {
    pub units: Vec<Unit>,
}

impl Course {
    pub fn lessons(&self) -> impl Iterator<Item = &Lesson> {
        self.units.iter().flat_map(|u| u.lessons.iter())
    }

    pub fn lesson(&self, id: &str) -> Option<&Lesson> {
        self.lessons().find(|l| l.id == id)
    }

    pub fn unit(&self, id: &str) -> Option<&Unit> {
        self.units.iter().find(|u| u.id == id)
    }

    /// The unit a lesson belongs to.
    pub fn unit_of(&self, lesson: &str) -> Option<&Unit> {
        self.units.iter().find(|u| u.lessons.iter().any(|l| l.id == lesson))
    }
}

/// `unit.yaml`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UnitMeta {
    pub id: String,
    pub title: String,
    pub summary: String,
    /// Accent color of the unit (`#rrggbb`).
    pub color: String,
    /// Illustration name (`app/src/assets/academy/unit-<image>.webp`).
    pub image: String,
    pub badge: BadgeDef,
    /// The final project: finishing it graduates the learner.
    #[serde(default)]
    pub capstone: bool,
}

#[derive(Debug, Clone)]
pub struct Unit {
    pub id: String,
    pub title: String,
    pub summary: String,
    pub color: String,
    pub image: String,
    pub badge: BadgeDef,
    pub capstone: bool,
    pub lessons: Vec<Lesson>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BadgeDef {
    pub id: String,
    pub name: String,
    pub description: String,
}

/// A lesson file's front matter.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LessonMeta {
    pub id: String,
    pub title: String,
    /// One sentence: the idea of the lesson.
    pub summary: String,
    /// Reading time.
    pub minutes: u32,
    /// The Zorvik version that added the lesson (`0.2.0`), for lessons added after 0.1.2:
    /// learners who started before it see them as new.
    #[serde(default)]
    pub added: Option<String>,
    #[serde(default)]
    pub lab: Option<Lab>,
    #[serde(default)]
    pub quiz: Vec<Question>,
}

#[derive(Debug, Clone)]
pub struct Lesson {
    pub id: String,
    pub unit: String,
    pub title: String,
    pub summary: String,
    pub minutes: u32,
    pub added: Option<String>,
    /// The reading, in Markdown.
    pub body: String,
    pub lab: Option<Lab>,
    pub quiz: Vec<Question>,
}

/// A hands-on exercise: servers started for it, variables in the "Lab" environment, and
/// steps checked against what the learner does in the app.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Lab {
    pub title: String,
    /// One sentence: what the learner builds.
    pub goal: String,
    pub minutes: u32,
    /// Servers by id (a `{{id}}` variable holds each one's address). Each is a saved
    /// server (`servers/*.yaml` format); the lab saves it as "Lab · <name>" and starts it.
    #[serde(default)]
    pub servers: IndexMap<String, Value>,
    #[serde(default)]
    pub playground: Playground,
    /// More variables for the "Lab" environment.
    #[serde(default)]
    pub vars: IndexMap<String, String>,
    /// Files written into the workspace when the lab starts (path → contents).
    #[serde(default)]
    pub files: IndexMap<String, String>,
    pub steps: Vec<Step>,
    /// Names of the `{{secret.<name>}}` values the lab uses: random each time it starts,
    /// so answers can't be copied from somewhere else.
    #[serde(skip)]
    pub secrets: Vec<String>,
}

/// Built-in practice servers: `{{playground}}` (HTTP endpoints for status codes, auth,
/// cookies, redirects, GraphQL, SSE, WebSocket, OAuth 2.0), `{{playgroundTls}}` (the same
/// over HTTPS with a certificate from a private authority, `lab-ca.pem`) and `{{grpc}}`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Playground {
    pub http: bool,
    pub tls: bool,
    pub grpc: bool,
}

impl Playground {
    pub fn any(self) -> bool {
        self.http || self.tls || self.grpc
    }
}

impl<'de> Deserialize<'de> for Playground {
    /// `playground: true` (HTTP) or `playground: { http, tls, grpc }`.
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Parts {
            #[serde(default)]
            http: bool,
            #[serde(default)]
            tls: bool,
            #[serde(default)]
            grpc: bool,
        }
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Form {
            On(bool),
            Parts(Parts),
        }
        Ok(match Form::deserialize(d)? {
            Form::On(on) => Playground { http: on, ..Default::default() },
            Form::Parts(p) => Playground { http: p.http, tls: p.tls, grpc: p.grpc },
        })
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Step {
    /// What to do, in Markdown (`{{name}}` shows as a variable).
    pub text: String,
    /// Up to three, from a nudge to the exact clicks.
    #[serde(default)]
    pub hints: Vec<String>,
    pub check: Check,
    /// What "Do it for me" runs, and what the course tests run to prove the step can pass.
    pub solution: Vec<Action>,
}

/// How a step is recognized as done. Patterns follow [`crate::matcher`].
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub enum Check {
    /// An HTTP request a lab server received: `{server, method, path, query, headers, body, json, status}`.
    Request(TrafficCheck),
    /// A message, connection or DNS query a lab server saw: `{server, kind, direction, text}`.
    Message(TrafficCheck),
    /// An HTTP request sent from Zorvik and its answer (see `docs/academy.md`).
    Send(Value),
    /// Any app action: `{method, params, result, ok}`.
    Call(Value),
    /// Something saved in the workspace: `{request | environment | server | loadTest | folder | workspace: pattern}`.
    Saved(Value),
    /// A collection run that finished during the lab (its summary).
    Run(Value),
    /// A load test run that finished during the lab (its record).
    Load(Value),
    /// The lab calls a server the learner runs.
    Probe(Probe),
    /// The learner types an answer (any of the patterns).
    Answer(Answer),
    All(Vec<Check>),
    Any(Vec<Check>),
}

impl Check {
    /// Whether the learner types an answer for this step.
    pub fn wants_answer(&self) -> bool {
        match self {
            Check::Answer(_) => true,
            Check::All(list) => list.iter().any(Check::wants_answer),
            _ => false,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct TrafficCheck {
    /// Lab server id.
    pub server: String,
    #[serde(flatten)]
    pub pattern: serde_json::Map<String, Value>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum Answer {
    One(String),
    AnyOf(Vec<String>),
}

impl Answer {
    pub fn patterns(&self) -> Vec<&str> {
        match self {
            Answer::One(p) => vec![p.as_str()],
            Answer::AnyOf(list) => list.iter().map(String::as_str).collect(),
        }
    }
}

/// A call the lab makes to a server the learner runs in the workspace.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Probe {
    /// `http`, `tcp`, `udp` or `mcp`.
    pub kind: ProbeKind,
    /// Only servers whose name matches (default: any running one of that kind).
    #[serde(default)]
    pub name: Option<String>,
    /// HTTP method (default GET); MCP: the JSON-RPC method (default `tools/list`).
    #[serde(default)]
    pub method: Option<String>,
    /// HTTP path and query (default `/`).
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub headers: IndexMap<String, String>,
    /// HTTP body, the text sent over TCP/UDP, or the MCP method's parameters (JSON).
    #[serde(default)]
    pub body: Option<String>,
    /// Pattern on the answer: `{status, headers, body, json}` for HTTP, `{result}` or `{error}`
    /// for MCP, `{text}` otherwise.
    pub expect: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProbeKind {
    Http,
    Tcp,
    Udp,
    /// An MCP server: connected to over Streamable HTTP, then asked one method.
    Mcp,
}

/// One thing "Do it for me" (and the course tests) do.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub enum Action {
    /// Send an HTTP request (a saved-request object; `name` and `seq` may be left out).
    Send(Value),
    /// Save a request at the top of the collection.
    Save(Value),
    /// Any app method: `{method, params}`.
    Call(CallAction),
    /// Type this answer.
    Answer(String),
    /// Wait this many milliseconds (for something that finishes in the background).
    Wait(u64),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CallAction {
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Question {
    pub question: String,
    pub options: Vec<String>,
    /// Index of the right option.
    pub answer: usize,
    /// Why, shown after answering.
    pub explain: String,
}
