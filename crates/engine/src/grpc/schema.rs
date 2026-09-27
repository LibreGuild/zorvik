//! Service definitions: `.proto` files compiled in Rust (`protox`, no
//! `protoc`), the methods they offer, example messages, and the proto3 JSON
//! mapping between the UI's JSON and protobuf messages (`prost-reflect`).

use std::path::{Path, PathBuf};

use prost::Message as _;
use prost_reflect::{
    DescriptorPool, DeserializeOptions, DynamicMessage, Kind, MessageDescriptor, MethodDescriptor, SerializeOptions,
};
use serde::Serialize;
use serde_json::{Value, json};
use ts_rs::TS;

use crate::error::{EngineError, Result};

/// A service and its methods, for the method picker.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct GrpcService {
    /// Full name, e.g. `shop.v1.Orders`.
    pub name: String,
    pub methods: Vec<GrpcMethod>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct GrpcMethod {
    /// `package.Service/Method`: what the request's `method` holds.
    pub path: String,
    pub name: String,
    pub client_streaming: bool,
    pub server_streaming: bool,
    pub input_type: String,
    pub output_type: String,
    /// JSON template of the input message (see [`example_json`]).
    pub example: String,
}

impl GrpcMethod {
    pub fn of(method: &MethodDescriptor) -> Self {
        GrpcMethod {
            path: method_path(method),
            name: method.name().to_string(),
            client_streaming: method.is_client_streaming(),
            server_streaming: method.is_server_streaming(),
            input_type: method.input().full_name().to_string(),
            output_type: method.output().full_name().to_string(),
            example: example_json(&method.input()),
        }
    }
}

/// Loaded service definitions: a descriptor pool and the services on offer.
#[derive(Debug, Clone)]
pub struct GrpcDescriptors {
    pub(crate) pool: DescriptorPool,
    /// Full service names in the server's (or the files') order.
    pub(crate) services: Vec<String>,
    /// `reflection v1`, `reflection v1alpha` or `proto files`.
    pub source: String,
    /// Files read from disk (the `.proto` files and their imports), so callers
    /// can tell when a cached copy is stale. Empty for reflection.
    pub files: Vec<PathBuf>,
}

impl GrpcDescriptors {
    pub(crate) fn new(pool: DescriptorPool, services: Vec<String>, source: &str) -> Self {
        let services = services.into_iter().filter(|s| !is_reflection_service(s)).collect();
        Self { pool, services, source: source.to_string(), files: Vec::new() }
    }

    /// Compile `.proto` files. Imports are looked up in `import_paths`, then in
    /// each file's own folder; `google/protobuf/*.proto` are built in.
    /// CPU-bound: call it off the async workers.
    pub fn from_proto_files(files: &[PathBuf], import_paths: &[PathBuf]) -> Result<Self> {
        if files.is_empty() {
            return Err(EngineError::invalid("No .proto files given"));
        }
        // Import paths first: a file under one of them is then named relative to it,
        // so its own `import "pkg/other.proto"` lines resolve the same way protoc would.
        let mut includes: Vec<PathBuf> = import_paths.to_vec();
        for file in files {
            if let Some(dir) = file.parent().filter(|d| !d.as_os_str().is_empty())
                && !includes.iter().any(|i| i == dir)
            {
                includes.push(dir.to_path_buf());
            }
        }
        let mut compiler = protox::Compiler::new(&includes).map_err(|e| proto_error(&e))?;
        compiler.include_imports(true);
        compiler.open_files(files).map_err(|e| proto_error(&e))?;
        let pool = compiler.descriptor_pool();
        let services: Vec<String> = compiler
            .files()
            .filter(|f| !f.is_import())
            .filter_map(|f| pool.get_file_by_name(f.name()))
            .flat_map(|f| f.services().map(|s| s.full_name().to_string()).collect::<Vec<_>>())
            .collect();
        let read: Vec<PathBuf> = compiler.files().filter_map(|f| f.path().map(Path::to_path_buf)).collect();
        let mut out = Self::new(pool, services, "proto files");
        out.files = read;
        Ok(out)
    }

