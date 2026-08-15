//! Typed plan validation and execution primitives.

mod executor;
mod ledger;
mod plan;
mod replanning;
mod tools;
mod validation;
mod verification;

pub use executor::{
    ExecutionError, ExecutionMode, Executor, NodeRunner, NodeState, RunResult, RunStatus,
};
pub use ledger::{EventEnvelope, EventKind, Ledger, LedgerError, RunSummary};
pub use plan::{
    AuthorityPolicy, FailurePolicy, InputSpec, Node, Plan, PlanLimits, ValueType, VerifierSpec,
};
pub use replanning::{
    FixtureReplanner, PatchOperation, PlanPatch, ReplanError, ReplanRequest, Replanner,
    ReplanningExecutor, plan_digest,
};
pub use tools::{
    Provenance, ResolvedOutput, ToolError, ToolRegistry, canonical_digest, resolve_inputs,
};
pub use validation::{Diagnostic, ValidationError, parse_plan, plan_json_schema, validate_plan};
pub use verification::{VerificationEvidence, verify_final, verify_node};

/// Returns the plan format version implemented by this crate.
#[must_use]
pub const fn plan_format_version() -> &'static str {
    "v1"
}

#[cfg(test)]
mod tests {
    #[test]
    fn exposes_plan_format_version() {
        assert_eq!(super::plan_format_version(), "v1");
    }
}
