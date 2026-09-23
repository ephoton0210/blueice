// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `blueice-extension-host`: the process binary. Deliberately thin --
//! all the real logic (`ExtensionRegistry`, `handle_extension_
//! connection`) lives in this crate's own `lib.rs`, already covered by
//! its own unit tests against in-process `UnixStream` pairs; this file
//! is just argument parsing and wiring a real `UnixListener` to that
//! already-tested connection handler, matching how `blueice-core`'s and
//! `blueice-ai-gatekeeper`'s own thin binaries are structured (see
//! those crates' docs). Excluded from the coverage gate the same way
//! (`CLAUDE.md`'s coverage command already excludes `extension/src/
//! main.rs$`) -- covered instead by `tests/extension_host_binary.rs`'s
//! real-subprocess test.
//!
//! In its original `--socket` mode this is a standalone, sequential protocol
//! reference server. In `--connect` mode it is instead the peer launched by
//! `blueice-core`: it validates the selected package, proves the fresh
//! credential that core supplied only through its environment, then runs the
//! installed module's bounded
//! `blueice_start` reactor without WASI or ambient OS authority.

use blueice_extension_host::{
    execute_installed_extension, execute_installed_extension_for_invocation,
    handle_extension_connection_with_gatekeeper, load_installed_extension,
    registry_for_installed_extension, ExtensionRegistry, RuntimeInvocation, CAPABILITY_DOM_READ,
    CAPABILITY_DOM_WRITE, CAPABILITY_NETWORK_INTERCEPT,
};
use blueice_ipc::extension::{
    read_extension_reply, write_extension_request, ExtensionReply, ExtensionRequest,
    ExtensionRuntimeEvent,
};
use blueice_ipc::gatekeeper::default_gatekeeper_socket_path;
use std::collections::BTreeMap;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Debug, PartialEq)]
struct Args {
    mode: Mode,
}

#[derive(Debug, PartialEq)]
enum Mode {
    Serve {
        socket: PathBuf,
        gatekeeper_socket: PathBuf,
        manifest: Option<PathBuf>,
    },
    Connect {
        socket: PathBuf,
        manifest: PathBuf,
    },
}

/// Takes an injectable argument iterator (rather than reading
/// `std::env::args()` directly) so every flag-parsing branch is a plain
/// unit test -- mirrors `blueice-core`'s own `parse_args` for the same
/// reason (see that binary's docs).
fn parse_args(args: impl Iterator<Item = String>) -> Result<Args, String> {
    let mut socket = None;
    let mut connect = None;
    let mut gatekeeper_socket = None;
    let mut manifest = None;

    let mut it = args;
    while let Some(flag) = it.next() {
        let mut value = || it.next().ok_or_else(|| format!("{flag} requires a value"));
        match flag.as_str() {
            "--socket" => socket = Some(PathBuf::from(value()?)),
            "--connect" => connect = Some(PathBuf::from(value()?)),
            "--gatekeeper-socket" => gatekeeper_socket = Some(PathBuf::from(value()?)),
            "--manifest" => manifest = Some(PathBuf::from(value()?)),
            other => return Err(format!("unrecognized argument: {other}")),
        }
    }

    match (socket, connect) {
        (Some(_), Some(_)) => Err("--socket and --connect are mutually exclusive".to_string()),
        (Some(socket), None) => Ok(Args {
            mode: Mode::Serve {
                socket,
                gatekeeper_socket: gatekeeper_socket.unwrap_or_else(default_gatekeeper_socket_path),
                manifest,
            },
        }),
        (None, Some(socket)) => {
            if gatekeeper_socket.is_some() {
                return Err("--gatekeeper-socket is only valid with --socket".to_string());
            }
            let manifest = manifest.ok_or_else(|| {
                "--connect requires --manifest so the host can derive its package identity"
                    .to_string()
            })?;
            Ok(Args {
                mode: Mode::Connect { socket, manifest },
            })
        }
        (None, None) => Err("--socket <path> or --connect <path> is required".to_string()),
    }
}

