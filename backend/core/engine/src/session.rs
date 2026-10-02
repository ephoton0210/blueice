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
use crate::navigation_request::BrowserNavigation;
#[cfg(unix)]
use crate::script::javascript_child::{OutOfProcessJavaScriptPageExecutor, PageHostConnection};
use crate::tabs::{extension_navigation_rules_block_url, HistoryDestination, HistoryDirection};
use crate::{
    compiler_ipc::{CompilerServiceIpcRequestReceiver, CoreCompilerServiceSession},
    debugger::DebuggerRequestReceiver,
    script::{
        direct_page::{DirectPageScriptHost, DirectPageScriptKind},
        inline_runner::{DirectPageInlineExecutor, DirectPageScriptExecutionReport},
        javascript::{
            BlueTsPageExecutionReport, JavaScriptPageExecutionReport, PageJavaScriptExecutor,
        },
        ScriptRequestReceiver,
    },
};
use crate::{GroupId, Page, TabGroup, TabId, TabManager};
use blueice_bluets::RuntimePolicy;
use blueice_dom::NodeId;
use blueice_ipc::downloads::TransferInfo;
use blueice_ipc::extension::ExtensionRuntimeEvent;
use blueice_ipc::{
    shm, BlueJsScriptExecutionOutcome, BlueJsScriptExecutionReport, BlueJsScriptKind,
    BlueTsScriptExecutionOutcome, BlueTsScriptExecutionReport, BlueTsScriptKind,
    BlueTsScriptRuntimePolicy, BlueTsScriptSourcePosition, ClientMessage, ExtensionPopup,
    NodeAction, ServerMessage, TabGroupSummary, TabSummary,
};
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
const OPTIONAL_REVOKE_ACK_TIMEOUT: Duration = Duration::from_secs(2);

/// A small seam over `std::os::unix::net::UnixStream::set_read_timeout`
/// so `run_session` can require it as a trait bound rather than
/// hardcoding `UnixStream` -- every real caller (the production
/// binary, and every test in this module, all via `UnixStream::pair`)
/// already uses `UnixStream`, so this isn't a breaking bound in
/// practice.
pub trait ReadTimeout {
    fn set_read_timeout(&self, dur: Option<Duration>) -> io::Result<()>;
}

pub mod message_pipe;

