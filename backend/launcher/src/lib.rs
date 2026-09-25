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

pub mod assistant;
pub mod control;
pub mod memory_pressure;
pub mod supervisor;
pub mod trusted_window;
pub mod update_watch;

pub use control::default_control_socket_path;

use blueice_ipc::{
    read_client_message_with_ids, read_server_message_with_id, read_server_message_with_ids,
    write_client_message_with_id, write_client_message_with_ids, write_server_message_with_ids,
    ClientMessage, ServerMessage, TabSummary,
};
use blueice_ipc::permission_control::{
    read_permission_control_reply, write_permission_control_request,
    PermissionControlReply, PermissionControlRequest,
};
use std::io::{self, Read, Write};
use std::net::Shutdown;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, SyncSender, TrySendError};
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
    match std::env::var_os("XDG_RUNTIME_DIR").filter(|dir| !dir.is_empty()) {
        Some(dir) => PathBuf::from(dir).join("blueice"),
        None => std::env::temp_dir().join(format!(
            "blueice-{}",
            blueice_ipc::local_socket::current_uid()
        )),
    }
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
pub fn broadcast_core_to_clients(
    mut core: UnixStream,
    clients: Arc<Mutex<Vec<Sender<TaggedServerMessage>>>>,
) {
    broadcast_core_to_clients_for_generation(&mut core, clients, None);
}

