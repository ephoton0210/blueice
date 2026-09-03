# Phase 5 — AI Representation Output Path

[← Back to plan](../BROWSER_CORE_PLAN.md)

**Status**: Not started

## Objective

Implement the AI-facing representation and API defined in Phase 1, sourced from the *same* render pass that drives Phase 4's human-visible output — this is where the project's core claim (human and AI perceive the same state from the same render pass) actually gets built and becomes testable, rather than just asserted in plan §1.

## Plan

- Implement extraction of the Phase 1 representation from the same pipeline state Phase 4 renders from — not a second, independently-timed pass.
- The API boundary question below is largely settled by plan §1's process architecture: `core` already exposes an IPC surface to `extension` and `frontend`, so the AI-facing API is a third client of that same surface rather than a separately-designed channel — implement it as such unless something concrete forces a divergence.
- Because "same render pass" is the entire point of the project, this phase needs an explicit test proving it: assert that the AI representation and the human-visible frame correspond to the same render pass / JS execution state, not just that they're usually close.
- Implement the browser-chrome control surface Phase 1 defined, starting with show/hide — backed by the Phase 4 windowing support for toggling visibility without restarting the engine — and verify the engine's render pass/JS state is unchanged across a hide/show cycle.
- Verify every element the representation exposes carries its DOM node's stable ID (plan §1), not a transient index recomputed per extraction.
- Document the API for consumers building the Phase 6 demo against it.

## Checklist

- [ ] Implement extraction of the chosen (Phase 1) representation from the shared render pass
- [ ] Implement the AI-facing API as a client of `core`'s shared IPC surface (plan §1), not a separately-designed boundary
- [ ] Add a test asserting the AI representation and human-visible frame correspond to the same render pass and JS state
- [ ] Implement the show/hide window control endpoint on the browser-chrome control surface (plan §1 / Phase 4)
- [ ] Add a test asserting engine state (render pass/JS state) is unchanged across a hide/show cycle — no restart occurs
- [ ] Add a test asserting represented elements are addressed by their stable DOM node ID, not a transient index
- [ ] Document the AI-facing API for consumers (this feeds Phase 6)
