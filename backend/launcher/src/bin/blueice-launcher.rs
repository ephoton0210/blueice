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

use blueice_launcher::memory_pressure::{self, SystemMemorySource};
use blueice_launcher::supervisor::{ProcessPolicy, ProcessRegistry};
use blueice_launcher::{default_rendezvous_socket_path, run_broker, SpawnedCore};
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Debug, PartialEq)]
struct Args {
    rendezvous_socket: PathBuf,
    width: f64,
    height: f64,
    frame_dir: Option<PathBuf>,
    /// Test/debug-only: use a [`memory_pressure::FixedMemorySource`]
    /// reporting zero availability instead of real host memory, so the
    /// memory-pressure-response path can be exercised deterministically
    /// (`backend/launcher/tests/supervisor_idle_teardown.rs`) rather
    /// than depending on the test machine's actual memory state.
    simulate_low_memory: bool,
    /// Test/debug-only override for [`memory_pressure::DEFAULT_POLL_INTERVAL`]
    /// -- the real default (10s) would make an integration test wait
    /// that long to observe a single poll.
    memory_poll_interval: Duration,
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
    let mut simulate_low_memory = false;
    let mut memory_poll_interval = memory_pressure::DEFAULT_POLL_INTERVAL;

    let mut it = args;
    while let Some(flag) = it.next() {
        let mut value = || it.next().ok_or_else(|| format!("{flag} requires a value"));
        match flag.as_str() {
            "--socket" => rendezvous_socket = Some(PathBuf::from(value()?)),
            "--width" => width = value()?.parse().map_err(|_| "--width must be a number".to_string())?,
            "--height" => height = value()?.parse().map_err(|_| "--height must be a number".to_string())?,
            "--frame-dir" => frame_dir = Some(PathBuf::from(value()?)),
            "--simulate-low-memory" => simulate_low_memory = true,
            "--memory-poll-interval-ms" => {
                let ms: u64 = value()?.parse().map_err(|_| "--memory-poll-interval-ms must be a number".to_string())?;
                memory_poll_interval = Duration::from_millis(ms);
            }
            other => return Err(format!("unrecognized argument: {other}")),
        }
    }

    let rendezvous_socket = rendezvous_socket.unwrap_or_else(default_rendezvous_socket_path);
    Ok(Args { rendezvous_socket, width, height, frame_dir, simulate_low_memory, memory_poll_interval })
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

    // `phase-8-live-core-hotswap/PLAN.md`'s fleet-memory-supervisor
    // minimal first slice: `core` is registered `AlwaysResident` with
    // `resident: None` (its real child is owned and torn down by
    // `SpawnedCore` above, not this registry -- see `ProcessRegistry::
    // is_resident`'s own docs for why an `AlwaysResident` role never
    // needs the registry to hold the child at all). `mcp-server` is
    // registered as a typed but inert `IdleTeardown` slot: real in the
    // data model and exercised in tests, but with no automatic spawn
    // path yet (see `phase-8-live-core-hotswap/PLAN.md`'s own follow-up
    // item for why that's a separate, still-open transport question).
    let registry = Arc::new(Mutex::new(ProcessRegistry::new()));
    {
        let mut registry = registry.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let now = Instant::now();
        registry.register("core", ProcessPolicy::AlwaysResident, None, now);
        registry.register("mcp-server", ProcessPolicy::IdleTeardown { idle_timeout: Duration::from_secs(300) }, None, now);
    }
    let memory_source: Arc<dyn memory_pressure::MemorySource> =
        if args.simulate_low_memory { Arc::new(memory_pressure::FixedMemorySource(0.0)) } else { Arc::new(SystemMemorySource::new()) };
    let _pressure_monitor = memory_pressure::spawn_pressure_monitor(Arc::clone(&registry), memory_source, memory_pressure::DEFAULT_PRESSURE_THRESHOLD, args.memory_poll_interval);

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
        assert!(!parsed.simulate_low_memory);
        assert_eq!(parsed.memory_poll_interval, memory_pressure::DEFAULT_POLL_INTERVAL);
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
            "--simulate-low-memory",
            "--memory-poll-interval-ms",
            "50",
        ])
        .unwrap();
        assert_eq!(
            parsed,
            Args {
                rendezvous_socket: PathBuf::from("/tmp/x.sock"),
                width: 100.0,
                height: 50.0,
                frame_dir: Some(PathBuf::from("/tmp/frames")),
                simulate_low_memory: true,
                memory_poll_interval: Duration::from_millis(50),
            }
        );
    }

    #[test]
    fn a_non_numeric_memory_poll_interval_is_an_error() {
        assert_eq!(args(&["--memory-poll-interval-ms", "not-a-number"]), Err("--memory-poll-interval-ms must be a number".to_string()));
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
