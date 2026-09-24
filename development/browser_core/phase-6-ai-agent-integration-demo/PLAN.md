# Phase 6 — AI Agent Integration Demo

[← Back to plan](../BROWSER_CORE_PLAN.md)

**Status**: In progress — the first-party, loopback-only scenario, shared
human/agent observer path, and local Ollama/Hugging Face TGI Chat/MCP
live-model drivers are implemented and tested through a compiled-stack
scripted-provider check; an actual configured LLM run and its human-observer
evidence remain.

## Objective

Prove the whole architecture end-to-end: an actual AI agent completing a task by driving BlueIce through the Phase 5 API — running on our own engine, not CDP/Puppeteer — with a human able to observe the same state at the same time. This is the payoff phase for the premise laid out in plan §1.

## Plan

### Selected safe scenario (2026-09-23)

The target is the project-owned [`demo-site/`](demo-site/) fixture, served only
from `127.0.0.1` for the duration of a run. Its exact task and acceptance
evidence live in [`SCENARIO.md`](SCENARIO.md). This replaces a real third-party
target with an explicit first-party scope: no ToS, robots, credentials, or
unapproved origin access is involved.

The reference frontend now accepts `--launcher` (or `--socket
<launcher-rendezvous.sock>` for an explicit isolated path) and joins the
launcher-owned core rather than starting a private core. In that mode closing
the human window does not terminate the shared core. The existing MCP server
already prefers the same default launcher socket, so the later LLM driver and
human observer can consume frames from one render pass. A model/provider
configuration is still required before an actual LLM-driven run can be made;
the scenario intentionally does not pretend a deterministic test client is an
LLM.

### Live-model driver (2026-09-23)

`blueice-phase6-agent` is an opt-in binary in `backend/mcp-server`. It uses a
bounded OpenAI-compatible Chat Completions function-calling loop: send narrowly
described tool definitions, execute every returned call locally, return its
output with the matching `tool_call_id`, and continue until the model provides
a final report.

**Local-model update (2026-09-23).** The driver supports two self-operated
local backends:

- `--provider ollama` (the default) uses Ollama's credential-free loopback
  OpenAI-compatible endpoint at `http://127.0.0.1:11434/v1/`; `--ollama-base`
  can change its loopback port.
- `--provider huggingface --huggingface-base <http://127.0.0.1:port/v1/>`
  targets an operator-run Hugging Face Text Generation Inference (TGI) server.
  This lets advanced operators choose the model, quantization, adapters, and
  accelerator allocation when they start their local server. TGI's Messages API
  and function calling are OpenAI Chat Completions-compatible (TGI 1.4.3 or
  newer for tool support): <https://huggingface.co/docs/text-generation-inference/guidance>.

Both selections send the same bounded function definitions and write tool
results back as chat tool messages. The runner accepts only credential-free
loopback `http(s)://.../v1/` bases, has no API-key flag, and never falls back to
a cloud endpoint. It records the provider and local base in the transcript, but
never credentials. The real-run/human-observer evidence requirements remain
unchanged.

While any scenario action remains, the runner requests `tool_choice: "auto"`.
After all six actions are complete it sends `tool_choice: "none"` for the final
report; this accommodates TGI's documented `auto` tool-selection behavior
without permitting another browser operation.

The driver does **not** give a model the raw MCP tool inventory. It exposes
only six zero-argument scenario operations: fixed loopback navigation, page
representation, screenshot, set the labelled `Name` field to the fixed value
`BlueIce`, highlight that same field, and follow the fixed confirmation link.
For every state-changing operation it first finds a live node in a fresh
representation and verifies role/name/value itself. `--demo-url` accepts only
credential-free `http://127.0.0.1:<port>/index.html`; a model cannot supply a
URL, arbitrary node ID, or arbitrary text. All browser work still travels
through a standard-stdio `blueice-mcp-server`, never CDP, Puppeteer, extension
IPC, or direct core IPC.

The runner requires `--launcher-socket`; it starts MCP with the matching new
`--launcher-socket` option. In that mode MCP attaches to that exact rendezvous
socket and fails if it is unavailable — it cannot silently start an unobserved
private core. It writes a create-new JSONL transcript (never API headers or
keys) and retained MCP PNG screenshots. It requires a screenshot before the
visible-content report and another after the Name field is highlighted. The
default ten-second post-highlight hold lets the attached human frontend record
the same highlighted frame. A runner exit is success only after all actions,
the highlight screenshot, and the core-confirmed `Task complete` page have
occurred. Its local tests cover URL containment, refusal of model-supplied
arguments, live-node assertions, and required-launcher no-fallback behavior;
they do not claim to be an LLM provider run.

