//! Stream a file over an open connection in binary messages (`fileStream.*`):
//! `chunkSize` bytes every `intervalMs`, optionally from an offset, for a length
//! and over and over. Works on WebSocket and socket sessions (TCP, UDP, MQTT
//! publishes, Socket.IO emits). Pacing runs here rather than in the UI, so a
//! hidden window doesn't slow it down, and it never runs ahead of the network:
//! sessions queue messages without limit, so a stream waits while more than
//! [`BACKLOG_BYTES`] of its messages are still unsent (see [`Settled`]).
//! Progress goes to the UI as [`StreamEvent::FileStream`] events tagged with
//! the connection id.

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio::sync::Notify;
use tokio::time::MissedTickBehavior;
use tokio_util::sync::CancellationToken;
use ts_rs::TS;
use zorvik_engine::{Direction, SocketEvent, SocketOutgoing, WsEvent, WsMessageKind, WsOutgoing};

use crate::{Api, ApiError, ApiResult, Inner, StreamEvent, lock, ok, params, remove_if_current};

/// Largest message a stream sends.
pub const MAX_CHUNK: u64 = 16 << 20;
/// Longest pause between messages.
const MAX_INTERVAL_MS: u64 = 3_600_000;
/// Progress events per second, at most.
const PROGRESS_EVERY: Duration = Duration::from_millis(100);
/// Bytes of a stream's messages that may wait unsent before it pauses (at least 2 messages).
const BACKLOG_BYTES: u64 = 4 << 20;

/// Outgoing messages a connection is done with, sent or failed, counted from its events (every
/// session reports each message it takes as sent or as an error). A file stream compares it
/// with the messages it handed over to know how many still wait.
#[derive(Default)]
pub(crate) struct Settled {
    count: AtomicU64,
    ended: AtomicBool,
    notify: Notify,
    /// Errors the session reported (a datagram too big, a broker refusing a publish), and the last.
    errors: AtomicU64,
    last_error: std::sync::Mutex<Option<String>>,
}

impl Settled {
    /// A WebSocket's events. Only binary messages count (what file streams send; the composer
    /// is locked while one runs); a failed send closes the connection, which ends the count.
    pub(crate) fn ws(&self, event: &WsEvent) {
        match event {
            WsEvent::Message { direction: Direction::Sent, kind: WsMessageKind::Binary, .. } => self.add(),
            WsEvent::Closed { .. } => self.end(),
            WsEvent::Message { .. } | WsEvent::Error { .. } | WsEvent::Info { .. } => {}
        }
    }

    /// A socket session's events. Acknowledgements the client sends by itself (answering a
    /// Socket.IO server) are not the stream's messages and don't count.
    pub(crate) fn socket(&self, event: &SocketEvent) {
        match event {
            SocketEvent::Message { direction: Direction::Sent, .. } if !event.is_automatic_ack() => self.add(),
            SocketEvent::Error { message } => {
                *lock(&self.last_error) = Some(message.clone());
                self.errors.fetch_add(1, Ordering::AcqRel);
                self.add();
            }
            SocketEvent::Closed { .. } => self.end(),
            SocketEvent::Message { .. } | SocketEvent::Info { .. } => {}
        }
    }

    /// The connection's events stopped: nothing settles anymore.
    pub(crate) fn end(&self) {
        self.ended.store(true, Ordering::Release);
        self.notify.notify_waiters();
    }

    fn add(&self) {
        self.count.fetch_add(1, Ordering::AcqRel);
        self.notify.notify_waiters();
    }

    fn count(&self) -> u64 {
        self.count.load(Ordering::Acquire)
    }

    fn errors(&self) -> u64 {
        self.errors.load(Ordering::Acquire)
    }

    /// Wait until fewer than `limit` messages of the first `handed` are unsettled. False when the
    /// connection ended first.
    async fn wait(&self, handed: u64, limit: u64) -> bool {
        loop {
            let notified = self.notify.notified();
            tokio::pin!(notified);
            // Registered before checking, so a settle in between still wakes it.
            notified.as_mut().enable();
            if self.ended.load(Ordering::Acquire) {
                return false;
            }
            if handed.saturating_sub(self.count()) < limit {
                return true;
            }
            notified.await;
        }
    }
}

/// A file picked for streaming.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FileStreamInfo {
    pub name: String,
    #[ts(type = "number")]
    pub size: u64,
    /// The audio format, for a WAV file.
    pub wav: Option<WavInfo>,
}

