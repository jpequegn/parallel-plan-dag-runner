use runner_core::{ValidationError, parse_plan, plan_json_schema, validate_plan};

const VALID: &str = include_str!("../../../examples/basic-plan.yaml");

fn diagnostics_for(source: &str) -> Vec<String> {
    let plan = parse_plan(source, "yaml").expect("fixture should parse");
    match validate_plan(&plan).expect_err("fixture should fail") {
        ValidationError::Invalid { diagnostics } => {
            diagnostics.into_iter().map(|item| item.code).collect()
        }
        other @ ValidationError::Parse { .. } => panic!("unexpected error: {other}"),
    }
}

#[test]
fn accepts_valid_yaml_and_json_round_trip() {
    let plan = parse_plan(VALID, "yaml").expect("valid YAML");
    validate_plan(&plan).expect("valid plan");
    let json = serde_json::to_string(&plan).expect("serialize plan");
    let reparsed = parse_plan(&json, "json").expect("valid JSON");
    assert_eq!(plan, reparsed);
}

#[test]
fn schema_is_deterministic() {
    let first = plan_json_schema().expect("schema");
    let second = plan_json_schema().expect("schema");
    assert_eq!(first, second);
    assert!(first.contains("max_concurrency"));
}

#[test]
fn rejects_missing_dependency_and_reference() {
    let source = VALID.replace("dependencies: [subtotal]", "dependencies: [missing]");
    let codes = diagnostics_for(&source);
    assert!(codes.contains(&"missing_dependency".to_owned()));
    assert!(codes.contains(&"undeclared_reference_dependency".to_owned()));
}

#[test]
fn rejects_cycles() {
    let source = VALID.replace(
        "  - id: subtotal\n    objective:",
        "  - id: subtotal\n    dependencies: [package]\n    objective:",
    );
    assert!(diagnostics_for(&source).contains(&"cycle".to_owned()));
}

#[test]
fn rejects_type_mismatches() {
    let source = VALID.replace(
        "type: number\n    output: object",
        "type: string\n    output: object",
    );
    assert!(diagnostics_for(&source).contains(&"reference_type_mismatch".to_owned()));
}

#[test]
fn rejects_undeclared_tools() {
    let source = VALID.replace("tool: calculator", "tool: document_lookup");
    assert!(diagnostics_for(&source).contains(&"undeclared_tool".to_owned()));
}

#[test]
fn rejects_authority_escalation() {
    let source = VALID.replacen("authority: [compute]", "authority: [network]", 1);
    assert!(diagnostics_for(&source).contains(&"authority_escalation".to_owned()));
}

#[test]
fn rejects_literal_type_mismatch_and_duplicate_ids() {
    let source = VALID
        .replace("type: string", "type: number")
        .replace("id: package", "id: subtotal");
    let codes = diagnostics_for(&source);
    assert!(codes.contains(&"literal_type_mismatch".to_owned()));
    assert!(codes.contains(&"duplicate_node".to_owned()));
}

#[test]
fn rejects_incomplete_verification_and_degrade_contracts() {
    let source = VALID
        .replace(
            "tool: calculator",
            "verifier:\n      kind: json_schema\n    tool: calculator",
        )
        .replace("failure_policy: stop", "failure_policy: degrade");
    let mut plan = parse_plan(&source, "yaml").expect("fixture should parse");
    plan.nodes[0].failure_policy = runner_core::FailurePolicy::Degrade;
    let error = validate_plan(&plan).expect_err("missing contracts");
    let ValidationError::Invalid { diagnostics } = error else {
        panic!("expected invalid plan");
    };
    let codes: Vec<_> = diagnostics.iter().map(|item| item.code.as_str()).collect();
    assert!(codes.contains(&"missing_output_schema"));
    assert!(codes.contains(&"missing_degrade_value"));
}
