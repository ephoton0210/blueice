// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The AI-facing representation's wire types, per the concrete schema
//! `phase-1-ai-representation-layer/PLAN.md` settled and
//! `phase-5-ai-representation-output/PLAN.md` implements. Lives here
//! (not in `blueice-engine`) for the same reason `ClientMessage`/
//! `ServerMessage` do: this is what actually crosses the `core`<->
//! client wire, so its shape belongs with the rest of the protocol,
//! not buried in the crate that happens to compute it.

use serde::{Deserialize, Serialize};

/// One `core`-served snapshot of the accessibility-tree-shaped
/// representation plan §3 settled on -- a flat list (not a nested
/// tree) since a wire format addressed by ID is simpler to look things
/// up in than a recursive structure, with [`AiNode::parent`]/
/// [`AiNode::children`] carrying the tree shape as plain ID
/// references.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AiSnapshot {
    /// The same counter [`crate::ServerMessage::FrameReady`] uses --
    /// a snapshot and a frame sharing a generation number is the
    /// concrete, checkable proof they came from the same render pass
    /// (`phase-5-ai-representation-output/PLAN.md`'s "same render
    /// pass" requirement).
    pub generation: u64,
    /// Which tab this snapshot is of -- `phase-16-multi-tab-and-tab-
    /// groups/PLAN.md`'s wire-protocol addressing. Carried here (not
    /// just on the envelope's `tab_id`) because a consumer like
    /// `blueice-mcp-server` unwraps `ServerMessage::Representation`
    /// before handing it on to its own MCP tool result, at which point
    /// the envelope is gone -- without this field an AI agent would
    /// have no self-contained way to know which tab a snapshot came
    /// from.
    pub tab_id: u64,
    pub url: Option<String>,
    pub scroll_y: f64,
    pub nodes: Vec<AiNode>,
}

/// One semantically-relevant node. Purely decorative/non-semantic
/// nodes (a bare `<div>`/`<span>` with no accessible name and no
/// explicit `role`) are never represented at all, matching
/// `phase-1-ai-representation-layer/spike.md`'s finding that this is
/// deliberate parity with human perception, not a gap.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AiNode {
    /// `blueice_dom::NodeId`'s raw value -- the stable ID (plan §1),
    /// not a transient index recomputed per snapshot.
    pub id: u64,
    pub parent: Option<u64>,
    pub children: Vec<u64>,
    pub role: Role,
    pub name: Option<String>,
    pub name_from: Option<NameFrom>,
    pub state: NodeState,
    /// Document-content coordinates (pre-scroll, the same space
    /// `blueice_layout::Fragment` already uses) -- combine with
    /// [`AiSnapshot::scroll_y`] for on-screen position. Deliberately
    /// not viewport-relative so bounds don't need recomputing on every
    /// scroll when nothing actually laid out differently.
    pub bounds: Bounds,
    pub opacity: f32,
    pub occluded: bool,
    pub occluded_by: Option<u64>,
    pub occluded_fraction: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Bounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct NodeState {
    /// The current value of a supported native text input, when the
    /// document supplies one. Keeping this in the representation makes a
    /// successful [`NodeAction::SetValue`] observable through the same
    /// core-owned state that the human frame renders.
    #[serde(default)]
    pub value: Option<String>,
    pub checked: Option<bool>,
    pub disabled: bool,
    pub required: bool,
    pub selected: bool,
    pub hovered: bool,
    pub focused: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Role {
    Heading {
        level: u8,
    },
    Link,
    Button,
    TextBox,
    CheckBox,
    /// A native numeric range input. Its `NodeState::value` is the current
    /// core-owned value, allowing a constrained extension write to remain
    /// visible through the same representation it addressed.
    Slider,
    /// A native single- or multiple-choice select control. Its visible
    /// options are represented as [`Self::Option`] children when they have
    /// layout fragments, so a capability-constrained extension can address a
    /// live choice through the same stable-ID representation it reads.
    ComboBox,
    /// One native `<option>` inside a represented select control.
    Option,
    List,
    ListItem,
    Paragraph,
    Image,
    /// An element with an explicit `role`/`aria-label` attribute that
    /// doesn't map to one of the more specific roles above.
    Generic,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum NameFrom {
    Contents,
    Attribute(String),
    Placeholder,
    Title,
}

/// An element addressed by its stable [`AiNode::id`] and the action to
/// perform on it -- deliberately not coordinate-based, unlike
/// `frontend`'s existing `ClientMessage::Click`/`Scroll`: `core`
/// resolves the ID to its current bounds internally, so an agent never
/// has to do pixel math or re-derive bounds before acting.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum NodeAction {
    /// Reuses the same href-follow-and-navigate behavior a coordinate
    /// `ClientMessage::Click` on a link already has.
    Click,
    Focus,
    /// Sets the target's `value` attribute directly (the DOM-binding
    /// surface `phase-2-mvp-scope/PLAN.md`'s JS scope named). The core
    /// relayouts after the mutation, so supported text inputs show the
    /// resulting value in both the rendered frame and [`NodeState`].
    SetValue(String),
    /// Scrolls the viewport so the target's top aligns with the
    /// viewport's top, clamped to the page's scrollable range -- a
    /// simple alignment policy, not real "minimal scroll to bring
    /// fully into view" (which would need to know the target's
    /// position relative to the *current* viewport, not just the
    /// page).
    ScrollIntoView,
}
