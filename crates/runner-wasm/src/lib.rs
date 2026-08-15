use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use wasm_bindgen::prelude::*;

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
struct NodeView {
    state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    output: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    provenance: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    verifier: Option<Value>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
struct TransitionView {
    sequence: usize,
    event_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    node_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    timestamp_ms: Option<u64>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
struct ReplayView {
    status: String,
    nodes: BTreeMap<String, NodeView>,
    transitions: Vec<TransitionView>,
}

#[wasm_bindgen]
#[must_use]
pub fn validate_plan_json(source: &str, format: &str) -> String {
    validate_payload(source, format)
}

#[wasm_bindgen]
#[must_use]
pub fn replay_events_json(source: &str) -> String {
    replay_payload(source)
}

#[must_use]
pub fn validate_payload(source: &str, format: &str) -> String {
    let response = match runner_core::parse_plan(source, format) {
        Ok(plan) => match runner_core::validate_plan(&plan) {
            Ok(()) => json!({"ok": true, "plan": plan, "diagnostics": []}),
            Err(runner_core::ValidationError::Invalid { diagnostics }) => {
                json!({"ok": false, "plan": plan, "diagnostics": diagnostics})
            }
            Err(error) => json!({"ok": false, "error": error.to_string(), "diagnostics": []}),
        },
        Err(error) => json!({"ok": false, "error": error.to_string(), "diagnostics": []}),
    };
    response.to_string()
}

#[must_use]
pub fn replay_payload(source: &str) -> String {
    let response = serde_json::from_str::<Vec<Value>>(source).map_or_else(
        |error| json!({"ok": false, "error": error.to_string()}),
        |events| match reduce_events(&events) {
            Ok(replay) => json!({"ok": true, "replay": replay}),
            Err(error) => json!({"ok": false, "error": error}),
        },
    );
    response.to_string()
}

#[allow(clippy::too_many_lines)]
fn reduce_events(events: &[Value]) -> Result<ReplayView, String> {
    let mut replay = ReplayView::default();
    let mut candidates = BTreeMap::<String, (Value, Option<Value>)>::new();
    for (index, envelope) in events.iter().enumerate() {
        let event = envelope.get("event").unwrap_or(envelope);
        let event_type = event
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("event {index} has no type"))?;
        let node_id = event
            .get("node_id")
            .and_then(Value::as_str)
            .map(str::to_owned);
        replay.transitions.push(TransitionView {
            sequence: envelope
                .get("sequence")
                .and_then(Value::as_u64)
                .and_then(|value| usize::try_from(value).ok())
                .unwrap_or(index),
            event_type: event_type.to_owned(),
            node_id: node_id.clone(),
            timestamp_ms: envelope.get("timestamp_ms").and_then(Value::as_u64),
        });
        match event_type {
            "run_started" => {
                let nodes = event
                    .get("nodes")
                    .and_then(Value::as_array)
                    .ok_or_else(|| "run_started has no nodes".to_owned())?;
                for node in nodes {
                    let id = node
                        .as_str()
                        .ok_or_else(|| "run_started node ID is not a string".to_owned())?;
                    replay.nodes.insert(
                        id.to_owned(),
                        NodeView {
                            state: "pending".to_owned(),
                            ..NodeView::default()
                        },
                    );
                }
            }
            "node_started" => update_state(&mut replay, node_id.as_deref(), "running")?,
            "tool_call" => {
                let id = required_node(node_id.as_deref())?;
                replay.nodes.entry(id.to_owned()).or_default().tool =
                    event.get("tool").and_then(Value::as_str).map(str::to_owned);
            }
            "tool_response" => {
                let id = required_node(node_id.as_deref())?;
                let output = event
                    .get("output")
                    .ok_or_else(|| "tool_response has no output".to_owned())?;
                candidates.insert(
                    id.to_owned(),
                    (
                        output.get("value").cloned().unwrap_or(Value::Null),
                        output.get("provenance").cloned(),
                    ),
                );
            }
            "verifier_result" => {
                if let Some(id) = node_id.as_deref() {
                    replay.nodes.entry(id.to_owned()).or_default().verifier =
                        event.get("evidence").cloned();
                }
            }
            "node_succeeded" | "node_degraded" => {
                let id = required_node(node_id.as_deref())?;
                let (output, provenance) = candidates
                    .remove(id)
                    .ok_or_else(|| format!("{event_type} has no candidate output"))?;
                let node = replay.nodes.entry(id.to_owned()).or_default();
                event_type
                    .trim_start_matches("node_")
                    .clone_into(&mut node.state);
                node.output = Some(output);
                node.provenance = provenance;
            }
            "node_failed" => update_state(&mut replay, node_id.as_deref(), "failed")?,
            "node_timed_out" => update_state(&mut replay, node_id.as_deref(), "timed_out")?,
            "node_blocked" => update_state(&mut replay, node_id.as_deref(), "blocked")?,
            "node_cancelled" => update_state(&mut replay, node_id.as_deref(), "cancelled")?,
            "node_needs_replan" => {
                update_state(&mut replay, node_id.as_deref(), "needs_replan")?;
            }
            "retry" => update_state(&mut replay, node_id.as_deref(), "pending")?,
            "replan" => {
                if let Some(removed) = event.get("removed_nodes").and_then(Value::as_array) {
                    for node in removed.iter().filter_map(Value::as_str) {
                        replay.nodes.remove(node);
                        candidates.remove(node);
                    }
                }
            }
            "run_completed" => {
                event
                    .get("status")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown")
                    .clone_into(&mut replay.status);
            }
            "cancellation" => "cancelled".clone_into(&mut replay.status),
            other => return Err(format!("unsupported event type '{other}'")),
        }
    }
    if replay.status.is_empty() {
        return Err("event stream has no run_completed status".to_owned());
    }
    Ok(replay)
}

fn required_node(node_id: Option<&str>) -> Result<&str, String> {
    node_id.ok_or_else(|| "event has no node_id".to_owned())
}

fn update_state(replay: &mut ReplayView, node_id: Option<&str>, state: &str) -> Result<(), String> {
    let id = required_node(node_id)?;
    state.clone_into(&mut replay.nodes.entry(id.to_owned()).or_default().state);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_incomplete_event_streams() {
        let output = replay_payload(r#"[{"type":"run_started","nodes":[]}]"#);
        assert_eq!(
            serde_json::from_str::<Value>(&output).expect("JSON")["ok"],
            false
        );
    }
}
