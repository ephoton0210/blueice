// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `blueice-core`: the process that owns the actual render pipeline
//! (per `CLAUDE.md`'s core requirement, one instance/one render pass
//! shared by whatever's watching -- a human's `frontend` window today,
//! an AI client later). This binary is deliberately thin: all the
//! logic it runs lives in `blueice_engine::{TabManager, session}`,
//! already covered by their own unit tests against an in-process `UnixStream`
//! pair -- this file is just argument parsing and wiring a real
//! `UnixListener` to that already-tested loop, matching how `dev` mode
//! is meant to be verified per `TEST_PLAN.md`: automated tests own the
//! logic, manual/e2e runs of the actual binary confirm the wiring.
//!
//! Accepts exactly one client connection, then exits when that client
//! disconnects or sends `Shutdown` -- there is no multi-frontend
//! support in this reference implementation. Optional additional Unix sockets
//! route the narrow BlueJS script, debugger, and registered-project compiler
//! protocols into that same session thread; their listeners never own DOM,
//! tab, realm, VM, source graph, or compiler-cache state themselves.
//!
//! `--stdio --frame-dir <new-directory>` instead accepts a private inherited
//! browser-protocol pipe on Unix or Windows. It reuses the same session loop
//! without the Unix launcher/service listeners; external navigation stays
//! blocked while gatekeeper review is unavailable.

#[cfg(unix)]
use blueice_engine::downloads_page::DownloadsSource;
#[cfg(unix)]
use blueice_engine::gatekeeper_settings_page::GatekeeperSettingsSource;
#[cfg(unix)]
use blueice_engine::session::ExtensionPageRequest;
#[cfg(unix)]
use blueice_engine::{
    compiler_ipc::{
        compiler_service_ipc_request_channel, CompilerServiceIpcRequestSender,
        CoreCompilerProjectCatalog,
    },
    script, session, HistorySnapshotMode, TabManager,
};
#[cfg(unix)]
use blueice_extension_host::{
    default_durable_storage_root,
    handle_extension_connection_with_actions_and_authentication_and_network_rules,
    load_installed_extension, registry_for_installed_extension, ExtensionActionDelegates,
    ExtensionConnectionAuthentication, ExtensionRegistry, ExtensionStorage,
};
#[cfg(unix)]
use blueice_ipc::extension::{ExtensionRuntimeEvent, NetworkResponseInfo, NetworkTraceInfo};
#[cfg(unix)]
use blueice_ipc::permission_control::{
    read_permission_control_request, write_permission_control_reply, EphemeralCapabilityInfo,
    OptionalCapabilityInfo, PermissionControlReply, PermissionControlRequest,
};
#[cfg(unix)]
use std::io::Write;
#[cfg(unix)]
use std::io::{self, Read};
#[cfg(unix)]
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
#[cfg(unix)]
use std::os::unix::net::{UnixListener, UnixStream};
#[cfg(unix)]
use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::process::ExitCode;
#[cfg(unix)]
use std::process::{Child, Command, Stdio};
#[cfg(unix)]
use std::sync::{
    atomic::{AtomicU64, Ordering},
    mpsc, Arc, Mutex,
};
#[cfg(unix)]
use std::thread;
#[cfg(unix)]
use std::time::Duration;

