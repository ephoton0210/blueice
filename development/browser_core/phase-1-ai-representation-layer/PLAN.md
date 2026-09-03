# Phase 1 — AI Representation Layer & API Shape

[← Back to plan](../BROWSER_CORE_PLAN.md)

**Status**: In progress

## Objective

Plan §3's representation-layer decision is settled (an accessibility-tree-shaped schema, keyed by BlueIce's stable `NodeId`, extended with four fields — see plan §3). What's left in this phase is defining the actual shape of the API the AI side receives: data format, versioning, how elements are addressed and acted on, and how it stays synchronized with human-facing UI state. This still gates Phase 5 (the AI representation output path can't be implemented until the API shape is defined) and should be settled before Phase 3/4 implementation work gets far enough that changes become expensive.

## How §3 was decided

Not by an independent validation exercise — the Accessibility Tree candidate is already proven at scale in the two production browsers this project studies (Gecko and Blink both drive real screen readers off it; Blink's schema is now also the basis of Chromium's own emerging AI-agent code). Reading how each engine actually implements it (`../research/accessibility-tree.md`, `../research/dom.md`) was sufficient to settle the choice directly — see [`spike.md`](spike.md) for the worked example that fixed the concrete schema (which fields beyond a stock accessibility tree are actually needed), not to validate feasibility that was already established by prior art.

**Two constraints from plan §1 apply to the API shape regardless**: every element must be addressed by the DOM node's stable ID (not a transient index/coordinate alone), and the AI-facing API needs a browser-chrome control surface — distinct from the page-content representation — for actions like show/hide that operate on the window rather than on a page.

## Checklist

- [x] Read Gecko's and Blink's accessibility-tree implementations in `../reference/`; write up findings in `../research/accessibility-tree.md`
- [x] Settle the §3 representation-layer decision (accessibility-tree-shaped schema + four fields, keyed by stable `NodeId`) — recorded in `BROWSER_CORE_PLAN.md` §3, worked example in `spike.md`
- [ ] Define the AI-facing data format/schema for the chosen representation as a concrete, implementable spec (field types, not just field names)
- [ ] Define how the API exposes each element by its stable DOM node ID (plan §1), plus coordinates, in a way an agent can act on (click, type, scroll)
- [ ] Define versioning/stability policy for the AI-facing API
- [ ] Define how this representation and human-facing UI state (e.g. highlights) stay synchronized, per the sync goal in plan §1
- [ ] Define a browser-chrome control surface in the API (starting with show/hide the window) separate from the page-content representation
