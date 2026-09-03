# Phase 4 — Human-Visible Rendering Path

[← Back to plan](../BROWSER_CORE_PLAN.md)

**Status**: Not started

## Objective

Get the Phase 3 paint output onto an actual screen with basic interaction, so there's a human-usable window onto the engine — the first half of the "human and AI share the same render pass" goal from plan §1.

## Plan

This phase is deliberately scoped to *displaying and interacting with* what Phase 3 already produces, not extending the pipeline itself:

- Pick a windowing/rendering backend for presenting paint output on screen.
- Wire Phase 3's paint stage to that backend's surface.
- Handle the minimum input needed to browse: scroll, click, window resize (each of which needs to feed back into layout/paint, not just be swallowed at the window layer).
- Handle basic navigation: load a URL, follow a link.
- Manually verify against the Phase 2 demo page(s) as a smoke test — this is the first point where the pipeline can be checked by eye instead of only by automated fixture assertions.

## Checklist

- [ ] Choose a windowing/rendering backend
- [ ] Wire Phase 3 paint output to an on-screen surface
- [ ] Handle scroll input, feeding back into layout/paint as needed
- [ ] Handle click input and hit-testing against the layout tree
- [ ] Handle window resize, feeding back into layout
- [ ] Handle basic navigation (load URL, follow links)
- [ ] Manually verify rendering against the Phase 2 demo page(s)
