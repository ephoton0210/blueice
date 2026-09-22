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
use std::io;
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
    /// Optional listener for the native debugger discovery channel. It remains
    /// separate from both frontend and DOM-script IPC; the session thread
    /// validates each requested tab/document generation before replying.
    debugger_socket: Option<PathBuf>,
    /// Optional listener for queries over projects a trusted core owner
    /// registered during startup. Its protocol does not accept registration,
    /// source, path, resolver, compiler-option, build, or write requests.
    compiler_socket: Option<PathBuf>,
    /// A compiled-in closed project profile selected by the core process
    /// owner. This is a startup-only test/integration seam, not a project
    /// file/path argument and never crosses compiler IPC.
    compiler_project_profile: Option<String>,
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

/// Takes an injectable argument iterator (rather than reading
/// `std::env::args()` directly) so every flag-parsing branch is a
/// plain unit test, not something only exercisable by actually
/// spawning the binary -- the subprocess-level integration test in
/// `tests/core_binary.rs` covers `main`'s own process wiring (bind,
/// accept, cleanup) instead, which this function deliberately knows
/// nothing about.
#[cfg(unix)]
fn parse_args(args: impl Iterator<Item = String>) -> Result<Args, String> {
    let mut socket = None;
    let mut width = 800.0;
    let mut height = 600.0;
    let mut frame_dir = None;
    let mut gatekeeper_socket = None;
    let mut script_socket = None;
    let mut debugger_socket = None;
    let mut compiler_socket = None;
    let mut compiler_project_profile = None;
    let mut inline_bluets_profile = None;
    let mut inline_bluejs = false;
    let mut out_of_process_bluejs_socket = None;
    let mut out_of_process_bluejs_token = None;
    let mut out_of_process_bluejs_page_script_profile = None;

    let mut it = args;
    while let Some(flag) = it.next() {
        let mut value = || it.next().ok_or_else(|| format!("{flag} requires a value"));
        match flag.as_str() {
            "--socket" => socket = Some(PathBuf::from(value()?)),
            "--width" => {
                width = value()?
                    .parse()
                    .map_err(|_| "--width must be a number".to_string())?
            }
            "--height" => {
                height = value()?
                    .parse()
                    .map_err(|_| "--height must be a number".to_string())?
            }
            "--frame-dir" => frame_dir = Some(PathBuf::from(value()?)),
            "--gatekeeper-socket" => gatekeeper_socket = Some(PathBuf::from(value()?)),
            "--script-socket" => script_socket = Some(PathBuf::from(value()?)),
            "--debugger-socket" => debugger_socket = Some(PathBuf::from(value()?)),
            "--compiler-socket" => compiler_socket = Some(PathBuf::from(value()?)),
            "--compiler-project-profile" => compiler_project_profile = Some(value()?),
            "--inline-bluets-profile" => inline_bluets_profile = Some(value()?),
            "--inline-bluejs" => inline_bluejs = true,
            "--out-of-process-bluejs-socket" => {
                out_of_process_bluejs_socket = Some(PathBuf::from(value()?))
            }
            "--out-of-process-bluejs-token" => out_of_process_bluejs_token = Some(value()?),
            "--out-of-process-bluejs-page-script-profile" => {
                out_of_process_bluejs_page_script_profile = Some(value()?)
            }
            other => return Err(format!("unrecognized argument: {other}")),
        }
    }

    let socket = socket.ok_or_else(|| "--socket <path> is required".to_string())?;
    if inline_bluets_profile.is_some() && inline_bluejs {
        return Err("--inline-bluejs cannot be combined with --inline-bluets-profile".to_string());
    }
    if out_of_process_bluejs_socket.is_some() != out_of_process_bluejs_token.is_some() {
        return Err(
            "--out-of-process-bluejs-socket and --out-of-process-bluejs-token must be provided together"
                .to_string(),
        );
    }
    if out_of_process_bluejs_token
        .as_deref()
        .is_some_and(str::is_empty)
    {
        return Err("--out-of-process-bluejs-token must not be empty".to_string());
    }
    if out_of_process_bluejs_page_script_profile.is_some() && out_of_process_bluejs_socket.is_none()
    {
        return Err(
            "--out-of-process-bluejs-page-script-profile requires the private out-of-process BlueJS host"
                .to_string(),
        );
    }
    if let Some(profile) = out_of_process_bluejs_page_script_profile.as_deref() {
        if profile != script::http_resource_authorizer::CORE_HTTP_PAGE_SCRIPT_FIXTURE_PROFILE {
            return Err(
                "--out-of-process-bluejs-page-script-profile must name the fixed core HTTP page-script profile"
                    .to_string(),
            );
        }
    }
    if out_of_process_bluejs_socket.is_some() && (inline_bluets_profile.is_some() || inline_bluejs)
    {
        return Err(
            "--out-of-process-bluejs-socket cannot be combined with --inline-bluejs or --inline-bluets-profile"
                .to_string(),
        );
    }
    if compiler_socket.is_some() != compiler_project_profile.is_some() {
        return Err(
            "--compiler-socket and --compiler-project-profile must be provided together"
                .to_string(),
        );
    }
    Ok(Args {
        socket,
        width,
        height,
        frame_dir,
        gatekeeper_socket,
        script_socket,
        debugger_socket,
        compiler_socket,
        compiler_project_profile,
        inline_bluets_profile,
        inline_bluejs,
        out_of_process_bluejs_socket,
        out_of_process_bluejs_token,
        out_of_process_bluejs_page_script_profile,
    })
}

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

