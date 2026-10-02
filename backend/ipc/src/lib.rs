// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Control-plane IPC protocol shared by `core`'s out-of-process clients
//! (`extension`, `frontend`, and eventually the Phase 5 AI-facing API),
//! per `BROWSER_CORE_PLAN.md` §1 and `research/frontend-ipc.md`.
//!
//! Two logically separate channels, matching what both Chromium
//! (`WidgetHost`/`WidgetInputHandler`/`FrameWidget` vs.
//! `CompositorFrameSink`) and Gecko (`PCompositorBridge` vs.
//! `PWebRenderBridge`) settled on independently, per the research:
//! **control-plane** (this crate) -- small, low-frequency structured
//! messages (navigation, input, lifecycle) -- and **frame-plane** --
//! the actual pixel data, which this crate deliberately never carries
//! (see [`ServerMessage::FrameReady`]: a shared-memory path plus
//! metadata, not bytes). Wire format is length-prefixed JSON: not the
//! most compact framing, but `serde_json` is mature/correct and this is
//! explicitly the "fastest to validate the boundary" reference
//! implementation (`phase-4-human-rendering-path/PLAN.md`) -- swapping
//! in a smaller binary encoding later doesn't change any type in this
//! module, only [`write_message`]/[`read_message`]'s internals.
//!
//! Like `ureq`/`fontdue`/`winit` elsewhere in Phase 4, this crate uses
//! `serde`/`serde_json` rather than hand-rolling encoding -- this is
//! infrastructure (a solved problem), not the parsing/cascade/layout
//! domain this project is written from scratch for.

use serde::{Deserialize, Serialize};
use std::io::{self, Read, Write};

/// Largest accepted length-prefixed JSON control-plane message. Pixel data is
/// deliberately on the shared-memory frame plane, so an IPC peer never needs
/// to make us allocate an unbounded buffer for it.
pub const MAX_FRAME_BYTES: usize = 8 * 1024 * 1024;

pub mod ai;
pub mod assistant;
pub mod compiler;
pub mod compiler_catalog;
pub mod compiler_output;
pub mod debugger;
pub mod downloads;
pub mod extension;
pub mod gatekeeper;
pub mod input;
pub mod local_socket;
pub mod owner_bootstrap;
pub mod page_host;
pub mod permission_control;
pub mod script;
pub mod shm;

pub use ai::{AiNode, AiSnapshot, Bounds, NameFrom, NodeAction, NodeState, Role};

/// The whole-protocol version this build of `blueice-ipc` speaks, per
/// `phase-1-ai-representation-layer/PLAN.md` §3's versioning decision:
/// one coarse version, bumped only on a breaking change (a variant
/// removed/renamed, a field's meaning changed) -- adding a new variant
/// or a new `#[serde(default)]` field does not bump it.
pub const PROTOCOL_VERSION: u32 = 2;

/// Browser-chrome control actions -- distinct from page-content
/// messages (`Navigate`, `ActOn`, ...) per
/// `phase-1-ai-representation-layer/PLAN.md`'s API-shape decision:
/// these operate on the window/`core` instance itself, not on the
/// current page's content, so they get their own nested group rather
/// than sitting as a same-level `ClientMessage` variant indistinguishable
/// from page actions.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum ChromeCommand {
    /// Per plan §1: the human-facing window's visibility is a property
    /// of the windowing layer, not of whether `core` exists -- this
    /// message exists so that requirement is something the *protocol*
    /// carries (and an AI-facing client can drive too), not just an
    /// implementation detail private to one frontend's window object.
    SetVisible(bool),
}

