# Phase 3 — Rust Core Engine Skeleton

[← Back to plan](../BROWSER_CORE_PLAN.md)

**Status**: Not started

## Objective

Stand up the minimal end-to-end pipeline in Rust — parse → DOM → CSS cascade → layout → paint — as defined by the Phase 2 scope. This is the first phase that produces actual engine code; everything before it is planning.

## Plan

Structure this as a Cargo workspace with one crate per pipeline stage (rather than one monolith), matching how Servo and similar projects decompose the problem, referenced in plan §4:

1. HTML tokenizer/parser producing a DOM tree
2. CSS parser + cascade producing a styled tree, against the Phase 2 CSS subset
3. A layout pass (block/inline flow at minimum) producing a layout tree
4. A paint stage producing a raster/paint-command output
5. An end-to-end smoke test that runs a small HTML+CSS fixture through all four stages and asserts on the output

Prioritize getting *something* end-to-end working over completeness at any one stage — a skeleton that runs all five stages on a trivial page is more valuable at this point than a highly complete parser with no layout/paint behind it.

Per plan §1, every DOM node must get an explicit, stable ID at creation — this is a property of the DOM data structure itself, not something bolted on later, so it needs to be part of the initial DOM design in stage 1, not retrofitted after Phase 1 settles the AI representation format. [`../research/dom.md`](../research/dom.md) recommends the concrete shape: a monotonic-counter-issued `NodeId` newtype (not a slab/array index, to avoid ABA reuse hazards) assigned eagerly at construction, plus a document-scoped `NodeId → node` lookup table.

[`../research/layout.md`](../research/layout.md) recommends the DOM/layout-tree split follow Blink's LayoutNG design (a persistent DOM-linked box type plus a freshly-built immutable fragment tree per layout pass) rather than Gecko's mutable, back-pointer-heavy frame graph, since the latter fights Rust's ownership model. It also recommends a single `match` on `display` dispatching to per-algorithm implementations (block/inline first; flex/grid as later arms), mirroring how both real engines structure this.

[`../research/css-cascade.md`](../research/css-cascade.md) recommends against vendoring Gecko's Stylo (`style` crate) directly — its `TElement` trait alone has 82 methods to implement against a new DOM — in favor of porting its packed-specificity/cascade-origin design as a lightweight from-scratch implementation, and separately evaluating the standalone `cssparser` crate for tokenization.

## Checklist

- [ ] Set up the Cargo workspace and crate boundaries for the pipeline
- [ ] Implement the HTML parser → DOM (against the Phase 2 HTML subset), assigning every node an explicit, stable `NodeId` at creation via a monotonic counter (plan §1, `research/dom.md`)
- [ ] Implement the CSS parser + cascade → styled tree (against the Phase 2 CSS subset), porting the packed-specificity/cascade-origin design from `research/css-cascade.md` rather than vendoring Stylo
- [ ] Implement a minimal layout algorithm (block/inline flow) using a DOM-linked-box / immutable-fragment-tree split per `research/layout.md`
- [ ] Implement minimal paint/raster output
- [ ] Add an end-to-end smoke test: fixture HTML+CSS → asserted paint output
- [ ] Set up CI (build + test) for the workspace
