//! Runs a plan on its own Tokio runtime (a dedicated thread, one worker per
//! core) so the app's runtime stays responsive: virtual users (closed model)
//! or a start schedule (open model), the aggregator that sends snapshots, stop
//! handling, and the final summary.

use std::collections::HashSet;
use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicU8, AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use tokio::sync::mpsc;
use tokio::time::MissedTickBehavior;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;
use zorvik_engine::pool::{Exchange, PooledClient, PooledRequest};
use zorvik_engine::{EngineError, ErrorKind};
use zorvik_formats::LoadModel;

use crate::capture::{CAPTURE_BODY, Capture, DataRow, UserVars, server_timing_ms};
use crate::metrics::{Outcome, Phases, Recorder, Sample, SummaryInfo};
use crate::schedule::{Picker, Profile};
use crate::{
    Destination, EventFn, LoadEvent, MAX_USERS, MetricsSummary, Plan, Render, RequestSource, RunPhase, Summary,
};

const SNAPSHOT_EVERY: Duration = Duration::from_millis(250);
/// Samples waiting for the aggregator; when it is full, senders record directly.
const CHANNEL: usize = 65_536;
/// Longest wait for requests in flight after the end or a stop (shorter when
/// the request timeout is).
const STOP_GRACE: Duration = Duration::from_secs(5);
/// Pause after a request that could not be built, so a broken template doesn't spin.
const RENDER_BACKOFF: Duration = Duration::from_millis(100);
/// How often virtual users are added or removed to follow the stages.
const USER_TICK: Duration = Duration::from_millis(50);

enum Source {
    Fixed(Box<PooledRequest>),
    Dynamic(Render),
}

/// A plan checked and ready to run.
pub(crate) struct Prepared {
    plan: Plan,
    client: PooledClient,
    /// Per target; `None` for targets with weight 0 (never sent).
    sources: Vec<Option<Source>>,
    /// Per target.
    captures: Vec<Vec<Capture>>,
    /// Where rendered requests may go (see [`crate::destinations`]).
    destinations: HashSet<Destination>,
}

/// Build the client and each target's request, so a broken URL, header,
/// certificate file or capture is reported before anything is sent.
pub(crate) fn prepare(plan: Plan) -> Result<Prepared, String> {
    let client = PooledClient::new(plan.options.clone(), plan.keep_alive).map_err(|e| e.message)?;
    let first = UserVars::new(plan.rows.first().cloned());
    let mut sources = Vec::with_capacity(plan.targets.len());
    let mut captures = Vec::with_capacity(plan.targets.len());
    for target in &plan.targets {
        let problem = |message: String| format!("{}: {message}", target.name);
        if target.weight == 0 {
            sources.push(None);
            captures.push(Vec::new());
            continue;
        }
        let list = target.captures.iter().map(Capture::new).collect::<Result<Vec<_>, _>>().map_err(problem)?;
        let keep = |req: PooledRequest| if list.is_empty() { req } else { req.keep_response(CAPTURE_BODY) };
        let source = match &target.source {
            RequestSource::Fixed(req) => {
                Source::Fixed(Box::new(keep(client.prepare(req.clone()).map_err(|e| problem(e.message))?)))
            }
            RequestSource::Dynamic(render) => {
                client.prepare(render(&first).map_err(problem)?).map_err(|e| problem(e.message))?;
                Source::Dynamic(render.clone())
            }
        };
        sources.push(Some(source));
        captures.push(list);
    }
    let destinations = crate::destinations(&plan);
    Ok(Prepared { plan, client, sources, captures, destinations })
}

/// Start the run on its own thread. [`LoadEvent::Finished`] is sent exactly
/// once from there, whatever happens.
pub(crate) fn spawn(prepared: Prepared, on_event: EventFn, stop: CancellationToken) -> Result<(), String> {
    std::thread::Builder::new()
        .name("zorvik-load".into())
        .spawn(move || run_thread(prepared, on_event, stop))
        .map(drop)
        .map_err(|e| format!("Could not start the load generator: {e}"))
}

