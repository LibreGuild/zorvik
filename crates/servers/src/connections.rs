//! Open connections of a server, so the UI can send to one or all of them.

use std::collections::HashMap;
use std::sync::Mutex;

use tokio::sync::mpsc;
use tokio::sync::mpsc::error::TrySendError;
use tokio_util::sync::CancellationToken;

/// Sends waiting for one connection; a client that does not read gets no more
/// (the UI is told), so its queue cannot grow without limit.
pub(crate) const MAX_QUEUED: usize = 256;

pub(crate) enum ConnCommand<M> {
    Send(M),
    Close,
}

struct Entry<M> {
    tx: mpsc::Sender<M>,
    /// Separate from the queue: a close gets through even when the queue is full.
    close: CancellationToken,
}

/// What a connection's task reads: messages to send, or the request to close.
pub(crate) struct Commands<M> {
    rx: mpsc::Receiver<M>,
    close: CancellationToken,
}

impl<M> Commands<M> {
    /// The next command (`None` once the connection is no longer registered).
    pub async fn recv(&mut self) -> Option<ConnCommand<M>> {
        tokio::select! {
            biased;
            _ = self.close.cancelled() => Some(ConnCommand::Close),
            message = self.rx.recv() => message.map(ConnCommand::Send),
        }
    }

    /// Resolves when the UI asks to close the connection (e.g. during a blocked write).
    pub async fn closing(&self) {
        self.close.cancelled().await
    }
}

pub(crate) struct Connections<M> {
    map: Mutex<HashMap<u64, Entry<M>>>,
}

impl<M> Default for Connections<M> {
    fn default() -> Self {
        Self { map: Mutex::new(HashMap::new()) }
    }
}

impl<M: Clone> Connections<M> {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<u64, Entry<M>>> {
        self.map.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Register a connection; its task reads commands from the receiver.
    pub fn add(&self, conn: u64) -> Commands<M> {
        let (tx, rx) = mpsc::channel(MAX_QUEUED);
        let close = CancellationToken::new();
        self.lock().insert(conn, Entry { tx, close: close.clone() });
        Commands { rx, close }
    }

    pub fn remove(&self, conn: u64) {
        self.lock().remove(&conn);
    }

    /// Send to `conn`, or to every connection when `None`. Returns how many got it.
    pub fn send(&self, conn: Option<u64>, message: M) -> Result<usize, String> {
        let map = self.lock();
        match conn {
            Some(id) => {
                let entry = map.get(&id).ok_or_else(|| format!("Connection #{id} is closed"))?;
                entry.tx.try_send(message).map_err(|e| match e {
                    TrySendError::Full(_) => {
                        format!("Not sent: client #{id} is not reading ({MAX_QUEUED} messages are still waiting)")
                    }
                    TrySendError::Closed(_) => format!("Connection #{id} is closed"),
                })?;
                Ok(1)
            }
            None if map.is_empty() => Err("No client is connected".into()),
            None => Ok(map.values().filter(|e| e.tx.try_send(message.clone()).is_ok()).count()),
        }
    }

    pub fn close(&self, conn: u64) {
        if let Some(entry) = self.lock().get(&conn) {
            entry.close.cancel();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn queues_are_bounded_and_close_gets_through() {
        let conns: Connections<u32> = Connections::default();
        let mut commands = conns.add(1);
        for i in 0..MAX_QUEUED as u32 {
            assert_eq!(conns.send(Some(1), i), Ok(1));
        }
        let err = conns.send(Some(1), 0).unwrap_err();
        assert!(err.contains("not reading"), "{err}");
        assert_eq!(conns.send(None, 0), Ok(0), "a full queue is skipped by a broadcast");
        // Closing is not stuck behind the queued messages.
        conns.close(1);
        assert!(matches!(commands.recv().await, Some(ConnCommand::Close)));
        conns.remove(1);
        assert_eq!(conns.send(Some(1), 0).unwrap_err(), "Connection #1 is closed");
    }
}
