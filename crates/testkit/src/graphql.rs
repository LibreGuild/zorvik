//! A tiny GraphQL endpoint for testing GraphQL requests (`/graphql`, and
//! `/graphql-auth` which wants a bearer token). Hand-written: a small parser for
//! executable documents (operations, variables, fragments, aliases) and an
//! executor that projects prepared JSON values onto the selection sets, so the
//! standard introspection query works like on a real server.
//!
//! ```graphql
//! type Query {
//!   hello(name: String = "world"): String!
//!   user(id: ID!): User
//!   users(first: Int): [User!]!
//!   greeting: String @deprecated(reason: "Use `hello`.")
//! }
//! type Mutation { echo(text: String!): String! }
//! type User { id: ID!, name: String!, email: String, role: Role!, posts: [Post!]! }
//! type Post { id: ID!, title: String! }
//! enum Role { ADMIN, MEMBER, GUEST @deprecated(reason: "No guests any more.") }
//! ```
//!
//! Requests: `POST {"query", "variables", "operationName"}` or `GET ?query=&variables=&operationName=`.
//! Errors are GraphQL-style (`{"errors": [{"message", "locations"}]}`, HTTP 400).
//! `?legacy=1` answers like an older server: no `specifiedByURL` or `isRepeatable`
//! in introspection (querying them is an error).

use std::collections::HashMap;
use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, Method, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use serde_json::{Map, Value, json};

use crate::{AppState, url_query};

/// Bearer token `/graphql-auth` accepts, besides access tokens from `/oauth/token`.
pub const GRAPHQL_TOKEN: &str = "graphql-token";

pub(crate) async fn endpoint(method: Method, uri: Uri, body: Bytes) -> Response {
    respond(&method, &uri, &body)
}

pub(crate) async fn endpoint_auth(
    State(state): State<Arc<AppState>>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or_default();
    let issued = state.tokens.lock().unwrap().iter().any(|t| t == token && t.starts_with("access-"));
    if token != GRAPHQL_TOKEN && !issued {
        let errors = json!({ "errors": [{ "message": "Not authenticated" }] });
        return (StatusCode::UNAUTHORIZED, [(header::WWW_AUTHENTICATE, "Bearer")], axum::Json(errors)).into_response();
    }
    respond(&method, &uri, &body)
}

fn respond(method: &Method, uri: &Uri, body: &[u8]) -> Response {
    let query: HashMap<String, String> = url_query(uri.query().unwrap_or_default()).into_iter().collect();
    let legacy = query.get("legacy").is_some_and(|v| v == "1" || v == "true");
    let request = if method == Method::GET {
        let fields = ["query", "variables", "operationName"].into_iter();
        Value::Object(fields.filter_map(|k| Some((k.to_string(), Value::String(query.get(k)?.clone())))).collect())
    } else {
        match serde_json::from_slice(body) {
            Ok(v) => v,
            Err(_) => Value::Null,
        }
    };
    let (status, result) = execute_request(&request, legacy);
    (status, axum::Json(result)).into_response()
}

/// Run one GraphQL request (`{"query", "variables", "operationName"}`).
pub(crate) fn execute_request(request: &Value, legacy: bool) -> (StatusCode, Value) {
    let failed = |message: &str| (StatusCode::BAD_REQUEST, json!({ "errors": [error(message, None)] }));
    let Some(query) = request.get("query").and_then(Value::as_str) else {
        return failed("Must provide query string.");
    };
    let variables = match request.get("variables") {
        Some(Value::Object(m)) => m.clone(),
        Some(Value::String(s)) if !s.trim().is_empty() => match serde_json::from_str(s) {
            Ok(Value::Object(m)) => m,
            _ => return failed("Variables are invalid JSON."),
        },
        None | Some(Value::Null | Value::String(_)) => Map::new(),
        Some(_) => return failed("Variables must be an object."),
    };
    let operation_name = request.get("operationName").and_then(Value::as_str).filter(|s| !s.is_empty());
    match run(query, variables, operation_name, legacy) {
        Ok(data) => (StatusCode::OK, json!({ "data": data })),
        Err(errors) => (StatusCode::BAD_REQUEST, json!({ "errors": errors })),
    }
}

