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

#[test]
fn runs_and_replays_from_sqlite() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let db = directory.path().join("runs.db");
    let output = Command::cargo_bin("plan-runner")
        .expect("binary should build")
        .args(["run", "../../examples/basic-plan.yaml", "--db"])
        .arg(&db)
        .output()
        .expect("run plan");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let payload: serde_json::Value = serde_json::from_slice(&output.stdout).expect("run JSON");
    let run_id = payload["run_id"].as_str().expect("run ID");
    Command::cargo_bin("plan-runner")
        .expect("binary should build")
        .arg("replay")
        .arg(run_id)
        .arg("--db")
        .arg(&db)
        .assert()
        .success()
        .stdout(predicate::str::contains("subtotal"));
}

#[test]
fn evaluates_fixture_suite_from_one_command() {
    let directory = tempfile::tempdir().expect("temporary directory");
    Command::cargo_bin("plan-runner")
        .expect("binary should build")
        .args([
            "evaluate",
            "--fixtures",
            "../../benchmarks/fixtures.json",
            "--output",
        ])
        .arg(directory.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "evaluated 18 fixtures across 54 runs",
        ));
    assert!(directory.path().join("evaluation.md").exists());
}
