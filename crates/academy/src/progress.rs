//! What the learner has done (kept in the app data folder, not in the workspace) and the
//! rules that turn it into XP, levels, ranks, streaks and badges.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::model::{Course, Lesson};

pub const XP_STEP: u32 = 10;
pub const XP_LESSON: u32 = 50;
/// Per question answered right on the first try.
pub const XP_QUESTION: u32 = 5;
/// Every question of a quiz right on the first try.
pub const XP_PERFECT_QUIZ: u32 = 20;
pub const XP_UNIT: u32 = 100;
pub const XP_GRADUATE: u32 = 250;
/// Share of a quiz's questions to get right for the lesson (and a unit's test-out) to count.
pub const QUIZ_PASS: f64 = 0.6;
pub const TEST_OUT_PASS: f64 = 0.8;

/// Ranks by the level they start at.
pub const RANKS: &[(u32, &str)] = &[
    (1, "Newbie Node"),
    (3, "Packet Pusher"),
    (6, "Header Hacker"),
    (10, "Protocol Pro"),
    (14, "Network Ninja"),
    (18, "Wire Wizard"),
];

/// XP needed to reach `level` (level 1 starts at 0).
pub fn xp_for_level(level: u32) -> u32 {
    20 * level.saturating_sub(1) * level
}

pub fn level_for(xp: u32) -> u32 {
    let mut level = 1;
    while xp_for_level(level + 1) <= xp {
        level += 1;
    }
    level
}

pub fn rank_for(level: u32) -> &'static str {
    RANKS.iter().rev().find(|(from, _)| level >= *from).map_or(RANKS[0].1, |(_, name)| name)
}