    /// Services and methods with streaming flags and example messages.
    pub fn describe(&self) -> Vec<GrpcService> {
        self.services
            .iter()
            .filter_map(|name| self.pool.get_service_by_name(name))
            .map(|service| GrpcService {
                name: service.full_name().to_string(),
                methods: service.methods().map(|m| GrpcMethod::of(&m)).collect(),
            })
            .collect()
    }

    /// The method named `package.Service/Method` (a leading `/` or a `.` before
    /// the method name are accepted too).
    pub fn method(&self, name: &str) -> Result<MethodDescriptor> {
        let name = name.trim().trim_start_matches('/');
        if name.is_empty() {
            return Err(EngineError::invalid("Pick a method to call (package.Service/Method)"));
        }
        let (service, method) = name
            .rsplit_once('/')
            .or_else(|| name.rsplit_once('.'))
            .ok_or_else(|| EngineError::invalid(format!("'{name}' is not a method name (package.Service/Method)")))?;
        let found = self.pool.get_service_by_name(service).ok_or_else(|| {
            EngineError::invalid(format!(
                "Service '{service}' is not in the {} (reload the services, or check the name)",
                self.source
            ))
        })?;
        found
            .methods()
            .find(|m| m.name() == method)
            .ok_or_else(|| EngineError::invalid(format!("Service '{service}' has no method '{method}'")))
    }

    pub fn pool(&self) -> &DescriptorPool {
        &self.pool
    }
}

fn is_reflection_service(name: &str) -> bool {
    name.starts_with("grpc.reflection.")
}

/// `package.Service/Method`.
pub fn method_path(method: &MethodDescriptor) -> String {
    format!("{}/{}", method.parent_service().full_name(), method.name())
}

/// protox's debug form carries `file:line:column: message`.
fn proto_error(err: &protox::Error) -> EngineError {
    EngineError::invalid(format!("Could not compile the .proto files: {err:?}"))
}

/// Parse a JSON message (proto3 JSON mapping). Empty text is an empty message;
/// unknown fields are refused so typos don't go unnoticed.
pub fn message_from_json(desc: &MessageDescriptor, text: &str) -> Result<DynamicMessage> {
    let text = if text.trim().is_empty() { "{}" } else { text };
    let mut de = serde_json::Deserializer::from_str(text);
    let options = DeserializeOptions::new().deny_unknown_fields(true);
    DynamicMessage::deserialize_with_options(desc.clone(), &mut de, &options)
        .and_then(|m| de.end().map(|()| m))
        .map_err(|e| EngineError::invalid(format!("The message is not a valid {}: {e}", desc.full_name())))
}

/// JSON of a message with default values included (so every field is visible).
pub(crate) fn message_value(msg: &DynamicMessage) -> std::result::Result<Value, serde_json::Error> {
    msg.serialize_with_options(serde_json::value::Serializer, &SerializeOptions::new().skip_default_fields(false))
}

/// Decode a received message into pretty JSON.
pub(crate) fn message_json(desc: &MessageDescriptor, payload: &[u8]) -> std::result::Result<String, String> {
    let msg = DynamicMessage::decode(desc.clone(), payload).map_err(|e| e.to_string())?;
    let value = message_value(&msg).map_err(|e| e.to_string())?;
    serde_json::to_string_pretty(&value).map_err(|e| e.to_string())
}

/// Encode a JSON message; returns the bytes and the message as pretty JSON.
pub(crate) fn encode_json(desc: &MessageDescriptor, text: &str) -> Result<(Vec<u8>, String)> {
    let msg = message_from_json(desc, text)?;
    let bytes = msg.encode_to_vec();
    if bytes.len() > super::codec::MAX_MESSAGE_BYTES {
        return Err(EngineError::invalid(format!(
            "The message is {} bytes, more than the {} MB limit",
            bytes.len(),
            super::codec::MAX_MESSAGE_BYTES >> 20
        )));
    }
    let shown = message_value(&msg).ok().and_then(|v| serde_json::to_string_pretty(&v).ok()).unwrap_or_default();
    Ok((bytes, shown))
}