fn run(
    query: &str,
    variables: Map<String, Value>,
    operation_name: Option<&str>,
    legacy: bool,
) -> Result<Value, Vec<Value>> {
    let doc = Parser { toks: lex(query).map_err(|e| vec![e])?, pos: 0 }.document().map_err(|e| vec![e])?;
    let op = match operation_name {
        Some(name) => doc
            .operations
            .iter()
            .find(|o| o.name.as_deref() == Some(name))
            .ok_or_else(|| vec![error(&format!("Unknown operation named \"{name}\"."), None)])?,
        None if doc.operations.len() == 1 => &doc.operations[0],
        None => return Err(vec![error("Must provide operation name if query contains multiple operations.", None)]),
    };
    let vars = coerce_variables(op, variables)?;
    let mut exec = Exec { doc: &doc, vars, schema: schema(legacy), errors: Vec::new(), spreading: Vec::new() };
    let data = exec.root(op);
    if exec.errors.is_empty() {
        return Ok(data);
    }
    // The same mistake is reported once, not once per object it was selected on.
    let mut errors: Vec<Value> = Vec::new();
    for e in exec.errors {
        if !errors.contains(&e) {
            errors.push(e);
        }
    }
    Err(errors)
}

fn error(message: &str, at: Option<(usize, usize)>) -> Value {
    match at {
        Some((line, column)) => json!({ "message": message, "locations": [{ "line": line, "column": column }] }),
        None => json!({ "message": message }),
    }
}

// ---- parsing ----------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Punct(char),
    Spread,
    Name(String),
    Str(String),
    Int(i64),
    Float(f64),
    End,
}

#[derive(Debug, Clone)]
struct Token {
    tok: Tok,
    line: usize,
    column: usize,
}

fn lex(src: &str) -> Result<Vec<Token>, Value> {
    let chars: Vec<char> = src.chars().collect();
    let (mut i, mut line, mut line_start) = (0, 1, 0);
    let mut out = Vec::new();
    while i < chars.len() {
        let c = chars[i];
        let (at_line, column) = (line, i - line_start + 1);
        let tok = match c {
            '\n' => {
                i += 1;
                line += 1;
                line_start = i;
                continue;
            }
            ' ' | '\t' | '\r' | ',' | '\u{feff}' => {
                i += 1;
                continue;
            }
            '#' => {
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
                continue;
            }
            '.' if chars[i..].starts_with(&['.', '.', '.']) => {
                i += 3;
                Tok::Spread
            }
            '!' | '$' | '&' | '(' | ')' | ':' | '=' | '@' | '[' | ']' | '{' | '|' | '}' => {
                i += 1;
                Tok::Punct(c)
            }
            '"' if chars[i..].starts_with(&['"', '"', '"']) => {
                let start = i + 3;
                let mut end = start;
                while end < chars.len() && !chars[end..].starts_with(&['"', '"', '"']) {
                    end += if chars[end..].starts_with(&['\\', '"', '"', '"']) { 4 } else { 1 };
                }
                if end >= chars.len() {
                    return Err(error("Syntax Error: Unterminated string.", Some((at_line, column))));
                }
                let text: String = chars[start..end].iter().collect();
                for (k, ch) in chars[i..end].iter().enumerate() {
                    if *ch == '\n' {
                        line += 1;
                        line_start = i + k + 1;
                    }
                }
                i = end + 3;
                Tok::Str(text.replace("\\\"\"\"", "\"\"\"").trim().to_string())
            }
            '"' => {
                let mut text = String::new();
                i += 1;
                loop {
                    match chars.get(i) {
                        None | Some('\n') => {
                            return Err(error("Syntax Error: Unterminated string.", Some((at_line, column))));
                        }
                        Some('"') => break,
                        Some('\\') => {
                            let escaped = match chars.get(i + 1) {
                                Some('n') => '\n',
                                Some('t') => '\t',
                                Some('r') => '\r',
                                Some('b') => '\u{8}',
                                Some('f') => '\u{c}',
                                Some('u') => {
                                    let hex: String = chars.iter().skip(i + 2).take(4).collect();
                                    i += 4;
                                    u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32).unwrap_or('\u{fffd}')
                                }
                                Some(other) => *other,
                                None => '\\',
                            };
                            text.push(escaped);
                            i += 2;
                        }
                        Some(other) => {
                            text.push(*other);
                            i += 1;
                        }
                    }
                }
                i += 1;
                Tok::Str(text)
            }
            c if c == '_' || c.is_ascii_alphabetic() => {
                let start = i;
                while i < chars.len() && (chars[i] == '_' || chars[i].is_ascii_alphanumeric()) {
                    i += 1;
                }
                Tok::Name(chars[start..i].iter().collect())
            }
            c if c == '-' || c.is_ascii_digit() => {
                let start = i;
                i += 1;
                while i < chars.len() && (chars[i].is_ascii_alphanumeric() || matches!(chars[i], '.' | '+' | '-')) {
                    i += 1;
                }
                let text: String = chars[start..i].iter().collect();
                match (text.parse::<i64>(), text.parse::<f64>()) {
                    (Ok(n), _) => Tok::Int(n),
                    (_, Ok(f)) => Tok::Float(f),
                    _ => {
                        return Err(error(
                            &format!("Syntax Error: Invalid number \"{text}\"."),
                            Some((at_line, column)),
                        ));
                    }
                }
            }
            other => {
                return Err(error(
                    &format!("Syntax Error: Unexpected character \"{other}\"."),
                    Some((at_line, column)),
                ));
            }
        };
        out.push(Token { tok, line: at_line, column });
    }
    out.push(Token { tok: Tok::End, line, column: chars.len() - line_start + 1 });
    Ok(out)
}

