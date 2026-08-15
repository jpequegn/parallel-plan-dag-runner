use std::collections::{HashMap, HashSet};

use petgraph::{algo::toposort, graph::DiGraph};
use schemars::schema_for;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{FailurePolicy, InputSpec, Plan, ValueType, VerifierSpec, plan_format_version};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Diagnostic {
    pub code: String,
    pub path: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
}

impl Diagnostic {
    fn plan(code: &str, path: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_owned(),
            path: path.to_owned(),
            message: message.into(),
            node_id: None,
        }
    }

    fn node(code: &str, node_id: &str, path: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_owned(),
            path: path.to_owned(),
            message: message.into(),
            node_id: Some(node_id.to_owned()),
        }
    }
}

#[derive(Debug, Error)]
pub enum ValidationError {
    #[error("cannot parse plan as {format}: {message}")]
    Parse { format: String, message: String },
    #[error("plan failed preflight validation")]
    Invalid { diagnostics: Vec<Diagnostic> },
}

/// Parse a plan from JSON or YAML.
///
/// # Errors
/// Returns [`ValidationError::Parse`] for unsupported formats or malformed input.
pub fn parse_plan(source: &str, format: &str) -> Result<Plan, ValidationError> {
    let normalized = format.trim_start_matches('.').to_ascii_lowercase();
    match normalized.as_str() {
        "json" => serde_json::from_str(source).map_err(|error| ValidationError::Parse {
            format: normalized,
            message: error.to_string(),
        }),
        "yaml" | "yml" => serde_yaml::from_str(source).map_err(|error| ValidationError::Parse {
            format: normalized,
            message: error.to_string(),
        }),
        _ => Err(ValidationError::Parse {
            format: normalized,
            message: "expected json, yaml, or yml".to_owned(),
        }),
    }
}

/// Render the supported plan contract as deterministic, pretty JSON Schema.
///
/// # Errors
/// Returns a serialization error if the generated schema cannot be encoded.
pub fn plan_json_schema() -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(&schema_for!(Plan))
}

