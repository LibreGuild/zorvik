//! The API catalog (`catalog.*`): well-known APIs a user imports in one click. The list ships
//! with the app (`catalog/catalog.yaml`); an API's description is downloaded from its publisher
//! only when the user imports it, so nothing is fetched unasked and nothing third-party is
//! bundled. APIs that publish no description get a small one written for Zorvik
//! (`catalog/starters/`); OpenAI-compatible APIs reuse OpenAI's. Design: docs/architecture.md
//! ("API catalog").

use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;
use zorvik_workspace::Workspace;
use zorvik_workspace::formats::import::{ImportSummary, ImportedCollection, ImportedItem};
use zorvik_workspace::formats::openapi::{self, OperationGroup};
use zorvik_workspace::formats::{
    Auth, Body, BodyType, GraphqlBody, KeyValue, NodeKind, OperationFilter, Request, RequestKind, Variable,
};

use crate::mock::DOWNLOAD_TIMEOUT;
use crate::spec_update::{find, requests_under};
use crate::{Api, ApiError, ApiResult, ImportKind, KeptSpec, MAX_IMPORT_FILE, detect_import, join_err, lock};

const CATALOG: &str = include_str!("../catalog/catalog.yaml");

/// Descriptions written for Zorvik, by file name (`Starter` entries).
const STARTERS: &[(&str, &str)] = &[
    ("airtable.yaml", include_str!("../catalog/starters/airtable.yaml")),
    ("anthropic.yaml", include_str!("../catalog/starters/anthropic.yaml")),
    ("openweathermap.yaml", include_str!("../catalog/starters/openweathermap.yaml")),
    ("telegram.yaml", include_str!("../catalog/starters/telegram.yaml")),
];

/// How long a download is reused: picking groups and then importing downloads once.
const KEEP_DOWNLOAD: Duration = Duration::from_secs(15 * 60);
/// Redirects a download follows at least, whatever the user's setting for requests.
const MIN_REDIRECTS: u32 = 10;

/// One API in the catalog.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct CatalogEntry {
    /// Stable id, e.g. `stripe`.
    pub id: String,
    pub name: String,
    pub category: CatalogCategory,
    /// One line on what the API does.
    pub description: String,
    /// The publisher's API docs: where to read about it, or download the description by hand.
    pub website: String,
    /// How requests sign in, in plain words (e.g. "Bearer secret key").
    pub auth: String,
    /// What the description is, for display (e.g. "OpenAPI 3.1", "Google Discovery").
    pub format: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub license: Option<CatalogLicense>,
    pub source: CatalogSource,
    /// The request opened after importing: `METHOD /path` as the description writes it, or a
    /// request name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub first: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum CatalogCategory {
    Ai,
    Cloud,
    DevTools,
    Monitoring,
    Data,
    Messaging,
    Productivity,
    Payments,
    Commerce,
    Marketing,
    Identity,
    Media,
}

/// The license the publisher gives its description under.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct CatalogLicense {
    /// e.g. "MIT", "Apache-2.0", or "Not stated".
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    #[ts(optional, as = "Option<String>")]
    pub url: String,
}

/// Where an entry's requests come from.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "camelCase")]
#[ts(export)]
pub enum CatalogSource {
    /// The publisher's description (OpenAPI, Swagger, Google Discovery, AWS Smithy or a Postman
    /// collection), downloaded when the user imports it.
    #[serde(rename_all = "camelCase")]
    Download {
        url: String,
        /// Where the API runs, when the description doesn't say (or says it relatively).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        base_url: Option<String>,
        /// Groups (first tags) ticked to begin with when the user picks; none = all.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        #[ts(optional, as = "Option<Vec<String>>")]
        groups: Vec<String>,
        /// The security scheme requests use, when the description offers several.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        scheme: Option<String>,
        /// Variables that hold credentials: kept as secrets.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        #[ts(optional, as = "Option<Vec<String>>")]
        secrets: Vec<String>,
        /// Better starting values for variables (e.g. a real model name instead of an example).
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        #[ts(optional, as = "Option<Vec<Variable>>")]
        variables: Vec<Variable>,
        /// How requests sign in, when the description doesn't say (or says it wrongly); its
        /// `{{variables}}` go in `variables`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        auth: Option<Auth>,
    },
    /// An OpenAI-compatible API: entry `of`'s operations on `paths`, sent to `base_url`.
    #[serde(rename_all = "camelCase")]
    Compatible { of: String, base_url: String, paths: Vec<String> },
    /// An OpenAPI description written for Zorvik (`catalog/starters/<file>`), for an API whose
    /// publisher has none.
    #[serde(rename_all = "camelCase")]
    Starter {
        file: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        #[ts(optional, as = "Option<Vec<String>>")]
        secrets: Vec<String>,
    },
    /// A GraphQL API: one request with a sample query (the editor completes the rest from the
    /// API's schema). Credentials go in secret `variables`.
    #[serde(rename_all = "camelCase")]
    Graphql {
        url: String,
        query: String,
        #[serde(default)]
        auth: Auth,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        #[ts(optional, as = "Option<Vec<KeyValue>>")]
        headers: Vec<KeyValue>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        #[ts(optional, as = "Option<Vec<Variable>>")]
        variables: Vec<Variable>,
    },
    /// Not importable (the publisher's terms don't allow it, or it publishes no description):
    /// the entry links to the API's docs.
    #[serde(rename_all = "camelCase")]
    Link { reason: String },
}

/// What importing an entry would bring in.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CatalogPreview {
    pub operations: u32,
    /// Groups to pick from (empty when the API comes whole).
    pub groups: Vec<OperationGroup>,
    /// Groups ticked to begin with (empty = all).
    pub selected: Vec<String>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CatalogImport {
    pub summary: ImportSummary,
    /// Path of the request to open first, when the entry names one.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub open: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CatalogImportParams {
    pub id: String,
    /// Folder to import into (`""` = top level).
    #[serde(default)]
    pub parent: String,
    /// Groups (first tags) to import; `None` = the entry's preset groups, or everything.
    #[serde(default)]
    pub groups: Option<Vec<String>>,
}

/// The last download, reused for a while (`KEEP_DOWNLOAD`).
#[derive(Default)]
pub(crate) struct Downloads(Mutex<Option<(String, Instant, Arc<String>)>>);

impl Downloads {
    /// The text downloaded from `url`, when recent.
    fn recent(&self, url: &str) -> Option<Arc<String>> {
        lock(&self.0)
            .as_ref()
            .filter(|(at_url, at, _)| at_url == url && at.elapsed() < KEEP_DOWNLOAD)
            .map(|k| k.2.clone())
    }

    fn keep(&self, url: &str, text: Arc<String>) {
        *lock(&self.0) = Some((url.to_string(), Instant::now(), text));
    }

    /// Stop reusing the download from `url`: it couldn't be read (trying again downloads it
    /// again), or it has been imported (no need to hold on to up to 50 MB).
    fn forget(&self, url: &str) {
        let mut kept = lock(&self.0);
        if kept.as_ref().is_some_and(|(at_url, ..)| at_url == url) {
            *kept = None;
        }
    }

    /// The text of a download that is being imported, forgotten (copied only when another
    /// call still holds it).
    fn take(&self, url: &str, text: Arc<String>) -> String {
        self.forget(url);
        Arc::try_unwrap(text).unwrap_or_else(|text| text.as_str().to_owned())
    }
}

