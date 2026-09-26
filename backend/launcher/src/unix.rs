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

use super::bluejs_host::{BlueJsHostCoreConfig, BlueJsHostRuntimeLimits, SpawnedBlueJsHost};
use super::control;

pub use super::control::default_control_socket_path;

use blueice_ipc::compiler_catalog::{write_compiler_catalog, CompilerCatalogBootstrap};
use blueice_ipc::owner_bootstrap::{
    write_core_owner_bootstrap, CoreOwnerBootstrap, OwnerHttpPolicyBootstrap,
    CORE_OWNER_BOOTSTRAP_VERSION,
};
use blueice_ipc::{
    read_client_message_with_ids, read_server_message_with_id, read_server_message_with_ids,
    write_client_message_with_id, write_client_message_with_ids, write_server_message_with_ids,
    ClientMessage, ServerMessage, TabSummary,
};
use std::io;
use std::net::Shutdown;
use std::os::unix::fs::FileTypeExt;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// Launcher-owned configuration for one `blueice-core` child.
///
/// This deliberately contains operational policy only. In particular, it
/// cannot carry a page-host endpoint or capability token: when
/// [`Self::supervise_out_of_process_bluejs`] is selected, the launcher
/// creates a fresh private pair while it starts the core and retains the
/// matching child supervisor for that core's lifetime.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CoreLaunchOptions {
    gatekeeper_socket: Option<PathBuf>,
    supervise_out_of_process_bluejs: bool,
    /// Immutable launcher-owner resource envelope forwarded only through
    /// trusted child bootstrap. The public launcher CLI, core, page, and
    /// page-host IPC cannot inspect or change it.
    bluejs_host_runtime_limits: BlueJsHostRuntimeLimits,
    /// Requests the one fixed, compiled-in HTTP page-script profile from
    /// the core while retaining the normal launcher-owned child setup.
    /// This is deliberately a boolean fixture selector rather than a
    /// resource-policy carrier: callers cannot supply URLs, manifests,
    /// resolvers, paths, sources, or fetch settings.
    core_http_page_script_fixture: bool,
    /// Separate owner-only proof profile for the child-to-core DOM
    /// lookup route. It exposes only a boolean JavaScript test callback,
    /// not node handles or the general DOM wrapper API.
    core_dom_lookup_probe_fixture: bool,
    /// Owner-selected first live DOM text profile; page and frontend
    /// traffic cannot select it or supply its private script socket.
    core_dom_text_fixture: bool,
    /// Separate owner-selected live DOM creation/append profile. It does
    /// not mutate the immutable text-v1 typing or callback inventory.
    core_dom_mutation_fixture: bool,
    /// Separate owner-selected click-event profile. It includes bounded
    /// live DOM mutation and exact VM-owned listener registration.
    core_dom_event_fixture: bool,
    /// A caller-selected Unix endpoint for a sealed core compiler catalog.
    /// The default builder selects the fixed compiled-in fixture; the
    /// separate trusted-owner builder may supply a complete closed graph.
    compiler_mcp_socket: Option<PathBuf>,
    /// Owner-supplied, bounded, closed graph for one core generation.
    /// Source text goes only to the new core's inherited stdin pipe.
    compiler_catalog: Option<CompilerCatalogBootstrap>,
    /// Owner-selected canonical URL/integrity policy for HTTP(S) page
    /// scripts. It crosses only the one-shot core startup pipe; the page
    /// and isolated child receive closed graphs, never this manifest.
    page_http_policy: Option<OwnerHttpPolicyBootstrap>,
    /// A caller-selected Unix endpoint for the core's bounded debugger
    /// protocol.  The endpoint is only a transport location: debugger
    /// protocol versioning and every target-bound operation remain
    /// enforced by the trusted core.
    debugger_socket: Option<PathBuf>,
    /// Owner-selected value-read policy. A debugger client must still
    /// negotiate its separate v37 grant and observe the active slot.
    debugger_bounded_values: bool,
    /// Owner-only policy for source-free opaque-handle inventory after a
    /// client requests it in `Hello`; it never grants a metadata record
    /// read.
    debugger_static_metadata_inventory: bool,
    /// Owner-only policy for the dependent bounded static-metadata
    /// summary. It requires the inventory policy and never grants source
    /// identity/text, spans, names, type displays, symbols, contracts,
    /// bytecode, or runtime values.
    debugger_static_metadata_summary: bool,
    /// Owner-only policy for a metadata-handle-bound compiler-minted
    /// source-record ID inventory. It requires the parent inventory and
    /// does not expose source identity, hash, text, or record detail.
    debugger_static_metadata_source_inventory: bool,
    /// Owner-only policy for source-free compiler provenance of one
    /// prior source ID. It requires source inventory and exposes only a
    /// canonical module identity plus labeled SHA-256 digest.
    debugger_static_metadata_source_provenance: bool,
    /// Owner-only policy for compiler-minted type-record IDs bound to an
    /// opaque metadata handle. Type displays remain default-denied.
    debugger_static_metadata_type_inventory: bool,
    /// Owner-only policy for one bounded compiler-produced type display
    /// under a previously inventoried type ID. Displays can contain
    /// project-authored names and therefore remain independently denied.
    debugger_static_metadata_type_display: bool,
    /// Owner-only policy for compiler-minted symbol-record IDs bound to
    /// an opaque metadata handle. Symbol detail remains default-denied.
    debugger_static_metadata_symbol_inventory: bool,
    /// Owner-only policy for compiler-minted contract IDs bound to an
    /// opaque metadata handle. Contract detail remains default-denied.
    debugger_static_metadata_contract_inventory: bool,
    /// Owner-only policy for compiler-produced contract displays bound to
    /// a prior opaque contract receipt. Plans and validation stay denied.
    debugger_static_metadata_contract_display: bool,
    /// Owner-only policy for a data-only validation against a prior opaque
    /// contract receipt. The public result is only a boolean; plan and
    /// structural failure data remain denied.
    debugger_static_metadata_contract_validation: bool,
    /// Owner-only policy for aggregate evidence about a verified direct
    /// BlueTS-to-BlueJS lowering map. Map entries and bytecode stay denied.
    debugger_static_metadata_lowering_summary: bool,
    /// Owner-only policy for compiler-produced symbol displays bound to a
    /// prior opaque symbol receipt. Source/static records remain denied.
    debugger_static_metadata_symbol_display: bool,
    /// Owner-only policy for one source-text-free byte range bound to
    /// separate opaque symbol and source receipts. Module identity,
    /// source text, line/column, names, types, contracts, and bytecode
    /// remain default-denied.
    debugger_static_metadata_symbol_location: bool,
    /// Owner-only policy for an exact BlueTS safe-point byte span under
    /// separate opaque metadata and source-ID receipts. No source text,
    /// module identity, or nearest-position map is granted.
    debugger_static_metadata_safe_point_span: bool,
    /// Separate owner-only policy for bounded original BlueTS source
    /// positions under exact metadata/source receipts.
    debugger_static_metadata_source_breakpoint: bool,
    /// Independent owner grant for paused BlueTS source-span stepping.
    debugger_static_metadata_source_span_step: bool,
    /// Owner-only policy for a bounded contract declaration range under
    /// separate contract and source receipts; no plan or source text.
    debugger_static_metadata_contract_location: bool,
    /// Owner-only policy for a compiler-verified symbol/type relation
    /// under two separately inventoried opaque IDs. Displays and static
    /// records remain independently denied.
    debugger_static_metadata_symbol_type: bool,
    /// Owner-only policy for a compiler-verified symbol/contract relation
    /// under two separately inventoried opaque IDs. Plans and validation
    /// remain independently denied.
    debugger_static_metadata_symbol_contract: bool,
    /// Independent owner policy for compiler-only paused-slot relations.
    debugger_static_scope_relation: bool,
}

