# Usage Patterns and Extensions

## Practical Patterns

Use the runner when work has explicit dependencies, independently executable branches, typed intermediate outputs, and deterministic acceptance checks. Common patterns include parallel evidence collection followed by synthesis, independent risk calculations followed by a register, and multiple workload indicators followed by a review.

Start with sequential mode while defining contracts, then compare parallel mode with the same plan and fixtures. Mark expensive accepted dependencies immutable before enabling replanning. Persist runs whenever decisions require an audit trail, and export inspected events to the browser for review without re-executing tools.

Avoid DAG machinery for a single short action, tightly stateful sequences, or work whose correctness cannot be expressed beyond subjective model judgment. This project is a learning-grade local runner, not a distributed queue, credential broker, or production sandbox.

## Innovative Extensions

- Add an LLM planner adapter that emits only Plan v1 JSON and receives validation diagnostics for bounded repair.
- Derive a critical-path forecast from historical per-tool latency and choose concurrency dynamically.
- Insert human approval nodes for authority changes, high-risk outputs, or expensive branches.
- Sign ledger checkpoints with a workload identity and anchor digests in external immutable storage.
- Dispatch ready nodes to distributed workers while retaining a single deterministic event-ordering service.
- Add policy-as-code checks for data residency, budget, model choice, and connector scope.
- Compare alternative subgraphs as competing candidates and promote only the verifier-selected result.
- Feed replay traces into an evaluation system to detect dependency omissions, flaky tools, and verifier drift.

These are extension points, not current capabilities. Preserve preflight validation, explicit authority, replayability, and bounded termination when adding them.
