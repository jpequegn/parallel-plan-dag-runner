# Architecture

## Execution Path

```text
YAML/JSON plan
  -> parser and preflight validator
  -> dependency scheduler
  -> authority-checked deterministic tools
  -> node and final verifiers
  -> explicit failure-policy decision
  -> hash-chained SQLite event ledger
  -> tool-free native or WASM replay
```

`runner-core` owns the contracts and behavior. `runner-cli` is a thin native adapter around those APIs. `runner-wasm` compiles only parsing, validation, and event reduction; Cargo's `native` feature gates the executor, tools, SQLite ledger, evaluation harness, and replanner. `web` is a Vite/TypeScript inspector over the WASM exports.

## Scheduler

Preflight constructs a DAG and rejects cycles before execution. A node becomes ready only when every declared dependency has a successful or degraded output. The executor starts ready nodes in stable ID order until `max_concurrency` is reached. Sequential mode uses the same scheduler with a limit of one, which makes output comparisons meaningful.

Independent node completion order can vary. Outputs, states, and event semantics remain deterministic for deterministic tools; consumers must not infer dependencies from completion order.

## Verification and Failure

Each successful tool response is a candidate until its node verifier accepts it. Verifiers emit versioned evidence. A rejected candidate follows exactly one declared policy: stop, retry within budget, accept a separately verified fallback, or hand control to bounded replanning. A final verifier can reject the assembled output after all nodes complete.

## Replanning

A replanner receives the failed node, completed dependency outputs, verifier evidence, and current plan digest. Returned patches must match that digest, stay within time/count/growth limits, retain completed immutable nodes, preserve authority boundaries, and validate as an acyclic typed plan. Completed dependencies are seeded into the repaired run and are not executed again.

## Persistence and Replay

SQLite stores each event with a sequence, previous digest, and digest. Database triggers prohibit event update and deletion. Inspection verifies schema version, sequence continuity, and the complete digest chain. Replay reconstructs state and outputs only from recorded events and never calls a tool.

The browser reducer accepts the same event envelope JSON emitted by `plan-runner inspect`. It provides review and animation, not execution.
