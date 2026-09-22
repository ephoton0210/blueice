// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Private isolated BlueJS page-host child launched only by
//! `blueice-launcher`. It owns no page loader, DOM, network, filesystem, or
//! frontend socket; its sole authority is to execute complete source graphs
//! that arrive after the launcher's versioned capability handshake.

#[cfg(unix)]
use blueice_launcher::bluejs_host::{
    bind_bluejs_host_socket, serve_bluejs_host_listener, BlueJsChildHost,
};
#[cfg(unix)]
use std::path::PathBuf;
#[cfg(unix)]
use std::process::ExitCode;

#[cfg(unix)]
#[derive(Debug, PartialEq)]
struct Args {
    socket: PathBuf,
    session_token: String,
}

#[cfg(unix)]
fn parse_args(args: impl Iterator<Item = String>) -> Result<Args, String> {
    let mut socket = None;
    let mut session_token = None;
    let mut args = args;
    while let Some(flag) = args.next() {
        let mut value = || {
            args.next()
                .ok_or_else(|| format!("{flag} requires a value"))
        };
        match flag.as_str() {
            "--socket" => socket = Some(PathBuf::from(value()?)),
            "--session-token" => session_token = Some(value()?),
            other => return Err(format!("unrecognized argument: {other}")),
        }
    }
    let socket = socket.ok_or_else(|| "--socket <path> is required".to_string())?;
    let session_token =
        session_token.ok_or_else(|| "--session-token <token> is required".to_string())?;
    if session_token.len() < 32 || !session_token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("--session-token must be a non-short hexadecimal capability".to_string());
    }
    Ok(Args {
        socket,
        session_token,
    })
}

#[cfg(unix)]
fn main() -> ExitCode {
    let args = match parse_args(std::env::args().skip(1)) {
        Ok(args) => args,
        Err(message) => {
            eprintln!("blueice-bluejs-host: {message}");
            return ExitCode::FAILURE;
        }
    };
    let listener = match bind_bluejs_host_socket(&args.socket) {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!(
                "blueice-bluejs-host: failed to bind {}: {error}",
                args.socket.display()
            );
            return ExitCode::FAILURE;
        }
    };
    let mut host = BlueJsChildHost::default();
    let result = serve_bluejs_host_listener(listener, args.session_token, &mut host);
    let _ = std::fs::remove_file(&args.socket);
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("blueice-bluejs-host: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(not(unix))]
fn main() {
    eprintln!("blueice-bluejs-host is currently supported only on Unix platforms");
    std::process::exit(1);
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn args(flags: &[&str]) -> Result<Args, String> {
        parse_args(flags.iter().map(|flag| flag.to_string()))
    }

    #[test]
    fn child_requires_both_private_startup_arguments() {
        assert_eq!(args(&[]), Err("--socket <path> is required".to_string()));
        assert_eq!(
            args(&["--socket", "/tmp/host.sock"]),
            Err("--session-token <token> is required".to_string())
        );
    }

    #[test]
    fn child_rejects_a_short_or_non_hex_capability() {
        assert_eq!(
            args(&["--socket", "/tmp/host.sock", "--session-token", "short"]),
            Err("--session-token must be a non-short hexadecimal capability".to_string())
        );
        assert_eq!(
            args(&[
                "--socket",
                "/tmp/host.sock",
                "--session-token",
                "zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz",
            ]),
            Err("--session-token must be a non-short hexadecimal capability".to_string())
        );
    }
}