**Common-frame evidence instrumentation (2026-09-24).** The MCP screenshot
response now precedes its untrusted-image warning with trusted metadata naming
the exact cached core `tab_id` and `generation` encoded into the PNG. The
runner requires that metadata and writes an `evidence_saved` JSONL event with
the PNG path and frame identity. It records the highlight's post-action
snapshot as `highlight_frame` and refuses to save or count a post-highlight
screenshot unless that PNG's tab and generation match the snapshot exactly.
The reference human frontend's opt-in
`--show-generation` flag draws the selected tab and generation in native
window chrome, without changing the core page frame or MCP PNG. During a live
run, a human screenshot of the highlighted page must show the same tab and
generation as the second MCP PNG entry and highlight snapshot. This closes an
evidence gap but does not substitute for the outstanding actual model/human
run.

**Compiled-stack orchestration check (2026-09-24).**
`backend/mcp-server/tests/phase6_agent_binary.rs` starts a real supervised
gatekeeper, launcher broker, core, loopback demo site, compiled Phase 6 agent,
and stdio MCP server. A second, independent launcher client receives the same
highlighted `FrameReady` tab/generation recorded by the agent's snapshot and
second retained PNG. Scripted loopback Chat Completions peers exercise both
Ollama-style tool-call arrays and Hugging Face/TGI-style single-object tool
calls through the complete browser path, including tool-result correlation,
the PNG image message, and final `tool_choice: none`. This verifies process
orchestration and shared-frame wiring, **not** model reasoning or a visible
human window; the live-model/human evidence item below stays open.

Run prerequisites are deliberately explicit: an operator must either install
and run a loopback Ollama server with a local vision-and-tool-capable lightweight
model, or operate a loopback Hugging Face TGI server with a model configured by
the operator. In both cases the model name passed through `--model` must match
the local server's configuration. No API key is accepted, needed, or recorded.
No local model/human observer was configured in this development environment on
2026-09-23; checks on 2026-09-24 still found no Ollama service at `11434` or
TGI service at `8080`, so no live model transcript or human screenshot is
claimed yet.

- Pick a small, concrete demo task and site/page scope, within what the Phase 2 MVP scope can actually render.
- Wire an LLM-driven agent to consume the Phase 5 API as its only channel for perceiving and acting on the page (no fallback to CDP/Puppeteer, since that would undermine what's being demonstrated).
- Demonstrate — and ideally capture evidence of — a human and the agent observing the same page/state, to make the "same render pass" claim concrete rather than architectural.
- Before starting, revisit the legal-exposure risk noted in plan §5: even on our own engine, the agent still needs its own policy on which sites it's allowed to browse for the demo (ToS, robots.txt, anti-bot considerations don't disappear just because it isn't CDP-driven).
- Capture what broke or surprised along the way and feed it back into earlier phases (representation shape, API ergonomics, MVP scope gaps) rather than treating the demo as a dead end.

## Checklist

- [x] Confirm the demo's target site(s)/page(s) are in-scope for the Phase 2 MVP and cleared under the Phase 5/plan §5 access policy — first-party `demo-site/`, loopback only
- [x] Pick and scope a concrete demo task — see `SCENARIO.md`: inspect/describe the visible MVP elements, set and confirm the labelled text-box value, highlight it, and follow the local confirmation link
- [x] Wire an LLM-driven agent to the Phase 5 API (no CDP/Puppeteer path) — `blueice-phase6-agent` confines a loopback Ollama or Hugging Face TGI Chat Completions function-calling loop to scenario-specific operations that each invoke the standard MCP adapter; targeted tests and MCP/core integration tests pass
- [x] Instrument common-frame evidence — MCP screenshots identify the exact tab/generation behind each PNG; the agent transcript pairs that identity with its saved file; the human frontend can display the same core frame identity using `--show-generation`
- [x] Verify compiled local-provider orchestration against a real shared core — deterministic loopback Chat Completions peers drive both supported response shapes, while an independent launcher client receives the exact highlighted frame retained by MCP; this is not the real-model/human proof
- [ ] Demonstrate human + agent observing the same page/state simultaneously
- [ ] Record results (what worked, what broke, what surprised)
- [ ] Feed findings back into earlier phases' plans as needed
