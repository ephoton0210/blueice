# Phase 2 — MVP Scope

[← Back to plan](../BROWSER_CORE_PLAN.md)

**Status**: In progress

## Objective

Turn the general MVP boundary already agreed in plan §4 ("a minimal usable HTML parse → DOM → CSS cascade → layout → paint pipeline, nothing more") into a concrete, checkable list of what HTML/CSS/JS is actually supported, so Phase 3 has a fixed target instead of an open-ended one.

## Plan

Scope each layer of the pipeline independently, favoring "smallest subset that can render real, simple pages" over completeness:

- **HTML**: which elements and attributes are parsed and represented in the DOM (structural elements, text, forms, media placeholders vs. actual media support).
- **CSS**: which selectors and properties participate in the cascade and layout (box model, basic flow layout at minimum; explicitly decide whether flexbox/grid are in or out of MVP rather than leaving it implicit).
- **JS**: **decided — a custom, from-scratch engine ("BlueJS"), not an embedded existing engine.** See [Phase 13](../phase-13-bluejs-engine/PLAN.md) for the engine itself; this phase still needs to scope which JS *language features* and DOM bindings are in the MVP subset, same as HTML/CSS below.
- Re-confirm the explicit non-goals already listed in plan §4 (extension ecosystem, multi-tab state sync, full JS engine optimization, DevTools) still hold, and add any newly discovered ones.
- Ground each of the above against the Gecko/Chromium reference checkouts under [`../reference/`](../reference/) rather than scoping from memory — write up what's actually found as [`../research/`](../research/) notes (`dom.md`, `css-cascade.md`, `layout.md`) so the scoping decisions here can cite specifics.

**Research done for all three layers.** [`../research/css-cascade.md`](../research/css-cascade.md) found flexbox/grid are cleanly separable from cascade (ordinary longhands, no special cascade path) but not from layout (each is its own large subsystem in both engines) — recommends including basic single-axis flexbox in MVP and deferring Grid. [`../research/layout.md`](../research/layout.md) recommends an immutable-fragment-tree design (Blink LayoutNG-style) over Gecko's mutable frame graph as a better fit for Rust, with flex/grid as later dispatch arms rather than a rewrite. [`../research/html-parsing.md`](../research/html-parsing.md) found the tokenizer/tree-builder split is a real interface boundary in both engines (not just file organization), that adoption-agency/foster-parenting error recovery is load-bearing even for "simple" real pages, and that foreign content (SVG/MathML) is a cleanly-gated subsystem safe to cut wholesale for MVP — notably, Blink ships its own restricted fast-path HTML parser (`HTMLDocumentParserFastpath`, a 19-tag whitelist with no auto-closing/misnesting handling, falling back to the full parser on anything unsupported) as direct production precedent for this kind of MVP cut.

## Checklist

- [x] Read Gecko/Blink's CSS cascade and layout code in `../reference/`; write up findings in `../research/css-cascade.md` and `../research/layout.md`
- [x] Read Gecko/Blink's HTML tokenizer/tree-builder code in `../reference/`; write up findings in `../research/html-parsing.md`
- [ ] List supported HTML elements/attributes for MVP (see `html-parsing.md` §4 for a starting cut: keep the full tokenizer state machine and adoption-agency/foster-parenting; cut foreign content, `document.write` reentrancy, speculative parsing, and the full named-character-reference table)
- [ ] List supported CSS selectors/properties for MVP; decide flexbox/grid in-or-out explicitly (see `css-cascade.md`/`layout.md` — leaning toward basic flexbox in, Grid deferred)
- [x] Decide the JS strategy for MVP — custom engine (BlueJS), tracked in [Phase 13](../phase-13-bluejs-engine/PLAN.md)
- [ ] Scope the MVP JS language-feature and DOM-binding subset (separate from Phase 13's engine-architecture questions)
- [ ] Re-confirm and extend the MVP non-goals list from plan §4
- [ ] Define one or more concrete demo pages the MVP must render correctly as its acceptance bar
- [ ] Cross-check the scope against the Phase 1 representation-layer decision (the chosen representation must be extractable from whatever this scope actually renders)
