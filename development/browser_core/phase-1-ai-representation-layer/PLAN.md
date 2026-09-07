# Phase 1 — AI Representation Layer & API Shape

[← Back to plan](../BROWSER_CORE_PLAN.md)

**Status**: In progress

## Objective

Plan §3's representation-layer decision is settled (an accessibility-tree-shaped schema, keyed by BlueIce's stable `NodeId`, extended with four fields — see plan §3). What's left in this phase is defining the actual shape of the API the AI side receives: data format, versioning, how elements are addressed and acted on, and how it stays synchronized with human-facing UI state. This still gates Phase 5 (the AI representation output path can't be implemented until the API shape is defined) and should be settled before Phase 3/4 implementation work gets far enough that changes become expensive.

## How §3 was decided

Not by an independent validation exercise — the Accessibility Tree candidate is already proven at scale in the two production browsers this project studies (Gecko and Blink both drive real screen readers off it; Blink's schema is now also the basis of Chromium's own emerging AI-agent code). Reading how each engine actually implements it (`../research/accessibility-tree.md`, `../research/dom.md`) was sufficient to settle the choice directly — see [`spike.md`](spike.md) for the worked example that fixed the concrete schema (which fields beyond a stock accessibility tree are actually needed), not to validate feasibility that was already established by prior art.

**Two constraints from plan §1 apply to the API shape regardless**: every element must be addressed by the DOM node's stable ID (not a transient index/coordinate alone), and the AI-facing API needs a browser-chrome control surface — distinct from the page-content representation — for actions like show/hide that operate on the window rather than on a page.

## API shape (decided)

Per `BROWSER_CORE_PLAN.md` §1's convergence point: `core` already exposes `blueice-ipc`'s `ClientMessage`/`ServerMessage` control-plane protocol to `frontend` (Phase 4, shipped); the AI-facing API is a third client of that *same* surface, not a separately-designed channel. Everything below extends that existing enum rather than inventing a parallel one. Implementation is Phase 5's job (`phase-5-ai-representation-output/PLAN.md`) — this section fixes the concrete shape so Phase 5 has a spec to build against, the same relationship Phase 2's HTML/CSS scope has to Phase 3's implementation.

### 1. Data format/schema

A flat snapshot, one entry per semantically-relevant node (the `spike.md` exclusion rule carries over unchanged: `display:none` and purely-decorative non-semantic nodes are absent, matching human perception rather than being a gap):

```rust
pub struct AiSnapshot {
    pub generation: u64,           // same counter ServerMessage::FrameReady already uses --
                                    // a snapshot and a frame sharing a generation number is
                                    // the concrete, checkable proof they came from the same
                                    // render pass (Phase 5's own "same render pass" test).
    pub url: Option<String>,
    pub scroll_y: f64,
    pub nodes: Vec<AiNode>,
}

pub struct AiNode {
    pub id: u64,                   // blueice_dom::NodeId's raw value -- the stable ID (plan §1),
                                    // not a transient index recomputed per snapshot.
    pub parent: Option<u64>,
    pub children: Vec<u64>,
    pub role: Role,
    pub name: Option<String>,
    pub name_from: Option<NameFrom>,   // provenance-tagged, per AXNodeData's NameFrom (research/accessibility-tree.md §2)
    pub state: NodeState,
    pub bounds: Bounds,             // document-content coordinates (pre-scroll, same space Fragment
                                     // already uses) -- deliberately NOT viewport-relative, so bounds
                                     // don't need recomputing on every scroll with no underlying
                                     // layout change; combine with AiSnapshot::scroll_y for on-screen position.
    pub opacity: f32,               // 0.0-1.0; pinned at 1.0 until Phase 2's opacity-property
                                     // prerequisite (see phase-2-mvp-scope/PLAN.md's cross-check) lands
    pub animating: Option<AnimatingState>,   // always None for MVP -- CSS transitions/animations
                                              // are an explicitly deferred cascade feature (Phase 2)
    pub occluded: bool,
    pub occluded_by: Option<u64>,
    pub occluded_fraction: f32,
}

pub struct NodeState { pub checked: Option<bool>, pub disabled: bool, pub required: bool, pub selected: bool, pub hovered: bool, pub focused: bool }
pub struct Bounds { pub x: f64, pub y: f64, pub width: f64, pub height: f64 }
pub struct AnimatingState { pub property: String, pub current_value: String }
pub enum NameFrom { Contents, Attribute(String), Placeholder, Title, Value }
pub enum Role { Heading { level: u8 }, Link, Button, TextBox, CheckBox, List, ListItem, Paragraph, Image, Generic /* extended as Phase 2's HTML element list needs */ }
```

