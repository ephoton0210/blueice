// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `blueice-launcher`: the minimal first slice of
//! `phase-8-live-core-hotswap/PLAN.md`'s supervisor role -- a rendezvous
//! broker letting a human's `frontend` and an AI's `mcp-server` (or any
//! other `blueice-ipc` client) share *one* running `core` instance and
//! its `Page`s, instead of each spawning/observing a separate one.
//! That's the exact dual-track split (a human on one instance, an AI
//! driving a separate one) CLAUDE.md's core goal rejects -- just as
//! fully present *within* BlueIce's own process fleet, before this
//! crate existed, as the external "human on a real browser, AI on
//! headless Chromium" case the whole project exists to avoid.
//!
//! **No changes were needed to `blueice-core`/`blueice_engine::session`
//! for the rendezvous-broker slice**: `core` still only ever sees one
//! connection (this crate's), exactly as it always has. The multi-client
//! fan-out/fan-in work all happens here: every external client's
//! [`blueice_ipc::ClientMessage`]s are forwarded into that one connection
//! ([`forward_client_to_core`]), and every [`blueice_ipc::ServerMessage`]
//! `core` sends back is broadcast to *every* currently-connected external
//! client ([`broadcast_core_to_clients`]) -- not just whichever one's
//! action triggered it, which is what actually delivers "same render
//! pass."
//!
//! **This crate's second job**, per `phase-8-live-core-hotswap/PLAN.md`'s
//! "Wiring design": a minimal-slice, manually-triggered *cutover*
//! mechanism -- spawn a fresh `core` (v2), replay v1's currently-open
//! tabs into it, health-check it, and only then cut every
//! already-registered external client's traffic over to it before
//! tearing v1 down, all without dropping a single already-connected
//! client. Triggered over a small, launcher-internal control protocol
//! ([`control`]), distinct from the external rendezvous socket. See
//! [`run_broker`]'s own docs for the generation-counter mechanism that
//! tells "core genuinely died" apart from "we deliberately superseded
//! it."

pub mod control;
pub mod memory_pressure;
pub mod supervisor;

pub use control::default_control_socket_path;

use blueice_ipc::{
    read_client_message_with_ids, read_server_message_with_id, read_server_message_with_ids, write_client_message_with_id, write_client_message_with_ids,
    write_server_message_with_ids, ClientMessage, ServerMessage, TabSummary,
};
use std::io;
use std::net::Shutdown;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

/// The rendezvous socket path clients connect to, when none is given
/// explicitly: `$XDG_RUNTIME_DIR/blueice/core.sock`, falling back to
/// `/tmp/blueice-<uid>/core.sock` if `XDG_RUNTIME_DIR` isn't set. Kept
/// per-user (never a single system-wide path) so two different users on
/// the same machine -- or two independent BlueIce sessions started by
/// the same user with an explicit override -- never collide.
pub fn default_rendezvous_socket_path() -> PathBuf {
    rendezvous_socket_dir().join("core.sock")
}

/// The directory both [`default_rendezvous_socket_path`] and
/// [`control::default_control_socket_path`] place their respective
/// socket in -- split out so the two stay in the same per-user
/// directory without duplicating the `XDG_RUNTIME_DIR`/`/tmp` fallback
/// logic.
pub(crate) fn rendezvous_socket_dir() -> PathBuf {
    match std::env::var_os("XDG_RUNTIME_DIR") {
        Some(dir) => PathBuf::from(dir).join("blueice"),
        None => std::env::temp_dir().join(format!("blueice-{}", unsafe { libc_getuid() })),
    }
}

// A tiny, deliberately minimal stand-in for `libc::getuid()` rather than
// adding a whole `libc` dependency for one syscall: reads the real UID
// via the `/proc/self/status` line every Linux (BlueIce's only
// currently-supported target -- see `blueice-core`'s own `UnixListener`
// dependency, already Unix-only) exposes, falling back to the process
// ID if that ever fails so the path is still unique per-process rather
// than panicking.
unsafe fn libc_getuid() -> u32 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|status| status.lines().find_map(|line| line.strip_prefix("Uid:")).and_then(|rest| rest.split_whitespace().next()).and_then(|s| s.parse().ok()))
        .unwrap_or_else(std::process::id)
}

/// One [`ServerMessage`] reply as relayed through the broker's
/// per-client fan-out channel, tagged with the `tab_id`/`request_id`
/// it arrived with. Replaces a bare `Sender<ServerMessage>` (which had
/// no field to carry either id at all) -- the fix for a real
/// pre-existing bug: without this, `tab_id`/`request_id` were silently
/// dropped on every message relayed through the broker, contradicting
/// `phase-16-multi-tab-and-tab-groups/PLAN.md`'s claim that a client
/// sharing the broker's connection can always tell which tab a reply is
/// about. See `forward_client_to_core`/`broadcast_core_to_clients`'s
/// own docs for the other half of this fix (the read/write side).
#[derive(Debug, Clone, PartialEq)]
pub struct TaggedServerMessage {
    pub tab_id: Option<u64>,
    pub request_id: Option<u64>,
    pub message: ServerMessage,
}

/// Forwards every [`blueice_ipc::ClientMessage`] read from `client`
/// (along with its `tab_id`/`request_id`, per the fix above) into
/// `core`, until `client` disconnects or errors, or writing to `core`
/// fails (the shared core connection is gone). Runs on its own thread
/// per connected external client; `core` is behind a [`Mutex`] because
/// multiple such threads write into the same one `core` connection
/// concurrently.
pub fn forward_client_to_core(mut client: UnixStream, core: Arc<Mutex<UnixStream>>) {
    loop {
        let (tab_id, request_id, msg) = match read_client_message_with_ids(&mut client) {
            Ok(triple) => triple,
            Err(_) => return,
        };
        let mut core = core.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if write_client_message_with_ids(&mut *core, tab_id, request_id, &msg).is_err() {
            return;
        }
    }
}

/// How long a single write to one client's socket may block before
/// that client is treated as unresponsive -- see [`register_client`]'s
/// docs for why this exists at all.
const CLIENT_WRITE_TIMEOUT: Duration = Duration::from_secs(5);

/// Reads every [`blueice_ipc::ServerMessage`] `core` sends (along with
/// its `tab_id`/`request_id`), until it disconnects or errors (`core`
/// crashed, exited, or -- during a cutover, see [`run_broker`] -- was
/// deliberately superseded), fanning each one out to every client
/// currently in `clients` -- not just whichever client's action
/// triggered it. Fan-out is a non-blocking channel `send` per client
/// (see [`register_client`]'s docs for why this isn't a direct socket
/// write here); a client whose *channel* is gone -- its own writer
/// thread already exited, per [`register_client`] -- is dropped from
/// the list rather than treated as fatal to the broadcast itself.
pub fn broadcast_core_to_clients(mut core: UnixStream, clients: Arc<Mutex<Vec<Sender<TaggedServerMessage>>>>) {
    loop {
        let (tab_id, request_id, message) = match read_server_message_with_ids(&mut core) {
            Ok(triple) => triple,
            Err(_) => return,
        };
        let tagged = TaggedServerMessage { tab_id, request_id, message };
        let mut clients = clients.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        clients.retain(|client| client.send(tagged.clone()).is_ok());
    }
}

