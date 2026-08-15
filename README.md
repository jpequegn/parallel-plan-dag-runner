# Parallel Plan DAG Runner

A Rust learning project for executing typed plan graphs with bounded concurrency, explicit
verification, provenance, deterministic replay, and bounded replanning.

The project implements [project-ideas #231](https://github.com/jpequegn/project-ideas/issues/231).

## Architecture

- `runner-core`: provider-neutral plan contracts and execution primitives.
- `runner-cli`: the `plan-runner` command-line interface.
- `web`: a later WASM validation and replay visualizer.

Live tools remain native. The browser build will validate plans and replay recorded events only.

## Development

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p runner-cli -- --help
```

## Status

The workspace is being implemented through repository issues. The initial scaffold intentionally
contains no execution behavior.

## License

MIT

Typed Rust DAG runner for verified parallel plans, replay, and bounded replanning
