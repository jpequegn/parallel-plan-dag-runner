use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use async_trait::async_trait;
use runner_core::{
    AuthorityPolicy, ExecutionMode, Executor, FailurePolicy, InputSpec, Node, NodeRunner,
    NodeState, Plan, PlanLimits, Provenance, ResolvedOutput, RunStatus, ToolError, ValueType,
    VerifierSpec, canonical_digest,
};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

struct ControlledRunner {
    delay: Duration,
    active: AtomicUsize,
    max_active: AtomicUsize,
    calls: Mutex<Vec<String>>,
}

impl ControlledRunner {
    fn new(delay_ms: u64) -> Self {
        Self {
            delay: Duration::from_millis(delay_ms),
            active: AtomicUsize::new(0),
            max_active: AtomicUsize::new(0),
            calls: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl NodeRunner for ControlledRunner {
    async fn run_node(
        &self,
        _plan: &Plan,
        node: &Node,
        inputs: &BTreeMap<String, Value>,
    ) -> Result<ResolvedOutput, ToolError> {
        let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.max_active.fetch_max(active, Ordering::SeqCst);
        self.calls.lock().expect("calls lock").push(node.id.clone());
        tokio::time::sleep(self.delay).await;
        self.active.fetch_sub(1, Ordering::SeqCst);
        let value = if inputs.is_empty() {
            json!({"node": node.id})
        } else {
            json!(inputs)
        };
        let digest = canonical_digest(&value);
        Ok(ResolvedOutput {
            value,
            provenance: Provenance {
                node_id: node.id.clone(),
                invocation_id: digest.clone(),
                tool_name: node.tool.clone(),
                request_digest: digest.clone(),
                response_digest: digest.clone(),
                content_digest: digest,
            },
        })
    }
}

fn node(id: &str, dependencies: &[&str], timeout_ms: u64) -> Node {
    let mut inputs = BTreeMap::new();
    for dependency in dependencies {
        inputs.insert(
            (*dependency).to_owned(),
            InputSpec::Reference {
                node: (*dependency).to_owned(),
                path: None,
                value_type: ValueType::Object,
            },
        );
    }
    Node {
        id: id.to_owned(),
        objective: format!("run {id}"),
        dependencies: dependencies.iter().map(ToString::to_string).collect(),
        inputs,
        output: ValueType::Object,
        tool: "controlled".to_owned(),
        authority: BTreeSet::from(["compute".to_owned()]),
        timeout_ms,
        retry_budget: 0,
        verifier: VerifierSpec::Always,
        failure_policy: FailurePolicy::Stop,
        immutable: false,
    }
}

fn diamond_plan(timeout_ms: u64) -> Plan {
    Plan {
        version: "v1".to_owned(),
        id: "diamond".to_owned(),
        authority: AuthorityPolicy {
            tools: BTreeSet::from(["controlled".to_owned()]),
            capabilities: BTreeSet::from(["compute".to_owned()]),
        },
        limits: PlanLimits {
            max_concurrency: 2,
            max_replans: 0,
            max_node_growth: 0,
        },
        nodes: vec![
            node("left", &[], timeout_ms),
            node("right", &[], timeout_ms),
            node("join", &["left", "right"], timeout_ms),
        ],
        final_verifier: None,
    }
}

#[tokio::test]
async fn honors_concurrency_cap_and_dependencies() {
    let runner = ControlledRunner::new(20);
    let result = Executor::new(&runner)
        .concurrency(2)
        .execute(&diamond_plan(1_000), CancellationToken::new())
        .await
        .expect("execute");
    assert_eq!(result.status, RunStatus::Succeeded);
    assert_eq!(runner.max_active.load(Ordering::SeqCst), 2);
    let join_start = result
        .started_order
        .iter()
        .position(|id| id == "join")
        .expect("join");
    assert_eq!(join_start, 2);
    assert_eq!(result.outputs.len(), 3);
    let calls = runner.calls.lock().expect("calls lock");
    assert_eq!(calls.iter().filter(|id| id.as_str() == "left").count(), 1);
}

#[tokio::test]
async fn sequential_and_parallel_outputs_are_equivalent() {
    let plan = diamond_plan(1_000);
    let sequential_runner = ControlledRunner::new(1);
    let parallel_runner = ControlledRunner::new(1);
    let sequential = Executor::new(&sequential_runner)
        .mode(ExecutionMode::Sequential)
        .execute(&plan, CancellationToken::new())
        .await
        .expect("sequential");
    let parallel = Executor::new(&parallel_runner)
        .execute(&plan, CancellationToken::new())
        .await
        .expect("parallel");
    assert_eq!(sequential.outputs, parallel.outputs);
    assert_eq!(sequential_runner.max_active.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn timeout_blocks_dependents() {
    let runner = ControlledRunner::new(40);
    let result = Executor::new(&runner)
        .execute(&diamond_plan(5), CancellationToken::new())
        .await
        .expect("execute");
    assert_eq!(result.status, RunStatus::Failed);
    assert_eq!(result.states["left"], NodeState::TimedOut);
    assert_eq!(result.states["join"], NodeState::Blocked);
}

#[tokio::test]
async fn pre_cancelled_run_starts_no_nodes() {
    let runner = Arc::new(ControlledRunner::new(20));
    let cancellation = CancellationToken::new();
    cancellation.cancel();
    let result = Executor::new(runner.as_ref())
        .execute(&diamond_plan(1_000), cancellation)
        .await
        .expect("cancelled result");
    assert_eq!(result.status, RunStatus::Cancelled);
    assert!(result.started_order.is_empty());
    assert!(
        result
            .states
            .values()
            .all(|state| *state == NodeState::Cancelled)
    );
}
