use std::{
    collections::{BTreeMap, BTreeSet},
    sync::atomic::{AtomicUsize, Ordering},
};

use async_trait::async_trait;
use runner_core::{
    AuthorityPolicy, EventKind, Executor, FailurePolicy, Node, NodeRunner, NodeState, Plan,
    PlanLimits, Provenance, ResolvedOutput, RunStatus, ToolError, ValueType, VerifierSpec,
    canonical_digest, verify_node,
};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

struct ScriptedRunner {
    failures: usize,
    calls: AtomicUsize,
    value: Value,
}

#[async_trait]
impl NodeRunner for ScriptedRunner {
    async fn run_node(
        &self,
        _plan: &Plan,
        node: &Node,
        _inputs: &BTreeMap<String, Value>,
    ) -> Result<ResolvedOutput, ToolError> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        if call < self.failures {
            return Err(ToolError::Execution(format!("scripted failure {call}")));
        }
        Ok(output(node, self.value.clone(), "scripted"))
    }
}

fn output(node: &Node, value: Value, tool: &str) -> ResolvedOutput {
    let digest = canonical_digest(&value);
    ResolvedOutput {
        value,
        provenance: Provenance {
            node_id: node.id.clone(),
            invocation_id: digest.clone(),
            tool_name: tool.to_owned(),
            request_digest: digest.clone(),
            response_digest: digest.clone(),
            content_digest: digest,
        },
    }
}

fn node(verifier: VerifierSpec, policy: FailurePolicy) -> Node {
    Node {
        id: "work".to_owned(),
        objective: "verify work".to_owned(),
        dependencies: vec![],
        inputs: BTreeMap::new(),
        output: ValueType::Number,
        output_schema: None,
        tool: "scripted".to_owned(),
        authority: BTreeSet::from(["compute".to_owned()]),
        timeout_ms: 1_000,
        retry_budget: 0,
        verifier,
        failure_policy: policy,
        degrade_value: None,
        immutable: false,
    }
}

fn plan(node: Node) -> Plan {
    Plan {
        version: "v1".to_owned(),
        id: "verification".to_owned(),
        authority: AuthorityPolicy {
            tools: BTreeSet::from(["scripted".to_owned()]),
            capabilities: BTreeSet::from(["compute".to_owned()]),
        },
        limits: PlanLimits::default(),
        nodes: vec![node],
        final_verifier: None,
        final_output_schema: None,
    }
}

#[test]
fn deterministic_verifiers_return_versioned_evidence() {
    let mut test_node = node(
        VerifierSpec::Equals {
            expected: json!(42),
        },
        FailurePolicy::Stop,
    );
    let equality = verify_node(&test_node, &output(&test_node, json!(42), "fixture"));
    assert!(equality.accepted);
    assert_eq!(equality.version, "v1");

    test_node.verifier = VerifierSpec::NumericRange {
        minimum: Some(40.0),
        maximum: Some(44.0),
    };
    assert!(verify_node(&test_node, &output(&test_node, json!(42), "fixture")).accepted);

    test_node.output = ValueType::Object;
    test_node.verifier = VerifierSpec::Expression {
        expression: "score >= 0.8 && approved == true".to_owned(),
    };
    let object = json!({"score": 0.9, "approved": true});
    assert!(verify_node(&test_node, &output(&test_node, object, "fixture")).accepted);

    test_node.verifier = VerifierSpec::JsonSchema;
    test_node.output_schema = Some(json!({
        "type": "object",
        "required": ["score"],
        "properties": {"score": {"type": "number", "minimum": 0.8}}
    }));
    assert!(
        verify_node(
            &test_node,
            &output(&test_node, json!({"score": 0.9}), "fixture")
        )
        .accepted
    );
    assert!(
        !verify_node(
            &test_node,
            &output(&test_node, json!({"score": 0.4}), "fixture")
        )
        .accepted
    );
}

#[tokio::test]
async fn retries_within_budget_and_records_each_decision() {
    let mut retry_node = node(VerifierSpec::Always, FailurePolicy::Retry);
    retry_node.retry_budget = 2;
    let plan = plan(retry_node);
    let runner = ScriptedRunner {
        failures: 2,
        calls: AtomicUsize::new(0),
        value: json!(42),
    };
    let result = Executor::new(&runner)
        .execute(&plan, CancellationToken::new())
        .await
        .expect("execute");
    assert_eq!(result.status, RunStatus::Succeeded);
    assert_eq!(runner.calls.load(Ordering::SeqCst), 3);
    assert_eq!(
        result
            .events
            .iter()
            .filter(|event| matches!(event, EventKind::Retry { .. }))
            .count(),
        2
    );
}

#[tokio::test]
async fn exhausted_retry_budget_stops_the_run() {
    let mut retry_node = node(VerifierSpec::Always, FailurePolicy::Retry);
    retry_node.retry_budget = 2;
    let plan = plan(retry_node);
    let runner = ScriptedRunner {
        failures: 10,
        calls: AtomicUsize::new(0),
        value: json!(42),
    };
    let result = Executor::new(&runner)
        .execute(&plan, CancellationToken::new())
        .await
        .expect("execute");
    assert_eq!(result.status, RunStatus::Failed);
    assert_eq!(runner.calls.load(Ordering::SeqCst), 3);
    assert!(matches!(result.states["work"], NodeState::Failed(_)));
}

#[tokio::test]
async fn degrade_requires_an_explicit_accepted_fallback() {
    let mut degraded_node = node(
        VerifierSpec::Equals { expected: json!(7) },
        FailurePolicy::Degrade,
    );
    degraded_node.degrade_value = Some(json!(7));
    let plan = plan(degraded_node);
    let runner = ScriptedRunner {
        failures: 1,
        calls: AtomicUsize::new(0),
        value: json!(42),
    };
    let result = Executor::new(&runner)
        .execute(&plan, CancellationToken::new())
        .await
        .expect("execute");
    assert_eq!(result.status, RunStatus::Succeeded);
    assert_eq!(result.states["work"], NodeState::Degraded);
    assert_eq!(result.outputs["work"].value, json!(7));
}

#[tokio::test]
async fn replan_policy_returns_an_explicit_handoff_state() {
    let plan = plan(node(VerifierSpec::Always, FailurePolicy::Replan));
    let runner = ScriptedRunner {
        failures: 1,
        calls: AtomicUsize::new(0),
        value: json!(42),
    };
    let result = Executor::new(&runner)
        .execute(&plan, CancellationToken::new())
        .await
        .expect("execute");
    assert_eq!(result.status, RunStatus::NeedsReplan);
    assert_eq!(result.states["work"], NodeState::NeedsReplan);
}

#[tokio::test]
async fn final_verifier_can_fail_completed_nodes() {
    let mut plan = plan(node(VerifierSpec::Always, FailurePolicy::Stop));
    plan.final_verifier = Some(VerifierSpec::Equals {
        expected: json!({"work": 43}),
    });
    let runner = ScriptedRunner {
        failures: 0,
        calls: AtomicUsize::new(0),
        value: json!(42),
    };
    let result = Executor::new(&runner)
        .execute(&plan, CancellationToken::new())
        .await
        .expect("execute");
    assert_eq!(result.status, RunStatus::Failed);
    assert_eq!(result.states["work"], NodeState::Succeeded);
    assert!(result.events.iter().any(|event| matches!(
        event,
        EventKind::VerifierResult {
            node_id: None,
            accepted: false,
            ..
        }
    )));
}
