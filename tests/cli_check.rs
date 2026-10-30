use assert_cmd::Command;

mod support;
use support::Fixture;

fn td2do() -> Command {
    Command::cargo_bin("td2do").unwrap()
}

/// `main` holds one old TODO with no REF; HEAD is a new branch `feat` on top of it.
fn repo() -> Fixture {
    let f = Fixture::new();
    f.write("old.rs", "fn old() {}\n// TODO: old thing\n");
    f.commit("init");
    f.git(&["checkout", "-q", "-b", "feat"]);
    f
}

struct Run {
    code: i32,
    stdout: String,
    stderr: String,
}

fn check(f: &Fixture, args: &[&str]) -> Run {
    let o = td2do()
        .arg("-C")
        .arg(f.path())
        .arg("check")
        .args(args)
        .output()
        .unwrap();
    Run {
        code: o.status.code().unwrap(),
        stdout: String::from_utf8(o.stdout).unwrap(),
        stderr: String::from_utf8(o.stderr).unwrap(),
    }
}

fn assert_code(r: &Run, code: i32) {
    assert_eq!(r.code, code, "stdout:\n{}\nstderr:\n{}", r.stdout, r.stderr);
}

#[test]
fn old_todos_pass() {
    let f = repo();
    let r = check(&f, &[]);
    assert_code(&r, 0);
    assert!(!r.stdout.contains("old.rs"), "{}", r.stdout);
}

#[test]
fn new_untracked_fails() {
    let f = repo();
    f.write("new.rs", "fn a() {}\n\n// TODO: x\n");
    f.commit("add");
    let r = check(&f, &["--base", "main"]);
    assert_code(&r, 1);
    assert!(r.stdout.contains("new.rs:3:4: TODO x"), "{}", r.stdout);
    assert!(r.stdout.contains("no_ref"), "{}", r.stdout);
    assert!(!r.stdout.contains("old.rs"), "{}", r.stdout);
}

#[test]
fn new_tracked_passes() {
    let f = repo();
    f.write("new.rs", "// TODO(gh#4): x\n");
    f.commit("add");
    assert_code(&check(&f, &[]), 0);
}

#[test]
fn new_relay_ref_fails_bad_ref() {
    let f = repo();
    f.write("new.rs", "// TODO(relay:abc): x\n");
    f.commit("add");
    let r = check(&f, &[]);
    assert_code(&r, 1);
    assert!(r.stdout.contains("bad_ref"), "{}", r.stdout);
    assert!(r.stdout.contains("relay"), "{}", r.stdout);
}

#[test]
fn new_malformed_fails() {
    let f = repo();
    f.write("new.rs", "// TODO(gh#abc): x\n");
    f.commit("add");
    let r = check(&f, &[]);
    assert_code(&r, 1);
    assert!(r.stdout.contains("bad_ref"), "{}", r.stdout);
    assert!(r.stdout.contains("gh#abc"), "{}", r.stdout);
}

#[test]
fn duplicate_warns() {
    let f = repo();
    f.write("other.rs", "// TODO(gh#9): old thing\n");
    f.commit("dup");
    let r = check(&f, &[]);
    assert_code(&r, 0);
    assert!(r.stdout.contains("duplicate_text"), "{}", r.stdout);
    assert!(r.stdout.contains("other.rs:1:4"), "{}", r.stdout);
    assert!(r.stdout.contains("old.rs:2:4"), "{}", r.stdout);
}

#[test]
fn duplicate_fail_config() {
    let f = repo();
    f.write(".td2do.toml", "[check]\nduplicate_text = \"fail\"\n");
    f.write("other.rs", "// TODO(gh#9): old thing\n");
    f.commit("dup");
    let r = check(&f, &[]);
    assert_code(&r, 1);
    assert!(r.stdout.contains("duplicate_text"), "{}", r.stdout);
}

