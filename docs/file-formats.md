# Plan and Event Formats

## Plan v1

A plan contains `version`, `id`, an authority envelope, execution limits, and nodes. Generate the authoritative JSON Schema with:

```bash
cargo run -p runner-cli -- schema --output /tmp/plan-v1.schema.json
```

Each node declares its dependencies, typed literal/reference inputs, output type, tool, capabilities, timeout, retry budget, verifier, failure policy, optional degrade value, and immutability. References use a source node plus an optional RFC 6901 JSON Pointer in `path`; interpolation inside strings is intentionally unsupported.

Supported value types are `any`, `null`, `bool`, `number`, `string`, `array`, and `object`. Supported deterministic verifiers are `always`, `equals`, `numeric_range`, `expression`, and `json_schema`.

The authority envelope is an allowlist. A node tool must appear in `authority.tools`, and every node capability must appear in `authority.capabilities`. Runtime tools also enforce their required capability.

## Event Envelope v1

`plan-runner inspect` emits an array of envelopes with:

- `schema_version`: event contract version.
- `run_id`: owning run.
- `sequence`: zero-based contiguous sequence.
- `timestamp_ms`: recording timestamp.
- `previous_digest`: digest of the preceding envelope payload.
- `digest`: digest covering the sequence, previous digest, and event.
- `event`: tagged event payload.

Events cover run lifecycle, node lifecycle, tool calls/responses, verifier results, retries, replans, cancellation, and terminal status. Tool responses include value plus provenance digests. `inspect` rejects gaps, tampering, unsupported schema versions, and a terminal digest mismatch before returning JSON.

## Compatibility

The implementation accepts only plan version `v1` and event schema version 1. Producers should fail closed on unknown versions. Format changes require fixtures, native/WASM parity tests, migration notes, and a new version rather than silent reinterpretation.