/// Serves one long-lived BlueJS script connection. Frame parsing lives at the
/// IPC boundary, but every request waits for the owning core session to apply
/// it against its live tab manager. A bad initial handshake gets a structured
/// reply and no DOM request is forwarded.
#[cfg(unix)]
fn serve_script_connection(
    mut stream: UnixStream,
    sender: script::ScriptRequestSender,
) -> io::Result<()> {
    let first = blueice_ipc::script::read_script_request(&mut stream)?;
    if !matches!(first, blueice_ipc::script::ScriptRequest::Hello) {
        blueice_ipc::script::write_script_reply(
            &mut stream,
            &blueice_ipc::script::ScriptReply::Error {
                message: "script protocol requires Hello as its first request".to_string(),
            },
        )?;
        return Ok(());
    }
    blueice_ipc::script::write_script_reply(&mut stream, &sender.request(first)?)?;

    loop {
        let request = match blueice_ipc::script::read_script_request(&mut stream) {
            Ok(request) => request,
            Err(error) if matches!(error.kind(), io::ErrorKind::UnexpectedEof) => return Ok(()),
            Err(error) => return Err(error),
        };
        let reply = sender.request(request)?;
        blueice_ipc::script::write_script_reply(&mut stream, &reply)?;
    }
}

/// Accepts successive script-host connections. A malformed or disconnected
/// host ends only its own connection; it never tears down the core session.
#[cfg(unix)]
fn serve_script_listener(listener: UnixListener, sender: script::ScriptRequestSender) {
    for stream in listener.incoming() {
        let Ok(stream) = stream else {
            break;
        };
        let _ = serve_script_connection(stream, sender.clone());
    }
}

/// Serves one native debugger discovery connection. The first-message
/// negotiation belongs at this transport boundary; every later target lookup
/// is forwarded to the core session thread, which owns live tab state.
#[cfg(unix)]
fn serve_debugger_connection(
    mut stream: UnixStream,
    sender: blueice_engine::debugger::DebuggerRequestSender,
) -> io::Result<()> {
    let first = blueice_ipc::debugger::read_debugger_request(&mut stream)?;
    let accepted = matches!(
        first,
        blueice_ipc::debugger::DebuggerRequest::Hello {
            protocol_version: blueice_ipc::debugger::DEBUGGER_PROTOCOL_VERSION,
        }
    );
    let reply = blueice_ipc::debugger::negotiate(&first);
    blueice_ipc::debugger::write_debugger_reply(&mut stream, &reply)?;
    if !accepted {
        return Ok(());
    }

    loop {
        let request = match blueice_ipc::debugger::read_debugger_request(&mut stream) {
            Ok(request) => request,
            Err(error) if matches!(error.kind(), io::ErrorKind::UnexpectedEof) => return Ok(()),
            Err(error) => return Err(error),
        };
        let reply = sender.request(request)?;
        blueice_ipc::debugger::write_debugger_reply(&mut stream, &reply)?;
    }
}

/// Accepts successive debugger peers. A malformed/disconnected peer ends only
/// its connection and never interrupts the owning frontend session.
#[cfg(unix)]
fn serve_debugger_listener(
    listener: UnixListener,
    sender: blueice_engine::debugger::DebuggerRequestSender,
) {
    for stream in listener.incoming() {
        let Ok(stream) = stream else {
            break;
        };
        let _ = serve_debugger_connection(stream, sender.clone());
    }
}