/// A request from the extension-protocol listener into the one thread that
/// owns `TabManager` and all live [`Page`] state. Keeping the reply channel
/// with the request means an extension handler can wait for a bounded answer
/// without ever sharing `Page` across threads or taking a mutable lock around
/// the render pipeline.
///
/// DOM version 1 requests leave `tab_id` absent and therefore preserve the
/// default-tab behavior. DOM versions 2 through 9 carry an explicit tab and a
/// stable target node ID for their constrained write operations. The separately
/// versioned network rule carries no page target. The session validates each
/// operation against its live `TabManager`/`Page` state before changing it.
pub enum ExtensionPageRequest {
    /// Core-internal completion barrier after an optional grant has already
    /// been revoked in the registry. The session retires all stale published
    /// effects and writes native UI removal messages before acknowledging.
    /// No extension wire request or ordinary frontend client maps to this.
    SynchronizeRevokedEffects { reply: mpsc::Sender<()> },
    /// Private parent-pipe read of the current document identity. A later
    /// native gesture must bind to this exact tab and epoch; public clients
    /// and extension guests cannot issue this internal request.
    InspectDocument {
        tab_id: u64,
        reply: mpsc::Sender<Result<(u64, Option<String>), String>>,
    },
    ReadRepresentation {
        tab_id: Option<u64>,
        reply: mpsc::Sender<Result<String, String>>,
    },
    ReadEphemeralRepresentation {
        tab_id: u64,
        ticket: String,
        reply: mpsc::Sender<Result<String, String>>,
    },
    ReadNetworkResponse {
        tab_id: u64,
        reply: mpsc::Sender<Result<Option<blueice_ipc::extension::NetworkResponseInfo>, String>>,
    },
    ReadNetworkTrace {
        tab_id: u64,
        reply: mpsc::Sender<Result<Option<blueice_ipc::extension::NetworkTraceInfo>, String>>,
    },
    SetToolbarButton {
        connection_id: u64,
        grant_generation: u64,
        label: String,
        reply: mpsc::Sender<Result<(), String>>,
    },
    ClearToolbarButton {
        connection_id: u64,
        reply: mpsc::Sender<()>,
    },
    ShowPopup {
        connection_id: u64,
        grant_generation: u64,
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
        grant_generation: u64,
        reply: mpsc::Sender<Result<(), String>>,
    },
    SetCheckboxChecked {
        tab_id: u64,
        node_id: u64,
        checked: bool,
        grant_generation: u64,
        reply: mpsc::Sender<Result<(), String>>,
    },
    SetTextareaValue {
        tab_id: u64,
        node_id: u64,
        value: String,
        grant_generation: u64,
        reply: mpsc::Sender<Result<(), String>>,
    },
    SetVisibleLeafText {
        tab_id: u64,
        node_id: u64,
        value: String,
        grant_generation: u64,
        reply: mpsc::Sender<Result<(), String>>,
    },
    SetVisibleTextContent {
        tab_id: u64,
        node_id: u64,
        value: String,
        grant_generation: u64,
        reply: mpsc::Sender<Result<(), String>>,
    },
    /// Sets one integer value on an explicit range input after core derives
    /// and validates that control's live constraints.
    SetRangeInputValue {
        tab_id: u64,
        node_id: u64,
        value: i64,
        grant_generation: u64,
        reply: mpsc::Sender<Result<(), String>>,
    },
    /// Selects one explicit radio while core owns the group-membership and
    /// mutual-exclusion semantics.
    SetRadioChecked {
        tab_id: u64,
        node_id: u64,
        grant_generation: u64,
        reply: mpsc::Sender<Result<(), String>>,
    },
    /// Selects one explicit option while core owns the live single-select
    /// validation and the corresponding sibling deselection.
    SelectOption {
        tab_id: u64,
        node_id: u64,
        grant_generation: u64,
        reply: mpsc::Sender<Result<(), String>>,
    },
    /// Adds one core-validated, connection-scoped exact navigation block rule
    /// evaluated for the initial request and later redirect hops. The opaque
    /// connection ID is allocated by `blueice-core`, never supplied by an
    /// extension. `grant_generation` is captured by the trusted host before
    /// Gatekeeper review and checked again by this session at the actual
    /// write, so a timed-out queued request cannot borrow a newer grant.
    RegisterNetworkBlockUrl {
        connection_id: u64,
        grant_generation: u64,
        url: String,
        reply: mpsc::Sender<Result<(), String>>,
    },
    /// Adds one core-validated, connection-scoped ASCII host/subdomain rule.
    RegisterNetworkBlockHost {
        connection_id: u64,
        grant_generation: u64,
        host: String,
        reply: mpsc::Sender<Result<(), String>>,
    },
    /// Adds one core-validated, connection-scoped literal host/path-prefix
    /// rule without granting the guest a callback or arbitrary URL pattern.
    RegisterNetworkBlockPathPrefix {
        connection_id: u64,
        grant_generation: u64,
        host: String,
        path_prefix: String,
        reply: mpsc::Sender<Result<(), String>>,
    },
    /// Adds one exact same-origin navigation rewrite after core validation.
    RegisterNetworkRedirectUrl {
        connection_id: u64,
        grant_generation: u64,
        source_url: String,
        target_url: String,
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

/// Core-internal revoke transaction for a future authenticated human-control
/// channel. The grant is withdrawn first, blocking new effects immediately;
/// only then does the caller wait for the session owner to retire previously
/// published UI and network rules. A timeout leaves the grant revoked and is
/// an error, never an implicit cleanup success. This is not mapped from the
/// public frontend, MCP, or extension wire protocols. Call only from a
/// separate trusted control worker, never from the session thread itself.
pub fn revoke_optional_and_wait_for_cleanup(
    registry: &blueice_extension_host::ExtensionRegistry,
    extension_id: &str,
    capability: &str,
    session_requests: &mpsc::Sender<ExtensionPageRequest>,
) -> Result<bool, String> {
    let changed = registry.revoke_optional(extension_id, capability)?;
    let (reply, completed) = mpsc::channel();
    session_requests.send(ExtensionPageRequest::SynchronizeRevokedEffects { reply })
        .map_err(|_| format!(
            "optional {capability} grant was revoked, but the core session is unavailable for cleanup"
        ))?;
    completed.recv_timeout(OPTIONAL_REVOKE_ACK_TIMEOUT)
        .map_err(|_| format!(
            "optional {capability} grant was revoked, but the core session did not acknowledge cleanup"
        ))?;
    Ok(changed)
}

/// Optional worker-to-session request channels for one core connection. The
/// workers own decoded transport only; the session owns all live tab/document
/// resolution. Grouping them keeps the composite lifecycle API explicit
/// without growing an unbounded list of transport parameters.
#[derive(Default)]
pub struct CoreSessionRequests<'a> {
    pub script: Option<&'a ScriptRequestReceiver>,
    pub debugger: Option<&'a DebuggerRequestReceiver>,
    /// The compiler listener's worker-to-session hand-off plus the sealed
    /// core-owned catalog. Neither field is present in the ordinary browser
    /// session, and the listener never receives the catalog itself.
    pub compiler: Option<CoreCompilerSessionRequests<'a>>,
    /// The private extension listener's request channel and its bounded
    /// lifecycle-event sender (Phase 9). Absent in an ordinary session.
    pub extension: Option<&'a mpsc::Receiver<ExtensionPageRequest>>,
    pub extension_events: Option<&'a mpsc::SyncSender<ExtensionRuntimeEvent>>,
}

/// Compiler-specific half of [`CoreSessionRequests`]. Keeping the mutable
/// sealed catalog paired with its receiver prevents a socket worker from
/// observing or mutating compiler cache state directly.
pub struct CoreCompilerSessionRequests<'a> {
    pub receiver: &'a CompilerServiceIpcRequestReceiver,
    pub service: &'a mut CoreCompilerServiceSession,
}

