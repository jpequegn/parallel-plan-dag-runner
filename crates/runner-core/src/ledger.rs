use std::{
    collections::BTreeMap,
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::{
    ExecutionMode, NodeState, Plan, ResolvedOutput, RunResult, RunStatus, VerificationEvidence,
    canonical_digest,
};

const EVENT_SCHEMA_VERSION: u32 = 1;
static RUN_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EventKind {
    RunStarted {
        plan_id: String,
        mode: ExecutionMode,
        nodes: Vec<String>,
    },
    NodeStarted {
        node_id: String,
    },
    ToolCall {
        node_id: String,
        tool: String,
        inputs: BTreeMap<String, Value>,
    },
    ToolResponse {
        node_id: String,
        output: ResolvedOutput,
    },
    VerifierResult {
        node_id: Option<String>,
        accepted: bool,
        evidence: VerificationEvidence,
    },
    Retry {
        node_id: String,
        attempt: u32,
        reason: String,
    },
    Replan {
        failed_node: String,
        patch_digest: String,
    },
    NodeSucceeded {
        node_id: String,
    },
    NodeDegraded {
        node_id: String,
        reason: String,
    },
    NodeFailed {
        node_id: String,
        error: String,
    },
    NodeTimedOut {
        node_id: String,
    },
    NodeBlocked {
        node_id: String,
    },
    NodeCancelled {
        node_id: String,
    },
    NodeNeedsReplan {
        node_id: String,
        reason: String,
    },
    Cancellation,
    RunCompleted {
        status: RunStatus,
    },
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct EventEnvelope {
    pub schema_version: u32,
    pub run_id: String,
    pub sequence: u64,
    pub timestamp_ms: u64,
    pub previous_digest: String,
    pub digest: String,
    pub event: EventKind,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RunSummary {
    pub run_id: String,
    pub plan_id: String,
    pub mode: ExecutionMode,
    pub status: RunStatus,
    pub created_at_ms: u64,
    pub event_count: usize,
    pub event_digest: String,
}

#[derive(Debug, Error)]
pub enum LedgerError {
    #[error(transparent)]
    Database(#[from] rusqlite::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error("run '{0}' does not exist")]
    MissingRun(String),
    #[error("corrupt event stream: {0}")]
    Corrupt(String),
}

pub struct Ledger {
    connection: Connection,
}

impl Ledger {
    /// Open a ledger and initialize its append-only schema.
    ///
    /// # Errors
    /// Returns a database error when the file cannot be opened or initialized.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, LedgerError> {
        let connection = Connection::open(path)?;
        connection.execute_batch(
            "PRAGMA foreign_keys = ON;
             CREATE TABLE IF NOT EXISTS runs (
               run_id TEXT PRIMARY KEY,
               plan_id TEXT NOT NULL,
               mode TEXT NOT NULL,
               status TEXT NOT NULL,
               created_at_ms INTEGER NOT NULL,
               event_count INTEGER NOT NULL,
               event_digest TEXT NOT NULL,
               schema_version INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS events (
               run_id TEXT NOT NULL REFERENCES runs(run_id),
               sequence INTEGER NOT NULL,
               timestamp_ms INTEGER NOT NULL,
               previous_digest TEXT NOT NULL,
               digest TEXT NOT NULL,
               payload TEXT NOT NULL,
               schema_version INTEGER NOT NULL,
               PRIMARY KEY (run_id, sequence)
             );
             CREATE TRIGGER IF NOT EXISTS events_no_update
               BEFORE UPDATE ON events BEGIN SELECT RAISE(ABORT, 'events are append-only'); END;
             CREATE TRIGGER IF NOT EXISTS events_no_delete
               BEFORE DELETE ON events BEGIN SELECT RAISE(ABORT, 'events are append-only'); END;",
        )?;
        Ok(Self { connection })
    }

    /// Store a completed in-memory run as one hash-chained transaction.
    ///
    /// # Errors
    /// Returns a database or serialization error if the transaction cannot commit.
    pub fn store_run(
        &mut self,
        plan: &Plan,
        mode: ExecutionMode,
        result: &RunResult,
    ) -> Result<String, LedgerError> {
        let created_at_ms = now_ms();
        let counter = RUN_COUNTER.fetch_add(1, Ordering::Relaxed);
        let run_id = format!(
            "run-{created_at_ms}-{}",
            &canonical_digest(&serde_json::json!({"plan": plan.id, "counter": counter}))[..12]
        );
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "INSERT INTO runs VALUES (?1, ?2, ?3, ?4, ?5, 0, '', ?6)",
            params![
                run_id,
                plan.id,
                serde_json::to_string(&mode)?,
                serde_json::to_string(&result.status)?,
                created_at_ms,
                EVENT_SCHEMA_VERSION,
            ],
        )?;
        let mut previous_digest = String::new();
        for (sequence, event) in result.events.iter().enumerate() {
            let sequence = u64::try_from(sequence).map_err(|_| {
                LedgerError::Corrupt("event count exceeds supported range".to_owned())
            })?;
            let timestamp_ms = now_ms();
            let digest = event_digest(sequence, &previous_digest, event);
            transaction.execute(
                "INSERT INTO events VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    run_id,
                    sequence,
                    timestamp_ms,
                    previous_digest,
                    digest,
                    serde_json::to_string(event)?,
                    EVENT_SCHEMA_VERSION,
                ],
            )?;
            previous_digest = digest;
        }
        transaction.execute(
            "UPDATE runs SET event_count = ?2, event_digest = ?3 WHERE run_id = ?1",
            params![run_id, result.events.len(), previous_digest],
        )?;
        transaction.commit()?;
        Ok(run_id)
    }

    /// Return all runs newest first.
    ///
    /// # Errors
    /// Returns a database or decoding error for invalid stored values.
    pub fn list_runs(&self) -> Result<Vec<RunSummary>, LedgerError> {
        let mut statement = self.connection.prepare(
            "SELECT run_id, plan_id, mode, status, created_at_ms, event_count, event_digest
             FROM runs ORDER BY created_at_ms DESC, run_id DESC",
        )?;
        let rows = statement.query_map([], |row| {
            let mode: String = row.get(2)?;
            let status: String = row.get(3)?;
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                mode,
                status,
                row.get::<_, u64>(4)?,
                row.get::<_, usize>(5)?,
                row.get::<_, String>(6)?,
            ))
        })?;
        rows.map(|row| {
            let (run_id, plan_id, mode, status, created_at_ms, event_count, event_digest) = row?;
            Ok(RunSummary {
                run_id,
                plan_id,
                mode: serde_json::from_str(&mode)?,
                status: serde_json::from_str(&status)?,
                created_at_ms,
                event_count,
                event_digest,
            })
        })
        .collect()
    }

    /// Load and verify a run's event chain.
    ///
    /// # Errors
    /// Returns an explicit corruption error for gaps, altered digests, or schema mismatch.
    pub fn inspect(&self, run_id: &str) -> Result<Vec<EventEnvelope>, LedgerError> {
        let summary = self
            .list_runs()?
            .into_iter()
            .find(|run| run.run_id == run_id)
            .ok_or_else(|| LedgerError::MissingRun(run_id.to_owned()))?;
        let mut statement = self.connection.prepare(
            "SELECT sequence, timestamp_ms, previous_digest, digest, payload, schema_version
             FROM events WHERE run_id = ?1 ORDER BY sequence",
        )?;
        let rows = statement.query_map([run_id], |row| {
            Ok((
                row.get::<_, u64>(0)?,
                row.get::<_, u64>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, u32>(5)?,
            ))
        })?;
        let mut envelopes = Vec::new();
        let mut previous_digest = String::new();
        for (expected_sequence, row) in rows.enumerate() {
            let (sequence, timestamp_ms, stored_previous, digest, payload, schema_version) = row?;
            let expected_sequence = u64::try_from(expected_sequence)
                .map_err(|_| LedgerError::Corrupt("event sequence overflow".to_owned()))?;
            if sequence != expected_sequence || stored_previous != previous_digest {
                return Err(LedgerError::Corrupt(format!(
                    "gap or previous-digest mismatch at sequence {expected_sequence}"
                )));
            }
            if schema_version != EVENT_SCHEMA_VERSION {
                return Err(LedgerError::Corrupt(format!(
                    "unsupported event schema {schema_version}"
                )));
            }
            let event: EventKind = serde_json::from_str(&payload)?;
            let expected_digest = event_digest(sequence, &stored_previous, &event);
            if digest != expected_digest {
                return Err(LedgerError::Corrupt(format!(
                    "digest mismatch at sequence {sequence}"
                )));
            }
            previous_digest.clone_from(&digest);
            envelopes.push(EventEnvelope {
                schema_version,
                run_id: run_id.to_owned(),
                sequence,
                timestamp_ms,
                previous_digest: stored_previous,
                digest,
                event,
            });
        }
        if envelopes.len() != summary.event_count || previous_digest != summary.event_digest {
            return Err(LedgerError::Corrupt(
                "event count or terminal digest mismatch".to_owned(),
            ));
        }
        Ok(envelopes)
    }

    /// Reconstruct a run solely from stored events and responses.
    ///
    /// # Errors
    /// Returns an explicit corruption error when the stream is incomplete or inconsistent.
    pub fn replay(&self, run_id: &str) -> Result<RunResult, LedgerError> {
        replay_events(&self.inspect(run_id)?)
    }
}

