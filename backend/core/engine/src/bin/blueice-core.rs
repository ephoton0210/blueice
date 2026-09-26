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

#[cfg(unix)]
use blueice_engine::{
    compiler_ipc::{
        compiler_service_ipc_request_channel, CompilerServiceIpcRequestSender,
        CoreCompilerProjectCatalog,
    },
    script, session, TabManager,
};
#[cfg(unix)]
use std::io::{self, Read};
#[cfg(unix)]
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
#[cfg(unix)]
use std::os::unix::net::{UnixListener, UnixStream};
#[cfg(unix)]
use std::path::PathBuf;
#[cfg(unix)]
use std::process::ExitCode;
#[cfg(unix)]
use std::thread;

#[cfg(unix)]
#[derive(Debug, PartialEq)]
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
use args::parse_args;

/// Registers the reference binary's deliberately compiled-in closed fixture.
/// Real embedders use [`CoreCompilerProjectCatalog`] directly at trusted core
/// startup, where they can supply their already-authorized graph and fixed
/// policy without ever making a path/source/configuration API available to a
/// compiler peer. Keeping this one profile in code gives the public process
/// seam a real lifecycle regression target without turning a CLI flag into a
/// filesystem project loader.
#[cfg(unix)]
fn register_compiler_startup_profile(
    catalog: &mut CoreCompilerProjectCatalog,
    profile: &str,
) -> Result<(), String> {
    use blueice_bluets::{
        AuthorizedModule, AuthorizedModuleLoader, CompilerOptions, RuntimePolicy,
    };

    match profile {
        "core-closed-fixture-v1" => {
            let entry_module = "project:///core-fixture/main.ts";
            catalog
                .register_startup_project(
                    blueice_engine::compiler_service::RegisteredProjectRegistration {
                        canonical_project_root: "project:///core-fixture".to_string(),
                        canonical_config_root: "project:///core-fixture/blue-ts.json".to_string(),
                        canonical_output_root: "project:///core-fixture-dist".to_string(),
                        entry_module: entry_module.to_string(),
                        loader: AuthorizedModuleLoader::new(
                            [AuthorizedModule::new(
                                entry_module,
                                "interface CoreFixtureSettings { enabled: boolean; } \
                                 export const coreFixtureSettings: CoreFixtureSettings = { enabled: true }; \
                                 export const coreRegisteredAnswer: number = 42;",
                            )],
                            [],
                        )
                        .map_err(|error| {
                            format!("invalid compiled-in compiler project profile: {error}")
                        })?,
                        compiler_options: CompilerOptions {
                            resolver_fingerprint: "core-closed-fixture-v1".to_string(),
                            runtime_policy: RuntimePolicy::Checked,
                            ..CompilerOptions::default()
                        },
                    },
                )
                .map_err(|error| format!("failed to register compiler startup profile: {error}"))?;
            Ok(())
        }
        _ => Err(format!(
            "unsupported compiler project profile: {profile}; only core-owned compiled-in profiles are accepted"
        )),
    }
}

