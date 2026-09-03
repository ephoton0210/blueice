# Phase 4 — Human-Visible Rendering Path

[← Back to plan](../BROWSER_CORE_PLAN.md)

**Status**: Not started

## Objective

Get the Phase 3 paint output onto an actual screen with basic interaction, so there's a human-usable window onto the engine — the first half of the "human and AI share the same render pass" goal from plan §1.

## Plan

This phase is deliberately scoped to *displaying and interacting with* what Phase 3 already produces, not extending the pipeline itself:

- Pick a windowing/rendering backend for presenting paint output on screen, and confirm it supports showing/hiding the window independently of the engine process — the window's visibility must not be tied to the engine instance's lifecycle (plan §1), so the AI can hide it for AI-only operation and show it again later without a restart.
- Wire Phase 3's paint stage to that backend's surface.
- Handle the minimum input needed to browse: scroll, click, window resize (each of which needs to feed back into layout/paint, not just be swallowed at the window layer).
- Handle basic navigation: load a URL, follow a link.
- Manually verify against the Phase 2 demo page(s) as a smoke test — this is the first point where the pipeline can be checked by eye instead of only by automated fixture assertions.
- Add a Help/About/Credits screen crediting Chromium and Gecko as technical references — this satisfies BSD-3-Clause's binary-distribution notice clause (reproducing Chromium's copyright notice "in the documentation and/or other materials provided with the distribution"), which the per-file source headers from Phase 0 don't cover. See [Phase 0](../phase-0-license-legal-foundation/PLAN.md) and plan §2.

## Checklist

- [ ] Choose a windowing/rendering backend that supports runtime show/hide independent of engine lifecycle (plan §1)
- [ ] Wire Phase 3 paint output to an on-screen surface
- [ ] Handle scroll input, feeding back into layout/paint as needed
- [ ] Handle click input and hit-testing against the layout tree
- [ ] Handle window resize, feeding back into layout
- [ ] Handle basic navigation (load URL, follow links)
- [ ] Manually verify rendering against the Phase 2 demo page(s)
- [ ] Add a Help/About/Credits screen reproducing the Chromium BSD-3-Clause notice and crediting Gecko (BSD-3-Clause binary-distribution requirement — see Phase 0)
