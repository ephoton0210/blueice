# Phase 5 — AI Representation Output Path

[← Back to plan](../BROWSER_CORE_PLAN.md)

**Status**: Done (for the schema/actions/sync mechanism Phase 1 spec'd; the `protocol_version` handshake Phase 1 §3 also decided is deliberately deferred — see "Deferred" below)

## Objective

Implement the AI-facing representation and API defined in Phase 1, sourced from the *same* render pass that drives Phase 4's human-visible output — this is where the project's core claim (human and AI perceive the same state from the same render pass) actually gets built and becomes testable, rather than just asserted in plan §1.

## What was implemented

**The two Phase 2 cross-check prerequisites, closed**: `opacity` is now a real `blueice-css` property (`ComputedStyle::opacity`), applied as a flat per-element alpha multiply in `blueice-paint`. `Page` gained real, persistent `hovered`/`focused`/`highlighted` interaction state (reset on every navigation, since a fresh document invalidates the `NodeId`s a prior interaction might reference).

**Extraction** (`blueice_engine::ai_snapshot`, `pub(crate)`, reached via `Page::snapshot`): walks the DOM directly rather than the fragment tree, keeping only elements with an inherently semantic role or an explicit `role`/`aria-label` attribute *and* an actual fragment (excludes `display:none` subtrees and purely-inline elements with no box of their own) — the concrete implementation of `spike.md`'s exclusion rule. Non-represented ancestors (a bare `<div>` wrapper) are skipped when linking parent/child, reparenting to the nearest represented ancestor rather than losing the relationship. Name computation is a deliberately small subset of the real accname algorithm (`aria-label` > element-specific rule > `title` > subtree text, stopping at any descendant that's independently represented so a container never duplicates a nested element's name). Occlusion is checked geometrically against the existing DOM/paint order, consistent with (not a new instance of) `phase-2-mvp-scope/PLAN.md`'s "no compositing layers or stacking contexts beyond plain DOM/paint order" non-goal.

**Wire protocol** (`blueice-ipc`, extending the existing `ClientMessage`/`ServerMessage` enums per Phase 1's decision to make the AI-facing API a client of the same surface `frontend` already uses, not a separate channel): `GetRepresentation`/`ServerMessage::Representation(AiSnapshot)`; `ActOn { id, action }` with `NodeAction::{Click, Focus, SetValue, ScrollIntoView}`, ID-addressed rather than coordinate-based (`Page::act` resolves the ID to current bounds internally); `Highlight { id: Option<u64> }`, rendered as a fresh outline derived from the target's current bounds on every paint (`Page::render`), so it tracks the node through any layout change instead of a caller recomputing a screen rectangle; `Hover { x, y }`, giving `core` a single source of truth for hover state (`frontend`'s `CursorMoved` now forwards it) that both `NodeState::hovered` and a future `:hover` style would read from the same place; `ClientMessage::SetVisible` regrouped under `Chrome(ChromeCommand::SetVisible)`, separating browser-chrome actions from page-content ones per Phase 1's API-shape decision.

**"Same render pass" test**: `session::tests::get_representation_shares_the_current_generation_with_the_last_frame` asserts a `Representation` and the `FrameReady` sent alongside the state change immediately before it carry the identical `generation` number — the concrete, checkable version of the project's core claim, not just an assertion that the two are usually close.

**Hide/show verification**: `session::tests::chrome_set_visible_does_not_change_engine_render_state` — a full hide-then-show round trip leaves `GetRepresentation`'s output byte-for-byte identical and produces no new frame, since `Chrome` messages never touch `Page` at all.

**Stable-ID verification**: every `ai_snapshot` test asserts against real `AiNode::id`/`parent`/`children` values (`blueice_dom::NodeId::as_u64`/`from_u64`, added for this round-trip), not a recomputed index; `session::tests::act_on_an_unknown_id_is_a_harmless_no_op` additionally proves a stale ID from before a navigation is a no-op rather than a panic or misdirected action (`Page::act`/`Document::contains`).

## Deferred (recorded, not forgotten)

Phase 1 §3's `protocol_version` handshake (`Hello` messages, additive-only-without-a-bump policy) was **not** implemented in this pass — it's a cross-cutting change touching every existing test harness and client (`session.rs`, `frontend-reference`, `core_binary.rs`) for a versioning concern that has no real second implementation to version against yet (there is exactly one `core` and one `frontend`, both built together in this workspace). Tracked as a small, well-specified follow-up for whenever a genuinely independent client (Phase 9's extension protocol, Phase 12's MCP server) makes protocol drift an actual risk rather than a hypothetical one.

## Checklist

- [x] Implement extraction of the chosen (Phase 1) representation from the shared render pass — `blueice_engine::ai_snapshot`
- [x] Implement the AI-facing API as a client of `core`'s shared IPC surface (plan §1), not a separately-designed boundary — new `ClientMessage`/`ServerMessage` variants in `blueice-ipc`, no separate channel
- [x] Add a test asserting the AI representation and human-visible frame correspond to the same render pass and JS state — `get_representation_shares_the_current_generation_with_the_last_frame` (no JS execution exists yet to assert about; tracked by Phase 13)
- [x] Implement the show/hide window control endpoint on the browser-chrome control surface (plan §1 / Phase 4) — `ClientMessage::Chrome(ChromeCommand::SetVisible)`
- [x] Add a test asserting engine state (render pass/JS state) is unchanged across a hide/show cycle — no restart occurs — `chrome_set_visible_does_not_change_engine_render_state`
- [x] Add a test asserting represented elements are addressed by their stable DOM node ID, not a transient index — throughout `ai_snapshot`'s own tests plus `act_on_an_unknown_id_is_a_harmless_no_op`
- [x] Document the AI-facing API for consumers (this feeds Phase 6) — this document plus the type-level docs on `blueice_ipc::ai` and `blueice_engine::ai_snapshot`

All checklist items are done for the scope described above; the deferred `protocol_version` handshake is recorded, not silently dropped.