fn broadcast_core_to_clients_for_generation(
    core: &mut UnixStream,
    clients: Arc<Mutex<Vec<Sender<TaggedServerMessage>>>>,
    generation: Option<(&AtomicU64, u64)>,
) {
    loop {
        let (tab_id, request_id, message) = match read_server_message_with_ids(core) {
            Ok(triple) => triple,
            Err(_) => return,
        };
        let tagged = TaggedServerMessage {
            tab_id,
            request_id,
            message,
        };
        let mut clients = clients
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        // Test the generation while holding the same lock used by the
        // cutover's replay-frame handoff. A v1 read that completed just
        // before the swap must not be queued *after* a v2 frame.
        if generation.is_some_and(|(current, mine)| current.load(Ordering::SeqCst) != mine) {
            return;
        }
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
pub fn register_client(
    client: UnixStream,
    core_writer: Arc<Mutex<UnixStream>>,
    clients: Arc<Mutex<Vec<Sender<TaggedServerMessage>>>>,
) -> io::Result<()> {
    let mut write_half = client_write_half(&client)?;
    let (sender, receiver) = mpsc::channel::<TaggedServerMessage>();
    clients
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .push(sender);
    thread::spawn(move || {
        for msg in receiver {
            if write_server_message_with_ids(
                &mut write_half,
                msg.tab_id,
                msg.request_id,
                &msg.message,
            )
            .is_err()
            {
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
fn spawn_generation_tagged_broadcast(
    core_stream: UnixStream,
    clients: Arc<Mutex<Vec<Sender<TaggedServerMessage>>>>,
    generation: Arc<AtomicU64>,
    my_generation: u64,
    done: Sender<()>,
) {
    thread::spawn(move || {
        let mut core_stream = core_stream;
        broadcast_core_to_clients_for_generation(
            &mut core_stream,
            clients,
            Some((&generation, my_generation)),
        );
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
    /// At most one cutover may own v1 capture, v2 replay, and the
    /// eventual writer/process swap at a time. Without this gate, two
    /// simultaneous control connections could capture different v1
    /// states and race to replace the same active core.
    cutover_gate: CutoverGate,
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
    /// The private, always-resident gatekeeper this launcher spawned
    /// before v1. Every replacement core receives the same path, so a
    /// cutover cannot accidentally turn the fail-closed navigation
    /// checkpoint into an unreachable default socket.
    gatekeeper_socket: PathBuf,
    /// The installed package, if any, must be revalidated in each fresh core
    /// generation rather than silently disappearing after a cutover.
    extension_manifest: Option<PathBuf>,
    /// The assistant wiring, when the launcher supervises one. Every replacement
    /// core receives the same, so live translation and assistant tasks keep
    /// working across a cutover.
    assistant: Option<AssistantWiring>,
    /// Signaled exactly once, by whichever generation-tagged broadcast
    /// thread's own death is NOT a deliberate cutover supersession --
    /// what [`run_broker`] blocks on to know when the whole launcher
    /// should exit.
    done: Sender<()>,
}

/// A small RAII gate for the one operation that must be globally
/// serialized within a broker: a v1 → v2 cutover. Releasing it in
/// [`Drop`] covers all fail-closed early returns in [`cutover`].
struct CutoverGate {
    in_progress: AtomicBool,
}

impl CutoverGate {
    fn new() -> Self {
        Self {
            in_progress: AtomicBool::new(false),
        }
    }

    fn try_acquire(&self) -> Option<CutoverGuard<'_>> {
        self.in_progress
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .ok()
            .map(|_| CutoverGuard { gate: self })
    }
}

struct CutoverGuard<'a> {
    gate: &'a CutoverGate,
}

impl Drop for CutoverGuard<'_> {
    fn drop(&mut self) {
        self.gate.in_progress.store(false, Ordering::Release);
    }
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
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
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
fn capture_v1_tabs(
    core_writer: &Arc<Mutex<UnixStream>>,
    clients: &Arc<Mutex<Vec<Sender<TaggedServerMessage>>>>,
    timeout: Duration,
) -> Result<Vec<TabSummary>, String> {
    let (sender, receiver) = mpsc::channel::<TaggedServerMessage>();
    clients
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .push(sender);

    let request_id = synthetic_request_id();
    {
        let mut core = core_writer
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        write_client_message_with_id(&mut *core, Some(request_id), &ClientMessage::ListTabs)
            .map_err(|e| format!("failed to send ListTabs to v1: {e}"))?;
    }

    let deadline = Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err("timed out waiting for v1's ListTabs reply".to_string());
        }
        match receiver.recv_timeout(remaining) {
            Ok(TaggedServerMessage {
                request_id: Some(id),
                message: ServerMessage::Tabs(tabs),
                ..
            }) if id == request_id => return Ok(tabs),
            Ok(_) => continue, // some other client's concurrent broadcast traffic
            // A timeout inside `recv_timeout` itself (as opposed to the
            // deadline check above) is not yet the final "timed out"
            // error -- loop back so that check produces the accurate
            // message once `remaining` is truly exhausted, rather than
            // this arm misreporting it as a disconnect.
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(
                    "v1's broadcast connection ended while waiting for its ListTabs reply"
                        .to_string(),
                );
            }
        }
    }
}

/// How many represented nodes v1 shows for each of `tabs`, in order, asked of
/// v1 exactly as [`capture_v1_tabs`] asks for the tab list. Best effort by
/// design: a tab whose representation does not arrive in time is `None` and is
/// simply skipped by the structural health check, since the health bar must not
/// fail a cutover merely because v1 was slow to describe one page.
fn capture_v1_node_counts(
    core_writer: &Arc<Mutex<UnixStream>>,
    clients: &Arc<Mutex<Vec<Sender<TaggedServerMessage>>>>,
    tabs: &[TabSummary],
    timeout: Duration,
) -> Vec<Option<usize>> {
    let mut counts = vec![None; tabs.len()];
    if tabs.is_empty() {
        return counts;
    }
    let (sender, receiver) = mpsc::channel::<TaggedServerMessage>();
    clients
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .push(sender);

    // One request per tab, each with its own id, so replies map back by id.
    let base = synthetic_request_id();
    {
        let mut core = core_writer
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for (index, tab) in tabs.iter().enumerate() {
            let sent = blueice_ipc::write_client_message_with_ids(
                &mut *core,
                Some(tab.id),
                Some(base.wrapping_add(index as u64 + 1)),
                &ClientMessage::GetRepresentation,
            );
            if sent.is_err() {
                return counts;
            }
        }
    }

    let deadline = Instant::now() + timeout;
    let mut answered = 0;
    while answered < tabs.len() {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        match receiver.recv_timeout(remaining) {
            Ok(TaggedServerMessage {
                request_id: Some(id),
                message: ServerMessage::Representation(snapshot),
                ..
            }) => {
                let index = id.wrapping_sub(base).wrapping_sub(1) as usize;
                if index < counts.len() && counts[index].is_none() {
                    counts[index] = Some(snapshot.nodes.len());
                    answered += 1;
                }
            }
            Ok(_) => continue, // some other client's concurrent broadcast traffic
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    counts
}

/// Whether v2's rendering of a tab is structurally comparable to v1's. A
/// first-pass bar, as `phase-8-live-core-hotswap/PLAN.md` planned: v2 must not
/// be blank when v1 was not, and its node count must stay within half to double
/// of v1's. That catches a blank or crashed render without failing on a page
/// whose content legitimately changed between the two fetches. A tab v1 itself
/// showed nothing for (a blank tab) constrains nothing.
fn structure_comparable(v1_nodes: usize, v2_nodes: usize) -> bool {
    if v1_nodes == 0 {
        return true;
    }
    v2_nodes > 0 && v2_nodes * 2 >= v1_nodes && v2_nodes <= v1_nodes * 2
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
fn replay_tabs(
    v2_stream: &mut UnixStream,
    captured_tabs: &[TabSummary],
) -> Result<Vec<TaggedServerMessage>, String> {
    let mut replay_frames = Vec::new();
    for (i, tab) in captured_tabs.iter().enumerate() {
        if i == 0 {
            let Some(url) = &tab.url else { continue };
            let request_id = synthetic_request_id();
            write_client_message_with_id(
                v2_stream,
                Some(request_id),
                &ClientMessage::Navigate { url: url.clone() },
            )
            .map_err(|e| format!("failed to replay the default tab's Navigate into v2: {e}"))?;
            replay_frames.push(expect_navigate_success(v2_stream, request_id)?);
        } else {
            let request_id = synthetic_request_id();
            write_client_message_with_id(
                v2_stream,
                Some(request_id),
                &ClientMessage::OpenTab {
                    url: tab.url.clone(),
                },
            )
            .map_err(|e| format!("failed to replay tab {i}'s OpenTab into v2: {e}"))?;
            if let Some(frame) = expect_open_tab_success(v2_stream, request_id, tab.url.is_some())? {
                replay_frames.push(frame);
            }
        }
    }
    Ok(replay_frames)
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
fn expect_navigate_success(
    v2_stream: &mut UnixStream,
    request_id: u64,
) -> Result<TaggedServerMessage, String> {
    let mut navigated = false;
    loop {
        let (tab_id, reply_id, message) = read_server_message_with_ids(v2_stream)
            .map_err(|e| format!("failed reading v2's reply while replaying: {e}"))?;
        if reply_id != Some(request_id) {
            continue;
        }
        match message {
            ServerMessage::Navigated { .. } => navigated = true,
            frame @ ServerMessage::FrameReady { .. } if navigated => {
                return Ok(TaggedServerMessage {
                    tab_id,
                    request_id: None, // a cutover handoff, not the synthetic replay request
                    message: frame,
                });
            }
            ServerMessage::FrameReady { .. } => continue, // shouldn't happen before Navigated, but don't misinterpret
            ServerMessage::Error { message } => {
                return Err(format!("v2 rejected the replayed Navigate: {message}"));
            }
            ServerMessage::GatekeeperBlocked { reason, .. } => {
                return Err(format!(
                    "v2's gatekeeper blocked the replayed Navigate: {reason}"
                ));
            }
            _ => continue,
        }
    }
}

/// Like [`expect_navigate_success`], for a replayed `OpenTab` --
/// `session.rs`'s `handle_open_tab` only sends a `FrameReady` after
/// `TabOpened` when the tab was actually navigated (`expects_frame`),
/// so a blank replayed tab is considered done as soon as `TabOpened`
/// itself arrives.
fn expect_open_tab_success(
    v2_stream: &mut UnixStream,
    request_id: u64,
    expects_frame: bool,
) -> Result<Option<TaggedServerMessage>, String> {
    let mut opened = false;
    loop {
        let (tab_id, reply_id, message) = read_server_message_with_ids(v2_stream)
            .map_err(|e| format!("failed reading v2's reply while replaying: {e}"))?;
        if reply_id != Some(request_id) {
            continue;
        }
        match message {
            ServerMessage::TabOpened { .. } => {
                opened = true;
                if !expects_frame {
                    return Ok(None);
                }
            }
            frame @ ServerMessage::FrameReady { .. } if opened => {
                return Ok(Some(TaggedServerMessage {
                    tab_id,
                    request_id: None,
                    message: frame,
                }));
            }
            ServerMessage::FrameReady { .. } => continue,
            ServerMessage::Error { message } => {
                return Err(format!("v2 rejected the replayed OpenTab: {message}"));
            }
            ServerMessage::GatekeeperBlocked { reason, .. } => {
                return Err(format!(
                    "v2's gatekeeper blocked the replayed OpenTab: {reason}"
                ));
            }
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
fn health_check(
    v2_stream: &mut UnixStream,
    captured_tabs: &[TabSummary],
) -> Result<Vec<u64>, String> {
    let request_id = synthetic_request_id();
    write_client_message_with_id(v2_stream, Some(request_id), &ClientMessage::ListTabs)
        .map_err(|e| format!("failed to send v2's health-check ListTabs: {e}"))?;
    loop {
        let (reply_id, message) = read_server_message_with_id(v2_stream)
            .map_err(|e| format!("failed reading v2's health-check reply: {e}"))?;
        if matches!(reply_id, Some(id) if id != request_id) {
            continue;
        }
        match message {
            ServerMessage::Tabs(tabs) => {
                let expected: Vec<&Option<String>> = captured_tabs.iter().map(|t| &t.url).collect();
                let actual: Vec<&Option<String>> = tabs.iter().map(|t| &t.url).collect();
                return if actual == expected {
                    Ok(tabs.iter().map(|t| t.id).collect())
                } else {
                    Err(format!(
                        "v2's post-replay ListTabs didn't match what was captured from v1: expected {expected:?}, got {actual:?}"
                    ))
                };
            }
            _ => continue,
        }
    }
}

/// The structural half of the health bar: each replayed tab of v2 (`v2_tab_ids`,
/// in replay order) must render something [`structure_comparable`] to what v1
/// showed for the same tab. Tabs with no v1 measurement are skipped.
fn structural_health_check(
    v2_stream: &mut UnixStream,
    v2_tab_ids: &[u64],
    v1_node_counts: &[Option<usize>],
) -> Result<(), String> {
    for (index, tab_id) in v2_tab_ids.iter().enumerate() {
        let Some(v1_nodes) = v1_node_counts.get(index).copied().flatten() else {
            continue;
        };
        let request_id = synthetic_request_id();
        blueice_ipc::write_client_message_with_ids(
            v2_stream,
            Some(*tab_id),
            Some(request_id),
            &ClientMessage::GetRepresentation,
        )
        .map_err(|e| format!("failed to ask v2 to describe replayed tab {index}: {e}"))?;
        let v2_nodes = loop {
            let (_, reply_id, message) = read_server_message_with_ids(v2_stream).map_err(|e| {
                format!("failed reading v2's description of replayed tab {index}: {e}")
            })?;
            if reply_id != Some(request_id) {
                continue;
            }
            match message {
                ServerMessage::Representation(snapshot) => break snapshot.nodes.len(),
                ServerMessage::Error { message } => {
                    return Err(format!(
                        "v2 could not describe replayed tab {index}: {message}"
                    ));
                }
                _ => continue,
            }
        };
        if !structure_comparable(v1_nodes, v2_nodes) {
            return Err(format!(
                "v2's post-replay representation of tab {index} is structurally different from v1's: v1 showed {v1_nodes} node(s), v2 shows {v2_nodes}"
            ));
        }
    }
    Ok(())
}

/// A fresh, unique frame directory for a newly-[`SpawnedCore::spawn`]ed
/// v2, derived from v1's own -- see [`Broker::frame_dir`]'s docs for
/// why sharing one directory between two simultaneously-live `core`
/// instances isn't safe. `target_generation` (the generation this
/// cutover attempt would bump to if it succeeds) makes this unique
/// across repeated cutover attempts too, not just between v1 and v2.
fn v2_frame_dir(v1_frame_dir: &Path, target_generation: u64) -> PathBuf {
    let stem = v1_frame_dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "frames".to_string());
    v1_frame_dir.with_file_name(format!("{stem}-cutover-{target_generation}"))
}

/// The actual swap, once replay + health check have both fully
/// succeeded. Retarget the writer under its lock, suppress late v1
/// broadcasts, hand the already-rendered v2 replay frames to existing
/// clients, then start v2's live broadcast. This ensures the frontend
/// sees the new core without needing another user action.
fn perform_swap(
    broker: &Arc<Broker>,
    v2: SpawnedCore,
    target_generation: u64,
    replay_frames: Vec<TaggedServerMessage>,
) {
    let v2_broadcast_stream = v2
        .stream
        .try_clone()
        .expect("try_clone on a fresh stream should not fail");
    let v2_writer_stream = v2
        .stream
        .try_clone()
        .expect("try_clone on a fresh stream should not fail");
    let mut writer = broker
        .core_writer
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut active = broker
        .active_core
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    // Publish the new writer, process, and generation while holding both
    // locks. A concurrent permission inspection cannot pair v1's private
    // pipe with v2's generation (or vice versa) during this transition.
    broker.generation.store(target_generation, Ordering::SeqCst);
    *writer = v2_writer_stream;
    let v1 = active.replace(v2);
    drop(active);
    if let Some(v1) = v1.as_ref() {
        // Unblocks v1's old broadcast thread's blocked read (shutdown
        // affects every fd sharing this socket's underlying open file
        // description, including that thread's own clone) so it exits
        // through the ordinary disconnect path -- which it now
        // recognizes as an expected supersession, not a fault, since
        // `generation` was already bumped above.
        let _ = v1.stream.shutdown(Shutdown::Both);
    }

    {
        let mut clients = broker.clients.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        for frame in replay_frames {
            clients.retain(|client| client.send(frame.clone()).is_ok());
        }
    }
    drop(writer);

    spawn_generation_tagged_broadcast(
        v2_broadcast_stream,
        Arc::clone(&broker.clients),
        Arc::clone(&broker.generation),
        target_generation,
        broker.done.clone(),
    );
    drop(v1); // reap v1 after clients have already received v2's frames
}

/// Performs one cutover attempt: captures v1's tab list, spawns v2,
/// replays the tabs into it, health-checks it, and -- only if all of
/// that succeeds -- performs the actual swap. A single attempt, fail
/// closed to v1 on any error -- see `phase-8-live-core-hotswap/PLAN.md`'s
/// "Wiring design" for why no retry policy exists yet. v1 is never
/// touched until every step through the health check has succeeded.
/// Why one cutover attempt did not succeed, and whether trying again could
/// plausibly help. Every failure leaves v1 untouched and serving.
#[derive(Debug, PartialEq, Eq)]
enum AttemptFailure {
    /// Environmental and plausibly transient (v2 failed to start, a reply timed
    /// out): retry.
    Retry(String),
    /// v2 ran and failed its health check. One more attempt is reasonable (it
    /// may have been a startup race); a second identical failure is
    /// deterministic.
    RetryOnce(String),
    /// Deterministic: v2 rejected the replay, or its gatekeeper blocked a
    /// replayed URL. Trying again would only repeat it.
    Final(String),
}

/// A cutover makes at most this many attempts.
const MAX_CUTOVER_ATTEMPTS: u32 = 3;
/// The pause before attempt `n + 1` is this times `2^(n - 1)`.
const CUTOVER_RETRY_BASE_PAUSE: Duration = Duration::from_millis(250);
/// How long v2 may take to answer any single replay or health-check message
/// before the attempt is abandoned rather than left hanging.
const V2_REPLY_TIMEOUT: Duration = Duration::from_secs(30);

/// Classifies a replay or health-check error message by what produced it. The
/// messages are all created in this file, and a test pins every one, so a
/// reworded message cannot silently change how it is retried.
fn classify_attempt_error(reason: String) -> AttemptFailure {
    if reason.contains("gatekeeper blocked") || reason.contains("rejected the replayed") {
        AttemptFailure::Final(reason)
    } else if reason.contains("post-replay ListTabs")
        || reason.contains("post-replay representation")
    {
        AttemptFailure::RetryOnce(reason)
    } else {
        AttemptFailure::Retry(reason)
    }
}

/// Runs `attempt` (given its 1-based number) until it succeeds or the policy
/// gives up: never more than [`MAX_CUTOVER_ATTEMPTS`], never again after a
/// [`AttemptFailure::Final`], and at most once again after a
/// [`AttemptFailure::RetryOnce`] has already happened. `pause` is called with
/// the delay before each retry (a parameter so the policy is a plain unit test).
fn run_with_retries<T>(
    mut attempt: impl FnMut(u32) -> Result<T, AttemptFailure>,
    mut pause: impl FnMut(Duration),
) -> Result<T, String> {
    let mut health_failures = 0u32;
    let mut last_reason = String::new();
    for number in 1..=MAX_CUTOVER_ATTEMPTS {
        match attempt(number) {
            Ok(done) => return Ok(done),
            Err(AttemptFailure::Final(reason)) => {
                return Err(format!("{reason} (attempt {number}; not retryable)"));
            }
            Err(AttemptFailure::RetryOnce(reason)) => {
                health_failures += 1;
                if health_failures >= 2 {
                    return Err(format!(
                        "{reason} (attempt {number}; failed the health check twice)"
                    ));
                }
                last_reason = reason;
            }
            Err(AttemptFailure::Retry(reason)) => last_reason = reason,
        }
        if number < MAX_CUTOVER_ATTEMPTS {
            pause(CUTOVER_RETRY_BASE_PAUSE * 2u32.pow(number - 1));
        }
    }
    Err(format!(
        "{last_reason} (gave up after {MAX_CUTOVER_ATTEMPTS} attempts)"
    ))
}

/// One try: spawn v2, replay v1's tabs into it, and health-check it. On any
/// failure v2 is dropped (killed and cleaned up) and v1 has not been touched.
fn attempt_cutover(
    broker: &Arc<Broker>,
    captured_tabs: &[TabSummary],
    v1_node_counts: &[Option<usize>],
    target_generation: u64,
    attempt: u32,
) -> Result<(SpawnedCore, Vec<TaggedServerMessage>), AttemptFailure> {
    let mut frame_dir = v2_frame_dir(&broker.frame_dir, target_generation);
    if attempt > 1 {
        // A failed earlier attempt may not have finished removing its own.
        let name = frame_dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        frame_dir.set_file_name(format!("{name}-retry{attempt}"));
    }
    let mut v2 = SpawnedCore::spawn_with_assistant(
        broker.width,
        broker.height,
        &frame_dir,
        &broker.gatekeeper_socket,
        broker.extension_manifest.as_deref(),
        broker.assistant.as_ref(),
    )
    .map_err(|e| AttemptFailure::Retry(format!("failed to spawn v2: {e}")))?;

    // A v2 that stops answering must fail this attempt, not hang the cutover.
    let _ = v2.stream.set_read_timeout(Some(V2_REPLY_TIMEOUT));
    let replay_frames = replay_tabs(&mut v2.stream, captured_tabs).map_err(classify_attempt_error)?;
    let v2_tab_ids =
        health_check(&mut v2.stream, captured_tabs).map_err(classify_attempt_error)?;
    structural_health_check(&mut v2.stream, &v2_tab_ids, v1_node_counts)
        .map_err(classify_attempt_error)?;
    // v2's stream is about to become the live connection, read by a broadcast
    // thread that blocks indefinitely by design.
    let _ = v2.stream.set_read_timeout(None);
    Ok((v2, replay_frames))
}

fn cutover(broker: &Arc<Broker>) -> control::ControlReply {
    let Some(_guard) = broker.cutover_gate.try_acquire() else {
        return control::ControlReply::CutoverBusy;
    };

    let captured_tabs =
        match capture_v1_tabs(&broker.core_writer, &broker.clients, TAB_CAPTURE_TIMEOUT) {
            Ok(tabs) => tabs,
            Err(reason) => return control::ControlReply::CutoverFailed { reason },
        };

    // What v1 shows for each tab, measured before v2 exists, so v2's rendering
    // has something structural to be compared against.
    let v1_node_counts = capture_v1_node_counts(
        &broker.core_writer,
        &broker.clients,
        &captured_tabs,
        TAB_CAPTURE_TIMEOUT,
    );

    let target_generation = broker.generation.load(Ordering::SeqCst) + 1;
    match run_with_retries(
        |attempt| {
            attempt_cutover(
                broker,
                &captured_tabs,
                &v1_node_counts,
                target_generation,
                attempt,
            )
        },
        thread::sleep,
    ) {
        Ok((v2, replay_frames)) => {
            let tabs_migrated = captured_tabs.len();
            perform_swap(broker, v2, target_generation, replay_frames);
            control::ControlReply::CutoverDone { tabs_migrated }
        }
        Err(reason) => control::ControlReply::CutoverFailed { reason },
    }
}

/// Handles one control-socket connection: routes cutover through the
/// single-flight gate, or performs a bounded, read-only inspection of the
/// active core's private permission pipe. This socket cannot grant or
/// revoke an extension capability.
fn handle_control_connection(mut conn: UnixStream, broker: &Arc<Broker>) -> io::Result<()> {
    let request = control::read_control_request(&mut conn)?;
    let reply = match request {
        control::ControlRequest::Cutover => cutover(broker),
        control::ControlRequest::InspectExtensionPermissions => {
            let active = broker.active_core.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            let core_generation = broker.generation.load(Ordering::SeqCst);
            match active.as_ref().map(SpawnedCore::inspect_installed_extension) {
                Some(Ok(installed)) => control::ControlReply::ExtensionPermissions {
                    core_generation,
                    installed,
                },
                Some(Err(error)) => control::ControlReply::ExtensionPermissionsUnavailable {
                    reason: error.to_string(),
                },
                None => control::ControlReply::ExtensionPermissionsUnavailable {
                    reason: "the active core is unavailable".into(),
                },
            }
        }
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
/// explicitly torn down (reaped, socket/frame_dir cleaned up) before
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
pub fn run_broker(
    rendezvous_listener: UnixListener,
    control_listener: UnixListener,
    core: SpawnedCore,
    width: f64,
    height: f64,
    gatekeeper_socket: PathBuf,
) -> io::Result<()> {
    run_broker_with_trusted_window(
        rendezvous_listener, control_listener, core, width, height,
        gatekeeper_socket, None,
    )
}

/// The same broker with an optional launcher-spawned native frontend child.
/// Only the anonymous pipes owned for that exact child can carry the private
/// trusted-window protocol; shared frontend and operator-control sockets
/// cannot grant or revoke optional permissions.
pub fn run_broker_with_trusted_window(
    rendezvous_listener: UnixListener,
    control_listener: UnixListener,
    core: SpawnedCore,
    width: f64,
    height: f64,
    gatekeeper_socket: PathBuf,
    trusted_window: Option<SpawnedTrustedWindow>,
) -> io::Result<()> {
    run_broker_with_options(
        rendezvous_listener,
        control_listener,
        core,
        width,
        height,
        gatekeeper_socket,
        BrokerOptions {
            trusted_window,
            auto_update_interval: None,
        },
    )
}

/// Optional broker behavior beyond the required arguments.
#[derive(Default)]
pub struct BrokerOptions {
    /// See [`run_broker_with_trusted_window`].
    pub trusted_window: Option<SpawnedTrustedWindow>,
    /// When set, the launcher polls its `blueice-core` binary this often and
    /// cuts over to a newer one on its own (see [`update_watch`]).
    pub auto_update_interval: Option<Duration>,
}

/// The path of the `blueice-core` binary this launcher spawns, the one an
/// update replaces.
pub fn core_binary_path() -> io::Result<PathBuf> {
    Ok(sibling_core_binary(&std::env::current_exe()?))
}

/// [`run_broker_with_trusted_window`] with the full option set.
pub fn run_broker_with_options(
    rendezvous_listener: UnixListener,
    control_listener: UnixListener,
    core: SpawnedCore,
    width: f64,
    height: f64,
    gatekeeper_socket: PathBuf,
    options: BrokerOptions,
) -> io::Result<()> {
    let BrokerOptions {
        mut trusted_window,
        auto_update_interval,
    } = options;
    let frame_dir = core.frame_dir.clone();
    let extension_manifest = core.extension_manifest.clone();
    let assistant = core.assistant.clone();
    let core_writer = Arc::new(Mutex::new(core.stream.try_clone()?));
    let broadcast_stream = core.stream.try_clone()?;
    let clients: Arc<Mutex<Vec<Sender<TaggedServerMessage>>>> = Arc::new(Mutex::new(Vec::new()));
    let generation = Arc::new(AtomicU64::new(0));
    let (done_tx, done_rx) = mpsc::channel();

    let broker = Arc::new(Broker {
        core_writer: Arc::clone(&core_writer),
        clients: Arc::clone(&clients),
        generation: Arc::clone(&generation),
        cutover_gate: CutoverGate::new(),
        active_core: Mutex::new(Some(core)),
        width,
        height,
        frame_dir,
        gatekeeper_socket,
        extension_manifest,
        assistant,
        done: done_tx.clone(),
    });

    let trusted_ready = if let Some(window) = trusted_window.as_mut() {
        let (requests, replies) = window.take_pipes()?;
        let broker = Arc::clone(&broker);
        let (ready_tx, ready_rx) = mpsc::channel();
        thread::spawn(move || {
            if let Err(error) = serve_trusted_window_pipe(requests, replies, &broker, ready_tx) {
                eprintln!("blueice-launcher: trusted window pipe ended: {error}");
            }
        });
        Some(ready_rx)
    } else {
        None
    };

    spawn_generation_tagged_broadcast(
        broadcast_stream,
        Arc::clone(&clients),
        Arc::clone(&generation),
        0,
        done_tx,
    );

    let accept_clients = Arc::clone(&clients);
    let accept_core_writer = Arc::clone(&core_writer);
    thread::spawn(move || {
        for incoming in rendezvous_listener.incoming() {
            let Ok(client) = incoming else { break };
            let _ = register_client(
                client,
                Arc::clone(&accept_core_writer),
                Arc::clone(&accept_clients),
            );
        }
    });

    let update_stop = Arc::new(AtomicBool::new(false));
    if let Some(interval) = auto_update_interval {
        match core_binary_path() {
            Ok(binary) => update_watch::spawn_update_watcher(
                Arc::clone(&broker),
                binary,
                interval,
                Arc::clone(&update_stop),
            ),
            Err(error) => eprintln!("blueice-launcher: automatic updates are off: {error}"),
        }
    }

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

    if let Some(ready) = trusted_ready {
        if ready.recv_timeout(Duration::from_secs(10)).is_err() {
            if let Ok(mut active) = broker.active_core.lock() {
                active.take();
            }
            drop(trusted_window);
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "trusted native frontend did not inspect its core before the startup deadline",
            ));
        }
    }

    let _ = done_rx.recv();
    update_stop.store(true, Ordering::Relaxed);

    if let Ok(mut active) = broker.active_core.lock() {
        active.take();
    }

    drop(trusted_window);

    Ok(())
}

fn serve_trusted_window_pipe<R: Read, W: Write>(
    mut requests: R,
    mut replies: W,
    broker: &Arc<Broker>,
    ready: Sender<()>,
) -> io::Result<()> {
    let mut ready = Some(ready);
    let mut reviewed_ephemeral = None;
    let result = (|| {
        while let Some(request) = trusted_window::read_request(&mut requests)? {
            let inspected = matches!(&request, trusted_window::TrustedWindowRequest::Inspect);
            let reply = handle_trusted_window_session_request(
                request, broker, &mut reviewed_ephemeral,
            );
            trusted_window::write_reply(&mut replies, &reply)?;
            if inspected && matches!(reply, trusted_window::TrustedWindowReply::State { .. }) {
                if let Some(ready) = ready.take() {
                    let _ = ready.send(());
                }
            }
        }
        Ok(())
    })();
    // The native window is the lifetime owner of its human-approved grants.
    // EOF, malformed frames, or a broken reply pipe must tear down the
    // active core, not leave those grants serving to ordinary/MCP clients.
    let _ = broker.done.send(());
    result
}

#[derive(PartialEq, Eq)]
struct ReviewedEphemeral {
    core_generation: u64,
    extension_id: String,
    capability: String,
    tab_id: u64,
    document_epoch: u64,
}

/// The private native pipe itself requires a successful review immediately
/// before one matching confirmation. Even a malformed or replayed request
/// from that exact child cannot skip the review or reuse it for a second arm.
fn handle_trusted_window_session_request(
    request: trusted_window::TrustedWindowRequest,
    broker: &Arc<Broker>,
    reviewed_ephemeral: &mut Option<ReviewedEphemeral>,
) -> trusted_window::TrustedWindowReply {
    let confirmation_matches_review = match &request {
        trusted_window::TrustedWindowRequest::ArmEphemeral {
            expected_core_generation, expected_extension_id, capability,
            tab_id, document_epoch,
        } => reviewed_ephemeral.as_ref().is_some_and(|review| {
            review.core_generation == *expected_core_generation
                && review.extension_id == *expected_extension_id
                && review.capability == *capability
                && review.tab_id == *tab_id
                && review.document_epoch == *document_epoch
        }),
        _ => true,
    };
    // Any intervening request cancels the old review. Arm consumes it before
    // reaching core, including when cutover or a changed document rejects it.
    *reviewed_ephemeral = None;
    if !confirmation_matches_review {
        return trusted_window::TrustedWindowReply::Rejected {
            reason: "review the live one-shot document before confirming it".into(),
        };
    }
    let reply = handle_trusted_window_request(request, broker);
    if let trusted_window::TrustedWindowReply::EphemeralReview {
        core_generation, installed, capability, tab_id, document_epoch, ..
    } = &reply {
        *reviewed_ephemeral = Some(ReviewedEphemeral {
            core_generation: *core_generation,
            extension_id: installed.extension_id.clone(),
            capability: capability.clone(),
            tab_id: *tab_id,
            document_epoch: *document_epoch,
        });
    }
    reply
}

fn handle_trusted_window_request(
    request: trusted_window::TrustedWindowRequest,
    broker: &Arc<Broker>,
) -> trusted_window::TrustedWindowReply {
    let result = (|| -> Result<trusted_window::TrustedWindowReply, String> {
        let mutating = matches!(&request,
            trusted_window::TrustedWindowRequest::Change { .. }
                | trusted_window::TrustedWindowRequest::ArmEphemeral { .. });
        // A mutation must not race a capture/replay/swap. A busy cutover
        // rejects it; the person can inspect the new generation afterward.
        let _cutover_guard = if mutating {
            Some(broker.cutover_gate.try_acquire().ok_or_else(||
                "a core cutover is in progress; inspect permissions again".to_string())?)
        } else { None };
        let mut active = broker.active_core.lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let generation = broker.generation.load(Ordering::SeqCst);
        let core = active.as_mut().ok_or_else(|| "the active core is unavailable".to_string())?;
        let installed = core.inspect_installed_extension()
            .map_err(|error| format!("the active core permission state is unavailable: {error}"))?;
        match request {
            trusted_window::TrustedWindowRequest::Inspect => {
                Ok(trusted_window::TrustedWindowReply::State {
                    core_generation: generation, installed,
                })
            }
            trusted_window::TrustedWindowRequest::Change {
                expected_core_generation, expected_extension_id, capability, action,
            } => {
                trusted_window::validate_change_target(
                    generation, installed.as_ref(), expected_core_generation,
                    &expected_extension_id, &capability,
                ).map_err(str::to_string)?;
                let updated = core.apply_optional_change(action, &capability)
                    .map_err(|error| format!("the active core could not confirm the permission change: {error}"))?;
                if updated.extension_id != expected_extension_id {
                    let _ = core.child.kill();
                    return Err("the installed extension changed during permission confirmation".into());
                }
                Ok(trusted_window::TrustedWindowReply::State {
                    core_generation: generation, installed: Some(updated),
                })
            }
            trusted_window::TrustedWindowRequest::InspectEphemeral {
                expected_core_generation, expected_extension_id, capability, tab_id,
            } => {
                trusted_window::validate_ephemeral_target(
                    generation, installed.as_ref(), expected_core_generation,
                    &expected_extension_id, &capability, tab_id,
                ).map_err(str::to_string)?;
                let (document_epoch, url) = core.inspect_document(tab_id)
                    .map_err(|error| format!("the live document is unavailable: {error}"))?;
                let url = url.filter(|url| url.starts_with("http://") || url.starts_with("https://"))
                    .ok_or_else(|| "one-shot DOM reads require a live HTTP(S) page".to_string())?;
                Ok(trusted_window::TrustedWindowReply::EphemeralReview {
                    core_generation: generation,
                    installed: installed.expect("a validated one-shot target has an installed package"),
                    capability, tab_id, document_epoch, url,
                })
            }
            trusted_window::TrustedWindowRequest::ArmEphemeral {
                expected_core_generation, expected_extension_id, capability,
                tab_id, document_epoch,
            } => {
                trusted_window::validate_ephemeral_target(
                    generation, installed.as_ref(), expected_core_generation,
                    &expected_extension_id, &capability, tab_id,
                ).map_err(str::to_string)?;
                let (current_epoch, url) = core.inspect_document(tab_id)
                    .map_err(|error| format!("the live document is unavailable: {error}"))?;
                if current_epoch != document_epoch || !url.as_deref().is_some_and(|url|
                    url.starts_with("http://") || url.starts_with("https://")) {
                    return Err("the reviewed HTTP(S) document changed before confirmation".into());
                }
                core.arm_ephemeral(&capability, tab_id, document_epoch)
                    .map_err(|error| format!("the active core could not confirm the one-shot read: {error}"))??;
                Ok(trusted_window::TrustedWindowReply::EphemeralArmed {
                    core_generation: generation,
                    installed: installed.expect("a validated one-shot target has an installed package"),
                    capability, tab_id, document_epoch,
                })
            }
        }
    })();
    match result {
        Ok(reply) => reply,
        Err(reason) => trusted_window::TrustedWindowReply::Rejected { reason },
    }
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
    let name = if cfg!(windows) {
        "blueice-core.exe"
    } else {
        "blueice-core"
    };
    let dir = this_exe.parent().unwrap_or_else(|| Path::new("."));
    let dir = if dir.file_name().is_some_and(|n| n == "deps") {
        dir.parent().unwrap_or(dir)
    } else {
        dir
    };
    dir.join(name)
}

/// The BlueJS daemon lives beside `blueice-core` in the installed/cargo
/// target directory. Keep this lookup parallel to [`sibling_core_binary`]
/// rather than trusting `$PATH`, so the launcher never pairs one checkout's
/// core with another checkout's script engine.
fn sibling_bluejs_binary(this_exe: &Path) -> PathBuf {
    let name = if cfg!(windows) {
        "bluejs.exe"
    } else {
        "bluejs"
    };
    let dir = this_exe.parent().unwrap_or_else(|| Path::new("."));
    let dir = if dir.file_name().is_some_and(|n| n == "deps") {
        dir.parent().unwrap_or(dir)
    } else {
        dir
    };
    dir.join(name)
}

/// The authenticated extension host must be from the same installed build as
/// launcher and core; never resolve a different executable from `$PATH`.
fn sibling_extension_host_binary(this_exe: &Path) -> PathBuf {
    let name = if cfg!(windows) {
        "blueice-extension-host.exe"
    } else {
        "blueice-extension-host"
    };
    let dir = this_exe.parent().unwrap_or_else(|| Path::new("."));
    let dir = if dir.file_name().is_some_and(|n| n == "deps") {
        dir.parent().unwrap_or(dir)
    } else {
        dir
    };
    dir.join(name)
}

/// The native frontend trusted for permission confirmation must
/// be from the same installed build. Never accept an arbitrary binary path
/// from a control-socket caller as a substitute for this child.
fn sibling_frontend_binary(this_exe: &Path) -> PathBuf {
    let name = if cfg!(windows) { "blueice-frontend.exe" } else { "blueice-frontend" };
    let dir = this_exe.parent().unwrap_or_else(|| Path::new("."));
    let dir = if dir.file_name().is_some_and(|n| n == "deps") {
        dir.parent().unwrap_or(dir)
    } else {
        dir
    };
    dir.join(name)
}

/// An exact sibling native frontend launched by this launcher. Its stdin and
/// stdout are private anonymous pipes, not the shared frontend/MCP socket.
/// Only this child can ask for native confirmation. Launcher independently
/// validates and applies each decision through the active core's private pipe.
pub struct SpawnedTrustedWindow {
    child: Child,
}

impl SpawnedTrustedWindow {
    pub fn spawn(rendezvous_socket: &Path) -> io::Result<Self> {
        let frontend_bin = sibling_frontend_binary(&std::env::current_exe()?);
        let child = Command::new(&frontend_bin)
            .arg("--socket")
            .arg(rendezvous_socket)
            .arg("--trusted-window-stdio")
            .arg("--url")
            .arg("about:blank")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .map_err(|error| io::Error::new(error.kind(), format!(
                "failed to start trusted native frontend {}: {error}", frontend_bin.display()
            )))?;
        Ok(Self { child })
    }

    fn take_pipes(&mut self) -> io::Result<(ChildStdout, ChildStdin)> {
        let requests = self.child.stdout.take().ok_or_else(|| {
            io::Error::other("trusted frontend has no private request pipe")
        })?;
        let replies = self.child.stdin.take().ok_or_else(|| {
            io::Error::other("trusted frontend has no private reply pipe")
        })?;
        Ok((requests, replies))
    }
}

impl Drop for SpawnedTrustedWindow {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The always-resident safety-gatekeeper daemon lives beside `core` and
/// BlueJS. Resolve it relative to the launcher rather than `$PATH`, so
/// one installed BlueIce release never launches another release's policy
/// process by accident.
fn sibling_gatekeeper_binary(this_exe: &Path) -> PathBuf {
    let name = if cfg!(windows) {
        "blueice-ai-gatekeeper.exe"
    } else {
        "blueice-ai-gatekeeper"
    };
    let dir = this_exe.parent().unwrap_or_else(|| Path::new("."));
    let dir = if dir.file_name().is_some_and(|n| n == "deps") {
        dir.parent().unwrap_or(dir)
    } else {
        dir
    };
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
    std::env::temp_dir().join(format!(
        "blueice-launcher-core-{}-{n}.sock",
        std::process::id()
    ))
}

fn unique_internal_script_socket_path() -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "blueice-launcher-bluejs-{}-{n}.sock",
        std::process::id()
    ))
}

fn unique_internal_extension_socket_path() -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    // Core's extension listener requires a private directory, and Darwin's
    // AF_UNIX path limit leaves little room for the leaf name.
    blueice_ipc::local_socket::default_socket_dir().join(format!(
        "l-ext-{}-{n}.sock", std::process::id()
    ))
}

/// A per-launcher private socket for its one always-resident
/// gatekeeper. It is deliberately not the well-known standalone socket:
/// multiple launchers may run at once, each with its own supervised
/// process and independent shutdown lifecycle.
fn unique_internal_gatekeeper_socket_path() -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    // `ai-gatekeeper` deliberately binds only within a private socket
    // directory. Do not use `temp_dir()` directly here: it can be an
    // OS-managed shared directory whose mode the gatekeeper must not
    // chmod merely to create one per-launcher socket.
    blueice_ipc::local_socket::default_socket_dir().join(format!(
        "blueice-launcher-gatekeeper-{}-{n}.sock",
        std::process::id()
    ))
}

/// [`wait_for_socket`], but gives up at once if `child` exits first: a core
/// that died on startup will never create its socket, and waiting out the full
/// timeout only delays reporting (and retrying) the failure.
fn wait_for_socket_or_exit(path: &Path, child: &mut Child, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if path.exists() {
            return true;
        }
        if child.try_wait().ok().flatten().is_some() {
            return false;
        }
        thread::sleep(Duration::from_millis(20));
    }
    false
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

/// The local deterministic rule-base process a launcher owns for its
/// full lifetime. It is intentionally separate from [`SpawnedCore`]:
/// a core hot-swap must preserve the same mandatory checkpoint instead
/// of creating a safety gap between v1 and v2.
pub struct SpawnedGatekeeper {
    child: Child,
    socket_path: PathBuf,
}

impl SpawnedGatekeeper {
    pub fn spawn() -> io::Result<Self> {
        let this_exe = std::env::current_exe()?;
        let gatekeeper_bin = sibling_gatekeeper_binary(&this_exe);
        let socket_path = unique_internal_gatekeeper_socket_path();
        let _ = std::fs::remove_file(&socket_path);

        let mut child = Command::new(&gatekeeper_bin)
            .arg("--socket")
            .arg(&socket_path)
            .spawn()?;
        if !wait_for_socket(&socket_path, Duration::from_secs(5)) {
            let _ = child.kill();
            let _ = child.wait();
            let _ = std::fs::remove_file(&socket_path);
            return Err(io::Error::other(format!(
                "blueice-ai-gatekeeper never created its socket at {}",
                socket_path.display()
            )));
        }
        Ok(Self { child, socket_path })
    }

    /// The private socket path every core managed by this launcher must
    /// use for its mandatory gatekeeper checks.
    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }
}

impl Drop for SpawnedGatekeeper {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.socket_path);
    }
}

const PERMISSION_INSPECT_TIMEOUT: Duration = Duration::from_secs(2);
const PERMISSION_CHANGE_TIMEOUT: Duration = Duration::from_secs(5);

type PermissionExchange = (
    PermissionControlRequest,
    Sender<io::Result<PermissionControlReply>>,
);

/// Owns the only parent-side handles of one core generation's permission
/// pipe. The worker serializes bounded requests; its sender is not exposed
/// through frontend IPC or to an extension guest. Grant/Revoke callers must
/// still prove they originate from the launcher's native confirmation UI.
struct PermissionControlChannel {
    requests: SyncSender<PermissionExchange>,
}

fn serve_permission_control_worker<R: Read, W: Write>(
    mut input: W,
    mut output: R,
    pending: Receiver<PermissionExchange>,
) {
    for (request, answer) in pending {
        let result = write_permission_control_request(&mut input, &request)
            .and_then(|()| read_permission_control_reply(&mut output));
        let failed = result.is_err();
        let _ = answer.send(result);
        if failed {
            break;
        }
    }
    // Closing input tells core to revoke every optional grant made on
    // this parent's pipe, including after a failed or unanswered exchange.
}

impl PermissionControlChannel {
    fn new(input: ChildStdin, output: ChildStdout) -> io::Result<Self> {
        let (requests, pending) = mpsc::sync_channel::<PermissionExchange>(1);
        thread::Builder::new()
            .name("blueice-permission-control".into())
            .spawn(move || serve_permission_control_worker(input, output, pending))?;
        Ok(Self { requests })
    }

    fn inspect(&self) -> io::Result<PermissionControlReply> {
        self.exchange(PermissionControlRequest::Inspect, PERMISSION_INSPECT_TIMEOUT)
    }

    fn exchange(
        &self,
        request: PermissionControlRequest,
        timeout: Duration,
    ) -> io::Result<PermissionControlReply> {
        let (answer, reply) = mpsc::channel();
        self.requests.try_send((request, answer)).map_err(|error| match error {
            TrySendError::Full(_) => io::Error::new(
                io::ErrorKind::WouldBlock,
                "core permission control is busy",
            ),
            TrySendError::Disconnected(_) => io::Error::new(
                io::ErrorKind::BrokenPipe,
                "core permission pipe is closed",
            ),
        })?;
        match reply.recv_timeout(timeout) {
            Ok(result) => result,
            Err(RecvTimeoutError::Timeout) => Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "core permission control timed out",
            )),
            Err(RecvTimeoutError::Disconnected) => Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "core permission control ended without a reply",
            )),
        }
    }
}