/// The mutually exclusive core-owned page-script lifecycle owners for one
/// session. Keeping their relation explicit avoids an ever-growing internal
/// session function signature and preserves the one-realm-owner invariant.
struct PageScriptRuntime<'a> {
    direct_page_host: Option<&'a mut DirectPageScriptHost>,
    inline_page_executor: Option<&'a mut DirectPageInlineExecutor>,
    javascript_executor: Option<&'a mut dyn PageJavaScriptExecutor>,
}

#[cfg(unix)]
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
    run_session_with_script_requests(tabs, stream, frame_dir, generation, gatekeeper_socket, None)
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
    run_session_with_core_session_requests(
        tabs,
        stream,
        frame_dir,
        generation,
        gatekeeper_socket,
        CoreSessionRequests {
            extension: Some(extension_requests),
            extension_events,
            ..CoreSessionRequests::default()
        },
    )
}

/// Like [`run_session`], while dispatching each pending external script
/// request on the owning core session thread between frontend reads. The
/// listener side receives only structured replies; it cannot borrow or move
/// the tab manager across the process/thread boundary.
pub fn run_session_with_script_requests<S: Read + Write + ReadTimeout>(
    tabs: &mut TabManager,
    stream: &mut S,
    frame_dir: &Path,
    generation: &mut u64,
    gatekeeper_socket: &Path,
    script_requests: Option<&ScriptRequestReceiver>,
) -> io::Result<()> {
    run_session_with_script_requests_and_direct_page_host(
        tabs,
        stream,
        frame_dir,
        generation,
        gatekeeper_socket,
        script_requests,
        None,
    )
}

