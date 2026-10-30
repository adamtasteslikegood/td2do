use assert_cmd::Command;
use predicates::str::contains;

fn td2do() -> Command {
    Command::cargo_bin("td2do").unwrap()
}

#[test]
fn bad_config_exits_2() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(".td2do.toml"), "tags = 3\n").unwrap();
    td2do()
        .arg("-C")
        .arg(dir.path())
        .arg("list")
        .assert()
        .code(2)
        .stderr(contains(".td2do.toml"));
}

#[test]
fn bare_command_is_list_and_exits_0() {
    let dir = tempfile::tempdir().unwrap();
    td2do()
        .arg("-C")
        .arg(dir.path())
        .assert()
        .success()
        .stdout("");
}

mod support;
use support::Fixture;

fn repo() -> Fixture {
    let f = Fixture::new();
    f.write(
        "src/a.rs",
        "fn a() {\n    // not a todo\n    //   TODO(gh#12) retry\n}\n// FIXME no ref here\n// HACK(gh:o/x#5) odd\n",
    );
    f.write("lib/b.py", "# BUG(jira:AB-1) crash\n# TODO later\n");
    f.commit("init");
    f
}

fn out(f: &Fixture, args: &[&str]) -> String {
    let o = td2do().arg("-C").arg(f.path()).args(args).output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8(o.stdout).unwrap()
}

#[test]
fn list_plain_when_piped() {
    let f = repo();
    let s = out(&f, &["list"]);
    assert!(
        s.lines().any(|l| l == "src/a.rs:3:10: TODO(gh#12) retry"),
        "{s}"
    );
    assert!(s.contains("src/a.rs:5:4: FIXME no ref here\n"), "{s}");
    assert!(s.contains("lib/b.py:1:3: BUG(jira:AB-1) crash\n"), "{s}");
    assert!(!s.contains('\x1b'));
}

#[test]
fn list_json_schema() {
    let f = repo();
    let v: serde_json::Value = serde_json::from_str(&out(&f, &["list", "--json"])).unwrap();
    assert_eq!(v["schema"], 1);
    assert_eq!(v["skipped_unknown_lang"], 0);
    let todos = v["todos"].as_array().unwrap();
    assert_eq!(todos.len(), 5);
    let t = todos.iter().find(|t| t["ref"] == "gh#12").unwrap();
    assert_eq!(t["file"], "src/a.rs");
    assert_eq!(t["line"], 3);
    assert_eq!(t["tag"], "TODO");
    assert_eq!(t["text"], "retry");
    assert!(t.get("extra").is_some());
    assert_eq!(t["fingerprint"].as_str().unwrap().len(), 16);
    let n = todos.iter().find(|t| t["tag"] == "FIXME").unwrap();
    assert!(n["ref"].is_null());
}

#[test]
fn list_untracked_filter() {
    let f = repo();
    let s = out(&f, &["list", "--untracked"]);
    assert_eq!(s.lines().count(), 2, "{s}");
    assert!(s.contains("FIXME") && s.contains("TODO later"));
}

#[test]
fn list_ref_filter() {
    let f = repo();
    let s = out(&f, &["list", "--ref", "gh:o/x#5"]);
    assert_eq!(s.lines().count(), 1, "{s}");
    assert!(s.contains("HACK(gh:o/x#5) odd"));
}

#[test]
fn list_tag_filter() {
    let f = repo();
    let s = out(&f, &["list", "--tag", "FIXME", "--tag", "bug"]);
    assert_eq!(s.lines().count(), 2, "{s}");
    assert!(s.contains("FIXME") && s.contains("BUG"));
}

#[test]
fn list_path_filter() {
    let f = repo();
    let s = out(&f, &["list", "--path", "lib/**"]);
    assert_eq!(s.lines().count(), 2, "{s}");
    assert!(s.lines().all(|l| l.starts_with("lib/b.py")));
}

#[test]
fn terminal_output_groups_by_file() {
    let f = repo();
    let s = out(&f, &["list", "--color", "always"]);
    assert!(s.contains("\x1b["), "{s}");
    assert!(s.contains("src/a.rs"));
    assert!(s.contains("5 TODOs, 2 untracked"), "{s}");
}

#[test]
fn no_color_env() {
    let f = repo();
    let o = td2do()
        .env("NO_COLOR", "1")
        .arg("-C")
        .arg(f.path())
        .args(["list", "--color", "always"])
        .output()
        .unwrap();
    let s = String::from_utf8(o.stdout).unwrap();
    assert!(!s.contains("\x1b["), "{s}");
    assert!(s.contains("5 TODOs, 2 untracked"), "{s}");
}

#[test]
fn bare_td2do_is_list() {
    let f = repo();
    let bare = td2do().arg("-C").arg(f.path()).output().unwrap();
    assert_eq!(String::from_utf8(bare.stdout).unwrap(), out(&f, &["list"]));
}

#[test]
fn list_into_closed_pipe_does_not_panic() {
    let f = repo();
    let mut child = std::process::Command::new(assert_cmd::cargo::cargo_bin("td2do"))
        .arg("-C")
        .arg(f.path())
        .arg("list")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    // Close the read end before td2do writes, so its write gets EPIPE.
    drop(child.stdout.take());
    let out = child.wait_with_output().unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!stderr.contains("panicked"), "{stderr}");
    assert_eq!(out.status.code(), Some(0), "{stderr}");
}

#[test]
fn list_tag_filter_keeps_custom_tag_case() {
    let f = Fixture::new();
    f.write(".td2do.toml", "tags = [\"@todo\", \"TODO\"]\n");
    f.write("a.rs", "// @todo(gh#1): custom\n// TODO: plain\n");
    let s = out(&f, &["list", "--tag", "@todo"]);
    assert_eq!(s, "a.rs:1:4: @todo(gh#1) custom\n");
    let s = out(&f, &["list", "--tag", "todo"]);
    assert_eq!(s, "a.rs:2:4: TODO plain\n");
}