/// Nested messages deeper than this are left empty (`{}`).
const EXAMPLE_DEPTH: usize = 4;
/// Most fields in one example. Wide messages nested a few levels deep would
/// otherwise make huge templates (and a hostile server's types endless work,
/// for every method); the fields beyond it are left out.
const EXAMPLE_FIELDS: usize = 1_000;

/// A JSON template for a message: every field with its default value, nested
/// messages filled in (to a limited depth), the first value of enums, one
/// element in repeated fields and maps, and the first field of each oneof.
pub fn example_json(desc: &MessageDescriptor) -> String {
    let mut budget = EXAMPLE_FIELDS;
    let value = example_message(desc, &mut Vec::new(), &mut budget);
    serde_json::to_string_pretty(&value).unwrap_or_else(|_| "{}".into())
}

fn example_message(desc: &MessageDescriptor, path: &mut Vec<String>, budget: &mut usize) -> Value {
    if let Some(v) = well_known_example(desc.full_name()) {
        return v;
    }
    let name = desc.full_name().to_string();
    // A recursive type is shown nested once, then left empty.
    if path.len() >= EXAMPLE_DEPTH || path.iter().filter(|p| **p == name).count() >= 2 {
        return json!({});
    }
    path.push(name);
    let mut fields = serde_json::Map::new();
    let mut oneofs_done: Vec<String> = Vec::new();
    for field in desc.fields() {
        if *budget == 0 {
            break;
        }
        if let Some(oneof) = field.containing_oneof().filter(|o| !o.is_synthetic()) {
            if oneofs_done.iter().any(|o| o == oneof.name()) {
                continue;
            }
            oneofs_done.push(oneof.name().to_string());
        }
        *budget -= 1;
        let value = if field.is_map() {
            let Kind::Message(entry) = field.kind() else { continue };
            let key = map_key_example(&entry.map_entry_key_field().kind());
            match example_value(&entry.map_entry_value_field().kind(), path, budget) {
                Some(v) => json!({ key: v }),
                None => json!({}),
            }
        } else {
            let Some(value) = example_value(&field.kind(), path, budget) else { continue };
            if field.is_list() { json!([value]) } else { value }
        };
        fields.insert(field.json_name().to_string(), value);
    }
    path.pop();
    Value::Object(fields)
}

fn example_value(kind: &Kind, path: &mut Vec<String>, budget: &mut usize) -> Option<Value> {
    Some(match kind {
        Kind::Double | Kind::Float => json!(0.0),
        Kind::Int32 | Kind::Sint32 | Kind::Sfixed32 | Kind::Uint32 | Kind::Fixed32 => json!(0),
        // 64-bit integers are strings in proto3 JSON (JavaScript loses precision otherwise).
        Kind::Int64 | Kind::Sint64 | Kind::Sfixed64 | Kind::Uint64 | Kind::Fixed64 => json!("0"),
        Kind::Bool => json!(false),
        Kind::String | Kind::Bytes => json!(""),
        Kind::Enum(e) => json!(e.default_value().name()),
        // An Any needs a type known to the pool; leave it out rather than guess.
        Kind::Message(m) if m.full_name() == "google.protobuf.Any" => return None,
        Kind::Message(m) => example_message(m, path, budget),
    })
}

fn map_key_example(kind: &Kind) -> String {
    match kind {
        Kind::String => "key".into(),
        Kind::Bool => "false".into(),
        _ => "0".into(),
    }
}