/// Like [`run_session_with_script_requests`], while an optional core-owned
/// direct BlueTS host observes document/tab lifecycle boundaries. The host
/// only synchronizes realms that previously admitted a direct script; ordinary
/// pages never allocate a BlueJS realm merely because the session observed
/// them. This is an in-process lifecycle seam, not an HTML script loader or
/// out-of-process BlueJS supervisor.
pub fn run_session_with_script_requests_and_direct_page_host<S: Read + Write + ReadTimeout>(
    tabs: &mut TabManager,
    stream: &mut S,
    frame_dir: &Path,
    generation: &mut u64,
    gatekeeper_socket: &Path,
    script_requests: Option<&ScriptRequestReceiver>,
    direct_page_host: Option<&mut DirectPageScriptHost>,
) -> io::Result<()> {
    run_session_with_script_runtime(
        tabs,
        stream,
        frame_dir,
        generation,
        gatekeeper_socket,
        CoreSessionRequests {
            script: script_requests,
            debugger: None,
            compiler: None,
            extension: None,
            extension_events: None,
        },
        PageScriptRuntime {
            direct_page_host,
            inline_page_executor: None,
            javascript_executor: None,
        },
    )
}

/// Like [`run_session_with_script_requests_and_direct_page_host`], but with
/// an explicitly configured inline BlueTS executor. Unlike the observer-only
/// host seam, this runs the current document's opted-in inline declarations
/// after each lifecycle batch. The default [`run_session`] does not enable it;
/// callers must select a verified host profile when constructing the executor.
pub fn run_session_with_script_requests_and_inline_page_executor<S: Read + Write + ReadTimeout>(
    tabs: &mut TabManager,
    stream: &mut S,
    frame_dir: &Path,
    generation: &mut u64,
    gatekeeper_socket: &Path,
    script_requests: Option<&ScriptRequestReceiver>,
    inline_page_executor: Option<&mut DirectPageInlineExecutor>,
) -> io::Result<()> {
    run_session_with_script_runtime(
        tabs,
        stream,
        frame_dir,
        generation,
        gatekeeper_socket,
        CoreSessionRequests {
            script: script_requests,
            debugger: None,
            compiler: None,
            extension: None,
            extension_events: None,
        },
        PageScriptRuntime {
            direct_page_host: None,
            inline_page_executor,
            javascript_executor: None,
        },
    )
}

/// Like [`run_session_with_script_requests`], while routing debugger discovery
/// requests through the owning session thread. The debugger receiver is a
/// separate transport from the page-script receiver and can only inspect the
/// live tab/document identity; it cannot borrow page or VM state.
pub fn run_session_with_script_and_debugger_requests<S: Read + Write + ReadTimeout>(
    tabs: &mut TabManager,
    stream: &mut S,
    frame_dir: &Path,
    generation: &mut u64,
    gatekeeper_socket: &Path,
    script_requests: Option<&ScriptRequestReceiver>,
    debugger_requests: Option<&DebuggerRequestReceiver>,
) -> io::Result<()> {
    run_session_with_script_runtime(
        tabs,
        stream,
        frame_dir,
        generation,
        gatekeeper_socket,
        CoreSessionRequests {
            script: script_requests,
            debugger: debugger_requests,
            compiler: None,
            extension: None,
            extension_events: None,
        },
        PageScriptRuntime {
            direct_page_host: None,
            inline_page_executor: None,
            javascript_executor: None,
        },
    )
}

/// Like [`run_session_with_script_and_debugger_requests`], with an optional
/// sealed registered-project compiler session. This is the composite core
/// startup seam used by the process binary when a trusted owner explicitly
/// enabled its compiler listener; normal callers can continue using the
/// narrower helper above and therefore have no compiler authority at all.
pub fn run_session_with_core_session_requests<S: Read + Write + ReadTimeout>(
    tabs: &mut TabManager,
    stream: &mut S,
    frame_dir: &Path,
    generation: &mut u64,
    gatekeeper_socket: &Path,
    requests: CoreSessionRequests<'_>,
) -> io::Result<()> {
    run_session_with_script_runtime(
        tabs,
        stream,
        frame_dir,
        generation,
        gatekeeper_socket,
        requests,
        PageScriptRuntime {
            direct_page_host: None,
            inline_page_executor: None,
            javascript_executor: None,
        },
    )
}

