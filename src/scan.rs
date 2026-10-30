//! Scans a repo for TODO comments.

use crate::config::Config;
use crate::todo::{self, Todo};
use crate::{git, lang, lexer};
use anyhow::{Context, Result};
use globset::{Glob, GlobSet, GlobSetBuilder};
use grep_regex::RegexMatcher;
use grep_searcher::{Searcher, sinks};
use ignore::{WalkBuilder, WalkState};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Files larger than this are skipped.
const MAX_BYTES: u64 = 2 * 1024 * 1024;
/// A NUL in this many leading bytes marks a file as binary.
const BINARY_PROBE: usize = 8 * 1024;
/// Directories whose contents are never ours.
const SKIPPED_DIRS: [&str; 3] = ["vendor", "node_modules", "third_party"];
/// Lockfiles whose names do not end in `.lock`.
const LOCKFILES: [&str; 3] = ["package-lock.json", "npm-shrinkwrap.json", "go.sum"];
/// Lockfiles such as `Cargo.lock` or `Gemfile.lock`.
const LOCK_SUFFIX: &str = ".lock";

pub struct Repo {
    pub root: PathBuf,
    /// `owner/repo` when `origin` is on GitHub.
    pub slug: Option<String>,
    pub config: Config,
}

pub struct ScanResult {
    /// Sorted by (path, line, col).
    pub todos: Vec<Todo>,
    /// Files with no entry in the language table.
    pub skipped_unknown_lang: usize,
}

/// Opens the repo containing `dir` (or `dir` itself outside git) and reads its config.
pub fn open(dir: &Path) -> Result<Repo> {
    let root = git::root(dir).unwrap_or_else(|| dir.to_path_buf());
    let config = crate::config::load(&root)?;
    let slug = git::origin_slug(&root);
    Ok(Repo { root, slug, config })
}

pub fn scan(repo: &Repo) -> Result<ScanResult> {
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
    scan_with_threads(repo, threads)
}

