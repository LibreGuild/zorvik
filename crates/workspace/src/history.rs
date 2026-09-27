//! Request history in a local SQLite database (app data dir, never in the workspace).
//!
//! Stores the unresolved request (so secrets from environments are not written
//! into history) plus a response summary.

use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use rusqlite::{Connection, ErrorCode as SqliteCode, params};
use serde::Serialize;
use ts_rs::TS;
use zorvik_formats::Request;

use crate::error::{Error, ErrorCode, Result};

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct HistoryEntry {
    #[ts(type = "number")]
    pub id: i64,
    pub request_path: Option<String>,
    pub method: String,
    /// URL as sent (variables resolved).
    pub url: String,
    pub status: Option<u16>,
    pub error: Option<String>,
    pub duration_ms: Option<f64>,
    #[ts(type = "number | null")]
    pub size: Option<i64>,
    /// Unix epoch milliseconds.
    #[ts(type = "number")]
    pub created_at: i64,
    pub request: Request,
}

pub struct NewEntry<'a> {
    pub workspace: &'a str,
    pub request_path: Option<&'a str>,
    pub url: &'a str,
    pub status: Option<u16>,
    pub error: Option<&'a str>,
    pub duration_ms: Option<f64>,
    pub size: Option<i64>,
    pub request: &'a Request,
}

pub struct History {
    conn: Mutex<Connection>,
}

fn db_err(e: rusqlite::Error) -> Error {
    Error::new(ErrorCode::Io, format!("History database error: {e}"))
}

