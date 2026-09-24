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
//! support in this reference implementation.

use blueice_engine::downloads_page::DownloadsSource;
use blueice_engine::gatekeeper_settings_page::GatekeeperSettingsSource;
use blueice_engine::script::ScriptSession;
use blueice_engine::session::ExtensionPageRequest;
use blueice_engine::{session, HistorySnapshotMode, TabManager};
use blueice_extension_host::{
    handle_extension_connection_with_actions_and_authentication_and_network_rules,
    load_installed_extension, registry_for_installed_extension, ExtensionActionDelegates,
    ExtensionConnectionAuthentication, ExtensionRegistry, ExtensionStorage,
};
use blueice_ipc::extension::{ExtensionRuntimeEvent, NetworkResponseInfo};
use std::io::Read;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitCode};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    mpsc, Arc, Mutex,
};
use std::thread;
use std::time::Duration;

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
    /// Where `about:downloads` reads the downloads list from, and where a
    /// downloads process started on the page's behalf listens -- `None`
    /// (the common case) resolves to `blueice_ipc::downloads::
    /// default_downloads_socket_path()`. Overridable for the same reason
    /// `gatekeeper_socket` is: a test points a real subprocess at its own.
    downloads_socket: Option<PathBuf>,
    /// The private long-lived BlueJS control/DOM socket. When supplied,
    /// `core` waits for the script process handshake before accepting its
    /// frontend client, so a page's first render can never race parser script.
    script_socket: Option<PathBuf>,
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
}

struct ExtensionService {
    socket: PathBuf,
    listener: UnixListener,
    registry: Arc<ExtensionRegistry>,
    storage: ExtensionStorage,
    required_authentication: Option<String>,
    runtime_start: Option<Arc<Mutex<mpsc::Receiver<()>>>>,
    runtime_events: Option<Arc<Mutex<mpsc::Receiver<ExtensionRuntimeEvent>>>>,
}

/// A core-owned response must be prompt enough not to hold an extension
/// connection forever if the frontend session has already ended, while still
/// comfortably exceeding the session loop's 25ms poll interval.
const EXTENSION_CORE_REQUEST_TIMEOUT: Duration = Duration::from_secs(1);

/// An extension cannot select or reuse this identifier. It connects a private
/// socket's short-lived declarative network rules to precisely that socket's
/// cleanup path, independent of the package's public extension identity.
static NEXT_EXTENSION_CONNECTION_ID: AtomicU64 = AtomicU64::new(1);

/// Core gives each host child a fresh 256-bit credential. This binary is Unix
/// only (it already uses Unix-domain sockets), so the kernel CSPRNG is the
/// appropriate local source and avoids persisting a credential in either the
/// package manifest or a temporary file.
fn new_extension_authentication() -> Result<String, String> {
    let mut random = [0_u8; 32];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut source| source.read_exact(&mut random))
        .map_err(|error| {
            format!("could not obtain extension-host authentication entropy: {error}")
        })?;
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(random.len() * 2);
    for byte in random {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    Ok(encoded)
}

/// Starts the BlueIce-owned host with its connection credential in the child
/// environment only. In particular, the secret is never placed on the command
/// line, in the manifest, or in a listener response.
fn spawn_extension_host(
    executable: &Path,
    socket: &Path,
    manifest: &Path,
    authentication: &str,
) -> Result<Child, String> {
    Command::new(executable)
        .arg("--connect")
        .arg(socket)
        .arg("--manifest")
        .arg(manifest)
        .env("BLUEICE_EXTENSION_AUTH_TOKEN", authentication)
        .spawn()
        .map_err(|error| {
            format!(
                "could not start extension host {}: {error}",
                executable.display()
            )
        })
}