/// Sends `Finished` once; also when the thread unwinds without a summary.
struct Finish {
    on_event: EventFn,
    started_at: f64,
    sent: bool,
}

impl Finish {
    fn send(&mut self, summary: Summary) {
        if !std::mem::replace(&mut self.sent, true) {
            (self.on_event)(LoadEvent::Finished { summary });
        }
    }
}

impl Drop for Finish {
    fn drop(&mut self) {
        let summary = failed(self.started_at, "The load generator stopped unexpectedly".into());
        self.send(summary);
    }
}

fn failed(started_at: f64, error: String) -> Summary {
    Summary {
        started_at,
        duration_ms: 0,
        totals: MetricsSummary::default(),
        targets: Vec::new(),
        points: Vec::new(),
        thresholds: Vec::new(),
        passed: false,
        stopped_early: false,
        error: Some(error),
        peak_cpu_percent: None,
    }
}

fn run_thread(prepared: Prepared, on_event: EventFn, stop: CancellationToken) {
    let started_at = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |d| d.as_millis() as f64);
    let mut finish = Finish { on_event: on_event.clone(), started_at, sent: false };
    raise_open_file_limit(prepared.plan.keep_alive);
    let _timers = TimerResolution::raise();
    let workers = std::thread::available_parallelism().map_or(4, |n| n.get());
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(workers)
        .thread_name("zorvik-load-worker")
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(e) => return finish.send(failed(started_at, format!("Could not start the load generator: {e}"))),
    };

    let Prepared { plan, client, sources, captures, destinations } = prepared;
    let profile = Profile::new(&plan.stages);
    let names = plan.targets.iter().map(|t| (t.name.clone(), t.request.clone())).collect();
    let recorder = Arc::new(Mutex::new(Recorder::new(
        names,
        plan.thresholds.clone(),
        profile.clone(),
        plan.model == LoadModel::ArrivalRate,
    )));
    let weights: Vec<u32> = plan.targets.iter().map(|t| t.weight).collect();
    let (samples, rx) = mpsc::channel(CHANNEL);
    let shared = Arc::new(Shared {
        client,
        sources,
        captures,
        destinations,
        in_order: plan.has_captures(),
        next_row: AtomicUsize::new(0),
        rows: plan.rows,
        picker: Picker::new(&weights),
        profile,
        model: plan.model,
        think_time: plan.think_time,
        max_in_flight: plan.max_in_flight,
        timeout: plan.options.timeout,
        t0: Instant::now(),
        samples,
        recorder: recorder.clone(),
        active: AtomicU32::new(0),
        phase: AtomicU8::new(RunPhase::Starting as u8),
        ending: CancellationToken::new(),
        abort: CancellationToken::new(),
        tasks: TaskTracker::new(),
    });
    let t0 = shared.t0;
    let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
        runtime.block_on(run(shared.clone(), rx, stop.clone(), on_event, started_at))
    }));
    // Close the connections, then let the workers finish dropping tasks (a
    // blocking DNS lookup is not waited for long).
    drop(shared);
    runtime.shutdown_timeout(Duration::from_millis(500));
    let summary = result.unwrap_or_else(|_| {
        let info = SummaryInfo {
            started_at,
            elapsed: t0.elapsed(),
            stopped_early: stop.is_cancelled(),
            error: Some("The load generator stopped because of an internal error".into()),
            peak_cpu_percent: None,
        };
        std::panic::catch_unwind(AssertUnwindSafe(|| lock(&recorder).summary(info)))
            .unwrap_or_else(|_| failed(started_at, "The load generator stopped because of an internal error".into()))
    });
    finish.send(summary);
}

/// Raise the open-file limit (macOS allows only 256 by default; every
/// connection is a descriptor). Windows has no such limit but only ~16k
/// ephemeral ports, which a new connection per request can exhaust.
fn raise_open_file_limit(keep_alive: bool) {
    #[cfg(unix)]
    match rlimit::increase_nofile_limit(u64::MAX) {
        Ok(limit) => tracing::debug!("open file limit: {limit}"),
        Err(e) => tracing::warn!("could not raise the open file limit: {e}"),
    }
    if cfg!(windows) && !keep_alive {
        tracing::warn!("load test without keep-alive: each request uses a new ephemeral port (TIME_WAIT)");
    }
}