/// Sent by a client (`frontend` today; `extension`/AI later) to `core`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ClientMessage {
    /// The protocol_version handshake (`phase-1-ai-representation-
    /// layer/PLAN.md` §3): every independent client sends this
    /// immediately after connecting, before anything else -- `core`
    /// rejects a fresh connection whose first message isn't this, or
    /// whose declared version it doesn't support, with a
    /// [`ServerMessage::Error`] before processing anything else. Once a
    /// connection has passed that initial gate, a later `Hello` (e.g.
    /// one `blueice-launcher`'s broker forwards from a second external
    /// client sharing the same underlying `core` connection) is just
    /// answered again rather than re-gating the whole session, since
    /// tearing down a shared connection over one client's handshake
    /// would end every other client's session too.
    Hello {
        protocol_version: u32,
    },
    /// Load a new URL, replacing the current page.
    Navigate {
        url: String,
    },
    /// Restore the addressed tab's preceding session-history entry. This is
    /// intentionally a tab-addressed page operation, never a global "active
    /// tab" command: separate frontend and MCP observers may be working in
    /// different tabs at the same time.
    GoBack,
    /// Restore the addressed tab's following session-history entry.
    GoForward,
    /// Requests whether the addressed tab can currently go backward and/or
    /// forward, replied to with [`ServerMessage::HistoryState`]. Kept
    /// separate from `ListTabs`: history availability can change without a
    /// tab being opened, closed, or selected.
    GetHistoryState,
    /// Turns live translation of pages fetched *after* this message on (a
    /// BCP 47 tag such as `zh-TW`) or off (`None`). The setting is core-wide;
    /// which assistant serves it is fixed by `blueice-core`'s startup flags, so
    /// a client can choose a language but never point `core` at a socket. An
    /// error reply says translation is unavailable when core has no assistant.
    /// Replied to with [`ServerMessage::TranslationState`].
    SetTranslationLanguage {
        target_language: Option<String>,
    },
    /// Shows the addressed tab's translation (`true`) or the page's original
    /// text (`false`), replied to with [`ServerMessage::TranslationState`]
    /// and, when the page changed, a fresh frame.
    ShowTranslation {
        shown: bool,
    },
    /// Requests the translation state, replied to with
    /// [`ServerMessage::TranslationState`].
    GetTranslationState,
    /// Asks the local assistant to summarize the addressed tab's shown text.
    /// The reply arrives when the task finishes -- a [`ServerMessage::
    /// AssistantResult`] or an `Error` carrying the same `request_id` -- and
    /// the result is also added to the `about:assistant` page. `core` keeps
    /// answering other traffic meanwhile.
    SummarizePage,
    /// Asks the local assistant to reorganize the addressed tab's shown text
    /// per `instruction` (for example "make a table of names and prices").
    /// Completes like [`ClientMessage::SummarizePage`].
    OrganizePage {
        instruction: String,
    },
    /// The viewport size changed; `core` re-lays-out at the new width.
    Resize {
        width: u32,
        height: u32,
    },
    /// A click at a point in viewport coordinates (post-scroll, i.e.
    /// `(0,0)` is always the top-left of what's currently visible).
    Click {
        x: f64,
        y: f64,
    },
    /// The pointer moved to this viewport point (same coordinate space
    /// as `Click`) -- `core` resolves it to a node the same way `Click`
    /// already does, becoming the single source of truth for "what's
    /// hovered" so both a future `:hover` visual effect and the AI-
    /// facing `NodeState::hovered` field read from the same state
    /// rather than two independently-tracked copies
    /// (`phase-1-ai-representation-layer/PLAN.md` §4).
    Hover {
        x: f64,
        y: f64,
    },
    /// Scroll the viewport by this many CSS pixels (positive = down).
    Scroll {
        delta_y: f64,
    },
    /// Inserts text at the end of the focused native text input. This is a
    /// deliberately narrow human-input path: the core identifies the focused
    /// input itself and refuses to turn keyboard events into arbitrary DOM
    /// writes.
    InsertText {
        text: String,
    },
    /// Removes the final Unicode scalar from the focused native text input.
    /// Like [`Self::InsertText`], this is a no-op unless the page has a
    /// currently focused supported text input.
    DeleteBackward,
    /// Inspect the current document's native selection/composition geometry.
    /// Protected control contents are redacted.
    GetTextInputState,
    /// Versioned, document-fenced native editing of the core's focused control.
    TextInput {
        context: input::TextInputContext,
        action: input::TextInputAction,
    },
    /// Requests a fresh [`AiSnapshot`] of the current page, replied to
    /// with [`ServerMessage::Representation`].
    GetRepresentation,
    /// Act on a specific, stably-addressed element -- see
    /// [`NodeAction`]'s own docs for why this is ID-addressed rather
    /// than coordinate-based.
    ActOn {
        id: u64,
        action: NodeAction,
    },
    /// Highlights `id` (drawn as an outline derived fresh from that
    /// node's current bounds on every paint) or clears the highlight
    /// (`None`) -- the AI-to-human sync direction plan §1 asks for:
    /// keyed by ID, so it automatically tracks the node through any
    /// layout change instead of a caller having to recompute a screen
    /// rectangle itself.
    Highlight {
        id: Option<u64>,
    },
    /// Requests a full DOM tree dump (`blueice_dom::dump`'s canonical
    /// text format -- the same one `blueice-testing`'s fixture corpus
    /// checks `blueice-html` against), replied to with
    /// [`ServerMessage::Dom`]. Deliberately separate from
    /// [`ClientMessage::GetRepresentation`]: the AI-facing snapshot
    /// intentionally excludes purely-decorative/non-semantic nodes
    /// (`phase-1-ai-representation-layer/spike.md`), which is exactly
    /// what a structural comparison against a real browser's DOM (the
    /// Chromium differential-testing harness, `TEST_PLAN.md`) needs to
    /// *not* have filtered out.
    GetDom,
    /// Requests source-free execution outcomes for inline BlueTS scripts in
    /// the addressed tab, replied to with [`ServerMessage::BlueTsScriptReports`].
    /// This is observability only: it neither enables inline execution nor
    /// exposes a script's source, diagnostics, or runtime values.
    GetBlueTsScriptReports,
    /// Requests source-free execution outcomes for standard JavaScript scripts
    /// in the addressed tab, replied to with [`ServerMessage::BlueJsScriptReports`].
    /// This is observability only: it neither enables JavaScript execution nor
    /// exposes a script's source, diagnostics, or runtime values.
    GetBlueJsScriptReports,
    /// Opens a new, blank tab, replied to with [`ServerMessage::TabOpened`]
    /// -- `phase-16-multi-tab-and-tab-groups/PLAN.md`'s minimal first
    /// slice. `url` is optional purely for convenience (equivalent to
    /// opening a blank tab, then a `Navigate` addressed to it); `None`
    /// opens a blank tab.
    OpenTab {
        url: Option<String>,
    },
    /// Closes the addressed tab (the envelope's `tab_id`, same as
    /// every other per-tab message -- not a redundant inline field),
    /// replied to with [`ServerMessage::TabClosed`]. Closing the last
    /// remaining tab (or the tab a bare, untagged envelope would
    /// otherwise resolve to) is allowed -- `core` doesn't force a tab
    /// to always exist.
    CloseTab,
    /// Requests the current tab list, replied to with
    /// [`ServerMessage::Tabs`].
    ListTabs,
    /// Creates a named, colored tab group. Groups are core-owned session
    /// state rather than window-local chrome: another observer (for example
    /// an MCP client) must see the same organization as a human frontend.
    CreateTabGroup {
        name: String,
        color: String,
    },
    /// Puts the addressed tab into `group_id`, or removes it from its group
    /// when `group_id` is `None`. The tab itself is addressed by the
    /// envelope, just like `CloseTab`.
    SetTabGroup {
        group_id: Option<u64>,
    },
    /// Changes one existing group's human-visible name.
    RenameTabGroup {
        group_id: u64,
        name: String,
    },
    /// Changes one existing group's human-visible color. Colors are CSS
    /// `#RRGGBB` strings, so native frontends can render the exact same
    /// stable value without interpreting arbitrary CSS.
    SetTabGroupColor {
        group_id: u64,
        color: String,
    },
    /// Collapses or expands one existing group. Collapse affects each
    /// frontend's tab-strip presentation, not the group's member tabs'
    /// independent navigable state.
    SetTabGroupCollapsed {
        group_id: u64,
        collapsed: bool,
    },
    /// Removes a group but leaves its member tabs open and ungrouped.
    CloseTabGroup {
        group_id: u64,
    },
    /// Requests every tab group in creation order, replied to with
    /// [`ServerMessage::TabGroups`].
    ListTabGroups,
    /// Query the currently published extension toolbar button, if any.
    GetExtensionToolbar,
    /// Activate the visible extension toolbar button for the addressed tab.
    /// This is a UI action, not an authenticated user-gesture capability.
    ActivateExtensionToolbar,
    /// Query the current core-owned extension popup, if any.
    GetExtensionPopup,
    /// Dismiss a popup only when it belongs to the addressed live tab.
    DismissExtensionPopup,
    /// Activate the current popup's one native action button. The core checks
    /// the popup ID so a stale click cannot activate a newer popup. As with
    /// toolbar activation, this wire message is not proof of a human gesture.
    ActivateExtensionPopupAction {
        popup_id: u64,
    },
    Chrome(ChromeCommand),
    Shutdown,
    /// Catch-all for a variant this build doesn't recognize (e.g. sent
    /// by a newer client than this `core`, or vice versa) -- per plan
    /// §3's versioning decision, an unrecognized *variant* fails soft
    /// (ignored) rather than erroring out the whole connection the way
    /// `serde_json`'s default unrecognized-enum-variant behavior would;
    /// only an unsupported *protocol_version* in [`ClientMessage::Hello`]
    /// is treated as fatal.
    #[serde(other)]
    Unknown,
}