/// A `core` process this launcher spawned and owns privately: killed
/// and cleaned up (process, internal socket, and frame directory) on
/// [`Drop`], the same lifetime discipline `mcp-server`'s `CoreProcess`
/// already established for its own (today, unshared) spawned `core`.
/// What the launcher tells every `core` it starts about the assistant it
/// supervises: the public socket to reach it, and the settings file `core` may
/// show (read-only) on `about:assistant`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssistantWiring {
    pub socket: PathBuf,
    pub settings_file: Option<PathBuf>,
}

pub struct SpawnedCore {
    child: Child,
    script_child: Child,
    internal_socket_path: PathBuf,
    script_socket_path: PathBuf,
    extension_socket_path: Option<PathBuf>,
    extension_manifest: Option<PathBuf>,
    /// The launcher-owned assistant wiring this core was given, so a cutover's
    /// replacement core is given the same.
    assistant: Option<AssistantWiring>,
    permission_control: Option<PermissionControlChannel>,
    frame_dir: PathBuf,
    pub stream: UnixStream,
}

impl SpawnedCore {
    /// Spawns a standalone core using the conventional gatekeeper
    /// socket. Launcher-managed cores should use
    /// [`Self::spawn_with_gatekeeper`] with their private supervised
    /// gatekeeper instead.
    pub fn spawn(width: f64, height: f64, frame_dir: &Path) -> io::Result<Self> {
        let gatekeeper_socket = blueice_ipc::gatekeeper::default_gatekeeper_socket_path();
        Self::spawn_with_gatekeeper(width, height, frame_dir, &gatekeeper_socket)
    }