#[derive(Default)]
struct Document {
    operations: Vec<Operation>,
    fragments: HashMap<String, Fragment>,
}

struct Operation {
    /// `query`, `mutation` or `subscription`.
    kind: String,
    name: Option<String>,
    variables: Vec<VarDef>,
    selection: Vec<Selection>,
}

struct VarDef {
    name: String,
    /// As written, e.g. `[ID!]!`.
    ty: String,
    default: Option<Arg>,
}

struct Fragment {
    on: String,
    selection: Vec<Selection>,
}

enum Selection {
    Field(Field),
    Spread { name: String, line: usize, column: usize },
    Inline { on: Option<String>, selection: Vec<Selection> },
}

struct Field {
    alias: Option<String>,
    name: String,
    args: Vec<(String, Arg)>,
    selection: Vec<Selection>,
    line: usize,
    column: usize,
}

enum Arg {
    Var(String),
    Const(Value),
    List(Vec<Arg>),
    Object(Vec<(String, Arg)>),
}

struct Parser {
    toks: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> &Tok {
        &self.toks[self.pos].tok
    }

    fn here(&self) -> (usize, usize) {
        (self.toks[self.pos].line, self.toks[self.pos].column)
    }

    fn is(&self, c: char) -> bool {
        *self.peek() == Tok::Punct(c)
    }

    fn is_name(&self, name: &str) -> bool {
        matches!(self.peek(), Tok::Name(n) if n == name)
    }

    fn unexpected(&self) -> Value {
        let found = match self.peek() {
            Tok::End => "<EOF>".to_string(),
            Tok::Punct(c) => format!("\"{c}\""),
            Tok::Spread => "\"...\"".to_string(),
            Tok::Name(n) => format!("Name \"{n}\""),
            Tok::Str(s) => format!("String \"{s}\""),
            Tok::Int(n) => format!("Int \"{n}\""),
            Tok::Float(f) => format!("Float \"{f}\""),
        };
        error(&format!("Syntax Error: Unexpected {found}."), Some(self.here()))
    }

    fn expect(&mut self, c: char) -> Result<(), Value> {
        if !self.is(c) {
            return Err(self.unexpected());
        }
        self.pos += 1;
        Ok(())
    }

    fn name(&mut self) -> Result<String, Value> {
        let Tok::Name(n) = self.peek() else { return Err(self.unexpected()) };
        let n = n.clone();
        self.pos += 1;
        Ok(n)
    }

    fn document(mut self) -> Result<Document, Value> {
        let mut doc = Document::default();
        while *self.peek() != Tok::End {
            if self.is('{') {
                let selection = self.selection_set()?;
                doc.operations.push(Operation { kind: "query".into(), name: None, variables: Vec::new(), selection });
            } else if self.is_name("query") || self.is_name("mutation") || self.is_name("subscription") {
                let kind = self.name()?;
                let name = if matches!(self.peek(), Tok::Name(_)) { Some(self.name()?) } else { None };
                let variables = if self.is('(') { self.variable_definitions()? } else { Vec::new() };
                self.directives()?;
                let selection = self.selection_set()?;
                doc.operations.push(Operation { kind, name, variables, selection });
            } else if self.is_name("fragment") {
                self.pos += 1;
                let name = self.name()?;
                if !self.is_name("on") {
                    return Err(self.unexpected());
                }
                self.pos += 1;
                let on = self.name()?;
                self.directives()?;
                let selection = self.selection_set()?;
                doc.fragments.insert(name, Fragment { on, selection });
            } else {
                return Err(self.unexpected());
            }
        }
        if doc.operations.is_empty() {
            return Err(error("Must provide an operation.", None));
        }
        Ok(doc)
    }