/// Accepts one already-connected external client: registers a channel
/// for [`broadcast_core_to_clients`] to fan messages out to, spawns
/// this client's own writer thread draining that channel onto its
/// socket, and spawns a second thread forwarding its incoming messages
/// into `core_writer`. Split out from the accept loop so it's
/// unit-testable with a [`UnixStream::pair`] fake client, without
/// needing a real [`std::os::unix::net::UnixListener`].
///
/// **Why a channel + a dedicated writer thread per client, not a
/// direct socket write from the shared broadcast loop**: a single
/// shared loop writing to every client's socket in turn, one after
/// another, means one client that stops reading (its kernel socket
/// buffer fills -- a large `Representation` nobody's draining is
/// enough) blocks that *one* write until it times out -- and while
/// blocked, the loop hasn't even reached any of the *other* clients
/// yet, so it stalls delivery to every one of them too, for the whole
/// timeout, even though they're reading just fine. Giving each client
/// its own channel and its own thread makes the shared loop's `send`
/// a cheap, non-blocking queue push (an unbounded `mpsc` channel never
/// blocks the sender), so a slow client only ever delays *its own*
/// delivery, never anyone else's -- restoring "one client, no matter
/// how slow, can't starve the others," which a bare write timeout on a
/// single shared thread cannot: it only bounds *how long* the stall
/// lasts, not whether it happens at all.
///
/// The write-half still gets a [`CLIENT_WRITE_TIMEOUT`], now purely to
/// eventually detect and prune a client that's truly gone (or stuck
/// forever) rather than merely behind -- once that write-half's own
/// thread gives up and exits, its `Sender`'s paired `Receiver` drops,
/// so the next broadcast's `send` to it fails and
/// [`broadcast_core_to_clients`] prunes it, the same way a plain
/// disconnect already does. This same self-pruning is what
/// [`capture_v1_tabs`]'s synthetic internal client relies on to clean
/// itself up, without needing any explicit client-identity tracking.
pub fn register_client(client: UnixStream, core_writer: Arc<Mutex<UnixStream>>, clients: Arc<Mutex<Vec<Sender<TaggedServerMessage>>>>) -> io::Result<()> {
    let mut write_half = client_write_half(&client)?;
    let (sender, receiver) = mpsc::channel::<TaggedServerMessage>();
    clients.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).push(sender);
    thread::spawn(move || {
        for msg in receiver {
            if write_server_message_with_ids(&mut write_half, msg.tab_id, msg.request_id, &msg.message).is_err() {
                return; // dropping `receiver` here is what prunes this client above
            }
        }
    });
    thread::spawn(move || forward_client_to_core(client, core_writer));
    Ok(())
}

/// Clones `client`'s write-half and gives it [`CLIENT_WRITE_TIMEOUT`] --
/// split out from [`register_client`] so the timeout is a plain, fast
/// unit test rather than one needing a real stalled write to observe.
fn client_write_half(client: &UnixStream) -> io::Result<UnixStream> {
    let write_half = client.try_clone()?;
    write_half.set_write_timeout(Some(CLIENT_WRITE_TIMEOUT))?;
    Ok(write_half)
}

/// Runs a [`broadcast_core_to_clients`] loop for `core_stream` on its
/// own thread, tagged with `my_generation`. This is the mechanism
/// [`run_broker`]/[`cutover`] use to distinguish "the active `core`
/// genuinely disconnected" (the whole launcher should exit, exactly as
/// before this phase's cutover mechanism existed) from "a cutover
/// deliberately superseded this connection with a newer one" (the
/// broker should keep running under the new generation, and this dying
/// thread should have no broker-wide effect):
///
/// When the broadcast loop ends (`core_stream` disconnected or errored,
/// for any reason), this checks whether `generation`'s *current* value
/// still equals `my_generation`:
/// - **Unchanged**: nothing has bumped `generation` since this thread
///   started, so this was NOT a deliberate supersession -- `core`
///   itself genuinely disconnected (crashed, exited, or a client's
///   `Shutdown` cascaded through it). `done` is signaled so
///   [`run_broker`] can return.
/// - **Changed**: a cutover already bumped `generation` (see
///   [`perform_swap`]) *before* deliberately closing this thread's own
///   `core_stream` -- this thread's death was expected. It exits
///   quietly without signaling `done`, leaving the broker running under
///   whichever generation is now current.
fn spawn_generation_tagged_broadcast(core_stream: UnixStream, clients: Arc<Mutex<Vec<Sender<TaggedServerMessage>>>>, generation: Arc<AtomicU64>, my_generation: u64, done: Sender<()>) {
    thread::spawn(move || {
        broadcast_core_to_clients(core_stream, clients);
        if generation.load(Ordering::SeqCst) == my_generation {
            let _ = done.send(());
        }
    });
}

/// Shared state one running broker operates on, and what [`cutover`]
/// swaps pieces of without disturbing already-registered external
/// clients -- `clients` itself is reused as-is across a cutover: v2's
/// new broadcast thread shares the exact same `Arc`
/// [`broadcast_core_to_clients`] was already fanning v1's traffic out
/// to, so no already-registered client ever needs to reconnect.
struct Broker {
    /// The one connection external clients' messages are currently
    /// forwarded into. Its *contents* (not the `Arc` itself) are
    /// swapped during a cutover, so [`forward_client_to_core`] threads
    /// (which re-acquire this mutex per message) naturally serialize
    /// against an in-flight swap with no new locking.
    core_writer: Arc<Mutex<UnixStream>>,
    clients: Arc<Mutex<Vec<Sender<TaggedServerMessage>>>>,
    /// Bumped by [`perform_swap`] *before* the old core connection is
    /// closed -- see [`spawn_generation_tagged_broadcast`]'s docs.
    generation: Arc<AtomicU64>,
    /// The `core` process currently backing this broker. `None` only in
    /// the brief window while [`run_broker`] is tearing everything
    /// down at the very end. Both `run_broker`'s final cleanup and
    /// every cutover's teardown of the *previous* core go through here,
    /// rather than a side channel.
    active_core: Mutex<Option<SpawnedCore>>,
    /// The window size every spawned `core` (v1 at startup, and every
    /// future v2) uses -- there is one physical `frontend` window
    /// regardless of which `core` is currently behind it.
    width: f64,
    height: f64,
    /// v1's original frame directory -- [`v2_frame_dir`] derives each
    /// future v2's own fresh directory from this, per
    /// `phase-8-live-core-hotswap/PLAN.md`'s flagged detail that
    /// sharing one frame directory between two simultaneously-live
    /// `core` instances would let their independently-zeroed
    /// `generation` counters collide on the same frame filename.
    frame_dir: PathBuf,
    /// Signaled exactly once, by whichever generation-tagged broadcast
    /// thread's own death is NOT a deliberate cutover supersession --
    /// what [`run_broker`] blocks on to know when the whole launcher
    /// should exit.
    done: Sender<()>,
}

/// A generous but bounded wait for [`capture_v1_tabs`]'s `Tabs` reply --
/// a few seconds, per `phase-8-live-core-hotswap/PLAN.md`'s "a
/// reasonable timeout... if it never arrives, that's a `CutoverFailed`."
const TAB_CAPTURE_TIMEOUT: Duration = Duration::from_secs(5);

/// A cheap, dependency-free, sufficiently-unique request id for the
/// launcher's own synthetic internal-client messages (`ListTabs` during
/// tab capture; `Navigate`/`OpenTab`/`ListTabs` during replay and the
/// post-replay health check) -- time-based rather than a small
/// sequential counter, so an accidental collision with another,
/// independently-numbered client's own (typically small, sequential)
/// request_id sharing the same broker is vanishingly unlikely. Mirrors
/// `blueice-mcp-server`'s own `fastrand_like_suffix`.
fn synthetic_request_id() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(0)
}

/// Captures v1's currently-open tab list, as a synthetic internal
/// client of the broker -- `core` only ever has the one real connection
/// `core_writer` holds; every external client's traffic (and this
/// synthetic one) is proxied through it, so this reuses the exact same
/// [`register_client`]-style registration rather than a side channel.
/// Filters the broadcast stream for the specific reply carrying the
/// request ID this call generated itself, exactly the discipline
/// `blueice-mcp-server`'s `send_and_drain` already established for
/// picking one reply out of shared, multi-client traffic --
/// reimplemented locally per this crate's existing duplicate-small-
/// helpers convention.
///
/// The synthetic client is never explicitly removed from `clients`:
/// once this call returns, its local `receiver` is dropped, so the
/// very next broadcast attempt to its paired `Sender` (still sitting in
/// `clients`) fails and [`broadcast_core_to_clients`]'s existing
/// dead-channel pruning removes it -- the same self-cleanup mechanism
/// already relied on for any other client's writer thread exiting, so
/// no new client-identity-tracking machinery is needed just for this.
fn capture_v1_tabs(core_writer: &Arc<Mutex<UnixStream>>, clients: &Arc<Mutex<Vec<Sender<TaggedServerMessage>>>>, timeout: Duration) -> Result<Vec<TabSummary>, String> {
    let (sender, receiver) = mpsc::channel::<TaggedServerMessage>();
    clients.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).push(sender);

    let request_id = synthetic_request_id();
    {
        let mut core = core_writer.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        write_client_message_with_id(&mut *core, Some(request_id), &ClientMessage::ListTabs).map_err(|e| format!("failed to send ListTabs to v1: {e}"))?;
    }

    let deadline = Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err("timed out waiting for v1's ListTabs reply".to_string());
        }
        match receiver.recv_timeout(remaining) {
            Ok(TaggedServerMessage { request_id: Some(id), message: ServerMessage::Tabs(tabs), .. }) if id == request_id => return Ok(tabs),
            Ok(_) => continue, // some other client's concurrent broadcast traffic
            // A timeout inside `recv_timeout` itself (as opposed to the
            // deadline check above) is not yet the final "timed out"
            // error -- loop back so that check produces the accurate
            // message once `remaining` is truly exhausted, rather than
            // this arm misreporting it as a disconnect.
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => return Err("v1's broadcast connection ended while waiting for its ListTabs reply".to_string()),
        }
    }
}