    /// Spawns a core wired to `gatekeeper_socket`, which must be a
    /// live, launcher-supervised mandatory checkpoint for production
    /// launcher use.
    pub fn spawn_with_gatekeeper(
        width: f64,
        height: f64,
        frame_dir: &Path,
        gatekeeper_socket: &Path,
    ) -> io::Result<Self> {
        Self::spawn_with_gatekeeper_and_extension(
            width, height, frame_dir, gatekeeper_socket, None,
        )
    }

    /// Starts an installed package through core's authenticated extension
    /// host when a manifest is supplied. Each cutover gets a fresh private
    /// extension socket and core-generated child credential. Optional grants
    /// are not copied to v2; a new core starts from manifest-declared grants.
    pub fn spawn_with_gatekeeper_and_extension(
        width: f64,
        height: f64,
        frame_dir: &Path,
        gatekeeper_socket: &Path,
        extension_manifest: Option<&Path>,
    ) -> io::Result<Self> {
        Self::spawn_with_assistant(
            width,
            height,
            frame_dir,
            gatekeeper_socket,
            extension_manifest,
            None,
        )
    }

    /// [`Self::spawn_with_gatekeeper_and_extension`] plus the launcher's public
    /// assistant socket, which core is given as `--assistant-socket` so live
    /// translation and assistant tasks reach the supervised assistant.
    pub fn spawn_with_assistant(
        width: f64,
        height: f64,
        frame_dir: &Path,
        gatekeeper_socket: &Path,
        extension_manifest: Option<&Path>,
        assistant: Option<&AssistantWiring>,
    ) -> io::Result<Self> {
        let this_exe = std::env::current_exe()?;
        let core_bin = sibling_core_binary(&this_exe);
        let bluejs_bin = sibling_bluejs_binary(&this_exe);
        let internal_socket_path = unique_internal_socket_path();
        let script_socket_path = unique_internal_script_socket_path();
        let extension_socket_path = extension_manifest.map(|_| unique_internal_extension_socket_path());
        let _ = std::fs::remove_file(&internal_socket_path);
        let _ = std::fs::remove_file(&script_socket_path);
        if let Some(path) = extension_socket_path.as_deref() {
            let _ = std::fs::remove_file(path);
        }

        let mut command = Command::new(&core_bin);
        command.arg("--socket")
            .arg(&internal_socket_path)
            .arg("--width")
            .arg(width.to_string())
            .arg("--height")
            .arg(height.to_string())
            .arg("--frame-dir")
            .arg(frame_dir)
            .arg("--gatekeeper-socket")
            .arg(gatekeeper_socket)
            .arg("--script-socket")
            .arg(&script_socket_path);
        if let Some(assistant) = assistant {
            command.arg("--assistant-socket").arg(&assistant.socket);
            if let Some(settings) = &assistant.settings_file {
                command.arg("--assistant-settings").arg(settings);
            }
        }
        if let (Some(manifest), Some(socket)) = (extension_manifest, extension_socket_path.as_deref()) {
            command.arg("--extension-socket").arg(socket)
                .arg("--extension-manifest").arg(manifest)
                .arg("--extension-host").arg(sibling_extension_host_binary(&this_exe))
                .arg("--permission-control-stdio")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped());
        }
        let mut child = command.spawn()?;

        // Core itself gives its authenticated extension host five seconds to
        // connect, then stops that child on failure. Leave a little margin
        // before launcher's fallback kill so we do not interrupt that cleanup.
        let startup_timeout = if extension_manifest.is_some() {
            Duration::from_secs(7)
        } else {
            Duration::from_secs(5)
        };
        if !wait_for_socket_or_exit(&internal_socket_path, &mut child, startup_timeout) {
            let _ = child.kill();
            let _ = child.wait();
            if let Some(path) = extension_socket_path.as_deref() {
                let _ = std::fs::remove_file(path);
            }
            return Err(io::Error::other(format!(
                "blueice-core never created its socket at {}",
                internal_socket_path.display()
            )));
        }
        let script_child = match Command::new(&bluejs_bin)
            .arg("--script-socket")
            .arg(&script_socket_path)
            .spawn()
        {
            Ok(child) => child,
            Err(error) => {
                let mut child = child;
                let _ = child.kill();
                let _ = child.wait();
                let _ = std::fs::remove_file(&internal_socket_path);
                let _ = std::fs::remove_file(&script_socket_path);
                if let Some(path) = extension_socket_path.as_deref() {
                    let _ = std::fs::remove_file(path);
                }
                return Err(io::Error::new(
                    error.kind(),
                    format!(
                        "failed to spawn bluejs at {}: {error}",
                        bluejs_bin.display()
                    ),
                ));
            }
        };
        let mut stream = match UnixStream::connect(&internal_socket_path) {
            Ok(stream) => stream,
            Err(error) => {
                let mut script_child = script_child;
                let _ = script_child.kill();
                let _ = script_child.wait();
                let _ = child.kill();
                let _ = child.wait();
                let _ = std::fs::remove_file(&internal_socket_path);
                let _ = std::fs::remove_file(&script_socket_path);
                if let Some(path) = extension_socket_path.as_deref() {
                    let _ = std::fs::remove_file(path);
                }
                return Err(error);
            }
        };
        // `core` requires the very first message on a fresh connection
        // to be `Hello` (`phase-1-ai-representation-layer/PLAN.md` §3);
        // this launcher is the connection's one and only direct client,
        // so it satisfies that gate itself, once, here -- external
        // clients connecting through the rendezvous socket send their
        // own `Hello` too, but by the time the broker forwards it into
        // this already-past-its-handshake connection, `core` just
        // answers it again rather than re-gating (see `blueice_engine::
        // session::run_session`'s own docs).
        if let Err(error) = blueice_ipc::client_handshake(&mut stream) {
            let mut script_child = script_child;
            let _ = script_child.kill();
            let _ = script_child.wait();
            let _ = child.kill();
            let _ = child.wait();
            let _ = std::fs::remove_file(&internal_socket_path);
            let _ = std::fs::remove_file(&script_socket_path);
            if let Some(path) = extension_socket_path.as_deref() {
                let _ = std::fs::remove_file(path);
            }
            return Err(error);
        }
        let mut core = SpawnedCore {
            child,
            script_child,
            internal_socket_path,
            script_socket_path,
            extension_socket_path,
            extension_manifest: extension_manifest.map(Path::to_path_buf),
            assistant: assistant.cloned(),
            permission_control: None,
            frame_dir: frame_dir.to_path_buf(),
            stream,
        };
        // A responsive, authenticated host is not enough: prove that this
        // generation's private parent pipe actually reaches its registry
        // before admitting it as v1 or cutting over to it as v2.
        if extension_manifest.is_some() {
            let input = core.child.stdin.take().ok_or_else(|| {
                io::Error::other("installed core has no private permission input pipe")
            })?;
            let output = core.child.stdout.take().ok_or_else(|| {
                io::Error::other("installed core has no private permission output pipe")
            })?;
            core.permission_control = Some(PermissionControlChannel::new(input, output)?);
            core.inspect_installed_extension()?.ok_or_else(|| {
                io::Error::other("installed core did not report extension permissions")
            })?;
        }
        Ok(core)
    }

    fn inspect_installed_extension(&self) -> io::Result<Option<control::InstalledExtensionPermissions>> {
        let Some(channel) = self.permission_control.as_ref() else {
            return Ok(None);
        };
        match channel.inspect()? {
            PermissionControlReply::State { extension_id, name, version, optional, runtime_ephemeral } => {
                Ok(Some(control::InstalledExtensionPermissions {
                    extension_id, name, version, optional, runtime_ephemeral,
                }))
            }
            _ => Err(io::Error::other(
                "core returned a non-state permission inspection reply"
            )),
        }
    }

    fn inspect_document(&self, tab_id: u64) -> io::Result<(u64, Option<String>)> {
        let channel = self.permission_control.as_ref().ok_or_else(|| {
            io::Error::other("the active core has no installed permission channel")
        })?;
        match channel.exchange(
            PermissionControlRequest::InspectDocument { tab_id }, PERMISSION_INSPECT_TIMEOUT,
        )? {
            PermissionControlReply::Document { tab_id: observed, document_epoch, url }
                if observed == tab_id => Ok((document_epoch, url)),
            PermissionControlReply::Rejected { reason } => Err(io::Error::other(reason)),
            _ => Err(io::Error::other(
                "core returned an unexpected document inspection reply"
            )),
        }
    }

    /// A core rejection (for example, a navigation after native review) is
    /// expected and leaves the core serving. Only an uncertain transport or
    /// malformed success kills the generation, preventing a late lease from
    /// outliving its trusted window.
    fn arm_ephemeral(
        &mut self, capability: &str, tab_id: u64, document_epoch: u64,
    ) -> io::Result<Result<(), String>> {
        let outcome = (|| {
            let channel = self.permission_control.as_ref().ok_or_else(|| {
                io::Error::other("the active core has no installed permission channel")
            })?;
            match channel.exchange(
                PermissionControlRequest::ArmEphemeral {
                    capability: capability.to_string(), tab_id, document_epoch,
                },
                PERMISSION_CHANGE_TIMEOUT,
            )? {
                PermissionControlReply::EphemeralArmed {
                    capability: armed, tab_id: armed_tab,
                    document_epoch: armed_epoch, ticket,
                } if armed == capability && armed_tab == tab_id
                    && armed_epoch == document_epoch && ticket.len() == 64
                    && ticket.bytes().all(|byte| byte.is_ascii_hexdigit()) => Ok(Ok(())),
                PermissionControlReply::Rejected { reason } => Ok(Err(reason)),
                _ => Err(io::Error::other(
                    "core returned an inconsistent one-shot permission reply"
                )),
            }
        })();
        if outcome.is_err() {
            let _ = self.child.kill();
        }
        outcome
    }

    /// The caller must hold the broker's cutover gate and active-core lock
    /// after validating this package/capability against a native decision.
    /// An uncertain mutating reply cannot leave a possibly granted core
    /// serving: terminate it, which also withdraws its process-local grants.
    fn apply_optional_change(
        &mut self,
        action: trusted_window::PermissionAction,
        capability: &str,
    ) -> io::Result<control::InstalledExtensionPermissions> {
        let outcome = (|| {
            let channel = self.permission_control.as_ref().ok_or_else(|| {
                io::Error::other("the active core has no installed permission channel")
            })?;
            let (request, expected_granted) = match action {
                trusted_window::PermissionAction::Grant => (
                    PermissionControlRequest::Grant { capability: capability.to_string() }, true,
                ),
                trusted_window::PermissionAction::Revoke => (
                    PermissionControlRequest::Revoke { capability: capability.to_string() }, false,
                ),
            };
            match channel.exchange(request, PERMISSION_CHANGE_TIMEOUT)? {
                PermissionControlReply::Updated { capability: updated, granted, .. }
                    if updated == capability && granted == expected_granted => {}
                PermissionControlReply::Rejected { reason } => return Err(io::Error::other(reason)),
                _ => return Err(io::Error::other(
                    "core returned an unexpected permission change reply"
                )),
            }
            let installed = self.inspect_installed_extension()?.ok_or_else(|| {
                io::Error::other("the installed extension disappeared after a permission change")
            })?;
            if !installed.optional.iter().any(|entry| {
                entry.capability == capability && entry.granted == expected_granted
            }) {
                return Err(io::Error::other(
                    "core permission state did not confirm the requested change",
                ));
            }
            Ok(installed)
        })();
        if outcome.is_err() {
            let _ = self.child.kill();
        }
        outcome
    }
}

