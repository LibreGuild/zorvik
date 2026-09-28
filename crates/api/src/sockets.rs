//! Socket client sessions (TCP, UDP, MQTT, Socket.IO, GraphQL subscriptions): `socket.connect`,
//! `socket.send`, `socket.close`. Events go to the UI as [`StreamEvent::Socket`].

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio_util::sync::CancellationToken;
use ts_rs::TS;
use zorvik_engine::mqtt::{MqttConfig, MqttProtocol};
use zorvik_engine::{EngineError, SocketConfig, SocketConnected, SocketOpened, SocketOutgoing, SocketSession};
use zorvik_workspace::formats::{MqttVersion, Request, RequestKind};
use zorvik_workspace::resolve::Resolved;

use crate::{Api, ApiError, ApiResult, StreamEvent, lock, ok, params, remove_if_current};

/// A socket session by caller id. Registered before connecting (`session` is
/// `None` until then) so `socket.close` can abort a pending connect.
pub(crate) struct SocketEntry {
    generation: u64,
    cancel: CancellationToken,
    session: Option<SocketSession>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SocketConnectResult {
    pub opened: SocketOpened,
    /// Variables that were referenced but not defined.
    pub unresolved: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConnectParams {
    conn_id: String,
    request: Request,
    path: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConnParam {
    conn_id: String,
}

impl Api {
    /// `socket.*` methods.
    pub(crate) async fn call_socket(&self, method: &str, p: Value) -> ApiResult<Value> {
        match method {
            "socket.connect" => ok(self.socket_connect(params(p)?).await?),
            "socket.send" => {
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase")]
                struct P {
                    conn_id: String,
                    message: SocketOutgoing,
                }
                let P { conn_id, message } = params(p)?;
                let sessions = lock(&self.inner.socket_sessions);
                let session = sessions
                    .get(&conn_id)
                    .and_then(|e| e.session.as_ref())
                    .ok_or_else(|| ApiError::new("notFound", "Not connected"))?;
                session.send(message)?;
                ok(())
            }
            "socket.close" => {
                let ConnParam { conn_id } = params(p)?;
                // Dropping the session closes the connection (its Closed event follows);
                // a connect still in progress is aborted.
                if let Some(entry) = lock(&self.inner.socket_sessions).remove(&conn_id) {
                    entry.cancel.cancel();
                }
                ok(())
            }
            other => Err(ApiError::new("notFound", format!("Unknown method '{other}'"))),
        }
    }

    async fn socket_connect(&self, p: ConnectParams) -> ApiResult<SocketConnectResult> {
        let ws = self.ws()?;
        let settings = self.settings();
        let cancel = CancellationToken::new();
        let generation = self.next_generation();
        let entry = SocketEntry { generation, cancel: cancel.clone(), session: None };
        if let Some(old) = lock(&self.inner.socket_sessions).insert(p.conn_id.clone(), entry) {
            old.cancel.cancel();
        }
        let connected = tokio::select! {
            r = async {
                let mut resolved = self.prepare(&ws, &p.request, p.path.as_deref())?;
                let opts = crate::request_options(&settings, &p.request.settings)?;
                self.authorize(&ws, &mut resolved, &opts).await?;
                let jar = settings.cookie_jar.then(|| self.jar(&ws));
                let unresolved = resolved.unresolved.clone();
                let conn = self.connect_socket(&p.request, resolved, &opts, jar.as_ref()).await?;
                if let Some(jar) = &jar {
                    self.save_jar(&ws, jar);
                }
                Ok::<_, ApiError>((conn, unresolved))
            } => r,
            _ = cancel.cancelled() => Err(EngineError::cancelled().into()),
        };
        let (conn, unresolved) = match connected {
            Ok(c) => c,
            Err(e) => {
                remove_if_current(&self.inner.socket_sessions, &p.conn_id, |e| e.generation == generation);
                return Err(e);
            }
        };
        let SocketConnected { opened, session, mut events, .. } = conn;
        match lock(&self.inner.socket_sessions).get_mut(&p.conn_id) {
            Some(entry) if entry.generation == generation => entry.session = Some(session),
            // Closed or replaced while connecting: dropping the session closes it.
            _ => return Err(EngineError::cancelled().into()),
        }
        let inner = self.inner.clone();
        let conn_id = p.conn_id.clone();
        tokio::spawn(async move {
            while let Some(event) = events.recv().await {
                let closed = matches!(event, zorvik_engine::SocketEvent::Closed { .. });
                inner.sink.emit(StreamEvent::Socket { conn_id: conn_id.clone(), event });
                if closed {
                    break;
                }
            }
            remove_if_current(&inner.socket_sessions, &conn_id, |e| e.generation == generation);
        });
        Ok(SocketConnectResult { opened, unresolved })
    }

    /// Open the connection for the request's kind (variables already resolved).
    pub(crate) async fn connect_socket(
        &self,
        request: &Request,
        resolved: Resolved,
        opts: &zorvik_engine::RequestOptions,
        jar: Option<&Arc<zorvik_engine::CookieJar>>,
    ) -> ApiResult<SocketConnected> {
        if let Some(subscription) = resolved.subscription {
            return Ok(
                subscribe(&self.inner.client, resolved.request, subscription, opts, jar.map(|j| j.as_ref())).await?
            );
        }
        if request.kind == RequestKind::SocketIo {
            use zorvik_engine::socketio::{SocketIoConfig, SocketIoTransport as Transport};
            use zorvik_workspace::formats::SocketIoTransport;
            let config = SocketIoConfig {
                path: request.socketio.path.clone(),
                auth: resolved.socketio_auth,
                transport: match request.socketio.transport {
                    SocketIoTransport::Auto => Transport::Auto,
                    SocketIoTransport::Websocket => Transport::Websocket,
                    SocketIoTransport::Polling => Transport::Polling,
                },
            };
            return Ok(self.inner.client.socketio(resolved.request, config, opts, jar.cloned()).await?);
        }
        let resolved = &resolved;
        let socket = &request.socket;
        let config = SocketConfig {
            framing: socket.framing,
            length_bytes: socket.length_bytes,
            line_ending: socket.line_ending,
            broadcast: socket.broadcast,
        };
        let url = &resolved.request.url;
        match request.kind {
            RequestKind::Tcp => Ok(self.inner.client.tcp(url, opts, config).await?),
            RequestKind::Udp => Ok(self.inner.client.udp(url, opts, config).await?),
            RequestKind::Mqtt => self.connect_mqtt(request, resolved, opts).await,
            _ => Err(ApiError::invalid("This request is not a TCP, UDP, MQTT or Socket.IO connection")),
        }
    }
}

/// Start a GraphQL subscription: `request` is the resolved HTTP request (its body the operation).
pub(crate) async fn subscribe(
    client: &zorvik_engine::Client,
    mut request: zorvik_engine::HttpRequest,
    subscription: zorvik_workspace::resolve::Subscription,
    opts: &zorvik_engine::RequestOptions,
    jar: Option<&zorvik_engine::CookieJar>,
) -> Result<SocketConnected, EngineError> {
    use zorvik_engine::GraphqlWsProtocol;
    use zorvik_workspace::formats::GraphqlTransport;
    request.url = subscription.url;
    let protocol = match subscription.transport {
        GraphqlTransport::Websocket => GraphqlWsProtocol::TransportWs,
        GraphqlTransport::WebsocketLegacy => GraphqlWsProtocol::Legacy,
        GraphqlTransport::Sse => {
            request.method = "POST".into();
            return client.graphql_sse(request, opts, jar).await;
        }
    };
    let operation = serde_json::from_slice::<Value>(&request.body)
        .map_err(|e| EngineError::invalid(format!("The GraphQL operation is not valid JSON: {e}")))?;
    request.body = Default::default();
    client.graphql_ws(request, protocol, operation, subscription.connection_params, opts, jar).await
}

// ---- MQTT -----------------------------------------------------------------------

impl Api {
    /// MQTT: the user name and password are the effective Basic auth (request, folder
    /// or workspace, variables rendered), which `resolve` already turned into an
    /// Authorization header. Client id and subscription topics may use variables too.
    async fn connect_mqtt(
        &self,
        request: &Request,
        resolved: &Resolved,
        opts: &zorvik_engine::RequestOptions,
    ) -> ApiResult<SocketConnected> {
        let options = &request.mqtt;
        let vars = self.try_ws().map(|ws| self.var_context(&ws)).unwrap_or_default();
        let mut missing = std::collections::BTreeSet::new();
        let mut render = |s: &str| vars.render(s.trim(), &mut missing);
        let (username, password) = basic_credentials(&resolved.request.headers).unwrap_or_default();
        let config = MqttConfig {
            client_id: render(&options.client_id),
            protocol: match options.version {
                MqttVersion::V311 => MqttProtocol::V311,
                MqttVersion::V5 => MqttProtocol::V5,
            },
            clean_session: options.clean_session,
            keep_alive_secs: options.keep_alive_secs,
            username,
            password,
            subscriptions: options
                .subscriptions
                .iter()
                .filter(|s| s.enabled && !s.topic.trim().is_empty())
                .map(|s| (render(&s.topic), s.qos.min(2)))
                .collect(),
        };
        Ok(self.inner.client.mqtt(&resolved.request.url, opts, &config).await?)
    }
}

/// User name and password of a `Basic` Authorization header.
fn basic_credentials(headers: &[zorvik_engine::Header]) -> Option<(String, String)> {
    use base64::Engine as _;
    let value = headers.iter().rev().find(|h| h.name.eq_ignore_ascii_case("authorization"))?.value.trim();
    let (scheme, token) = value.split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("basic") {
        return None;
    }
    let decoded = base64::engine::general_purpose::STANDARD.decode(token.trim()).ok()?;
    let (user, password) = std::str::from_utf8(&decoded).ok()?.split_once(':')?;
    Some((user.to_string(), password.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use zorvik_engine::Header;

    #[test]
    fn basic_credentials_from_the_header() {
        let h = |v: &str| vec![Header::new("X-Other", "1"), Header::new("authorization", v)];
        // "user:p:ss" — the password may contain colons.
        assert_eq!(basic_credentials(&h("Basic dXNlcjpwOnNz")), Some(("user".into(), "p:ss".into())));
        assert_eq!(basic_credentials(&h("Basic Og==")), Some((String::new(), String::new())));
        assert_eq!(basic_credentials(&h("Bearer abc")), None);
        assert_eq!(basic_credentials(&h("Basic !!!")), None);
        assert_eq!(basic_credentials(&[]), None);
    }
}
