# Phase 6 — AI Agent Integration Demo

[← Back to plan](../BROWSER_CORE_PLAN.md)

**Status**: Not started

## Objective

Prove the whole architecture end-to-end: an actual AI agent completing a task by driving BlueIce through the Phase 5 API — running on our own engine, not CDP/Puppeteer — with a human able to observe the same state at the same time. This is the payoff phase for the premise laid out in plan §1.

## Plan

- Pick a small, concrete demo task and site/page scope, within what the Phase 2 MVP scope can actually render.
- Wire an LLM-driven agent to consume the Phase 5 API as its only channel for perceiving and acting on the page (no fallback to CDP/Puppeteer, since that would undermine what's being demonstrated).
- Demonstrate — and ideally capture evidence of — a human and the agent observing the same page/state, to make the "same render pass" claim concrete rather than architectural.
- Before starting, revisit the legal-exposure risk noted in plan §5: even on our own engine, the agent still needs its own policy on which sites it's allowed to browse for the demo (ToS, robots.txt, anti-bot considerations don't disappear just because it isn't CDP-driven).
- Capture what broke or surprised along the way and feed it back into earlier phases (representation shape, API ergonomics, MVP scope gaps) rather than treating the demo as a dead end.

## Checklist

- [ ] Confirm the demo's target site(s)/page(s) are in-scope for the Phase 2 MVP and cleared under the Phase 5/plan §5 access policy
- [ ] Pick and scope a concrete demo task
- [ ] Wire an LLM-driven agent to the Phase 5 API (no CDP/Puppeteer path)
- [ ] Demonstrate human + agent observing the same page/state simultaneously
- [ ] Record results (what worked, what broke, what surprised)
- [ ] Feed findings back into earlier phases' plans as needed
