//! Shared test fixture: a throwaway git repository.
#![allow(dead_code)] // each test crate uses a different part

use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

pub struct Fixture {
    pub dir: TempDir,
}

impl Fixture {
    /// A fresh repository on `main` with a committer set and `origin` at
    /// `https://github.com/o/r.git`.
    pub fn new() -> Self {
        let f = Fixture {
            dir: tempfile::tempdir().unwrap(),
        };
        f.git(&["init", "-q", "-b", "main"]);
        f.git(&["config", "user.name", "Test"]);
        f.git(&["config", "user.email", "test@example.com"]);
        f.git(&["config", "commit.gpgsign", "false"]);
        f.git(&["remote", "add", "origin", "https://github.com/o/r.git"]);
        f
    }

    pub fn path(&self) -> &Path {
        self.dir.path()
    }

    /// Writes `contents` to `path` (relative to the repo), making directories.
    pub fn write(&self, path: &str, contents: impl AsRef<[u8]>) {
        let full = self.dir.path().join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, contents).unwrap();
    }

    /// Stages everything and commits it.
    pub fn commit(&self, msg: &str) {
        self.git(&["add", "-A"]);
        self.git(&["commit", "-q", "--allow-empty", "-m", msg]);
    }

    /// Creates a branch at HEAD without switching to it.
    pub fn branch(&self, name: &str) {
        self.git(&["branch", name]);
    }

    pub fn checkout(&self, name: &str) {
        self.git(&["checkout", "-q", name]);
    }

    /// Runs git in the repo and returns its stdout; panics on failure.
    pub fn git(&self, args: &[&str]) -> String {
        let out = Command::new("git")
            .args(args)
            .current_dir(self.dir.path())
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    }
}
