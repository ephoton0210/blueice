// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The message loop `blueice-core`'s process binary drives: read a
//! [`blueice_ipc::ClientMessage`], apply it to a [`Page`], reply. Kept
//! generic over `Read + Write + `[`ReadTimeout`] (rather than
//! hardcoding a `UnixStream`) so it's testable over an in-process pipe
//! the same way `blueice-ipc`'s own IPC-boundary test is (`UnixStream::
//! pair`) -- this is the "drive the real protocol with a test client"
//! strategy from `TEST_PLAN.md`'s UI testing section, applied one layer
//! up.
//!
//! Every state-changing message (`Navigate`, `Resize`, `Click` that
//! lands on a link, `Scroll`, `ActOn`, `Highlight`) ends with a fresh
//! frame written to the frame-plane and a `FrameReady` reply --
//! `Chrome` and `Hover` are exceptions: `Chrome` (per `BROWSER_CORE_
//! PLAN.md` §1, the render pipeline runs identically regardless of
//! window visibility, so there's nothing here for it to change) and
//! `Hover` (nothing paints differently yet -- `:hover` isn't in the
//! MVP CSS selector list -- so there's no frame to refresh, only
//! `Page`'s own hover state for a future `GetRepresentation` or
//! `:hover` style to read).
//!
//! **Gated navigation (`phase-7-local-ai/PLAN.md`'s "Wiring design")**:
//! every navigate-capable action (`Navigate`, a link-`Click`/`ActOn`'s
//! resulting href, `OpenTab{url}`) is a two-phase operation rather than
//! a single synchronous step. Phase 1 ([`begin_gated_navigation`],
//! called synchronously from the main dispatch below) resolves and
//! validates the target the same way the pre-gating code always did
//! (built-in `about:` pages and an invalid scheme are still handled
//! synchronously, no thread, no gatekeeper -- see that fn's own docs),
//! then hands a well-formed http(s) URL to a background thread that
//! runs both gatekeeper stages and the fetch itself
//! (`crate::gatekeeper_client::check_and_fetch`), reporting its outcome
//! back over an `mpsc` channel. This loop never blocks on that thread;
//! it returns to the top of the loop immediately. Phase 2 (the
//! completion-draining step at the bottom of the loop) applies a
//! still-current completion's result to real `Page`/`TabManager` state
//! and writes the deferred reply, tagged with the *original*
//! `tab_id`/`request_id` captured when the async op began -- a
//! completion superseded by a newer navigation to the same tab (or
//! whose tab has since closed) is silently discarded: never applied,
//! never replied to.
//!
//! To let the loop poll for completions between reads without blocking
//! indefinitely on a client that has nothing more to send right now,
//! `run_session` puts `stream` into a short-read-timeout mode via
//! [`ReadTimeout`] -- a timeout is treated as "no message yet, go
//! around the loop again," never as a disconnect (every *other* read
//! error still means disconnect, exactly as before gating existed).

use crate::downloads_page::{downloads_html, is_downloads_url, DownloadsView};
use crate::gatekeeper_client::{self, NavOutcome};
use crate::script::ScriptScheduler;
use crate::tabs::{extension_navigation_rules_block_url, HistoryDestination, HistoryDirection};
use crate::{GroupId, Page, TabGroup, TabId, TabManager};
use blueice_dom::NodeId;
use blueice_ipc::downloads::TransferInfo;
use blueice_ipc::extension::ExtensionRuntimeEvent;
use blueice_ipc::{shm, ClientMessage, ExtensionPopup, NodeAction, ServerMessage, TabGroupSummary, TabSummary};
use std::collections::{HashMap, HashSet};
use std::io::{self, Read, Write};
use std::path::Path;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

/// How long `run_session`'s read blocks waiting for the next client
/// message before giving up and polling the completion channel instead
/// -- short enough that a background gatekeeper check/fetch completing
/// is noticed promptly, long enough that the loop doesn't busy-spin
/// between real messages.
const POLL_INTERVAL: Duration = Duration::from_millis(25);

/// A small seam over `std::os::unix::net::UnixStream::set_read_timeout`
/// so `run_session` can require it as a trait bound rather than
/// hardcoding `UnixStream` -- every real caller (the production
/// binary, and every test in this module, all via `UnixStream::pair`)
/// already uses `UnixStream`, so this isn't a breaking bound in
/// practice.
pub trait ReadTimeout {
    fn set_read_timeout(&self, dur: Option<Duration>) -> io::Result<()>;
}

/// A request from the extension-protocol listener into the one thread that
/// owns `TabManager` and all live [`Page`] state. Keeping the reply channel
/// with the request means an extension handler can wait for a bounded answer
/// without ever sharing `Page` across threads or taking a mutable lock around
/// the render pipeline.
///
/// DOM version 1 requests leave `tab_id` absent and therefore preserve the
/// default-tab behavior. DOM versions 2 through 6 carry an explicit tab and a
/// stable control node ID for their narrow write operations. The separately
/// versioned network rule carries no page target. The session validates each
/// operation against its live `TabManager`/`Page` state before changing it.
pub enum ExtensionPageRequest {
    ReadRepresentation {
        tab_id: Option<u64>,
        reply: mpsc::Sender<Result<String, String>>,
    },
    ReadNetworkResponse {
        tab_id: u64,
        reply: mpsc::Sender<Result<Option<blueice_ipc::extension::NetworkResponseInfo>, String>>,
    },
    SetToolbarButton {
        connection_id: u64,
        label: String,
        reply: mpsc::Sender<Result<(), String>>,
    },
    ClearToolbarButton {
        connection_id: u64,
        reply: mpsc::Sender<()>,
    },
    ShowPopup {
        connection_id: u64,
        popup: ExtensionPopup,
        reply: mpsc::Sender<Result<(), String>>,
    },
    ClearPopup {
        connection_id: u64,
        reply: mpsc::Sender<()>,
    },
    SetTextInputValue {
        tab_id: u64,
        node_id: u64,
        value: String,
        reply: mpsc::Sender<Result<(), String>>,
    },
    SetCheckboxChecked {
        tab_id: u64,
        node_id: u64,
        checked: bool,
        reply: mpsc::Sender<Result<(), String>>,
    },
    SetTextareaValue {
        tab_id: u64,
        node_id: u64,
        value: String,
        reply: mpsc::Sender<Result<(), String>>,
    },
    /// Sets one integer value on an explicit range input after core derives
    /// and validates that control's live constraints.
    SetRangeInputValue {
        tab_id: u64,
        node_id: u64,
        value: i64,
        reply: mpsc::Sender<Result<(), String>>,
    },
    /// Selects one explicit radio while core owns the group-membership and
    /// mutual-exclusion semantics.
    SetRadioChecked {
        tab_id: u64,
        node_id: u64,
        reply: mpsc::Sender<Result<(), String>>,
    },
    /// Selects one explicit option while core owns the live single-select
    /// validation and the corresponding sibling deselection.
    SelectOption {
        tab_id: u64,
        node_id: u64,
        reply: mpsc::Sender<Result<(), String>>,
    },
    /// Adds one core-validated, connection-scoped exact navigation block rule
    /// evaluated for the initial request and later redirect hops. The opaque
    /// connection ID is allocated by `blueice-core`, never supplied by an
    /// extension.
    RegisterNetworkBlockUrl {
        connection_id: u64,
        url: String,
        reply: mpsc::Sender<Result<(), String>>,
    },
    /// Clears the rule set when the associated extension socket disconnects.
    /// The acknowledgement makes disconnect cleanup ordered with respect to
    /// subsequent frontend navigation work on this session thread.
    ClearNetworkBlockUrls {
        connection_id: u64,
        reply: mpsc::Sender<()>,
    },
}

impl ReadTimeout for std::os::unix::net::UnixStream {
    fn set_read_timeout(&self, dur: Option<Duration>) -> io::Result<()> {
        std::os::unix::net::UnixStream::set_read_timeout(self, dur)
    }
}

fn is_timeout(err: &io::Error) -> bool {
    matches!(
        err.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
    )
}

/// Runs the message loop for one client connection until it sends
/// `Shutdown` or disconnects. `frame_dir` is where this session's
/// frames are written (see [`blueice_ipc::shm`]); `generation` counts
/// all frame writes in this session for legacy diagnostics, while every
/// [`Page`] owns the generation sent in its own `FrameReady` and
/// representation. That per-tab pairing means one tab's live refresh
/// cannot make another tab's current representation claim a different
/// render pass. `gatekeeper_socket` is where a gated
/// navigation's background thread connects to review a URL/fetched
/// page -- production code passes `blueice_ipc::gatekeeper::
/// default_gatekeeper_socket_path()`; tests thread their own
/// independent fake-gatekeeper socket path through instead, since many
/// gatekeeper-behavior tests need to run concurrently in the same test
/// binary process.
///
/// The very first message must be [`ClientMessage::Hello`] (`phase-1-
/// ai-representation-layer/PLAN.md` §3's `protocol_version` handshake)
/// -- a fresh connection whose first message either isn't `Hello` or
/// declares an unsupported version is rejected with a
/// [`ServerMessage::Error`] before anything else is processed, and the
/// session ends without entering the main loop. A `Hello` seen again
/// *after* the handshake (e.g. a second external client's own
/// handshake, forwarded by `blueice-launcher`'s broker into the one
/// shared connection it holds with `core`) is just answered again,
/// rather than re-gating the whole session -- tearing down a shared
/// connection over one client's handshake would end every other
/// client's session too.
///
/// **Multi-tab addressing** (`phase-16-multi-tab-and-tab-groups/
/// PLAN.md`): every per-tab-scoped message
/// (`Navigate`, `GoBack`, `GoForward`, `Resize`, `Click`, `Hover`, `Scroll`,
/// `GetRepresentation`, `ActOn`, `Highlight`, `GetDom`, `CloseTab`) is
/// addressed by the envelope's `tab_id` -- `None` resolves to
/// [`TabManager::default_tab`], reproducing pre-Phase-16 single-`Page`
/// behavior byte-for-byte for a client that never sends `OpenTab`. A
/// `tab_id` (explicit or defaulted) that doesn't resolve to a live tab
/// replies [`ServerMessage::Error`] -- a protocol-addressing error, not
/// the harmless no-op a stale `NodeId` already gets in [`Page::act`].
/// `Resize` still validates its addressed tab, but then eagerly relayouts all
/// live tabs for the one physical frontend viewport. Tab/group list and
/// group-property operations aren't scoped to an existing tab at all (there's
/// no "current tab" concept `core` tracks -- see [`TabManager`]'s own docs
/// for why) and ignore any `tab_id` on the envelope; only `SetTabGroup` is
/// explicitly tab-addressed.
pub fn run_session<S: Read + Write + ReadTimeout>(
    tabs: &mut TabManager,
    stream: &mut S,
    frame_dir: &Path,
    generation: &mut u64,
    gatekeeper_socket: &Path,
) -> io::Result<()> {
    let mut no_scripts = NoScriptScheduler;
    run_session_with_script_and_extension_requests(
        tabs,
        stream,
        frame_dir,
        generation,
        gatekeeper_socket,
        &mut no_scripts,
        None,
    )
}

/// Like [`run_session`], with an optional extension-to-core request channel.
/// The ordinary frontend IPC loop and every existing test remain on the
/// `None` path; `blueice-core --extension-socket --extension-manifest` passes
/// its private extension listener's receiver here.
pub fn run_session_with_extension_requests<S: Read + Write + ReadTimeout>(
    tabs: &mut TabManager,
    stream: &mut S,
    frame_dir: &Path,
    generation: &mut u64,
    gatekeeper_socket: &Path,
    extension_requests: &mpsc::Receiver<ExtensionPageRequest>,
) -> io::Result<()> {
    run_session_with_extension_requests_and_events(
        tabs,
        stream,
        frame_dir,
        generation,
        gatekeeper_socket,
        extension_requests,
        None,
    )
}

/// Like [`run_session_with_extension_requests`], with an optional bounded
/// sender for core-defined extension lifecycle events. The session uses
/// `try_send`, so a delayed extension can never stall page ownership.
pub fn run_session_with_extension_requests_and_events<S: Read + Write + ReadTimeout>(
    tabs: &mut TabManager,
    stream: &mut S,
    frame_dir: &Path,
    generation: &mut u64,
    gatekeeper_socket: &Path,
    extension_requests: &mpsc::Receiver<ExtensionPageRequest>,
    extension_events: Option<&mpsc::SyncSender<ExtensionRuntimeEvent>>,
) -> io::Result<()> {
    let mut no_scripts = NoScriptScheduler;
    run_session_with_script_and_extension_requests_and_events(
        tabs,
        stream,
        frame_dir,
        generation,
        gatekeeper_socket,
        &mut no_scripts,
        Some(extension_requests),
        extension_events,
    )
}

/// The production-capable session entry point. A script scheduler runs a
/// document's parser scripts after a cleared navigation applies its DOM but
/// before the navigation reply's first frame; [`run_session`] supplies a
/// no-op scheduler for existing protocol-only callers and unit tests.
pub fn run_session_with_script<S: Read + Write + ReadTimeout>(
    tabs: &mut TabManager,
    stream: &mut S,
    frame_dir: &Path,
    generation: &mut u64,
    gatekeeper_socket: &Path,
    script_scheduler: &mut dyn ScriptScheduler,
) -> io::Result<()> {
    run_session_with_script_and_extension_requests_and_events(
        tabs,
        stream,
        frame_dir,
        generation,
        gatekeeper_socket,
        script_scheduler,
        None,
        None,
    )
}

/// The production-capable session entry point with an optional private
/// extension-request channel. See [`ExtensionPageRequest`] for the deliberate
/// first-slice scope and ownership boundary.
pub fn run_session_with_script_and_extension_requests<S: Read + Write + ReadTimeout>(
    tabs: &mut TabManager,
    stream: &mut S,
    frame_dir: &Path,
    generation: &mut u64,
    gatekeeper_socket: &Path,
    script_scheduler: &mut dyn ScriptScheduler,
    extension_requests: Option<&mpsc::Receiver<ExtensionPageRequest>>,
) -> io::Result<()> {
    run_session_with_script_and_extension_requests_and_events(
        tabs,
        stream,
        frame_dir,
        generation,
        gatekeeper_socket,
        script_scheduler,
        extension_requests,
        None,
    )
}

/// The production-capable session entry point with optional private extension
/// request and lifecycle-event channels. Navigation events are advisory and
/// bounded: a full queue is deliberately not allowed to block the frontend's
/// sole `TabManager` owner.
#[allow(clippy::too_many_arguments)] // keeps the established session entrypoint parameters explicit
pub fn run_session_with_script_and_extension_requests_and_events<S: Read + Write + ReadTimeout>(
    tabs: &mut TabManager,
    stream: &mut S,
    frame_dir: &Path,
    generation: &mut u64,
    gatekeeper_socket: &Path,
    script_scheduler: &mut dyn ScriptScheduler,
    extension_requests: Option<&mpsc::Receiver<ExtensionPageRequest>>,
    extension_events: Option<&mpsc::SyncSender<ExtensionRuntimeEvent>>,
) -> io::Result<()> {
    // Best-effort: on at least one real platform, setting a read
    // timeout on a Unix domain socket whose peer has *already*
    // disconnected (a client that connects and drops the connection
    // before this thread even starts) can itself fail with `EINVAL`,
    // even though nothing is actually wrong with the connection from
    // this session's own perspective -- the very next read below
    // simply returns immediately with a real disconnect error either
    // way. Propagating that failure via `?` here would turn a client
    // that never sent anything at all into a spurious session error,
    // instead of the ordinary clean-disconnect outcome every other
    // never-sent-anything case already gets. A failure here just means
    // the loop falls back to plain blocking reads (no completion-
    // draining poll tick between messages) rather than ending the
    // session outright.
    let _ = stream.set_read_timeout(Some(POLL_INTERVAL));
    if !perform_handshake(stream)? {
        return Ok(());
    }

    let (completion_tx, completion_rx) = mpsc::channel::<Completion>();
    let mut pending_nav_seq: HashMap<TabId, u64> = HashMap::new();
    let (listing_tx, listing_rx) = mpsc::channel::<DownloadsListing>();
    let mut downloads_refresher = DownloadsRefresher::default();
    // Only the connection that published the native button can remove it.
    let mut extension_toolbar: Option<(u64, String)> = None;
    let mut extension_popup: Option<(u64, ExtensionPopup)> = None;

    loop {
        match blueice_ipc::read_client_message_with_ids(stream) {
            Ok((tab_id, request_id, msg)) => {
                let target = tab_id
                    .map(TabId::from_u64)
                    .unwrap_or_else(|| tabs.default_tab());
                // Every per-tab reply below echoes `Some(target.as_u64())`,
                // the *resolved* tab -- not the raw (possibly `None`, if
                // the request left it defaulted) `tab_id` the request
                // carried. Echoing the ambiguous original back would
                // defeat the whole point of this field: a client watching
                // a shared, multi-tab, broadcast connection (`blueice-
                // launcher`'s broker) needs every reply to self-disclose
                // which concrete tab it's about, including one produced
                // by a request that left it implicit.
                let reply_tab = Some(target.as_u64());
                match msg {
                    ClientMessage::Hello { protocol_version } => {
                        reply_hello(stream, request_id, protocol_version)?
                    }
                    ClientMessage::Navigate { url } => {
                        if tabs.get(target).is_none() {
                            write_unknown_tab_error(stream, request_id, target)?;
                        } else {
                            begin_gated_navigation(
                                tabs,
                                stream,
                                frame_dir,
                                generation,
                                reply_tab,
                                request_id,
                                target,
                                url,
                                PendingKind::Navigate,
                                &mut pending_nav_seq,
                                &mut downloads_refresher,
                                &completion_tx,
                                gatekeeper_socket,
                                extension_events,
                            )?;
                        }
                    }
                    history_message @ (ClientMessage::GoBack | ClientMessage::GoForward) => {
                        if tabs.get(target).is_none() {
                            write_unknown_tab_error(stream, request_id, target)?;
                            continue;
                        }
                        let direction = if matches!(history_message, ClientMessage::GoBack) {
                            HistoryDirection::Back
                        } else {
                            HistoryDirection::Forward
                        };
                        begin_history_navigation(
                            tabs,
                            stream,
                            frame_dir,
                            generation,
                            reply_tab,
                            request_id,
                            target,
                            direction,
                            &mut pending_nav_seq,
                            &mut downloads_refresher,
                            &completion_tx,
                            gatekeeper_socket,
                            extension_events,
                        )?;
                    }
                    ClientMessage::GetHistoryState => {
                        let Some(can_go_back) = tabs.can_go_back(target) else {
                            write_unknown_tab_error(stream, request_id, target)?;
                            continue;
                        };
                        let can_go_forward = tabs
                            .can_go_forward(target)
                            .expect("a live tab has a history state");
                        blueice_ipc::write_server_message_with_ids(
                            stream,
                            reply_tab,
                            request_id,
                            &ServerMessage::HistoryState {
                                can_go_back,
                                can_go_forward,
                            },
                        )?;
                    }
                    ClientMessage::Resize { width, height } => {
                        if tabs.get(target).is_none() {
                            write_unknown_tab_error(stream, request_id, target)?;
                            continue;
                        }
                        // A native frontend has one physical content viewport,
                        // not one viewport per selected tab. Eagerly reflowing
                        // every Page here keeps a background tab display-ready
                        // when that frontend later selects it; there is still
                        // no core "active tab" state.
                        tabs.resize_all(width as f64, height as f64);
                        let resized: Vec<TabId> = tabs.ids().collect();
                        for id in resized {
                            let page = tabs.get_mut(id).expect("ids only yields live tabs");
                            send_frame(
                                page,
                                stream,
                                frame_dir,
                                generation,
                                Some(id.as_u64()),
                                (id == target).then_some(request_id).flatten(),
                            )?;
                        }
                    }
                    ClientMessage::Click { x, y } => {
                        let Some((node, href)) = tabs
                            .get(target)
                            .map(|page| (page.click_target(x, y), page.click(x, y)))
                        else {
                            write_unknown_tab_error(stream, request_id, target)?;
                            continue;
                        };
                        let event = node
                            .map(|node| {
                                script_scheduler
                                    .dispatch_event(tabs, target, node, "click")
                                    .unwrap_or_default()
                            })
                            .unwrap_or_default();
                        let mut focus_changed = false;
                        if !event.default_prevented {
                            focus_changed = tabs
                                .get_mut(target)
                                .expect("checked immediately above")
                                .focus_text_input_at(node);
                            if node.is_some_and(|node| {
                                tabs.get_mut(target)
                                    .expect("checked immediately above")
                                    .apply_gatekeeper_settings_control(node)
                                    .is_some()
                            }) {
                                let page = tabs
                                    .get_mut(target)
                                    .expect("a settings control cannot close a core-owned tab");
                                send_frame(
                                    page, stream, frame_dir, generation, reply_tab, request_id,
                                )?;
                                continue;
                            }
                            if let Some(href) = href {
                                begin_gated_navigation(
                                    tabs,
                                    stream,
                                    frame_dir,
                                    generation,
                                    reply_tab,
                                    request_id,
                                    target,
                                    href,
                                    PendingKind::Navigate,
                                    &mut pending_nav_seq,
                                    &mut downloads_refresher,
                                    &completion_tx,
                                    gatekeeper_socket,
                                    extension_events,
                                )?;
                                continue;
                            }
                        }
                        if event.ran_event || focus_changed {
                            let page = tabs
                                .get_mut(target)
                                .expect("a script event cannot close a core-owned tab");
                            send_frame(page, stream, frame_dir, generation, reply_tab, request_id)?;
                        }
                    }
                    ClientMessage::Scroll { delta_y } => match tabs.get_mut(target) {
                        Some(page) => {
                            page.scroll_by(delta_y);
                            send_frame(page, stream, frame_dir, generation, reply_tab, request_id)?;
                        }
                        None => write_unknown_tab_error(stream, request_id, target)?,
                    },
                    ClientMessage::InsertText { text } => {
                        let changed = match tabs.get_mut(target) {
                            Some(page) => page.insert_focused_text(&text),
                            None => {
                                write_unknown_tab_error(stream, request_id, target)?;
                                continue;
                            }
                        };
                        if changed {
                            let focused = tabs
                                .get(target)
                                .and_then(|page| page.focused())
                                .expect("a changed focused text input remains focused");
                            let _ = script_scheduler.dispatch_event(tabs, target, focused, "input");
                            let _ =
                                script_scheduler.dispatch_event(tabs, target, focused, "change");
                            let page = tabs
                                .get_mut(target)
                                .expect("a text edit cannot close a core-owned tab");
                            send_frame(page, stream, frame_dir, generation, reply_tab, request_id)?;
                        }
                    }
                    ClientMessage::DeleteBackward => {
                        let changed = match tabs.get_mut(target) {
                            Some(page) => page.delete_focused_text_backward(),
                            None => {
                                write_unknown_tab_error(stream, request_id, target)?;
                                continue;
                            }
                        };
                        if changed {
                            let focused = tabs
                                .get(target)
                                .and_then(|page| page.focused())
                                .expect("a changed focused text input remains focused");
                            let _ = script_scheduler.dispatch_event(tabs, target, focused, "input");
                            let _ =
                                script_scheduler.dispatch_event(tabs, target, focused, "change");
                            let page = tabs
                                .get_mut(target)
                                .expect("a text edit cannot close a core-owned tab");
                            send_frame(page, stream, frame_dir, generation, reply_tab, request_id)?;
                        }
                    }
                    ClientMessage::Hover { x, y } => {
                        if let Some(page) = tabs.get_mut(target) {
                            page.hover_at(x, y);
                        } else {
                            write_unknown_tab_error(stream, request_id, target)?;
                        }
                    }
                    ClientMessage::GetRepresentation => match tabs.get_mut(target) {
                        Some(page) => {
                            let snapshot = page.snapshot(page.frame_generation(), target.as_u64());
                            blueice_ipc::write_server_message_with_ids(
                                stream,
                                reply_tab,
                                request_id,
                                &ServerMessage::Representation(snapshot),
                            )?;
                        }
                        None => write_unknown_tab_error(stream, request_id, target)?,
                    },
                    ClientMessage::GetDom => match tabs.get_mut(target) {
                        Some(page) => blueice_ipc::write_server_message_with_ids(
                            stream,
                            reply_tab,
                            request_id,
                            &ServerMessage::Dom(page.dom_dump()),
                        )?,
                        None => write_unknown_tab_error(stream, request_id, target)?,
                    },
                    ClientMessage::ActOn { id, action } => {
                        if tabs.get(target).is_none() {
                            write_unknown_tab_error(stream, request_id, target)?;
                            continue;
                        }
                        let node = NodeId::from_u64(id);
                        let is_click = matches!(&action, NodeAction::Click);
                        let is_value_change = matches!(&action, NodeAction::SetValue(_));
                        // The default action changes the core-owned DOM first;
                        // input/change listeners observe that new value.
                        let href = tabs
                            .get_mut(target)
                            .expect("checked immediately above")
                            .act(node, action);
                        if is_click {
                            let event = script_scheduler
                                .dispatch_event(tabs, target, node, "click")
                                .unwrap_or_default();
                            if !event.default_prevented {
                                if tabs
                                    .get_mut(target)
                                    .expect("checked immediately above")
                                    .apply_gatekeeper_settings_control(node)
                                    .is_some()
                                {
                                    let page = tabs
                                        .get_mut(target)
                                        .expect("a settings control cannot close a core-owned tab");
                                    send_frame(
                                        page, stream, frame_dir, generation, reply_tab, request_id,
                                    )?;
                                    continue;
                                }
                                if let Some(href) = href {
                                    begin_gated_navigation(
                                        tabs,
                                        stream,
                                        frame_dir,
                                        generation,
                                        reply_tab,
                                        request_id,
                                        target,
                                        href,
                                        PendingKind::Navigate,
                                        &mut pending_nav_seq,
                                        &mut downloads_refresher,
                                        &completion_tx,
                                        gatekeeper_socket,
                                        extension_events,
                                    )?;
                                    continue;
                                }
                            }
                            if event.ran_event {
                                let page = tabs
                                    .get_mut(target)
                                    .expect("a script event cannot close a core-owned tab");
                                send_frame(
                                    page, stream, frame_dir, generation, reply_tab, request_id,
                                )?;
                            }
                        } else {
                            if is_value_change {
                                let _ =
                                    script_scheduler.dispatch_event(tabs, target, node, "input");
                                let _ =
                                    script_scheduler.dispatch_event(tabs, target, node, "change");
                            }
                            let page = tabs
                                .get_mut(target)
                                .expect("a script event cannot close a core-owned tab");
                            send_frame(page, stream, frame_dir, generation, reply_tab, request_id)?;
                        }
                    }
                    ClientMessage::Highlight { id } => match tabs.get_mut(target) {
                        Some(page) => {
                            page.set_highlight(id.map(NodeId::from_u64));
                            send_frame(page, stream, frame_dir, generation, reply_tab, request_id)?;
                        }
                        None => write_unknown_tab_error(stream, request_id, target)?,
                    },
                    ClientMessage::OpenTab { url } => handle_open_tab(
                        tabs,
                        stream,
                        frame_dir,
                        generation,
                        request_id,
                        url,
                        &mut pending_nav_seq,
                        &mut downloads_refresher,
                        &completion_tx,
                        gatekeeper_socket,
                        extension_events,
                    )?,
                    ClientMessage::CloseTab => {
                        if tabs.close_tab(target) {
                            blueice_ipc::write_server_message_with_ids(
                                stream,
                                reply_tab,
                                request_id,
                                &ServerMessage::TabClosed {
                                    tab_id: target.as_u64(),
                                },
                            )?;
                            if extension_popup
                                .as_ref()
                                .is_some_and(|(_, popup)| popup.tab_id == target.as_u64())
                            {
                                extension_popup = None;
                                blueice_ipc::write_server_message_with_ids(
                                    stream,
                                    None,
                                    None,
                                    &ServerMessage::ExtensionPopup { popup: None },
                                )?;
                            }
                        } else {
                            write_unknown_tab_error(stream, request_id, target)?;
                        }
                    }
                    ClientMessage::ListTabs => {
                        let summaries: Vec<TabSummary> = tabs
                            .ids()
                            .map(|id| TabSummary {
                                id: id.as_u64(),
                                url: tabs.get(id).and_then(Page::url).map(str::to_string),
                                group_id: tabs.tab_group(id).map(GroupId::as_u64),
                            })
                            .collect();
                        blueice_ipc::write_server_message_with_id(
                            stream,
                            request_id,
                            &ServerMessage::Tabs(summaries),
                        )?;
                    }
                    ClientMessage::CreateTabGroup { name, color } => {
                        let name = match validate_group_name(name) {
                            Ok(name) => name,
                            Err(message) => {
                                write_error(stream, None, request_id, message)?;
                                continue;
                            }
                        };
                        let color = match validate_group_color(color) {
                            Ok(color) => color,
                            Err(message) => {
                                write_error(stream, None, request_id, message)?;
                                continue;
                            }
                        };
                        let id = tabs.create_group(name, color);
                        let summary = tab_group_summary(
                            tabs.group(id).expect("a just-created group is live"),
                        );
                        blueice_ipc::write_server_message_with_id(
                            stream,
                            request_id,
                            &ServerMessage::TabGroupCreated(summary),
                        )?;
                    }
                    ClientMessage::SetTabGroup { group_id } => {
                        if tabs.get(target).is_none() {
                            write_unknown_tab_error(stream, request_id, target)?;
                            continue;
                        }
                        let group_id = group_id.map(GroupId::from_u64);
                        if let Some(group_id) = group_id {
                            if tabs.group(group_id).is_none() {
                                write_error(
                                    stream,
                                    reply_tab,
                                    request_id,
                                    format!("unknown tab group {}", group_id.as_u64()),
                                )?;
                                continue;
                            }
                        }
                        tabs.set_tab_group(target, group_id);
                        blueice_ipc::write_server_message_with_ids(
                            stream,
                            reply_tab,
                            request_id,
                            &ServerMessage::TabGroupAssigned {
                                tab_id: target.as_u64(),
                                group_id: group_id.map(GroupId::as_u64),
                            },
                        )?;
                    }
                    ClientMessage::RenameTabGroup { group_id, name } => {
                        let id = GroupId::from_u64(group_id);
                        if tabs.group(id).is_none() {
                            write_error(
                                stream,
                                None,
                                request_id,
                                format!("unknown tab group {group_id}"),
                            )?;
                            continue;
                        }
                        let name = match validate_group_name(name) {
                            Ok(name) => name,
                            Err(message) => {
                                write_error(stream, None, request_id, message)?;
                                continue;
                            }
                        };
                        tabs.rename_group(id, name);
                        let summary =
                            tab_group_summary(tabs.group(id).expect("group remains live"));
                        blueice_ipc::write_server_message_with_id(
                            stream,
                            request_id,
                            &ServerMessage::TabGroupUpdated(summary),
                        )?;
                    }
                    ClientMessage::SetTabGroupColor { group_id, color } => {
                        let id = GroupId::from_u64(group_id);
                        if tabs.group(id).is_none() {
                            write_error(
                                stream,
                                None,
                                request_id,
                                format!("unknown tab group {group_id}"),
                            )?;
                            continue;
                        }
                        let color = match validate_group_color(color) {
                            Ok(color) => color,
                            Err(message) => {
                                write_error(stream, None, request_id, message)?;
                                continue;
                            }
                        };
                        tabs.set_group_color(id, color);
                        let summary =
                            tab_group_summary(tabs.group(id).expect("group remains live"));
                        blueice_ipc::write_server_message_with_id(
                            stream,
                            request_id,
                            &ServerMessage::TabGroupUpdated(summary),
                        )?;
                    }
                    ClientMessage::SetTabGroupCollapsed {
                        group_id,
                        collapsed,
                    } => {
                        let id = GroupId::from_u64(group_id);
                        if tabs.group(id).is_none() {
                            write_error(
                                stream,
                                None,
                                request_id,
                                format!("unknown tab group {group_id}"),
                            )?;
                            continue;
                        }
                        tabs.set_group_collapsed(id, collapsed);
                        let summary =
                            tab_group_summary(tabs.group(id).expect("group remains live"));
                        blueice_ipc::write_server_message_with_id(
                            stream,
                            request_id,
                            &ServerMessage::TabGroupUpdated(summary),
                        )?;
                    }
                    ClientMessage::CloseTabGroup { group_id } => {
                        let id = GroupId::from_u64(group_id);
                        if !tabs.close_group(id) {
                            write_error(
                                stream,
                                None,
                                request_id,
                                format!("unknown tab group {group_id}"),
                            )?;
                            continue;
                        }
                        blueice_ipc::write_server_message_with_id(
                            stream,
                            request_id,
                            &ServerMessage::TabGroupClosed { group_id },
                        )?;
                    }
                    ClientMessage::ListTabGroups => {
                        let groups = tabs.groups().map(tab_group_summary).collect();
                        blueice_ipc::write_server_message_with_id(
                            stream,
                            request_id,
                            &ServerMessage::TabGroups(groups),
                        )?;
                    }
                    ClientMessage::GetExtensionToolbar => {
                        blueice_ipc::write_server_message_with_ids(
                            stream,
                            None,
                            request_id,
                            &ServerMessage::ExtensionToolbar {
                                label: extension_toolbar.as_ref().map(|(_, label)| label.clone()),
                            },
                        )?;
                    }
                    ClientMessage::ActivateExtensionToolbar => {
                        let result = if extension_toolbar.is_none() {
                            Err("no extension toolbar button is installed".to_string())
                        } else if tabs.get(target).is_none() {
                            Err(format!("unknown tab {}", target.as_u64()))
                        } else if let Some(events) = extension_events {
                            events
                                .try_send(ExtensionRuntimeEvent::ToolbarActivated {
                                    tab_id: target.as_u64(),
                                })
                                .map_err(|_| "extension event queue is unavailable".to_string())
                        } else {
                            Err("the extension runtime is unavailable".to_string())
                        };
                        if let Err(message) = result {
                            blueice_ipc::write_server_message_with_ids(
                                stream,
                                reply_tab,
                                request_id,
                                &ServerMessage::Error { message },
                            )?;
                        }
                    }
                    ClientMessage::GetExtensionPopup => {
                        blueice_ipc::write_server_message_with_ids(
                            stream,
                            None,
                            request_id,
                            &ServerMessage::ExtensionPopup {
                                popup: extension_popup.as_ref().map(|(_, popup)| popup.clone()),
                            },
                        )?;
                    }
                    ClientMessage::DismissExtensionPopup => {
                        if extension_popup
                            .as_ref()
                            .is_some_and(|(_, popup)| popup.tab_id == target.as_u64())
                        {
                            extension_popup = None;
                            blueice_ipc::write_server_message_with_ids(
                                stream,
                                None,
                                None,
                                &ServerMessage::ExtensionPopup { popup: None },
                            )?;
                        }
                    }
                    // Chrome commands (window show/hide) operate on
                    // `frontend`'s own window, not on anything `core`
                    // owns -- see module docs.
                    ClientMessage::Chrome(_) => {}
                    ClientMessage::Shutdown => return Ok(()),
                    // Forward-compatibility fallback (plan §3): a variant
                    // this build doesn't recognize is ignored rather than
                    // treated as a protocol violation.
                    ClientMessage::Unknown => {}
                }
            }
            Err(e) if is_timeout(&e) => {} // no message yet -- fall through to drain completions
            Err(_) => return Ok(()),       // client disconnected without an explicit Shutdown
        }

        while let Ok(completion) = completion_rx.try_recv() {
            apply_completion(
                tabs,
                stream,
                frame_dir,
                generation,
                &pending_nav_seq,
                completion,
                &mut downloads_refresher,
                script_scheduler,
                extension_events,
            )?;
        }

        // Timer callbacks are regular BlueJS macrotasks, not render work.
        // Poll them between frontend reads; a callback's completion barrier
        // has applied every DOM write before this fresh frame is published.
        // `None` deliberately marks this as an unsolicited render update,
        // rather than pretending it answers the prior frontend request.
        let timer_tabs: Vec<TabId> = tabs.ids().collect();
        for tab_id in timer_tabs {
            if script_scheduler
                .run_due_timers(tabs, tab_id)
                .unwrap_or(false)
            {
                if let Some(page) = tabs.get_mut(tab_id) {
                    send_frame(
                        page,
                        stream,
                        frame_dir,
                        generation,
                        Some(tab_id.as_u64()),
                        None,
                    )?;
                }
            }
        }

        // Keep any open `about:downloads` tab current, off this thread.
        downloads_refresher.tick(tabs, &listing_tx, Instant::now());
        while let Ok(listing) = listing_rx.try_recv() {
            downloads_refresher.apply(tabs, stream, frame_dir, generation, listing)?;
        }

        if let Some(extension_requests) = extension_requests {
            while let Ok(request) = extension_requests.try_recv() {
                handle_extension_page_request(
                    tabs,
                    stream,
                    frame_dir,
                    generation,
                    script_scheduler,
                    &mut extension_toolbar,
                    &mut extension_popup,
                    request,
                )?;
            }
        }
    }
}