`Role` is inferred from the Phase 2 HTML element list itself (`h1`-`h6` → `Heading`, `a[href]` → `Link`, `button`/`input[type=submit]` → `Button`, `input[type=checkbox]` → `CheckBox`, `ul`/`ol` → `List`, `li` → `ListItem`, ...), with an explicit `role`/`aria-*` attribute (already carried through unrejected by the parser, per Phase 2's HTML scope) overriding the inferred value when present — Phase 2's cross-check already confirmed every field here is sourceable from the current MVP scope except `opacity` and `animating`, both accounted for above as known, tracked gaps rather than surprises.

### 2. Addressing and acting on elements

Deliberately **not** coordinate-based for the AI path (unlike `frontend`'s existing `Click{x,y}`/`Scroll{delta_y}`, which stay as they are for the human/mouse path) — an agent names a `NodeId` and `core` resolves it to a fragment/bounds internally, the same way `Page::click`'s existing hit-testing already turns a point into a node, just run in the opposite direction (ID → current bounds → act) so the agent never has to do pixel math or re-derive bounds itself before acting:

```rust
pub enum NodeAction { Click, Focus, SetValue(String), ScrollIntoView }
```

`Click` on a link reuses `Page`'s existing href-follow-and-navigate behavior (`session.rs`'s current `ClientMessage::Click` handling), just entered by ID instead of a hit-tested point. `SetValue` maps to the `.value`/`.checked` DOM-binding surface Phase 2's JS scope already named (mutates `Page` state directly here, independent of whether BlueJS has run) — it changes DOM/AX state immediately but won't visually change the painted frame until a later phase renders input values as text, which is fine: the two are allowed to be temporarily out of sync in that one narrow, already-known-and-documented way, not silently wrong.

### 3. Versioning/stability policy

**Decision**: one coarse `protocol_version: u32`, declared once by both sides as the very first message on a new connection (`ClientMessage::Hello { protocol_version }` / `ServerMessage::Hello { protocol_version }`), bumped only on a breaking change (a variant removed/renamed, a field's meaning changed) — adding a new variant or a new `#[serde(default)]` field is not a breaking change and doesn't bump it. `core` rejects a connection whose declared version it doesn't support with a `ServerMessage::Error` before processing anything else, rather than silently misbehaving on a mismatched client. Deliberately coarse (whole-protocol version, not per-message/per-field capability negotiation): this project has one protocol and a handful of client kinds (`frontend`, later `extension`/AI/Phase 12's MCP adapter), not an ecosystem of independently-versioned third parties that would justify finer-grained negotiation machinery. Both `ClientMessage`/`ServerMessage` should also gain a `#[serde(other)] Unknown` catch-all variant as part of this so an older client that receives a message type added after it shipped fails soft (logs and ignores) rather than erroring out the whole connection on `serde_json`'s default "unrecognized enum variant" behavior.

### 4. Sync with human-facing UI state

Read direction (state → AI) is what `NodeState::hovered`/`focused` already are: both are read from the same interaction state a human-facing hover/focus effect would use, not independently tracked. This is exactly the `Page`-level gap Phase 2's cross-check found (`Page` has no persistent hover/focus state today) — closing it is what makes this direction possible at all, and it has one more concrete consequence found here: `frontend`'s `CursorMoved` handler currently only remembers the cursor position locally for the next `Click`; it needs a new lightweight `ClientMessage::Hover { x, y }` (viewport coordinates, resolved to a node ID inside `core` the same way `Click` already is) so `core` becomes the single source of truth for "what's hovered," rather than that state living only inside `frontend`'s own process where the AI path can't see it. Added as a third Phase 5 prerequisite below, alongside the two Phase 2 already recorded.

Write direction (AI → human-visible state) is new: `ClientMessage::Highlight { id: Option<u64> }` (`None` clears it), stored as a small piece of `Page` state and drawn as an additional outline `PaintCommand` derived fresh from that node's *current* fragment bounds on every paint — not a separately-tracked screen-space rectangle a caller could let drift out of sync with scroll/resize/relayout. This is plan §1's "stay in sync by ID rather than by structural position" applied concretely: the highlight is keyed to the `NodeId`, so it automatically tracks the node through any layout change instead of needing to be recomputed or reissued by the caller.

### 5. Browser-chrome control surface

`ClientMessage::SetVisible(bool)` already exists (Phase 4) but sits as a flat, ungrouped variant alongside page-content messages, which is exactly what plan §1 asks to keep separate. **Decision**: nest it under a dedicated `ClientMessage::Chrome(ChromeCommand)` with `ChromeCommand::SetVisible(bool)` as its only variant today, reserving room for chrome-level actions later without those ever being confusable with page-content actions (`Navigate`, `ActOn`, `Highlight`, ...). This is a breaking wire-format change to what Phase 4 already shipped, so Phase 5 applies it together with the version bump from the policy above — not two separate protocol changes.

## Phase 5 prerequisites surfaced here (see also `phase-5-ai-representation-output/PLAN.md`)

1. `opacity` isn't in the MVP CSS property list (Phase 2's cross-check) — needed before `AiNode::opacity` can be anything but a hardcoded `1.0`.
2. `Page` has no persistent hover/focus state (Phase 2's cross-check) — needed before `NodeState::hovered`/`focused` can be populated at all.
3. `frontend`'s `CursorMoved` handling needs to forward hover position to `core` via a new `ClientMessage::Hover{x,y}` (found here, in item 4 above) — otherwise `core` has no way to learn what's hovered even once prerequisite 2 gives it somewhere to store that state.

## Checklist

- [x] Read Gecko's and Blink's accessibility-tree implementations in `../reference/`; write up findings in `../research/accessibility-tree.md`
- [x] Settle the §3 representation-layer decision (accessibility-tree-shaped schema + four fields, keyed by stable `NodeId`) — recorded in `BROWSER_CORE_PLAN.md` §3, worked example in `spike.md`
- [x] Define the AI-facing data format/schema for the chosen representation as a concrete, implementable spec (field types, not just field names) — see "API shape (decided)" §1 above
- [x] Define how the API exposes each element by its stable DOM node ID (plan §1), plus coordinates, in a way an agent can act on (click, type, scroll) — see §2 above (`NodeAction`, ID-addressed rather than coordinate-based)
- [x] Define versioning/stability policy for the AI-facing API — see §3 above (`protocol_version` handshake, additive-only without a bump)
- [x] Define how this representation and human-facing UI state (e.g. highlights) stay synchronized, per the sync goal in plan §1 — see §4 above (`hovered`/`focused` read from shared `Page` state; `Highlight{id}` for the write direction, keyed by ID not screen position)
- [x] Define a browser-chrome control surface in the API (starting with show/hide the window) separate from the page-content representation — see §5 above (`ClientMessage::Chrome(ChromeCommand::SetVisible)`)

All checklist items are done. The API shape defined here is the concrete spec [Phase 5](../phase-5-ai-representation-output/PLAN.md) implements; its own plan now carries the three prerequisites this phase surfaced.
