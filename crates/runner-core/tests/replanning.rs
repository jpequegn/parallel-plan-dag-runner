use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Mutex,
};

use async_trait::async_trait;
use runner_core::{
    AuthorityPolicy, EventKind, FailurePolicy, FixtureReplanner, Ledger, Node, NodeRunner,
    PatchOperation, Plan, PlanLimits, PlanPatch, Provenance, ReplanError, ReplanRequest, Replanner,
    ReplanningExecutor, ResolvedOutput, RunStatus, ToolError, ValueType, VerifierSpec,
    canonical_digest, plan_digest,
};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

#[derive(Default)]
struct ToolRunner {
    calls: Mutex<BTreeMap<String, usize>>,
}

#[async_trait]
impl NodeRunner for ToolRunner {
    async fn run_node(
        &self,
        _plan: &Plan,
        node: &Node,
        _inputs: &BTreeMap<String, Value>,
    ) -> Result<ResolvedOutput, ToolError> {
        *self
            .calls
            .lock()
            .expect("calls lock")
            .entry(node.tool.clone())
            .or_default() += 1;
        let value = match node.tool.as_str() {
            "root" => json!(10),
            "bad" => json!(1),
            "good" => json!(2),
            other => {
                return Err(ToolError::Execution(format!(
                    "unknown fixture tool {other}"
                )));
            }
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

struct CapturingReplanner {
    request: Mutex<Option<ReplanRequest>>,
    patch: PlanPatch,
}

#[async_trait]
impl Replanner for CapturingReplanner {
    async fn replan(&self, request: ReplanRequest) -> Result<PlanPatch, String> {
        *self.request.lock().map_err(|_| "request lock".to_owned())? = Some(request);
        Ok(self.patch.clone())
    }
}

fn node(id: &str, tool: &str, dependencies: &[&str], immutable: bool) -> Node {
    Node {
        id: id.to_owned(),
        objective: format!("execute {id}"),
        dependencies: dependencies.iter().map(ToString::to_string).collect(),
        inputs: dependencies
            .iter()
            .map(|dependency| {
                (
                    (*dependency).to_owned(),
                    runner_core::InputSpec::Reference {
                        node: (*dependency).to_owned(),
                        path: None,
                        value_type: ValueType::Number,
                    },
                )
            })
            .collect(),
        output: ValueType::Number,
        output_schema: None,
        tool: tool.to_owned(),
        authority: BTreeSet::from(["compute".to_owned()]),
        timeout_ms: 1_000,
        retry_budget: 0,
        verifier: if id == "work" {
            VerifierSpec::Equals { expected: json!(2) }
        } else {
            VerifierSpec::Always
        },
        failure_policy: if id == "work" {
            FailurePolicy::Replan
        } else {
            FailurePolicy::Stop
        },
        degrade_value: None,
        immutable,
    }
}

fn test_plan() -> Plan {
    Plan {
        version: "v1".to_owned(),
        id: "replanning".to_owned(),
        authority: AuthorityPolicy {
            tools: BTreeSet::from(["root".to_owned(), "bad".to_owned(), "good".to_owned()]),
            capabilities: BTreeSet::from(["compute".to_owned()]),
        },
        limits: PlanLimits {
            max_concurrency: 2,
            max_replans: 2,
            max_node_growth: 1,
            max_replan_wall_time_ms: 5_000,
        },
        nodes: vec![
            node("root", "root", &[], true),
            node("work", "bad", &["root"], false),
        ],
        final_verifier: None,
        final_output_schema: None,
    }
}

fn repair_patch(plan: &Plan) -> PlanPatch {
    PlanPatch {
        base_plan_digest: plan_digest(plan).expect("plan digest"),
        operations: vec![PatchOperation::Replace {
            node: node("work", "good", &["root"], false),
        }],
    }
}

#[tokio::test]
async fn repairs_a_failed_node_without_rerunning_completed_dependencies() {
    let plan = test_plan();
    let replanner = CapturingReplanner {
        request: Mutex::new(None),
        patch: repair_patch(&plan),
    };
    let runner = ToolRunner::default();
    let result = ReplanningExecutor::new(&runner, &replanner)
        .execute(&plan, CancellationToken::new())
        .await
        .expect("replanned run");
    assert_eq!(result.status, RunStatus::Succeeded);
    assert_eq!(result.outputs["work"].value, json!(2));
    let calls = runner.calls.lock().expect("calls lock");
    assert_eq!(calls["root"], 1);
    assert_eq!(calls["bad"], 1);
    assert_eq!(calls["good"], 1);
    drop(calls);
    assert!(
        result
            .events
            .iter()
            .any(|event| matches!(event, EventKind::Replan { .. }))
    );
    let request = replanner.request.lock().expect("request lock");
    let request = request.as_ref().expect("captured request");
    assert_eq!(request.failed_node.id, "work");
    assert_eq!(
        request
            .dependencies
            .iter()
            .map(|node| node.id.as_str())
            .collect::<Vec<_>>(),
        ["root"]
    );
    assert_eq!(request.dependency_outputs.len(), 1);
    assert!(!request.verifier_evidence.is_empty());

    let directory = tempfile::tempdir().expect("temporary directory");
    let mut ledger = Ledger::open(directory.path().join("runs.db")).expect("ledger");
    let run_id = ledger
        .store_run(&plan, runner_core::ExecutionMode::Parallel, &result)
        .expect("store replanned run");
    assert_eq!(
        ledger.replay(&run_id).expect("replay replanned run"),
        result
    );
}

#[tokio::test]
async fn rejects_mutation_of_completed_immutable_nodes() {
    let plan = test_plan();
    let patch = PlanPatch {
        base_plan_digest: plan_digest(&plan).expect("digest"),
        operations: vec![PatchOperation::Replace {
            node: node("root", "good", &[], true),
        }],
    };
    let replanner = FixtureReplanner::new([patch]);
    let runner = ToolRunner::default();
    let error = ReplanningExecutor::new(&runner, &replanner)
        .execute(&plan, CancellationToken::new())
        .await
        .expect_err("immutable change must fail");
    assert!(matches!(error, ReplanError::ImmutableCompleted(id) if id == "root"));
}

#[tokio::test]
async fn rejects_stale_authority_broadening_and_cycles() {
    let plan = test_plan();
    let runner = ToolRunner::default();

    let stale = PlanPatch {
        base_plan_digest: "stale".to_owned(),
        operations: vec![],
    };
    let error = ReplanningExecutor::new(&runner, &FixtureReplanner::new([stale]))
        .execute(&plan, CancellationToken::new())
        .await
        .expect_err("stale patch");
    assert!(matches!(error, ReplanError::StalePatch { .. }));

    let mut escalated = node("work", "good", &["root"], false);
    escalated.authority.insert("shell".to_owned());
    let patch = PlanPatch {
        base_plan_digest: plan_digest(&plan).expect("digest"),
        operations: vec![PatchOperation::Replace { node: escalated }],
    };
    let error = ReplanningExecutor::new(&runner, &FixtureReplanner::new([patch]))
        .execute(&plan, CancellationToken::new())
        .await
        .expect_err("authority escalation");
    assert!(matches!(error, ReplanError::InvalidPatch(_)));

    let patch = PlanPatch {
        base_plan_digest: plan_digest(&plan).expect("digest"),
        operations: vec![PatchOperation::Replace {
            node: node("work", "good", &["work"], false),
        }],
    };
    let error = ReplanningExecutor::new(&runner, &FixtureReplanner::new([patch]))
        .execute(&plan, CancellationToken::new())
        .await
        .expect_err("cycle");
    assert!(matches!(error, ReplanError::InvalidPatch(_)));
}

#[tokio::test]
async fn terminates_replan_loops_and_count_exhaustion() {
    let plan = test_plan();
    let identical = PlanPatch {
        base_plan_digest: plan_digest(&plan).expect("digest"),
        operations: vec![PatchOperation::Replace {
            node: node("work", "bad", &["root"], false),
        }],
    };
    let runner = ToolRunner::default();
    let error = ReplanningExecutor::new(&runner, &FixtureReplanner::new([identical]))
        .execute(&plan, CancellationToken::new())
        .await
        .expect_err("loop");
    assert!(matches!(error, ReplanError::Loop));

    let mut no_replans = plan.clone();
    no_replans.limits.max_replans = 0;
    let error = ReplanningExecutor::new(&runner, &FixtureReplanner::new([]))
        .execute(&no_replans, CancellationToken::new())
        .await
        .expect_err("count limit");
    assert!(matches!(error, ReplanError::CountLimit(0)));
}
