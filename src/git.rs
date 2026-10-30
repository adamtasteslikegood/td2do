//! The few git facts td2do needs: the repo root, the GitHub slug of `origin`,
//! the list of files, and which lines a branch adds.

use anyhow::{Context, Result, bail};
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// The top of the work tree containing `dir`, or `None` outside a repo.
pub fn root(dir: &Path) -> Option<PathBuf> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let top = String::from_utf8(out.stdout).ok()?;
    let top = top.trim_end_matches(['\n', '\r']);
    (!top.is_empty()).then(|| PathBuf::from(top))
}

/// `owner/repo` when `origin` is on GitHub; `None` for any other host or no `origin`.
pub fn origin_slug(root: &Path) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["remote", "get-url", "origin"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    parse_slug(String::from_utf8(out.stdout).ok()?.trim())
}

fn parse_slug(url: &str) -> Option<String> {
    let path = [
        "git@github.com:",
        "https://github.com/",
        "ssh://git@github.com/",
        "git://github.com/",
    ]
    .iter()
    .find_map(|p| url.strip_prefix(p))?;
    let path = path.trim_end_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    let (owner, repo) = path.split_once('/')?;
    let ok = |s: &str| !s.is_empty() && !s.contains('/') && !s.contains(char::is_whitespace);
    (ok(owner) && ok(repo)).then(|| format!("{owner}/{repo}"))
}

/// Tracked files plus untracked files that are not ignored, relative to `root`,
/// sorted and without duplicates. Names that are not UTF-8 are left out.
pub fn files(root: &Path) -> Result<Vec<PathBuf>> {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args([
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ])
        .output()
        .context("running git ls-files")?;
    if !out.status.success() {
        bail!(
            "git ls-files failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    let mut files: Vec<PathBuf> = out
        .stdout
        .split(|b| *b == 0)
        .filter(|name| !name.is_empty())
        .filter_map(|name| std::str::from_utf8(name).ok())
        .map(PathBuf::from)
        .collect();
    files.sort();
    files.dedup();
    Ok(files)
}

/// Runs git in `root` and returns its output, whatever the exit status.
fn git(root: &Path, args: &[&str]) -> Result<Output> {
    Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .with_context(|| format!("running git {}", args.first().unwrap_or(&"")))
}

/// Runs git in `root` and returns its stdout; a failure is an error.
fn git_ok(root: &Path, args: &[&str]) -> Result<Vec<u8>> {
    let out = git(root, args)?;
    if !out.status.success() {
        bail!(
            "git {} failed: {}",
            args.first().unwrap_or(&""),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(out.stdout)
}

/// The branch to compare with when none is configured: what `origin/HEAD`
/// points to (e.g. `origin/main`), else `main`.
pub fn default_base(root: &Path) -> String {
    git(
        root,
        &[
            "symbolic-ref",
            "--quiet",
            "--short",
            "refs/remotes/origin/HEAD",
        ],
    )
    .ok()
    .filter(|o| o.status.success())
    .and_then(|o| String::from_utf8(o.stdout).ok())
    .map(|s| s.trim().to_string())
    .filter(|s| !s.is_empty())
    .unwrap_or_else(|| "main".to_string())
}

#[derive(Debug)]
pub enum BaseError {
    /// The base does not resolve to a commit, or shares no history with HEAD
    /// (as in a shallow clone).
    Missing { base: String },
    /// git itself could not be run.
    Git(anyhow::Error),
}

impl std::fmt::Display for BaseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BaseError::Missing { base } => write!(
                f,
                "no merge base with `{base}`: the ref is missing or shares no history with HEAD. \
                 Fetch it with full history (`git fetch --unshallow`, or `fetch-depth: 0` \
                 for actions/checkout in CI), or pass --base"
            ),
            BaseError::Git(e) => write!(f, "{e:#}"),
        }
    }
}

impl std::error::Error for BaseError {}

/// The merge base of HEAD and `base`.
pub fn merge_base(root: &Path, base: &str) -> Result<String, BaseError> {
    let missing = || BaseError::Missing {
        base: base.to_string(),
    };
    let commit = format!("{base}^{{commit}}");
    let out = git(
        root,
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            "--end-of-options",
            &commit,
        ],
    )
    .map_err(BaseError::Git)?;
    if !out.status.success() {
        return Err(missing());
    }
    let sha = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let out = git(root, &["merge-base", "HEAD", &sha]).map_err(BaseError::Git)?;
    if !out.status.success() {
        return Err(missing());
    }
    let sha = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if sha.is_empty() {
        return Err(missing());
    }
    Ok(sha)
}