/// Serves one query-only registered-project compiler peer. Its `Hello`
/// negotiation is intentionally completed on the listener side, while every
/// later decoded request is synchronously handed to the sealed core catalog on
/// the session thread. The worker owns no source, project registration, or
/// incremental compiler cache.
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
    let reply = blueice_ipc::compiler::negotiate(&first);
    blueice_ipc::compiler::write_compiler_reply(&mut stream, &reply)?;
    if !accepted {
        return Ok(());
    }

    loop {
        let request = match blueice_ipc::compiler::read_compiler_request(&mut stream) {
            Ok(request) => request,
            Err(error) if matches!(error.kind(), io::ErrorKind::UnexpectedEof) => return Ok(()),
            Err(error) => return Err(error),
        };
        let reply = sender.request(request)?;
        blueice_ipc::compiler::write_compiler_reply(&mut stream, &reply)?;
    }
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

/// Binds the compiler control socket with an explicit owner-only filesystem
/// mode. Opaque project IDs are not an authorization replacement, and a
/// process that chooses to expose static compiler metadata must not rely on a
/// permissive ambient umask to keep arbitrary local users off the listener.
#[cfg(unix)]
fn bind_compiler_listener(path: &std::path::Path) -> io::Result<UnixListener> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_socket() => match UnixStream::connect(path) {
            // Never unlink a working peer merely because a second core was
            // pointed at its endpoint.  The launcher preflights this too, but
            // direct core invocation must preserve the same boundary.
            Ok(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::AddrInUse,
                    format!("compiler socket is already active: {}", path.display()),
                ));
            }
            // This is the one recoverable startup residue: a dead core can
            // leave its socket inode behind after a forceful stop.
            Err(error) if error.kind() == io::ErrorKind::ConnectionRefused => {
                remove_compiler_socket_if_owned(path);
            }
            Err(error) => return Err(error),
        },
        Ok(_) => {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!(
                    "compiler socket is occupied by a non-socket path: {}",
                    path.display()
                ),
            ));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let listener = UnixListener::bind(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(listener)
}

