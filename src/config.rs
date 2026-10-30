//! `.td2do.toml` loading. Every key is optional; unknown keys are errors.

use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::Path;

pub const CONFIG_FILE: &str = ".td2do.toml";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Fail,
    Warn,
    Off,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CheckConfig {
    pub base: Option<String>,
    pub duplicate_text: Level,
}

impl Default for CheckConfig {
    fn default() -> Self {
        Self {
            base: None,
            duplicate_text: Level::Warn,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub tags: Vec<String>,
    pub label: String,
    pub ignore: Vec<String>,
    pub check: CheckConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            tags: ["TODO", "FIXME", "HACK", "XXX", "BUG"]
                .map(String::from)
                .to_vec(),
            label: "todo".to_string(),
            ignore: Vec::new(),
            check: CheckConfig::default(),
        }
    }
}

/// Reads `<root>/.td2do.toml`. A missing file gives the defaults; an unknown
/// key or a bad value is an error that names the file and the key.
pub fn load(root: &Path) -> Result<Config> {
    let path = root.join(CONFIG_FILE);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Config::default()),
        Err(e) => return Err(e).with_context(|| format!("reading {CONFIG_FILE}")),
    };
    toml::from_str(&text).with_context(|| format!("invalid {CONFIG_FILE}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, text: &str) {
        std::fs::write(dir.join(CONFIG_FILE), text).unwrap();
    }

    #[test]
    fn defaults_when_missing() {
        let dir = tempfile::tempdir().unwrap();
        let c = load(dir.path()).unwrap();
        assert_eq!(c.tags, ["TODO", "FIXME", "HACK", "XXX", "BUG"]);
        assert_eq!(c.label, "todo");
        assert_eq!(c.check.duplicate_text, Level::Warn);
        assert_eq!(c.check.base, None);
    }

    #[test]
    fn reads_file() {
        let dir = tempfile::tempdir().unwrap();
        write(
            dir.path(),
            r#"tags = ["TODO", "FIXME", "HACK", "XXX", "BUG"]
label = "todo"
ignore = ["docs/**", "**/*.snap"]

[check]
base = "main"
duplicate_text = "warn"   # or "fail" / "off"
"#,
        );
        let c = load(dir.path()).unwrap();
        assert_eq!(c.ignore, ["docs/**", "**/*.snap"]);
        assert_eq!(c.check.base.as_deref(), Some("main"));
        assert_eq!(c.check.duplicate_text, Level::Warn);
    }

    #[test]
    fn unknown_key_is_error() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "tagz = []\n");
        let err = format!("{:#}", load(dir.path()).unwrap_err());
        assert!(err.contains("tagz"), "{err}");
    }

    #[test]
    fn bad_level_is_error() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "[check]\nduplicate_text = \"loud\"\n");
        let err = format!("{:#}", load(dir.path()).unwrap_err());
        assert!(err.contains("loud"), "{err}");
    }
}
