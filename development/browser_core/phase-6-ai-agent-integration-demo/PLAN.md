# Phase 6 — AI Agent Integration Demo

[← Back to plan](../BROWSER_CORE_PLAN.md)

**Status**: In progress — the first-party, loopback-only scenario, shared
human/agent observer path, and local Ollama/Hugging Face TGI/llama.cpp Chat/MCP
live-model drivers are implemented. A real Qwen3.5-4B GGUF vision/tool run
through a local llama.cpp server completed the task on 2026-09-24; independent
human-window screenshot evidence remains outstanding. See [RESULTS.md](RESULTS.md).

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
configuration is required to reproduce the LLM-driven run; the scenario does
not pretend a deterministic test client is an LLM.

### Live-model driver (2026-09-23)

`blueice-phase6-agent` is an opt-in binary in `backend/mcp-server`. It uses a
bounded OpenAI-compatible Chat Completions function-calling loop: send narrowly
described tool definitions, execute every returned call locally, return its
output with the matching `tool_call_id`, and continue until the model provides
a final report.

**Local-model update (2026-09-24).** The driver supports three self-operated
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
- `--provider llamacpp --llamacpp-base <http://127.0.0.1:port/v1/>` targets a
  self-operated llama.cpp server with a GGUF model and matching vision
  projector. It has its own provider label in the transcript; a llama.cpp run
  is not presented as a Hugging Face TGI run.

All three selections send the same bounded function definitions and write tool
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
evidence gap but does not substitute for the outstanding human-window
screenshot.

**Cutover-aware frame-source follow-up (2026-09-24).** The first evidence
format above was insufficient across a Phase 8 core swap: v2 can reuse the
same tab ID and frame generation, and the reference frontend used to discard
v2's lower generation as stale. The IPC frame-plane now derives a stable
`frame_source` identifier from the frame directory, which launcher changes
for each core generation. Core includes it in AI snapshots; MCP screenshots
report the matching identifier; the agent requires the full
source/tab/generation triple for the highlighted screenshot and rejects a
source change during the highlight step. The opt-in human badge displays the
source as 16 hex digits and accepts the new core's lower generation instead
of freezing on v1. A compiled launcher/core test exercises a real generation
reset, changed source, and matching v2 snapshot. This source is scoped to a
frame-directory path, not a globally unique process identity if a later
independent run reuses that path. The earlier Qwen evidence predates this field
and remains a no-cutover run; the independent human-window screenshot remains
outstanding. The launcher now also hands v2's already-rendered replay frames
to existing clients immediately after a successful cutover. Otherwise the
reference frontend could remain on v1's last pixels until another action
generated a new frame, even though it could correctly identify v2 once that
later frame arrived. A real two-tab cutover test verifies both unsolicited
handoff frames.

**Compiled-stack orchestration check (2026-09-24).**
`backend/mcp-server/tests/phase6_agent_binary.rs` starts a real supervised
gatekeeper, launcher broker, core, loopback demo site, compiled Phase 6 agent,
and stdio MCP server. A second, independent launcher client receives the same
highlighted `FrameReady` tab/generation recorded by the agent's snapshot and
second retained PNG. Scripted loopback Chat Completions peers exercise
Ollama/llama.cpp-style tool-call arrays and Hugging Face/TGI-style single-object tool
calls through the complete browser path, including tool-result correlation,
the PNG image message, and final `tool_choice: none`. This verifies process
orchestration and shared-frame wiring, **not** model reasoning or a visible
human window; the separate real-model result below does not close the latter.

**Repeatable human-window evidence runbook (2026-09-25).**
[`run-human-evidence.sh`](run-human-evidence.sh) now builds the local binaries,
serves the first-party demo site only on `127.0.0.1`, starts a fresh launcher
with an isolated gatekeeper settings directory and an independent frontend
with the source/tab/generation badge, then waits until a person confirms that
the window is visible before starting the chosen already-running loopback
Ollama, Hugging Face TGI, or llama.cpp model. It announces the 90-second
highlight hold for a window-only OS screenshot, retains the transcript/MCP
PNGs/logs, verifies the highlighted snapshot and retained PNG have the same
frame identity, and prints that identity in the badge's exact hexadecimal
format. It never captures the whole desktop or claims that a supplied PNG
matches the badge without visual inspection. A no-model startup/cleanup
preflight launched the real demo site, launcher, and graphical frontend, then
intentionally withheld the human Enter confirmation; the script cleanly
stopped its process tree and removed its sockets. The full real-model and
human-screenshot path has not yet been rerun through this script.
The separate [`verify-human-evidence.py`](verify-human-evidence.py) check
requires the operator to transcribe the badge from their window PNG, compares
it with the post-highlight MCP frame, rejects stale frame identities and any
byte-identical MCP PNG substituted as the human capture, and writes hashes
plus the attestation to a create-new JSON report. Five black-box verifier
tests run locally and in CI. This is evidence bookkeeping, not an automated
claim that the image visibly contains a browser window.

