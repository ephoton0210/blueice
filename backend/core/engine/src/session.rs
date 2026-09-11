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
use crate::{Page, TabId, TabManager};
use blueice_dom::NodeId;
use blueice_ipc::{shm, ClientMessage, NodeAction, ServerMessage, TabSummary};
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
/// `GetRepresentation`, `ActOn`, `Highlight`, `GetDom`, `CloseTab`) is
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

        while let Ok(completion) = completion_rx.try_recv() {
            apply_completion(
                tabs,
                stream,
                frame_dir,
                generation,
                &pending_nav_seq,
                completion,
            )?;
        }
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
fn apply_completion<S: Write>(
    tabs: &mut TabManager,
    stream: &mut S,
    frame_dir: &Path,
    generation: &mut u64,
    pending_nav_seq: &HashMap<TabId, u64>,
    completion: Completion,
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
    let Some(page) = tabs.get_mut(tab_id) else {
        return Ok(()); // the tab closed while this navigation was pending
    };
    let reply_tab = Some(tab_id.as_u64());
    match outcome {
        NavOutcome::Cleared {
            clearance,
            final_url,
            html,
        } => {
            page.apply_fetched(clearance, &final_url, &html);
            reply_success(
                page,
                stream,
                frame_dir,
                generation,
                reply_tab,
                request_id,
                &kind,
                tab_id.as_u64(),
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

    fn client_pair() -> (UnixStream, UnixStream) {
        UnixStream::pair().unwrap()
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
    fn synchronous_navigation_paths_supersede_pending_navigation_sequences() {
        let dir = temp_frame_dir("sync-navigation-supersedes");
        std::fs::create_dir_all(&dir).unwrap();
        let mut page = Page::new(320.0, 200.0);
        let mut stream = Vec::new();
        let mut generation = 0;
        let tab = TabId::from_u64(7);
        let mut pending_nav_seq = HashMap::from([(tab, 41)]);
        let (completion_tx, _completion_rx) = mpsc::channel();
        let unused_gatekeeper = unique_gatekeeper_socket_path("sync-navigation-supersedes");

        begin_gated_navigation(
            &mut page,
            &mut stream,
            &dir,
            &mut generation,
            Some(tab.as_u64()),
            None,
            tab,
            "about:blank".to_string(),
            PendingKind::Navigate,
            &mut pending_nav_seq,
            &completion_tx,
            &unused_gatekeeper,
        )
        .unwrap();
        assert_eq!(pending_nav_seq[&tab], 42);

        begin_gated_navigation(
            &mut page,
            &mut stream,
            &dir,
            &mut generation,
            Some(tab.as_u64()),
            None,
            tab,
            "file:///not-allowed".to_string(),
            PendingKind::Navigate,
            &mut pending_nav_seq,
            &completion_tx,
            &unused_gatekeeper,
        )
        .unwrap();
        assert_eq!(pending_nav_seq[&tab], 43);

        let _ = std::fs::remove_dir_all(dir);
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
        assert!(dump.contains("<div>"), "a bare div has no AI-representation role but must still appear in the full DOM dump: {dump}");

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
                url: "about:blank".to_string()
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
        assert!(start.elapsed() < Duration::from_millis(150), "tab_b's reply must arrive well before tab_a's stalled gatekeeper check resolves, took {:?}", start.elapsed());

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
}
