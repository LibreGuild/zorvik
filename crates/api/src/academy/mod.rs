//! Training Bootcamp (`academy.*`): the built-in workspace, labs (servers started for a
//! lesson, steps checked against what the app sees) and the learner's progress.
//!
//! While a lab runs, every app call is noted in a short journal ([`Api::academy_observe`])
//! and the lab's steps are checked after each one and once a second ([`checks`]).
//! Rewards reach the UI only as `academy` events, whatever earned them.

mod checks;

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;
use ts_rs::TS;
use zorvik_academy::progress::{self, Clock, Progress, Rewards};
use zorvik_academy::{Course, Lesson, matcher};
use zorvik_workspace::Workspace;
use zorvik_workspace::formats::{Environment, Server, Variable};

use crate::{Api, ApiError, ApiResult, StreamEvent, lock, ok, params};

pub const WORKSPACE_NAME: &str = "Training Bootcamp";
/// The Bootcamp workspace folder, in the app data folder.
const WORKSPACE_DIR: &str = "bootcamp";
const PROGRESS_FILE: &str = "academy-progress.json";
/// The practice HTTPS server's certificate authority, written into the workspace.
const LAB_CA: &str = "lab-ca.pem";
/// The environment a lab fills in and switches to.
const LAB_ENV: &str = "Lab";
/// Servers a lab saves are named "Lab · <name>" (and removed when another lab starts).
const LAB_SERVER_PREFIX: &str = "Lab · ";
const JOURNAL_MAX: usize = 400;
/// App methods that save requests, folders and servers: what "Do it for me" saves keeps its
/// `{{variables}}`, as the learner's own would (only `{{secret.*}}` values, which no environment
/// holds, are filled in). Environments get real values: they are where variables come from.
const SAVES_FILES: &[&str] = &[
    "request.create",
    "request.save",
    "folder.create",
    "folder.save",
    "workspace.saveMeta",
    "server.create",
    "server.save",
    "mock.addRoute",
    "load.create",
    "load.save",
];
/// Ids of the servers the running lab saved, so a lab cut short by quitting is cleaned up
/// next time (and nothing the learner made).
const LAB_SERVERS_FILE: &str = "academy-lab-servers.json";
/// Calls that change nothing a check looks at, left out of the journal.
const QUIET: &[&str] = &[
    "app.info",
    "settings.get",
    "workspace.current",
    "workspace.recent",
    "workspace.tree",
    "request.read",
    "folder.read",
    "env.list",
    "vars.list",
    "vars.local",
    "vars.render",
    "history.list",
    "cookies.list",
    "server.list",
    "server.read",
    "server.running",
    "server.log",
    "load.list",
    "load.read",
    "load.runs",
    "load.run",
    "load.active",
    "oauth2.status",
    "runner.preview",
    "agent.status",
    "agent.sessions",
];

#[derive(Default)]
pub(crate) struct AcademyState {
    /// A lab runs: app calls are journaled and its steps checked.
    active: AtomicBool,
    lab: Mutex<Option<ActiveLab>>,
    journal: Mutex<VecDeque<Fact>>,
    /// Loaded on first use.
    progress: Mutex<Option<Progress>>,
    /// The learner's offset from UTC in minutes (for streak days and time-of-day badges).
    utc_offset: AtomicI32,
    generation: AtomicU64,
    /// One check at a time.
    evaluating: tokio::sync::Mutex<()>,
    /// Starting, stopping and resetting labs one at a time.
    lifecycle: tokio::sync::Mutex<()>,
    playground: tokio::sync::Mutex<Playground>,
}

/// Practice servers labs share, started when a lab first needs them.
#[derive(Default)]
struct Playground {
    http: Option<zorvik_testkit::TestServer>,
    tls: Option<(zorvik_testkit::TestServer, String)>,
    grpc: Option<zorvik_testkit::GrpcTestServer>,
}

/// Something the learner did during a lab (see [`checks`]).
#[derive(Debug, Clone)]
pub(crate) struct Fact {
    pub at: i64,
    /// `{method, params, ok, result, error}`; for `http.send` the result is a summary of the exchange.
    pub value: Value,
}

struct ActiveLab {
    generation: u64,
    lesson: String,
    started_at: i64,
    /// The "Lab" environment's variables.
    vars: Vec<(String, String)>,
    secrets: HashMap<String, String>,
    servers: Vec<LabServer>,
    steps: Vec<StepRun>,
    finished: bool,
    ticker: CancellationToken,
}

impl ActiveLab {
    fn lookup(&self, name: &str) -> Option<String> {
        if let Some(secret) = name.strip_prefix("secret.") {
            return self.secrets.get(secret).cloned();
        }
        self.vars.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone())
    }

    fn current(&self) -> Option<usize> {
        self.steps.iter().position(|s| !s.done)
    }
}

#[derive(Clone)]
struct LabServer {
    /// Id in the lab file.
    key: String,
    /// Saved server id in the workspace.
    file_id: String,
    run_id: String,
    name: String,
    url: String,
}

#[derive(Debug, Clone, Default)]
struct StepRun {
    done: bool,
    /// "Do it for me" was used: no XP.
    assisted: bool,
    hints: u32,
    answer: Option<String>,
}

