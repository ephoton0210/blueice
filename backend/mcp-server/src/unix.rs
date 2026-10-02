// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `blueice-mcp-server`: a thin MCP adapter over `core`'s existing
//! IPC control-plane protocol, per `phase-12-mcp-server/PLAN.md`'s
//! "MCP should be an adapter, not a fourth protocol" design decision.
//! Built now, ahead of Phase 12 proper, because differential testing
//! against a real Chromium (via Puppeteer) needs a stable way for an
//! external agent to drive BlueIce early -- this is deliberately the
//! *foundation* Phase 12 will later broaden (downloads, transfer
//! protocols, `bluejs_run`/`bluejs_analyze`) and harden (on-demand
//! process spawning via Phase 8's launcher, the `protocol_version`
//! handshake Phase 1/5 deferred), not the finished feature.
//!
//! **Every tool here wraps a `blueice-ipc` `ClientMessage`/
//! `ServerMessage` round trip -- no browsing logic of its own**, per
//! Phase 12's adapter-not-parallel-channel principle.
//!
//! **The reply sequencing that avoids a read-timeout hack**: several
//! `ClientMessage`s produce a *variable* number of replies (a
//! coordinate/ID `Click` that doesn't land on a link produces none at
//! all -- see `blueice_engine::session`'s own module docs). Those
//! actions pipeline a `GetRepresentation` after their own message and
//! read until its one deterministic `Representation` reply. `Navigate`
//! is deliberately different: an HTTP(S) navigation completes in the
//! background, so it first waits for that request's `Navigated`,
//! `GatekeeperBlocked`, or `Error` reply (and its success frame) before
//! asking for a representation. Both paths avoid timers and never
//! leave a reply on the wire for the next call to misinterpret.

#[cfg(test)]
use crate::{wrap_untrusted_page_content, UNTRUSTED_CONTENT_MARKER};
use blueice_ipc::{
    AiSnapshot, ChromeCommand, ClientMessage, NodeAction, ServerMessage, TabGroupSummary,
    TabSummary,
};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The most recent `FrameReady` seen -- cached so [`CoreConnection::screenshot`]
/// can serve the latest frame without having to trigger a new one
/// (there is no `ClientMessage` that means "just resend the current
/// frame" -- every frame comes as the side effect of a state change).
#[derive(Debug, Clone, PartialEq)]
pub struct FrameInfo {
    pub shm_path: String,
    pub width: u32,
    pub height: u32,
    pub generation: u64,
}

impl FrameInfo {
    pub fn frame_source(&self) -> u64 {
        blueice_ipc::shm::frame_source_id_for_path(&self.shm_path)
    }
}

/// The common result shape for every state-changing tool: the
/// resulting page representation, plus an error message if `core`
/// reported one along the way (e.g. a failed `navigate`) -- `core`
/// still processes the pipelined `GetRepresentation` even after an
/// error, so `snapshot` is always populated (describing whatever page
/// is current, which is the *previous* page on a failed navigation).
#[derive(Debug, Clone, PartialEq)]
pub struct ToolOutcome {
    pub error: Option<String>,
    pub snapshot: AiSnapshot,
}

/// The result of a live-translation control: `core`'s `TranslationState` reply
/// plus the page representation that follows it (translated text is primary,
/// with each node's `original_name` attached). `error` is set when `core`
/// refused the request (translation unavailable, invalid tag, unknown tab).
#[derive(Debug, Clone, PartialEq)]
pub struct TranslationOutcome {
    pub error: Option<String>,
    /// The core-wide target language for pages fetched from now on.
    pub language: Option<String>,
    /// Whether the addressed page has translated text to toggle.
    pub available: bool,
    /// Whether the translation (not the original) is on screen.
    pub shown: bool,
    pub snapshot: AiSnapshot,
}

/// A finished assistant task: the model's text (derived from untrusted page
/// text, so callers frame it as such), or why it could not be completed.
#[derive(Debug, Clone, PartialEq)]
pub enum AssistantOutcome {
    Done {
        kind: blueice_ipc::AssistantTaskKind,
        text: String,
    },
    Failed(String),
}

/// [`CoreConnection::open_tab`]'s result: either the new tab, or an
/// error message if a requested navigation into it failed (`core`
/// doesn't report the new tab's id in that case -- see
/// `blueice_ipc::ServerMessage::TabOpened`'s own docs).
#[derive(Debug, Clone, PartialEq)]
pub enum OpenTabOutcome {
    Opened { tab_id: u64, url: Option<String> },
    Error(String),
}

/// [`CoreConnection::close_tab`]'s result.
#[derive(Debug, Clone, PartialEq)]
pub enum CloseTabOutcome {
    Closed,
    Error(String),
}

/// Result for the core-owned tab-group tools. Unlike selection, groups are
/// shared session state, so an MCP caller receives the same object a native
/// frontend will render in its tab strip.
#[derive(Debug, Clone, PartialEq)]
pub enum TabGroupOutcome {
    Group(TabGroupSummary),
    Assigned { tab_id: u64, group_id: Option<u64> },
    Closed { group_id: u64 },
    Error(String),
}

/// Wraps one `core` connection, generic over the stream so the actual
/// message-sequencing logic (the part worth testing) doesn't need a
/// real subprocess and Unix socket -- `UnixStream::pair()` plus a fake
/// responder thread is enough, the same strategy
/// `blueice_engine::session`'s own tests already use.
pub struct CoreConnection<S> {
    stream: S,
    /// Keyed by tab_id -- `phase-16-multi-tab-and-tab-groups/PLAN.md`
    /// means two tabs can each have their own most-recent frame, so a
    /// single cached value would silently return whichever tab was
    /// rendered *last*, regardless of which one a caller actually
    /// wants a screenshot of.
    last_frames: std::collections::HashMap<u64, FrameInfo>,
    /// The tab_id whose frame was most recently produced in response to
    /// this MCP connection's own request. Unsolicited broadcast frames
    /// remain available by explicit tab id but must never move this
    /// fallback to a human's tab.
    last_seen_tab: Option<u64>,
    next_request_id: u64,
}