/// Like [`scan`] with a fixed number of worker threads. The result does not depend on it.
pub fn scan_with_threads(repo: &Repo, threads: usize) -> Result<ScanResult> {
    let threads = threads.max(1);
    let ignore = ignore_set(&repo.config.ignore)?;
    let mut paths: Vec<PathBuf> = match git::root(&repo.root) {
        Some(_) => git::files(&repo.root)?,
        None => walk(&repo.root, threads)?,
    };
    paths.retain(|p| !skipped_by_path(p, &ignore));
    let Some(matcher) = tag_matcher(&repo.config.tags)? else {
        return Ok(ScanResult {
            todos: Vec::new(),
            skipped_unknown_lang: 0,
        });
    };

    let chunk = paths.len().div_ceil(threads).max(1);
    let outcomes: Vec<Outcome> = std::thread::scope(|s| {
        let handles: Vec<_> = paths
            .chunks(chunk)
            .map(|files| {
                let matcher = &matcher;
                s.spawn(move || {
                    let mut out = Outcome::default();
                    for rel in files {
                        scan_file(repo, matcher, rel, &mut out);
                    }
                    out
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().expect("scan thread panicked"))
            .collect()
    });

    let mut result = ScanResult {
        todos: Vec::new(),
        skipped_unknown_lang: 0,
    };
    for o in outcomes {
        result.todos.extend(o.todos);
        result.skipped_unknown_lang += o.unknown_lang;
    }
    result
        .todos
        .sort_by(|a, b| (&a.path, a.line, a.col).cmp(&(&b.path, b.line, b.col)));
    Ok(result)
}

/// The TODOs in `src`, taken as the content of `path` (relative to the repo root).
/// Used for content that is not on disk, such as the index. It applies the same
/// path, size and binary rules as a full scan (ruling R8).
pub fn scan_source(repo: &Repo, path: &Path, src: &[u8]) -> Vec<Todo> {
    let ignore = ignore_set(&repo.config.ignore).unwrap_or_else(|_| GlobSet::empty());
    if skipped_by_path(path, &ignore) || src.len() as u64 > MAX_BYTES || is_binary(src) {
        return Vec::new();
    }
    let Some(language) = lang::for_path(path) else {
        return Vec::new();
    };
    parse_source(repo, language, path, src)
}

fn is_binary(bytes: &[u8]) -> bool {
    bytes.iter().take(BINARY_PROBE).any(|b| *b == 0)
}

#[derive(Default)]
struct Outcome {
    todos: Vec<Todo>,
    unknown_lang: usize,
}

fn parse_source(repo: &Repo, language: &lang::Lang, path: &Path, src: &[u8]) -> Vec<Todo> {
    let decoded = String::from_utf8_lossy(src);
    let text = decoded.strip_prefix('\u{feff}').unwrap_or(&decoded);
    let slug = repo.slug.as_deref().unwrap_or("");
    let path = slash_path(path);
    let mut todos = Vec::new();
    for comment in lexer::comments(language, text) {
        for (line, col, tag, reference, extra, body, last_line) in
            todo::parse_comment(&comment, &repo.config.tags, repo.slug.is_some())
        {
            let fingerprint = todo::fingerprint(slug, &tag, &body);
            todos.push(Todo {
                path: path.clone(),
                line,
                col,
                tag,
                reference,
                extra,
                text: body,
                fingerprint,
                last_line,
            });
        }
    }
    todos
}

fn scan_file(repo: &Repo, matcher: &RegexMatcher, rel: &Path, out: &mut Outcome) {
    let Some(language) = lang::for_path(rel) else {
        out.unknown_lang += 1;
        return;
    };
    let full = repo.root.join(rel);
    // Anything unreadable (deleted but still in the index, a submodule, a
    // symlink, a permission error) is not a file we can scan.
    let Ok(meta) = std::fs::symlink_metadata(&full) else {
        return;
    };
    if !meta.is_file() || meta.len() > MAX_BYTES {
        return;
    }
    let Ok(bytes) = std::fs::read(&full) else {
        return;
    };
    if is_binary(&bytes) || !has_tag(matcher, &bytes) {
        return;
    }
    out.todos.extend(parse_source(repo, language, rel, &bytes));
}

/// The cheap pre-filter: does any tag appear in the bytes at all?
fn has_tag(matcher: &RegexMatcher, bytes: &[u8]) -> bool {
    let mut found = false;
    let sink = sinks::Bytes(|_, _| {
        found = true;
        Ok(false)
    });
    Searcher::new().search_slice(matcher, bytes, sink).is_ok() && found
}

fn tag_matcher(tags: &[String]) -> Result<Option<RegexMatcher>> {
    if tags.is_empty() {
        return Ok(None);
    }
    // No word boundaries: the parser decides what may follow a tag, and this
    // pre-filter must accept everything the parser would.
    let alternatives: Vec<String> = tags.iter().map(|t| regex::escape(t)).collect();
    let pattern = format!("(?:{})", alternatives.join("|"));
    Ok(Some(
        RegexMatcher::new(&pattern).context("building the tag matcher")?,
    ))
}

fn ignore_set(patterns: &[String]) -> Result<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    for p in patterns {
        builder.add(Glob::new(p).with_context(|| format!("invalid glob {p:?} in ignore"))?);
    }
    builder.build().context("building the ignore globs")
}

fn skipped_by_path(rel: &Path, ignore: &GlobSet) -> bool {
    let Some(name) = rel.file_name().and_then(|n| n.to_str()) else {
        return true;
    };
    let in_skipped_dir = rel.parent().is_some_and(|dir| {
        dir.components()
            .any(|c| SKIPPED_DIRS.iter().any(|d| c.as_os_str() == *d))
    });
    in_skipped_dir
        || name.ends_with(LOCK_SUFFIX)
        || LOCKFILES.contains(&name)
        || name.contains(".min.")
        || ignore.is_match(slash_path(rel))
}

/// `a/b/c.rs` with `/` separators on every platform.
fn slash_path(path: &Path) -> PathBuf {
    let parts: Vec<_> = path
        .components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect();
    PathBuf::from(parts.join("/"))
}

/// Files below `root` outside a git repo, relative to it. Honours `.gitignore` files.
fn walk(root: &Path, threads: usize) -> Result<Vec<PathBuf>> {
    let found = Mutex::new(Vec::new());
    WalkBuilder::new(root)
        .hidden(false)
        .require_git(false)
        .threads(threads)
        .filter_entry(|e| e.file_name() != ".git")
        .build_parallel()
        .run(|| {
            Box::new(|entry| {
                if let Ok(entry) = entry
                    && entry.file_type().is_some_and(|t| t.is_file())
                    && let Ok(rel) = entry.path().strip_prefix(root)
                {
                    found.lock().unwrap().push(rel.to_path_buf());
                }
                WalkState::Continue
            })
        });
    let mut files = found.into_inner().unwrap();
    files.sort();
    Ok(files)
}

#[cfg(test)]
#[path = "../tests/support/mod.rs"]
mod support;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::todo::Ref;

    use super::support::Fixture;

    fn scan_dir(dir: &Path) -> ScanResult {
        scan(&open(dir).unwrap()).unwrap()
    }

    fn places(r: &ScanResult) -> Vec<(String, usize)> {
        r.todos
            .iter()
            .map(|t| (t.path.to_string_lossy().into_owned(), t.line))
            .collect()
    }

    #[test]
    fn finds_todos_sorted() {
        let f = Fixture::new();
        f.write("z.rs", "// TODO: z\n");
        f.write("a/b.rs", "fn f() {}\n\n// FIXME(gh#1): b2\n// TODO: b1\n");
        f.write("a/a.py", "# HACK: a\n");
        let r = scan_dir(f.path());
        assert_eq!(
            places(&r),
            [
                ("a/a.py".to_string(), 1),
                ("a/b.rs".to_string(), 3),
                ("a/b.rs".to_string(), 4),
                ("z.rs".to_string(), 1)
            ]
        );
        let t = &r.todos[1];
        assert_eq!(t.tag, "FIXME");
        assert_eq!(
            t.reference,
            Some(Ref::Gh {
                repo: None,
                number: 1
            })
        );
        assert_eq!(t.text, "b2");
        assert_eq!(t.fingerprint.len(), 16);
    }

    #[test]
    fn respects_gitignore() {
        let f = Fixture::new();
        f.write(".gitignore", "ignored.rs\n");
        f.write("ignored.rs", "// TODO: no\n");
        f.write("kept.rs", "// TODO: yes\n");
        assert_eq!(places(&scan_dir(f.path())), [("kept.rs".to_string(), 1)]);
    }

    #[test]
    fn skips_binary() {
        let f = Fixture::new();
        f.write("bin.rs", b"// TODO: x\n\0\x01\x02");
        assert!(scan_dir(f.path()).todos.is_empty());
    }

    #[test]
    fn skips_large() {
        let f = Fixture::new();
        let mut big = b"// TODO: x\n".to_vec();
        big.resize(2 * 1024 * 1024 + 1, b' ');
        f.write("big.rs", &big);
        let mut ok = b"// TODO: x\n".to_vec();
        ok.resize(2 * 1024 * 1024, b' ');
        f.write("ok.rs", &ok);
        assert_eq!(places(&scan_dir(f.path())), [("ok.rs".to_string(), 1)]);
    }

    #[test]
    fn skips_lockfiles_and_min() {
        let f = Fixture::new();
        f.write("Cargo.lock", "# TODO: x\n");
        f.write("package-lock.json", "// TODO: x\n");
        f.write("a.min.js", "// TODO: x\n");
        f.write("a.js", "// TODO: y\n");
        let r = scan_dir(f.path());
        assert_eq!(places(&r), [("a.js".to_string(), 1)]);
        assert_eq!(r.skipped_unknown_lang, 0);
    }

    #[test]
    fn skips_vendor() {
        let f = Fixture::new();
        f.write("vendor/x.go", "// TODO: x\n");
        f.write("a/node_modules/y.js", "// TODO: x\n");
        f.write("third_party/z.c", "// TODO: x\n");
        f.write("src/w.go", "// TODO: w\n");
        assert_eq!(places(&scan_dir(f.path())), [("src/w.go".to_string(), 1)]);
    }

    #[test]
    fn applies_config_ignore() {
        let f = Fixture::new();
        f.write(".td2do.toml", "ignore = [\"docs/**\"]\n");
        f.write("docs/a.md", "<!-- TODO: x -->\n");
        f.write("src/a.rs", "// TODO: y\n");
        assert_eq!(places(&scan_dir(f.path())), [("src/a.rs".to_string(), 1)]);
    }

    #[test]
    fn bad_ignore_glob_is_error() {
        let f = Fixture::new();
        f.write(".td2do.toml", "ignore = [\"[\"]\n");
        let repo = open(f.path()).unwrap();
        assert!(scan(&repo).is_err());
    }

    #[test]
    fn counts_unknown_language() {
        let f = Fixture::new();
        f.write("notes.xyz", "TODO: x\n");
        f.write("a.rs", "// TODO: y\n");
        let r = scan_dir(f.path());
        assert_eq!(r.skipped_unknown_lang, 1);
        assert_eq!(r.todos.len(), 1);
    }

    #[test]
    fn non_utf8_is_lossy() {
        let f = Fixture::new();
        f.write("latin.c", b"// TODO: caf\xe9\n");
        let r = scan_dir(f.path());
        assert_eq!(r.todos.len(), 1);
        assert_eq!(r.todos[0].text, "caf\u{fffd}");
    }

    #[test]
    fn outside_git_walks() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("sub")).unwrap();
        std::fs::write(dir.path().join("sub/a.rs"), "// TODO: x\n").unwrap();
        std::fs::write(dir.path().join("b.rs"), "// FIXME: y\n").unwrap();
        std::fs::write(dir.path().join(".gitignore"), "c.rs\n").unwrap();
        std::fs::write(dir.path().join("c.rs"), "// TODO: ignored\n").unwrap();
        let r = scan_dir(dir.path());
        assert_eq!(
            places(&r),
            [("b.rs".to_string(), 1), ("sub/a.rs".to_string(), 1)]
        );
        assert_eq!(
            r.todos[0].fingerprint,
            crate::todo::fingerprint("", "FIXME", "y")
        );
    }

    #[test]
    fn origin_slug_feeds_fingerprint_and_bare_refs() {
        let f = Fixture::new();
        f.write("a.rs", "// TODO(#7): x\n");
        let r = scan_dir(f.path());
        assert_eq!(
            r.todos[0].reference,
            Some(Ref::Gh {
                repo: None,
                number: 7
            })
        );
        assert_eq!(
            r.todos[0].fingerprint,
            crate::todo::fingerprint("o/r", "TODO", "x")
        );
    }

    #[test]
    fn same_result_any_threads() {
        let f = Fixture::new();
        for i in 0..25 {
            f.write(
                &format!("d{}/f{i}.rs", i % 4),
                format!("// TODO: t{i}\nfn f() {{}}\n// FIXME(gh#{i}): u{i}\n"),
            );
        }
        f.write("n.xyz", "x\n");
        let repo = open(f.path()).unwrap();
        let base = scan_with_threads(&repo, 1).unwrap();
        assert_eq!(base.todos.len(), 50);
        let mut runner = proptest::test_runner::TestRunner::default();
        runner
            .run(&(1usize..8), |threads| {
                let r = scan_with_threads(&repo, threads).unwrap();
                proptest::prop_assert_eq!(&r.todos, &base.todos);
                proptest::prop_assert_eq!(r.skipped_unknown_lang, base.skipped_unknown_lang);
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn strips_bom() {
        let f = Fixture::new();
        f.write("a.yml", "\u{feff}# TODO(gh#1): x\n");
        let r = scan_dir(f.path());
        assert_eq!(r.todos.len(), 1);
        assert_eq!((r.todos[0].line, r.todos[0].col), (1, 3));
    }

    #[test]
    fn javadoc_block() {
        let f = Fixture::new();
        f.write("A.java", "/**\n * TODO(gh#2): a\n */\nclass A {}\n");
        let r = scan_dir(f.path());
        assert_eq!(r.todos.len(), 1);
        let t = &r.todos[0];
        assert_eq!((t.tag.as_str(), t.text.as_str()), ("TODO", "a"));
        assert_eq!(
            t.reference,
            Some(Ref::Gh {
                repo: None,
                number: 2
            })
        );
    }

    #[test]
    fn rust_doc_comment() {
        let f = Fixture::new();
        f.write("a.rs", "/// TODO(gh#3): b\nfn f() {}\n");
        let r = scan_dir(f.path());
        assert_eq!(r.todos.len(), 1);
        assert_eq!(r.todos[0].text, "b");
    }

    #[test]
    fn html_comment_has_no_terminator() {
        let f = Fixture::new();
        f.write("a.html", "<!-- TODO(gh#4): c -->\n");
        let r = scan_dir(f.path());
        assert_eq!(r.todos.len(), 1);
        assert_eq!(r.todos[0].text, "c");
    }

    #[test]
    fn custom_tag_with_symbol_is_found() {
        let f = Fixture::new();
        f.write(".td2do.toml", "tags = [\"@todo\"]\n");
        f.write("a.rs", "// @todo(gh#1): x\n");
        let r = scan_dir(f.path());
        assert_eq!(r.todos.len(), 1);
        assert_eq!(r.todos[0].tag, "@todo");
    }

    #[test]
    fn scan_source_applies_skip_rules() {
        let f = Fixture::new();
        f.write(".td2do.toml", "ignore = [\"gen/**\"]\n");
        let repo = open(f.path()).unwrap();
        let src = b"// TODO: x\n";
        assert_eq!(scan_source(&repo, Path::new("src/x.rs"), src).len(), 1);
        for skipped in [
            "vendor/x.rs",
            "a/node_modules/x.rs",
            "gen/x.rs",
            "x.min.js",
            "Cargo.lock",
        ] {
            assert!(
                scan_source(&repo, Path::new(skipped), src).is_empty(),
                "{skipped}"
            );
        }
        assert!(scan_source(&repo, Path::new("src/x.rs"), b"// TODO: x\n\0").is_empty());
        let mut big = src.to_vec();
        big.resize(2 * 1024 * 1024 + 1, b' ');
        assert!(scan_source(&repo, Path::new("src/x.rs"), &big).is_empty());
    }

    #[test]
    fn committed_file_is_found() {
        let f = Fixture::new();
        f.write("a.rs", "// TODO: committed\n");
        f.commit("add");
        assert_eq!(places(&scan_dir(f.path())), [("a.rs".to_string(), 1)]);
    }

    #[test]
    fn committed_then_deleted_file_is_skipped() {
        let f = Fixture::new();
        f.write("gone.rs", "// TODO: gone\n");
        f.write("kept.rs", "// TODO: kept\n");
        f.commit("add");
        std::fs::remove_file(f.path().join("gone.rs")).unwrap();
        assert_eq!(places(&scan_dir(f.path())), [("kept.rs".to_string(), 1)]);
    }

    #[test]
    fn scan_source_reads_given_bytes() {
        let f = Fixture::new();
        let repo = open(f.path()).unwrap();
        let todos = scan_source(&repo, Path::new("x/y.rs"), b"fn f() {}\n// TODO: staged\n");
        assert_eq!(todos.len(), 1);
        assert_eq!(todos[0].path, Path::new("x/y.rs"));
        assert_eq!(todos[0].line, 2);
        assert!(scan_source(&repo, Path::new("x/y.xyz"), b"// TODO: staged\n").is_empty());
    }
}
