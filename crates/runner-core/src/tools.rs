use std::collections::{BTreeMap, HashMap};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Number, Value};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::{InputSpec, Node, Plan, ValueType};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Provenance {
    pub node_id: String,
    pub invocation_id: String,
    pub tool_name: String,
    pub request_digest: String,
    pub response_digest: String,
    pub content_digest: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ResolvedOutput {
    pub value: Value,
    pub provenance: Provenance,
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ToolError {
    #[error("tool '{0}' is not registered")]
    Unregistered(String),
    #[error("tool '{0}' is not declared by the plan")]
    Undeclared(String),
    #[error("node '{node}' lacks required capability '{capability}'")]
    MissingCapability { node: String, capability: String },
    #[error("node '{node}' requests authority outside the plan: {capability}")]
    AuthorityEscalation { node: String, capability: String },
    #[error("input '{0}' is missing")]
    MissingInput(String),
    #[error("input '{name}' expected {expected:?} but found {actual:?}")]
    TypeMismatch {
        name: String,
        expected: ValueType,
        actual: ValueType,
    },
    #[error("dependency output '{0}' is missing")]
    MissingOutput(String),
    #[error("JSON pointer '{path}' does not exist in output '{node}'")]
    MissingPath { node: String, path: String },
    #[error("tool execution failed: {0}")]
    Execution(String),
}

#[async_trait]
trait Tool: Send + Sync {
    fn name(&self) -> &'static str;
    fn required_capability(&self) -> &'static str;
    async fn execute(&self, inputs: &BTreeMap<String, Value>) -> Result<Value, ToolError>;
}

pub struct ToolRegistry {
    tools: HashMap<String, Box<dyn Tool>>,
}

impl ToolRegistry {
    #[must_use]
    pub fn deterministic(
        documents: BTreeMap<String, String>,
        routes: BTreeMap<String, Value>,
    ) -> Self {
        let tools: Vec<Box<dyn Tool>> = vec![
            Box::new(Calculator),
            Box::new(DocumentLookup { documents }),
            Box::new(JsonTransform),
            Box::new(FixtureHttp { routes }),
        ];
        Self {
            tools: tools
                .into_iter()
                .map(|tool| (tool.name().to_owned(), tool))
                .collect(),
        }
    }

    /// Execute a validated node with resolved inputs and attach stable provenance.
    ///
    /// # Errors
    /// Fails if plan/node authority is insufficient, the tool is absent, or execution fails.
    pub async fn execute(
        &self,
        plan: &Plan,
        node: &Node,
        inputs: &BTreeMap<String, Value>,
    ) -> Result<ResolvedOutput, ToolError> {
        if !plan.authority.tools.contains(&node.tool) {
            return Err(ToolError::Undeclared(node.tool.clone()));
        }
        if let Some(capability) = node
            .authority
            .difference(&plan.authority.capabilities)
            .next()
        {
            return Err(ToolError::AuthorityEscalation {
                node: node.id.clone(),
                capability: capability.clone(),
            });
        }
        let tool = self
            .tools
            .get(&node.tool)
            .ok_or_else(|| ToolError::Unregistered(node.tool.clone()))?;
        if !node.authority.contains(tool.required_capability()) {
            return Err(ToolError::MissingCapability {
                node: node.id.clone(),
                capability: tool.required_capability().to_owned(),
            });
        }

        let request = serde_json::json!({
            "node_id": node.id,
            "tool": node.tool,
            "inputs": inputs,
            "authority": node.authority,
        });
        let request_digest = canonical_digest(&request);
        let value = tool.execute(inputs).await?;
        let actual = ValueType::of(&value);
        if !node.output.accepts(&actual) {
            return Err(ToolError::TypeMismatch {
                name: "output".to_owned(),
                expected: node.output.clone(),
                actual,
            });
        }
        let response_digest = canonical_digest(&serde_json::json!({
            "tool": node.tool,
            "value": value,
        }));
        let content_digest = canonical_digest(&value);
        let invocation_id = canonical_digest(&serde_json::json!({
            "node_id": node.id,
            "request": request_digest,
        }));
        Ok(ResolvedOutput {
            value,
            provenance: Provenance {
                node_id: node.id.clone(),
                invocation_id,
                tool_name: node.tool.clone(),
                request_digest,
                response_digest,
                content_digest,
            },
        })
    }
}

/// Resolve literal and dependency inputs without interpolating strings.
///
/// # Errors
/// Fails when an output/path is absent or a runtime value violates its declared type.
pub fn resolve_inputs(
    node: &Node,
    outputs: &BTreeMap<String, ResolvedOutput>,
) -> Result<BTreeMap<String, Value>, ToolError> {
    node.inputs
        .iter()
        .map(|(name, input)| {
            let (value, expected) = match input {
                InputSpec::Literal { value, value_type } => (value.clone(), value_type),
                InputSpec::Reference {
                    node: source,
                    path,
                    value_type,
                } => {
                    let output = outputs
                        .get(source)
                        .ok_or_else(|| ToolError::MissingOutput(source.clone()))?;
                    let value = if let Some(pointer) = path {
                        output.value.pointer(pointer).cloned().ok_or_else(|| {
                            ToolError::MissingPath {
                                node: source.clone(),
                                path: pointer.clone(),
                            }
                        })?
                    } else {
                        output.value.clone()
                    };
                    (value, value_type)
                }
            };
            let actual = ValueType::of(&value);
            if !expected.accepts(&actual) {
                return Err(ToolError::TypeMismatch {
                    name: name.clone(),
                    expected: expected.clone(),
                    actual,
                });
            }
            Ok((name.clone(), value))
        })
        .collect()
}

#[must_use]
pub fn canonical_digest(value: &Value) -> String {
    let canonical = canonicalize(value);
    hex::encode(Sha256::digest(canonical.to_string().as_bytes()))
}

fn canonicalize(value: &Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.iter().map(canonicalize).collect()),
        Value::Object(object) => {
            let sorted: BTreeMap<_, _> = object
                .iter()
                .map(|(key, value)| (key.clone(), canonicalize(value)))
                .collect();
            Value::Object(sorted.into_iter().collect())
        }
        scalar => scalar.clone(),
    }
}