impl<S: Read + Write> CoreConnection<S> {
    pub fn new(stream: S) -> Self {
        CoreConnection {
            stream,
            last_frames: std::collections::HashMap::new(),
            last_seen_tab: None,
            next_request_id: 0,
        }
    }

    /// Records a `FrameReady` reply for `tab_id` (a no-op if the reply
    /// didn't carry one). `updates_default` is true only for a frame
    /// correlated with this connection's current request; broadcast
    /// refreshes are cached for explicit lookup but cannot redirect an
    /// unqualified screenshot to another user's tab.
    fn record_frame(&mut self, tab_id: Option<u64>, frame: FrameInfo, updates_default: bool) {
        if let Some(tab_id) = tab_id {
            self.last_frames.insert(tab_id, frame);
            if updates_default {
                self.last_seen_tab = Some(tab_id);
            }
        }
    }

    /// Performs the `protocol_version` handshake (`phase-1-ai-
    /// representation-layer/PLAN.md` §3) `core` requires as the very
    /// first message on a fresh connection -- callers that construct a
    /// `CoreConnection` directly over a real `core`/`blueice-launcher`
    /// connection (`CoreProcess::spawn`/`connect_to`) must call this
    /// immediately, before any tool method. Split out from `new`
    /// itself (which stays infallible, doing no I/O) so the many
    /// existing tests exercising the message-sequencing methods below
    /// against a fake responder don't all need a scripted `Hello` reply
    /// they have no reason to care about.
    pub fn handshake(&mut self) -> io::Result<()> {
        blueice_ipc::client_handshake(&mut self.stream)
    }

    fn next_request_id(&mut self) -> u64 {
        self.next_request_id += 1;
        self.next_request_id
    }

    /// Sends an action that has no asynchronous completion signal,
    /// addressed to `tab_id` (`None` for the default tab), then pipelines
    /// a `GetRepresentation` addressed to the same tab and drains until
    /// it arrives. This is for `ActOn` and `Highlight`; [`Self::navigate`]
    /// cannot use it because its HTTP(S) completion may arrive after an
    /// immediately-pipelined representation of the old page. Each outgoing
    /// message gets its own request_id, and replies for other ids are
    /// skipped rather than consumed, as required on the launcher's shared
    /// broadcast connection.
    fn send_and_drain(
        &mut self,
        tab_id: Option<u64>,
        msg: &ClientMessage,
    ) -> io::Result<ToolOutcome> {
        let action_id = self.next_request_id();
        let representation_id = self.next_request_id();
        blueice_ipc::write_client_message_with_ids(&mut self.stream, tab_id, Some(action_id), msg)?;
        blueice_ipc::write_client_message_with_ids(
            &mut self.stream,
            tab_id,
            Some(representation_id),
            &ClientMessage::GetRepresentation,
        )?;
        let mut error = None;
        loop {
            let (frame_tab_id, request_id, message) =
                blueice_ipc::read_server_message_with_ids(&mut self.stream)?;
            if request_id != Some(action_id) && request_id != Some(representation_id) {
                continue;
            }
            match message {
                ServerMessage::Representation(snapshot) => return Ok(ToolOutcome { error, snapshot }),
                ServerMessage::Error { message } => error = Some(message),
                // `phase-7-local-ai/PLAN.md`'s gatekeeper blocked this
                // action's navigation -- surfaced through the same
                // `ToolOutcome::error` field a plain `Error` already
                // uses, since from this MCP-facing call's perspective
                // both are "the requested action didn't happen, here's
                // why." Building a dedicated gatekeeper-aware MCP tool
                // result is Phase 12's own future adapter work, not
                // this slice's.
                ServerMessage::GatekeeperBlocked { reason, category, url } => error = Some(format!("blocked by the gatekeeper ({category}) for {url}: {reason}")),
                ServerMessage::FrameReady { shm_path, width, height, generation } => {
                    self.record_frame(frame_tab_id, FrameInfo { shm_path, width, height, generation }, true);
                }
                ServerMessage::Navigated { .. }
                | ServerMessage::Dom(_)
                | ServerMessage::BlueTsScriptReports(_)
                | ServerMessage::BlueJsScriptReports(_)
                | ServerMessage::Hello { .. }
                | ServerMessage::TextInputState(_)
                | ServerMessage::FindState(_)
                    | ServerMessage::WindowState(_)
                | ServerMessage::ViewportState(_)
                    | ServerMessage::DisplayPreferencesState(_)
                    | ServerMessage::ContextMenu(_)
                    | ServerMessage::ContextMenuLink { .. }
                | ServerMessage::NavigationStarted { .. }
                | ServerMessage::FormResubmissionResolved { .. }
                | ServerMessage::FormResubmission { .. }
                | ServerMessage::Unknown
                // `mcp-server` doesn't call `OpenTab`/`CloseTab`/`ListTabs`
                // itself in this slice, so these can only arrive here
                // as another client's broadcasted traffic (the same
                // reasoning as `Navigated`/`Dom` above) -- ignored for
                // the same reason.
                | ServerMessage::TabOpened { .. }
                | ServerMessage::TabClosed { .. }
                | ServerMessage::Tabs(_)
                | ServerMessage::TabGroupCreated(_)
                | ServerMessage::TabGroupUpdated(_)
                | ServerMessage::TabGroupAssigned { .. }
                | ServerMessage::TabGroupClosed { .. }
                | ServerMessage::TabGroups(_)
                | ServerMessage::HistoryState { .. }
                | ServerMessage::TranslationState { .. }
                | ServerMessage::AssistantResult { .. }
                | ServerMessage::ExtensionToolbar { .. }
                | ServerMessage::ExtensionPopup { .. } => {}
            }
        }
    }

