import {
  ChartNoAxesColumnIncreasing,
  createIcons,
  FileJson,
  FolderOpen,
  GitBranch,
  Pause,
  Play,
  RotateCcw,
  ShieldCheck,
} from "lucide";
import { replayEvents, validatePlan } from "./bridge";
import { graphLevels, nodeStatesAtTransition, statusTone, transitionRange } from "./model";
import { sampleEvents, samplePlan, widthComparisons } from "./samples";
import type { Diagnostic, Plan, Replay } from "./types";
import "./style.css";

type Tab = "plan" | "replay" | "compare";

const state: {
  tab: Tab;
  plan?: Plan;
  replay?: Replay;
  diagnostics: Diagnostic[];
  selected?: string;
  engine: "loading" | "wasm" | "error";
  playbackCursor: number;
  playing: boolean;
  error?: string;
} = {
  tab: "plan",
  diagnostics: [],
  engine: "loading",
  playbackCursor: -1,
  playing: false,
};

let playbackTimer: number | undefined;

const app = document.querySelector<HTMLDivElement>("#app");
if (!app) throw new Error("app root is missing");
const root = app;

function escape(value: unknown): string {
  return String(value)
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#039;");
}

function render(): void {
  const plan = state.plan;
  const replay = state.replay;
  root.innerHTML = `
    <header class="topbar">
      <div class="brand"><i data-lucide="git-branch"></i><strong>Plan DAG Inspector</strong></div>
      <div class="engine ${state.engine}"><span></span>${state.engine === "wasm" ? "Rust/WASM" : state.engine}</div>
      <div class="actions">
        <input id="plan-file" type="file" accept=".json,.yaml,.yml" hidden />
        <input id="event-file" type="file" accept=".json" hidden />
        <button class="icon-button" id="open-plan" title="Open plan" aria-label="Open plan"><i data-lucide="file-json"></i></button>
        <button class="icon-button" id="open-events" title="Open event stream" aria-label="Open event stream"><i data-lucide="folder-open"></i></button>
      </div>
    </header>
    <nav class="tabs" aria-label="Views">
      ${tabButton("plan", "Plan", "git-branch")}
      ${tabButton("replay", "Replay", "play")}
      ${tabButton("compare", "Compare", "chart-no-axes-column-increasing")}
    </nav>
    <main>
      ${state.error ? `<div class="error-banner">${escape(state.error)}</div>` : ""}
      ${state.tab === "plan" ? renderPlan(plan) : ""}
      ${state.tab === "replay" ? renderReplay(plan, replay) : ""}
      ${state.tab === "compare" ? renderCompare() : ""}
    </main>`;
  bindEvents();
  createIcons({
    icons: { ChartNoAxesColumnIncreasing, FileJson, FolderOpen, GitBranch, Pause, Play, RotateCcw, ShieldCheck },
  });
}

function tabButton(tab: Tab, label: string, icon: string): string {
  return `<button class="tab ${state.tab === tab ? "active" : ""}" data-tab="${tab}" aria-selected="${state.tab === tab}"><i data-lucide="${icon}"></i>${label}</button>`;
}

function renderPlan(plan?: Plan): string {
  if (!plan) return `<section class="empty">No validated plan</section>`;
  const valid = state.diagnostics.length === 0;
  return `
    <section class="summary-band">
      <div><span class="label">Plan</span><strong>${escape(plan.id)}</strong></div>
      <div><span class="label">Nodes</span><strong>${plan.nodes.length}</strong></div>
      <div><span class="label">Concurrency</span><strong>${plan.limits.max_concurrency}</strong></div>
      <div><span class="label">Preflight</span><strong class="tone-${valid ? "good" : "bad"}">${valid ? "Valid" : `${state.diagnostics.length} errors`}</strong></div>
    </section>
    <section class="workspace-grid">
      <div class="graph-surface">${renderGraph(plan, state.replay)}</div>
      <aside class="inspector">${renderInspector(plan, state.replay)}</aside>
    </section>
    ${state.diagnostics.length ? renderDiagnostics(state.diagnostics) : ""}`;
}