/// Which assistant task produced a [`ServerMessage::AssistantResult`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AssistantTaskKind {
    Summary,
    Organized,
}

/// Sent by `core` to a client.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ServerMessage {
    /// Reply to [`ClientMessage::Hello`]: `protocol_version` is this
    /// `core`'s own, echoed so the client can also self-check
    /// compatibility, not just rely on not having received an
    /// [`ServerMessage::Error`].
    Hello {
        protocol_version: u32,
    },
    /// A new frame is available. `shm_path` names a shared-memory-
    /// backed file the client maps read-only; `generation` increases on
    /// every frame within one core generation; it resets on a cutover.
    /// Compare the frame directory source as well before dropping stale
    /// notifications (playing the role Chromium's `SyncToken`/Gecko's
    /// fence do for producer/consumer synchronization, per
    /// `research/frontend-ipc.md` §4) without needing its own clock.
    FrameReady {
        shm_path: String,
        width: u32,
        height: u32,
        generation: u64,
    },
    /// Native text state for one live document and render generation.
    TextInputState(input::TextInputState),
    /// Navigation finished (or failed) -- `url` is the final URL after
    /// following any redirects.
    Navigated {
        url: String,
    },
    /// Reply to [`ClientMessage::GetHistoryState`]. Like every page reply it
    /// is envelope-addressed to a concrete tab, never a global active tab.
    HistoryState {
        can_go_back: bool,
        can_go_forward: bool,
    },
    /// Reply to [`ClientMessage::SetTranslationLanguage`],
    /// [`ClientMessage::ShowTranslation`], and
    /// [`ClientMessage::GetTranslationState`]. `language` is the core-wide
    /// target for later navigations (`None` = off); `available` says the
    /// addressed page has translated text to toggle; `shown` says the
    /// translation (rather than the original) is on screen.
    TranslationState {
        language: Option<String>,
        available: bool,
        shown: bool,
    },
    /// A finished [`ClientMessage::SummarizePage`] or
    /// [`ClientMessage::OrganizePage`]. The text is model output derived from
    /// untrusted page text and must be treated as such.
    AssistantResult {
        kind: AssistantTaskKind,
        text: String,
    },
    /// Reply to [`ClientMessage::GetRepresentation`].
    Representation(AiSnapshot),
    /// Reply to [`ClientMessage::GetDom`].
    Dom(String),
    /// Reply to [`ClientMessage::GetBlueTsScriptReports`].
    ///
    /// The reports deliberately include only stable tab/document identity,
    /// script kind, ordinal, and a source-free outcome category. They are not
    /// an execution-result, diagnostic, or source-inspection API.
    BlueTsScriptReports(Vec<BlueTsScriptExecutionReport>),
    /// Reply to [`ClientMessage::GetBlueJsScriptReports`]. These bounded
    /// records have the same source-free shape as BlueTS reports, but identify
    /// standard JavaScript declarations executed by an explicitly enabled
    /// BlueJS page host.
    BlueJsScriptReports(Vec<BlueJsScriptExecutionReport>),
    /// Reply to [`ClientMessage::OpenTab`]. `url` reflects whatever
    /// actually ended up loaded -- `None` for a blank tab (`OpenTab`
    /// was given no `url`), `Some(final_url)` once a requested
    /// navigation succeeds. A navigation failure inside `OpenTab`
    /// replies [`ServerMessage::Error`] instead of this (the new tab
    /// still exists, just blank -- a real, narrow limitation of this
    /// minimal first slice: the client isn't told that orphaned tab's
    /// id directly, though `ListTabs` will show it).
    TabOpened {
        tab_id: u64,
        url: Option<String>,
    },
    /// Reply to [`ClientMessage::CloseTab`].
    TabClosed {
        tab_id: u64,
    },
    /// Reply to [`ClientMessage::ListTabs`].
    Tabs(Vec<TabSummary>),
    /// Reply to [`ClientMessage::CreateTabGroup`].
    TabGroupCreated(TabGroupSummary),
    /// Reply to any update to a group's own properties
    /// (`RenameTabGroup`/`SetTabGroupColor`/`SetTabGroupCollapsed`).
    TabGroupUpdated(TabGroupSummary),
    /// Reply to [`ClientMessage::SetTabGroup`].
    TabGroupAssigned {
        tab_id: u64,
        group_id: Option<u64>,
    },
    /// Reply to [`ClientMessage::CloseTabGroup`]. Its former member tabs
    /// remain open and have `group_id: None`.
    TabGroupClosed {
        group_id: u64,
    },
    /// Reply to [`ClientMessage::ListTabGroups`].
    TabGroups(Vec<TabGroupSummary>),
    /// Current native toolbar label, returned for a query or broadcast when
    /// the installed extension changes/disconnects. `None` removes it.
    ExtensionToolbar {
        label: Option<String>,
    },
    /// A native, non-interactive extension message associated with a tab.
    /// Frontends must distinguish it visually from browser-owned UI.
    ExtensionPopup {
        popup: Option<ExtensionPopup>,
    },
    Error {
        message: String,
    },
    /// A navigation was blocked by `ai-gatekeeper`'s review (either the
    /// URL stage or the content stage, see [`crate::gatekeeper`]) --
    /// `phase-7-local-ai/PLAN.md`'s "Wiring design": `core` never
    /// applies a fetched page to its engine state without a cleared
    /// verdict from *both* stages (fail-closed, including when the
    /// gatekeeper process itself is unreachable), so a client sees this
    /// instead of [`ServerMessage::Navigated`]/[`ServerMessage::
    /// TabOpened`] whenever that review didn't clear. `url` is the
    /// (possibly post-redirect) URL the blocked review was about.
    GatekeeperBlocked {
        reason: String,
        category: String,
        url: String,
    },
    /// See [`ClientMessage::Unknown`] -- the same forward-compatibility
    /// fallback, in the other direction.
    #[serde(other)]
    Unknown,
}

