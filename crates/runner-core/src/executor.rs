use std::{collections::BTreeMap, time::Duration};

use async_trait::async_trait;
use futures::{FutureExt, StreamExt, future::BoxFuture, stream::FuturesUnordered};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;

use crate::{
    EventKind, FailurePolicy, Node, Plan, Provenance, ResolvedOutput, ToolError, ToolRegistry,
    ValidationError, VerificationEvidence, canonical_digest, resolve_inputs, validate_plan,
    verify_final, verify_node,
};

#[async_trait]
pub trait NodeRunner: Send + Sync {
    /// Execute one node after the scheduler has resolved its typed inputs.
    ///
    /// # Errors
    /// Returns a tool-level failure without applying retry or failure policy.
    async fn run_node(
        &self,
        plan: &Plan,
        node: &Node,
        inputs: &BTreeMap<String, Value>,
    ) -> Result<ResolvedOutput, ToolError>;
}

#[async_trait]
impl NodeRunner for ToolRegistry {
    async fn run_node(
        &self,
        plan: &Plan,
        node: &Node,
        inputs: &BTreeMap<String, Value>,
    ) -> Result<ResolvedOutput, ToolError> {
        self.execute(plan, node, inputs).await
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionMode {
    Sequential,
    #[default]
    Parallel,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "state", content = "detail", rename_all = "snake_case")]
pub enum NodeState {
    Pending,
    Running,
    Succeeded,
    Degraded,
    Failed(String),
    TimedOut,
    Cancelled,
    Blocked,
    NeedsReplan,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Succeeded,
    Failed,
    Cancelled,
    NeedsReplan,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct RunResult {
    pub status: RunStatus,
    pub states: BTreeMap<String, NodeState>,
    pub outputs: BTreeMap<String, ResolvedOutput>,
    pub started_order: Vec<String>,
    pub completion_order: Vec<String>,
    pub events: Vec<EventKind>,
}

#[derive(Debug, Error)]
pub enum ExecutionError {
    #[error(transparent)]
    Validation(#[from] ValidationError),
    #[error("validated graph could not make progress")]
    Deadlock,
}

pub struct Executor<'a> {
    runner: &'a dyn NodeRunner,
    mode: ExecutionMode,
    concurrency_override: Option<usize>,
}

type NodeFuture<'a> = BoxFuture<
    'a,
    (
        Node,
        BTreeMap<String, Value>,
        u32,
        Result<Result<ResolvedOutput, ToolError>, tokio::time::error::Elapsed>,
    ),
>;

enum FailureDecision {
    Retry,
    Degrade(Box<(ResolvedOutput, VerificationEvidence)>),
    NeedsReplan,
    Stop,
}

fn decide_failure(node: &Node, attempt: u32, reason: &str) -> FailureDecision {
    match node.failure_policy {
        FailurePolicy::Retry if attempt < node.retry_budget => FailureDecision::Retry,
        FailurePolicy::Degrade => {
            let Some(value) = node.degrade_value.clone() else {
                return FailureDecision::Stop;
            };
            let content_digest = canonical_digest(&value);
            let request_digest = canonical_digest(&serde_json::json!({
                "node_id": node.id,
                "degrade_reason": reason,
            }));
            let output = ResolvedOutput {
                value,
                provenance: Provenance {
                    node_id: node.id.clone(),
                    invocation_id: request_digest.clone(),
                    tool_name: "degrade_fallback".to_owned(),
                    request_digest,
                    response_digest: content_digest.clone(),
                    content_digest,
                },
            };
            let evidence = verify_node(node, &output);
            if evidence.accepted {
                FailureDecision::Degrade(Box::new((output, evidence)))
            } else {
                FailureDecision::Stop
            }
        }
        FailurePolicy::Replan => FailureDecision::NeedsReplan,
        FailurePolicy::Stop | FailurePolicy::Retry => FailureDecision::Stop,
    }
}

impl<'a> Executor<'a> {
    #[must_use]
    pub const fn new(runner: &'a dyn NodeRunner) -> Self {
        Self {
            runner,
            mode: ExecutionMode::Parallel,
            concurrency_override: None,
        }
    }

    #[must_use]
    pub const fn mode(mut self, mode: ExecutionMode) -> Self {
        self.mode = mode;
        self
    }

    #[must_use]
    pub const fn concurrency(mut self, limit: usize) -> Self {
        self.concurrency_override = Some(limit);
        self
    }

