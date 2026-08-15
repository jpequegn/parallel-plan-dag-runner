//! Typed plan validation and execution primitives.

#[cfg(feature = "native")]
mod evaluation;
#[cfg(feature = "native")]
mod executor;
#[cfg(feature = "native")]
mod ledger;
mod plan;
#[cfg(feature = "native")]
mod replanning;
#[cfg(feature = "native")]
mod tools;
mod validation;
#[cfg(feature = "native")]
mod verification;

#[cfg(feature = "native")]
pub use evaluation::{
    EvaluationError, EvaluationReport, ExperimentHarness, ExperimentMode, ExperimentRecord,
    FixtureSpec,
};
#[cfg(feature = "native")]
pub use executor::{
    ExecutionError, ExecutionMode, Executor, NodeRunner, NodeState, RunResult, RunStatus,
};
#[cfg(feature = "native")]
pub use ledger::{EventEnvelope, EventKind, Ledger, LedgerError, RunSummary};
pub use plan::{
    AuthorityPolicy, FailurePolicy, InputSpec, Node, Plan, PlanLimits, ValueType, VerifierSpec,
};
#[cfg(feature = "native")]
pub use replanning::{
    FixtureReplanner, PatchOperation, PlanPatch, ReplanError, ReplanRequest, Replanner,
    ReplanningExecutor, plan_digest,
};
#[cfg(feature = "native")]
pub use tools::{
    Provenance, ResolvedOutput, ToolError, ToolRegistry, canonical_digest, resolve_inputs,
};
pub use validation::{Diagnostic, ValidationError, parse_plan, plan_json_schema, validate_plan};
#[cfg(feature = "native")]
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