/// One tab's summary, as reported by [`ServerMessage::Tabs`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TabSummary {
    pub id: u64,
    pub url: Option<String>,
    /// The core-owned group this tab belongs to, if any. Optional by design:
    /// groups are an additive Phase-16 capability and ungrouped tabs remain
    /// first-class.
    #[serde(default)]
    pub group_id: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtensionPopup {
    /// Core-assigned identity for rejecting stale activation messages.
    #[serde(default)]
    pub id: u64,
    pub tab_id: u64,
    pub title: String,
    pub body: String,
    /// A single browser-rendered button; absent for the v2 text-only popup.
    #[serde(default)]
    pub action_label: Option<String>,
}

/// One tab group's observable state, as reported by
/// [`ServerMessage::TabGroups`] and group mutation replies. This small wire
/// summary deliberately carries no frontend-local selection state: different
/// observers can view different tabs while agreeing on the organization.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TabGroupSummary {
    pub id: u64,
    pub name: String,
    /// A canonical CSS `#RRGGBB` color.
    pub color: String,
    pub collapsed: bool,
}

/// The HTML script classification used by a [`BlueTsScriptExecutionReport`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BlueTsScriptKind {
    Classic,
    Module,
}

/// The source-free outcome of an inline BlueTS script attempt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BlueTsScriptExecutionOutcome {
    Executed,
    Rejected {
        /// A bounded policy/compiler category, never source or diagnostics.
        category: String,
    },
}

