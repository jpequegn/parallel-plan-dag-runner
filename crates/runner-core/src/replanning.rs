use std::{
    collections::{BTreeMap, BTreeSet, HashSet, VecDeque},
    sync::Mutex,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;

use crate::{
    EventKind, ExecutionError, ExecutionMode, Executor, Node, NodeRunner, NodeState, Plan,
    ResolvedOutput, RunResult, RunStatus, VerificationEvidence, canonical_digest, validate_plan,
};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ReplanRequest {
    pub plan_digest: String,
    pub replan_index: u32,
    pub failed_node: Node,
    pub dependencies: Vec<Node>,
    pub dependency_outputs: BTreeMap<String, ResolvedOutput>,
    pub verifier_evidence: Vec<VerificationEvidence>,
    pub mutable_scope: BTreeSet<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct PlanPatch {
    pub base_plan_digest: String,
    pub operations: Vec<PatchOperation>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "operation", rename_all = "snake_case")]
pub enum PatchOperation {
    Add { node: Node },
    Replace { node: Node },
    Remove { node_id: String },
}

#[async_trait]
pub trait Replanner: Send + Sync {
    /// Propose a graph patch using only the bounded failure request.
    ///
    /// # Errors
    /// Returns a provider error without changing the plan.
    async fn replan(&self, request: ReplanRequest) -> Result<PlanPatch, String>;
}

pub struct FixtureReplanner {
    patches: Mutex<VecDeque<PlanPatch>>,
}

impl FixtureReplanner {
    #[must_use]
    pub fn new(patches: impl IntoIterator<Item = PlanPatch>) -> Self {
        Self {
            patches: Mutex::new(patches.into_iter().collect()),
        }
    }
}

#[async_trait]
impl Replanner for FixtureReplanner {
    async fn replan(&self, _request: ReplanRequest) -> Result<PlanPatch, String> {
        self.patches
            .lock()
            .map_err(|_| "fixture replanner lock is poisoned".to_owned())?
            .pop_front()
            .ok_or_else(|| "fixture replanner has no remaining patch".to_owned())
    }
}

#[derive(Debug, Error)]
pub enum ReplanError {
    #[error(transparent)]
    Execution(#[from] ExecutionError),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error("replanner failed: {0}")]
    Provider(String),
    #[error("replanning exceeded the count budget of {0}")]
    CountLimit(u32),
    #[error("replanning exceeded its wall-time budget")]
    WallTimeLimit,
    #[error("patch is stale: expected base {expected}, found {actual}")]
    StalePatch { expected: String, actual: String },
    #[error("patch target '{0}' is outside the mutable scope")]
    OutOfScope(String),
    #[error("patch would change completed immutable node '{0}'")]
    ImmutableCompleted(String),
    #[error("patch refers to missing or duplicate node '{0}'")]
    InvalidTarget(String),
    #[error("patch exceeds maximum node growth")]
    NodeGrowth,
    #[error("patched plan failed preflight validation: {0}")]
    InvalidPatch(String),
    #[error("replanner produced a repeated plan")]
    Loop,
    #[error("needs-replan result did not identify a failed node")]
    MissingFailedNode,
}

pub struct ReplanningExecutor<'a> {
    runner: &'a dyn NodeRunner,
    replanner: &'a dyn Replanner,
    mode: ExecutionMode,
}

impl<'a> ReplanningExecutor<'a> {
    #[must_use]
    pub const fn new(runner: &'a dyn NodeRunner, replanner: &'a dyn Replanner) -> Self {
        Self {
            runner,
            replanner,
            mode: ExecutionMode::Parallel,
        }
    }

    #[must_use]
    pub const fn mode(mut self, mode: ExecutionMode) -> Self {
        self.mode = mode;
        self
    }

    /// Execute a plan and apply bounded, fully revalidated patches when requested.
    ///
    /// # Errors
    /// Returns execution, provider, budget, scope, immutability, loop, or validation failures.
    #[allow(clippy::too_many_lines)]
    pub async fn execute(
        &self,
        original: &Plan,
        cancellation: CancellationToken,
    ) -> Result<RunResult, ReplanError> {
        validate_plan(original).map_err(|error| ReplanError::InvalidPatch(error.to_string()))?;
        let started = Instant::now();
        let budget = Duration::from_millis(original.limits.max_replan_wall_time_ms);
        let max_nodes = original.nodes.len() + original.limits.max_node_growth;
        let mut current = original.clone();
        let mut seeded_outputs = BTreeMap::new();
        let mut all_events = Vec::new();
        let mut all_started = Vec::new();
        let mut all_completed = Vec::new();
        let mut replan_index = 0;
        let mut seen_plans = HashSet::from([plan_digest(&current)?]);

        loop {
            if started.elapsed() >= budget {
                return Err(ReplanError::WallTimeLimit);
            }
            let mut result = Executor::new(self.runner)
                .mode(self.mode)
                .execute_seeded(&current, cancellation.clone(), seeded_outputs)
                .await?;
            let needs_replan = result.status == RunStatus::NeedsReplan;
            let mut round_events = std::mem::take(&mut result.events);
            if !all_events.is_empty()
                && matches!(round_events.first(), Some(EventKind::RunStarted { .. }))
            {
                round_events.remove(0);
            }
            if needs_replan && matches!(round_events.last(), Some(EventKind::RunCompleted { .. })) {
                round_events.pop();
            }
            all_events.extend(round_events);
            all_started.append(&mut result.started_order);
            all_completed.append(&mut result.completion_order);
            result.events.clone_from(&all_events);

            if !needs_replan {
                result.events = all_events;
                result.started_order = all_started;
                result.completion_order = all_completed;
                return Ok(result);
            }
            if replan_index >= original.limits.max_replans {
                return Err(ReplanError::CountLimit(original.limits.max_replans));
            }
            let failed_id = result
                .states
                .iter()
                .find_map(|(id, state)| (*state == NodeState::NeedsReplan).then(|| id.clone()))
                .ok_or(ReplanError::MissingFailedNode)?;
            let request = build_request(&current, &result, &failed_id, replan_index)?;
            let remaining = budget.saturating_sub(started.elapsed());
            let patch = timeout(remaining, self.replanner.replan(request))
                .await
                .map_err(|_| ReplanError::WallTimeLimit)?
                .map_err(ReplanError::Provider)?;
            let patch_digest = canonical_digest(&serde_json::to_value(&patch)?);
            let completed: BTreeSet<_> = result
                .states
                .iter()
                .filter(|(_, state)| matches!(state, NodeState::Succeeded | NodeState::Degraded))
                .map(|(id, _)| id.clone())
                .collect();
            let applied = apply_patch(&current, patch, &failed_id, &completed, max_nodes)?;
            let digest = plan_digest(&applied.plan)?;
            if !seen_plans.insert(digest) {
                return Err(ReplanError::Loop);
            }
            all_events.push(EventKind::Replan {
                failed_node: failed_id,
                patch_digest,
                removed_nodes: applied.removed_nodes,
            });
            seeded_outputs = result.outputs;
            seeded_outputs.retain(|id, _| !applied.invalidated.contains(id));
            current = applied.plan;
            replan_index += 1;
        }
    }
}

struct AppliedPatch {
    plan: Plan,
    invalidated: BTreeSet<String>,
    removed_nodes: Vec<String>,
}

fn apply_patch(
    plan: &Plan,
    patch: PlanPatch,
    failed_id: &str,
    completed: &BTreeSet<String>,
    max_nodes: usize,
) -> Result<AppliedPatch, ReplanError> {
    let expected = plan_digest(plan)?;
    if patch.base_plan_digest != expected {
        return Err(ReplanError::StalePatch {
            expected,
            actual: patch.base_plan_digest,
        });
    }
    let mutable_scope = descendants(plan, failed_id);
    let original_nodes: BTreeMap<_, _> = plan
        .nodes
        .iter()
        .cloned()
        .map(|node| (node.id.clone(), node))
        .collect();
    let mut patched_nodes = original_nodes.clone();
    let mut operated = HashSet::new();
    for operation in patch.operations {
        let target = match &operation {
            PatchOperation::Add { node } | PatchOperation::Replace { node } => node.id.clone(),
            PatchOperation::Remove { node_id } => node_id.clone(),
        };
        if !operated.insert(target.clone()) {
            return Err(ReplanError::InvalidTarget(target));
        }
        if completed.contains(&target)
            && original_nodes
                .get(&target)
                .is_some_and(|node| node.immutable)
        {
            return Err(ReplanError::ImmutableCompleted(target));
        }
        match operation {
            PatchOperation::Add { node } => {
                if patched_nodes.contains_key(&node.id) {
                    return Err(ReplanError::InvalidTarget(node.id));
                }
                patched_nodes.insert(node.id.clone(), node);
            }
            PatchOperation::Replace { node } => {
                if !mutable_scope.contains(&node.id) || !patched_nodes.contains_key(&node.id) {
                    return Err(ReplanError::OutOfScope(node.id));
                }
                patched_nodes.insert(node.id.clone(), node);
            }
            PatchOperation::Remove { node_id } => {
                if !mutable_scope.contains(&node_id) {
                    return Err(ReplanError::OutOfScope(node_id));
                }
                if patched_nodes.remove(&node_id).is_none() {
                    return Err(ReplanError::InvalidTarget(node_id));
                }
            }
        }
    }
    if patched_nodes.len() > max_nodes {
        return Err(ReplanError::NodeGrowth);
    }
    let mut patched = plan.clone();
    patched.nodes = patched_nodes.values().cloned().collect();
    validate_plan(&patched).map_err(|error| ReplanError::InvalidPatch(error.to_string()))?;

    let changed: BTreeSet<_> = original_nodes
        .keys()
        .chain(patched_nodes.keys())
        .filter(|id| original_nodes.get(*id) != patched_nodes.get(*id))
        .cloned()
        .collect();
    let mut invalidated = changed.clone();
    for id in &changed {
        invalidated.extend(descendants(plan, id));
        invalidated.extend(descendants(&patched, id));
    }
    if let Some(id) = invalidated.iter().find(|id| {
        completed.contains(*id) && original_nodes.get(*id).is_some_and(|node| node.immutable)
    }) {
        return Err(ReplanError::ImmutableCompleted(id.clone()));
    }
    let removed_nodes = original_nodes
        .keys()
        .filter(|id| !patched_nodes.contains_key(*id))
        .cloned()
        .collect();
    Ok(AppliedPatch {
        plan: patched,
        invalidated,
        removed_nodes,
    })
}

fn build_request(
    plan: &Plan,
    result: &RunResult,
    failed_id: &str,
    replan_index: u32,
) -> Result<ReplanRequest, ReplanError> {
    let failed_node = plan
        .nodes
        .iter()
        .find(|node| node.id == failed_id)
        .cloned()
        .ok_or(ReplanError::MissingFailedNode)?;
    let dependencies: Vec<_> = failed_node
        .dependencies
        .iter()
        .filter_map(|id| plan.nodes.iter().find(|node| node.id == *id).cloned())
        .collect();
    let dependency_outputs = failed_node
        .dependencies
        .iter()
        .filter_map(|id| {
            result
                .outputs
                .get(id)
                .cloned()
                .map(|output| (id.clone(), output))
        })
        .collect();
    let verifier_evidence = result
        .events
        .iter()
        .filter_map(|event| match event {
            EventKind::VerifierResult {
                node_id: Some(node_id),
                evidence,
                ..
            } if node_id == failed_id => Some(evidence.clone()),
            _ => None,
        })
        .collect();
    Ok(ReplanRequest {
        plan_digest: plan_digest(plan)?,
        replan_index,
        failed_node,
        dependencies,
        dependency_outputs,
        verifier_evidence,
        mutable_scope: descendants(plan, failed_id),
    })
}

fn descendants(plan: &Plan, root: &str) -> BTreeSet<String> {
    let mut found = BTreeSet::from([root.to_owned()]);
    loop {
        let additions: Vec<_> = plan
            .nodes
            .iter()
            .filter(|node| {
                !found.contains(&node.id)
                    && node
                        .dependencies
                        .iter()
                        .any(|dependency| found.contains(dependency))
            })
            .map(|node| node.id.clone())
            .collect();
        if additions.is_empty() {
            break;
        }
        found.extend(additions);
    }
    found
}

/// Compute the immutable digest a patch must reference.
///
/// # Errors
/// Returns a JSON serialization error if the plan cannot be encoded.
pub fn plan_digest(plan: &Plan) -> Result<String, serde_json::Error> {
    Ok(canonical_digest(&serde_json::to_value(plan)?))
}