/// Run CPU-heavy work (parsing a description of up to 50 MB) off the async workers.
async fn off_workers<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> ApiResult<T> {
    tokio::task::spawn_blocking(work).await.map_err(join_err)
}

/// The catalog as shipped (checked by the tests below).
pub(crate) fn entries() -> &'static [CatalogEntry] {
    static ENTRIES: OnceLock<Vec<CatalogEntry>> = OnceLock::new();
    ENTRIES.get_or_init(|| serde_yaml_ng::from_str(CATALOG).expect("catalog/catalog.yaml is valid (see the tests)"))
}

pub(crate) fn entry(id: &str) -> ApiResult<&'static CatalogEntry> {
    entries()
        .iter()
        .find(|e| e.id == id)
        .ok_or_else(|| ApiError::new("notFound", format!("No API \"{id}\" in the catalog")))
}

fn starter(file: &str) -> ApiResult<&'static str> {
    STARTERS
        .iter()
        .find(|(name, _)| *name == file)
        .map(|(_, text)| *text)
        .ok_or_else(|| ApiError::new("notFound", format!("No starter \"{file}\" in the catalog")))
}

/// The download URL and options of the entry whose description `entry` uses.
fn download_of(entry: &'static CatalogEntry) -> ApiResult<(&'static CatalogEntry, &'static str)> {
    let source = match &entry.source {
        CatalogSource::Compatible { of, .. } => self::entry(of)?,
        _ => entry,
    };
    match &source.source {
        CatalogSource::Download { url, .. } => Ok((source, url)),
        _ => Err(ApiError::invalid(format!("{} isn't downloaded", entry.name))),
    }
}

/// Why an entry can't be imported, with what to do instead.
fn link_error(entry: &CatalogEntry, reason: &str) -> ApiError {
    ApiError::new("catalogLink", format!("{reason} Read about the {} API at {}.", entry.name, entry.website))
}

/// The download failed or brought back something else: say so, and how to get the file by hand.
fn unavailable(entry: &CatalogEntry, why: String) -> ApiError {
    ApiError::new(
        "catalogUnavailable",
        format!(
            "{why} You can download the {} API description from {} yourself and import the file \
             (Import → File).",
            entry.name, entry.website
        ),
    )
}

fn host_of(url: &str) -> String {
    url::Url::parse(url).ok().and_then(|u| u.host_str().map(str::to_string)).unwrap_or_else(|| url.to_string())
}

/// The plain-http address a download that started on https went through (a redirect to it
/// could have been changed on the way, and the description says where credentials go).
fn insecure_hop<'a>(start: &str, hops: impl IntoIterator<Item = &'a str>) -> Option<&'a str> {
    let https = |url: &str| url.get(..8).is_some_and(|s| s.eq_ignore_ascii_case("https://"));
    if !https(start) {
        return None;
    }
    hops.into_iter().find(|url| !https(url))
}

/// The preset groups the document still has (a publisher may rename its tags); none left
/// means all of it, as with no preset.
fn preset_groups(outline: &openapi::OpenApiOutline, preset: &[String]) -> Vec<String> {
    preset.iter().filter(|g| outline.groups.iter().any(|o| &o.tag == *g)).cloned().collect()
}

/// `text` looks like a web page rather than a description (a moved file often redirects to one).
fn is_web_page(content_type: Option<&str>, text: &str) -> bool {
    let head: String = text.trim_start().chars().take(64).collect::<String>().to_ascii_lowercase();
    content_type.is_some_and(|c| c.to_ascii_lowercase().contains("text/html"))
        || head.starts_with("<!doctype html")
        || head.starts_with("<html")
}

/// Mark the variables named in `secrets` as secret, and give `values` their values (adding
/// the ones the import didn't make).
fn set_variables(collection: &mut ImportedCollection, secrets: &[String], values: &[Variable]) {
    for v in collection.variables.iter_mut().filter(|v| secrets.contains(&v.key)) {
        v.secret = true;
    }
    for value in values {
        match collection.variables.iter_mut().find(|v| v.key == value.key) {
            Some(v) => {
                v.value = value.value.clone();
                v.secret |= value.secret;
            }
            None => collection.variables.push(value.clone()),
        }
    }
}

/// Remove the empty headers named like the one `auth` sends (a description that lists the
/// credential as a header parameter on every operation): sent empty, they would get in its way.
pub(crate) fn drop_auth_headers(items: &mut [ImportedItem], auth: &Auth) {
    let name = match auth {
        Auth::ApiKey { key, location: zorvik_workspace::formats::ApiKeyLocation::Header, .. } => key.as_str(),
        Auth::None | Auth::Inherit | Auth::ApiKey { .. } => return,
        _ => "Authorization",
    };
    for item in items {
        match item {
            ImportedItem::Folder { children, .. } => drop_auth_headers(children, auth),
            ImportedItem::Request(r) => {
                r.headers.retain(|h| !(h.key.eq_ignore_ascii_case(name) && h.value.trim().is_empty()))
            }
        }
    }
}

/// Set `baseUrl` to `url`.
fn set_base_url(collection: &mut ImportedCollection, url: &str) {
    match collection.variables.iter_mut().find(|v| v.key == "baseUrl") {
        Some(v) => v.value = url.to_string(),
        None => collection
            .variables
            .insert(0, Variable { key: "baseUrl".into(), value: url.to_string(), enabled: true, secret: false }),
    }
    collection.warnings.retain(|w| !w.contains("baseUrl"));
}

/// The collection of a GraphQL entry: one request with the sample query.
fn graphql_collection(
    entry: &CatalogEntry,
    url: &str,
    query: &str,
    auth: &Auth,
    headers: &[KeyValue],
    variables: &[Variable],
) -> ImportedCollection {
    let mut request = Request::new(format!("{} query", entry.name), RequestKind::Http);
    request.method = "POST".into();
    request.url = url.to_string();
    request.headers = headers.to_vec();
    request.body = Body {
        body_type: BodyType::Graphql,
        graphql: GraphqlBody { query: query.to_string(), ..Default::default() },
        ..Default::default()
    };
    ImportedCollection {
        name: entry.name.clone(),
        variables: variables.to_vec(),
        auth: auth.clone(),
        items: vec![ImportedItem::Request(request)],
        ..Default::default()
    }
}

/// Count the operations of a parsed document that `only` takes.
fn count_operations(doc: &Value, only: &OperationFilter) -> u32 {
    if only.paths.is_empty() {
        return openapi::outline(doc)
            .groups
            .iter()
            .filter(|g| only.tags.is_empty() || only.tags.contains(&g.tag))
            .map(|g| g.operations)
            .sum();
    }
    let methods = ["get", "put", "post", "delete", "options", "head", "patch", "trace"];
    only.paths
        .iter()
        .filter_map(|p| doc.get("paths").and_then(|paths| paths.get(p)).and_then(Value::as_object))
        .map(|item| item.keys().filter(|m| methods.contains(&m.to_ascii_lowercase().as_str())).count() as u32)
        .sum()
}

