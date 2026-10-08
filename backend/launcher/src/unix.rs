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
use super::{assistant_proposals, assistant_settings_service, trusted_window, update_watch};

pub use super::control::default_control_socket_path;

use blueice_ipc::compiler_catalog::{write_compiler_catalog, CompilerCatalogBootstrap};
use blueice_ipc::owner_bootstrap::{
    write_core_owner_bootstrap, CoreOwnerBootstrap, OwnerHttpPolicyBootstrap,
    CORE_OWNER_BOOTSTRAP_VERSION,
};
use blueice_ipc::permission_control::{
    read_permission_control_reply, write_permission_control_request, PermissionControlReply,
    PermissionControlRequest,
};
use blueice_ipc::{
    read_client_message_with_ids, read_server_message_with_id, read_server_message_with_ids,
    write_client_message_with_id, write_client_message_with_ids, write_server_message_with_ids,
    ClientMessage, ServerMessage, TabSummary,
};
use std::io::{self, Read, Write};
use std::net::Shutdown;
use std::os::unix::fs::FileTypeExt;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, SyncSender, TrySendError};
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
    /// An installed package to run through core's authenticated extension
    /// host. Each core generation gets a fresh private socket and credential.
    extension_manifest: Option<PathBuf>,
    /// The assistant the launcher supervises, reproduced on every core.
    assistant: Option<AssistantWiring>,
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
    core_dom_file_bindings: bool,
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
pub(crate) struct Broker {
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
    /// Owner of the assistant's settings and pending proposals, when the
    /// launcher supervises an assistant.
    assistant_settings: Option<Arc<assistant_settings_service::AssistantSettingsService>>,
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
        rendezvous_listener,
        control_listener,
        core,
        width,
        height,
        gatekeeper_socket,
        None,
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
            assistant_settings: None,
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
    /// The assistant's settings owner, when one is supervised. Agents reach it
    /// only through the operator-control socket's propose-only requests.
    pub assistant_settings: Option<Arc<assistant_settings_service::AssistantSettingsService>>,
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
        assistant_settings,
    } = options;
    let frame_dir = core.frame_dir.clone();
    let extension_manifest = core.options.extension_manifest.clone();
    let assistant = core.options.assistant.clone();
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
        assistant_settings,
        core_options,
        route_gate,
        compiler_mcp_relay,
        debugger_relay,
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
    if let Some(relay) = &broker.compiler_mcp_relay {
        relay.close();
    }
    if let Some(relay) = &broker.debugger_relay {
        relay.close();
    }

    drop(trusted_window);

    Ok(())
}

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
                options.core_dom_file_bindings,
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
    internal_socket_path: PathBuf,
    extension_socket_path: Option<PathBuf>,
    permission_control: Option<PermissionControlChannel>,
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

pub use processes::{SpawnedGatekeeper, SpawnedTrustedWindow};
#[path = "unix/cutover.rs"]
mod cutover_pipeline;
pub(crate) use cutover_pipeline::cutover;
use cutover_pipeline::*;
#[path = "unix/trusted_window.rs"]
mod window_session;
use window_session::*;
mod processes;
use processes::*;
mod permission_worker;
use permission_worker::*;