    fn variable_definitions(&mut self) -> Result<Vec<VarDef>, Value> {
        self.expect('(')?;
        let mut out = Vec::new();
        while !self.is(')') {
            self.expect('$')?;
            let name = self.name()?;
            self.expect(':')?;
            let ty = self.type_ref()?;
            let default = if self.is('=') {
                self.pos += 1;
                Some(self.value()?)
            } else {
                None
            };
            self.directives()?;
            out.push(VarDef { name, ty, default });
        }
        self.pos += 1;
        Ok(out)
    }

    fn type_ref(&mut self) -> Result<String, Value> {
        let mut ty = if self.is('[') {
            self.pos += 1;
            let inner = self.type_ref()?;
            self.expect(']')?;
            format!("[{inner}]")
        } else {
            self.name()?
        };
        if self.is('!') {
            self.pos += 1;
            ty.push('!');
        }
        Ok(ty)
    }

    /// Directives are accepted and ignored.
    fn directives(&mut self) -> Result<(), Value> {
        while self.is('@') {
            self.pos += 1;
            self.name()?;
            if self.is('(') {
                self.arguments()?;
            }
        }
        Ok(())
    }

    fn selection_set(&mut self) -> Result<Vec<Selection>, Value> {
        self.expect('{')?;
        let mut out = Vec::new();
        loop {
            out.push(self.selection()?);
            if self.is('}') {
                break;
            }
        }
        self.pos += 1;
        Ok(out)
    }

    fn selection(&mut self) -> Result<Selection, Value> {
        let (line, column) = self.here();
        if *self.peek() == Tok::Spread {
            self.pos += 1;
            if matches!(self.peek(), Tok::Name(n) if n != "on") {
                let name = self.name()?;
                self.directives()?;
                return Ok(Selection::Spread { name, line, column });
            }
            let on = if self.is_name("on") {
                self.pos += 1;
                Some(self.name()?)
            } else {
                None
            };
            self.directives()?;
            return Ok(Selection::Inline { on, selection: self.selection_set()? });
        }
        let mut name = self.name()?;
        let mut alias = None;
        if self.is(':') {
            self.pos += 1;
            alias = Some(std::mem::replace(&mut name, self.name()?));
        }
        let args = if self.is('(') { self.arguments()? } else { Vec::new() };
        self.directives()?;
        let selection = if self.is('{') { self.selection_set()? } else { Vec::new() };
        Ok(Selection::Field(Field { alias, name, args, selection, line, column }))
    }

    fn arguments(&mut self) -> Result<Vec<(String, Arg)>, Value> {
        self.expect('(')?;
        let mut out = Vec::new();
        while !self.is(')') {
            let name = self.name()?;
            self.expect(':')?;
            out.push((name, self.value()?));
        }
        self.pos += 1;
        Ok(out)
    }

    fn value(&mut self) -> Result<Arg, Value> {
        let tok = self.peek().clone();
        self.pos += 1;
        Ok(match tok {
            Tok::Punct('$') => Arg::Var(self.name()?),
            Tok::Int(n) => Arg::Const(json!(n)),
            Tok::Float(f) => Arg::Const(json!(f)),
            Tok::Str(s) => Arg::Const(Value::String(s)),
            Tok::Name(n) => Arg::Const(match n.as_str() {
                "true" => json!(true),
                "false" => json!(false),
                "null" => Value::Null,
                _ => Value::String(n),
            }),
            Tok::Punct('[') => {
                let mut items = Vec::new();
                while !self.is(']') {
                    items.push(self.value()?);
                }
                self.pos += 1;
                Arg::List(items)
            }
            Tok::Punct('{') => {
                let mut fields = Vec::new();
                while !self.is('}') {
                    let key = self.name()?;
                    self.expect(':')?;
                    fields.push((key, self.value()?));
                }
                self.pos += 1;
                Arg::Object(fields)
            }
            _ => {
                self.pos -= 1;
                return Err(self.unexpected());
            }
        })
    }
}

// ---- execution --------------------------------------------------------------

fn coerce_variables(op: &Operation, mut given: Map<String, Value>) -> Result<Map<String, Value>, Vec<Value>> {
    let (mut out, mut errors) = (Map::new(), Vec::new());
    for def in &op.variables {
        let required = def.ty.ends_with('!');
        match (given.remove(&def.name), &def.default) {
            (Some(v), _) if !(v.is_null() && required) => {
                out.insert(def.name.clone(), v);
            }
            (_, Some(default)) => {
                out.insert(def.name.clone(), constant(default));
            }
            _ if required => errors.push(error(
                &format!("Variable \"${}\" of required type \"{}\" was not provided.", def.name, def.ty),
                None,
            )),
            _ => {}
        }
    }
    if errors.is_empty() { Ok(out) } else { Err(errors) }
}

