use std::fs;

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

#[test]
fn validates_the_example_plan() {
    Command::cargo_bin("plan-runner")
        .expect("binary should build")
        .args(["validate", "../../examples/basic-plan.yaml"])
        .assert()
        .success()
        .stdout(predicate::str::contains("valid plan: basic-plan"));
}

#[test]
fn writes_the_plan_schema() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let output = directory.path().join("plan.schema.json");
    Command::cargo_bin("plan-runner")
        .expect("binary should build")
        .args(["schema", "--output"])
        .arg(&output)
        .assert()
        .success();
    assert!(
        fs::read_to_string(output)
            .expect("schema file")
            .contains("max_concurrency")
    );
}