/// The format of a WAV file and where its samples are.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct WavInfo {
    /// `pcm`, `float`, `alaw`, `mulaw`, or `format N` for others.
    pub encoding: String,
    pub channels: u16,
    pub sample_rate: u32,
    pub bits_per_sample: u16,
    /// Bytes per sample frame (all channels).
    pub block_align: u16,
    pub byte_rate: u32,
    /// Where the samples start, after the header.
    #[ts(type = "number")]
    pub data_offset: u64,
    #[ts(type = "number")]
    pub data_size: u64,
}

/// How each chunk is sent on a socket session (a WebSocket always sends binary messages).
#[derive(Debug, Clone, Default, Deserialize, TS)]
#[serde(tag = "type", rename_all = "camelCase")]
#[ts(export)]
pub enum ChunkMessage {
    /// A binary message (TCP, UDP).
    #[default]
    Binary,
    /// MQTT: a publish to `topic`.
    #[serde(rename_all = "camelCase")]
    Publish {
        topic: String,
        #[serde(default)]
        qos: u8,
        #[serde(default)]
        retain: bool,
    },
    /// Socket.IO: an emit of `event` with the chunk as its one binary argument.
    #[serde(rename_all = "camelCase")]
    Emit {
        event: String,
        #[serde(default)]
        ack: bool,
    },
}

impl ChunkMessage {
    fn socket(&self, base64: String) -> SocketOutgoing {
        match self {
            ChunkMessage::Binary => SocketOutgoing::Binary { base64 },
            ChunkMessage::Publish { topic, qos, retain } => SocketOutgoing::Publish {
                topic: topic.clone(),
                text: None,
                base64: Some(base64),
                qos: *qos,
                retain: *retain,
            },
            ChunkMessage::Emit { event, ack } => {
                SocketOutgoing::Emit { event: event.clone(), args: String::new(), base64: Some(base64), ack: *ack }
            }
        }
    }
}