/// Consumes the trusted launcher's already-selected closed graph. This runs
/// before any core, compiler, debugger, or script listener is bound. Loader
/// construction and catalog registration reject invalid graphs atomically.
#[cfg(unix)]
fn register_owner_compiler_catalog(
    catalog: &mut CoreCompilerProjectCatalog,
    bootstrap: blueice_ipc::compiler_catalog::CompilerCatalogBootstrap,
) -> Result<(), String> {
    use blueice_bluets::{
        AuthorizedModule, AuthorizedModuleLoader, AuthorizedModuleResolution, CompilerOptions,
        EcmaTarget, ModuleSource, RuntimePolicy,
    };
    use blueice_ipc::compiler_catalog::{CompilerCatalogRuntimePolicy, CompilerCatalogTarget};

    bootstrap.validate().map_err(|error| error.to_string())?;
    for project in bootstrap.projects {
        let loader = AuthorizedModuleLoader::new(
            project
                .modules
                .into_iter()
                .map(|module| AuthorizedModule::new(module.canonical_id, module.text)),
            project.resolutions.into_iter().map(|edge| {
                AuthorizedModuleResolution::new(
                    edge.from_module,
                    edge.specifier,
                    edge.target_module,
                )
            }),
        )
        .map_err(|error| format!("invalid owner compiler graph: {error}"))?;
        let compiler_options = CompilerOptions {
            target: match project.options.target {
                CompilerCatalogTarget::Es2020 => EcmaTarget::Es2020,
                CompilerCatalogTarget::Es2022 => EcmaTarget::Es2022,
            },
            runtime_policy: match project.options.runtime_policy {
                CompilerCatalogRuntimePolicy::TranspileOnly => RuntimePolicy::TranspileOnly,
                CompilerCatalogRuntimePolicy::Checked => RuntimePolicy::Checked,
                CompilerCatalogRuntimePolicy::StrictRuntime => RuntimePolicy::StrictRuntime,
            },
            source_map: project.options.source_map,
            declaration: project.options.declaration,
            resolver_fingerprint: project.options.resolver_fingerprint,
            ambient_declaration_modules: project
                .options
                .ambient_declaration_modules
                .into_iter()
                .map(|module| ModuleSource::new(module.canonical_id, module.text))
                .collect(),
            require_declared_global_calls: project.options.require_declared_global_calls,
            ..CompilerOptions::default()
        };
        let registration = blueice_engine::compiler_service::RegisteredProjectRegistration {
            canonical_project_root: project.canonical_project_root,
            canonical_config_root: project.canonical_config_root,
            canonical_output_root: project.canonical_output_root,
            entry_module: project.entry_module,
            loader,
            compiler_options,
        };
        (if project.expose_to_compiler_ipc {
            catalog.register_startup_project(registration)
        } else {
            catalog.register_startup_project_private(registration)
        })
        .map_err(|error| format!("failed to register owner compiler project: {error}"))?;
    }
    Ok(())
}

/// Reuses the core's one HTTP(S) source-authorizer implementation. This is
/// deliberately constructed before any listener: malformed canonical URLs,
/// origin rules, integrity entries, or limits cannot create a partly live
/// browser or compiler endpoint.
#[cfg(unix)]
fn construct_owner_http_page_policy(
    bootstrap: blueice_ipc::owner_bootstrap::OwnerHttpPolicyBootstrap,
) -> Result<script::http_resource_authorizer::HttpScriptResourcePolicy, String> {
    use blueice_ipc::owner_bootstrap::OwnerHttpOriginRule;
    use script::http_resource_authorizer::{
        HttpScriptIntegrityManifest, HttpScriptResourceLimits, HttpScriptResourceOriginRule,
        HttpScriptResourcePolicy,
    };

    bootstrap.validate().map_err(|error| error.to_string())?;
    let origin_rule = match bootstrap.origin_rule {
        OwnerHttpOriginRule::SameDocumentOrigin => {
            HttpScriptResourceOriginRule::same_document_origin()
        }
        OwnerHttpOriginRule::ExactOrigin(origin) => {
            HttpScriptResourceOriginRule::exact_origin(origin).map_err(|error| error.to_string())?
        }
    };
    let manifest = HttpScriptIntegrityManifest::new(
        bootstrap
            .resources
            .into_iter()
            .map(|resource| (resource.canonical_url, resource.integrity)),
    )
    .map_err(|error| error.to_string())?;
    HttpScriptResourcePolicy::new(origin_rule, manifest, HttpScriptResourceLimits::default())
        .map_err(|error| error.to_string())
}