    fn send_navigation_and_wait(
        &mut self,
        tab_id: Option<u64>,
        message: ClientMessage,
    ) -> io::Result<ToolOutcome> {
        let action_id = self.next_request_id();
        blueice_ipc::write_client_message_with_ids(
            &mut self.stream,
            tab_id,
            Some(action_id),
            &message,
        )?;

        // `session.rs` writes Navigated followed by FrameReady for a
        // successful navigation. Wait for both before requesting the
        // representation: doing it sooner can race the background
        // gatekeeper/fetch and describe the preceding page instead.
        let mut error = None;
        let mut navigated = false;
        loop {
            let (frame_tab_id, request_id, message) =
                blueice_ipc::read_server_message_with_ids(&mut self.stream)?;
            if request_id != Some(action_id) {
                continue;
            }
            match message {
                ServerMessage::FormResubmission { .. } => {
                    error = Some("Form resubmission requires confirmation in the browser".into());
                    break;
                }
                ServerMessage::Navigated { .. } => navigated = true,
                ServerMessage::Error { message } => {
                    error = Some(message);
                    break;
                }
                ServerMessage::GatekeeperBlocked {
                    reason,
                    category,
                    url,
                } => {
                    error = Some(format!(
                        "blocked by the gatekeeper ({category}) for {url}: {reason}"
                    ));
                    break;
                }
                ServerMessage::FrameReady {
                    shm_path,
                    width,
                    height,
                    generation,
                } => {
                    self.record_frame(
                        frame_tab_id,
                        FrameInfo {
                            shm_path,
                            width,
                            height,
                            generation,
                        },
                        true,
                    );
                    if navigated {
                        break;
                    }
                }
                ServerMessage::Representation(_)
                | ServerMessage::Dom(_)
                | ServerMessage::BlueTsScriptReports(_)
                | ServerMessage::BlueJsScriptReports(_)
                | ServerMessage::Hello { .. }
                | ServerMessage::TextInputState(_)
                | ServerMessage::FindState(_)
                | ServerMessage::WindowState(_)
                | ServerMessage::ViewportState(_)
                | ServerMessage::DisplayPreferencesState(_)
                | ServerMessage::ContextMenu(_)
                | ServerMessage::ContextMenuLink { .. }
                | ServerMessage::NavigationStarted { .. }
                | ServerMessage::FormResubmissionResolved { .. }
                | ServerMessage::Unknown
                | ServerMessage::TabOpened { .. }
                | ServerMessage::TabClosed { .. }
                | ServerMessage::Tabs(_)
                | ServerMessage::TabGroupCreated(_)
                | ServerMessage::TabGroupUpdated(_)
                | ServerMessage::TabGroupAssigned { .. }
                | ServerMessage::TabGroupClosed { .. }
                | ServerMessage::TabGroups(_)
                | ServerMessage::HistoryState { .. }
                | ServerMessage::TranslationState { .. }
                | ServerMessage::AssistantResult { .. }
                | ServerMessage::ExtensionToolbar { .. }
                | ServerMessage::ExtensionPopup { .. } => {}
            }
        }

        let snapshot = self.representation(tab_id)?;
        Ok(ToolOutcome { error, snapshot })
    }

    pub fn navigate(&mut self, url: &str, tab_id: Option<u64>) -> io::Result<ToolOutcome> {
        self.send_navigation_and_wait(
            tab_id,
            ClientMessage::Navigate {
                url: url.to_string(),
            },
        )
    }

    /// Restore the previous session-history entry for `tab_id` and return the
    /// restored accessibility snapshot. The core, rather than this adapter,
    /// owns the history stack, so this cannot accidentally move a different
    /// observer's tab.
    pub fn go_back(&mut self, tab_id: Option<u64>) -> io::Result<ToolOutcome> {
        self.send_navigation_and_wait(tab_id, ClientMessage::GoBack)
    }

    /// Restore the next session-history entry for `tab_id`.
    pub fn go_forward(&mut self, tab_id: Option<u64>) -> io::Result<ToolOutcome> {
        self.send_navigation_and_wait(tab_id, ClientMessage::GoForward)
    }

    /// Chooses the language pages fetched from now on are translated into
    /// (`None` turns translation off). `core` fixes which assistant serves it
    /// at startup, so this cannot redirect page text anywhere.
    pub fn set_translation_language(
        &mut self,
        target_language: Option<String>,
        tab_id: Option<u64>,
    ) -> io::Result<TranslationOutcome> {
        self.translation_action(
            tab_id,
            &ClientMessage::SetTranslationLanguage { target_language },
        )
    }

    /// Shows the addressed tab's translation or the page's original text.
    pub fn show_translation(
        &mut self,
        shown: bool,
        tab_id: Option<u64>,
    ) -> io::Result<TranslationOutcome> {
        self.translation_action(tab_id, &ClientMessage::ShowTranslation { shown })
    }

    /// Asks the local assistant to summarize `tab_id`'s shown text and waits for
    /// the result. `core` answers other clients meanwhile; this call waits only
    /// for its own request-correlated reply, which arrives when the assistant
    /// finishes (or fails, or times out on `core`'s own deadline).
    pub fn summarize_page(&mut self, tab_id: Option<u64>) -> io::Result<AssistantOutcome> {
        self.assistant_task(tab_id, &ClientMessage::SummarizePage)
    }

    /// Asks the local assistant to reorganize `tab_id`'s shown text per
    /// `instruction` and waits for the result.
    pub fn organize_page(
        &mut self,
        instruction: String,
        tab_id: Option<u64>,
    ) -> io::Result<AssistantOutcome> {
        self.assistant_task(tab_id, &ClientMessage::OrganizePage { instruction })
    }

    fn assistant_task(
        &mut self,
        tab_id: Option<u64>,
        msg: &ClientMessage,
    ) -> io::Result<AssistantOutcome> {
        let request_id = self.next_request_id();
        blueice_ipc::write_client_message_with_ids(
            &mut self.stream,
            tab_id,
            Some(request_id),
            msg,
        )?;
        loop {
            let (_, reply_id, message) =
                blueice_ipc::read_server_message_with_ids(&mut self.stream)?;
            // Skip every other reply, as the launcher's shared connection
            // broadcasts other clients' traffic here too.
            if reply_id != Some(request_id) {
                continue;
            }
            match message {
                ServerMessage::AssistantResult { kind, text } => {
                    return Ok(AssistantOutcome::Done { kind, text })
                }
                ServerMessage::Error { message } => return Ok(AssistantOutcome::Failed(message)),
                // Nothing else answers this request.
                _ => {}
            }
        }
    }