/// Core-selected BlueTS runtime policy; transpile-only cannot run on pages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BlueTsScriptRuntimePolicy {
    Checked,
    StrictRuntime,
}

/// A verified half-open byte range in the original inline BlueTS input.
/// The report never carries its module identity or source contents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlueTsScriptSourcePosition {
    pub start: u32,
    pub end: u32,
}

/// One source-free inline BlueTS execution report for a tab/document pair.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlueTsScriptExecutionReport {
    pub tab_id: u64,
    pub document_generation: u64,
    pub ordinal: u32,
    pub kind: BlueTsScriptKind,
    pub policy: BlueTsScriptRuntimePolicy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_position: Option<BlueTsScriptSourcePosition>,
    pub outcome: BlueTsScriptExecutionOutcome,
}

/// The HTML script classification used by a [`BlueJsScriptExecutionReport`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BlueJsScriptKind {
    Classic,
    Module,
}

/// The source-free outcome of one standard JavaScript script attempt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BlueJsScriptExecutionOutcome {
    Executed,
    Rejected {
        /// A bounded host/parser/compiler/runtime category, never page source
        /// or diagnostics.
        category: String,
    },
}

/// One source-free standard JavaScript execution report for a tab/document
/// pair. This is not a JavaScript completion, debugger, or source API.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlueJsScriptExecutionReport {
    pub tab_id: u64,
    pub document_generation: u64,
    pub ordinal: u32,
    pub kind: BlueJsScriptKind,
    pub outcome: BlueJsScriptExecutionOutcome,
}