/// Serves one long-lived BlueJS script connection. Frame parsing lives at the
/// IPC boundary, but every request waits for the owning core session to apply
/// it against its live tab manager. A bad initial handshake gets a structured
/// reply and no DOM request is forwarded.
#[cfg(unix)]
fn serve_script_connection(
    mut stream: UnixStream,
    sender: script::ScriptRequestSender,
    expected_token: &str,
) -> io::Result<()> {
    // An unauthenticated peer must not monopolize this serialized listener
    // indefinitely by connecting without sending a complete Hello frame.
    stream.set_read_timeout(Some(std::time::Duration::from_secs(2)))?;
    let first = blueice_ipc::script::read_script_request(&mut stream)?;
    if !matches!(
        &first,
        blueice_ipc::script::ScriptRequest::Hello { protocol_version, session_token }
            if *protocol_version == blueice_ipc::script::SCRIPT_PROTOCOL_VERSION
                && script_capability_matches(expected_token, session_token)
    ) {
        blueice_ipc::script::write_script_reply(
            &mut stream,
            &blueice_ipc::script::ScriptReply::Error {
                message: "script handshake denied".to_string(),
            },
        )?;
        return Ok(());
    }
    stream.set_read_timeout(None)?;
    blueice_ipc::script::write_script_reply(
        &mut stream,
        &blueice_ipc::script::ScriptReply::HelloAck {
            protocol_version: blueice_ipc::script::SCRIPT_PROTOCOL_VERSION,
        },
    )?;

    let mut next_call_id = 1u64;
    loop {
        let request = match blueice_ipc::script::read_script_request(&mut stream) {
            Ok(request) => request,
            Err(error) if matches!(error.kind(), io::ErrorKind::UnexpectedEof) => return Ok(()),
            Err(error) => return Err(error),
        };
        let blueice_ipc::script::ScriptRequest::Call {
            request_id,
            request,
        } = request
        else {
            blueice_ipc::script::write_script_reply(
                &mut stream,
                &blueice_ipc::script::ScriptReply::Error {
                    message: "script DOM calls require a post-handshake envelope".to_string(),
                },
            )?;
            return Ok(());
        };
        let Some(target) = request.document_target() else {
            blueice_ipc::script::write_script_reply(
                &mut stream,
                &blueice_ipc::script::ScriptReply::Error {
                    message: "nested or target-free script call denied".to_string(),
                },
            )?;
            return Ok(());
        };
        if request_id != next_call_id {
            blueice_ipc::script::write_script_reply(
                &mut stream,
                &blueice_ipc::script::ScriptReply::Error {
                    message: "script call ID is not the next connection ID".to_string(),
                },
            )?;
            return Ok(());
        }
        next_call_id = next_call_id.checked_add(1).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "script call ID space exhausted")
        })?;
        let reply = sender.request(*request)?;
        blueice_ipc::script::write_script_reply(
            &mut stream,
            &blueice_ipc::script::ScriptReply::CallResult {
                request_id,
                target,
                reply: Box::new(reply),
            },
        )?;
    }
}

/// Avoid prefix matches and data-dependent early exits at the capability
/// boundary. Both strings have the same validated fixed length in production.
#[cfg(unix)]
fn script_capability_matches(expected: &str, presented: &str) -> bool {
    if !blueice_ipc::script::valid_script_session_token(presented) {
        return false;
    }
    expected
        .bytes()
        .zip(presented.bytes())
        .fold(0u8, |difference, (left, right)| difference | (left ^ right))
        == 0
}

/// Accepts successive script-host connections. A malformed or disconnected
/// host ends only its own connection; it never tears down the core session.
#[cfg(unix)]
fn serve_script_listener(
    listener: UnixListener,
    sender: script::ScriptRequestSender,
    expected_token: String,
) {
    for stream in listener.incoming() {
        let Ok(stream) = stream else {
            break;
        };
        let _ = serve_script_connection(stream, sender.clone(), &expected_token);
    }
}

/// Serves one native debugger discovery connection. The first-message
/// negotiation belongs at this transport boundary; every later target lookup
/// is forwarded to the core session thread, which owns live tab state.
#[cfg(unix)]
fn serve_debugger_connection(
    mut stream: UnixStream,
    sender: blueice_engine::debugger::DebuggerRequestSender,
    allowed_metadata_capabilities: &blueice_ipc::debugger::DebuggerMetadataCapabilityManifest,
    allow_bounded_values: bool,
) -> io::Result<()> {
    let first = blueice_ipc::debugger::read_debugger_request(&mut stream)?;
    let reply = blueice_ipc::debugger::negotiate_with_values(
        &first,
        allowed_metadata_capabilities,
        allow_bounded_values,
    );
    blueice_ipc::debugger::write_debugger_reply(&mut stream, &reply)?;
    let Some(metadata_session) =
        blueice_ipc::debugger::metadata_session_authorization(&first, &reply)
    else {
        return Ok(());
    };

    loop {
        let request = match blueice_ipc::debugger::read_debugger_request(&mut stream) {
            Ok(request) => request,
            Err(error) if matches!(error.kind(), io::ErrorKind::UnexpectedEof) => return Ok(()),
            Err(error) => return Err(error),
        };
        let reply = sender
            .request_with_metadata_session_authorization(request, metadata_session.clone())?;
        blueice_ipc::debugger::write_debugger_reply(&mut stream, &reply)?;
    }
}