    /// Sends a translation control, pipelines a `GetRepresentation` to the same
    /// tab, and drains until it arrives, keeping the `TranslationState` reply.
    /// Only replies to these two requests are consumed, as on the launcher's
    /// shared broadcast connection every other reply must be skipped.
    fn translation_action(
        &mut self,
        tab_id: Option<u64>,
        msg: &ClientMessage,
    ) -> io::Result<TranslationOutcome> {
        let action_id = self.next_request_id();
        let representation_id = self.next_request_id();
        blueice_ipc::write_client_message_with_ids(&mut self.stream, tab_id, Some(action_id), msg)?;
        blueice_ipc::write_client_message_with_ids(
            &mut self.stream,
            tab_id,
            Some(representation_id),
            &ClientMessage::GetRepresentation,
        )?;
        let mut error = None;
        let mut state = (None, false, false);
        loop {
            let (frame_tab_id, request_id, message) =
                blueice_ipc::read_server_message_with_ids(&mut self.stream)?;
            if request_id != Some(action_id) && request_id != Some(representation_id) {
                continue;
            }
            match message {
                ServerMessage::Representation(snapshot) => {
                    return Ok(TranslationOutcome {
                        error,
                        language: state.0,
                        available: state.1,
                        shown: state.2,
                        snapshot,
                    })
                }
                ServerMessage::TranslationState {
                    language,
                    available,
                    shown,
                } => state = (language, available, shown),
                ServerMessage::Error { message } => error = Some(message),
                ServerMessage::FrameReady {
                    shm_path,
                    width,
                    height,
                    generation,
                } => self.record_frame(
                    frame_tab_id,
                    FrameInfo {
                        shm_path,
                        width,
                        height,
                        generation,
                    },
                    true,
                ),
                // Nothing else can answer these two requests.
                _ => {}
            }
        }
    }

    pub fn act(
        &mut self,
        id: u64,
        action: NodeAction,
        tab_id: Option<u64>,
    ) -> io::Result<ToolOutcome> {
        // A linked click begins an asynchronous navigation.  Unlike focus
        // and other immediate actions, it cannot pipeline a representation:
        // that representation could describe the preceding page. Identify a
        // current link first, then use the same completion barrier as
        // `navigate`. An absent/stale/non-link ID keeps the old no-reply
        // behavior and uses the ordinary pipelined representation instead.
        if matches!(&action, NodeAction::Click)
            && self
                .representation(tab_id)?
                .nodes
                .iter()
                .any(|node| node.id == id && node.role == blueice_ipc::Role::Link)
        {
            return self.send_navigation_and_wait(tab_id, ClientMessage::ActOn { id, action });
        }
        self.send_and_drain(tab_id, &ClientMessage::ActOn { id, action })
    }

    pub fn highlight(&mut self, id: Option<u64>, tab_id: Option<u64>) -> io::Result<ToolOutcome> {
        self.send_and_drain(tab_id, &ClientMessage::Highlight { id })
    }

    /// Unlike the action-shaped methods above, this sends nothing but
    /// `GetRepresentation` itself -- there's no prior state change to
    /// pipeline it after.
    pub fn representation(&mut self, tab_id: Option<u64>) -> io::Result<AiSnapshot> {
        let request_id = self.next_request_id();
        blueice_ipc::write_client_message_with_ids(
            &mut self.stream,
            tab_id,
            Some(request_id),
            &ClientMessage::GetRepresentation,
        )?;
        loop {
            let (frame_tab_id, reply_id, message) =
                blueice_ipc::read_server_message_with_ids(&mut self.stream)?;
            if reply_id != Some(request_id) {
                continue;
            }
            match message {
                ServerMessage::Representation(snapshot) => return Ok(snapshot),
                ServerMessage::FrameReady {
                    shm_path,
                    width,
                    height,
                    generation,
                } => {
                    self.record_frame(
                        frame_tab_id,
                        FrameInfo {
                            shm_path,
                            width,
                            height,
                            generation,
                        },
                        true,
                    );
                }
                ServerMessage::Error { .. }
                | ServerMessage::GatekeeperBlocked { .. }
                | ServerMessage::Navigated { .. }
                | ServerMessage::Dom(_)
                | ServerMessage::BlueTsScriptReports(_)
                | ServerMessage::BlueJsScriptReports(_)
                | ServerMessage::Hello { .. }
                | ServerMessage::TextInputState(_)
                | ServerMessage::FindState(_)
                | ServerMessage::WindowState(_)
                | ServerMessage::ViewportState(_)
                | ServerMessage::DisplayPreferencesState(_)
                | ServerMessage::ContextMenu(_)
                | ServerMessage::ContextMenuLink { .. }
                | ServerMessage::NavigationStarted { .. }
                | ServerMessage::FormResubmissionResolved { .. }
                | ServerMessage::FormResubmission { .. }
                | ServerMessage::Unknown
                | ServerMessage::TabOpened { .. }
                | ServerMessage::TabClosed { .. }
                | ServerMessage::Tabs(_)
                | ServerMessage::TabGroupCreated(_)
                | ServerMessage::TabGroupUpdated(_)
                | ServerMessage::TabGroupAssigned { .. }
                | ServerMessage::TabGroupClosed { .. }
                | ServerMessage::TabGroups(_)
                | ServerMessage::HistoryState { .. }
                | ServerMessage::TranslationState { .. }
                | ServerMessage::AssistantResult { .. }
                | ServerMessage::ExtensionToolbar { .. }
                | ServerMessage::ExtensionPopup { .. } => {}
            }
        }
    }