/// A default value (no variables in it).
fn constant(arg: &Arg) -> Value {
    match arg {
        Arg::Var(_) => Value::Null,
        Arg::Const(v) => v.clone(),
        Arg::List(items) => Value::Array(items.iter().map(constant).collect()),
        Arg::Object(fields) => Value::Object(fields.iter().map(|(k, a)| (k.clone(), constant(a))).collect()),
    }
}

struct Exec<'d> {
    doc: &'d Document,
    vars: Map<String, Value>,
    schema: Value,
    errors: Vec<Value>,
    /// Fragments being expanded (a fragment that spreads itself would never end).
    spreading: Vec<&'d str>,
}

impl<'d> Exec<'d> {
    fn root(&mut self, op: &'d Operation) -> Value {
        let root = match op.kind.as_str() {
            "mutation" => "Mutation",
            "subscription" => {
                self.errors.push(error("Subscriptions are not supported by this test server.", None));
                return Value::Null;
            }
            _ => "Query",
        };
        let mut fields = Vec::new();
        self.collect(&op.selection, root, &mut fields);
        let mut out = Map::new();
        for field in fields {
            let value = self.resolve(root, field);
            out.insert(field.alias.clone().unwrap_or_else(|| field.name.clone()), value);
        }
        Value::Object(out)
    }

    /// A root field's value, projected onto its selection.
    fn resolve(&mut self, root: &str, field: &'d Field) -> Value {
        let args: Map<String, Value> = field.args.iter().map(|(n, a)| (n.clone(), self.arg(a))).collect();
        let arg = |name: &str| args.get(name).filter(|v| !v.is_null());
        let value = match (root, field.name.as_str()) {
            (_, "__typename") => return json!(root),
            ("Query", "__schema") => self.schema.clone(),
            ("Query", "__type") => {
                let name = arg("name").and_then(Value::as_str).unwrap_or_default();
                let types = self.schema["types"].as_array();
                types.and_then(|t| t.iter().find(|t| t["name"] == name)).cloned().unwrap_or(Value::Null)
            }
            ("Query", "hello") => json!(format!("Hello, {}!", arg("name").and_then(Value::as_str).unwrap_or("world"))),
            ("Query", "greeting") => json!("Hi!"),
            ("Query", "user") => {
                let id = match arg("id") {
                    Some(Value::String(s)) => s.clone(),
                    Some(Value::Number(n)) => n.to_string(),
                    _ => return self.missing(field, "id", "ID!"),
                };
                users().into_iter().find(|u| u["id"] == id).unwrap_or(Value::Null)
            }
            ("Query", "users") => {
                let first = arg("first").and_then(Value::as_u64).map_or(usize::MAX, |n| n as usize);
                Value::Array(users().into_iter().take(first).collect())
            }
            ("Mutation", "echo") => match arg("text").and_then(Value::as_str) {
                Some(text) => json!(text),
                None => return self.missing(field, "text", "String!"),
            },
            _ => {
                let message = format!("Cannot query field \"{}\" on type \"{root}\".", field.name);
                self.errors.push(error(&message, Some((field.line, field.column))));
                return Value::Null;
            }
        };
        self.project(&value, field)
    }

    fn missing(&mut self, field: &Field, arg: &str, ty: &str) -> Value {
        let message = format!("Argument \"{arg}\" of required type \"{ty}\" was not provided.");
        self.errors.push(error(&message, Some((field.line, field.column))));
        Value::Null
    }

    fn arg(&self, arg: &Arg) -> Value {
        match arg {
            Arg::Var(name) => self.vars.get(name).cloned().unwrap_or(Value::Null),
            Arg::Const(v) => v.clone(),
            Arg::List(items) => Value::Array(items.iter().map(|a| self.arg(a)).collect()),
            Arg::Object(fields) => Value::Object(fields.iter().map(|(k, a)| (k.clone(), self.arg(a))).collect()),
        }
    }

