# Phase 3 — Rust Core Engine Skeleton

[← Back to plan](../BROWSER_CORE_PLAN.md)

**Status**: In progress

## Objective

Stand up the minimal end-to-end pipeline in Rust — parse → DOM → CSS cascade → layout → paint — as defined by the Phase 2 scope. This is the first phase that produces actual engine code; everything before it is planning.

## Plan

Structure this as a Cargo workspace with one crate per pipeline stage (rather than one monolith), matching how Servo and similar projects decompose the problem, referenced in plan §4 — and laid out under `backend/core/` per plan §1's process architecture, alongside a stubbed `backend/extension/` and a shared `ipc` crate, so the process boundary exists from the first commit rather than being retrofitted:

1. HTML tokenizer/parser producing a DOM tree
2. CSS parser + cascade producing a styled tree, against the Phase 2 CSS subset
3. A layout pass (block/inline flow at minimum) producing a layout tree
4. A paint stage producing a raster/paint-command output
5. An end-to-end smoke test that runs a small HTML+CSS fixture through all four stages and asserts on the output

`backend/extension/` stays a stub in this phase — no real extension API ships in MVP (plan §4) — but it exists as a separate crate/binary target from day one specifically so `core` never grows an in-process extension-loading code path that would later need ripping out for process isolation. The `ipc` crate similarly starts as a placeholder for the control-plane protocol plan §1 describes (shared by `extension`, the Phase 5 AI-facing API, and the Phase 4 frontend boundary) — not implemented yet, just reserved as a workspace member so its eventual dependents don't need restructuring to adopt it.

Prioritize getting *something* end-to-end working over completeness at any one stage — a skeleton that runs all five stages on a trivial page is more valuable at this point than a highly complete parser with no layout/paint behind it.

Per plan §1, every DOM node must get an explicit, stable ID at creation — this is a property of the DOM data structure itself, not something bolted on later, so it needs to be part of the initial DOM design in stage 1, not retrofitted after Phase 1 settles the AI representation format. [`../research/dom.md`](../research/dom.md) recommends the concrete shape: a monotonic-counter-issued `NodeId` newtype (not a slab/array index, to avoid ABA reuse hazards) assigned eagerly at construction, plus a document-scoped `NodeId → node` lookup table.

[`../research/layout.md`](../research/layout.md) recommends the DOM/layout-tree split follow Blink's LayoutNG design (a persistent DOM-linked box type plus a freshly-built immutable fragment tree per layout pass) rather than Gecko's mutable, back-pointer-heavy frame graph, since the latter fights Rust's ownership model. It also recommends a single `match` on `display` dispatching to per-algorithm implementations (block/inline first; flex/grid as later arms), mirroring how both real engines structure this.

[`../research/css-cascade.md`](../research/css-cascade.md) recommends against vendoring Gecko's Stylo (`style` crate) directly — its `TElement` trait alone has 82 methods to implement against a new DOM — in favor of porting its packed-specificity/cascade-origin design as a lightweight from-scratch implementation, and separately evaluating the standalone `cssparser` crate for tokenization.

Testing policy for everything in this phase (coverage gate, what belongs in unit vs. fixture tests) is defined once in [`../testing/TEST_PLAN.md`](../testing/TEST_PLAN.md), not repeated per checklist item below — each item below is expected to satisfy it, including keeping its crate off the coverage gate's ignore-list once it stops being a stub.

## Checklist

- [x] Set up the Cargo workspace and crate boundaries for the pipeline — `backend/core/{dom,html,css,layout,paint,engine}`, stubbed `backend/extension/`, stubbed `ipc` crate (plan §1)
- [x] Implement the DOM tree data structure — `NodeId` (monotonic, never reused), tree operations (`append_child`/`detach`/`remove_subtree`), memory actually reclaimed on removal (plan §1 low-memory requirement, `research/dom.md`); 96%+ line coverage, off the coverage gate's stub ignore-list
- [ ] Implement the HTML parser → DOM (against the Phase 2 HTML subset), producing `blueice-dom` trees via its existing `NodeId`/tree API
- [ ] Implement the CSS parser + cascade → styled tree (against the Phase 2 CSS subset), porting the packed-specificity/cascade-origin design from `research/css-cascade.md` rather than vendoring Stylo
- [ ] Implement a minimal layout algorithm (block/inline flow) using a DOM-linked-box / immutable-fragment-tree split per `research/layout.md`
- [ ] Implement minimal paint/raster output
- [ ] Add an end-to-end smoke test: fixture HTML+CSS → asserted paint output (per `testing/TEST_PLAN.md`'s integration/fixture-test category)
- [x] Set up CI (build + test) for the workspace — `.github/workflows/ci.yml`: build + test + clippy, plus a coverage gate (`testing/TEST_PLAN.md`)
