// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `blueice-ai-gatekeeper`: the process binary. Deliberately thin --
//! all the logic it runs (`handle_one_check`, applying the versioned
//! deterministic rule set) lives in `blueice_ai_gatekeeper`'s `lib.rs`, already
//! covered by its own unit tests against an in-process `UnixStream`
//! pair. This file is just parsing an optional private socket override,
//! binding a real `UnixListener`, and accepting connections -- matching
//! how `blueice-core`'s own thin binary is structured (see that crate's
//! `src/bin/blueice-core.rs` docs). The override lets `blueice-launcher`
//! give each supervised browser session its own gatekeeper instead of
//! competing for the well-known standalone-development socket.
//!
//! `core` opens a short-lived, per-check connection per review (connect
//! -> request -> reply -> disconnect). Each accepted connection runs in its
//! own bounded-time worker, so an idle local peer cannot block other reviews.

use blueice_ai_gatekeeper::{default_settings_path, GatekeeperService};
use blueice_ipc::gatekeeper::default_gatekeeper_socket_path;
use blueice_ipc::local_socket::{bind_private_listener, ensure_private_socket_dir};
use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::ExitCode;
use std::thread;
use std::time::{Duration, Instant};

const CHECK_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, PartialEq)]
struct Args {
    socket: PathBuf,
    settings: PathBuf,
}

fn parse_args(args: impl Iterator<Item = String>) -> Result<Args, String> {
    let mut socket = None;
    let mut settings = None;
    let mut it = args;
    while let Some(flag) = it.next() {
        let mut value = || it.next().ok_or_else(|| format!("{flag} requires a value"));
        match flag.as_str() {
            "--socket" => socket = Some(PathBuf::from(value()?)),
            "--settings" => settings = Some(PathBuf::from(value()?)),
            other => return Err(format!("unrecognized argument: {other}")),
        }
    }
    Ok(Args {
        socket: socket.unwrap_or_else(default_gatekeeper_socket_path),
        settings: settings.unwrap_or_else(default_settings_path),
    })
}

struct DeadlineStream {
    stream: UnixStream,
    deadline: Instant,
}

impl DeadlineStream {
    fn new(stream: UnixStream) -> Self {
        DeadlineStream {
            stream,
            deadline: Instant::now() + CHECK_TIMEOUT,
        }
    }
}

impl Read for DeadlineStream {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let remaining = self.deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                "the gatekeeper request exceeded its deadline",
            ));
        }
        self.stream.set_read_timeout(Some(remaining))?;
        self.stream.read(buffer)
    }
}

impl Write for DeadlineStream {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.stream.write(buffer)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.stream.flush()
    }
}

fn run(path: PathBuf, settings_path: PathBuf) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        ensure_private_socket_dir(parent)?;
    }
    // A stale socket file from a previous run (e.g. one that crashed
    // instead of exiting cleanly) makes bind() fail with AddrInUse even
    // though nothing is actually listening -- remove it first, same as
    // `blueice-core`'s own binary does for its own socket.
    let _ = std::fs::remove_file(&path);

    let listener = bind_private_listener(&path)?;
    let service =
        std::sync::Arc::new(GatekeeperService::new(Some(settings_path)).map_err(io::Error::other)?);
    for stream in listener.incoming().flatten() {
        let service = service.clone();
        thread::spawn(move || {
            let _ = stream.set_write_timeout(Some(CHECK_TIMEOUT));
            let mut stream = DeadlineStream::new(stream);
            let _ = service.handle_connection(&mut stream);
        });
    }
    Ok(())
}

fn main() -> ExitCode {
    let args = match parse_args(std::env::args().skip(1)) {
        Ok(args) => args,
        Err(message) => {
            eprintln!("blueice-ai-gatekeeper: {message}");
            return ExitCode::FAILURE;
        }
    };

    if let Err(error) = run(args.socket, args.settings) {
        eprintln!("blueice-ai-gatekeeper: {error}");
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(flags: &[&str]) -> Result<Args, String> {
        parse_args(flags.iter().map(|flag| flag.to_string()))
    }

    #[test]
    fn no_flags_uses_the_well_known_standalone_socket() {
        assert_eq!(
            args(&[]).unwrap(),
            Args {
                socket: default_gatekeeper_socket_path(),
                settings: default_settings_path(),
            }
        );
    }

    #[test]
    fn socket_override_is_parsed() {
        assert_eq!(
            args(&["--socket", "/tmp/private-gatekeeper.sock"]).unwrap(),
            Args {
                socket: PathBuf::from("/tmp/private-gatekeeper.sock"),
                settings: default_settings_path(),
            }
        );
    }

    #[test]
    fn settings_override_is_parsed() {
        assert_eq!(
            args(&["--settings", "/tmp/gatekeeper.json"]),
            Ok(Args {
                socket: default_gatekeeper_socket_path(),
                settings: PathBuf::from("/tmp/gatekeeper.json"),
            })
        );
    }

    #[test]
    fn a_socket_flag_without_a_value_is_an_error() {
        assert_eq!(
            args(&["--socket"]),
            Err("--socket requires a value".to_string())
        );
    }

    #[test]
    fn an_unknown_argument_is_an_error() {
        assert_eq!(
            args(&["--other"]),
            Err("unrecognized argument: --other".to_string())
        );
    }
}