impl History {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| Error::io("Could not create data folder", e))?;
        }
        // URLs as sent may carry secrets: owner-only (SQLite gives -wal/-shm the same mode).
        let open = || {
            let conn = Connection::open(path);
            crate::fsutil::restrict_to_owner(path);
            conn
        };
        match Self::init(open().map_err(db_err)?) {
            // A corrupt database should not stop the app: start fresh and keep the old file.
            // Other failures (e.g. locked by a second instance) must not discard history.
            Err(e) if matches!(e.sqlite_error_code(), Some(SqliteCode::NotADatabase | SqliteCode::DatabaseCorrupt)) => {
                tracing::warn!("history database unusable ({e}), recreating");
                let _ = std::fs::rename(path, path.with_extension("corrupt"));
                Self::init(open().map_err(db_err)?).map_err(db_err)
            }
            other => other.map_err(db_err),
        }
    }

    pub fn in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory().map_err(db_err)?).map_err(db_err)
    }

    fn init(conn: Connection) -> rusqlite::Result<Self> {
        conn.busy_timeout(Duration::from_secs(5))?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             CREATE TABLE IF NOT EXISTS history (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 workspace TEXT NOT NULL,
                 request_path TEXT,
                 method TEXT NOT NULL,
                 url TEXT NOT NULL,
                 status INTEGER,
                 error TEXT,
                 duration_ms REAL,
                 size INTEGER,
                 created_at INTEGER NOT NULL,
                 request_json TEXT NOT NULL
             );
             CREATE INDEX IF NOT EXISTS history_ws_time ON history(workspace, created_at DESC);",
        )?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Record an entry and prune the workspace history to `limit` entries.
    pub fn add(&self, entry: NewEntry<'_>, limit: u32) -> Result<i64> {
        let json = serde_json::to_string(entry.request).map_err(|e| Error::invalid(e.to_string()))?;
        let now = (time::OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000) as i64;
        let conn = self.lock();
        conn.execute(
            "INSERT INTO history (workspace, request_path, method, url, status, error, duration_ms, size, created_at, request_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                entry.workspace,
                entry.request_path,
                entry.request.method,
                entry.url,
                entry.status,
                entry.error,
                entry.duration_ms,
                entry.size,
                now,
                json
            ],
        )
        .map_err(db_err)?;
        let id = conn.last_insert_rowid();
        conn.execute(
            "DELETE FROM history WHERE workspace = ?1 AND id NOT IN
               (SELECT id FROM history WHERE workspace = ?1 ORDER BY id DESC LIMIT ?2)",
            params![entry.workspace, limit.max(1)],
        )
        .map_err(db_err)?;
        Ok(id)
    }

    /// Newest first; `search` filters by URL or method (case-insensitive).
    pub fn list(&self, workspace: &str, search: &str, limit: u32, offset: u32) -> Result<Vec<HistoryEntry>> {
        let pattern = format!("%{}%", search.trim().replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_"));
        let conn = self.lock();
        let mut stmt = conn
            .prepare(
                "SELECT id, request_path, method, url, status, error, duration_ms, size, created_at, request_json
                 FROM history WHERE workspace = ?1 AND (url LIKE ?2 ESCAPE '\\' OR method LIKE ?2 ESCAPE '\\')
                 ORDER BY id DESC LIMIT ?3 OFFSET ?4",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map(params![workspace, pattern, limit, offset], |row| {
                let json: String = row.get(9)?;
                Ok((
                    HistoryEntry {
                        id: row.get(0)?,
                        request_path: row.get(1)?,
                        method: row.get(2)?,
                        url: row.get(3)?,
                        status: row.get(4)?,
                        error: row.get(5)?,
                        duration_ms: row.get(6)?,
                        size: row.get(7)?,
                        created_at: row.get(8)?,
                        request: zorvik_formats::Request::new("", Default::default()),
                    },
                    json,
                ))
            })
            .map_err(db_err)?;
        let mut out = Vec::new();
        for row in rows {
            let (mut entry, json) = row.map_err(db_err)?;
            // Skip rows whose request no longer parses (format changes).
            if let Ok(request) = serde_json::from_str(&json) {
                entry.request = request;
                out.push(entry);
            }
        }
        Ok(out)
    }

    pub fn delete(&self, id: i64) -> Result<()> {
        self.lock().execute("DELETE FROM history WHERE id = ?1", params![id]).map_err(db_err)?;
        Ok(())
    }

    pub fn clear(&self, workspace: &str) -> Result<()> {
        self.lock().execute("DELETE FROM history WHERE workspace = ?1", params![workspace]).map_err(db_err)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zorvik_formats::RequestKind;

    fn add(h: &History, ws: &str, url: &str, limit: u32) {
        let mut req = Request::new("r", RequestKind::Http);
        req.url = url.into();
        h.add(
            NewEntry {
                workspace: ws,
                request_path: None,
                url,
                status: Some(200),
                error: None,
                duration_ms: Some(1.5),
                size: Some(10),
                request: &req,
            },
            limit,
        )
        .unwrap();
    }

    #[test]
    fn add_list_search_prune() {
        let h = History::in_memory().unwrap();
        for i in 0..5 {
            add(&h, "a", &format!("http://h/{i}"), 3);
        }
        add(&h, "b", "http://other/100%_x", 10);
        let list = h.list("a", "", 50, 0).unwrap();
        assert_eq!(list.iter().map(|e| e.url.as_str()).collect::<Vec<_>>(), ["http://h/4", "http://h/3", "http://h/2"]);
        assert_eq!(list[0].request.url, "http://h/4");
        assert_eq!(h.list("a", "/3", 50, 0).unwrap().len(), 1);
        assert_eq!(h.list("b", "100%_", 50, 0).unwrap().len(), 1);
        assert_eq!(h.list("b", "%", 50, 0).unwrap().len(), 1);
        assert_eq!(h.list("a", "get", 50, 0).unwrap().len(), 3);
        h.delete(list[0].id).unwrap();
        assert_eq!(h.list("a", "", 50, 0).unwrap().len(), 2);
        h.clear("a").unwrap();
        assert!(h.list("a", "", 50, 0).unwrap().is_empty());
        assert_eq!(h.list("b", "", 50, 0).unwrap().len(), 1);
    }

    #[test]
    fn corrupt_file_is_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.sqlite3");
        std::fs::write(&path, b"this is not sqlite at all, definitely not").unwrap();
        let h = History::open(&path).unwrap();
        add(&h, "a", "http://h/", 10);
        assert_eq!(h.list("a", "", 10, 0).unwrap().len(), 1);
        assert!(path.with_extension("corrupt").exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        }
    }
}
