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

/// Writes `text` to stdout. A reader that closes early (`td2do list | head`)
/// is not an error (ruling R10).
pub fn print(text: &str) -> std::io::Result<()> {
    write_all_or_closed(&mut std::io::stdout().lock(), text)
}

fn write_all_or_closed(w: &mut impl std::io::Write, text: &str) -> std::io::Result<()> {
    match w.write_all(text.as_bytes()).and_then(|()| w.flush()) {
        Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => Ok(()),
        other => other,
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    struct Closed;

    impl std::io::Write for Closed {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::ErrorKind::BrokenPipe.into())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn closed_pipe_is_not_an_error() {
        assert!(write_all_or_closed(&mut Closed, "x\n").is_ok());
        let mut buf = Vec::new();
        write_all_or_closed(&mut buf, "a\nb\n").unwrap();
        assert_eq!(buf, b"a\nb\n");
    }
}