mod launch_options;

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
        .and_then(|status| {
            status
                .lines()
                .find_map(|line| line.strip_prefix("Uid:"))
                .and_then(|rest| rest.split_whitespace().next())
                .and_then(|s| s.parse().ok())
        })
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
pub fn broadcast_core_to_clients(
    mut core: UnixStream,
    clients: Arc<Mutex<Vec<Sender<TaggedServerMessage>>>>,
) {
    loop {
        let (tab_id, request_id, message) = match read_server_message_with_ids(&mut core) {
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
    /// A cutover spans tab capture, v2 startup/replay, health checking and
    /// the swap. Control connections are served on independent threads, so
    /// this lock makes that whole transaction serial: a second request sees
    /// the newly active generation rather than racing for the same v2 frame
    /// directory or generation number.
    cutover_lock: Mutex<()>,
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
    /// The launcher-owned startup policy that must be reproduced for a
    /// replacement core. An out-of-process host is deliberately fresh
    /// per core generation, so no private child capability crosses a
    /// cutover boundary.
    core_options: CoreLaunchOptions,
    /// The shared acceptance/handoff gate for every stable auxiliary
    /// endpoint.  It makes browser, compiler, and debugger clients see
    /// one generation boundary even when both protocol relays are
    /// selected.
    route_gate: Arc<Mutex<()>>,
    /// The optional public launcher-owned compiler endpoint.
    compiler_mcp_relay: Option<Arc<GenerationPinnedUnixRelay>>,
    /// The optional public launcher-owned debugger endpoint.
    debugger_relay: Option<Arc<GenerationPinnedUnixRelay>>,
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
                )
            }
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
            write_client_message_with_id(
                v2_stream,
                Some(request_id),
                &ClientMessage::Navigate { url: url.clone() },
            )
            .map_err(|e| format!("failed to replay the default tab's Navigate into v2: {e}"))?;
            expect_navigate_success(v2_stream, request_id)?;
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
        let (reply_id, message) = read_server_message_with_id(v2_stream)
            .map_err(|e| format!("failed reading v2's reply while replaying: {e}"))?;
        if matches!(reply_id, Some(id) if id != request_id) {
            continue;
        }
        match message {
            ServerMessage::Navigated { .. } => navigated = true,
            ServerMessage::FrameReady { .. } if navigated => return Ok(()),
            ServerMessage::FrameReady { .. } => continue, // shouldn't happen before Navigated, but don't misinterpret
            ServerMessage::Error { message } => {
                return Err(format!("v2 rejected the replayed Navigate: {message}"))
            }
            ServerMessage::GatekeeperBlocked { reason, .. } => {
                return Err(format!(
                    "v2's gatekeeper blocked the replayed Navigate: {reason}"
                ))
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
) -> Result<(), String> {
    let mut opened = false;
    loop {
        let (reply_id, message) = read_server_message_with_id(v2_stream)
            .map_err(|e| format!("failed reading v2's reply while replaying: {e}"))?;
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
            ServerMessage::Error { message } => {
                return Err(format!("v2 rejected the replayed OpenTab: {message}"))
            }
            ServerMessage::GatekeeperBlocked { reason, .. } => {
                return Err(format!(
                    "v2's gatekeeper blocked the replayed OpenTab: {reason}"
                ))
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
fn health_check(v2_stream: &mut UnixStream, captured_tabs: &[TabSummary]) -> Result<(), String> {
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
    let stem = v1_frame_dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "frames".to_string());
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
    // Every relay accept snapshots its target while holding this gate.
    // Hold it across the browser-writer swap and both relay activations,
    // so no newly accepted compiler or debugger connection can observe a
    // different core generation from browser traffic.
    let _route_handoff = broker
        .route_gate
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    broker.generation.store(target_generation, Ordering::SeqCst);

    let v2_broadcast_stream = v2
        .stream
        .try_clone()
        .expect("try_clone on a fresh stream should not fail");
    spawn_generation_tagged_broadcast(
        v2_broadcast_stream,
        Arc::clone(&broker.clients),
        Arc::clone(&broker.generation),
        target_generation,
        broker.done.clone(),
    );

    let v2_writer_stream = v2
        .stream
        .try_clone()
        .expect("try_clone on a fresh stream should not fail");
    *broker
        .core_writer
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = v2_writer_stream;

    // The replacement private listeners are ready before replay/health
    // checking. Under the shared handoff gate, make them targets only for
    // future accepts after browser traffic has moved to the same core.
    v2.activate_relays_after_handoff();

    let mut active = broker
        .active_core
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
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
    let _cutover = broker
        .cutover_lock
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let captured_tabs =
        match capture_v1_tabs(&broker.core_writer, &broker.clients, TAB_CAPTURE_TIMEOUT) {
            Ok(tabs) => tabs,
            Err(reason) => return control::ControlReply::CutoverFailed { reason },
        };

    let target_generation = broker.generation.load(Ordering::SeqCst) + 1;
    let frame_dir = v2_frame_dir(&broker.frame_dir, target_generation);
    let mut v2 = match SpawnedCore::spawn_with_options_and_relays(
        broker.width,
        broker.height,
        &frame_dir,
        broker.core_options.clone(),
        RelaySet {
            route_gate: Arc::clone(&broker.route_gate),
            compiler_mcp_relay: broker.compiler_mcp_relay.clone(),
            debugger_relay: broker.debugger_relay.clone(),
        },
    ) {
        Ok(v2) => v2,
        Err(e) => {
            return control::ControlReply::CutoverFailed {
                reason: format!("failed to spawn v2: {e}"),
            }
        }
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
pub fn run_broker(
    rendezvous_listener: UnixListener,
    control_listener: UnixListener,
    core: SpawnedCore,
    width: f64,
    height: f64,
) -> io::Result<()> {
    let frame_dir = core.frame_dir.clone();
    let core_options = core.options.clone();
    let route_gate = core.route_gate.clone();
    let compiler_mcp_relay = core.compiler_mcp_relay.clone();
    let debugger_relay = core.debugger_relay.clone();
    let core_writer = Arc::new(Mutex::new(core.stream.try_clone()?));
    let broadcast_stream = core.stream.try_clone()?;
    let clients: Arc<Mutex<Vec<Sender<TaggedServerMessage>>>> = Arc::new(Mutex::new(Vec::new()));
    let generation = Arc::new(AtomicU64::new(0));
    let (done_tx, done_rx) = mpsc::channel();

    let broker = Arc::new(Broker {
        cutover_lock: Mutex::new(()),
        core_writer: Arc::clone(&core_writer),
        clients: Arc::clone(&clients),
        generation: Arc::clone(&generation),
        active_core: Mutex::new(Some(core)),
        width,
        height,
        frame_dir,
        core_options,
        route_gate,
        compiler_mcp_relay,
        debugger_relay,
        done: done_tx.clone(),
    });

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
    if let Some(relay) = &broker.compiler_mcp_relay {
        relay.close();
    }
    if let Some(relay) = &broker.debugger_relay {
        relay.close();
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

/// A generation-private compiler socket.  Unlike the public endpoint
/// selected by the caller, this name is launcher-generated and never
/// exposed as a CLI/MCP input.  v1 and a staged v2 therefore can each
/// bind their own listener while the public relay remains stable.
fn unique_internal_compiler_socket_path() -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "blueice-launcher-compiler-{}-{n}.sock",
        std::process::id()
    ))
}

/// A script-DOM listener unique to one supervised core/child pair. The
/// child capability, not knowledge of this pathname, grants access.
fn unique_internal_script_socket_path() -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "blueice-launcher-script-{}-{n}.sock",
        std::process::id()
    ))
}

/// A generation-private debugger socket. Like the compiler socket, this
/// name is launcher-generated and cannot be supplied through the public
/// debugger transport.
fn unique_internal_debugger_socket_path() -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "blueice-launcher-debugger-{}-{n}.sock",
        std::process::id()
    ))
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

