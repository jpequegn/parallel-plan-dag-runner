use std::collections::{BTreeMap, BTreeSet};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

fn default_concurrency() -> usize {
    4
}

fn default_timeout_ms() -> u64 {
    30_000
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ValueType {
    Any,
    Null,
    Bool,
    Number,
    String,
    Array,
    Object,
}

impl ValueType {
    #[must_use]
    pub fn accepts(&self, actual: &Self) -> bool {
        matches!(self, Self::Any) || self == actual
    }

    #[must_use]
    pub fn of(value: &Value) -> Self {
        match value {
            Value::Null => Self::Null,
            Value::Bool(_) => Self::Bool,
            Value::Number(_) => Self::Number,
            Value::String(_) => Self::String,
            Value::Array(_) => Self::Array,
            Value::Object(_) => Self::Object,
        }
    }
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum InputSpec {
    Literal {
        value: Value,
        #[serde(rename = "type")]
        value_type: ValueType,
    },
    Reference {
        node: String,
        #[serde(default)]
        path: Option<String>,
        #[serde(rename = "type")]
        value_type: ValueType,
    },
}

#[derive(Clone, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum VerifierSpec {
    #[default]
    Always,
    JsonSchema,
    Equals {
        expected: Value,
    },
}

#[derive(Clone, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FailurePolicy {
    #[default]
    Stop,
    Retry,
    Replan,
    Degrade,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
pub struct Node {
    pub id: String,
    pub objective: String,
    #[serde(default)]
    pub dependencies: Vec<String>,
    #[serde(default)]
    pub inputs: BTreeMap<String, InputSpec>,
    pub output: ValueType,
    pub tool: String,
    #[serde(default)]
    pub authority: BTreeSet<String>,
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u64,
    #[serde(default)]
    pub retry_budget: u32,
    #[serde(default)]
    pub verifier: VerifierSpec,
    #[serde(default)]
    pub failure_policy: FailurePolicy,
    #[serde(default)]
    pub immutable: bool,
}

#[derive(Clone, Debug, Default, Deserialize, JsonSchema, PartialEq, Serialize)]
pub struct AuthorityPolicy {
    #[serde(default)]
    pub tools: BTreeSet<String>,
    #[serde(default)]
    pub capabilities: BTreeSet<String>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
pub struct PlanLimits {
    #[serde(default = "default_concurrency")]
    pub max_concurrency: usize,
    #[serde(default)]
    pub max_replans: u32,
    #[serde(default)]
    pub max_node_growth: usize,
}

impl Default for PlanLimits {
    fn default() -> Self {
        Self {
            max_concurrency: default_concurrency(),
            max_replans: 0,
            max_node_growth: 0,
        }
    }
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
pub struct Plan {
    pub version: String,
    pub id: String,
    #[serde(default)]
    pub authority: AuthorityPolicy,
    #[serde(default)]
    pub limits: PlanLimits,
    pub nodes: Vec<Node>,
    #[serde(default)]
    pub final_verifier: Option<VerifierSpec>,
}