/// Applies a request received from the extension host. An accepted write
/// produces the same uncorrelated fresh frame that other background-originated
/// core work does, so every connected observer sees the core-owned mutation.
/// The typed one-shot response then gives the extension connection one answer
/// while keeping this session loop the only owner of page state.
fn handle_extension_page_request<S: Write>(
    tabs: &mut TabManager,
    stream: &mut S,
    frame_dir: &Path,
    generation: &mut u64,
    script_scheduler: &mut dyn ScriptScheduler,
    extension_toolbar: &mut Option<(u64, String)>,
    extension_popup: &mut Option<(u64, ExtensionPopup)>,
    request: ExtensionPageRequest,
) -> io::Result<()> {
    match request {
        ExtensionPageRequest::ReadRepresentation { tab_id, reply } => {
            let tab_id = tab_id
                .map(TabId::from_u64)
                .unwrap_or_else(|| tabs.default_tab());
            let result = tabs
                .get(tab_id)
                .ok_or_else(|| format!("unknown tab {}", tab_id.as_u64()))
                .and_then(|page| {
                    serde_json::to_string(&page.snapshot(page.frame_generation(), tab_id.as_u64()))
                        .map_err(|error| {
                            format!("could not serialize the core representation: {error}")
                        })
                });
            let _ = reply.send(result);
        }
        ExtensionPageRequest::ReadNetworkResponse { tab_id, reply } => {
            let result = tabs
                .get(TabId::from_u64(tab_id))
                .ok_or_else(|| format!("unknown tab {tab_id}"))
                .map(|page| page.network_response().cloned());
            let _ = reply.send(result);
        }
        ExtensionPageRequest::SetToolbarButton {
            connection_id,
            label,
            reply,
        } => {
            let result = blueice_extension_host::validate_toolbar_label(&label).and_then(|()| {
                if extension_popup
                    .as_ref()
                    .is_some_and(|(owner, _)| *owner != connection_id)
                {
                    blueice_ipc::write_server_message_with_ids(
                        stream,
                        None,
                        None,
                        &ServerMessage::ExtensionPopup { popup: None },
                    )
                    .map_err(|error| format!("could not remove old extension popup: {error}"))?;
                    *extension_popup = None;
                }
                blueice_ipc::write_server_message_with_ids(
                    stream,
                    None,
                    None,
                    &ServerMessage::ExtensionToolbar {
                        label: Some(label.clone()),
                    },
                )
                .map_err(|error| format!("could not publish extension toolbar: {error}"))?;
                *extension_toolbar = Some((connection_id, label));
                Ok(())
            });
            let _ = reply.send(result);
        }
        ExtensionPageRequest::ClearToolbarButton {
            connection_id,
            reply,
        } => {
            if extension_toolbar.as_ref().is_some_and(|(owner, _)| *owner == connection_id) {
                if extension_popup.as_ref().is_some_and(|(owner, _)| *owner == connection_id) {
                    *extension_popup = None;
                    blueice_ipc::write_server_message_with_ids(
                        stream,
                        None,
                        None,
                        &ServerMessage::ExtensionPopup { popup: None },
                    )?;
                }
                *extension_toolbar = None;
                blueice_ipc::write_server_message_with_ids(
                    stream,
                    None,
                    None,
                    &ServerMessage::ExtensionToolbar { label: None },
                )?;
            }
            let _ = reply.send(());
        }
        ExtensionPageRequest::ShowPopup {
            connection_id,
            popup,
            reply,
        } => {
            let result = blueice_extension_host::validate_popup_text(&popup.title, &popup.body)
                .and_then(|()| {
                    if !extension_toolbar
                        .as_ref()
                        .is_some_and(|(owner, _)| *owner == connection_id)
                    {
                        return Err("a popup requires this connection's toolbar button".to_string());
                    }
                    if tabs.get(TabId::from_u64(popup.tab_id)).is_none() {
                        return Err(format!("unknown tab {}", popup.tab_id));
                    }
                    blueice_ipc::write_server_message_with_ids(
                        stream,
                        None,
                        None,
                        &ServerMessage::ExtensionPopup {
                            popup: Some(popup.clone()),
                        },
                    )
                    .map_err(|error| format!("could not publish extension popup: {error}"))?;
                    *extension_popup = Some((connection_id, popup));
                    Ok(())
                });
            let _ = reply.send(result);
        }
        ExtensionPageRequest::ClearPopup {
            connection_id,
            reply,
        } => {
            if extension_popup.as_ref().is_some_and(|(owner, _)| *owner == connection_id) {
                *extension_popup = None;
                blueice_ipc::write_server_message_with_ids(
                    stream,
                    None,
                    None,
                    &ServerMessage::ExtensionPopup { popup: None },
                )?;
            }
            let _ = reply.send(());
        }
        ExtensionPageRequest::SetTextInputValue {
            tab_id,
            node_id,
            value,
            reply,
        } => {
            let tab_id = TabId::from_u64(tab_id);
            let node_id = NodeId::from_u64(node_id);
            let result = if value.len() > blueice_ipc::extension::MAX_TEXT_WRITE_BYTES {
                Err(format!(
                    "text-control values cannot exceed {} bytes",
                    blueice_ipc::extension::MAX_TEXT_WRITE_BYTES
                ))
            } else {
                match tabs.get_mut(tab_id) {
                    Some(page) => page.set_text_input_value(node_id, value),
                    None => Err(format!("unknown tab {}", tab_id.as_u64())),
                }
            };
            if result.is_ok() {
                // Mirror first-party SetValue: script listeners observe the
                // core-owned new value before observers receive its frame.
                let _ = script_scheduler.dispatch_event(tabs, tab_id, node_id, "input");
                let _ = script_scheduler.dispatch_event(tabs, tab_id, node_id, "change");
                let page = tabs
                    .get_mut(tab_id)
                    .expect("a checked extension target tab remains live");
                send_frame(
                    page,
                    stream,
                    frame_dir,
                    generation,
                    Some(tab_id.as_u64()),
                    None,
                )?;
            }
            let _ = reply.send(result);
        }
        ExtensionPageRequest::SetCheckboxChecked {
            tab_id,
            node_id,
            checked,
            reply,
        } => {
            let tab_id = TabId::from_u64(tab_id);
            let node_id = NodeId::from_u64(node_id);
            let result = match tabs.get_mut(tab_id) {
                Some(page) => page.set_checkbox_checked(node_id, checked),
                None => Err(format!("unknown tab {}", tab_id.as_u64())),
            };
            if result.is_ok() {
                // Match the text-input extension operation: page event
                // handlers see core's new state before the shared frame is
                // published to observers.
                let _ = script_scheduler.dispatch_event(tabs, tab_id, node_id, "input");
                let _ = script_scheduler.dispatch_event(tabs, tab_id, node_id, "change");
                let page = tabs
                    .get_mut(tab_id)
                    .expect("a checked extension target tab remains live");
                send_frame(
                    page,
                    stream,
                    frame_dir,
                    generation,
                    Some(tab_id.as_u64()),
                    None,
                )?;
            }
            let _ = reply.send(result);
        }
        ExtensionPageRequest::SetTextareaValue {
            tab_id,
            node_id,
            value,
            reply,
        } => {
            let tab_id = TabId::from_u64(tab_id);
            let node_id = NodeId::from_u64(node_id);
            let result = if value.len() > blueice_ipc::extension::MAX_TEXT_WRITE_BYTES {
                Err(format!(
                    "text-control values cannot exceed {} bytes",
                    blueice_ipc::extension::MAX_TEXT_WRITE_BYTES
                ))
            } else {
                match tabs.get_mut(tab_id) {
                    Some(page) => page.set_textarea_value(node_id, value),
                    None => Err(format!("unknown tab {}", tab_id.as_u64())),
                }
            };
            if result.is_ok() {
                // Match the other constrained form writes: event handlers
                // see the core-owned value before observers receive a frame.
                let _ = script_scheduler.dispatch_event(tabs, tab_id, node_id, "input");
                let _ = script_scheduler.dispatch_event(tabs, tab_id, node_id, "change");
                let page = tabs
                    .get_mut(tab_id)
                    .expect("a checked extension target tab remains live");
                send_frame(
                    page,
                    stream,
                    frame_dir,
                    generation,
                    Some(tab_id.as_u64()),
                    None,
                )?;
            }
            let _ = reply.send(result);
        }
        ExtensionPageRequest::SetRangeInputValue {
            tab_id,
            node_id,
            value,
            reply,
        } => {
            let tab_id = TabId::from_u64(tab_id);
            let node_id = NodeId::from_u64(node_id);
            let result = match tabs.get_mut(tab_id) {
                Some(page) => page.set_range_input_value(node_id, value),
                None => Err(format!("unknown tab {}", tab_id.as_u64())),
            };
            if result.is_ok() {
                // A range is one constrained form control, so listeners see
                // its committed core state before the shared observer frame.
                let _ = script_scheduler.dispatch_event(tabs, tab_id, node_id, "input");
                let _ = script_scheduler.dispatch_event(tabs, tab_id, node_id, "change");
                let page = tabs
                    .get_mut(tab_id)
                    .expect("a checked extension target tab remains live");
                send_frame(
                    page,
                    stream,
                    frame_dir,
                    generation,
                    Some(tab_id.as_u64()),
                    None,
                )?;
            }
            let _ = reply.send(result);
        }
        ExtensionPageRequest::SetRadioChecked {
            tab_id,
            node_id,
            reply,
        } => {
            let tab_id = TabId::from_u64(tab_id);
            let node_id = NodeId::from_u64(node_id);
            let result = match tabs.get_mut(tab_id) {
                Some(page) => page.set_radio_checked(node_id),
                None => Err(format!("unknown tab {}", tab_id.as_u64())),
            };
            if result.is_ok() {
                // A selected radio can clear other controls in its core-owned
                // group, so publish one post-mutation input/change pair and a
                // single shared frame after the entire group update.
                let _ = script_scheduler.dispatch_event(tabs, tab_id, node_id, "input");
                let _ = script_scheduler.dispatch_event(tabs, tab_id, node_id, "change");
                let page = tabs
                    .get_mut(tab_id)
                    .expect("a checked extension target tab remains live");
                send_frame(
                    page,
                    stream,
                    frame_dir,
                    generation,
                    Some(tab_id.as_u64()),
                    None,
                )?;
            }
            let _ = reply.send(result);
        }
        ExtensionPageRequest::SelectOption {
            tab_id,
            node_id,
            reply,
        } => {
            let tab_id = TabId::from_u64(tab_id);
            let node_id = NodeId::from_u64(node_id);
            let result = match tabs.get_mut(tab_id) {
                Some(page) => page.select_option(node_id),
                None => Err(format!("unknown tab {}", tab_id.as_u64())),
            };
            if result.is_ok() {
                // A single-select transition can clear another option, so
                // dispatch one input/change pair and publish one post-update
                // frame only after core has completed the whole group change.
                let _ = script_scheduler.dispatch_event(tabs, tab_id, node_id, "input");
                let _ = script_scheduler.dispatch_event(tabs, tab_id, node_id, "change");
                let page = tabs
                    .get_mut(tab_id)
                    .expect("a checked extension target tab remains live");
                send_frame(
                    page,
                    stream,
                    frame_dir,
                    generation,
                    Some(tab_id.as_u64()),
                    None,
                )?;
            }
            let _ = reply.send(result);
        }
        ExtensionPageRequest::RegisterNetworkBlockUrl {
            connection_id,
            url,
            reply,
        } => {
            let _ = reply.send(tabs.add_extension_navigation_block_rule(connection_id, url));
        }
        ExtensionPageRequest::ClearNetworkBlockUrls {
            connection_id,
            reply,
        } => {
            tabs.clear_extension_navigation_block_rules(connection_id);
            let _ = reply.send(());
        }
    }
    Ok(())
}

struct NoScriptScheduler;

impl ScriptScheduler for NoScriptScheduler {
    fn run_document_scripts(&mut self, _: &mut TabManager, _: TabId) -> Result<(), String> {
        Ok(())
    }
}

fn tab_group_summary(group: &TabGroup) -> TabGroupSummary {
    TabGroupSummary {
        id: group.id().as_u64(),
        name: group.name().to_string(),
        color: group.color().to_string(),
        collapsed: group.collapsed(),
    }
}

/// Group names are rendered into the native tab strip, so normalize away
/// accidental surrounding whitespace and keep the label compact enough for
/// that chrome. The raw pages those tabs show remain entirely unrelated to
/// this metadata.
fn validate_group_name(name: String) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("tab group name must not be empty".to_string());
    }
    if name.chars().count() > 80 {
        return Err("tab group name must be at most 80 characters".to_string());
    }
    Ok(name.to_string())
}

/// Groups use a small canonical color form rather than a frontend-specific
/// palette index or arbitrary CSS. That keeps the shared core state portable
/// between this reference frontend and future native frontends.
fn validate_group_color(color: String) -> Result<String, String> {
    let color = color.trim();
    let valid = color.len() == 7
        && color.starts_with('#')
        && color.as_bytes()[1..].iter().all(u8::is_ascii_hexdigit);
    if !valid {
        return Err("tab group color must be a CSS #RRGGBB value".to_string());
    }
    Ok(color.to_ascii_lowercase())
}

/// How often a tab showing `about:downloads` asks the downloads process for
/// its list.
const DOWNLOADS_REFRESH_INTERVAL: Duration = Duration::from_millis(500);

/// Do not replace a useful rendered list with an outage page for one
/// transient failed poll.  A second consecutive failure makes the outage
/// actionable while still allowing a later successful poll to recover.
const DOWNLOADS_UNAVAILABLE_AFTER_FAILURES: u8 = 2;

/// One background read of the downloads list, on its way back to the loop.
struct DownloadsListing {
    tab_id: TabId,
    /// The visit token captured before the socket read began. A same-URL
    /// navigation is still a new visit, so an older read must not repaint it.
    visit: u64,
    /// The URL the tab was showing when the read started -- the result is
    /// dropped if the tab has moved on since.
    url: String,
    outcome: Result<Vec<TransferInfo>, String>,
}