    /// Keep the selected fields of `value` (objects carry their `__typename`).
    fn project(&mut self, value: &Value, field: &'d Field) -> Value {
        match value {
            Value::Array(items) => Value::Array(items.iter().map(|v| self.project(v, field)).collect()),
            Value::Object(obj) if !field.selection.is_empty() => {
                let typename = obj.get("__typename").and_then(Value::as_str).unwrap_or("Object").to_string();
                let mut fields = Vec::new();
                self.collect(&field.selection, &typename, &mut fields);
                let mut out = Map::new();
                for f in fields {
                    let v = if f.name == "__typename" {
                        json!(typename)
                    } else if let Some(v) = obj.get(&f.name) {
                        let v = self.visible(v, f);
                        self.project(&v, f)
                    } else {
                        let message = format!("Cannot query field \"{}\" on type \"{typename}\".", f.name);
                        self.errors.push(error(&message, Some((f.line, f.column))));
                        Value::Null
                    };
                    out.insert(f.alias.clone().unwrap_or_else(|| f.name.clone()), v);
                }
                Value::Object(out)
            }
            Value::Object(_) => {
                let message = format!("Field \"{}\" must have a selection of subfields.", field.name);
                self.errors.push(error(&message, Some((field.line, field.column))));
                Value::Null
            }
            leaf if !field.selection.is_empty() && !leaf.is_null() => {
                let message = format!("Field \"{}\" must not have a selection since it has no subfields.", field.name);
                self.errors.push(error(&message, Some((field.line, field.column))));
                Value::Null
            }
            leaf => leaf.clone(),
        }
    }

    /// Introspection leaves deprecated fields and enum values out unless `includeDeprecated: true`.
    fn visible(&self, value: &Value, f: &Field) -> Value {
        if let Value::Array(items) = value
            && matches!(f.name.as_str(), "fields" | "enumValues")
            && !f.args.iter().any(|(n, a)| n == "includeDeprecated" && self.arg(a) == json!(true))
        {
            return Value::Array(items.iter().filter(|i| i["isDeprecated"] != json!(true)).cloned().collect());
        }
        value.clone()
    }

    /// Fields of a selection set for an object of `typename`, with fragments expanded.
    fn collect(&mut self, selection: &'d [Selection], typename: &str, out: &mut Vec<&'d Field>) {
        let doc = self.doc;
        for s in selection {
            match s {
                Selection::Field(f) => out.push(f),
                Selection::Inline { on, selection } => {
                    if on.as_deref().is_none_or(|t| t == typename) {
                        self.collect(selection, typename, out);
                    }
                }
                Selection::Spread { name, line, column } => match doc.fragments.get(name) {
                    Some(_) if self.spreading.contains(&name.as_str()) => {
                        let message = format!("Cannot spread fragment \"{name}\" within itself.");
                        self.errors.push(error(&message, Some((*line, *column))));
                    }
                    Some(fragment) if fragment.on == typename => {
                        self.spreading.push(name);
                        self.collect(&fragment.selection, typename, out);
                        self.spreading.pop();
                    }
                    Some(_) => {}
                    None => self.errors.push(error(&format!("Unknown fragment \"{name}\"."), Some((*line, *column)))),
                },
            }
        }
    }
}

fn users() -> Vec<Value> {
    let post = |id: &str, title: &str| json!({ "__typename": "Post", "id": id, "title": title });
    vec![
        json!({
            "__typename": "User", "id": "1", "name": "Ada Lovelace", "email": "ada@example.test", "role": "ADMIN",
            "posts": [post("1", "Notes on the Analytical Engine"), post("2", "On Bernoulli numbers")],
        }),
        json!({
            "__typename": "User", "id": "2", "name": "Alan Turing", "email": null, "role": "MEMBER",
            "posts": [post("3", "Computing Machinery and Intelligence")],
        }),
    ]
}

// ---- introspection ------------------------------------------------------------

fn type_ref(kind: &str, name: &str) -> Value {
    json!({ "__typename": "__Type", "kind": kind, "name": name, "ofType": null })
}

fn non_null(of: Value) -> Value {
    json!({ "__typename": "__Type", "kind": "NON_NULL", "name": null, "ofType": of })
}

fn list_of(of: Value) -> Value {
    json!({ "__typename": "__Type", "kind": "LIST", "name": null, "ofType": of })
}

fn input(name: &str, description: Option<&str>, ty: Value, default: Option<&str>) -> Value {
    json!({ "__typename": "__InputValue", "name": name, "description": description, "type": ty, "defaultValue": default })
}

fn field(name: &str, description: Option<&str>, args: Vec<Value>, ty: Value, deprecated: Option<&str>) -> Value {
    json!({
        "__typename": "__Field", "name": name, "description": description, "args": args, "type": ty,
        "isDeprecated": deprecated.is_some(), "deprecationReason": deprecated,
    })
}