fn stop_extension_host(mut child: Child) {
    // A normal session close first drops the lifecycle-event sender, letting a
    // host blocked in NextRuntimeEvent receive RuntimeEventStreamClosed and
    // exit on its own. Give that bounded shutdown path a brief chance before
    // falling back to process containment for a misbehaving host.
    for _ in 0..10 {
        match child.try_wait() {
            Ok(Some(_)) | Err(_) => return,
            Ok(None) => thread::sleep(Duration::from_millis(10)),
        }
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// Takes an injectable argument iterator (rather than reading
/// `std::env::args()` directly) so every flag-parsing branch is a
/// plain unit test, not something only exercisable by actually
/// spawning the binary -- the subprocess-level integration test in
/// `tests/core_binary.rs` covers `main`'s own process wiring (bind,
/// accept, cleanup) instead, which this function deliberately knows
/// nothing about.
fn parse_args(args: impl Iterator<Item = String>) -> Result<Args, String> {
    let mut socket = None;
    let mut width = 800.0;
    let mut height = 600.0;
    let mut frame_dir = None;
    let mut gatekeeper_socket = None;
    let mut downloads_socket = None;
    let mut script_socket = None;
    let mut history_snapshots = false;
    let mut extension_socket = None;
    let mut extension_manifest = None;
    let mut extension_host = None;

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
            "--downloads-socket" => downloads_socket = Some(PathBuf::from(value()?)),
            "--script-socket" => script_socket = Some(PathBuf::from(value()?)),
            "--history-snapshots" => history_snapshots = true,
            "--extension-socket" => extension_socket = Some(PathBuf::from(value()?)),
            "--extension-manifest" => extension_manifest = Some(PathBuf::from(value()?)),
            "--extension-host" => extension_host = Some(PathBuf::from(value()?)),
            other => return Err(format!("unrecognized argument: {other}")),
        }
    }

    let socket = socket.ok_or_else(|| "--socket <path> is required".to_string())?;
    if extension_socket.is_some() != extension_manifest.is_some() {
        return Err(
            "--extension-socket and --extension-manifest must be supplied together".to_string(),
        );
    }
    if extension_host.is_some() && extension_socket.is_none() {
        return Err(
            "--extension-host requires --extension-socket and --extension-manifest".to_string(),
        );
    }
    Ok(Args {
        socket,
        width,
        height,
        frame_dir,
        gatekeeper_socket,
        downloads_socket,
        script_socket,
        history_snapshots,
        extension_socket,
        extension_manifest,
        extension_host,
    })
}

fn request_tab_representation(
    tx: &mpsc::Sender<ExtensionPageRequest>,
    tab_id: Option<u64>,
) -> Result<String, String> {
    let (reply_tx, reply_rx) = mpsc::channel();
    tx.send(ExtensionPageRequest::ReadRepresentation {
        tab_id,
        reply: reply_tx,
    })
    .map_err(|_| "blueice-core session is no longer available".to_string())?;
    reply_rx
        .recv_timeout(EXTENSION_CORE_REQUEST_TIMEOUT)
        .map_err(|_| "blueice-core did not answer the extension request in time".to_string())?
}

fn request_network_response(
    tx: &mpsc::Sender<ExtensionPageRequest>,
    tab_id: u64,
) -> Result<Option<NetworkResponseInfo>, String> {
    let (reply_tx, reply_rx) = mpsc::channel();
    tx.send(ExtensionPageRequest::ReadNetworkResponse {
        tab_id,
        reply: reply_tx,
    })
    .map_err(|_| "blueice-core session is no longer available".to_string())?;
    reply_rx
        .recv_timeout(EXTENSION_CORE_REQUEST_TIMEOUT)
        .map_err(|_| "blueice-core did not answer the extension request in time".to_string())?
}