/// Windows wakes timers on its 15.6 ms tick by default, so open-model requests
/// would start up to that late, and the delay would count as latency (it is
/// measured from the scheduled start). Asks for 1 ms timers while a run lasts.
struct TimerResolution {
    #[cfg(windows)]
    raised: bool,
}

impl TimerResolution {
    fn raise() -> Self {
        #[cfg(windows)]
        {
            // SAFETY: no pointers involved; undone in `drop` when it succeeded.
            let raised = unsafe { windows_sys::Win32::Media::timeBeginPeriod(1) } == 0;
            Self { raised }
        }
        #[cfg(not(windows))]
        Self {}
    }
}

impl Drop for TimerResolution {
    fn drop(&mut self) {
        #[cfg(windows)]
        if self.raised {
            // SAFETY: matches the successful `timeBeginPeriod(1)` above.
            unsafe { windows_sys::Win32::Media::timeEndPeriod(1) };
        }
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

struct Shared {
    client: PooledClient,
    sources: Vec<Option<Source>>,
    captures: Vec<Vec<Capture>>,
    destinations: HashSet<Destination>,
    /// Each user goes through the targets in order (the plan has captures);
    /// else every pick takes the next target of one shared order.
    in_order: bool,
    rows: Vec<DataRow>,
    /// Open model: the next request's row.
    next_row: AtomicUsize,
    picker: Picker,
    profile: Profile,
    model: LoadModel,
    think_time: Duration,
    max_in_flight: u32,
    timeout: Option<Duration>,
    t0: Instant,
    samples: mpsc::Sender<Sample>,
    recorder: Arc<Mutex<Recorder>>,
    /// Users (closed model) or requests in flight (open model).
    active: AtomicU32,
    phase: AtomicU8,
    /// Start nothing new (planned end or stop).
    ending: CancellationToken,
    /// Give up on requests still in flight.
    abort: CancellationToken,
    tasks: TaskTracker,
}

impl Shared {
    fn phase(&self) -> RunPhase {
        match self.phase.load(Ordering::Relaxed) {
            0 => RunPhase::Starting,
            1 => RunPhase::Running,
            2 => RunPhase::Stopping,
            _ => RunPhase::Finished,
        }
    }

    fn set_phase(&self, phase: RunPhase) {
        self.phase.store(phase as u8, Ordering::Relaxed);
    }

    /// Hand a sample to the aggregator, or record it here when the channel is full.
    fn record(&self, target: usize, outcome: Outcome) {
        let sample = Sample { target, at_us: self.t0.elapsed().as_micros() as u64, outcome };
        if let Err(e) = self.samples.try_send(sample) {
            let sample = match e {
                mpsc::error::TrySendError::Full(s) | mpsc::error::TrySendError::Closed(s) => s,
            };
            lock(&self.recorder).record(&sample);
        }
    }

    fn record_batch(&self, batch: &mut Vec<Sample>) {
        let mut recorder = lock(&self.recorder);
        for sample in batch.drain(..) {
            recorder.record(&sample);
        }
    }

    /// Row `i` of the data file (wrapping around), if there is one.
    fn row(&self, i: usize) -> Option<DataRow> {
        (!self.rows.is_empty()).then(|| self.rows[i % self.rows.len()].clone())
    }

    /// One iteration of `target` for a user with `vars`; latency counts from
    /// `started`. The target's captures update `vars` from the response.
    async fn send(&self, target: usize, started: Instant, vars: &mut UserVars) -> Outcome {
        let captures = self.captures.get(target).map_or(&[][..], Vec::as_slice);
        let exchange = match self.sources.get(target).and_then(Option::as_ref) {
            Some(Source::Fixed(req)) => self.client.send(req, started).await,
            Some(Source::Dynamic(render)) => match self.build(render, vars, !captures.is_empty()) {
                Ok(req) => self.client.send(&req, started).await,
                Err(e) => {
                    tracing::debug!("load test request could not be built: {}", e.message);
                    tokio::time::sleep(RENDER_BACKOFF).await;
                    return failure(e.kind, None);
                }
            },
            None => return failure(ErrorKind::InvalidRequest, None),
        };
        let misses = match &exchange.response {
            Some(response) if !captures.is_empty() => crate::capture::apply(captures, response, vars),
            _ => 0,
        };
        outcome(exchange, misses)
    }

    /// Render a request for this user; it must go where the run was started
    /// to send: the same scheme, host, port and `Host` header.
    fn build(&self, render: &Render, vars: &UserVars, keep: bool) -> Result<PooledRequest, EngineError> {
        let req = render(vars).map_err(EngineError::invalid)?;
        match Destination::of(&req) {
            Some(to) if !self.destinations.contains(&to) => {
                Err(EngineError::new(ErrorKind::NotAllowed, format!("{to} is not where this run was started to send")))
            }
            // Without a destination the URL can't be sent: preparing it says why.
            _ => {
                let req = self.client.prepare(req)?;
                Ok(if keep { req.keep_response(CAPTURE_BODY) } else { req })
            }
        }
    }

    /// Open model: start one request scheduled for `due`, unless `maxInFlight`
    /// requests are running already (then it is counted as dropped, not queued).
    fn start_one(self: &Arc<Self>, due: Instant) {
        let target = self.picker.next();
        if self.active.fetch_add(1, Ordering::AcqRel) >= self.max_in_flight {
            self.active.fetch_sub(1, Ordering::AcqRel);
            self.record(target, Outcome::Dropped);
            return;
        }
        // Each request is its own iteration: the next row, nothing captured yet.
        let mut vars = UserVars::new(self.row(self.next_row.fetch_add(1, Ordering::Relaxed)));
        let shared = self.clone();
        self.tasks.spawn(async move {
            let _active = Active(&shared.active);
            let outcome = tokio::select! {
                biased;
                _ = shared.abort.cancelled() => failure(ErrorKind::Cancelled, Some(due)),
                outcome = shared.send(target, due, &mut vars) => outcome,
            };
            shared.record(target, outcome);
        });
    }
}

/// Counts down `active` when a user or request ends.
struct Active<'a>(&'a AtomicU32);

impl Drop for Active<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

/// A failed iteration; `waited_since` gives it a latency (time waited, for cut-off requests).
fn failure(kind: ErrorKind, waited_since: Option<Instant>) -> Outcome {
    Outcome::Done {
        latency_us: waited_since.map(|t| t.elapsed().as_micros() as u64),
        status: None,
        error: Some(kind),
        bytes_in: 0,
        bytes_out: 0,
        new_connection: false,
        phases: Phases::default(),
        capture_misses: 0,
    }
}

fn micros(d: Duration) -> u64 {
    d.as_micros() as u64
}

fn outcome(e: Exchange, capture_misses: u32) -> Outcome {
    let kind = e.error.as_ref().map(|e| e.kind);
    if let Some(err) = &e.error {
        tracing::trace!("load test request failed: {}", err.message);
    }
    // Answered requests and timeouts have a latency; failed connections don't.
    let timed = e.ttfb.is_some() || matches!(kind, Some(ErrorKind::Timeout | ErrorKind::Cancelled));
    Outcome::Done {
        latency_us: timed.then_some(e.latency.as_micros() as u64),
        status: e.status,
        error: kind,
        bytes_in: e.bytes_in,
        bytes_out: e.bytes_out,
        new_connection: e.new_connection,
        phases: Phases {
            connect_us: e.connect.map(micros),
            ttfb_us: e.ttfb.map(micros),
            transfer_us: e.transfer.map(micros),
            server_us: e.server_timing.as_deref().and_then(server_timing_ms).map(|ms| (ms * 1000.0).round() as u64),
        },
        capture_misses,
    }
}

async fn run(
    shared: Arc<Shared>,
    rx: mpsc::Receiver<Sample>,
    stop: CancellationToken,
    on_event: EventFn,
    started_at: f64,
) -> Summary {
    let quit = CancellationToken::new();
    let aggregator = tokio::spawn(aggregate(shared.clone(), rx, quit.clone(), on_event));
    shared.set_phase(RunPhase::Running);
    match shared.model {
        LoadModel::VirtualUsers => shared.tasks.spawn(virtual_users(shared.clone())),
        LoadModel::ArrivalRate => shared.tasks.spawn(arrivals(shared.clone())),
    };

    let planned_end = tokio::time::Instant::from_std(shared.t0 + shared.profile.duration());
    let stopped_early = tokio::select! {
        _ = tokio::time::sleep_until(planned_end) => false,
        _ = stop.cancelled() => true,
    };
    // No new requests; the ones in flight get a grace period, then are cut off.
    // The chart's last second shows the users/requests active until now, and
    // throughput counts up to here (at most the planned duration).
    {
        let now = shared.t0.elapsed();
        let mut recorder = lock(&shared.recorder);
        recorder.note_active(now, shared.active.load(Ordering::Relaxed));
        recorder.end_sending(now.min(shared.profile.duration()));
    }
    shared.set_phase(RunPhase::Stopping);
    shared.ending.cancel();
    shared.tasks.close();
    let grace = shared.timeout.map_or(STOP_GRACE, |t| t.min(STOP_GRACE));
    let drained = tokio::select! {
        _ = shared.tasks.wait() => true,
        _ = tokio::time::sleep(grace) => false,
        // A stop while winding down at the planned end cuts the wait short.
        _ = stop.cancelled(), if !stopped_early => false,
    };
    if !drained {
        shared.abort.cancel();
        let _ = tokio::time::timeout(Duration::from_secs(2), shared.tasks.wait()).await;
    }
    let elapsed = shared.t0.elapsed();

    quit.cancel();
    let (peak_cpu_percent, error) = match aggregator.await {
        Ok(peak) => (peak, None),
        Err(_) => (None, Some("Live updates stopped because of an internal error".to_string())),
    };
    let info = SummaryInfo { started_at, elapsed, stopped_early, error, peak_cpu_percent };
    lock(&shared.recorder).summary(info)
}

/// Receives samples and sends a snapshot every 250 ms. Returns the peak CPU.
async fn aggregate(
    shared: Arc<Shared>,
    mut rx: mpsc::Receiver<Sample>,
    quit: CancellationToken,
    on_event: EventFn,
) -> Option<f32> {
    let mut cpu = Cpu::new();
    let mut tick = tokio::time::interval(SNAPSHOT_EVERY);
    tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut batch = Vec::with_capacity(1024);
    loop {
        tokio::select! {
            biased;
            _ = quit.cancelled() => break,
            _ = tick.tick() => {
                let cpu_percent = cpu.sample();
                let (phase, elapsed, active) = (shared.phase(), shared.t0.elapsed(), shared.active.load(Ordering::Relaxed));
                let snapshot = lock(&shared.recorder).snapshot(phase, elapsed, active, cpu_percent);
                on_event(LoadEvent::Snapshot { snapshot });
            }
            n = rx.recv_many(&mut batch, 1024) => {
                if n == 0 {
                    break;
                }
                shared.record_batch(&mut batch);
            }
        }
    }
    // Every request task has ended: take what is still queued.
    while let Ok(sample) = rx.try_recv() {
        batch.push(sample);
    }
    shared.record_batch(&mut batch);
    cpu.peak
}

/// Closed model: keep as many users running as the stage wants.
async fn virtual_users(shared: Arc<Shared>) {
    let mut users: Vec<CancellationToken> = Vec::new();
    // Users started so far: the next one's number (and data row).
    let mut started = 0;
    let mut tick = tokio::time::interval(USER_TICK);
    tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            biased;
            _ = shared.ending.cancelled() => break,
            _ = tick.tick() => {}
        }
        let wanted = shared.profile.target_at(shared.t0.elapsed().as_secs_f64()).round() as usize;
        let wanted = wanted.min(MAX_USERS as usize);
        while users.len() < wanted {
            let retire = CancellationToken::new();
            shared.tasks.spawn(user(shared.clone(), retire.clone(), started));
            users.push(retire);
            started += 1;
        }
        // Retired users finish the request they are sending.
        while users.len() > wanted {
            if let Some(retire) = users.pop() {
                retire.cancel();
            }
        }
    }
}

