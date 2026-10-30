use assert_cmd::Command;

mod support;
use support::Fixture;

fn td2do() -> Command {
    Command::cargo_bin("td2do").unwrap()
}

fn repo() -> Fixture {
    let f = Fixture::new();
    f.write("src/lib.rs", "fn main() {}\n");
    f.commit("init");
    f
}

fn init(f: &Fixture, args: &[&str]) -> Command {
    let mut c = td2do();
    c.arg("-C").arg(f.path()).arg("init").args(args);
    c
}

#[test]
fn init_yes_writes_three() {
    let f = repo();
    let out = init(&f, &["--yes"]).output().unwrap();
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        stdout.contains("agent rule: available in phase 2 (needs td2do find and add)"),
        "{stdout}"
    );

    let config = std::fs::read_to_string(f.path().join(".td2do.toml")).unwrap();
    assert!(config.contains("base = \"main\""), "{config}");
    assert!(config.contains("duplicate_text = \"warn\""), "{config}");

    let hook = f.path().join(".git/hooks/pre-commit");
    assert_eq!(
        std::fs::read_to_string(&hook).unwrap(),
        "#!/bin/sh\nexec td2do check --staged\n"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&hook).unwrap().permissions().mode() & 0o777,
            0o755
        );
    }

    let wf = std::fs::read_to_string(f.path().join(".github/workflows/td2do.yml")).unwrap();
    assert!(wf.contains("pull_request"), "{wf}");
    assert!(
        wf.contains("actions/checkout@11d5960a326750d5838078e36cf38b85af677262 # v4"),
        "{wf}"
    );
    assert!(wf.contains("fetch-depth: 0"), "{wf}");
    assert!(
        wf.contains("cargo install --locked --git https://github.com/adamtasteslikegood/td2do"),
        "{wf}"
    );
    assert!(
        wf.contains("td2do check --base origin/${{ github.base_ref }}"),
        "{wf}"
    );

    // The files init wrote must not trip td2do itself.
    td2do()
        .arg("-C")
        .arg(f.path())
        .arg("check")
        .assert()
        .success();
}

#[test]
fn init_keeps_origin_in_the_default_base() {
    let f = repo();
    f.git(&["update-ref", "refs/remotes/origin/trunk", "HEAD"]);
    f.git(&[
        "symbolic-ref",
        "refs/remotes/origin/HEAD",
        "refs/remotes/origin/trunk",
    ]);
    init(&f, &["--yes"]).assert().success();
    let config = std::fs::read_to_string(f.path().join(".td2do.toml")).unwrap();
    assert!(config.contains("base = \"origin/trunk\""), "{config}");
}

#[test]
fn init_on_a_clone_writes_origin_main() {
    let f = repo();
    let dest = tempfile::tempdir().unwrap();
    let clone = dest.path().join("c");
    f.git(&[
        "clone",
        "-q",
        &format!("file://{}", f.path().display()),
        clone.to_str().unwrap(),
    ]);
    let out = init_in(&clone);
    assert_eq!(out.status.code(), Some(0));
    let config = std::fs::read_to_string(clone.join(".td2do.toml")).unwrap();
    assert!(config.contains("\nbase = \"origin/main\""), "{config}");
}

#[test]
fn init_without_a_base_comments_it_out() {
    // No commits yet: no ref resolves.
    let f = Fixture::new();
    let out = init(&f, &["--yes"]).output().unwrap();
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("check will use its default"), "{stdout}");
    let config = std::fs::read_to_string(f.path().join(".td2do.toml")).unwrap();
    assert!(config.contains("\n# base = \"main\"\n"), "{config}");
    assert!(!config.contains("\nbase ="), "{config}");

    // The hook's command works before the first commit.
    f.write("a.rs", "// TODO: first\n");
    f.git(&["add", "a.rs"]);
    let check = |args: &[&str]| {
        td2do()
            .arg("-C")
            .arg(f.path())
            .arg("check")
            .args(args)
            .output()
            .unwrap()
    };
    let o = check(&["--staged"]);
    let stdout = String::from_utf8_lossy(&o.stdout);
    assert_eq!(
        o.status.code(),
        Some(1),
        "{stdout}{}",
        String::from_utf8_lossy(&o.stderr)
    );
    assert!(stdout.contains("a.rs:1:4"), "{stdout}");
    f.write("a.rs", "// TODO(gh#1): first\n");
    f.git(&["add", "a.rs"]);
    let o = check(&["--staged"]);
    assert_eq!(
        o.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
}

#[test]
fn init_keeps_existing_hook() {
    let f = repo();
    f.write(".git/hooks/pre-commit", "#!/bin/sh\necho mine\n");
    let out = init(&f, &["--yes"]).output().unwrap();
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("exec td2do check --staged"), "{stdout}");
    assert!(
        stdout.contains("add this line to your existing hook"),
        "{stdout}"
    );
    assert_eq!(
        std::fs::read_to_string(f.path().join(".git/hooks/pre-commit")).unwrap(),
        "#!/bin/sh\necho mine\n"
    );
}

#[test]
fn init_never_overwrites_config() {
    let f = repo();
    f.write(".td2do.toml", "label = \"mine\"\n");
    f.write(".github/workflows/td2do.yml", "name: mine\n");
    let out = init(&f, &["--yes"]).output().unwrap();
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("exists, skipped"), "{stdout}");
    assert_eq!(
        std::fs::read_to_string(f.path().join(".td2do.toml")).unwrap(),
        "label = \"mine\"\n"
    );
    assert_eq!(
        std::fs::read_to_string(f.path().join(".github/workflows/td2do.yml")).unwrap(),
        "name: mine\n"
    );
}

#[test]
fn init_declined_writes_nothing() {
    let f = repo();
    let out = init(&f, &[]).write_stdin("n\nn\nn\n").output().unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert!(!f.path().join(".td2do.toml").exists());
    assert!(!f.path().join(".git/hooks/pre-commit").exists());
    assert!(!f.path().join(".github").exists());
}

#[test]
fn init_eof_counts_as_no() {
    let f = repo();
    init(&f, &[]).write_stdin("").assert().success();
    assert!(!f.path().join(".td2do.toml").exists());
}

#[test]
fn init_answers_yes_from_stdin() {
    let f = repo();
    init(&f, &[]).write_stdin("y\nn\nn\n").assert().success();
    assert!(f.path().join(".td2do.toml").exists());
    assert!(!f.path().join(".git/hooks/pre-commit").exists());
    assert!(!f.path().join(".github").exists());
}

#[test]
fn init_json() {
    let f = repo();
    f.write(".git/hooks/pre-commit", "#!/bin/sh\n");
    let out = init(&f, &["--yes", "--json"]).output().unwrap();
    assert_eq!(out.status.code(), Some(0));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["schema"], 1);
    assert_eq!(
        v["written"],
        serde_json::json!([".td2do.toml", ".github/workflows/td2do.yml"])
    );
    assert_eq!(v["skipped"][0]["path"], ".git/hooks/pre-commit");
    assert_eq!(v["skipped"][0]["reason"], "exists");
}

#[test]
fn init_outside_git_exits_2() {
    let dir = tempfile::tempdir().unwrap();
    let out = init_in(dir.path());
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("not a git repository"));
    assert!(!dir.path().join(".td2do.toml").exists());
}

fn init_in(dir: &std::path::Path) -> std::process::Output {
    td2do()
        .arg("-C")
        .arg(dir)
        .args(["init", "--yes"])
        .output()
        .unwrap()
}