#[cfg(unix)]
#[derive(Debug, Default, PartialEq)]
struct Args {
    socket: PathBuf,
    width: f64,
    height: f64,
    frame_dir: Option<PathBuf>,
    /// Where a gated navigation's background thread connects to review
    /// a URL/fetched page (`phase-7-local-ai/PLAN.md`'s "Wiring
    /// design") -- `None` (the common case) resolves to
    /// `blueice_ipc::gatekeeper::default_gatekeeper_socket_path()`;
    /// overridable so `tests/core_binary.rs` can point a real spawned
    /// subprocess at its own fake/stub gatekeeper instead of the
    /// system-wide default path.
    gatekeeper_socket: Option<PathBuf>,
    /// Where `about:downloads` reads the downloads list from, and where a
    /// downloads process started on the page's behalf listens -- `None`
    /// (the common case) resolves to `blueice_ipc::downloads::
    /// default_downloads_socket_path()`. Overridable for the same reason
    /// `gatekeeper_socket` is: a test points a real subprocess at its own.
    downloads_socket: Option<PathBuf>,
    /// Keep displayable page snapshots in history instead of the default
    /// URL-only entries. This is opt-in because normal Back/Forward behavior
    /// re-fetches the URL and should therefore observe updated web content.
    history_snapshots: bool,
    /// Private extension-protocol listener. It is accepted only together
    /// with `extension_manifest`, so core never exposes the standalone
    /// hardcoded reference identity as a production-facing endpoint.
    extension_socket: Option<PathBuf>,
    /// Strict installed package manifest used to establish the server-side
    /// extension identity/grants before the extension socket is published.
    extension_manifest: Option<PathBuf>,
    /// The trusted BlueIce extension-host executable that core starts for the
    /// validated package. This enables a fresh child credential; without
    /// it, the explicit extension socket remains the legacy development mode.
    extension_host: Option<PathBuf>,
    /// Explicitly enables a private, framed parent-to-core permission pipe on
    /// stdin/stdout. It is never multiplexed over the public frontend socket.
    permission_control_stdio: bool,
    /// Live translation (`phase-7-local-ai/PLAN.md`): the `ai-assistant`
    /// socket (only a startup flag can name it) and, optionally, the BCP 47
    /// tag to start translating fetched pages into. A client can later choose
    /// or clear the language; without a socket, translation is unavailable.
    assistant_socket: Option<PathBuf>,
    translate_to: Option<String>,
    /// The whole-navigation translation budget; `None` means the default.
    translate_deadline_ms: Option<u64>,
    /// The assistant settings file `about:assistant` shows, read-only (`core`
    /// never writes it). The launcher passes the file it loaded.
    assistant_settings: Option<PathBuf>,
    /// Optional listener for the long-lived BlueJS script host. It is separate
    /// from the frontend protocol socket and can be injected by the launcher
    /// or an integration test; omitting it preserves the reference binary's
    /// current frontend-only mode.
    script_socket: Option<PathBuf>,
    /// Capability for this generation's child on the private script socket.
    /// The socket is never bound without this fixed-shape owner secret.
    script_session_token: Option<String>,
    /// Optional listener for the native debugger discovery channel. It remains
    /// separate from both frontend and DOM-script IPC; the session thread
    /// validates each requested tab/document generation before replying.
    debugger_socket: Option<PathBuf>,
    /// Owner policy for the separately negotiated bounded paused-value read.
    /// The owner flag alone never grants a debugger client access.
    debugger_bounded_values: bool,
    /// Core-owner opt-in for a bounded opaque static-metadata inventory. It
    /// never exposes a metadata record itself and still requires a client
    /// request plus a live child-side capability.
    debugger_static_metadata_inventory: bool,
    /// Core-owner opt-in for bounded source-free summaries of handles from
    /// the separately enabled metadata inventory. It exposes only compiler
    /// fingerprints and aggregate counts, never source identity/text, spans,
    /// names, types, symbols, contracts, bytecode, or runtime values.
    debugger_static_metadata_summary: bool,
    /// Core-owner opt-in for bounded metadata-handle-bound source-record IDs.
    /// IDs disclose no module, hash, source text, span, or record detail.
    debugger_static_metadata_source_inventory: bool,
    /// Core-owner opt-in for source-free compiler provenance of an already
    /// inventoried source ID. It requires metadata and source inventory and
    /// reveals only canonical module identity plus a labeled SHA-256 digest.
    debugger_static_metadata_source_provenance: bool,
    /// Core-owner opt-in for opaque compiler-minted type-record IDs under an
    /// already inventoried metadata handle. Type displays remain unavailable.
    debugger_static_metadata_type_inventory: bool,
    /// Core-owner opt-in for one bounded compiler-produced display under a
    /// type ID previously emitted by the separate type inventory.
    debugger_static_metadata_type_display: bool,
    /// Core-owner opt-in for opaque compiler-minted symbol-record IDs under
    /// an already inventoried metadata handle. Symbol detail remains denied.
    debugger_static_metadata_symbol_inventory: bool,
    /// Core-owner opt-in for opaque compiler-minted contract IDs under an
    /// already inventoried metadata handle. Contract detail remains denied.
    debugger_static_metadata_contract_inventory: bool,
    /// Core-owner opt-in for one bounded compiler-produced display under a
    /// contract ID previously emitted by the separate contract inventory.
    debugger_static_metadata_contract_display: bool,
    /// Core-owner opt-in for data-only validation against a contract ID
    /// previously emitted by the separate contract inventory. The reply is
    /// only a boolean; plans and structural failure detail stay private.
    debugger_static_metadata_contract_validation: bool,
    /// Core-owner opt-in for an aggregate direct-lowering-map summary under a
    /// prior opaque metadata receipt. It contains no map entries, spans, or
    /// bytecode locations.
    debugger_static_metadata_lowering_summary: bool,
    /// Core-owner opt-in for one bounded compiler-produced display under a
    /// symbol ID previously emitted by the separate symbol inventory.
    debugger_static_metadata_symbol_display: bool,
    /// Core-owner opt-in for one source-text-free half-open byte range under
    /// separately inventoried symbol and source IDs. It exposes no source,
    /// module identity, line/column data, type, contract, or bytecode.
    debugger_static_metadata_symbol_location: bool,
    /// Core-owner opt-in for one exact original BlueTS safe-point byte span
    /// under prior opaque metadata and source-ID receipts.
    debugger_static_metadata_safe_point_span: bool,
    /// Core-owner opt-in for bounded original BlueTS source-position binding.
    /// This is a distinct source-map oracle from exact safe-point span reads.
    debugger_static_metadata_source_breakpoint: bool,
    /// Independent owner grant for paused BlueTS source-span stepping.
    debugger_static_metadata_source_span_step: bool,
    /// Core-owner opt-in for a bounded contract declaration range under
    /// separately receipted contract and source IDs; no plan or source text.
    debugger_static_metadata_contract_location: bool,
    /// Core-owner opt-in for one compiler-verified symbol/type relation under
    /// separately inventoried IDs. It exposes no display or static record.
    debugger_static_metadata_symbol_type: bool,
    /// Core-owner opt-in for one compiler-verified symbol/contract relation
    /// under separately inventoried IDs. It exposes no plan or static record.
    debugger_static_metadata_symbol_contract: bool,
    /// Independent compiler-only paused lexical-slot relation grant.
    debugger_static_scope_relation: bool,
    /// Optional listener for queries over projects a trusted core owner
    /// registered during startup. Its protocol does not accept registration,
    /// source, path, resolver, compiler-option, build, or write requests.
    compiler_socket: Option<PathBuf>,
    /// Independent owner-granted build endpoint; never shares the query socket.
    compiler_output_socket: Option<PathBuf>,
    /// A compiled-in closed project profile selected by the core process
    /// owner. This is a startup-only test/integration seam, not a project
    /// file/path argument and never crosses compiler IPC.
    compiler_project_profile: Option<String>,
    /// One bounded, owner-only catalog is read from inherited stdin before
    /// listeners are created; no public request can supply another catalog.
    compiler_catalog_stdin: bool,
    /// One combined owner bootstrap carries an optional compiler catalog and
    /// HTTP page-resource manifest over inherited stdin before listeners.
    owner_bootstrap_stdin: bool,
    /// An explicitly selected, core-owned host typing profile for executing
    /// discovered inline BlueTS page declarations. Omission preserves the
    /// default no-inline-execution process mode; page content cannot select a
    /// profile or alter the compiler policy.
    inline_bluets_profile: Option<String>,
    /// Enables the bounded in-process standard JavaScript page host. It has no
    /// DOM bindings and is mutually exclusive with the experimental inline
    /// BlueTS executor so one page cannot acquire two independent VMs.
    inline_bluejs: bool,
    /// Owner-only child socket selected by a launcher/supervisor for the
    /// explicit out-of-process JavaScript host path. It is meaningful only
    /// together with its per-spawn capability token below.
    out_of_process_bluejs_socket: Option<PathBuf>,
    /// Per-spawn capability supplied by the trusted launcher/supervisor. This
    /// is never reflected to frontend/page code or printed by this binary.
    out_of_process_bluejs_token: Option<String>,
    /// A private launcher-to-core selector for one compiled-in external page
    /// script profile. It accepts only a fixed profile identifier; it never
    /// accepts a URL, manifest, resolver, path, source, or fetch setting.
    out_of_process_bluejs_page_script_profile: Option<String>,
}