/// Replays `captured_tabs` (v1's [`TabSummary`] list, in the order
/// [`capture_v1_tabs`] returned them) into `v2`, talking directly to
/// `v2_stream` -- v2 has no other client yet, so there's no broadcast/
/// synthetic-client dance needed here, unlike [`capture_v1_tabs`].
/// `Navigate{url}` for the first (v2's own already-existing default)
/// tab, skipped entirely if it had no url (leaves that slot blank, same
/// as a fresh tab); `OpenTab{url}` for every subsequent one (itself
/// `Option<String>`, so a blank later tab is replayed as a blank
/// `OpenTab` too, preserving v1's own tab count). The first failure (an
/// `Error`/`GatekeeperBlocked` reply, or the connection breaking)
/// aborts the whole replay -- `phase-8-live-core-hotswap/PLAN.md`'s
/// single-attempt, fail-closed contract.
fn replay_tabs(v2_stream: &mut UnixStream, captured_tabs: &[TabSummary]) -> Result<(), String> {
    for (i, tab) in captured_tabs.iter().enumerate() {
        if i == 0 {
            let Some(url) = &tab.url else { continue };
            let request_id = synthetic_request_id();
            write_client_message_with_id(v2_stream, Some(request_id), &ClientMessage::Navigate { url: url.clone() })
                .map_err(|e| format!("failed to replay the default tab's Navigate into v2: {e}"))?;
            expect_navigate_success(v2_stream, request_id)?;
        } else {
            let request_id = synthetic_request_id();
            write_client_message_with_id(v2_stream, Some(request_id), &ClientMessage::OpenTab { url: tab.url.clone() }).map_err(|e| format!("failed to replay tab {i}'s OpenTab into v2: {e}"))?;
            expect_open_tab_success(v2_stream, request_id, tab.url.is_some())?;
        }
    }
    Ok(())
}

/// Reads from `v2_stream` until the reply to `request_id` -- `Navigated`
/// on success -- is seen, then keeps reading until the `FrameReady`
/// that always follows it (`session.rs`'s `reply_success` always sends
/// both, in that order) arrives too, so this call never leaves an
/// unread `FrameReady` on the wire for the next replay message to
/// misinterpret -- mirroring `blueice-mcp-server`'s own
/// `send_and_drain`/`open_tab` discipline. Any `Error`/
/// `GatekeeperBlocked` reply, or the read itself failing (v2's
/// connection broke, e.g. because writing the frame failed), is a
/// replay failure.
fn expect_navigate_success(v2_stream: &mut UnixStream, request_id: u64) -> Result<(), String> {
    let mut navigated = false;
    loop {
        let (reply_id, message) = read_server_message_with_id(v2_stream).map_err(|e| format!("failed reading v2's reply while replaying: {e}"))?;
        if matches!(reply_id, Some(id) if id != request_id) {
            continue;
        }
        match message {
            ServerMessage::Navigated { .. } => navigated = true,
            ServerMessage::FrameReady { .. } if navigated => return Ok(()),
            ServerMessage::FrameReady { .. } => continue, // shouldn't happen before Navigated, but don't misinterpret
            ServerMessage::Error { message } => return Err(format!("v2 rejected the replayed Navigate: {message}")),
            ServerMessage::GatekeeperBlocked { reason, .. } => return Err(format!("v2's gatekeeper blocked the replayed Navigate: {reason}")),
            _ => continue,
        }
    }
}

/// Like [`expect_navigate_success`], for a replayed `OpenTab` --
/// `session.rs`'s `handle_open_tab` only sends a `FrameReady` after
/// `TabOpened` when the tab was actually navigated (`expects_frame`),
/// so a blank replayed tab is considered done as soon as `TabOpened`
/// itself arrives.
fn expect_open_tab_success(v2_stream: &mut UnixStream, request_id: u64, expects_frame: bool) -> Result<(), String> {
    let mut opened = false;
    loop {
        let (reply_id, message) = read_server_message_with_id(v2_stream).map_err(|e| format!("failed reading v2's reply while replaying: {e}"))?;
        if matches!(reply_id, Some(id) if id != request_id) {
            continue;
        }
        match message {
            ServerMessage::TabOpened { .. } => {
                opened = true;
                if !expects_frame {
                    return Ok(());
                }
            }
            ServerMessage::FrameReady { .. } if opened => return Ok(()),
            ServerMessage::FrameReady { .. } => continue,
            ServerMessage::Error { message } => return Err(format!("v2 rejected the replayed OpenTab: {message}")),
            ServerMessage::GatekeeperBlocked { reason, .. } => return Err(format!("v2's gatekeeper blocked the replayed OpenTab: {reason}")),
            _ => continue,
        }
    }
}

/// Confirms v2 actually reflects what was just replayed: sends
/// `ListTabs` directly on `v2_stream` and checks the tab count and URLs
/// match `captured_tabs` -- the "lighter" health bar this minimal
/// slice's plan settled on (structurally diffing each tab's DOM/
/// representation against v1's own captured state is explicitly
/// deferred).
fn health_check(v2_stream: &mut UnixStream, captured_tabs: &[TabSummary]) -> Result<(), String> {
    let request_id = synthetic_request_id();
    write_client_message_with_id(v2_stream, Some(request_id), &ClientMessage::ListTabs).map_err(|e| format!("failed to send v2's health-check ListTabs: {e}"))?;
    loop {
        let (reply_id, message) = read_server_message_with_id(v2_stream).map_err(|e| format!("failed reading v2's health-check reply: {e}"))?;
        if matches!(reply_id, Some(id) if id != request_id) {
            continue;
        }
        match message {
            ServerMessage::Tabs(tabs) => {
                let expected: Vec<&Option<String>> = captured_tabs.iter().map(|t| &t.url).collect();
                let actual: Vec<&Option<String>> = tabs.iter().map(|t| &t.url).collect();
                return if actual == expected {
                    Ok(())
                } else {
                    Err(format!("v2's post-replay ListTabs didn't match what was captured from v1: expected {expected:?}, got {actual:?}"))
                };
            }
            _ => continue,
        }
    }
}

/// A fresh, unique frame directory for a newly-[`SpawnedCore::spawn`]ed
/// v2, derived from v1's own -- see [`Broker::frame_dir`]'s docs for
/// why sharing one directory between two simultaneously-live `core`
/// instances isn't safe. `target_generation` (the generation this
/// cutover attempt would bump to if it succeeds) makes this unique
/// across repeated cutover attempts too, not just between v1 and v2.
fn v2_frame_dir(v1_frame_dir: &Path, target_generation: u64) -> PathBuf {
    let stem = v1_frame_dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "frames".to_string());
    v1_frame_dir.with_file_name(format!("{stem}-cutover-{target_generation}"))
}