/// The sole compiler profile the launcher may ask its child to register.
/// This label is an implementation detail of the trusted startup edge;
/// it is not a caller-selectable profile or a compiler IPC field.
const CORE_CLOSED_COMPILER_PROJECT_PROFILE: &str = "core-closed-fixture-v1";

/// Keep selected filesystem endpoints comfortably below the smallest
/// common `sockaddr_un.sun_path` capacity.  The actual platform capacity
/// varies, so a conservative launcher-side limit rejects a bad setup
/// before a core (or an optional BlueJS host) is spawned.
const MAX_STABLE_ENDPOINT_SOCKET_PATH_BYTES: usize = 100;

/// Removes only a Unix-domain socket.  A caller-selected endpoint must
/// never let cleanup unlink an ordinary file, directory, or symlink that
/// happens to occupy the same path after a child exits.
fn remove_owned_socket_if_owned(path: &Path) {
    let Ok(metadata) = std::fs::symlink_metadata(path) else {
        return;
    };
    if metadata.file_type().is_socket() {
        let _ = std::fs::remove_file(path);
    }
}

fn remove_compiler_mcp_socket_if_owned(path: &Path) {
    remove_owned_socket_if_owned(path);
}

/// Validates the only public compiler configuration before any child is
/// created.  A stale socket may be reclaimed; a live listener, a symlink,
/// or any non-socket file is an explicit configuration error.  In
/// particular, never blindly unlink a caller's arbitrary path merely
/// because it was supplied as a compiler endpoint.
#[cfg(test)]
fn prepare_compiler_mcp_endpoint(path: &Path) -> io::Result<()> {
    prepare_stable_endpoint(path, "compiler MCP")
}

