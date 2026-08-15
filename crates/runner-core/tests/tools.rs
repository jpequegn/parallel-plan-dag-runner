use std::collections::{BTreeMap, BTreeSet};

use runner_core::{
    AuthorityPolicy, FailurePolicy, InputSpec, Node, Plan, PlanLimits, ToolError, ToolRegistry,
    ValueType, VerifierSpec, canonical_digest, resolve_inputs,
};
use serde_json::json;

fn plan_for(tool: &str, capability: &str, output: ValueType) -> (Plan, Node) {
    let node = Node {
        id: "work".to_owned(),
        objective: "test".to_owned(),
        dependencies: vec![],
        inputs: BTreeMap::new(),
        output,
        tool: tool.to_owned(),
        authority: BTreeSet::from([capability.to_owned()]),
        timeout_ms: 1_000,
        retry_budget: 0,
        verifier: VerifierSpec::Always,
        failure_policy: FailurePolicy::Stop,
        immutable: false,
    };
    let plan = Plan {
        version: "v1".to_owned(),
        id: "tools".to_owned(),
        authority: AuthorityPolicy {
            tools: BTreeSet::from([tool.to_owned()]),
            capabilities: BTreeSet::from([capability.to_owned()]),
        },
        limits: PlanLimits::default(),
        nodes: vec![node.clone()],
        final_verifier: None,
    };
    (plan, node)
}

#[tokio::test]
async fn calculator_is_deterministic_and_provenanced() {
    let registry = ToolRegistry::deterministic(BTreeMap::new(), BTreeMap::new());
    let (plan, node) = plan_for("calculator", "compute", ValueType::Number);
    let inputs = BTreeMap::from([("expression".to_owned(), json!("20 * 2 + 2"))]);
    let first = registry
        .execute(&plan, &node, &inputs)
        .await
        .expect("calculate");
    let second = registry
        .execute(&plan, &node, &inputs)
        .await
        .expect("calculate");
    assert_eq!(first, second);
    assert_eq!(first.value, json!(42.0));
    assert_eq!(first.provenance.node_id, "work");
    assert_eq!(first.provenance.tool_name, "calculator");
}

#[tokio::test]
async fn document_and_http_tools_use_only_fixtures() {
    let registry = ToolRegistry::deterministic(
        BTreeMap::from([("guide".to_owned(), "verified text".to_owned())]),
        BTreeMap::from([(
            "GET https://fixture.test/data".to_owned(),
            json!({"ok": true}),
        )]),
    );
    let (doc_plan, doc_node) = plan_for("document_lookup", "read_documents", ValueType::String);
    let doc = registry
        .execute(
            &doc_plan,
            &doc_node,
            &BTreeMap::from([("document_id".to_owned(), json!("guide"))]),
        )
        .await
        .expect("document");
    assert_eq!(doc.value, json!("verified text"));

    let (http_plan, http_node) = plan_for("fixture_http", "fixture_network", ValueType::Object);
    let response = registry
        .execute(
            &http_plan,
            &http_node,
            &BTreeMap::from([("url".to_owned(), json!("https://fixture.test/data"))]),
        )
        .await
        .expect("fixture response");
    assert_eq!(response.value, json!({"ok": true}));
}

#[tokio::test]
async fn json_transform_packages_inputs() {
    let registry = ToolRegistry::deterministic(BTreeMap::new(), BTreeMap::new());
    let (plan, node) = plan_for("json_transform", "compute", ValueType::Object);
    let output = registry
        .execute(
            &plan,
            &node,
            &BTreeMap::from([("answer".to_owned(), json!(42))]),
        )
        .await
        .expect("transform");
    assert_eq!(output.value, json!({"answer": 42}));
}

#[tokio::test]
async fn runtime_authority_checks_fail_closed() {
    let registry = ToolRegistry::deterministic(BTreeMap::new(), BTreeMap::new());
    let (plan, mut node) = plan_for("calculator", "compute", ValueType::Number);
    node.authority.clear();
    let error = registry
        .execute(&plan, &node, &BTreeMap::new())
        .await
        .expect_err("missing capability");
    assert!(matches!(error, ToolError::MissingCapability { .. }));

    node.authority.insert("shell".to_owned());
    let error = registry
        .execute(&plan, &node, &BTreeMap::new())
        .await
        .expect_err("authority escalation");
    assert!(matches!(error, ToolError::AuthorityEscalation { .. }));
}

#[test]
fn resolves_typed_references_and_json_pointers() {
    let (_, mut source_node) = plan_for("json_transform", "compute", ValueType::Object);
    source_node.id = "source".to_owned();
    let outputs = BTreeMap::from([(
        "source".to_owned(),
        runner_core::ResolvedOutput {
            value: json!({"nested": {"answer": 42}}),
            provenance: runner_core::Provenance {
                node_id: "source".to_owned(),
                invocation_id: "i".to_owned(),
                tool_name: "json_transform".to_owned(),
                request_digest: "r".to_owned(),
                response_digest: "s".to_owned(),
                content_digest: "c".to_owned(),
            },
        },
    )]);
    let (_, mut node) = plan_for("json_transform", "compute", ValueType::Object);
    node.inputs.insert(
        "answer".to_owned(),
        InputSpec::Reference {
            node: "source".to_owned(),
            path: Some("/nested/answer".to_owned()),
            value_type: ValueType::Number,
        },
    );
    assert_eq!(
        resolve_inputs(&node, &outputs).expect("resolve")["answer"],
        json!(42)
    );
}

#[test]
fn canonical_digest_ignores_object_insertion_order() {
    assert_eq!(
        canonical_digest(&json!({"a": 1, "b": 2})),
        canonical_digest(&json!({"b": 2, "a": 1}))
    );
}