    /// Execute a validated plan until success, failure, or cancellation.
    ///
    /// # Errors
    /// Returns preflight validation failures or a scheduler deadlock.
    #[allow(clippy::too_many_lines)]
    pub async fn execute(
        &self,
        plan: &Plan,
        cancellation: CancellationToken,
    ) -> Result<RunResult, ExecutionError> {
        validate_plan(plan)?;
        let requested = self
            .concurrency_override
            .unwrap_or(plan.limits.max_concurrency);
        let concurrency = match self.mode {
            ExecutionMode::Sequential => 1,
            ExecutionMode::Parallel => requested.max(1),
        };
        let mut pending: BTreeMap<String, Node> = plan
            .nodes
            .iter()
            .cloned()
            .map(|node| (node.id.clone(), node))
            .collect();
        let mut states: BTreeMap<String, NodeState> = pending
            .keys()
            .cloned()
            .map(|id| (id, NodeState::Pending))
            .collect();
        let mut outputs = BTreeMap::new();
        let mut started_order = Vec::new();
        let mut completion_order = Vec::new();
        let mut events = vec![EventKind::RunStarted {
            plan_id: plan.id.clone(),
            mode: self.mode,
            nodes: plan.nodes.iter().map(|node| node.id.clone()).collect(),
        }];
        let mut attempts = BTreeMap::<String, u32>::new();
        let mut running: FuturesUnordered<NodeFuture<'_>> = FuturesUnordered::new();
        let mut stop_requested = false;

        while !pending.is_empty() || !running.is_empty() {
            if cancellation.is_cancelled() {
                events.push(EventKind::Cancellation);
                for id in pending.keys() {
                    states.insert(id.clone(), NodeState::Cancelled);
                    completion_order.push(id.clone());
                    events.push(EventKind::NodeCancelled {
                        node_id: id.clone(),
                    });
                }
                for (id, state) in &mut states {
                    if *state == NodeState::Running {
                        *state = NodeState::Cancelled;
                        completion_order.push(id.clone());
                        events.push(EventKind::NodeCancelled {
                            node_id: id.clone(),
                        });
                    }
                }
                events.push(EventKind::RunCompleted {
                    status: RunStatus::Cancelled,
                });
                return Ok(RunResult {
                    status: RunStatus::Cancelled,
                    states,
                    outputs,
                    started_order,
                    completion_order,
                    events,
                });
            }

            let blocked: Vec<_> = pending
                .iter()
                .filter(|(_, node)| {
                    node.dependencies.iter().any(|dependency| {
                        matches!(
                            states.get(dependency),
                            Some(
                                NodeState::Failed(_)
                                    | NodeState::TimedOut
                                    | NodeState::Cancelled
                                    | NodeState::Blocked
                                    | NodeState::NeedsReplan
                            )
                        )
                    })
                })
                .map(|(id, _)| id.clone())
                .collect();
            for id in blocked {
                pending.remove(&id);
                states.insert(id.clone(), NodeState::Blocked);
                completion_order.push(id.clone());
                events.push(EventKind::NodeBlocked { node_id: id });
            }

            if !stop_requested {
                let available = concurrency.saturating_sub(running.len());
                let ready: Vec<_> = pending
                    .iter()
                    .filter(|(_, node)| {
                        node.dependencies.iter().all(|dependency| {
                            matches!(
                                states.get(dependency),
                                Some(NodeState::Succeeded | NodeState::Degraded)
                            )
                        })
                    })
                    .take(available)
                    .map(|(id, _)| id.clone())
                    .collect();
                for id in ready {
                    let Some(node) = pending.remove(&id) else {
                        return Err(ExecutionError::Deadlock);
                    };
                    let inputs = match resolve_inputs(&node, &outputs) {
                        Ok(inputs) => inputs,
                        Err(error) => {
                            states.insert(id.clone(), NodeState::Failed(error.to_string()));
                            completion_order.push(id.clone());
                            events.push(EventKind::NodeFailed {
                                node_id: id,
                                error: error.to_string(),
                            });
                            stop_requested = true;
                            continue;
                        }
                    };
                    states.insert(id.clone(), NodeState::Running);
                    started_order.push(id.clone());
                    events.push(EventKind::NodeStarted {
                        node_id: id.clone(),
                    });
                    events.push(EventKind::ToolCall {
                        node_id: id.clone(),
                        tool: node.tool.clone(),
                        inputs: inputs.clone(),
                    });
                    let runner = self.runner;
                    let timeout_ms = node.timeout_ms;
                    let attempt = attempts.get(&id).copied().unwrap_or(0);
                    running.push(
                        async move {
                            let result = timeout(
                                Duration::from_millis(timeout_ms),
                                runner.run_node(plan, &node, &inputs),
                            )
                            .await;
                            (node, inputs, attempt, result)
                        }
                        .boxed(),
                    );
                }
            }

            if running.is_empty() {
                if pending.is_empty() || stop_requested {
                    break;
                }
                return Err(ExecutionError::Deadlock);
            }

            tokio::select! {
                () = cancellation.cancelled() => {},
                Some((node, _inputs, attempt, result)) = running.next() => {
                    let id = node.id.clone();
                    let mut failure = None;
                    let mut timed_out = false;
                    match result {
                        Ok(Ok(output)) => {
                            events.push(EventKind::ToolResponse {
                                node_id: id.clone(),
                                output: output.clone(),
                            });
                            let evidence = verify_node(&node, &output);
                            events.push(EventKind::VerifierResult {
                                node_id: Some(id.clone()),
                                accepted: evidence.accepted,
                                evidence: evidence.clone(),
                            });
                            if evidence.accepted {
                                outputs.insert(id.clone(), output);
                                states.insert(id.clone(), NodeState::Succeeded);
                                completion_order.push(id.clone());
                                events.push(EventKind::NodeSucceeded { node_id: id.clone() });
                            } else {
                                failure = Some(evidence.reason);
                            }
                        }
                        Ok(Err(error)) => {
                            failure = Some(error.to_string());
                        }
                        Err(_) => {
                            timed_out = true;
                            failure = Some(format!("node timed out after {} ms", node.timeout_ms));
                        }
                    }
                    if let Some(reason) = failure {
                        match decide_failure(&node, attempt, &reason) {
                            FailureDecision::Retry => {
                                let next_attempt = attempt.saturating_add(1);
                                attempts.insert(id.clone(), next_attempt);
                                states.insert(id.clone(), NodeState::Pending);
                                events.push(EventKind::Retry {
                                    node_id: id.clone(),
                                    attempt: next_attempt,
                                    reason,
                                });
                                pending.insert(id, node);
                            }
                            FailureDecision::Degrade(degraded) => {
                                let (output, evidence) = *degraded;
                                events.push(EventKind::ToolResponse {
                                    node_id: id.clone(),
                                    output: output.clone(),
                                });
                                events.push(EventKind::VerifierResult {
                                    node_id: Some(id.clone()),
                                    accepted: true,
                                    evidence,
                                });
                                outputs.insert(id.clone(), output);
                                states.insert(id.clone(), NodeState::Degraded);
                                completion_order.push(id.clone());
                                events.push(EventKind::NodeDegraded {
                                    node_id: id,
                                    reason,
                                });
                            }
                            FailureDecision::NeedsReplan => {
                                states.insert(id.clone(), NodeState::NeedsReplan);
                                completion_order.push(id.clone());
                                events.push(EventKind::NodeNeedsReplan {
                                    node_id: id,
                                    reason,
                                });
                                stop_requested = true;
                            }
                            FailureDecision::Stop => {
                                completion_order.push(id.clone());
                                if timed_out {
                                    states.insert(id.clone(), NodeState::TimedOut);
                                    events.push(EventKind::NodeTimedOut { node_id: id });
                                } else {
                                    states.insert(id.clone(), NodeState::Failed(reason.clone()));
                                    events.push(EventKind::NodeFailed {
                                        node_id: id,
                                        error: reason,
                                    });
                                }
                                stop_requested = true;
                            }
                        }
                    }
                }
            }
        }

        if stop_requested {
            for id in pending.keys() {
                states.insert(id.clone(), NodeState::Blocked);
                completion_order.push(id.clone());
                events.push(EventKind::NodeBlocked {
                    node_id: id.clone(),
                });
            }
        }
        let nodes_accepted = states
            .values()
            .all(|state| matches!(state, NodeState::Succeeded | NodeState::Degraded));
        let final_accepted = if nodes_accepted {
            if let Some(verifier) = &plan.final_verifier {
                let evidence = verify_final(verifier, &outputs, plan.final_output_schema.as_ref());
                events.push(EventKind::VerifierResult {
                    node_id: None,
                    accepted: evidence.accepted,
                    evidence: evidence.clone(),
                });
                evidence.accepted
            } else {
                true
            }
        } else {
            false
        };
        let run_status = if nodes_accepted && final_accepted {
            RunStatus::Succeeded
        } else if cancellation.is_cancelled() {
            RunStatus::Cancelled
        } else if states
            .values()
            .any(|state| *state == NodeState::NeedsReplan)
        {
            RunStatus::NeedsReplan
        } else {
            RunStatus::Failed
        };
        events.push(EventKind::RunCompleted { status: run_status });
        Ok(RunResult {
            status: run_status,
            states,
            outputs,
            started_order,
            completion_order,
            events,
        })
    }
}