fn replay_events(events: &[EventEnvelope]) -> Result<RunResult, LedgerError> {
    let mut states = BTreeMap::new();
    let mut outputs = BTreeMap::new();
    let mut started_order = Vec::new();
    let mut completion_order = Vec::new();
    let mut terminal = None;
    for envelope in events {
        match &envelope.event {
            EventKind::RunStarted { nodes, .. } => {
                if !states.is_empty() {
                    return Err(LedgerError::Corrupt("duplicate run_started".to_owned()));
                }
                states.extend(nodes.iter().cloned().map(|id| (id, NodeState::Pending)));
            }
            EventKind::NodeStarted { node_id } => {
                states.insert(node_id.clone(), NodeState::Running);
                started_order.push(node_id.clone());
            }
            EventKind::ToolResponse { node_id, output } => {
                outputs.insert(node_id.clone(), output.clone());
            }
            EventKind::NodeSucceeded { node_id } => {
                if !outputs.contains_key(node_id) {
                    return Err(LedgerError::Corrupt(format!(
                        "node '{node_id}' succeeded without a recorded response"
                    )));
                }
                states.insert(node_id.clone(), NodeState::Succeeded);
                completion_order.push(node_id.clone());
            }
            EventKind::NodeDegraded { node_id, .. } => {
                if !outputs.contains_key(node_id) {
                    return Err(LedgerError::Corrupt(format!(
                        "node '{node_id}' degraded without a recorded response"
                    )));
                }
                states.insert(node_id.clone(), NodeState::Degraded);
                completion_order.push(node_id.clone());
            }
            EventKind::NodeFailed { node_id, error } => {
                states.insert(node_id.clone(), NodeState::Failed(error.clone()));
                completion_order.push(node_id.clone());
            }
            EventKind::NodeTimedOut { node_id } => {
                states.insert(node_id.clone(), NodeState::TimedOut);
                completion_order.push(node_id.clone());
            }
            EventKind::NodeBlocked { node_id } => {
                states.insert(node_id.clone(), NodeState::Blocked);
                completion_order.push(node_id.clone());
            }
            EventKind::NodeCancelled { node_id } => {
                states.insert(node_id.clone(), NodeState::Cancelled);
                completion_order.push(node_id.clone());
            }
            EventKind::NodeNeedsReplan { node_id, .. } => {
                states.insert(node_id.clone(), NodeState::NeedsReplan);
                completion_order.push(node_id.clone());
            }
            EventKind::RunCompleted { status } => terminal = Some(*status),
            EventKind::ToolCall { .. }
            | EventKind::VerifierResult { .. }
            | EventKind::Retry { .. }
            | EventKind::Replan { .. }
            | EventKind::Cancellation => {}
        }
    }
    let run_status =
        terminal.ok_or_else(|| LedgerError::Corrupt("missing run_completed".to_owned()))?;
    Ok(RunResult {
        status: run_status,
        states,
        outputs,
        started_order,
        completion_order,
        events: events.iter().map(|item| item.event.clone()).collect(),
    })
}

fn event_digest(sequence: u64, previous_digest: &str, event: &EventKind) -> String {
    canonical_digest(&serde_json::json!({
        "sequence": sequence,
        "previous_digest": previous_digest,
        "event": event,
    }))
}

fn now_ms() -> u64 {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    u64::try_from(millis).unwrap_or(u64::MAX)
}
