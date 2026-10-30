//! Finds TODO tags in a comment, reads their issue references (REFs) and
//! computes the fingerprint that identifies a TODO across edits.

use crate::lexer::Comment;
use sha2::{Digest, Sha256};
use std::path::PathBuf;

/// An issue reference written in a TODO's parentheses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ref {
    /// `gh#123` or `gh:owner/repo#123` (and `#123` when `origin` is on GitHub).
    Gh { repo: Option<String>, number: u64 },
    /// A recognised prefix with no backend in phase 1 (`relay:`, `jira:`, a bare `#`).
    Unknown { prefix: String, raw: String },
    /// REF-shaped but not valid (`gh#abc`, `gh:o#5`).
    Malformed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Todo {
    pub path: PathBuf,
    pub line: usize,
    pub col: usize,
    pub tag: String,
    pub reference: Option<Ref>,
    pub extra: Option<String>,
    pub text: String,
    pub fingerprint: String,
}

/// One TODO found in a comment: line, column, tag, REF, extra, text.
pub type Parsed = (usize, usize, String, Option<Ref>, Option<String>, String);

/// The TODOs in one comment.
///
/// A tag starts a comment line (after whitespace) and is followed by `(`, `:`,
/// whitespace or the end of the line (ruling R2). Following lines with no tag
/// of their own continue its text until an empty line or another tag.
pub fn parse_comment(c: &Comment, tags: &[String], github_origin: bool) -> Vec<Parsed> {
    let mut out: Vec<Parsed> = Vec::new();
    // Whether the last TODO may still take continuation lines.
    let mut open = false;
    for l in &c.lines {
        let body = l.text.trim_start();
        if body.trim().is_empty() || l.text.contains(IGNORE) {
            open = false;
            continue;
        }
        let col = l.col + l.text.chars().count() - body.chars().count();
        match tag_at(body, tags) {
            Some((tag, rest)) => {
                let (reference, extra, text) = split_rest(rest, github_origin);
                out.push((l.line, col, tag.to_string(), reference, extra, text));
                open = true;
            }
            None if open => {
                if let Some(last) = out.last_mut() {
                    if !last.5.is_empty() {
                        last.5.push(' ');
                    }
                    last.5.push_str(body.trim_end());
                }
            }
            None => {}
        }
    }
    out
}

/// The inline escape: a line holding it yields no TODO.
const IGNORE: &str = "td2do:ignore";

/// The tag `body` starts with, and what follows it.
fn tag_at<'a>(body: &'a str, tags: &'a [String]) -> Option<(&'a str, &'a str)> {
    tags.iter().find_map(|tag| {
        let rest = body.strip_prefix(tag.as_str())?;
        match rest.chars().next() {
            None | Some('(' | ':') => Some((tag.as_str(), rest)),
            Some(ch) if ch.is_whitespace() => Some((tag.as_str(), rest)),
            _ => None,
        }
    })
}

/// Splits what follows a tag into REF, extra and text: `(inner): text`.
fn split_rest(rest: &str, github_origin: bool) -> (Option<Ref>, Option<String>, String) {
    let rest = rest.trim_start();
    let mut reference = None;
    let mut extra = None;
    let mut after = rest;
    if let Some(open) = rest.strip_prefix('(')
        && let Some((inner, tail)) = open.split_once(')')
    {
        after = tail;
        let (first, others) = match inner.split_once(',') {
            Some((a, b)) => (a.trim(), Some(b.trim())),
            None => (inner.trim(), None),
        };
        if is_ref_shaped(first) {
            reference = Some(parse_ref(first, github_origin));
            extra = others.filter(|e| !e.is_empty()).map(String::from);
        } else if !inner.trim().is_empty() {
            extra = Some(inner.trim().to_string());
        }
    }
    let text = after.trim_start().strip_prefix(':').unwrap_or(after);
    (reference, extra, text.trim().to_string())
}

fn is_ref_shaped(s: &str) -> bool {
    if s.starts_with('#') {
        return true;
    }
    match s.split_once(':') {
        Some((word, _)) => !word.is_empty() && word.chars().all(|c| c.is_ascii_lowercase()),
        None => s.starts_with("gh#"),
    }
}

/// Reads a REF-shaped string (see `is_ref_shaped`).
fn parse_ref(s: &str, github_origin: bool) -> Ref {
    let malformed = || Ref::Malformed(s.to_string());
    if let Some(n) = s.strip_prefix("gh#") {
        return number(n).map_or_else(malformed, |number| Ref::Gh { repo: None, number });
    }
    if let Some(n) = s.strip_prefix('#') {
        return match number(n) {
            Some(number) if github_origin => Ref::Gh { repo: None, number },
            Some(_) => Ref::Unknown {
                prefix: "#".into(),
                raw: s.to_string(),
            },
            None => malformed(),
        };
    }
    let (prefix, tail) = s.split_once(':').unwrap_or((s, ""));
    if prefix != "gh" {
        return Ref::Unknown {
            prefix: prefix.to_string(),
            raw: s.to_string(),
        };
    }
    match tail.split_once('#') {
        Some((repo, n)) if is_repo(repo) => number(n).map_or_else(malformed, |number| Ref::Gh {
            repo: Some(repo.to_string()),
            number,
        }),
        _ => malformed(),
    }
}

