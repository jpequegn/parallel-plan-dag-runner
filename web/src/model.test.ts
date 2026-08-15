import { describe, expect, it } from "vitest";
import { graphLevels, nodeStatesAtTransition, statusTone, transitionRange } from "./model";
import type { Plan } from "./types";

const plan: Plan = {
  id: "p",
  version: "v1",
  limits: { max_concurrency: 2 },
  nodes: [
    { id: "a", objective: "a", dependencies: [], tool: "t", authority: [], timeout_ms: 1, output: "number" },
    { id: "b", objective: "b", dependencies: [], tool: "t", authority: [], timeout_ms: 1, output: "number" },
    { id: "c", objective: "c", dependencies: ["a", "b"], tool: "t", authority: [], timeout_ms: 1, output: "number" },
  ],
};

describe("visualizer model", () => {
  it("lays out independent nodes at the same level", () => {
    const levels = graphLevels(plan);
    expect(levels.get("a")).toBe(0);
    expect(levels.get("b")).toBe(0);
    expect(levels.get("c")).toBe(1);
  });

  it("derives stable timestamp ranges", () => {
    expect(
      transitionRange([
        { sequence: 0, event_type: "start", timestamp_ms: 10 },
        { sequence: 1, event_type: "end", timestamp_ms: 25 },
      ]),
    ).toEqual({ start: 10, duration: 15 });
  });

  it("maps terminal states to semantic tones", () => {
    expect(statusTone("succeeded")).toBe("good");
    expect(statusTone("needs_replan")).toBe("warn");
    expect(statusTone("failed")).toBe("bad");
  });

  it("reconstructs node states at an event cursor", () => {
    const transitions = [
      { sequence: 0, event_type: "run_started" },
      { sequence: 1, event_type: "node_started", node_id: "a" },
      { sequence: 2, event_type: "node_succeeded", node_id: "a" },
      { sequence: 3, event_type: "node_started", node_id: "c" },
    ];
    expect(nodeStatesAtTransition(plan, transitions, 1)).toMatchObject({ a: "running", c: "pending" });
    expect(nodeStatesAtTransition(plan, transitions, 3)).toMatchObject({ a: "succeeded", c: "running" });
  });
});