struct Calculator;

#[async_trait]
impl Tool for Calculator {
    fn name(&self) -> &'static str {
        "calculator"
    }

    fn required_capability(&self) -> &'static str {
        "compute"
    }

    async fn execute(&self, inputs: &BTreeMap<String, Value>) -> Result<Value, ToolError> {
        let expression = string_input(inputs, "expression")?;
        let result = evalexpr::eval_number(expression)
            .map_err(|error| ToolError::Execution(format!("invalid expression: {error}")))?;
        Number::from_f64(result).map(Value::Number).ok_or_else(|| {
            ToolError::Execution("calculator produced a non-finite value".to_owned())
        })
    }
}

struct DocumentLookup {
    documents: BTreeMap<String, String>,
}

#[async_trait]
impl Tool for DocumentLookup {
    fn name(&self) -> &'static str {
        "document_lookup"
    }

    fn required_capability(&self) -> &'static str {
        "read_documents"
    }

    async fn execute(&self, inputs: &BTreeMap<String, Value>) -> Result<Value, ToolError> {
        let document_id = string_input(inputs, "document_id")?;
        self.documents
            .get(document_id)
            .cloned()
            .map(Value::String)
            .ok_or_else(|| ToolError::Execution(format!("document '{document_id}' not found")))
    }
}

struct JsonTransform;

#[async_trait]
impl Tool for JsonTransform {
    fn name(&self) -> &'static str {
        "json_transform"
    }

    fn required_capability(&self) -> &'static str {
        "compute"
    }

    async fn execute(&self, inputs: &BTreeMap<String, Value>) -> Result<Value, ToolError> {
        let object: Map<String, Value> = inputs
            .iter()
            .filter(|(key, _)| key.as_str() != "operation")
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        Ok(Value::Object(object))
    }
}

struct FixtureHttp {
    routes: BTreeMap<String, Value>,
}

#[async_trait]
impl Tool for FixtureHttp {
    fn name(&self) -> &'static str {
        "fixture_http"
    }

    fn required_capability(&self) -> &'static str {
        "fixture_network"
    }

    async fn execute(&self, inputs: &BTreeMap<String, Value>) -> Result<Value, ToolError> {
        let method = inputs
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or("GET");
        let url = string_input(inputs, "url")?;
        let key = format!("{} {url}", method.to_ascii_uppercase());
        self.routes
            .get(&key)
            .cloned()
            .ok_or_else(|| ToolError::Execution(format!("fixture route '{key}' not found")))
    }
}

fn string_input<'a>(inputs: &'a BTreeMap<String, Value>, name: &str) -> Result<&'a str, ToolError> {
    let value = inputs
        .get(name)
        .ok_or_else(|| ToolError::MissingInput(name.to_owned()))?;
    value.as_str().ok_or_else(|| ToolError::TypeMismatch {
        name: name.to_owned(),
        expected: ValueType::String,
        actual: ValueType::of(value),
    })
}
