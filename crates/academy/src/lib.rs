//! Training Bootcamp: the course embedded from `course/` (see `docs/academy.md`), the
//! patterns lab checks use ([`matcher`]) and the progress rules ([`progress`]).
//! Running labs (servers, checks against what the app saw) lives in `zorvik-api`.

pub mod matcher;
pub mod model;
pub mod progress;

use std::collections::HashSet;
use std::sync::OnceLock;

use serde::de::DeserializeOwned;

pub use model::*;

include!(concat!(env!("OUT_DIR"), "/course_files.rs"));

/// The course, parsed once. An error names the file that is wrong (the course tests
/// catch it before a release).
pub fn course() -> Result<&'static Course, &'static str> {
    static COURSE: OnceLock<Result<Course, String>> = OnceLock::new();
    COURSE.get_or_init(|| load(COURSE_FILES)).as_ref().map_err(String::as_str)
}

/// YAML parsed through JSON values, so enums read as `{variant: value}` maps.
fn parse_yaml<T: DeserializeOwned>(text: &str) -> Result<T, String> {
    let value: serde_json::Value = serde_yaml_ng::from_str(text).map_err(|e| e.to_string())?;
    serde_json::from_value(value).map_err(|e| e.to_string())
}

/// Front matter (between `---` lines) and the Markdown after it.
fn split_front_matter(text: &str) -> Option<(&str, &str)> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let rest = text.strip_prefix("---\n").or_else(|| text.strip_prefix("---\r\n"))?;
    let end = rest.find("\n---\n").or_else(|| rest.find("\n---\r\n"))?;
    let body = &rest[end + 4..];
    Some((&rest[..end], body.trim_start_matches(['\r', '\n'])))
}

fn load(files: &[(&str, &str)]) -> Result<Course, String> {
    let mut units: Vec<Unit> = Vec::new();
    for (path, text) in files {
        let Some((dir, name)) = path.split_once('/') else { continue };
        if name == "unit.yaml" {
            let meta: UnitMeta = parse_yaml(text).map_err(|e| format!("{path}: {e}"))?;
            units.push(Unit {
                id: meta.id,
                title: meta.title,
                summary: meta.summary,
                color: meta.color,
                image: meta.image,
                badge: meta.badge,
                capstone: meta.capstone,
                lessons: Vec::new(),
            });
            // Lessons of this folder follow (files are sorted, `unit.yaml` after the `NN-*.md`).
            let unit = units.last_mut().unwrap();
            for (lesson_path, lesson_text) in files.iter().filter(|(p, _)| {
                p.strip_prefix(dir).and_then(|r| r.strip_prefix('/')).is_some_and(|n| n.ends_with(".md"))
            }) {
                let (front, body) = split_front_matter(lesson_text)
                    .ok_or_else(|| format!("{lesson_path}: starts without a --- front matter block"))?;
                let mut meta: LessonMeta = parse_yaml(front).map_err(|e| format!("{lesson_path}: {e}"))?;
                if let Some(lab) = meta.lab.as_mut() {
                    lab.secrets = secret_names(front);
                }
                unit.lessons.push(Lesson {
                    id: meta.id,
                    unit: unit.id.clone(),
                    title: meta.title,
                    summary: meta.summary,
                    minutes: meta.minutes,
                    added: meta.added,
                    body: body.to_string(),
                    lab: meta.lab,
                    quiz: meta.quiz,
                });
            }
        }
    }
    let course = Course { units };
    validate(&course)?;
    Ok(course)
}

/// Checks a file's types can't: unique ids, known servers, answers in range.
fn validate(course: &Course) -> Result<(), String> {
    let mut ids = HashSet::new();
    let mut badges: HashSet<&str> = progress::EXTRA_BADGES.iter().map(|b| b.0).collect();
    for unit in &course.units {
        if !ids.insert(format!("unit:{}", unit.id)) {
            return Err(format!("unit '{}' twice", unit.id));
        }
        if !badges.insert(&unit.badge.id) {
            return Err(format!("badge '{}' twice", unit.badge.id));
        }
        if unit.lessons.is_empty() {
            return Err(format!("unit '{}' has no lessons", unit.id));
        }
        for lesson in &unit.lessons {
            let at = |e: String| format!("lesson '{}': {e}", lesson.id);
            if !ids.insert(lesson.id.clone()) {
                return Err(at("id used twice".into()));
            }
            for (i, q) in lesson.quiz.iter().enumerate() {
                if q.options.len() < 2 || q.answer >= q.options.len() {
                    return Err(at(format!("question {} needs two options or more and a valid answer", i + 1)));
                }
            }
            let Some(lab) = &lesson.lab else { continue };
            if lab.steps.is_empty() {
                return Err(at("the lab has no steps".into()));
            }
            for (id, server) in &lab.servers {
                if !is_var_name(id) {
                    return Err(at(format!("server id '{id}' must be a variable name (letters, digits, _)")));
                }
                let server: zorvik_formats::Server =
                    serde_json::from_value(server.clone()).map_err(|e| at(format!("server '{id}': {e}")))?;
                if server.name.trim().is_empty() {
                    return Err(at(format!("server '{id}' needs a name")));
                }
            }
            for (i, step) in lab.steps.iter().enumerate() {
                let at = |e: String| at(format!("step {}: {e}", i + 1));
                if step.solution.is_empty() {
                    return Err(at("needs a solution".into()));
                }
                if step.hints.len() > 3 {
                    return Err(at("at most three hints".into()));
                }
                check_servers(&step.check, lab).map_err(at)?;
            }
        }
    }
    Ok(())
}

/// `name` of every `{{secret.name}}` in `text`, once each.
fn secret_names(text: &str) -> Vec<String> {
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    let re = RE.get_or_init(|| regex::Regex::new(r"\{\{\s*secret\.([A-Za-z0-9_]+)\s*\}\}").unwrap());
    let mut names: Vec<String> = Vec::new();
    for c in re.captures_iter(text) {
        if !names.iter().any(|n| n == &c[1]) {
            names.push(c[1].to_string());
        }
    }
    names
}

fn is_var_name(s: &str) -> bool {
    let mut chars = s.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn check_servers(check: &Check, lab: &Lab) -> Result<(), String> {
    match check {
        Check::Request(t) | Check::Message(t) if !lab.servers.contains_key(&t.server) => {
            Err(format!("checks server '{}', which the lab does not start", t.server))
        }
        Check::All(list) | Check::Any(list) => list.iter().try_for_each(|c| check_servers(c, lab)),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn course_loads() {
        let course = course().unwrap_or_else(|e| panic!("{e}"));
        assert!(!course.units.is_empty());
        assert!(course.units.iter().filter(|u| u.capstone).count() <= 1);
    }

    #[test]
    fn front_matter() {
        assert_eq!(split_front_matter("---\na: 1\n---\n\n# Hi\n"), Some(("a: 1", "# Hi\n")));
        assert_eq!(split_front_matter("# no"), None);
    }
}