    /// The full DOM tree (`blueice_dom::dump`'s canonical text format),
    /// unfiltered by the AI-representation's semantic-role/`display:
    /// none` exclusion -- what the Chromium differential-testing
    /// harness (`TEST_PLAN.md`) diffs a serialized Chromium DOM
    /// against. Same "nothing to pipeline it after" shape as
    /// [`CoreConnection::representation`].
    pub fn dom(&mut self, tab_id: Option<u64>) -> io::Result<String> {
        let request_id = self.next_request_id();
        blueice_ipc::write_client_message_with_ids(
            &mut self.stream,
            tab_id,
            Some(request_id),
            &ClientMessage::GetDom,
        )?;
        loop {
            let (frame_tab_id, reply_id, message) =
                blueice_ipc::read_server_message_with_ids(&mut self.stream)?;
            if reply_id != Some(request_id) {
                continue;
            }
            match message {
                ServerMessage::Dom(dump) => return Ok(dump),
                ServerMessage::FrameReady {
                    shm_path,
                    width,
                    height,
                    generation,
                } => {
                    self.record_frame(
                        frame_tab_id,
                        FrameInfo {
                            shm_path,
                            width,
                            height,
                            generation,
                        },
                        true,
                    );
                }
                ServerMessage::Error { .. }
                | ServerMessage::GatekeeperBlocked { .. }
                | ServerMessage::Navigated { .. }
                | ServerMessage::Representation(_)
                | ServerMessage::BlueTsScriptReports(_)
                | ServerMessage::BlueJsScriptReports(_)
                | ServerMessage::Hello { .. }
                | ServerMessage::TextInputState(_)
                | ServerMessage::FindState(_)
                | ServerMessage::WindowState(_)
                | ServerMessage::ViewportState(_)
                | ServerMessage::DisplayPreferencesState(_)
                | ServerMessage::ContextMenu(_)
                | ServerMessage::ContextMenuLink { .. }
                | ServerMessage::NavigationStarted { .. }
                | ServerMessage::FormResubmissionResolved { .. }
                | ServerMessage::FormResubmission { .. }
                | ServerMessage::Unknown
                | ServerMessage::TabOpened { .. }
                | ServerMessage::TabClosed { .. }
                | ServerMessage::Tabs(_)
                | ServerMessage::TabGroupCreated(_)
                | ServerMessage::TabGroupUpdated(_)
                | ServerMessage::TabGroupAssigned { .. }
                | ServerMessage::TabGroupClosed { .. }
                | ServerMessage::TabGroups(_)
                | ServerMessage::HistoryState { .. }
                | ServerMessage::TranslationState { .. }
                | ServerMessage::AssistantResult { .. }
                | ServerMessage::ExtensionToolbar { .. }
                | ServerMessage::ExtensionPopup { .. } => {}
            }
        }
    }

    /// Opens a new tab (blank, or navigated to `url` if given) --
    /// `phase-16-multi-tab-and-tab-groups/PLAN.md`'s `OpenTab`. No
    /// "current tab" is tracked here or in `core` itself (see that
    /// phase's own docs for why -- a human's `frontend` and an AI's
    /// `mcp-server` may legitimately be working with different tabs at
    /// once); the caller is expected to hold onto the returned
    /// `tab_id` and pass it explicitly to address that tab afterward.
    pub fn open_tab(&mut self, url: Option<&str>) -> io::Result<OpenTabOutcome> {
        let request_id = self.next_request_id();
        blueice_ipc::write_client_message_with_id(
            &mut self.stream,
            Some(request_id),
            &ClientMessage::OpenTab {
                url: url.map(str::to_string),
            },
        )?;
        // `session.rs`'s `handle_open_tab` sends exactly one more
        // message (a `FrameReady`) after `TabOpened` when -- and only
        // when -- `TabOpened.url` came back `Some(_)` (a URL was given
        // and navigation succeeded): a blank tab or a failed navigation
        // never gets one. `outcome` is only returned once that
        // guaranteed follow-up (if any) has actually been drained, so
        // this call never leaves an unread `FrameReady` on the wire for
        // the next call to misinterpret -- the same "pipeline, then
        // read until definitively done" discipline `send_and_drain`
        // already uses, just derived from `TabOpened.url` instead of a
        // second message this call sent itself.
        let mut outcome = None;
        loop {
            let (frame_tab_id, reply_id, message) =
                blueice_ipc::read_server_message_with_ids(&mut self.stream)?;
            if reply_id != Some(request_id) {
                continue;
            }
            match message {
                ServerMessage::TabOpened { tab_id, url, .. } => {
                    let expects_frame = url.is_some();
                    outcome = Some(OpenTabOutcome::Opened { tab_id, url });
                    if !expects_frame {
                        return Ok(outcome.expect("just set"));
                    }
                }
                // A failed `Navigate` inside `OpenTab` replies bare
                // `Error` -- the new (blank) tab's id isn't reported
                // here, a documented limitation of this minimal slice
                // (see `ServerMessage::TabOpened`'s own docs); a caller
                // that needs it can fall back to `list_tabs`.
                ServerMessage::Error { message } => return Ok(OpenTabOutcome::Error(message)),
                // Same shape as the plain-`Error` case above, but for a
                // navigation `phase-7-local-ai/PLAN.md`'s gatekeeper
                // blocked rather than one that merely failed to fetch --
                // `OpenTabOutcome` has no dedicated variant for this
                // distinction yet, so it's reported through the same
                // `Error(String)` case for now.
                ServerMessage::GatekeeperBlocked {
                    reason,
                    category,
                    url,
                } => {
                    return Ok(OpenTabOutcome::Error(format!(
                        "blocked by the gatekeeper ({category}) for {url}: {reason}"
                    )));
                }
                ServerMessage::FrameReady {
                    shm_path,
                    width,
                    height,
                    generation,
                } => {
                    self.record_frame(
                        frame_tab_id,
                        FrameInfo {
                            shm_path,
                            width,
                            height,
                            generation,
                        },
                        true,
                    );
                    if let Some(outcome) = outcome {
                        return Ok(outcome);
                    }
                    // A `FrameReady` arriving before its `TabOpened` would
                    // be unexpected given `session.rs`'s send order, but
                    // there's nothing unsafe about just continuing to
                    // wait for it rather than assuming protocol violation.
                }
                ServerMessage::Navigated { .. }
                | ServerMessage::Dom(_)
                | ServerMessage::BlueTsScriptReports(_)
                | ServerMessage::BlueJsScriptReports(_)
                | ServerMessage::Representation(_)
                | ServerMessage::Hello { .. }
                | ServerMessage::TextInputState(_)
                | ServerMessage::FindState(_)
                | ServerMessage::WindowState(_)
                | ServerMessage::ViewportState(_)
                | ServerMessage::DisplayPreferencesState(_)
                | ServerMessage::ContextMenu(_)
                | ServerMessage::ContextMenuLink { .. }
                | ServerMessage::NavigationStarted { .. }
                | ServerMessage::FormResubmissionResolved { .. }
                | ServerMessage::FormResubmission { .. }
                | ServerMessage::Unknown
                | ServerMessage::TabClosed { .. }
                | ServerMessage::Tabs(_)
                | ServerMessage::TabGroupCreated(_)
                | ServerMessage::TabGroupUpdated(_)
                | ServerMessage::TabGroupAssigned { .. }
                | ServerMessage::TabGroupClosed { .. }
                | ServerMessage::TabGroups(_)
                | ServerMessage::HistoryState { .. }
                | ServerMessage::TranslationState { .. }
                | ServerMessage::AssistantResult { .. }
                | ServerMessage::ExtensionToolbar { .. }
                | ServerMessage::ExtensionPopup { .. } => {}
            }
        }
    }