function renderGraph(plan: Plan, replay?: Replay): string {
  const levels = graphLevels(plan);
  const maxLevel = Math.max(1, ...levels.values());
  const positions = new Map<string, { x: number; y: number }>();
  for (let level = 0; level <= maxLevel; level += 1) {
    const nodes = plan.nodes.filter((node) => levels.get(node.id) === level);
    nodes.forEach((node, index) => {
      positions.set(node.id, {
        x: 8 + (level / maxLevel) * 76,
        y: 12 + ((index + 1) / (nodes.length + 1)) * 72,
      });
    });
  }
  const edges = plan.nodes
    .flatMap((node) =>
      node.dependencies.map((dependency) => {
        const from = positions.get(dependency);
        const to = positions.get(node.id);
        if (!from || !to) return "";
        return `<line x1="${from.x + 8}" y1="${from.y + 4}" x2="${to.x}" y2="${to.y + 4}" />`;
      }),
    )
    .join("");
  const nodes = plan.nodes
    .map((node) => {
      const position = positions.get(node.id)!;
      const tone = statusTone(replay?.nodes[node.id]?.state);
      return `<button class="dag-node tone-${tone} ${state.selected === node.id ? "selected" : ""}" data-node="${escape(node.id)}" style="left:${position.x}%;top:${position.y}%">
        <span>${escape(node.id)}</span><small>${escape(replay?.nodes[node.id]?.state ?? node.tool)}</small>
      </button>`;
    })
    .join("");
  return `<div class="graph-head"><span>DAG</span><span>${plan.nodes.reduce((count, node) => count + node.dependencies.length, 0)} edges</span></div><div class="graph-canvas"><svg aria-hidden="true" viewBox="0 0 100 100" preserveAspectRatio="none">${edges}</svg>${nodes}</div>`;
}

function renderInspector(plan: Plan, replay?: Replay): string {
  const node = plan.nodes.find((candidate) => candidate.id === state.selected) ?? plan.nodes[0];
  if (!node) return "";
  const recorded = replay?.nodes[node.id];
  return `
    <div class="inspector-title"><span>Node</span><strong>${escape(node.id)}</strong></div>
    <dl>
      <dt>Objective</dt><dd>${escape(node.objective)}</dd>
      <dt>Tool</dt><dd>${escape(node.tool)}</dd>
      <dt>Output</dt><dd>${escape(node.output)}</dd>
      <dt>Dependencies</dt><dd>${node.dependencies.length ? node.dependencies.map(escape).join(", ") : "None"}</dd>
      <dt>Authority</dt><dd>${node.authority.map(escape).join(", ")}</dd>
      <dt>Timeout</dt><dd>${node.timeout_ms} ms</dd>
    </dl>
    ${recorded?.verifier ? `<div class="evidence"><span>Verifier</span><pre>${escape(JSON.stringify(recorded.verifier, null, 2))}</pre></div>` : ""}
    ${recorded?.provenance ? `<div class="evidence"><span>Provenance</span><pre>${escape(JSON.stringify(recorded.provenance, null, 2))}</pre></div>` : ""}`;
}

function renderDiagnostics(diagnostics: Diagnostic[]): string {
  return `<section class="diagnostics"><h2>Preflight diagnostics</h2>${diagnostics
    .map(
      (item) => `<div><code>${escape(item.code)}</code><span>${escape(item.node_id ?? item.path)}</span><p>${escape(item.message)}</p></div>`,
    )
    .join("")}</section>`;
}