/// A downloaded Postman collection or AWS service model, imported whole (off the async workers).
async fn import_whole(text: Arc<String>, kind: ImportKind) -> ApiResult<ImportedCollection> {
    tokio::task::spawn_blocking(move || match kind {
        ImportKind::Smithy => zorvik_workspace::formats::smithy::import_smithy(&text),
        _ => zorvik_workspace::formats::postman::import_postman_collection(&text),
    })
    .await
    .map_err(join_err)?
    .map_err(ApiError::from)
}

/// What an entry brings in, ready to write.
struct Prepared {
    collection: ImportedCollection,
    spec: Option<KeptSpec>,
}

impl Api {
    /// The text at `url`, for `entry`: from the last download when recent, else downloaded.
    /// Failures say what happened and how to get the file by hand.
    async fn catalog_download(&self, entry: &CatalogEntry, url: &str) -> ApiResult<Arc<String>> {
        if let Some(text) = self.inner.catalog.recent(url) {
            return Ok(text);
        }
        let mut opts = crate::request_options(&self.settings(), &Default::default())?;
        opts.max_body_bytes = MAX_IMPORT_FILE as usize;
        // Nothing can cancel the download: never wait forever, even with timeouts turned off.
        opts.timeout.get_or_insert(DOWNLOAD_TIMEOUT);
        // Moved files are often redirects, whatever the user's setting for requests.
        opts.follow_redirects = true;
        opts.max_redirects = opts.max_redirects.max(MIN_REDIRECTS);
        let request = zorvik_engine::HttpRequest {
            method: "GET".into(),
            url: url.to_string(),
            headers: vec![zorvik_engine::Header::new("Accept", "application/json, application/yaml, */*")],
            body: Default::default(),
        };
        let host = host_of(url);
        let response = match self.inner.client.send(request, &opts, None).await {
            Ok(response) => response,
            Err(e) => {
                let e = ApiError::from(e);
                return Err(unavailable(
                    entry,
                    format!(
                        "Couldn't download the {} API description from {host}: {}. Check your internet \
                         connection and try again.",
                        entry.name,
                        e.message.trim_end_matches('.')
                    ),
                ));
            }
        };
        let status = response.meta.status;
        let code = format!(
            "HTTP {status}{}",
            match response.meta.status_text.trim() {
                "" => String::new(),
                text => format!(" {text}"),
            }
        );
        let why = match status {
            200..=299 => None,
            404 | 410 => Some(format!(
                "The {} API description is no longer at {url} ({code}): the publisher moved or removed it.",
                entry.name
            )),
            401 | 403 => Some(format!("{host} refused the download ({code}).")),
            429 => Some(format!("{host} is limiting downloads right now ({code}). Try again in a few minutes.")),
            500..=599 => Some(format!("{host} couldn't send the file right now ({code}). Try again later.")),
            _ => Some(format!("Downloading the {} API description from {host} failed ({code}).", entry.name)),
        };
        if let Some(why) = why {
            return Err(unavailable(entry, why));
        }
        let hops = response.meta.redirects.iter().map(|h| h.url.as_str()).chain([response.meta.url.as_str()]);
        if let Some(plain) = insecure_hop(url, hops) {
            return Err(unavailable(
                entry,
                format!(
                    "The download of the {} API description was redirected to {plain}, which isn't secure (https).",
                    entry.name
                ),
            ));
        }
        if response.body_truncated {
            return Err(unavailable(entry, format!("The {} API description is larger than 50 MB.", entry.name)));
        }
        let content_type = response
            .meta
            .headers
            .iter()
            .find(|h| h.name.eq_ignore_ascii_case("content-type"))
            .map(|h| h.value.as_str());
        let text = crate::document_text(&response.body);
        if is_web_page(content_type, &text) {
            let moved = if response.meta.url != url {
                format!(" (it now redirects to {})", response.meta.url)
            } else {
                String::new()
            };
            return Err(unavailable(
                entry,
                format!(
                    "{url} now leads to a web page instead of the API description{moved}, so the file has \
                     probably moved."
                ),
            ));
        }
        let text = Arc::new(text);
        self.inner.catalog.keep(url, text.clone());
        Ok(text)
    }

    /// The downloaded description of `entry`, parsed as OpenAPI (Google Discovery documents
    /// arrive converted). `None` when it is another kind (see `detect_import`).
    async fn catalog_document(
        &self,
        entry: &'static CatalogEntry,
    ) -> ApiResult<(Arc<String>, ImportKind, Option<Value>)> {
        let (owner, url) = download_of(entry)?;
        let text = self.catalog_download(owner, url).await?;
        // Telling the kind apart parses JSON documents whole: off the async workers too.
        let parsed = text.clone();
        let (kind, doc) = off_workers(move || {
            let kind = detect_import(&parsed);
            let doc = (kind == ImportKind::OpenApi).then(|| openapi::parse_document(&parsed).ok());
            (kind, doc)
        })
        .await?;
        match (kind, doc) {
            (ImportKind::OpenApi, Some(Some(doc))) => Ok((text, kind, Some(doc))),
            (ImportKind::Postman | ImportKind::Smithy, _) => Ok((text, kind, None)),
            _ => {
                // Not kept: trying again downloads it again (it may have been a passing glitch).
                self.inner.catalog.forget(url);
                Err(unavailable(
                    owner,
                    format!(
                        "The file at {url} isn't an API description Zorvik can read any more (the publisher \
                         may have changed it)."
                    ),
                ))
            }
        }
    }

    pub(crate) async fn catalog_preview(&self, id: &str) -> ApiResult<CatalogPreview> {
        self.preview_entry(entry(id)?).await
    }

    async fn preview_entry(&self, entry: &'static CatalogEntry) -> ApiResult<CatalogPreview> {
        let whole = |operations| CatalogPreview { operations, groups: Vec::new(), selected: Vec::new() };
        match &entry.source {
            CatalogSource::Link { reason } => Err(link_error(entry, reason)),
            CatalogSource::Graphql { .. } => Ok(whole(1)),
            CatalogSource::Starter { file, .. } => {
                let doc = openapi::parse_document(starter(file)?)?;
                Ok(whole(count_operations(&doc, &OperationFilter::default())))
            }
            CatalogSource::Compatible { paths, .. } => {
                let (_, _, doc) = self.catalog_document(entry).await?;
                let only = OperationFilter { paths: paths.clone(), ..Default::default() };
                Ok(whole(off_workers(move || doc.map_or(0, |d| count_operations(&d, &only))).await?))
            }
            CatalogSource::Download { groups, .. } => match self.catalog_document(entry).await? {
                (_, _, Some(doc)) => {
                    let outline = off_workers(move || openapi::outline(&doc)).await?;
                    let selected = preset_groups(&outline, groups);
                    Ok(CatalogPreview { operations: outline.operations, groups: outline.groups, selected })
                }
                (text, kind, None) => Ok(whole(import_whole(text, kind).await?.request_count() as u32)),
            },
        }
    }