/// Validates a caller-selected stable relay endpoint before a child is
/// created. A stale socket may be reclaimed; a live listener, symlink,
/// or other filesystem object fails closed. The label is launcher-owned
/// diagnostic text, never a protocol or capability input.
fn prepare_stable_endpoint(path: &Path, label: &str) -> io::Result<()> {
    use std::os::unix::ffi::OsStrExt;

    if !path.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{label} socket path must be absolute"),
        ));
    }
    if path.as_os_str().as_bytes().len() > MAX_STABLE_ENDPOINT_SOCKET_PATH_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{label} socket path exceeds {MAX_STABLE_ENDPOINT_SOCKET_PATH_BYTES} bytes"),
        ));
    }
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{label} socket path must have a parent directory"),
        )
    })?;
    if !parent.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!(
                "{label} socket parent does not exist or is not a directory: {}",
                parent.display()
            ),
        ));
    }

    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if metadata.file_type().is_symlink() || !metadata.file_type().is_socket() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!(
                "{label} endpoint is occupied by a non-socket path: {}",
                path.display()
            ),
        ));
    }

    match UnixStream::connect(path) {
        // It is a live owner.  Do not touch it: this also makes an
        // accidental second launcher and a cutover attempt fail closed.
        Ok(_) => Err(io::Error::new(
            io::ErrorKind::AddrInUse,
            format!("{label} endpoint is already active: {}", path.display()),
        )),
        // A socket inode without a listener is recoverable state from a
        // previous crashed child.  It is safe to reclaim only after its
        // type was checked above.
        Err(error) if error.kind() == io::ErrorKind::ConnectionRefused => {
            std::fs::remove_file(path)
        }
        Err(error) => Err(io::Error::new(
            error.kind(),
            format!(
                "could not determine whether {label} endpoint is live at {}: {error}",
                path.display()
            ),
        )),
    }
}

