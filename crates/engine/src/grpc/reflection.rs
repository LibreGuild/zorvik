//! Server reflection client: `grpc.reflection.v1.ServerReflection`, then
//! `v1alpha` for older servers. Lists the services and fetches the file
//! descriptors that define them, with every dependency, over one stream.
//!
//! What a server sends is bounded (files, bytes, requests), and `import public`
//! is flattened before building the pool, so a broken or hostile server can't
//! exhaust memory, loop forever or crash the app.

use std::collections::{HashMap, HashSet};

use prost::Message as _;
use prost_reflect::DescriptorPool;
use prost_reflect::prost_types::FileDescriptorProto;

use super::codec::{self, UNIMPLEMENTED};
use super::{CallEvent, Channel, GrpcDescriptors, GrpcStatus, RawCall};
use crate::error::{EngineError, ErrorKind, Result};

// Messages of reflection.proto (v1 and v1alpha share them); only the parts used here.

#[derive(Clone, PartialEq, prost::Message)]
struct ReflectionRequest {
    #[prost(string, tag = "1")]
    host: String,
    #[prost(oneof = "Ask", tags = "3, 4, 7")]
    message_request: Option<Ask>,
}

#[derive(Clone, PartialEq, prost::Oneof)]
enum Ask {
    #[prost(string, tag = "3")]
    FileByFilename(String),
    #[prost(string, tag = "4")]
    FileContainingSymbol(String),
    #[prost(string, tag = "7")]
    ListServices(String),
}

#[derive(Clone, PartialEq, prost::Message)]
struct ReflectionResponse {
    #[prost(oneof = "Answer", tags = "4, 6, 7")]
    message_response: Option<Answer>,
}

#[derive(Clone, PartialEq, prost::Oneof)]
enum Answer {
    #[prost(message, tag = "4")]
    Files(FileDescriptorResponse),
    #[prost(message, tag = "6")]
    Services(ListServiceResponse),
    #[prost(message, tag = "7")]
    Error(ErrorResponse),
}

#[derive(Clone, PartialEq, prost::Message)]
struct FileDescriptorResponse {
    #[prost(bytes = "vec", repeated, tag = "1")]
    file_descriptor_proto: Vec<Vec<u8>>,
}

#[derive(Clone, PartialEq, prost::Message)]
struct ListServiceResponse {
    #[prost(message, repeated, tag = "1")]
    service: Vec<ServiceResponse>,
}

#[derive(Clone, PartialEq, prost::Message)]
struct ServiceResponse {
    #[prost(string, tag = "1")]
    name: String,
}

#[derive(Clone, PartialEq, prost::Message)]
struct ErrorResponse {
    #[prost(int32, tag = "1")]
    error_code: i32,
    #[prost(string, tag = "2")]
    error_message: String,
}

enum Failure {
    /// This reflection version is not offered: try the next one.
    Unimplemented,
    Failed(EngineError),
}

impl From<EngineError> for Failure {
    fn from(e: EngineError) -> Self {
        Failure::Failed(e)
    }
}

const VERSIONS: [(&str, &str); 2] = [
    ("v1", "/grpc.reflection.v1.ServerReflection/ServerReflectionInfo"),
    ("v1alpha", "/grpc.reflection.v1alpha.ServerReflection/ServerReflectionInfo"),
];

/// Most files, encoded bytes and requests one load may take (large real APIs
/// stay far below).
const MAX_FILES: usize = 5_000;
const MAX_BYTES: usize = 128 << 20;
const MAX_REQUESTS: usize = 10_000;

pub(crate) async fn load(channel: &mut Channel, headers: &http::HeaderMap) -> Result<GrpcDescriptors> {
    for (version, path) in VERSIONS {
        match load_version(channel, path, headers).await {
            Ok((pool, services)) => return Ok(GrpcDescriptors::new(pool, services, &format!("reflection {version}"))),
            Err(Failure::Unimplemented) => continue,
            Err(Failure::Failed(e)) => return Err(e),
        }
    }
    Err(EngineError::new(
        ErrorKind::Protocol,
        "The server does not offer reflection (grpc.reflection.v1 or v1alpha). Add the service's .proto files in the Proto files tab.",
    ))
}

