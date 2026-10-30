//! The offline rules `td2do check` applies to new TODOs.

use crate::config::{CheckConfig, Level};
use crate::output;
use crate::todo::{Ref, Todo};
use serde_json::{Value, json};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rule {
    /// A new TODO has no REF.
    NoRef,
    /// A new TODO's REF is malformed or names a prefix with no backend.
    BadRef,
    /// A new TODO's fingerprint equals another TODO's.
    DuplicateText,
}

impl Rule {
    pub fn name(self) -> &'static str {
        match self {
            Rule::NoRef => "no_ref",
            Rule::BadRef => "bad_ref",
            Rule::DuplicateText => "duplicate_text",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Fail,
    Warn,
}

impl Severity {
    pub fn name(self) -> &'static str {
        match self {
            Severity::Fail => "fail",
            Severity::Warn => "warn",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    pub rule: Rule,
    pub severity: Severity,
    pub todo: Todo,
    /// For `DuplicateText`, the TODO with the same fingerprint.
    pub other: Option<Todo>,
}

impl Violation {
    /// Why the TODO broke the rule, for people.
    pub fn message(&self) -> String {
        match (self.rule, &self.todo.reference, &self.other) {
            (Rule::NoRef, ..) => "no issue reference; write it as TODO(gh#123)".to_string(),
            (Rule::BadRef, Some(Ref::Unknown { prefix, raw }), _) => {
                format!("unknown REF prefix `{prefix}` in `{raw}`")
            }
            (Rule::BadRef, Some(Ref::Malformed(raw)), _) => format!("malformed REF `{raw}`"),
            (Rule::DuplicateText, _, Some(o)) => format!(
                "same text as {}:{}:{}",
                output::path_string(&o.path),
                o.line,
                o.col
            ),
            _ => String::new(),
        }
    }

    pub fn to_json(&self) -> Value {
        json!({
            "rule": self.rule.name(),
            "severity": self.severity.name(),
            "todo": output::todo_json(&self.todo),
            "other": self.other.as_ref().map(output::todo_json),
        })
    }
}

/// The rule violations of the TODOs `new` picks out. `todos` is every TODO in
/// the repo, sorted by (path, line, col); the result keeps that order.
pub fn run(todos: &[Todo], new: &dyn Fn(&Todo) -> bool, cfg: &CheckConfig) -> Vec<Violation> {
    let duplicate = match cfg.duplicate_text {
        Level::Fail => Some(Severity::Fail),
        Level::Warn => Some(Severity::Warn),
        Level::Off => None,
    };
    let mut by_fingerprint: HashMap<&str, Vec<&Todo>> = HashMap::new();
    for t in todos {
        by_fingerprint.entry(&t.fingerprint).or_default().push(t);
    }
    let mut out = Vec::new();
    for t in todos.iter().filter(|t| new(t)) {
        let mut add = |rule, severity, other: Option<&Todo>| {
            out.push(Violation {
                rule,
                severity,
                todo: t.clone(),
                other: other.cloned(),
            })
        };
        match &t.reference {
            None => add(Rule::NoRef, Severity::Fail, None),
            Some(Ref::Unknown { .. } | Ref::Malformed(_)) => {
                add(Rule::BadRef, Severity::Fail, None)
            }
            Some(Ref::Gh { .. }) => {}
        }
        if let Some(severity) = duplicate {
            let other = by_fingerprint[t.fingerprint.as_str()]
                .iter()
                .find(|o| (&o.path, o.line, o.col) != (&t.path, t.line, t.col));
            if let Some(other) = other {
                add(Rule::DuplicateText, severity, Some(other));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    fn todo(path: &str, line: usize, reference: Option<Ref>, fingerprint: &str) -> Todo {
        Todo {
            path: PathBuf::from(path),
            line,
            col: 4,
            tag: "TODO".into(),
            reference,
            extra: None,
            text: "x".into(),
            fingerprint: fingerprint.into(),
            last_line: line,
        }
    }

    fn gh(n: u64) -> Option<Ref> {
        Some(Ref::Gh {
            repo: None,
            number: n,
        })
    }

    fn rules(v: &[Violation]) -> Vec<(&str, usize, Rule, Severity)> {
        v.iter()
            .map(|v| {
                (
                    v.todo.path.to_str().unwrap(),
                    v.todo.line,
                    v.rule,
                    v.severity,
                )
            })
            .collect()
    }

    #[test]
    fn only_new_todos_are_checked() {
        let todos = [todo("a.rs", 1, None, "f1"), todo("b.rs", 1, None, "f2")];
        let v = run(
            &todos,
            &|t| t.path == Path::new("b.rs"),
            &CheckConfig::default(),
        );
        assert_eq!(rules(&v), [("b.rs", 1, Rule::NoRef, Severity::Fail)]);
    }

    #[test]
    fn bad_refs_fail() {
        let todos = [
            todo(
                "a.rs",
                1,
                Some(Ref::Unknown {
                    prefix: "relay".into(),
                    raw: "relay:x".into(),
                }),
                "f1",
            ),
            todo("a.rs", 2, Some(Ref::Malformed("gh#x".into())), "f2"),
            todo("a.rs", 3, gh(1), "f3"),
        ];
        let v = run(&todos, &|_| true, &CheckConfig::default());
        assert_eq!(
            rules(&v),
            [
                ("a.rs", 1, Rule::BadRef, Severity::Fail),
                ("a.rs", 2, Rule::BadRef, Severity::Fail)
            ]
        );
        assert!(v[0].message().contains("relay"), "{}", v[0].message());
        assert!(v[1].message().contains("gh#x"), "{}", v[1].message());
    }

    #[test]
    fn duplicate_levels() {
        let todos = [
            todo("a.rs", 1, gh(1), "same"),
            todo("b.rs", 1, gh(2), "same"),
        ];
        let new = |t: &Todo| t.path == Path::new("b.rs");
        let mut cfg = CheckConfig::default();
        let v = run(&todos, &new, &cfg);
        assert_eq!(
            rules(&v),
            [("b.rs", 1, Rule::DuplicateText, Severity::Warn)]
        );
        assert_eq!(v[0].other.as_ref().unwrap().path, PathBuf::from("a.rs"));
        assert_eq!(v[0].message(), "same text as a.rs:1:4");
        cfg.duplicate_text = Level::Fail;
        assert_eq!(run(&todos, &new, &cfg)[0].severity, Severity::Fail);
        cfg.duplicate_text = Level::Off;
        assert!(run(&todos, &new, &cfg).is_empty());
    }

    #[test]
    fn a_todo_is_not_its_own_duplicate() {
        let todos = [todo("a.rs", 1, gh(1), "f")];
        assert!(run(&todos, &|_| true, &CheckConfig::default()).is_empty());
    }

    #[test]
    fn json_shape() {
        let todos = [todo("a.rs", 1, None, "f")];
        let v = run(&todos, &|_| true, &CheckConfig::default());
        let j = v[0].to_json();
        assert_eq!(j["rule"], "no_ref");
        assert_eq!(j["severity"], "fail");
        assert_eq!(j["todo"]["file"], "a.rs");
        assert!(j["other"].is_null());
    }
}
