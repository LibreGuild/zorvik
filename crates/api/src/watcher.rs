//! Watches the open workspace so edits from Git or an editor show up in the UI.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use notify::RecursiveMode;
use notify_debouncer_full::{DebounceEventResult, Debouncer, RecommendedCache, new_debouncer_opt};

use crate::{EventSink, StreamEvent};

pub struct Watcher {
    _debouncer: Debouncer<notify::RecommendedWatcher, RecommendedCache>,
    /// Cleared on drop: the debouncer thread may still flush one batch after
    /// it is told to stop, which must not reach the UI (workspace closed or switched).
    active: Arc<AtomicBool>,
}

impl Drop for Watcher {
    fn drop(&mut self) {
        self.active.store(false, Ordering::Release);
    }
}

impl Watcher {
    pub fn start(root: &Path, sink: Arc<dyn EventSink>) -> notify::Result<Self> {
        // Events carry canonical paths (e.g. /private/var/… for /var/… on macOS).
        let roots = [root.to_path_buf(), std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf())];
        let active = Arc::new(AtomicBool::new(true));
        let still_active = active.clone();
        let handler = move |result: DebounceEventResult| {
            let Ok(events) = result else { return };
            let mut paths: Vec<String> = events
                .iter()
                .flat_map(|e| e.paths.iter())
                .filter_map(|p| roots.iter().find_map(|r| p.strip_prefix(r).ok()))
                .filter(|p| {
                    // Ignore temp files from atomic writes and VCS internals.
                    !p.components().any(|c| {
                        let s = c.as_os_str().to_string_lossy();
                        s.starts_with('.')
                    })
                })
                .map(|p| p.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/"))
                .collect();
            paths.sort();
            paths.dedup();
            if !paths.is_empty() && still_active.load(Ordering::Acquire) {
                sink.emit(StreamEvent::WorkspaceChanged { paths });
            }
        };
        // Workspaces come from Git: a symlink in one (say, to /) must not make the watcher walk
        // the whole disk.
        let config = notify::Config::default().with_follow_symlinks(false);
        let mut debouncer = new_debouncer_opt::<_, notify::RecommendedWatcher, _>(
            Duration::from_millis(300),
            None,
            handler,
            RecommendedCache::new(),
            config,
        )?;
        debouncer.watch(root, RecursiveMode::Recursive)?;
        Ok(Self { _debouncer: debouncer, active })
    }
}