/// Virtual user number `index`: send, wait for the answer, think, repeat.
/// It keeps its data row and captured values for its whole life.
async fn user(shared: Arc<Shared>, retire: CancellationToken, index: usize) {
    shared.active.fetch_add(1, Ordering::AcqRel);
    let _active = Active(&shared.active);
    let mut vars = UserVars::new(shared.row(index));
    let mut step = 0;
    while !retire.is_cancelled() && !shared.ending.is_cancelled() {
        let target = if shared.in_order { shared.picker.at(step) } else { shared.picker.next() };
        step += 1;
        let started = Instant::now();
        let outcome = tokio::select! {
            biased;
            _ = shared.abort.cancelled() => failure(ErrorKind::Cancelled, Some(started)),
            outcome = shared.send(target, started, &mut vars) => outcome,
        };
        shared.record(target, outcome);
        if shared.abort.is_cancelled() {
            break;
        }
        if !shared.think_time.is_zero() {
            tokio::select! {
                _ = tokio::time::sleep(shared.think_time) => {}
                _ = retire.cancelled() => break,
                _ = shared.ending.cancelled() => break,
            }
        }
    }
}

/// Open model: start requests on schedule, however slow the answers are.
async fn arrivals(shared: Arc<Shared>) {
    let mut schedule = shared.profile.arrivals();
    let mut next = schedule.next();
    while let Some(at) = next {
        let due = shared.t0 + Duration::from_secs_f64(at);
        let ending = if due > Instant::now() {
            tokio::select! {
                biased;
                _ = shared.ending.cancelled() => true,
                _ = tokio::time::sleep_until(tokio::time::Instant::from_std(due)) => false,
            }
        } else {
            // Behind schedule: let other tasks run between batches.
            tokio::task::yield_now().await;
            shared.ending.is_cancelled()
        };
        // Everything due now starts now, each with its own scheduled time
        // (latency counts from there: a late start is not hidden). Also when the run is
        // ending: a timer that woke late (Windows wakes in ~15 ms steps) must not drop a
        // request that was due before the end.
        let now = Instant::now();
        while let Some(at) = next {
            let due = shared.t0 + Duration::from_secs_f64(at);
            if due > now {
                break;
            }
            shared.start_one(due);
            next = schedule.next();
        }
        if ending {
            return;
        }
    }
}