    /// Closes `tab_id` -- `phase-16-multi-tab-and-tab-groups/PLAN.md`'s
    /// `CloseTab`, addressed via the envelope the same way every other
    /// per-tab message is.
    pub fn close_tab(&mut self, tab_id: u64) -> io::Result<CloseTabOutcome> {
        let request_id = self.next_request_id();
        blueice_ipc::write_client_message_with_ids(
            &mut self.stream,
            Some(tab_id),
            Some(request_id),
            &ClientMessage::CloseTab,
        )?;
        loop {
            let (frame_tab_id, reply_id, message) =
                blueice_ipc::read_server_message_with_ids(&mut self.stream)?;
            if reply_id != Some(request_id) {
                continue;
            }
            match message {
                ServerMessage::TabClosed { .. } => return Ok(CloseTabOutcome::Closed),
                ServerMessage::Error { message } => return Ok(CloseTabOutcome::Error(message)),
                ServerMessage::FrameReady {
                    shm_path,
                    width,
                    height,
                    generation,
                } => {
                    self.record_frame(
                        frame_tab_id,
                        FrameInfo {
                            shm_path,
                            width,
                            height,
                            generation,
                        },
                        true,
                    );
                }
                ServerMessage::GatekeeperBlocked { .. }
                | ServerMessage::Navigated { .. }
                | ServerMessage::Dom(_)
                | ServerMessage::BlueTsScriptReports(_)
                | ServerMessage::BlueJsScriptReports(_)
                | ServerMessage::Representation(_)
                | ServerMessage::Hello { .. }
                | ServerMessage::TextInputState(_)
                | ServerMessage::FindState(_)
                | ServerMessage::WindowState(_)
                | ServerMessage::ViewportState(_)
                | ServerMessage::DisplayPreferencesState(_)
                | ServerMessage::ContextMenu(_)
                | ServerMessage::ContextMenuLink { .. }
                | ServerMessage::NavigationStarted { .. }
                | ServerMessage::FormResubmissionResolved { .. }
                | ServerMessage::FormResubmission { .. }
                | ServerMessage::Unknown
                | ServerMessage::TabOpened { .. }
                | ServerMessage::Tabs(_)
                | ServerMessage::TabGroupCreated(_)
                | ServerMessage::TabGroupUpdated(_)
                | ServerMessage::TabGroupAssigned { .. }
                | ServerMessage::TabGroupClosed { .. }
                | ServerMessage::HistoryState { .. }
                | ServerMessage::TranslationState { .. }
                | ServerMessage::AssistantResult { .. }
                | ServerMessage::ExtensionToolbar { .. }
                | ServerMessage::ExtensionPopup { .. }
                | ServerMessage::TabGroups(_) => {}
            }
        }
    }

    /// Lists every currently-open tab -- `phase-16-multi-tab-and-tab-
    /// groups/PLAN.md`'s `ListTabs`. Always succeeds (an empty list is
    /// valid -- `core` doesn't force a tab to always exist).
    pub fn list_tabs(&mut self) -> io::Result<Vec<TabSummary>> {
        let request_id = self.next_request_id();
        blueice_ipc::write_client_message_with_id(
            &mut self.stream,
            Some(request_id),
            &ClientMessage::ListTabs,
        )?;
        loop {
            let (frame_tab_id, reply_id, message) =
                blueice_ipc::read_server_message_with_ids(&mut self.stream)?;
            if reply_id != Some(request_id) {
                continue;
            }
            match message {
                ServerMessage::Tabs(tabs) => return Ok(tabs),
                ServerMessage::FrameReady {
                    shm_path,
                    width,
                    height,
                    generation,
                } => {
                    self.record_frame(
                        frame_tab_id,
                        FrameInfo {
                            shm_path,
                            width,
                            height,
                            generation,
                        },
                        true,
                    );
                }
                ServerMessage::Error { .. }
                | ServerMessage::GatekeeperBlocked { .. }
                | ServerMessage::Navigated { .. }
                | ServerMessage::Dom(_)
                | ServerMessage::BlueTsScriptReports(_)
                | ServerMessage::BlueJsScriptReports(_)
                | ServerMessage::Representation(_)
                | ServerMessage::Hello { .. }
                | ServerMessage::TextInputState(_)
                | ServerMessage::FindState(_)
                | ServerMessage::WindowState(_)
                | ServerMessage::ViewportState(_)
                | ServerMessage::DisplayPreferencesState(_)
                | ServerMessage::ContextMenu(_)
                | ServerMessage::ContextMenuLink { .. }
                | ServerMessage::NavigationStarted { .. }
                | ServerMessage::FormResubmissionResolved { .. }
                | ServerMessage::FormResubmission { .. }
                | ServerMessage::Unknown
                | ServerMessage::TabOpened { .. }
                | ServerMessage::TabClosed { .. }
                | ServerMessage::TabGroupCreated(_)
                | ServerMessage::TabGroupUpdated(_)
                | ServerMessage::TabGroupAssigned { .. }
                | ServerMessage::TabGroupClosed { .. }
                | ServerMessage::TabGroups(_)
                | ServerMessage::HistoryState { .. }
                | ServerMessage::TranslationState { .. }
                | ServerMessage::AssistantResult { .. }
                | ServerMessage::ExtensionToolbar { .. }
                | ServerMessage::ExtensionPopup { .. } => {}
            }
        }
    }