function renderReplay(plan?: Plan, replay?: Replay): string {
  if (!plan || !replay) return `<section class="empty">No recorded run</section>`;
  const range = transitionRange(replay.transitions);
  const cursor = Math.min(Math.max(state.playbackCursor, -1), replay.transitions.length - 1);
  const states = nodeStatesAtTransition(plan, replay.transitions, cursor);
  const current = replay.transitions[cursor];
  const currentTimestamp = current?.timestamp_ms ?? range.start;
  const terminal = cursor === replay.transitions.length - 1;
  const rows = plan.nodes
    .map((node) => {
      const events = replay.transitions.filter((transition) => transition.node_id === node.id);
      const times = events.map((event) => event.timestamp_ms).filter((value): value is number => value !== undefined);
      const start = times.length ? Math.min(...times) : range.start;
      const end = times.length ? Math.max(...times) : start;
      const left = ((start - range.start) / range.duration) * 86;
      const visible = currentTimestamp >= start;
      const visibleEnd = Math.min(end, Math.max(start, currentTimestamp));
      const visibleWidth = Math.max(2, ((visibleEnd - start) / range.duration) * 86);
      const nodeState = states[node.id] ?? "pending";
      const tone = statusTone(nodeState);
      return `<div class="timeline-row"><button data-node="${escape(node.id)}">${escape(node.id)}</button><div class="track"><span class="tone-${tone}" style="left:${left}%;width:${visibleWidth}%;${visible ? "" : "display:none"}"></span></div><small>${escape(nodeState)}</small></div>`;
    })
    .join("");
  return `
    <section class="summary-band replay-summary">
      <div><span class="label">Run state</span><strong class="tone-${terminal ? statusTone(replay.status) : "neutral"}">${escape(terminal ? replay.status : state.playing ? "replaying" : "paused")}</strong></div>
      <div><span class="label">Transitions</span><strong>${replay.transitions.length}</strong></div>
      <div><span class="label">Duration</span><strong>${range.duration} ms</strong></div>
      <div><span class="label">Tool execution</span><strong>Native only</strong></div>
    </section>
    <section class="replay-layout">
      <div class="timeline">
        <div class="timeline-head"><span>Recorded node timeline</span><span>${range.start}-${range.start + range.duration} ms</span></div>
        <div class="playback-controls">
          <button class="icon-button compact" id="playback-toggle" title="${state.playing ? "Pause replay" : "Play replay"}" aria-label="${state.playing ? "Pause replay" : "Play replay"}"><i data-lucide="${state.playing ? "pause" : "play"}"></i></button>
          <button class="icon-button compact" id="playback-reset" title="Reset replay" aria-label="Reset replay"><i data-lucide="rotate-ccw"></i></button>
          <input id="playback-cursor" type="range" min="-1" max="${replay.transitions.length - 1}" value="${cursor}" aria-label="Replay sequence" />
          <output>${cursor + 1} / ${replay.transitions.length}${current ? ` - ${escape(current.event_type)}` : ""}</output>
        </div>
        ${rows}
      </div>
      <aside class="inspector">${renderInspector(plan, replay)}</aside>
    </section>`;
}

function renderCompare(): string {
  const max = Math.max(...widthComparisons.map((item) => item.sequential));
  return `
    <section class="summary-band">
      <div><span class="label">Fixture runs</span><strong>54</strong></div>
      <div><span class="label">Material break-even</span><strong>Width 2</strong></div>
      <div><span class="label">Flawed merges caught</span><strong class="tone-good">18 / 18</strong></div>
      <div><span class="label">Replay divergence</span><strong class="tone-good">0</strong></div>
    </section>
    <section class="comparison">
      <div class="comparison-head"><span>Wall time by graph width</span><span>milliseconds</span></div>
      ${widthComparisons
        .map(
          (item) => `<div class="comparison-row">
            <strong>W${item.width}</strong>
            <div class="bars"><span class="sequential" style="width:${(item.sequential / max) * 100}%"><i>${item.sequential}</i></span><span class="parallel" style="width:${(item.parallel / max) * 100}%"><i>${item.parallel}</i></span></div>
            <b>${item.speedup.toFixed(1)}x</b>
          </div>`,
        )
        .join("")}
      <div class="legend"><span><i class="sequential"></i>Sequential</span><span><i class="parallel"></i>Parallel</span></div>
    </section>`;
}

