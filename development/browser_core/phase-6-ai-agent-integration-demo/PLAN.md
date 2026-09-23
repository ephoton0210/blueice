# Phase 6 — AI Agent Integration Demo

[← Back to plan](../BROWSER_CORE_PLAN.md)

**Status**: In progress — the first-party, loopback-only scenario and the
shared human/agent observer path are defined; an actual configured LLM run and
its evidence remain.

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

- Pick a small, concrete demo task and site/page scope, within what the Phase 2 MVP scope can actually render.
- Wire an LLM-driven agent to consume the Phase 5 API as its only channel for perceiving and acting on the page (no fallback to CDP/Puppeteer, since that would undermine what's being demonstrated).
- Demonstrate — and ideally capture evidence of — a human and the agent observing the same page/state, to make the "same render pass" claim concrete rather than architectural.
- Before starting, revisit the legal-exposure risk noted in plan §5: even on our own engine, the agent still needs its own policy on which sites it's allowed to browse for the demo (ToS, robots.txt, anti-bot considerations don't disappear just because it isn't CDP-driven).
- Capture what broke or surprised along the way and feed it back into earlier phases (representation shape, API ergonomics, MVP scope gaps) rather than treating the demo as a dead end.

## Checklist

- [x] Confirm the demo's target site(s)/page(s) are in-scope for the Phase 2 MVP and cleared under the Phase 5/plan §5 access policy — first-party `demo-site/`, loopback only
- [x] Pick and scope a concrete demo task — see `SCENARIO.md`: inspect/describe the visible MVP elements, highlight the labelled text box, and follow the local confirmation link
- [ ] Wire an LLM-driven agent to the Phase 5 API (no CDP/Puppeteer path)
- [ ] Demonstrate human + agent observing the same page/state simultaneously
- [ ] Record results (what worked, what broke, what surprised)
- [ ] Feed findings back into earlier phases' plans as needed