    /// Sends a group-management message and waits only for its correlated
    /// response. It deliberately has no implicit tab selection: only
    /// `SetTabGroup` passes an explicit `tab_id`; every other group operation
    /// is scoped directly by its group id.
    fn group_request(
        &mut self,
        tab_id: Option<u64>,
        message: ClientMessage,
    ) -> io::Result<TabGroupOutcome> {
        let request_id = self.next_request_id();
        blueice_ipc::write_client_message_with_ids(
            &mut self.stream,
            tab_id,
            Some(request_id),
            &message,
        )?;
        loop {
            let (frame_tab_id, reply_id, message) =
                blueice_ipc::read_server_message_with_ids(&mut self.stream)?;
            if reply_id != Some(request_id) {
                continue;
            }
            match message {
                ServerMessage::TabGroupCreated(group) | ServerMessage::TabGroupUpdated(group) => {
                    return Ok(TabGroupOutcome::Group(group));
                }
                ServerMessage::TabGroupAssigned { tab_id, group_id } => {
                    return Ok(TabGroupOutcome::Assigned { tab_id, group_id });
                }
                ServerMessage::TabGroupClosed { group_id } => {
                    return Ok(TabGroupOutcome::Closed { group_id });
                }
                ServerMessage::Error { message } => return Ok(TabGroupOutcome::Error(message)),
                ServerMessage::FrameReady {
                    shm_path,
                    width,
                    height,
                    generation,
                } => self.record_frame(
                    frame_tab_id,
                    FrameInfo {
                        shm_path,
                        width,
                        height,
                        generation,
                    },
                    true,
                ),
                ServerMessage::GatekeeperBlocked { .. }
                | ServerMessage::Navigated { .. }
                | ServerMessage::Dom(_)
                | ServerMessage::BlueTsScriptReports(_)
                | ServerMessage::BlueJsScriptReports(_)
                | ServerMessage::Representation(_)
                | ServerMessage::Hello { .. }
                | ServerMessage::TextInputState(_)
                | ServerMessage::FindState(_)
                | ServerMessage::WindowState(_)
                | ServerMessage::ViewportState(_)
                | ServerMessage::DisplayPreferencesState(_)
                | ServerMessage::ContextMenu(_)
                | ServerMessage::ContextMenuLink { .. }
                | ServerMessage::NavigationStarted { .. }
                | ServerMessage::FormResubmissionResolved { .. }
                | ServerMessage::FormResubmission { .. }
                | ServerMessage::Unknown
                | ServerMessage::TabOpened { .. }
                | ServerMessage::TabClosed { .. }
                | ServerMessage::Tabs(_)
                | ServerMessage::TabGroups(_)
                | ServerMessage::HistoryState { .. }
                | ServerMessage::TranslationState { .. }
                | ServerMessage::AssistantResult { .. }
                | ServerMessage::ExtensionToolbar { .. }
                | ServerMessage::ExtensionPopup { .. } => {}
            }
        }
    }

    pub fn create_tab_group(&mut self, name: &str, color: &str) -> io::Result<TabGroupOutcome> {
        self.group_request(
            None,
            ClientMessage::CreateTabGroup {
                name: name.to_string(),
                color: color.to_string(),
            },
        )
    }

    pub fn set_tab_group(
        &mut self,
        tab_id: u64,
        group_id: Option<u64>,
    ) -> io::Result<TabGroupOutcome> {
        self.group_request(Some(tab_id), ClientMessage::SetTabGroup { group_id })
    }

    pub fn rename_tab_group(&mut self, group_id: u64, name: &str) -> io::Result<TabGroupOutcome> {
        self.group_request(
            None,
            ClientMessage::RenameTabGroup {
                group_id,
                name: name.to_string(),
            },
        )
    }

    pub fn set_tab_group_color(
        &mut self,
        group_id: u64,
        color: &str,
    ) -> io::Result<TabGroupOutcome> {
        self.group_request(
            None,
            ClientMessage::SetTabGroupColor {
                group_id,
                color: color.to_string(),
            },
        )
    }

    pub fn set_tab_group_collapsed(
        &mut self,
        group_id: u64,
        collapsed: bool,
    ) -> io::Result<TabGroupOutcome> {
        self.group_request(
            None,
            ClientMessage::SetTabGroupCollapsed {
                group_id,
                collapsed,
            },
        )
    }

    pub fn close_tab_group(&mut self, group_id: u64) -> io::Result<TabGroupOutcome> {
        self.group_request(None, ClientMessage::CloseTabGroup { group_id })
    }