fn serve(socket: PathBuf, gatekeeper_socket: PathBuf, manifest: Option<PathBuf>) -> ExitCode {
    // Validate the package and populate the server-side registry before
    // publishing a socket. A client must never race a briefly listening host
    // whose identity/capability table has not been established yet.
    let registry = match manifest.as_deref() {
        Some(manifest_path) => match load_installed_extension(manifest_path) {
            Ok(extension) => registry_for_installed_extension(&extension),
            Err(error) => {
                eprintln!(
                    "blueice-extension-host: could not install {}: {error}",
                    manifest_path.display()
                );
                let _ = std::fs::remove_file(&socket);
                return ExitCode::FAILURE;
            }
        },
        None => ExtensionRegistry::minimal_slice(),
    };

    // A stale socket file from a previous run (e.g. one that crashed
    // instead of exiting cleanly) makes bind() fail with AddrInUse even
    // though nothing is actually listening -- remove it first, same as
    // `blueice-core`'s and `blueice-ai-gatekeeper`'s own binaries do.
    if let Some(parent) = socket.parent() {
        if let Err(error) = blueice_ipc::local_socket::ensure_private_socket_dir(parent) {
            eprintln!(
                "blueice-extension-host: failed to prepare private socket directory {}: {error}",
                parent.display()
            );
            return ExitCode::FAILURE;
        }
    }
    let _ = std::fs::remove_file(&socket);

    let listener = match blueice_ipc::local_socket::bind_private_listener(&socket) {
        Ok(listener) => listener,
        Err(e) => {
            eprintln!(
                "blueice-extension-host: failed to bind {}: {e}",
                socket.display()
            );
            return ExitCode::FAILURE;
        }
    };
    for mut stream in listener.incoming().flatten() {
        let _ =
            handle_extension_connection_with_gatekeeper(&registry, &gatekeeper_socket, &mut stream);
    }

    let _ = std::fs::remove_file(&socket);
    ExitCode::SUCCESS
}

/// The core's child-side handshake. The token comes only from core's child
/// environment -- there is intentionally no command-line flag for it, where a
/// process listing or shell history could disclose it.
fn connect_to_core(socket: PathBuf, manifest: PathBuf) -> Result<(), String> {
    let installed = load_installed_extension(&manifest).map_err(|error| {
        format!(
            "could not install {} before connecting to core: {error}",
            manifest.display()
        )
    })?;
    let authentication = std::env::var("BLUEICE_EXTENSION_AUTH_TOKEN").map_err(|_| {
        "BLUEICE_EXTENSION_AUTH_TOKEN is required in --connect mode and must be supplied by blueice-core"
            .to_string()
    })?;
    if authentication.is_empty() {
        return Err("BLUEICE_EXTENSION_AUTH_TOKEN must not be empty".to_string());
    }

    // A package declares only the APIs it needs. The one-shot Wasm ABI uses
    // explicit tab reads (v2) and the two existing bounded write operations
    // (v2/v3), so negotiate the highest safe version per declared capability
    // before guest code can invoke an import. Core remains free to reject an
    // unsupported declaration without granting it any authority.
    let capability_versions: BTreeMap<_, _> = installed
        .manifest()
        .capabilities()
        .declared()
        .iter()
        .map(|capability| {
            let version = match capability.as_str() {
                CAPABILITY_DOM_READ => 2,
                CAPABILITY_DOM_WRITE => 3,
                CAPABILITY_NETWORK_INTERCEPT => 1,
                _ => 1,
            };
            (capability.clone(), version)
        })
        .collect();
    let mut stream = UnixStream::connect(&socket).map_err(|error| {
        format!(
            "could not connect to core extension socket {}: {error}",
            socket.display()
        )
    })?;
    write_extension_request(
        &mut stream,
        &ExtensionRequest::HelloAuthenticated {
            extension_id: installed.extension_id().to_string(),
            capability_versions,
            authentication,
        },
    )
    .map_err(|error| format!("could not send authenticated extension hello: {error}"))?;
    match read_extension_reply(&mut stream)
        .map_err(|error| format!("core rejected the authenticated extension hello: {error}"))?
    {
        ExtensionReply::HelloAck { .. } => {}
        reply => {
            return Err(format!(
                "core returned an unexpected extension hello reply: {reply:?}"
            ))
        }
    }

    // Authentication proves that this is the core-spawned package host, but
    // the core's session does not own a live `TabManager` until a frontend has
    // connected. Wait for its one-shot lifecycle barrier before a guest can
    // issue a page request, avoiding a startup-time fake acknowledgement or
    // one-second session-channel timeout.
    write_extension_request(&mut stream, &ExtensionRequest::RuntimeReady)
        .map_err(|error| format!("could not announce extension runtime readiness: {error}"))?;
    match read_extension_reply(&mut stream)
        .map_err(|error| format!("core did not start the extension runtime: {error}"))?
    {
        ExtensionReply::RuntimeStart => {}
        reply => {
            return Err(format!(
                "core returned an unexpected extension runtime-start reply: {reply:?}"
            ))
        }
    }

    // Each core-defined event receives an entirely fresh resource-bounded
    // Wasm instance. Clone the authenticated socket only for the duration of
    // that invocation, preserving a single request/reply reader afterward.
    execute_installed_extension(
        &installed,
        stream.try_clone().map_err(|error| {
            format!("could not clone the authenticated extension stream for startup: {error}")
        })?,
    )
    .map_err(|error| format!("could not run the installed WASM extension at startup: {error}"))?;

    loop {
        write_extension_request(&mut stream, &ExtensionRequest::NextRuntimeEvent).map_err(
            |error| format!("could not wait for the next core lifecycle event: {error}"),
        )?;
        match read_extension_reply(&mut stream)
            .map_err(|error| format!("core did not provide the next lifecycle event: {error}"))?
        {
            ExtensionReply::RuntimeEvent(ExtensionRuntimeEvent::NavigationCommitted { tab_id }) => {
                let invocation = RuntimeInvocation::NavigationCommitted { tab_id };
                let event_stream = stream.try_clone().map_err(|error| {
                    format!(
                        "could not clone the authenticated extension stream for an event: {error}"
                    )
                })?;
                execute_installed_extension_for_invocation(&installed, event_stream, invocation)
                    .map_err(|error| {
                        format!("could not run the installed WASM extension for navigation event: {error}")
                    })?;
            }
            ExtensionReply::RuntimeEventStreamClosed => return Ok(()),
            reply => {
                return Err(format!(
                    "core returned an unexpected lifecycle event reply: {reply:?}"
                ));
            }
        }
    }
}