/// CPU used by this process (percent of one core, summed over cores).
struct Cpu {
    system: sysinfo::System,
    pid: Option<sysinfo::Pid>,
    last: Option<Instant>,
    value: Option<f32>,
    peak: Option<f32>,
}

impl Cpu {
    fn new() -> Self {
        let pid = if sysinfo::IS_SUPPORTED_SYSTEM { sysinfo::get_current_pid().ok() } else { None };
        Self { system: sysinfo::System::new(), pid, last: None, value: None, peak: None }
    }

    /// Refreshed at most once a second; the first refresh only sets the baseline.
    fn sample(&mut self) -> Option<f32> {
        let pid = self.pid?;
        if self.last.is_some_and(|t| t.elapsed() < Duration::from_secs(1)) {
            return self.value;
        }
        self.system.refresh_processes_specifics(
            sysinfo::ProcessesToUpdate::Some(&[pid]),
            false,
            sysinfo::ProcessRefreshKind::nothing().with_cpu(),
        );
        if self.last.is_some() {
            self.value = self.system.process(pid).map(|p| p.cpu_usage());
            if let Some(v) = self.value {
                self.peak = Some(self.peak.map_or(v, |p| p.max(v)));
            }
        }
        self.last = Some(Instant::now());
        self.value
    }
}