impl Drop for SpawnedCore {
    fn drop(&mut self) {
        // Discard this generation's private authority before disconnecting
        // it. The worker closes core's stdin on EOF; core then withdraws
        // optional grants even if the normal client-shutdown path stalls.
        self.permission_control.take();
        // Let core observe its private client disconnect and reap its own
        // extension-host child before the force-kill fallback. SIGKILL first
        // would orphan that child during every successful cutover.
        let _ = self.stream.shutdown(Shutdown::Both);
        let deadline = Instant::now() + Duration::from_millis(750);
        loop {
            match self.child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
                _ => {
                    let _ = self.child.kill();
                    let _ = self.child.wait();
                    break;
                }
            }
        }
        let _ = self.script_child.kill();
        let _ = self.script_child.wait();
        let _ = std::fs::remove_file(&self.internal_socket_path);
        let _ = std::fs::remove_file(&self.script_socket_path);
        if let Some(path) = self.extension_socket_path.as_deref() {
            let _ = std::fs::remove_file(path);
        }
        // The bounded fallback still needs launcher-owned cleanup if core
        // did not get to remove its own frame directory before termination.
        let _ = std::fs::remove_dir_all(&self.frame_dir);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use blueice_ipc::*;
    use blueice_ipc::permission_control::{
        read_permission_control_request, write_permission_control_reply,
    };

    #[test]
    fn private_permission_worker_serializes_inspect_grant_and_revoke() {
        let (parent_input, mut child_input) = UnixStream::pair().unwrap();
        let (mut child_output, parent_output) = UnixStream::pair().unwrap();
        let (requests, pending) = mpsc::sync_channel(1);
        let worker = thread::spawn(move || {
            serve_permission_control_worker(parent_input, parent_output, pending)
        });
        let core = thread::spawn(move || {
            for expected in [
                PermissionControlRequest::Inspect,
                PermissionControlRequest::InspectDocument { tab_id: 7 },
                PermissionControlRequest::Grant { capability: "storage".into() },
                PermissionControlRequest::Revoke { capability: "storage".into() },
            ] {
                assert_eq!(read_permission_control_request(&mut child_input).unwrap(), Some(expected.clone()));
                let reply = match expected {
                    PermissionControlRequest::Inspect => PermissionControlReply::State {
                        extension_id: "sha256:installed".into(),
                        name: "Notes".into(),
                        version: "1".into(),
                        optional: vec![],
                        runtime_ephemeral: vec![],
                    },
                    PermissionControlRequest::InspectDocument { tab_id } => PermissionControlReply::Document {
                        tab_id, document_epoch: 3, url: Some("https://example.test/".into()),
                    },
                    PermissionControlRequest::ArmEphemeral { .. } => PermissionControlReply::Rejected {
                        reason: "the test worker has no ephemeral declaration".into(),
                    },
                    PermissionControlRequest::Grant { capability } => PermissionControlReply::Updated {
                        capability, granted: true, changed: true,
                    },
                    PermissionControlRequest::Revoke { capability } => PermissionControlReply::Updated {
                        capability, granted: false, changed: true,
                    },
                };
                write_permission_control_reply(&mut child_output, &reply).unwrap();
            }
            assert!(read_permission_control_request(&mut child_input).unwrap().is_none());
        });
        let channel = PermissionControlChannel { requests };
        assert!(matches!(channel.inspect().unwrap(), PermissionControlReply::State { .. }));
        assert!(matches!(
            channel.exchange(PermissionControlRequest::InspectDocument { tab_id: 7 }, PERMISSION_INSPECT_TIMEOUT).unwrap(),
            PermissionControlReply::Document { tab_id: 7, document_epoch: 3, .. }
        ));
        assert!(matches!(
            channel.exchange(PermissionControlRequest::Grant { capability: "storage".into() }, PERMISSION_CHANGE_TIMEOUT).unwrap(),
            PermissionControlReply::Updated { granted: true, .. }
        ));
        assert!(matches!(
            channel.exchange(PermissionControlRequest::Revoke { capability: "storage".into() }, PERMISSION_CHANGE_TIMEOUT).unwrap(),
            PermissionControlReply::Updated { granted: false, .. }
        ));
        drop(channel);
        worker.join().unwrap();
        core.join().unwrap();
    }