#[cfg(unix)]
#[path = "blueice-core/args.rs"]
mod args;
#[cfg(unix)]
#[path = "blueice-core/compiler_output.rs"]
mod compiler_output;
#[cfg(unix)]
use args::parse_args;

#[cfg(unix)]
fn cleanup_private_sockets(
    script: &Option<PathBuf>,
    debugger: &Option<PathBuf>,
    compiler: &Option<PathBuf>,
    compiler_output: &Option<PathBuf>,
) {
    if let Some(path) = script {
        remove_owned_socket_if_owned(path);
    }
    if let Some(path) = debugger {
        let _ = std::fs::remove_file(path);
    }
    if let Some(path) = compiler {
        remove_owned_socket_if_owned(path);
    }
    if let Some(path) = compiler_output {
        remove_owned_socket_if_owned(path);
    }
}

#[cfg(unix)]
fn main() -> ExitCode {
    if std::env::args_os()
        .nth(1)
        .is_some_and(|arg| arg == "--stdio")
    {
        return stdio::main();
    }
    let args = match parse_args(std::env::args().skip(1)) {
        Ok(args) => args,
        Err(message) => {
            eprintln!("blueice-core: {message}");
            return ExitCode::FAILURE;
        }
    };

    let frame_dir = args.frame_dir.unwrap_or_else(|| {
        std::env::temp_dir().join(format!("blueice-core-frames-{}", std::process::id()))
    });
    let gatekeeper_socket = args
        .gatekeeper_socket
        .unwrap_or_else(blueice_ipc::gatekeeper::default_gatekeeper_socket_path);
    let downloads_socket = args.downloads_socket.clone();

    // An installed extension is a core concern: validate its package and
    // derive its registry identity before core publishes either socket. When
    // `--extension-host` is supplied, the listener additionally requires the
    // freshly generated credential from exactly that core-spawned child.
    let mut extension_capability_origins = Default::default();
    let mut permission_metadata = None;
    let (extension_service, extension_runtime_start, extension_runtime_events) = match (
        args.extension_socket.as_ref(),
        args.extension_manifest.as_deref(),
    ) {
        (Some(socket), Some(manifest)) => {
            let installed = match load_installed_extension(manifest) {
                Ok(installed) => installed,
                Err(error) => {
                    eprintln!(
                        "blueice-core: could not install extension {}: {error}",
                        manifest.display()
                    );
                    return ExitCode::FAILURE;
                }
            };
            extension_capability_origins = installed.manifest().capability_origins().clone();
            permission_metadata = Some(PermissionControlMetadata {
                extension_id: installed.extension_id().to_string(),
                name: installed.manifest().name().to_string(),
                version: installed.manifest().version().to_string(),
                optional: installed
                    .manifest()
                    .capabilities()
                    .optional()
                    .iter()
                    .map(|capability| OptionalCapabilityInfo {
                        capability: capability.clone(),
                        granted: false,
                        origins: installed
                            .manifest()
                            .capability_origins()
                            .get(capability)
                            .map(|origins| origins.iter().cloned().collect())
                            .unwrap_or_default(),
                    })
                    .collect(),
                ephemeral: installed
                    .manifest()
                    .capabilities()
                    .runtime_ephemeral()
                    .iter()
                    .map(|capability| EphemeralCapabilityInfo {
                        capability: capability.clone(),
                        origins: installed
                            .manifest()
                            .capability_origins()
                            .get(capability)
                            .map(|origins| origins.iter().cloned().collect())
                            .unwrap_or_default(),
                    })
                    .collect(),
            });
            if let Some(parent) = socket.parent() {
                if let Err(error) = blueice_ipc::local_socket::ensure_private_socket_dir(parent) {
                    eprintln!(
                        "blueice-core: failed to prepare private extension socket directory {}: {error}",
                        parent.display()
                    );
                    return ExitCode::FAILURE;
                }
            }
            let _ = std::fs::remove_file(socket);
            let listener = match blueice_ipc::local_socket::bind_private_listener(socket) {
                Ok(listener) => listener,
                Err(error) => {
                    eprintln!(
                        "blueice-core: failed to bind extension socket {}: {error}",
                        socket.display()
                    );
                    return ExitCode::FAILURE;
                }
            };
            let required_authentication = match args.extension_host.as_ref() {
                Some(_) => match new_extension_authentication() {
                    Ok(authentication) => Some(authentication),
                    Err(error) => {
                        let _ = std::fs::remove_file(socket);
                        eprintln!("blueice-core: {error}");
                        return ExitCode::FAILURE;
                    }
                },
                None => None,
            };
            let (runtime_start, runtime_start_receiver, runtime_events, runtime_event_receiver) =
                if args.extension_host.is_some() {
                    let (sender, receiver) = mpsc::channel();
                    let (event_sender, event_receiver) = mpsc::sync_channel(16);
                    (
                        Some(sender),
                        Some(Arc::new(Mutex::new(receiver))),
                        Some(event_sender),
                        Some(Arc::new(Mutex::new(event_receiver))),
                    )
                } else {
                    (None, None, None, None)
                };
            (
                Some(ExtensionService {
                    socket: socket.clone(),
                    listener,
                    registry: Arc::new(registry_for_installed_extension(&installed)),
                    extension_id: installed.extension_id().to_string(),
                    storage: match default_durable_storage_root() {
                        Ok(root) => ExtensionStorage::default().with_durable_root(root),
                        Err(reason) => {
                            eprintln!(
                                "blueice-core: durable extension storage unavailable: {reason}"
                            );
                            ExtensionStorage::default()
                        }
                    },
                    required_authentication,
                    runtime_start: runtime_start_receiver,
                    runtime_events: runtime_event_receiver,
                }),
                runtime_start,
                runtime_events,
            )
        }
        (None, None) => (None, None, None),
        // `parse_args` enforces this before `main`; retain a total match so a
        // future construction of `Args` cannot accidentally make an unsafe
        // partial configuration reachable.
        _ => unreachable!("extension options were validated during argument parsing"),
    };

    let script_socket = args.script_socket.clone();
    let script_session_token = args.script_session_token.clone();
    let debugger_socket = args.debugger_socket.clone();
    // The owner choice is carried to this core generation's debugger
    // listener and intersected with each client's current-version Hello request.
    let debugger_bounded_values = args.debugger_bounded_values;
    let debugger_allowed_metadata_capabilities = if args.debugger_static_metadata_inventory {
        blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::opaque_selected(
            blueice_ipc::debugger::DebuggerMetadataCapabilitySelection {
                summary: args.debugger_static_metadata_summary,
                source_inventory: args.debugger_static_metadata_source_inventory,
                source_provenance: args.debugger_static_metadata_source_provenance,
                type_inventory: args.debugger_static_metadata_type_inventory,
                type_display: args.debugger_static_metadata_type_display,
                symbol_inventory: args.debugger_static_metadata_symbol_inventory,
                contract_inventory: args.debugger_static_metadata_contract_inventory,
                symbol_display: args.debugger_static_metadata_symbol_display,
                contract_display: args.debugger_static_metadata_contract_display,
                contract_validation: args.debugger_static_metadata_contract_validation,
                lowering_summary: args.debugger_static_metadata_lowering_summary,
                symbol_location: args.debugger_static_metadata_symbol_location,
                safe_point_span: args.debugger_static_metadata_safe_point_span,
                source_breakpoint: args.debugger_static_metadata_source_breakpoint,
                source_span_step: args.debugger_static_metadata_source_span_step,
                contract_location: args.debugger_static_metadata_contract_location,
                symbol_type: args.debugger_static_metadata_symbol_type,
                symbol_contract: args.debugger_static_metadata_symbol_contract,
                static_scope_relation: args.debugger_static_scope_relation,
            },
        )
    } else {
        blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::empty()
    };
    let compiler_socket = args.compiler_socket.clone();
    let compiler_output_socket = args.compiler_output_socket.clone();
    let inline_bluets_profile = args.inline_bluets_profile.clone();
    let inline_bluejs = args.inline_bluejs;
    let out_of_process_bluejs_socket = args.out_of_process_bluejs_socket.clone();
    let out_of_process_bluejs_token = args.out_of_process_bluejs_token.clone();
    let out_of_process_bluejs_page_script_profile =
        args.out_of_process_bluejs_page_script_profile.clone();

    // The optional compiler catalog is populated before *any* listener is
    // bound. After `seal`, only its session owner can dispatch opaque query
    // requests; neither this CLI nor a socket request accepts project inputs.
    let mut compiler_catalog = compiler_socket
        .as_ref()
        .map(|_| CoreCompilerProjectCatalog::default());
    if let Some(profile) = args.compiler_project_profile.as_deref() {
        let catalog = compiler_catalog
            .as_mut()
            .expect("argument validation requires a compiler socket for a profile");
        if let Err(error) = register_compiler_startup_profile(catalog, profile) {
            eprintln!("blueice-core: {error}");
            return ExitCode::FAILURE;
        }
    }
    if args.compiler_catalog_stdin {
        let bootstrap =
            match blueice_ipc::compiler_catalog::read_compiler_catalog(&mut io::stdin().lock()) {
                Ok(bootstrap) => bootstrap,
                Err(error) => {
                    eprintln!("blueice-core: invalid owner compiler catalog: {error}");
                    return ExitCode::FAILURE;
                }
            };
        let catalog = compiler_catalog
            .as_mut()
            .expect("argument validation requires a compiler socket for a catalog");
        if let Err(error) = register_owner_compiler_catalog(catalog, bootstrap) {
            eprintln!("blueice-core: {error}");
            return ExitCode::FAILURE;
        }
    }
    let mut owner_page_http_policy = None;
    if args.owner_bootstrap_stdin {
        let bootstrap = match blueice_ipc::owner_bootstrap::read_core_owner_bootstrap(
            &mut io::stdin().lock(),
        ) {
            Ok(bootstrap) => bootstrap,
            Err(error) => {
                eprintln!("blueice-core: invalid owner bootstrap: {error}");
                return ExitCode::FAILURE;
            }
        };
        if compiler_socket.is_some()
            != (bootstrap.compiler_catalog.is_some() || args.compiler_project_profile.is_some())
            || (bootstrap.compiler_catalog.is_some() && args.compiler_project_profile.is_some())
            || (bootstrap.page_http_policy.is_some() && out_of_process_bluejs_socket.is_none())
        {
            eprintln!("blueice-core: owner bootstrap does not match its private startup endpoints");
            return ExitCode::FAILURE;
        }
        if let Some(page_policy) = bootstrap.page_http_policy {
            owner_page_http_policy = match construct_owner_http_page_policy(page_policy) {
                Ok(policy) => Some(policy),
                Err(error) => {
                    eprintln!("blueice-core: invalid owner HTTP page policy: {error}");
                    return ExitCode::FAILURE;
                }
            };
        }
        if let Some(projects) = bootstrap.compiler_catalog {
            let catalog = compiler_catalog
                .as_mut()
                .expect("owner bootstrap compiler catalog requires a compiler socket");
            if let Err(error) = register_owner_compiler_catalog(catalog, projects) {
                eprintln!("blueice-core: {error}");
                return ExitCode::FAILURE;
            }
        }
    }
    let mut compiler_service = compiler_catalog.map(CoreCompilerProjectCatalog::seal);
    if compiler_output_socket.is_some()
        && !compiler_service
            .as_ref()
            .is_some_and(|service| service.has_exposed_output_grants())
    {
        eprintln!("blueice-core: compiler output socket requires an exposed owner write grant");
        return ExitCode::FAILURE;
    }

    let script_listener = match script_socket.as_ref() {
        Some(path) => match bind_script_listener(path) {
            Ok(listener) => Some(listener),
            Err(error) => {
                if let Some(path) = &args.extension_socket {
                    let _ = std::fs::remove_file(path);
                }
                eprintln!(
                    "blueice-core: failed to bind script socket {}: {error}",
                    path.display()
                );
                return ExitCode::FAILURE;
            }
        },
        None => None,
    };
    let debugger_listener = match debugger_socket.as_ref() {
        Some(path) => {
            let _ = std::fs::remove_file(path);
            match UnixListener::bind(path) {
                Ok(listener) => Some(listener),
                Err(error) => {
                    if let Some(path) = &script_socket {
                        remove_owned_socket_if_owned(path);
                    }
                    if let Some(path) = &args.extension_socket {
                        let _ = std::fs::remove_file(path);
                    }
                    eprintln!(
                        "blueice-core: failed to bind debugger socket {}: {error}",
                        path.display()
                    );
                    return ExitCode::FAILURE;
                }
            }
        }
        None => None,
    };
    let compiler_listener = match compiler_socket.as_ref() {
        Some(path) => match bind_compiler_listener(path) {
            Ok(listener) => Some(listener),
            Err(error) => {
                if let Some(path) = &script_socket {
                    remove_owned_socket_if_owned(path);
                }
                if let Some(path) = &debugger_socket {
                    let _ = std::fs::remove_file(path);
                }
                if let Some(path) = &args.extension_socket {
                    let _ = std::fs::remove_file(path);
                }
                eprintln!(
                    "blueice-core: failed to bind compiler socket {}: {error}",
                    path.display()
                );
                return ExitCode::FAILURE;
            }
        },
        None => None,
    };
    let compiler_output_listener = match compiler_output_socket.as_ref() {
        Some(path) => match bind_owner_only_listener(path, "compiler output") {
            Ok(listener) => Some(listener),
            Err(error) => {
                if let Some(path) = &script_socket {
                    remove_owned_socket_if_owned(path);
                }
                if let Some(path) = &debugger_socket {
                    let _ = std::fs::remove_file(path);
                }
                if let Some(path) = &compiler_socket {
                    remove_owned_socket_if_owned(path);
                }
                if let Some(path) = &args.extension_socket {
                    let _ = std::fs::remove_file(path);
                }
                eprintln!(
                    "blueice-core: failed to bind compiler output socket {}: {error}",
                    path.display()
                );
                return ExitCode::FAILURE;
            }
        },
        None => None,
    };

    // A stale socket file from a previous run (e.g. one that crashed
    // instead of exiting cleanly) makes bind() fail with AddrInUse
    // even though nothing is actually listening -- remove it first.
    let extension_socket = extension_service
        .as_ref()
        .map(|service| service.socket.clone());
    let extension_permissions = extension_service
        .as_ref()
        .map(|service| (Arc::clone(&service.registry), service.extension_id.clone()));
    let mut permission_session_requests = None;
    let (extension_requests, mut extension_host_child) = if let Some(service) = extension_service {
        let (tx, rx) = mpsc::channel();
        if args.permission_control_stdio {
            permission_session_requests = Some(tx.clone());
        }
        let required_authentication = service.required_authentication.clone();
        let (authenticated_ready, ready_rx) = if args.extension_host.is_some() {
            let (ready_tx, ready_rx) = mpsc::channel();
            (Some(ready_tx), Some(ready_rx))
        } else {
            (None, None)
        };
        spawn_extension_listener(service, gatekeeper_socket.clone(), tx, authenticated_ready);

        let child = if let Some(host) = args.extension_host.as_deref() {
            let manifest = args
                .extension_manifest
                .as_deref()
                .expect("extension-host configuration requires a manifest");
            let socket = extension_socket
                .as_deref()
                .expect("extension-host configuration requires an extension socket");
            let authentication = required_authentication
                .as_deref()
                .expect("extension-host configuration requires an authentication token");
            let child = match spawn_extension_host(host, socket, manifest, authentication) {
                Ok(child) => child,
                Err(error) => {
                    let _ = std::fs::remove_file(socket);
                    cleanup_private_sockets(
                        &script_socket,
                        &debugger_socket,
                        &compiler_socket,
                        &compiler_output_socket,
                    );
                    eprintln!("blueice-core: {error}");
                    return ExitCode::FAILURE;
                }
            };
            let ready_rx = ready_rx.expect("extension-host readiness receiver is configured");
            match ready_rx.recv_timeout(Duration::from_secs(5)) {
                Ok(()) => Some(child),
                Err(error) => {
                    stop_extension_host(child);
                    let _ = std::fs::remove_file(socket);
                    cleanup_private_sockets(
                        &script_socket,
                        &debugger_socket,
                        &compiler_socket,
                        &compiler_output_socket,
                    );
                    eprintln!(
                        "blueice-core: extension host did not authenticate before frontend readiness: {error}"
                    );
                    return ExitCode::FAILURE;
                }
            }
        } else {
            None
        };
        (Some(rx), child)
    } else {
        (None, None)
    };

    if args.permission_control_stdio {
        let metadata = permission_metadata
            .take()
            .expect("permission control requires a package");
        let (registry, _) = extension_permissions
            .as_ref()
            .expect("permission control requires a registry");
        let registry = Arc::clone(registry);
        let requests = permission_session_requests
            .take()
            .expect("permission control requires a session channel");
        let runtime_events = extension_runtime_events.clone();
        thread::spawn(move || {
            if let Err(error) = serve_permission_control(
                io::stdin(),
                io::stdout(),
                metadata,
                registry,
                requests,
                runtime_events,
            ) {
                eprintln!("blueice-core: private permission control ended: {error}");
            }
        });
    }

    let _ = std::fs::remove_file(&args.socket);

    let listener = match UnixListener::bind(&args.socket) {
        Ok(listener) => listener,
        Err(e) => {
            if let Some(child) = extension_host_child.take() {
                stop_extension_host(child);
            }
            if let Some(extension_socket) = extension_socket.as_ref() {
                let _ = std::fs::remove_file(extension_socket);
            }
            cleanup_private_sockets(
                &script_socket,
                &debugger_socket,
                &compiler_socket,
                &compiler_output_socket,
            );
            eprintln!(
                "blueice-core: failed to bind {}: {e}",
                args.socket.display()
            );
            return ExitCode::FAILURE;
        }
    };

    let result = (|| -> std::io::Result<()> {
        let (script_sender, script_requests) = script::script_request_channel();
        let (debugger_sender, debugger_requests) =
            blueice_engine::debugger::debugger_request_channel();
        let (compiler_sender, compiler_requests) = compiler_service_ipc_request_channel();
        if let Some(listener) = script_listener {
            let token = script_session_token.expect("script listener requires its capability");
            thread::spawn(move || serve_script_listener(listener, script_sender, token));
        }
        if let Some(listener) = debugger_listener {
            thread::spawn(move || {
                serve_debugger_listener(
                    listener,
                    debugger_sender,
                    debugger_allowed_metadata_capabilities,
                    debugger_bounded_values,
                )
            });
        }
        if let Some(listener) = compiler_listener {
            thread::spawn({
                let compiler_sender = compiler_sender.clone();
                move || serve_compiler_listener(listener, compiler_sender)
            });
        }
        if let Some(listener) = compiler_output_listener {
            thread::spawn(move || {
                compiler_output::serve_compiler_output_listener(listener, compiler_sender)
            });
        }
        let (mut stream, _) = listener.accept()?;
        let history_mode = if args.history_snapshots {
            HistorySnapshotMode::Snapshot
        } else {
            HistorySnapshotMode::Reload
        };
        let mut tabs =
            TabManager::new_with_history_snapshot_mode(args.width, args.height, history_mode);
        tabs.set_extension_capability_origins(extension_capability_origins);
        if let Some((registry, extension_id)) = extension_permissions.as_ref() {
            tabs.set_extension_permission_registry(Arc::clone(registry), extension_id.clone());
        }
        tabs.set_downloads_source(Arc::new(match downloads_socket {
            Some(socket) => DownloadsSource::at(socket),
            None => DownloadsSource::new(),
        }));
        tabs.set_gatekeeper_settings_source(Arc::new(GatekeeperSettingsSource::at(
            gatekeeper_socket.clone(),
        )));
        tabs.assistant_panel()
            .set_settings_file(args.assistant_settings.clone());
        if let Some(socket) = &args.assistant_socket {
            tabs.set_translation_endpoint(
                socket.clone(),
                Duration::from_millis(
                    args.translate_deadline_ms
                        .unwrap_or(DEFAULT_TRANSLATE_DEADLINE_MS),
                ),
            );
            tabs.set_translation_language(args.translate_to.clone());
        }
        if let Some(runtime_start) = extension_runtime_start.as_ref() {
            // The accepted frontend and its newly constructed session are the
            // earliest point at which a Wasm host request can reach a live
            // `TabManager`. The sender is one-shot: only the authenticated
            // child can consume its paired receiver through RuntimeReady.
            let _ = runtime_start.send(());
        }
        let mut generation = 0u64;
        if inline_bluejs {
            let mut javascript_executor = script::javascript::JavaScriptPageExecutor::with_config(
                script::javascript::JavaScriptPageExecutorConfig {
                    // Deferral changes scheduling, so activate it only when
                    // this process also owns the private debugger transport.
                    // The ordinary `--inline-bluejs` route remains immediate.
                    native_debugger_execution_control: debugger_socket.is_some(),
                    ..script::javascript::JavaScriptPageExecutorConfig::default()
                },
            )
            .map_err(|error| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("invalid inline JavaScript host configuration: {error}"),
                )
            })?;
            session::run_session_with_script_and_debugger_requests_and_inline_javascript_executor(
                &mut tabs,
                &mut stream,
                &frame_dir,
                &mut generation,
                &gatekeeper_socket,
                session::CoreSessionRequests {
                    script: script_socket.as_ref().map(|_| &script_requests),
                    debugger: debugger_socket.as_ref().map(|_| &debugger_requests),
                    compiler: compiler_service.as_mut().map(|service| {
                        session::CoreCompilerSessionRequests {
                            receiver: &compiler_requests,
                            service,
                        }
                    }),
                    extension: extension_requests.as_ref(),
                    extension_events: extension_runtime_events.as_ref(),
                },
                Some(&mut javascript_executor),
            )
        } else if let (Some(socket), Some(token)) = (
            out_of_process_bluejs_socket.as_deref(),
            out_of_process_bluejs_token.as_deref(),
        ) {
            let mut javascript_executor = if let Some(policy) = owner_page_http_policy.take() {
                script::javascript_child::OutOfProcessJavaScriptPageExecutor::connect_with_external_source_authorizer(
                    socket,
                    token,
                    script::http_resource_authorizer::HttpOutOfProcessPageScriptSourceAuthorizer::new(policy),
                )
            } else {
                match out_of_process_bluejs_page_script_profile.as_deref() {
                None => script::javascript_child::OutOfProcessJavaScriptPageExecutor::connect(
                    socket, token,
                ),
                Some(script::http_resource_authorizer::CORE_HTTP_PAGE_SCRIPT_FIXTURE_PROFILE) => {
                    script::javascript_child::OutOfProcessJavaScriptPageExecutor::connect_with_external_source_authorizer(
                        socket,
                        token,
                        script::http_resource_authorizer::CoreHttpPageScriptFixtureAuthorizer::new(),
                    )
                }
                // `parse_args` rejects every other value before this point.
                Some(_) => unreachable!("page script profile was validated during argument parsing"),
                }
            }
            .map_err(|error| {
                io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    format!("failed to connect to explicit BlueJS child host: {error}"),
                )
            })?;
            // The private child path keeps its ordinary immediate execution
            // schedule unless this core also owns the independent debugger
            // listener. Selecting both at trusted startup activates only the
            // bounded root-classic lifecycle; it does not expose a child VM,
            // source, bytecode, values, or generic interruption operation.
            if debugger_socket.is_some() {
                javascript_executor.enable_debugger_execution_control();
            }
            session::run_session_with_script_and_debugger_requests_and_out_of_process_javascript_executor(
                &mut tabs,
                &mut stream,
                &frame_dir,
                &mut generation,
                &gatekeeper_socket,
                session::CoreSessionRequests {
                    script: script_socket.as_ref().map(|_| &script_requests),
                    debugger: debugger_socket.as_ref().map(|_| &debugger_requests),
                    compiler: compiler_service.as_mut().map(|service| {
                        session::CoreCompilerSessionRequests {
                            receiver: &compiler_requests,
                            service,
                        }
                    }),
                    extension: extension_requests.as_ref(),
                    extension_events: extension_runtime_events.as_ref(),
                },
                Some(&mut javascript_executor),
            )
        } else if let Some(feature_profile) = inline_bluets_profile {
            let mut inline_executor = script::inline_runner::DirectPageInlineExecutor::new(
                script::host_typings::core_script_host_type_catalog(),
                feature_profile,
                blueice_bluets::CompilerOptions::default(),
            )
            .map_err(|error| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("invalid inline BlueTS profile: {error}"),
                )
            })?;
            session::run_session_with_script_and_debugger_requests_and_inline_page_executor(
                &mut tabs,
                &mut stream,
                &frame_dir,
                &mut generation,
                &gatekeeper_socket,
                session::CoreSessionRequests {
                    script: script_socket.as_ref().map(|_| &script_requests),
                    debugger: debugger_socket.as_ref().map(|_| &debugger_requests),
                    compiler: compiler_service.as_mut().map(|service| {
                        session::CoreCompilerSessionRequests {
                            receiver: &compiler_requests,
                            service,
                        }
                    }),
                    extension: extension_requests.as_ref(),
                    extension_events: extension_runtime_events.as_ref(),
                },
                Some(&mut inline_executor),
            )
        } else {
            session::run_session_with_core_session_requests(
                &mut tabs,
                &mut stream,
                &frame_dir,
                &mut generation,
                &gatekeeper_socket,
                session::CoreSessionRequests {
                    script: script_socket.as_ref().map(|_| &script_requests),
                    debugger: debugger_socket.as_ref().map(|_| &debugger_requests),
                    compiler: compiler_service.as_mut().map(|service| {
                        session::CoreCompilerSessionRequests {
                            receiver: &compiler_requests,
                            service,
                        }
                    }),
                    extension: extension_requests.as_ref(),
                    extension_events: extension_runtime_events.as_ref(),
                },
            )
        }
    })();

    // Closing the bounded producer is the normal lifecycle shutdown signal.
    // The authenticated handler turns it into RuntimeEventStreamClosed before
    // the child receives the containment fallback below.
    drop(extension_runtime_events);
    if let Some(child) = extension_host_child.take() {
        stop_extension_host(child);
    }
    if let Some(path) = extension_socket.as_ref() {
        let _ = std::fs::remove_file(path);
    }
    let _ = std::fs::remove_file(&args.socket);
    if let Some(path) = script_socket {
        remove_owned_socket_if_owned(&path);
    }
    if let Some(path) = debugger_socket {
        let _ = std::fs::remove_file(path);
    }
    if let Some(path) = compiler_socket {
        remove_owned_socket_if_owned(&path);
    }
    if let Some(path) = compiler_output_socket {
        remove_owned_socket_if_owned(&path);
    }
    let _ = std::fs::remove_dir_all(&frame_dir);

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("blueice-core: session error: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(not(unix))]
fn main() -> std::process::ExitCode {
    if std::env::args_os()
        .nth(1)
        .is_some_and(|arg| arg == "--stdio")
    {
        stdio::main()
    } else {
        eprintln!("blueice-core on this platform requires --stdio and --frame-dir");
        std::process::ExitCode::FAILURE
    }
}

#[path = "blueice-core/stdio.rs"]
mod stdio;

#[cfg(all(test, unix))]
#[path = "blueice-core/tests.rs"]
mod tests;

#[cfg(all(test, unix))]
#[path = "blueice-core/feature_tests.rs"]
mod feature_tests;

#[cfg(unix)]
#[path = "blueice-core/extension_service.rs"]
mod extension_service;
#[cfg(unix)]
use extension_service::*;
#[cfg(unix)]
#[path = "blueice-core/compiler_bootstrap.rs"]
mod compiler_bootstrap;
#[cfg(unix)]
use compiler_bootstrap::*;
#[cfg(unix)]
#[path = "blueice-core/protocol_servers.rs"]
mod protocol_servers;
#[cfg(unix)]
use protocol_servers::*;
