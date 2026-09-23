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
//! Extension connections are long-lived (unlike `ai-gatekeeper`'s
//! one-shot-per-check connections), but this minimal slice still only
//! needs to serve them one at a time, sequentially. `--manifest` installs one
//! validated package for that process lifetime; without it the historic
//! hardcoded reference slice remains available for protocol-only testing.

use blueice_extension_host::{
    handle_extension_connection_with_gatekeeper, load_installed_extension,
    registry_for_installed_extension, ExtensionRegistry,
};
use blueice_ipc::gatekeeper::default_gatekeeper_socket_path;
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Debug, PartialEq)]
struct Args {
    socket: PathBuf,
    gatekeeper_socket: PathBuf,
    manifest: Option<PathBuf>,
}

/// Takes an injectable argument iterator (rather than reading
/// `std::env::args()` directly) so every flag-parsing branch is a plain
/// unit test -- mirrors `blueice-core`'s own `parse_args` for the same
/// reason (see that binary's docs).
fn parse_args(args: impl Iterator<Item = String>) -> Result<Args, String> {
    let mut socket = None;
    let mut gatekeeper_socket = None;
    let mut manifest = None;

    let mut it = args;
    while let Some(flag) = it.next() {
        let mut value = || it.next().ok_or_else(|| format!("{flag} requires a value"));
        match flag.as_str() {
            "--socket" => socket = Some(PathBuf::from(value()?)),
            "--gatekeeper-socket" => gatekeeper_socket = Some(PathBuf::from(value()?)),
            "--manifest" => manifest = Some(PathBuf::from(value()?)),
            other => return Err(format!("unrecognized argument: {other}")),
        }
    }

    let socket = socket.ok_or_else(|| "--socket <path> is required".to_string())?;
    Ok(Args {
        socket,
        gatekeeper_socket: gatekeeper_socket.unwrap_or_else(default_gatekeeper_socket_path),
        manifest,
    })
}

fn main() -> ExitCode {
    let args = match parse_args(std::env::args().skip(1)) {
        Ok(args) => args,
        Err(message) => {
            eprintln!("blueice-extension-host: {message}");
            return ExitCode::FAILURE;
        }
    };

    // Validate the package and populate the server-side registry before
    // publishing a socket. A client must never race a briefly listening host
    // whose identity/capability table has not been established yet.
    let registry = match args.manifest.as_deref() {
        Some(manifest_path) => match load_installed_extension(manifest_path) {
            Ok(extension) => registry_for_installed_extension(&extension),
            Err(error) => {
                eprintln!(
                    "blueice-extension-host: could not install {}: {error}",
                    manifest_path.display()
                );
                let _ = std::fs::remove_file(&args.socket);
                return ExitCode::FAILURE;
            }
        },
        None => ExtensionRegistry::minimal_slice(),
    };

    // A stale socket file from a previous run (e.g. one that crashed
    // instead of exiting cleanly) makes bind() fail with AddrInUse even
    // though nothing is actually listening -- remove it first, same as
    // `blueice-core`'s and `blueice-ai-gatekeeper`'s own binaries do.
    let _ = std::fs::remove_file(&args.socket);

    let listener = match UnixListener::bind(&args.socket) {
        Ok(listener) => listener,
        Err(e) => {
            eprintln!(
                "blueice-extension-host: failed to bind {}: {e}",
                args.socket.display()
            );
            return ExitCode::FAILURE;
        }
    };
    for mut stream in listener.incoming().flatten() {
        let _ = handle_extension_connection_with_gatekeeper(
            &registry,
            &args.gatekeeper_socket,
            &mut stream,
        );
    }

    let _ = std::fs::remove_file(&args.socket);
    ExitCode::SUCCESS
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
    fn socket_flag_is_parsed() {
        assert_eq!(
            args(&["--socket", "/tmp/x.sock"]).unwrap(),
            Args {
                socket: PathBuf::from("/tmp/x.sock"),
                gatekeeper_socket: default_gatekeeper_socket_path(),
                manifest: None,
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
                socket: PathBuf::from("/tmp/x.sock"),
                gatekeeper_socket: PathBuf::from("/tmp/gatekeeper.sock"),
                manifest: None,
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
            args(&["--socket", "/tmp/x.sock", "--manifest", "/tmp/extension.json"])
                .unwrap(),
            Args {
                socket: PathBuf::from("/tmp/x.sock"),
                gatekeeper_socket: default_gatekeeper_socket_path(),
                manifest: Some(PathBuf::from("/tmp/extension.json")),
            }
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
