//! Building mock servers from what the workspace already has (`mock.*`):
//! a folder of saved requests, an OpenAPI document, or one response.

use std::collections::HashSet;
use std::io::Read as _;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use ts_rs::TS;
use zorvik_workspace::Workspace;
use zorvik_workspace::formats::mock::{routes_from_openapi, routes_from_requests};
use zorvik_workspace::formats::{MockRoute, NodeKind, Request, Server, ServerKind, TreeNode};

use crate::{Api, ApiError, ApiResult, MAX_IMPORT_FILE, join_err, ok, params};

/// First port tried for a new mock (ports of other saved servers are skipped).
const FIRST_PORT: u16 = 3000;
/// Longest wait for an OpenAPI document download when requests have no timeout.
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(120);

/// A mock server made from a folder or an OpenAPI document.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MockCreated {
    /// Id of the new server (file stem under `servers/`).
    pub id: String,
    /// Number of routes it has.
    pub routes: u32,
    /// Parts of an OpenAPI document that could not be used.
    pub warnings: Vec<String>,
}

impl Api {
    /// `mock.*` methods.
    pub(crate) async fn call_mock(&self, method: &str, p: Value) -> ApiResult<Value> {
        match method {
            "mock.fromFolder" => {
                #[derive(Deserialize)]
                struct P {
                    /// Folder path under `requests/`; empty for the whole collection.
                    #[serde(default)]
                    folder: String,
                    name: String,
                }
                let P { folder, name } = params(p)?;
                let ws = self.ws()?;
                let ws2 = ws.clone();
                let requests = tokio::task::spawn_blocking(move || saved_requests(&ws2, folder.trim_matches('/')))
                    .await
                    .map_err(join_err)??;
                let routes = routes_from_requests(&requests);
                if routes.is_empty() {
                    return Err(ApiError::invalid("There are no HTTP requests here to mock"));
                }
                ok(create_mock(&ws, &name, routes, Vec::new())?)
            }
            "mock.fromOpenApi" => {
                #[derive(Deserialize)]
                struct P {
                    text: Option<String>,
                    path: Option<String>,
                    url: Option<String>,
                    name: String,
                }
                let P { text, path, url, name } = params(p)?;
                let ws = self.ws()?;
                let text = match (text, path, url) {
                    (Some(text), _, _) => text,
                    (None, Some(path), _) => {
                        tokio::task::spawn_blocking(move || read_document(&path)).await.map_err(join_err)??
                    }
                    (None, None, Some(url)) => self.download_document(&url).await?,
                    (None, None, None) => {
                        return Err(ApiError::invalid("Choose an OpenAPI file, a URL or paste the document"));
                    }
                };
                // Parsing and generating examples for a big spec is CPU-bound.
                let (routes, warnings) =
                    tokio::task::spawn_blocking(move || routes_from_openapi(&text)).await.map_err(join_err)??;
                if routes.is_empty() {
                    return Err(ApiError::invalid("The document has no operations to mock"));
                }
                ok(create_mock(&ws, &name, routes, warnings)?)
            }
            "mock.addRoute" => {
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase")]
                struct P {
                    server_id: String,
                    route: MockRoute,
                }
                let P { server_id, route } = params(p)?;
                let ws = self.ws()?;
                let mut server = ws.read_server(&server_id)?;
                if server.kind != ServerKind::Http {
                    return Err(ApiError::invalid(format!("\"{}\" is not a mock API", server.name)));
                }
                // A server allowed to start with the workspace stays allowed.
                let trusted = self.is_trusted(&ws, &server_id, &server);
                server.http.routes.push(route);
                let id = ws.save_server(&server_id, &server)?;
                if trusted {
                    self.trust_server(&ws, &id, &server);
                }
                self.update_running(&ws, &id, server).await;
                ok(id)
            }
            other => Err(ApiError::new("notFound", format!("Unknown method '{other}'"))),
        }
    }

