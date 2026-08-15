# Parallel Plan DAG Runner

A deterministic Rust runner for typed plan DAGs with bounded concurrency, explicit verification,
provenance, append-only replay, bounded replanning, and a Rust/WASM browser inspector.

This repository implements [project-ideas #231](https://github.com/jpequegn/project-ideas/issues/231).

## What It Demonstrates

- Preflight rejection of cycles, invalid references, type mismatches, undeclared tools, and authority escalation.
- Concurrent execution of ready nodes under a plan-level limit, with deterministic sequential mode for comparison.
- Versioned verifier evidence and explicit stop, retry, degrade, or replan failure policies.
- Hash-chained SQLite event streams that replay without invoking tools.
- Bounded plan patches that cannot alter completed immutable work or broaden authority.
- A 54-run evaluation suite comparing sequential, parallel, and intentionally flawed dependency graphs.
- Browser-only Rust/WASM validation and recorded-event replay with no native tool execution.

## Quick Start

Prerequisites: stable Rust, Node.js 22 or newer, and npm.

```bash
cargo run -p runner-cli -- validate examples/basic-plan.yaml
cargo run -p runner-cli -- run examples/basic-plan.yaml --db /tmp/plan-runs.db
cargo run -p runner-cli -- runs --db /tmp/plan-runs.db
```

Use the `run_id` returned by `run`:

```bash
cargo run -p runner-cli -- replay RUN_ID --db /tmp/plan-runs.db
cargo run -p runner-cli -- inspect RUN_ID --db /tmp/plan-runs.db > /tmp/events.json
```

Launch the visualizer and load a plan plus `/tmp/events.json` with the two file buttons:

```bash
cd web
npm install
npm run dev
```

Open `http://127.0.0.1:5173/`. The app includes a sample run, replay controls, provenance and verifier inspection, and the baseline sequential/parallel comparison.

## Examples

```bash
cargo run -p runner-cli -- run examples/research-plan.yaml --db /tmp/research.db
cargo run -p runner-cli -- run examples/risk-plan.yaml --db /tmp/risk.db
cargo run -p runner-cli -- run examples/endurance-plan.yaml --db /tmp/endurance.db
```

All bundled examples use deterministic local tools. `document_lookup` and `fixture_http` exist for injected test fixtures; the CLI intentionally does not perform live network access.

## Evaluation

```bash
cargo run -p runner-cli -- evaluate \
  --fixtures benchmarks/fixtures.json \
  --output reports/latest
```

The checked-in baseline covers 18 graph shapes and 54 runs. It observed a material parallelism break-even point at graph width 2, using a greater-than-10-percent speedup threshold. Timings depend on the machine; correctness and flawed-merge detection are the stable signals.

## Verification

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cd web
npm ci
npm test
npm run build
```

## Documentation

- [Architecture](docs/architecture.md)
- [Plan and event formats](docs/file-formats.md)
- [Security and authority model](docs/security.md)
- [Evaluation guide](docs/evaluation.md)
- [Usage patterns and extensions](docs/usage-and-extensions.md)
- [Troubleshooting](docs/troubleshooting.md)
- [Release checklist](RELEASE_CHECKLIST.md)
- [Project status](PROJECT_STATUS.md)

## License

MIT
