use std::collections::BTreeMap;

use runner_core::{
    ExecutionMode, Executor, Ledger, LedgerError, ToolRegistry, parse_plan, validate_plan,
};
use tokio_util::sync::CancellationToken;

const PLAN: &str = include_str!("../../../examples/basic-plan.yaml");

async fn completed_run() -> (runner_core::Plan, runner_core::RunResult) {
    let plan = parse_plan(PLAN, "yaml").expect("parse plan");
    validate_plan(&plan).expect("valid plan");
    let registry = ToolRegistry::deterministic(BTreeMap::new(), BTreeMap::new());
    let result = Executor::new(&registry)
        .execute(&plan, CancellationToken::new())
        .await
        .expect("execute plan");
    (plan, result)
}

#[tokio::test]
async fn stores_inspects_and_replays_without_tools() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let (plan, result) = completed_run().await;
    let mut ledger = Ledger::open(directory.path().join("runs.db")).expect("open ledger");
    let run_id = ledger
        .store_run(&plan, ExecutionMode::Parallel, &result)
        .expect("store run");
    let summaries = ledger.list_runs().expect("list runs");
    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].run_id, run_id);
    let events = ledger.inspect(&run_id).expect("inspect events");
    assert_eq!(events.len(), result.events.len());
    assert!(events.iter().enumerate().all(|(index, event)| {
        event.sequence == u64::try_from(index).expect("event index fits in u64")
    }));
    assert_eq!(ledger.replay(&run_id).expect("replay"), result);
}

#[tokio::test]
async fn sqlite_guards_events_against_update_and_delete() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("runs.db");
    let (plan, result) = completed_run().await;
    let mut ledger = Ledger::open(&path).expect("open ledger");
    ledger
        .store_run(&plan, ExecutionMode::Parallel, &result)
        .expect("store run");
    drop(ledger);
    let connection = rusqlite::Connection::open(path).expect("open raw database");
    let update = connection.execute("UPDATE events SET payload = '{}' WHERE sequence = 0", []);
    assert!(
        update
            .expect_err("updates must fail")
            .to_string()
            .contains("append-only")
    );
    let delete = connection.execute("DELETE FROM events WHERE sequence = 0", []);
    assert!(
        delete
            .expect_err("deletes must fail")
            .to_string()
            .contains("append-only")
    );
}

#[tokio::test]
async fn detects_a_tampered_event_chain() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("runs.db");
    let (plan, result) = completed_run().await;
    let mut ledger = Ledger::open(&path).expect("open ledger");
    let run_id = ledger
        .store_run(&plan, ExecutionMode::Parallel, &result)
        .expect("store run");
    drop(ledger);
    let connection = rusqlite::Connection::open(&path).expect("open raw database");
    connection
        .execute_batch(
            "DROP TRIGGER events_no_update;
             UPDATE events SET digest = 'altered' WHERE sequence = 1;",
        )
        .expect("simulate storage corruption");
    let ledger = Ledger::open(path).expect("reopen ledger");
    assert!(matches!(
        ledger.inspect(&run_id),
        Err(LedgerError::Corrupt(_))
    ));
}
