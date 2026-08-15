# Troubleshooting

## Plan Fails Preflight

Run `plan-runner validate` and read the diagnostic code, node ID, and path. Common causes are a cycle, missing dependency, reference to a non-dependency, literal/reference type mismatch, undeclared tool, capability outside the plan authority, zero concurrency, or an incomplete degrade/verifier contract.

## Run Fails Immediately

The CLI registry is intentionally deterministic. `document_lookup` and `fixture_http` need injected fixtures through the library API and will fail from the stock CLI when data is absent. Confirm each tool's required capability is declared at plan and node level.

## Replay Rejects a Database

`inspect` and `replay` verify contiguous sequence numbers, previous digests, event digests, schema version, event count, and terminal digest. A failure indicates corruption, manual modification, or an incompatible event version. Preserve the database for investigation; do not rewrite events to make it pass.

## WASM Build Fails

Use stable Rust, Node.js 22 or newer, and npm. `npm run build` invokes `wasm-pack`, installs the `wasm32-unknown-unknown` target when needed, type-checks TypeScript, and builds Vite. Corporate proxies may require the Rust target and npm dependencies to be preinstalled.

## Visualizer Shows No Run

Load a valid YAML/JSON plan with the document button and the JSON array from `plan-runner inspect` with the folder button. `plan-runner replay` emits a reconstructed result, not the event envelope array expected by the browser.

## Parallel Speedup Is Small

Width-one graphs cannot exploit concurrency, and short tools may cost less than scheduling overhead. Compare `reports/latest` with the baseline, check graph width and critical path, and increase fixture delay to model the intended workload rather than assuming parallel execution is always faster.
