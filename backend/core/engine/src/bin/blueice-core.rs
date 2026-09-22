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
//! support in this reference implementation. An optional second Unix socket
//! routes the narrow, long-lived BlueJS script protocol into that same session
//! thread; its listener never owns DOM or tab state itself. A third,
//! optional Unix socket accepts `blueice_ipc::automation` connections --
//! unlike the script socket, *many* may be connected simultaneously
//! (two automation observers watching the same shared `TabManager` is
//! the whole point of `phase-17-automation-devtools-and-ajax/PLAN.md`'s
//! Slice 1 item 3), so each accepted connection is served on its own
//! thread rather than one at a time.

#[cfg(unix)]
use blueice_engine::automation_service::{
    self, AutomationConnectionIdAllocator, AutomationRequestSenderFactory, AutomationRequests,
    AutomationServiceState,
};
#[cfg(unix)]
use blueice_engine::{script, session, TabManager};
#[cfg(unix)]
use std::io;
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
    /// Optional listener for `blueice_ipc::automation` connections
    /// (`phase-17-automation-devtools-and-ajax/PLAN.md`'s Slice 1 item
    /// 3). Unlike `script_socket`, more than one client may connect at
    /// once -- see module docs.
    automation_socket: Option<PathBuf>,
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
    let mut automation_socket = None;

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
            "--automation-socket" => automation_socket = Some(PathBuf::from(value()?)),
            other => return Err(format!("unrecognized argument: {other}")),
        }
    }

    let socket = socket.ok_or_else(|| "--socket <path> is required".to_string())?;
    Ok(Args {
        socket,
        width,
        height,
        frame_dir,
        gatekeeper_socket,
        script_socket,
        automation_socket,
    })
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

/// Serves one automation connection. Its first request must be `Hello`
/// (per `blueice_ipc::automation`'s module docs); anything else gets a
/// structured error reply and the connection is closed without ever
/// reaching the shared session -- the same first-message gate
/// `serve_script_connection` enforces for the script protocol.
#[cfg(unix)]
fn serve_automation_connection(
    mut stream: UnixStream,
    sender: automation_service::AutomationRequestSender,
) -> io::Result<()> {
    let first = blueice_ipc::automation::read_automation_request(&mut stream)?;
    if !matches!(
        first,
        blueice_ipc::automation::AutomationRequest::Hello { .. }
    ) {
        blueice_ipc::automation::write_automation_reply(
            &mut stream,
            &blueice_ipc::automation::AutomationReply::Error(
                blueice_ipc::automation::AutomationError::Unsupported {
                    detail: "automation protocol requires Hello as its first request".to_string(),
                },
            ),
        )?;
        return Ok(());
    }
    blueice_ipc::automation::write_automation_reply(&mut stream, &sender.request(first)?)?;

    loop {
        let request = match blueice_ipc::automation::read_automation_request(&mut stream) {
            Ok(request) => request,
            Err(error) if matches!(error.kind(), io::ErrorKind::UnexpectedEof) => return Ok(()),
            Err(error) => return Err(error),
        };
        let reply = sender.request(request)?;
        blueice_ipc::automation::write_automation_reply(&mut stream, &reply)?;
    }
}

/// Accepts successive automation connections, each on its own thread --
/// unlike `serve_script_listener`, more than one may be live at once
/// (see module docs). A malformed or disconnected client only ever ends
/// its own connection/thread; the shared core session is untouched.
#[cfg(unix)]
fn serve_automation_listener(listener: UnixListener, factory: AutomationRequestSenderFactory) {
    let mut allocator = AutomationConnectionIdAllocator::default();
    for stream in listener.incoming() {
        let Ok(stream) = stream else {
            break;
        };
        let sender = factory.for_connection(allocator.allocate());
        thread::spawn(move || {
            let _ = serve_automation_connection(stream, sender);
        });
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
    let automation_socket = args.automation_socket.clone();

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

    let automation_listener = match automation_socket.as_ref() {
        Some(path) => {
            let _ = std::fs::remove_file(path);
            match UnixListener::bind(path) {
                Ok(listener) => Some(listener),
                Err(error) => {
                    eprintln!(
                        "blueice-core: failed to bind automation socket {}: {error}",
                        path.display()
                    );
                    if let Some(path) = &script_socket {
                        let _ = std::fs::remove_file(path);
                    }
                    return ExitCode::FAILURE;
                }
            }
        }
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
            if let Some(path) = &automation_socket {
                let _ = std::fs::remove_file(path);
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
        if let Some(listener) = script_listener {
            thread::spawn(move || serve_script_listener(listener, script_sender));
        }
        let (automation_factory, automation_requests) =
            automation_service::automation_request_channel();
        if let Some(listener) = automation_listener {
            thread::spawn(move || serve_automation_listener(listener, automation_factory));
        }
        let (mut stream, _) = listener.accept()?;
        let mut tabs = TabManager::new(args.width, args.height);
        let mut generation = 0u64;
        let mut automation_state = AutomationServiceState::default();
        session::run_session_with_script_and_automation_requests(
            &mut tabs,
            &mut stream,
            &frame_dir,
            &mut generation,
            &gatekeeper_socket,
            script_socket.as_ref().map(|_| &script_requests),
            automation_socket.as_ref().map(|_| AutomationRequests {
                receiver: &automation_requests,
                state: &mut automation_state,
            }),
        )
    })();

    let _ = std::fs::remove_file(&args.socket);
    if let Some(path) = script_socket {
        let _ = std::fs::remove_file(path);
    }
    if let Some(path) = automation_socket {
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
        assert_eq!(parsed.automation_socket, None);
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
            "--automation-socket",
            "/tmp/automation.sock",
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
                automation_socket: Some(PathBuf::from("/tmp/automation.sock")),
            }
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