/// The lines added since `merge_base`, per repo-relative path. It compares the
/// merge base with the working tree, or with the index when `staged`. In
/// working-tree mode every line of an untracked, not-ignored file counts as
/// added (ruling R5).
pub fn added_lines(
    root: &Path,
    merge_base: &str,
    staged: bool,
) -> Result<HashMap<PathBuf, BTreeSet<usize>>> {
    // Fixed prefixes and no external diff or textconv, whatever the user's config (ruling R4).
    let mut args = vec![
        "-c",
        "core.quotePath=false",
        "diff",
        "-U0",
        "-M",
        "--no-color",
        "--no-ext-diff",
        "--no-textconv",
        "--src-prefix=a/",
        "--dst-prefix=b/",
    ];
    if staged {
        args.push("--cached");
    }
    args.extend([merge_base, "--"]);
    let mut added = parse_diff(&git_ok(root, &args)?);
    if !staged {
        let out = git_ok(root, &["ls-files", "-z", "--others", "--exclude-standard"])?;
        for name in out.split(|b| *b == 0).filter(|n| !n.is_empty()) {
            let Ok(name) = std::str::from_utf8(name) else {
                continue;
            };
            let Ok(bytes) = std::fs::read(root.join(name)) else {
                continue;
            };
            let count = bytes.iter().filter(|b| **b == b'\n').count() + 1;
            added.insert(PathBuf::from(name), (1..=count).collect());
        }
    }
    Ok(added)
}

/// The content of `path` in the index.
pub fn index_content(root: &Path, path: &Path) -> Result<Vec<u8>> {
    let spec = format!(":{}", path.to_string_lossy());
    git_ok(root, &["cat-file", "blob", &spec])
        .with_context(|| format!("reading {} from the index", path.display()))
}

/// Reads `git diff -U0` output: the `+++ b/<path>` header names the file and
/// each `@@ -a,b +c,d @@` header adds lines c..c+d. Content lines are skipped by
/// count, so a line that looks like a header is never taken for one.
fn parse_diff(diff: &[u8]) -> HashMap<PathBuf, BTreeSet<usize>> {
    let mut added: HashMap<PathBuf, BTreeSet<usize>> = HashMap::new();
    let mut current: Option<PathBuf> = None;
    // Content lines left in the current hunk.
    let mut pending = 0usize;
    for line in diff.split(|b| *b == b'\n') {
        if pending > 0 {
            match line.first() {
                Some(b'+' | b'-' | b' ') => {
                    pending -= 1;
                    continue;
                }
                // `\ No newline at end of file`
                Some(b'\\') => continue,
                _ => pending = 0,
            }
        }
        if let Some(rest) = line.strip_prefix(b"+++ ") {
            current = header_path(rest);
            if let Some(p) = &current {
                added.entry(p.clone()).or_default();
            }
        } else if let Some(rest) = line.strip_prefix(b"@@ ") {
            let Some((old, new)) = hunk_counts(rest) else {
                continue;
            };
            pending = old + new.1;
            if let Some(p) = &current {
                added
                    .entry(p.clone())
                    .or_default()
                    .extend(new.0..new.0 + new.1);
            }
        }
    }
    added
}

/// The path in a `+++` header, without its `b/` prefix; `None` for `/dev/null`
/// or a name that is not UTF-8.
fn header_path(rest: &[u8]) -> Option<PathBuf> {
    let name = if rest.first() == Some(&b'"') {
        unquote(rest)?
    } else {
        // git ends the header with a tab when the name holds a space.
        rest.strip_suffix(b"\t").unwrap_or(rest).to_vec()
    };
    let name = name.strip_prefix(b"b/")?;
    String::from_utf8(name.to_vec()).ok().map(PathBuf::from)
}

