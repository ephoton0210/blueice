// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `blueice-automation`: binds an automation-adapter socket, issues a
//! fresh per-run token (written to a per-user token file a real
//! client reads to authenticate), and proxies each connection that
//! presents that token straight through to a real `blueice-core`
//! process's own `--automation-socket`. See `lib.rs` for the actual
//! logic (already covered by its own unit tests over `UnixStream`
//! pairs and a fake "core" listener); this file is deliberately thin
//! argument-parsing plus real socket bind/accept wiring, verified
//! against the real compiled binaries (this one *and* `blueice-core`)
//! in `tests/automation_binary.rs` -- the same "unit-test the logic,
//! subprocess-test the wiring" split `blueice-core`'s own
//! `bin/blueice-core.rs` uses.
//!
//! Runs until killed: unlike `blueice-core`, there is no single
//! primary client whose disconnect ends the process, since an
//! automation adapter is meant to keep accepting client after client
//! for as long as anything might want to drive BlueIce -- matching
//! `blueice-ai-gatekeeper`'s own perpetual-accept-loop shape rather
//! than `blueice-core`'s single-frontend one.

use blueice_automation::{default_token_path, generate_token, serve_connection, write_token_file};
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::process::ExitCode;
use std::thread;

#[derive(Debug, PartialEq)]
struct Args {
    socket: PathBuf,
    core_socket: PathBuf,
    token_file: Option<PathBuf>,
}

/// Takes an injectable argument iterator, matching
/// `blueice-core::bin::parse_args`'s own reasoning: every flag-parsing
/// branch is a plain unit test here, and the real process wiring
/// (bind, accept, the actual token file) is covered by
/// `tests/automation_binary.rs` spawning the real compiled binary
/// instead.
fn parse_args(args: impl Iterator<Item = String>) -> Result<Args, String> {
    let mut socket = None;
    let mut core_socket = None;
    let mut token_file = None;

    let mut it = args;
    while let Some(flag) = it.next() {
        let mut value = || it.next().ok_or_else(|| format!("{flag} requires a value"));
        match flag.as_str() {
            "--socket" => socket = Some(PathBuf::from(value()?)),
            "--core-socket" => core_socket = Some(PathBuf::from(value()?)),
            "--token-file" => token_file = Some(PathBuf::from(value()?)),
            other => return Err(format!("unrecognized argument: {other}")),
        }
    }

    Ok(Args {
        socket: socket.ok_or_else(|| "--socket <path> is required".to_string())?,
        core_socket: core_socket.ok_or_else(|| "--core-socket <path> is required".to_string())?,
        token_file,
    })
}

fn main() -> ExitCode {
    let args = match parse_args(std::env::args().skip(1)) {
        Ok(args) => args,
        Err(message) => {
            eprintln!("blueice-automation: {message}");
            return ExitCode::FAILURE;
        }
    };

    let token = match generate_token() {
        Ok(token) => token,
        Err(error) => {
            eprintln!("blueice-automation: failed to generate a token: {error}");
            return ExitCode::FAILURE;
        }
    };
    let token_path = args.token_file.clone().unwrap_or_else(default_token_path);
    if let Err(error) = write_token_file(&token_path, &token) {
        eprintln!(
            "blueice-automation: failed to write token file {}: {error}",
            token_path.display()
        );
        return ExitCode::FAILURE;
    }

    // A stale socket file from a previous run makes bind() fail with
    // AddrInUse even though nothing is actually listening -- remove
    // it first, same as `blueice-core`'s own binary does for its own
    // socket.
    let _ = std::fs::remove_file(&args.socket);
    let listener = match UnixListener::bind(&args.socket) {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!(
                "blueice-automation: failed to bind {}: {error}",
                args.socket.display()
            );
            let _ = std::fs::remove_file(&token_path);
            return ExitCode::FAILURE;
        }
    };

    for incoming in listener.incoming() {
        let Ok(stream) = incoming else { break };
        let token = token.clone();
        let core_socket = args.core_socket.clone();
        thread::spawn(move || {
            let _ = serve_connection(stream, &token, &core_socket);
        });
    }

    let _ = std::fs::remove_file(&args.socket);
    let _ = std::fs::remove_file(&token_path);
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
    fn core_socket_is_required() {
        assert_eq!(
            args(&["--socket", "/tmp/a.sock"]),
            Err("--core-socket <path> is required".to_string())
        );
    }

    #[test]
    fn every_flag_is_parsed() {
        let parsed = args(&[
            "--socket",
            "/tmp/a.sock",
            "--core-socket",
            "/tmp/core.sock",
            "--token-file",
            "/tmp/token",
        ])
        .unwrap();
        assert_eq!(
            parsed,
            Args {
                socket: PathBuf::from("/tmp/a.sock"),
                core_socket: PathBuf::from("/tmp/core.sock"),
                token_file: Some(PathBuf::from("/tmp/token")),
            }
        );
    }

    #[test]
    fn token_file_is_optional() {
        let parsed = args(&["--socket", "/tmp/a.sock", "--core-socket", "/tmp/core.sock"]).unwrap();
        assert_eq!(parsed.token_file, None);
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
            args(&["--bogus"]),
            Err("unrecognized argument: --bogus".to_string())
        );
    }
}