/// Progress of a file stream. Every stream ends with `ended`.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(tag = "type", rename_all = "camelCase")]
#[ts(export)]
pub enum FileStreamEvent {
    Progress(FileStreamProgress),
    #[serde(rename_all = "camelCase")]
    Ended {
        progress: FileStreamProgress,
        /// Stopped before the end (by the user, or the connection closed by the app).
        stopped: bool,
        /// Why it stopped early, when something went wrong.
        error: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, Default, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FileStreamProgress {
    /// Bytes sent in all passes.
    #[ts(type = "number")]
    pub sent: u64,
    #[ts(type = "number")]
    pub messages: u64,
    /// The pass through the file, from 1 (more than 1 when repeating).
    pub pass: u32,
    /// Bytes sent in this pass.
    #[ts(type = "number")]
    pub position: u64,
    /// Bytes in one pass.
    #[ts(type = "number")]
    pub total: u64,
}

/// Result of `fileStream.start` (progress follows as events).
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FileStreamStarted {
    /// Bytes in one pass.
    #[ts(type = "number")]
    pub total: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StartParams {
    conn_id: String,
    path: String,
    chunk_size: u64,
    interval_ms: u64,
    /// Skip this many bytes (a header, say).
    #[serde(default)]
    offset: u64,
    /// Send at most this many bytes from `offset` (default: to the end).
    #[serde(default)]
    length: Option<u64>,
    /// Start over at `offset` after the last chunk, until stopped.
    #[serde(default)]
    repeat: bool,
    #[serde(default)]
    message: ChunkMessage,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PathParam {
    path: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConnParam {
    conn_id: String,
}

impl Api {
    /// `fileStream.*` methods.
    pub(crate) async fn call_file_stream(&self, method: &str, p: Value) -> ApiResult<Value> {
        match method {
            "fileStream.info" => {
                let PathParam { path } = params(p)?;
                ok(tokio::task::spawn_blocking(move || file_info(Path::new(path.trim())))
                    .await
                    .map_err(crate::join_err)??)
            }
            "fileStream.start" => ok(self.start_file_stream(params(p)?).await?),
            "fileStream.stop" => {
                let ConnParam { conn_id } = params(p)?;
                stop_file_stream(&self.inner, &conn_id);
                ok(())
            }
            other => Err(ApiError::new("notFound", format!("Unknown method '{other}'"))),
        }
    }

    async fn start_file_stream(&self, p: StartParams) -> ApiResult<FileStreamStarted> {
        if !(1..=MAX_CHUNK).contains(&p.chunk_size) {
            return Err(ApiError::invalid(format!("Chunk size must be between 1 byte and {} MB", MAX_CHUNK >> 20)));
        }
        if !(1..=MAX_INTERVAL_MS).contains(&p.interval_ms) {
            return Err(ApiError::invalid("The interval must be between 1 ms and 1 hour"));
        }
        let path = p.path.trim().to_string();
        let size = tokio::task::spawn_blocking({
            let path = path.clone();
            move || file_info(Path::new(&path)).map(|info| info.size)
        })
        .await
        .map_err(crate::join_err)??;
        if p.length == Some(0) {
            return Err(ApiError::invalid("Send at least 1 byte"));
        }
        let total = size.saturating_sub(p.offset).min(p.length.unwrap_or(u64::MAX));
        if total == 0 {
            return Err(ApiError::invalid(if p.offset >= size && size > 0 {
                format!("The file has only {size} bytes: start before byte {size}")
            } else {
                "The file is empty: nothing to send".to_string()
            }));
        }
        let Some(settled) = settled_of(&self.inner, &p.conn_id) else {
            return Err(ApiError::new("notFound", "Not connected: connect first, then start the stream"));
        };
        if p.chunk_size > zorvik_engine::udp::MAX_DATAGRAM as u64
            && self.socket_kind(&p.conn_id) == Some(zorvik_workspace::formats::RequestKind::Udp)
        {
            return Err(ApiError::invalid(format!(
                "A UDP datagram carries at most {} bytes: send smaller messages",
                zorvik_engine::udp::MAX_DATAGRAM
            )));
        }
        let mut file = tokio::fs::File::open(&path)
            .await
            .map_err(|e| ApiError::new("io", format!("Could not open '{path}': {e}")))?;
        file.seek(SeekFrom::Start(p.offset))
            .await
            .map_err(|e| ApiError::new("io", format!("Could not read '{path}': {e}")))?;

        let generation = self.next_generation();
        let cancel = CancellationToken::new();
        if let Some((_, old)) = lock(&self.inner.file_streams).insert(p.conn_id.clone(), (generation, cancel.clone())) {
            old.cancel();
        }
        let inner = self.inner.clone();
        let plan = Plan {
            chunk: p.chunk_size as usize,
            interval: Duration::from_millis(p.interval_ms),
            offset: p.offset,
            total,
            repeat: p.repeat,
            backlog: (BACKLOG_BYTES / p.chunk_size).max(2),
            message: p.message,
        };
        tokio::spawn(async move {
            let conn_id = p.conn_id;
            let emit = |event| inner.sink.emit(StreamEvent::FileStream { conn_id: conn_id.clone(), event });
            let (progress, end) = run(
                file,
                &plan,
                &cancel,
                &settled,
                |bytes| send_chunk(&inner, &conn_id, &plan.message, bytes),
                |p| emit(FileStreamEvent::Progress(p)),
            )
            .await;
            let (stopped, error) = match end {
                End::Done => (false, None),
                End::Stopped => (true, None),
                End::Failed(message) => (true, Some(message)),
            };
            emit(FileStreamEvent::Ended { progress, stopped, error });
            remove_if_current(&inner.file_streams, &conn_id, |(g, _)| *g == generation);
        });
        Ok(FileStreamStarted { total })
    }
}

/// Stop the file stream on a connection, if one runs.
pub(crate) fn stop_file_stream(inner: &Inner, conn_id: &str) {
    if let Some((_, cancel)) = lock(&inner.file_streams).remove(conn_id) {
        cancel.cancel();
    }
}

/// The settle counter of an open connection, or `None` when it isn't open.
fn settled_of(inner: &Inner, conn_id: &str) -> Option<Arc<Settled>> {
    if let Some(e) = lock(&inner.ws_sessions).get(conn_id).filter(|e| e.session.is_some()) {
        return Some(e.settled.clone());
    }
    lock(&inner.socket_sessions).get(conn_id).filter(|e| e.session.is_some()).map(|e| e.settled.clone())
}

fn send_chunk(inner: &Inner, conn_id: &str, message: &ChunkMessage, bytes: &[u8]) -> Result<(), String> {
    let base64 = base64::engine::general_purpose::STANDARD.encode(bytes);
    if let Some(session) = lock(&inner.ws_sessions).get(conn_id).and_then(|e| e.session.as_ref()) {
        return session.send(WsOutgoing::Binary { base64 }).map_err(|e| e.message);
    }
    match lock(&inner.socket_sessions).get(conn_id).and_then(|e| e.session.as_ref()) {
        Some(session) => session.send(message.socket(base64)).map_err(|e| e.message),
        None => Err("The connection closed".into()),
    }
}

struct Plan {
    chunk: usize,
    interval: Duration,
    offset: u64,
    total: u64,
    repeat: bool,
    /// Messages that may wait unsent before the stream pauses.
    backlog: u64,
    message: ChunkMessage,
}

enum End {
    Done,
    Stopped,
    Failed(String),
}

/// Send `plan.total` bytes of `file` (already at `plan.offset`) in chunks, one per interval, and
/// not while the connection is behind by more than [`BACKLOG_BYTES`].
async fn run(
    mut file: tokio::fs::File,
    plan: &Plan,
    cancel: &CancellationToken,
    settled: &Settled,
    send: impl Fn(&[u8]) -> Result<(), String>,
    progress: impl Fn(FileStreamProgress),
) -> (FileStreamProgress, End) {
    let mut state = FileStreamProgress { pass: 1, total: plan.total, ..Default::default() };
    let mut buf = vec![0u8; plan.chunk.min(plan.total as usize)];
    // After a stall (a busy machine, sleep), keep the spacing rather than catching up in a burst.
    let mut tick = tokio::time::interval(plan.interval);
    tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut reported = Instant::now();
    let base = settled.count();
    let base_errors = settled.errors();
    loop {
        tokio::select! {
            biased;
            _ = cancel.cancelled() => return (state, End::Stopped),
            _ = tick.tick() => {}
        }
        tokio::select! {
            biased;
            _ = cancel.cancelled() => return (state, End::Stopped),
            ready = settled.wait(base + state.messages, plan.backlog) => if !ready {
                return (state, End::Failed("The connection closed".into()));
            }
        }
        // A message the session couldn't send (it says why) fails the stream.
        if settled.errors() > base_errors {
            let why = lock(&settled.last_error).clone().unwrap_or_default();
            return (state, End::Failed(format!("The connection reported a problem: {why}")));
        }
        let want = (plan.total - state.position).min(plan.chunk as u64) as usize;
        if let Err(e) = file.read_exact(&mut buf[..want]).await {
            return (state, End::Failed(format!("Could not read the file: {e}")));
        }
        if let Err(message) = send(&buf[..want]) {
            return (state, End::Failed(message));
        }
        state.sent += want as u64;
        state.position += want as u64;
        state.messages += 1;
        if state.position == plan.total {
            if !plan.repeat {
                return (state, End::Done);
            }
            if let Err(e) = file.seek(SeekFrom::Start(plan.offset)).await {
                return (state, End::Failed(format!("Could not read the file: {e}")));
            }
            state.position = 0;
            state.pass += 1;
        }
        if reported.elapsed() >= PROGRESS_EVERY {
            reported = Instant::now();
            progress(state);
        }
    }
}

/// Size of a regular file and, when it is one, its WAV format.
fn file_info(path: &Path) -> ApiResult<FileStreamInfo> {
    let shown = path.display();
    let meta = std::fs::metadata(path).map_err(|e| ApiError::new("io", format!("Could not open '{shown}': {e}")))?;
    // Devices such as /dev/zero never end.
    if !meta.is_file() {
        return Err(ApiError::invalid(format!("'{shown}' is not a regular file")));
    }
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let wav = std::fs::File::open(path).ok().and_then(|f| read_wav(f, meta.len()));
    Ok(FileStreamInfo { name, size: meta.len(), wav })
}

/// The format and sample data of a RIFF/WAVE file, or `None` when it isn't one.
fn read_wav(mut f: impl Read + Seek, size: u64) -> Option<WavInfo> {
    let mut riff = [0u8; 12];
    f.read_exact(&mut riff).ok()?;
    if &riff[0..4] != b"RIFF" || &riff[8..12] != b"WAVE" {
        return None;
    }
    let mut format: Option<(u16, u16, u32, u32, u16, u16)> = None;
    let mut pos = 12u64;
    loop {
        let mut head = [0u8; 8];
        f.read_exact(&mut head).ok()?;
        let len = u32::from_le_bytes(head[4..8].try_into().ok()?) as u64;
        pos += 8;
        match &head[0..4] {
            b"fmt " if len >= 16 => {
                let mut fmt = vec![0u8; len.min(64) as usize];
                f.read_exact(&mut fmt).ok()?;
                let u16_at = |i: usize| u16::from_le_bytes([fmt[i], fmt[i + 1]]);
                let u32_at = |i: usize| u32::from_le_bytes([fmt[i], fmt[i + 1], fmt[i + 2], fmt[i + 3]]);
                let mut code = u16_at(0);
                // WAVE_FORMAT_EXTENSIBLE: the real format is the start of the sub-format GUID.
                if code == 0xFFFE && fmt.len() >= 26 {
                    code = u16_at(24);
                }
                format = Some((code, u16_at(2), u32_at(4), u32_at(8), u16_at(12), u16_at(14)));
            }
            b"data" => {
                let (code, channels, sample_rate, byte_rate, block_align, bits_per_sample) = format?;
                let encoding = match code {
                    1 => "pcm".to_string(),
                    3 => "float".to_string(),
                    6 => "alaw".to_string(),
                    7 => "mulaw".to_string(),
                    other => format!("format {other}"),
                };
                // Streamed WAVs leave the length at 0 or 0xFFFFFFFF: the samples run to the end.
                let rest = size.saturating_sub(pos);
                let data_size = if len == 0 || len == u32::MAX as u64 { rest } else { len.min(rest) };
                return Some(WavInfo {
                    encoding,
                    channels,
                    sample_rate,
                    bits_per_sample,
                    block_align,
                    byte_rate,
                    data_offset: pos,
                    data_size,
                });
            }
            _ => {}
        }
        // Chunks are padded to an even length.
        pos += len + (len & 1);
        if pos >= size {
            return None;
        }
        f.seek(SeekFrom::Start(pos)).ok()?;
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;
    use std::sync::Mutex;

    use super::*;

    #[test]
    fn automatic_acknowledgements_are_not_stream_messages() {
        let settled = Settled::default();
        let sent = |detail: Option<&str>| {
            let mut event = SocketEvent::message(Direction::Sent, b"[1]");
            if let SocketEvent::Message { detail: d, .. } = &mut event {
                *d = detail.map(str::to_string);
            }
            event
        };
        settled.socket(&sent(None));
        settled.socket(&sent(Some("acknowledgement of #4")));
        settled.socket(&sent(Some("asks for an acknowledgement (#5)")));
        assert_eq!(settled.count(), 2);
    }

    /// A WAV file: 16-bit PCM, `rate` Hz, `channels`, `samples` frames of silence, with a LIST chunk
    /// before the data like many tools write.
    fn wav(rate: u32, channels: u16, samples: u32) -> Vec<u8> {
        let block = channels * 2;
        let data = samples * block as u32;
        let mut out = Vec::new();
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(4 + 24 + 8 + 5 + 1 + 8 + data).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&channels.to_le_bytes());
        out.extend_from_slice(&rate.to_le_bytes());
        out.extend_from_slice(&(rate * block as u32).to_le_bytes());
        out.extend_from_slice(&block.to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"LIST");
        out.extend_from_slice(&5u32.to_le_bytes());
        out.extend_from_slice(b"INFO!\0");
        out.extend_from_slice(b"data");
        out.extend_from_slice(&data.to_le_bytes());
        out.resize(out.len() + data as usize, 0);
        out
    }

    #[test]
    fn reads_the_wav_format_past_other_chunks() {
        let bytes = wav(16_000, 1, 100);
        let info = read_wav(Cursor::new(&bytes), bytes.len() as u64).unwrap();
        assert_eq!(
            info,
            WavInfo {
                encoding: "pcm".into(),
                channels: 1,
                sample_rate: 16_000,
                bits_per_sample: 16,
                block_align: 2,
                byte_rate: 32_000,
                data_offset: 58,
                data_size: 200,
            }
        );
        assert_eq!(bytes.len() as u64, info.data_offset + info.data_size);
    }

    #[test]
    fn streamed_wav_lengths_run_to_the_end_and_other_files_are_not_wav() {
        let mut bytes = wav(8_000, 2, 10);
        let at = bytes.len() - 40 - 4;
        bytes[at..at + 4].copy_from_slice(&u32::MAX.to_le_bytes());
        let info = read_wav(Cursor::new(&bytes), bytes.len() as u64).unwrap();
        assert_eq!(info.data_size, 40);
        assert!(read_wav(Cursor::new(b"ID3\x04 not a wav file at all"), 26).is_none());
        assert!(read_wav(Cursor::new(&bytes[..30]), 30).is_none());
    }

    async fn stream(data: &[u8], plan: Plan, cancel: &CancellationToken) -> (Vec<Vec<u8>>, FileStreamProgress, End) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("f.bin");
        std::fs::write(&path, data).unwrap();
        let mut file = tokio::fs::File::open(&path).await.unwrap();
        file.seek(SeekFrom::Start(plan.offset)).await.unwrap();
        let sent = Mutex::new(Vec::new());
        // Every message is sent at once.
        let settled = Settled::default();
        let (progress, end) = run(
            file,
            &plan,
            cancel,
            &settled,
            |b| {
                let mut sent = sent.lock().unwrap();
                sent.push(b.to_vec());
                settled.add();
                if sent.len() == 7 { Err("closed".into()) } else { Ok(()) }
            },
            |_| {},
        )
        .await;
        (sent.into_inner().unwrap(), progress, end)
    }

    fn plan(chunk: usize, offset: u64, total: u64, repeat: bool) -> Plan {
        Plan {
            chunk,
            interval: Duration::from_millis(1),
            offset,
            total,
            repeat,
            backlog: 2,
            message: ChunkMessage::Binary,
        }
    }

    #[tokio::test]
    async fn sends_the_range_in_chunks_and_repeats_until_the_connection_fails() {
        let data: Vec<u8> = (0..10).collect();
        let cancel = CancellationToken::new();
        let (sent, progress, end) = stream(&data, plan(4, 2, 7, false), &cancel).await;
        assert_eq!(sent, vec![vec![2, 3, 4, 5], vec![6, 7, 8]]);
        assert!(matches!(end, End::Done));
        assert_eq!((progress.sent, progress.messages, progress.pass, progress.position), (7, 2, 1, 7));

        // Repeating goes back to the offset; the 7th send fails, as a closed connection would.
        let (sent, progress, end) = stream(&data, plan(4, 6, 4, true), &cancel).await;
        assert_eq!(sent.len(), 7);
        assert!(sent.iter().all(|c| c == &[6, 7, 8, 9]));
        assert!(matches!(end, End::Failed(ref m) if m == "closed"));
        assert_eq!((progress.pass, progress.messages), (7, 6));
    }

    #[tokio::test]
    async fn stops_when_cancelled_and_paces_by_the_interval() {
        let data = vec![0u8; 100];
        let cancel = CancellationToken::new();
        cancel.cancel();
        let (sent, _, end) = stream(&data, plan(10, 0, 100, false), &cancel).await;
        assert!(matches!(end, End::Stopped));
        assert!(sent.is_empty());

        let started = Instant::now();
        let mut slow = plan(25, 0, 100, false);
        slow.interval = Duration::from_millis(40);
        let (sent, _, end) = stream(&data, slow, &CancellationToken::new()).await;
        assert!(matches!(end, End::Done));
        assert_eq!(sent.len(), 4);
        // The first chunk goes at once, then one per interval.
        assert!(started.elapsed() >= Duration::from_millis(115), "{:?}", started.elapsed());
    }

    #[tokio::test]
    async fn waits_while_the_connection_is_behind_and_ends_with_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("f.bin");
        std::fs::write(&path, vec![0u8; 100]).unwrap();
        let settled = Arc::new(Settled::default());
        let sent = Arc::new(AtomicU64::new(0));
        let task = tokio::spawn({
            let (settled, sent) = (settled.clone(), sent.clone());
            async move {
                let file = tokio::fs::File::open(&path).await.unwrap();
                let cancel = CancellationToken::new();
                let plan = Plan { backlog: 3, ..plan(10, 0, 100, false) };
                run(
                    file,
                    &plan,
                    &cancel,
                    &settled,
                    |_| {
                        sent.fetch_add(1, Ordering::SeqCst);
                        Ok(())
                    },
                    |_| {},
                )
                .await
            }
        });
        // Nothing settles: 3 messages wait unsent, so it pauses.
        tokio::time::sleep(Duration::from_millis(60)).await;
        assert_eq!(sent.load(Ordering::SeqCst), 3);
        // Two settle: two more go.
        settled.add();
        settled.add();
        tokio::time::sleep(Duration::from_millis(60)).await;
        assert_eq!(sent.load(Ordering::SeqCst), 5);
        settled.end();
        let (progress, end) = tokio::time::timeout(Duration::from_secs(2), task).await.unwrap().unwrap();
        assert!(matches!(end, End::Failed(ref m) if m == "The connection closed"));
        assert_eq!(progress.messages, 5);
    }
}