/// The actual swap, once replay + health check have both fully
/// succeeded -- see `phase-8-live-core-hotswap/PLAN.md`'s "Concrete
/// cutover mechanism" for the exact ordering this follows: bump
/// `generation`, start v2's own generation-tagged broadcast thread
/// (sharing the same `clients` list), retarget `core_writer`'s
/// contents at v2, then close v1's stream (letting its now-superseded
/// broadcast thread exit quietly, per [`spawn_generation_tagged_broadcast`])
/// and drop v1 itself (killing its process, cleaning up its socket and
/// frame directory).
fn perform_swap(broker: &Arc<Broker>, v2: SpawnedCore, target_generation: u64) {
    broker.generation.store(target_generation, Ordering::SeqCst);

    let v2_broadcast_stream = v2.stream.try_clone().expect("try_clone on a fresh stream should not fail");
    spawn_generation_tagged_broadcast(v2_broadcast_stream, Arc::clone(&broker.clients), Arc::clone(&broker.generation), target_generation, broker.done.clone());

    let v2_writer_stream = v2.stream.try_clone().expect("try_clone on a fresh stream should not fail");
    *broker.core_writer.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = v2_writer_stream;

    let mut active = broker.active_core.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(v1) = active.replace(v2) {
        // Unblocks v1's old broadcast thread's blocked read (shutdown
        // affects every fd sharing this socket's underlying open file
        // description, including that thread's own clone) so it exits
        // through the ordinary disconnect path -- which it now
        // recognizes as an expected supersession, not a fault, since
        // `generation` was already bumped above.
        let _ = v1.stream.shutdown(Shutdown::Both);
        drop(v1); // kills v1's process, cleans up its socket + frame_dir
    }
}

/// Performs one cutover attempt: captures v1's tab list, spawns v2,
/// replays the tabs into it, health-checks it, and -- only if all of
/// that succeeds -- performs the actual swap. A single attempt, fail
/// closed to v1 on any error -- see `phase-8-live-core-hotswap/PLAN.md`'s
/// "Wiring design" for why no retry policy exists yet. v1 is never
/// touched until every step through the health check has succeeded.
fn cutover(broker: &Arc<Broker>) -> control::ControlReply {
    let captured_tabs = match capture_v1_tabs(&broker.core_writer, &broker.clients, TAB_CAPTURE_TIMEOUT) {
        Ok(tabs) => tabs,
        Err(reason) => return control::ControlReply::CutoverFailed { reason },
    };

    let target_generation = broker.generation.load(Ordering::SeqCst) + 1;
    let frame_dir = v2_frame_dir(&broker.frame_dir, target_generation);
    let mut v2 = match SpawnedCore::spawn(broker.width, broker.height, &frame_dir) {
        Ok(v2) => v2,
        Err(e) => return control::ControlReply::CutoverFailed { reason: format!("failed to spawn v2: {e}") },
    };

    if let Err(reason) = replay_tabs(&mut v2.stream, &captured_tabs) {
        return control::ControlReply::CutoverFailed { reason }; // v2 dropped here: killed, cleaned up
    }
    if let Err(reason) = health_check(&mut v2.stream, &captured_tabs) {
        return control::ControlReply::CutoverFailed { reason }; // v2 dropped here: killed, cleaned up
    }

    let tabs_migrated = captured_tabs.len();
    perform_swap(broker, v2, target_generation);
    control::ControlReply::CutoverDone { tabs_migrated }
}

/// Handles one control-socket connection: reads its one
/// [`control::ControlRequest`], routes `Cutover` into [`cutover`], and
/// writes back the resulting [`control::ControlReply`].
fn handle_control_connection(mut conn: UnixStream, broker: &Arc<Broker>) -> io::Result<()> {
    let request = control::read_control_request(&mut conn)?;
    let reply = match request {
        control::ControlRequest::Cutover => cutover(broker),
    };
    control::write_control_reply(&mut conn, &reply)
}

/// Runs the broker for as long as the currently-active `core` isn't
/// deliberately superseded: accepts external client connections from
/// `rendezvous_listener` (registering each via [`register_client`]),
/// accepts control connections from `control_listener` (each one
/// routed into [`handle_control_connection`]/[`cutover`] on its own
/// thread), and blocks the calling thread until the active `core`
/// disconnects for a reason that ISN'T a deliberate cutover -- see
/// [`spawn_generation_tagged_broadcast`]'s docs for exactly how that's
/// told apart from an ordinary supersession. This reproduces this
/// function's pre-cutover behavior exactly for the never-cutover case:
/// a client's `Shutdown` still cascades through `core`, closes the
/// connection, and ends the whole launcher, same as before this phase's
/// work existed.
///
/// `core` (v1) is fully owned by this call from here on: whichever
/// `SpawnedCore` is active when this function is about to return is
/// explicitly torn down (killed, socket/frame_dir cleaned up) before
/// returning, mirroring the `drop(core)` `main()` used to do itself
/// before this function grew cutover support.
///
/// `rendezvous_listener.incoming()`/`control_listener.incoming()` both
/// block indefinitely waiting for their next connection, so there's no
/// clean way to also stop those accept threads at this point; the
/// caller is expected to exit the process shortly after this returns,
/// which the OS reclaims regardless of those threads' blocked state
/// (matching `blueice-core`'s own "just run until the underlying
/// connection ends" simplicity level).
pub fn run_broker(rendezvous_listener: UnixListener, control_listener: UnixListener, core: SpawnedCore, width: f64, height: f64) -> io::Result<()> {
    let frame_dir = core.frame_dir.clone();
    let core_writer = Arc::new(Mutex::new(core.stream.try_clone()?));
    let broadcast_stream = core.stream.try_clone()?;
    let clients: Arc<Mutex<Vec<Sender<TaggedServerMessage>>>> = Arc::new(Mutex::new(Vec::new()));
    let generation = Arc::new(AtomicU64::new(0));
    let (done_tx, done_rx) = mpsc::channel();

    let broker = Arc::new(Broker {
        core_writer: Arc::clone(&core_writer),
        clients: Arc::clone(&clients),
        generation: Arc::clone(&generation),
        active_core: Mutex::new(Some(core)),
        width,
        height,
        frame_dir,
        done: done_tx.clone(),
    });

    spawn_generation_tagged_broadcast(broadcast_stream, Arc::clone(&clients), Arc::clone(&generation), 0, done_tx);

    let accept_clients = Arc::clone(&clients);
    let accept_core_writer = Arc::clone(&core_writer);
    thread::spawn(move || {
        for incoming in rendezvous_listener.incoming() {
            let Ok(client) = incoming else { break };
            let _ = register_client(client, Arc::clone(&accept_core_writer), Arc::clone(&accept_clients));
        }
    });

    let control_broker = Arc::clone(&broker);
    thread::spawn(move || {
        for incoming in control_listener.incoming() {
            let Ok(conn) = incoming else { break };
            let broker = Arc::clone(&control_broker);
            thread::spawn(move || {
                let _ = handle_control_connection(conn, &broker);
            });
        }
    });

    let _ = done_rx.recv();

    if let Ok(mut active) = broker.active_core.lock() {
        active.take();
    }

    Ok(())
}

/// The `blueice-core` binary's path, resolved relative to this
/// (`blueice-launcher`'s) own executable -- mirrors `blueice-mcp-
/// server`'s identical `sibling_core_binary` helper (not shared between
/// the two crates: each is ~10 lines, and the two processes' spawn
/// helpers are otherwise independent enough that a shared crate just
/// for this would be more indirection than the duplication costs).
/// Steps out of a `deps` directory first so the same lookup works both
/// for the installed binary and for a `cargo test` integration-test
/// binary, which lands one level deeper (`target/<profile>/deps/`).
fn sibling_core_binary(this_exe: &Path) -> PathBuf {
    let name = if cfg!(windows) { "blueice-core.exe" } else { "blueice-core" };
    let dir = this_exe.parent().unwrap_or_else(|| Path::new("."));
    let dir = if dir.file_name().is_some_and(|n| n == "deps") { dir.parent().unwrap_or(dir) } else { dir };
    dir.join(name)
}

/// A path for `core`'s *internal* socket -- never exposed to external
/// clients, which only ever see the rendezvous socket this launcher
/// itself listens on. Includes a monotonic counter alongside the PID:
/// a PID alone isn't unique enough once a single launcher process can
/// call [`SpawnedCore::spawn`] more than once in its own lifetime (v1
/// at startup, then a fresh v2 on every cutover) -- without the
/// counter, v2's internal socket path would collide with v1's still-
/// live one.
fn unique_internal_socket_path() -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("blueice-launcher-core-{}-{n}.sock", std::process::id()))
}