#[test]
fn duplicate_off_config() {
    let f = repo();
    f.write(".td2do.toml", "[check]\nduplicate_text = \"off\"\n");
    f.write("other.rs", "// TODO(gh#9): old thing\n");
    f.commit("dup");
    let r = check(&f, &[]);
    assert_code(&r, 0);
    assert!(!r.stdout.contains("duplicate_text"), "{}", r.stdout);
}

#[test]
fn staged_mode() {
    let f = repo();
    f.write("staged.rs", "// TODO: staged\n");
    f.git(&["add", "staged.rs"]);
    let r = check(&f, &["--staged"]);
    assert_code(&r, 1);
    assert!(r.stdout.contains("staged.rs:1:4"), "{}", r.stdout);

    let f = repo();
    f.write("loose.rs", "fn a() {}\n");
    f.git(&["add", "loose.rs"]);
    // The TODO is only in the working tree, not in the index.
    f.write("loose.rs", "fn a() {}\n// TODO: unstaged\n");
    assert_code(&check(&f, &["--staged"]), 0);
    let r = check(&f, &[]);
    assert_code(&r, 1);
    assert!(r.stdout.contains("loose.rs:2:4"), "{}", r.stdout);
}

#[test]
fn staged_reads_index_lines() {
    let f = repo();
    f.write("s.rs", "// TODO: staged\n");
    f.git(&["add", "s.rs"]);
    // Unstaged lines above it move it in the working tree; the index line counts.
    f.write("s.rs", "fn a() {}\nfn b() {}\n// TODO: staged\n");
    let r = check(&f, &["--staged"]);
    assert_code(&r, 1);
    assert!(r.stdout.contains("s.rs:1:4"), "{}", r.stdout);
}

#[test]
fn untracked_file_counts_in_default_mode_only() {
    let f = repo();
    f.write("scratch.rs", "// TODO: brand new\n");
    let r = check(&f, &[]);
    assert_code(&r, 1);
    assert!(r.stdout.contains("scratch.rs:1:4"), "{}", r.stdout);
    assert_code(&check(&f, &["--staged"]), 0);
}

#[test]
fn ignored_untracked_file_is_not_checked() {
    let f = repo();
    f.write(".gitignore", "scratch.rs\n");
    f.commit("ignore");
    f.write("scratch.rs", "// TODO: brand new\n");
    assert_code(&check(&f, &[]), 0);
}

#[test]
fn all_flag() {
    let f = repo();
    let r = check(&f, &["--all"]);
    assert_code(&r, 1);
    assert!(r.stdout.contains("old.rs:2:4"), "{}", r.stdout);
}

#[test]
fn ignore_escape_passes() {
    let f = repo();
    f.write("new.rs", "// TODO: x td2do:ignore\n");
    f.commit("add");
    assert_code(&check(&f, &[]), 0);
}

#[test]
fn continuation_line_makes_todo_new() {
    let f = repo();
    f.write("c.rs", "// TODO: first\n");
    f.commit("c");
    f.git(&["checkout", "-q", "main"]);
    f.git(&["merge", "-q", "--ff-only", "feat"]);
    f.git(&["checkout", "-q", "feat"]);
    f.write("c.rs", "// TODO: first\n// second\n");
    let r = check(&f, &[]);
    assert_code(&r, 1);
    assert!(
        r.stdout.contains("c.rs:1:4: TODO first second"),
        "{}",
        r.stdout
    );
}

#[test]
fn missing_base_exits_2() {
    let f = repo();
    let r = check(&f, &["--base", "nope"]);
    assert_code(&r, 2);
    assert!(r.stderr.contains("fetch-depth: 0"), "{}", r.stderr);
    assert!(r.stderr.contains("nope"), "{}", r.stderr);
}

#[test]
fn config_base_is_used_and_flag_overrides() {
    let f = repo();
    f.write(".td2do.toml", "[check]\nbase = \"nope\"\n");
    f.commit("cfg");
    let r = check(&f, &[]);
    assert_code(&r, 2);
    assert!(r.stderr.contains("nope"), "{}", r.stderr);
    assert_code(&check(&f, &["--base", "main"]), 0);
}