function bindEvents(): void {
  document.querySelectorAll<HTMLButtonElement>("[data-tab]").forEach((button) => {
    button.addEventListener("click", () => {
      stopPlayback();
      state.tab = button.dataset.tab as Tab;
      render();
    });
  });
  document.querySelectorAll<HTMLButtonElement>("[data-node]").forEach((button) => {
    button.addEventListener("click", () => {
      state.selected = button.dataset.node;
      render();
    });
  });
  document.querySelector("#open-plan")?.addEventListener("click", () =>
    document.querySelector<HTMLInputElement>("#plan-file")?.click(),
  );
  document.querySelector("#open-events")?.addEventListener("click", () =>
    document.querySelector<HTMLInputElement>("#event-file")?.click(),
  );
  document.querySelector<HTMLInputElement>("#plan-file")?.addEventListener("change", (event) => {
    const file = (event.target as HTMLInputElement).files?.[0];
    if (file) void loadPlanFile(file);
  });
  document.querySelector<HTMLInputElement>("#event-file")?.addEventListener("change", (event) => {
    const file = (event.target as HTMLInputElement).files?.[0];
    if (file) void loadEventFile(file);
  });
  document.querySelector("#playback-toggle")?.addEventListener("click", () => {
    if (state.playing) stopPlayback(true);
    else startPlayback();
  });
  document.querySelector("#playback-reset")?.addEventListener("click", () => {
    stopPlayback();
    state.playbackCursor = -1;
    render();
  });
  document.querySelector<HTMLInputElement>("#playback-cursor")?.addEventListener("change", (event) => {
    stopPlayback();
    state.playbackCursor = Number((event.target as HTMLInputElement).value);
    render();
  });
}

function stopPlayback(renderAfter = false): void {
  if (playbackTimer !== undefined) window.clearInterval(playbackTimer);
  playbackTimer = undefined;
  state.playing = false;
  if (renderAfter) render();
}

function startPlayback(): void {
  const transitions = state.replay?.transitions;
  if (!transitions?.length) return;
  if (state.playbackCursor >= transitions.length - 1) state.playbackCursor = -1;
  state.playing = true;
  render();
  playbackTimer = window.setInterval(() => {
    state.playbackCursor += 1;
    if (state.playbackCursor >= transitions.length - 1) stopPlayback();
    render();
  }, 240);
}

async function loadPlanFile(file: File): Promise<void> {
  try {
    const response = await validatePlan(await file.text(), file.name.split(".").pop() ?? "json");
    state.plan = response.plan;
    state.diagnostics = response.diagnostics;
    state.selected = response.plan?.nodes[0]?.id;
    state.error = response.error;
  } catch (error) {
    state.error = error instanceof Error ? error.message : String(error);
  }
  render();
}

async function loadEventFile(file: File): Promise<void> {
  try {
    const response = await replayEvents(await file.text());
    state.replay = response.replay;
    state.playbackCursor = response.replay ? response.replay.transitions.length - 1 : -1;
    state.error = response.error;
    state.tab = "replay";
  } catch (error) {
    state.error = error instanceof Error ? error.message : String(error);
  }
  render();
}

async function initialize(): Promise<void> {
  render();
  try {
    const [validation, replay] = await Promise.all([
      validatePlan(samplePlan, "json"),
      replayEvents(sampleEvents),
    ]);
    state.plan = validation.plan;
    state.diagnostics = validation.diagnostics;
    state.replay = replay.replay;
    state.playbackCursor = replay.replay ? replay.replay.transitions.length - 1 : -1;
    state.selected = validation.plan?.nodes[0]?.id;
    state.engine = "wasm";
    state.error = validation.error ?? replay.error;
  } catch (error) {
    state.engine = "error";
    state.error = error instanceof Error ? error.message : String(error);
  }
  render();
}

void initialize();