fn wait_for_socket(path: &Path, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if path.exists() {
            return true;
        }
        thread::sleep(Duration::from_millis(20));
    }
    false
}

/// A `core` process this launcher spawned and owns privately: killed
/// and cleaned up (process, internal socket, and frame directory) on
/// [`Drop`], the same lifetime discipline `mcp-server`'s `CoreProcess`
/// already established for its own (today, unshared) spawned `core`.
pub struct SpawnedCore {
    child: Child,
    internal_socket_path: PathBuf,
    frame_dir: PathBuf,
    pub stream: UnixStream,
}

impl SpawnedCore {
    pub fn spawn(width: f64, height: f64, frame_dir: &Path) -> io::Result<Self> {
        let this_exe = std::env::current_exe()?;
        let core_bin = sibling_core_binary(&this_exe);
        let internal_socket_path = unique_internal_socket_path();
        let _ = std::fs::remove_file(&internal_socket_path);

        let child = Command::new(&core_bin)
            .arg("--socket")
            .arg(&internal_socket_path)
            .arg("--width")
            .arg(width.to_string())
            .arg("--height")
            .arg(height.to_string())
            .arg("--frame-dir")
            .arg(frame_dir)
            .spawn()?;

        if !wait_for_socket(&internal_socket_path, Duration::from_secs(5)) {
            return Err(io::Error::other(format!("blueice-core never created its socket at {}", internal_socket_path.display())));
        }
        let mut stream = UnixStream::connect(&internal_socket_path)?;
        // `core` requires the very first message on a fresh connection
        // to be `Hello` (`phase-1-ai-representation-layer/PLAN.md` §3);
        // this launcher is the connection's one and only direct client,
        // so it satisfies that gate itself, once, here -- external
        // clients connecting through the rendezvous socket send their
        // own `Hello` too, but by the time the broker forwards it into
        // this already-past-its-handshake connection, `core` just
        // answers it again rather than re-gating (see `blueice_engine::
        // session::run_session`'s own docs).
        blueice_ipc::client_handshake(&mut stream)?;
        Ok(SpawnedCore { child, internal_socket_path, frame_dir: frame_dir.to_path_buf(), stream })
    }
}

