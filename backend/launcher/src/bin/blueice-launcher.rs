// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `blueice-launcher`: the process external clients (`frontend`,
//! `mcp-server`, ...) connect to instead of a specific `core` instance
//! -- see `blueice_launcher`'s crate docs and
//! `phase-8-live-core-hotswap/PLAN.md`'s "Minimal first slice" for the
//! design. Deliberately thin, matching `blueice-core.rs`'s own split:
//! all the real logic (spawning `core`, the fan-in/fan-out broker)
//! lives in `blueice_launcher`, already covered by its own unit tests
//! against fake `UnixStream` pairs and a real-subprocess integration
//! test -- this file is just argument parsing and wiring a real
//! `UnixListener` to that already-tested logic.

use blueice_launcher::{default_rendezvous_socket_path, run_broker, SpawnedCore};
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Debug, PartialEq)]
struct Args {
    rendezvous_socket: PathBuf,
    width: f64,
    height: f64,
    frame_dir: Option<PathBuf>,
}

/// Takes an injectable argument iterator for the same reason
/// `blueice-core.rs`'s `parse_args` does: every flag-parsing branch is a
/// plain unit test, not something only exercisable by actually spawning
/// the binary.
fn parse_args(args: impl Iterator<Item = String>) -> Result<Args, String> {
    let mut rendezvous_socket = None;
    let mut width = 800.0;
    let mut height = 600.0;
    let mut frame_dir = None;

    let mut it = args;
    while let Some(flag) = it.next() {
        let mut value = || it.next().ok_or_else(|| format!("{flag} requires a value"));
        match flag.as_str() {
            "--socket" => rendezvous_socket = Some(PathBuf::from(value()?)),
            "--width" => width = value()?.parse().map_err(|_| "--width must be a number".to_string())?,
            "--height" => height = value()?.parse().map_err(|_| "--height must be a number".to_string())?,
            "--frame-dir" => frame_dir = Some(PathBuf::from(value()?)),
            other => return Err(format!("unrecognized argument: {other}")),
        }
    }

    let rendezvous_socket = rendezvous_socket.unwrap_or_else(default_rendezvous_socket_path);
    Ok(Args { rendezvous_socket, width, height, frame_dir })
}

fn main() -> ExitCode {
    let args = match parse_args(std::env::args().skip(1)) {
        Ok(args) => args,
        Err(message) => {
            eprintln!("blueice-launcher: {message}");
            return ExitCode::FAILURE;
        }
    };

    let frame_dir = args.frame_dir.unwrap_or_else(|| std::env::temp_dir().join(format!("blueice-launcher-frames-{}", std::process::id())));

    let core = match SpawnedCore::spawn(args.width, args.height, &frame_dir) {
        Ok(core) => core,
        Err(e) => {
            eprintln!("blueice-launcher: failed to spawn blueice-core: {e}");
            return ExitCode::FAILURE;
        }
    };

    if let Some(parent) = args.rendezvous_socket.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    // A stale socket file from a previous run (e.g. one that crashed
    // instead of exiting cleanly) makes bind() fail with AddrInUse even
    // though nothing is actually listening -- remove it first, same as
    // `blueice-core.rs` does for its own socket.
    let _ = std::fs::remove_file(&args.rendezvous_socket);

    let listener = match UnixListener::bind(&args.rendezvous_socket) {
        Ok(listener) => listener,
        Err(e) => {
            eprintln!("blueice-launcher: failed to bind {}: {e}", args.rendezvous_socket.display());
            return ExitCode::FAILURE;
        }
    };

    let result = run_broker(listener, core.stream.try_clone().expect("try_clone on a fresh stream should not fail"));

    let _ = std::fs::remove_file(&args.rendezvous_socket);
    drop(core); // kills the spawned blueice-core and cleans up its internal socket

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("blueice-launcher: broker error: {e}");
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
    fn no_flags_uses_the_default_rendezvous_socket_and_default_size() {
        let parsed = args(&[]).unwrap();
        assert_eq!(parsed.rendezvous_socket, default_rendezvous_socket_path());
        assert_eq!(parsed.width, 800.0);
        assert_eq!(parsed.height, 600.0);
        assert_eq!(parsed.frame_dir, None);
    }

    #[test]
    fn every_flag_is_parsed() {
        let parsed = args(&["--socket", "/tmp/x.sock", "--width", "100", "--height", "50", "--frame-dir", "/tmp/frames"]).unwrap();
        assert_eq!(parsed, Args { rendezvous_socket: PathBuf::from("/tmp/x.sock"), width: 100.0, height: 50.0, frame_dir: Some(PathBuf::from("/tmp/frames")) });
    }

    #[test]
    fn a_flag_missing_its_value_is_an_error() {
        assert_eq!(args(&["--socket"]), Err("--socket requires a value".to_string()));
    }

    #[test]
    fn a_non_numeric_width_is_an_error() {
        assert_eq!(args(&["--width", "not-a-number"]), Err("--width must be a number".to_string()));
    }

    #[test]
    fn a_non_numeric_height_is_an_error() {
        assert_eq!(args(&["--height", "not-a-number"]), Err("--height must be a number".to_string()));
    }

    #[test]
    fn an_unrecognized_flag_is_an_error() {
        assert_eq!(args(&["--bogus"]), Err("unrecognized argument: --bogus".to_string()));
    }
}
