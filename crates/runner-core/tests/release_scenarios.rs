use std::collections::{BTreeMap, BTreeSet};

use async_trait::async_trait;
use runner_core::{
    AuthorityPolicy, EventKind, Executor, FailurePolicy, FixtureReplanner, Ledger, Node,
    NodeRunner, NodeState, PatchOperation, Plan, PlanLimits, PlanPatch, Provenance,
    ReplanningExecutor, ResolvedOutput, RunStatus, ToolError, ValueType, VerifierSpec,
    canonical_digest, plan_digest, validate_plan,
};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

struct ScenarioRunner;

#[async_trait]
impl NodeRunner for ScenarioRunner {
    async fn run_node(
        &self,
        _plan: &Plan,
        node: &Node,
        _inputs: &BTreeMap<String, Value>,
    ) -> Result<ResolvedOutput, ToolError> {
        if node.tool == "fail" {
            return Err(ToolError::Execution("scenario failure".to_owned()));
        }
        let value = json!(7);
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

fn node(tool: &str, failure_policy: FailurePolicy) -> Node {
    Node {
        id: "work".to_owned(),
        objective: "complete verified work".to_owned(),
        dependencies: vec![],
        inputs: BTreeMap::new(),
        output: ValueType::Number,
        output_schema: None,
        tool: tool.to_owned(),
        authority: BTreeSet::from(["compute".to_owned()]),
        timeout_ms: 1_000,
        retry_budget: 0,
        verifier: VerifierSpec::Equals { expected: json!(7) },
        failure_policy,
        degrade_value: None,
        immutable: false,
    }
}

fn plan(work: Node) -> Plan {
    Plan {
        version: "v1".to_owned(),
        id: "release-scenario".to_owned(),
        authority: AuthorityPolicy {
            tools: BTreeSet::from(["fail".to_owned(), "good".to_owned()]),
            capabilities: BTreeSet::from(["compute".to_owned()]),
        },
        limits: PlanLimits {
            max_concurrency: 1,
            max_replans: 1,
            max_node_growth: 0,
            max_replan_wall_time_ms: 5_000,
        },
        nodes: vec![work],
        final_verifier: None,
        final_output_schema: None,
    }
}

#[tokio::test]
async fn degraded_run_is_verified_persisted_and_replayed() {
    let mut work = node("fail", FailurePolicy::Degrade);
    work.degrade_value = Some(json!(7));
    let plan = plan(work);
    let result = Executor::new(&ScenarioRunner)
        .execute(&plan, CancellationToken::new())
        .await
        .expect("degraded execution");
    assert_eq!(result.status, RunStatus::Succeeded);
    assert_eq!(result.states["work"], NodeState::Degraded);
    assert!(
        result
            .events
            .iter()
            .any(|event| matches!(event, EventKind::VerifierResult { accepted: true, .. }))
    );

    let directory = tempfile::tempdir().expect("temporary directory");
    let mut ledger = Ledger::open(directory.path().join("runs.db")).expect("ledger");
    let run_id = ledger
        .store_run(&plan, runner_core::ExecutionMode::Parallel, &result)
        .expect("store degraded run");
    assert_eq!(ledger.replay(&run_id).expect("replay degraded run"), result);
}

#[tokio::test]
async fn bounded_replan_repairs_the_failed_node() {
    let plan = plan(node("fail", FailurePolicy::Replan));
    let patch = PlanPatch {
        base_plan_digest: plan_digest(&plan).expect("plan digest"),
        operations: vec![PatchOperation::Replace {
            node: node("good", FailurePolicy::Stop),
        }],
    };
    let replanner = FixtureReplanner::new([patch]);
    let result = ReplanningExecutor::new(&ScenarioRunner, &replanner)
        .execute(&plan, CancellationToken::new())
        .await
        .expect("replanned execution");
    assert_eq!(result.status, RunStatus::Succeeded);
    assert_eq!(result.outputs["work"].value, json!(7));
    assert!(
        result
            .events
            .iter()
            .any(|event| matches!(event, EventKind::Replan { .. }))
    );
}

#[tokio::test]
async fn rejected_plan_never_reaches_execution() {
    let mut rejected = plan(node("good", FailurePolicy::Stop));
    rejected.nodes[0].dependencies.push("work".to_owned());
    assert!(validate_plan(&rejected).is_err());
    assert!(
        Executor::new(&ScenarioRunner)
            .execute(&rejected, CancellationToken::new())
            .await
            .is_err()
    );
}