/// Like [`run_session_with_script_and_debugger_requests`], while also running
/// one explicitly configured inline BlueTS executor. This is the composite
/// production seam used only when core selected both optional socket/profile
/// features at startup.
pub fn run_session_with_script_and_debugger_requests_and_inline_page_executor<
    S: Read + Write + ReadTimeout,
>(
    tabs: &mut TabManager,
    stream: &mut S,
    frame_dir: &Path,
    generation: &mut u64,
    gatekeeper_socket: &Path,
    requests: CoreSessionRequests<'_>,
    inline_page_executor: Option<&mut DirectPageInlineExecutor>,
) -> io::Result<()> {
    run_session_with_script_runtime(
        tabs,
        stream,
        frame_dir,
        generation,
        gatekeeper_socket,
        requests,
        PageScriptRuntime {
            direct_page_host: None,
            inline_page_executor,
            javascript_executor: None,
        },
    )
}

/// Like [`run_session_with_script_and_debugger_requests`], while running one
/// explicitly selected standard-JavaScript page executor. The executor may be
/// the bounded in-process host or a separately configured launcher-supervised
/// child connection; either way it has no DOM bindings and may not be paired
/// with the separate BlueTS executor, which would otherwise allocate a second
/// realm for the same page.
pub fn run_session_with_script_and_debugger_requests_and_inline_javascript_executor<
    S: Read + Write + ReadTimeout,
>(
    tabs: &mut TabManager,
    stream: &mut S,
    frame_dir: &Path,
    generation: &mut u64,
    gatekeeper_socket: &Path,
    requests: CoreSessionRequests<'_>,
    javascript_executor: Option<&mut dyn PageJavaScriptExecutor>,
) -> io::Result<()> {
    run_session_with_script_runtime(
        tabs,
        stream,
        frame_dir,
        generation,
        gatekeeper_socket,
        requests,
        PageScriptRuntime {
            direct_page_host: None,
            inline_page_executor: None,
            javascript_executor,
        },
    )
}

/// Like [`run_session_with_script_and_debugger_requests`], while routing page
/// declarations through the explicitly configured launcher-supervised BlueJS
/// child host. The caller must have obtained its socket and per-spawn
/// capability through a trusted launcher boundary; normal sessions never
/// construct this executor themselves.
#[cfg(unix)]
pub fn run_session_with_script_and_debugger_requests_and_out_of_process_javascript_executor<
    S: Read + Write + ReadTimeout,
>(
    tabs: &mut TabManager,
    stream: &mut S,
    frame_dir: &Path,
    generation: &mut u64,
    gatekeeper_socket: &Path,
    requests: CoreSessionRequests<'_>,
    out_of_process_javascript_executor: Option<
        &mut OutOfProcessJavaScriptPageExecutor<PageHostConnection>,
    >,
) -> io::Result<()> {
    run_session_with_script_and_debugger_requests_and_inline_javascript_executor(
        tabs,
        stream,
        frame_dir,
        generation,
        gatekeeper_socket,
        requests,
        out_of_process_javascript_executor
            .map(|executor| executor as &mut dyn PageJavaScriptExecutor),
    )
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

mod script_runtime;
use script_runtime::*;

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
    )?;
    if page.display_viewport.is_some() {
        blueice_ipc::write_server_message_with_ids(
            stream,
            Some(tab_id),
            request_id,
            &ServerMessage::ViewportState(
                page.viewport_state(shm::frame_source_id(frame_dir), tab_id),
            ),
        )?;
    }
    Ok(())
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

#[cfg(all(test, unix))]
#[path = "session/tests.rs"]
mod tests;

#[cfg(all(test, unix))]
mod feature_tests;

mod run_loop;
use run_loop::*;
mod extension_bridge;
use extension_bridge::*;
mod navigation;
use navigation::*;
mod assistant_tasks;
use assistant_tasks::*;
