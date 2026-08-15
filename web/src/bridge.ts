import type { ReplayResponse, ValidationResponse } from "./types";

type WasmModule = {
  default: () => Promise<unknown>;
  validate_plan_json: (source: string, format: string) => string;
  replay_events_json: (source: string) => string;
};

let modulePromise: Promise<WasmModule> | undefined;

async function load(): Promise<WasmModule> {
  modulePromise ??= import("./wasm/runner_wasm.js").then(async (module) => {
    const wasm = module as WasmModule;
    await wasm.default();
    return wasm;
  });
  return modulePromise;
}

export async function validatePlan(source: string, format: string): Promise<ValidationResponse> {
  const wasm = await load();
  return JSON.parse(wasm.validate_plan_json(source, format)) as ValidationResponse;
}

export async function replayEvents(source: string): Promise<ReplayResponse> {
  const wasm = await load();
  return JSON.parse(wasm.replay_events_json(source)) as ReplayResponse;
}