    pub(crate) async fn download_document(&self, url: &str) -> ApiResult<String> {
        let mut opts = crate::request_options(&self.settings(), &Default::default())?;
        opts.max_body_bytes = MAX_IMPORT_FILE as usize;
        // Nothing can cancel the download: never wait forever, even with timeouts turned off.
        opts.timeout.get_or_insert(DOWNLOAD_TIMEOUT);
        let request = zorvik_engine::HttpRequest {
            method: "GET".into(),
            url: url.trim().to_string(),
            headers: vec![zorvik_engine::Header::new("Accept", "application/json, application/yaml, */*")],
            body: Default::default(),
        };
        let response = self.inner.client.send(request, &opts, None).await?;
        if !(200..300).contains(&response.meta.status) {
            return Err(ApiError::new(
                "network",
                format!("Download failed: HTTP {} {}", response.meta.status, response.meta.status_text),
            ));
        }
        if response.body_truncated {
            return Err(ApiError::invalid("The download is larger than 50 MB"));
        }
        Ok(String::from_utf8_lossy(&response.body).into_owned())
    }

    /// A running copy of the server picks up the change right away.
    async fn update_running(&self, ws: &Workspace, server_id: &str, server: Server) {
        let root = ws.root().to_string_lossy().into_owned();
        let Ok(running) = self.call_server("server.running", json!({})).await else { return };
        let run_id = running.as_array().into_iter().flatten().find_map(|r| {
            (r["workspacePath"] == root.as_str() && r["serverId"] == server_id).then(|| r["runId"].clone())
        });
        if let Some(run_id) = run_id {
            let _ = self.call_server("server.update", json!({ "runId": run_id, "server": server })).await;
        }
    }
}

/// HTTP requests under `folder` (all of them for ""), in sidebar order.
fn saved_requests(ws: &Workspace, folder: &str) -> ApiResult<Vec<(String, Request)>> {
    fn find<'t>(nodes: &'t [TreeNode], path: &str) -> Option<&'t TreeNode> {
        nodes
            .iter()
            .filter(|n| n.kind == NodeKind::Folder)
            .find_map(|n| if n.path == path { Some(n) } else { find(&n.children, path) })
    }
    fn collect(ws: &Workspace, nodes: &[TreeNode], out: &mut Vec<(String, Request)>) {
        for node in nodes {
            match node.kind {
                NodeKind::Folder => collect(ws, &node.children, out),
                // Files that don't parse are skipped (the sidebar flags them).
                NodeKind::Request if node.error.is_none() => {
                    if let Ok(request) = ws.read_request(&node.path) {
                        out.push((node.path.clone(), request));
                    }
                }
                NodeKind::Request => {}
            }
        }
    }
    let tree = ws.tree()?;
    let nodes = if folder.is_empty() {
        &tree[..]
    } else {
        &find(&tree, folder).ok_or_else(|| ApiError::new("notFound", format!("Folder '{folder}' not found")))?.children
            [..]
    };
    let mut out = Vec::new();
    collect(ws, nodes, &mut out);
    Ok(out)
}

/// A document of at most 50 MB from a regular file (not a device or a pipe,
/// which could be endless).
pub(crate) fn read_document(path: &str) -> ApiResult<String> {
    let io_err = |e: std::io::Error| ApiError::new("io", format!("Could not read {path}: {e}"));
    let meta = std::fs::metadata(path).map_err(io_err)?;
    if !meta.is_file() {
        return Err(ApiError::invalid(format!("{path} is not a file")));
    }
    let too_big = || ApiError::invalid("File is larger than 50 MB");
    if meta.len() > MAX_IMPORT_FILE {
        return Err(too_big());
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path).and_then(|f| f.take(MAX_IMPORT_FILE + 1).read_to_end(&mut bytes)).map_err(io_err)?;
    if bytes.len() as u64 > MAX_IMPORT_FILE {
        return Err(too_big());
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// Save a new mock API with `routes` on a port no other saved server uses.
fn create_mock(ws: &Workspace, name: &str, routes: Vec<MockRoute>, warnings: Vec<String>) -> ApiResult<MockCreated> {
    let name = match name.trim() {
        "" => "Mock API",
        name => name,
    };
    let used: HashSet<u16> = ws.list_servers()?.iter().map(|n| n.port).collect();
    let mut server = Server::new(name, ServerKind::Http);
    server.port = (FIRST_PORT..=u16::MAX).find(|p| !used.contains(p)).unwrap_or(FIRST_PORT);
    server.http.routes = routes;
    let count = server.http.routes.len() as u32;
    let id = ws.create_server(&server)?;
    Ok(MockCreated { id, routes: count, warnings })
}