fn full_type(kind: &str, name: &str, description: Option<&str>, members: Vec<Value>, legacy: bool) -> Value {
    let (fields, enum_values) = match kind {
        "OBJECT" => (json!(members), Value::Null),
        "ENUM" => (Value::Null, json!(members)),
        _ => (Value::Null, Value::Null),
    };
    let mut t = json!({
        "__typename": "__Type", "kind": kind, "name": name, "description": description,
        "fields": fields, "inputFields": null, "interfaces": if kind == "OBJECT" { json!([]) } else { Value::Null },
        "enumValues": enum_values, "possibleTypes": null, "ofType": null,
    });
    if !legacy {
        t["specifiedByURL"] = Value::Null;
    }
    t
}

fn enum_value(name: &str, deprecated: Option<&str>) -> Value {
    json!({
        "__typename": "__EnumValue", "name": name, "description": null,
        "isDeprecated": deprecated.is_some(), "deprecationReason": deprecated,
    })
}

fn directive(name: &str, description: &str, locations: &[&str], args: Vec<Value>, legacy: bool) -> Value {
    let mut d = json!({
        "__typename": "__Directive", "name": name, "description": description, "locations": locations, "args": args,
    });
    if !legacy {
        d["isRepeatable"] = json!(false);
    }
    d
}