/// Apply all fail-closed preflight checks to a parsed plan.
///
/// # Errors
/// Returns [`ValidationError::Invalid`] with structured diagnostics when any check fails.
#[allow(clippy::too_many_lines)]
pub fn validate_plan(plan: &Plan) -> Result<(), ValidationError> {
    let mut diagnostics = Vec::new();
    if plan.version != plan_format_version() {
        diagnostics.push(Diagnostic::plan(
            "unsupported_version",
            "version",
            format!(
                "expected {} but found {}",
                plan_format_version(),
                plan.version
            ),
        ));
    }
    if !valid_id(&plan.id) {
        diagnostics.push(Diagnostic::plan(
            "invalid_id",
            "id",
            "plan ID must use ASCII letters, digits, '.', '_', or '-'",
        ));
    }
    if plan.limits.max_concurrency == 0 {
        diagnostics.push(Diagnostic::plan(
            "invalid_limit",
            "limits.max_concurrency",
            "max_concurrency must be at least one",
        ));
    }
    if matches!(plan.final_verifier, Some(VerifierSpec::JsonSchema))
        && plan.final_output_schema.is_none()
    {
        diagnostics.push(Diagnostic::plan(
            "missing_final_output_schema",
            "final_output_schema",
            "json_schema final verifier requires final_output_schema",
        ));
    }

    let mut indices = HashMap::new();
    for (index, node) in plan.nodes.iter().enumerate() {
        if !valid_id(&node.id) {
            diagnostics.push(Diagnostic::node(
                "invalid_id",
                &node.id,
                &format!("nodes[{index}].id"),
                "node ID must use ASCII letters, digits, '.', '_', or '-'",
            ));
        }
        if indices.insert(node.id.as_str(), index).is_some() {
            diagnostics.push(Diagnostic::node(
                "duplicate_node",
                &node.id,
                &format!("nodes[{index}].id"),
                "node ID is not unique",
            ));
        }
        if !plan.authority.tools.contains(&node.tool) {
            diagnostics.push(Diagnostic::node(
                "undeclared_tool",
                &node.id,
                &format!("nodes[{index}].tool"),
                format!("tool '{}' is absent from plan authority", node.tool),
            ));
        }
        for capability in node.authority.difference(&plan.authority.capabilities) {
            diagnostics.push(Diagnostic::node(
                "authority_escalation",
                &node.id,
                &format!("nodes[{index}].authority"),
                format!("capability '{capability}' is absent from plan authority"),
            ));
        }
        if node.timeout_ms == 0 {
            diagnostics.push(Diagnostic::node(
                "invalid_limit",
                &node.id,
                &format!("nodes[{index}].timeout_ms"),
                "timeout_ms must be at least one",
            ));
        }
        if matches!(node.verifier, VerifierSpec::JsonSchema) && node.output_schema.is_none() {
            diagnostics.push(Diagnostic::node(
                "missing_output_schema",
                &node.id,
                &format!("nodes[{index}].output_schema"),
                "json_schema verifier requires output_schema",
            ));
        }
        if matches!(node.failure_policy, FailurePolicy::Degrade) && node.degrade_value.is_none() {
            diagnostics.push(Diagnostic::node(
                "missing_degrade_value",
                &node.id,
                &format!("nodes[{index}].degrade_value"),
                "degrade policy requires an explicit fallback value",
            ));
        }
        if let Some(value) = &node.degrade_value {
            let actual = ValueType::of(value);
            if !node.output.accepts(&actual) {
                diagnostics.push(Diagnostic::node(
                    "degrade_type_mismatch",
                    &node.id,
                    &format!("nodes[{index}].degrade_value"),
                    format!("node outputs {:?} but fallback is {actual:?}", node.output),
                ));
            }
        }
    }

    for (index, node) in plan.nodes.iter().enumerate() {
        let mut seen_dependencies = HashSet::new();
        for dependency in &node.dependencies {
            if !seen_dependencies.insert(dependency) {
                diagnostics.push(Diagnostic::node(
                    "duplicate_dependency",
                    &node.id,
                    &format!("nodes[{index}].dependencies"),
                    format!("dependency '{dependency}' is repeated"),
                ));
            }
            if !indices.contains_key(dependency.as_str()) {
                diagnostics.push(Diagnostic::node(
                    "missing_dependency",
                    &node.id,
                    &format!("nodes[{index}].dependencies"),
                    format!("dependency '{dependency}' does not exist"),
                ));
            }
        }
        for (name, input) in &node.inputs {
            match input {
                InputSpec::Literal { value, value_type } => {
                    let actual = ValueType::of(value);
                    if !value_type.accepts(&actual) {
                        diagnostics.push(Diagnostic::node(
                            "literal_type_mismatch",
                            &node.id,
                            &format!("nodes[{index}].inputs.{name}"),
                            format!("declared {value_type:?} but literal is {actual:?}"),
                        ));
                    }
                }
                InputSpec::Reference {
                    node: source,
                    value_type,
                    ..
                } => match indices.get(source.as_str()) {
                    None => diagnostics.push(Diagnostic::node(
                        "unresolved_reference",
                        &node.id,
                        &format!("nodes[{index}].inputs.{name}"),
                        format!("referenced node '{source}' does not exist"),
                    )),
                    Some(source_index) => {
                        if !node.dependencies.contains(source) {
                            diagnostics.push(Diagnostic::node(
                                "undeclared_reference_dependency",
                                &node.id,
                                &format!("nodes[{index}].inputs.{name}"),
                                format!("referenced node '{source}' is not a declared dependency"),
                            ));
                        }
                        let actual = &plan.nodes[*source_index].output;
                        if !value_type.accepts(actual) {
                            diagnostics.push(Diagnostic::node(
                                "reference_type_mismatch",
                                &node.id,
                                &format!("nodes[{index}].inputs.{name}"),
                                format!(
                                    "input expects {value_type:?} but '{source}' outputs {actual:?}"
                                ),
                            ));
                        }
                    }
                },
            }
        }
    }

    validate_acyclic(plan, &indices, &mut diagnostics);

    if diagnostics.is_empty() {
        Ok(())
    } else {
        Err(ValidationError::Invalid { diagnostics })
    }
}

fn validate_acyclic(
    plan: &Plan,
    indices: &HashMap<&str, usize>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let mut graph = DiGraph::<&str, ()>::new();
    let graph_nodes: Vec<_> = plan
        .nodes
        .iter()
        .map(|node| graph.add_node(node.id.as_str()))
        .collect();
    for (index, node) in plan.nodes.iter().enumerate() {
        for dependency in &node.dependencies {
            if let Some(dependency_index) = indices.get(dependency.as_str()) {
                graph.add_edge(graph_nodes[*dependency_index], graph_nodes[index], ());
            }
        }
    }
    if let Err(cycle) = toposort(&graph, None) {
        let node_id = graph[cycle.node_id()];
        diagnostics.push(Diagnostic::node(
            "cycle",
            node_id,
            "nodes",
            "dependency graph contains a cycle",
        ));
    }
}

fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}