    pub fn list_tab_groups(&mut self) -> io::Result<Result<Vec<TabGroupSummary>, String>> {
        let request_id = self.next_request_id();
        blueice_ipc::write_client_message_with_id(
            &mut self.stream,
            Some(request_id),
            &ClientMessage::ListTabGroups,
        )?;
        loop {
            let (frame_tab_id, reply_id, message) =
                blueice_ipc::read_server_message_with_ids(&mut self.stream)?;
            if reply_id != Some(request_id) {
                continue;
            }
            match message {
                ServerMessage::TabGroups(groups) => return Ok(Ok(groups)),
                ServerMessage::Error { message } => return Ok(Err(message)),
                ServerMessage::FrameReady {
                    shm_path,
                    width,
                    height,
                    generation,
                } => self.record_frame(
                    frame_tab_id,
                    FrameInfo {
                        shm_path,
                        width,
                        height,
                        generation,
                    },
                    true,
                ),
                ServerMessage::GatekeeperBlocked { .. }
                | ServerMessage::Navigated { .. }
                | ServerMessage::Dom(_)
                | ServerMessage::BlueTsScriptReports(_)
                | ServerMessage::BlueJsScriptReports(_)
                | ServerMessage::Representation(_)
                | ServerMessage::Hello { .. }
                | ServerMessage::TextInputState(_)
                | ServerMessage::FindState(_)
                | ServerMessage::WindowState(_)
                | ServerMessage::ViewportState(_)
                | ServerMessage::DisplayPreferencesState(_)
                | ServerMessage::ContextMenu(_)
                | ServerMessage::ContextMenuLink { .. }
                | ServerMessage::NavigationStarted { .. }
                | ServerMessage::FormResubmissionResolved { .. }
                | ServerMessage::FormResubmission { .. }
                | ServerMessage::Unknown
                | ServerMessage::TabOpened { .. }
                | ServerMessage::TabClosed { .. }
                | ServerMessage::Tabs(_)
                | ServerMessage::TabGroupCreated(_)
                | ServerMessage::TabGroupUpdated(_)
                | ServerMessage::TabGroupAssigned { .. }
                | ServerMessage::TabGroupClosed { .. }
                | ServerMessage::HistoryState { .. }
                | ServerMessage::TranslationState { .. }
                | ServerMessage::AssistantResult { .. }
                | ServerMessage::ExtensionToolbar { .. }
                | ServerMessage::ExtensionPopup { .. } => {}
            }
        }
    }

    /// The most recent frame for `tab_id`, or (if `None`) the frame for
    /// the tab this MCP connection most recently rendered through one
    /// of its own requests. An unsolicited frame for a human's tab is
    /// available only by naming that tab and cannot redirect the
    /// unqualified fallback.
    pub fn last_frame(&self, tab_id: Option<u64>) -> Option<&FrameInfo> {
        self.last_frame_with_tab_id(tab_id).map(|(_, frame)| frame)
    }

    /// The same cached frame plus the *resolved* tab identity. A screenshot
    /// without an explicit tab must record this identity with its generation
    /// so evidence cannot be attributed to another observer's tab.
    pub fn last_frame_with_tab_id(&self, tab_id: Option<u64>) -> Option<(u64, &FrameInfo)> {
        let tab_id = tab_id.or(self.last_seen_tab)?;
        self.last_frames.get(&tab_id).map(|frame| (tab_id, frame))
    }

    pub fn shutdown(&mut self) -> io::Result<()> {
        blueice_ipc::write_client_message(&mut self.stream, &ClientMessage::Shutdown)
    }

    /// Not wired to any tool yet (Phase 12 proper owns the human/AI
    /// visibility-control surface); exposed so a future tool is a
    /// one-line addition rather than a new connection method.
    pub fn set_visible(&mut self, visible: bool) -> io::Result<()> {
        blueice_ipc::write_client_message(
            &mut self.stream,
            &ClientMessage::Chrome(ChromeCommand::SetVisible(visible)),
        )
    }
}

/// Renders `frame` (already mapped from shared memory) to PNG bytes,
/// for the `screenshot` tool's base64-encoded MCP image content --
/// round-trips through a temp file rather than adding an in-memory PNG
/// encoder entry point to `blueice-raster`'s public API for this one
/// caller.
pub fn frame_to_png_bytes(pixels: &[u8], width: u32, height: u32) -> io::Result<Vec<u8>> {
    let pixmap = blueice_raster::Pixmap {
        width,
        height,
        pixels: pixels.to_vec(),
    };
    let path = std::env::temp_dir().join(format!(
        "blueice-mcp-screenshot-{}-{}.png",
        std::process::id(),
        fastrand_like_suffix()
    ));
    pixmap.save_png(&path)?;
    let bytes = std::fs::read(&path)?;
    let _ = std::fs::remove_file(&path);
    Ok(bytes)
}

/// A cheap, dependency-free unique-enough suffix for the temp file
/// name above -- not a real RNG, just the low bits of the current
/// time, sufficient to avoid two concurrent screenshots on the same
/// process colliding.
fn fastrand_like_suffix() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

/// `core` is expected to sit next to this binary in the same build
/// output directory, same convention `blueice-frontend-reference`
/// already uses for the same reason (both are workspace members
/// landing in the same `target/<profile>/`) -- except a `cargo test`
/// integration-test binary lands one level deeper, in `target/
/// <profile>/deps/`, so this steps back out of a `deps` directory
/// before joining, letting the same lookup work from either place.
fn sibling_core_binary(this_exe: &Path) -> PathBuf {
    sibling_binary(this_exe, "blueice-core")
}

/// The workspace binary `stem` next to `this_exe` -- the same lookup for
/// `blueice-core` and `blueice-downloads`, since both land in the same
/// `target/<profile>/` directory as this crate's own binary.
pub(crate) fn sibling_binary(this_exe: &Path, stem: &str) -> PathBuf {
    let name = if cfg!(windows) {
        format!("{stem}.exe")
    } else {
        stem.to_string()
    };
    let dir = this_exe.parent().unwrap_or_else(|| Path::new("."));
    let dir = if dir.file_name().is_some_and(|n| n == "deps") {
        dir.parent().unwrap_or(dir)
    } else {
        dir
    };
    dir.join(name)
}

/// Process ID alone isn't actually unique enough: `CoreProcess::spawn`
/// (and so this) can be called more than once within one process --
/// e.g. two `#[test]`s in the same test binary, which Rust runs
/// concurrently on separate threads of the *same* process by default --
/// and two concurrent calls sharing a path would race to bind the same
/// socket. A monotonic counter alongside the PID makes every call
/// unique regardless of how many happen in this process's lifetime.
mod process;
pub use process::CoreProcess;
#[cfg(test)]
use process::*;

use super::server;
pub use server::BlueIceMcpServer;

#[cfg(test)]
mod tests;