fn request_text_input_value(
    tx: &mpsc::Sender<ExtensionPageRequest>,
    tab_id: u64,
    node_id: u64,
    value: String,
) -> Result<(), String> {
    let (reply_tx, reply_rx) = mpsc::channel();
    tx.send(ExtensionPageRequest::SetTextInputValue {
        tab_id,
        node_id,
        value,
        reply: reply_tx,
    })
    .map_err(|_| "blueice-core session is no longer available".to_string())?;
    reply_rx
        .recv_timeout(EXTENSION_CORE_REQUEST_TIMEOUT)
        .map_err(|_| "blueice-core did not answer the extension request in time".to_string())?
}

fn request_checkbox_checked(
    tx: &mpsc::Sender<ExtensionPageRequest>,
    tab_id: u64,
    node_id: u64,
    checked: bool,
) -> Result<(), String> {
    let (reply_tx, reply_rx) = mpsc::channel();
    tx.send(ExtensionPageRequest::SetCheckboxChecked {
        tab_id,
        node_id,
        checked,
        reply: reply_tx,
    })
    .map_err(|_| "blueice-core session is no longer available".to_string())?;
    reply_rx
        .recv_timeout(EXTENSION_CORE_REQUEST_TIMEOUT)
        .map_err(|_| "blueice-core did not answer the extension request in time".to_string())?
}

fn request_radio_checked(
    tx: &mpsc::Sender<ExtensionPageRequest>,
    tab_id: u64,
    node_id: u64,
) -> Result<(), String> {
    let (reply_tx, reply_rx) = mpsc::channel();
    tx.send(ExtensionPageRequest::SetRadioChecked {
        tab_id,
        node_id,
        reply: reply_tx,
    })
    .map_err(|_| "blueice-core session is no longer available".to_string())?;
    reply_rx
        .recv_timeout(EXTENSION_CORE_REQUEST_TIMEOUT)
        .map_err(|_| "blueice-core did not answer the extension request in time".to_string())?
}

fn request_select_option(
    tx: &mpsc::Sender<ExtensionPageRequest>,
    tab_id: u64,
    node_id: u64,
) -> Result<(), String> {
    let (reply_tx, reply_rx) = mpsc::channel();
    tx.send(ExtensionPageRequest::SelectOption {
        tab_id,
        node_id,
        reply: reply_tx,
    })
    .map_err(|_| "blueice-core session is no longer available".to_string())?;
    reply_rx
        .recv_timeout(EXTENSION_CORE_REQUEST_TIMEOUT)
        .map_err(|_| "blueice-core did not answer the extension request in time".to_string())?
}

fn request_textarea_value(
    tx: &mpsc::Sender<ExtensionPageRequest>,
    tab_id: u64,
    node_id: u64,
    value: String,
) -> Result<(), String> {
    let (reply_tx, reply_rx) = mpsc::channel();
    tx.send(ExtensionPageRequest::SetTextareaValue {
        tab_id,
        node_id,
        value,
        reply: reply_tx,
    })
    .map_err(|_| "blueice-core session is no longer available".to_string())?;
    reply_rx
        .recv_timeout(EXTENSION_CORE_REQUEST_TIMEOUT)
        .map_err(|_| "blueice-core did not answer the extension request in time".to_string())?
}

fn request_range_input_value(
    tx: &mpsc::Sender<ExtensionPageRequest>,
    tab_id: u64,
    node_id: u64,
    value: i64,
) -> Result<(), String> {
    let (reply_tx, reply_rx) = mpsc::channel();
    tx.send(ExtensionPageRequest::SetRangeInputValue {
        tab_id,
        node_id,
        value,
        reply: reply_tx,
    })
    .map_err(|_| "blueice-core session is no longer available".to_string())?;
    reply_rx
        .recv_timeout(EXTENSION_CORE_REQUEST_TIMEOUT)
        .map_err(|_| "blueice-core did not answer the extension request in time".to_string())?
}

