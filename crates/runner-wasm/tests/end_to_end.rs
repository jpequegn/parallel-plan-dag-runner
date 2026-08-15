use std::collections::BTreeMap;

use runner_core::{
    ExecutionMode, Executor, ExperimentHarness, FixtureSpec, Ledger, RunStatus, ToolRegistry,
    parse_plan, validate_plan,
};
use runner_wasm::{replay_payload, validate_payload};
use serde_json::Value;
use tokio_util::sync::CancellationToken;

const RISK_PLAN: &str = include_str!("../../../examples/risk-plan.yaml");

#[tokio::test]
async fn native_run_replays_in_native_wasm_and_evaluation_outputs() {
    let validation: Value =
        serde_json::from_str(&validate_payload(RISK_PLAN, "yaml")).expect("WASM validation JSON");
    assert_eq!(validation["ok"], true);

    let plan = parse_plan(RISK_PLAN, "yaml").expect("parse plan");
    validate_plan(&plan).expect("validate plan");
    let registry = ToolRegistry::deterministic(BTreeMap::new(), BTreeMap::new());
    let result = Executor::new(&registry)
        .mode(ExecutionMode::Parallel)
        .execute(&plan, CancellationToken::new())
        .await
        .expect("execute plan");
    assert_eq!(result.status, RunStatus::Succeeded);

    let directory = tempfile::tempdir().expect("temporary directory");
    let mut ledger = Ledger::open(directory.path().join("runs.db")).expect("open ledger");
    let run_id = ledger
        .store_run(&plan, ExecutionMode::Parallel, &result)
        .expect("store run");
    let events = ledger.inspect(&run_id).expect("inspect event stream");
    assert_eq!(ledger.replay(&run_id).expect("native replay"), result);

    let visualized: Value = serde_json::from_str(&replay_payload(
        &serde_json::to_string(&events).expect("serialize events"),
    ))
    .expect("WASM replay JSON");
    assert_eq!(visualized["ok"], true);
    assert_eq!(visualized["replay"]["status"], "succeeded");
    assert_eq!(
        visualized["replay"]["nodes"]["risk-register"]["output"],
        result.outputs["risk-register"].value
    );

    let fixtures = [FixtureSpec {
        id: "release-width-2".to_owned(),
        domain: "release".to_owned(),
        width: 2,
        tail_depth: 1,
        delay_ms: 1,
    }];
    let report = ExperimentHarness::run(&fixtures)
        .await
        .expect("run evaluation");
    let reports = directory.path().join("reports");
    ExperimentHarness::write(&report, &reports).expect("write reports");
    assert_eq!(report.run_count, 3);
    assert!(reports.join("evaluation.json").is_file());
    assert!(reports.join("evaluation.csv").is_file());
    assert!(reports.join("evaluation.md").is_file());
}