fn write_framed<W: Write, T: Serialize>(w: &mut W, msg: &T) -> io::Result<()> {
    write_framed_with_limit(w, msg, MAX_FRAME_BYTES)
}

pub(crate) fn write_framed_with_limit<W: Write, T: Serialize>(
    w: &mut W,
    msg: &T,
    max_bytes: usize,
) -> io::Result<()> {
    let bytes = serde_json::to_vec(msg).map_err(io::Error::other)?;
    if bytes.len() > max_bytes {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "frame exceeds protocol byte limit",
        ));
    }
    let len = u32::try_from(bytes.len()).map_err(io::Error::other)?;
    w.write_all(&len.to_le_bytes())?;
    w.write_all(&bytes)?;
    w.flush()
}

fn read_frame_bytes<R: Read>(r: &mut R) -> io::Result<Vec<u8>> {
    read_frame_bytes_with_limit(r, MAX_FRAME_BYTES)
}

/// Reads one framed payload while rejecting an oversized length before any
/// payload allocation. Private protocol modules with materially larger source
/// records use this instead of trusting an unbounded `u32` length from their
/// peer. It is crate-visible so every protocol still shares the same partial-
/// read/timeout framing discipline below.
pub(crate) fn read_frame_bytes_with_limit<R: Read>(
    r: &mut R,
    max_bytes: usize,
) -> io::Result<Vec<u8>> {
    let mut len_bytes = [0u8; 4];
    read_exact_no_progress_loss(r, &mut len_bytes)?;
    let len = u32::from_le_bytes(len_bytes) as usize;
    if len > max_bytes {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("frame length {len} exceeds the protocol maximum of {max_bytes}"),
        ));
    }
    let mut buf = vec![0u8; len];
    read_exact_no_progress_loss(r, &mut buf)?;
    Ok(buf)
}

/// Reads one framed payload while rejecting an advertised size before it can
/// allocate. Retained for compiler IPC's independently documented bound.
fn read_frame_bytes_bounded<R: Read>(r: &mut R, maximum_bytes: usize) -> io::Result<Vec<u8>> {
    read_frame_bytes_with_limit(r, maximum_bytes)
}

/// Like [`Read::read_exact`], but a read *timeout* that occurs after
/// some bytes have already been consumed into `buf` is retried rather
/// than propagated as an error. Plain `read_exact` would propagate it
/// immediately -- silently discarding those already-read bytes forever
/// (the stream's position has already advanced past them), which
/// desynchronizes every frame read afterward on that connection. This
/// matters now that `blueice_engine::session`'s poll loop puts its
/// stream into a short-read-timeout mode ([`core`]'s Phase 7 gated-
/// navigation design) so it can periodically check for other work
/// between client messages -- a length-prefixed frame must never be
/// abandoned mid-read just because the timeout window closed while
/// only *part* of it had arrived.
///
/// A timeout with *zero* bytes read so far for this call is still
/// propagated immediately (nothing has been consumed, so there's
/// nothing to lose) -- this is what lets a caller polling for other
/// work between whole messages actually get control back promptly.
fn read_exact_no_progress_loss<R: Read>(r: &mut R, buf: &mut [u8]) -> io::Result<()> {
    let mut filled = 0;
    while filled < buf.len() {
        match r.read(&mut buf[filled..]) {
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "failed to fill whole buffer",
                ))
            }
            Ok(n) => filled += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e)
                if filled > 0
                    && matches!(
                        e.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                    ) =>
            {
                continue
            }
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// Reads one frame and interprets it as a [`ClientEnvelope`], falling
/// back to [`ClientMessage::Unknown`] (preserving `request_id` if it
/// can still be pulled out of the raw JSON) when the bytes are
/// syntactically valid JSON but don't match a known message -- an
/// unrecognized variant tag, carrying data or not, or a recognized tag
/// with fields this build doesn't know about. `#[serde(other)]` alone
/// only covers a *unit*-shaped unrecognized tag (the default
/// externally-tagged JSON representation serializes those as a bare
/// string), since it can't know in advance what shape a genuinely new,
/// data-carrying variant's content should be parsed as. This is
/// `phase-1-ai-representation-layer/PLAN.md` §3's actual fail-soft
/// guarantee: syntactically malformed JSON (truncated, not JSON at
/// all) still surfaces as a real `io::Error` -- only a
/// well-formed-but-unrecognized *message* is swallowed.
fn read_client_envelope<R: Read>(r: &mut R) -> io::Result<ClientEnvelope> {
    let buf = read_frame_bytes(r)?;
    let value: serde_json::Value = serde_json::from_slice(&buf).map_err(io::Error::other)?;
    if let Ok(envelope) = serde_json::from_value::<ClientEnvelope>(value.clone()) {
        return Ok(envelope);
    }
    let request_id = value.get("request_id").and_then(serde_json::Value::as_u64);
    let tab_id = value.get("tab_id").and_then(serde_json::Value::as_u64);
    Ok(ClientEnvelope {
        request_id,
        tab_id,
        message: ClientMessage::Unknown,
    })
}

