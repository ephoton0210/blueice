# Phase 5 — AI Representation Output Path

[← Back to plan](../BROWSER_CORE_PLAN.md)

**Status**: Not started

## Objective

Implement the AI-facing representation and API defined in Phase 1, sourced from the *same* render pass that drives Phase 4's human-visible output — this is where the project's core claim (human and AI perceive the same state from the same render pass) actually gets built and becomes testable, rather than just asserted in plan §1.

## Plan

- Implement extraction of the Phase 1 representation from the same pipeline state Phase 4 renders from — not a second, independently-timed pass.
- Decide and implement the API boundary: in-process Rust API, an IPC/RPC boundary for out-of-process agents, or both.
- Because "same render pass" is the entire point of the project, this phase needs an explicit test proving it: assert that the AI representation and the human-visible frame correspond to the same render pass / JS execution state, not just that they're usually close.
- Document the API for consumers building the Phase 6 demo against it.

## Checklist

- [ ] Implement extraction of the chosen (Phase 1) representation from the shared render pass
- [ ] Decide and implement the API boundary (in-process / IPC-RPC / both)
- [ ] Add a test asserting the AI representation and human-visible frame correspond to the same render pass and JS state
- [ ] Document the AI-facing API for consumers (this feeds Phase 6)
