export const samplePlan = JSON.stringify(
  {
    version: "v1",
    id: "research-synthesis",
    authority: {
      tools: ["document_lookup", "json_transform"],
      capabilities: ["read_documents", "compute"],
    },
    limits: {
      max_concurrency: 3,
      max_replans: 1,
      max_node_growth: 2,
      max_replan_wall_time_ms: 30000,
    },
    nodes: [
      {
        id: "source-a",
        objective: "Extract claims from source A",
        dependencies: [],
        inputs: { document_id: { kind: "literal", value: "source-a", type: "string" } },
        output: "string",
        tool: "document_lookup",
        authority: ["read_documents"],
      },
      {
        id: "source-b",
        objective: "Extract claims from source B",
        dependencies: [],
        inputs: { document_id: { kind: "literal", value: "source-b", type: "string" } },
        output: "string",
        tool: "document_lookup",
        authority: ["read_documents"],
      },
      {
        id: "source-c",
        objective: "Extract claims from source C",
        dependencies: [],
        inputs: { document_id: { kind: "literal", value: "source-c", type: "string" } },
        output: "string",
        tool: "document_lookup",
        authority: ["read_documents"],
      },
      {
        id: "synthesis",
        objective: "Merge verified claims",
        dependencies: ["source-a", "source-b", "source-c"],
        inputs: {
          a: { kind: "reference", node: "source-a", type: "string" },
          b: { kind: "reference", node: "source-b", type: "string" },
          c: { kind: "reference", node: "source-c", type: "string" },
        },
        output: "object",
        tool: "json_transform",
        authority: ["compute"],
      },
    ],
  },
  null,
  2,
);

const output = (node: string, value: unknown) => ({
  value,
  provenance: {
    node_id: node,
    tool_name: node === "synthesis" ? "json_transform" : "document_lookup",
    content_digest: `${node}-8bc9f3`,
    response_digest: `${node}-42ac11`,
  },
});

export const sampleEvents = JSON.stringify(
  [
    { sequence: 0, timestamp_ms: 0, event: { type: "run_started", plan_id: "research-synthesis", mode: "parallel", nodes: ["source-a", "source-b", "source-c", "synthesis"] } },
    { sequence: 1, timestamp_ms: 10, event: { type: "node_started", node_id: "source-a" } },
    { sequence: 2, timestamp_ms: 12, event: { type: "node_started", node_id: "source-b" } },
    { sequence: 3, timestamp_ms: 14, event: { type: "node_started", node_id: "source-c" } },
    { sequence: 4, timestamp_ms: 15, event: { type: "tool_call", node_id: "source-a", tool: "document_lookup", inputs: {} } },
    { sequence: 5, timestamp_ms: 16, event: { type: "tool_call", node_id: "source-b", tool: "document_lookup", inputs: {} } },
    { sequence: 6, timestamp_ms: 17, event: { type: "tool_call", node_id: "source-c", tool: "document_lookup", inputs: {} } },
    { sequence: 7, timestamp_ms: 83, event: { type: "tool_response", node_id: "source-b", output: output("source-b", "Claim B") } },
    { sequence: 8, timestamp_ms: 84, event: { type: "verifier_result", node_id: "source-b", accepted: true, evidence: { verifier: "always", version: "v1", accepted: true, reason: "accepted", input_digest: "b" } } },
    { sequence: 9, timestamp_ms: 85, event: { type: "node_succeeded", node_id: "source-b" } },
    { sequence: 10, timestamp_ms: 91, event: { type: "tool_response", node_id: "source-a", output: output("source-a", "Claim A") } },
    { sequence: 11, timestamp_ms: 92, event: { type: "verifier_result", node_id: "source-a", accepted: true, evidence: { verifier: "always", version: "v1", accepted: true, reason: "accepted", input_digest: "a" } } },
    { sequence: 12, timestamp_ms: 93, event: { type: "node_succeeded", node_id: "source-a" } },
    { sequence: 13, timestamp_ms: 98, event: { type: "tool_response", node_id: "source-c", output: output("source-c", "Claim C") } },
    { sequence: 14, timestamp_ms: 99, event: { type: "verifier_result", node_id: "source-c", accepted: true, evidence: { verifier: "always", version: "v1", accepted: true, reason: "accepted", input_digest: "c" } } },
    { sequence: 15, timestamp_ms: 100, event: { type: "node_succeeded", node_id: "source-c" } },
    { sequence: 16, timestamp_ms: 104, event: { type: "node_started", node_id: "synthesis" } },
    { sequence: 17, timestamp_ms: 105, event: { type: "tool_call", node_id: "synthesis", tool: "json_transform", inputs: {} } },
    { sequence: 18, timestamp_ms: 174, event: { type: "tool_response", node_id: "synthesis", output: output("synthesis", { a: "Claim A", b: "Claim B", c: "Claim C" }) } },
    { sequence: 19, timestamp_ms: 175, event: { type: "verifier_result", node_id: "synthesis", accepted: true, evidence: { verifier: "json_schema", version: "v1", accepted: true, reason: "value satisfies output schema", input_digest: "s" } } },
    { sequence: 20, timestamp_ms: 176, event: { type: "node_succeeded", node_id: "synthesis" } },
    { sequence: 21, timestamp_ms: 178, event: { type: "run_completed", status: "succeeded" } },
  ],
  null,
  2,
);

export const widthComparisons = [
  { width: 1, speedup: 1.0, sequential: 16.5, parallel: 16.4 },
  { width: 2, speedup: 1.2, sequential: 20.5, parallel: 17.1 },
  { width: 3, speedup: 1.5, sequential: 24.6, parallel: 16.4 },
  { width: 4, speedup: 1.8, sequential: 28.7, parallel: 15.9 },
  { width: 5, speedup: 2.1, sequential: 32.8, parallel: 15.6 },
  { width: 6, speedup: 2.5, sequential: 36.9, parallel: 14.8 },
];
