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
//! needs to serve them one at a time, sequentially -- there's exactly
//! one hardcoded extension in this slice, so there's no concurrency to
//! prove yet.

use blueice_extension_host::{handle_extension_connection, ExtensionRegistry};
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Debug, PartialEq)]
struct Args {
    socket: PathBuf,
}

/// Takes an injectable argument iterator (rather than reading
/// `std::env::args()` directly) so every flag-parsing branch is a plain
/// unit test -- mirrors `blueice-core`'s own `parse_args` for the same
/// reason (see that binary's docs).
fn parse_args(args: impl Iterator<Item = String>) -> Result<Args, String> {
    let mut socket = None;

    let mut it = args;
    while let Some(flag) = it.next() {
        let mut value = || it.next().ok_or_else(|| format!("{flag} requires a value"));
        match flag.as_str() {
            "--socket" => socket = Some(PathBuf::from(value()?)),
            other => return Err(format!("unrecognized argument: {other}")),
        }
    }

    let socket = socket.ok_or_else(|| "--socket <path> is required".to_string())?;
    Ok(Args { socket })
}

fn main() -> ExitCode {
    let args = match parse_args(std::env::args().skip(1)) {
        Ok(args) => args,
        Err(message) => {
            eprintln!("blueice-extension-host: {message}");
            return ExitCode::FAILURE;
        }
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

    let registry = ExtensionRegistry::minimal_slice();
    for mut stream in listener.incoming().flatten() {
        let _ = handle_extension_connection(&registry, &mut stream);
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
                socket: PathBuf::from("/tmp/x.sock")
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
    fn an_unrecognized_flag_is_an_error() {
        assert_eq!(
            args(&["--socket", "/tmp/x.sock", "--bogus"]),
            Err("unrecognized argument: --bogus".to_string())
        );
    }
}