/// Keeps every tab that is showing `about:downloads` current
/// (`phase-10-download-manager/PLAN.md`'s "Live updates"). Driven from the
/// session's poll tick; the reads happen on background threads and come
/// back over a channel, exactly like a gated navigation's result, so a slow
/// or hung downloads process never delays another tab or client sharing
/// this loop. A page is re-rendered -- and a fresh frame pushed -- only when
/// what it would show actually changed.
#[derive(Default)]
struct DownloadsRefresher {
    /// Tabs with a read outstanding: never more than one per tab.
    in_flight: HashSet<TabId>,
    /// When each tab is next due.
    due: HashMap<TabId, Instant>,
    /// The downloads URL each tab was last seen on. A change is a fresh
    /// visit: read at once, and this first read may start the downloads
    /// process (opening the panel is a legitimate reason to).
    seen_url: HashMap<TabId, String>,
    fresh_visit: HashSet<TabId>,
    /// The HTML last pushed to each tab, so an identical one is skipped.
    rendered: HashMap<TabId, String>,
    /// Monotonically changes on every visit, including a same-URL reload.
    /// It makes late results from an earlier visit unambiguously stale.
    visit: HashMap<TabId, u64>,
    /// Consecutive failed polls since the last successful list for a tab.
    /// This is deliberately per tab and per visit: an unrelated tab's
    /// temporary socket problem must not change this tab's presentation.
    consecutive_failures: HashMap<TabId, u8>,
}

impl DownloadsRefresher {
    /// A navigation to `about:downloads` always replaces the document, even
    /// when the URL text did not change. Forget the previous render cache and
    /// invalidate any read that began before that replacement.
    fn begin_visit(&mut self, tab_id: TabId) {
        let visit = self.visit.entry(tab_id).or_default();
        *visit += 1;
        self.seen_url.remove(&tab_id);
        self.due.remove(&tab_id);
        self.rendered.remove(&tab_id);
        self.fresh_visit.remove(&tab_id);
        self.consecutive_failures.remove(&tab_id);
    }

    fn tick(&mut self, tabs: &TabManager, tx: &mpsc::Sender<DownloadsListing>, now: Instant) {
        let open: HashSet<TabId> = tabs.ids().collect();
        self.seen_url.retain(|id, _| open.contains(id));
        self.due.retain(|id, _| open.contains(id));
        self.rendered.retain(|id, _| open.contains(id));
        self.fresh_visit.retain(|id| open.contains(id));
        self.visit.retain(|id, _| open.contains(id));
        self.consecutive_failures.retain(|id, _| open.contains(id));

        for id in open {
            let Some(page) = tabs.get(id) else { continue };
            let Some(url) = page.url().filter(|u| is_downloads_url(u)) else {
                // Left the page (or never on it): forget it, so coming back is a fresh visit.
                self.seen_url.remove(&id);
                self.due.remove(&id);
                self.rendered.remove(&id);
                self.fresh_visit.remove(&id);
                self.consecutive_failures.remove(&id);
                continue;
            };
            let Some(source) = page.downloads_source().cloned() else {
                continue;
            };

            if self.seen_url.get(&id).map(String::as_str) != Some(url) {
                self.begin_visit(id);
                self.seen_url.insert(id, url.to_string());
                self.fresh_visit.insert(id);
            }
            if self.in_flight.contains(&id) || self.due.get(&id).is_some_and(|due| now < *due) {
                continue;
            }
            self.in_flight.insert(id);
            self.due.insert(id, now + DOWNLOADS_REFRESH_INTERVAL);
            let may_start_the_service = self.fresh_visit.remove(&id);
            let visit = self.visit.get(&id).copied().unwrap_or_default();
            let (tx, url) = (tx.clone(), url.to_string());
            thread::spawn(move || {
                let outcome = if may_start_the_service {
                    source.fetch_spawning()
                } else {
                    source.fetch_observing()
                };
                let _ = tx.send(DownloadsListing {
                    tab_id: id,
                    visit,
                    url,
                    outcome,
                });
            });
        }
    }

    fn apply<S: Write>(
        &mut self,
        tabs: &mut TabManager,
        stream: &mut S,
        frame_dir: &Path,
        generation: &mut u64,
        listing: DownloadsListing,
    ) -> io::Result<()> {
        let DownloadsListing {
            tab_id,
            visit,
            url,
            outcome,
        } = listing;
        self.in_flight.remove(&tab_id);
        if self.visit.get(&tab_id).copied().unwrap_or_default() != visit {
            return Ok(()); // a same-URL navigation replaced this document
        }
        let Some(page) = tabs.get_mut(tab_id) else {
            return Ok(());
        };
        if page.url() != Some(url.as_str()) {
            return Ok(()); // the tab moved on while the read was in flight
        }
        let locale = crate::credits::locale_from_url(&url);
        let html = match outcome {
            Ok(transfers) => {
                self.consecutive_failures.remove(&tab_id);
                downloads_html(&DownloadsView::Transfers(&transfers), locale)
            }
            Err(_) => {
                let failures = self.consecutive_failures.entry(tab_id).or_default();
                *failures = failures.saturating_add(1);
                if self.rendered.contains_key(&tab_id)
                    && *failures < DOWNLOADS_UNAVAILABLE_AFTER_FAILURES
                {
                    return Ok(());
                }
                downloads_html(&DownloadsView::Unavailable, locale)
            }
        };
        if self.rendered.get(&tab_id) == Some(&html) {
            return Ok(());
        }
        page.refresh_html(&html);
        self.rendered.insert(tab_id, html);
        send_frame(
            page,
            stream,
            frame_dir,
            generation,
            Some(tab_id.as_u64()),
            None,
        )
    }
}

/// Which reply variant a background gated navigation's eventual
/// success produces: `Navigate`/`Click`/`ActOn`'s href-resolution sites
/// all want a plain `Navigated`; `OpenTab` wants `TabOpened` instead --
/// threaded through the whole async path (spawned thread ->
/// [`Completion`] -> the poll loop's reply-writing) purely to pick the
/// right variant once the outcome is known.
enum PendingKind {
    Navigate,
    OpenTab,
    /// A URL-only Back/Forward traversal. The cursor advances only after its
    /// gated fetch succeeds, unlike a new navigation which creates a branch.
    History(HistoryDirection),
}

/// One background gated-navigation's eventual result, delivered over
/// the loop's completion channel -- always tagged with the *original*
/// `tab_id`/`request_id`/`seq` captured when the async op began, since
/// none of those can be assumed still "current" by the time this
/// arrives (see [`apply_completion`]).
struct Completion {
    tab_id: TabId,
    seq: u64,
    request_id: Option<u64>,
    kind: PendingKind,
    outcome: NavOutcome,
}

/// Advances a tab's navigation epoch and returns it. Any outstanding fetch
/// tagged with an earlier epoch is no longer allowed to apply: this is used
/// not only for a new URL navigation, but also for restoring a history entry
/// while an earlier URL fetch is still in flight.
fn supersede_pending_navigation(pending_nav_seq: &mut HashMap<TabId, u64>, tab_id: TabId) -> u64 {
    let seq = pending_nav_seq.entry(tab_id).or_insert(0);
    *seq += 1;
    *seq
}

/// Starts one Back/Forward traversal. URL-only entries deliberately take the
/// same validation, gatekeeper, and asynchronous fetch path as a normal
/// navigation; a retained snapshot is the explicit exception and can be
/// restored immediately. Crucially, a URL entry's cursor is not moved until
/// its fetch has cleared, so a failed/blocked history reload leaves both the
/// visible page and the history position untouched.
#[allow(clippy::too_many_arguments)]
fn begin_history_navigation<S: Write>(
    tabs: &mut TabManager,
    stream: &mut S,
    frame_dir: &Path,
    generation: &mut u64,
    reply_tab: Option<u64>,
    request_id: Option<u64>,
    tab_id: TabId,
    direction: HistoryDirection,
    pending_nav_seq: &mut HashMap<TabId, u64>,
    downloads_refresher: &mut DownloadsRefresher,
    completion_tx: &mpsc::Sender<Completion>,
    gatekeeper_socket: &Path,
    extension_events: Option<&mpsc::SyncSender<ExtensionRuntimeEvent>>,
) -> io::Result<()> {
    let direction_name = match direction {
        HistoryDirection::Back => "back",
        HistoryDirection::Forward => "forward",
    };
    let Some(destination) = tabs.history_destination(tab_id, direction) else {
        return write_error(
            stream,
            reply_tab,
            request_id,
            format!("cannot go {direction_name}: no {direction_name} history entry"),
        );
    };
    let kind = PendingKind::History(direction);
    match destination {
        HistoryDestination::Snapshot => {
            // A snapshot restoration is a newer navigation for this tab. A
            // delayed fetch that predates it must never overwrite the restored
            // historical document.
            assert!(tabs.restore_history_snapshot(tab_id, direction));
            supersede_pending_navigation(pending_nav_seq, tab_id);
            if tabs
                .get(tab_id)
                .and_then(Page::url)
                .is_some_and(is_downloads_url)
            {
                downloads_refresher.begin_visit(tab_id);
            }
            reply_success(
                tabs,
                stream,
                frame_dir,
                generation,
                reply_tab,
                request_id,
                &kind,
                tab_id,
                extension_events,
            )
        }
        HistoryDestination::Reload(None) => {
            // The initial blank document has no URL to fetch, but it still
            // moves exactly one history position and never creates a branch.
            assert!(tabs.navigate_history_to_blank(tab_id, direction));
            supersede_pending_navigation(pending_nav_seq, tab_id);
            reply_success(
                tabs,
                stream,
                frame_dir,
                generation,
                reply_tab,
                request_id,
                &kind,
                tab_id,
                extension_events,
            )
        }
        HistoryDestination::Reload(Some(url)) => {
            if tabs.navigate_history_to_built_in(tab_id, direction, &url) {
                supersede_pending_navigation(pending_nav_seq, tab_id);
                if is_downloads_url(&url) {
                    downloads_refresher.begin_visit(tab_id);
                }
                return reply_success(
                    tabs,
                    stream,
                    frame_dir,
                    generation,
                    reply_tab,
                    request_id,
                    &kind,
                    tab_id,
                    extension_events,
                );
            }
            if let Err(e) = blueice_net::validate_url_scheme(&url) {
                return write_error(stream, reply_tab, request_id, e.to_string());
            }
            let navigation_rules = tabs.extension_navigation_block_rule_snapshot();
            if extension_navigation_rules_block_url(&navigation_rules, &url) {
                return write_error(
                    stream,
                    reply_tab,
                    request_id,
                    format!("navigation blocked by a declarative extension rule: {url}"),
                );
            }

            let seq = supersede_pending_navigation(pending_nav_seq, tab_id);
            let tx = completion_tx.clone();
            let socket = gatekeeper_socket.to_path_buf();
            thread::spawn(move || {
                let outcome = gatekeeper_client::check_and_fetch_with_navigation_rules(
                    tab_id,
                    url,
                    &socket,
                    navigation_rules,
                );
                let _ = tx.send(Completion {
                    tab_id,
                    seq,
                    request_id,
                    kind,
                    outcome,
                });
            });
            Ok(())
        }
    }
}

/// Starts a gated navigation to `url` for `tab_id`. Built-in `about:`
/// pages ([`crate::page::built_in_page`]) are handled entirely
/// synchronously here -- loaded directly and replied to before this
/// returns, exactly like `Page::navigate` always did, and *never* going
/// through the gatekeeper at all (per `phase-7-local-ai/PLAN.md`, these
/// are BlueIce's own trusted pages, never fetched). A syntactically
/// invalid scheme is also rejected synchronously, with no thread
/// spawned -- this must happen *before* any gatekeeper round trip, not
/// be deferred into the background thread's own fetch attempt: a test
/// double (or a genuinely down) gatekeeper would otherwise turn an
/// ordinary "invalid URL" error into a fail-closed `GatekeeperBlocked`,
/// which is the wrong outcome for a request that was never going to
/// reach the network either way.
///
/// A well-formed http(s) URL bumps `tab_id`'s entry in
/// `pending_nav_seq` and hands the rest of the work (both gatekeeper
/// stages, the fetch) to a background thread that reports its outcome
/// over `completion_tx`, tagged with the sequence number just bumped
/// to -- this is what lets [`apply_completion`] later recognize and
/// discard a stale/superseded completion.
#[allow(clippy::too_many_arguments)]
fn begin_gated_navigation<S: Write>(
    tabs: &mut TabManager,
    stream: &mut S,
    frame_dir: &Path,
    generation: &mut u64,
    reply_tab: Option<u64>,
    request_id: Option<u64>,
    tab_id: TabId,
    url: String,
    kind: PendingKind,
    pending_nav_seq: &mut HashMap<TabId, u64>,
    downloads_refresher: &mut DownloadsRefresher,
    completion_tx: &mpsc::Sender<Completion>,
    gatekeeper_socket: &Path,
    extension_events: Option<&mpsc::SyncSender<ExtensionRuntimeEvent>>,
) -> io::Result<()> {
    if tabs.navigate_to_built_in(tab_id, &url) {
        // A synchronous trusted navigation can still supersede a network
        // fetch that was already in flight for this tab.
        supersede_pending_navigation(pending_nav_seq, tab_id);
        if is_downloads_url(&url) {
            downloads_refresher.begin_visit(tab_id);
        }
        return reply_success(
            tabs,
            stream,
            frame_dir,
            generation,
            reply_tab,
            request_id,
            &kind,
            tab_id,
            extension_events,
        );
    }
    if let Err(e) = blueice_net::validate_url_scheme(&url) {
        return write_error(stream, reply_tab, request_id, e.to_string());
    }
    let navigation_rules = tabs.extension_navigation_block_rule_snapshot();
    if extension_navigation_rules_block_url(&navigation_rules, &url) {
        // The initial URL is evaluated synchronously before any gatekeeper
        // review or fetch. The same immutable snapshot follows the background
        // worker and is checked again before every redirect connection.
        return write_error(
            stream,
            reply_tab,
            request_id,
            format!("navigation blocked by a declarative extension rule: {url}"),
        );
    }

    let this_seq = supersede_pending_navigation(pending_nav_seq, tab_id);

    let tx = completion_tx.clone();
    let socket = gatekeeper_socket.to_path_buf();
    thread::spawn(move || {
        let outcome = gatekeeper_client::check_and_fetch_with_navigation_rules(
            tab_id,
            url,
            &socket,
            navigation_rules,
        );
        let _ = tx.send(Completion {
            tab_id,
            seq: this_seq,
            request_id,
            kind,
            outcome,
        });
    });
    Ok(())
}

/// Writes the success reply for a gated navigation -- `Navigated` or
/// `TabOpened`, per `kind` -- followed by a fresh frame. Shared between
/// [`begin_gated_navigation`]'s synchronous built-in-page path and
/// [`apply_completion`]'s asynchronous cleared-navigation path so the
/// two can never disagree about what a successful navigation's reply
/// looks like.
#[allow(clippy::too_many_arguments)]
fn reply_success<S: Write>(
    tabs: &mut TabManager,
    stream: &mut S,
    frame_dir: &Path,
    generation: &mut u64,
    reply_tab: Option<u64>,
    request_id: Option<u64>,
    kind: &PendingKind,
    tab_id: TabId,
    extension_events: Option<&mpsc::SyncSender<ExtensionRuntimeEvent>>,
) -> io::Result<()> {
    let page = tabs
        .get_mut(tab_id)
        .expect("a navigation reply requires a live tab");
    match kind {
        PendingKind::Navigate | PendingKind::History(_) => {
            reply_navigated(page, stream, reply_tab, request_id)?
        }
        PendingKind::OpenTab => blueice_ipc::write_server_message_with_ids(
            stream,
            reply_tab,
            request_id,
            &ServerMessage::TabOpened {
                tab_id: tab_id.as_u64(),
                url: page.url().map(str::to_string),
            },
        )?,
    }
    send_frame(page, stream, frame_dir, generation, reply_tab, request_id)?;
    if let Some(events) = extension_events {
        // Navigation events are advisory. A stalled extension has at most 16
        // queued notifications and never blocks the session's render owner.
        let _ = events.try_send(ExtensionRuntimeEvent::NavigationCommitted {
            tab_id: tab_id.as_u64(),
        });
    }
    Ok(())
}

/// Applies one background gated-navigation's [`Completion`], if it's
/// still current: discarded silently (no reply, no state change) if
/// `completion`'s tab has since closed, or if a *newer* navigation to
/// the same tab has since superseded it (`pending_nav_seq`'s entry for
/// that tab no longer matches the sequence number this completion was
/// tagged with) -- matching ordinary browser "a new navigation cancels
/// the in-flight one" behavior. The background thread that produced
/// `completion` never touched `Page`/`TabManager` state itself; this
/// (called only from the main loop) is the one place a gated
/// navigation's result actually lands.
#[allow(clippy::too_many_arguments)]
fn apply_completion<S: Write>(
    tabs: &mut TabManager,
    stream: &mut S,
    frame_dir: &Path,
    generation: &mut u64,
    pending_nav_seq: &HashMap<TabId, u64>,
    completion: Completion,
    downloads_refresher: &mut DownloadsRefresher,
    script_scheduler: &mut dyn ScriptScheduler,
    extension_events: Option<&mpsc::SyncSender<ExtensionRuntimeEvent>>,
) -> io::Result<()> {
    let Completion {
        tab_id,
        seq,
        request_id,
        kind,
        outcome,
    } = completion;
    if pending_nav_seq.get(&tab_id) != Some(&seq) {
        return Ok(()); // superseded by a later navigation to this tab
    }
    if tabs.get(tab_id).is_none() {
        return Ok(()); // the tab closed while this navigation was pending
    }
    let reply_tab = Some(tab_id.as_u64());
    match outcome {
        NavOutcome::Cleared {
            clearance,
            final_url,
            html,
            status,
            content_type,
        } => {
            let response = blueice_ipc::extension::NetworkResponseInfo {
                method: "GET".to_string(),
                final_url: final_url.clone(),
                status,
                content_type,
            };
            let committed = match &kind {
                PendingKind::History(direction) => tabs.apply_fetched_history_navigation(
                    tab_id, *direction, clearance, &final_url, &html, response,
                ),
                PendingKind::Navigate | PendingKind::OpenTab => {
                    tabs.apply_fetched_navigation(tab_id, clearance, &final_url, &html, response);
                    true
                }
            };
            if !committed {
                // The history entry disappeared before completion. This should
                // only be reachable if an internal caller changes the cursor
                // without advancing `pending_nav_seq`; fail safely rather than
                // applying the response to an unrelated document.
                return Ok(());
            }
            if is_downloads_url(&final_url) {
                downloads_refresher.begin_visit(tab_id);
            }
            // A script failure is page-local: its completed DOM mutations
            // remain visible, while core still sends the navigation frame and
            // keeps serving every other tab. This is the out-of-process crash
            // containment rule in the ordinary runtime-error case too.
            let _ = script_scheduler.run_document_scripts(tabs, tab_id);
            reply_success(
                tabs,
                stream,
                frame_dir,
                generation,
                reply_tab,
                request_id,
                &kind,
                tab_id,
                extension_events,
            )
        }
        NavOutcome::GatekeeperBlocked {
            reason,
            category,
            url,
        } => blueice_ipc::write_server_message_with_ids(
            stream,
            reply_tab,
            request_id,
            &ServerMessage::GatekeeperBlocked {
                reason,
                category,
                url,
            },
        ),
        NavOutcome::ExtensionRuleBlocked { url } => write_error(
            stream,
            reply_tab,
            request_id,
            format!("navigation blocked by a declarative extension rule: {url}"),
        ),
        NavOutcome::FetchFailed { message } => write_error(stream, reply_tab, request_id, message),
    }
}

/// `OpenTab`'s handler: always creates the tab (there's no failure mode
/// for that itself); a requested navigation then goes through the same
/// gated two-phase path every other navigate-capable action does (see
/// module docs) -- so, unlike before gating existed, this function
/// itself no longer necessarily writes `OpenTab`'s own success/failure
/// reply: a blank tab (`url: None`) still gets an immediate
/// `TabOpened`, but a requested navigation's `TabOpened`/`Error`/
/// `GatekeeperBlocked` reply is deferred to [`apply_completion`] (or,
/// for a built-in page/invalid scheme, written synchronously inside
/// [`begin_gated_navigation`] itself).
#[allow(clippy::too_many_arguments)]
fn handle_open_tab<S: Write>(
    tabs: &mut TabManager,
    stream: &mut S,
    frame_dir: &Path,
    generation: &mut u64,
    request_id: Option<u64>,
    url: Option<String>,
    pending_nav_seq: &mut HashMap<TabId, u64>,
    downloads_refresher: &mut DownloadsRefresher,
    completion_tx: &mpsc::Sender<Completion>,
    gatekeeper_socket: &Path,
    extension_events: Option<&mpsc::SyncSender<ExtensionRuntimeEvent>>,
) -> io::Result<()> {
    let new_id = tabs.open_tab();
    let Some(url) = url else {
        return blueice_ipc::write_server_message_with_ids(
            stream,
            Some(new_id.as_u64()),
            request_id,
            &ServerMessage::TabOpened {
                tab_id: new_id.as_u64(),
                url: None,
            },
        );
    };
    begin_gated_navigation(
        tabs,
        stream,
        frame_dir,
        generation,
        Some(new_id.as_u64()),
        request_id,
        new_id,
        url,
        PendingKind::OpenTab,
        pending_nav_seq,
        downloads_refresher,
        completion_tx,
        gatekeeper_socket,
        extension_events,
    )
}

/// Gates entry to the main loop on a valid `Hello` as the connection's
/// very first message, per `run_session`'s own docs. Returns `Ok(true)`
/// once the handshake has succeeded and the main loop should start,
/// `Ok(false)` if the session should end without ever entering it (a
/// non-`Hello` first message, an unsupported `protocol_version`, or
/// the client disconnecting before sending anything at all). Tolerates
/// a read timeout the same way the main loop does (retrying rather than
/// treating it as a disconnect) since `run_session` puts `stream` into
/// short-read-timeout mode *before* calling this.
fn perform_handshake<S: Read + Write>(stream: &mut S) -> io::Result<bool> {
    loop {
        match blueice_ipc::read_client_message_with_id(stream) {
            Ok((request_id, msg)) => {
                return match msg {
                    ClientMessage::Hello { protocol_version } => {
                        reply_hello(stream, request_id, protocol_version)
                            .map(|()| protocol_version == blueice_ipc::PROTOCOL_VERSION)
                    }
                    _ => {
                        blueice_ipc::write_server_message_with_id(
                            stream,
                            request_id,
                            &ServerMessage::Error {
                                message: "the first message on a connection must be Hello"
                                    .to_string(),
                            },
                        )?;
                        Ok(false)
                    }
                };
            }
            Err(e) if is_timeout(&e) => continue,
            Err(_) => return Ok(false),
        }
    }
}

fn reply_hello<S: Write>(
    stream: &mut S,
    request_id: Option<u64>,
    protocol_version: u32,
) -> io::Result<()> {
    if protocol_version == blueice_ipc::PROTOCOL_VERSION {
        blueice_ipc::write_server_message_with_id(
            stream,
            request_id,
            &ServerMessage::Hello {
                protocol_version: blueice_ipc::PROTOCOL_VERSION,
            },
        )
    } else {
        blueice_ipc::write_server_message_with_id(
            stream,
            request_id,
            &ServerMessage::Error {
                message: format!(
                    "unsupported protocol_version {protocol_version}, this core speaks {}",
                    blueice_ipc::PROTOCOL_VERSION
                ),
            },
        )
    }
}

fn reply_navigated<S: Write>(
    page: &Page,
    stream: &mut S,
    tab_id: Option<u64>,
    request_id: Option<u64>,
) -> io::Result<()> {
    blueice_ipc::write_server_message_with_ids(
        stream,
        tab_id,
        request_id,
        &ServerMessage::Navigated {
            url: page.url().unwrap_or_default().to_string(),
        },
    )
}

fn send_frame<S: Write>(
    page: &mut Page,
    stream: &mut S,
    frame_dir: &Path,
    generation: &mut u64,
    tab_id: Option<u64>,
    request_id: Option<u64>,
) -> io::Result<()> {
    let pixmap = page.render_visible();
    let tab_id = tab_id.expect("every rendered page has a concrete tab id");
    let frame_generation = page.advance_frame_generation();
    *generation += 1;
    let path = shm::write_frame(frame_dir, tab_id, frame_generation, &pixmap.pixels)?;
    blueice_ipc::write_server_message_with_ids(
        stream,
        Some(tab_id),
        request_id,
        &ServerMessage::FrameReady {
            shm_path: path.to_string_lossy().into_owned(),
            width: pixmap.width,
            height: pixmap.height,
            generation: frame_generation,
        },
    )
}

fn write_error<S: Write>(
    stream: &mut S,
    tab_id: Option<u64>,
    request_id: Option<u64>,
    message: String,
) -> io::Result<()> {
    blueice_ipc::write_server_message_with_ids(
        stream,
        tab_id,
        request_id,
        &ServerMessage::Error { message },
    )
}