    /// The collection `entry` imports, taking the groups `groups` (see `CatalogImportParams`).
    async fn catalog_prepare(&self, entry: &'static CatalogEntry, groups: Option<Vec<String>>) -> ApiResult<Prepared> {
        let mut prepared = match &entry.source {
            CatalogSource::Link { reason } => return Err(link_error(entry, reason)),
            CatalogSource::Graphql { url, query, auth, headers, variables } => {
                Prepared { collection: graphql_collection(entry, url, query, auth, headers, variables), spec: None }
            }
            CatalogSource::Starter { file, secrets } => {
                let text = starter(file)?.to_string();
                let doc = openapi::parse_document(&text)?;
                let mut collection = openapi::import_document(&doc, &OperationFilter::default())?;
                set_variables(&mut collection, secrets, &[]);
                // Written for Zorvik, not by the publisher: responses aren't checked against it.
                Prepared {
                    collection,
                    spec: Some(KeptSpec {
                        text,
                        source: String::new(),
                        validate: false,
                        only: Default::default(),
                        scheme: String::new(),
                    }),
                }
            }
            CatalogSource::Compatible { base_url, paths, .. } => {
                let (text, _, doc) = self.catalog_document(entry).await?;
                let (_, url) = download_of(entry)?;
                let Some(doc) = doc else { return Err(ApiError::invalid(format!("{} isn't OpenAPI", entry.name))) };
                let only = OperationFilter { paths: paths.clone(), ..Default::default() };
                let filter = only.clone();
                let mut collection = off_workers(move || openapi::import_document(&doc, &filter)).await??;
                set_base_url(&mut collection, base_url);
                // Compatible APIs answer in their own way at times: checking against OpenAI's
                // schemas would fail requests that work.
                Prepared {
                    collection,
                    spec: Some(KeptSpec {
                        text: self.inner.catalog.take(url, text),
                        source: url.to_string(),
                        validate: false,
                        only,
                        scheme: String::new(),
                    }),
                }
            }
            CatalogSource::Download { url, base_url, groups: preset, scheme, secrets, variables, auth } => {
                let (text, kind, doc) = self.catalog_document(entry).await?;
                let is_openapi = doc.is_some();
                let mut prepared = match doc {
                    Some(mut doc) => {
                        let (scheme_name, preset) = (scheme.clone(), preset.clone());
                        let (collection, only) = off_workers(move || {
                            if let Some(scheme) = &scheme_name {
                                openapi::prefer_scheme(&mut doc, scheme);
                            }
                            // As the preview offered them: presets the document no longer has
                            // are left out, and with none left it comes whole.
                            let tags = groups.unwrap_or_else(|| {
                                if preset.is_empty() {
                                    Vec::new()
                                } else {
                                    preset_groups(&openapi::outline(&doc), &preset)
                                }
                            });
                            let only = OperationFilter { tags, ..Default::default() };
                            openapi::import_document(&doc, &only).map(|collection| (collection, only))
                        })
                        .await??;
                        // The publisher's text is kept as it came (a Google Discovery document is
                        // converted again whenever it's read); the scheme is remembered for updates.
                        Prepared {
                            collection,
                            spec: Some(KeptSpec {
                                text: self.inner.catalog.take(url, text),
                                source: url.clone(),
                                validate: true,
                                only,
                                scheme: scheme.clone().unwrap_or_default(),
                            }),
                        }
                    }
                    None => {
                        let collection = import_whole(text, kind).await?;
                        self.inner.catalog.forget(url);
                        Prepared { collection, spec: None }
                    }
                };
                match base_url {
                    Some(base) => set_base_url(&mut prepared.collection, base),
                    // A relative OpenAPI server URL is relative to where the description is (an
                    // AWS model or a Postman collection that names no address isn't).
                    None if is_openapi => {
                        let origin = url::Url::parse(url).map(|u| u.origin().ascii_serialization()).ok();
                        crate::apply_base_url(&mut prepared.collection, origin.as_deref())?;
                    }
                    None => {}
                }
                set_variables(&mut prepared.collection, secrets, variables);
                if let Some(auth) = auth {
                    prepared.collection.auth = auth.clone();
                    drop_auth_headers(&mut prepared.collection.items, auth);
                }
                prepared
            }
        };
        prepared.collection.name = entry.name.clone();
        Ok(prepared)
    }

    pub(crate) async fn catalog_import(&self, p: CatalogImportParams) -> ApiResult<CatalogImport> {
        let entry = entry(&p.id)?;
        let ws = self.ws()?;
        let Prepared { collection, spec } = self.catalog_prepare(entry, p.groups).await?;
        if collection.request_count() == 0 {
            return Err(ApiError::invalid(format!(
                "Nothing to import: no requests in the chosen part of {}",
                entry.name
            )));
        }
        let summary = self.write_collection(&ws, p.parent, collection, spec).await?;
        if let Some(folder) = &summary.folder_path {
            let mut meta = ws.read_folder(folder)?;
            meta.docs = folder_docs(entry, &meta.docs);
            ws.save_folder(folder, &meta)?;
        }
        if let Some(id) = &summary.environment_id {
            let mut local = lock(&self.inner.local);
            local.active_env.insert(ws.meta().id.clone(), id.clone());
            local.save(&self.inner.data_dir.join("state.json"));
        }
        // Reads the new requests one by one (thousands, for a large API): off the async workers.
        let open = match (entry.first.clone(), summary.folder_path.clone()) {
            (Some(first), Some(folder)) => off_workers(move || first_request(&ws, &folder, &first)).await?,
            _ => None,
        };
        Ok(CatalogImport { summary, open })
    }
}

/// The folder's docs: what the API is, where it came from, under which license.
fn folder_docs(entry: &'static CatalogEntry, docs: &str) -> String {
    let mut out = format!("{}\n\nAPI docs: {}\n", entry.description, entry.website);
    if let Ok((owner, url)) = download_of(entry) {
        out.push_str(&format!("\nImported from the API catalog: {url}"));
        if let Some(license) = &owner.license {
            match license.url.as_str() {
                "" => out.push_str(&format!(" (license: {})", license.name)),
                link => out.push_str(&format!(" (license: [{}]({link}))", license.name)),
            }
        }
        out.push('\n');
    }
    if !docs.trim().is_empty() {
        out.push('\n');
        out.push_str(docs);
    }
    out
}

