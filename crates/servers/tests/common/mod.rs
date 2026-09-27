//! Shared helpers: start a server and collect what it reports.

#![allow(dead_code)]

use std::sync::{Arc, Mutex};
use std::time::Duration;

use zorvik_engine::{Client, RequestOptions};
use zorvik_formats::Server;
use zorvik_servers::{Reporter, RunningServer, ServerEvent, StartOptions, TrafficEntry, TrafficKind};
use zorvik_workspace::vars::VarContext;

#[derive(Clone, Default)]
pub struct Events(pub Arc<Mutex<Vec<ServerEvent>>>);

impl Events {
    pub fn traffic(&self) -> Vec<TrafficEntry> {
        self.0
            .lock()
            .unwrap()
            .iter()
            .filter_map(|e| match e {
                ServerEvent::Traffic { entry } => Some(entry.clone()),
                _ => None,
            })
            .collect()
    }

    pub fn stopped(&self) -> Option<Option<String>> {
        self.0.lock().unwrap().iter().find_map(|e| match e {
            ServerEvent::Stopped { error } => Some(error.clone()),
            _ => None,
        })
    }

    /// Wait until an entry matches.
    pub async fn wait(&self, what: &str, pred: impl Fn(&TrafficEntry) -> bool) -> TrafficEntry {
        for _ in 0..250 {
            if let Some(e) = self.traffic().into_iter().find(|e| pred(e)) {
                return e;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("no {what} in {:#?}", self.traffic());
    }

    pub async fn wait_kind(&self, kind: TrafficKind) -> TrafficEntry {
        self.wait(&format!("{kind:?}"), |e| e.kind == kind).await
    }
}

pub fn options() -> StartOptions {
    StartOptions {
        base_dir: std::env::temp_dir(),
        client: Arc::new(Client::new()),
        request_options: RequestOptions::default(),
    }
}

/// Start `server` on a free port with the given variables.
pub async fn start_with(mut server: Server, vars: VarContext) -> (RunningServer, Events) {
    server.port = 0;
    let events = Events::default();
    let sink = events.clone();
    let reporter = Reporter::new(move |e| sink.0.lock().unwrap().push(e));
    let running = zorvik_servers::start(server, vars, options(), reporter).await.expect("server starts");
    (running, events)
}

pub async fn start(server: Server) -> (RunningServer, Events) {
    start_with(server, VarContext::new()).await
}
