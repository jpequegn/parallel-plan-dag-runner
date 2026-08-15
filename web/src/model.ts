import type { Plan, Transition } from "./types";

export function graphLevels(plan: Plan): Map<string, number> {
  const levels = new Map<string, number>();
  const visit = (id: string, path: Set<string>): number => {
    if (levels.has(id)) return levels.get(id)!;
    if (path.has(id)) return 0;
    const node = plan.nodes.find((candidate) => candidate.id === id);
    if (!node || node.dependencies.length === 0) {
      levels.set(id, 0);
      return 0;
    }
    const nextPath = new Set(path).add(id);
    const level = Math.max(...node.dependencies.map((dependency) => visit(dependency, nextPath))) + 1;
    levels.set(id, level);
    return level;
  };
  plan.nodes.forEach((node) => visit(node.id, new Set()));
  return levels;
}

export function transitionRange(transitions: Transition[]): { start: number; duration: number } {
  const timestamps = transitions
    .map((transition) => transition.timestamp_ms)
    .filter((value): value is number => value !== undefined);
  if (timestamps.length === 0) return { start: 0, duration: Math.max(1, transitions.length - 1) };
  const start = Math.min(...timestamps);
  return { start, duration: Math.max(1, Math.max(...timestamps) - start) };
}

export function statusTone(state?: string): "good" | "warn" | "bad" | "neutral" {
  if (state === "succeeded" || state === "degraded") return state === "succeeded" ? "good" : "warn";
  if (state === "failed" || state === "timed_out" || state === "blocked") return "bad";
  if (state === "needs_replan") return "warn";
  return "neutral";
}

export function nodeStatesAtTransition(
  plan: Plan,
  transitions: Transition[],
  cursor: number,
): Record<string, string> {
  const states = Object.fromEntries(plan.nodes.map((node) => [node.id, "pending"]));
  for (const transition of transitions.slice(0, cursor + 1)) {
    if (!transition.node_id) continue;
    const next = transitionState(transition.event_type);
    if (next) states[transition.node_id] = next;
  }
  return states;
}

function transitionState(eventType: string): string | undefined {
  const states: Record<string, string> = {
    node_started: "running",
    node_succeeded: "succeeded",
    node_degraded: "degraded",
    node_failed: "failed",
    node_timed_out: "timed_out",
    node_blocked: "blocked",
    node_cancelled: "cancelled",
    node_needs_replan: "needs_replan",
    retry: "pending",
  };
  return states[eventType];
}