fn request_network_block_url(
    tx: &mpsc::Sender<ExtensionPageRequest>,
    connection_id: u64,
    url: String,
) -> Result<(), String> {
    let (reply_tx, reply_rx) = mpsc::channel();
    tx.send(ExtensionPageRequest::RegisterNetworkBlockUrl {
        connection_id,
        url,
        reply: reply_tx,
    })
    .map_err(|_| "blueice-core session is no longer available".to_string())?;
    reply_rx
        .recv_timeout(EXTENSION_CORE_REQUEST_TIMEOUT)
        .map_err(|_| "blueice-core did not answer the extension request in time".to_string())?
}

fn clear_network_block_urls(
    tx: &mpsc::Sender<ExtensionPageRequest>,
    connection_id: u64,
) -> Result<(), String> {
    let (reply_tx, reply_rx) = mpsc::channel();
    tx.send(ExtensionPageRequest::ClearNetworkBlockUrls {
        connection_id,
        reply: reply_tx,
    })
    .map_err(|_| "blueice-core session is no longer available".to_string())?;
    reply_rx
        .recv_timeout(EXTENSION_CORE_REQUEST_TIMEOUT)
        .map_err(|_| "blueice-core did not answer the extension request in time".to_string())
}

fn request_toolbar_button(
    tx: &mpsc::Sender<ExtensionPageRequest>,
    connection_id: u64,
    label: String,
) -> Result<(), String> {
    let (reply_tx, reply_rx) = mpsc::channel();
    tx.send(ExtensionPageRequest::SetToolbarButton {
        connection_id,
        label,
        reply: reply_tx,
    })
    .map_err(|_| "blueice-core session is no longer available".to_string())?;
    reply_rx
        .recv_timeout(EXTENSION_CORE_REQUEST_TIMEOUT)
        .map_err(|_| "blueice-core did not answer the extension request in time".to_string())?
}

fn clear_toolbar_button(
    tx: &mpsc::Sender<ExtensionPageRequest>,
    connection_id: u64,
) -> Result<(), String> {
    let (reply_tx, reply_rx) = mpsc::channel();
    tx.send(ExtensionPageRequest::ClearToolbarButton {
        connection_id,
        reply: reply_tx,
    })
    .map_err(|_| "blueice-core session is no longer available".to_string())?;
    reply_rx
        .recv_timeout(EXTENSION_CORE_REQUEST_TIMEOUT)
        .map_err(|_| "blueice-core did not answer the extension request in time".to_string())
}

