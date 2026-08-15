use runner_wasm::{replay_payload, validate_payload};
use serde_json::{Value, json};

const PLAN: &str = include_str!("../../../examples/basic-plan.yaml");

#[test]
fn wasm_validation_matches_the_native_contract() {
    let response: Value = serde_json::from_str(&validate_payload(PLAN, "yaml")).expect("JSON");
    assert_eq!(response["ok"], true);
    assert_eq!(response["plan"]["id"], "basic-plan");
    let plan = runner_core::parse_plan(PLAN, "yaml").expect("native parse");
    runner_core::validate_plan(&plan).expect("native validation");
    assert_eq!(
        response["plan"],
        serde_json::to_value(plan).expect("native plan JSON")
    );
}

#[test]
fn replay_reduces_states_provenance_and_evidence() {
    let events = json!([
        {"sequence":0,"timestamp_ms":10,"event":{"type":"run_started","plan_id":"p","mode":"parallel","nodes":["a"]}},
        {"sequence":1,"timestamp_ms":11,"event":{"type":"node_started","node_id":"a"}},
        {"sequence":2,"timestamp_ms":12,"event":{"type":"tool_call","node_id":"a","tool":"calculator","inputs":{}}},
        {"sequence":3,"timestamp_ms":13,"event":{"type":"tool_response","node_id":"a","output":{"value":42,"provenance":{"content_digest":"abc"}}}},
        {"sequence":4,"timestamp_ms":14,"event":{"type":"verifier_result","node_id":"a","accepted":true,"evidence":{"verifier":"equals"}}},
        {"sequence":5,"timestamp_ms":15,"event":{"type":"node_succeeded","node_id":"a"}},
        {"sequence":6,"timestamp_ms":16,"event":{"type":"run_completed","status":"succeeded"}}
    ]);
    let response: Value =
        serde_json::from_str(&replay_payload(&events.to_string())).expect("replay JSON");
    assert_eq!(response["ok"], true);
    assert_eq!(response["replay"]["status"], "succeeded");
    assert_eq!(response["replay"]["nodes"]["a"]["output"], 42);
    assert_eq!(
        response["replay"]["nodes"]["a"]["provenance"]["content_digest"],
        "abc"
    );
    assert_eq!(
        response["replay"]["nodes"]["a"]["verifier"]["verifier"],
        "equals"
    );
}
