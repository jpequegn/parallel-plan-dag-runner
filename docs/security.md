# Security and Authority Model

## Boundaries

- Plans use explicit allowlists for tools and capabilities. Validation and runtime both reject authority escalation.
- The native registry contains deterministic calculator, JSON transform, injected document, and injected HTTP-fixture tools. It has no arbitrary shell tool and performs no live HTTP requests.
- Inputs are structured values and typed references. The runner does not interpolate or evaluate generated shell commands.
- SQLite events are append-only and hash chained. This detects mutation; it is not a digital signature and does not establish who produced a run.
- Replanning is bounded by plan digest, authority, immutable completed nodes, node growth, count, and elapsed time.

## Reasoning Privacy

The runner records declared objectives, structured inputs, tool responses, provenance, verifier evidence, and state transitions. It neither requests nor stores hidden chain-of-thought. Do not put private model reasoning or secrets into objectives, inputs, evidence, or tool outputs because those values are inspectable in the event ledger.

## Browser Boundary

The WASM crate is compiled without `runner-core`'s `native` feature. It can parse and validate plans and reduce already-recorded event JSON. It cannot execute native tools, access SQLite, replan, or run the evaluation harness. The visualizer performs no live tool execution; loaded files remain in browser memory unless the hosting environment adds separate telemetry.

## Production Extensions

Before adding real connectors, require per-tool credential scopes, outbound destination policy, secret redaction, signed event checkpoints, storage access controls, retention limits, and adversarial tests for confused-deputy behavior. Treat a planner as untrusted input: validate its plan before granting any execution capability.
