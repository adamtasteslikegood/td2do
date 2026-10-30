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