/// A launcher-owned stable auxiliary endpoint. The listener is public
/// only to the launching Unix user (`0600`); each connection is bound
/// exactly once, at accept time, to one generation-private core listener.
///
/// This relay is deliberately protocol-agnostic. It parses no compiler
/// or debugger request and cannot manufacture either service's authority:
/// it only copies bytes between a validated owner-only public socket and
/// the launcher-selected private listener for the accepted generation.
struct GenerationPinnedUnixRelay {
    public_socket_path: PathBuf,
    /// Serializes one relay accept's target snapshot with the broker's
    /// small browser/compiler-generation handoff.
    route_gate: Arc<Mutex<()>>,
    target: Arc<Mutex<Option<PathBuf>>>,
    accepting: Arc<AtomicBool>,
    accept_thread: Mutex<Option<JoinHandle<()>>>,
}

mod relay;

/// Copies a single accepted public connection to its one private core
/// peer. If the staged/old core no longer owns that private endpoint, the
/// accepted connection is closed; it is never retried against a newer
/// generation.
fn relay_generation_pinned_connection(mut client: UnixStream, target: PathBuf) {
    let mut core = match UnixStream::connect(target) {
        Ok(core) => core,
        Err(_) => {
            let _ = client.shutdown(Shutdown::Both);
            return;
        }
    };
    let Ok(mut client_to_core) = client.try_clone() else {
        let _ = client.shutdown(Shutdown::Both);
        return;
    };
    let Ok(mut core_to_client) = core.try_clone() else {
        let _ = client.shutdown(Shutdown::Both);
        return;
    };

    let client_to_core_thread = thread::spawn(move || {
        let _ = io::copy(&mut client_to_core, &mut core);
        let _ = core.shutdown(Shutdown::Write);
    });
    let _ = io::copy(&mut core_to_client, &mut client);
    let _ = client.shutdown(Shutdown::Write);
    let _ = client_to_core_thread.join();
}