/// Accepts successive debugger peers. A malformed/disconnected peer ends only
/// its connection and never interrupts the owning frontend session.
#[cfg(unix)]
fn serve_debugger_listener(
    listener: UnixListener,
    sender: blueice_engine::debugger::DebuggerRequestSender,
    allowed_metadata_capabilities: blueice_ipc::debugger::DebuggerMetadataCapabilityManifest,
    allow_bounded_values: bool,
) {
    for stream in listener.incoming() {
        let Ok(stream) = stream else {
            break;
        };
        let _ = serve_debugger_connection(
            stream,
            sender.clone(),
            &allowed_metadata_capabilities,
            allow_bounded_values,
        );
    }
}

/// Serves one query-only registered-project compiler peer. Its `Hello`
/// negotiation is intentionally completed on the listener side, while every
/// later decoded request is synchronously handed to the sealed core catalog on
/// the session thread under its core-minted stream attestation. Abandoned
/// pagination cursors are revoked when this stream closes. The worker owns no
/// source, project registration, or incremental compiler cache.
#[cfg(unix)]
fn serve_compiler_connection(
    mut stream: UnixStream,
    sender: CompilerServiceIpcRequestSender,
) -> io::Result<()> {
    let first = blueice_ipc::compiler::read_compiler_request(&mut stream)?;
    let accepted = matches!(
        first,
        blueice_ipc::compiler::CompilerRequest::Hello {
            protocol_version: blueice_ipc::compiler::COMPILER_PROTOCOL_VERSION,
        }
    );
    let session_evidence = accepted
        .then(mint_compiler_session_hello_evidence)
        .transpose()?;
    let session_sender = session_evidence
        .as_ref()
        .map(|evidence| sender.bind_session(evidence.session_attestation.clone()))
        .transpose()?;
    let reply = blueice_ipc::compiler::negotiate(&first, session_evidence);
    blueice_ipc::compiler::write_compiler_reply(&mut stream, &reply)?;
    if !accepted {
        return Ok(());
    }
    let session_sender = session_sender.expect("an accepted Hello mints a bound compiler stream");

    loop {
        let request = match blueice_ipc::compiler::read_compiler_request(&mut stream) {
            Ok(request) => request,
            Err(error) if matches!(error.kind(), io::ErrorKind::UnexpectedEof) => return Ok(()),
            Err(error) => return Err(error),
        };
        let reply = session_sender.request(request)?;
        blueice_ipc::compiler::write_compiler_reply(&mut stream, &reply)?;
    }
}

/// Mints all handshake evidence for one accepted compiler stream. The core
/// creates it only after the exact v6 `Hello`: an opaque per-stream
/// attestation and the canonical fixed query-only manifest. Neither is tied
/// to a project, source graph, catalog, path, or any extra authority.
#[cfg(unix)]
fn mint_compiler_session_hello_evidence(
) -> io::Result<blueice_ipc::compiler::CompilerSessionHelloEvidence> {
    let mut bytes = [0u8; 32];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    let mut id = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write;
        write!(id, "{byte:02x}").expect("writing to a String cannot fail");
    }
    let session_attestation = blueice_ipc::compiler::CompilerSessionAttestation { id };
    debug_assert!(session_attestation.is_well_formed());
    let capability_manifest =
        blueice_ipc::compiler::CompilerSessionCapabilityManifest::fixed_query_only();
    debug_assert!(capability_manifest.is_well_formed());
    Ok(blueice_ipc::compiler::CompilerSessionHelloEvidence {
        session_attestation,
        capability_manifest,
    })
}