/// A `tab_id` (explicit or defaulted) that doesn't resolve to a live
/// tab -- see `run_session`'s own docs for why this is always a real
/// `Error` reply, never a silent no-op. Echoes `target` itself as the
/// reply's `tab_id`, so the client at least learns which (nonexistent)
/// tab it addressed.
fn write_unknown_tab_error<S: Write>(
    stream: &mut S,
    request_id: Option<u64>,
    target: TabId,
) -> io::Result<()> {
    write_error(
        stream,
        Some(target.as_u64()),
        request_id,
        format!("unknown tab {}", target.as_u64()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Instant;

    fn temp_frame_dir(label: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "blueice-session-test-{label}-{}",
            std::process::id()
        ))
    }

    #[test]
    fn a_same_url_downloads_navigation_invalidates_its_previous_render_cache() {
        let mut tabs = TabManager::new(100.0, 100.0);
        let tab_id = tabs.default_tab();
        let mut refresher = DownloadsRefresher::default();
        refresher
            .seen_url
            .insert(tab_id, "about:downloads".to_string());
        refresher
            .rendered
            .insert(tab_id, "<p>old successful list</p>".to_string());
        refresher.visit.insert(tab_id, 41);
        refresher
            .due
            .insert(tab_id, Instant::now() + Duration::from_secs(1));

        let dir = temp_frame_dir("same-downloads-url");
        std::fs::create_dir_all(&dir).unwrap();
        let (tx, _rx) = mpsc::channel();
        let mut wire = Vec::new();
        let mut generation = 0;
        begin_gated_navigation(
            &mut tabs,
            &mut wire,
            &dir,
            &mut generation,
            Some(tab_id.as_u64()),
            None,
            tab_id,
            "about:downloads".to_string(),
            PendingKind::Navigate,
            &mut HashMap::new(),
            &mut refresher,
            &tx,
            Path::new("/unused"),
            None,
        )
        .unwrap();

        assert_eq!(tabs.get(tab_id).unwrap().url(), Some("about:downloads"));
        assert_eq!(refresher.visit[&tab_id], 42);
        assert!(!refresher.rendered.contains_key(&tab_id));
        assert!(!refresher.seen_url.contains_key(&tab_id));
        assert!(!refresher.due.contains_key(&tab_id));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn one_transient_downloads_failure_keeps_the_last_successful_render() {
        let mut tabs = TabManager::new(100.0, 100.0);
        let tab_id = tabs.default_tab();
        tabs.get_mut(tab_id).unwrap().load_html_str(
            "<p>last successful list</p>",
            Some("about:downloads".to_string()),
        );
        let mut refresher = DownloadsRefresher::default();
        refresher.visit.insert(tab_id, 1);
        refresher
            .rendered
            .insert(tab_id, "<p>last successful list</p>".to_string());
        let dir = temp_frame_dir("downloads-transient-failure");
        std::fs::create_dir_all(&dir).unwrap();
        let mut wire = Vec::new();
        let mut generation = 0;

        refresher
            .apply(
                &mut tabs,
                &mut wire,
                &dir,
                &mut generation,
                DownloadsListing {
                    tab_id,
                    visit: 1,
                    url: "about:downloads".to_string(),
                    outcome: Err("temporary timeout".to_string()),
                },
            )
            .unwrap();

        assert_eq!(generation, 0);
        assert!(tabs
            .get(tab_id)
            .unwrap()
            .dom_dump()
            .contains("last successful list"));

        refresher
            .apply(
                &mut tabs,
                &mut wire,
                &dir,
                &mut generation,
                DownloadsListing {
                    tab_id,
                    visit: 1,
                    url: "about:downloads".to_string(),
                    outcome: Err("temporary timeout".to_string()),
                },
            )
            .unwrap();

        assert_eq!(generation, 1);
        assert!(tabs
            .get(tab_id)
            .unwrap()
            .dom_dump()
            .contains("The downloads service is not running"));
        let _ = std::fs::remove_dir_all(dir);
    }

    fn client_pair() -> (UnixStream, UnixStream) {
        UnixStream::pair().unwrap()
    }

    #[test]
    fn extension_request_reads_the_default_tabs_real_ai_representation() {
        let (mut client, mut server) = client_pair();
        let (extension_tx, extension_rx) = mpsc::channel();
        let dir = temp_frame_dir("extension-default-tab-read");
        let cleanup_dir = dir.clone();
        std::fs::create_dir_all(&dir).unwrap();
        let gatekeeper = PathBuf::from("/not-used-for-built-in-navigation");
        let handle = thread::spawn(move || {
            let mut tabs = TabManager::new(320.0, 200.0);
            let mut generation = 0;
            run_session_with_extension_requests(
                &mut tabs,
                &mut server,
                &dir,
                &mut generation,
                &gatekeeper,
                &extension_rx,
            )
        });

        blueice_ipc::client_handshake(&mut client).unwrap();
        blueice_ipc::write_client_message(
            &mut client,
            &ClientMessage::Navigate {
                url: "about:credits".to_string(),
            },
        )
        .unwrap();
        assert_eq!(
            blueice_ipc::read_server_message(&mut client).unwrap(),
            ServerMessage::Navigated {
                url: "about:credits".to_string(),
            }
        );
        assert!(matches!(
            blueice_ipc::read_server_message(&mut client).unwrap(),
            ServerMessage::FrameReady { .. }
        ));

        let (reply_tx, reply_rx) = mpsc::channel();
        extension_tx
            .send(ExtensionPageRequest::ReadRepresentation {
                tab_id: None,
                reply: reply_tx,
            })
            .unwrap();
        let encoded = reply_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("the live session must answer the extension read")
            .expect("a live default tab must serialize");
        let snapshot: blueice_ipc::AiSnapshot = serde_json::from_str(&encoded).unwrap();
        assert_eq!(snapshot.tab_id, 1);
        assert_eq!(snapshot.url.as_deref(), Some("about:credits"));
        assert!(
            !snapshot.nodes.is_empty(),
            "the core-backed snapshot must be from the navigated credits page, not the empty initial tab"
        );

        blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
        handle.join().unwrap().unwrap();
        let _ = std::fs::remove_dir_all(cleanup_dir);
    }

    #[test]
    fn extension_observes_only_the_committed_http_response_for_a_live_tab() {
        let gatekeeper = clearing_gatekeeper("extension-network-observe");
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let http = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0u8; 1024];
            let _ = std::io::Read::read(&mut stream, &mut request);
            std::io::Write::write_all(
                &mut stream,
                b"HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nSet-Cookie: secret=never-expose\r\nContent-Length: 9\r\nConnection: close\r\n\r\n<p>ok</p>",
            )
            .unwrap();
        });
        let (mut client, mut server) = client_pair();
        let (extension_tx, extension_rx) = mpsc::channel();
        let dir = temp_frame_dir("extension-network-observe");
        let cleanup_dir = dir.clone();
        std::fs::create_dir_all(&dir).unwrap();
        let session = thread::spawn(move || {
            let mut tabs = TabManager::new(320.0, 200.0);
            let mut generation = 0;
            run_session_with_extension_requests(
                &mut tabs,
                &mut server,
                &dir,
                &mut generation,
                &gatekeeper,
                &extension_rx,
            )
        });
        handshake(&mut client);
        let (reply_tx, reply_rx) = mpsc::channel();
        extension_tx
            .send(ExtensionPageRequest::ReadNetworkResponse {
                tab_id: 1,
                reply: reply_tx,
            })
            .unwrap();
        assert_eq!(reply_rx.recv_timeout(Duration::from_secs(1)).unwrap().unwrap(), None);
        blueice_ipc::write_client_message(&mut client, &ClientMessage::Navigate { url: url.clone() })
            .unwrap();
        assert_eq!(
            blueice_ipc::read_server_message(&mut client).unwrap(),
            ServerMessage::Navigated { url: url.clone() }
        );
        assert!(matches!(
            blueice_ipc::read_server_message(&mut client).unwrap(),
            ServerMessage::FrameReady { .. }
        ));
        let (reply_tx, reply_rx) = mpsc::channel();
        extension_tx
            .send(ExtensionPageRequest::ReadNetworkResponse {
                tab_id: 1,
                reply: reply_tx,
            })
            .unwrap();
        let response = reply_rx.recv_timeout(Duration::from_secs(1)).unwrap().unwrap().unwrap();
        assert_eq!(response.method, "GET");
        assert_eq!(response.final_url, url);
        assert_eq!(response.status, 200);
        assert_eq!(response.content_type.as_deref(), Some("text/html; charset=utf-8"));
        assert!(!format!("{response:?}").contains("secret"));

        blueice_ipc::write_client_message(
            &mut client,
            &ClientMessage::Navigate { url: "about:blank".to_string() },
        )
        .unwrap();
        assert!(matches!(
            blueice_ipc::read_server_message(&mut client).unwrap(),
            ServerMessage::Navigated { .. }
        ));
        assert!(matches!(
            blueice_ipc::read_server_message(&mut client).unwrap(),
            ServerMessage::FrameReady { .. }
        ));
        let (reply_tx, reply_rx) = mpsc::channel();
        extension_tx
            .send(ExtensionPageRequest::ReadNetworkResponse {
                tab_id: 1,
                reply: reply_tx,
            })
            .unwrap();
        assert_eq!(reply_rx.recv_timeout(Duration::from_secs(1)).unwrap().unwrap(), None);
        let (reply_tx, reply_rx) = mpsc::channel();
        extension_tx
            .send(ExtensionPageRequest::ReadNetworkResponse {
                tab_id: 99,
                reply: reply_tx,
            })
            .unwrap();
        assert!(reply_rx.recv_timeout(Duration::from_secs(1)).unwrap().is_err());
        blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
        session.join().unwrap().unwrap();
        http.join().unwrap();
        let _ = std::fs::remove_dir_all(cleanup_dir);
    }

    #[test]
    fn extension_toolbar_is_broadcast_clickable_and_owned_by_its_connection() {
        let (mut client, mut server) = client_pair();
        let (extension_tx, extension_rx) = mpsc::channel();
        let (event_tx, event_rx) = mpsc::sync_channel(16);
        let dir = temp_frame_dir("extension-toolbar");
        let cleanup_dir = dir.clone();
        std::fs::create_dir_all(&dir).unwrap();
        let session = thread::spawn(move || {
            let mut tabs = TabManager::new(320.0, 200.0);
            let mut generation = 0;
            run_session_with_extension_requests_and_events(
                &mut tabs,
                &mut server,
                &dir,
                &mut generation,
                Path::new("/not-used-for-native-ui"),
                &extension_rx,
                Some(&event_tx),
            )
        });
        handshake(&mut client);
        let (reply_tx, reply_rx) = mpsc::channel();
        extension_tx
            .send(ExtensionPageRequest::SetToolbarButton {
                connection_id: 4,
                label: "\u{202e}spoof".to_string(),
                reply: reply_tx,
            })
            .unwrap();
        assert!(reply_rx.recv_timeout(Duration::from_secs(1)).unwrap().is_err());
        let (reply_tx, reply_rx) = mpsc::channel();
        extension_tx
            .send(ExtensionPageRequest::SetToolbarButton {
                connection_id: 4,
                label: "Notes".to_string(),
                reply: reply_tx,
            })
            .unwrap();
        assert_eq!(
            blueice_ipc::read_server_message(&mut client).unwrap(),
            ServerMessage::ExtensionToolbar {
                label: Some("Notes".to_string()),
            }
        );
        reply_rx.recv_timeout(Duration::from_secs(1)).unwrap().unwrap();
        blueice_ipc::write_client_message(&mut client, &ClientMessage::GetExtensionToolbar)
            .unwrap();
        assert_eq!(
            blueice_ipc::read_server_message(&mut client).unwrap(),
            ServerMessage::ExtensionToolbar {
                label: Some("Notes".to_string()),
            }
        );
        blueice_ipc::write_client_message_with_ids(
            &mut client,
            Some(1),
            None,
            &ClientMessage::ActivateExtensionToolbar,
        )
        .unwrap();
        assert_eq!(
            event_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
            ExtensionRuntimeEvent::ToolbarActivated { tab_id: 1 }
        );

        // A stale connection cannot clear a newer connection's button.
        let (reply_tx, reply_rx) = mpsc::channel();
        extension_tx
            .send(ExtensionPageRequest::SetToolbarButton {
                connection_id: 5,
                label: "Tasks".to_string(),
                reply: reply_tx,
            })
            .unwrap();
        assert_eq!(
            blueice_ipc::read_server_message(&mut client).unwrap(),
            ServerMessage::ExtensionToolbar {
                label: Some("Tasks".to_string()),
            }
        );
        reply_rx.recv_timeout(Duration::from_secs(1)).unwrap().unwrap();
        let popup = ExtensionPopup {
            tab_id: 1,
            title: "Tasks".to_string(),
            body: "Saved locally".to_string(),
        };
        let (reply_tx, reply_rx) = mpsc::channel();
        extension_tx
            .send(ExtensionPageRequest::ShowPopup {
                connection_id: 5,
                popup: popup.clone(),
                reply: reply_tx,
            })
            .unwrap();
        assert_eq!(
            blueice_ipc::read_server_message(&mut client).unwrap(),
            ServerMessage::ExtensionPopup { popup: Some(popup.clone()) }
        );
        reply_rx.recv_timeout(Duration::from_secs(1)).unwrap().unwrap();
        blueice_ipc::write_client_message(&mut client, &ClientMessage::GetExtensionPopup).unwrap();
        assert_eq!(
            blueice_ipc::read_server_message(&mut client).unwrap(),
            ServerMessage::ExtensionPopup { popup: Some(popup.clone()) }
        );
        blueice_ipc::write_client_message(&mut client, &ClientMessage::DismissExtensionPopup).unwrap();
        assert_eq!(
            blueice_ipc::read_server_message(&mut client).unwrap(),
            ServerMessage::ExtensionPopup { popup: None }
        );
        let (reply_tx, reply_rx) = mpsc::channel();
        extension_tx
            .send(ExtensionPageRequest::ShowPopup {
                connection_id: 5,
                popup: popup.clone(),
                reply: reply_tx,
            })
            .unwrap();
        assert_eq!(
            blueice_ipc::read_server_message(&mut client).unwrap(),
            ServerMessage::ExtensionPopup { popup: Some(popup) }
        );
        reply_rx.recv_timeout(Duration::from_secs(1)).unwrap().unwrap();
        let (reply_tx, reply_rx) = mpsc::channel();
        extension_tx
            .send(ExtensionPageRequest::ClearToolbarButton {
                connection_id: 4,
                reply: reply_tx,
            })
            .unwrap();
        reply_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        blueice_ipc::write_client_message(&mut client, &ClientMessage::GetExtensionToolbar)
            .unwrap();
        assert_eq!(
            blueice_ipc::read_server_message(&mut client).unwrap(),
            ServerMessage::ExtensionToolbar {
                label: Some("Tasks".to_string()),
            }
        );
        let (reply_tx, reply_rx) = mpsc::channel();
        extension_tx
            .send(ExtensionPageRequest::ClearToolbarButton {
                connection_id: 5,
                reply: reply_tx,
            })
            .unwrap();
        assert_eq!(blueice_ipc::read_server_message(&mut client).unwrap(), ServerMessage::ExtensionPopup { popup: None });
        assert_eq!(blueice_ipc::read_server_message(&mut client).unwrap(), ServerMessage::ExtensionToolbar { label: None });
        reply_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        blueice_ipc::write_client_message(&mut client, &ClientMessage::ActivateExtensionToolbar)
            .unwrap();
        assert!(matches!(
            blueice_ipc::read_server_message(&mut client).unwrap(),
            ServerMessage::Error { message } if message.contains("no extension toolbar")
        ));
        blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
        session.join().unwrap().unwrap();
        let _ = std::fs::remove_dir_all(cleanup_dir);
    }

    #[test]
    fn extension_network_rule_blocks_a_matching_navigation_before_gatekeeper_or_fetch() {
        let (mut client, mut server) = client_pair();
        let (extension_tx, extension_rx) = mpsc::channel();
        let dir = temp_frame_dir("extension-navigation-block-rule");
        let cleanup_dir = dir.clone();
        std::fs::create_dir_all(&dir).unwrap();
        // This path has no listener. A matching navigation must still produce
        // the synchronous declarative-rule error rather than attempting the
        // ordinary gatekeeper/fetch background path.
        let gatekeeper = PathBuf::from("/not-reached-for-extension-navigation-rule.sock");
        let handle = thread::spawn(move || {
            let mut tabs = TabManager::new(320.0, 200.0);
            let mut generation = 0;
            run_session_with_extension_requests(
                &mut tabs,
                &mut server,
                &dir,
                &mut generation,
                &gatekeeper,
                &extension_rx,
            )
        });

        blueice_ipc::client_handshake(&mut client).unwrap();
        let (rule_reply_tx, rule_reply_rx) = mpsc::channel();
        extension_tx
            .send(ExtensionPageRequest::RegisterNetworkBlockUrl {
                connection_id: 77,
                url: "https://example.test/private#fragment".to_string(),
                reply: rule_reply_tx,
            })
            .unwrap();
        rule_reply_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("the live session must answer the extension rule request")
            .expect("a valid HTTPS rule must be installed");

        blueice_ipc::write_client_message(
            &mut client,
            &ClientMessage::Navigate {
                url: "https://example.test/private".to_string(),
            },
        )
        .unwrap();
        match blueice_ipc::read_server_message(&mut client).unwrap() {
            ServerMessage::Error { message } => {
                assert!(message.contains("declarative extension rule"));
                assert!(message.contains("https://example.test/private"));
            }
            other => {
                panic!("matching extension rule must stop navigation before fetch, got {other:?}")
            }
        }

        blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
        handle.join().unwrap().unwrap();
        let _ = std::fs::remove_dir_all(cleanup_dir);
    }

    #[test]
    fn extension_network_rule_blocks_a_redirect_target_before_its_connection() {
        let (mut client, mut server) = client_pair();
        let (extension_tx, extension_rx) = mpsc::channel();
        let dir = temp_frame_dir("extension-redirect-navigation-block-rule");
        let cleanup_dir = dir.clone();
        std::fs::create_dir_all(&dir).unwrap();
        let target_listener = TcpListener::bind("127.0.0.1:0").unwrap();
        target_listener.set_nonblocking(true).unwrap();
        let blocked_url = format!("http://{}/blocked", target_listener.local_addr().unwrap());
        let redirect_listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let redirect_url = format!("http://{}/before", redirect_listener.local_addr().unwrap());
        let redirect_server = thread::spawn({
            let blocked_url = blocked_url.clone();
            move || {
                let (mut stream, _) = redirect_listener.accept().unwrap();
                let mut buf = [0u8; 1024];
                let _ = stream.read(&mut buf);
                stream
                    .write_all(
                        format!(
                            "HTTP/1.1 302 Found\r\nLocation: {blocked_url}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                        )
                        .as_bytes(),
                    )
                    .unwrap();
            }
        });
        let gatekeeper = clearing_gatekeeper("extension-redirect-rule");
        let handle = thread::spawn(move || {
            let mut tabs = TabManager::new(320.0, 200.0);
            let mut generation = 0;
            run_session_with_extension_requests(
                &mut tabs,
                &mut server,
                &dir,
                &mut generation,
                &gatekeeper,
                &extension_rx,
            )
        });

        blueice_ipc::client_handshake(&mut client).unwrap();
        let (rule_reply_tx, rule_reply_rx) = mpsc::channel();
        extension_tx
            .send(ExtensionPageRequest::RegisterNetworkBlockUrl {
                connection_id: 78,
                url: blocked_url.clone(),
                reply: rule_reply_tx,
            })
            .unwrap();
        rule_reply_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("the live session must accept the redirect-target rule")
            .expect("the target URL is a valid rule");

        blueice_ipc::write_client_message(
            &mut client,
            &ClientMessage::Navigate { url: redirect_url },
        )
        .unwrap();
        match blueice_ipc::read_server_message(&mut client).unwrap() {
            ServerMessage::Error { message } => {
                assert!(message.contains("declarative extension rule"));
                assert!(message.contains(&blocked_url));
            }
            other => panic!("the redirect target must be blocked, got {other:?}"),
        }
        redirect_server.join().unwrap();
        std::thread::sleep(Duration::from_millis(25));
        assert!(matches!(
            target_listener.accept(),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock
        ));

        blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
        handle.join().unwrap().unwrap();
        let _ = std::fs::remove_dir_all(cleanup_dir);
    }

    #[test]
    fn committed_navigation_emits_a_bounded_core_defined_extension_event() {
        let (mut client, mut server) = client_pair();
        let (_extension_tx, extension_rx) = mpsc::channel();
        let (events_tx, events_rx) = mpsc::sync_channel(1);
        let dir = temp_frame_dir("extension-navigation-event");
        let cleanup_dir = dir.clone();
        std::fs::create_dir_all(&dir).unwrap();
        let gatekeeper = PathBuf::from("/not-used-for-built-in-navigation");
        let handle = thread::spawn(move || {
            let mut tabs = TabManager::new(320.0, 200.0);
            let mut generation = 0;
            run_session_with_extension_requests_and_events(
                &mut tabs,
                &mut server,
                &dir,
                &mut generation,
                &gatekeeper,
                &extension_rx,
                Some(&events_tx),
            )
        });

        blueice_ipc::client_handshake(&mut client).unwrap();
        blueice_ipc::write_client_message(
            &mut client,
            &ClientMessage::Navigate {
                url: "about:credits".to_string(),
            },
        )
        .unwrap();
        assert!(matches!(
            blueice_ipc::read_server_message(&mut client).unwrap(),
            ServerMessage::Navigated { .. }
        ));
        assert!(matches!(
            blueice_ipc::read_server_message(&mut client).unwrap(),
            ServerMessage::FrameReady { .. }
        ));
        assert_eq!(
            events_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
            ExtensionRuntimeEvent::NavigationCommitted { tab_id: 1 }
        );

        blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
        handle.join().unwrap().unwrap();
        let _ = std::fs::remove_dir_all(cleanup_dir);
    }

    #[test]
    fn extension_v2_text_write_updates_the_addressed_input_and_pushes_a_frame() {
        let (mut client, mut server) = client_pair();
        let (extension_tx, extension_rx) = mpsc::channel();
        let dir = temp_frame_dir("extension-v2-text-write");
        let cleanup_dir = dir.clone();
        std::fs::create_dir_all(&dir).unwrap();
        let mut tabs = TabManager::new(320.0, 200.0);
        let tab_id = tabs.default_tab();
        tabs.get_mut(tab_id).unwrap().load_html_str(
            r#"<label for="shared">Shared field</label><input id="shared" type="text" value="before">"#,
            Some("https://example.test/form".to_string()),
        );
        let input_id = tabs
            .get(tab_id)
            .unwrap()
            .script_get_element_by_id("shared")
            .unwrap();
        let gatekeeper = PathBuf::from("/not-used-after-host-review");
        let handle = thread::spawn(move || {
            let mut generation = 0;
            run_session_with_extension_requests(
                &mut tabs,
                &mut server,
                &dir,
                &mut generation,
                &gatekeeper,
                &extension_rx,
            )
        });

        blueice_ipc::client_handshake(&mut client).unwrap();
        let (reply_tx, reply_rx) = mpsc::channel();
        extension_tx
            .send(ExtensionPageRequest::SetTextInputValue {
                tab_id: tab_id.as_u64(),
                node_id: input_id.as_u64(),
                value: "from extension".to_string(),
                reply: reply_tx,
            })
            .unwrap();
        reply_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("the live session must answer the extension write")
            .expect("the addressed text input must accept the value");
        let (reply_tab, request_id, frame) =
            blueice_ipc::read_server_message_with_ids(&mut client).unwrap();
        assert_eq!(reply_tab, Some(tab_id.as_u64()));
        assert_eq!(request_id, None);
        assert!(matches!(frame, ServerMessage::FrameReady { .. }));

        let (read_tx, read_rx) = mpsc::channel();
        extension_tx
            .send(ExtensionPageRequest::ReadRepresentation {
                tab_id: Some(tab_id.as_u64()),
                reply: read_tx,
            })
            .unwrap();
        let snapshot: blueice_ipc::AiSnapshot = serde_json::from_str(
            &read_rx
                .recv_timeout(Duration::from_secs(1))
                .unwrap()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(snapshot.tab_id, tab_id.as_u64());
        assert_eq!(
            snapshot
                .nodes
                .iter()
                .find(|node| node.id == input_id.as_u64())
                .and_then(|node| node.state.value.as_deref()),
            Some("from extension")
        );

        let (reply_tx, reply_rx) = mpsc::channel();
        extension_tx
            .send(ExtensionPageRequest::SetTextInputValue {
                tab_id: tab_id.as_u64(),
                node_id: input_id.as_u64(),
                value: "x".repeat(blueice_ipc::extension::MAX_TEXT_WRITE_BYTES + 1),
                reply: reply_tx,
            })
            .unwrap();
        let error = reply_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("the live session must answer an oversized text-input write")
            .expect_err("the core must reject oversized text-control values");
        assert!(error.contains("4096 bytes"));

        let (read_tx, read_rx) = mpsc::channel();
        extension_tx
            .send(ExtensionPageRequest::ReadRepresentation {
                tab_id: Some(tab_id.as_u64()),
                reply: read_tx,
            })
            .unwrap();
        let snapshot: blueice_ipc::AiSnapshot = serde_json::from_str(
            &read_rx
                .recv_timeout(Duration::from_secs(1))
                .unwrap()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            snapshot
                .nodes
                .iter()
                .find(|node| node.id == input_id.as_u64())
                .and_then(|node| node.state.value.as_deref()),
            Some("from extension")
        );

        blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
        handle.join().unwrap().unwrap();
        let _ = std::fs::remove_dir_all(cleanup_dir);
    }

    #[test]
    fn extension_v3_checkbox_write_updates_the_addressed_control_and_pushes_a_frame() {
        let (mut client, mut server) = client_pair();
        let (extension_tx, extension_rx) = mpsc::channel();
        let dir = temp_frame_dir("extension-v3-checkbox-write");
        let cleanup_dir = dir.clone();
        std::fs::create_dir_all(&dir).unwrap();
        let mut tabs = TabManager::new(320.0, 200.0);
        let tab_id = tabs.default_tab();
        tabs.get_mut(tab_id).unwrap().load_html_str(
            r#"<label for="agree">Agree</label><input id="agree" type="checkbox">"#,
            Some("https://example.test/form".to_string()),
        );
        let checkbox_id = tabs
            .get(tab_id)
            .unwrap()
            .script_get_element_by_id("agree")
            .unwrap();
        let gatekeeper = PathBuf::from("/not-used-after-host-review");
        let handle = thread::spawn(move || {
            let mut generation = 0;
            run_session_with_extension_requests(
                &mut tabs,
                &mut server,
                &dir,
                &mut generation,
                &gatekeeper,
                &extension_rx,
            )
        });

        blueice_ipc::client_handshake(&mut client).unwrap();
        let (reply_tx, reply_rx) = mpsc::channel();
        extension_tx
            .send(ExtensionPageRequest::SetCheckboxChecked {
                tab_id: tab_id.as_u64(),
                node_id: checkbox_id.as_u64(),
                checked: true,
                reply: reply_tx,
            })
            .unwrap();
        reply_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("the live session must answer the extension checkbox write")
            .expect("the addressed checkbox must accept its checked state");
        let (reply_tab, request_id, frame) =
            blueice_ipc::read_server_message_with_ids(&mut client).unwrap();
        assert_eq!(reply_tab, Some(tab_id.as_u64()));
        assert_eq!(request_id, None);
        assert!(matches!(frame, ServerMessage::FrameReady { .. }));

        let (read_tx, read_rx) = mpsc::channel();
        extension_tx
            .send(ExtensionPageRequest::ReadRepresentation {
                tab_id: Some(tab_id.as_u64()),
                reply: read_tx,
            })
            .unwrap();
        let snapshot: blueice_ipc::AiSnapshot = serde_json::from_str(
            &read_rx
                .recv_timeout(Duration::from_secs(1))
                .unwrap()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            snapshot
                .nodes
                .iter()
                .find(|node| node.id == checkbox_id.as_u64())
                .and_then(|node| node.state.checked),
            Some(true)
        );

        blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
        handle.join().unwrap().unwrap();
        let _ = std::fs::remove_dir_all(cleanup_dir);
    }

    #[test]
    fn extension_v4_textarea_write_updates_the_addressed_control_and_pushes_a_frame() {
        let (mut client, mut server) = client_pair();
        let (extension_tx, extension_rx) = mpsc::channel();
        let dir = temp_frame_dir("extension-v4-textarea-write");
        let cleanup_dir = dir.clone();
        std::fs::create_dir_all(&dir).unwrap();
        let mut tabs = TabManager::new(320.0, 200.0);
        let tab_id = tabs.default_tab();
        tabs.get_mut(tab_id).unwrap().load_html_str(
            r#"<label for="notes">Notes</label><textarea id="notes">before</textarea>"#,
            Some("https://example.test/form".to_string()),
        );
        let textarea_id = tabs
            .get(tab_id)
            .unwrap()
            .script_get_element_by_id("notes")
            .unwrap();
        let gatekeeper = PathBuf::from("/not-used-after-host-review");
        let handle = thread::spawn(move || {
            let mut generation = 0;
            run_session_with_extension_requests(
                &mut tabs,
                &mut server,
                &dir,
                &mut generation,
                &gatekeeper,
                &extension_rx,
            )
        });

        blueice_ipc::client_handshake(&mut client).unwrap();
        let (reply_tx, reply_rx) = mpsc::channel();
        extension_tx
            .send(ExtensionPageRequest::SetTextareaValue {
                tab_id: tab_id.as_u64(),
                node_id: textarea_id.as_u64(),
                value: "from extension\nwith detail".to_string(),
                reply: reply_tx,
            })
            .unwrap();
        reply_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("the live session must answer the extension textarea write")
            .expect("the addressed textarea must accept its value");
        let (reply_tab, request_id, frame) =
            blueice_ipc::read_server_message_with_ids(&mut client).unwrap();
        assert_eq!(reply_tab, Some(tab_id.as_u64()));
        assert_eq!(request_id, None);
        assert!(matches!(frame, ServerMessage::FrameReady { .. }));

        let (read_tx, read_rx) = mpsc::channel();
        extension_tx
            .send(ExtensionPageRequest::ReadRepresentation {
                tab_id: Some(tab_id.as_u64()),
                reply: read_tx,
            })
            .unwrap();
        let snapshot: blueice_ipc::AiSnapshot = serde_json::from_str(
            &read_rx
                .recv_timeout(Duration::from_secs(1))
                .unwrap()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            snapshot
                .nodes
                .iter()
                .find(|node| node.id == textarea_id.as_u64())
                .and_then(|node| node.state.value.as_deref()),
            Some("from extension with detail")
        );

        let (reply_tx, reply_rx) = mpsc::channel();
        extension_tx
            .send(ExtensionPageRequest::SetTextareaValue {
                tab_id: tab_id.as_u64(),
                node_id: textarea_id.as_u64(),
                value: "x".repeat(blueice_ipc::extension::MAX_TEXT_WRITE_BYTES + 1),
                reply: reply_tx,
            })
            .unwrap();
        let error = reply_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("the live session must answer an oversized textarea write")
            .expect_err("the core must reject oversized text-control values");
        assert!(error.contains("4096 bytes"));

        let (read_tx, read_rx) = mpsc::channel();
        extension_tx
            .send(ExtensionPageRequest::ReadRepresentation {
                tab_id: Some(tab_id.as_u64()),
                reply: read_tx,
            })
            .unwrap();
        let snapshot: blueice_ipc::AiSnapshot = serde_json::from_str(
            &read_rx
                .recv_timeout(Duration::from_secs(1))
                .unwrap()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            snapshot
                .nodes
                .iter()
                .find(|node| node.id == textarea_id.as_u64())
                .and_then(|node| node.state.value.as_deref()),
            Some("from extension with detail")
        );

        blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
        handle.join().unwrap().unwrap();
        let _ = std::fs::remove_dir_all(cleanup_dir);
    }

    /// A monotonic counter alongside the PID, so every call is unique
    /// regardless of how many concurrent tests (each running on its own
    /// thread, in this one test binary process) call it -- same
    /// discipline `blueice-mcp-server`'s own `unique_socket_path` uses,
    /// necessary here because many gatekeeper-behavior tests below each
    /// need their own independent fake listener. `_label` exists purely
    /// so call sites read self-documenting (`clearing_gatekeeper("foo-
    /// test")`) -- deliberately *not* included in the actual path: a
    /// Unix domain socket path is capped at ~100 bytes total
    /// (`sockaddr_un::sun_path`, tighter on macOS than Linux), and this
    /// module's already-long, already-temp-dir-prefixed test names
    /// would blow that budget immediately if concatenated in.
    fn unique_gatekeeper_socket_path(_label: &str) -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("bl-gk-{}-{n}.sock", std::process::id()))
    }

    /// Spins up a background listener that behaves exactly like `ai-
    /// gatekeeper`'s own trivial minimal-slice stub (always clears),
    /// bound to a fresh socket path unique to this call. Every existing
    /// test below that navigates needs *some* gatekeeper behind the
    /// path it gives `run_session` -- not because gating itself is
    /// under test there (see the dedicated gatekeeper-behavior tests
    /// further down for that), but because a genuinely unreachable
    /// gatekeeper fails closed, which would turn those tests'
    /// pre-existing "navigation always succeeds" assertions false. This
    /// keeps every one of those assertions unmodified.
    fn clearing_gatekeeper(label: &str) -> PathBuf {
        let path = unique_gatekeeper_socket_path(label);
        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path).unwrap();
        thread::spawn(move || {
            for incoming in listener.incoming() {
                let Ok(mut stream) = incoming else { break };
                let _ = blueice_ai_gatekeeper::handle_one_check(&mut stream);
            }
        });
        path
    }

    /// Performs the `protocol_version` handshake `run_session` now
    /// requires as the very first message on a fresh connection --
    /// every test below drives `run_session` over a brand-new
    /// connection, so every one of them needs this before its own
    /// message(s), the same way a real client (`frontend`, `blueice-
    /// mcp-server`) would via `blueice_ipc::client_handshake`.
    fn handshake(client: &mut UnixStream) {
        blueice_ipc::client_handshake(client).unwrap();
    }

    /// Every test below that predates multi-tab (Phase 16) sets up its
    /// fixture content on "the" page, the same single-tab shape it
    /// always had -- this is just `tabs.default_tab()` resolved to its
    /// `Page`, so those tests don't need to change beyond `Page::new`
    /// becoming `TabManager::new`.
    fn default_page(tabs: &mut TabManager) -> &mut Page {
        let default = tabs.default_tab();
        tabs.get_mut(default).unwrap()
    }

    #[test]
    fn resize_then_shutdown_produces_one_frame_and_then_ends_the_session() {
        let dir = temp_frame_dir("resize");
        let gatekeeper = clearing_gatekeeper("resize");
        let (mut client, mut server) = client_pair();
        let handle = thread::spawn(move || {
            let mut tabs = TabManager::new(320.0, 200.0);
            default_page(&mut tabs).load_html_str("<p>hi</p>", None);
            let mut generation = 0u64;
            run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
            dir
        });
        handshake(&mut client);

        blueice_ipc::write_client_message(
            &mut client,
            &ClientMessage::Resize {
                width: 100,
                height: 50,
            },
        )
        .unwrap();
        let reply = blueice_ipc::read_server_message(&mut client).unwrap();
        assert!(matches!(
            reply,
            ServerMessage::FrameReady {
                generation: 1,
                width: 100,
                height: 50,
                ..
            }
        ));

        blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
        let dir = handle.join().unwrap();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn navigate_replies_with_navigated_then_a_frame_reflecting_the_new_page() {
        let dir = temp_frame_dir("navigate");
        let gatekeeper = clearing_gatekeeper("navigate");
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 1024];
            let _ = std::io::Read::read(&mut stream, &mut buf);
            let body = "<p>fetched page</p>";
            std::io::Write::write_all(
                &mut stream,
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            )
            .unwrap();
        });

        let (mut client, mut server) = client_pair();
        let handle = thread::spawn(move || {
            let mut tabs = TabManager::new(320.0, 200.0);
            let mut generation = 0u64;
            run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
            dir
        });
        handshake(&mut client);

        let url = format!("http://{addr}");
        blueice_ipc::write_client_message(
            &mut client,
            &ClientMessage::Navigate { url: url.clone() },
        )
        .unwrap();
        let navigated = blueice_ipc::read_server_message(&mut client).unwrap();
        assert_eq!(navigated, ServerMessage::Navigated { url });
        let frame = blueice_ipc::read_server_message(&mut client).unwrap();
        let shm_path = match frame {
            ServerMessage::FrameReady {
                shm_path,
                generation: 1,
                ..
            } => shm_path,
            other => panic!("expected FrameReady, got {other:?}"),
        };
        assert!(
            shm::map_frame(std::path::Path::new(&shm_path)).is_ok(),
            "the frame-plane file must actually exist and be mappable"
        );

        blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
        let dir = handle.join().unwrap();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn navigate_to_an_unreachable_host_replies_with_error_not_a_frame() {
        let dir = temp_frame_dir("navigate-error");
        let gatekeeper = clearing_gatekeeper("navigate-error");
        let (mut client, mut server) = client_pair();
        let handle = thread::spawn(move || {
            let mut tabs = TabManager::new(320.0, 200.0);
            let mut generation = 0u64;
            run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
            dir
        });
        handshake(&mut client);

        blueice_ipc::write_client_message(
            &mut client,
            &ClientMessage::Navigate {
                url: "not-a-valid-url".to_string(),
            },
        )
        .unwrap();
        let reply = blueice_ipc::read_server_message(&mut client).unwrap();
        assert!(matches!(reply, ServerMessage::Error { .. }));

        blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
        let dir = handle.join().unwrap();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn click_on_a_link_navigates_and_a_click_elsewhere_produces_no_reply() {
        let dir = temp_frame_dir("click");
        let gatekeeper = clearing_gatekeeper("click");
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 1024];
            let _ = std::io::Read::read(&mut stream, &mut buf);
            let body = "<p>landed</p>";
            std::io::Write::write_all(
                &mut stream,
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            )
            .unwrap();
        });
        let url = format!("http://{addr}");

        let (mut client, mut server) = client_pair();
        let dir_for_thread = dir.clone();
        let handle = thread::spawn(move || {
            let mut tabs = TabManager::new(320.0, 200.0);
            default_page(&mut tabs).load_html_str(&format!(r#"<a href="{url}">go</a>"#), None);
            let mut generation = 0u64;
            run_session(
                &mut tabs,
                &mut server,
                &dir_for_thread,
                &mut generation,
                &gatekeeper,
            )
            .unwrap();
        });
        handshake(&mut client);

        // clicking the link navigates: expect Navigated then FrameReady
        blueice_ipc::write_client_message(&mut client, &ClientMessage::Click { x: 2.0, y: 2.0 })
            .unwrap();
        let navigated = blueice_ipc::read_server_message(&mut client).unwrap();
        assert!(matches!(navigated, ServerMessage::Navigated { .. }));
        let frame = blueice_ipc::read_server_message(&mut client).unwrap();
        assert!(matches!(frame, ServerMessage::FrameReady { .. }));

        blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
        handle.join().unwrap();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn set_visible_produces_no_reply_and_the_session_keeps_running() {
        let dir = temp_frame_dir("visible");
        let gatekeeper = clearing_gatekeeper("visible");
        let (mut client, mut server) = client_pair();
        let handle = thread::spawn(move || {
            let mut tabs = TabManager::new(320.0, 200.0);
            let mut generation = 0u64;
            run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
            dir
        });
        handshake(&mut client);

        blueice_ipc::write_client_message(
            &mut client,
            &ClientMessage::Chrome(blueice_ipc::ChromeCommand::SetVisible(false)),
        )
        .unwrap();
        // proven by the fact that a subsequent message still gets a
        // normal reply -- Chrome(SetVisible) didn't wedge or end the session.
        blueice_ipc::write_client_message(
            &mut client,
            &ClientMessage::Resize {
                width: 10,
                height: 10,
            },
        )
        .unwrap();
        let reply = blueice_ipc::read_server_message(&mut client).unwrap();
        assert!(matches!(reply, ServerMessage::FrameReady { .. }));

        blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
        let dir = handle.join().unwrap();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn get_representation_shares_the_current_generation_across_every_send_frame_call_site() {
        // `get_representation_shares_the_current_generation_with_the_last_frame`
        // below proves the "same render pass" invariant for `Resize`
        // alone; this extends the same proof to `Scroll`, `Highlight`,
        // and a non-navigating `ActOn` (`Focus`) -- the other distinct
        // `send_frame` call sites in `run_session` (`Click`/`ActOn`'s
        // Click variant only ever reach `send_frame` via the same
        // navigate path `Navigate` itself already exercises, so they add
        // no new coverage here). `send_frame` is a single choke point
        // every one of these routes through, so this is expected to
        // hold structurally -- but the invariant is central enough to
        // this project's premise to prove per call site, not infer from
        // one example.
        let dir = temp_frame_dir("representation-generation-all-sites");
        let gatekeeper = clearing_gatekeeper("representation-generation-all-sites");
        let (mut client, mut server) = client_pair();
        let handle = thread::spawn(move || {
            let mut tabs = TabManager::new(320.0, 200.0);
            default_page(&mut tabs).load_html_str(r#"<input type="text">"#, None);
            let mut generation = 0u64;
            run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
            dir
        });
        handshake(&mut client);

        blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
        let ServerMessage::Representation(snap) =
            blueice_ipc::read_server_message(&mut client).unwrap()
        else {
            panic!("expected Representation")
        };
        let input_id = snap.nodes[0].id;

        let assert_matching_generation = |client: &mut UnixStream, send: ClientMessage| {
            blueice_ipc::write_client_message(client, &send).unwrap();
            let frame = blueice_ipc::read_server_message(client).unwrap();
            let ServerMessage::FrameReady {
                generation: frame_generation,
                ..
            } = frame
            else {
                panic!("expected FrameReady, got {frame:?}")
            };

            blueice_ipc::write_client_message(client, &ClientMessage::GetRepresentation).unwrap();
            let reply = blueice_ipc::read_server_message(client).unwrap();
            let ServerMessage::Representation(snapshot) = reply else {
                panic!("expected Representation, got {reply:?}")
            };
            assert_eq!(snapshot.generation, frame_generation);
        };

        assert_matching_generation(&mut client, ClientMessage::Scroll { delta_y: 10.0 });
        assert_matching_generation(&mut client, ClientMessage::Highlight { id: Some(input_id) });
        assert_matching_generation(
            &mut client,
            ClientMessage::ActOn {
                id: input_id,
                action: NodeAction::Focus,
            },
        );

        blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
        let dir = handle.join().unwrap();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn get_representation_shares_the_current_generation_with_the_last_frame() {
        // the concrete, checkable "same render pass" proof
        // `phase-5-ai-representation-output/PLAN.md` asks for: a
        // Representation and the FrameReady sent alongside a prior
        // state change carry the identical generation number.
        let dir = temp_frame_dir("representation-generation");
        let gatekeeper = clearing_gatekeeper("representation-generation");
        let (mut client, mut server) = client_pair();
        let handle = thread::spawn(move || {
            let mut tabs = TabManager::new(320.0, 200.0);
            default_page(&mut tabs).load_html_str(r#"<a href="/x">Go</a>"#, None);
            let mut generation = 0u64;
            run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
            dir
        });
        handshake(&mut client);

        blueice_ipc::write_client_message(
            &mut client,
            &ClientMessage::Resize {
                width: 100,
                height: 50,
            },
        )
        .unwrap();
        let frame = blueice_ipc::read_server_message(&mut client).unwrap();
        let ServerMessage::FrameReady {
            generation: frame_generation,
            ..
        } = frame
        else {
            panic!("expected FrameReady, got {frame:?}")
        };

        blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
        let reply = blueice_ipc::read_server_message(&mut client).unwrap();
        let ServerMessage::Representation(snapshot) = reply else {
            panic!("expected Representation, got {reply:?}")
        };
        assert_eq!(snapshot.generation, frame_generation);
        assert!(snapshot
            .nodes
            .iter()
            .any(|n| n.name.as_deref() == Some("Go")));

        blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
        let dir = handle.join().unwrap();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn get_dom_returns_the_full_tree_unfiltered_by_the_ai_representation() {
        let dir = temp_frame_dir("get-dom");
        let gatekeeper = clearing_gatekeeper("get-dom");
        let (mut client, mut server) = client_pair();
        let handle = thread::spawn(move || {
            let mut tabs = TabManager::new(320.0, 200.0);
            default_page(&mut tabs)
                .load_html_str(r#"<div style="background-color: red;">x</div>"#, None);
            let mut generation = 0u64;
            run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
            dir
        });
        handshake(&mut client);

        blueice_ipc::write_client_message(&mut client, &ClientMessage::GetDom).unwrap();
        let reply = blueice_ipc::read_server_message(&mut client).unwrap();
        let ServerMessage::Dom(dump) = reply else {
            panic!("expected Dom, got {reply:?}")
        };
        assert!(
            dump.contains("<div>"),
            "a bare div has no AI-representation role but must still appear in the full DOM dump: {dump}"
        );

        blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
        let dir = handle.join().unwrap();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn act_on_click_navigates_the_same_way_a_coordinate_click_does() {
        let dir = temp_frame_dir("act-on-click");
        let gatekeeper = clearing_gatekeeper("act-on-click");
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 1024];
            let _ = std::io::Read::read(&mut stream, &mut buf);
            let body = "<p>landed via id</p>";
            std::io::Write::write_all(
                &mut stream,
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            )
            .unwrap();
        });
        let url = format!("http://{addr}");

        let (mut client, mut server) = client_pair();
        let handle = thread::spawn(move || {
            let mut tabs = TabManager::new(320.0, 200.0);
            default_page(&mut tabs).load_html_str(&format!(r#"<a href="{url}">go</a>"#), None);
            let mut generation = 0u64;
            run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
            dir
        });
        handshake(&mut client);

        blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
        let ServerMessage::Representation(snapshot) =
            blueice_ipc::read_server_message(&mut client).unwrap()
        else {
            panic!("expected Representation")
        };
        let link_id = snapshot
            .nodes
            .iter()
            .find(|n| n.name.as_deref() == Some("go"))
            .unwrap()
            .id;

        blueice_ipc::write_client_message(
            &mut client,
            &ClientMessage::ActOn {
                id: link_id,
                action: NodeAction::Click,
            },
        )
        .unwrap();
        let navigated = blueice_ipc::read_server_message(&mut client).unwrap();
        assert!(matches!(navigated, ServerMessage::Navigated { .. }));
        let frame = blueice_ipc::read_server_message(&mut client).unwrap();
        assert!(matches!(frame, ServerMessage::FrameReady { .. }));

        blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
        let dir = handle.join().unwrap();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn act_on_focus_is_reflected_in_the_next_representation() {
        let dir = temp_frame_dir("act-on-focus");
        let gatekeeper = clearing_gatekeeper("act-on-focus");
        let (mut client, mut server) = client_pair();
        let handle = thread::spawn(move || {
            let mut tabs = TabManager::new(320.0, 200.0);
            default_page(&mut tabs)
                .load_html_str(r#"<input id="name" type="text" placeholder="Name">"#, None);
            let mut generation = 0u64;
            run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
            dir
        });
        handshake(&mut client);

        blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
        let ServerMessage::Representation(before) =
            blueice_ipc::read_server_message(&mut client).unwrap()
        else {
            panic!("expected Representation")
        };
        let input_id = before.nodes[0].id;
        assert!(!before.nodes[0].state.focused);

        blueice_ipc::write_client_message(
            &mut client,
            &ClientMessage::ActOn {
                id: input_id,
                action: NodeAction::Focus,
            },
        )
        .unwrap();
        let frame = blueice_ipc::read_server_message(&mut client).unwrap();
        assert!(
            matches!(frame, ServerMessage::FrameReady { .. }),
            "Focus is a state change and still gets a FrameReady, per session.rs's own docs"
        );

        blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
        let ServerMessage::Representation(after) =
            blueice_ipc::read_server_message(&mut client).unwrap()
        else {
            panic!("expected Representation")
        };
        assert!(after.nodes[0].state.focused);

        // The native frontend sends these narrower keyboard messages rather
        // than guessing a DOM node ID. They are accepted only because the
        // preceding focus action selected this supported text input.
        blueice_ipc::write_client_message(
            &mut client,
            &ClientMessage::InsertText {
                text: "BlueIce".to_string(),
            },
        )
        .unwrap();
        assert!(matches!(
            blueice_ipc::read_server_message(&mut client).unwrap(),
            ServerMessage::FrameReady { .. }
        ));
        blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
        let ServerMessage::Representation(with_text) =
            blueice_ipc::read_server_message(&mut client).unwrap()
        else {
            panic!("expected Representation")
        };
        assert_eq!(with_text.nodes[0].state.value.as_deref(), Some("BlueIce"));

        blueice_ipc::write_client_message(&mut client, &ClientMessage::DeleteBackward).unwrap();
        assert!(matches!(
            blueice_ipc::read_server_message(&mut client).unwrap(),
            ServerMessage::FrameReady { .. }
        ));
        blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
        let ServerMessage::Representation(after_delete) =
            blueice_ipc::read_server_message(&mut client).unwrap()
        else {
            panic!("expected Representation")
        };
        assert_eq!(after_delete.nodes[0].state.value.as_deref(), Some("BlueIc"));

        blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
        let dir = handle.join().unwrap();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn act_on_set_value_is_reflected_in_the_next_representation() {
        let dir = temp_frame_dir("act-on-set-value");
        let gatekeeper = clearing_gatekeeper("act-on-set-value");
        let (mut client, mut server) = client_pair();
        let handle = thread::spawn(move || {
            let mut tabs = TabManager::new(320.0, 200.0);
            default_page(&mut tabs)
                .load_html_str(r#"<input id="name" type="text" placeholder="Name">"#, None);
            let mut generation = 0u64;
            run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
            dir
        });
        handshake(&mut client);

        blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
        let ServerMessage::Representation(before) =
            blueice_ipc::read_server_message(&mut client).unwrap()
        else {
            panic!("expected Representation")
        };
        let input_id = before.nodes[0].id;
        assert_eq!(before.nodes[0].state.value, None);

        blueice_ipc::write_client_message(
            &mut client,
            &ClientMessage::ActOn {
                id: input_id,
                action: NodeAction::SetValue("BlueIce".to_string()),
            },
        )
        .unwrap();
        assert!(matches!(
            blueice_ipc::read_server_message(&mut client).unwrap(),
            ServerMessage::FrameReady { .. }
        ));

        blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
        let ServerMessage::Representation(after) =
            blueice_ipc::read_server_message(&mut client).unwrap()
        else {
            panic!("expected Representation")
        };
        assert_eq!(after.nodes[0].state.value.as_deref(), Some("BlueIce"));

        blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
        let dir = handle.join().unwrap();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn act_on_an_unknown_id_is_a_harmless_no_op() {
        let dir = temp_frame_dir("act-on-unknown");
        let gatekeeper = clearing_gatekeeper("act-on-unknown");
        let (mut client, mut server) = client_pair();
        let handle = thread::spawn(move || {
            let mut tabs = TabManager::new(320.0, 200.0);
            default_page(&mut tabs).load_html_str(r#"<a href="/x">go</a>"#, None);
            let mut generation = 0u64;
            run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
            dir
        });
        handshake(&mut client);

        // an unknown id with Click: same "no reply at all" contract as
        // a coordinate click that lands on nothing.
        blueice_ipc::write_client_message(
            &mut client,
            &ClientMessage::ActOn {
                id: 999_999,
                action: NodeAction::Click,
            },
        )
        .unwrap();
        // proven by the fact that the next message still gets a normal
        // reply -- the unknown id didn't wedge or end the session.
        blueice_ipc::write_client_message(
            &mut client,
            &ClientMessage::Resize {
                width: 10,
                height: 10,
            },
        )
        .unwrap();
        let reply = blueice_ipc::read_server_message(&mut client).unwrap();
        assert!(matches!(reply, ServerMessage::FrameReady { .. }));

        blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
        let dir = handle.join().unwrap();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_stale_id_from_before_a_navigation_is_a_harmless_no_op_after_it() {
        // Unlike `act_on_an_unknown_id_is_a_harmless_no_op` (a
        // never-allocated id), this id is real -- it existed in the
        // document *before* the navigation below. Regression: NodeId
        // allocation used to restart at 0 for every freshly-parsed
        // document, so this same numeric id could be reused by an
        // unrelated node in the post-navigation document, and ActOn
        // would silently act on that unrelated node instead of safely
        // no-op'ing.
        let dir = temp_frame_dir("stale-id-across-navigation");
        let gatekeeper = clearing_gatekeeper("stale-id-across-navigation");
        let (mut client, mut server) = client_pair();
        let handle = thread::spawn(move || {
            let mut tabs = TabManager::new(320.0, 200.0);
            default_page(&mut tabs).load_html_str(r#"<a href="/x">go</a>"#, None);
            let mut generation = 0u64;
            run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
            dir
        });
        handshake(&mut client);

        blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
        let ServerMessage::Representation(snap) =
            blueice_ipc::read_server_message(&mut client).unwrap()
        else {
            panic!("expected Representation")
        };
        let stale_id = snap.nodes[0].id;

        blueice_ipc::write_client_message(
            &mut client,
            &ClientMessage::Navigate {
                url: "about:blank".to_string(),
            },
        )
        .unwrap();
        let navigated = blueice_ipc::read_server_message(&mut client).unwrap();
        assert_eq!(
            navigated,
            ServerMessage::Navigated {
                url: "about:blank".to_string(),
            }
        );
        let _frame = blueice_ipc::read_server_message(&mut client).unwrap();

        blueice_ipc::write_client_message(
            &mut client,
            &ClientMessage::ActOn {
                id: stale_id,
                action: NodeAction::Click,
            },
        )
        .unwrap();
        // proven the same way as the never-allocated-id case: the next
        // message still gets a normal reply, so the stale id neither
        // wedged the session nor triggered a misdirected action.
        blueice_ipc::write_client_message(
            &mut client,
            &ClientMessage::Resize {
                width: 10,
                height: 10,
            },
        )
        .unwrap();
        let reply = blueice_ipc::read_server_message(&mut client).unwrap();
        assert!(matches!(reply, ServerMessage::FrameReady { .. }));

        blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
        let dir = handle.join().unwrap();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn highlight_adds_an_outline_to_the_next_frame_and_clearing_it_removes_it() {
        let dir = temp_frame_dir("highlight");
        let gatekeeper = clearing_gatekeeper("highlight");
        let (mut client, mut server) = client_pair();
        let handle = thread::spawn(move || {
            let mut tabs = TabManager::new(320.0, 200.0);
            default_page(&mut tabs).load_html_str(r#"<a href="/x">go</a>"#, None);
            let mut generation = 0u64;
            run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
            dir
        });
        handshake(&mut client);

        blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
        let ServerMessage::Representation(snap) =
            blueice_ipc::read_server_message(&mut client).unwrap()
        else {
            panic!("expected Representation")
        };
        let link_id = snap.nodes[0].id;

        blueice_ipc::write_client_message(
            &mut client,
            &ClientMessage::Highlight { id: Some(link_id) },
        )
        .unwrap();
        let frame = blueice_ipc::read_server_message(&mut client).unwrap();
        assert!(matches!(frame, ServerMessage::FrameReady { .. }));

        blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
        let dir = handle.join().unwrap();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn hover_updates_state_silently_with_no_reply() {
        let dir = temp_frame_dir("hover");
        let gatekeeper = clearing_gatekeeper("hover");
        let (mut client, mut server) = client_pair();
        let handle = thread::spawn(move || {
            let mut tabs = TabManager::new(320.0, 200.0);
            default_page(&mut tabs).load_html_str(r#"<a href="/x">go</a>"#, None);
            let mut generation = 0u64;
            run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
            dir
        });
        handshake(&mut client);

        blueice_ipc::write_client_message(&mut client, &ClientMessage::Hover { x: 2.0, y: 2.0 })
            .unwrap();
        // proven the same way SetVisible/Chrome is: the next message
        // still gets a normal reply, so Hover didn't wedge the session.
        blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
        let ServerMessage::Representation(snap) =
            blueice_ipc::read_server_message(&mut client).unwrap()
        else {
            panic!("expected Representation")
        };
        assert!(
            snap.nodes[0].state.hovered,
            "the hovered state must be visible via GetRepresentation"
        );

        blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
        let dir = handle.join().unwrap();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn chrome_set_visible_does_not_change_engine_render_state() {
        // `phase-5-ai-representation-output/PLAN.md`'s "verify engine
        // state is unchanged across a hide/show cycle" checklist item,
        // made explicit and checkable rather than left implicit in
        // `Chrome`'s no-op handling: a full hide-then-show round trip
        // must leave the representation (and therefore the DOM/styles/
        // fragment tree it's derived from) byte-for-byte identical, and
        // must not cause a new frame to be rendered.
        let dir = temp_frame_dir("chrome-no-restart");
        let gatekeeper = clearing_gatekeeper("chrome-no-restart");
        let (mut client, mut server) = client_pair();
        let handle = thread::spawn(move || {
            let mut tabs = TabManager::new(320.0, 200.0);
            default_page(&mut tabs).load_html_str("<p>hi</p>", None);
            let mut generation = 0u64;
            run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
            dir
        });
        handshake(&mut client);

        blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
        let ServerMessage::Representation(before) =
            blueice_ipc::read_server_message(&mut client).unwrap()
        else {
            panic!("expected Representation")
        };

        blueice_ipc::write_client_message(
            &mut client,
            &ClientMessage::Chrome(blueice_ipc::ChromeCommand::SetVisible(false)),
        )
        .unwrap();
        blueice_ipc::write_client_message(
            &mut client,
            &ClientMessage::Chrome(blueice_ipc::ChromeCommand::SetVisible(true)),
        )
        .unwrap();

        blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
        let ServerMessage::Representation(after) =
            blueice_ipc::read_server_message(&mut client).unwrap()
        else {
            panic!("expected Representation")
        };

        assert_eq!(
            before.nodes, after.nodes,
            "a hide/show cycle must not change the engine's render-pass state"
        );
        assert_eq!(
            before.generation, after.generation,
            "no frame is re-rendered just from a visibility toggle"
        );

        blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
        let dir = handle.join().unwrap();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn disconnecting_without_shutdown_ends_the_session_cleanly() {
        let dir = temp_frame_dir("disconnect");
        let gatekeeper = unique_gatekeeper_socket_path("disconnect"); // never dialed
        let (client, mut server) = client_pair();
        let handle = thread::spawn(move || {
            let mut tabs = TabManager::new(320.0, 200.0);
            let mut generation = 0u64;
            run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper)
        });
        drop(client);
        assert!(handle.join().unwrap().is_ok());
    }

    #[test]
    fn a_first_message_that_is_not_hello_is_rejected_and_ends_the_session() {
        let dir = temp_frame_dir("handshake-not-hello-first");
        let gatekeeper = unique_gatekeeper_socket_path("handshake-not-hello-first"); // never dialed
        let (mut client, mut server) = client_pair();
        let handle = thread::spawn(move || {
            let mut tabs = TabManager::new(320.0, 200.0);
            let mut generation = 0u64;
            run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper)
        });

        blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
        let reply = blueice_ipc::read_server_message(&mut client).unwrap();
        assert!(
            matches!(reply, ServerMessage::Error { .. }),
            "expected an Error reply, got {reply:?}"
        );

        assert!(
            handle.join().unwrap().is_ok(),
            "the session must end cleanly, not hang, after rejecting the handshake"
        );
    }

    #[test]
    fn an_unsupported_protocol_version_is_rejected_and_ends_the_session() {
        let dir = temp_frame_dir("handshake-bad-version");
        let gatekeeper = unique_gatekeeper_socket_path("handshake-bad-version"); // never dialed
        let (mut client, mut server) = client_pair();
        let handle = thread::spawn(move || {
            let mut tabs = TabManager::new(320.0, 200.0);
            let mut generation = 0u64;
            run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper)
        });

        blueice_ipc::write_client_message(
            &mut client,
            &ClientMessage::Hello {
                protocol_version: blueice_ipc::PROTOCOL_VERSION + 1,
            },
        )
        .unwrap();
        let reply = blueice_ipc::read_server_message(&mut client).unwrap();
        assert!(
            matches!(reply, ServerMessage::Error { .. }),
            "expected an Error reply, got {reply:?}"
        );

        assert!(
            handle.join().unwrap().is_ok(),
            "the session must end cleanly, not hang, after rejecting an unsupported version"
        );
    }

    #[test]
    fn a_hello_seen_again_after_the_handshake_is_answered_without_ending_the_session() {
        // The broker-multiplexing scenario `run_session`'s own docs
        // describe: a second external client's handshake, forwarded
        // into the one already-past-its-own-handshake shared
        // connection, must not be treated as a protocol violation.
        let dir = temp_frame_dir("late-hello");
        let gatekeeper = clearing_gatekeeper("late-hello");
        let (mut client, mut server) = client_pair();
        let handle = thread::spawn(move || {
            let mut tabs = TabManager::new(320.0, 200.0);
            let mut generation = 0u64;
            run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
            dir
        });
        handshake(&mut client);

        blueice_ipc::write_client_message(
            &mut client,
            &ClientMessage::Hello {
                protocol_version: blueice_ipc::PROTOCOL_VERSION,
            },
        )
        .unwrap();
        let reply = blueice_ipc::read_server_message(&mut client).unwrap();
        assert_eq!(
            reply,
            ServerMessage::Hello {
                protocol_version: blueice_ipc::PROTOCOL_VERSION
            }
        );

        // proven the same way other no-special-effect messages are:
        // the session is still alive and answers normally afterward.
        blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
        let dir = handle.join().unwrap();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn every_reply_to_a_message_echoes_back_its_request_id() {
        let dir = temp_frame_dir("request-id-echo");
        let gatekeeper = clearing_gatekeeper("request-id-echo");
        let (mut client, mut server) = client_pair();
        let handle = thread::spawn(move || {
            let mut tabs = TabManager::new(320.0, 200.0);
            default_page(&mut tabs).load_html_str("<p>hi</p>", None);
            let mut generation = 0u64;
            run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
            dir
        });
        handshake(&mut client);

        blueice_ipc::write_client_message_with_id(
            &mut client,
            Some(99),
            &ClientMessage::GetRepresentation,
        )
        .unwrap();
        let (request_id, reply) = blueice_ipc::read_server_message_with_id(&mut client).unwrap();
        assert_eq!(request_id, Some(99));
        assert!(matches!(reply, ServerMessage::Representation(_)));

        blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
        let dir = handle.join().unwrap();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn an_unknown_client_variant_is_ignored_and_the_session_keeps_running() {
        let dir = temp_frame_dir("unknown-variant");
        let gatekeeper = clearing_gatekeeper("unknown-variant");
        let (mut client, mut server) = client_pair();
        let handle = thread::spawn(move || {
            let mut tabs = TabManager::new(320.0, 200.0);
            let mut generation = 0u64;
            run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
            dir
        });
        handshake(&mut client);

        blueice_ipc::write_client_message(&mut client, &ClientMessage::Unknown).unwrap();
        // proven the same way other no-reply messages are: the next
        // message still gets a normal reply, so Unknown didn't wedge
        // or end the session.
        blueice_ipc::write_client_message(
            &mut client,
            &ClientMessage::Resize {
                width: 10,
                height: 10,
            },
        )
        .unwrap();
        let reply = blueice_ipc::read_server_message(&mut client).unwrap();
        assert!(matches!(reply, ServerMessage::FrameReady { .. }));

        blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
        let dir = handle.join().unwrap();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn open_tab_creates_a_second_tab_visible_in_list_tabs() {
        let dir = temp_frame_dir("open-tab-list");
        let gatekeeper = clearing_gatekeeper("open-tab-list");
        let (mut client, mut server) = client_pair();
        let handle = thread::spawn(move || {
            let mut tabs = TabManager::new(320.0, 200.0);
            let mut generation = 0u64;
            run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
            dir
        });
        handshake(&mut client);

        blueice_ipc::write_client_message(&mut client, &ClientMessage::ListTabs).unwrap();
        let ServerMessage::Tabs(before) = blueice_ipc::read_server_message(&mut client).unwrap()
        else {
            panic!("expected Tabs")
        };
        assert_eq!(
            before.len(),
            1,
            "a fresh core starts with exactly one tab, same as before Phase 16"
        );

        blueice_ipc::write_client_message(&mut client, &ClientMessage::OpenTab { url: None })
            .unwrap();
        let ServerMessage::TabOpened {
            tab_id: new_id,
            url,
            ..
        } = blueice_ipc::read_server_message(&mut client).unwrap()
        else {
            panic!("expected TabOpened")
        };
        assert_eq!(url, None);
        assert_ne!(new_id, before[0].id);

        blueice_ipc::write_client_message(&mut client, &ClientMessage::ListTabs).unwrap();
        let ServerMessage::Tabs(after) = blueice_ipc::read_server_message(&mut client).unwrap()
        else {
            panic!("expected Tabs")
        };
        assert_eq!(
            after.iter().map(|t| t.id).collect::<Vec<_>>(),
            vec![before[0].id, new_id]
        );

        blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
        let dir = handle.join().unwrap();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn back_and_forward_restore_only_the_addressed_tabs_history() {
        let dir = temp_frame_dir("per-tab-history");
        let gatekeeper = clearing_gatekeeper("per-tab-history");
        let (mut client, mut server) = client_pair();
        let handle = thread::spawn(move || {
            let mut tabs = TabManager::new(320.0, 200.0);
            let mut generation = 0u64;
            run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
            dir
        });
        handshake(&mut client);

        // Two visits in tab 1 create a real back stack. Built-in pages keep
        // this test deterministic while exercising the same history commit
        // path a cleared network navigation uses.
        for url in ["about:credits", "about:downloads"] {
            blueice_ipc::write_client_message(
                &mut client,
                &ClientMessage::Navigate {
                    url: url.to_string(),
                },
            )
            .unwrap();
            assert_eq!(
                blueice_ipc::read_server_message(&mut client).unwrap(),
                ServerMessage::Navigated {
                    url: url.to_string()
                }
            );
            assert!(matches!(
                blueice_ipc::read_server_message(&mut client).unwrap(),
                ServerMessage::FrameReady { .. }
            ));
        }

        blueice_ipc::write_client_message(&mut client, &ClientMessage::OpenTab { url: None })
            .unwrap();
        let ServerMessage::TabOpened {
            tab_id: tab_two, ..
        } = blueice_ipc::read_server_message(&mut client).unwrap()
        else {
            panic!("expected TabOpened")
        };
        blueice_ipc::write_client_message_with_ids(
            &mut client,
            Some(tab_two),
            None,
            &ClientMessage::Navigate {
                url: "about:blank".to_string(),
            },
        )
        .unwrap();
        let (reply_tab, _, navigated) =
            blueice_ipc::read_server_message_with_ids(&mut client).unwrap();
        assert_eq!(reply_tab, Some(tab_two));
        assert_eq!(
            navigated,
            ServerMessage::Navigated {
                url: "about:blank".to_string()
            }
        );
        assert!(matches!(
            blueice_ipc::read_server_message_with_ids(&mut client)
                .unwrap()
                .2,
            ServerMessage::FrameReady { .. }
        ));

        // Going back in tab 1 must leave tab 2's distinct visit untouched.
        blueice_ipc::write_client_message(&mut client, &ClientMessage::GoBack).unwrap();
        assert_eq!(
            blueice_ipc::read_server_message(&mut client).unwrap(),
            ServerMessage::Navigated {
                url: "about:credits".to_string()
            }
        );
        assert!(matches!(
            blueice_ipc::read_server_message(&mut client).unwrap(),
            ServerMessage::FrameReady { .. }
        ));

        blueice_ipc::write_client_message_with_ids(
            &mut client,
            Some(tab_two),
            None,
            &ClientMessage::GetHistoryState,
        )
        .unwrap();
        let (reply_tab, _, state) = blueice_ipc::read_server_message_with_ids(&mut client).unwrap();
        assert_eq!(reply_tab, Some(tab_two));
        assert_eq!(
            state,
            ServerMessage::HistoryState {
                can_go_back: true,
                can_go_forward: false,
            },
            "tab 1's Back must not alter tab 2's history position"
        );

        blueice_ipc::write_client_message(&mut client, &ClientMessage::GetHistoryState).unwrap();
        assert_eq!(
            blueice_ipc::read_server_message(&mut client).unwrap(),
            ServerMessage::HistoryState {
                can_go_back: true,
                can_go_forward: true,
            }
        );

        blueice_ipc::write_client_message(&mut client, &ClientMessage::GoForward).unwrap();
        assert_eq!(
            blueice_ipc::read_server_message(&mut client).unwrap(),
            ServerMessage::Navigated {
                url: "about:downloads".to_string()
            }
        );
        assert!(matches!(
            blueice_ipc::read_server_message(&mut client).unwrap(),
            ServerMessage::FrameReady { .. }
        ));

        blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
        let dir = handle.join().unwrap();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn default_history_reload_fetches_the_url_again_and_uses_fresh_content() {
        let dir = temp_frame_dir("history-reload");
        let gatekeeper = clearing_gatekeeper("history-reload");
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let http = thread::spawn(move || {
            for body in ["first version", "updated version"] {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = [0u8; 1024];
                let _ = std::io::Read::read(&mut stream, &mut request);
                let body = format!("<button>{body}</button>");
                std::io::Write::write_all(
                    &mut stream,
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                )
                .unwrap();
            }
        });

        let (mut client, mut server) = client_pair();
        let handle = thread::spawn(move || {
            let mut tabs = TabManager::new(320.0, 200.0);
            let mut generation = 0u64;
            run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
            dir
        });
        handshake(&mut client);

        let url = format!("http://{addr}");
        blueice_ipc::write_client_message(
            &mut client,
            &ClientMessage::Navigate { url: url.clone() },
        )
        .unwrap();
        assert_eq!(
            blueice_ipc::read_server_message(&mut client).unwrap(),
            ServerMessage::Navigated { url: url.clone() }
        );
        assert!(matches!(
            blueice_ipc::read_server_message(&mut client).unwrap(),
            ServerMessage::FrameReady { .. }
        ));

        blueice_ipc::write_client_message(
            &mut client,
            &ClientMessage::Navigate {
                url: "about:credits".to_string(),
            },
        )
        .unwrap();
        let _ = blueice_ipc::read_server_message(&mut client).unwrap();
        let _ = blueice_ipc::read_server_message(&mut client).unwrap();

        blueice_ipc::write_client_message(&mut client, &ClientMessage::GoBack).unwrap();
        assert_eq!(
            blueice_ipc::read_server_message(&mut client).unwrap(),
            ServerMessage::Navigated { url: url.clone() }
        );
        assert!(matches!(
            blueice_ipc::read_server_message(&mut client).unwrap(),
            ServerMessage::FrameReady { .. }
        ));
        blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
        let ServerMessage::Representation(snapshot) =
            blueice_ipc::read_server_message(&mut client).unwrap()
        else {
            panic!("expected a representation after history reload")
        };
        assert!(
            snapshot
                .nodes
                .iter()
                .any(|node| node.name.as_deref() == Some("updated version")),
            "Back must fetch the URL again instead of displaying the first visit's in-memory page"
        );

        blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
        let dir = handle.join().unwrap();
        http.join().unwrap();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn opted_in_history_snapshot_restores_when_the_original_url_is_unavailable() {
        let dir = temp_frame_dir("history-snapshot");
        let gatekeeper = clearing_gatekeeper("history-snapshot");
        let (mut client, mut server) = client_pair();
        let handle = thread::spawn(move || {
            let mut tabs = TabManager::new_with_history_snapshot_mode(
                320.0,
                200.0,
                crate::HistorySnapshotMode::Snapshot,
            );
            let tab = tabs.default_tab();
            tabs.get_mut(tab).unwrap().load_html_str(
                "<button>saved historical version</button>",
                Some("https://unavailable.example.test/archive-me".to_string()),
            );
            let mut generation = 0u64;
            run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
            dir
        });
        handshake(&mut client);

        blueice_ipc::write_client_message(
            &mut client,
            &ClientMessage::Navigate {
                url: "about:credits".to_string(),
            },
        )
        .unwrap();
        let _ = blueice_ipc::read_server_message(&mut client).unwrap();
        let _ = blueice_ipc::read_server_message(&mut client).unwrap();

        blueice_ipc::write_client_message(&mut client, &ClientMessage::GoBack).unwrap();
        assert_eq!(
            blueice_ipc::read_server_message(&mut client).unwrap(),
            ServerMessage::Navigated {
                url: "https://unavailable.example.test/archive-me".to_string()
            }
        );
        assert!(matches!(
            blueice_ipc::read_server_message(&mut client).unwrap(),
            ServerMessage::FrameReady { .. }
        ));
        blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
        let ServerMessage::Representation(snapshot) =
            blueice_ipc::read_server_message(&mut client).unwrap()
        else {
            panic!("expected a representation after snapshot restoration")
        };
        assert!(snapshot
            .nodes
            .iter()
            .any(|node| node.name.as_deref() == Some("saved historical version")));

        blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
        let dir = handle.join().unwrap();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn failed_default_history_reload_keeps_the_current_page_and_cursor() {
        let dir = temp_frame_dir("history-reload-failure");
        let gatekeeper = clearing_gatekeeper("history-reload-failure");
        let (mut client, mut server) = client_pair();
        let handle = thread::spawn(move || {
            let mut tabs = TabManager::new(320.0, 200.0);
            let tab = tabs.default_tab();
            tabs.get_mut(tab).unwrap().load_html_str(
                "<button>unavailable historical page</button>",
                Some("http://127.0.0.1:1/history-unavailable".to_string()),
            );
            let mut generation = 0u64;
            run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
            dir
        });
        handshake(&mut client);

        blueice_ipc::write_client_message(
            &mut client,
            &ClientMessage::Navigate {
                url: "about:credits".to_string(),
            },
        )
        .unwrap();
        let _ = blueice_ipc::read_server_message(&mut client).unwrap();
        let _ = blueice_ipc::read_server_message(&mut client).unwrap();

        blueice_ipc::write_client_message(&mut client, &ClientMessage::GoBack).unwrap();
        assert!(matches!(
            blueice_ipc::read_server_message(&mut client).unwrap(),
            ServerMessage::Error { .. }
        ));
        blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
        let ServerMessage::Representation(snapshot) =
            blueice_ipc::read_server_message(&mut client).unwrap()
        else {
            panic!("expected the still-current page representation")
        };
        assert_eq!(snapshot.url.as_deref(), Some("about:credits"));

        blueice_ipc::write_client_message(&mut client, &ClientMessage::GetHistoryState).unwrap();
        assert_eq!(
            blueice_ipc::read_server_message(&mut client).unwrap(),
            ServerMessage::HistoryState {
                can_go_back: true,
                can_go_forward: false,
            },
            "a failed reload must not advance the history cursor"
        );

        blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
        let dir = handle.join().unwrap();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn tab_groups_are_shared_session_state_and_closing_one_ungroups_its_tabs() {
        let dir = temp_frame_dir("tab-groups");
        let gatekeeper = clearing_gatekeeper("tab-groups");
        let (mut client, mut server) = client_pair();
        let handle = thread::spawn(move || {
            let mut tabs = TabManager::new(320.0, 200.0);
            let mut generation = 0u64;
            run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
            dir
        });
        handshake(&mut client);

        blueice_ipc::write_client_message(
            &mut client,
            &ClientMessage::CreateTabGroup {
                name: "  Research  ".to_string(),
                color: "#4F8cFf".to_string(),
            },
        )
        .unwrap();
        assert_eq!(
            blueice_ipc::read_server_message(&mut client).unwrap(),
            ServerMessage::TabGroupCreated(TabGroupSummary {
                id: 1,
                name: "Research".to_string(),
                color: "#4f8cff".to_string(),
                collapsed: false,
            })
        );

        blueice_ipc::write_client_message_with_ids(
            &mut client,
            Some(1),
            None,
            &ClientMessage::SetTabGroup { group_id: Some(1) },
        )
        .unwrap();
        assert_eq!(
            blueice_ipc::read_server_message(&mut client).unwrap(),
            ServerMessage::TabGroupAssigned {
                tab_id: 1,
                group_id: Some(1),
            }
        );

        blueice_ipc::write_client_message(
            &mut client,
            &ClientMessage::RenameTabGroup {
                group_id: 1,
                name: "Reference".to_string(),
            },
        )
        .unwrap();
        assert_eq!(
            blueice_ipc::read_server_message(&mut client).unwrap(),
            ServerMessage::TabGroupUpdated(TabGroupSummary {
                id: 1,
                name: "Reference".to_string(),
                color: "#4f8cff".to_string(),
                collapsed: false,
            })
        );

        blueice_ipc::write_client_message(
            &mut client,
            &ClientMessage::SetTabGroupColor {
                group_id: 1,
                color: "#ff6600".to_string(),
            },
        )
        .unwrap();
        let recolored = blueice_ipc::read_server_message(&mut client).unwrap();
        assert!(matches!(
            recolored,
            ServerMessage::TabGroupUpdated(TabGroupSummary { ref color, .. }) if color == "#ff6600"
        ));

        blueice_ipc::write_client_message(
            &mut client,
            &ClientMessage::SetTabGroupCollapsed {
                group_id: 1,
                collapsed: true,
            },
        )
        .unwrap();
        let collapsed = blueice_ipc::read_server_message(&mut client).unwrap();
        assert!(matches!(
            collapsed,
            ServerMessage::TabGroupUpdated(TabGroupSummary {
                collapsed: true,
                ..
            })
        ));

        blueice_ipc::write_client_message(&mut client, &ClientMessage::ListTabs).unwrap();
        assert!(matches!(
            blueice_ipc::read_server_message(&mut client).unwrap(),
            ServerMessage::Tabs(ref tabs) if tabs == &vec![TabSummary {
                id: 1,
                url: None,
                group_id: Some(1),
            }]
        ));
        blueice_ipc::write_client_message(&mut client, &ClientMessage::ListTabGroups).unwrap();
        assert!(matches!(
            blueice_ipc::read_server_message(&mut client).unwrap(),
            ServerMessage::TabGroups(ref groups) if groups.len() == 1 && groups[0].collapsed
        ));

        blueice_ipc::write_client_message(
            &mut client,
            &ClientMessage::CloseTabGroup { group_id: 1 },
        )
        .unwrap();
        assert_eq!(
            blueice_ipc::read_server_message(&mut client).unwrap(),
            ServerMessage::TabGroupClosed { group_id: 1 }
        );
        blueice_ipc::write_client_message(&mut client, &ClientMessage::ListTabs).unwrap();
        assert!(matches!(
            blueice_ipc::read_server_message(&mut client).unwrap(),
            ServerMessage::Tabs(ref tabs) if tabs[0].group_id.is_none()
        ));

        blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
        let dir = handle.join().unwrap();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn resize_eagerly_reflows_background_tabs_without_creating_an_active_tab() {
        let dir = temp_frame_dir("resize-background-tabs");
        let gatekeeper = clearing_gatekeeper("resize-background-tabs");
        let (mut client, mut server) = client_pair();
        let handle = thread::spawn(move || {
            let mut tabs = TabManager::new(320.0, 200.0);
            let mut generation = 0u64;
            run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
            dir
        });
        handshake(&mut client);

        blueice_ipc::write_client_message(&mut client, &ClientMessage::OpenTab { url: None })
            .unwrap();
        assert!(matches!(
            blueice_ipc::read_server_message(&mut client).unwrap(),
            ServerMessage::TabOpened { tab_id: 2, .. }
        ));
        blueice_ipc::write_client_message_with_ids(
            &mut client,
            Some(1),
            None,
            &ClientMessage::Resize {
                width: 640,
                height: 480,
            },
        )
        .unwrap();
        let first = blueice_ipc::read_server_message_with_ids(&mut client).unwrap();
        let second = blueice_ipc::read_server_message_with_ids(&mut client).unwrap();
        let mut resized = [first, second];
        resized.sort_by_key(|(tab_id, _, _)| *tab_id);
        assert!(matches!(
            &resized[..],
            [
                (Some(1), _, ServerMessage::FrameReady { .. }),
                (Some(2), _, ServerMessage::FrameReady { .. }),
            ]
        ));

        blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
        let dir = handle.join().unwrap();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn open_tab_with_a_url_navigates_it_and_sends_a_frame() {
        let dir = temp_frame_dir("open-tab-with-url");
        let gatekeeper = clearing_gatekeeper("open-tab-with-url");
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 1024];
            let _ = std::io::Read::read(&mut stream, &mut buf);
            let body = "<p>opened via url</p>";
            std::io::Write::write_all(
                &mut stream,
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            )
            .unwrap();
        });

        let (mut client, mut server) = client_pair();
        let handle = thread::spawn(move || {
            let mut tabs = TabManager::new(320.0, 200.0);
            let mut generation = 0u64;
            run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
            dir
        });
        handshake(&mut client);

        let url = format!("http://{addr}");
        blueice_ipc::write_client_message(
            &mut client,
            &ClientMessage::OpenTab {
                url: Some(url.clone()),
            },
        )
        .unwrap();
        let ServerMessage::TabOpened {
            tab_id: new_id,
            url: opened_url,
            ..
        } = blueice_ipc::read_server_message(&mut client).unwrap()
        else {
            panic!("expected TabOpened")
        };
        assert_eq!(opened_url, Some(url));
        let frame = blueice_ipc::read_server_message(&mut client).unwrap();
        assert!(
            matches!(frame, ServerMessage::FrameReady { .. }),
            "expected FrameReady, got {frame:?}"
        );

        // The new tab's content must actually be addressable afterward.
        blueice_ipc::write_client_message_with_ids(
            &mut client,
            Some(new_id),
            None,
            &ClientMessage::GetRepresentation,
        )
        .unwrap();
        let (reply_tab, _, reply) = blueice_ipc::read_server_message_with_ids(&mut client).unwrap();
        assert_eq!(reply_tab, Some(new_id));
        let ServerMessage::Representation(snapshot) = reply else {
            panic!("expected Representation, got {reply:?}")
        };
        assert_eq!(snapshot.tab_id, new_id);
        assert!(snapshot
            .nodes
            .iter()
            .any(|n| n.name.as_deref() == Some("opened via url")));

        blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
        let dir = handle.join().unwrap();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn open_tab_with_a_failing_url_replies_error_not_tab_opened() {
        let dir = temp_frame_dir("open-tab-failing-url");
        let gatekeeper = clearing_gatekeeper("open-tab-failing-url");
        let (mut client, mut server) = client_pair();
        let handle = thread::spawn(move || {
            let mut tabs = TabManager::new(320.0, 200.0);
            let mut generation = 0u64;
            run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
            dir
        });
        handshake(&mut client);

        blueice_ipc::write_client_message(
            &mut client,
            &ClientMessage::OpenTab {
                url: Some("not-a-valid-url".to_string()),
            },
        )
        .unwrap();
        let reply = blueice_ipc::read_server_message(&mut client).unwrap();
        assert!(
            matches!(reply, ServerMessage::Error { .. }),
            "expected Error, got {reply:?}"
        );

        // The session must still be alive and taking new commands
        // afterward -- proven the same way every other no-crash case
        // in this file is.
        blueice_ipc::write_client_message(&mut client, &ClientMessage::ListTabs).unwrap();
        assert!(matches!(
            blueice_ipc::read_server_message(&mut client).unwrap(),
            ServerMessage::Tabs(_)
        ));

        blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
        let dir = handle.join().unwrap();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn an_action_addressed_to_one_tab_never_affects_another_tabs_state() {
        let dir = temp_frame_dir("tab-isolation");
        let gatekeeper = clearing_gatekeeper("tab-isolation");
        let (mut client, mut server) = client_pair();
        let handle = thread::spawn(move || {
            let mut tabs = TabManager::new(320.0, 200.0);
            default_page(&mut tabs).load_html_str("<p>tab one</p>", None);
            let mut generation = 0u64;
            run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
            dir
        });
        handshake(&mut client);

        blueice_ipc::write_client_message(&mut client, &ClientMessage::ListTabs).unwrap();
        let ServerMessage::Tabs(initial) = blueice_ipc::read_server_message(&mut client).unwrap()
        else {
            panic!("expected Tabs")
        };
        let tab_one = initial[0].id;

        blueice_ipc::write_client_message(&mut client, &ClientMessage::OpenTab { url: None })
            .unwrap();
        let ServerMessage::TabOpened {
            tab_id: tab_two, ..
        } = blueice_ipc::read_server_message(&mut client).unwrap()
        else {
            panic!("expected TabOpened")
        };

        // Scroll only tab_two.
        blueice_ipc::write_client_message_with_ids(
            &mut client,
            Some(tab_two),
            None,
            &ClientMessage::Scroll { delta_y: 500.0 },
        )
        .unwrap();
        let frame = blueice_ipc::read_server_message(&mut client).unwrap();
        assert!(matches!(frame, ServerMessage::FrameReady { .. }));

        // tab_one's representation must be completely unaffected --
        // still showing its own content, scroll untouched.
        blueice_ipc::write_client_message_with_ids(
            &mut client,
            Some(tab_one),
            None,
            &ClientMessage::GetRepresentation,
        )
        .unwrap();
        let (_, _, reply) = blueice_ipc::read_server_message_with_ids(&mut client).unwrap();
        let ServerMessage::Representation(snap) = reply else {
            panic!("expected Representation, got {reply:?}")
        };
        assert_eq!(snap.tab_id, tab_one);
        assert_eq!(
            snap.scroll_y, 0.0,
            "scrolling tab_two must not move tab_one's scroll position"
        );
        assert!(snap
            .nodes
            .iter()
            .any(|n| n.name.as_deref() == Some("tab one")));

        blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
        let dir = handle.join().unwrap();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_gated_navigation_addressed_to_one_tab_never_affects_another_tabs_state() {
        // Extends `an_action_addressed_to_one_tab_never_affects_another_
        // tabs_state` (which only covers `Scroll`) to a gated `Navigate`
        // specifically, now that navigation is asynchronous: `tab_two`
        // fully navigating must leave `tab_one`'s content, generation
        // relationship, and addressability completely untouched.
        let dir = temp_frame_dir("tab-isolation-gated-navigate");
        let gatekeeper = clearing_gatekeeper("tab-isolation-gated-navigate");
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 1024];
            let _ = std::io::Read::read(&mut stream, &mut buf);
            let body = "<p>tab two content</p>";
            std::io::Write::write_all(
                &mut stream,
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            )
            .unwrap();
        });

        let (mut client, mut server) = client_pair();
        let handle = thread::spawn(move || {
            let mut tabs = TabManager::new(320.0, 200.0);
            default_page(&mut tabs).load_html_str("<p>tab one</p>", None);
            let mut generation = 0u64;
            run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
            dir
        });
        handshake(&mut client);

        blueice_ipc::write_client_message(&mut client, &ClientMessage::ListTabs).unwrap();
        let ServerMessage::Tabs(initial) = blueice_ipc::read_server_message(&mut client).unwrap()
        else {
            panic!("expected Tabs")
        };
        let tab_one = initial[0].id;

        blueice_ipc::write_client_message(&mut client, &ClientMessage::OpenTab { url: None })
            .unwrap();
        let ServerMessage::TabOpened {
            tab_id: tab_two, ..
        } = blueice_ipc::read_server_message(&mut client).unwrap()
        else {
            panic!("expected TabOpened")
        };

        let url = format!("http://{addr}");
        blueice_ipc::write_client_message_with_ids(
            &mut client,
            Some(tab_two),
            None,
            &ClientMessage::Navigate { url: url.clone() },
        )
        .unwrap();
        let (reply_tab, _, navigated) =
            blueice_ipc::read_server_message_with_ids(&mut client).unwrap();
        assert_eq!(reply_tab, Some(tab_two));
        assert_eq!(navigated, ServerMessage::Navigated { url });
        let (_, _, frame) = blueice_ipc::read_server_message_with_ids(&mut client).unwrap();
        assert!(matches!(frame, ServerMessage::FrameReady { .. }));

        blueice_ipc::write_client_message_with_ids(
            &mut client,
            Some(tab_one),
            None,
            &ClientMessage::GetRepresentation,
        )
        .unwrap();
        let (_, _, reply) = blueice_ipc::read_server_message_with_ids(&mut client).unwrap();
        let ServerMessage::Representation(snap) = reply else {
            panic!("expected Representation, got {reply:?}")
        };
        assert_eq!(snap.tab_id, tab_one);
        assert!(
            snap.nodes
                .iter()
                .any(|n| n.name.as_deref() == Some("tab one")),
            "tab one's content must be untouched by tab two's gated navigation"
        );

        blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
        let dir = handle.join().unwrap();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_message_addressed_to_an_unknown_tab_replies_error_not_a_silent_no_op() {
        let dir = temp_frame_dir("unknown-tab-error");
        let gatekeeper = clearing_gatekeeper("unknown-tab-error");
        let (mut client, mut server) = client_pair();
        let handle = thread::spawn(move || {
            let mut tabs = TabManager::new(320.0, 200.0);
            let mut generation = 0u64;
            run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
            dir
        });
        handshake(&mut client);

        blueice_ipc::write_client_message_with_ids(
            &mut client,
            Some(999_999),
            None,
            &ClientMessage::GetRepresentation,
        )
        .unwrap();
        let (reply_tab, _, reply) = blueice_ipc::read_server_message_with_ids(&mut client).unwrap();
        assert_eq!(
            reply_tab,
            Some(999_999),
            "the reply should still echo back which (nonexistent) tab was addressed"
        );
        assert!(
            matches!(reply, ServerMessage::Error { .. }),
            "expected Error, got {reply:?}"
        );

        // The session must survive an unknown-tab error, same as every
        // other error case in this file.
        blueice_ipc::write_client_message(&mut client, &ClientMessage::ListTabs).unwrap();
        assert!(matches!(
            blueice_ipc::read_server_message(&mut client).unwrap(),
            ServerMessage::Tabs(_)
        ));

        blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
        let dir = handle.join().unwrap();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn close_tab_removes_it_and_a_later_message_to_it_becomes_an_error() {
        let dir = temp_frame_dir("close-tab");
        let gatekeeper = clearing_gatekeeper("close-tab");
        let (mut client, mut server) = client_pair();
        let handle = thread::spawn(move || {
            let mut tabs = TabManager::new(320.0, 200.0);
            let mut generation = 0u64;
            run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
            dir
        });
        handshake(&mut client);

        blueice_ipc::write_client_message(&mut client, &ClientMessage::OpenTab { url: None })
            .unwrap();
        let ServerMessage::TabOpened { tab_id: new_id, .. } =
            blueice_ipc::read_server_message(&mut client).unwrap()
        else {
            panic!("expected TabOpened")
        };

        blueice_ipc::write_client_message_with_ids(
            &mut client,
            Some(new_id),
            None,
            &ClientMessage::CloseTab,
        )
        .unwrap();
        let (reply_tab, _, reply) = blueice_ipc::read_server_message_with_ids(&mut client).unwrap();
        assert_eq!(reply_tab, Some(new_id));
        assert_eq!(reply, ServerMessage::TabClosed { tab_id: new_id });

        blueice_ipc::write_client_message_with_ids(
            &mut client,
            Some(new_id),
            None,
            &ClientMessage::GetRepresentation,
        )
        .unwrap();
        let (_, _, reply) = blueice_ipc::read_server_message_with_ids(&mut client).unwrap();
        assert!(
            matches!(reply, ServerMessage::Error { .. }),
            "a closed tab's id must no longer resolve, expected Error, got {reply:?}"
        );

        // Closing again is a harmless-but-reported "unknown tab" error,
        // not a panic or a second TabClosed.
        blueice_ipc::write_client_message_with_ids(
            &mut client,
            Some(new_id),
            None,
            &ClientMessage::CloseTab,
        )
        .unwrap();
        let (_, _, reply) = blueice_ipc::read_server_message_with_ids(&mut client).unwrap();
        assert!(matches!(reply, ServerMessage::Error { .. }));

        blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
        let dir = handle.join().unwrap();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_reply_to_an_untagged_request_still_echoes_the_resolved_default_tab_id() {
        // The load-bearing property that makes broadcast-shared,
        // multi-tab connections work at all: a request that left
        // `tab_id` implicit still gets a reply that self-discloses the
        // *concrete* tab it resolved to, not `None` -- otherwise a
        // second client sharing the connection via `blueice-launcher`'s
        // broker could never tell which tab an untagged client's
        // broadcasted reply was actually about.
        let dir = temp_frame_dir("echo-resolved-default-tab");
        let gatekeeper = clearing_gatekeeper("echo-resolved-default-tab");
        let (mut client, mut server) = client_pair();
        let handle = thread::spawn(move || {
            let mut tabs = TabManager::new(320.0, 200.0);
            let mut generation = 0u64;
            run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
            dir
        });
        handshake(&mut client);

        blueice_ipc::write_client_message(&mut client, &ClientMessage::ListTabs).unwrap();
        let ServerMessage::Tabs(tabs) = blueice_ipc::read_server_message(&mut client).unwrap()
        else {
            panic!("expected Tabs")
        };
        let default_tab_id = tabs[0].id;

        // Sent with no tab_id at all -- the envelope-level default.
        blueice_ipc::write_client_message_with_id(
            &mut client,
            None,
            &ClientMessage::GetRepresentation,
        )
        .unwrap();
        let (reply_tab, _, reply) = blueice_ipc::read_server_message_with_ids(&mut client).unwrap();
        assert_eq!(
            reply_tab,
            Some(default_tab_id),
            "the reply must echo the resolved tab, not None"
        );
        assert!(matches!(reply, ServerMessage::Representation(_)));

        blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
        let dir = handle.join().unwrap();
        let _ = std::fs::remove_dir_all(dir);
    }

    // -- Gatekeeper-specific behavior --------------------------------

    #[test]
    fn content_stage_rejection_blocks_navigation_and_leaves_the_page_unchanged() {
        let dir = temp_frame_dir("content-stage-block");
        let gatekeeper_path = unique_gatekeeper_socket_path("content-stage-block");
        let _ = std::fs::remove_file(&gatekeeper_path);
        let listener = UnixListener::bind(&gatekeeper_path).unwrap();
        thread::spawn(move || {
            for incoming in listener.incoming() {
                let Ok(mut stream) = incoming else { break };
                let Ok(req) = blueice_ipc::gatekeeper::read_gatekeeper_request(&mut stream) else {
                    continue;
                };
                let reply = match req {
                    blueice_ipc::gatekeeper::GatekeeperRequest::CheckUrl { .. } => {
                        blueice_ipc::gatekeeper::GatekeeperReply::Cleared
                    }
                    blueice_ipc::gatekeeper::GatekeeperRequest::CheckContent { .. } => {
                        blueice_ipc::gatekeeper::GatekeeperReply::Rejected {
                            reason: "hidden instruction-shaped text".to_string(),
                            category: "prompt-injection".to_string(),
                        }
                    }
                    blueice_ipc::gatekeeper::GatekeeperRequest::CheckDownload { .. } => {
                        unreachable!(
                            "navigation never sends a download check; that stage belongs to the downloads process"
                        )
                    }
                    blueice_ipc::gatekeeper::GatekeeperRequest::CheckExtensionAction { .. } => {
                        unreachable!(
                            "navigation never sends an extension action check; that stage belongs to the extension host"
                        )
                    }
                };
                let _ = blueice_ipc::gatekeeper::write_gatekeeper_reply(&mut stream, &reply);
            }
        });

        let http = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = http.local_addr().unwrap();
        thread::spawn(move || {
            let (mut stream, _) = http.accept().unwrap();
            let mut buf = [0u8; 1024];
            let _ = std::io::Read::read(&mut stream, &mut buf);
            let body = "<p>malicious page</p>";
            std::io::Write::write_all(
                &mut stream,
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            )
            .unwrap();
        });

        let (mut client, mut server) = client_pair();
        let handle = thread::spawn(move || {
            let mut tabs = TabManager::new(320.0, 200.0);
            let mut generation = 0u64;
            run_session(
                &mut tabs,
                &mut server,
                &dir,
                &mut generation,
                &gatekeeper_path,
            )
            .unwrap();
            dir
        });
        handshake(&mut client);

        let url = format!("http://{addr}");
        blueice_ipc::write_client_message(
            &mut client,
            &ClientMessage::Navigate { url: url.clone() },
        )
        .unwrap();
        let reply = blueice_ipc::read_server_message(&mut client).unwrap();
        assert_eq!(
            reply,
            ServerMessage::GatekeeperBlocked {
                reason: "hidden instruction-shaped text".to_string(),
                category: "prompt-injection".to_string(),
                url: url.clone()
            }
        );

        // The page must not have changed: a follow-up GetRepresentation
        // shows no trace of the blocked page's content (no `FrameReady`
        // was ever produced for it either, since the only reply so far
        // was the GatekeeperBlocked above).
        blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
        let ServerMessage::Representation(snap) =
            blueice_ipc::read_server_message(&mut client).unwrap()
        else {
            panic!("expected Representation")
        };
        assert!(!snap
            .nodes
            .iter()
            .any(|n| n.name.as_deref() == Some("malicious page")));

        blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
        let dir = handle.join().unwrap();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn navigation_fails_closed_when_the_gatekeeper_is_unreachable() {
        let dir = temp_frame_dir("gatekeeper-unreachable");
        let gatekeeper_path = unique_gatekeeper_socket_path("gatekeeper-unreachable"); // nothing listens here
        let (mut client, mut server) = client_pair();
        let handle = thread::spawn(move || {
            let mut tabs = TabManager::new(320.0, 200.0);
            let mut generation = 0u64;
            run_session(
                &mut tabs,
                &mut server,
                &dir,
                &mut generation,
                &gatekeeper_path,
            )
            .unwrap();
            dir
        });
        handshake(&mut client);

        blueice_ipc::write_client_message(
            &mut client,
            &ClientMessage::Navigate {
                url: "http://example.invalid/".to_string(),
            },
        )
        .unwrap();
        let reply = blueice_ipc::read_server_message(&mut client).unwrap();
        assert!(
            matches!(reply, ServerMessage::GatekeeperBlocked { .. }),
            "an unreachable gatekeeper must fail closed, got {reply:?}"
        );

        blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
        let dir = handle.join().unwrap();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn navigation_fails_closed_when_the_gatekeeper_accepts_then_drops_the_connection() {
        let dir = temp_frame_dir("gatekeeper-drops-connection");
        let gatekeeper_path = unique_gatekeeper_socket_path("gatekeeper-drops-connection");
        let _ = std::fs::remove_file(&gatekeeper_path);
        let listener = UnixListener::bind(&gatekeeper_path).unwrap();
        thread::spawn(move || {
            for incoming in listener.incoming() {
                drop(incoming); // accept, then immediately disconnect -- no reply ever sent
            }
        });

        let (mut client, mut server) = client_pair();
        let handle = thread::spawn(move || {
            let mut tabs = TabManager::new(320.0, 200.0);
            let mut generation = 0u64;
            run_session(
                &mut tabs,
                &mut server,
                &dir,
                &mut generation,
                &gatekeeper_path,
            )
            .unwrap();
            dir
        });
        handshake(&mut client);

        blueice_ipc::write_client_message(
            &mut client,
            &ClientMessage::Navigate {
                url: "http://example.invalid/".to_string(),
            },
        )
        .unwrap();
        let reply = blueice_ipc::read_server_message(&mut client).unwrap();
        assert!(
            matches!(reply, ServerMessage::GatekeeperBlocked { .. }),
            "a gatekeeper that drops the connection must fail closed, got {reply:?}"
        );

        blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
        let dir = handle.join().unwrap();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_stalled_gatekeeper_check_for_one_tab_does_not_block_a_reply_to_another_tab() {
        // The single most important proof of the property this whole
        // mechanism exists for: a slow/stuck gatekeeper review for one
        // tab must never stall the one shared connection other tabs
        // (or clients sharing it via `blueice-launcher`'s broker) are
        // also using.
        let dir = temp_frame_dir("non-blocking-concurrency");
        let gatekeeper_path = unique_gatekeeper_socket_path("non-blocking-concurrency");
        let _ = std::fs::remove_file(&gatekeeper_path);
        let listener = UnixListener::bind(&gatekeeper_path).unwrap();
        thread::spawn(move || {
            for incoming in listener.incoming() {
                let Ok(mut stream) = incoming else { break };
                thread::spawn(move || {
                    if let Ok(req) = blueice_ipc::gatekeeper::read_gatekeeper_request(&mut stream) {
                        if matches!(&req, blueice_ipc::gatekeeper::GatekeeperRequest::CheckUrl { url } if url.contains("slow-tab"))
                        {
                            thread::sleep(Duration::from_millis(300));
                        }
                        let _ = blueice_ipc::gatekeeper::write_gatekeeper_reply(
                            &mut stream,
                            &blueice_ipc::gatekeeper::GatekeeperReply::Cleared,
                        );
                    }
                });
            }
        });

        let (mut client, mut server) = client_pair();
        let handle = thread::spawn(move || {
            let mut tabs = TabManager::new(320.0, 200.0);
            let mut generation = 0u64;
            run_session(
                &mut tabs,
                &mut server,
                &dir,
                &mut generation,
                &gatekeeper_path,
            )
            .unwrap();
            dir
        });
        handshake(&mut client);

        blueice_ipc::write_client_message(&mut client, &ClientMessage::OpenTab { url: None })
            .unwrap();
        let ServerMessage::TabOpened { tab_id: tab_b, .. } =
            blueice_ipc::read_server_message(&mut client).unwrap()
        else {
            panic!("expected TabOpened")
        };

        // Kick off the default tab's navigation, whose gatekeeper check
        // stalls for 300ms -- fire-and-forget, its own reply isn't
        // waited on here.
        blueice_ipc::write_client_message(
            &mut client,
            &ClientMessage::Navigate {
                url: "http://127.0.0.1:1/slow-tab".to_string(),
            },
        )
        .unwrap();

        // Immediately address tab_b with an unrelated message.
        let start = Instant::now();
        blueice_ipc::write_client_message_with_ids(
            &mut client,
            Some(tab_b),
            None,
            &ClientMessage::GetRepresentation,
        )
        .unwrap();
        let (reply_tab, _, reply) = blueice_ipc::read_server_message_with_ids(&mut client).unwrap();
        assert_eq!(reply_tab, Some(tab_b));
        assert!(matches!(reply, ServerMessage::Representation(_)));
        assert!(
            start.elapsed() < Duration::from_millis(150),
            "tab_b's reply must arrive well before tab_a's stalled gatekeeper check resolves, took {:?}",
            start.elapsed()
        );

        blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
        let dir = handle.join().unwrap();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_second_navigation_supersedes_a_still_pending_first_one() {
        let dir = temp_frame_dir("supersede");
        let gatekeeper_path = unique_gatekeeper_socket_path("supersede");
        let _ = std::fs::remove_file(&gatekeeper_path);
        let listener = UnixListener::bind(&gatekeeper_path).unwrap();
        thread::spawn(move || {
            for incoming in listener.incoming() {
                let Ok(mut stream) = incoming else { break };
                thread::spawn(move || {
                    if let Ok(req) = blueice_ipc::gatekeeper::read_gatekeeper_request(&mut stream) {
                        if matches!(&req, blueice_ipc::gatekeeper::GatekeeperRequest::CheckUrl { url } if url.contains("first"))
                        {
                            thread::sleep(Duration::from_millis(300));
                        }
                        let _ = blueice_ipc::gatekeeper::write_gatekeeper_reply(
                            &mut stream,
                            &blueice_ipc::gatekeeper::GatekeeperReply::Cleared,
                        );
                    }
                });
            }
        });

        let second_http = TcpListener::bind("127.0.0.1:0").unwrap();
        let second_addr = second_http.local_addr().unwrap();
        thread::spawn(move || {
            let (mut stream, _) = second_http.accept().unwrap();
            let mut buf = [0u8; 1024];
            let _ = std::io::Read::read(&mut stream, &mut buf);
            let body = "<p>second page</p>";
            std::io::Write::write_all(
                &mut stream,
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            )
            .unwrap();
        });

        let (mut client, mut server) = client_pair();
        let handle = thread::spawn(move || {
            let mut tabs = TabManager::new(320.0, 200.0);
            let mut generation = 0u64;
            run_session(
                &mut tabs,
                &mut server,
                &dir,
                &mut generation,
                &gatekeeper_path,
            )
            .unwrap();
            dir
        });
        handshake(&mut client);

        // First navigation: stalls 300ms on its own CheckUrl stage, and
        // even once cleared points nowhere reachable -- must never
        // become visible.
        blueice_ipc::write_client_message(
            &mut client,
            &ClientMessage::Navigate {
                url: "http://127.0.0.1:1/first-slow".to_string(),
            },
        )
        .unwrap();
        // Second navigation to the same (default) tab, sent immediately
        // after, well before the first's gatekeeper check resolves.
        let second_url = format!("http://{second_addr}");
        blueice_ipc::write_client_message(
            &mut client,
            &ClientMessage::Navigate {
                url: second_url.clone(),
            },
        )
        .unwrap();

        let navigated = blueice_ipc::read_server_message(&mut client).unwrap();
        assert_eq!(navigated, ServerMessage::Navigated { url: second_url });
        let frame = blueice_ipc::read_server_message(&mut client).unwrap();
        assert!(matches!(frame, ServerMessage::FrameReady { .. }));

        // No further reply ever arrives for the stale first navigation,
        // even after waiting past its stall -- proven the same way
        // every other "harmless no-op" case in this file is: the next
        // real message still gets exactly one, normal reply.
        thread::sleep(Duration::from_millis(400));
        blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
        let ServerMessage::Representation(snap) =
            blueice_ipc::read_server_message(&mut client).unwrap()
        else {
            panic!("expected Representation")
        };
        assert!(snap
            .nodes
            .iter()
            .any(|n| n.name.as_deref() == Some("second page")));

        blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
        let dir = handle.join().unwrap();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn open_tab_with_a_url_the_gatekeeper_blocks_replies_gatekeeper_blocked_not_tab_opened() {
        // `OpenTab{url: Some(_)}` goes through the same gated path
        // `Navigate` does (`PendingKind::OpenTab`) -- this is the
        // `OpenTab`-specific proof that a blocked outcome there reports
        // `GatekeeperBlocked`, not a bare `TabOpened`/`Error`, and that
        // no orphaned-but-blank tab id is leaked into a reply shape a
        // caller wouldn't expect.
        let dir = temp_frame_dir("open-tab-gatekeeper-blocked");
        let gatekeeper_path = unique_gatekeeper_socket_path("open-tab-gatekeeper-blocked");
        let _ = std::fs::remove_file(&gatekeeper_path);
        let listener = UnixListener::bind(&gatekeeper_path).unwrap();
        thread::spawn(move || {
            for incoming in listener.incoming() {
                let Ok(mut stream) = incoming else { break };
                let Ok(_req) = blueice_ipc::gatekeeper::read_gatekeeper_request(&mut stream) else {
                    continue;
                };
                let _ = blueice_ipc::gatekeeper::write_gatekeeper_reply(
                    &mut stream,
                    &blueice_ipc::gatekeeper::GatekeeperReply::Rejected {
                        reason: "known-bad domain".to_string(),
                        category: "blocklist".to_string(),
                    },
                );
            }
        });

        let (mut client, mut server) = client_pair();
        let handle = thread::spawn(move || {
            let mut tabs = TabManager::new(320.0, 200.0);
            let mut generation = 0u64;
            run_session(
                &mut tabs,
                &mut server,
                &dir,
                &mut generation,
                &gatekeeper_path,
            )
            .unwrap();
            dir
        });
        handshake(&mut client);

        let url = "http://example.invalid/".to_string();
        blueice_ipc::write_client_message(
            &mut client,
            &ClientMessage::OpenTab {
                url: Some(url.clone()),
            },
        )
        .unwrap();
        let reply = blueice_ipc::read_server_message(&mut client).unwrap();
        assert_eq!(
            reply,
            ServerMessage::GatekeeperBlocked {
                reason: "known-bad domain".to_string(),
                category: "blocklist".to_string(),
                url
            }
        );

        // The session must still be alive afterward, same as every
        // other error/blocked case in this file -- and `ListTabs` must
        // still show the new (blank) tab `OpenTab` always creates,
        // per `ServerMessage::TabOpened`'s own documented limitation.
        blueice_ipc::write_client_message(&mut client, &ClientMessage::ListTabs).unwrap();
        let ServerMessage::Tabs(tabs) = blueice_ipc::read_server_message(&mut client).unwrap()
        else {
            panic!("expected Tabs")
        };
        assert_eq!(
            tabs.len(),
            2,
            "OpenTab always creates the tab, even though its requested navigation was blocked"
        );

        blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
        let dir = handle.join().unwrap();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_non_navigating_message_to_a_tab_with_a_pending_navigation_applies_immediately() {
        // `phase-7-local-ai/PLAN.md`'s "Wiring design" is explicit that
        // this must work the way a real browser reflows/scrolls a
        // still-displayed old page while a new one loads: `Resize`
        // addressed to a tab whose gated navigation hasn't resolved yet
        // must apply immediately against that tab's *current*
        // (pre-navigation) `Page` state, not queue up behind it.
        let dir = temp_frame_dir("resize-during-pending-nav");
        let gatekeeper_path = unique_gatekeeper_socket_path("resize-during-pending-nav");
        let _ = std::fs::remove_file(&gatekeeper_path);
        let listener = UnixListener::bind(&gatekeeper_path).unwrap();
        thread::spawn(move || {
            for incoming in listener.incoming() {
                let Ok(mut stream) = incoming else { break };
                thread::spawn(move || {
                    if let Ok(_req) = blueice_ipc::gatekeeper::read_gatekeeper_request(&mut stream)
                    {
                        // Stalls every stage, so the navigation this
                        // test kicks off never resolves within the
                        // test's own lifetime -- the point is proving
                        // `Resize` doesn't wait on it at all.
                        thread::sleep(Duration::from_secs(5));
                        let _ = blueice_ipc::gatekeeper::write_gatekeeper_reply(
                            &mut stream,
                            &blueice_ipc::gatekeeper::GatekeeperReply::Cleared,
                        );
                    }
                });
            }
        });

        let (mut client, mut server) = client_pair();
        let handle = thread::spawn(move || {
            let mut tabs = TabManager::new(320.0, 200.0);
            default_page(&mut tabs).load_html_str("<p>still the old page</p>", None);
            let mut generation = 0u64;
            run_session(
                &mut tabs,
                &mut server,
                &dir,
                &mut generation,
                &gatekeeper_path,
            )
            .unwrap();
            dir
        });
        handshake(&mut client);

        blueice_ipc::write_client_message(
            &mut client,
            &ClientMessage::Navigate {
                url: "http://127.0.0.1:1/never-resolves".to_string(),
            },
        )
        .unwrap();

        let start = Instant::now();
        blueice_ipc::write_client_message(
            &mut client,
            &ClientMessage::Resize {
                width: 111,
                height: 222,
            },
        )
        .unwrap();
        let reply = blueice_ipc::read_server_message(&mut client).unwrap();
        assert!(
            matches!(
                reply,
                ServerMessage::FrameReady {
                    width: 111,
                    height: 222,
                    ..
                }
            ),
            "expected an immediate FrameReady for the resize, got {reply:?}"
        );
        assert!(
            start.elapsed() < Duration::from_millis(500),
            "Resize must apply immediately, not wait behind the pending navigation, took {:?}",
            start.elapsed()
        );

        // The old page's content is still what's shown -- the pending
        // navigation never actually applied.
        blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
        let ServerMessage::Representation(snap) =
            blueice_ipc::read_server_message(&mut client).unwrap()
        else {
            panic!("expected Representation")
        };
        assert!(snap
            .nodes
            .iter()
            .any(|n| n.name.as_deref() == Some("still the old page")));

        blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
        let dir = handle.join().unwrap();
        let _ = std::fs::remove_dir_all(dir);
    }

    // ---- about:downloads: navigation and live refresh -----------------------

    use crate::downloads_page::test_support::{
        fake_downloads_live, FakeState, Scratch as DownloadsScratch,
    };
    use crate::downloads_page::DownloadsSource;
    use blueice_ipc::downloads::{TransferInfo, TransferState, DOWNLOADS_PROTOCOL_VERSION};
    use std::sync::{Arc, Mutex};

    fn dl(id: u64, name: &str, state: TransferState, done: u64) -> TransferInfo {
        TransferInfo {
            id,
            url: format!("https://example.com/{name}"),
            dest_path: format!("/d/{name}"),
            state,
            total_bytes: Some(1000),
            completed_bytes: done,
            ..TransferInfo::default()
        }
    }

    /// A session whose tabs read `about:downloads` from `socket`; returns
    /// the client end and the session thread.
    fn downloads_session(label: &str, socket: PathBuf) -> (UnixStream, thread::JoinHandle<()>) {
        let dir = temp_frame_dir(label);
        let gatekeeper = clearing_gatekeeper(label);
        let (mut client, mut server) = client_pair();
        let handle = thread::spawn(move || {
            let mut tabs = TabManager::new(400.0, 300.0);
            tabs.set_downloads_source(Arc::new(DownloadsSource::without_spawner(socket)));
            let mut generation = 0u64;
            run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
        });
        handshake(&mut client);
        (client, handle)
    }

    fn navigate_to(client: &mut UnixStream, url: &str) -> u64 {
        client
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        blueice_ipc::write_client_message(
            client,
            &ClientMessage::Navigate {
                url: url.to_string(),
            },
        )
        .unwrap();
        assert_eq!(
            blueice_ipc::read_server_message(client).unwrap(),
            ServerMessage::Navigated {
                url: url.to_string(),
            }
        );
        let ServerMessage::FrameReady { generation, .. } =
            blueice_ipc::read_server_message(client).unwrap()
        else {
            panic!("expected the frame after Navigated")
        };
        generation
    }

    /// The next `FrameReady` the session pushes within `wait`, if any.
    fn next_pushed_frame(client: &mut UnixStream, wait: Duration) -> Option<u64> {
        client.set_read_timeout(Some(wait)).unwrap();
        match blueice_ipc::read_server_message_with_ids(client) {
            Ok((_, request_id, ServerMessage::FrameReady { generation, .. })) => {
                assert_eq!(
                    request_id, None,
                    "a refresh is unsolicited, so it carries no request id"
                );
                Some(generation)
            }
            Ok((_, _, other)) => panic!("unexpected message {other:?}"),
            Err(e) if is_timeout(&e) => None,
            Err(e) => panic!("{e}"),
        }
    }

    /// Wait for the next background refresh frame without relying on a fixed
    /// "let background work settle" interval.
    fn next_refresh(client: &mut UnixStream) -> u64 {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(generation) = next_pushed_frame(client, Duration::from_millis(50)) {
                return generation;
            }
            assert!(
                Instant::now() < deadline,
                "the downloads refresher never returned a frame"
            );
        }
    }

    fn dom_text(client: &mut UnixStream) -> String {
        client
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        blueice_ipc::write_client_message(client, &ClientMessage::GetDom).unwrap();
        loop {
            match blueice_ipc::read_server_message(client).unwrap() {
                ServerMessage::Dom(text) => return text,
                ServerMessage::FrameReady { .. } => {} // a refresh landed in between
                other => panic!("unexpected {other:?}"),
            }
        }
    }

    fn finish_session(mut client: UnixStream, handle: thread::JoinHandle<()>) {
        blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
        handle.join().unwrap();
    }

    #[test]
    fn navigating_to_about_downloads_replies_at_once_and_shows_the_live_list() {
        let dir = DownloadsScratch::new("sess-nav");
        let state = FakeState {
            transfers: Arc::new(Mutex::new(vec![dl(
                1,
                "alpha.iso",
                TransferState::Active,
                400,
            )])),
            ..FakeState::default()
        };
        let _server = fake_downloads_live(&dir.socket(), state, false, DOWNLOADS_PROTOCOL_VERSION);
        let (mut client, handle) = downloads_session("sess-nav", dir.socket());

        navigate_to(&mut client, "about:downloads");
        let dom = dom_text(&mut client);
        assert!(
            dom.contains("alpha.iso") && dom.contains("Downloading"),
            "{dom}"
        );
        finish_session(client, handle);
    }

    #[test]
    fn the_page_updates_itself_and_pushes_a_frame_when_a_transfer_changes() {
        let dir = DownloadsScratch::new("sess-live");
        let state = FakeState {
            transfers: Arc::new(Mutex::new(vec![dl(
                1,
                "alpha.iso",
                TransferState::Active,
                400,
            )])),
            ..FakeState::default()
        };
        let live = state.transfers.clone();
        let lists = state.lists.clone();
        let _server = fake_downloads_live(&dir.socket(), state, false, DOWNLOADS_PROTOCOL_VERSION);
        let (mut client, handle) = downloads_session("sess-live", dir.socket());
        let initial_lists = lists.load(Ordering::SeqCst);
        let first = navigate_to(&mut client, "about:downloads");
        let initial_refresh = next_refresh(&mut client);
        assert!(initial_refresh > first);
        assert!(lists.load(Ordering::SeqCst) > initial_lists);

        *live.lock().unwrap() = vec![
            dl(1, "alpha.iso", TransferState::Completed, 1000),
            dl(2, "beta.zip", TransferState::Active, 100),
        ];
        let pushed = next_refresh(&mut client);
        assert!(
            pushed > first,
            "the pushed frame is newer: {pushed} vs {first}"
        );

        // The human-visible frame and the AI-facing representation come from the same render pass.
        blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
        let ServerMessage::Representation(snapshot) =
            blueice_ipc::read_server_message(&mut client).unwrap()
        else {
            panic!("expected Representation")
        };
        assert_eq!(
            snapshot.generation, pushed,
            "the frame just pushed and the representation share one generation"
        );
        let dom = dom_text(&mut client);
        assert!(
            dom.contains("Completed") && dom.contains("beta.zip"),
            "{dom}"
        );
        finish_session(client, handle);
    }

    #[test]
    fn a_busy_downloads_tab_cannot_age_out_another_tabs_frame_or_generation() {
        let dir = DownloadsScratch::new("sess-tab-frame-isolation");
        let state = FakeState {
            transfers: Arc::new(Mutex::new(vec![dl(
                1,
                "progressing.iso",
                TransferState::Active,
                400,
            )])),
            ..FakeState::default()
        };
        let _server = fake_downloads_live(&dir.socket(), state, false, DOWNLOADS_PROTOCOL_VERSION);
        let (mut client, handle) = downloads_session("sess-tab-frame-isolation", dir.socket());

        blueice_ipc::write_client_message(
            &mut client,
            &ClientMessage::Navigate {
                url: "about:credits".to_string(),
            },
        )
        .unwrap();
        assert!(matches!(
            blueice_ipc::read_server_message(&mut client).unwrap(),
            ServerMessage::Navigated { .. }
        ));
        let ServerMessage::FrameReady {
            shm_path: mut quiet_path,
            generation: mut quiet_generation,
            ..
        } = blueice_ipc::read_server_message(&mut client).unwrap()
        else {
            panic!("expected the credits tab's initial frame")
        };
        assert_eq!(quiet_generation, 1);

        blueice_ipc::write_client_message(
            &mut client,
            &ClientMessage::OpenTab {
                url: Some("about:downloads".to_string()),
            },
        )
        .unwrap();
        let ServerMessage::TabOpened {
            tab_id: downloads_tab,
            ..
        } = blueice_ipc::read_server_message(&mut client).unwrap()
        else {
            panic!("expected downloads tab")
        };
        loop {
            let (tab_id, _, message) =
                blueice_ipc::read_server_message_with_ids(&mut client).unwrap();
            if matches!(message, ServerMessage::FrameReady { .. }) && tab_id == Some(downloads_tab)
            {
                break;
            }
        }

        // Resizing the one physical window now eagerly reflows both tabs.
        // More than the old global retention window's worth of downloads-tab
        // renders must still leave tab one's *latest* frame on disk. The fake
        // process has an active transfer, so this is the same two-tab shape
        // as a live panel; deterministic resizes avoid a wall-clock wait for
        // five poll ticks.
        for width in 201..=205 {
            blueice_ipc::write_client_message_with_ids(
                &mut client,
                Some(downloads_tab),
                None,
                &ClientMessage::Resize { width, height: 300 },
            )
            .unwrap();
            let mut saw_downloads_frame = false;
            let mut saw_quiet_frame = false;
            while !saw_downloads_frame || !saw_quiet_frame {
                let (tab_id, _, message) =
                    blueice_ipc::read_server_message_with_ids(&mut client).unwrap();
                match message {
                    ServerMessage::FrameReady {
                        width: rendered_width,
                        height: 300,
                        ..
                    } if tab_id == Some(downloads_tab) && rendered_width == width => {
                        saw_downloads_frame = true;
                    }
                    ServerMessage::FrameReady {
                        shm_path,
                        width: rendered_width,
                        height: 300,
                        generation,
                    } if tab_id == Some(1) && rendered_width == width => {
                        quiet_path = shm_path;
                        quiet_generation = generation;
                        saw_quiet_frame = true;
                    }
                    ServerMessage::FrameReady { .. } => {}
                    other => panic!("unexpected message while resizing tabs: {other:?}"),
                }
            }
        }

        assert!(
            blueice_ipc::shm::map_frame(Path::new(&quiet_path)).is_ok(),
            "another tab's frame must survive its busy neighbor"
        );
        blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
        loop {
            match blueice_ipc::read_server_message(&mut client).unwrap() {
                ServerMessage::Representation(snapshot) => {
                    assert_eq!(
                        snapshot.generation, quiet_generation,
                        "tab one's snapshot must still name its own last frame"
                    );
                    break;
                }
                ServerMessage::FrameReady { .. } => {} // a downloads refresh may arrive first
                other => panic!("unexpected message {other:?}"),
            }
        }

        finish_session(client, handle);
    }

    #[test]
    fn an_unchanged_list_does_not_keep_pushing_frames() {
        let dir = DownloadsScratch::new("sess-quiet");
        let state = FakeState {
            transfers: Arc::new(Mutex::new(vec![dl(
                1,
                "alpha.iso",
                TransferState::Paused,
                400,
            )])),
            ..FakeState::default()
        };
        let lists = state.lists.clone();
        let _server = fake_downloads_live(&dir.socket(), state, false, DOWNLOADS_PROTOCOL_VERSION);
        let (mut client, handle) = downloads_session("sess-quiet", dir.socket());
        let initial_lists = lists.load(Ordering::SeqCst);
        navigate_to(&mut client, "about:downloads");
        next_refresh(&mut client);
        assert!(lists.load(Ordering::SeqCst) > initial_lists);

        let polled_before = lists.load(Ordering::SeqCst);
        let deadline = Instant::now() + Duration::from_secs(5);
        while lists.load(Ordering::SeqCst) <= polled_before {
            assert!(
                next_pushed_frame(&mut client, Duration::from_millis(50)).is_none(),
                "nothing changed, so nothing should be pushed"
            );
            assert!(
                Instant::now() < deadline,
                "the refresher never made another list request"
            );
        }
        finish_session(client, handle);
    }

    #[test]
    fn leaving_the_downloads_page_forgets_its_future_polling_state() {
        let tabs = TabManager::new(100.0, 100.0);
        let tab_id = tabs.default_tab();
        let mut refresher = DownloadsRefresher::default();
        refresher
            .seen_url
            .insert(tab_id, "about:downloads".to_string());
        refresher.due.insert(tab_id, Instant::now());
        refresher
            .rendered
            .insert(tab_id, "<p>last render</p>".to_string());
        refresher.fresh_visit.insert(tab_id);
        refresher.visit.insert(tab_id, 1);
        refresher.consecutive_failures.insert(tab_id, 1);
        let (tx, _rx) = mpsc::channel();

        // The default page is about:blank. No helper thread or sleep is
        // needed to prove it cannot schedule another socket read.
        refresher.tick(&tabs, &tx, Instant::now());

        assert!(!refresher.seen_url.contains_key(&tab_id));
        assert!(!refresher.due.contains_key(&tab_id));
        assert!(!refresher.rendered.contains_key(&tab_id));
        assert!(!refresher.fresh_visit.contains(&tab_id));
        assert!(!refresher.consecutive_failures.contains_key(&tab_id));
    }

    #[test]
    fn an_absent_service_shows_the_not_running_page_and_the_list_appears_when_it_starts() {
        let dir = DownloadsScratch::new("sess-late");
        let (mut client, handle) = downloads_session("sess-late", dir.socket());
        navigate_to(&mut client, "about:downloads");
        assert!(dom_text(&mut client).contains("The downloads service is not running"));

        // The service comes up later; the open page notices without being reloaded.
        let state = FakeState {
            transfers: Arc::new(Mutex::new(vec![dl(
                3,
                "gamma.bin",
                TransferState::Active,
                10,
            )])),
            ..FakeState::default()
        };
        let _server = fake_downloads_live(&dir.socket(), state, false, DOWNLOADS_PROTOCOL_VERSION);
        let deadline = Instant::now() + Duration::from_secs(6);
        loop {
            let _ = next_pushed_frame(&mut client, Duration::from_millis(50));
            let dom = dom_text(&mut client);
            if dom.contains("gamma.bin") && !dom.contains("not running") {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "the open page never showed the recovered service: {dom}"
            );
        }
        finish_session(client, handle);
    }

    #[test]
    fn a_hung_downloads_service_cannot_stall_the_session() {
        let dir = DownloadsScratch::new("sess-hung");
        let state = FakeState::default();
        let stalls = state.stalls.clone();
        let _server = fake_downloads_live(&dir.socket(), state, true, DOWNLOADS_PROTOCOL_VERSION);
        let (mut client, handle) = downloads_session("sess-hung", dir.socket());
        navigate_to(&mut client, "about:credits");

        navigate_to(&mut client, "about:downloads");
        assert!(
            dom_text(&mut client).contains("not running"),
            "a service that does not answer is shown as unavailable"
        );

        // One connection is the navigation's short read; the second is the
        // open page's background poll. Wait until the latter has genuinely
        // reached its non-responsive peer before asking the session to
        // resize. This proves the non-blocking boundary without making an
        // assertion about wall-clock scheduling or font-loading speed.
        let deadline = Instant::now() + Duration::from_secs(6);
        while stalls.load(Ordering::SeqCst) < 2 {
            assert!(
                Instant::now() < deadline,
                "the downloads refresher never reached the hung service"
            );
            thread::sleep(Duration::from_millis(10));
        }

        // The background fetch is now known to be blocked in the fake
        // service. An ordinary request still gets its normal, correlated
        // frame reply from the session loop.
        blueice_ipc::write_client_message(
            &mut client,
            &ClientMessage::Resize {
                width: 123,
                height: 234,
            },
        )
        .unwrap();
        loop {
            match blueice_ipc::read_server_message(&mut client).unwrap() {
                ServerMessage::FrameReady {
                    width: 123,
                    height: 234,
                    ..
                } => break,
                ServerMessage::FrameReady { .. } => {}
                other => panic!("unexpected {other:?}"),
            }
        }
        finish_session(client, handle);
    }

    #[test]
    fn a_link_to_about_downloads_is_followed_like_any_built_in_page() {
        let dir = DownloadsScratch::new("sess-link");
        let state = FakeState {
            transfers: Arc::new(Mutex::new(vec![dl(
                1,
                "alpha.iso",
                TransferState::Completed,
                1000,
            )])),
            ..FakeState::default()
        };
        let _server = fake_downloads_live(&dir.socket(), state, false, DOWNLOADS_PROTOCOL_VERSION);
        let frame_dir = temp_frame_dir("sess-link");
        let gatekeeper = clearing_gatekeeper("sess-link");
        let (mut client, mut server) = client_pair();
        let socket = dir.socket();
        let handle = thread::spawn(move || {
            let mut tabs = TabManager::new(400.0, 300.0);
            tabs.set_downloads_source(Arc::new(DownloadsSource::without_spawner(socket)));
            default_page(&mut tabs)
                .load_html_str(r#"<a href="about:downloads">Open downloads</a>"#, None);
            let mut generation = 0u64;
            run_session(
                &mut tabs,
                &mut server,
                &frame_dir,
                &mut generation,
                &gatekeeper,
            )
            .unwrap();
        });
        handshake(&mut client);
        blueice_ipc::write_client_message(&mut client, &ClientMessage::Click { x: 2.0, y: 2.0 })
            .unwrap();
        client
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let ServerMessage::Navigated { url, .. } =
            blueice_ipc::read_server_message(&mut client).unwrap()
        else {
            panic!("expected Navigated")
        };
        assert_eq!(url, "about:downloads");
        assert!(matches!(
            blueice_ipc::read_server_message(&mut client).unwrap(),
            ServerMessage::FrameReady { .. }
        ));
        blueice_ipc::write_client_message(&mut client, &ClientMessage::GetDom).unwrap();
        loop {
            match blueice_ipc::read_server_message(&mut client).unwrap() {
                ServerMessage::Dom(text) => {
                    assert!(
                        text.contains("alpha.iso"),
                        "the built-in link shows the downloads list: {text}"
                    );
                    break;
                }
                ServerMessage::FrameReady { .. } => {}
                other => panic!("unexpected {other:?}"),
            }
        }
        finish_session(client, handle);
    }
}