/// Serves extension connections outside the session thread, but asks that
/// thread for the one piece of real `Page` data Phase 9 currently supports.
/// This keeps a `Page` single-thread-owned just like navigation and frontend
/// IPC do; no mutable DOM state is shared with an extension handler.
fn spawn_extension_listener(
    service: ExtensionService,
    gatekeeper_socket: PathBuf,
    request_tx: mpsc::Sender<ExtensionPageRequest>,
    authenticated_ready: Option<mpsc::Sender<()>>,
) {
    thread::spawn(move || {
        for incoming in service.listener.incoming() {
            let Ok(mut stream) = incoming else { break };
            let registry = Arc::clone(&service.registry);
            let storage = service.storage.clone();
            let gatekeeper_socket = gatekeeper_socket.clone();
            let request_tx = request_tx.clone();
            let required_authentication = service.required_authentication.clone();
            let authenticated_ready = authenticated_ready.clone();
            let runtime_start = service.runtime_start.clone();
            let runtime_events = service.runtime_events.clone();
            thread::spawn(move || {
                let connection_id = NEXT_EXTENSION_CONNECTION_ID.fetch_add(1, Ordering::Relaxed);
                let authentication = match required_authentication.as_deref() {
                    Some(expected) => ExtensionConnectionAuthentication::required(expected),
                    None => ExtensionConnectionAuthentication::unauthenticated(),
                };
                let authentication = match authenticated_ready {
                    Some(ready) => authentication.with_ready_notification(ready),
                    None => authentication,
                };
                let authentication = match runtime_start {
                    Some(receiver) => authentication.with_runtime_start_receiver(receiver),
                    None => authentication,
                };
                let authentication = match runtime_events {
                    Some(receiver) => authentication.with_runtime_event_receiver(receiver),
                    None => authentication,
                };
                let read_tx = request_tx.clone();
                let observe_tx = request_tx.clone();
                let write_tx = request_tx.clone();
                let rule_tx = request_tx.clone();
                let clear_tx = request_tx.clone();
                let toolbar_tx = request_tx.clone();
                let toolbar_clear_tx = request_tx.clone();
                let _ =
                    handle_extension_connection_with_actions_and_authentication_and_network_rules(
                        &registry,
                        &gatekeeper_socket,
                        &mut stream,
                        authentication,
                        ExtensionActionDelegates::new(
                            move |tab_id| request_tab_representation(&read_tx, tab_id),
                            move |target,
                                  value,
                                  write_target: &blueice_ipc::extension::DomWriteTarget| {
                                match target {
                                Some((tab_id, node_id)) => match write_target {
                                    blueice_ipc::extension::DomWriteTarget::FormInput {
                                        input_type,
                                    } if input_type.eq_ignore_ascii_case("checkbox") => {
                                        request_checkbox_checked(
                                            &write_tx,
                                            tab_id,
                                            node_id,
                                            value == "true",
                                        )
                                    }
                                    blueice_ipc::extension::DomWriteTarget::FormInput {
                                        input_type,
                                    } if input_type.eq_ignore_ascii_case("radio") => {
                                        request_radio_checked(&write_tx, tab_id, node_id)
                                    }
                                    blueice_ipc::extension::DomWriteTarget::FormInput {
                                        input_type,
                                    } if input_type.eq_ignore_ascii_case("select") => {
                                        request_select_option(&write_tx, tab_id, node_id)
                                    }
                                    blueice_ipc::extension::DomWriteTarget::FormInput {
                                        input_type,
                                    } if input_type.eq_ignore_ascii_case("textarea") => {
                                        request_textarea_value(&write_tx, tab_id, node_id, value)
                                    }
                                    blueice_ipc::extension::DomWriteTarget::FormInput {
                                        input_type,
                                    } if input_type.eq_ignore_ascii_case("range") => {
                                        let value = value.parse::<i64>().map_err(|_| {
                                            "core-backed range input values must be integers"
                                                .to_string()
                                        })?;
                                        request_range_input_value(&write_tx, tab_id, node_id, value)
                                    }
                                    _ => request_text_input_value(&write_tx, tab_id, node_id, value),
                                },
                                None => Err(
                                    "core-backed legacy dom:write has no stable target node; negotiate dom:write version 2 or 3 and use an explicit control operation"
                                        .to_string(),
                                ),
                            }
                            },
                            || {
                                Err(
                                    "core-backed network:intercept needs a declarative rule format; the current extension wire protocol does not carry one"
                                        .to_string(),
                                )
                            },
                            move |url| request_network_block_url(&rule_tx, connection_id, url),
                            move || clear_network_block_urls(&clear_tx, connection_id),
                        )
                        .with_storage(storage)
                        .with_network_observer(move |tab_id| {
                            request_network_response(&observe_tx, tab_id)
                        })
                        .with_toolbar_button(move |label| {
                            request_toolbar_button(&toolbar_tx, connection_id, label)
                        })
                        .with_toolbar_clearer(move || {
                            clear_toolbar_button(&toolbar_clear_tx, connection_id)
                        }),
                    );
                let _ = clear_network_block_urls(&request_tx, connection_id);
                let _ = clear_toolbar_button(&request_tx, connection_id);
            });
        }
    });
}

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
    let downloads_socket = args.downloads_socket;

    // An installed extension is a core concern: validate its package and
    // derive its registry identity before core publishes either socket. When
    // `--extension-host` is supplied, the listener additionally requires the
    // freshly generated credential from exactly that core-spawned child.
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
                    storage: ExtensionStorage::default(),
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

    // Bind the script listener before exposing core's frontend socket. The
    // launcher waits for the latter as its readiness signal, which guarantees
    // its BlueJS child never races this bind/connect sequence.
    let script_listener = if let Some(path) = args.script_socket.as_ref() {
        if let Some(parent) = path.parent() {
            if let Err(error) = std::fs::create_dir_all(parent) {
                if let Some(extension_socket) = args.extension_socket.as_ref() {
                    let _ = std::fs::remove_file(extension_socket);
                }
                eprintln!(
                    "blueice-core: failed to create script socket directory {}: {error}",
                    parent.display()
                );
                return ExitCode::FAILURE;
            }
        }
        let _ = std::fs::remove_file(path);
        match UnixListener::bind(path) {
            Ok(listener) => Some(listener),
            Err(e) => {
                if let Some(extension_socket) = args.extension_socket.as_ref() {
                    let _ = std::fs::remove_file(extension_socket);
                }
                eprintln!(
                    "blueice-core: failed to bind script socket {}: {e}",
                    path.display()
                );
                return ExitCode::FAILURE;
            }
        }
    } else {
        None
    };

    let extension_socket = extension_service
        .as_ref()
        .map(|service| service.socket.clone());
    let (extension_requests, mut extension_host_child) = if let Some(service) = extension_service {
        let (tx, rx) = mpsc::channel();
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
                    if let Some(path) = args.script_socket.as_ref() {
                        let _ = std::fs::remove_file(path);
                    }
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
                    if let Some(path) = args.script_socket.as_ref() {
                        let _ = std::fs::remove_file(path);
                    }
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

    // A stale socket file from a previous run (e.g. one that crashed
    // instead of exiting cleanly) makes bind() fail with AddrInUse
    // even though nothing is actually listening -- remove it first. This
    // happens only after a requested extension host authenticated, so this
    // socket remains the public readiness signal for the complete core setup.
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
            eprintln!(
                "blueice-core: failed to bind {}: {e}",
                args.socket.display()
            );
            return ExitCode::FAILURE;
        }
    };

    let result = (|| -> std::io::Result<()> {
        let mut script = match script_listener {
            Some(listener) => {
                let (stream, _) = listener.accept()?;
                Some(ScriptSession::accept(stream)?)
            }
            None => None,
        };
        let (mut stream, _) = listener.accept()?;
        let history_mode = if args.history_snapshots {
            HistorySnapshotMode::Snapshot
        } else {
            HistorySnapshotMode::Reload
        };
        let mut tabs =
            TabManager::new_with_history_snapshot_mode(args.width, args.height, history_mode);
        tabs.set_downloads_source(Arc::new(match downloads_socket {
            Some(socket) => DownloadsSource::at(socket),
            None => DownloadsSource::new(),
        }));
        tabs.set_gatekeeper_settings_source(Arc::new(GatekeeperSettingsSource::at(
            gatekeeper_socket.clone(),
        )));
        if let Some(runtime_start) = extension_runtime_start.as_ref() {
            // The accepted frontend and its newly constructed session are the
            // earliest point at which a Wasm host request can reach a live
            // `TabManager`. The sender is one-shot: only the authenticated
            // child can consume its paired receiver through RuntimeReady.
            let _ = runtime_start.send(());
        }
        let mut generation = 0u64;
        let result = match (script.as_mut(), extension_requests.as_ref()) {
            (Some(script), Some(extension_requests)) => {
                session::run_session_with_script_and_extension_requests_and_events(
                    &mut tabs,
                    &mut stream,
                    &frame_dir,
                    &mut generation,
                    &gatekeeper_socket,
                    script,
                    Some(extension_requests),
                    extension_runtime_events.as_ref(),
                )
            }
            (Some(script), None) => session::run_session_with_script(
                &mut tabs,
                &mut stream,
                &frame_dir,
                &mut generation,
                &gatekeeper_socket,
                script,
            ),
            (None, Some(extension_requests)) => {
                session::run_session_with_extension_requests_and_events(
                    &mut tabs,
                    &mut stream,
                    &frame_dir,
                    &mut generation,
                    &gatekeeper_socket,
                    extension_requests,
                    extension_runtime_events.as_ref(),
                )
            }
            (None, None) => session::run_session(
                &mut tabs,
                &mut stream,
                &frame_dir,
                &mut generation,
                &gatekeeper_socket,
            ),
        };
        if let Some(script) = script.as_mut() {
            let _ = script.shutdown();
        }
        result
    })();

    // Closing the bounded producer is the normal lifecycle shutdown signal.
    // The authenticated handler turns it into RuntimeEventStreamClosed before
    // the child receives the containment fallback below.
    drop(extension_runtime_events);
    if let Some(child) = extension_host_child.take() {
        stop_extension_host(child);
    }
    let _ = std::fs::remove_file(&args.socket);
    if let Some(path) = args.script_socket.as_ref() {
        let _ = std::fs::remove_file(path);
    }
    if let Some(path) = extension_socket.as_ref() {
        let _ = std::fs::remove_file(path);
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

#[cfg(test)]
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
        assert!(!parsed.history_snapshots);
        assert_eq!(parsed.extension_socket, None);
        assert_eq!(parsed.extension_manifest, None);
        assert_eq!(parsed.extension_host, None);
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
            "--downloads-socket",
            "/tmp/dl.sock",
            "--script-socket",
            "/tmp/js.sock",
            "--history-snapshots",
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
                downloads_socket: Some(PathBuf::from("/tmp/dl.sock")),
                script_socket: Some(PathBuf::from("/tmp/js.sock")),
                history_snapshots: true,
                extension_socket: None,
                extension_manifest: None,
                extension_host: None,
            }
        );
    }

    #[test]
    fn extension_socket_and_manifest_are_an_atomic_configuration() {
        assert_eq!(
            args(&[
                "--socket",
                "/tmp/x.sock",
                "--extension-socket",
                "/tmp/ext.sock"
            ]),
            Err(
                "--extension-socket and --extension-manifest must be supplied together".to_string()
            )
        );
        assert_eq!(
            args(&[
                "--socket",
                "/tmp/x.sock",
                "--extension-manifest",
                "/tmp/extension.json"
            ]),
            Err(
                "--extension-socket and --extension-manifest must be supplied together".to_string()
            )
        );
        let parsed = args(&[
            "--socket",
            "/tmp/x.sock",
            "--extension-socket",
            "/tmp/ext.sock",
            "--extension-manifest",
            "/tmp/extension.json",
        ])
        .unwrap();
        assert_eq!(
            parsed.extension_socket,
            Some(PathBuf::from("/tmp/ext.sock"))
        );
        assert_eq!(
            parsed.extension_manifest,
            Some(PathBuf::from("/tmp/extension.json"))
        );
        assert_eq!(parsed.extension_host, None);
    }

    #[test]
    fn extension_host_is_available_only_for_a_complete_installed_extension() {
        assert_eq!(
            args(&[
                "--socket",
                "/tmp/x.sock",
                "--extension-host",
                "/tmp/blueice-extension-host",
            ]),
            Err(
                "--extension-host requires --extension-socket and --extension-manifest".to_string()
            )
        );
        let parsed = args(&[
            "--socket",
            "/tmp/x.sock",
            "--extension-socket",
            "/tmp/ext.sock",
            "--extension-manifest",
            "/tmp/extension.json",
            "--extension-host",
            "/tmp/blueice-extension-host",
        ])
        .unwrap();
        assert_eq!(
            parsed.extension_host,
            Some(PathBuf::from("/tmp/blueice-extension-host"))
        );
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
