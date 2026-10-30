//! `td2do init`: writes the config, the pre-commit hook and the CI step,
//! showing each in full and asking first.

use crate::{Exit, git, output};
use anyhow::{Result, bail};
use serde_json::json;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

const HOOK: &str = "#!/bin/sh\nexec td2do check --staged\n";
const HOOK_LINE: &str = "exec td2do check --staged";

/// Pinned to the commit the repo's own CI uses.
const WORKFLOW: &str = "\
name: td2do

on:
  pull_request:

permissions:
  contents: read

jobs:
  check:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@11d5960a326750d5838078e36cf38b85af677262 # v4
        with:
          fetch-depth: 0
      # This install line changes once td2do is published.
      # The repo is private, so a token may be needed.
      - run: cargo install --locked --git https://github.com/adamtasteslikegood/td2do
      - run: td2do check --base origin/${{ github.base_ref }}
";

const AGENT_RULE_NOTE: &str = "agent rule: available in phase 2 (needs td2do find and add)";

fn config_text(base: &str) -> String {
    format!(
        "\
tags = [\"TODO\", \"FIXME\", \"HACK\", \"XXX\", \"BUG\"]
label = \"todo\"
ignore = [\"docs/**\", \"**/*.snap\"]

[check]
base = \"{base}\"
duplicate_text = \"warn\"   # or \"fail\" / \"off\"
"
    )
}

/// Where init talks: stdout normally, stderr when stdout carries JSON.
struct Talk {
    json: bool,
}

impl Talk {
    fn say(&self, text: &str) -> Result<()> {
        if self.json {
            let mut err = std::io::stderr().lock();
            err.write_all(text.as_bytes())?;
            err.flush()?;
        } else {
            output::print(text)?;
        }
        Ok(())
    }
}

/// Asks `[y/N]`; end of input counts as no.
fn confirm(talk: &Talk, input: &mut impl BufRead, question: &str) -> Result<bool> {
    talk.say(&format!("{question} [y/N] "))?;
    let mut line = String::new();
    let n = input.read_line(&mut line)?;
    let yes = n > 0 && matches!(line.trim().to_ascii_lowercase().as_str(), "y" | "yes");
    if n == 0 || !line.ends_with('\n') {
        talk.say("\n")?;
    }
    Ok(yes)
}

#[derive(Default)]
struct Outcome {
    written: Vec<String>,
    skipped: Vec<(String, &'static str)>,
}

/// The path to show: relative to the repo root when it lies inside it.
fn shown(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// One file init may create.
struct NewFile<'a> {
    label: &'a str,
    path: PathBuf,
    shown: String,
    contents: &'a str,
    executable: bool,
}

/// What init is doing: where it talks, where answers come from, what it did.
struct Session<I: BufRead> {
    talk: Talk,
    input: I,
    yes: bool,
    out: Outcome,
}

impl<I: BufRead> Session<I> {
    /// Notes a file that is already there; nothing is written.
    fn exists(&mut self, shown: String) -> Result<()> {
        self.talk.say(&format!("{shown}: exists, skipped\n"))?;
        self.out.skipped.push((shown, "exists"));
        Ok(())
    }

    /// Offers one new file: shows it, asks unless `--yes`, writes it.
    fn offer(&mut self, file: NewFile) -> Result<()> {
        let NewFile {
            label,
            path,
            shown,
            contents,
            executable,
        } = file;
        self.talk
            .say(&format!("--- {shown} ---\n{contents}---\n"))?;
        if !self.yes
            && !confirm(
                &self.talk,
                &mut self.input,
                &format!("Write {label} to {shown}?"),
            )?
        {
            self.out.skipped.push((shown, "declined"));
            return Ok(());
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        // `create_new` so a file that appeared since the check is never overwritten.
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(mut f) => f.write_all(contents.as_bytes())?,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => return self.exists(shown),
            Err(e) => return Err(e.into()),
        }
        #[cfg(unix)]
        if executable {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))?;
        }
        #[cfg(not(unix))]
        let _ = executable;
        self.talk.say(&format!("wrote {shown}\n"))?;
        self.out.written.push(shown);
        Ok(())
    }
}

fn hook_path(root: &Path) -> PathBuf {
    git::hook_path(root).unwrap_or_else(|| root.join(".git/hooks/pre-commit"))
}

pub fn run(dir: &Path, yes: bool, json: bool) -> Result<Exit> {
    let Some(root) = git::root(dir) else {
        bail!("not a git repository: {}", dir.display());
    };
    let mut s = Session {
        talk: Talk { json },
        input: std::io::stdin().lock(),
        yes,
        out: Outcome::default(),
    };

    // 1. The config.
    let base = git::default_base(&root);
    let base = base.strip_prefix("origin/").unwrap_or(&base);
    let config = root.join(".td2do.toml");
    if config.exists() {
        s.exists(".td2do.toml".into())?;
    } else {
        s.offer(NewFile {
            label: "the config",
            path: config,
            shown: ".td2do.toml".into(),
            contents: &config_text(base),
            executable: false,
        })?;
    }

    // 2. The pre-commit hook; an existing one is never touched.
    let hook = hook_path(&root);
    let hook_shown = shown(&root, &hook);
    if hook.exists() {
        s.talk.say(&format!(
            "{hook_shown}: exists, skipped\nadd this line to your existing hook:\n{HOOK_LINE}\n"
        ))?;
        s.out.skipped.push((hook_shown, "exists"));
    } else {
        s.offer(NewFile {
            label: "the pre-commit hook",
            path: hook,
            shown: hook_shown,
            contents: HOOK,
            executable: true,
        })?;
    }

    // 3. The CI step.
    let workflow = root.join(".github/workflows/td2do.yml");
    if workflow.exists() {
        s.exists(".github/workflows/td2do.yml".into())?;
    } else {
        s.offer(NewFile {
            label: "the CI step",
            path: workflow,
            shown: ".github/workflows/td2do.yml".into(),
            contents: WORKFLOW,
            executable: false,
        })?;
    }

    // 4. The agent rule needs `find` and `add`, which arrive in phase 2.
    s.talk.say(&format!("{AGENT_RULE_NOTE}\n"))?;

    if json {
        let skipped: Vec<_> = s
            .out
            .skipped
            .iter()
            .map(|(path, reason)| json!({ "path": path, "reason": reason }))
            .collect();
        let doc = json!({ "schema": 1, "written": s.out.written, "skipped": skipped });
        output::print(&format!("{}\n", serde_json::to_string_pretty(&doc)?))?;
    }
    Ok(Exit::Ok)
}