#[test]
fn shallow_clone_exits_2() {
    let f = repo();
    f.write("new.rs", "// TODO(gh#1): x\n");
    f.commit("add");
    let dest = tempfile::tempdir().unwrap();
    let clone = dest.path().join("c");
    f.git(&[
        "clone",
        "-q",
        "--depth",
        "1",
        "--no-single-branch",
        &format!("file://{}", f.path().display()),
        clone.to_str().unwrap(),
    ]);
    let o = td2do()
        .arg("-C")
        .arg(&clone)
        .args(["check", "--base", "origin/main"])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&o.stderr);
    assert_eq!(o.status.code(), Some(2), "{stderr}");
    assert!(stderr.contains("fetch-depth: 0"), "{stderr}");
}

#[test]
fn rename_counts_only_new_line() {
    let f = repo();
    f.write("a.rs", "fn a() {}\n// TODO: kept\nfn b() {}\nfn c() {}\n");
    f.commit("a");
    f.git(&["checkout", "-q", "main"]);
    f.git(&["merge", "-q", "--ff-only", "feat"]);
    f.git(&["checkout", "-q", "feat"]);
    f.git(&["mv", "a.rs", "b.rs"]);
    f.write(
        "b.rs",
        "fn a() {}\n// TODO: kept\nfn b() {}\nfn c() {}\n// TODO: added\n",
    );
    f.commit("mv");
    let r = check(&f, &[]);
    assert_code(&r, 1);
    assert!(r.stdout.contains("b.rs:5:4: TODO added"), "{}", r.stdout);
    assert!(!r.stdout.contains("kept"), "{}", r.stdout);
}

#[test]
fn odd_paths() {
    let f = repo();
    f.write("dir with space/é.rs", "// TODO: odd\n");
    f.write("q\"uote\\.rs", "// TODO: quote\n");
    f.commit("odd");
    let r = check(&f, &[]);
    assert_code(&r, 1);
    assert!(
        r.stdout.contains("dir with space/é.rs:1:4: TODO odd"),
        "{}",
        r.stdout
    );
    assert!(
        r.stdout.contains("q\"uote\\.rs:1:4: TODO quote"),
        "{}",
        r.stdout
    );
}

#[test]
fn deleted_file_adds_nothing() {
    let f = repo();
    f.git(&["rm", "-q", "old.rs"]);
    f.commit("rm");
    assert_code(&check(&f, &[]), 0);
}

#[test]
fn check_json() {
    let f = repo();
    f.write("new.rs", "// TODO: x\n");
    f.commit("add");
    let r = check(&f, &["--json"]);
    assert_code(&r, 1);
    let v: serde_json::Value = serde_json::from_str(&r.stdout).unwrap();
    assert_eq!(v["schema"], 1);
    assert_eq!(v["passed"], false);
    assert_eq!(v["checked"], "new");
    assert_eq!(v["base"], "main");
    let violations = v["violations"].as_array().unwrap();
    assert_eq!(violations.len(), 1, "{v}");
    assert_eq!(violations[0]["rule"], "no_ref");
    assert_eq!(violations[0]["severity"], "fail");
    assert_eq!(violations[0]["todo"]["file"], "new.rs");
    assert_eq!(violations[0]["other"], serde_json::Value::Null);
}

#[test]
fn check_json_all_passes() {
    let f = Fixture::new();
    f.write("a.rs", "// TODO(gh#1): x\n");
    f.commit("init");
    let r = check(&f, &["--json", "--all"]);
    assert_code(&r, 0);
    let v: serde_json::Value = serde_json::from_str(&r.stdout).unwrap();
    assert_eq!(v["passed"], true);
    assert_eq!(v["checked"], "all");
    assert_eq!(v["violations"].as_array().unwrap().len(), 0);
}

