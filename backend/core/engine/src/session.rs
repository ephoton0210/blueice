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

/// Shared session implementation for the observer-only direct host and the
/// explicitly enabled inline runners. [`PageScriptRuntime`] rejects multiple
/// hosts at once: independent hosts would allocate separate page realms for
/// one page.
fn run_session_with_script_runtime<S: Read + Write + ReadTimeout>(
    tabs: &mut TabManager,
    stream: &mut S,
    frame_dir: &Path,
    generation: &mut u64,
    gatekeeper_socket: &Path,
    requests: CoreSessionRequests<'_>,
    mut page_script_runtime: PageScriptRuntime<'_>,
) -> io::Result<()> {
    if [
        page_script_runtime.direct_page_host.is_some(),
        page_script_runtime.inline_page_executor.is_some(),
        page_script_runtime.javascript_executor.is_some(),
    ]
    .into_iter()
    .filter(|enabled| *enabled)
    .count()
        > 1
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "page-script runtime owners cannot share one session",
        ));
    }
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
    synchronize_page_script_runtime(&mut page_script_runtime, tabs, requests.script)?;
    let extension_requests = requests.extension;
    let extension_events = requests.extension_events;

    let (completion_tx, completion_rx) = mpsc::channel::<Completion>();
    let (assistant_tx, assistant_rx) = mpsc::channel::<AssistantCompletion>();
    let mut pending_nav_seq: HashMap<TabId, u64> = HashMap::new();
    let (listing_tx, listing_rx) = mpsc::channel::<DownloadsListing>();
    let mut downloads_refresher = DownloadsRefresher::default();
    // Only the connection that published the native button can remove it.
    let mut extension_toolbar: Option<(u64, String, u64)> = None;
    let mut extension_popup: Option<(u64, ExtensionPopup, u64)> = None;

    let mut requests = requests;
    loop {
        let incoming = blueice_ipc::read_client_message_with_ids(stream);
        if !matches!(&incoming, Err(error) if !is_timeout(error)) {
            prune_stale_extension_effects(
                tabs,
                stream,
                &mut extension_toolbar,
                &mut extension_popup,
            )?;
        }
        match incoming {
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
                    ClientMessage::SetTranslationLanguage { target_language } => {
                        if tabs.get(target).is_none() {
                            write_unknown_tab_error(stream, request_id, target)?;
                            continue;
                        }
                        if let Some(tag) = &target_language {
                            if let Err(reason) = blueice_ipc::assistant::validate_language_tag(tag)
                            {
                                write_error(stream, reply_tab, request_id, reason)?;
                                continue;
                            }
                        }
                        if !tabs.set_translation_language(target_language) {
                            write_error(
                                stream,
                                reply_tab,
                                request_id,
                                "translation is unavailable: blueice-core was started without an assistant"
                                    .to_string(),
                            )?;
                            continue;
                        }
                        write_translation_state(tabs, stream, reply_tab, request_id, target)?;
                    }
                    ClientMessage::ShowTranslation { shown } => {
                        let Some(page) = tabs.get_mut(target) else {
                            write_unknown_tab_error(stream, request_id, target)?;
                            continue;
                        };
                        let changed = page.set_translation_shown(shown);
                        write_translation_state(tabs, stream, reply_tab, request_id, target)?;
                        if changed {
                            let page = tabs.get_mut(target).expect("checked immediately above");
                            send_frame(page, stream, frame_dir, generation, reply_tab, request_id)?;
                        }
                    }
                    ClientMessage::GetTranslationState => {
                        if tabs.get(target).is_none() {
                            write_unknown_tab_error(stream, request_id, target)?;
                            continue;
                        }
                        write_translation_state(tabs, stream, reply_tab, request_id, target)?;
                    }
                    task @ (ClientMessage::SummarizePage | ClientMessage::OrganizePage { .. }) => {
                        begin_assistant_task(
                            tabs,
                            stream,
                            reply_tab,
                            request_id,
                            target,
                            task,
                            &assistant_tx,
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
                        let Some((node, event_target, href)) = tabs.get(target).map(|page| {
                            (
                                page.click_target(x, y),
                                page.click_event_target(x, y)
                                    .map(|node| (node, page.document_generation())),
                                page.click(x, y),
                            )
                        }) else {
                            write_unknown_tab_error(stream, request_id, target)?;
                            continue;
                        };
                        // A BlueJS click listener (when one is configured)
                        // runs before any default action and may prevent it.
                        let mut prevented = None;
                        if let Some((event_node, clicked_generation)) = event_target {
                            match dispatch_click_before_default(
                                &mut page_script_runtime.javascript_executor,
                                tabs,
                                target,
                                event_node,
                                requests.script,
                            ) {
                                Err(_) => {
                                    write_error(
                                        stream,
                                        reply_tab,
                                        request_id,
                                        "page click listener unavailable".to_string(),
                                    )?;
                                    continue;
                                }
                                Ok(result) => prevented = result,
                            }
                            let page = tabs.get_mut(target).expect("hit-tested tab is live");
                            if page.document_generation() != clicked_generation {
                                send_frame(
                                    page, stream, frame_dir, generation, reply_tab, request_id,
                                )?;
                                continue;
                            }
                        }
                        let mut focus_changed = false;
                        if prevented != Some(true) {
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
                        if prevented.is_some() || focus_changed {
                            let page = tabs
                                .get_mut(target)
                                .expect("a click cannot close a core-owned tab");
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
                            let mut snapshot =
                                page.snapshot(page.frame_generation(), target.as_u64());
                            snapshot.frame_source = blueice_ipc::shm::frame_source_id(frame_dir);
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
                    ClientMessage::GetBlueTsScriptReports => match tabs.get(target) {
                        Some(_) => {
                            if let Some(executor) =
                                page_script_runtime.inline_page_executor.as_deref_mut()
                            {
                                blueice_ipc::write_server_message_with_ids(
                                    stream,
                                    reply_tab,
                                    request_id,
                                    &ServerMessage::BlueTsScriptReports(inline_execution_reports(
                                        executor.drain_reports_for_tab(target),
                                    )),
                                )?;
                            } else if let Some(executor) =
                                page_script_runtime.javascript_executor.as_deref_mut()
                            {
                                if executor.supports_blue_ts_page_execution() {
                                    blueice_ipc::write_server_message_with_ids(
                                        stream,
                                        reply_tab,
                                        request_id,
                                        &ServerMessage::BlueTsScriptReports(
                                            child_blue_ts_execution_reports(
                                                executor.drain_blue_ts_reports_for_tab(target),
                                            ),
                                        ),
                                    )?;
                                } else {
                                    write_error(
                                        stream,
                                        reply_tab,
                                        request_id,
                                        "inline BlueTS execution is not enabled".to_string(),
                                    )?;
                                }
                            } else {
                                write_error(
                                    stream,
                                    reply_tab,
                                    request_id,
                                    "inline BlueTS execution is not enabled".to_string(),
                                )?;
                            }
                        }
                        None => write_unknown_tab_error(stream, request_id, target)?,
                    },
                    ClientMessage::GetBlueJsScriptReports => match tabs.get(target) {
                        Some(_) => match page_script_runtime.javascript_executor.as_deref_mut() {
                            Some(executor) => blueice_ipc::write_server_message_with_ids(
                                stream,
                                reply_tab,
                                request_id,
                                &ServerMessage::BlueJsScriptReports(
                                    inline_javascript_execution_reports(
                                        executor.drain_reports_for_tab(target),
                                    ),
                                ),
                            )?,
                            None => write_error(
                                stream,
                                reply_tab,
                                request_id,
                                "inline JavaScript execution is not enabled".to_string(),
                            )?,
                        },
                        None => write_unknown_tab_error(stream, request_id, target)?,
                    },
                    ClientMessage::ActOn { id, action } => {
                        if tabs.get(target).is_none() {
                            write_unknown_tab_error(stream, request_id, target)?;
                            continue;
                        }
                        let node = NodeId::from_u64(id);
                        let is_click = matches!(&action, NodeAction::Click);
                        let clicked_generation = if is_click {
                            tabs.get(target).map(Page::document_generation)
                        } else {
                            None
                        };
                        // A click listener (when configured) runs before the
                        // default action and may prevent it.
                        let dispatch = if is_click {
                            tabs.get(target)
                                .and_then(|page| page.event_element_target(node))
                                .map(|event_node| {
                                    dispatch_click_before_default(
                                        &mut page_script_runtime.javascript_executor,
                                        tabs,
                                        target,
                                        event_node,
                                        requests.script,
                                    )
                                })
                        } else {
                            None
                        };
                        if matches!(dispatch, Some(Err(_))) {
                            write_error(
                                stream,
                                reply_tab,
                                request_id,
                                "page click listener unavailable".to_string(),
                            )?;
                            continue;
                        }
                        let prevented = dispatch.and_then(Result::ok).flatten();
                        let page = tabs
                            .get_mut(target)
                            .expect("a click listener cannot close a core-owned tab");
                        if is_click
                            && (clicked_generation != Some(page.document_generation())
                                || prevented == Some(true))
                        {
                            send_frame(page, stream, frame_dir, generation, reply_tab, request_id)?;
                            continue;
                        }
                        let href = page.act(node, action);
                        if is_click {
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
                            if prevented.is_some() {
                                let page = tabs
                                    .get_mut(target)
                                    .expect("a click listener cannot close a core-owned tab");
                                send_frame(
                                    page, stream, frame_dir, generation, reply_tab, request_id,
                                )?;
                            }
                        } else {
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
                                .is_some_and(|(_, popup, _)| popup.tab_id == target.as_u64())
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
                                label: extension_toolbar
                                    .as_ref()
                                    .map(|(_, label, _)| label.clone()),
                            },
                        )?;
                    }
                    ClientMessage::ActivateExtensionToolbar => {
                        #[allow(clippy::unnecessary_unwrap)]
                        let result = if extension_toolbar.is_none() {
                            Err("no extension toolbar button is installed".to_string())
                        } else if tabs.get(target).is_none() {
                            Err(format!("unknown tab {}", target.as_u64()))
                        } else if let Some(events) = extension_events {
                            events
                                .try_send(ExtensionRuntimeEvent::ToolbarActivated {
                                    tab_id: target.as_u64(),
                                    grant_generation: extension_toolbar.as_ref().unwrap().2,
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
                                popup: extension_popup.as_ref().map(|(_, popup, _)| popup.clone()),
                            },
                        )?;
                    }
                    ClientMessage::DismissExtensionPopup => {
                        if extension_popup
                            .as_ref()
                            .is_some_and(|(_, popup, _)| popup.tab_id == target.as_u64())
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
                    ClientMessage::ActivateExtensionPopupAction { popup_id } => {
                        let valid_popup = extension_popup.as_ref().is_some_and(|(_, popup, _)| {
                            popup.id != 0
                                && popup.id == popup_id
                                && popup.tab_id == target.as_u64()
                                && popup.action_label.is_some()
                        });
                        let result = if !valid_popup || tabs.get(target).is_none() {
                            Err("no matching live extension popup action".to_string())
                        } else if let Some(events) = extension_events {
                            events
                                .try_send(ExtensionRuntimeEvent::PopupActionActivated {
                                    tab_id: target.as_u64(),
                                    grant_generation: extension_popup.as_ref().unwrap().2,
                                })
                                .map_err(|_| "extension event queue is unavailable".to_string())
                        } else {
                            Err("the extension runtime is unavailable".to_string())
                        };
                        match result {
                            Ok(()) => {
                                extension_popup = None;
                                blueice_ipc::write_server_message_with_ids(
                                    stream,
                                    None,
                                    None,
                                    &ServerMessage::ExtensionPopup { popup: None },
                                )?;
                            }
                            Err(message) => blueice_ipc::write_server_message_with_ids(
                                stream,
                                reply_tab,
                                request_id,
                                &ServerMessage::Error { message },
                            )?,
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

        while let Ok(done) = assistant_rx.try_recv() {
            finish_assistant_task(tabs, stream, frame_dir, generation, done)?;
        }

        let mut synchronized_after_completion = false;
        while let Ok(completion) = completion_rx.try_recv() {
            synchronized_after_completion |= apply_completion(
                tabs,
                stream,
                frame_dir,
                generation,
                &pending_nav_seq,
                completion,
                &mut downloads_refresher,
                &mut page_script_runtime,
                requests.script,
                extension_events,
            )?;
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
                    &mut extension_toolbar,
                    &mut extension_popup,
                    request,
                )?;
            }
        }
        if let Some(script_requests) = requests.script {
            script_requests.dispatch_pending(tabs);
        }
        if let Some(debugger_requests) = requests.debugger {
            let debugger_executor = page_script_runtime.javascript_executor.as_deref_mut();
            debugger_requests.dispatch_pending(tabs, debugger_executor);
        }
        if let Some(compiler_requests) = requests.compiler.as_mut() {
            compiler_requests
                .service
                .dispatch_pending(compiler_requests.receiver);
        }
        // `apply_completion` already synchronized the just-admitted document
        // before publishing its navigation reply. Do not immediately run a
        // second lifecycle turn here: that would make a newly admitted
        // root-entry debugger program execute before its peer can even ask
        // for the opaque location needed to arm it.
        if !synchronized_after_completion {
            synchronize_page_script_runtime(&mut page_script_runtime, tabs, requests.script)?;
        }
    }
}

/// Removes connection-owned effects published under an earlier optional
/// grant generation. The session polls even without client traffic, so a
/// revoke does not need a public IPC trigger to retire UI or network rules.
fn prune_stale_extension_effects<S: Write>(
    tabs: &mut TabManager,
    stream: &mut S,
    toolbar: &mut Option<(u64, String, u64)>,
    popup: &mut Option<(u64, ExtensionPopup, u64)>,
) -> io::Result<()> {
    tabs.prune_stale_extension_navigation_rules();
    let current = tabs.extension_capability_generation("ui:inject");
    let toolbar_is_stale = toolbar
        .as_ref()
        .is_some_and(|(_, _, generation)| current != Some(*generation));
    let popup_is_stale = popup
        .as_ref()
        .is_some_and(|(_, _, generation)| current != Some(*generation));
    if (toolbar_is_stale || popup_is_stale) && popup.take().is_some() {
        blueice_ipc::write_server_message_with_ids(
            stream,
            None,
            None,
            &ServerMessage::ExtensionPopup { popup: None },
        )?;
    }
    if toolbar_is_stale {
        toolbar.take();
        blueice_ipc::write_server_message_with_ids(
            stream,
            None,
            None,
            &ServerMessage::ExtensionToolbar { label: None },
        )?;
    }
    Ok(())
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
    extension_toolbar: &mut Option<(u64, String, u64)>,
    extension_popup: &mut Option<(u64, ExtensionPopup, u64)>,
    request: ExtensionPageRequest,
) -> io::Result<()> {
    match request {
        ExtensionPageRequest::SynchronizeRevokedEffects { reply } => {
            prune_stale_extension_effects(tabs, stream, extension_toolbar, extension_popup)?;
            let _ = reply.send(());
        }
        ExtensionPageRequest::InspectDocument { tab_id, reply } => {
            let id = TabId::from_u64(tab_id);
            let result = tabs
                .document_epoch(id)
                .map(|epoch| {
                    (
                        epoch,
                        tabs.get(id).and_then(|page| page.url()).map(str::to_string),
                    )
                })
                .ok_or_else(|| "the requested tab is not live".to_string());
            let _ = reply.send(result);
        }
        ExtensionPageRequest::ReadRepresentation { tab_id, reply } => {
            let tab_id = tab_id
                .map(TabId::from_u64)
                .unwrap_or_else(|| tabs.default_tab());
            let result = tabs
                .check_extension_origin("dom:read", tab_id)
                .and_then(|()| {
                    let page = tabs.get(tab_id).expect("the checked tab remains live");
                    let mut snapshot = page.snapshot(page.frame_generation(), tab_id.as_u64());
                    snapshot.frame_source = blueice_ipc::shm::frame_source_id(frame_dir);
                    serde_json::to_string(&snapshot).map_err(|error| {
                        format!("could not serialize the core representation: {error}")
                    })
                });
            let _ = reply.send(result);
        }
        ExtensionPageRequest::ReadEphemeralRepresentation {
            tab_id,
            ticket,
            reply,
        } => {
            let id = TabId::from_u64(tab_id);
            let result = tabs
                .check_extension_origin("dom:read", id)
                .and_then(|()| tabs.consume_extension_runtime_ephemeral("dom:read", id, &ticket))
                .and_then(|()| {
                    let page = tabs.get(id).expect("the consumed lease names a live tab");
                    let mut snapshot = page.snapshot(page.frame_generation(), tab_id);
                    snapshot.frame_source = blueice_ipc::shm::frame_source_id(frame_dir);
                    serde_json::to_string(&snapshot).map_err(|error| {
                        format!("could not serialize the core representation: {error}")
                    })
                });
            let _ = reply.send(result);
        }
        ExtensionPageRequest::ReadNetworkResponse { tab_id, reply } => {
            let tab_id = TabId::from_u64(tab_id);
            let result = tabs
                .check_extension_origin("network:observe", tab_id)
                .map(|()| {
                    tabs.get(tab_id)
                        .expect("the checked tab remains live")
                        .network_response()
                        .cloned()
                });
            let _ = reply.send(result);
        }
        ExtensionPageRequest::ReadNetworkTrace { tab_id, reply } => {
            let tab_id = TabId::from_u64(tab_id);
            let result = tabs
                .check_extension_origin("network:observe", tab_id)
                .map(|()| {
                    tabs.get(tab_id)
                        .expect("the checked tab remains live")
                        .network_trace()
                        .cloned()
                });
            let _ = reply.send(result);
        }
        ExtensionPageRequest::SetToolbarButton {
            connection_id,
            grant_generation,
            label,
            reply,
        } => {
            let result = tabs
                .with_stable_extension_capability("ui:inject", grant_generation, |_| {
                    blueice_extension_host::validate_toolbar_label(&label).and_then(|()| {
                        if extension_popup
                            .as_ref()
                            .is_some_and(|(owner, _, _)| *owner != connection_id)
                        {
                            blueice_ipc::write_server_message_with_ids(
                                stream,
                                None,
                                None,
                                &ServerMessage::ExtensionPopup { popup: None },
                            )
                            .map_err(|error| {
                                format!("could not remove old extension popup: {error}")
                            })?;
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
                        *extension_toolbar = Some((connection_id, label, grant_generation));
                        Ok(())
                    })
                })
                .and_then(|result| result);
            let _ = reply.send(result);
        }
        ExtensionPageRequest::ClearToolbarButton {
            connection_id,
            reply,
        } => {
            if extension_toolbar
                .as_ref()
                .is_some_and(|(owner, _, _)| *owner == connection_id)
            {
                if extension_popup
                    .as_ref()
                    .is_some_and(|(owner, _, _)| *owner == connection_id)
                {
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
            grant_generation,
            popup,
            reply,
        } => {
            let result = tabs
                .with_stable_extension_capability("ui:inject", grant_generation, |tabs| {
                    blueice_extension_host::validate_popup_text(&popup.title, &popup.body)
                        .and_then(|()| match popup.action_label.as_deref() {
                            Some(_) if popup.id == 0 => {
                                Err("a popup action needs a core-assigned ID".to_string())
                            }
                            Some(label) => blueice_extension_host::validate_toolbar_label(label),
                            None => Ok(()),
                        })
                        .and_then(|()| {
                            if !extension_toolbar
                                .as_ref()
                                .is_some_and(|(owner, _, generation)| {
                                    *owner == connection_id && *generation == grant_generation
                                })
                            {
                                return Err(
                                    "a popup requires this connection's toolbar button".to_string()
                                );
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
                            .map_err(|error| {
                                format!("could not publish extension popup: {error}")
                            })?;
                            *extension_popup = Some((connection_id, popup, grant_generation));
                            Ok(())
                        })
                })
                .and_then(|result| result);
            let _ = reply.send(result);
        }
        ExtensionPageRequest::ClearPopup {
            connection_id,
            reply,
        } => {
            if extension_popup
                .as_ref()
                .is_some_and(|(owner, _, _)| *owner == connection_id)
            {
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
            grant_generation,
            reply,
        } => {
            let tab_id = TabId::from_u64(tab_id);
            let node_id = NodeId::from_u64(node_id);
            let result = tabs
                .with_stable_extension_capability("dom:write", grant_generation, |tabs| {
                    if value.len() > blueice_ipc::extension::MAX_TEXT_WRITE_BYTES {
                        Err(format!(
                            "text-control values cannot exceed {} bytes",
                            blueice_ipc::extension::MAX_TEXT_WRITE_BYTES
                        ))
                    } else {
                        tabs.check_extension_origin("dom:write", tab_id)
                            .and_then(|()| match tabs.get_mut(tab_id) {
                                Some(page) => page.set_text_input_value(node_id, value),
                                None => Err(format!("unknown tab {}", tab_id.as_u64())),
                            })
                    }
                })
                .and_then(|result| result);
            if result.is_ok() {
                // Mirror first-party SetValue: script listeners observe the
                // core-owned new value before observers receive its frame.
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
            grant_generation,
            reply,
        } => {
            let tab_id = TabId::from_u64(tab_id);
            let node_id = NodeId::from_u64(node_id);
            let result = tabs
                .with_stable_extension_capability("dom:write", grant_generation, |tabs| {
                    tabs.check_extension_origin("dom:write", tab_id)
                        .and_then(|()| match tabs.get_mut(tab_id) {
                            Some(page) => page.set_checkbox_checked(node_id, checked),
                            None => Err(format!("unknown tab {}", tab_id.as_u64())),
                        })
                })
                .and_then(|result| result);
            if result.is_ok() {
                // Match the text-input extension operation: page event
                // handlers see core's new state before the shared frame is
                // published to observers.
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
            grant_generation,
            reply,
        } => {
            let tab_id = TabId::from_u64(tab_id);
            let node_id = NodeId::from_u64(node_id);
            let result = tabs
                .with_stable_extension_capability("dom:write", grant_generation, |tabs| {
                    if value.len() > blueice_ipc::extension::MAX_TEXT_WRITE_BYTES {
                        Err(format!(
                            "text-control values cannot exceed {} bytes",
                            blueice_ipc::extension::MAX_TEXT_WRITE_BYTES
                        ))
                    } else {
                        tabs.check_extension_origin("dom:write", tab_id)
                            .and_then(|()| match tabs.get_mut(tab_id) {
                                Some(page) => page.set_textarea_value(node_id, value),
                                None => Err(format!("unknown tab {}", tab_id.as_u64())),
                            })
                    }
                })
                .and_then(|result| result);
            if result.is_ok() {
                // Match the other constrained form writes: event handlers
                // see the core-owned value before observers receive a frame.
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
        ExtensionPageRequest::SetVisibleLeafText {
            tab_id,
            node_id,
            value,
            grant_generation,
            reply,
        } => {
            let tab_id = TabId::from_u64(tab_id);
            let node_id = NodeId::from_u64(node_id);
            let result = tabs
                .with_stable_extension_capability("dom:write", grant_generation, |tabs| {
                    tabs.check_extension_origin("dom:write", tab_id)
                        .and_then(|()| match tabs.get_mut(tab_id) {
                            Some(page) => page.set_visible_leaf_text(node_id, value),
                            None => Err(format!("unknown tab {}", tab_id.as_u64())),
                        })
                })
                .and_then(|result| result);
            if result.is_ok() {
                let page = tabs
                    .get_mut(tab_id)
                    .expect("the extension target tab remains live");
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
        ExtensionPageRequest::SetVisibleTextContent {
            tab_id,
            node_id,
            value,
            grant_generation,
            reply,
        } => {
            let tab_id = TabId::from_u64(tab_id);
            let node_id = NodeId::from_u64(node_id);
            let result = tabs
                .with_stable_extension_capability("dom:write", grant_generation, |tabs| {
                    tabs.check_extension_origin("dom:write", tab_id)
                        .and_then(|()| match tabs.get_mut(tab_id) {
                            Some(page) => page.set_visible_text_content(node_id, value),
                            None => Err(format!("unknown tab {}", tab_id.as_u64())),
                        })
                })
                .and_then(|result| result);
            if result.is_ok() {
                let page = tabs
                    .get_mut(tab_id)
                    .expect("the extension target tab remains live");
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
            grant_generation,
            reply,
        } => {
            let tab_id = TabId::from_u64(tab_id);
            let node_id = NodeId::from_u64(node_id);
            let result = tabs
                .with_stable_extension_capability("dom:write", grant_generation, |tabs| {
                    tabs.check_extension_origin("dom:write", tab_id)
                        .and_then(|()| match tabs.get_mut(tab_id) {
                            Some(page) => page.set_range_input_value(node_id, value),
                            None => Err(format!("unknown tab {}", tab_id.as_u64())),
                        })
                })
                .and_then(|result| result);
            if result.is_ok() {
                // A range is one constrained form control, so listeners see
                // its committed core state before the shared observer frame.
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
            grant_generation,
            reply,
        } => {
            let tab_id = TabId::from_u64(tab_id);
            let node_id = NodeId::from_u64(node_id);
            let result = tabs
                .with_stable_extension_capability("dom:write", grant_generation, |tabs| {
                    tabs.check_extension_origin("dom:write", tab_id)
                        .and_then(|()| match tabs.get_mut(tab_id) {
                            Some(page) => page.set_radio_checked(node_id),
                            None => Err(format!("unknown tab {}", tab_id.as_u64())),
                        })
                })
                .and_then(|result| result);
            if result.is_ok() {
                // A selected radio can clear other controls in its core-owned
                // group, so publish one post-mutation input/change pair and a
                // single shared frame after the entire group update.
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
            grant_generation,
            reply,
        } => {
            let tab_id = TabId::from_u64(tab_id);
            let node_id = NodeId::from_u64(node_id);
            let result = tabs
                .with_stable_extension_capability("dom:write", grant_generation, |tabs| {
                    tabs.check_extension_origin("dom:write", tab_id)
                        .and_then(|()| match tabs.get_mut(tab_id) {
                            Some(page) => page.select_option(node_id),
                            None => Err(format!("unknown tab {}", tab_id.as_u64())),
                        })
                })
                .and_then(|result| result);
            if result.is_ok() {
                // A single-select transition can clear another option, so
                // dispatch one input/change pair and publish one post-update
                // frame only after core has completed the whole group change.
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
            grant_generation,
            url,
            reply,
        } => {
            let result = tabs
                .with_stable_extension_capability("network:intercept", grant_generation, |tabs| {
                    tabs.add_extension_navigation_block_rule(connection_id, url)
                })
                .and_then(|result| result);
            let _ = reply.send(result);
        }
        ExtensionPageRequest::RegisterNetworkBlockHost {
            connection_id,
            grant_generation,
            host,
            reply,
        } => {
            let result = tabs
                .with_stable_extension_capability("network:intercept", grant_generation, |tabs| {
                    tabs.add_extension_navigation_block_host_rule(connection_id, host)
                })
                .and_then(|result| result);
            let _ = reply.send(result);
        }
        ExtensionPageRequest::RegisterNetworkBlockPathPrefix {
            connection_id,
            grant_generation,
            host,
            path_prefix,
            reply,
        } => {
            let result = tabs
                .with_stable_extension_capability("network:intercept", grant_generation, |tabs| {
                    tabs.add_extension_navigation_block_path_prefix_rule(
                        connection_id,
                        host,
                        path_prefix,
                    )
                })
                .and_then(|result| result);
            let _ = reply.send(result);
        }
        ExtensionPageRequest::RegisterNetworkRedirectUrl {
            connection_id,
            grant_generation,
            source_url,
            target_url,
            reply,
        } => {
            let result = tabs
                .with_stable_extension_capability("network:intercept", grant_generation, |tabs| {
                    tabs.add_extension_navigation_redirect_rule(
                        connection_id,
                        source_url,
                        target_url,
                    )
                })
                .and_then(|result| result);
            let _ = reply.send(result);
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
    /// The assistant's translation of a cleared page's text, obtained on the
    /// navigation thread; `None` keeps the page as fetched.
    translations: Option<Vec<String>>,
}

/// A summarize or organize task that finished on its background thread.
struct AssistantCompletion {
    reply_tab: Option<u64>,
    request_id: Option<u64>,
    kind: blueice_ipc::AssistantTaskKind,
    source_url: Option<String>,
    request: Option<String>,
    outcome: Result<String, String>,
}

/// Starts a summarize or organize task for `target`'s shown text.
///
/// Refusals that can be decided immediately (no assistant, unknown tab, an
/// invalid instruction, a page with no text) are answered with an `Error` at
/// once. Otherwise the assistant is asked on a background thread, so `core`
/// keeps serving every other tab and client, and the result arrives later as a
/// reply carrying the same `request_id` (see [`finish_assistant_task`]).
fn begin_assistant_task<S: Write>(
    tabs: &mut TabManager,
    stream: &mut S,
    reply_tab: Option<u64>,
    request_id: Option<u64>,
    target: TabId,
    task: ClientMessage,
    assistant_tx: &mpsc::Sender<AssistantCompletion>,
) -> io::Result<()> {
    let Some(page) = tabs.get(target) else {
        return write_unknown_tab_error(stream, request_id, target);
    };
    let Some(socket) = tabs.assistant_socket() else {
        return write_error(
            stream,
            reply_tab,
            request_id,
            "the assistant is unavailable: blueice-core was started without one".to_string(),
        );
    };
    let (kind, instruction) = match task {
        ClientMessage::OrganizePage { instruction } => {
            (blueice_ipc::AssistantTaskKind::Organized, Some(instruction))
        }
        _ => (blueice_ipc::AssistantTaskKind::Summary, None),
    };
    let text = page.visible_text();
    if text.is_empty() {
        return write_error(
            stream,
            reply_tab,
            request_id,
            "this page has no text for the assistant to read".to_string(),
        );
    }
    // Validate exactly what will be sent, so an out-of-bounds instruction is an
    // immediate error rather than a failure reported only after a thread ran.
    let checked = match &instruction {
        Some(instruction) => blueice_ipc::assistant::AssistantRequest::Organize {
            request_id: 0,
            text: text.clone(),
            instruction: instruction.clone(),
        },
        None => blueice_ipc::assistant::AssistantRequest::Summarize {
            request_id: 0,
            text: text.clone(),
        },
    };
    if let Err(reason) = checked.validate() {
        return write_error(stream, reply_tab, request_id, reason);
    }
    let source_url = page.url().map(str::to_string);
    let tx = assistant_tx.clone();
    thread::spawn(move || {
        let deadline = crate::assistant_client::DEFAULT_TASK_DEADLINE;
        let outcome = match &instruction {
            Some(instruction) => {
                crate::assistant_client::organize_text(&socket, deadline, &text, instruction)
            }
            None => crate::assistant_client::summarize_text(&socket, deadline, &text),
        };
        let _ = tx.send(AssistantCompletion {
            reply_tab,
            request_id,
            kind,
            source_url,
            request: instruction,
            outcome,
        });
    });
    Ok(())
}

/// Records a finished task on the shared `about:assistant` panel, answers the
/// request that started it, and publishes a fresh frame for every tab showing
/// the panel. A failure is recorded and answered too: nothing else would tell
/// the person why no result appeared.
fn finish_assistant_task<S: Write>(
    tabs: &mut TabManager,
    stream: &mut S,
    frame_dir: &Path,
    generation: &mut u64,
    done: AssistantCompletion,
) -> io::Result<()> {
    let panel_kind = match done.kind {
        blueice_ipc::AssistantTaskKind::Summary => crate::assistant_page::PanelKind::Summary,
        blueice_ipc::AssistantTaskKind::Organized => crate::assistant_page::PanelKind::Organized,
    };
    tabs.assistant_panel().push(
        panel_kind,
        done.source_url,
        done.request,
        done.outcome.clone(),
    );
    match done.outcome {
        Ok(text) => blueice_ipc::write_server_message_with_ids(
            stream,
            done.reply_tab,
            done.request_id,
            &ServerMessage::AssistantResult {
                kind: done.kind,
                text,
            },
        )?,
        Err(reason) => write_error(stream, done.reply_tab, done.request_id, reason)?,
    }
    for id in tabs.refresh_assistant_panels() {
        let page = tabs.get_mut(id).expect("refresh only returns live tabs");
        send_frame(page, stream, frame_dir, generation, Some(id.as_u64()), None)?;
    }
    Ok(())
}

/// Replies with the core-wide translation language and the addressed tab's
/// translation availability and shown/original state.
fn write_translation_state<S: Write>(
    tabs: &TabManager,
    stream: &mut S,
    reply_tab: Option<u64>,
    request_id: Option<u64>,
    target: TabId,
) -> io::Result<()> {
    let page = tabs.get(target).expect("the caller checked the tab exists");
    blueice_ipc::write_server_message_with_ids(
        stream,
        reply_tab,
        request_id,
        &ServerMessage::TranslationState {
            language: tabs.translation_language().map(str::to_string),
            available: page.has_translation(),
            shown: page.translation_shown(),
        },
    )
}

/// Translates a *cleared* page on the navigation thread, so the session loop
/// never waits on the assistant. The gatekeeper has already reviewed the
/// original HTML by the time an outcome is `Cleared`; every other outcome, an
/// unset assistant, or any assistant failure yields `None` (the original page).
fn translate_cleared(
    outcome: &NavOutcome,
    config: Option<&crate::assistant_client::AssistantConfig>,
) -> Option<Vec<String>> {
    match (outcome, config) {
        (NavOutcome::Cleared { html, .. }, Some(config)) => {
            crate::assistant_client::translate_html(config, html)
        }
        _ => None,
    }
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
            let translation = tabs.translation_config();
            thread::spawn(move || {
                let outcome = gatekeeper_client::check_and_fetch_with_navigation_rules(
                    tab_id,
                    url,
                    &socket,
                    navigation_rules,
                );
                let translations = translate_cleared(&outcome, translation.as_ref());
                let _ = tx.send(Completion {
                    tab_id,
                    seq,
                    request_id,
                    kind,
                    outcome,
                    translations,
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
    // Every navigation supersedes an older in-flight fetch for this tab
    // before choosing its synchronous or asynchronous path, so a built-in
    // page or an invalid-URL response cancels a stale fetch just as an
    // http(s) navigation does.
    let this_seq = supersede_pending_navigation(pending_nav_seq, tab_id);
    if tabs.navigate_to_built_in(tab_id, &url) {
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

    let tx = completion_tx.clone();
    let socket = gatekeeper_socket.to_path_buf();
    let translation = tabs.translation_config();
    thread::spawn(move || {
        let outcome = gatekeeper_client::check_and_fetch_with_navigation_rules(
            tab_id,
            url,
            &socket,
            navigation_rules,
        );
        let translations = translate_cleared(&outcome, translation.as_ref());
        let _ = tx.send(Completion {
            tab_id,
            seq: this_seq,
            request_id,
            kind,
            outcome,
            translations,
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
    page_script_runtime: &mut PageScriptRuntime<'_>,
    script_requests: Option<&ScriptRequestReceiver>,
    extension_events: Option<&mpsc::SyncSender<ExtensionRuntimeEvent>>,
) -> io::Result<bool> {
    let Completion {
        tab_id,
        seq,
        request_id,
        kind,
        outcome,
        translations,
    } = completion;
    if pending_nav_seq.get(&tab_id) != Some(&seq) {
        return Ok(false); // superseded by a later navigation to this tab
    }
    if tabs.get(tab_id).is_none() {
        return Ok(false); // the tab closed while this navigation was pending
    }
    let reply_tab = Some(tab_id.as_u64());
    match outcome {
        NavOutcome::Cleared {
            clearance,
            final_url,
            html,
            status,
            content_type,
            request_url,
            redirects,
        } => {
            let response = blueice_ipc::extension::NetworkResponseInfo {
                method: "GET".to_string(),
                final_url: final_url.clone(),
                status,
                content_type,
            };
            let trace = blueice_ipc::extension::NetworkTraceInfo {
                request_url,
                redirects,
                response,
            };
            let committed = match &kind {
                PendingKind::History(direction) => tabs.apply_fetched_history_navigation(
                    tab_id,
                    *direction,
                    clearance,
                    &final_url,
                    &html,
                    translations.as_deref(),
                    trace,
                ),
                PendingKind::Navigate | PendingKind::OpenTab => {
                    tabs.apply_fetched_navigation(
                        tab_id,
                        clearance,
                        &final_url,
                        &html,
                        translations.as_deref(),
                        trace,
                    );
                    true
                }
            };
            if !committed {
                // The history entry disappeared before completion. This should
                // only be reachable if an internal caller changes the cursor
                // without advancing `pending_nav_seq`; fail safely rather than
                // applying the response to an unrelated document.
                return Ok(false);
            }
            if is_downloads_url(&final_url) {
                downloads_refresher.begin_visit(tab_id);
            }
            // A configured runner observes the loaded document before its
            // first success reply/frame. This preserves future DOM script
            // semantics while the default session has no runner at all.
            synchronize_page_script_runtime(page_script_runtime, tabs, script_requests)?;
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
            )?;
            Ok(true)
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
        )
        .map(|()| false),
        NavOutcome::ExtensionRuleBlocked { url } => write_error(
            stream,
            reply_tab,
            request_id,
            format!("navigation blocked by a declarative extension rule: {url}"),
        )
        .map(|()| false),
        NavOutcome::FetchFailed { message } => {
            write_error(stream, reply_tab, request_id, message).map(|()| false)
        }
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

#[cfg(all(test, unix))]
#[path = "session/tests.rs"]
mod tests;

#[cfg(all(test, unix))]
mod feature_tests;
