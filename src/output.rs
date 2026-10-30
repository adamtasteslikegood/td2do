//! Renders TODOs for people (terminal, plain) and for machines (JSON).

use crate::todo::{Ref, Todo};
use serde_json::{Value, json};
use std::fmt::Write;
use std::path::Path;

const RESET: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const RED: &str = "\x1b[31m";
const YELLOW: &str = "\x1b[33m";

/// A repo-relative path with `/` separators on every platform.
pub fn path_string(p: &Path) -> String {
    p.components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// The REF as written: `gh#12`, `gh:o/r#5`, or the raw text of an unknown or malformed one.
pub fn ref_string(r: &Ref) -> String {
    match r {
        Ref::Gh { repo: None, number } => format!("gh#{number}"),
        Ref::Gh {
            repo: Some(repo),
            number,
        } => format!("gh:{repo}#{number}"),
        Ref::Unknown { raw, .. } => raw.clone(),
        Ref::Malformed(raw) => raw.clone(),
    }
}

/// One line per TODO: `path:line:col: TAG(REF) text`.
pub fn render_plain(todos: &[Todo]) -> String {
    let mut out = String::new();
    for t in todos {
        let _ = write!(
            out,
            "{}:{}:{}: {}",
            path_string(&t.path),
            t.line,
            t.col,
            t.tag
        );
        if let Some(r) = &t.reference {
            let _ = write!(out, "({})", ref_string(r));
        }
        if !t.text.is_empty() {
            let _ = write!(out, " {}", t.text);
        }
        out.push('\n');
    }
    out
}

fn tag_colour(tag: &str) -> &'static str {
    match tag {
        "FIXME" | "BUG" => RED,
        "HACK" | "XXX" => YELLOW,
        _ => "",
    }
}

/// TODOs grouped under a header per file, with a summary line.
pub fn render_terminal(todos: &[Todo], color: bool) -> String {
    let paint = |style: &str, s: &str| {
        if color && !style.is_empty() {
            format!("{style}{s}{RESET}")
        } else {
            s.to_string()
        }
    };
    let mut out = String::new();
    let mut current: Option<&Path> = None;
    for t in todos {
        if current != Some(t.path.as_path()) {
            if current.is_some() {
                out.push('\n');
            }
            let _ = writeln!(out, "{}", paint(BOLD, &path_string(&t.path)));
            current = Some(t.path.as_path());
        }
        let reference = match &t.reference {
            Some(r) => ref_string(r),
            None => paint(DIM, "untracked"),
        };
        let _ = writeln!(
            out,
            "  {}:{}  {}  {}  {}",
            t.line,
            t.col,
            paint(tag_colour(&t.tag), &t.tag),
            reference,
            t.text
        );
    }
    if current.is_some() {
        out.push('\n');
    }
    let untracked = todos.iter().filter(|t| t.reference.is_none()).count();
    let _ = writeln!(out, "{} TODOs, {} untracked", todos.len(), untracked);
    out
}

pub fn todo_json(t: &Todo) -> Value {
    json!({
        "file": path_string(&t.path),
        "line": t.line,
        "col": t.col,
        "tag": t.tag,
        "ref": t.reference.as_ref().map(ref_string),
        "extra": t.extra,
        "text": t.text,
        "fingerprint": t.fingerprint,
    })
}
