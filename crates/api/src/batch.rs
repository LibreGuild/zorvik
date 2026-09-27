//! Batches UI events: WebSocket, socket and server traffic can produce
//! thousands of events a second, and each IPC message to the webview has a
//! fixed cost. Events are collected for a few milliseconds and sent together
//! as [`StreamEvent::Batch`], in order.
//!
//! Server traffic that piles up faster than it can be sent (e.g. a load test
//! against a local mock) is thinned out, oldest first: the UI only shows the
//! newest entries, and `server.log` keeps them all. Other events are never dropped.

use std::sync::{Arc, Condvar, Mutex, Weak};
use std::time::Duration;

use crate::{EventSink, StreamEvent};

/// How long events wait for company before they are sent.
const WINDOW: Duration = Duration::from_millis(25);
/// Server traffic entries waiting at most; beyond it the oldest half is dropped.
const MAX_PENDING_TRAFFIC: usize = 10_000;

#[derive(Default)]
struct Pending {
    events: Vec<StreamEvent>,
    /// How many of `events` are server traffic entries.
    traffic: usize,
}

impl Pending {
    fn push(&mut self, event: StreamEvent) {
        if is_traffic(&event) {
            if self.traffic >= MAX_PENDING_TRAFFIC {
                let mut drop = self.traffic / 2;
                self.traffic -= drop;
                self.events.retain(|e| {
                    let dropped = drop > 0 && is_traffic(e);
                    drop -= usize::from(dropped);
                    !dropped
                });
            }
            self.traffic += 1;
        }
        self.events.push(event);
    }
}

fn is_traffic(event: &StreamEvent) -> bool {
    matches!(event, StreamEvent::Server { event: zorvik_servers::ServerEvent::Traffic { .. }, .. })
}

struct Queue {
    events: Mutex<Pending>,
    ready: Condvar,
}

/// Wraps the UI sink (Tauri or the dev bridge). Uses its own thread, so it
/// works whether or not a Tokio runtime is running.
pub struct BatchingSink {
    queue: Arc<Queue>,
    inner: Arc<dyn EventSink>,
}

impl BatchingSink {
    pub fn new(inner: Arc<dyn EventSink>) -> Self {
        let queue = Arc::new(Queue { events: Mutex::new(Pending::default()), ready: Condvar::new() });
        let weak = Arc::downgrade(&queue);
        let target = inner.clone();
        std::thread::Builder::new()
            .name("zorvik-events".into())
            .spawn(move || flush_loop(weak, target))
            .expect("start event thread");
        Self { queue, inner }
    }
}

fn flush_loop(queue: Weak<Queue>, sink: Arc<dyn EventSink>) {
    loop {
        let Some(q) = queue.upgrade() else { return };
        {
            let events = q.events.lock().unwrap_or_else(|e| e.into_inner());
            let (events, _) = q
                .ready
                .wait_timeout_while(events, Duration::from_millis(200), |p| p.events.is_empty())
                .unwrap_or_else(|e| e.into_inner());
            if events.events.is_empty() {
                continue;
            }
        }
        std::thread::sleep(WINDOW);
        let mut batch = std::mem::take(&mut *q.events.lock().unwrap_or_else(|e| e.into_inner())).events;
        drop(q);
        match batch.len() {
            0 => {}
            1 => sink.emit(batch.pop().expect("one event")),
            _ => sink.emit(StreamEvent::Batch { events: batch }),
        }
    }
}

impl EventSink for BatchingSink {
    fn emit(&self, event: StreamEvent) {
        self.queue.events.lock().unwrap_or_else(|e| e.into_inner()).push(event);
        self.queue.ready.notify_one();
    }

    fn open_url(&self, url: &str) -> Result<(), String> {
        self.inner.open_url(url)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Collect(Mutex<Vec<StreamEvent>>);

    impl EventSink for Collect {
        fn emit(&self, event: StreamEvent) {
            self.0.lock().unwrap().push(event);
        }
    }

    #[test]
    fn bursts_are_batched_in_order() {
        let collect = Arc::new(Collect::default());
        let sink = BatchingSink::new(collect.clone());
        for i in 0..100 {
            sink.emit(StreamEvent::WorkspaceChanged { paths: vec![i.to_string()] });
        }
        std::thread::sleep(Duration::from_millis(300));
        sink.emit(StreamEvent::WorkspaceChanged { paths: vec!["last".into()] });
        std::thread::sleep(Duration::from_millis(300));
        let got = collect.0.lock().unwrap();
        let mut flat = Vec::new();
        for e in got.iter() {
            match e {
                StreamEvent::Batch { events } => flat.extend(events.iter().cloned()),
                other => flat.push(other.clone()),
            }
        }
        let paths: Vec<String> = flat
            .iter()
            .map(|e| match e {
                StreamEvent::WorkspaceChanged { paths } => paths[0].clone(),
                _ => unreachable!(),
            })
            .collect();
        assert_eq!(paths.len(), 101);
        assert_eq!(paths.last().unwrap(), "last");
        assert!(paths[..100].iter().enumerate().all(|(i, p)| *p == i.to_string()));
        assert!(got.len() <= 4, "{} emits", got.len());
    }

    #[test]
    fn piled_up_server_traffic_is_thinned_out_oldest_first() {
        use zorvik_servers::{ServerEvent, TrafficEntry, TrafficKind};
        let traffic = |id: u64| {
            let entry = TrafficEntry {
                id,
                timestamp: 0.0,
                kind: TrafficKind::Info,
                conn: None,
                peer: None,
                direction: None,
                summary: String::new(),
                text: None,
                base64: None,
                size: 0,
                truncated: false,
                http: None,
            };
            StreamEvent::Server { run_id: "r".into(), event: ServerEvent::Traffic { entry } }
        };
        let mut pending = Pending::default();
        pending.push(StreamEvent::QuitRequested { running: 1, load_test: false });
        for id in 1..=25_000 {
            pending.push(traffic(id));
        }
        pending.push(StreamEvent::Server { run_id: "r".into(), event: ServerEvent::Stopped { error: None } });
        let ids: Vec<u64> = pending
            .events
            .iter()
            .filter_map(|e| match e {
                StreamEvent::Server { event: ServerEvent::Traffic { entry }, .. } => Some(entry.id),
                _ => None,
            })
            .collect();
        assert_eq!(ids.len(), pending.traffic);
        assert!(ids.len() <= MAX_PENDING_TRAFFIC, "{}", ids.len());
        assert_eq!(*ids.last().unwrap(), 25_000, "the newest entries are kept");
        assert!(ids.windows(2).all(|w| w[0] < w[1]), "in order");
        // Other events are never dropped.
        assert!(matches!(pending.events.first(), Some(StreamEvent::QuitRequested { .. })));
        assert!(matches!(pending.events.last(), Some(StreamEvent::Server { event: ServerEvent::Stopped { .. }, .. })));
    }
}