/// The request under `folder` matching `first` (an operation like `GET /v1/models`, or a name).
fn first_request(ws: &Workspace, folder: &str, first: &str) -> Option<String> {
    let tree = ws.tree().ok()?;
    let node = find(&tree, folder).filter(|n| n.kind == NodeKind::Folder)?;
    let mut paths = Vec::new();
    requests_under(&node.children, &mut paths);
    paths.into_iter().find(|path| {
        ws.read_request(path).is_ok_and(|r| {
            r.openapi.as_ref().is_some_and(|o| o.operation.eq_ignore_ascii_case(first)) || r.name == first
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn every_entry_is_complete() {
        let mut ids = HashSet::new();
        for e in entries() {
            assert!(ids.insert(e.id.as_str()), "{}: duplicate id", e.id);
            assert!(
                e.id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
                "{}: ids are lowercase-with-dashes",
                e.id
            );
            assert!(!e.name.trim().is_empty() && !e.description.trim().is_empty(), "{}: name and description", e.id);
            assert!(e.website.starts_with("https://"), "{}: website must be https", e.id);
            if let Some(license) = &e.license {
                assert!(!license.name.trim().is_empty(), "{}: license name", e.id);
                assert!(
                    license.url.is_empty() || license.url.starts_with("https://"),
                    "{}: license url must be https",
                    e.id
                );
            }
            assert!(!e.auth.trim().is_empty() && !e.format.trim().is_empty(), "{}: auth and format", e.id);
            match &e.source {
                CatalogSource::Download { url, base_url, auth, variables, .. } => {
                    assert!(url.starts_with("https://"), "{}: downloads use https", e.id);
                    if let Some(auth) = auth {
                        let used = serde_json::to_string(auth).unwrap();
                        for v in used.split("{{").skip(1).filter_map(|p| p.split_once("}}")).map(|(v, _)| v) {
                            assert!(
                                variables.iter().any(|x| x.key == v && x.secret),
                                "{}: auth uses {{{{{v}}}}}: declare it in variables as a secret",
                                e.id
                            );
                        }
                    }
                    assert!(base_url.as_deref().is_none_or(|b| b.contains("://")), "{}: base URL", e.id);
                    assert!(e.license.is_some(), "{}: say the license (\"Not stated\" when none)", e.id);
                }
                CatalogSource::Compatible { of, base_url, paths } => {
                    let of = entry(of).unwrap_or_else(|_| panic!("{}: compatible with unknown {of}", e.id));
                    assert!(
                        matches!(of.source, CatalogSource::Download { .. }),
                        "{}: compatible with a download",
                        e.id
                    );
                    assert!(base_url.contains("://") && !paths.is_empty(), "{}: base URL and paths", e.id);
                }
                CatalogSource::Starter { file, .. } => {
                    let text = starter(file).unwrap_or_else(|_| panic!("{}: missing starter {file}", e.id));
                    let collection = openapi::import_openapi(text).unwrap();
                    assert!(collection.request_count() > 0, "{}: starter has requests", e.id);
                }
                CatalogSource::Graphql { url, query, .. } => {
                    assert!(url.starts_with("https://") && !query.trim().is_empty(), "{}: URL and query", e.id);
                }
                CatalogSource::Link { reason } => assert!(reason.ends_with('.'), "{}: reason is a sentence", e.id),
            }
        }
        for (file, _) in STARTERS {
            assert!(
                entries().iter().any(|e| matches!(&e.source, CatalogSource::Starter { file: f, .. } if f == file)),
                "starter {file} is used by an entry"
            );
        }
    }

    #[test]
    fn no_entry_has_unknown_fields() {
        let mut unknown = Vec::new();
        let _: Vec<CatalogEntry> = serde_ignored::deserialize(serde_yaml_ng::Deserializer::from_str(CATALOG), |path| {
            unknown.push(path.to_string())
        })
        .unwrap();
        assert!(unknown.is_empty(), "unknown fields in catalog.yaml: {unknown:?}");
    }

    #[test]
    fn starters_name_their_first_request() {
        for e in entries() {
            let (CatalogSource::Starter { file, .. }, Some(first)) = (&e.source, &e.first) else { continue };
            let collection = openapi::import_openapi(starter(file).unwrap()).unwrap();
            fn all(items: &[ImportedItem], out: &mut Vec<Request>) {
                for i in items {
                    match i {
                        ImportedItem::Folder { children, .. } => all(children, out),
                        ImportedItem::Request(r) => out.push(r.clone()),
                    }
                }
            }
            let mut requests = Vec::new();
            all(&collection.items, &mut requests);
            assert!(
                requests.iter().any(|r| r.openapi.as_ref().is_some_and(|o| o.operation.eq_ignore_ascii_case(first))),
                "{}: no request {first}",
                e.id
            );
        }
    }

    #[test]
    fn a_preferred_scheme_replaces_the_alternatives() {
        let mut doc = serde_json::json!({
            "openapi": "3.0.3",
            "components": {"securitySchemes": {"oauth2": {"type": "oauth2"}, "apiKey": {"type": "apiKey", "in": "query", "name": "key"}}},
            "security": [{"oauth2": ["a"]}, {"apiKey": []}],
            "paths": {"/x": {"get": {"security": [{"oauth2": ["b"]}]}, "post": {}, "put": {"security": []}}}
        });
        assert!(openapi::prefer_scheme(&mut doc, "apiKey"));
        assert_eq!(doc["security"], serde_json::json!([{"apiKey": []}]));
        // Operations inherit it, except those that need no auth at all.
        assert!(doc["paths"]["/x"]["get"].get("security").is_none());
        assert!(doc["paths"]["/x"]["post"].get("security").is_none());
        assert_eq!(doc["paths"]["/x"]["put"]["security"], serde_json::json!([]));
        // A document-wide requirement combining the scheme with another (a key and a token) stays.
        let mut combined = serde_json::json!({
            "openapi": "3.0.3",
            "components": {"securitySchemes": {"key": {"type": "apiKey"}, "token": {"type": "apiKey"}, "oauth": {"type": "oauth2"}}},
            "security": [{"key": [], "token": []}],
            "paths": {"/x": {"get": {"security": [{"oauth": ["read"]}]}}}
        });
        assert!(openapi::prefer_scheme(&mut combined, "key"));
        assert_eq!(combined["security"], serde_json::json!([{"key": [], "token": []}]));
        assert!(combined["paths"]["/x"]["get"].get("security").is_none());
        // An unknown scheme leaves the document as it was.
        let before = doc.clone();
        assert!(!openapi::prefer_scheme(&mut doc, "nope"));
        assert_eq!(doc, before);
    }

    #[test]
    fn empty_credential_headers_make_way_for_the_entry_auth() {
        let mut request = Request::new("Get", RequestKind::Http);
        request.headers =
            vec![KeyValue::new("X-Token", ""), KeyValue::new("x-token", "kept"), KeyValue::new("Accept", "")];
        let mut items =
            vec![ImportedItem::Folder { meta: Default::default(), children: vec![ImportedItem::Request(request)] }];
        let auth = Auth::ApiKey { key: "X-Token".into(), value: "{{token}}".into(), location: Default::default() };
        drop_auth_headers(&mut items, &auth);
        let ImportedItem::Folder { children, .. } = &items[0] else { unreachable!() };
        let ImportedItem::Request(r) = &children[0] else { unreachable!() };
        let keys: Vec<_> = r.headers.iter().map(|h| (h.key.as_str(), h.value.as_str())).collect();
        assert_eq!(keys, [("x-token", "kept"), ("Accept", "")]);
    }

    #[test]
    fn a_redirect_from_https_to_plain_http_is_refused() {
        let start = "https://raw.example.com/api.json";
        assert_eq!(insecure_hop(start, [start, "https://cdn.example.com/api.json"]), None);
        assert_eq!(
            insecure_hop(start, [start, "http://cdn.example.com/api.json", "https://cdn.example.com/x"]),
            Some("http://cdn.example.com/api.json")
        );
        assert_eq!(insecure_hop(start, ["HTTPS://RAW.EXAMPLE.COM/api.json"]), None);
        // A download that starts on plain http (local servers in tests) isn't held to it.
        assert_eq!(insecure_hop("http://127.0.0.1:1/x", ["http://127.0.0.1:1/x"]), None);
    }

    #[test]
    fn web_pages_are_told_apart_from_descriptions() {
        assert!(is_web_page(Some("text/html; charset=utf-8"), "{}"));
        assert!(is_web_page(None, "  <!DOCTYPE html><html>"));
        assert!(!is_web_page(Some("application/json"), "{\"openapi\": \"3.0.0\"}"));
        assert!(!is_web_page(Some("text/plain"), "openapi: 3.1.0"));
    }

    #[test]
    fn counting_follows_tags_and_paths() {
        let doc = serde_json::json!({"openapi": "3.0.0", "paths": {
            "/a": {"get": {"tags": ["one"]}, "post": {"tags": ["two"]}},
            "/b": {"get": {}}
        }});
        assert_eq!(count_operations(&doc, &OperationFilter::default()), 3);
        assert_eq!(count_operations(&doc, &OperationFilter { tags: vec!["one".into()], ..Default::default() }), 1);
        assert_eq!(count_operations(&doc, &OperationFilter { tags: vec!["".into()], ..Default::default() }), 1);
        assert_eq!(count_operations(&doc, &OperationFilter { paths: vec!["/a".into()], ..Default::default() }), 2);
    }

    // ---- importing, against local servers -------------------------------------------------

    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    struct NoEvents;

    impl crate::EventSink for NoEvents {
        fn emit(&self, _: crate::StreamEvent) {}
    }

    /// An API with a workspace open; the folders live as long as the guard.
    async fn api() -> (Api, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let api = Api::new(dir.path().join("data"), Arc::new(NoEvents));
        let ws = dir.path().join("ws");
        api.call("workspace.create", serde_json::json!({ "path": ws, "name": "Test" })).await.unwrap();
        (api, dir)
    }

    /// Answer every request on 127.0.0.1 with `status` and `body`; the URL to ask.
    async fn serve(status: u16, content_type: &'static str, body: String) -> String {
        serve_in_turn(status, content_type, vec![body]).await.0
    }

    /// Answer the requests on 127.0.0.1 with `bodies` in turn (then the last one again); the
    /// URL to ask, and how many requests came.
    async fn serve_in_turn(
        status: u16,
        content_type: &'static str,
        bodies: Vec<String>,
    ) -> (String, Arc<std::sync::atomic::AtomicUsize>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let served = count.clone();
        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let n = served.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let body = bodies[n.min(bodies.len() - 1)].clone();
                tokio::spawn(async move {
                    let mut head = [0u8; 4096];
                    let _ = stream.read(&mut head).await;
                    let reply = format!(
                        "HTTP/1.1 {status} Status\r\ncontent-type: {content_type}\r\ncontent-length: {}\r\n\
                         connection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = stream.write_all(reply.as_bytes()).await;
                });
            }
        });
        (format!("http://{addr}/description"), count)
    }

    fn download_entry(
        url: String,
        groups: Vec<String>,
        scheme: Option<&str>,
        secrets: Vec<String>,
    ) -> &'static CatalogEntry {
        Box::leak(Box::new(CatalogEntry {
            id: "pets".into(),
            name: "Pets".into(),
            category: CatalogCategory::DevTools,
            description: "Pets for tests.".into(),
            website: "https://example.com/docs".into(),
            auth: "API key".into(),
            format: "OpenAPI 3.0".into(),
            license: Some(CatalogLicense { name: "MIT".into(), url: String::new() }),
            source: CatalogSource::Download {
                url,
                base_url: None,
                groups,
                scheme: scheme.map(str::to_string),
                secrets,
                variables: vec![Variable { key: "storeId".into(), value: "main".into(), enabled: true, secret: false }],
                auth: None,
            },
            first: Some("GET /pets".into()),
        }))
    }

    const PETS: &str = r#"{
        "openapi": "3.0.3",
        "info": {"title": "Pet store", "version": "1"},
        "servers": [{"url": "/v1"}],
        "tags": [{"name": "pets"}, {"name": "stores"}],
        "components": {"securitySchemes": {
            "bearer": {"type": "http", "scheme": "bearer"},
            "key": {"type": "apiKey", "in": "header", "name": "X-Key"}
        }},
        "security": [{"bearer": []}, {"key": []}],
        "paths": {
            "/pets": {"get": {"tags": ["pets"], "summary": "List pets"}, "post": {"tags": ["pets"]}},
            "/stores/{storeId}": {"get": {"tags": ["stores"], "parameters": [
                {"name": "storeId", "in": "path", "required": true, "schema": {"type": "string"}}
            ]}},
            "/health": {"get": {}}
        }
    }"#;

    #[tokio::test]
    async fn a_download_imports_the_chosen_groups_with_its_environment() {
        let (api, _dir) = api().await;
        let url = serve(200, "application/json", PETS.into()).await;
        let entry = download_entry(url.clone(), vec!["pets".into()], Some("key"), vec![]);

        let preview = match &entry.source {
            CatalogSource::Download { .. } => {
                let (_, _, doc) = api.catalog_document(entry).await.unwrap();
                openapi::outline(&doc.unwrap())
            }
            _ => unreachable!(),
        };
        let tags: Vec<_> = preview.groups.iter().map(|g| (g.tag.as_str(), g.operations)).collect();
        assert_eq!(tags, [("pets", 2), ("stores", 1), ("", 1)]);

        let ws = api.ws().unwrap();
        let Prepared { collection, spec } = api.catalog_prepare(entry, None).await.unwrap();
        assert_eq!(collection.name, "Pets");
        assert_eq!(collection.request_count(), 2, "only the preset group");
        // The preferred scheme won, and the relative server URL is relative to the download.
        assert!(matches!(collection.auth, Auth::ApiKey { .. }), "{:?}", collection.auth);
        let origin = url::Url::parse(&url).unwrap().origin().ascii_serialization();
        assert!(collection.variables.iter().any(|v| v.key == "baseUrl" && v.value == format!("{origin}/v1")));
        let spec = spec.unwrap();
        assert_eq!(spec.only.tags, ["pets"]);
        assert_eq!(spec.source, url);
        assert_eq!(spec.scheme, "key");

        let summary = api.write_collection(&ws, String::new(), collection, Some(spec)).await.unwrap();
        let folder = summary.folder_path.clone().unwrap();
        let meta = ws.read_folder(&folder).unwrap();
        assert_eq!(meta.openapi.as_ref().unwrap().only.tags, ["pets"]);
        assert!(summary.environment_id.is_some());
        assert_eq!(
            first_request(&ws, &folder, "GET /pets").map(|p| ws.read_request(&p).unwrap().name).as_deref(),
            Some("List pets")
        );

        // Picking other groups takes those instead.
        let picked = api.catalog_prepare(entry, Some(vec!["stores".into(), "".into()])).await.unwrap();
        assert_eq!(picked.collection.request_count(), 2);
        // The entry's own starting value replaces the document's example.
        assert!(picked.collection.variables.iter().any(|v| v.key == "storeId" && v.value == "main"));
    }

    #[tokio::test]
    async fn updating_keeps_to_the_chosen_groups_and_scheme() {
        let (api, _dir) = api().await;
        let url = serve(200, "application/json", PETS.into()).await;
        let entry = download_entry(url, vec!["pets".into()], Some("key"), vec![]);
        let ws = api.ws().unwrap();
        let Prepared { collection, spec } = api.catalog_prepare(entry, None).await.unwrap();
        let summary = api.write_collection(&ws, String::new(), collection, spec).await.unwrap();
        let folder = summary.folder_path.unwrap();
        let auth = ws.read_folder(&folder).unwrap().auth;
        assert!(matches!(&auth, Auth::ApiKey { key, .. } if key == "X-Key"), "{auth:?}");

        // A new version adds an operation to each group.
        let newer = PETS.replace(
            r#""/health": {"get": {}}"#,
            r#""/health": {"get": {}}, "/pets/{petId}": {"get": {"tags": ["pets"]}}, "/stores": {"get": {"tags": ["stores"]}}"#,
        );
        let params =
            crate::spec_update::SpecUpdateParams { folder: folder.clone(), text: Some(newer), path: None, url: None };
        let update = api.spec_update(params, true).await.unwrap();
        let added: Vec<_> = update.added.iter().map(|c| c.operation.as_str()).collect();
        assert_eq!(added, ["GET /pets/{petId}"], "only the chosen group grows");
        assert!(update.changed.is_empty(), "auth stays on the chosen scheme: {:?}", update.changed);
    }

    #[tokio::test]
    async fn importing_activates_the_new_environment_and_documents_the_folder() {
        let (api, _dir) = api().await;
        let url = serve(200, "application/yaml", PETS.into()).await;
        let entry = download_entry(url, vec![], None, vec![]);
        let Prepared { collection, spec } = api.catalog_prepare(entry, None).await.unwrap();
        assert_eq!(collection.request_count(), 4);
        // What `catalog_import` does after writing, minus finding the entry by id.
        let ws = api.ws().unwrap();
        let summary = api.write_collection(&ws, String::new(), collection, spec).await.unwrap();
        let folder = summary.folder_path.clone().unwrap();
        let docs = folder_docs(entry, "");
        assert!(docs.contains("https://example.com/docs") && docs.contains("license: MIT"), "{docs}");
        assert!(ws.read_folder(&folder).unwrap().openapi.unwrap().validate);
    }

    #[tokio::test]
    async fn the_starter_and_graphql_entries_import_without_downloading() {
        let (api, _dir) = api().await;
        let anthropic = api
            .catalog_import(CatalogImportParams { id: "anthropic".into(), parent: String::new(), groups: None })
            .await
            .unwrap();
        assert!(anthropic.summary.requests >= 10);
        let ws = api.ws().unwrap();
        let opened = ws.read_request(anthropic.open.as_deref().unwrap()).unwrap();
        assert_eq!(opened.method, "POST");
        assert!(opened.headers.iter().any(|h| h.key == "anthropic-version" && h.value == "2023-06-01"));
        // The Files API is out of beta: its old beta header would bring back the beta's answers.
        let folder = anthropic.summary.folder_path.clone().unwrap();
        let tree = ws.tree().unwrap();
        let mut paths = Vec::new();
        requests_under(&find(&tree, &folder).unwrap().children, &mut paths);
        for path in paths {
            let r = ws.read_request(&path).unwrap();
            assert!(!r.headers.iter().any(|h| h.key.eq_ignore_ascii_case("anthropic-beta")), "{}", r.name);
        }
        // The key is a secret of the active environment.
        let env_id = anthropic.summary.environment_id.clone().unwrap();
        let env = ws.read_environment(&env_id).unwrap();
        assert!(env.variables.iter().any(|v| v.secret), "{:?}", env.variables);
        assert_eq!(lock(&api.inner.local).active_env.get(&ws.meta().id), Some(&env_id));
        let meta = ws.read_folder(anthropic.summary.folder_path.as_deref().unwrap()).unwrap();
        assert!(!meta.openapi.unwrap().validate, "starters aren't the publisher's: responses aren't checked");

        let graphql = api
            .catalog_import(CatalogImportParams { id: "github-graphql".into(), parent: String::new(), groups: None })
            .await
            .unwrap();
        assert_eq!(graphql.summary.requests, 1);
        let folder = graphql.summary.folder_path.unwrap();
        let path = first_request(&ws, &folder, "GitHub GraphQL query").unwrap();
        let request = ws.read_request(&path).unwrap();
        assert_eq!(request.body.body_type, BodyType::Graphql);
        assert!(request.body.graphql.query.contains("viewer"));
    }

    #[tokio::test]
    async fn link_entries_say_why_and_where_to_go() {
        let (api, _dir) = api().await;
        let e = api
            .catalog_import(CatalogImportParams { id: "hubspot".into(), parent: String::new(), groups: None })
            .await
            .unwrap_err();
        assert_eq!(e.code, "catalogLink");
        assert!(e.message.contains("developers.hubspot.com"), "{}", e.message);
        let e = api.catalog_preview("nope").await.unwrap_err();
        assert_eq!(e.code, "notFound");
    }

    /// The error importing from `url` gives.
    async fn failure(url: String) -> ApiError {
        let (api, _dir) = api().await;
        let entry = download_entry(url, vec![], None, vec![]);
        match api.catalog_prepare(entry, None).await {
            Ok(_) => panic!("expected a failure"),
            Err(e) => e,
        }
    }

    #[tokio::test]
    async fn failed_downloads_say_what_happened_and_how_to_import_by_hand() {
        let by_hand = "download the Pets API description from https://example.com/docs yourself and import the file";
        for (status, said) in [
            (404, "is no longer at"),
            (410, "moved or removed"),
            (403, "refused the download"),
            (429, "Try again in a few minutes"),
            (503, "Try again later"),
            (418, "failed (HTTP 418"),
        ] {
            let e = failure(serve(status, "text/plain", "nope".into()).await).await;
            assert_eq!(e.code, "catalogUnavailable", "{status}");
            assert!(e.message.contains(said) && e.message.contains(by_hand), "{status}: {}", e.message);
        }
        // A file that became a web page, directly or through a redirect.
        let page = serve(200, "text/html", "<!doctype html><title>Moved</title>".into()).await;
        let e = failure(page.clone()).await;
        assert!(e.message.contains("leads to a web page"), "{}", e.message);
        let server = zorvik_testkit::TestServer::start().await;
        let redirect = server.url(&format!("/redirect-to?url={page}"));
        let e = failure(redirect).await;
        assert!(e.message.contains(&format!("it now redirects to {page}")), "{}", e.message);
        // Something that isn't a description.
        let e = failure(serve(200, "application/json", "{\"hello\": 1}".into()).await).await;
        assert!(e.message.contains("isn't an API description Zorvik can read"), "{}", e.message);
        // Nobody listening.
        let closed = {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            format!("http://{}/description", listener.local_addr().unwrap())
        };
        let e = failure(closed).await;
        assert_eq!(e.code, "catalogUnavailable");
        assert!(e.message.contains("Couldn't download the Pets API description from 127.0.0.1"), "{}", e.message);
        assert!(e.message.contains("Check your internet connection"), "{}", e.message);
    }

    #[tokio::test]
    async fn a_download_is_reused_for_a_while() {
        let (api, _dir) = api().await;
        let url = serve(200, "application/json", PETS.into()).await;
        let entry = download_entry(url.clone(), vec![], None, vec![]);
        let first = api.catalog_download(entry, &url).await.unwrap();
        let again = api.catalog_download(entry, &url).await.unwrap();
        assert!(Arc::ptr_eq(&first, &again));
    }

    #[tokio::test]
    async fn an_unreadable_download_is_downloaded_again_and_an_imported_one_let_go() {
        let (api, _dir) = api().await;
        // A passing glitch: the first answer isn't a description, the next ones are.
        let (url, count) = serve_in_turn(200, "application/json", vec!["{\"hello\": 1}".into(), PETS.into()]).await;
        let entry = download_entry(url.clone(), vec![], None, vec![]);
        let e = api.preview_entry(entry).await.unwrap_err();
        assert!(e.message.contains("isn't an API description Zorvik can read"), "{}", e.message);
        let preview = api.preview_entry(entry).await.unwrap();
        assert_eq!(preview.operations, 4);
        assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 2, "trying again downloads again");
        // Importing after the preview reuses its download, then lets go of it.
        let prepared = api.catalog_prepare(entry, None).await.unwrap();
        assert_eq!(prepared.collection.request_count(), 4);
        assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 2);
        assert!(api.inner.catalog.recent(&url).is_none(), "the imported download isn't held on to");
        assert_eq!(prepared.spec.unwrap().text, PETS);
    }

    #[tokio::test]
    async fn presets_the_document_no_longer_has_are_left_out() {
        let (api, _dir) = api().await;
        let url = serve(200, "application/json", PETS.into()).await;
        // The publisher renamed every preset group: the preview ticks all, and so does the import.
        let gone = download_entry(url.clone(), vec!["Pets (old)".into()], None, vec![]);
        assert!(api.preview_entry(gone).await.unwrap().selected.is_empty());
        let Prepared { collection, spec } = api.catalog_prepare(gone, None).await.unwrap();
        assert_eq!(collection.request_count(), 4);
        assert!(spec.unwrap().only.tags.is_empty());
        // Some still there: those are taken.
        let some = download_entry(url, vec!["Pets (old)".into(), "pets".into()], None, vec![]);
        assert_eq!(api.preview_entry(some).await.unwrap().selected, ["pets"]);
        let Prepared { collection, spec } = api.catalog_prepare(some, None).await.unwrap();
        assert_eq!(collection.request_count(), 2);
        assert_eq!(spec.unwrap().only.tags, ["pets"]);
    }

    #[tokio::test]
    async fn an_aws_model_without_an_address_doesnt_take_the_download_host() {
        let (api, _dir) = api().await;
        let model = r#"{"smithy": "2.0", "shapes": {
            "example.weather#Weather": {"type": "service", "version": "1",
              "operations": [{"target": "example.weather#Ping"}],
              "traits": {"aws.protocols#restJson1": {}}},
            "example.weather#Ping": {"type": "operation", "traits": {"smithy.api#http": {"method": "GET", "uri": "/ping"}}}
        }}"#;
        let url = serve(200, "application/json", model.into()).await;
        let Prepared { collection, .. } =
            api.catalog_prepare(download_entry(url, vec![], None, vec![]), None).await.unwrap();
        assert_eq!(collection.request_count(), 1);
        let base = collection.variables.iter().find(|v| v.key == "baseUrl").unwrap();
        assert_eq!(base.value, "", "where the file is says nothing about where the API runs");
        assert!(collection.warnings.iter().any(|w| w.contains("baseUrl")), "{:?}", collection.warnings);
    }

    #[tokio::test]
    async fn moved_files_are_followed_whatever_the_redirect_setting() {
        let (api, _dir) = api().await;
        api.update_settings(|s| s.request.max_redirects = 0).unwrap();
        let url = serve(200, "application/json", PETS.into()).await;
        let server = zorvik_testkit::TestServer::start().await;
        let entry = download_entry(server.url(&format!("/redirect-to?url={url}")), vec![], None, vec![]);
        assert_eq!(api.preview_entry(entry).await.unwrap().operations, 4);
    }

    #[tokio::test]
    async fn updating_adds_variables_to_the_active_environment_of_that_name() {
        let (api, _dir) = api().await;
        let url = serve(200, "application/json", PETS.into()).await;
        let entry = download_entry(url, vec![], None, vec![]);
        let ws = api.ws().unwrap();
        // The same API imported twice: two environments named "Pets".
        let mut imported = Vec::new();
        for _ in 0..2 {
            let Prepared { collection, spec } = api.catalog_prepare(entry, None).await.unwrap();
            imported.push(api.write_collection(&ws, String::new(), collection, spec).await.unwrap());
        }
        let newer = PETS.replace(
            r#""/health": {"get": {}}"#,
            r#""/health": {"get": {}}, "/pets/{petId}": {"get": {"tags": ["pets"]}}"#,
        );
        let has_pet = |i: usize| {
            let id = imported[i].environment_id.as_deref().unwrap();
            ws.read_environment(id).unwrap().variables.iter().any(|v| v.key == "petId")
        };
        // Each folder in turn, with its environment active: the new variable goes there (whichever
        // of the two environments the workspace lists first).
        for i in [0, 1] {
            let active = imported[i].environment_id.clone().unwrap();
            api.call("env.setActive", serde_json::json!({ "id": active })).await.unwrap();
            let folder = imported[i].folder_path.clone().unwrap();
            let params =
                crate::spec_update::SpecUpdateParams { folder, text: Some(newer.clone()), path: None, url: None };
            let update = api.spec_update(params, true).await.unwrap();
            assert_eq!(update.variables, ["petId"], "folder {i}");
            assert!(has_pet(i), "folder {i}");
            assert_eq!(has_pet(1 - i), i == 1, "folder {i}: the other environment is left alone");
        }
    }

    #[tokio::test]
    async fn operations_an_update_adds_send_the_entry_auth_too() {
        let (api, _dir) = api().await;
        // The credential is a header parameter of every operation, and the entry's auth.
        let keyed = r##"{"openapi": "3.0.3", "info": {"title": "Keyed", "version": "1"},
            "servers": [{"url": "https://api.example.com"}],
            "components": {"parameters": {"token": {"name": "X-Token", "in": "header", "required": true, "schema": {"type": "string"}}}},
            "paths": {"/a": {"get": {"parameters": [{"$ref": "#/components/parameters/token"}]}}}}"##;
        let url = serve(200, "application/json", keyed.into()).await;
        let mut entry = download_entry(url, vec![], None, vec![]).clone();
        if let CatalogSource::Download { auth, variables, .. } = &mut entry.source {
            *auth =
                Some(Auth::ApiKey { key: "X-Token".into(), value: "{{token}}".into(), location: Default::default() });
            variables.push(Variable { key: "token".into(), value: String::new(), enabled: true, secret: true });
        }
        let entry: &'static CatalogEntry = Box::leak(Box::new(entry));
        let ws = api.ws().unwrap();
        let Prepared { collection, spec } = api.catalog_prepare(entry, None).await.unwrap();
        let folder = api.write_collection(&ws, String::new(), collection, spec).await.unwrap().folder_path.unwrap();
        let token_header = |operation: &str| {
            let path = first_request(&ws, &folder, operation).unwrap();
            ws.read_request(&path).unwrap().headers.iter().any(|h| h.key.eq_ignore_ascii_case("X-Token"))
        };
        assert!(!token_header("GET /a"));

        let newer = keyed.replace(
            r##""/a": {"get""##,
            r##""/b": {"get": {"parameters": [{"$ref": "#/components/parameters/token"}]}}, "/a": {"get""##,
        );
        let params =
            crate::spec_update::SpecUpdateParams { folder: folder.clone(), text: Some(newer), path: None, url: None };
        let update = api.spec_update(params, true).await.unwrap();
        assert_eq!(update.added.len(), 1);
        assert!(update.changed.is_empty(), "{:?}", update.changed);
        assert!(!token_header("GET /b"), "an empty X-Token header would keep the auth's from being sent");
    }
}