/// One bidirectional stream: list the services, then ask for the file of each
/// service and for any dependency not received yet.
async fn load_version(
    channel: &mut Channel,
    path: &str,
    headers: &http::HeaderMap,
) -> std::result::Result<(DescriptorPool, Vec<String>), Failure> {
    let mut call = channel.call(path, headers.clone()).await?;
    let mut asked = 0;
    let services = match ask(&mut call, &mut asked, Ask::ListServices(String::new())).await? {
        Answer::Services(list) => list.service.into_iter().map(|s| s.name).collect::<Vec<_>>(),
        Answer::Error(e) => return Err(answer_error("list the services", &e).into()),
        Answer::Files(_) => return Err(unexpected().into()),
    };

    let mut files = Files::default();
    for service in services.iter().filter(|s| !s.starts_with("grpc.reflection.")) {
        // One file often defines several services.
        if files.services.contains(service) {
            continue;
        }
        let answer = ask(&mut call, &mut asked, Ask::FileContainingSymbol(service.clone())).await?;
        files.add(answer, &format!("describe {service}"))?;
    }
    // Servers usually send dependencies along; fetch whatever is still missing.
    loop {
        let mut missing: Vec<String> = files
            .protos
            .values()
            .flat_map(|f| f.dependency.iter())
            .filter(|d| !files.protos.contains_key(*d))
            .cloned()
            .collect();
        missing.sort();
        missing.dedup();
        if missing.is_empty() {
            break;
        }
        for name in missing {
            match ask(&mut call, &mut asked, Ask::FileByFilename(name.clone())).await? {
                Answer::Error(e) => match builtin_file(&name) {
                    // Well-known types the server doesn't serve are built in.
                    Some(file) => files.insert(file)?,
                    None => return Err(answer_error(&format!("fetch {name}"), &e).into()),
                },
                answer => files.add(answer, &format!("fetch {name}"))?,
            }
            if !files.protos.contains_key(&name) {
                return Err(EngineError::new(
                    ErrorKind::Protocol,
                    format!("Reflection did not return '{name}', which the services depend on"),
                )
                .into());
            }
        }
    }
    call.close_send();
    Ok((build_pool(files.protos)?, services))
}

/// The pool of the received files. `import public` is flattened first:
/// prost-reflect follows public imports recursively without noticing a cycle
/// (a stack overflow that ends the app), and a server can send one.
fn build_pool(mut files: HashMap<String, FileDescriptorProto>) -> Result<DescriptorPool> {
    flatten_public_imports(&mut files);
    let mut pool = DescriptorPool::new();
    pool.add_file_descriptor_protos(files.into_values()).map_err(|e| {
        EngineError::new(ErrorKind::Protocol, format!("The service definitions from reflection are invalid: {e}"))
    })?;
    Ok(pool)
}

/// Every file gets the files it sees through public imports (transitively) as
/// plain dependencies, which makes the same names visible without any
/// `public_dependency` left to follow.
fn flatten_public_imports(files: &mut HashMap<String, FileDescriptorProto>) {
    let public: HashMap<String, Vec<String>> = files
        .iter()
        .map(|(name, f)| {
            let deps = f
                .public_dependency
                .iter()
                .filter_map(|&i| usize::try_from(i).ok().and_then(|i| f.dependency.get(i)).cloned())
                .collect();
            (name.clone(), deps)
        })
        .collect();
    for file in files.values_mut() {
        let mut seen: HashSet<String> = file.dependency.iter().cloned().collect();
        let mut stack = file.dependency.clone();
        while let Some(name) = stack.pop() {
            for dep in public.get(&name).into_iter().flatten() {
                if seen.insert(dep.clone()) {
                    file.dependency.push(dep.clone());
                    stack.push(dep.clone());
                }
            }
        }
        file.public_dependency.clear();
    }
}

/// Files received so far, within [`MAX_FILES`] and [`MAX_BYTES`].
#[derive(Default)]
struct Files {
    protos: HashMap<String, FileDescriptorProto>,
    /// Full names of the services defined in `protos`.
    services: HashSet<String>,
    bytes: usize,
}

impl Files {
    fn add(&mut self, answer: Answer, what: &str) -> Result<()> {
        match answer {
            Answer::Files(r) => {
                for raw in r.file_descriptor_proto {
                    self.bytes += raw.len();
                    let file = FileDescriptorProto::decode(raw.as_slice()).map_err(|e| {
                        EngineError::new(ErrorKind::Protocol, format!("Invalid file descriptor from reflection: {e}"))
                    })?;
                    self.insert(file)?;
                }
                Ok(())
            }
            Answer::Error(e) => Err(answer_error(what, &e)),
            Answer::Services(_) => Err(unexpected()),
        }
    }

    fn insert(&mut self, file: FileDescriptorProto) -> Result<()> {
        if self.protos.contains_key(file.name()) {
            return Ok(());
        }
        if self.protos.len() >= MAX_FILES || self.bytes > MAX_BYTES {
            return Err(EngineError::new(
                ErrorKind::Protocol,
                format!(
                    "Reflection sent more than {MAX_FILES} files or {} MB of definitions; add the .proto files instead",
                    MAX_BYTES >> 20
                ),
            ));
        }
        let package = file.package();
        for service in &file.service {
            let name =
                if package.is_empty() { service.name().to_string() } else { format!("{package}.{}", service.name()) };
            self.services.insert(name);
        }
        self.protos.insert(file.name().to_string(), file);
        Ok(())
    }
}