/// Undoes git's C-style quoting: `"a\tb\"c\\d\303\251"`.
fn unquote(quoted: &[u8]) -> Option<Vec<u8>> {
    let inner = quoted.strip_prefix(b"\"")?;
    let mut out = Vec::new();
    let mut i = 0;
    while i < inner.len() {
        match inner[i] {
            b'"' => return Some(out),
            b'\\' => {
                let c = *inner.get(i + 1)?;
                i += 2;
                out.push(match c {
                    b'a' => 0x07,
                    b'b' => 0x08,
                    b't' => b'\t',
                    b'n' => b'\n',
                    b'v' => 0x0b,
                    b'f' => 0x0c,
                    b'r' => b'\r',
                    b'0'..=b'7' => {
                        let digits = inner.get(i - 1..i + 2)?;
                        if !digits.iter().all(|d| (b'0'..=b'7').contains(d)) {
                            return None;
                        }
                        i += 2;
                        digits.iter().fold(0u32, |n, d| n * 8 + u32::from(d - b'0')) as u8
                    }
                    other => other,
                });
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    None
}

/// From `-a[,b] +c[,d] @@ …`: the old line count, and the new start and count.
fn hunk_counts(rest: &[u8]) -> Option<(usize, (usize, usize))> {
    let text = std::str::from_utf8(rest).ok()?;
    let mut parts = text.split(' ');
    let old = parts.next()?.strip_prefix('-')?;
    let new = parts.next()?.strip_prefix('+')?;
    let range = |s: &str| -> Option<(usize, usize)> {
        match s.split_once(',') {
            Some((start, count)) => Some((start.parse().ok()?, count.parse().ok()?)),
            None => Some((s.parse().ok()?, 1)),
        }
    };
    Some((range(old)?.1, range(new)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_forms() {
        for url in [
            "git@github.com:o/r.git",
            "git@github.com:o/r",
            "https://github.com/o/r.git",
            "https://github.com/o/r",
            "https://github.com/o/r/",
            "ssh://git@github.com/o/r.git",
        ] {
            assert_eq!(parse_slug(url).as_deref(), Some("o/r"), "{url}");
        }
    }

    #[test]
    fn other_hosts_have_no_slug() {
        for url in [
            "git@gitlab.com:o/r.git",
            "https://example.com/o/r.git",
            "https://github.com.evil.com/o/r",
            "https://github.com/o",
            "https://github.com/o/r/extra",
            "",
        ] {
            assert_eq!(parse_slug(url), None, "{url}");
        }
    }

    fn lines(v: &[usize]) -> BTreeSet<usize> {
        v.iter().copied().collect()
    }

    #[test]
    fn parses_hunks() {
        let diff = "diff --git a/x.rs b/x.rs\n--- a/x.rs\n+++ b/x.rs\n@@ -3,0 +4,2 @@\n+a\n+b\n@@ -10 +12 @@\n-old\n+new\n@@ -20,3 +22,0 @@\n-x\n-y\n-z\n";
        let got = parse_diff(diff.as_bytes());
        assert_eq!(got.len(), 1);
        assert_eq!(got[Path::new("x.rs")], lines(&[4, 5, 12]));
        let got = parse_diff(b"+++ b/y.rs\n@@ -1 +1 @@\n-a\n+b\n");
        assert_eq!(got[Path::new("y.rs")], lines(&[1]));
        let got = parse_diff(b"+++ b/z.rs\n@@ -1,2 +1,0 @@\n-a\n-b\n");
        assert!(got.get(Path::new("z.rs")).is_none_or(|s| s.is_empty()));
    }

    #[test]
    fn deleted_file_adds_nothing() {
        let got = parse_diff(b"--- a/gone.rs\n+++ /dev/null\n@@ -1 +0,0 @@\n-x\n");
        assert!(got.values().all(|s| s.is_empty()), "{got:?}");
    }

    #[test]
    fn unquotes_paths() {
        let got = parse_diff(
            b"+++ \"b/q\\\"uote\\\\.rs\"\n@@ -0,0 +1 @@\n+x\n+++ \"b/tab\\there.rs\"\n@@ -0,0 +2 @@\n+x\n+++ \"b/caf\\303\\251.rs\"\n@@ -0,0 +3 @@\n+x\n+++ b/dir with space/a.rs\t\n@@ -0,0 +4 @@\n+x\n",
        );
        assert_eq!(got[Path::new("q\"uote\\.rs")], lines(&[1]));
        assert_eq!(got[Path::new("tab\there.rs")], lines(&[2]));
        assert_eq!(got[Path::new("café.rs")], lines(&[3]));
        assert_eq!(got[Path::new("dir with space/a.rs")], lines(&[4]));
    }

    #[test]
    fn hunk_header_inside_content_is_not_a_header() {
        // An added line that itself reads `++ b/...` shows as `+++ b/...` only in
        // the file header position; content lines follow a hunk header and are
        // counted, not parsed.
        let got = parse_diff(b"+++ b/a.rs\n@@ -0,0 +1,2 @@\n+++ b/evil.rs\n+@@ -0,0 +9 @@\n");
        assert_eq!(got.len(), 1, "{got:?}");
        assert_eq!(got[Path::new("a.rs")], lines(&[1, 2]));
    }
}
