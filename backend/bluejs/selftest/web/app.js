// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

"use strict";
const $ = id => document.getElementById(id);
let state = null;
let graph = null;
let graphVersion = null;
let selectedSource = null;
let view = "graph";
let refreshInFlight = false;
const titles = {
  graph: ["Test impact graph", "Follow source changes to the tests that exercise them."],
  runs: ["Runs & progress", "Verify the affected tests, then complete the full engine gate."],
  reports: ["Coverage & reports", "Read verified results from one complete source snapshot."]
};

function node(tag, text = "", className = "") {
  const element = document.createElement(tag);
  element.textContent = text;
  if (className) element.className = className;
  return element;
}
function duration(seconds) {
  if (!Number.isFinite(seconds)) return "—";
  if (seconds < 60) return `${seconds.toFixed(seconds < 10 ? 1 : 0)}s`;
  return `${Math.floor(seconds / 60)}m ${Math.round(seconds % 60)}s`;
}
function shortPath(path) {
  return path.replace(/^backend\/bluejs\/(src|tests)\//, "");
}
function badge(status) {
  return node("span", status, `badge ${status}`);
}
function announce(message, error = false) {
  $("notice").textContent = message;
  $("notice").className = `notice${error ? " error" : ""}`;
  $("notice").hidden = false;
}
async function api(path, body) {
  const options = body === undefined ? {} : {
    method: "POST", headers: {"Content-Type": "application/json", "X-BlueJS-Token": state?.token || ""},
    body: JSON.stringify(body)
  };
  const response = await fetch(path, options);
  const value = await response.json();
  if (!response.ok) throw new Error(value.error || "Request failed");
  return value;
}
function setView(name) {
  view = name;
  document.querySelectorAll(".view").forEach(section => { section.hidden = section.id !== `view-${name}`; });
  document.querySelectorAll(".nav-item").forEach(button => button.classList.toggle("active", button.dataset.view === name));
  $("page-title").textContent = titles[name][0];
  $("page-description").textContent = titles[name][1];
  location.hash = name;
}
function renderPlan() {
  const plan = state.plan;
  const partitions = state.partitions;
  const correction = partitions && !(state.active?.status === "running" && state.active.mode === "full");
  $("selection-count").textContent = correction ? partitions.selected_targets : plan.selected_tests;
  $("selection-total").textContent = correction ? `of ${partitions.total_rust_targets} Rust targets · related fixture sets below` : `of ${plan.total_tests} current test targets`;
  $("estimated-time").textContent = correction ? "—" : plan.selected_tests && !plan.estimated_serial_seconds ? "—" : duration(plan.estimated_serial_seconds);
  $("snapshot-badge").textContent = `Snapshot ${plan.snapshot.slice(0, 10)}`;
  const summary = $("plan-summary");
  const files = correction ? partitions.changed_files.filter(path => path.endsWith(".rs")) : plan.changed_files;
  summary.replaceChildren(node("strong", correction ? `${files.length} changed Rust files · ${partitions.boundaries.length} mapped boundaries · ${partitions.selected_targets} selected Rust targets` : `${plan.changed_files.length} changed files · ${plan.affected_files.length} affected files · ${plan.selected_tests} selected targets`));
  summary.append(node("div", files.length ? files.map(shortPath).join(" · ") : "No source changes selected. Choose files or use the Git diff."));
  const fallback = $("fallbacks");
  fallback.hidden = correction ? !partitions.unknown_sources.length : !plan.fallbacks.length;
  fallback.textContent = correction ? `No case boundary recorded: ${partitions.unknown_sources.map(shortPath).join(" · ")}` : plan.fallbacks.length ? `Expanded verification: ${plan.fallbacks.slice(0, 4).join("; ")}${plan.fallbacks.length > 4 ? `; and ${plan.fallbacks.length - 4} other reasons` : ""}` : "";
}
function renderSources() {
  if (!graph) return;
  const search = $("source-search").value.toLowerCase();
  const sources = graph.sources.filter(path => path.toLowerCase().includes(search));
  $("source-count").textContent = `${sources.length} files`;
  const list = $("source-list");
  list.replaceChildren();
  for (const path of sources) {
    const button = node("button", shortPath(path), `source-item${path === selectedSource ? " active" : ""}`);
    button.type = "button";
    button.title = path;
    button.setAttribute("role", "listitem");
    button.addEventListener("click", () => { selectedSource = path; renderSources(); renderRelationships(); });
    list.append(button);
  }
  if (!sources.length) list.append(node("div", "No files match this search.", "empty"));
}
function relationships(source) {
  if (!graph) return [];
  const adjacent = new Map();
  for (const edge of graph.edges) {
    if (!adjacent.has(edge.source)) adjacent.set(edge.source, []);
    adjacent.get(edge.source).push(edge);
  }
  const seen = new Set([source]);
  const queue = [{source, kind: "observed"}];
  const found = new Map();
  while (queue.length) {
    const current = queue.shift();
    for (const edge of adjacent.get(current.source) || []) {
      const kind = current.kind === "static" || edge.kind !== "observed" ? "static" : "observed";
      if (graph.tests[edge.target]) {
        const existing = found.get(edge.target);
        if (!existing || existing.kind === "static") found.set(edge.target, {...graph.tests[edge.target], kind});
      } else if (!seen.has(edge.target)) {
        seen.add(edge.target);
        queue.push({source: edge.target, kind});
      }
    }
  }
  return [...found.values()].sort((a, b) => (a.kind === b.kind ? a.label.localeCompare(b.label) : a.kind === "observed" ? -1 : 1));
}
function svgElement(tag, attributes, text) {
  const element = document.createElementNS("http://www.w3.org/2000/svg", tag);
  for (const [key, value] of Object.entries(attributes)) element.setAttribute(key, value);
  if (text !== undefined) element.textContent = text;
  return element;
}
function renderRelationships() {
  renderSharedFunctions();
  const svg = $("relationship-graph");
  svg.replaceChildren();
  $("selected-source").textContent = selectedSource ? shortPath(selectedSource) : "Select a source file";
  $("add-source").disabled = !selectedSource;
  const related = selectedSource ? relationships(selectedSource) : [];
  $("related-count").textContent = `${related.length} targets`;
  $("graph-empty").hidden = related.length > 0;
  $("graph-empty").textContent = selectedSource ? "No measured relationships yet. The planner expands verification conservatively." : "Select a source file to explore its test relationships.";
  const visible = related.slice(0, 6);
  if (visible.length) {
    svg.append(svgElement("rect", {x: 32, y: 135, width: 265, height: 72, rx: 10, fill: "#edf4ff", stroke: "#9cbdf1"}));
    svg.append(svgElement("text", {x: 50, y: 157, fill: "#6483ae", "font-size": 10, "font-family": "sans-serif"}, "SOURCE FILE"));
    const label = shortPath(selectedSource);
    svg.append(svgElement("text", {x: 50, y: 181, fill: "#245da9", "font-size": 11, "font-family": "monospace"}, label.length > 31 ? "…" + label.slice(-30) : label));
    visible.forEach((test, index) => {
      const y = visible.length === 1 ? 142 : 22 + index * (270 / Math.max(1, visible.length - 1));
      const color = test.kind === "observed" ? "#8bb2ef" : "#b8c6d7";
      svg.append(svgElement("path", {d: `M 297 171 C 370 171, 365 ${y + 21}, 443 ${y + 21}`, fill: "none", stroke: color, "stroke-width": 1.6, "stroke-dasharray": test.kind === "static" ? "5 5" : "none"}));
      svg.append(svgElement("circle", {cx: 443, cy: y + 21, r: 3, fill: color}));
      svg.append(svgElement("rect", {x: 450, y, width: 288, height: 43, rx: 7, fill: "white", stroke: "#dce5f1"}));
      svg.append(svgElement("text", {x: 465, y: y + 17, fill: "#536f91", "font-size": 10, "font-family": "monospace"}, test.label.length > 37 ? test.label.slice(0, 34) + "…" : test.label));
      svg.append(svgElement("text", {x: 465, y: y + 32, fill: "#99a9bd", "font-size": 9, "font-family": "sans-serif"}, `${test.kind === "observed" ? "Measured execution" : "Static dependency path"} · ${duration(test.elapsed_seconds)}`));
    });
    if (related.length > visible.length) svg.append(svgElement("text", {x: 453, y: 337, fill: "#8b9db5", "font-size": 10, "font-family": "sans-serif"}, `+ ${related.length - visible.length} more targets in the list below`));
  }
  const table = $("related-tests");
  table.replaceChildren();
  for (const test of related.slice(0, 100)) {
    const row = node("tr");
    row.append(node("td", test.label), node("td", test.kind === "observed" ? "Observed execution" : "Static dependency"), node("td", duration(test.elapsed_seconds)));
    row.title = test.id;
    table.append(row);
  }
}
function renderSharedFunctions() {
  const list = $("shared-function-list");
  const functions = (graph?.shared_functions || []).filter(item =>
    item.definitions.some(definition => definition.source === selectedSource) ||
    item.references.some(reference => reference.source === selectedSource));
  $("shared-function-count").textContent = `${functions.length} functions`;
  list.replaceChildren();
  let displayed = 0;
  let total = 0;
  for (const item of functions) {
    const owned = item.definitions.some(definition => definition.source === selectedSource);
    for (const reference of item.references.filter(ref => owned || ref.source === selectedSource)) {
      total++;
      if (displayed++ >= 200) continue;
      const row = node("tr");
      const name = node("td", item.symbol);
      name.append(node("div", item.boundary, "muted"));
      row.append(name, node("td", `${shortPath(reference.source)} · ${reference.function || "module scope"}`), node("td", String(reference.line)));
      row.title = item.resolution;
      list.append(row);
    }
  }
  if (!total) {
    const row = node("tr");
    const cell = node("td", "Select a shared boundary or its caller to explore references.", "muted");
    cell.colSpan = 3;
    row.append(cell);
    list.append(row);
  } else if (total > 200) {
    const row = node("tr");
    const cell = node("td", `${total - 200} additional source references are retained in the graph data.`, "muted");
    cell.colSpan = 3;
    row.append(cell);
    list.append(row);
  }
}
async function showOutput(path, title) {
  const response = await fetch(path);
  if (!response.ok) throw new Error("Output is not available yet");
  $("output-title").textContent = title;
  $("output-text").textContent = await response.text();
  $("output-dialog").showModal();
}
function renderRun() {
  const run = state.active;
  const busy = run?.status === "running" || state.indexing.status === "running";
  $("impact-button").disabled = busy;
  $("partition-button").disabled = busy;
  $("pipeline-button").disabled = busy;
  $("full-button").disabled = busy || !state.ready_for_full;
  $("cancel-button").disabled = run?.status !== "running";
  $("index-button").disabled = busy;
  $("workspace-gates").disabled = busy;
  $("worker-setting").textContent = `${state.jobs} workers · ${state.test_threads} Rust test threads`;
  const tasks = run?.tasks || [];
  const finished = tasks.filter(task => ["passed", "failed", "cancelled", "skipped"].includes(task.status)).length;
  $("run-progress").style.width = `${tasks.length ? 100 * finished / tasks.length : 0}%`;
  $("run-title").textContent = run ? `${run.mode === "pipeline" ? "Affected → full" : run.mode} · ${run.id}` : "Execution progress";
  const status = $("run-status");
  const expired = run?.status === "passed" && run.snapshot !== state.plan.snapshot;
  const shownStatus = expired ? "stale" : run?.status || "idle";
  status.textContent = expired ? "stale" : run?.status || "No active run";
  status.className = `badge ${shownStatus}`;
  $("run-message").textContent = expired ? "Sources changed after this run passed. Run the affected selection again." : run ? `${finished}/${tasks.length} steps finished · ${run.message}` : "Your selected tests will appear here.";
  const list = $("task-list");
  list.replaceChildren();
  for (const task of tasks) {
    const row = node("tr");
    row.append(node("td", task.label), node("td", task.stage));
    const statusCell = node("td"); statusCell.append(badge(task.status)); row.append(statusCell);
    const seconds = task.status === "running" && task.started_at ? (Date.now() - Date.parse(task.started_at)) / 1000 : task.elapsed_seconds;
    row.append(node("td", duration(seconds)));
    const output = node("td");
    const button = node("button", "View log", "button compact");
    button.disabled = task.status === "queued";
    button.addEventListener("click", () => showOutput(`/api/log?run=${encodeURIComponent(run.id)}&task=${encodeURIComponent(task.id)}`, task.label).catch(e => announce(e.message, true)));
    output.append(button); row.append(output); list.append(row);
  }
  const history = $("history-list");
  history.replaceChildren();
  for (const retained of state.history) {
    const row = node("div", "", "history-row");
    const description = node("div", `${retained.mode} · ${retained.id}`);
    description.append(node("small", `Snapshot ${retained.snapshot.slice(0, 12)} · ${duration(retained.elapsed_seconds)}`));
    row.append(description, badge(retained.status));
    history.append(row);
  }
  if (!state.history.length) history.append(node("p", "No new runs yet. Retained engine measurements can seed the graph.", "help"));
}
function renderCoverage() {
  const measurement = state.baseline;
  const rawTotals = measurement?.audit?.totals?._raw || measurement?.audit?.totals;
  $("coverage-count").textContent = measurement ? `${measurement.audit.complete_files} / ${measurement.audit.instrumented_files}` : "—";
  $("coverage-note").textContent = measurement ? "Files at 100% raw lines, functions & regions" : "Full measurement required";
  $("measurement-date").textContent = measurement ? new Date(measurement.measured_at).toLocaleString() : "No full measurement";
  const metrics = $("coverage-metrics");
  metrics.replaceChildren();
  for (const key of ["lines", "functions", "regions"]) {
    const value = rawTotals?.[key];
    const card = node("article");
    card.append(node("span", key[0].toUpperCase() + key.slice(1)), node("strong", value ? `${(100 * value.covered / value.count).toFixed(6)}%` : "—"),
      node("small", value ? `${value.covered.toLocaleString()} / ${value.count.toLocaleString()}` : "No measured counters"));
    metrics.append(card);
  }
  $("measurement-details").textContent = measurement ? `${measurement.rust.counts[0].toLocaleString()} Rust checks passed across ${measurement.rust.targets} targets; ${measurement.test262.results.pass.toLocaleString()} applicable Test262 modes passed. Coverage here comes from a complete run; affected-test runs retain their own partial profiles.` : "Build the graph from a retained full measurement, or complete a verification pipeline.";
  const actions = $("report-actions");
  actions.replaceChildren();
  const existing = node("button", "View current repository report", "button secondary");
  existing.addEventListener("click", () => showOutput("/api/current-report", "Current repository report").catch(e => announce(e.message, true)));
  actions.append(existing);
  const fullRun = state.history.find(run => run.status === "passed" && run.report);
  if (fullRun) {
    const show = node("button", "View new complete report", "button primary");
    show.addEventListener("click", () => showOutput(`/api/report?run=${fullRun.id}`, "Complete verification report").catch(e => announce(e.message, true)));
    const publish = node("button", "Update repository report", "button secondary");
    publish.addEventListener("click", async () => { try { const result = await api("/api/publish", {run: fullRun.id}); announce(`Updated ${result.path}`); } catch (e) { announce(e.message, true); } });
    actions.append(show, publish);
  }
  const table = $("coverage-table");
  table.replaceChildren();
  for (const [name, summary] of Object.entries(graph?.coverage || {}).sort((a, b) => a[0].localeCompare(b[0]))) {
    const raw = summary._raw || summary;
    const complete = ["lines", "functions", "regions"].every(key => raw[key].covered === raw[key].count);
    const row = node("tr"); row.append(node("td", name));
    for (const key of ["lines", "functions", "regions"]) row.append(node("td", `${(100 * raw[key].covered / raw[key].count).toFixed(3)}%`));
    const status = node("td"); status.append(badge(complete ? "complete" : "incomplete")); row.append(status); table.append(row);
  }
}
async function refresh() {
  if (refreshInFlight) return;
  refreshInFlight = true;
  try {
    state = await api("/api/state");
    const currentGraphVersion = `${state.graph.created_at}:${state.plan.snapshot}`;
    if (graphVersion !== currentGraphVersion) {
      graph = await api("/api/graph");
      graphVersion = currentGraphVersion;
      if (!selectedSource) selectedSource = graph.sources.find(p => p.endsWith("vm/builtins/generators.rs")) || graph.sources[0];
      renderSources(); renderRelationships();
    }
    $("edge-count").textContent = state.graph.observed_edges.toLocaleString();
    $("graph-age").textContent = `${state.graph.observed_targets} targets have measured profiles`;
    const activeStatus = state.indexing.status === "running" ? "Indexing" : state.active?.status === "running" ? "Running" : "Idle";
    $("global-status").textContent = activeStatus;
    $("global-status").className = `badge ${activeStatus === "Idle" ? "idle" : "running"}`;
    if (state.indexing.status === "running") announce(`${state.indexing.message}: ${state.indexing.completed}/${state.indexing.total}`);
    if (state.indexing.status === "failed") announce(state.indexing.message, true);
    if (!$("workspace-gates").dataset.edited) $("workspace-gates").checked = state.workspace;
    renderPlan(); renderRun(); renderCoverage(); renderPartitions();
  } catch (error) { announce(error.message, true); }
  finally { refreshInFlight = false; }
}
async function analyze(useGit = false) {
  const files = useGit || !$("changed-files").value.trim() ? null : $("changed-files").value.split("\n").map(p => p.trim()).filter(Boolean);
  await api("/api/plan", {files});
  if (useGit) $("changed-files").value = "";
  await refresh();
}
async function run(mode) {
  await api("/api/run", {mode, workspace: $("workspace-gates").checked});
  setView("runs");
  await refresh();
}
function renderPartitions() {
  const plan = state.partitions;
  if (!plan) return;
  $("partition-summary").textContent = `${plan.selected_targets} of ${plan.total_rust_targets} Rust targets`;
  $("partition-anchor").textContent = plan.anchor ? `Changes since verified Rust snapshot ${plan.anchor}. ${plan.boundaries.length} mapped boundaries; discovery selects native test names before execution.` : "No verified Rust anchor; unknown changes require broader verification.";
  $("partition-unknown").hidden = !plan.unknown_sources.length;
  $("partition-unknown").textContent = `No case boundary recorded: ${plan.unknown_sources.map(shortPath).join(" · ")}`;
  const body = $("partition-plan");
  body.replaceChildren();
  for (const test of plan.tests) {
    const row = node("tr");
    const target = node("td", test.label);
    const filters = test.case_filters || [];
    const description = !filters.length || filters.includes("") ? "All cases in this target" : filters.join(" · ");
    target.append(node("div", description, "muted"));
    row.append(target, node("td", test.reasons.join("; ")));
    body.append(row);
  }
}
document.querySelectorAll(".nav-item").forEach(button => button.addEventListener("click", () => setView(button.dataset.view)));
$("source-search").addEventListener("input", renderSources);
$("plan-button").addEventListener("click", () => analyze().catch(e => announce(e.message, true)));
$("git-button").addEventListener("click", () => analyze(true).catch(e => announce(e.message, true)));
$("add-source").addEventListener("click", () => {
  const files = new Set($("changed-files").value.split("\n").map(p => p.trim()).filter(Boolean));
  files.add(selectedSource);
  $("changed-files").value = [...files].join("\n");
  analyze().catch(e => announce(e.message, true));
});
$("index-button").addEventListener("click", () => api("/api/index", {}).then(refresh).catch(e => announce(e.message, true)));
$("impact-button").addEventListener("click", () => run("impact").catch(e => announce(e.message, true)));
$("partition-button").addEventListener("click", () => run("partition").catch(e => announce(e.message, true)));
$("pipeline-button").addEventListener("click", () => run("pipeline").catch(e => announce(e.message, true)));
$("full-button").addEventListener("click", () => run("full").catch(e => announce(e.message, true)));
$("cancel-button").addEventListener("click", () => api("/api/cancel", {}).then(refresh).catch(e => announce(e.message, true)));
$("workspace-gates").addEventListener("change", () => { $("workspace-gates").dataset.edited = "true"; });
$("close-output").addEventListener("click", () => $("output-dialog").close());
setView(titles[location.hash.slice(1)] ? location.hash.slice(1) : "graph");
refresh();
setInterval(refresh, 2000);