#[test]
fn inter_hunk_context_config_adds_no_context_lines() {
    let f = repo();
    f.write(
        "ctx.rs",
        "fn a() {}\n// TODO: between\nfn b() {}\nfn c() {}\n",
    );
    f.commit("ctx");
    f.git(&["checkout", "-q", "main"]);
    f.git(&["merge", "-q", "--ff-only", "feat"]);
    f.git(&["checkout", "-q", "feat"]);
    f.git(&["config", "diff.interHunkContext", "5"]);
    f.write(
        "ctx.rs",
        "fn a2() {}\n// TODO: between\nfn b() {}\nfn c2() {}\n",
    );
    f.commit("edit around the old TODO");
    let r = check(&f, &[]);
    assert_code(&r, 0);
    assert!(!r.stdout.contains("between"), "{}", r.stdout);
}

#[cfg(unix)]
#[test]
fn untracked_symlink_is_not_read_through() {
    let f = repo();
    let outside = tempfile::tempdir().unwrap();
    let target = outside.path().join("t.rs");
    std::fs::write(&target, "// TODO: outside\n").unwrap();
    std::os::unix::fs::symlink(&target, f.path().join("link.rs")).unwrap();
    f.write("real.rs", "// TODO: real\n");
    let r = check(&f, &[]);
    assert_code(&r, 1);
    assert!(r.stdout.contains("real.rs:1:4"), "{}", r.stdout);
    assert!(!r.stdout.contains("link.rs"), "{}", r.stdout);
    assert!(!r.stdout.contains("outside"), "{}", r.stdout);
}

#[cfg(unix)]
#[test]
fn staged_name_with_stage_like_prefix() {
    let f = repo();
    f.write("1:x.rs", "// TODO: colon\n");
    f.git(&["add", "1:x.rs"]);
    let r = check(&f, &["--staged"]);
    assert_code(&r, 1);
    assert!(r.stdout.contains("1:x.rs:1:4: TODO colon"), "{}", r.stdout);
}

#[test]
fn unborn_head_counts_everything_as_added() {
    let f = Fixture::new();
    f.write("a.rs", "// TODO: staged\n");
    f.git(&["add", "a.rs"]);
    f.write("b.rs", "// TODO: untracked\n");
    let r = check(&f, &["--staged"]);
    assert_code(&r, 1);
    assert!(r.stdout.contains("a.rs:1:4"), "{}", r.stdout);
    assert!(!r.stdout.contains("b.rs"), "{}", r.stdout);
    let r = check(&f, &[]);
    assert_code(&r, 1);
    assert!(r.stdout.contains("a.rs:1:4"), "{}", r.stdout);
    assert!(r.stdout.contains("b.rs:1:4"), "{}", r.stdout);
}

#[test]
fn master_without_a_remote_is_the_default_base() {
    let f = Fixture::new();
    f.git(&["remote", "remove", "origin"]);
    f.git(&["symbolic-ref", "HEAD", "refs/heads/master"]);
    f.write("old.rs", "// TODO: old\n");
    f.commit("init");
    f.git(&["checkout", "-q", "-b", "feat"]);
    f.write("new.rs", "// TODO: new\n");
    f.commit("add");
    let r = check(&f, &[]);
    assert_code(&r, 1);
    assert!(r.stdout.contains("new.rs:1:4"), "{}", r.stdout);
    assert!(!r.stdout.contains("old.rs"), "{}", r.stdout);
    assert!(r.stdout.contains("(base master)"), "{}", r.stdout);
}

#[test]
fn binary_attribute_does_not_hide_todos() {
    let f = repo();
    f.write(".gitattributes", "*.gen.rs -diff\n");
    f.write("a.gen.rs", "fn a() {}\n// TODO: hidden\n");
    f.commit("gen");
    let r = check(&f, &[]);
    assert_code(&r, 1);
    assert!(
        r.stdout.contains("a.gen.rs:2:4: TODO hidden"),
        "{}",
        r.stdout
    );
}
