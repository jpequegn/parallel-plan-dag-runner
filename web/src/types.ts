export type PlanNode = {
  id: string;
  objective: string;
  dependencies: string[];
  tool: string;
  authority: string[];
  timeout_ms: number;
  output: string;
};

export type Plan = {
  id: string;
  version: string;
  nodes: PlanNode[];
  limits: { max_concurrency: number };
};

export type Diagnostic = {
  code: string;
  path: string;
  message: string;
  node_id?: string;
};

export type NodeReplay = {
  state: string;
  tool?: string;
  output?: unknown;
  provenance?: Record<string, unknown>;
  verifier?: Record<string, unknown>;
};

export type Transition = {
  sequence: number;
  event_type: string;
  node_id?: string;
  timestamp_ms?: number;
};

export type Replay = {
  status: string;
  nodes: Record<string, NodeReplay>;
  transitions: Transition[];
};

export type ValidationResponse = {
  ok: boolean;
  plan?: Plan;
  diagnostics: Diagnostic[];
  error?: string;
};

export type ReplayResponse = {
  ok: boolean;
  replay?: Replay;
  error?: string;
};
