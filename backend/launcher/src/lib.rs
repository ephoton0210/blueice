// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#[cfg(unix)]
pub mod bluejs_host;
#[cfg(unix)]
pub mod control;
#[cfg(unix)]
pub mod memory_pressure;
#[cfg(unix)]
pub mod supervisor;

#[cfg(unix)]
mod unix {
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
    use std::process::{Child, Command};
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
        /// A caller-selected Unix endpoint for the one fixed, core-owned
        /// compiler project profile.  The profile is deliberately not an API
        /// field: neither a launcher caller nor its CLI can choose a source,
        /// resolver, compiler option, or alternate profile.
        compiler_mcp_socket: Option<PathBuf>,
        /// A caller-selected Unix endpoint for the core's bounded debugger
        /// protocol.  The endpoint is only a transport location: debugger
        /// protocol versioning and every target-bound operation remain
        /// enforced by the trusted core.
        debugger_socket: Option<PathBuf>,
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
    }

    impl CoreLaunchOptions {
        /// Forwards an owner-selected gatekeeper endpoint to the trusted core.
        /// This is intentionally unrelated to the page-host capability.
        pub fn with_gatekeeper_socket(mut self, path: PathBuf) -> Self {
            self.gatekeeper_socket = Some(path);
            self
        }

        /// Selects the launcher-owned, out-of-process BlueJS page-host route.
        ///
        /// This remains disabled by default. The launcher creates the private
        /// socket and fresh token itself; callers cannot configure either
        /// value through this API.
        pub fn supervise_out_of_process_bluejs(mut self) -> Self {
            self.supervise_out_of_process_bluejs = true;
            self
        }

        /// Starts an isolated page host with the embedding owner's fixed
        /// per-realm runtime limits. They bound realm/program/root-bytecode
        /// admission and VM managed heap, but are not a child-process RSS or
        /// aggregate memory limit. The ordinary `blueice-launcher` CLI has no
        /// equivalent flags, and neither core nor page traffic can widen them.
        pub fn supervise_out_of_process_bluejs_with_runtime_limits(
            mut self,
            limits: BlueJsHostRuntimeLimits,
        ) -> Self {
            self.supervise_out_of_process_bluejs = true;
            self.bluejs_host_runtime_limits = limits;
            self
        }

        /// Starts the launcher-supervised page host with core's sole fixed
        /// HTTP page-script integration fixture.
        ///
        /// The public `blueice-launcher` CLI deliberately has no equivalent
        /// switch. This trusted embedding API selects only a compiled profile
        /// identity; its resource path, integrity manifest, origin relation,
        /// resolver policy, byte limits, and fetch behavior remain inside the
        /// core binary and cannot be caller supplied.
        pub fn supervise_out_of_process_bluejs_with_core_http_fixture(mut self) -> Self {
            self.supervise_out_of_process_bluejs = true;
            self.core_http_page_script_fixture = true;
            self
        }

        /// Opts this core generation into the fixed, closed compiler project
        /// profile and selects the Unix endpoint that its query-only compiler
        /// listener will own.
        ///
        /// The endpoint is the only caller-provided compiler value.  The
        /// launcher always passes the compiled-in
        /// `core-closed-fixture-v1` profile to `blueice-core`; this method
        /// cannot carry project paths, sources, resolver/compiler options,
        /// update/build/write authority, or a caller-selected profile.  The
        /// endpoint is validated before the launcher creates any child.
        ///
        /// The public endpoint is owned by the launcher, rather than a core
        /// generation.  It relays each accepted connection to exactly one
        /// generation-private compiler socket.  That lets a cutover stage a
        /// verified v2 listener before it changes the public route, without
        /// ever retargeting an existing compiler/MCP connection.
        pub fn with_core_closed_compiler_mcp_endpoint(mut self, path: PathBuf) -> Self {
            self.compiler_mcp_socket = Some(path);
            self
        }

        /// Selects the launcher-owned stable endpoint for the core debugger
        /// protocol.
        ///
        /// The launcher binds the supplied public path as owner-only (`0600`)
        /// and relays each accepted stream to exactly one core generation's
        /// private listener.  It does not add debugger operations, source,
        /// bytecode, runtime values, or arbitrary child-host authority.
        /// Replacement cores receive fresh private sockets; live streams are
        /// never retargeted across a cutover.
        pub fn with_debugger_endpoint(mut self, path: PathBuf) -> Self {
            self.debugger_socket = Some(path);
            self
        }

        /// Enables the bounded opaque static-metadata inventory for this
        /// launch profile. A debugger endpoint must also be selected before
        /// this policy can take effect. It never enables source, type, symbol,
        /// span, contract, bytecode, or runtime-value access.
        pub fn with_debugger_static_metadata_inventory(mut self) -> Self {
            self.debugger_static_metadata_inventory = true;
            self
        }

        /// Enables bounded source-free summaries for handles from the
        /// explicitly selected static-metadata inventory. The method also
        /// enables that prerequisite inventory, but a client must still
        /// negotiate both distinct capabilities on its debugger stream.
        pub fn with_debugger_static_metadata_summary(mut self) -> Self {
            self.debugger_static_metadata_inventory = true;
            self.debugger_static_metadata_summary = true;
            self
        }

        /// Enables only compiler-minted source-record IDs for an exact
        /// static-metadata handle. The parent inventory remains the required
        /// opaque authority; individual source/provenance detail is absent.
        pub fn with_debugger_static_metadata_source_inventory(mut self) -> Self {
            self.debugger_static_metadata_inventory = true;
            self.debugger_static_metadata_source_inventory = true;
            self
        }

        /// Enables the distinct source-provenance disclosure policy together
        /// with its required opaque parent and source-ID inventory. A debugger
        /// peer must still negotiate all canonical capabilities on its stream.
        pub fn with_debugger_static_metadata_source_provenance(mut self) -> Self {
            self.debugger_static_metadata_inventory = true;
            self.debugger_static_metadata_source_inventory = true;
            self.debugger_static_metadata_source_provenance = true;
            self
        }

        /// Enables only compiler-minted type-record IDs for one exact static
        /// metadata handle. The parent inventory remains the required opaque
        /// authority; type displays and records are not exposed.
        pub fn with_debugger_static_metadata_type_inventory(mut self) -> Self {
            self.debugger_static_metadata_inventory = true;
            self.debugger_static_metadata_type_inventory = true;
            self
        }

        /// Enables one bounded compiler-produced type display for a type ID
        /// returned by the exact debugger stream's type inventory. This also
        /// selects the necessary opaque parent and type-ID inventory policy;
        /// a client must still negotiate all three capabilities.
        pub fn with_debugger_static_metadata_type_display(mut self) -> Self {
            self.debugger_static_metadata_inventory = true;
            self.debugger_static_metadata_type_inventory = true;
            self.debugger_static_metadata_type_display = true;
            self
        }

        /// Enables compiler-minted symbol-record IDs for one exact metadata
        /// handle. This also selects the required opaque parent inventory,
        /// while symbol names, spans, types, and record reads remain denied.
        pub fn with_debugger_static_metadata_symbol_inventory(mut self) -> Self {
            self.debugger_static_metadata_inventory = true;
            self.debugger_static_metadata_symbol_inventory = true;
            self
        }

        /// Enables compiler-minted contract IDs for one exact metadata handle.
        /// This also selects the required opaque parent inventory, while
        /// contract names, spans, plans, and validation remain denied.
        pub fn with_debugger_static_metadata_contract_inventory(mut self) -> Self {
            self.debugger_static_metadata_inventory = true;
            self.debugger_static_metadata_contract_inventory = true;
            self
        }

        /// Enables a bounded compiler-produced display for an already
        /// inventoried contract ID. This also selects parent and contract
        /// inventories, while source spans, plans, validation, and records stay denied.
        pub fn with_debugger_static_metadata_contract_display(mut self) -> Self {
            self.debugger_static_metadata_inventory = true;
            self.debugger_static_metadata_contract_inventory = true;
            self.debugger_static_metadata_contract_display = true;
            self
        }

        /// Enables a bounded data-only validation for an already inventoried
        /// contract ID. This also selects parent and contract inventories, but
        /// only exposes a boolean outcome—never the plan or failure detail.
        pub fn with_debugger_static_metadata_contract_validation(mut self) -> Self {
            self.debugger_static_metadata_inventory = true;
            self.debugger_static_metadata_contract_inventory = true;
            self.debugger_static_metadata_contract_validation = true;
            self
        }

        /// Enables an aggregate verified direct-lowering-map summary for a
        /// prior opaque metadata handle. This selects only the parent
        /// inventory; source spans, map entries, and bytecode stay denied.
        pub fn with_debugger_static_metadata_lowering_summary(mut self) -> Self {
            self.debugger_static_metadata_inventory = true;
            self.debugger_static_metadata_lowering_summary = true;
            self
        }

        /// Enables a bounded compiler-produced display for an already
        /// inventoried symbol ID. This also selects parent and symbol
        /// inventories, while source spans, types, contracts, and records stay denied.
        pub fn with_debugger_static_metadata_symbol_display(mut self) -> Self {
            self.debugger_static_metadata_inventory = true;
            self.debugger_static_metadata_symbol_inventory = true;
            self.debugger_static_metadata_symbol_display = true;
            self
        }

        /// Enables one bounded half-open UTF-8 byte range for an exact
        /// separately inventoried symbol/source pair. This selects parent,
        /// source-ID, and symbol-ID inventories; it never enables source
        /// text, module identity, line/column mappings, metadata records,
        /// names, types, contracts, bytecode, or runtime values.
        pub fn with_debugger_static_metadata_symbol_location(mut self) -> Self {
            self.debugger_static_metadata_inventory = true;
            self.debugger_static_metadata_source_inventory = true;
            self.debugger_static_metadata_symbol_inventory = true;
            self.debugger_static_metadata_symbol_location = true;
            self
        }

        /// Enables only exact original BlueTS safe-point spans and their
        /// required opaque parent/source inventories. The debugger stream
        /// must negotiate this distinct grant and receive the source ID.
        pub fn with_debugger_static_metadata_safe_point_span(mut self) -> Self {
            self.debugger_static_metadata_inventory = true;
            self.debugger_static_metadata_source_inventory = true;
            self.debugger_static_metadata_safe_point_span = true;
            self
        }

        /// Enables one bounded contract/source location under the existing
        /// opaque parent and separate ID inventories. The debugger peer must
        /// still negotiate each grant and receive both IDs on its stream.
        pub fn with_debugger_static_metadata_contract_location(mut self) -> Self {
            self.debugger_static_metadata_inventory = true;
            self.debugger_static_metadata_source_inventory = true;
            self.debugger_static_metadata_contract_inventory = true;
            self.debugger_static_metadata_contract_location = true;
            self
        }

        /// Enables one verified symbol/type relation and its parent, symbol,
        /// and type inventory policies. A debugger peer still must negotiate
        /// each grant and obtain both exact ID receipts on its own stream.
        pub fn with_debugger_static_metadata_symbol_type(mut self) -> Self {
            self.debugger_static_metadata_inventory = true;
            self.debugger_static_metadata_type_inventory = true;
            self.debugger_static_metadata_symbol_inventory = true;
            self.debugger_static_metadata_symbol_type = true;
            self
        }

        /// Enables one verified symbol/contract relation and its parent,
        /// symbol, and contract inventory policies. A debugger peer still
        /// must negotiate each grant and obtain both exact ID receipts.
        pub fn with_debugger_static_metadata_symbol_contract(mut self) -> Self {
            self.debugger_static_metadata_inventory = true;
            self.debugger_static_metadata_symbol_inventory = true;
            self.debugger_static_metadata_contract_inventory = true;
            self.debugger_static_metadata_symbol_contract = true;
            self
        }
    }

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
    fn health_check(
        v2_stream: &mut UnixStream,
        captured_tabs: &[TabSummary],
    ) -> Result<(), String> {
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
                    let expected: Vec<&Option<String>> =
                        captured_tabs.iter().map(|t| &t.url).collect();
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
        let clients: Arc<Mutex<Vec<Sender<TaggedServerMessage>>>> =
            Arc::new(Mutex::new(Vec::new()));
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
                format!(
                    "{label} socket path exceeds {MAX_STABLE_ENDPOINT_SOCKET_PATH_BYTES} bytes"
                ),
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

    impl GenerationPinnedUnixRelay {
        fn bind(
            path: &Path,
            endpoint_label: &'static str,
            route_gate: Arc<Mutex<()>>,
        ) -> io::Result<Self> {
            prepare_stable_endpoint(path, endpoint_label)?;
            let listener = UnixListener::bind(path)?;
            // Do not depend on the process umask for a public capability
            // boundary.  This also keeps the public stable endpoint at the
            // same owner-only mode as the former core-owned listener.
            if let Err(error) =
                std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            {
                remove_owned_socket_if_owned(path);
                return Err(error);
            }
            if let Err(error) = listener.set_nonblocking(true) {
                remove_owned_socket_if_owned(path);
                return Err(error);
            }

            let target = Arc::new(Mutex::new(None));
            let accepting = Arc::new(AtomicBool::new(true));
            let thread_target = Arc::clone(&target);
            let thread_route_gate = Arc::clone(&route_gate);
            let thread_accepting = Arc::clone(&accepting);
            let accept_thread = thread::spawn(move || {
                while thread_accepting.load(Ordering::Acquire) {
                    match listener.accept() {
                        Ok((client, _)) => {
                            // Accepted descriptors inherit the listener's
                            // nonblocking flag on some Unix platforms.  The
                            // forwarding pair intentionally does blocking
                            // byte copies, so restore the per-connection
                            // default before it can mistake EAGAIN for EOF.
                            if client.set_nonblocking(false).is_err() {
                                let _ = client.shutdown(Shutdown::Both);
                                continue;
                            }
                            // Snapshot the route while accepting.  This
                            // gate is shared with the browser-writer handoff,
                            // so a newly paired MCP adapter cannot observe
                            // different browser/compiler generations.  The
                            // forwarding pair below owns that concrete
                            // private stream and never consults `target`
                            // again, so an old cursor/request stream cannot
                            // cross a catalog-generation cutover.
                            let _route_handoff = thread_route_gate
                                .lock()
                                .unwrap_or_else(|poisoned| poisoned.into_inner());
                            let target = thread_target
                                .lock()
                                .unwrap_or_else(|poisoned| poisoned.into_inner())
                                .clone();
                            if let Some(target) = target {
                                thread::spawn(move || {
                                    relay_generation_pinned_connection(client, target)
                                });
                            } else {
                                // A core is staged but not committed, or the
                                // launcher is stopping.  There is no safe
                                // fallback generation, so fail closed.
                                let _ = client.shutdown(Shutdown::Both);
                            }
                        }
                        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(10));
                        }
                        Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                        Err(_) => break,
                    }
                }
            });

            Ok(Self {
                public_socket_path: path.to_path_buf(),
                route_gate,
                target,
                accepting,
                accept_thread: Mutex::new(Some(accept_thread)),
            })
        }

        /// Makes subsequently accepted public connections target `socket`.
        /// Existing connections keep their already-open private stream.  The
        /// ordinary initial activation has no concurrent browser handoff.
        fn activate_generation(&self, socket: &Path) {
            let _route_handoff = self
                .route_gate
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            self.activate_generation_after_handoff(socket);
        }

        /// The caller holds [`Self::route_gate`] alongside the browser
        /// writer swap.  Kept separate so cutover never recursively locks
        /// the handoff mutex.
        fn activate_generation_after_handoff(&self, socket: &Path) {
            *self
                .target
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(socket.to_path_buf());
        }

        fn close(&self) {
            if !self.accepting.swap(false, Ordering::AcqRel) {
                return;
            }
            *self
                .target
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
            if let Some(thread) = self
                .accept_thread
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .take()
            {
                let _ = thread.join();
            }
            remove_owned_socket_if_owned(&self.public_socket_path);
        }
    }

    impl Drop for GenerationPinnedUnixRelay {
        fn drop(&mut self) {
            self.close();
        }
    }

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

    /// A `core` process this launcher spawned and owns privately: killed
    /// and cleaned up (process, internal socket, and frame directory) on
    /// [`Drop`], the same lifetime discipline `mcp-server`'s `CoreProcess`
    /// already established for its own (today, unshared) spawned `core`.
    pub struct SpawnedCore {
        child: Child,
        internal_socket_path: PathBuf,
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

    impl SpawnedCore {
        pub fn spawn(width: f64, height: f64, frame_dir: &Path) -> io::Result<Self> {
            Self::spawn_with_options(width, height, frame_dir, CoreLaunchOptions::default())
        }

        /// Starts a core under launcher-owned operational policy.
        ///
        /// When out-of-process BlueJS is selected, this method is the only
        /// place that creates the endpoint/token pair. It passes that private
        /// capability directly to the newly spawned core, then retains the
        /// child supervisor in this returned owner. Neither the public
        /// frontend broker nor a page receives either value. This v1 launch
        /// seam uses the core's existing private startup arguments; their
        /// values are launcher-generated, are never logged or forwarded over
        /// frontend IPC, and are not an operator-configurable interface. A
        /// descriptor-passing startup channel would be separate Unix process
        /// bootstrap work, not a reason to expose a second runtime protocol.
        pub fn spawn_with_options(
            width: f64,
            height: f64,
            frame_dir: &Path,
            options: CoreLaunchOptions,
        ) -> io::Result<Self> {
            // Do this before creating either child. A malformed, partial, or
            // occupied caller-selected endpoint cannot briefly spawn a core
            // or page host that would then need cleanup.
            let route_gate = Arc::new(Mutex::new(()));
            let compiler_mcp_relay = options
                .compiler_mcp_socket
                .as_deref()
                .map(|path| {
                    GenerationPinnedUnixRelay::bind(path, "compiler MCP", Arc::clone(&route_gate))
                })
                .transpose()?
                .map(Arc::new);
            let debugger_relay = options
                .debugger_socket
                .as_deref()
                .map(|path| {
                    GenerationPinnedUnixRelay::bind(path, "debugger", Arc::clone(&route_gate))
                })
                .transpose()?
                .map(Arc::new);
            let (bluejs_host, page_host_config) = if options.supervise_out_of_process_bluejs {
                let (host, config) = SpawnedBlueJsHost::spawn_for_core_with_runtime_limits(
                    options.bluejs_host_runtime_limits,
                )?;
                (Some(host), Some(config))
            } else {
                (None, None)
            };
            let core = Self::spawn_with_private_host(
                width,
                height,
                frame_dir,
                options,
                bluejs_host,
                page_host_config,
                RelaySet {
                    route_gate,
                    compiler_mcp_relay,
                    debugger_relay,
                },
            )?;
            core.activate_relays();
            Ok(core)
        }

        /// Stages a core during a broker cutover. The relays are existing
        /// public listeners, not new caller-selected endpoints; their
        /// targets intentionally stay on v1 until replay and health checking
        /// complete in [`cutover`].
        fn spawn_with_options_and_relays(
            width: f64,
            height: f64,
            frame_dir: &Path,
            options: CoreLaunchOptions,
            relays: RelaySet,
        ) -> io::Result<Self> {
            let (bluejs_host, page_host_config) = if options.supervise_out_of_process_bluejs {
                let (host, config) = SpawnedBlueJsHost::spawn_for_core_with_runtime_limits(
                    options.bluejs_host_runtime_limits,
                )?;
                (Some(host), Some(config))
            } else {
                (None, None)
            };
            Self::spawn_with_private_host(
                width,
                height,
                frame_dir,
                options,
                bluejs_host,
                page_host_config,
                relays,
            )
        }

        fn spawn_with_private_host(
            width: f64,
            height: f64,
            frame_dir: &Path,
            options: CoreLaunchOptions,
            bluejs_host: Option<SpawnedBlueJsHost>,
            page_host_config: Option<BlueJsHostCoreConfig>,
            relays: RelaySet,
        ) -> io::Result<Self> {
            let this_exe = std::env::current_exe()?;
            let core_bin = sibling_core_binary(&this_exe);
            let internal_socket_path = unique_internal_socket_path();
            let _ = std::fs::remove_file(&internal_socket_path);
            let compiler_private_socket_path = relays
                .compiler_mcp_relay
                .as_ref()
                .map(|_| unique_internal_compiler_socket_path());
            if let Some(path) = &compiler_private_socket_path {
                let _ = std::fs::remove_file(path);
            }
            let debugger_private_socket_path = relays
                .debugger_relay
                .as_ref()
                .map(|_| unique_internal_debugger_socket_path());
            if let Some(path) = &debugger_private_socket_path {
                let _ = std::fs::remove_file(path);
            }

            let mut command = Command::new(&core_bin);
            command
                .arg("--socket")
                .arg(&internal_socket_path)
                .arg("--width")
                .arg(width.to_string())
                .arg("--height")
                .arg(height.to_string())
                .arg("--frame-dir")
                .arg(frame_dir);
            if let Some(gatekeeper_socket) = &options.gatekeeper_socket {
                command.arg("--gatekeeper-socket").arg(gatekeeper_socket);
            }
            if let Some(config) = &page_host_config {
                command
                    .arg("--out-of-process-bluejs-socket")
                    .arg(config.socket_path())
                    .arg("--out-of-process-bluejs-token")
                    .arg(config.session_token());
            }
            if options.core_http_page_script_fixture {
                command
                    .arg("--out-of-process-bluejs-page-script-profile")
                    .arg("core-page-http-fixture-v1");
            }
            if let Some(compiler_socket) = &compiler_private_socket_path {
                command
                    .arg("--compiler-socket")
                    .arg(compiler_socket)
                    .arg("--compiler-project-profile")
                    .arg(CORE_CLOSED_COMPILER_PROJECT_PROFILE);
            }
            if let Some(debugger_socket) = &debugger_private_socket_path {
                command.arg("--debugger-socket").arg(debugger_socket);
                if options.debugger_static_metadata_inventory {
                    command.arg("--debugger-static-metadata-inventory");
                }
                if options.debugger_static_metadata_summary {
                    command.arg("--debugger-static-metadata-summary");
                }
                if options.debugger_static_metadata_source_inventory {
                    command.arg("--debugger-static-metadata-source-inventory");
                }
                if options.debugger_static_metadata_source_provenance {
                    command.arg("--debugger-static-metadata-source-provenance");
                }
                if options.debugger_static_metadata_type_inventory {
                    command.arg("--debugger-static-metadata-type-inventory");
                }
                if options.debugger_static_metadata_type_display {
                    command.arg("--debugger-static-metadata-type-display");
                }
                if options.debugger_static_metadata_symbol_inventory {
                    command.arg("--debugger-static-metadata-symbol-inventory");
                }
                if options.debugger_static_metadata_contract_inventory {
                    command.arg("--debugger-static-metadata-contract-inventory");
                }
                if options.debugger_static_metadata_contract_display {
                    command.arg("--debugger-static-metadata-contract-display");
                }
                if options.debugger_static_metadata_contract_validation {
                    command.arg("--debugger-static-metadata-contract-validation");
                }
                if options.debugger_static_metadata_lowering_summary {
                    command.arg("--debugger-static-metadata-lowering-summary");
                }
                if options.debugger_static_metadata_symbol_display {
                    command.arg("--debugger-static-metadata-symbol-display");
                }
                if options.debugger_static_metadata_symbol_location {
                    command.arg("--debugger-static-metadata-symbol-location");
                }
                if options.debugger_static_metadata_safe_point_span {
                    command.arg("--debugger-static-metadata-safe-point-span");
                }
                if options.debugger_static_metadata_contract_location {
                    command.arg("--debugger-static-metadata-contract-location");
                }
                if options.debugger_static_metadata_symbol_type {
                    command.arg("--debugger-static-metadata-symbol-type");
                }
                if options.debugger_static_metadata_symbol_contract {
                    command.arg("--debugger-static-metadata-symbol-contract");
                }
            }
            let mut child = command.spawn()?;

            if !wait_for_socket(&internal_socket_path, Duration::from_secs(5)) {
                let _ = child.kill();
                let _ = child.wait();
                let _ = std::fs::remove_file(&internal_socket_path);
                if let Some(compiler_socket) = &compiler_private_socket_path {
                    remove_compiler_mcp_socket_if_owned(compiler_socket);
                }
                if let Some(debugger_socket) = &debugger_private_socket_path {
                    remove_owned_socket_if_owned(debugger_socket);
                }
                return Err(io::Error::other(format!(
                    "blueice-core never created its socket at {}",
                    internal_socket_path.display()
                )));
            }
            if let Some(debugger_socket) = &debugger_private_socket_path {
                if !wait_for_socket(debugger_socket, Duration::from_secs(5)) {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = std::fs::remove_file(&internal_socket_path);
                    if let Some(compiler_socket) = &compiler_private_socket_path {
                        remove_compiler_mcp_socket_if_owned(compiler_socket);
                    }
                    remove_owned_socket_if_owned(debugger_socket);
                    return Err(io::Error::other(format!(
                        "blueice-core never created its debugger socket at {}",
                        debugger_socket.display()
                    )));
                }
            }
            let mut stream = match UnixStream::connect(&internal_socket_path) {
                Ok(stream) => stream,
                Err(error) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = std::fs::remove_file(&internal_socket_path);
                    if let Some(compiler_socket) = &compiler_private_socket_path {
                        remove_compiler_mcp_socket_if_owned(compiler_socket);
                    }
                    if let Some(debugger_socket) = &debugger_private_socket_path {
                        remove_owned_socket_if_owned(debugger_socket);
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
                let _ = child.kill();
                let _ = child.wait();
                let _ = std::fs::remove_file(&internal_socket_path);
                if let Some(compiler_socket) = &compiler_private_socket_path {
                    remove_compiler_mcp_socket_if_owned(compiler_socket);
                }
                if let Some(debugger_socket) = &debugger_private_socket_path {
                    remove_owned_socket_if_owned(debugger_socket);
                }
                return Err(error);
            }
            Ok(SpawnedCore {
                child,
                internal_socket_path,
                compiler_private_socket_path,
                debugger_private_socket_path,
                frame_dir: frame_dir.to_path_buf(),
                options,
                bluejs_host,
                route_gate: relays.route_gate,
                compiler_mcp_relay: relays.compiler_mcp_relay,
                debugger_relay: relays.debugger_relay,
                stream,
            })
        }

        fn activate_relays(&self) {
            if let (Some(relay), Some(socket)) = (
                self.compiler_mcp_relay.as_ref(),
                self.compiler_private_socket_path.as_ref(),
            ) {
                relay.activate_generation(socket);
            }
            if let (Some(relay), Some(socket)) = (
                self.debugger_relay.as_ref(),
                self.debugger_private_socket_path.as_ref(),
            ) {
                relay.activate_generation(socket);
            }
        }

        /// Activates this staged core while the shared `route_gate` is held
        /// by [`perform_swap`].
        fn activate_relays_after_handoff(&self) {
            if let (Some(relay), Some(socket)) = (
                self.compiler_mcp_relay.as_ref(),
                self.compiler_private_socket_path.as_ref(),
            ) {
                relay.activate_generation_after_handoff(socket);
            }
            if let (Some(relay), Some(socket)) = (
                self.debugger_relay.as_ref(),
                self.debugger_private_socket_path.as_ref(),
            ) {
                relay.activate_generation_after_handoff(socket);
            }
        }
    }

    impl Drop for SpawnedCore {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
            // A delegated child may still be blocked on the core-owned
            // protocol connection. Reap it only after core is gone, rather
            // than allowing the child's private capability to outlive its
            // intended core generation.
            drop(self.bluejs_host.take());
            let _ = std::fs::remove_file(&self.internal_socket_path);
            if let Some(compiler_socket) = &self.compiler_private_socket_path {
                remove_compiler_mcp_socket_if_owned(compiler_socket);
            }
            if let Some(debugger_socket) = &self.debugger_private_socket_path {
                remove_owned_socket_if_owned(debugger_socket);
            }
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
        fn broadcast_core_to_clients_drops_a_client_whose_channel_is_gone_without_affecting_the_others(
        ) {
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
            assert!(start.elapsed() < Duration::from_secs(1), "fanning out must be a cheap non-blocking queue push regardless of any client's own writer-thread state");

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

        fn unique_compiler_mcp_test_path(label: &str) -> PathBuf {
            PathBuf::from("/tmp").join(format!(
                "blueice-launcher-compiler-mcp-{label}-{}-{}.sock",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ))
        }

        #[test]
        fn compiler_mcp_endpoint_validation_rejects_non_socket_paths_without_unlinking_them() {
            let path = unique_compiler_mcp_test_path("regular-file");
            std::fs::write(&path, b"do not remove").unwrap();

            let error = prepare_compiler_mcp_endpoint(&path).unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
            assert_eq!(std::fs::read(&path).unwrap(), b"do not remove");

            let _ = std::fs::remove_file(path);
        }

        #[test]
        fn compiler_mcp_endpoint_validation_reclaims_only_a_stale_socket_and_rejects_a_live_one() {
            let stale = unique_compiler_mcp_test_path("stale");
            let stale_listener = UnixListener::bind(&stale).unwrap();
            drop(stale_listener);
            // macOS can retain a just-closed local listener briefly while it
            // drains the final descriptor state.  The production path only
            // runs at startup, but give this deterministic stale-fixture a
            // moment to become observably refused.
            thread::sleep(Duration::from_millis(20));
            prepare_compiler_mcp_endpoint(&stale).unwrap();
            assert!(
                !stale.exists(),
                "a stale socket may be reclaimed before any child is spawned"
            );

            let live = unique_compiler_mcp_test_path("live");
            let _live_listener = UnixListener::bind(&live).unwrap();
            let error = prepare_compiler_mcp_endpoint(&live).unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::AddrInUse);
            assert!(live.exists(), "a live endpoint must remain intact");

            let _ = std::fs::remove_file(live);
        }

        #[test]
        fn compiler_mcp_endpoint_validation_happens_before_core_child_spawn() {
            let path = unique_compiler_mcp_test_path("preflight");
            std::fs::write(&path, b"must survive preflight").unwrap();
            let frame_dir = std::env::temp_dir().join(format!(
                "blueice-launcher-compiler-mcp-preflight-{}",
                std::process::id()
            ));
            let result = SpawnedCore::spawn_with_options(
                1.0,
                1.0,
                &frame_dir,
                CoreLaunchOptions::default().with_core_closed_compiler_mcp_endpoint(path.clone()),
            );
            let error = match result {
                Ok(_) => panic!("a non-socket compiler endpoint must reject before spawning core"),
                Err(error) => error,
            };
            assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
            assert_eq!(std::fs::read(&path).unwrap(), b"must survive preflight");
            assert!(
                !frame_dir.exists(),
                "a rejected compiler endpoint must not start a core that creates a frame directory"
            );

            let _ = std::fs::remove_file(path);
        }

        #[test]
        fn wait_for_socket_returns_true_once_the_path_exists() {
            let path = std::env::temp_dir()
                .join(format!("blueice-launcher-wait-test-{}", std::process::id()));
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

            let (tab_id, request_id, msg) =
                read_client_message_with_ids(&mut core_observed).unwrap();
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

            let (tab_id, request_id, msg) =
                read_server_message_with_ids(&mut client_observed).unwrap();
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

            spawn_generation_tagged_broadcast(
                core_side,
                clients,
                Arc::clone(&generation),
                0,
                done_tx,
            );

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
                let (_, request_id, msg) =
                    read_client_message_with_ids(&mut core_observed).unwrap();
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
                    url: Some("about:blank".to_string())
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
                },
                TabSummary {
                    id: 2,
                    url: Some("about:credits".to_string()),
                },
                TabSummary { id: 3, url: None },
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
            let tabs = vec![TabSummary {
                id: 1,
                url: Some("http://bad".to_string()),
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
                TabSummary { id: 1, url: None },
                TabSummary {
                    id: 2,
                    url: Some("http://bad".to_string()),
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
            }];
            let responder = thread::spawn(move || {
                let (_, req, _msg) = read_client_message_with_ids(&mut server).unwrap();
                write_server_message_with_id(&mut server, req, &ServerMessage::Tabs(vec![]))
                    .unwrap();
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
}

#[cfg(unix)]
pub use unix::*;