/// Well-known types have their own JSON forms.
fn well_known_example(name: &str) -> Option<Value> {
    Some(match name {
        "google.protobuf.Timestamp" => json!("1970-01-01T00:00:00Z"),
        "google.protobuf.Duration" => json!("0s"),
        "google.protobuf.FieldMask" => json!(""),
        "google.protobuf.Struct" | "google.protobuf.Empty" => json!({}),
        "google.protobuf.Value" => Value::Null,
        "google.protobuf.ListValue" => json!([]),
        "google.protobuf.DoubleValue" | "google.protobuf.FloatValue" => json!(0.0),
        "google.protobuf.Int32Value" | "google.protobuf.UInt32Value" => json!(0),
        "google.protobuf.Int64Value" | "google.protobuf.UInt64Value" => json!("0"),
        "google.protobuf.BoolValue" => json!(false),
        "google.protobuf.StringValue" | "google.protobuf.BytesValue" => json!(""),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROTO: &str = r#"
        syntax = "proto3";
        package t.v1;
        import "google/protobuf/timestamp.proto";
        import "google/protobuf/wrappers.proto";
        import "google/protobuf/struct.proto";
        import "google/protobuf/any.proto";
        enum Color { COLOR_UNSPECIFIED = 0; RED = 1; }
        message Node { string name = 1; Node child = 2; repeated Node kids = 3; }
        message Req {
          string s = 1; int64 big = 2; double d = 3; bool b = 4; bytes raw = 5;
          Color color = 6; repeated string tags = 7; map<string, Node> nodes = 8; map<int32, bool> flags = 9;
          oneof pick { string text = 10; int32 num = 11; }
          optional int32 maybe = 12;
          google.protobuf.Timestamp at = 13; google.protobuf.StringValue label = 14;
          google.protobuf.Struct extra = 15; google.protobuf.Value any_value = 16; google.protobuf.Any blob = 17;
          Node root = 18; uint32 user_id = 19;
        }
        message Reply { string s = 1; int64 big = 2; }
        service Svc {
          rpc Get(Req) returns (Reply);
          rpc Watch(Req) returns (stream Reply);
          rpc Chat(stream Req) returns (stream Reply);
        }
    "#;

    fn descriptors() -> (tempfile::TempDir, GrpcDescriptors) {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("t.proto");
        std::fs::write(&file, PROTO).unwrap();
        let d = GrpcDescriptors::from_proto_files(&[file], &[]).unwrap();
        (dir, d)
    }

    #[test]
    fn describes_services_and_finds_methods() {
        let (_dir, d) = descriptors();
        let services = d.describe();
        assert_eq!(services.len(), 1);
        assert_eq!(services[0].name, "t.v1.Svc");
        let m: Vec<_> =
            services[0].methods.iter().map(|m| (m.path.as_str(), m.client_streaming, m.server_streaming)).collect();
        assert_eq!(m, [("t.v1.Svc/Get", false, false), ("t.v1.Svc/Watch", false, true), ("t.v1.Svc/Chat", true, true)]);
        assert_eq!(services[0].methods[0].input_type, "t.v1.Req");
        assert_eq!(d.method("/t.v1.Svc/Watch").unwrap().name(), "Watch");
        assert_eq!(d.method("t.v1.Svc.Chat").unwrap().name(), "Chat");
        assert!(d.method("t.v1.Svc/Nope").unwrap_err().message.contains("no method 'Nope'"));
        assert!(d.method("x.Y/Z").unwrap_err().message.contains("not in the proto files"));
        assert!(d.method(" ").is_err());
        assert!(d.files.iter().any(|f| f.ends_with("t.proto")));
    }

    #[test]
    fn example_messages_are_valid_json_for_the_type() {
        let (_dir, d) = descriptors();
        let req = d.method("t.v1.Svc/Get").unwrap().input();
        let example = example_json(&req);
        let v: Value = serde_json::from_str(&example).unwrap();
        assert_eq!(v["big"], "0");
        assert_eq!(v["color"], "COLOR_UNSPECIFIED");
        assert_eq!(v["tags"], json!([""]));
        assert_eq!(v["flags"], json!({ "0": false }));
        assert_eq!(v["nodes"]["key"]["name"], "");
        // Only the first member of a oneof; proto3 `optional` is a normal field.
        assert!(v.get("text").is_some() && v.get("num").is_none());
        assert_eq!(v["maybe"], 0);
        assert_eq!(v["at"], "1970-01-01T00:00:00Z");
        assert!(v.get("blob").is_none());
        assert_eq!(v["userId"], 0);
        // A recursive type is shown nested once.
        assert_eq!(v["root"]["child"]["name"], "");
        assert_eq!(v["root"]["child"]["child"], json!({}));
        message_from_json(&req, &example).expect("example parses back");
    }

    #[test]
    fn examples_of_wide_nested_types_stay_small() {
        // 40 fields per level, 4 levels: 2.5 million values without a limit.
        let mut proto = String::from("syntax = \"proto3\"; package w; message L4 { string s = 1; }\n");
        for (level, child) in [("L3", "L4"), ("L2", "L3"), ("L1", "L2")] {
            let fields: String = (1..=40).map(|i| format!("{child} f{i} = {i}; ")).collect();
            proto.push_str(&format!("message {level} {{ {fields}}}\n"));
        }
        proto.push_str("service S { rpc M(L1) returns (L1); }");
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("w.proto");
        std::fs::write(&file, proto).unwrap();
        let d = GrpcDescriptors::from_proto_files(&[file], &[]).unwrap();
        let input = d.method("w.S/M").unwrap().input();
        let example = example_json(&input);
        assert!(example.len() < 200_000, "{} bytes", example.len());
        let v: Value = serde_json::from_str(&example).unwrap();
        assert_eq!(v["f1"]["f1"]["f1"]["s"], "");
        message_from_json(&input, &example).expect("a partial example is still valid");
    }

    #[test]
    fn json_round_trip_and_clear_errors() {
        let (_dir, d) = descriptors();
        let req = d.method("t.v1.Svc/Get").unwrap().input();
        let (bytes, shown) = encode_json(&req, r#"{"s": "hi", "big": "9007199254740993"}"#).unwrap();
        assert!(shown.contains("\"9007199254740993\""));
        let reply = d.method("t.v1.Svc/Get").unwrap().output();
        // Req and Reply share field numbers 1 and 2.
        let text = message_json(&reply, &bytes).unwrap();
        assert!(text.contains("\"s\": \"hi\"") && text.contains("9007199254740993"), "{text}");
        assert!(message_from_json(&req, "").is_ok());
        let err = message_from_json(&req, r#"{"nope": 1}"#).unwrap_err().message;
        assert!(err.contains("t.v1.Req") && err.contains("nope"), "{err}");
        assert!(message_from_json(&req, r#"{"s": 1}"#).is_err());
        assert!(message_from_json(&req, "{} x").is_err());
    }

    #[test]
    fn imports_resolve_through_import_paths_and_errors_name_the_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("pkg")).unwrap();
        std::fs::write(dir.path().join("pkg/common.proto"), "syntax = \"proto3\"; package pkg; message C {}").unwrap();
        std::fs::write(
            dir.path().join("pkg/api.proto"),
            "syntax = \"proto3\"; package pkg; import \"pkg/common.proto\"; service A { rpc M(C) returns (C); }",
        )
        .unwrap();
        let api = dir.path().join("pkg/api.proto");
        let d = GrpcDescriptors::from_proto_files(std::slice::from_ref(&api), &[dir.path().to_path_buf()]).unwrap();
        assert_eq!(d.describe()[0].methods[0].path, "pkg.A/M");
        assert_eq!(d.files.len(), 2);
        // Without the import path the import can't be found.
        let err = GrpcDescriptors::from_proto_files(&[api], &[]).unwrap_err().message;
        assert!(err.contains("pkg/common.proto"), "{err}");

        std::fs::write(dir.path().join("bad.proto"), "syntax = \"proto3\";\nmessage {").unwrap();
        let err = GrpcDescriptors::from_proto_files(&[dir.path().join("bad.proto")], &[]).unwrap_err().message;
        assert!(err.contains("bad.proto"), "{err}");
    }
}
