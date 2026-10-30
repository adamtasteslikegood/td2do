//! The few git facts td2do needs: the repo root, the GitHub slug of `origin`
//! and the list of files.

use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};
use std::process::Command;

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
}