/// The [`ServerMessage`] counterpart to [`read_client_envelope`].
fn read_server_envelope<R: Read>(r: &mut R) -> io::Result<ServerEnvelope> {
    let buf = read_frame_bytes(r)?;
    let value: serde_json::Value = serde_json::from_slice(&buf).map_err(io::Error::other)?;
    if let Ok(envelope) = serde_json::from_value::<ServerEnvelope>(value.clone()) {
        return Ok(envelope);
    }
    let request_id = value.get("request_id").and_then(serde_json::Value::as_u64);
    let tab_id = value.get("tab_id").and_then(serde_json::Value::as_u64);
    Ok(ServerEnvelope {
        request_id,
        tab_id,
        message: ServerMessage::Unknown,
    })
}

/// A client-generated correlation id, echoed back verbatim on the
/// [`ServerMessage`] reply a given [`ClientMessage`] produces --
/// closes `phase-8-live-core-hotswap/PLAN.md`'s flagged broadcast-
/// misattribution gap: `blueice-launcher`'s broker broadcasts every
/// `ServerMessage` to every connected external client, so a caller
/// multiplexing several in-flight requests over one connection (or
/// simply sharing a connection with other clients) needs a way to
/// tell "the reply to *my* request" apart from "some other client's
/// concurrent traffic." `None` when a caller doesn't need this (every
/// single-request-at-a-time caller, and every existing test).
// Deliberately *not* `#[serde(flatten)]`: several variants of both
// enums are unit (`GetDom`, `Shutdown`, `Unknown`, ...) and serialize
// as a bare JSON string under the default externally-tagged
// representation, which `flatten` can't merge as object keys into the
// parent envelope. A plain nested `message` field has no such
// restriction on the inner value's JSON shape.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct ClientEnvelope {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    request_id: Option<u64>,
    /// Which tab a per-tab message (`Navigate`/`Resize`/`Click`/...)
    /// applies to -- `phase-16-multi-tab-and-tab-groups/PLAN.md`'s
    /// wire-protocol addressing, the same optional-sibling-field shape
    /// `request_id` already established. `None` means "the default
    /// tab," reproducing pre-Phase-16 single-`Page` behavior
    /// byte-for-byte for a client that never sends `OpenTab`.
    /// Meaningless for connection/window-level messages (`Hello`,
    /// `Shutdown`, `Chrome`) and for tab-lifecycle messages that aren't
    /// scoped to an *existing* tab (`OpenTab`, `ListTabs`) -- callers
    /// just leave it `None` for those.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tab_id: Option<u64>,
    message: ClientMessage,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct ServerEnvelope {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    request_id: Option<u64>,
    /// Echoes back which tab the reply is about -- without this, a
    /// client watching a shared, multi-tab, broadcast connection
    /// (`blueice-launcher`'s broker) has no way to tell which tab a
    /// `FrameReady`/`Navigated`/... broadcast belongs to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tab_id: Option<u64>,
    message: ServerMessage,
}

pub fn write_client_message<W: Write>(w: &mut W, msg: &ClientMessage) -> io::Result<()> {
    write_client_message_with_id(w, None, msg)
}

