//! How many connections one server keeps open at once. Past that, new ones are closed as they
//! arrive: a flood (or a leak in a client) can't use up the app's open files.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use crate::report::Reporter;

/// Connections one server keeps open at once.
pub(crate) const MAX_CONNECTIONS: usize = 1024;
/// The "too many connections" note shows at most this often.
const NOTE_EVERY: Duration = Duration::from_secs(60);

pub(crate) struct Slots {
    free: Arc<Semaphore>,
    noted: Mutex<Option<Instant>>,
}

impl Slots {
    pub(crate) fn new() -> Self {
        Self { free: Arc::new(Semaphore::new(MAX_CONNECTIONS)), noted: Mutex::new(None) }
    }

    /// A place for a new connection (held until it ends), or `None`: then close it.
    pub(crate) fn take(&self, reporter: &Reporter) -> Option<OwnedSemaphorePermit> {
        let slot = self.free.clone().try_acquire_owned().ok();
        if slot.is_none() {
            let mut noted = self.noted.lock().unwrap_or_else(|e| e.into_inner());
            if noted.is_none_or(|at| at.elapsed() >= NOTE_EVERY) {
                *noted = Some(Instant::now());
                reporter.error(
                    None,
                    None,
                    format!("{MAX_CONNECTIONS} connections are open: new ones are closed until some end"),
                );
            }
        }
        slot
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_full_server_refuses_more() {
        let slots = Slots::new();
        let reporter = Reporter::new(|_| {});
        let held: Vec<_> = (0..MAX_CONNECTIONS).map(|_| slots.take(&reporter).expect("room")).collect();
        assert!(slots.take(&reporter).is_none());
        drop(held);
        assert!(slots.take(&reporter).is_some());
    }
}
