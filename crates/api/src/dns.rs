//! DNS queries (`dns.query`): a request whose URL is the name and whose method
//! is the record type; `request.dns` picks the resolver and the RD flag.
//! Variables are rendered from the active environment and the workspace. The
//! DNS lookup tool calls it with an unsaved request, also with no workspace open.
//! `requestId` (optional) lets `http.cancel` stop a query in flight.

use std::collections::BTreeSet;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio_util::sync::CancellationToken;
use ts_rs::TS;
use zorvik_engine::dns::DEFAULT_DNS_TIMEOUT;
use zorvik_engine::{DnsQuery, DnsResult, EngineError};
use zorvik_workspace::formats::Request;
use zorvik_workspace::vars::VarContext;
use zorvik_workspace::{Error, ErrorCode};

use crate::{Api, ApiError, ApiResult, lock, ok, params, remove_if_current};

/// The answer plus the variables that were referenced but not defined.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DnsQueryResult {
    #[serde(flatten)]
    #[ts(flatten)]
    pub result: DnsResult,
    pub unresolved: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct QueryParams {
    #[serde(default)]
    request_id: Option<String>,
    request: Request,
}

impl Api {
    /// `dns.*` methods.
    pub(crate) async fn call_dns(&self, method: &str, p: Value) -> ApiResult<Value> {
        match method {
            "dns.query" => ok(self.dns_query(params(p)?).await?),
            other => Err(ApiError::new("notFound", format!("Unknown method '{other}'"))),
        }
    }

    async fn dns_query(&self, p: QueryParams) -> ApiResult<DnsQueryResult> {
        let cancel = CancellationToken::new();
        let generation = self.next_generation();
        if let Some(id) = &p.request_id
            && let Some((_, previous)) = lock(&self.inner.inflight).insert(id.clone(), (generation, cancel.clone()))
        {
            previous.cancel();
        }
        let outcome = async {
            let vars = self.try_ws().map(|ws| self.var_context(&ws)).unwrap_or_default();
            let (query, unresolved) = dns_query_for(&p.request, &vars)?;
            let opts = crate::request_options(&self.settings(), &p.request.settings)?;
            tokio::select! {
                r = self.inner.client.dns_query(&query, &opts) => Ok(DnsQueryResult { result: r?, unresolved }),
                _ = cancel.cancelled() => Err(EngineError::cancelled().into()),
            }
        }
        .await;
        if let Some(id) = &p.request_id {
            remove_if_current(&self.inner.inflight, id, |(g, _)| *g == generation);
        }
        outcome
    }
}

/// The engine query for a DNS request, with variables rendered. A name or
/// server that still holds an undefined `{{variable}}` is refused (a lookup of
/// the literal text would only fail confusingly).
fn dns_query_for(request: &Request, vars: &VarContext) -> ApiResult<(DnsQuery, Vec<String>)> {
    let mut missing = BTreeSet::new();
    let name = vars.render(request.url.trim(), &mut missing);
    let server = vars.render(request.dns.server.trim(), &mut missing);
    let unresolved: Vec<String> = missing.into_iter().collect();
    let undefined: Vec<String> = unresolved
        .iter()
        .filter(|v| name.contains(&format!("{{{{{v}}}}}")) || server.contains(&format!("{{{{{v}}}}}")))
        .map(|v| format!("{{{{{v}}}}}"))
        .collect();
    if !undefined.is_empty() {
        return Err(Error::new(
            ErrorCode::UndefinedVariable,
            format!(
                "{} is not defined. Select an environment that defines it, or add it under Environments. Names are case-sensitive.",
                undefined.join(", ")
            ),
        )
        .into());
    }
    let timeout = match request.settings.timeout_ms {
        Some(ms) if ms > 0 => Duration::from_millis(ms),
        _ => DEFAULT_DNS_TIMEOUT,
    };
    let query = DnsQuery {
        name,
        record_type: request.method.trim().to_string(),
        server,
        recursion: request.dns.recursion,
        timeout,
    };
    Ok((query, unresolved))
}

#[cfg(test)]
mod tests {
    use super::*;
    use zorvik_workspace::formats::{RequestKind, Variable};

    #[test]
    fn variables_are_rendered_and_undefined_ones_refused() {
        let mut vars = VarContext::new();
        vars.push_layer(&[Variable {
            key: "host".into(),
            value: "api.example.test".into(),
            enabled: true,
            secret: false,
        }]);
        let mut request = Request::new("q", RequestKind::Dns);
        request.method = "MX".into();
        request.url = " {{host}} ".into();
        request.dns.server = "{{dns}}".into();
        let err = dns_query_for(&request, &vars).unwrap_err();
        assert_eq!(err.code, "undefinedVariable");
        assert!(err.message.contains("{{dns}}"), "{}", err.message);

        request.dns.server = "1.1.1.1".into();
        request.settings.timeout_ms = Some(750);
        let (query, unresolved) = dns_query_for(&request, &vars).unwrap();
        assert_eq!((query.name.as_str(), query.record_type.as_str()), ("api.example.test", "MX"));
        assert_eq!(
            (query.server.as_str(), query.timeout, query.recursion),
            ("1.1.1.1", Duration::from_millis(750), true)
        );
        assert!(unresolved.is_empty());
        request.settings.timeout_ms = Some(0);
        assert_eq!(dns_query_for(&request, &vars).unwrap().0.timeout, DEFAULT_DNS_TIMEOUT);
    }
}