pub fn write_client_message_with_id<W: Write>(
    w: &mut W,
    request_id: Option<u64>,
    msg: &ClientMessage,
) -> io::Result<()> {
    write_framed(
        w,
        &ClientEnvelope {
            request_id,
            tab_id: None,
            message: msg.clone(),
        },
    )
}

/// Like [`write_client_message_with_id`], additionally addressing the
/// message to `tab_id` (`None` for the default tab, or for a message
/// that isn't per-tab-scoped at all).
pub fn write_client_message_with_ids<W: Write>(
    w: &mut W,
    tab_id: Option<u64>,
    request_id: Option<u64>,
    msg: &ClientMessage,
) -> io::Result<()> {
    write_framed(
        w,
        &ClientEnvelope {
            request_id,
            tab_id,
            message: msg.clone(),
        },
    )
}

pub fn read_client_message<R: Read>(r: &mut R) -> io::Result<ClientMessage> {
    Ok(read_client_message_with_id(r)?.1)
}

pub fn read_client_message_with_id<R: Read>(r: &mut R) -> io::Result<(Option<u64>, ClientMessage)> {
    let envelope = read_client_envelope(r)?;
    Ok((envelope.request_id, envelope.message))
}

/// Like [`read_client_message_with_id`], additionally returning the
/// envelope's `tab_id` as `(tab_id, request_id, message)`.
pub fn read_client_message_with_ids<R: Read>(
    r: &mut R,
) -> io::Result<(Option<u64>, Option<u64>, ClientMessage)> {
    let envelope = read_client_envelope(r)?;
    Ok((envelope.tab_id, envelope.request_id, envelope.message))
}

pub fn write_server_message<W: Write>(w: &mut W, msg: &ServerMessage) -> io::Result<()> {
    write_server_message_with_id(w, None, msg)
}

pub fn write_server_message_with_id<W: Write>(
    w: &mut W,
    request_id: Option<u64>,
    msg: &ServerMessage,
) -> io::Result<()> {
    write_framed(
        w,
        &ServerEnvelope {
            request_id,
            tab_id: None,
            message: msg.clone(),
        },
    )
}

/// Like [`write_server_message_with_id`], additionally echoing back
/// which tab this reply is about.
pub fn write_server_message_with_ids<W: Write>(
    w: &mut W,
    tab_id: Option<u64>,
    request_id: Option<u64>,
    msg: &ServerMessage,
) -> io::Result<()> {
    write_framed(
        w,
        &ServerEnvelope {
            request_id,
            tab_id,
            message: msg.clone(),
        },
    )
}

pub fn read_server_message<R: Read>(r: &mut R) -> io::Result<ServerMessage> {
    Ok(read_server_message_with_id(r)?.1)
}

pub fn read_server_message_with_id<R: Read>(r: &mut R) -> io::Result<(Option<u64>, ServerMessage)> {
    let envelope = read_server_envelope(r)?;
    Ok((envelope.request_id, envelope.message))
}

/// Like [`read_server_message_with_id`], additionally returning the
/// envelope's `tab_id` as `(tab_id, request_id, message)`.
pub fn read_server_message_with_ids<R: Read>(
    r: &mut R,
) -> io::Result<(Option<u64>, Option<u64>, ServerMessage)> {
    let envelope = read_server_envelope(r)?;
    Ok((envelope.tab_id, envelope.request_id, envelope.message))
}

/// The client side of the `protocol_version` handshake (`phase-1-ai-
/// representation-layer/PLAN.md` §3): sends [`ClientMessage::Hello`]
/// declaring [`PROTOCOL_VERSION`], then blocks for `core`'s reply.
/// Every independent client -- `frontend`, `blueice-mcp-server`, and
/// `blueice-launcher`'s own connection to the `core` it spawns -- calls
/// this immediately after connecting and before sending anything else,
/// since `core` rejects a fresh connection whose first message isn't
/// `Hello`.
pub fn client_handshake<S: Read + Write>(stream: &mut S) -> io::Result<()> {
    write_client_message(
        stream,
        &ClientMessage::Hello {
            protocol_version: PROTOCOL_VERSION,
        },
    )?;
    match read_server_message(stream)? {
        ServerMessage::Hello { protocol_version } if protocol_version == PROTOCOL_VERSION => Ok(()),
        ServerMessage::Error { message } => Err(io::Error::other(message)),
        other => Err(io::Error::other(format!(
            "expected a Hello handshake reply, got {other:?}"
        ))),
    }
}

#[cfg(test)]
#[path = "browser_protocol_tests.rs"]
mod tests;