fn main() -> ExitCode {
    let args = match parse_args(std::env::args().skip(1)) {
        Ok(args) => args,
        Err(message) => {
            eprintln!("blueice-extension-host: {message}");
            return ExitCode::FAILURE;
        }
    };

    match args.mode {
        Mode::Serve {
            socket,
            gatekeeper_socket,
            manifest,
        } => serve(socket, gatekeeper_socket, manifest),
        Mode::Connect { socket, manifest } => match connect_to_core(socket, manifest) {
            Ok(()) => ExitCode::SUCCESS,
            Err(message) => {
                eprintln!("blueice-extension-host: {message}");
                ExitCode::FAILURE
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(flags: &[&str]) -> Result<Args, String> {
        parse_args(flags.iter().map(|s| s.to_string()))
    }

    #[test]
    fn a_server_socket_or_core_connection_is_required() {
        assert_eq!(
            args(&[]),
            Err("--socket <path> or --connect <path> is required".to_string())
        );
    }

    #[test]
    fn socket_flag_is_parsed() {
        assert_eq!(
            args(&["--socket", "/tmp/x.sock"]).unwrap(),
            Args {
                mode: Mode::Serve {
                    socket: PathBuf::from("/tmp/x.sock"),
                    gatekeeper_socket: default_gatekeeper_socket_path(),
                    manifest: None,
                },
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
    fn a_gatekeeper_socket_override_is_parsed() {
        assert_eq!(
            args(&[
                "--socket",
                "/tmp/x.sock",
                "--gatekeeper-socket",
                "/tmp/gatekeeper.sock"
            ])
            .unwrap(),
            Args {
                mode: Mode::Serve {
                    socket: PathBuf::from("/tmp/x.sock"),
                    gatekeeper_socket: PathBuf::from("/tmp/gatekeeper.sock"),
                    manifest: None,
                },
            }
        );
    }

    #[test]
    fn a_gatekeeper_socket_flag_missing_its_value_is_an_error() {
        assert_eq!(
            args(&["--socket", "/tmp/x.sock", "--gatekeeper-socket"]),
            Err("--gatekeeper-socket requires a value".to_string())
        );
    }

    #[test]
    fn a_manifest_override_is_parsed() {
        assert_eq!(
            args(&[
                "--socket",
                "/tmp/x.sock",
                "--manifest",
                "/tmp/extension.json"
            ])
            .unwrap(),
            Args {
                mode: Mode::Serve {
                    socket: PathBuf::from("/tmp/x.sock"),
                    gatekeeper_socket: default_gatekeeper_socket_path(),
                    manifest: Some(PathBuf::from("/tmp/extension.json")),
                },
            }
        );
    }

    #[test]
    fn core_connection_requires_an_installed_manifest() {
        assert_eq!(
            args(&["--connect", "/tmp/core-extension.sock"]),
            Err(
                "--connect requires --manifest so the host can derive its package identity"
                    .to_string()
            )
        );
        assert_eq!(
            args(&[
                "--connect",
                "/tmp/core-extension.sock",
                "--manifest",
                "/tmp/extension.json",
            ])
            .unwrap(),
            Args {
                mode: Mode::Connect {
                    socket: PathBuf::from("/tmp/core-extension.sock"),
                    manifest: PathBuf::from("/tmp/extension.json"),
                },
            }
        );
    }

    #[test]
    fn server_and_core_connection_modes_cannot_be_combined() {
        assert_eq!(
            args(&[
                "--socket",
                "/tmp/server.sock",
                "--connect",
                "/tmp/core.sock",
            ]),
            Err("--socket and --connect are mutually exclusive".to_string())
        );
    }

    #[test]
    fn a_manifest_flag_missing_its_value_is_an_error() {
        assert_eq!(
            args(&["--socket", "/tmp/x.sock", "--manifest"]),
            Err("--manifest requires a value".to_string())
        );
    }

    #[test]
    fn an_unrecognized_flag_is_an_error() {
        assert_eq!(
            args(&["--socket", "/tmp/x.sock", "--bogus"]),
            Err("unrecognized argument: --bogus".to_string())
        );
    }
}