impl Drop for SpawnedCore {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.internal_socket_path);
        // `child.kill()` sends SIGKILL, which never lets `blueice-core`
        // run its own graceful-exit cleanup (which would otherwise
        // remove this itself) -- matters specifically for a cutover's
        // forcefully-superseded v1, which never gets the chance to exit
        // on its own.
        let _ = std::fs::remove_dir_all(&self.frame_dir);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use blueice_ipc::*;

    #[test]
    fn forward_client_to_core_relays_one_message_then_stops_on_disconnect() {
        let (client_side, mut client_observed) = UnixStream::pair().unwrap();
        let (core_side, mut core_observed) = UnixStream::pair().unwrap();
        let core = Arc::new(Mutex::new(core_side));

        write_client_message(&mut client_observed, &ClientMessage::Resize { width: 10, height: 20 }).unwrap();
        drop(client_observed); // triggers a clean disconnect after the one message

        forward_client_to_core(client_side, Arc::clone(&core));

        assert_eq!(read_client_message(&mut core_observed).unwrap(), ClientMessage::Resize { width: 10, height: 20 });
    }

    #[test]
    fn forward_client_to_core_stops_once_the_core_connection_is_gone() {
        let (client_side, mut client_observed) = UnixStream::pair().unwrap();
        let (core_side, core_observed) = UnixStream::pair().unwrap();
        drop(core_observed); // core is "gone" before any message arrives
        let core = Arc::new(Mutex::new(core_side));

        write_client_message(&mut client_observed, &ClientMessage::Shutdown).unwrap();
        // Also close the client's own peer: writing to an already-
        // closed Unix domain socket peer isn't *always* an immediate
        // error (the kernel can let one small write through before it
        // fully propagates the peer's close) -- without this, a rare
        // timing race could let the write-to-`core` attempt below
        // spuriously succeed once, sending this loop around for a
        // second `read` that would then block forever with nothing
        // left to read. Closing this peer too guarantees a prompt
        // return via *that* read failing, regardless of which race
        // outcome the write hits.
        drop(client_observed);

        // Must return (not hang or panic) once the connection to
        // `core` and/or `client` is gone.
        forward_client_to_core(client_side, core);
    }

    #[test]
    fn forward_client_to_core_preserves_tab_id_and_request_id() {
        // Direct regression coverage for Part 1's fix at the primitive
        // level: previously this used the plain (ids-discarding)
        // `read_client_message`/`write_client_message`, so a client's
        // `tab_id`/`request_id` never survived being forwarded into
        // `core`.
        let (client_side, mut client_observed) = UnixStream::pair().unwrap();
        let (core_side, mut core_observed) = UnixStream::pair().unwrap();
        let core = Arc::new(Mutex::new(core_side));

        write_client_message_with_ids(&mut client_observed, Some(3), Some(42), &ClientMessage::Navigate { url: "https://example.com".to_string() }).unwrap();
        drop(client_observed);

        forward_client_to_core(client_side, Arc::clone(&core));

        assert_eq!(
            read_client_message_with_ids(&mut core_observed).unwrap(),
            (Some(3), Some(42), ClientMessage::Navigate { url: "https://example.com".to_string() })
        );
    }

    #[test]
    fn broadcast_core_to_clients_relays_one_message_to_every_registered_client() {
        let (core_side, mut core_observed) = UnixStream::pair().unwrap();
        let (sender1, receiver1) = mpsc::channel();
        let (sender2, receiver2) = mpsc::channel();
        let clients = Arc::new(Mutex::new(vec![sender1, sender2]));

        write_server_message(&mut core_observed, &ServerMessage::Navigated { url: "about:blank".to_string() }).unwrap();
        drop(core_observed); // ends the broadcaster loop after the one message

        broadcast_core_to_clients(core_side, clients);

        let expected = TaggedServerMessage { tab_id: None, request_id: None, message: ServerMessage::Navigated { url: "about:blank".to_string() } };
        assert_eq!(receiver1.recv().unwrap(), expected);
        assert_eq!(receiver2.recv().unwrap(), expected);
    }

    #[test]
    fn broadcast_core_to_clients_preserves_tab_id_and_request_id() {
        let (core_side, mut core_observed) = UnixStream::pair().unwrap();
        let (sender, receiver) = mpsc::channel();
        let clients = Arc::new(Mutex::new(vec![sender]));

        write_server_message_with_ids(&mut core_observed, Some(3), Some(42), &ServerMessage::Navigated { url: "about:blank".to_string() }).unwrap();
        drop(core_observed);

        broadcast_core_to_clients(core_side, clients);

        assert_eq!(receiver.recv().unwrap(), TaggedServerMessage { tab_id: Some(3), request_id: Some(42), message: ServerMessage::Navigated { url: "about:blank".to_string() } });
    }

    #[test]
    fn broadcast_core_to_clients_drops_a_client_whose_channel_is_gone_without_affecting_the_others() {
        let (core_side, mut core_observed) = UnixStream::pair().unwrap();
        let (dead_sender, dead_receiver) = mpsc::channel();
        drop(dead_receiver); // stands in for that client's writer thread having already exited
        let (live_sender, live_receiver) = mpsc::channel();
        let clients = Arc::new(Mutex::new(vec![dead_sender, live_sender]));

        write_server_message(&mut core_observed, &ServerMessage::Navigated { url: "about:blank".to_string() }).unwrap();
        drop(core_observed);

        broadcast_core_to_clients(core_side, Arc::clone(&clients));

        assert_eq!(live_receiver.recv().unwrap().message, ServerMessage::Navigated { url: "about:blank".to_string() });
        // the dead client's sender must have been pruned from the list.
        assert_eq!(clients.lock().unwrap().len(), 1);
    }

    #[test]
    fn register_client_forwards_its_messages_and_receives_broadcasts() {
        let (client_side, mut client_observed) = UnixStream::pair().unwrap();
        let (core_side, mut core_observed) = UnixStream::pair().unwrap();
        let core_writer = Arc::new(Mutex::new(core_side));
        let clients: Arc<Mutex<Vec<Sender<TaggedServerMessage>>>> = Arc::new(Mutex::new(Vec::new()));

        register_client(client_side, Arc::clone(&core_writer), Arc::clone(&clients)).unwrap();

        // fan-in: a message the "client" sends must reach core.
        write_client_message(&mut client_observed, &ClientMessage::GetRepresentation).unwrap();
        assert_eq!(read_client_message(&mut core_observed).unwrap(), ClientMessage::GetRepresentation);

        // fan-out: a message sent into the registered channel (standing
        // in for the broadcaster) must reach the client's real socket,
        // relayed by this client's own writer thread.
        let registered = clients.lock().unwrap().pop().unwrap();
        registered.send(TaggedServerMessage { tab_id: Some(1), request_id: Some(9), message: ServerMessage::Navigated { url: "x".to_string() } }).unwrap();
        assert_eq!(read_server_message_with_ids(&mut client_observed).unwrap(), (Some(1), Some(9), ServerMessage::Navigated { url: "x".to_string() }));
    }

    #[test]
    fn client_write_half_gets_a_bounded_write_timeout() {
        // Regression coverage for a real deadlock this fixes: without a
        // write timeout, a client whose writer thread stalls (its
        // kernel socket buffer fills and nobody drains it) would never
        // notice the client is gone -- its channel would just queue up
        // forever instead of eventually being pruned. See
        // `a_slow_client_does_not_block_delivery_to_another_client` for
        // the actual "doesn't block other clients" property this and
        // the channel/writer-thread split together provide.
        let (client_side, _client_observed) = UnixStream::pair().unwrap();
        let write_half = client_write_half(&client_side).unwrap();
        assert_eq!(write_half.write_timeout().unwrap(), Some(CLIENT_WRITE_TIMEOUT));
    }

    #[test]
    fn a_slow_client_does_not_block_delivery_to_another_client() {
        // The actual bug this channel/writer-thread design fixes: a
        // single shared thread writing to every registered client's
        // socket directly, one after another, meant one client that
        // stalls (its kernel socket buffer fills, e.g. a large message
        // nobody drains) blocked delivery to every *other* client too,
        // for as long as that stalled write took to time out --
        // confirmed against a real launcher/core pair before this fix
        // existed. Proven here with a real filled socket buffer, not a
        // mock: `slow_client`'s peer end is never read; `live_client`'s
        // is read immediately after this call returns, and must have
        // its message waiting regardless of the slow client's state.
        let (core_side, mut core_observed) = UnixStream::pair().unwrap();
        let (slow_client_side, _never_read) = UnixStream::pair().unwrap();
        let (live_client_side, mut live_client_observed) = UnixStream::pair().unwrap();
        let (core_writer_side, _unused) = UnixStream::pair().unwrap();
        let core_writer = Arc::new(Mutex::new(core_writer_side));
        let clients: Arc<Mutex<Vec<Sender<TaggedServerMessage>>>> = Arc::new(Mutex::new(Vec::new()));

        register_client(slow_client_side, Arc::clone(&core_writer), Arc::clone(&clients)).unwrap();
        register_client(live_client_side, Arc::clone(&core_writer), Arc::clone(&clients)).unwrap();

        // Comfortably larger than any realistic default kernel socket
        // buffer, so the write to `slow_client`'s writer thread genuinely
        // blocks rather than merely being slow to observe. Written on
        // its own thread since *this* write can itself block until
        // `broadcast_core_to_clients` below is actively reading
        // `core_side` -- `core_observed`'s own send buffer is no bigger
        // than any other socket's here.
        let big_message = ServerMessage::Dom("x".repeat(4 * 1024 * 1024));
        let sent = big_message.clone();
        thread::spawn(move || {
            write_server_message(&mut core_observed, &sent).unwrap();
            drop(core_observed); // ends the broadcaster loop after the one message
        });

        let start = Instant::now();
        broadcast_core_to_clients(core_side, clients);
        assert!(start.elapsed() < Duration::from_secs(1), "fanning out must be a cheap non-blocking queue push regardless of any client's own writer-thread state");

        let received = read_server_message(&mut live_client_observed).unwrap();
        assert_eq!(received, big_message, "the live client must receive its own copy promptly, not stalled behind the slow one");
    }

    #[test]
    fn default_rendezvous_socket_path_is_per_user_not_system_wide() {
        let path = default_rendezvous_socket_path();
        assert_eq!(path.file_name().unwrap(), "core.sock");
        // must not resolve to a single fixed system-wide path regardless
        // of environment -- it has to vary by runtime dir or uid.
        assert_ne!(path, PathBuf::from("/core.sock"));
    }

    #[test]
    fn sibling_core_binary_sits_next_to_the_launcher_binary() {
        let exe = PathBuf::from("/some/target/debug/blueice-launcher");
        assert_eq!(sibling_core_binary(&exe), PathBuf::from("/some/target/debug/blueice-core"));
    }

    #[test]
    fn sibling_core_binary_steps_out_of_a_deps_directory_for_integration_tests() {
        let exe = PathBuf::from("/some/target/debug/deps/broker_end_to_end-abc123");
        assert_eq!(sibling_core_binary(&exe), PathBuf::from("/some/target/debug/blueice-core"));
    }

    #[test]
    fn unique_internal_socket_path_stays_short_enough_for_af_unix() {
        assert!(unique_internal_socket_path().to_string_lossy().len() < 100);
    }

    #[test]
    fn unique_internal_socket_path_differs_across_multiple_calls_in_the_same_process() {
        // A cutover calls `SpawnedCore::spawn` a second time within the
        // same launcher process (v1 at startup, v2 during cutover) --
        // PID alone would collide, so this must also vary per call.
        assert_ne!(unique_internal_socket_path(), unique_internal_socket_path());
    }

    #[test]
    fn wait_for_socket_returns_true_once_the_path_exists() {
        let path = std::env::temp_dir().join(format!("blueice-launcher-wait-test-{}", std::process::id()));
        let _ = std::fs::remove_file(&path);
        std::fs::write(&path, b"x").unwrap();
        assert!(wait_for_socket(&path, Duration::from_millis(50)));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn wait_for_socket_times_out_if_the_path_never_appears() {
        let path = std::env::temp_dir().join(format!("blueice-launcher-wait-test-missing-{}", std::process::id()));
        let _ = std::fs::remove_file(&path);
        assert!(!wait_for_socket(&path, Duration::from_millis(50)));
    }

    #[test]
    fn tab_id_and_request_id_survive_a_full_relay_through_the_broker() {
        // The end-to-end regression test for Part 1's fix, driven over
        // the real broker primitives (`register_client`/
        // `forward_client_to_core`/`broadcast_core_to_clients`), not
        // just one function in isolation: a client's tagged message
        // must reach `core` with its ids intact, and `core`'s tagged
        // reply must reach the client's own socket with its ids intact
        // too.
        let (client_side, mut client_observed) = UnixStream::pair().unwrap();
        let (core_side, mut core_observed) = UnixStream::pair().unwrap();
        let core_writer = Arc::new(Mutex::new(core_side.try_clone().unwrap()));
        let clients: Arc<Mutex<Vec<Sender<TaggedServerMessage>>>> = Arc::new(Mutex::new(Vec::new()));

        register_client(client_side, Arc::clone(&core_writer), Arc::clone(&clients)).unwrap();

        write_client_message_with_ids(&mut client_observed, Some(3), Some(42), &ClientMessage::Navigate { url: "https://example.com".to_string() }).unwrap();

        let (tab_id, request_id, msg) = read_client_message_with_ids(&mut core_observed).unwrap();
        assert_eq!(tab_id, Some(3), "the broker must not silently drop tab_id on the forwarding path");
        assert_eq!(request_id, Some(42), "the broker must not silently drop request_id on the forwarding path");
        assert_eq!(msg, ClientMessage::Navigate { url: "https://example.com".to_string() });

        write_server_message_with_ids(&mut core_observed, Some(3), Some(42), &ServerMessage::Navigated { url: "https://example.com".to_string() }).unwrap();
        drop(core_observed); // ends the broadcaster loop after the one message

        broadcast_core_to_clients(core_side, Arc::clone(&clients));

        let (tab_id, request_id, msg) = read_server_message_with_ids(&mut client_observed).unwrap();
        assert_eq!(tab_id, Some(3), "the broker must not silently drop tab_id on the reply path");
        assert_eq!(request_id, Some(42), "the broker must not silently drop request_id on the reply path");
        assert_eq!(msg, ServerMessage::Navigated { url: "https://example.com".to_string() });
    }

    #[test]
    fn generation_tagged_broadcast_signals_done_when_its_generation_is_still_current() {
        let (core_side, core_observed) = UnixStream::pair().unwrap();
        drop(core_observed); // "core" is already gone
        let clients: Arc<Mutex<Vec<Sender<TaggedServerMessage>>>> = Arc::new(Mutex::new(Vec::new()));
        let generation = Arc::new(AtomicU64::new(0));
        let (done_tx, done_rx) = mpsc::channel();

        spawn_generation_tagged_broadcast(core_side, clients, Arc::clone(&generation), 0, done_tx);

        done_rx.recv_timeout(Duration::from_secs(5)).expect("an unsuperseded broadcast thread's death must signal done");
    }

    #[test]
    fn generation_tagged_broadcast_stays_quiet_when_superseded() {
        let (core_side, core_observed) = UnixStream::pair().unwrap();
        drop(core_observed); // standing in for a cutover's deliberate close of v1's stream
        let clients: Arc<Mutex<Vec<Sender<TaggedServerMessage>>>> = Arc::new(Mutex::new(Vec::new()));
        let generation = Arc::new(AtomicU64::new(1)); // already bumped past this thread's own generation
        let (done_tx, done_rx) = mpsc::channel();

        spawn_generation_tagged_broadcast(core_side, clients, generation, 0, done_tx);

        assert!(done_rx.recv_timeout(Duration::from_millis(200)).is_err(), "a superseded broadcast thread's death must NOT signal done");
    }

    #[test]
    fn capture_v1_tabs_filters_for_the_matching_request_id_and_ignores_other_traffic() {
        let (core_side, mut core_observed) = UnixStream::pair().unwrap();
        let core_writer = Arc::new(Mutex::new(core_side.try_clone().unwrap()));
        let clients: Arc<Mutex<Vec<Sender<TaggedServerMessage>>>> = Arc::new(Mutex::new(Vec::new()));

        // `capture_v1_tabs` only ever *writes* into `core_writer` and
        // then waits on its own registered channel -- something else
        // (the real broker's own broadcast thread, in production) has
        // to actually read `core`'s replies and fan them out to
        // `clients`. Stands that in here.
        let broadcast_clients = Arc::clone(&clients);
        let broadcaster = thread::spawn(move || broadcast_core_to_clients(core_side, broadcast_clients));

        let responder = thread::spawn(move || {
            let (_, request_id, msg) = read_client_message_with_ids(&mut core_observed).unwrap();
            assert!(matches!(msg, ClientMessage::ListTabs));
            // Some unrelated broadcast traffic first (another client's
            // concurrent action) -- must be skipped, not mistaken for
            // this call's own reply.
            write_server_message_with_id(&mut core_observed, Some(999_999), &ServerMessage::Navigated { url: "https://unrelated.example".to_string() }).unwrap();
            write_server_message_with_id(&mut core_observed, request_id, &ServerMessage::Tabs(vec![TabSummary { id: 1, url: Some("about:blank".to_string()) }])).unwrap();
            drop(core_observed); // ends the broadcaster loop
        });

        let tabs = capture_v1_tabs(&core_writer, &clients, Duration::from_secs(5)).unwrap();
        assert_eq!(tabs, vec![TabSummary { id: 1, url: Some("about:blank".to_string()) }]);
        responder.join().unwrap();
        broadcaster.join().unwrap();
    }

    #[test]
    fn capture_v1_tabs_fails_if_no_reply_arrives_within_the_timeout() {
        let (core_side, _core_observed) = UnixStream::pair().unwrap(); // nobody replies
        let core_writer = Arc::new(Mutex::new(core_side));
        let clients: Arc<Mutex<Vec<Sender<TaggedServerMessage>>>> = Arc::new(Mutex::new(Vec::new()));

        assert!(capture_v1_tabs(&core_writer, &clients, Duration::from_millis(100)).is_err());
    }

    #[test]
    fn capture_v1_tabs_fails_if_the_broadcast_connection_ends_before_a_reply_arrives() {
        let (core_side, core_observed) = UnixStream::pair().unwrap();
        drop(core_observed); // "core" is already gone before ever replying
        let core_writer = Arc::new(Mutex::new(core_side));
        let clients: Arc<Mutex<Vec<Sender<TaggedServerMessage>>>> = Arc::new(Mutex::new(Vec::new()));

        // A short timeout parameter, not the real multi-second
        // `TAB_CAPTURE_TIMEOUT`: whether the write itself fails
        // immediately (broken pipe) or this call has to fall back to
        // its own deadline depends on OS-level socket-teardown timing,
        // which isn't worth this test waiting several real seconds to
        // observe either way -- both paths return `Err`.
        assert!(capture_v1_tabs(&core_writer, &clients, Duration::from_millis(200)).is_err());
    }

    #[test]
    fn capture_v1_tabs_registers_and_the_stale_sender_self_prunes_on_the_next_broadcast() {
        let (core_side, mut core_observed) = UnixStream::pair().unwrap();
        let core_writer = Arc::new(Mutex::new(core_side.try_clone().unwrap()));
        let clients: Arc<Mutex<Vec<Sender<TaggedServerMessage>>>> = Arc::new(Mutex::new(Vec::new()));

        let broadcast_clients = Arc::clone(&clients);
        let broadcaster = thread::spawn(move || broadcast_core_to_clients(core_side, broadcast_clients));

        // Explicit hand-off, not just "send two messages in a row": the
        // *second* message must not reach the broadcaster until
        // `capture_v1_tabs` below has actually returned (and so dropped
        // its own `receiver`) -- otherwise there's a real race where the
        // second `send` could land while that receiver is still alive
        // (the drop happening a few instructions later, as the call
        // stack unwinds), succeeding instead of triggering the prune
        // this test means to prove.
        let (capture_returned_tx, capture_returned_rx) = mpsc::channel::<()>();
        let responder = thread::spawn(move || {
            let (_, request_id, _) = read_client_message_with_ids(&mut core_observed).unwrap();
            write_server_message_with_id(&mut core_observed, request_id, &ServerMessage::Tabs(vec![])).unwrap();
            let _ = capture_returned_rx.recv();
            // This second broadcast message, sent only once the caller
            // below has confirmed `capture_v1_tabs` already returned, is
            // what the stale, still-registered `Sender` fails to
            // deliver, triggering its self-prune.
            write_server_message(&mut core_observed, &ServerMessage::Navigated { url: "x".to_string() }).unwrap();
            drop(core_observed); // ends the broadcaster loop
        });

        capture_v1_tabs(&core_writer, &clients, Duration::from_secs(5)).unwrap();
        assert_eq!(clients.lock().unwrap().len(), 1, "the synthetic client is still registered right after capture returns");
        let _ = capture_returned_tx.send(());

        // Wait for the broadcaster to process the second message (and
        // then end, once `core_observed` is dropped) before checking
        // that the stale sender was pruned.
        broadcaster.join().unwrap();
        assert_eq!(clients.lock().unwrap().len(), 0, "the stale synthetic sender must self-prune once its receiver has been dropped");
        responder.join().unwrap();
    }

    #[test]
    fn replay_tabs_navigates_the_first_tab_and_opens_the_rest() {
        let (mut stream, mut server) = UnixStream::pair().unwrap();
        let tabs = vec![
            TabSummary { id: 1, url: Some("about:blank".to_string()) },
            TabSummary { id: 2, url: Some("about:credits".to_string()) },
            TabSummary { id: 3, url: None },
        ];
        let responder = thread::spawn(move || {
            let (_, req, msg) = read_client_message_with_ids(&mut server).unwrap();
            assert_eq!(msg, ClientMessage::Navigate { url: "about:blank".to_string() });
            write_server_message_with_id(&mut server, req, &ServerMessage::Navigated { url: "about:blank".to_string() }).unwrap();
            write_server_message_with_id(&mut server, req, &ServerMessage::FrameReady { shm_path: "x".into(), width: 1, height: 1, generation: 1 }).unwrap();

            let (_, req, msg) = read_client_message_with_ids(&mut server).unwrap();
            assert_eq!(msg, ClientMessage::OpenTab { url: Some("about:credits".to_string()) });
            write_server_message_with_id(&mut server, req, &ServerMessage::TabOpened { tab_id: 2, url: Some("about:credits".to_string()) }).unwrap();
            write_server_message_with_id(&mut server, req, &ServerMessage::FrameReady { shm_path: "y".into(), width: 1, height: 1, generation: 2 }).unwrap();

            let (_, req, msg) = read_client_message_with_ids(&mut server).unwrap();
            assert_eq!(msg, ClientMessage::OpenTab { url: None });
            write_server_message_with_id(&mut server, req, &ServerMessage::TabOpened { tab_id: 3, url: None }).unwrap();
            // no FrameReady expected, since url was None
        });

        replay_tabs(&mut stream, &tabs).unwrap();
        responder.join().unwrap();
    }

    #[test]
    fn replay_tabs_skips_the_first_tab_entirely_if_it_has_no_url() {
        let (mut stream, server) = UnixStream::pair().unwrap();
        let tabs = vec![TabSummary { id: 1, url: None }];

        replay_tabs(&mut stream, &tabs).unwrap();

        // No message should ever have been sent for the blank default
        // tab -- dropping the peer without ever reading confirms
        // nothing was written (a write into a full/closed pipe would
        // otherwise still succeed locally without a reader, so the real
        // proof is simply that this returns `Ok` with no interaction).
        drop(server);
    }

    #[test]
    fn replay_tabs_aborts_on_the_first_error_reply() {
        let (mut stream, mut server) = UnixStream::pair().unwrap();
        let tabs = vec![TabSummary { id: 1, url: Some("http://bad".to_string()) }];
        let responder = thread::spawn(move || {
            let (_, req, _msg) = read_client_message_with_ids(&mut server).unwrap();
            write_server_message_with_id(&mut server, req, &ServerMessage::Error { message: "boom".to_string() }).unwrap();
        });

        let err = replay_tabs(&mut stream, &tabs).unwrap_err();
        assert!(err.contains("boom"));
        responder.join().unwrap();
    }

    #[test]
    fn replay_tabs_aborts_on_a_gatekeeper_blocked_reply_for_a_later_tab() {
        let (mut stream, mut server) = UnixStream::pair().unwrap();
        let tabs = vec![TabSummary { id: 1, url: None }, TabSummary { id: 2, url: Some("http://bad".to_string()) }];
        let responder = thread::spawn(move || {
            let (_, req, msg) = read_client_message_with_ids(&mut server).unwrap();
            assert!(matches!(msg, ClientMessage::OpenTab { .. }));
            write_server_message_with_id(&mut server, req, &ServerMessage::GatekeeperBlocked { reason: "nope".to_string(), category: "test".to_string(), url: "http://bad".to_string() }).unwrap();
        });

        let err = replay_tabs(&mut stream, &tabs).unwrap_err();
        assert!(err.contains("nope"));
        responder.join().unwrap();
    }

    #[test]
    fn replay_tabs_aborts_on_a_gatekeeper_blocked_reply_for_the_first_default_tab() {
        let (mut stream, mut server) = UnixStream::pair().unwrap();
        let tabs = vec![TabSummary { id: 1, url: Some("http://bad".to_string()) }];
        let responder = thread::spawn(move || {
            let (_, req, msg) = read_client_message_with_ids(&mut server).unwrap();
            assert!(matches!(msg, ClientMessage::Navigate { .. }));
            write_server_message_with_id(&mut server, req, &ServerMessage::GatekeeperBlocked { reason: "blocked".to_string(), category: "test".to_string(), url: "http://bad".to_string() }).unwrap();
        });

        let err = replay_tabs(&mut stream, &tabs).unwrap_err();
        assert!(err.contains("blocked"));
        responder.join().unwrap();
    }

    #[test]
    fn replay_tabs_ignores_a_reply_carrying_a_mismatched_request_id() {
        // The same request-id-filtering discipline `capture_v1_tabs`
        // uses, exercised on the replay side: a reply tagged with a
        // different id than the one this call's own message was tagged
        // with must be skipped, not mistaken for the real reply.
        let (mut stream, mut server) = UnixStream::pair().unwrap();
        let tabs = vec![TabSummary { id: 1, url: Some("about:blank".to_string()) }];
        let responder = thread::spawn(move || {
            let (_, req, msg) = read_client_message_with_ids(&mut server).unwrap();
            assert!(matches!(msg, ClientMessage::Navigate { .. }));
            write_server_message_with_id(&mut server, Some(123_456), &ServerMessage::Error { message: "belongs to someone else".to_string() }).unwrap();
            write_server_message_with_id(&mut server, req, &ServerMessage::Navigated { url: "about:blank".to_string() }).unwrap();
            write_server_message_with_id(&mut server, req, &ServerMessage::FrameReady { shm_path: "x".into(), width: 1, height: 1, generation: 1 }).unwrap();
        });

        replay_tabs(&mut stream, &tabs).unwrap();
        responder.join().unwrap();
    }

    #[test]
    fn health_check_ignores_a_reply_carrying_a_mismatched_request_id_and_other_traffic() {
        let (mut stream, mut server) = UnixStream::pair().unwrap();
        let expected = vec![TabSummary { id: 1, url: Some("about:blank".to_string()) }];
        let responder = thread::spawn(move || {
            let (_, req, msg) = read_client_message_with_ids(&mut server).unwrap();
            assert!(matches!(msg, ClientMessage::ListTabs));
            write_server_message_with_id(&mut server, Some(999), &ServerMessage::Navigated { url: "unrelated".to_string() }).unwrap();
            write_server_message_with_id(&mut server, req, &ServerMessage::FrameReady { shm_path: "z".into(), width: 1, height: 1, generation: 1 }).unwrap();
            write_server_message_with_id(&mut server, req, &ServerMessage::Tabs(vec![TabSummary { id: 99, url: Some("about:blank".to_string()) }])).unwrap();
        });

        health_check(&mut stream, &expected).unwrap();
        responder.join().unwrap();
    }

    #[test]
    fn health_check_succeeds_when_urls_match() {
        let (mut stream, mut server) = UnixStream::pair().unwrap();
        let expected = vec![TabSummary { id: 1, url: Some("about:blank".to_string()) }];
        let responder = thread::spawn(move || {
            let (_, req, msg) = read_client_message_with_ids(&mut server).unwrap();
            assert!(matches!(msg, ClientMessage::ListTabs));
            // v2's own ids differ from v1's -- only urls must match.
            write_server_message_with_id(&mut server, req, &ServerMessage::Tabs(vec![TabSummary { id: 99, url: Some("about:blank".to_string()) }])).unwrap();
        });

        health_check(&mut stream, &expected).unwrap();
        responder.join().unwrap();
    }

    #[test]
    fn health_check_fails_when_urls_dont_match() {
        let (mut stream, mut server) = UnixStream::pair().unwrap();
        let expected = vec![TabSummary { id: 1, url: Some("about:blank".to_string()) }];
        let responder = thread::spawn(move || {
            let (_, req, _msg) = read_client_message_with_ids(&mut server).unwrap();
            write_server_message_with_id(&mut server, req, &ServerMessage::Tabs(vec![])).unwrap();
        });

        assert!(health_check(&mut stream, &expected).is_err());
        responder.join().unwrap();
    }

    #[test]
    fn v2_frame_dir_differs_from_v1s_and_varies_by_target_generation() {
        let v1 = PathBuf::from("/tmp/blueice-frames-123");
        let a = v2_frame_dir(&v1, 1);
        let b = v2_frame_dir(&v1, 2);
        assert_ne!(a, v1);
        assert_ne!(b, v1);
        assert_ne!(a, b, "a repeated cutover must not reuse the same v2 frame_dir");
    }
}
