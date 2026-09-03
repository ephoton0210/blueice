# Phase 2 — MVP Scope

[← Back to plan](../BROWSER_CORE_PLAN.md)

**Status**: Not started

## Objective

Turn the general MVP boundary already agreed in plan §4 ("a minimal usable HTML parse → DOM → CSS cascade → layout → paint pipeline, nothing more") into a concrete, checkable list of what HTML/CSS/JS is actually supported, so Phase 3 has a fixed target instead of an open-ended one.

## Plan

Scope each layer of the pipeline independently, favoring "smallest subset that can render real, simple pages" over completeness:

- **HTML**: which elements and attributes are parsed and represented in the DOM (structural elements, text, forms, media placeholders vs. actual media support).
- **CSS**: which selectors and properties participate in the cascade and layout (box model, basic flow layout at minimum; explicitly decide whether flexbox/grid are in or out of MVP rather than leaving it implicit).
- **JS**: whether MVP includes JS execution at all, and if so, whether via an embedded existing engine or something custom — this is a large enough decision that it may need its own spike before Phase 3 starts.
- Re-confirm the explicit non-goals already listed in plan §4 (extension ecosystem, multi-tab state sync, full JS engine optimization, DevTools) still hold, and add any newly discovered ones.
- Ground each of the above against the Gecko/Chromium reference checkouts under [`../reference/`](../reference/) rather than scoping from memory — write up what's actually found as [`../research/`](../research/) notes (`dom.md`, `css-cascade.md`, `layout.md`) so the scoping decisions here can cite specifics.

## Checklist

- [ ] Read Gecko/Blink's HTML parsing, CSS cascade, and layout code in `../reference/`; write up findings in `../research/`
- [ ] List supported HTML elements/attributes for MVP
- [ ] List supported CSS selectors/properties for MVP; decide flexbox/grid in-or-out explicitly
- [ ] Decide the JS strategy for MVP (no JS / embed existing engine / custom) and record the rationale
- [ ] Re-confirm and extend the MVP non-goals list from plan §4
- [ ] Define one or more concrete demo pages the MVP must render correctly as its acceptance bar
- [ ] Cross-check the scope against the Phase 1 representation-layer decision (the chosen representation must be extractable from whatever this scope actually renders)