**Local-model readiness preflight (2026-09-25).** Before starting the demo
site or opening the human window, the runbook now invokes the same Phase 6
agent with `--preflight-only`. It validates the selected Ollama, local Hugging
Face TGI, or llama.cpp loopback base using the agent's normal URL rules, then
checks only that its TCP endpoint accepts a connection. It sends no prompt or
model request and does not create a transcript. A real run still has to prove
that the named model itself responds. The agent's ten tests pass, including a
listening and a closed loopback endpoint; a full-script negative preflight
exited before launching the site or frontend when given a closed endpoint.
This removes one avoidable interruption to person-driven evidence collection,
but does not fill the outstanding human screenshot or real-model proof.

Run prerequisites are deliberately explicit: an operator must run a loopback
Ollama, Hugging Face TGI, or llama.cpp server with a local vision-and-tool-capable
model (and a matching vision projector for the llama.cpp GGUF path). In every
case the model name passed through `--model` must match the local server's
configuration. No API key is accepted, needed, or recorded. On 2026-09-24 a
temporary llama.cpp server ran a cached Qwen3.5-4B Q4_K_M GGUF plus its
matching projector. Two real-model runs completed the six actions and retained
same-frame MCP PNGs. The second run launched the reference frontend and held
the highlighted frame for 90 seconds, but macOS denied a window-only capture,
and no human screenshot has yet been supplied. Neither run proves a genuine
Ollama or TGI server, nor the still-open human-observer evidence requirement.

- Pick a small, concrete demo task and site/page scope, within what the Phase 2 MVP scope can actually render.
- Wire an LLM-driven agent to consume the Phase 5 API as its only channel for perceiving and acting on the page (no fallback to CDP/Puppeteer, since that would undermine what's being demonstrated).
- Demonstrate — and ideally capture evidence of — a human and the agent observing the same page/state, to make the "same render pass" claim concrete rather than architectural.
- Before starting, revisit the legal-exposure risk noted in plan §5: even on our own engine, the agent still needs its own policy on which sites it's allowed to browse for the demo (ToS, robots.txt, anti-bot considerations don't disappear just because it isn't CDP-driven).
- Capture what broke or surprised along the way and feed it back into earlier phases (representation shape, API ergonomics, MVP scope gaps) rather than treating the demo as a dead end.

## Checklist

- [x] Confirm the demo's target site(s)/page(s) are in-scope for the Phase 2 MVP and cleared under the Phase 5/plan §5 access policy — first-party `demo-site/`, loopback only
- [x] Pick and scope a concrete demo task — see `SCENARIO.md`: inspect/describe the visible MVP elements, set and confirm the labelled text-box value, highlight it, and follow the local confirmation link
- [x] Wire an LLM-driven agent to the Phase 5 API (no CDP/Puppeteer path) — `blueice-phase6-agent` confines a loopback Ollama, Hugging Face TGI, or llama.cpp Chat Completions function-calling loop to scenario-specific operations that each invoke the standard MCP adapter; targeted tests and MCP/core integration tests pass
- [x] Instrument common-frame evidence — MCP screenshots identify the frame-source/tab/generation triple behind each PNG; the agent transcript pairs that identity with its saved file; the human frontend can display the same scoped frame identity using `--show-generation`
- [x] Verify compiled local-provider orchestration against a real shared core — deterministic loopback Chat Completions peers drive both supported response shapes, while an independent launcher client receives the exact highlighted frame retained by MCP; this is not the real-model/human proof
- [x] Run a real local vision/tool model through the shared core — Qwen3.5-4B GGUF via llama.cpp completed the first-party scenario, with a matched highlighted MCP PNG and transcript; see [RESULTS.md](RESULTS.md)
- [ ] Demonstrate human + agent observing the same page/state simultaneously
- [x] Record results (what worked, what broke, what surprised) — [RESULTS.md](RESULTS.md) records the real-model task, evidence identities/hashes, provider-label correction, viewport change, and missing human-window capture
- [x] Feed findings back into earlier phases' plans — Phase 5 records the live common-frame evidence boundary and now the frame-source follow-up; Phase 8 records the human-frontend viewport change, cutover generation reset, and why evidence must be compared after observer attachment. These notes do not substitute for the still-missing human-window screenshot.
