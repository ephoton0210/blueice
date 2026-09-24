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

use crate::gatekeeper_client::{self, NavOutcome};
#[cfg(unix)]
use crate::script::javascript_child::{OutOfProcessJavaScriptPageExecutor, PageHostConnection};
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
    Page, TabId, TabManager,
};
use blueice_dom::NodeId;
use blueice_ipc::{
    shm, BlueJsScriptExecutionOutcome, BlueJsScriptExecutionReport, BlueJsScriptKind,
    BlueTsScriptExecutionOutcome, BlueTsScriptExecutionReport, BlueTsScriptKind, ClientMessage,
    NodeAction, ServerMessage, TabSummary,
};
use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::path::Path;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

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
/// frames are written (see [`blueice_ipc::shm`]); `generation` is a
/// single, session-wide frame-sequence counter shared across every tab
/// (not one per tab) -- every tab's `FrameReady` still gets the next
/// global monotonic number, so `blueice_ipc::shm` needs no per-tab
/// awareness at all (filenames stay collision-free by construction),
/// and the AI-facing "same generation = same render pass" property
/// still holds across tabs. `gatekeeper_socket` is where a gated
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
/// PLAN.md`'s minimal first slice): every per-tab-scoped message
/// (`Navigate`, `Resize`, `Click`, `Hover`, `Scroll`,
/// `GetRepresentation`, `ActOn`, `Highlight`, `GetDom`,
/// `GetBlueTsScriptReports`, `CloseTab`) is
/// addressed by the envelope's `tab_id` -- `None` resolves to
/// [`TabManager::default_tab`], reproducing pre-Phase-16 single-`Page`
/// behavior byte-for-byte for a client that never sends `OpenTab`. A
/// `tab_id` (explicit or defaulted) that doesn't resolve to a live tab
/// replies [`ServerMessage::Error`] -- a protocol-addressing error, not
/// the harmless no-op a stale `NodeId` already gets in [`Page::act`].
/// `OpenTab`/`ListTabs` aren't scoped to an existing tab at all (there's
/// no "current tab" concept `core` tracks -- see [`TabManager`]'s own
/// docs for why) and ignore any `tab_id` on the envelope.
pub fn run_session<S: Read + Write + ReadTimeout>(
    tabs: &mut TabManager,
    stream: &mut S,
    frame_dir: &Path,
    generation: &mut u64,
    gatekeeper_socket: &Path,
) -> io::Result<()> {
    run_session_with_script_requests(tabs, stream, frame_dir, generation, gatekeeper_socket, None)
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

    let (completion_tx, completion_rx) = mpsc::channel::<Completion>();
    let mut pending_nav_seq: HashMap<TabId, u64> = HashMap::new();

    let mut requests = requests;
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
                    ClientMessage::Navigate { url } => match tabs.get_mut(target) {
                        Some(page) => begin_gated_navigation(
                            page,
                            stream,
                            frame_dir,
                            generation,
                            reply_tab,
                            request_id,
                            target,
                            url,
                            PendingKind::Navigate,
                            &mut pending_nav_seq,
                            &completion_tx,
                            gatekeeper_socket,
                        )?,
                        None => write_unknown_tab_error(stream, request_id, target)?,
                    },
                    ClientMessage::Resize { width, height } => {
                        tabs.set_window_size(width as f64, height as f64);
                        match tabs.get_mut(target) {
                            Some(page) => {
                                page.resize(width as f64, height as f64);
                                send_frame(
                                    page, stream, frame_dir, generation, reply_tab, request_id,
                                )?;
                            }
                            None => write_unknown_tab_error(stream, request_id, target)?,
                        }
                    }
                    ClientMessage::Click { x, y } => match tabs.get_mut(target) {
                        Some(page) => {
                            if let Some(href) = page.click(x, y) {
                                begin_gated_navigation(
                                    page,
                                    stream,
                                    frame_dir,
                                    generation,
                                    reply_tab,
                                    request_id,
                                    target,
                                    href,
                                    PendingKind::Navigate,
                                    &mut pending_nav_seq,
                                    &completion_tx,
                                    gatekeeper_socket,
                                )?;
                            }
                        }
                        None => write_unknown_tab_error(stream, request_id, target)?,
                    },
                    ClientMessage::Scroll { delta_y } => match tabs.get_mut(target) {
                        Some(page) => {
                            page.scroll_by(delta_y);
                            send_frame(page, stream, frame_dir, generation, reply_tab, request_id)?;
                        }
                        None => write_unknown_tab_error(stream, request_id, target)?,
                    },
                    ClientMessage::Hover { x, y } => {
                        if let Some(page) = tabs.get_mut(target) {
                            page.hover_at(x, y);
                        } else {
                            write_unknown_tab_error(stream, request_id, target)?;
                        }
                    }
                    ClientMessage::GetRepresentation => match tabs.get_mut(target) {
                        Some(page) => {
                            let snapshot = page.snapshot(*generation, target.as_u64());
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
                    ClientMessage::ActOn { id, action } => match tabs.get_mut(target) {
                        Some(page) => {
                            let is_click = matches!(action, NodeAction::Click);
                            match page.act(NodeId::from_u64(id), action) {
                                Some(href) => begin_gated_navigation(
                                    page,
                                    stream,
                                    frame_dir,
                                    generation,
                                    reply_tab,
                                    request_id,
                                    target,
                                    href,
                                    PendingKind::Navigate,
                                    &mut pending_nav_seq,
                                    &completion_tx,
                                    gatekeeper_socket,
                                )?,
                                // A Click that didn't land on a link is a
                                // no-op, same as a coordinate Click
                                // elsewhere -- no reply.
                                None if is_click => {}
                                None => send_frame(
                                    page, stream, frame_dir, generation, reply_tab, request_id,
                                )?,
                            }
                        }
                        None => write_unknown_tab_error(stream, request_id, target)?,
                    },
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
                        &completion_tx,
                        gatekeeper_socket,
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
                            })
                            .collect();
                        blueice_ipc::write_server_message_with_id(
                            stream,
                            request_id,
                            &ServerMessage::Tabs(summaries),
                        )?;
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

        let mut synchronized_after_completion = false;
        while let Ok(completion) = completion_rx.try_recv() {
            synchronized_after_completion |= apply_completion(
                tabs,
                stream,
                frame_dir,
                generation,
                &pending_nav_seq,
                completion,
                &mut page_script_runtime,
                requests.script,
            )?;
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

/// Converts core-owned direct-page execution records into the public IPC
/// observation format. Deliberately map only the fixed report category: page
/// source, compiler diagnostics, and runtime values never cross this boundary.
fn inline_execution_reports(
    reports: Vec<DirectPageScriptExecutionReport>,
) -> Vec<BlueTsScriptExecutionReport> {
    reports
        .into_iter()
        .map(|report| match report {
            DirectPageScriptExecutionReport::Executed {
                tab_id,
                document_generation,
                ordinal,
                kind,
            } => BlueTsScriptExecutionReport {
                tab_id,
                document_generation,
                ordinal,
                kind: inline_script_kind(kind),
                outcome: BlueTsScriptExecutionOutcome::Executed,
            },
            DirectPageScriptExecutionReport::Rejected {
                tab_id,
                document_generation,
                ordinal,
                kind,
                message,
            } => BlueTsScriptExecutionReport {
                tab_id,
                document_generation,
                ordinal,
                kind: inline_script_kind(kind),
                outcome: BlueTsScriptExecutionOutcome::Rejected { category: message },
            },
        })
        .collect()
}

fn inline_script_kind(kind: DirectPageScriptKind) -> BlueTsScriptKind {
    match kind {
        DirectPageScriptKind::Classic => BlueTsScriptKind::Classic,
        DirectPageScriptKind::Module => BlueTsScriptKind::Module,
    }
}

/// Converts core-owned standard JavaScript execution records into the public
/// source-free observation format. As with BlueTS reports, no source,
/// diagnostic, bytecode, program identity, or completion value can cross this
/// control-plane query.
fn inline_javascript_execution_reports(
    reports: Vec<JavaScriptPageExecutionReport>,
) -> Vec<BlueJsScriptExecutionReport> {
    reports
        .into_iter()
        .map(|report| match report {
            JavaScriptPageExecutionReport::Executed {
                tab_id,
                document_generation,
                ordinal,
                kind,
            } => BlueJsScriptExecutionReport {
                tab_id,
                document_generation,
                ordinal,
                kind: inline_javascript_kind(kind),
                outcome: BlueJsScriptExecutionOutcome::Executed,
            },
            JavaScriptPageExecutionReport::Rejected {
                tab_id,
                document_generation,
                ordinal,
                kind,
                category,
            } => BlueJsScriptExecutionReport {
                tab_id,
                document_generation,
                ordinal,
                kind: inline_javascript_kind(kind),
                outcome: BlueJsScriptExecutionOutcome::Rejected {
                    category: category.to_string(),
                },
            },
        })
        .collect()
}

fn inline_javascript_kind(kind: crate::script::BlueJsPageScriptKind) -> BlueJsScriptKind {
    match kind {
        crate::script::BlueJsPageScriptKind::Classic => BlueJsScriptKind::Classic,
        crate::script::BlueJsPageScriptKind::Module => BlueJsScriptKind::Module,
    }
}

/// Converts the private child-host BlueTS outcomes into the existing
/// language-specific, source-free control-plane report shape. The direct
/// child remains the owner of compiler artifacts and runtime state.
fn child_blue_ts_execution_reports(
    reports: Vec<BlueTsPageExecutionReport>,
) -> Vec<BlueTsScriptExecutionReport> {
    reports
        .into_iter()
        .map(|report| match report {
            BlueTsPageExecutionReport::Executed {
                tab_id,
                document_generation,
                ordinal,
                kind,
            } => BlueTsScriptExecutionReport {
                tab_id,
                document_generation,
                ordinal,
                kind: child_blue_ts_kind(kind),
                outcome: BlueTsScriptExecutionOutcome::Executed,
            },
            BlueTsPageExecutionReport::Rejected {
                tab_id,
                document_generation,
                ordinal,
                kind,
                category,
            } => BlueTsScriptExecutionReport {
                tab_id,
                document_generation,
                ordinal,
                kind: child_blue_ts_kind(kind),
                outcome: BlueTsScriptExecutionOutcome::Rejected {
                    category: category.to_string(),
                },
            },
        })
        .collect()
}

fn child_blue_ts_kind(kind: crate::script::direct_page::DirectPageScriptKind) -> BlueTsScriptKind {
    match kind {
        crate::script::direct_page::DirectPageScriptKind::Classic => BlueTsScriptKind::Classic,
        crate::script::direct_page::DirectPageScriptKind::Module => BlueTsScriptKind::Module,
    }
}

fn synchronize_page_script_runtime(
    page_script_runtime: &mut PageScriptRuntime<'_>,
    tabs: &mut TabManager,
    script_requests: Option<&ScriptRequestReceiver>,
) -> io::Result<()> {
    if let Some(direct_page_host) = page_script_runtime.direct_page_host.as_deref_mut() {
        direct_page_host.synchronize_tabs(tabs).map_err(|error| {
            io::Error::other(format!(
                "direct page lifecycle synchronization failed: {error}"
            ))
        })?;
    }
    synchronize_inline_page_executor(&mut page_script_runtime.inline_page_executor, tabs)?;
    synchronize_javascript_executor(
        &mut page_script_runtime.javascript_executor,
        tabs,
        script_requests,
    )?;
    Ok(())
}

fn synchronize_javascript_executor(
    javascript_executor: &mut Option<&mut dyn PageJavaScriptExecutor>,
    tabs: &mut TabManager,
    script_requests: Option<&ScriptRequestReceiver>,
) -> io::Result<()> {
    if let Some(javascript_executor) = javascript_executor.as_deref_mut() {
        javascript_executor.synchronize_and_execute_serving_script(tabs, script_requests)?;
    }
    Ok(())
}

fn synchronize_inline_page_executor(
    inline_page_executor: &mut Option<&mut DirectPageInlineExecutor>,
    tabs: &TabManager,
) -> io::Result<()> {
    if let Some(inline_page_executor) = inline_page_executor.as_deref_mut() {
        inline_page_executor
            .synchronize_and_execute(tabs)
            .map_err(|error| io::Error::other(format!("inline page execution failed: {error}")))?;
    }
    Ok(())
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

/// Starts a gated navigation to `url` for `tab_id`. Built-in `about:`
/// pages ([`crate::page::built_in_page`]) are handled entirely
/// synchronously here -- loaded directly and replied to before this
/// returns, and *never* going
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
/// Every navigation bumps `tab_id`'s entry in `pending_nav_seq` before
/// choosing its synchronous or asynchronous path. That makes a built-in
/// page or invalid-URL response supersede an older fetch just as an http(s)
/// navigation does. A well-formed http(s) URL then hands the rest of the
/// work (both gatekeeper stages, the fetch) to a background thread tagged
/// with the sequence number just bumped to -- this is what lets
/// [`apply_completion`] later recognize and discard a stale completion.
#[allow(clippy::too_many_arguments)]
fn begin_gated_navigation<S: Write>(
    page: &mut Page,
    stream: &mut S,
    frame_dir: &Path,
    generation: &mut u64,
    reply_tab: Option<u64>,
    request_id: Option<u64>,
    tab_id: TabId,
    url: String,
    kind: PendingKind,
    pending_nav_seq: &mut HashMap<TabId, u64>,
    completion_tx: &mpsc::Sender<Completion>,
    gatekeeper_socket: &Path,
) -> io::Result<()> {
    let seq = pending_nav_seq.entry(tab_id).or_insert(0);
    *seq += 1;
    let this_seq = *seq;

    if let Some(html) = crate::page::built_in_page(&url) {
        page.load_html_str(&html, Some(url));
        return reply_success(
            page,
            stream,
            frame_dir,
            generation,
            reply_tab,
            request_id,
            &kind,
            tab_id.as_u64(),
        );
    }
    if let Err(e) = blueice_net::validate_url_scheme(&url) {
        return write_error(stream, reply_tab, request_id, e.to_string());
    }

    let tx = completion_tx.clone();
    let socket = gatekeeper_socket.to_path_buf();
    thread::spawn(move || {
        let outcome = gatekeeper_client::check_and_fetch(tab_id, url, &socket);
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
    page: &Page,
    stream: &mut S,
    frame_dir: &Path,
    generation: &mut u64,
    reply_tab: Option<u64>,
    request_id: Option<u64>,
    kind: &PendingKind,
    tab_id: u64,
) -> io::Result<()> {
    match kind {
        PendingKind::Navigate => reply_navigated(page, stream, reply_tab, request_id)?,
        PendingKind::OpenTab => blueice_ipc::write_server_message_with_ids(
            stream,
            reply_tab,
            request_id,
            &ServerMessage::TabOpened {
                tab_id,
                url: page.url().map(str::to_string),
            },
        )?,
    }
    send_frame(page, stream, frame_dir, generation, reply_tab, request_id)
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
#[allow(clippy::too_many_arguments)] // Navigation reply state and script dispatch stay on this session thread.
fn apply_completion<S: Write>(
    tabs: &mut TabManager,
    stream: &mut S,
    frame_dir: &Path,
    generation: &mut u64,
    pending_nav_seq: &HashMap<TabId, u64>,
    completion: Completion,
    page_script_runtime: &mut PageScriptRuntime<'_>,
    script_requests: Option<&ScriptRequestReceiver>,
) -> io::Result<bool> {
    let Completion {
        tab_id,
        seq,
        request_id,
        kind,
        outcome,
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
        } => {
            tabs.get_mut(tab_id)
                .expect("the checked live tab must remain available on this session thread")
                .apply_fetched(clearance, &final_url, &html);
            // A configured runner observes the loaded document before its
            // first success reply/frame. This preserves future DOM script
            // semantics while the default session has no runner at all.
            synchronize_page_script_runtime(page_script_runtime, tabs, script_requests)?;
            let page = tabs
                .get(tab_id)
                .expect("the session thread exclusively owns the checked tab");
            reply_success(
                page,
                stream,
                frame_dir,
                generation,
                reply_tab,
                request_id,
                &kind,
                tab_id.as_u64(),
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
    completion_tx: &mpsc::Sender<Completion>,
    gatekeeper_socket: &Path,
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
    let page = tabs
        .get_mut(new_id)
        .expect("a tab this function just created must exist");
    begin_gated_navigation(
        page,
        stream,
        frame_dir,
        generation,
        Some(new_id.as_u64()),
        request_id,
        new_id,
        url,
        PendingKind::OpenTab,
        pending_nav_seq,
        completion_tx,
        gatekeeper_socket,
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
    page: &Page,
    stream: &mut S,
    frame_dir: &Path,
    generation: &mut u64,
    tab_id: Option<u64>,
    request_id: Option<u64>,
) -> io::Result<()> {
    let pixmap = page.render_visible();
    *generation += 1;
    let path = shm::write_frame(frame_dir, *generation, &pixmap.pixels)?;
    blueice_ipc::write_server_message_with_ids(
        stream,
        tab_id,
        request_id,
        &ServerMessage::FrameReady {
            shm_path: path.to_string_lossy().into_owned(),
            width: pixmap.width,
            height: pixmap.height,
            generation: *generation,
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
