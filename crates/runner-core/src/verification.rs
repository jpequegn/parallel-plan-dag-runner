use std::collections::BTreeMap;

use evalexpr::{
    ContextWithMutableVariables, HashMapContext, Value as ExpressionValue,
    eval_boolean_with_context,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{Node, ResolvedOutput, VerifierSpec, canonical_digest};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct VerificationEvidence {
    pub verifier: String,
    pub version: String,
    pub accepted: bool,
    pub reason: String,
    pub input_digest: String,
}

#[must_use]
pub fn verify_node(node: &Node, output: &ResolvedOutput) -> VerificationEvidence {
    verify_spec(&node.verifier, &output.value, node.output_schema.as_ref())
}

#[must_use]
pub fn verify_final(
    verifier: &VerifierSpec,
    outputs: &BTreeMap<String, ResolvedOutput>,
    schema: Option<&Value>,
) -> VerificationEvidence {
    let aggregate: serde_json::Map<String, Value> = outputs
        .iter()
        .map(|(id, output)| (id.clone(), output.value.clone()))
        .collect();
    verify_spec(verifier, &Value::Object(aggregate), schema)
}

fn verify_spec(
    verifier: &VerifierSpec,
    value: &Value,
    schema: Option<&Value>,
) -> VerificationEvidence {
    let (name, accepted, reason) = match verifier {
        VerifierSpec::Always => ("always", true, "unconditional acceptance".to_owned()),
        VerifierSpec::JsonSchema => match schema {
            Some(schema) => match jsonschema::validator_for(schema) {
                Ok(validator) if validator.is_valid(value) => (
                    "json_schema",
                    true,
                    "value satisfies output schema".to_owned(),
                ),
                Ok(_) => (
                    "json_schema",
                    false,
                    "value does not satisfy output schema".to_owned(),
                ),
                Err(error) => (
                    "json_schema",
                    false,
                    format!("invalid output schema: {error}"),
                ),
            },
            None => ("json_schema", false, "output schema is missing".to_owned()),
        },
        VerifierSpec::Equals { expected } => (
            "equals",
            value == expected,
            if value == expected {
                "value equals expected value".to_owned()
            } else {
                "value differs from expected value".to_owned()
            },
        ),
        VerifierSpec::NumericRange { minimum, maximum } => verify_range(value, *minimum, *maximum),
        VerifierSpec::Expression { expression } => verify_expression(value, expression),
    };
    VerificationEvidence {
        verifier: name.to_owned(),
        version: "v1".to_owned(),
        accepted,
        reason,
        input_digest: canonical_digest(value),
    }
}

fn verify_range(
    value: &Value,
    minimum: Option<f64>,
    maximum: Option<f64>,
) -> (&'static str, bool, String) {
    let Some(number) = value.as_f64() else {
        return ("numeric_range", false, "value is not numeric".to_owned());
    };
    let above_minimum = minimum.is_none_or(|limit| number >= limit);
    let below_maximum = maximum.is_none_or(|limit| number <= limit);
    let accepted = above_minimum && below_maximum;
    (
        "numeric_range",
        accepted,
        if accepted {
            "value is inside the inclusive range".to_owned()
        } else {
            format!("value {number} is outside [{minimum:?}, {maximum:?}]")
        },
    )
}

fn verify_expression(value: &Value, expression: &str) -> (&'static str, bool, String) {
    let mut context = HashMapContext::new();
    if let Err(error) = add_expression_values(&mut context, value) {
        return ("expression", false, error);
    }
    match eval_boolean_with_context(expression, &context) {
        Ok(accepted) => (
            "expression",
            accepted,
            if accepted {
                "expression evaluated to true".to_owned()
            } else {
                "expression evaluated to false".to_owned()
            },
        ),
        Err(error) => ("expression", false, format!("expression error: {error}")),
    }
}

fn add_expression_values(context: &mut HashMapContext, value: &Value) -> Result<(), String> {
    if let Some(scalar) = expression_value(value) {
        context
            .set_value("value".to_owned(), scalar)
            .map_err(|error| error.to_string())?;
    }
    if let Value::Object(object) = value {
        for (key, value) in object {
            if let Some(scalar) = expression_value(value) {
                context
                    .set_value(key.clone(), scalar)
                    .map_err(|error| error.to_string())?;
            }
        }
    }
    Ok(())
}

fn expression_value(value: &Value) -> Option<ExpressionValue> {
    match value {
        Value::Bool(value) => Some(ExpressionValue::Boolean(*value)),
        Value::Number(value) => value.as_f64().map(ExpressionValue::Float),
        Value::String(value) => Some(ExpressionValue::String(value.clone())),
        Value::Null | Value::Array(_) | Value::Object(_) => None,
    }
}