/// Badges earned by how the learner works rather than by finishing a unit.
pub const EXTRA_BADGES: &[(&str, &str, &str)] = &[
    ("no-hints", "No Hints Needed", "Finish a lab without a hint."),
    ("perfect-score", "Perfect Score", "Answer every question of a quiz right the first time."),
    ("speed-runner", "Speed Runner", "Finish a lab of three steps or more in under two minutes."),
    ("night-owl", "Night Owl", "Finish a lesson between midnight and 5 am."),
    ("early-bird", "Early Bird", "Finish a lesson between 5 and 8 am."),
    ("on-fire", "On Fire", "Learn three days in a row."),
    ("unstoppable", "Unstoppable", "Learn seven days in a row."),
    ("halfway", "Halfway There", "Finish half of the lessons."),
];

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Progress {
    pub xp: u32,
    pub lessons: BTreeMap<String, LessonProgress>,
    /// Units finished (all lessons) or tested out of, by id, with the time.
    pub units: BTreeMap<String, i64>,
    /// Badges by id, with the time they were earned.
    pub badges: BTreeMap<String, i64>,
    pub streak: Streak,
    /// The lesson opened last ("Continue").
    pub last_lesson: Option<String>,
    pub graduated_at: Option<i64>,
    /// Lessons the learner has seen in the course: lessons an update adds show as new until
    /// opened. Empty in progress saved before 0.2 (see [`Progress::know_lessons`]).
    pub known_lessons: BTreeSet<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LessonProgress {
    pub completed_at: Option<i64>,
    /// Lab steps that earned XP (each earns it once).
    pub steps: BTreeSet<u32>,
    pub lab_done: bool,
    /// Best quiz score (right answers).
    pub quiz_best: Option<u32>,
    /// Whether the quiz was answered once (XP for first-try answers is given then).
    pub quiz_tried: bool,
    /// Read without a lab or quiz ("Mark as done").
    pub read: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Streak {
    pub days: u32,
    pub best: u32,
    /// Local date (`YYYY-MM-DD`) of the last day XP was earned.
    pub last_day: Option<String>,
}

/// What an action earned, for the celebration.
#[derive(Debug, Clone, Default, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Rewards {
    pub xp: u32,
    /// What earned the XP ("Step done", "Lesson complete", …).
    pub reasons: Vec<String>,
    /// The new level, when it went up.
    pub level_up: Option<u32>,
    /// The new rank, when it changed.
    pub rank_up: Option<String>,
    pub badges: Vec<String>,
    pub lesson_completed: Option<String>,
    pub unit_completed: Option<String>,
    pub graduated: bool,
}

impl Rewards {
    pub fn is_empty(&self) -> bool {
        self.xp == 0 && self.badges.is_empty() && self.lesson_completed.is_none() && self.unit_completed.is_none()
    }
}

/// The local time of an action: `now` in milliseconds, the local date and hour.
#[derive(Debug, Clone)]
pub struct Clock {
    pub now: i64,
    pub day: String,
    pub hour: u32,
}

/// Days between two `YYYY-MM-DD` dates (`b - a`).
fn days_between(a: &str, b: &str) -> Option<i64> {
    fn days(d: &str) -> Option<i64> {
        let mut it = d.split('-').map(|x| x.parse::<i64>().ok());
        let (y, m, d) = (it.next()??, it.next()??, it.next()??);
        // Days from civil (Howard Hinnant's algorithm).
        let y = if m <= 2 { y - 1 } else { y };
        let era = y.div_euclid(400);
        let yoe = y - era * 400;
        let mp = (m + 9) % 12;
        let doy = (153 * mp + 2) / 5 + d - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        Some(era * 146_097 + doe)
    }
    Some(days(b)? - days(a)?)
}

impl Progress {
    /// Fills `known_lessons` the first time: a new learner knows every lesson (none is new);
    /// one who started before lessons carried `added` knows the ones without it. Returns
    /// whether anything changed.
    pub fn know_lessons(&mut self, course: &Course) -> bool {
        if !self.known_lessons.is_empty() {
            return false;
        }
        let started = self.xp > 0 || !self.lessons.is_empty() || self.last_lesson.is_some();
        self.known_lessons = course.lessons().filter(|l| !started || l.added.is_none()).map(|l| l.id.clone()).collect();
        true
    }

    /// Added by an update and not opened (or finished) yet.
    pub fn is_new(&self, lesson: &str) -> bool {
        !self.known_lessons.is_empty() && !self.known_lessons.contains(lesson) && !self.is_complete(lesson)
    }

    pub fn level(&self) -> u32 {
        level_for(self.xp)
    }

    pub fn lesson(&mut self, id: &str) -> &mut LessonProgress {
        self.lessons.entry(id.to_string()).or_default()
    }

    pub fn is_complete(&self, lesson: &str) -> bool {
        self.lessons.get(lesson).is_some_and(|l| l.completed_at.is_some())
    }

    /// Add XP, keeping the streak and noting level and rank changes in `rewards`.
    fn earn(&mut self, xp: u32, reason: &str, clock: &Clock, rewards: &mut Rewards) {
        if xp == 0 {
            return;
        }
        let (level, rank) = (self.level(), rank_for(self.level()));
        self.xp += xp;
        rewards.xp += xp;
        rewards.reasons.push(reason.to_string());
        if self.level() > level {
            rewards.level_up = Some(self.level());
            if rank_for(self.level()) != rank {
                rewards.rank_up = Some(rank_for(self.level()).to_string());
            }
        }
        self.touch_streak(clock, rewards);
    }

    fn touch_streak(&mut self, clock: &Clock, rewards: &mut Rewards) {
        let s = &mut self.streak;
        if s.last_day.as_deref() == Some(clock.day.as_str()) {
            return;
        }
        s.days = match s.last_day.as_deref().and_then(|last| days_between(last, &clock.day)) {
            Some(1) => s.days + 1,
            _ => 1,
        };
        s.last_day = Some(clock.day.clone());
        s.best = s.best.max(s.days);
        let days = s.days;
        if days >= 3 {
            self.award("on-fire", clock, rewards);
        }
        if days >= 7 {
            self.award("unstoppable", clock, rewards);
        }
    }

    /// Give a badge once.
    pub fn award(&mut self, badge: &str, clock: &Clock, rewards: &mut Rewards) {
        if !self.badges.contains_key(badge) {
            self.badges.insert(badge.to_string(), clock.now);
            rewards.badges.push(badge.to_string());
        }
    }

    /// A lab step passed. `assisted`: "Do it for me" did it (no XP).
    pub fn step_done(&mut self, lesson: &str, step: u32, assisted: bool, clock: &Clock, rewards: &mut Rewards) {
        let first = self.lesson(lesson).steps.insert(step);
        if first && !assisted {
            self.earn(XP_STEP, "Step done", clock, rewards);
        }
    }

    /// Every step of the lab passed.
    #[allow(clippy::too_many_arguments)]
    pub fn lab_done(
        &mut self,
        course: &Course,
        lesson: &Lesson,
        hints: u32,
        assisted: bool,
        secs: i64,
        clock: &Clock,
        rewards: &mut Rewards,
    ) {
        self.lesson(&lesson.id).lab_done = true;
        let steps = lesson.lab.as_ref().map_or(0, |l| l.steps.len());
        if hints == 0 && !assisted {
            self.award("no-hints", clock, rewards);
        }
        if !assisted && steps >= 3 && secs < 120 {
            self.award("speed-runner", clock, rewards);
        }
        self.try_complete(course, lesson, clock, rewards);
    }

    /// A quiz was answered: `right` of the lesson's questions, `first_try` answers right on the
    /// first attempt (only counted the first time). Returns whether it passed.
    pub fn quiz_done(
        &mut self,
        course: &Course,
        lesson: &Lesson,
        right: u32,
        clock: &Clock,
        rewards: &mut Rewards,
    ) -> bool {
        let total = lesson.quiz.len() as u32;
        let p = self.lesson(&lesson.id);
        let first = !p.quiz_tried;
        p.quiz_tried = true;
        p.quiz_best = Some(p.quiz_best.unwrap_or(0).max(right));
        if first {
            self.earn(right * XP_QUESTION, "Quiz answers", clock, rewards);
            if right == total && total >= 3 {
                self.earn(XP_PERFECT_QUIZ, "Perfect quiz", clock, rewards);
                self.award("perfect-score", clock, rewards);
            }
        }
        let passed = f64::from(right) >= (f64::from(total) * QUIZ_PASS).ceil();
        if passed {
            self.try_complete(course, lesson, clock, rewards);
        }
        passed
    }

    /// "Mark as done" on a lesson with nothing to do but read.
    pub fn read_done(&mut self, course: &Course, lesson: &Lesson, clock: &Clock, rewards: &mut Rewards) {
        self.lesson(&lesson.id).read = true;
        self.try_complete(course, lesson, clock, rewards);
    }

    fn lesson_requirements_met(&self, lesson: &Lesson) -> bool {
        let Some(p) = self.lessons.get(&lesson.id) else { return false };
        let total = lesson.quiz.len() as u32;
        let lab = lesson.lab.is_none() || p.lab_done;
        let quiz = total == 0 || f64::from(p.quiz_best.unwrap_or(0)) >= (f64::from(total) * QUIZ_PASS).ceil();
        let read = lesson.lab.is_some() || total > 0 || p.read;
        lab && quiz && read
    }

    fn try_complete(&mut self, course: &Course, lesson: &Lesson, clock: &Clock, rewards: &mut Rewards) {
        if self.is_complete(&lesson.id) || !self.lesson_requirements_met(lesson) {
            return;
        }
        self.lesson(&lesson.id).completed_at = Some(clock.now);
        rewards.lesson_completed = Some(lesson.id.clone());
        self.earn(XP_LESSON, "Lesson complete", clock, rewards);
        match clock.hour {
            0..=4 => self.award("night-owl", clock, rewards),
            5..=7 => self.award("early-bird", clock, rewards),
            _ => {}
        }
        let total = course.lessons().count();
        let done = course.lessons().filter(|l| self.is_complete(&l.id)).count();
        if total > 0 && done * 2 >= total {
            self.award("halfway", clock, rewards);
        }
        let Some(unit) = course.unit_of(&lesson.id) else { return };
        if unit.lessons.iter().all(|l| self.is_complete(&l.id)) {
            self.unit_done(course, &unit.id, clock, rewards);
        }
    }

    /// A unit was finished or tested out of: its badge, and the unit XP once.
    pub fn unit_done(&mut self, course: &Course, unit: &str, clock: &Clock, rewards: &mut Rewards) {
        let Some(unit) = course.unit(unit) else { return };
        if self.units.contains_key(&unit.id) {
            return;
        }
        self.units.insert(unit.id.clone(), clock.now);
        rewards.unit_completed = Some(unit.id.clone());
        self.earn(XP_UNIT, "Unit complete", clock, rewards);
        self.award(&unit.badge.id, clock, rewards);
        if unit.capstone && self.graduated_at.is_none() {
            self.graduated_at = Some(clock.now);
            rewards.graduated = true;
            self.earn(XP_GRADUATE, "Graduated", clock, rewards);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clock(day: &str, hour: u32) -> Clock {
        Clock { now: 1, day: day.into(), hour }
    }

    #[test]
    fn levels_and_ranks() {
        assert_eq!(level_for(0), 1);
        assert_eq!(level_for(39), 1);
        assert_eq!(level_for(40), 2);
        assert_eq!(level_for(120), 3);
        assert_eq!(rank_for(1), "Newbie Node");
        assert_eq!(rank_for(4), "Packet Pusher");
        assert_eq!(rank_for(30), "Wire Wizard");
    }

    #[test]
    fn streaks() {
        let mut p = Progress::default();
        let mut r = Rewards::default();
        p.earn(10, "x", &clock("2026-02-27", 9), &mut r);
        p.earn(10, "x", &clock("2026-02-28", 9), &mut r);
        p.earn(10, "x", &clock("2026-03-01", 9), &mut r);
        assert_eq!(p.streak.days, 3);
        assert!(p.badges.contains_key("on-fire"));
        p.earn(10, "x", &clock("2026-03-03", 9), &mut r);
        assert_eq!((p.streak.days, p.streak.best), (1, 3));
        assert_eq!(days_between("2024-12-31", "2025-01-01"), Some(1));
    }

    #[test]
    fn lessons_added_by_an_update_are_new_for_earlier_learners() {
        let mut course = crate::course().unwrap().clone();
        let unit = &mut course.units[1];
        let mut added = unit.lessons[0].clone();
        added.id = "added-later".into();
        added.added = Some("0.2.0".into());
        unit.lessons.push(added);

        // Someone who learned before the update keeps everything, and sees only the new lesson as new.
        let old_id = course.units[0].lessons[0].id.clone();
        let mut p = Progress { xp: 120, last_lesson: Some(old_id.clone()), ..Default::default() };
        p.units.insert(course.units[1].id.clone(), 7);
        p.graduated_at = Some(9);
        assert!(p.know_lessons(&course));
        assert!(p.is_new("added-later"));
        assert!(!p.is_new(&old_id));
        assert!(!p.know_lessons(&course), "only once");
        assert_eq!((p.units.len(), p.graduated_at, p.xp), (1, Some(9), 120), "progress kept");
        p.known_lessons.insert("added-later".into());
        assert!(!p.is_new("added-later"), "opened");

        // A new learner has nothing new: every lesson is new to them anyway.
        let mut fresh = Progress::default();
        fresh.know_lessons(&course);
        assert!(!fresh.is_new("added-later"));
    }

    #[test]
    fn course_rules() {
        let course = crate::course().unwrap();
        let unit = &course.units[0];
        let mut p = Progress::default();
        let mut r = Rewards::default();
        let c = clock("2026-01-01", 2);
        for lesson in &unit.lessons {
            if let Some(lab) = &lesson.lab {
                for i in 0..lab.steps.len() as u32 {
                    p.step_done(&lesson.id, i, false, &c, &mut r);
                    p.step_done(&lesson.id, i, false, &c, &mut r);
                }
                p.lab_done(course, lesson, 0, false, 30, &c, &mut r);
            }
            if !lesson.quiz.is_empty() {
                p.quiz_done(course, lesson, lesson.quiz.len() as u32, &c, &mut r);
            }
            if lesson.lab.is_none() && lesson.quiz.is_empty() {
                p.read_done(course, lesson, &c, &mut r);
            }
            assert!(p.is_complete(&lesson.id), "{}", lesson.id);
        }
        assert!(p.units.contains_key(&unit.id));
        assert!(p.badges.contains_key(&unit.badge.id));
        assert!(p.badges.contains_key("night-owl"));
        assert_eq!(p.xp, r.xp);
    }
}