/// Removes a compiler listener endpoint only if it is still a Unix socket.
/// The core may be force-killed by its supervisor, but lifecycle cleanup must
/// never unlink a regular file, directory, or symlink that has appeared at a
/// caller-selected path since the listener was created.
#[cfg(unix)]
fn remove_compiler_socket_if_owned(path: &std::path::Path) {
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
    let debugger_socket = args.debugger_socket.clone();
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
    let mut compiler_service = compiler_catalog.map(CoreCompilerProjectCatalog::seal);

    let script_listener = match script_socket.as_ref() {
        Some(path) => {
            let _ = std::fs::remove_file(path);
            match UnixListener::bind(path) {
                Ok(listener) => Some(listener),
                Err(error) => {
                    eprintln!(
                        "blueice-core: failed to bind script socket {}: {error}",
                        path.display()
                    );
                    return ExitCode::FAILURE;
                }
            }
        }
        None => None,
    };
    let debugger_listener = match debugger_socket.as_ref() {
        Some(path) => {
            let _ = std::fs::remove_file(path);
            match UnixListener::bind(path) {
                Ok(listener) => Some(listener),
                Err(error) => {
                    if let Some(path) = &script_socket {
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
                    let _ = std::fs::remove_file(path);
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
                let _ = std::fs::remove_file(path);
            }
            if let Some(path) = &debugger_socket {
                let _ = std::fs::remove_file(path);
            }
            if let Some(path) = &compiler_socket {
                remove_compiler_socket_if_owned(path);
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
            thread::spawn(move || serve_script_listener(listener, script_sender));
        }
        if let Some(listener) = debugger_listener {
            thread::spawn(move || serve_debugger_listener(listener, debugger_sender));
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
            let mut javascript_executor = match out_of_process_bluejs_page_script_profile.as_deref()
            {
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
        let _ = std::fs::remove_file(path);
    }
    if let Some(path) = debugger_socket {
        let _ = std::fs::remove_file(path);
    }
    if let Some(path) = compiler_socket {
        remove_compiler_socket_if_owned(&path);
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
mod tests {
    use super::*;

    fn args(flags: &[&str]) -> Result<Args, String> {
        parse_args(flags.iter().map(|s| s.to_string()))
    }

    #[test]
    fn socket_is_required() {
        assert_eq!(args(&[]), Err("--socket <path> is required".to_string()));
    }

    #[test]
    fn socket_alone_uses_default_width_height_and_frame_dir() {
        let parsed = args(&["--socket", "/tmp/x.sock"]).unwrap();
        assert_eq!(parsed.socket, PathBuf::from("/tmp/x.sock"));
        assert_eq!(parsed.width, 800.0);
        assert_eq!(parsed.height, 600.0);
        assert_eq!(parsed.frame_dir, None);
        assert_eq!(parsed.gatekeeper_socket, None);
        assert_eq!(parsed.script_socket, None);
        assert_eq!(parsed.debugger_socket, None);
        assert_eq!(parsed.compiler_socket, None);
        assert_eq!(parsed.compiler_project_profile, None);
        assert_eq!(parsed.inline_bluets_profile, None);
        assert!(!parsed.inline_bluejs);
        assert_eq!(parsed.out_of_process_bluejs_socket, None);
        assert_eq!(parsed.out_of_process_bluejs_token, None);
        assert_eq!(parsed.out_of_process_bluejs_page_script_profile, None);
    }

    #[test]
    fn every_flag_is_parsed() {
        let parsed = args(&[
            "--socket",
            "/tmp/x.sock",
            "--width",
            "100",
            "--height",
            "50",
            "--frame-dir",
            "/tmp/frames",
            "--gatekeeper-socket",
            "/tmp/gk.sock",
            "--script-socket",
            "/tmp/script.sock",
            "--debugger-socket",
            "/tmp/debugger.sock",
            "--compiler-socket",
            "/tmp/compiler.sock",
            "--compiler-project-profile",
            "core-closed-fixture-v1",
            "--inline-bluets-profile",
            "core-script-document-text-v1",
        ])
        .unwrap();
        assert_eq!(
            parsed,
            Args {
                socket: PathBuf::from("/tmp/x.sock"),
                width: 100.0,
                height: 50.0,
                frame_dir: Some(PathBuf::from("/tmp/frames")),
                gatekeeper_socket: Some(PathBuf::from("/tmp/gk.sock")),
                script_socket: Some(PathBuf::from("/tmp/script.sock")),
                debugger_socket: Some(PathBuf::from("/tmp/debugger.sock")),
                compiler_socket: Some(PathBuf::from("/tmp/compiler.sock")),
                compiler_project_profile: Some("core-closed-fixture-v1".to_string()),
                inline_bluets_profile: Some("core-script-document-text-v1".to_string()),
                inline_bluejs: false,
                out_of_process_bluejs_socket: None,
                out_of_process_bluejs_token: None,
                out_of_process_bluejs_page_script_profile: None,
            }
        );
    }

    #[test]
    fn inline_javascript_host_is_opt_in_and_cannot_share_a_session_with_bluets() {
        assert!(
            args(&["--socket", "/tmp/x.sock", "--inline-bluejs"])
                .unwrap()
                .inline_bluejs
        );
        assert_eq!(
            args(&[
                "--socket",
                "/tmp/x.sock",
                "--inline-bluejs",
                "--inline-bluets-profile",
                "core-script-document-text-v1",
            ]),
            Err("--inline-bluejs cannot be combined with --inline-bluets-profile".to_string())
        );
    }

    #[test]
    fn out_of_process_javascript_host_requires_an_explicit_complete_trusted_endpoint() {
        let parsed = args(&[
            "--socket",
            "/tmp/x.sock",
            "--out-of-process-bluejs-socket",
            "/tmp/bluejs-host.sock",
            "--out-of-process-bluejs-token",
            "launcher-issued-capability",
        ])
        .unwrap();
        assert_eq!(
            parsed.out_of_process_bluejs_socket,
            Some(PathBuf::from("/tmp/bluejs-host.sock"))
        );
        assert_eq!(
            parsed.out_of_process_bluejs_token.as_deref(),
            Some("launcher-issued-capability")
        );
        assert_eq!(
            args(&[
                "--socket",
                "/tmp/x.sock",
                "--out-of-process-bluejs-socket",
                "/tmp/bluejs-host.sock",
            ]),
            Err(
                "--out-of-process-bluejs-socket and --out-of-process-bluejs-token must be provided together"
                    .to_string()
            )
        );
        assert_eq!(
            args(&[
                "--socket",
                "/tmp/x.sock",
                "--out-of-process-bluejs-token",
                "launcher-issued-capability",
            ]),
            Err(
                "--out-of-process-bluejs-socket and --out-of-process-bluejs-token must be provided together"
                    .to_string()
            )
        );
    }

    #[test]
    fn out_of_process_http_profile_is_fixed_and_requires_the_private_host() {
        let profile = script::http_resource_authorizer::CORE_HTTP_PAGE_SCRIPT_FIXTURE_PROFILE;
        assert_eq!(
            args(&[
                "--socket",
                "/tmp/x.sock",
                "--out-of-process-bluejs-page-script-profile",
                profile,
            ]),
            Err(
                "--out-of-process-bluejs-page-script-profile requires the private out-of-process BlueJS host"
                    .to_string()
            )
        );
        assert_eq!(
            args(&[
                "--socket",
                "/tmp/x.sock",
                "--out-of-process-bluejs-socket",
                "/tmp/bluejs-host.sock",
                "--out-of-process-bluejs-token",
                "launcher-issued-capability",
                "--out-of-process-bluejs-page-script-profile",
                "https://page.example.test/not-a-profile.js",
            ]),
            Err(
                "--out-of-process-bluejs-page-script-profile must name the fixed core HTTP page-script profile"
                    .to_string()
            )
        );
        assert_eq!(
            args(&[
                "--socket",
                "/tmp/x.sock",
                "--out-of-process-bluejs-socket",
                "/tmp/bluejs-host.sock",
                "--out-of-process-bluejs-token",
                "launcher-issued-capability",
                "--out-of-process-bluejs-page-script-profile",
                profile,
            ])
            .unwrap()
            .out_of_process_bluejs_page_script_profile
            .as_deref(),
            Some(profile)
        );
    }

    #[test]
    fn out_of_process_javascript_host_cannot_share_a_page_with_other_executors() {
        assert_eq!(
            args(&[
                "--socket",
                "/tmp/x.sock",
                "--inline-bluejs",
                "--out-of-process-bluejs-socket",
                "/tmp/bluejs-host.sock",
                "--out-of-process-bluejs-token",
                "launcher-issued-capability",
            ]),
            Err(
                "--out-of-process-bluejs-socket cannot be combined with --inline-bluejs or --inline-bluets-profile"
                    .to_string()
            )
        );
    }

    #[test]
    fn compiler_profile_and_query_listener_are_an_indivisible_startup_pair() {
        assert_eq!(
            args(&[
                "--socket",
                "/tmp/x.sock",
                "--compiler-project-profile",
                "core-closed-fixture-v1",
            ]),
            Err(
                "--compiler-socket and --compiler-project-profile must be provided together"
                    .to_string()
            )
        );
        assert_eq!(
            args(&[
                "--socket",
                "/tmp/x.sock",
                "--compiler-socket",
                "/tmp/compiler.sock",
            ]),
            Err(
                "--compiler-socket and --compiler-project-profile must be provided together"
                    .to_string()
            )
        );
        let parsed = args(&[
            "--socket",
            "/tmp/x.sock",
            "--compiler-socket",
            "/tmp/compiler.sock",
            "--compiler-project-profile",
            "core-closed-fixture-v1",
        ])
        .unwrap();
        assert_eq!(
            parsed.compiler_project_profile.as_deref(),
            Some("core-closed-fixture-v1")
        );
    }

    #[test]
    fn compiler_startup_accepts_only_the_compiled_in_closed_profile() {
        let mut catalog = CoreCompilerProjectCatalog::default();
        assert!(
            register_compiler_startup_profile(&mut catalog, "../untrusted-project")
                .unwrap_err()
                .contains("unsupported compiler project profile")
        );
        assert_eq!(catalog.registered_project_count(), 0);
        register_compiler_startup_profile(&mut catalog, "core-closed-fixture-v1").unwrap();
        assert_eq!(catalog.registered_project_count(), 1);
    }

    #[test]
    fn compiler_listener_does_not_unlink_an_active_owner_selected_endpoint() {
        let path = PathBuf::from("/tmp").join(format!(
            "blueice-core-active-compiler-{}-{}.sock",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let listener = UnixListener::bind(&path).unwrap();

        let error = bind_compiler_listener(&path).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AddrInUse);
        assert!(path.exists(), "the active endpoint must remain reachable");

        drop(listener);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn a_flag_missing_its_value_is_an_error() {
        assert_eq!(
            args(&["--socket"]),
            Err("--socket requires a value".to_string())
        );
    }

    #[test]
    fn a_non_numeric_width_is_an_error() {
        assert_eq!(
            args(&["--socket", "/tmp/x.sock", "--width", "not-a-number"]),
            Err("--width must be a number".to_string())
        );
    }

    #[test]
    fn a_non_numeric_height_is_an_error() {
        assert_eq!(
            args(&["--socket", "/tmp/x.sock", "--height", "not-a-number"]),
            Err("--height must be a number".to_string())
        );
    }

    #[test]
    fn an_unrecognized_flag_is_an_error() {
        assert_eq!(
            args(&["--bogus"]),
            Err("unrecognized argument: --bogus".to_string())
        );
    }
}