/// Accepts successive compiler query peers. Bad handshakes and disconnected
/// peers affect only their own stream; they cannot tear down the frontend or
/// alter the catalog registered by core startup.
#[cfg(unix)]
fn serve_compiler_listener(listener: UnixListener, sender: CompilerServiceIpcRequestSender) {
    for stream in listener.incoming() {
        let Ok(stream) = stream else {
            break;
        };
        let _ = serve_compiler_connection(stream, sender.clone());
    }
}

/// Binds either private capability-bearing listener with an explicit
/// owner-only filesystem mode. A bearer token or opaque project ID is not a
/// reason to rely on a permissive ambient umask. Existing live or non-socket
/// paths are preserved; only an abandoned socket inode can be reclaimed.
#[cfg(unix)]
fn bind_owner_only_listener(path: &std::path::Path, label: &str) -> io::Result<UnixListener> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_socket() => match UnixStream::connect(path) {
            // Never unlink a working peer merely because a second core was
            // pointed at its endpoint.  The launcher preflights this too, but
            // direct core invocation must preserve the same boundary.
            Ok(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::AddrInUse,
                    format!("{label} socket is already active: {}", path.display()),
                ));
            }
            // This is the one recoverable startup residue: a dead core can
            // leave its socket inode behind after a forceful stop.
            Err(error) if error.kind() == io::ErrorKind::ConnectionRefused => {
                remove_owned_socket_if_owned(path);
            }
            Err(error) => return Err(error),
        },
        Ok(_) => {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!(
                    "{label} socket is occupied by a non-socket path: {}",
                    path.display()
                ),
            ));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let listener = UnixListener::bind(path)?;
    if let Err(error) = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)) {
        remove_owned_socket_if_owned(path);
        return Err(error);
    }
    Ok(listener)
}

#[cfg(unix)]
fn bind_compiler_listener(path: &std::path::Path) -> io::Result<UnixListener> {
    bind_owner_only_listener(path, "compiler")
}

#[cfg(unix)]
fn bind_script_listener(path: &std::path::Path) -> io::Result<UnixListener> {
    bind_owner_only_listener(path, "script")
}

/// Removes a private listener endpoint only if it is still a Unix socket.
/// The core may be force-killed by its supervisor, but lifecycle cleanup must
/// never unlink a regular file, directory, or symlink that has appeared at a
/// caller-selected path since the listener was created.
#[cfg(unix)]
fn remove_owned_socket_if_owned(path: &std::path::Path) {
    let Ok(metadata) = std::fs::symlink_metadata(path) else {
        return;
    };
    if metadata.file_type().is_socket() {
        let _ = std::fs::remove_file(path);
    }
}

#[cfg(unix)]
fn main() -> ExitCode {
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

    let script_listener = match script_socket.as_ref() {
        Some(path) => match bind_script_listener(path) {
            Ok(listener) => Some(listener),
            Err(error) => {
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
                eprintln!(
                    "blueice-core: failed to bind compiler socket {}: {error}",
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
    let _ = std::fs::remove_file(&args.socket);

    let listener = match UnixListener::bind(&args.socket) {
        Ok(listener) => listener,
        Err(e) => {
            if let Some(path) = &script_socket {
                remove_owned_socket_if_owned(path);
            }
            if let Some(path) = &debugger_socket {
                let _ = std::fs::remove_file(path);
            }
            if let Some(path) = &compiler_socket {
                remove_owned_socket_if_owned(path);
            }
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
            thread::spawn(move || serve_compiler_listener(listener, compiler_sender));
        }
        let (mut stream, _) = listener.accept()?;
        let mut tabs = TabManager::new(args.width, args.height);
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
                },
            )
        }
    })();

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
fn main() {
    eprintln!("blueice-core is currently supported only on Unix platforms");
    std::process::exit(1);
}

#[cfg(all(test, unix))]
#[path = "blueice-core/tests.rs"]
mod tests;
