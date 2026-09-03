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

Per plan §1, every DOM node must get an explicit, stable ID at creation — this is a property of the DOM data structure itself, not something bolted on later, so it needs to be part of the initial DOM design in stage 1, not retrofitted after Phase 1 settles the AI representation format.

## Checklist

- [ ] Set up the Cargo workspace and crate boundaries for the pipeline
- [ ] Implement the HTML parser → DOM (against the Phase 2 HTML subset), assigning every node an explicit, stable ID at creation (plan §1)
- [ ] Implement the CSS parser + cascade → styled tree (against the Phase 2 CSS subset)
- [ ] Implement a minimal layout algorithm (block/inline flow)
- [ ] Implement minimal paint/raster output
- [ ] Add an end-to-end smoke test: fixture HTML+CSS → asserted paint output
- [ ] Set up CI (build + test) for the workspace