/// `__schema` of the test schema, with every field an introspection query may select.
fn schema(legacy: bool) -> Value {
    let scalar = |name: &str| type_ref("SCALAR", name);
    let object = |name: &str| type_ref("OBJECT", name);
    let id = || field("id", None, vec![], non_null(scalar("ID")), None);
    let types = vec![
        full_type(
            "OBJECT",
            "Query",
            Some("The root of all queries."),
            vec![
                field(
                    "hello",
                    Some("Greets someone."),
                    vec![input("name", Some("Who to greet."), scalar("String"), Some("\"world\""))],
                    non_null(scalar("String")),
                    None,
                ),
                field(
                    "user",
                    Some("A user by id (`1` or `2`)."),
                    vec![input("id", None, non_null(scalar("ID")), None)],
                    object("User"),
                    None,
                ),
                field(
                    "users",
                    Some("All users."),
                    vec![input("first", Some("Return at most this many."), scalar("Int"), None)],
                    non_null(list_of(non_null(object("User")))),
                    None,
                ),
                field("greeting", None, vec![], scalar("String"), Some("Use `hello`.")),
            ],
            legacy,
        ),
        full_type(
            "OBJECT",
            "Mutation",
            None,
            vec![field(
                "echo",
                Some("Returns the text."),
                vec![input("text", None, non_null(scalar("String")), None)],
                non_null(scalar("String")),
                None,
            )],
            legacy,
        ),
        full_type(
            "OBJECT",
            "User",
            Some("Someone with posts."),
            vec![
                id(),
                field("name", None, vec![], non_null(scalar("String")), None),
                field("email", Some("Not everyone shares it."), vec![], scalar("String"), None),
                field("role", None, vec![], non_null(type_ref("ENUM", "Role")), None),
                field("posts", None, vec![], non_null(list_of(non_null(object("Post")))), None),
            ],
            legacy,
        ),
        full_type(
            "OBJECT",
            "Post",
            None,
            vec![id(), field("title", None, vec![], non_null(scalar("String")), None)],
            legacy,
        ),
        full_type(
            "ENUM",
            "Role",
            Some("What a user may do."),
            vec![
                enum_value("ADMIN", None),
                enum_value("MEMBER", None),
                enum_value("GUEST", Some("No guests any more.")),
            ],
            legacy,
        ),
        full_type("SCALAR", "ID", None, vec![], legacy),
        full_type("SCALAR", "String", None, vec![], legacy),
        full_type("SCALAR", "Int", None, vec![], legacy),
        full_type("SCALAR", "Boolean", None, vec![], legacy),
    ];
    let condition = || vec![input("if", None, non_null(scalar("Boolean")), None)];
    let mut directives = vec![
        directive(
            "include",
            "Include only when `if` is true.",
            &["FIELD", "FRAGMENT_SPREAD", "INLINE_FRAGMENT"],
            condition(),
            legacy,
        ),
        directive(
            "skip",
            "Skip when `if` is true.",
            &["FIELD", "FRAGMENT_SPREAD", "INLINE_FRAGMENT"],
            condition(),
            legacy,
        ),
        directive(
            "deprecated",
            "Marks an element as no longer supported.",
            &["FIELD_DEFINITION", "ARGUMENT_DEFINITION", "INPUT_FIELD_DEFINITION", "ENUM_VALUE"],
            vec![input("reason", None, scalar("String"), Some("\"No longer supported\""))],
            legacy,
        ),
    ];
    let mut schema = json!({
        "__typename": "__Schema", "queryType": object("Query"), "mutationType": object("Mutation"),
        "subscriptionType": null, "types": types,
    });
    if !legacy {
        directives.push(directive(
            "specifiedBy",
            "Where a custom scalar is specified.",
            &["SCALAR"],
            vec![input("url", None, non_null(scalar("String")), None)],
            legacy,
        ));
        schema["description"] = json!("A tiny schema for testing GraphQL requests.");
    }
    schema["directives"] = json!(directives);
    schema
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exec(request: Value) -> (StatusCode, Value) {
        execute_request(&request, false)
    }

    #[test]
    fn queries_with_variables_aliases_and_fragments() {
        let query = r#"
            # Two operations; the name picks one.
            query One($id: ID!, $n: Int = 1) {
              user(id: $id) { ...U posts { title } }
              first: users(first: $n) { name }
              hi: hello
              __typename
            }
            query Two { hello(name: "you") }
            fragment U on User { id name ... on User { email role } }
        "#;
        let (status, body) = exec(json!({ "query": query, "variables": { "id": 2 }, "operationName": "One" }));
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(
            body,
            json!({ "data": {
                "user": { "id": "2", "name": "Alan Turing", "email": null, "role": "MEMBER",
                          "posts": [{ "title": "Computing Machinery and Intelligence" }] },
                "first": [{ "name": "Ada Lovelace" }],
                "hi": "Hello, world!",
                "__typename": "Query",
            }})
        );
        let (_, body) = exec(json!({ "query": query, "operationName": "Two" }));
        assert_eq!(body, json!({ "data": { "hello": "Hello, you!" } }));
        let (_, body) = exec(json!({ "query": "mutation { echo(text: \"a\\\"b\\u00e9\") }" }));
        assert_eq!(body, json!({ "data": { "echo": "a\"bé" } }));
    }

    #[test]
    fn errors_are_graphql_style() {
        let message = |request: Value| {
            let (status, body) = exec(request);
            assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
            body["errors"][0]["message"].as_str().unwrap().to_string()
        };
        assert_eq!(
            message(json!({ "query": "{ user(id: 1) { name nope } }" })),
            "Cannot query field \"nope\" on type \"User\"."
        );
        let (_, body) = exec(json!({ "query": "{\n  nope\n}" }));
        assert_eq!(body["errors"][0]["locations"], json!([{ "line": 2, "column": 3 }]));
        assert!(message(json!({ "query": "{ hello" })).starts_with("Syntax Error"));
        assert!(message(json!({ "query": "query A { hello } query B { hello }" })).contains("operation name"));
        assert_eq!(message(json!({ "query": "{ hello }", "operationName": "X" })), "Unknown operation named \"X\".");
        assert_eq!(
            message(json!({ "query": "query($id: ID!) { user(id: $id) { id } }" })),
            "Variable \"$id\" of required type \"ID!\" was not provided."
        );
        assert!(message(json!({ "query": "{ user(id: 1) }" })).contains("selection of subfields"));
        assert_eq!(message(json!({ "variables": {} })), "Must provide query string.");
        // A fragment cycle is an error, not an endless expansion.
        assert_eq!(
            message(json!({ "query": "{ ...A } fragment A on Query { hello ...B } fragment B on Query { ...A }" })),
            "Cannot spread fragment \"A\" within itself."
        );
    }

    #[test]
    fn introspection_and_legacy_servers() {
        let query = "{ __schema { queryType { name } types { name specifiedByURL fields(includeDeprecated: true) { name isDeprecated } } directives { name isRepeatable } } }";
        let (status, body) = exec(json!({ "query": query }));
        assert_eq!(status, StatusCode::OK, "{body}");
        let schema = &body["data"]["__schema"];
        assert_eq!(schema["queryType"]["name"], "Query");
        let query_type = &schema["types"][0];
        assert_eq!(query_type["fields"].as_array().unwrap().len(), 4);
        assert_eq!(query_type["fields"][3], json!({ "name": "greeting", "isDeprecated": true }));
        // Without includeDeprecated the deprecated field is left out.
        let (_, body) = exec(json!({ "query": "{ __type(name: \"Query\") { fields { name } } }" }));
        assert_eq!(body["data"]["__type"]["fields"].as_array().unwrap().len(), 3);

        let (status, body) = execute_request(&json!({ "query": query }), true);
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["errors"][0]["message"], "Cannot query field \"specifiedByURL\" on type \"__Type\".");
    }
}