/// The launcher-owned stable endpoint set for one core generation. The
/// public listeners outlive individual generations; only these private
/// targets and the shared handoff gate travel with a staged core.
#[derive(Clone)]
struct RelaySet {
    route_gate: Arc<Mutex<()>>,
    compiler_mcp_relay: Option<Arc<GenerationPinnedUnixRelay>>,
    debugger_relay: Option<Arc<GenerationPinnedUnixRelay>>,
}

/// One inseparable launcher-issued child/core script capability pair.
/// Keeping the supervisor, page-host handshake, and script socket together
/// prevents a staged core from accidentally receiving a mismatched path.
struct PrivatePageHostLaunch {
    host: SpawnedBlueJsHost,
    config: BlueJsHostCoreConfig,
    script_socket: PathBuf,
}

impl PrivatePageHostLaunch {
    fn spawn(options: &CoreLaunchOptions) -> io::Result<Option<Self>> {
        if !options.supervise_out_of_process_bluejs {
            return Ok(None);
        }
        let script_socket = unique_internal_script_socket_path();
        let (host, config) =
            SpawnedBlueJsHost::spawn_for_core_with_script_socket_and_runtime_limits(
                &script_socket,
                options.bluejs_host_runtime_limits,
                options.core_dom_lookup_probe_fixture,
                options.core_dom_text_fixture,
                options.core_dom_mutation_fixture,
                options.core_dom_event_fixture,
            )?;
        Ok(Some(Self {
            host,
            config,
            script_socket,
        }))
    }
}

/// A `core` process this launcher spawned and owns privately: killed
/// and cleaned up (process, internal socket, and frame directory) on
/// [`Drop`], the same lifetime discipline `mcp-server`'s `CoreProcess`
/// already established for its own (today, unshared) spawned `core`.
pub struct SpawnedCore {
    child: Child,
    internal_socket_path: PathBuf,
    script_private_socket_path: Option<PathBuf>,
    /// A launcher-generated core-private compiler listener. The public
    /// endpoint is owned by [`GenerationPinnedUnixRelay`] instead, so
    /// this path can be unique for every live/staged generation.
    compiler_private_socket_path: Option<PathBuf>,
    /// A launcher-generated core-private debugger listener. The public
    /// endpoint is likewise relay-owned and never directly exposed.
    debugger_private_socket_path: Option<PathBuf>,
    frame_dir: PathBuf,
    /// Retained so a cutover can reproduce the selected launcher policy
    /// without preserving a generation-specific page-host capability.
    options: CoreLaunchOptions,
    /// The launcher owns this handle, not the core or any frontend. Its
    /// `Drop` implementation kills/reaps the isolated child after this
    /// core has been terminated, and removes the child-only socket.
    bluejs_host: Option<SpawnedBlueJsHost>,
    /// Shared by the browser broker and every relay accept, so a cutover
    /// changes all future protocol routes at one generation boundary.
    route_gate: Arc<Mutex<()>>,
    /// Shared with the broker during a cutover so v1's Drop cannot close
    /// stable public endpoints while v2 is being staged.
    compiler_mcp_relay: Option<Arc<GenerationPinnedUnixRelay>>,
    debugger_relay: Option<Arc<GenerationPinnedUnixRelay>>,
    pub stream: UnixStream,
}

mod spawned_core;

#[cfg(test)]
mod tests;
