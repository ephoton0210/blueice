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

use blueice_engine::{session, TabManager};
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Debug, PartialEq)]
struct Args {
    socket: PathBuf,
    width: f64,
    height: f64,
    frame_dir: Option<PathBuf>,
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

    let mut it = args;
    while let Some(flag) = it.next() {
        let mut value = || it.next().ok_or_else(|| format!("{flag} requires a value"));
        match flag.as_str() {
            "--socket" => socket = Some(PathBuf::from(value()?)),
            "--width" => width = value()?.parse().map_err(|_| "--width must be a number".to_string())?,
            "--height" => height = value()?.parse().map_err(|_| "--height must be a number".to_string())?,
            "--frame-dir" => frame_dir = Some(PathBuf::from(value()?)),
            other => return Err(format!("unrecognized argument: {other}")),
        }
    }

    let socket = socket.ok_or_else(|| "--socket <path> is required".to_string())?;
    Ok(Args { socket, width, height, frame_dir })
}

fn main() -> ExitCode {
    let args = match parse_args(std::env::args().skip(1)) {
        Ok(args) => args,
        Err(message) => {
            eprintln!("blueice-core: {message}");
            return ExitCode::FAILURE;
        }
    };

    let frame_dir = args.frame_dir.unwrap_or_else(|| std::env::temp_dir().join(format!("blueice-core-frames-{}", std::process::id())));

    // A stale socket file from a previous run (e.g. one that crashed
    // instead of exiting cleanly) makes bind() fail with AddrInUse
    // even though nothing is actually listening -- remove it first.
    let _ = std::fs::remove_file(&args.socket);

    let listener = match UnixListener::bind(&args.socket) {
        Ok(listener) => listener,
        Err(e) => {
            eprintln!("blueice-core: failed to bind {}: {e}", args.socket.display());
            return ExitCode::FAILURE;
        }
    };

    let result = (|| -> std::io::Result<()> {
        let (mut stream, _) = listener.accept()?;
        let mut tabs = TabManager::new(args.width, args.height);
        let mut generation = 0u64;
        session::run_session(&mut tabs, &mut stream, &frame_dir, &mut generation)
    })();

    let _ = std::fs::remove_file(&args.socket);
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
    }

    #[test]
    fn every_flag_is_parsed() {
        let parsed = args(&["--socket", "/tmp/x.sock", "--width", "100", "--height", "50", "--frame-dir", "/tmp/frames"]).unwrap();
        assert_eq!(parsed, Args { socket: PathBuf::from("/tmp/x.sock"), width: 100.0, height: 50.0, frame_dir: Some(PathBuf::from("/tmp/frames")) });
    }

    #[test]
    fn a_flag_missing_its_value_is_an_error() {
        assert_eq!(args(&["--socket"]), Err("--socket requires a value".to_string()));
    }

    #[test]
    fn a_non_numeric_width_is_an_error() {
        assert_eq!(args(&["--socket", "/tmp/x.sock", "--width", "not-a-number"]), Err("--width must be a number".to_string()));
    }

    #[test]
    fn a_non_numeric_height_is_an_error() {
        assert_eq!(args(&["--socket", "/tmp/x.sock", "--height", "not-a-number"]), Err("--height must be a number".to_string()));
    }

    #[test]
    fn an_unrecognized_flag_is_an_error() {
        assert_eq!(args(&["--bogus"]), Err("unrecognized argument: --bogus".to_string()));
    }
}