/// `owner/repo`: two non-empty parts and no whitespace.
fn is_repo(s: &str) -> bool {
    match s.split_once('/') {
        Some((owner, name)) => {
            !owner.is_empty()
                && !name.is_empty()
                && !name.contains('/')
                && !s.contains(char::is_whitespace)
        }
        None => false,
    }
}

/// Decimal digits only (so no sign), fitting a `u64`.
fn number(s: &str) -> Option<u64> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

/// `sha256(slug + tag + normalized text)`, as its first 16 hex characters.
pub fn fingerprint(slug: &str, tag: &str, text: &str) -> String {
    let mut hash = Sha256::new();
    hash.update(slug.as_bytes());
    hash.update(tag.as_bytes());
    hash.update(normalize(text).as_bytes());
    hash.finalize()
        .iter()
        .take(8)
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Lowercases `text`, drops REFs and comment markers around it, and collapses
/// whitespace.
pub fn normalize(text: &str) -> String {
    let mut words: Vec<String> = text
        .split_whitespace()
        .filter(|w| !is_gh_ref(w.trim_matches(|c| "()[]{}:,;".contains(c))))
        .map(str::to_lowercase)
        .collect();
    // Comment markers (`//`, `/*`, `*/`, `#`, `--`) only count at either end.
    let marker = |w: &String| w.chars().all(|c| "/*#!-;%".contains(c));
    while words.last().is_some_and(marker) {
        words.pop();
    }
    let lead = words.iter().take_while(|w| marker(w)).count();
    words.drain(..lead);
    words.join(" ")
}

fn is_gh_ref(w: &str) -> bool {
    match w.strip_prefix("gh#") {
        Some(n) => number(n).is_some(),
        None => w
            .strip_prefix("gh:")
            .and_then(|r| r.split_once('#'))
            .is_some_and(|(repo, n)| is_repo(repo) && number(n).is_some()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::for_path;
    use crate::lexer::{CommentLine, comments};
    use std::path::Path;

    fn tags() -> Vec<String> {
        ["TODO", "FIXME", "HACK", "XXX", "BUG"]
            .map(String::from)
            .to_vec()
    }

    /// A comment whose lines each start at column 4 (after `// `).
    fn comment(lines: &[&str]) -> Comment {
        Comment {
            start_line: 1,
            lines: lines
                .iter()
                .enumerate()
                .map(|(i, t)| CommentLine {
                    line: i + 1,
                    col: 3,
                    text: format!(" {t}"),
                })
                .collect(),
        }
    }

    fn parse(lines: &[&str]) -> Vec<Parsed> {
        parse_comment(&comment(lines), &tags(), true)
    }

    fn parse_origin(line: &str, github_origin: bool) -> Vec<Parsed> {
        parse_comment(&comment(&[line]), &tags(), github_origin)
    }

    #[test]
    fn parses_gh_ref() {
        let found = parse(&["TODO(gh#12): retry"]);
        assert_eq!(
            found,
            vec![(
                1,
                4,
                "TODO".to_string(),
                Some(Ref::Gh {
                    repo: None,
                    number: 12
                }),
                None,
                "retry".to_string()
            )]
        );
    }

    #[test]
    fn parses_cross_repo() {
        let found = parse(&["TODO(gh:owner/repo#5): x"]);
        assert_eq!(
            found[0].3,
            Some(Ref::Gh {
                repo: Some("owner/repo".into()),
                number: 5
            })
        );
    }

    #[test]
    fn hash_ref_depends_on_origin() {
        assert_eq!(
            parse_origin("TODO(#7): x", true)[0].3,
            Some(Ref::Gh {
                repo: None,
                number: 7
            })
        );
        assert_eq!(
            parse_origin("TODO(#7): x", false)[0].3,
            Some(Ref::Unknown {
                prefix: "#".into(),
                raw: "#7".into()
            })
        );
    }

    #[test]
    fn owner_is_not_ref() {
        let found = parse(&["TODO(alice): x"]);
        assert_eq!(found[0].3, None);
        assert_eq!(found[0].4.as_deref(), Some("alice"));
        assert_eq!(found[0].5, "x");
    }

    #[test]
    fn extra_after_comma() {
        let found = parse(&["FIXME(gh#3, p1): y"]);
        assert_eq!(found[0].2, "FIXME");
        assert_eq!(
            found[0].3,
            Some(Ref::Gh {
                repo: None,
                number: 3
            })
        );
        assert_eq!(found[0].4.as_deref(), Some("p1"));
        assert_eq!(found[0].5, "y");
        // A non-REF first part keeps the whole inner text as extra.
        let found = parse(&["TODO(alice, gh#3): z"]);
        assert_eq!(found[0].3, None);
        assert_eq!(found[0].4.as_deref(), Some("alice, gh#3"));
    }

    #[test]
    fn malformed_ref() {
        assert_eq!(
            parse(&["TODO(gh#abc): x"])[0].3,
            Some(Ref::Malformed("gh#abc".into()))
        );
        assert_eq!(
            parse(&["TODO(gh:o#5): x"])[0].3,
            Some(Ref::Malformed("gh:o#5".into()))
        );
        assert_eq!(
            parse(&["TODO(#x): x"])[0].3,
            Some(Ref::Malformed("#x".into()))
        );
    }

    #[test]
    fn relay_is_unknown() {
        assert_eq!(
            parse(&["TODO(relay:abc): x"])[0].3,
            Some(Ref::Unknown {
                prefix: "relay".into(),
                raw: "relay:abc".into()
            })
        );
        assert_eq!(
            parse(&["TODO(jira:K-1): x"])[0].3,
            Some(Ref::Unknown {
                prefix: "jira".into(),
                raw: "jira:K-1".into()
            })
        );
    }

    #[test]
    fn continuation_joins() {
        let found = parse(&["TODO: first", "second"]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].5, "first second");
    }

    #[test]
    fn stops_at_next_tag() {
        let found = parse(&["TODO: one", "more", "FIXME: two"]);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].5, "one more");
        assert_eq!(
            (found[1].0, found[1].2.as_str(), found[1].5.as_str()),
            (3, "FIXME", "two")
        );
    }

    #[test]
    fn empty_line_ends_continuation() {
        let found = parse(&["TODO: one", "", "stray"]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].5, "one");
    }

    #[test]
    fn ignore_escape() {
        assert!(parse(&["TODO: x td2do:ignore"]).is_empty());
        assert!(parse(&["TODO: x", "td2do:ignore"])[0].5 == "x");
        // An ignored TODO's continuation does not become anything.
        assert!(parse(&["TODO: x td2do:ignore", "more"]).is_empty());
    }

    #[test]
    fn lowercase_todo_is_not_a_tag() {
        assert!(parse(&["todo: x"]).is_empty());
        assert!(parse(&["Todo: x"]).is_empty());
    }

    #[test]
    fn todolist_is_not_a_tag() {
        for t in ["todoList", "TODOS", "TODO-x", "TODOLIST: x"] {
            assert!(parse(&[t]).is_empty(), "{t}");
        }
    }

    #[test]
    fn tag_boundaries() {
        assert_eq!(parse(&["TODO"])[0].5, "");
        assert_eq!(parse(&["TODO:x"])[0].5, "x");
        assert_eq!(parse(&["TODO fix this"])[0].5, "fix this");
        assert!(parse(&["see TODO: later"]).is_empty());
    }

    #[test]
    fn col_is_the_tag_column() {
        let c = Comment {
            start_line: 4,
            lines: vec![CommentLine {
                line: 4,
                col: 5,
                text: "   TODO x".into(),
            }],
        };
        let found = parse_comment(&c, &tags(), true);
        assert_eq!((found[0].0, found[0].1), (4, 8));
    }

    #[test]
    fn crlf_same_as_lf() {
        let lang = for_path(Path::new("x.rs")).unwrap();
        let lf = "  // TODO(gh#1): first\n  // second\n";
        let crlf = lf.replace('\n', "\r\n");
        let run = |src: &str| -> Vec<Parsed> {
            comments(lang, src)
                .iter()
                .flat_map(|c| parse_comment(c, &tags(), true))
                .collect()
        };
        let got = run(&crlf);
        assert_eq!(got, run(lf));
        assert_eq!(got.len(), 1);
        assert_eq!((got[0].0, got[0].1), (1, 6));
        assert_eq!(got[0].5, "first second");
    }

    #[test]
    fn fingerprint_stable() {
        let a = fingerprint("o/r", "TODO", "Retry  on 429");
        assert_eq!(a, fingerprint("o/r", "TODO", "retry on 429"));
        assert_eq!(a.len(), 16);
        assert!(
            a.chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        );
        assert_ne!(a, fingerprint("o/r", "FIXME", "retry on 429"));
        assert_ne!(a, fingerprint("", "TODO", "retry on 429"));
    }

    #[test]
    fn fingerprint_ignores_ref() {
        let plain = fingerprint("o/r", "TODO", "retry on 429");
        assert_eq!(plain, fingerprint("o/r", "TODO", "gh#1 retry on 429"));
        assert_eq!(
            plain,
            fingerprint("o/r", "TODO", "(gh:o/r#12) retry on 429")
        );
        assert_eq!(plain, fingerprint("o/r", "TODO", "// retry on 429 */"));
    }

    #[test]
    fn normalize_collapses() {
        assert_eq!(normalize("  Retry \t on\n429 "), "retry on 429");
        assert_eq!(normalize("a - b"), "a - b");
    }
}
