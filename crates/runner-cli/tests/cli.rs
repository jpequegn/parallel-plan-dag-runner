use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn help_describes_the_cli() {
    Command::cargo_bin("plan-runner")
        .expect("binary should build")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("typed plan DAGs"));
}

#[test]
fn prints_supported_format_version() {
    Command::cargo_bin("plan-runner")
        .expect("binary should build")
        .arg("format-version")
        .assert()
        .success()
        .stdout("v1\n");
}