// ---- views --------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CourseView {
    pub units: Vec<UnitView>,
    /// Badges earned by how the learner works (unit badges are on the units).
    pub extra_badges: Vec<BadgeView>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct UnitView {
    pub id: String,
    pub title: String,
    pub summary: String,
    pub color: String,
    pub image: String,
    pub capstone: bool,
    pub badge: BadgeView,
    pub lessons: Vec<LessonSummary>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct BadgeView {
    pub id: String,
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LessonSummary {
    pub id: String,
    pub title: String,
    pub summary: String,
    pub minutes: u32,
    pub lab_minutes: Option<u32>,
    pub lab_steps: u32,
    pub questions: u32,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LessonView {
    pub id: String,
    pub unit: String,
    pub title: String,
    pub summary: String,
    pub minutes: u32,
    /// The reading, in Markdown.
    pub body: String,
    pub lab: Option<LabInfo>,
    pub quiz: Vec<QuestionView>,
    pub prev: Option<String>,
    pub next: Option<String>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LabInfo {
    pub title: String,
    pub goal: String,
    pub minutes: u32,
    /// What each step asks, in Markdown.
    pub steps: Vec<String>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct QuestionView {
    pub question: String,
    pub options: Vec<String>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ProgressView {
    pub xp: u32,
    pub level: u32,
    pub rank: String,
    /// XP at which the current level started, and the next one starts.
    pub level_xp: u32,
    pub next_level_xp: u32,
    pub streak: u32,
    pub best_streak: u32,
    /// Whether today already counts for the streak.
    pub active_today: bool,
    pub badges: Vec<EarnedBadge>,
    pub lessons: Vec<LessonState>,
    /// Finished (or tested-out) unit ids.
    pub units: Vec<String>,
    pub last_lesson: Option<String>,
    /// Unix epoch milliseconds.
    pub graduated_at: Option<f64>,
    pub completed_lessons: u32,
    pub total_lessons: u32,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct EarnedBadge {
    pub id: String,
    /// Unix epoch milliseconds.
    pub at: f64,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LessonState {
    pub id: String,
    pub completed: bool,
    pub steps_done: u32,
    pub lab_done: bool,
    pub quiz_best: Option<u32>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LabView {
    pub lesson: String,
    pub title: String,
    pub goal: String,
    /// Unix epoch milliseconds.
    pub started_at: f64,
    pub steps: Vec<StepView>,
    /// The "Lab" environment: server addresses and the lab's own values.
    pub vars: Vec<LabVar>,
    pub servers: Vec<LabServerView>,
    pub finished: bool,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct StepView {
    /// What to do, in Markdown.
    pub text: String,
    /// The hints opened so far.
    pub hints: Vec<String>,
    pub hints_total: u32,
    pub done: bool,
    pub assisted: bool,
    /// The step asks for a typed answer.
    pub answer: bool,
    /// The lab calls the learner's server: "Check now" helps after changing it.
    pub probe: bool,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LabVar {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LabServerView {
    pub name: String,
    pub url: String,
    /// Saved server id (opens it).
    pub server_id: String,
}

/// Pushed as the `academy` event: the lab, progress, and what was just earned.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AcademyUpdate {
    pub lab: Option<LabView>,
    pub progress: ProgressView,
    pub rewards: Option<Rewards>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct QuizResult {
    pub results: Vec<QuestionResult>,
    pub right: u32,
    pub total: u32,
    pub passed: bool,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct QuestionResult {
    pub correct: bool,
    /// The right option.
    pub answer: u32,
    pub explain: String,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TestOutQuestion {
    /// `<lesson id>:<question index>`.
    pub id: String,
    pub question: String,
    pub options: Vec<String>,
}

// ---- views from the course and progress -----------------------------------------

fn badge_view(b: &zorvik_academy::BadgeDef) -> BadgeView {
    BadgeView { id: b.id.clone(), name: b.name.clone(), description: b.description.clone() }
}

fn course_view(course: &Course) -> CourseView {
    let units = course
        .units
        .iter()
        .map(|u| UnitView {
            id: u.id.clone(),
            title: u.title.clone(),
            summary: u.summary.clone(),
            color: u.color.clone(),
            image: u.image.clone(),
            capstone: u.capstone,
            badge: badge_view(&u.badge),
            lessons: u
                .lessons
                .iter()
                .map(|l| LessonSummary {
                    id: l.id.clone(),
                    title: l.title.clone(),
                    summary: l.summary.clone(),
                    minutes: l.minutes,
                    lab_minutes: l.lab.as_ref().map(|lab| lab.minutes),
                    lab_steps: l.lab.as_ref().map_or(0, |lab| lab.steps.len() as u32),
                    questions: l.quiz.len() as u32,
                })
                .collect(),
        })
        .collect();
    let extra_badges = progress::EXTRA_BADGES
        .iter()
        .map(|(id, name, description)| BadgeView {
            id: (*id).into(),
            name: (*name).into(),
            description: (*description).into(),
        })
        .collect();
    CourseView { units, extra_badges }
}

fn lesson_view(course: &Course, lesson: &Lesson) -> LessonView {
    let all: Vec<&Lesson> = course.lessons().collect();
    let i = all.iter().position(|l| l.id == lesson.id).unwrap_or(0);
    LessonView {
        id: lesson.id.clone(),
        unit: lesson.unit.clone(),
        title: lesson.title.clone(),
        summary: lesson.summary.clone(),
        minutes: lesson.minutes,
        body: lesson.body.clone(),
        lab: lesson.lab.as_ref().map(|lab| LabInfo {
            title: lab.title.clone(),
            goal: lab.goal.clone(),
            minutes: lab.minutes,
            steps: lab.steps.iter().map(|s| s.text.trim().to_string()).collect(),
        }),
        quiz: lesson
            .quiz
            .iter()
            .map(|q| QuestionView { question: q.question.clone(), options: q.options.clone() })
            .collect(),
        prev: i.checked_sub(1).map(|p| all[p].id.clone()),
        next: all.get(i + 1).map(|l| l.id.clone()),
    }
}

fn progress_view(course: &Course, p: &Progress, clock: &Clock) -> ProgressView {
    let level = p.level();
    let ms = |t: i64| t as f64;
    ProgressView {
        xp: p.xp,
        level,
        rank: progress::rank_for(level).into(),
        level_xp: progress::xp_for_level(level),
        next_level_xp: progress::xp_for_level(level + 1),
        streak: p.streak.days,
        best_streak: p.streak.best,
        active_today: p.streak.last_day.as_deref() == Some(clock.day.as_str()),
        badges: p.badges.iter().map(|(id, at)| EarnedBadge { id: id.clone(), at: ms(*at) }).collect(),
        lessons: p
            .lessons
            .iter()
            .map(|(id, l)| LessonState {
                id: id.clone(),
                completed: l.completed_at.is_some(),
                steps_done: l.steps.len() as u32,
                lab_done: l.lab_done,
                quiz_best: l.quiz_best,
            })
            .collect(),
        units: p.units.keys().cloned().collect(),
        last_lesson: p.last_lesson.clone(),
        graduated_at: p.graduated_at.map(ms),
        completed_lessons: course.lessons().filter(|l| p.is_complete(&l.id)).count() as u32,
        total_lessons: course.lessons().count() as u32,
    }
}

fn lab_view(course: &Course, lab: &ActiveLab) -> LabView {
    let lesson = course.lesson(&lab.lesson);
    let spec = lesson.and_then(|l| l.lab.as_ref());
    let steps = lab
        .steps
        .iter()
        .enumerate()
        .map(|(i, run)| {
            let step = spec.and_then(|s| s.steps.get(i));
            let hints = step.map(|s| s.hints.clone()).unwrap_or_default();
            StepView {
                text: step.map(|s| s.text.trim().to_string()).unwrap_or_default(),
                hints: hints.iter().take(run.hints as usize).cloned().collect(),
                hints_total: hints.len() as u32,
                done: run.done,
                assisted: run.assisted,
                answer: step.is_some_and(|s| s.check.wants_answer()),
                probe: step.is_some_and(|s| checks::has_probe(&s.check)),
            }
        })
        .collect();
    LabView {
        lesson: lab.lesson.clone(),
        title: spec.map(|s| s.title.clone()).unwrap_or_default(),
        goal: spec.map(|s| s.goal.clone()).unwrap_or_default(),
        started_at: lab.started_at as f64,
        steps,
        vars: lab.vars.iter().map(|(key, value)| LabVar { key: key.clone(), value: value.clone() }).collect(),
        servers: lab
            .servers
            .iter()
            .map(|s| LabServerView { name: s.name.clone(), url: s.url.clone(), server_id: s.file_id.clone() })
            .collect(),
        finished: lab.finished,
    }
}

fn course() -> ApiResult<&'static Course> {
    zorvik_academy::course().map_err(|e| ApiError::new("internal", format!("The course could not be loaded: {e}")))
}

fn lesson(id: &str) -> ApiResult<&'static Lesson> {
    course()?.lesson(id).ok_or_else(|| ApiError::new("notFound", format!("No lesson '{id}'")))
}

/// A random value for `{{secret.*}}`: easy to read and type.
fn random_secret() -> String {
    const WORDS: &[&str] = &[
        "amber", "comet", "coral", "ember", "falcon", "maple", "nova", "otter", "pixel", "quartz", "river", "saffron",
        "tiger", "violet", "willow", "zephyr", "cobalt", "lotus", "mango", "orbit",
    ];
    let bytes = *uuid::Uuid::new_v4().as_bytes();
    let word = WORDS[bytes[0] as usize % WORDS.len()];
    let number = u16::from_le_bytes([bytes[1], bytes[2]]) % 9000 + 1000;
    format!("{word}-{number}")
}

fn same_dir(a: &Path, b: &Path) -> bool {
    let canon = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    a == b || canon(a) == canon(b)
}

impl Api {
    /// `academy.*` methods.
    pub(crate) async fn call_academy(&self, method: &str, p: Value) -> ApiResult<Value> {
        #[derive(Deserialize)]
        struct Id {
            id: String,
        }
        #[derive(Deserialize)]
        struct StepParam {
            step: usize,
        }
        match method {
            "academy.course" => ok(course_view(course()?)),
            "academy.progress" => {
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase")]
                struct P {
                    utc_offset_minutes: Option<i32>,
                }
                let P { utc_offset_minutes } = params(p)?;
                if let Some(offset) = utc_offset_minutes {
                    self.inner.academy.utc_offset.store(offset.clamp(-14 * 60, 14 * 60), Ordering::Relaxed);
                }
                ok(self.academy_progress_view()?)
            }
            "academy.lesson" => {
                let Id { id } = params(p)?;
                let lesson = lesson(&id)?;
                self.with_progress(|p| p.last_lesson = Some(id.clone()));
                ok(lesson_view(course()?, lesson))
            }
            "academy.workspace" => {
                let _one = self.inner.academy.lifecycle.lock().await;
                let dir = self.ensure_bootcamp()?;
                // Servers of a lab that ended with the app are cleared away.
                if lock(&self.inner.academy.lab).is_none()
                    && let Ok(ws) = Workspace::open(&dir)
                {
                    self.remove_lab_servers(&ws);
                }
                ok(json!({ "path": dir.to_string_lossy() }))
            }
            "academy.reset" => {
                let _one = self.inner.academy.lifecycle.lock().await;
                ok(json!({ "path": Box::pin(self.reset_bootcamp()).await?.to_string_lossy() }))
            }
            "academy.startLab" => {
                let Id { id } = params(p)?;
                let _one = self.inner.academy.lifecycle.lock().await;
                ok(Box::pin(self.start_lab(&id)).await?)
            }
            "academy.stopLab" => {
                let _one = self.inner.academy.lifecycle.lock().await;
                self.stop_lab();
                self.emit_academy(None);
                ok(())
            }
            "academy.lab" => ok(self.lab_view()),
            "academy.check" => {
                self.evaluate_now(checks::Scope::ALL).await;
                ok(self.lab_view())
            }
            "academy.hint" => {
                let StepParam { step } = params(p)?;
                let total = self.current_lab_step(step)?.hints.len() as u32;
                self.update_lab(|lab| {
                    if let Some(run) = lab.steps.get_mut(step) {
                        run.hints = (run.hints + 1).min(total);
                    }
                });
                self.emit_academy(None);
                ok(self.lab_view())
            }
            "academy.answer" => {
                #[derive(Deserialize)]
                struct P {
                    step: usize,
                    value: String,
                }
                let P { step, value } = params(p)?;
                self.current_lab_step(step)?;
                self.update_lab(|lab| {
                    if let Some(run) = lab.steps.get_mut(step) {
                        run.answer = Some(value.trim().to_string());
                    }
                });
                self.evaluate_now(checks::Scope::ALL).await;
                let correct = self.with_lab(|lab| lab.steps.get(step).is_some_and(|s| s.done)).unwrap_or(false);
                ok(json!({ "correct": correct, "lab": self.lab_view() }))
            }
            "academy.doStep" => {
                let StepParam { step } = params(p)?;
                let api = self.clone();
                // On a task of its own: sends nest deep futures, too deep for a caller's stack in debug builds.
                tokio::spawn(async move { api.do_step(step).await }).await.map_err(crate::join_err)??;
                ok(self.lab_view())
            }
            "academy.quiz" => {
                #[derive(Deserialize)]
                struct P {
                    id: String,
                    answers: Vec<Option<usize>>,
                }
                let P { id, answers } = params(p)?;
                ok(self.grade_quiz(&id, &answers)?)
            }
            "academy.markRead" => {
                let Id { id } = params(p)?;
                let lesson = lesson(&id)?;
                if lesson.lab.is_some() || !lesson.quiz.is_empty() {
                    return Err(ApiError::invalid("This lesson is done by its lab and quiz"));
                }
                let clock = self.clock();
                let course = course()?;
                let rewards = self.with_progress(|p| {
                    let mut r = Rewards::default();
                    p.read_done(course, lesson, &clock, &mut r);
                    r
                });
                self.emit_academy(Some(rewards));
                ok(())
            }
            "academy.testOut" => {
                let Id { id } = params(p)?;
                ok(test_out_questions(&id)?)
            }
            "academy.testOutSubmit" => {
                #[derive(Deserialize)]
                struct P {
                    id: String,
                    answers: Vec<(String, usize)>,
                }
                let P { id, answers } = params(p)?;
                ok(self.grade_test_out(&id, &answers)?)
            }
            "academy.saveCertificate" => {
                #[derive(Deserialize)]
                struct P {
                    path: String,
                    /// PNG, base64.
                    png: String,
                }
                let P { path, png } = params(p)?;
                use base64::Engine as _;
                let data = base64::engine::general_purpose::STANDARD
                    .decode(png.as_bytes())
                    .map_err(|_| ApiError::invalid("Not a PNG image"))?;
                if !data.starts_with(b"\x89PNG") || !path.to_lowercase().ends_with(".png") {
                    return Err(ApiError::invalid("The certificate is saved as a .png file"));
                }
                if self.with_progress(|p| p.graduated_at.is_none()) {
                    return Err(ApiError::invalid("Finish the capstone to get the certificate"));
                }
                std::fs::write(&path, data)
                    .map_err(|e| ApiError::new("io", format!("Could not save the certificate: {e}")))?;
                ok(())
            }
            other => Err(ApiError::new("notFound", format!("Unknown method '{other}'"))),
        }
    }

    // ---- the Bootcamp workspace ----------------------------------------------

    fn bootcamp_dir(&self) -> PathBuf {
        self.inner.data_dir.join(WORKSPACE_DIR)
    }

    /// The Bootcamp workspace folder, created when missing.
    fn ensure_bootcamp(&self) -> ApiResult<PathBuf> {
        let dir = self.bootcamp_dir();
        if Workspace::open(&dir).is_err() {
            std::fs::create_dir_all(&dir)
                .map_err(|e| ApiError::new("io", format!("Could not create the Bootcamp workspace: {e}")))?;
            Workspace::create(&dir, WORKSPACE_NAME)?;
        }
        Ok(dir)
    }

    fn is_bootcamp(&self, ws: &Workspace) -> bool {
        same_dir(ws.root(), &self.bootcamp_dir())
    }

    /// Start the Bootcamp workspace over: labs and servers stop, files go, progress stays.
    async fn reset_bootcamp(&self) -> ApiResult<PathBuf> {
        self.stop_lab();
        let dir = self.bootcamp_dir();
        let was_open = self.try_ws().is_some_and(|ws| self.is_bootcamp(&ws));
        self.stop_servers_in(&dir.to_string_lossy());
        if was_open {
            *self.inner.workspace.write().unwrap_or_else(|e| e.into_inner()) = None;
            *lock(&self.inner.watcher) = None;
        }
        if zorvik_workspace::fsutil::is_symlink(&dir) {
            return Err(ApiError::invalid("The Bootcamp folder is a link; it is not removed"));
        }
        // A learner may have trusted the lab's CA file in Settings: it stays, or every HTTPS
        // request would fail on the missing file.
        let lab_ca = std::fs::read(dir.join(LAB_CA)).ok();
        let remove = dir.clone();
        tokio::task::spawn_blocking(move || match std::fs::remove_dir_all(&remove) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        })
        .await
        .map_err(crate::join_err)?
        .map_err(|e| ApiError::new("io", format!("Could not reset the Bootcamp workspace: {e}")))?;
        let dir = self.ensure_bootcamp()?;
        if let Some(ca) = lab_ca {
            let _ = std::fs::write(dir.join(LAB_CA), ca);
        }
        if was_open {
            self.activate(Workspace::open(&dir)?)?;
        }
        Ok(dir)
    }

    // ---- progress --------------------------------------------------------------

    fn clock(&self) -> Clock {
        let now = time::OffsetDateTime::now_utc();
        let offset = self.inner.academy.utc_offset.load(Ordering::Relaxed);
        let local = now + time::Duration::minutes(i64::from(offset));
        Clock {
            now: crate::now_ms(),
            day: format!("{:04}-{:02}-{:02}", local.year(), u8::from(local.month()), local.day()),
            hour: u32::from(local.hour()),
        }
    }

    /// Run `f` on the progress (loaded on first use), saving it when it changed.
    fn with_progress<T>(&self, f: impl FnOnce(&mut Progress) -> T) -> T {
        let path = self.inner.data_dir.join(PROGRESS_FILE);
        let mut slot = lock(&self.inner.academy.progress);
        let progress = slot.get_or_insert_with(|| match std::fs::read(&path) {
            Ok(data) => serde_json::from_slice(&data).unwrap_or_else(|e| {
                // Keep the unreadable file for recovery instead of overwriting it.
                tracing::warn!("academy progress unreadable, starting over: {e}");
                let _ = std::fs::rename(&path, path.with_extension("json.bak"));
                Progress::default()
            }),
            Err(_) => Progress::default(),
        });
        let before = progress.clone();
        let out = f(progress);
        if *progress != before
            && let Err(e) = zorvik_workspace::store::save_json(&path, progress)
        {
            tracing::warn!("could not save academy progress: {}", e.message);
        }
        out
    }

    fn academy_progress_view(&self) -> ApiResult<ProgressView> {
        let course = course()?;
        let clock = self.clock();
        Ok(self.with_progress(|p| progress_view(course, p, &clock)))
    }

    /// Tell the UI what changed (the lab, progress, and `rewards` when something was earned).
    fn emit_academy(&self, rewards: Option<Rewards>) {
        let Ok(progress) = self.academy_progress_view() else { return };
        let update = AcademyUpdate { lab: self.lab_view(), progress, rewards: rewards.filter(|r| !r.is_empty()) };
        self.inner.sink.emit(StreamEvent::Academy { update: Box::new(update) });
    }

    fn grade_quiz(&self, id: &str, answers: &[Option<usize>]) -> ApiResult<QuizResult> {
        let course = course()?;
        let lesson = lesson(id)?;
        let results: Vec<QuestionResult> = lesson
            .quiz
            .iter()
            .enumerate()
            .map(|(i, q)| QuestionResult {
                correct: answers.get(i).copied().flatten() == Some(q.answer),
                answer: q.answer as u32,
                explain: q.explain.clone(),
            })
            .collect();
        let right = results.iter().filter(|r| r.correct).count() as u32;
        let clock = self.clock();
        let (passed, rewards) = self.with_progress(|p| {
            let mut r = Rewards::default();
            let passed = p.quiz_done(course, lesson, right, &clock, &mut r);
            (passed, r)
        });
        self.emit_academy(Some(rewards));
        Ok(QuizResult { total: results.len() as u32, results, right, passed })
    }

    fn grade_test_out(&self, unit: &str, answers: &[(String, usize)]) -> ApiResult<QuizResult> {
        let course = course()?;
        let questions = test_out_questions(unit)?;
        // One answer per question: the last one given.
        let answers: HashMap<&str, usize> = answers.iter().map(|(id, a)| (id.as_str(), *a)).collect();
        let results: Vec<QuestionResult> = questions
            .iter()
            .map(|q| {
                let (lesson, i) = q.id.split_once(':').unwrap_or_default();
                let question = course.lesson(lesson).and_then(|l| l.quiz.get(i.parse::<usize>().ok()?));
                let right = question.map_or(0, |x| x.answer);
                QuestionResult {
                    correct: answers.get(q.id.as_str()) == Some(&right),
                    answer: right as u32,
                    explain: question.map(|x| x.explain.clone()).unwrap_or_default(),
                }
            })
            .collect();
        let right = results.iter().filter(|r| r.correct).count() as u32;
        let total = results.len() as u32;
        let passed = total > 0 && f64::from(right) >= f64::from(total) * progress::TEST_OUT_PASS;
        let mut rewards = None;
        if passed {
            let clock = self.clock();
            rewards = Some(self.with_progress(|p| {
                let mut r = Rewards::default();
                p.unit_done(course, unit, &clock, &mut r);
                r
            }));
        }
        self.emit_academy(rewards);
        // A failed attempt shows the score only: the right answers would make the next one trivial.
        let results = if passed { results } else { Vec::new() };
        Ok(QuizResult { results, right, total, passed })
    }

    // ---- labs ------------------------------------------------------------------

    fn with_lab<T>(&self, f: impl FnOnce(&ActiveLab) -> T) -> Option<T> {
        lock(&self.inner.academy.lab).as_ref().map(f)
    }

    fn update_lab(&self, f: impl FnOnce(&mut ActiveLab)) {
        if let Some(lab) = lock(&self.inner.academy.lab).as_mut() {
            f(lab);
        }
    }

    fn lab_view(&self) -> Option<LabView> {
        let course = zorvik_academy::course().ok()?;
        self.with_lab(|lab| lab_view(course, lab))
    }

    /// Step `i` of the running lab (an error when no lab runs or the step doesn't exist).
    fn current_lab_step(&self, i: usize) -> ApiResult<&'static zorvik_academy::Step> {
        let id = self.with_lab(|lab| lab.lesson.clone()).ok_or_else(|| ApiError::invalid("No lab is running"))?;
        lesson(&id)?.lab.as_ref().and_then(|l| l.steps.get(i)).ok_or_else(|| ApiError::invalid("No such step"))
    }

    async fn start_lab(&self, id: &str) -> ApiResult<LabView> {
        let lesson = lesson(id)?;
        let spec = lesson.lab.as_ref().ok_or_else(|| ApiError::invalid("This lesson has no lab"))?;
        let ws = self.ws()?;
        if !self.is_bootcamp(&ws) {
            return Err(ApiError::new("notBootcamp", "Labs run in the Training Bootcamp workspace. Open it first."));
        }
        self.stop_lab();
        self.remove_lab_servers(&ws);
        // Each lab starts clean: servers the learner ran in an earlier one stop (they stay saved).
        self.stop_servers_in(&ws.root().to_string_lossy());

        let secrets: HashMap<String, String> = spec.secrets.iter().map(|n| (n.clone(), random_secret())).collect();
        let mut vars: Vec<(String, String)> = Vec::new();
        if spec.playground.any() {
            vars.extend(self.playground(spec.playground, &ws).await?);
        }
        let lookup = |vars: &[(String, String)], name: &str| -> Option<String> {
            match name.strip_prefix("secret.") {
                Some(s) => secrets.get(s).cloned(),
                None => vars.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone()),
            }
        };

        let mut servers = Vec::new();
        let mut created: Vec<String> = Vec::new();
        for (key, raw) in &spec.servers {
            let value = matcher::substitute(raw, &|n| lookup(&vars, n));
            let mut server: Server = serde_json::from_value(value)
                .map_err(|e| ApiError::new("internal", format!("Lab server '{key}' is invalid: {e}")))?;
            server.name = format!("{LAB_SERVER_PREFIX}{}", server.name);
            server.auto_start = false;
            let file_id = ws.create_server(&server)?;
            created.push(file_id.clone());
            self.remember_lab_servers(&created);
            let info = match self.start_server(&ws, file_id.clone(), server.clone()).await {
                Ok(info) => info,
                Err(e) => {
                    let _ = ws.remove_server(&file_id);
                    self.stop_lab_servers(&ws, &servers);
                    return Err(e);
                }
            };
            // The file keeps the port, so starting it again by hand keeps the address.
            server.port = info.port;
            let _ = ws.save_server(&file_id, &server);
            self.trust_server(&ws, &file_id, &server);
            vars.push((key.clone(), info.url.clone()));
            vars.push((format!("{key}_port"), info.port.to_string()));
            vars.push((format!("{key}_host"), format!("127.0.0.1:{}", info.port)));
            servers.push(LabServer {
                key: key.clone(),
                file_id,
                run_id: info.run_id,
                name: server.name,
                url: info.url,
            });
        }
        for (key, value) in &spec.vars {
            let value = matcher::substitute_text(value, &|n| lookup(&vars, n));
            vars.push((key.clone(), value));
        }
        for (path, text) in &spec.files {
            let text = matcher::substitute_text(text, &|n| lookup(&vars, n));
            let Some(target) = crate::specs::spec_path(ws.root(), path) else {
                return Err(ApiError::new("internal", format!("Lab file '{path}' must stay inside the workspace")));
            };
            if let Some(parent) = target.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            std::fs::write(&target, text).map_err(|e| ApiError::new("io", format!("Could not write {path}: {e}")))?;
        }
        self.write_lab_environment(&ws, &vars).await?;

        let generation = self.inner.academy.generation.fetch_add(1, Ordering::Relaxed) + 1;
        let ticker = CancellationToken::new();
        let replaced = lock(&self.inner.academy.lab).replace(ActiveLab {
            generation,
            lesson: id.to_string(),
            started_at: crate::now_ms(),
            vars,
            secrets,
            servers,
            steps: vec![StepRun::default(); spec.steps.len()],
            finished: false,
            ticker: ticker.clone(),
        });
        if let Some(old) = replaced {
            old.ticker.cancel();
            self.stop_lab_servers(&ws, &old.servers);
        }
        lock(&self.inner.academy.journal).clear();
        self.inner.academy.active.store(true, Ordering::Relaxed);
        self.with_progress(|p| p.last_lesson = Some(id.to_string()));
        tokio::spawn(checks::tick(self.clone(), ticker));
        self.emit_academy(None);
        self.lab_view().ok_or_else(|| ApiError::new("internal", "The lab stopped"))
    }

    /// Stop the running lab: its servers stop and their files go; the playground stays up.
    fn stop_lab(&self) {
        self.inner.academy.active.store(false, Ordering::Relaxed);
        let Some(lab) = lock(&self.inner.academy.lab).take() else { return };
        lab.ticker.cancel();
        lock(&self.inner.academy.journal).clear();
        match self.try_ws().filter(|ws| self.is_bootcamp(ws)) {
            Some(ws) => self.stop_lab_servers(&ws, &lab.servers),
            None => {
                for s in &lab.servers {
                    self.stop_run(&s.run_id);
                }
            }
        }
    }

    fn stop_lab_servers(&self, ws: &Workspace, servers: &[LabServer]) {
        for s in servers {
            self.stop_run(&s.run_id);
            let _ = ws.remove_server(&s.file_id);
        }
        self.remember_lab_servers(&[]);
    }

    /// Note the servers a lab saved (none: forget them).
    fn remember_lab_servers(&self, ids: &[String]) {
        let path = self.inner.data_dir.join(LAB_SERVERS_FILE);
        if ids.is_empty() {
            let _ = std::fs::remove_file(path);
        } else if let Err(e) = zorvik_workspace::store::save_json(&path, &ids) {
            tracing::warn!("could not note the lab's servers: {}", e.message);
        }
    }

    /// Servers an earlier lab left behind (the app quit during it): only the ones it noted,
    /// and only while they still carry the lab's name.
    fn remove_lab_servers(&self, ws: &Workspace) {
        let path = self.inner.data_dir.join(LAB_SERVERS_FILE);
        let Ok(ids) = std::fs::read(&path).map(|d| serde_json::from_slice::<Vec<String>>(&d).unwrap_or_default())
        else {
            return;
        };
        for id in ids {
            if ws.read_server(&id).is_ok_and(|s| s.name.starts_with(LAB_SERVER_PREFIX)) {
                self.stop_server_file(ws, &id);
                let _ = ws.remove_server(&id);
            }
        }
        self.remember_lab_servers(&[]);
    }

    /// The "Lab" environment gets exactly the lab's variables, and becomes the active one.
    async fn write_lab_environment(&self, ws: &Workspace, vars: &[(String, String)]) -> ApiResult<()> {
        let variables: Vec<Variable> = vars
            .iter()
            .map(|(k, v)| Variable { key: k.clone(), value: v.clone(), enabled: true, secret: false })
            .collect();
        let existing = ws.list_environments()?.into_iter().find(|e| e.environment.name == LAB_ENV);
        let id = match existing {
            Some(entry) => {
                let env = Environment { variables, name: entry.environment.name };
                ws.save_environment(&entry.id, &env)?
            }
            None => ws.create_environment(&Environment { name: LAB_ENV.into(), variables })?,
        };
        Box::pin(self.dispatch("env.setActive", json!({ "id": id }))).await?;
        // Values an earlier lab's scripts set would override this lab's.
        Box::pin(self.dispatch("vars.clearLocal", json!({ "scope": "environment", "environmentId": id, "key": null })))
            .await?;
        Ok(())
    }

    /// Start what the lab needs of the playground; its variables.
    async fn playground(&self, want: zorvik_academy::Playground, ws: &Workspace) -> ApiResult<Vec<(String, String)>> {
        let mut pg = self.inner.academy.playground.lock().await;
        let mut vars = Vec::new();
        if want.http {
            if pg.http.is_none() {
                pg.http = Some(zorvik_testkit::TestServer::start().await);
            }
            vars.push(("playground".into(), pg.http.as_ref().unwrap().url("")));
        }
        if want.tls {
            if pg.tls.is_none() {
                let certs = zorvik_testkit::TestCerts::generate();
                let server = zorvik_testkit::TestServer::start_tls(&certs).await;
                pg.tls = Some((server, certs.ca_pem));
            }
            let (server, ca) = pg.tls.as_ref().unwrap();
            std::fs::write(ws.root().join(LAB_CA), ca)
                .map_err(|e| ApiError::new("io", format!("Could not write lab-ca.pem: {e}")))?;
            vars.push(("playgroundTls".into(), server.url("")));
        }
        if want.grpc {
            if pg.grpc.is_none() {
                pg.grpc = Some(zorvik_testkit::GrpcTestServer::start().await);
            }
            vars.push(("grpc".into(), pg.grpc.as_ref().unwrap().url()));
        }
        Ok(vars)
    }

    /// "Do it for me": run the step's solution; the step then passes without XP.
    async fn do_step(&self, i: usize) -> ApiResult<()> {
        let step = self.current_lab_step(i)?;
        if self.with_lab(|lab| lab.current() != Some(i)).unwrap_or(true) {
            return Err(ApiError::invalid("Do the steps before this one first"));
        }
        self.update_lab(|lab| {
            if let Some(run) = lab.steps.get_mut(i) {
                run.assisted = true;
            }
        });
        for action in &step.solution {
            Box::pin(self.run_action(i, action)).await?;
        }
        // What the solution started may land a moment later (server traffic, background runs).
        for _ in 0..20 {
            self.evaluate_now(checks::Scope::ALL).await;
            if self.with_lab(|lab| lab.steps.get(i).is_none_or(|s| s.done)).unwrap_or(true) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        }
        Ok(())
    }

    async fn run_action(&self, step: usize, action: &zorvik_academy::Action) -> ApiResult<()> {
        use zorvik_academy::Action;
        // `secrets_only`: leave the other `{{names}}` for the Lab environment to fill in.
        let fill = |v: &Value, secrets_only: bool| {
            let lab = lock(&self.inner.academy.lab);
            match lab.as_ref() {
                Some(lab) => matcher::substitute(v, &|n| {
                    if secrets_only && !n.starts_with("secret.") { None } else { lab.lookup(n) }
                }),
                None => v.clone(),
            }
        };
        let subst = |v: &Value| fill(v, false);
        let request = |v: &Value, secrets_only: bool| {
            let mut r = json!({ "name": "Lab request", "seq": 0 });
            if let (Some(base), Value::Object(fields)) = (r.as_object_mut(), fill(v, secrets_only)) {
                base.extend(fields);
            }
            r
        };
        match action {
            Action::Send(v) => {
                let id = uuid::Uuid::new_v4().to_string();
                // A failed send (e.g. a TLS error the step is about) is what the step checks.
                let _ = self
                    .call("http.send", json!({ "requestId": id, "request": request(v, false), "path": null }))
                    .await;
            }
            Action::Save(v) => {
                self.call("request.create", json!({ "parent": "", "request": request(v, true) })).await?;
            }
            Action::Call(c) => {
                let _ = self.call(&c.method, fill(&c.params, SAVES_FILES.contains(&c.method.as_str()))).await;
            }
            Action::Answer(a) => {
                let value = matcher::text_of(&subst(&Value::String(a.clone())));
                self.update_lab(|lab| {
                    if let Some(run) = lab.steps.get_mut(step) {
                        run.answer = Some(value);
                    }
                });
            }
            Action::Wait(ms) => tokio::time::sleep(std::time::Duration::from_millis((*ms).min(30_000))).await,
        }
        Ok(())
    }

    /// Check the lab now, on a task of its own (probes send requests: deep futures).
    async fn evaluate_now(&self, scope: checks::Scope) {
        let api = self.clone();
        let _ = tokio::spawn(async move { api.evaluate(scope).await }).await;
    }

    /// Whether to note an app call for the lab's checks: only while a lab runs, and only
    /// what happens in the Bootcamp workspace.
    pub(crate) fn academy_watching(&self, method: &str) -> bool {
        self.inner.academy.active.load(Ordering::Relaxed)
            && !method.starts_with("academy.")
            && !QUIET.contains(&method)
            && self.try_ws().is_some_and(|ws| self.is_bootcamp(&ws))
    }

    pub(crate) fn academy_observe(&self, method: &str, params: Value, result: &ApiResult<Value>) {
        let value = checks::fact_value(method, params, result);
        {
            let mut journal = lock(&self.inner.academy.journal);
            journal.push_back(Fact { at: crate::now_ms(), value });
            while journal.len() > JOURNAL_MAX {
                journal.pop_front();
            }
        }
        let api = self.clone();
        let probes = checks::PROBE_TRIGGERS.contains(&method);
        let settle = checks::SETTLE_TRIGGERS.contains(&method);
        tokio::spawn(async move {
            api.evaluate(checks::Scope { probes, files: true }).await;
            if settle {
                // A server logs an exchange as it answers: give its log a moment.
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                api.evaluate(checks::Scope::TICK).await;
            }
        });
    }
}

/// Up to eight questions from the unit's quizzes, spread over its lessons.
fn test_out_questions(unit: &str) -> ApiResult<Vec<TestOutQuestion>> {
    let unit = course()?.unit(unit).ok_or_else(|| ApiError::new("notFound", format!("No unit '{unit}'")))?;
    if unit.capstone {
        return Err(ApiError::invalid("The capstone is a project: it can't be tested out of"));
    }
    let mut out = Vec::new();
    let most = unit.lessons.iter().map(|l| l.quiz.len()).max().unwrap_or(0);
    'outer: for round in 0..most {
        for lesson in &unit.lessons {
            if let Some(q) = lesson.quiz.get(round) {
                out.push(TestOutQuestion {
                    id: format!("{}:{round}", lesson.id),
                    question: q.question.clone(),
                    options: q.options.clone(),
                });
                if out.len() == 8 {
                    break 'outer;
                }
            }
        }
    }
    Ok(out)
}