/// Send one reflection request and wait for its answer.
async fn ask(call: &mut RawCall, asked: &mut usize, request: Ask) -> std::result::Result<Answer, Failure> {
    *asked += 1;
    if *asked > MAX_REQUESTS {
        return Err(EngineError::new(
            ErrorKind::Protocol,
            format!("Reflection needed more than {MAX_REQUESTS} requests; add the .proto files instead"),
        )
        .into());
    }
    let message = ReflectionRequest { host: String::new(), message_request: Some(request) };
    call.send(&message.encode_to_vec())?;
    loop {
        match call.next().await {
            Some(CallEvent::Headers(_)) => continue,
            Some(CallEvent::Message(bytes)) => {
                let response = ReflectionResponse::decode(bytes)
                    .map_err(|e| EngineError::new(ErrorKind::Protocol, format!("Invalid reflection response: {e}")))?;
                return response
                    .message_response
                    .ok_or_else(|| EngineError::new(ErrorKind::Protocol, "Empty reflection response").into());
            }
            Some(CallEvent::End { status, .. }) if status.code == UNIMPLEMENTED => return Err(Failure::Unimplemented),
            Some(CallEvent::End { status, .. }) => return Err(ended(&status).into()),
            None => return Err(EngineError::new(ErrorKind::Protocol, "Reflection stream ended").into()),
        }
    }
}

fn builtin_file(name: &str) -> Option<FileDescriptorProto> {
    use protox::file::FileResolver as _;
    if !name.starts_with("google/protobuf/") {
        return None;
    }
    protox::file::GoogleFileResolver::new().open_file(name).ok().map(|f| f.file_descriptor_proto().clone())
}

fn ended(status: &GrpcStatus) -> EngineError {
    let message = if status.message.is_empty() { String::new() } else { format!(": {}", status.message) };
    EngineError::new(ErrorKind::Protocol, format!("Reflection failed with {}{message}", status.name))
}

fn answer_error(what: &str, e: &ErrorResponse) -> EngineError {
    let name = codec::status_name(e.error_code.max(0) as u32);
    EngineError::new(ErrorKind::Protocol, format!("Reflection could not {what}: {name} {}", e.error_message))
}

fn unexpected() -> EngineError {
    EngineError::new(ErrorKind::Protocol, "Unexpected reflection response")
}

#[cfg(test)]
mod tests {
    use prost_reflect::prost_types::{
        DescriptorProto, FieldDescriptorProto, ServiceDescriptorProto, field_descriptor_proto,
    };

    use super::*;

    fn file(name: &str, deps: &[&str], public: &[i32], message: Option<(&str, &str)>) -> FileDescriptorProto {
        // `message`: (name, type of its one field, e.g. `.p.D`).
        let message_type = message
            .map(|(name, field_type)| DescriptorProto {
                name: Some(name.into()),
                field: vec![FieldDescriptorProto {
                    name: Some("f".into()),
                    number: Some(1),
                    label: Some(field_descriptor_proto::Label::Optional as i32),
                    r#type: Some(field_descriptor_proto::Type::Message as i32),
                    type_name: Some(field_type.into()),
                    ..Default::default()
                }],
                ..Default::default()
            })
            .into_iter()
            .collect();
        FileDescriptorProto {
            name: Some(name.into()),
            package: Some("p".into()),
            dependency: deps.iter().map(|d| d.to_string()).collect(),
            public_dependency: public.to_vec(),
            message_type,
            syntax: Some("proto3".into()),
            ..Default::default()
        }
    }

    fn files(list: Vec<FileDescriptorProto>) -> HashMap<String, FileDescriptorProto> {
        list.into_iter().map(|f| (f.name().to_string(), f)).collect()
    }

    #[test]
    fn public_import_cycles_do_not_crash() {
        // prost-reflect alone overflows the stack on these (the whole app would end).
        let own = files(vec![file("a.proto", &["a.proto"], &[0], None)]);
        let _ = build_pool(own);
        let pair = files(vec![file("a.proto", &["b.proto"], &[0], None), file("b.proto", &["a.proto"], &[0], None)]);
        let _ = build_pool(pair);
    }

    #[test]
    fn public_imports_are_visible_through_chains() {
        let mut d = file("d.proto", &[], &[], None);
        d.message_type.push(DescriptorProto { name: Some("D".into()), ..Default::default() });
        let pool = build_pool(files(vec![
            file("a.proto", &["b.proto"], &[], Some(("A", ".p.D"))),
            file("b.proto", &["c.proto"], &[0], None),
            file("c.proto", &["d.proto"], &[0], None),
            d,
        ]))
        .expect("D is visible in a.proto through b and c");
        let a = pool.get_message_by_name("p.A").unwrap();
        assert_eq!(a.get_field_by_name("f").unwrap().kind().as_message().unwrap().full_name(), "p.D");
    }

    #[test]
    fn received_files_are_bounded_and_services_noted() {
        let mut received = Files::default();
        let mut with_service = file("s.proto", &[], &[], None);
        with_service.service.push(ServiceDescriptorProto { name: Some("Svc".into()), ..Default::default() });
        received.insert(with_service).unwrap();
        assert!(received.services.contains("p.Svc"));
        for i in 1..MAX_FILES {
            received.insert(file(&format!("f{i}.proto"), &[], &[], None)).unwrap();
        }
        // A file already there is not counted again; a new one is too many.
        received.insert(file("f1.proto", &[], &[], None)).unwrap();
        let err = received.insert(file("more.proto", &[], &[], None)).unwrap_err();
        assert!(err.message.contains(".proto files instead"), "{}", err.message);
    }
}