    #[test]
    fn native_pipe_scopes_optional_grants_and_one_shot_reads_outside_cutover() {
        use std::sync::atomic::AtomicBool;
        let root = std::env::temp_dir().join(format!(
            "blueice-native-permission-unit-{}-{}",
            std::process::id(), synthetic_request_id(),
        ));
        std::fs::create_dir(&root).unwrap();
        let (parent_input, mut child_input) = UnixStream::pair().unwrap();
        let (mut child_output, parent_output) = UnixStream::pair().unwrap();
        let (requests, pending) = mpsc::sync_channel(1);
        let worker = thread::spawn(move || {
            serve_permission_control_worker(parent_input, parent_output, pending)
        });
        let granted = Arc::new(AtomicBool::new(false));
        let grant_count = Arc::new(AtomicU64::new(0));
        let document_epoch = Arc::new(AtomicU64::new(12));
        let arm_count = Arc::new(AtomicU64::new(0));
        let simulated_core = thread::spawn({
            let granted = Arc::clone(&granted);
            let grant_count = Arc::clone(&grant_count);
            let document_epoch = Arc::clone(&document_epoch);
            let arm_count = Arc::clone(&arm_count);
            move || {
                while let Some(request) = read_permission_control_request(&mut child_input).unwrap() {
                    let reply = match request {
                        PermissionControlRequest::Inspect => PermissionControlReply::State {
                            extension_id: "sha256:installed".into(),
                            name: "Notes".into(),
                            version: "1".into(),
                            optional: vec![blueice_ipc::permission_control::OptionalCapabilityInfo {
                                capability: "storage".into(),
                                granted: granted.load(Ordering::SeqCst),
                                origins: vec!["https://example.test".into()],
                            }],
                            runtime_ephemeral: vec![blueice_ipc::permission_control::EphemeralCapabilityInfo {
                                capability: "dom:read".into(),
                                origins: vec!["https://example.test".into()],
                            }],
                        },
                        PermissionControlRequest::InspectDocument { tab_id } => PermissionControlReply::Document {
                            tab_id, document_epoch: document_epoch.load(Ordering::SeqCst),
                            url: Some("https://example.test/page".into()),
                        },
                        PermissionControlRequest::ArmEphemeral { capability, tab_id, document_epoch: expected } => {
                            if expected != document_epoch.load(Ordering::SeqCst) {
                                PermissionControlReply::Rejected { reason: "stale document".into() }
                            } else {
                                arm_count.fetch_add(1, Ordering::SeqCst);
                                PermissionControlReply::EphemeralArmed {
                                    capability, tab_id, document_epoch: expected,
                                    ticket: "a".repeat(64),
                                }
                            }
                        },
                        PermissionControlRequest::Grant { capability } => {
                            grant_count.fetch_add(1, Ordering::SeqCst);
                            let changed = !granted.swap(true, Ordering::SeqCst);
                            PermissionControlReply::Updated { capability, granted: true, changed }
                        }
                        PermissionControlRequest::Revoke { capability } => {
                            let changed = granted.swap(false, Ordering::SeqCst);
                            PermissionControlReply::Updated { capability, granted: false, changed }
                        }
                    };
                    write_permission_control_reply(&mut child_output, &reply).unwrap();
                }
            }
        });
        let (core_stream, _core_peer) = UnixStream::pair().unwrap();
        let fake_child = || Command::new("true").spawn().unwrap();
        let core = SpawnedCore {
            child: fake_child(), script_child: fake_child(),
            internal_socket_path: root.join("core.sock"),
            script_socket_path: root.join("script.sock"),
            extension_socket_path: None, extension_manifest: None, assistant: None,
            permission_control: Some(PermissionControlChannel { requests }),
            frame_dir: root.join("frames"), stream: core_stream,
        };
        let (done, done_rx) = mpsc::channel();
        let broker = Arc::new(Broker {
            core_writer: Arc::new(Mutex::new(core.stream.try_clone().unwrap())),
            clients: Arc::new(Mutex::new(Vec::new())),
            generation: Arc::new(AtomicU64::new(3)),
            cutover_gate: CutoverGate::new(),
            active_core: Mutex::new(Some(core)),
            width: 320.0, height: 200.0,
            frame_dir: root.join("frames"), gatekeeper_socket: root.join("gate.sock"),
            extension_manifest: None, assistant: None, done,
        });
        let change = |generation, id: &str, capability: &str, action| {
            trusted_window::TrustedWindowRequest::Change {
                expected_core_generation: generation,
                expected_extension_id: id.into(), capability: capability.into(), action,
            }
        };
        let initial = handle_trusted_window_request(trusted_window::TrustedWindowRequest::Inspect, &broker);
        assert!(matches!(initial, trusted_window::TrustedWindowReply::State {
            core_generation: 3, installed: Some(_),
        }));
        for invalid in [
            change(2, "sha256:installed", "storage", trusted_window::PermissionAction::Grant),
            change(3, "sha256:wrong", "storage", trusted_window::PermissionAction::Grant),
            change(3, "sha256:installed", "dom:read", trusted_window::PermissionAction::Grant),
        ] {
            assert!(matches!(handle_trusted_window_request(invalid, &broker),
                trusted_window::TrustedWindowReply::Rejected { .. }));
        }
        let cutover = broker.cutover_gate.try_acquire().unwrap();
        assert!(matches!(handle_trusted_window_request(
            change(3, "sha256:installed", "storage", trusted_window::PermissionAction::Grant), &broker,
        ), trusted_window::TrustedWindowReply::Rejected { .. }));
        drop(cutover);
        assert_eq!(grant_count.load(Ordering::SeqCst), 0);
        let granted_reply = handle_trusted_window_request(
            change(3, "sha256:installed", "storage", trusted_window::PermissionAction::Grant), &broker,
        );
        assert!(matches!(granted_reply, trusted_window::TrustedWindowReply::State {
            core_generation: 3, installed: Some(ref installed),
        } if installed.optional[0].granted));
        assert_eq!(grant_count.load(Ordering::SeqCst), 1);
        let revoked_reply = handle_trusted_window_request(
            change(3, "sha256:installed", "storage", trusted_window::PermissionAction::Revoke), &broker,
        );
        assert!(matches!(revoked_reply, trusted_window::TrustedWindowReply::State {
            core_generation: 3, installed: Some(ref installed),
        } if !installed.optional[0].granted));
        let review = |generation, id: &str, tab_id| {
            trusted_window::TrustedWindowRequest::InspectEphemeral {
                expected_core_generation: generation,
                expected_extension_id: id.into(),
                capability: "dom:read".into(), tab_id,
            }
        };
        for invalid in [
            review(2, "sha256:installed", 7),
            review(3, "sha256:other", 7),
            review(3, "sha256:installed", 0),
        ] {
            assert!(matches!(handle_trusted_window_request(invalid, &broker),
                trusted_window::TrustedWindowReply::Rejected { .. }));
        }
        let reviewed = handle_trusted_window_request(review(3, "sha256:installed", 7), &broker);
        assert!(matches!(reviewed, trusted_window::TrustedWindowReply::EphemeralReview {
            core_generation: 3, tab_id: 7, document_epoch: 12,
            ref url, ref installed, ..
        } if url == "https://example.test/page"
            && installed.runtime_ephemeral[0].origins == ["https://example.test"]));
        let arm = |epoch| trusted_window::TrustedWindowRequest::ArmEphemeral {
            expected_core_generation: 3,
            expected_extension_id: "sha256:installed".into(),
            capability: "dom:read".into(), tab_id: 7, document_epoch: epoch,
        };
        document_epoch.store(13, Ordering::SeqCst);
        assert!(matches!(handle_trusted_window_request(arm(12), &broker),
            trusted_window::TrustedWindowReply::Rejected { .. }));
        assert_eq!(arm_count.load(Ordering::SeqCst), 0,
            "navigation after review must prevent arming");
        let cutover = broker.cutover_gate.try_acquire().unwrap();
        assert!(matches!(handle_trusted_window_request(arm(13), &broker),
            trusted_window::TrustedWindowReply::Rejected { .. }));
        drop(cutover);
        assert_eq!(arm_count.load(Ordering::SeqCst), 0);
        assert!(matches!(handle_trusted_window_request(review(3, "sha256:installed", 7), &broker),
            trusted_window::TrustedWindowReply::EphemeralReview { document_epoch: 13, .. }));
        assert!(matches!(handle_trusted_window_request(arm(13), &broker),
            trusted_window::TrustedWindowReply::EphemeralArmed {
                core_generation: 3, tab_id: 7, document_epoch: 13, ..
            }));
        assert_eq!(arm_count.load(Ordering::SeqCst), 1);
        let mut inbound = Vec::new();
        for request in [
            trusted_window::TrustedWindowRequest::Inspect,
            arm(13), // no review on this trusted pipe
            review(3, "sha256:installed", 7),
            arm(12), // mismatched review consumes it
            arm(13), // the matching arm can no longer reuse it
            review(3, "sha256:installed", 7),
            arm(13), // exactly one authorized arm
            arm(13), // duplicate confirmation
            review(3, "sha256:installed", 7),
            trusted_window::TrustedWindowRequest::Inspect, // cancels review
            arm(13),
        ] {
            trusted_window::write_request(&mut inbound, &request).unwrap();
        }
        let mut outbound = Vec::new();
        let (ready, ready_rx) = mpsc::channel();
        serve_trusted_window_pipe(inbound.as_slice(), &mut outbound, &broker, ready).unwrap();
        ready_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        let mut replies = outbound.as_slice();
        for expected in [
            "state", "rejected", "review", "rejected", "rejected", "review",
            "armed", "rejected", "review", "state", "rejected",
        ] {
            let reply = trusted_window::read_reply(&mut replies).unwrap().unwrap();
            assert!(match expected {
                "state" => matches!(&reply, trusted_window::TrustedWindowReply::State { .. }),
                "review" => matches!(&reply, trusted_window::TrustedWindowReply::EphemeralReview { .. }),
                "armed" => matches!(&reply, trusted_window::TrustedWindowReply::EphemeralArmed { .. }),
                _ => matches!(&reply, trusted_window::TrustedWindowReply::Rejected { .. }),
            }, "expected {expected} from the trusted-window session, got {reply:?}");
        }
        assert!(replies.is_empty());
        assert_eq!(arm_count.load(Ordering::SeqCst), 2,
            "only a fresh matching review can reach the core's one-shot arm");
        done_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        let (ready, _ready_rx) = mpsc::channel();
        assert!(serve_trusted_window_pipe([1_u8].as_slice(), &mut Vec::new(), &broker, ready).is_err());
        done_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        broker.active_core.lock().unwrap().take();
        worker.join().unwrap();
        simulated_core.join().unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[ignore = "requires compiled sibling core, BlueJS, and extension-host binaries"]
    fn real_installed_core_confirms_private_grant_and_revoke() {
        let root = std::env::temp_dir().join(format!(
            "blueice-real-optional-permission-{}-{}",
            std::process::id(), synthetic_request_id(),
        ));
        std::fs::create_dir(&root).unwrap();
        let manifest = root.join("extension.json");
        std::fs::write(&manifest,
            r#"{"name":"Private permission proof","version":"1","blueice_api_version":1,"entry_point":"extension.wasm","capabilities":{"declared":["storage"],"optional":["dom:read"]},"capability_origins":{"dom:read":["https://example.test"]}}"#,
        ).unwrap();
        std::fs::write(root.join("extension.wasm"),
            wat::parse_str(r#"(module (func (export "blueice_start")))"#).unwrap(),
        ).unwrap();
        let core = SpawnedCore::spawn_with_gatekeeper_and_extension(
            320.0, 200.0, &root.join("frames"),
            &root.join("unused-gatekeeper.sock"), Some(&manifest),
        ).unwrap();
        let initial = core.inspect_installed_extension().unwrap().unwrap();
        assert_eq!(initial.optional.len(), 1);
        assert_eq!(initial.optional[0].capability, "dom:read");
        assert_eq!(initial.optional[0].origins, ["https://example.test"]);
        assert!(!initial.optional[0].granted);
        let (done, done_rx) = mpsc::channel();
        let broker = Arc::new(Broker {
            core_writer: Arc::new(Mutex::new(core.stream.try_clone().unwrap())),
            clients: Arc::new(Mutex::new(Vec::new())),
            generation: Arc::new(AtomicU64::new(0)),
            cutover_gate: CutoverGate::new(),
            active_core: Mutex::new(Some(core)),
            width: 320.0, height: 200.0,
            frame_dir: root.join("frames"),
            gatekeeper_socket: root.join("unused-gatekeeper.sock"),
            extension_manifest: Some(manifest), assistant: None, done,
        });
        let mut requests = Vec::new();
        for request in [
            trusted_window::TrustedWindowRequest::Inspect,
            trusted_window::TrustedWindowRequest::Change {
                expected_core_generation: 0,
                expected_extension_id: initial.extension_id.clone(),
                capability: "dom:read".into(),
                action: trusted_window::PermissionAction::Grant,
            },
            trusted_window::TrustedWindowRequest::Change {
                expected_core_generation: 0,
                expected_extension_id: initial.extension_id.clone(),
                capability: "dom:read".into(),
                action: trusted_window::PermissionAction::Revoke,
            },
        ] {
            trusted_window::write_request(&mut requests, &request).unwrap();
        }
        let mut replies = Vec::new();
        let (ready, ready_rx) = mpsc::channel();
        serve_trusted_window_pipe(requests.as_slice(), &mut replies, &broker, ready).unwrap();
        ready_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        let mut replies = replies.as_slice();
        for expected_granted in [false, true, false] {
            let reply = trusted_window::read_reply(&mut replies).unwrap().unwrap();
            assert!(matches!(reply, trusted_window::TrustedWindowReply::State {
                core_generation: 0, installed: Some(ref package),
            } if package.extension_id == initial.extension_id
                && package.optional[0].granted == expected_granted));
        }
        done_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        broker.active_core.lock().unwrap().take();
        drop(broker);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn malformed_mutation_reply_terminates_the_possibly_granted_core() {
        let root = std::env::temp_dir().join(format!(
            "blueice-uncertain-grant-unit-{}-{}",
            std::process::id(), synthetic_request_id(),
        ));
        std::fs::create_dir(&root).unwrap();
        let (parent_input, mut child_input) = UnixStream::pair().unwrap();
        let (mut child_output, parent_output) = UnixStream::pair().unwrap();
        let (requests, pending) = mpsc::sync_channel(1);
        let worker = thread::spawn(move || {
            serve_permission_control_worker(parent_input, parent_output, pending)
        });
        let fake_core = thread::spawn(move || {
            assert_eq!(read_permission_control_request(&mut child_input).unwrap(),
                Some(PermissionControlRequest::Grant { capability: "storage".into() }));
            child_output.write_all(&[1, 0, 0, 0, b'{']).unwrap();
        });
        let (stream, _peer) = UnixStream::pair().unwrap();
        let mut core = SpawnedCore {
            child: Command::new("sleep").arg("30").spawn().unwrap(),
            script_child: Command::new("true").spawn().unwrap(),
            internal_socket_path: root.join("core.sock"),
            script_socket_path: root.join("script.sock"),
            extension_socket_path: None,
            extension_manifest: None,
            assistant: None,
            permission_control: Some(PermissionControlChannel { requests }),
            frame_dir: root.join("frames"), stream,
        };
        assert!(core.apply_optional_change(
            trusted_window::PermissionAction::Grant, "storage",
        ).is_err());
        let deadline = Instant::now() + Duration::from_secs(1);
        while core.child.try_wait().unwrap().is_none() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        assert!(core.child.try_wait().unwrap().is_some(),
            "an uncertain Grant reply cannot leave its core process alive");
        drop(core);
        worker.join().unwrap();
        fake_core.join().unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

    // ---- bounded retry (`phase-8-live-core-hotswap/PLAN.md`) ----

    fn no_pause(_: Duration) {}

    #[test]
    fn a_first_attempt_that_succeeds_is_not_retried() {
        let mut attempts = 0;
        let out = run_with_retries(
            |n| {
                attempts += 1;
                Ok::<_, AttemptFailure>(n)
            },
            no_pause,
        );
        assert_eq!(out, Ok(1));
        assert_eq!(attempts, 1);
    }

    #[test]
    fn a_transient_failure_is_retried_until_it_clears() {
        let out = run_with_retries(
            |n| {
                if n < 3 {
                    Err(AttemptFailure::Retry(format!("spawn failed on {n}")))
                } else {
                    Ok(n)
                }
            },
            no_pause,
        );
        assert_eq!(out, Ok(3));
    }

    #[test]
    fn retries_are_bounded_and_the_last_reason_is_reported() {
        let mut attempts = 0;
        let out: Result<(), String> = run_with_retries(
            |n| {
                attempts += 1;
                Err(AttemptFailure::Retry(format!("still failing on {n}")))
            },
            no_pause,
        );
        assert_eq!(attempts, MAX_CUTOVER_ATTEMPTS);
        let reason = out.unwrap_err();
        assert!(reason.contains("still failing on 3"), "{reason}");
        assert!(reason.contains("gave up after 3 attempts"), "{reason}");
    }

    #[test]
    fn a_deterministic_failure_is_never_retried() {
        let mut attempts = 0;
        let out: Result<(), String> = run_with_retries(
            |_| {
                attempts += 1;
                Err(AttemptFailure::Final("v2's gatekeeper blocked it".into()))
            },
            no_pause,
        );
        assert_eq!(attempts, 1);
        assert!(out.unwrap_err().contains("not retryable"));
    }

    #[test]
    fn a_failed_health_check_is_retried_once_and_a_second_is_final() {
        let mut attempts = 0;
        let out: Result<(), String> = run_with_retries(
            |_| {
                attempts += 1;
                Err(AttemptFailure::RetryOnce("post-replay ListTabs differed".into()))
            },
            no_pause,
        );
        assert_eq!(attempts, 2, "one retry, then stop");
        assert!(out.unwrap_err().contains("failed the health check twice"));
        // A health failure followed by a transient one still leaves a budget.
        let mut seen = Vec::new();
        let _: Result<(), String> = run_with_retries(
            |n| {
                seen.push(n);
                if n == 1 {
                    Err(AttemptFailure::RetryOnce("health".into()))
                } else {
                    Err(AttemptFailure::Retry("spawn".into()))
                }
            },
            no_pause,
        );
        assert_eq!(seen, [1, 2, 3]);
    }

    #[test]
    fn the_pause_grows_between_attempts_and_there_is_none_after_the_last() {
        let mut pauses = Vec::new();
        let _: Result<(), String> = run_with_retries(
            |_| Err(AttemptFailure::Retry("x".into())),
            |d| pauses.push(d),
        );
        assert_eq!(
            pauses,
            [CUTOVER_RETRY_BASE_PAUSE, CUTOVER_RETRY_BASE_PAUSE * 2]
        );
    }

    #[test]
    fn every_replay_and_health_message_this_file_produces_is_classified_as_intended() {
        for (reason, expected) in [
            ("v2 rejected the replayed Navigate: bad url", "final"),
            ("v2 rejected the replayed OpenTab: bad url", "final"),
            ("v2's gatekeeper blocked the replayed Navigate: rule", "final"),
            ("v2's gatekeeper blocked the replayed OpenTab: rule", "final"),
            ("v2's post-replay ListTabs didn't match what was captured from v1", "once"),
            ("v2's post-replay representation of tab 0 is structurally different from v1's: v1 showed 9 node(s), v2 shows 0", "once"),
            ("failed to ask v2 to describe replayed tab 0: broken pipe", "retry"),
            ("failed reading v2's description of replayed tab 0: timed out", "retry"),
            ("v2 could not describe replayed tab 0: no such tab", "retry"),
            ("failed reading v2's reply while replaying: timed out", "retry"),
            ("failed reading v2's health-check reply: eof", "retry"),
            ("failed to send v2's health-check ListTabs: broken pipe", "retry"),
            ("failed to replay the default tab's Navigate into v2: broken pipe", "retry"),
            ("failed to replay tab 1's OpenTab into v2: broken pipe", "retry"),
        ] {
            let got = match classify_attempt_error(reason.to_string()) {
                AttemptFailure::Final(_) => "final",
                AttemptFailure::RetryOnce(_) => "once",
                AttemptFailure::Retry(_) => "retry",
            };
            assert_eq!(got, expected, "{reason}");
        }
    }

    #[test]
    fn waiting_for_a_socket_gives_up_at_once_when_the_child_exits() {
        let mut child = Command::new("true").spawn().unwrap();
        let started = Instant::now();
        let path = std::env::temp_dir().join(format!("never-{}.sock", std::process::id()));
        assert!(!wait_for_socket_or_exit(&path, &mut child, Duration::from_secs(20)));
        assert!(started.elapsed() < Duration::from_secs(5));
        let _ = child.wait();
    }

    // ---- structural per-tab health diff ----

    #[test]
    fn a_render_is_comparable_within_half_to_double_and_never_blank_when_v1_was_not() {
        for (v1, v2, ok) in [
            (10, 10, true),
            (10, 5, true),   // exactly half
            (10, 4, false),  // less than half
            (10, 20, true),  // exactly double
            (10, 21, false), // more than double
            (10, 0, false),  // blank where v1 had content
            (1, 1, true),
            (1, 2, true),
            (1, 3, false),
            (0, 0, true), // a blank tab constrains nothing
            (0, 50, true),
        ] {
            assert_eq!(structure_comparable(v1, v2), ok, "v1={v1} v2={v2}");
        }
    }

    /// A snapshot with `n` nodes, for counting.
    fn snapshot_with_nodes(n: usize) -> blueice_ipc::AiSnapshot {
        use blueice_ipc::{AiNode, Bounds, NodeState, Role};
        blueice_ipc::AiSnapshot {
            frame_source: 0,
            generation: 1,
            tab_id: 1,
            url: None,
            scroll_y: 0.0,
            nodes: (0..n as u64)
                .map(|id| AiNode {
                    id,
                    parent: None,
                    children: vec![],
                    role: Role::Paragraph,
                    name: None,
                    name_from: None,
                    original_name: None,
                    state: NodeState::default(),
                    bounds: Bounds {
                        x: 0.0,
                        y: 0.0,
                        width: 1.0,
                        height: 1.0,
                    },
                    opacity: 1.0,
                    occluded: false,
                    occluded_by: None,
                    occluded_fraction: 0.0,
                })
                .collect(),
        }
    }

    fn tab(id: u64) -> TabSummary {
        TabSummary {
            id,
            url: Some(format!("about:tab{id}")),
            group_id: None,
        }
    }

    #[test]
    fn v1_node_counts_are_captured_per_tab_in_order_ignoring_other_traffic() {
        let (core_side, mut core_observed) = UnixStream::pair().unwrap();
        let core_writer = Arc::new(Mutex::new(core_side.try_clone().unwrap()));
        let clients: Arc<Mutex<Vec<Sender<TaggedServerMessage>>>> =
            Arc::new(Mutex::new(Vec::new()));
        let broadcast_clients = Arc::clone(&clients);
        let broadcaster =
            thread::spawn(move || broadcast_core_to_clients(core_side, broadcast_clients));

        let responder = thread::spawn(move || {
            // Two requests arrive, one per tab, each addressed to its own tab.
            let mut requests = Vec::new();
            for _ in 0..2 {
                let (tab_id, request_id, message) =
                    read_client_message_with_ids(&mut core_observed).unwrap();
                assert!(matches!(message, ClientMessage::GetRepresentation));
                requests.push((tab_id.unwrap(), request_id.unwrap()));
            }
            assert_eq!(requests.iter().map(|r| r.0).collect::<Vec<_>>(), [7, 9]);
            // Unrelated traffic first, then the answers out of order.
            write_server_message_with_id(
                &mut core_observed,
                Some(1),
                &ServerMessage::Navigated { url: "x".into() },
            )
            .unwrap();
            write_server_message_with_id(
                &mut core_observed,
                Some(requests[1].1),
                &ServerMessage::Representation(snapshot_with_nodes(3)),
            )
            .unwrap();
            write_server_message_with_id(
                &mut core_observed,
                Some(requests[0].1),
                &ServerMessage::Representation(snapshot_with_nodes(12)),
            )
            .unwrap();
            drop(core_observed);
        });

        let counts = capture_v1_node_counts(
            &core_writer,
            &clients,
            &[tab(7), tab(9)],
            Duration::from_secs(5),
        );
        assert_eq!(counts, [Some(12), Some(3)]);
        responder.join().unwrap();
        broadcaster.join().unwrap();
    }

    #[test]
    fn a_tab_v1_does_not_describe_in_time_is_unmeasured_not_a_failure() {
        let (core_side, mut core_observed) = UnixStream::pair().unwrap();
        let core_writer = Arc::new(Mutex::new(core_side.try_clone().unwrap()));
        let clients: Arc<Mutex<Vec<Sender<TaggedServerMessage>>>> =
            Arc::new(Mutex::new(Vec::new()));
        let broadcast_clients = Arc::clone(&clients);
        let _broadcaster =
            thread::spawn(move || broadcast_core_to_clients(core_side, broadcast_clients));
        let responder = thread::spawn(move || {
            let (_, first, _) = read_client_message_with_ids(&mut core_observed).unwrap();
            let _ = read_client_message_with_ids(&mut core_observed).unwrap(); // the second is never answered
            write_server_message_with_id(
                &mut core_observed,
                first,
                &ServerMessage::Representation(snapshot_with_nodes(5)),
            )
            .unwrap();
            core_observed // keep the connection open so the wait times out
        });
        let counts = capture_v1_node_counts(
            &core_writer,
            &clients,
            &[tab(1), tab(2)],
            Duration::from_millis(400),
        );
        assert_eq!(counts, [Some(5), None]);
        drop(responder.join().unwrap());
        // Nothing to ask for means nothing to wait for.
        assert!(
            capture_v1_node_counts(&core_writer, &clients, &[], Duration::from_secs(5)).is_empty()
        );
    }

    /// A v2 stand-in answering each `GetRepresentation` with the next canned
    /// reply, recording which tab each was addressed to.
    fn fake_v2(replies: Vec<ServerMessage>) -> (UnixStream, thread::JoinHandle<Vec<Option<u64>>>) {
        let (client, mut server) = UnixStream::pair().unwrap();
        let handle = thread::spawn(move || {
            let mut asked = Vec::new();
            for reply in replies {
                let (tab_id, request_id, message) =
                    read_client_message_with_ids(&mut server).unwrap();
                assert!(matches!(message, ClientMessage::GetRepresentation));
                asked.push(tab_id);
                write_server_message_with_id(&mut server, request_id, &reply).unwrap();
            }
            asked
        });
        (client, handle)
    }

    #[test]
    fn a_comparable_replay_passes_the_structural_check_and_each_tab_is_asked_about() {
        let (mut stream, v2) = fake_v2(vec![
            ServerMessage::Representation(snapshot_with_nodes(11)),
            ServerMessage::Representation(snapshot_with_nodes(2)),
        ]);
        structural_health_check(&mut stream, &[41, 42], &[Some(10), Some(3)]).unwrap();
        assert_eq!(v2.join().unwrap(), [Some(41), Some(42)]);
    }

    #[test]
    fn a_blank_or_far_smaller_render_fails_the_structural_check_with_a_retry_once_reason() {
        for v2_nodes in [0, 2] {
            let (mut stream, v2) = fake_v2(vec![ServerMessage::Representation(
                snapshot_with_nodes(v2_nodes),
            )]);
            let reason = structural_health_check(&mut stream, &[5], &[Some(10)]).unwrap_err();
            v2.join().unwrap();
            assert!(reason.contains("structurally different"), "{reason}");
            assert!(matches!(
                classify_attempt_error(reason),
                AttemptFailure::RetryOnce(_)
            ));
        }
    }

    #[test]
    fn unmeasured_tabs_are_skipped_and_never_asked_about() {
        // Only the middle tab has a v1 measurement, so v2 is asked exactly once.
        let (mut stream, v2) = fake_v2(vec![ServerMessage::Representation(snapshot_with_nodes(4))]);
        structural_health_check(&mut stream, &[1, 2, 3], &[None, Some(4)]).unwrap();
        assert_eq!(v2.join().unwrap(), [Some(2)]);
        // No measurements at all: nothing is asked.
        let (mut stream, _peer) = UnixStream::pair().unwrap();
        structural_health_check(&mut stream, &[1, 2], &[]).unwrap();
    }

    #[test]
    fn a_v2_that_cannot_describe_a_tab_or_stops_answering_fails_the_check() {
        let (mut stream, v2) = fake_v2(vec![ServerMessage::Error {
            message: "no such tab".into(),
        }]);
        let reason = structural_health_check(&mut stream, &[9], &[Some(3)]).unwrap_err();
        v2.join().unwrap();
        assert!(reason.contains("could not describe"), "{reason}");

        let (mut stream, peer) = UnixStream::pair().unwrap();
        drop(peer);
        assert!(structural_health_check(&mut stream, &[9], &[Some(3)]).is_err());
    }

    #[test]
    fn cutover_gate_allows_only_one_in_flight_cutover_and_releases_on_drop() {
        let gate = CutoverGate::new();
        let first = gate
            .try_acquire()
            .expect("the first cutover must acquire the gate");
        assert!(
            gate.try_acquire().is_none(),
            "a concurrent cutover must be rejected while the first owns the gate"
        );
        drop(first);
        assert!(
            gate.try_acquire().is_some(),
            "a later cutover must be able to proceed after the first returns"
        );
    }

    #[test]
    fn forward_client_to_core_relays_one_message_then_stops_on_disconnect() {
        let (client_side, mut client_observed) = UnixStream::pair().unwrap();
        let (core_side, mut core_observed) = UnixStream::pair().unwrap();
        let core = Arc::new(Mutex::new(core_side));

        write_client_message(
            &mut client_observed,
            &ClientMessage::Resize {
                width: 10,
                height: 20,
            },
        )
        .unwrap();
        drop(client_observed); // triggers a clean disconnect after the one message

        forward_client_to_core(client_side, Arc::clone(&core));

        assert_eq!(
            read_client_message(&mut core_observed).unwrap(),
            ClientMessage::Resize {
                width: 10,
                height: 20
            }
        );
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

        write_client_message_with_ids(
            &mut client_observed,
            Some(3),
            Some(42),
            &ClientMessage::Navigate {
                url: "https://example.com".to_string(),
            },
        )
        .unwrap();
        drop(client_observed);

        forward_client_to_core(client_side, Arc::clone(&core));

        assert_eq!(
            read_client_message_with_ids(&mut core_observed).unwrap(),
            (
                Some(3),
                Some(42),
                ClientMessage::Navigate {
                    url: "https://example.com".to_string()
                }
            )
        );
    }

    #[test]
    fn broadcast_core_to_clients_relays_one_message_to_every_registered_client() {
        let (core_side, mut core_observed) = UnixStream::pair().unwrap();
        let (sender1, receiver1) = mpsc::channel();
        let (sender2, receiver2) = mpsc::channel();
        let clients = Arc::new(Mutex::new(vec![sender1, sender2]));

        write_server_message(
            &mut core_observed,
            &ServerMessage::Navigated {
                url: "about:blank".to_string(),
            },
        )
        .unwrap();
        drop(core_observed); // ends the broadcaster loop after the one message

        broadcast_core_to_clients(core_side, clients);

        let expected = TaggedServerMessage {
            tab_id: None,
            request_id: None,
            message: ServerMessage::Navigated {
                url: "about:blank".to_string(),
            },
        };
        assert_eq!(receiver1.recv().unwrap(), expected);
        assert_eq!(receiver2.recv().unwrap(), expected);
    }

    #[test]
    fn broadcast_core_to_clients_preserves_tab_id_and_request_id() {
        let (core_side, mut core_observed) = UnixStream::pair().unwrap();
        let (sender, receiver) = mpsc::channel();
        let clients = Arc::new(Mutex::new(vec![sender]));

        write_server_message_with_ids(
            &mut core_observed,
            Some(3),
            Some(42),
            &ServerMessage::Navigated {
                url: "about:blank".to_string(),
            },
        )
        .unwrap();
        drop(core_observed);

        broadcast_core_to_clients(core_side, clients);

        assert_eq!(
            receiver.recv().unwrap(),
            TaggedServerMessage {
                tab_id: Some(3),
                request_id: Some(42),
                message: ServerMessage::Navigated {
                    url: "about:blank".to_string()
                }
            }
        );
    }

    #[test]
    fn broadcast_core_to_clients_drops_a_client_whose_channel_is_gone_without_affecting_the_others()
    {
        let (core_side, mut core_observed) = UnixStream::pair().unwrap();
        let (dead_sender, dead_receiver) = mpsc::channel();
        drop(dead_receiver); // stands in for that client's writer thread having already exited
        let (live_sender, live_receiver) = mpsc::channel();
        let clients = Arc::new(Mutex::new(vec![dead_sender, live_sender]));

        write_server_message(
            &mut core_observed,
            &ServerMessage::Navigated {
                url: "about:blank".to_string(),
            },
        )
        .unwrap();
        drop(core_observed);

        broadcast_core_to_clients(core_side, Arc::clone(&clients));

        assert_eq!(
            live_receiver.recv().unwrap().message,
            ServerMessage::Navigated {
                url: "about:blank".to_string()
            }
        );
        // the dead client's sender must have been pruned from the list.
        assert_eq!(clients.lock().unwrap().len(), 1);
    }

    #[test]
    fn register_client_forwards_its_messages_and_receives_broadcasts() {
        let (client_side, mut client_observed) = UnixStream::pair().unwrap();
        let (core_side, mut core_observed) = UnixStream::pair().unwrap();
        let core_writer = Arc::new(Mutex::new(core_side));
        let clients: Arc<Mutex<Vec<Sender<TaggedServerMessage>>>> =
            Arc::new(Mutex::new(Vec::new()));

        register_client(client_side, Arc::clone(&core_writer), Arc::clone(&clients)).unwrap();

        // fan-in: a message the "client" sends must reach core.
        write_client_message(&mut client_observed, &ClientMessage::GetRepresentation).unwrap();
        assert_eq!(
            read_client_message(&mut core_observed).unwrap(),
            ClientMessage::GetRepresentation
        );

        // fan-out: a message sent into the registered channel (standing
        // in for the broadcaster) must reach the client's real socket,
        // relayed by this client's own writer thread.
        let registered = clients.lock().unwrap().pop().unwrap();
        registered
            .send(TaggedServerMessage {
                tab_id: Some(1),
                request_id: Some(9),
                message: ServerMessage::Navigated {
                    url: "x".to_string(),
                },
            })
            .unwrap();
        assert_eq!(
            read_server_message_with_ids(&mut client_observed).unwrap(),
            (
                Some(1),
                Some(9),
                ServerMessage::Navigated {
                    url: "x".to_string()
                }
            )
        );
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
        assert_eq!(
            write_half.write_timeout().unwrap(),
            Some(CLIENT_WRITE_TIMEOUT)
        );
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
        let clients: Arc<Mutex<Vec<Sender<TaggedServerMessage>>>> =
            Arc::new(Mutex::new(Vec::new()));

        register_client(
            slow_client_side,
            Arc::clone(&core_writer),
            Arc::clone(&clients),
        )
        .unwrap();
        register_client(
            live_client_side,
            Arc::clone(&core_writer),
            Arc::clone(&clients),
        )
        .unwrap();

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
        assert!(
            start.elapsed() < Duration::from_secs(1),
            "fanning out must be a cheap non-blocking queue push regardless of any client's own writer-thread state"
        );

        let received = read_server_message(&mut live_client_observed).unwrap();
        assert_eq!(
            received, big_message,
            "the live client must receive its own copy promptly, not stalled behind the slow one"
        );
    }

    #[test]
    fn default_rendezvous_socket_path_is_per_user_not_system_wide() {
        let path = default_rendezvous_socket_path();
        assert_eq!(path.file_name().unwrap(), "core.sock");
        assert_eq!(
            path,
            blueice_ipc::local_socket::default_socket_dir().join("core.sock")
        );
        // must not resolve to a single fixed system-wide path regardless
        // of environment -- it has to vary by runtime dir or uid.
        assert_ne!(path, PathBuf::from("/core.sock"));
    }

    #[test]
    fn sibling_core_binary_sits_next_to_the_launcher_binary() {
        let exe = PathBuf::from("/some/target/debug/blueice-launcher");
        assert_eq!(
            sibling_core_binary(&exe),
            PathBuf::from("/some/target/debug/blueice-core")
        );
    }

    #[test]
    fn sibling_core_binary_steps_out_of_a_deps_directory_for_integration_tests() {
        let exe = PathBuf::from("/some/target/debug/deps/broker_end_to_end-abc123");
        assert_eq!(
            sibling_core_binary(&exe),
            PathBuf::from("/some/target/debug/blueice-core")
        );
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
        let path =
            std::env::temp_dir().join(format!("blueice-launcher-wait-test-{}", std::process::id()));
        let _ = std::fs::remove_file(&path);
        std::fs::write(&path, b"x").unwrap();
        assert!(wait_for_socket(&path, Duration::from_millis(50)));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn wait_for_socket_times_out_if_the_path_never_appears() {
        let path = std::env::temp_dir().join(format!(
            "blueice-launcher-wait-test-missing-{}",
            std::process::id()
        ));
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
        let clients: Arc<Mutex<Vec<Sender<TaggedServerMessage>>>> =
            Arc::new(Mutex::new(Vec::new()));

        register_client(client_side, Arc::clone(&core_writer), Arc::clone(&clients)).unwrap();

        write_client_message_with_ids(
            &mut client_observed,
            Some(3),
            Some(42),
            &ClientMessage::Navigate {
                url: "https://example.com".to_string(),
            },
        )
        .unwrap();

        let (tab_id, request_id, msg) = read_client_message_with_ids(&mut core_observed).unwrap();
        assert_eq!(
            tab_id,
            Some(3),
            "the broker must not silently drop tab_id on the forwarding path"
        );
        assert_eq!(
            request_id,
            Some(42),
            "the broker must not silently drop request_id on the forwarding path"
        );
        assert_eq!(
            msg,
            ClientMessage::Navigate {
                url: "https://example.com".to_string()
            }
        );

        write_server_message_with_ids(
            &mut core_observed,
            Some(3),
            Some(42),
            &ServerMessage::Navigated {
                url: "https://example.com".to_string(),
            },
        )
        .unwrap();
        drop(core_observed); // ends the broadcaster loop after the one message

        broadcast_core_to_clients(core_side, Arc::clone(&clients));

        let (tab_id, request_id, msg) = read_server_message_with_ids(&mut client_observed).unwrap();
        assert_eq!(
            tab_id,
            Some(3),
            "the broker must not silently drop tab_id on the reply path"
        );
        assert_eq!(
            request_id,
            Some(42),
            "the broker must not silently drop request_id on the reply path"
        );
        assert_eq!(
            msg,
            ServerMessage::Navigated {
                url: "https://example.com".to_string()
            }
        );
    }

    #[test]
    fn generation_tagged_broadcast_signals_done_when_its_generation_is_still_current() {
        let (core_side, core_observed) = UnixStream::pair().unwrap();
        drop(core_observed); // "core" is already gone
        let clients: Arc<Mutex<Vec<Sender<TaggedServerMessage>>>> =
            Arc::new(Mutex::new(Vec::new()));
        let generation = Arc::new(AtomicU64::new(0));
        let (done_tx, done_rx) = mpsc::channel();

        spawn_generation_tagged_broadcast(core_side, clients, Arc::clone(&generation), 0, done_tx);

        done_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("an unsuperseded broadcast thread's death must signal done");
    }

    #[test]
    fn generation_tagged_broadcast_stays_quiet_when_superseded() {
        let (core_side, core_observed) = UnixStream::pair().unwrap();
        drop(core_observed); // standing in for a cutover's deliberate close of v1's stream
        let clients: Arc<Mutex<Vec<Sender<TaggedServerMessage>>>> =
            Arc::new(Mutex::new(Vec::new()));
        let generation = Arc::new(AtomicU64::new(1)); // already bumped past this thread's own generation
        let (done_tx, done_rx) = mpsc::channel();

        spawn_generation_tagged_broadcast(core_side, clients, generation, 0, done_tx);

        assert!(
            done_rx.recv_timeout(Duration::from_millis(200)).is_err(),
            "a superseded broadcast thread's death must NOT signal done"
        );
    }

    #[test]
    fn superseded_generation_cannot_publish_a_late_frame() {
        let (mut core_side, mut core_observed) = UnixStream::pair().unwrap();
        let (sender, receiver) = mpsc::channel();
        let clients = Arc::new(Mutex::new(vec![sender]));
        let generation = AtomicU64::new(1);
        write_server_message_with_ids(
            &mut core_observed,
            Some(1),
            None,
            &ServerMessage::FrameReady {
                shm_path: "old-core-frame".into(),
                width: 1,
                height: 1,
                generation: 12,
            },
        )
        .unwrap();
        drop(core_observed);

        broadcast_core_to_clients_for_generation(&mut core_side, clients, Some((&generation, 0)));
        assert!(receiver.try_recv().is_err(), "a late v1 frame must not follow v2 handoff");
    }

    #[test]
    fn capture_v1_tabs_filters_for_the_matching_request_id_and_ignores_other_traffic() {
        let (core_side, mut core_observed) = UnixStream::pair().unwrap();
        let core_writer = Arc::new(Mutex::new(core_side.try_clone().unwrap()));
        let clients: Arc<Mutex<Vec<Sender<TaggedServerMessage>>>> =
            Arc::new(Mutex::new(Vec::new()));

        // `capture_v1_tabs` only ever *writes* into `core_writer` and
        // then waits on its own registered channel -- something else
        // (the real broker's own broadcast thread, in production) has
        // to actually read `core`'s replies and fan them out to
        // `clients`. Stands that in here.
        let broadcast_clients = Arc::clone(&clients);
        let broadcaster =
            thread::spawn(move || broadcast_core_to_clients(core_side, broadcast_clients));

        let responder = thread::spawn(move || {
            let (_, request_id, msg) = read_client_message_with_ids(&mut core_observed).unwrap();
            assert!(matches!(msg, ClientMessage::ListTabs));
            // Some unrelated broadcast traffic first (another client's
            // concurrent action) -- must be skipped, not mistaken for
            // this call's own reply.
            write_server_message_with_id(
                &mut core_observed,
                Some(999_999),
                &ServerMessage::Navigated {
                    url: "https://unrelated.example".to_string(),
                },
            )
            .unwrap();
            write_server_message_with_id(
                &mut core_observed,
                request_id,
                &ServerMessage::Tabs(vec![TabSummary {
                    id: 1,
                    url: Some("about:blank".to_string()),
                    group_id: None,
                }]),
            )
            .unwrap();
            drop(core_observed); // ends the broadcaster loop
        });

        let tabs = capture_v1_tabs(&core_writer, &clients, Duration::from_secs(5)).unwrap();
        assert_eq!(
            tabs,
            vec![TabSummary {
                id: 1,
                url: Some("about:blank".to_string()),
                group_id: None,
            }]
        );
        responder.join().unwrap();
        broadcaster.join().unwrap();
    }

    #[test]
    fn capture_v1_tabs_fails_if_no_reply_arrives_within_the_timeout() {
        let (core_side, _core_observed) = UnixStream::pair().unwrap(); // nobody replies
        let core_writer = Arc::new(Mutex::new(core_side));
        let clients: Arc<Mutex<Vec<Sender<TaggedServerMessage>>>> =
            Arc::new(Mutex::new(Vec::new()));

        assert!(capture_v1_tabs(&core_writer, &clients, Duration::from_millis(100)).is_err());
    }

    #[test]
    fn capture_v1_tabs_fails_if_the_broadcast_connection_ends_before_a_reply_arrives() {
        let (core_side, core_observed) = UnixStream::pair().unwrap();
        drop(core_observed); // "core" is already gone before ever replying
        let core_writer = Arc::new(Mutex::new(core_side));
        let clients: Arc<Mutex<Vec<Sender<TaggedServerMessage>>>> =
            Arc::new(Mutex::new(Vec::new()));

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
        let clients: Arc<Mutex<Vec<Sender<TaggedServerMessage>>>> =
            Arc::new(Mutex::new(Vec::new()));

        let broadcast_clients = Arc::clone(&clients);
        let broadcaster =
            thread::spawn(move || broadcast_core_to_clients(core_side, broadcast_clients));

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
            write_server_message_with_id(
                &mut core_observed,
                request_id,
                &ServerMessage::Tabs(vec![]),
            )
            .unwrap();
            let _ = capture_returned_rx.recv();
            // This second broadcast message, sent only once the caller
            // below has confirmed `capture_v1_tabs` already returned, is
            // what the stale, still-registered `Sender` fails to
            // deliver, triggering its self-prune.
            write_server_message(
                &mut core_observed,
                &ServerMessage::Navigated {
                    url: "x".to_string(),
                },
            )
            .unwrap();
            drop(core_observed); // ends the broadcaster loop
        });

        capture_v1_tabs(&core_writer, &clients, Duration::from_secs(5)).unwrap();
        assert_eq!(
            clients.lock().unwrap().len(),
            1,
            "the synthetic client is still registered right after capture returns"
        );
        let _ = capture_returned_tx.send(());

        // Wait for the broadcaster to process the second message (and
        // then end, once `core_observed` is dropped) before checking
        // that the stale sender was pruned.
        broadcaster.join().unwrap();
        assert_eq!(
            clients.lock().unwrap().len(),
            0,
            "the stale synthetic sender must self-prune once its receiver has been dropped"
        );
        responder.join().unwrap();
    }

    #[test]
    fn replay_tabs_navigates_the_first_tab_and_opens_the_rest() {
        let (mut stream, mut server) = UnixStream::pair().unwrap();
        let tabs = vec![
            TabSummary {
                id: 1,
                url: Some("about:blank".to_string()),
                group_id: None,
            },
            TabSummary {
                id: 2,
                url: Some("about:credits".to_string()),
                group_id: None,
            },
            TabSummary {
                id: 3,
                url: None,
                group_id: None,
            },
        ];
        let responder = thread::spawn(move || {
            let (_, req, msg) = read_client_message_with_ids(&mut server).unwrap();
            assert_eq!(
                msg,
                ClientMessage::Navigate {
                    url: "about:blank".to_string()
                }
            );
            write_server_message_with_id(
                &mut server,
                req,
                &ServerMessage::Navigated {
                    url: "about:blank".to_string(),
                },
            )
            .unwrap();
            write_server_message_with_id(
                &mut server,
                req,
                &ServerMessage::FrameReady {
                    shm_path: "x".into(),
                    width: 1,
                    height: 1,
                    generation: 1,
                },
            )
            .unwrap();

            let (_, req, msg) = read_client_message_with_ids(&mut server).unwrap();
            assert_eq!(
                msg,
                ClientMessage::OpenTab {
                    url: Some("about:credits".to_string())
                }
            );
            write_server_message_with_id(
                &mut server,
                req,
                &ServerMessage::TabOpened {
                    tab_id: 2,
                    url: Some("about:credits".to_string()),
                },
            )
            .unwrap();
            write_server_message_with_id(
                &mut server,
                req,
                &ServerMessage::FrameReady {
                    shm_path: "y".into(),
                    width: 1,
                    height: 1,
                    generation: 2,
                },
            )
            .unwrap();

            let (_, req, msg) = read_client_message_with_ids(&mut server).unwrap();
            assert_eq!(msg, ClientMessage::OpenTab { url: None });
            write_server_message_with_id(
                &mut server,
                req,
                &ServerMessage::TabOpened {
                    tab_id: 3,
                    url: None,
                },
            )
            .unwrap();
            // no FrameReady expected, since url was None
        });

        let frames = replay_tabs(&mut stream, &tabs).unwrap();
        assert_eq!(frames.len(), 2, "only navigated tabs have replay frames");
        assert!(frames.iter().all(|frame| frame.request_id.is_none()));
        assert!(matches!(frames[0].message, ServerMessage::FrameReady { .. }));
        assert!(matches!(frames[1].message, ServerMessage::FrameReady { .. }));
        responder.join().unwrap();
    }

    #[test]
    fn replay_tabs_skips_the_first_tab_entirely_if_it_has_no_url() {
        let (mut stream, server) = UnixStream::pair().unwrap();
        let tabs = vec![TabSummary {
            id: 1,
            url: None,
            group_id: None,
        }];

        assert!(replay_tabs(&mut stream, &tabs).unwrap().is_empty());

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
        let tabs = vec![TabSummary {
            id: 1,
            url: Some("http://bad".to_string()),
            group_id: None,
        }];
        let responder = thread::spawn(move || {
            let (_, req, _msg) = read_client_message_with_ids(&mut server).unwrap();
            write_server_message_with_id(
                &mut server,
                req,
                &ServerMessage::Error {
                    message: "boom".to_string(),
                },
            )
            .unwrap();
        });

        let err = replay_tabs(&mut stream, &tabs).unwrap_err();
        assert!(err.contains("boom"));
        responder.join().unwrap();
    }

    #[test]
    fn replay_tabs_aborts_on_a_gatekeeper_blocked_reply_for_a_later_tab() {
        let (mut stream, mut server) = UnixStream::pair().unwrap();
        let tabs = vec![
            TabSummary {
                id: 1,
                url: None,
                group_id: None,
            },
            TabSummary {
                id: 2,
                url: Some("http://bad".to_string()),
                group_id: None,
            },
        ];
        let responder = thread::spawn(move || {
            let (_, req, msg) = read_client_message_with_ids(&mut server).unwrap();
            assert!(matches!(msg, ClientMessage::OpenTab { .. }));
            write_server_message_with_id(
                &mut server,
                req,
                &ServerMessage::GatekeeperBlocked {
                    reason: "nope".to_string(),
                    category: "test".to_string(),
                    url: "http://bad".to_string(),
                },
            )
            .unwrap();
        });

        let err = replay_tabs(&mut stream, &tabs).unwrap_err();
        assert!(err.contains("nope"));
        responder.join().unwrap();
    }

    #[test]
    fn replay_tabs_aborts_on_a_gatekeeper_blocked_reply_for_the_first_default_tab() {
        let (mut stream, mut server) = UnixStream::pair().unwrap();
        let tabs = vec![TabSummary {
            id: 1,
            url: Some("http://bad".to_string()),
            group_id: None,
        }];
        let responder = thread::spawn(move || {
            let (_, req, msg) = read_client_message_with_ids(&mut server).unwrap();
            assert!(matches!(msg, ClientMessage::Navigate { .. }));
            write_server_message_with_id(
                &mut server,
                req,
                &ServerMessage::GatekeeperBlocked {
                    reason: "blocked".to_string(),
                    category: "test".to_string(),
                    url: "http://bad".to_string(),
                },
            )
            .unwrap();
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
        let tabs = vec![TabSummary {
            id: 1,
            url: Some("about:blank".to_string()),
            group_id: None,
        }];
        let responder = thread::spawn(move || {
            let (_, req, msg) = read_client_message_with_ids(&mut server).unwrap();
            assert!(matches!(msg, ClientMessage::Navigate { .. }));
            write_server_message_with_id(
                &mut server,
                Some(123_456),
                &ServerMessage::Error {
                    message: "belongs to someone else".to_string(),
                },
            )
            .unwrap();
            write_server_message_with_id(
                &mut server,
                req,
                &ServerMessage::Navigated {
                    url: "about:blank".to_string(),
                },
            )
            .unwrap();
            write_server_message_with_id(
                &mut server,
                req,
                &ServerMessage::FrameReady {
                    shm_path: "x".into(),
                    width: 1,
                    height: 1,
                    generation: 1,
                },
            )
            .unwrap();
        });

        replay_tabs(&mut stream, &tabs).unwrap();
        responder.join().unwrap();
    }

    #[test]
    fn health_check_ignores_a_reply_carrying_a_mismatched_request_id_and_other_traffic() {
        let (mut stream, mut server) = UnixStream::pair().unwrap();
        let expected = vec![TabSummary {
            id: 1,
            url: Some("about:blank".to_string()),
            group_id: None,
        }];
        let responder = thread::spawn(move || {
            let (_, req, msg) = read_client_message_with_ids(&mut server).unwrap();
            assert!(matches!(msg, ClientMessage::ListTabs));
            write_server_message_with_id(
                &mut server,
                Some(999),
                &ServerMessage::Navigated {
                    url: "unrelated".to_string(),
                },
            )
            .unwrap();
            write_server_message_with_id(
                &mut server,
                req,
                &ServerMessage::FrameReady {
                    shm_path: "z".into(),
                    width: 1,
                    height: 1,
                    generation: 1,
                },
            )
            .unwrap();
            write_server_message_with_id(
                &mut server,
                req,
                &ServerMessage::Tabs(vec![TabSummary {
                    id: 99,
                    url: Some("about:blank".to_string()),
                    group_id: None,
                }]),
            )
            .unwrap();
        });

        health_check(&mut stream, &expected).unwrap();
        responder.join().unwrap();
    }

    #[test]
    fn health_check_succeeds_when_urls_match() {
        let (mut stream, mut server) = UnixStream::pair().unwrap();
        let expected = vec![TabSummary {
            id: 1,
            url: Some("about:blank".to_string()),
            group_id: None,
        }];
        let responder = thread::spawn(move || {
            let (_, req, msg) = read_client_message_with_ids(&mut server).unwrap();
            assert!(matches!(msg, ClientMessage::ListTabs));
            // v2's own ids differ from v1's -- only urls must match.
            write_server_message_with_id(
                &mut server,
                req,
                &ServerMessage::Tabs(vec![TabSummary {
                    id: 99,
                    url: Some("about:blank".to_string()),
                    group_id: None,
                }]),
            )
            .unwrap();
        });

        health_check(&mut stream, &expected).unwrap();
        responder.join().unwrap();
    }

    #[test]
    fn health_check_fails_when_urls_dont_match() {
        let (mut stream, mut server) = UnixStream::pair().unwrap();
        let expected = vec![TabSummary {
            id: 1,
            url: Some("about:blank".to_string()),
            group_id: None,
        }];
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
        assert_ne!(
            a, b,
            "a repeated cutover must not reuse the same v2 frame_dir"
        );
    }
}
