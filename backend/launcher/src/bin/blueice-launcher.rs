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

#[cfg(unix)]
use blueice_launcher::memory_pressure::{self, SystemMemorySource};
#[cfg(unix)]
use blueice_launcher::supervisor::{ProcessPolicy, ProcessRegistry};
#[cfg(unix)]
use blueice_launcher::{
    default_control_socket_path, default_rendezvous_socket_path, run_broker, CoreLaunchOptions,
    SpawnedCore,
};
#[cfg(unix)]
use std::os::unix::net::UnixListener;
#[cfg(unix)]
use std::path::PathBuf;
#[cfg(unix)]
use std::process::ExitCode;
#[cfg(unix)]
use std::sync::{Arc, Mutex};
#[cfg(unix)]
use std::time::{Duration, Instant};

#[cfg(unix)]
#[derive(Debug, PartialEq)]
struct Args {
    rendezvous_socket: PathBuf,
    /// The launcher-internal control socket (`ControlRequest::Cutover`
    /// et al., `phase-8-live-core-hotswap/PLAN.md`'s minimal-slice
    /// trigger mechanism) -- distinct from `rendezvous_socket`, and
    /// separately overridable so tests can run more than one launcher
    /// side by side without either socket colliding.
    control_socket: PathBuf,
    width: f64,
    height: f64,
    frame_dir: Option<PathBuf>,
    /// Optional owner-selected gatekeeper endpoint for the core child. This
    /// is unrelated to the private BlueJS child-host capability and exists so
    /// an operator can keep the core's existing gatekeeper routing explicit.
    gatekeeper_socket: Option<PathBuf>,
    /// Explicitly asks the launcher to create and supervise one isolated
    /// BlueJS page-host child for each core generation. The endpoint/token
    /// are generated internally and are never CLI values.
    out_of_process_bluejs: bool,
    /// Explicit opt-in compiler MCP attachment endpoint.  The caller chooses
    /// only this Unix socket path; the launcher passes its one fixed,
    /// core-owned closed project profile and never accepts a profile, source,
    /// resolver, compiler option, or write/build input from the CLI.
    compiler_mcp_socket: Option<PathBuf>,
    /// Explicit opt-in stable debugger endpoint. The caller selects only its
    /// Unix socket path; the launcher owns the public owner-only listener and
    /// supplies each core generation a fresh private listener. Debugger
    /// protocol authorization remains in `blueice-core`.
    debugger_socket: Option<PathBuf>,
    /// Owner opt-in for the debugger's source-free opaque static-metadata
    /// inventory. It requires the separate debugger endpoint and does not
    /// grant metadata reads or source/runtime inspection.
    debugger_static_metadata_inventory: bool,
    /// Owner opt-in for bounded source-free metadata summaries of handles
    /// from the inventory. It requires the inventory flag and still does not
    /// grant source identity/text, type/symbol/contract records, or values.
    debugger_static_metadata_summary: bool,
    /// Owner opt-in for compiler-minted source-record IDs from a prior opaque
    /// metadata handle. It requires the parent inventory and exposes no
    /// source identity, hash, text, or record detail.
    debugger_static_metadata_source_inventory: bool,
    /// Owner opt-in for source-free canonical module identity and SHA-256
    /// provenance for a previously inventoried source ID. It requires the
    /// parent source inventory and does not grant source text or record reads.
    debugger_static_metadata_source_provenance: bool,
    /// Owner opt-in for opaque compiler-minted type-record IDs under a prior
    /// metadata handle. It does not expose type labels or static records.
    debugger_static_metadata_type_inventory: bool,
    /// Owner opt-in for one bounded compiler-produced display under a type ID
    /// previously emitted by the separate type inventory.
    debugger_static_metadata_type_display: bool,
    /// Owner opt-in for opaque compiler-minted symbol-record IDs under a
    /// prior metadata handle. It does not expose symbol detail or records.
    debugger_static_metadata_symbol_inventory: bool,
    /// Owner opt-in for opaque compiler-minted contract IDs under a prior
    /// metadata handle. It does not expose contract detail or records.
    debugger_static_metadata_contract_inventory: bool,
    /// Owner opt-in for a bounded compiler-produced display under an already
    /// inventoried contract ID. It does not expose a plan or validation result.
    debugger_static_metadata_contract_display: bool,
    /// Owner opt-in for a bounded data-only validation under an already
    /// inventoried contract ID. It returns only a boolean, never plan/error detail.
    debugger_static_metadata_contract_validation: bool,
    /// Owner opt-in for a bounded compiler-produced display under an already
    /// inventoried symbol ID. It does not expose a symbol record or source span.
    debugger_static_metadata_symbol_display: bool,
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
#[cfg(unix)]
fn parse_args(args: impl Iterator<Item = String>) -> Result<Args, String> {
    let mut rendezvous_socket = None;
    let mut control_socket = None;
    let mut width = 800.0;
    let mut height = 600.0;
    let mut frame_dir = None;
    let mut gatekeeper_socket = None;
    let mut out_of_process_bluejs = false;
    let mut compiler_mcp_socket = None;
    let mut debugger_socket = None;
    let mut debugger_static_metadata_inventory = false;
    let mut debugger_static_metadata_summary = false;
    let mut debugger_static_metadata_source_inventory = false;
    let mut debugger_static_metadata_source_provenance = false;
    let mut debugger_static_metadata_type_inventory = false;
    let mut debugger_static_metadata_type_display = false;
    let mut debugger_static_metadata_symbol_inventory = false;
    let mut debugger_static_metadata_contract_inventory = false;
    let mut debugger_static_metadata_contract_display = false;
    let mut debugger_static_metadata_contract_validation = false;
    let mut debugger_static_metadata_symbol_display = false;
    let mut simulate_low_memory = false;
    let mut memory_poll_interval = memory_pressure::DEFAULT_POLL_INTERVAL;

    let mut it = args;
    while let Some(flag) = it.next() {
        let mut value = || it.next().ok_or_else(|| format!("{flag} requires a value"));
        match flag.as_str() {
            "--socket" => rendezvous_socket = Some(PathBuf::from(value()?)),
            "--control-socket" => control_socket = Some(PathBuf::from(value()?)),
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
            "--out-of-process-bluejs" => out_of_process_bluejs = true,
            "--compiler-mcp-socket" => compiler_mcp_socket = Some(PathBuf::from(value()?)),
            "--debugger-socket" => debugger_socket = Some(PathBuf::from(value()?)),
            "--debugger-static-metadata-inventory" => debugger_static_metadata_inventory = true,
            "--debugger-static-metadata-summary" => debugger_static_metadata_summary = true,
            "--debugger-static-metadata-source-inventory" => {
                debugger_static_metadata_source_inventory = true
            }
            "--debugger-static-metadata-source-provenance" => {
                debugger_static_metadata_source_provenance = true
            }
            "--debugger-static-metadata-type-inventory" => {
                debugger_static_metadata_type_inventory = true
            }
            "--debugger-static-metadata-type-display" => {
                debugger_static_metadata_type_display = true
            }
            "--debugger-static-metadata-symbol-inventory" => {
                debugger_static_metadata_symbol_inventory = true
            }
            "--debugger-static-metadata-contract-inventory" => {
                debugger_static_metadata_contract_inventory = true
            }
            "--debugger-static-metadata-contract-display" => {
                debugger_static_metadata_contract_display = true
            }
            "--debugger-static-metadata-contract-validation" => {
                debugger_static_metadata_contract_validation = true
            }
            "--debugger-static-metadata-symbol-display" => {
                debugger_static_metadata_symbol_display = true
            }
            "--simulate-low-memory" => simulate_low_memory = true,
            "--memory-poll-interval-ms" => {
                let ms: u64 = value()?
                    .parse()
                    .map_err(|_| "--memory-poll-interval-ms must be a number".to_string())?;
                memory_poll_interval = Duration::from_millis(ms);
            }
            other => return Err(format!("unrecognized argument: {other}")),
        }
    }

    let rendezvous_socket = rendezvous_socket.unwrap_or_else(default_rendezvous_socket_path);
    let control_socket = control_socket.unwrap_or_else(default_control_socket_path);
    if debugger_static_metadata_inventory && debugger_socket.is_none() {
        return Err("--debugger-static-metadata-inventory requires --debugger-socket".to_string());
    }
    if debugger_static_metadata_summary && debugger_socket.is_none() {
        return Err("--debugger-static-metadata-summary requires --debugger-socket".to_string());
    }
    if debugger_static_metadata_summary && !debugger_static_metadata_inventory {
        return Err(
            "--debugger-static-metadata-summary requires --debugger-static-metadata-inventory"
                .to_string(),
        );
    }
    if debugger_static_metadata_source_inventory && debugger_socket.is_none() {
        return Err(
            "--debugger-static-metadata-source-inventory requires --debugger-socket".to_string(),
        );
    }
    if debugger_static_metadata_source_inventory && !debugger_static_metadata_inventory {
        return Err(
            "--debugger-static-metadata-source-inventory requires --debugger-static-metadata-inventory"
                .to_string(),
        );
    }
    if debugger_static_metadata_source_provenance && debugger_socket.is_none() {
        return Err(
            "--debugger-static-metadata-source-provenance requires --debugger-socket".to_string(),
        );
    }
    if debugger_static_metadata_source_provenance && !debugger_static_metadata_source_inventory {
        return Err(
            "--debugger-static-metadata-source-provenance requires --debugger-static-metadata-source-inventory"
                .to_string(),
        );
    }
    if debugger_static_metadata_type_inventory && debugger_socket.is_none() {
        return Err(
            "--debugger-static-metadata-type-inventory requires --debugger-socket".to_string(),
        );
    }
    if debugger_static_metadata_type_inventory && !debugger_static_metadata_inventory {
        return Err(
            "--debugger-static-metadata-type-inventory requires --debugger-static-metadata-inventory"
                .to_string(),
        );
    }
    if debugger_static_metadata_type_display && debugger_socket.is_none() {
        return Err(
            "--debugger-static-metadata-type-display requires --debugger-socket".to_string(),
        );
    }
    if debugger_static_metadata_type_display && !debugger_static_metadata_type_inventory {
        return Err(
            "--debugger-static-metadata-type-display requires --debugger-static-metadata-type-inventory"
                .to_string(),
        );
    }
    if debugger_static_metadata_symbol_inventory && debugger_socket.is_none() {
        return Err(
            "--debugger-static-metadata-symbol-inventory requires --debugger-socket".to_string(),
        );
    }
    if debugger_static_metadata_symbol_inventory && !debugger_static_metadata_inventory {
        return Err(
            "--debugger-static-metadata-symbol-inventory requires --debugger-static-metadata-inventory"
                .to_string(),
        );
    }
    if debugger_static_metadata_contract_inventory && debugger_socket.is_none() {
        return Err(
            "--debugger-static-metadata-contract-inventory requires --debugger-socket".to_string(),
        );
    }
    if debugger_static_metadata_contract_inventory && !debugger_static_metadata_inventory {
        return Err(
            "--debugger-static-metadata-contract-inventory requires --debugger-static-metadata-inventory"
                .to_string(),
        );
    }
    if debugger_static_metadata_contract_display && debugger_socket.is_none() {
        return Err(
            "--debugger-static-metadata-contract-display requires --debugger-socket".to_string(),
        );
    }
    if debugger_static_metadata_contract_display && !debugger_static_metadata_inventory {
        return Err(
            "--debugger-static-metadata-contract-display requires --debugger-static-metadata-inventory"
                .to_string(),
        );
    }
    if debugger_static_metadata_contract_validation && debugger_socket.is_none() {
        return Err(
            "--debugger-static-metadata-contract-validation requires --debugger-socket".to_string(),
        );
    }
    if debugger_static_metadata_contract_validation && !debugger_static_metadata_inventory {
        return Err(
            "--debugger-static-metadata-contract-validation requires --debugger-static-metadata-inventory"
                .to_string(),
        );
    }
    if debugger_static_metadata_symbol_display && debugger_socket.is_none() {
        return Err(
            "--debugger-static-metadata-symbol-display requires --debugger-socket".to_string(),
        );
    }
    if debugger_static_metadata_symbol_display && !debugger_static_metadata_inventory {
        return Err(
            "--debugger-static-metadata-symbol-display requires --debugger-static-metadata-inventory"
                .to_string(),
        );
    }
    Ok(Args {
        rendezvous_socket,
        control_socket,
        width,
        height,
        frame_dir,
        gatekeeper_socket,
        out_of_process_bluejs,
        compiler_mcp_socket,
        debugger_socket,
        debugger_static_metadata_inventory,
        debugger_static_metadata_summary,
        debugger_static_metadata_source_inventory,
        debugger_static_metadata_source_provenance,
        debugger_static_metadata_type_inventory,
        debugger_static_metadata_type_display,
        debugger_static_metadata_symbol_inventory,
        debugger_static_metadata_contract_inventory,
        debugger_static_metadata_contract_display,
        debugger_static_metadata_contract_validation,
        debugger_static_metadata_symbol_display,
        simulate_low_memory,
        memory_poll_interval,
    })
}

#[cfg(unix)]
fn main() -> ExitCode {
    let args = match parse_args(std::env::args().skip(1)) {
        Ok(args) => args,
        Err(message) => {
            eprintln!("blueice-launcher: {message}");
            return ExitCode::FAILURE;
        }
    };

    let frame_dir = args.frame_dir.unwrap_or_else(|| {
        std::env::temp_dir().join(format!("blueice-launcher-frames-{}", std::process::id()))
    });

    let mut core_options = CoreLaunchOptions::default();
    if let Some(gatekeeper_socket) = args.gatekeeper_socket.clone() {
        core_options = core_options.with_gatekeeper_socket(gatekeeper_socket);
    }
    if args.out_of_process_bluejs {
        core_options = core_options.supervise_out_of_process_bluejs();
    }
    if let Some(compiler_mcp_socket) = args.compiler_mcp_socket.clone() {
        core_options = core_options.with_core_closed_compiler_mcp_endpoint(compiler_mcp_socket);
    }
    if let Some(debugger_socket) = args.debugger_socket.clone() {
        core_options = core_options.with_debugger_endpoint(debugger_socket);
    }
    if args.debugger_static_metadata_inventory {
        core_options = core_options.with_debugger_static_metadata_inventory();
    }
    if args.debugger_static_metadata_summary {
        core_options = core_options.with_debugger_static_metadata_summary();
    }
    if args.debugger_static_metadata_source_inventory {
        core_options = core_options.with_debugger_static_metadata_source_inventory();
    }
    if args.debugger_static_metadata_source_provenance {
        core_options = core_options.with_debugger_static_metadata_source_provenance();
    }
    if args.debugger_static_metadata_type_inventory {
        core_options = core_options.with_debugger_static_metadata_type_inventory();
    }
    if args.debugger_static_metadata_type_display {
        core_options = core_options.with_debugger_static_metadata_type_display();
    }
    if args.debugger_static_metadata_symbol_inventory {
        core_options = core_options.with_debugger_static_metadata_symbol_inventory();
    }
    if args.debugger_static_metadata_contract_inventory {
        core_options = core_options.with_debugger_static_metadata_contract_inventory();
    }
    if args.debugger_static_metadata_contract_display {
        core_options = core_options.with_debugger_static_metadata_contract_display();
    }
    if args.debugger_static_metadata_contract_validation {
        core_options = core_options.with_debugger_static_metadata_contract_validation();
    }
    if args.debugger_static_metadata_symbol_display {
        core_options = core_options.with_debugger_static_metadata_symbol_display();
    }
    let core =
        match SpawnedCore::spawn_with_options(args.width, args.height, &frame_dir, core_options) {
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
        let mut registry = registry
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let now = Instant::now();
        registry.register("core", ProcessPolicy::AlwaysResident, None, now);
        registry.register(
            "mcp-server",
            ProcessPolicy::IdleTeardown {
                idle_timeout: Duration::from_secs(300),
            },
            None,
            now,
        );
    }
    let memory_source: Arc<dyn memory_pressure::MemorySource> = if args.simulate_low_memory {
        Arc::new(memory_pressure::FixedMemorySource(0.0))
    } else {
        Arc::new(SystemMemorySource::new())
    };
    let _pressure_monitor = memory_pressure::spawn_pressure_monitor(
        Arc::clone(&registry),
        memory_source,
        memory_pressure::DEFAULT_PRESSURE_THRESHOLD,
        args.memory_poll_interval,
    );

    if let Some(parent) = args.rendezvous_socket.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Some(parent) = args.control_socket.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    // A stale socket file from a previous run (e.g. one that crashed
    // instead of exiting cleanly) makes bind() fail with AddrInUse even
    // though nothing is actually listening -- remove it first, same as
    // `blueice-core.rs` does for its own socket.
    let _ = std::fs::remove_file(&args.rendezvous_socket);
    let _ = std::fs::remove_file(&args.control_socket);

    let listener = match UnixListener::bind(&args.rendezvous_socket) {
        Ok(listener) => listener,
        Err(e) => {
            eprintln!(
                "blueice-launcher: failed to bind {}: {e}",
                args.rendezvous_socket.display()
            );
            return ExitCode::FAILURE;
        }
    };
    let control_listener = match UnixListener::bind(&args.control_socket) {
        Ok(listener) => listener,
        Err(e) => {
            eprintln!(
                "blueice-launcher: failed to bind {}: {e}",
                args.control_socket.display()
            );
            return ExitCode::FAILURE;
        }
    };

    // `run_broker` takes full ownership of `core` from here on --
    // including spawning/health-checking/swapping in a fresh v2 on a
    // `Cutover` control request, and tearing down whichever `core` is
    // currently active before it returns.
    let result = run_broker(listener, control_listener, core, args.width, args.height);

    let _ = std::fs::remove_file(&args.rendezvous_socket);
    let _ = std::fs::remove_file(&args.control_socket);

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("blueice-launcher: broker error: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(not(unix))]
fn main() {
    eprintln!("blueice-launcher is currently supported only on Unix platforms");
    std::process::exit(1);
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn args(flags: &[&str]) -> Result<Args, String> {
        parse_args(flags.iter().map(|s| s.to_string()))
    }

    #[test]
    fn no_flags_uses_the_default_rendezvous_socket_and_default_size() {
        let parsed = args(&[]).unwrap();
        assert_eq!(parsed.rendezvous_socket, default_rendezvous_socket_path());
        assert_eq!(parsed.control_socket, default_control_socket_path());
        assert_eq!(parsed.width, 800.0);
        assert_eq!(parsed.height, 600.0);
        assert_eq!(parsed.frame_dir, None);
        assert_eq!(parsed.gatekeeper_socket, None);
        assert!(!parsed.out_of_process_bluejs);
        assert_eq!(parsed.compiler_mcp_socket, None);
        assert_eq!(parsed.debugger_socket, None);
        assert!(!parsed.debugger_static_metadata_inventory);
        assert!(!parsed.debugger_static_metadata_summary);
        assert!(!parsed.debugger_static_metadata_source_inventory);
        assert!(!parsed.debugger_static_metadata_source_provenance);
        assert!(!parsed.debugger_static_metadata_type_inventory);
        assert!(!parsed.debugger_static_metadata_type_display);
        assert!(!parsed.debugger_static_metadata_symbol_inventory);
        assert!(!parsed.debugger_static_metadata_contract_inventory);
        assert!(!parsed.debugger_static_metadata_contract_display);
        assert!(!parsed.debugger_static_metadata_contract_validation);
        assert!(!parsed.debugger_static_metadata_symbol_display);
        assert!(!parsed.simulate_low_memory);
        assert_eq!(
            parsed.memory_poll_interval,
            memory_pressure::DEFAULT_POLL_INTERVAL
        );
    }

    #[test]
    fn every_flag_is_parsed() {
        let parsed = args(&[
            "--socket",
            "/tmp/x.sock",
            "--control-socket",
            "/tmp/x-control.sock",
            "--width",
            "100",
            "--height",
            "50",
            "--frame-dir",
            "/tmp/frames",
            "--gatekeeper-socket",
            "/tmp/gatekeeper.sock",
            "--out-of-process-bluejs",
            "--compiler-mcp-socket",
            "/tmp/compiler-mcp.sock",
            "--debugger-socket",
            "/tmp/debugger.sock",
            "--debugger-static-metadata-inventory",
            "--debugger-static-metadata-summary",
            "--debugger-static-metadata-source-inventory",
            "--debugger-static-metadata-source-provenance",
            "--debugger-static-metadata-type-inventory",
            "--debugger-static-metadata-type-display",
            "--debugger-static-metadata-symbol-inventory",
            "--debugger-static-metadata-contract-inventory",
            "--debugger-static-metadata-contract-display",
            "--debugger-static-metadata-contract-validation",
            "--debugger-static-metadata-symbol-display",
            "--simulate-low-memory",
            "--memory-poll-interval-ms",
            "50",
        ])
        .unwrap();
        assert_eq!(
            parsed,
            Args {
                rendezvous_socket: PathBuf::from("/tmp/x.sock"),
                control_socket: PathBuf::from("/tmp/x-control.sock"),
                width: 100.0,
                height: 50.0,
                frame_dir: Some(PathBuf::from("/tmp/frames")),
                gatekeeper_socket: Some(PathBuf::from("/tmp/gatekeeper.sock")),
                out_of_process_bluejs: true,
                compiler_mcp_socket: Some(PathBuf::from("/tmp/compiler-mcp.sock")),
                debugger_socket: Some(PathBuf::from("/tmp/debugger.sock")),
                debugger_static_metadata_inventory: true,
                debugger_static_metadata_summary: true,
                debugger_static_metadata_source_inventory: true,
                debugger_static_metadata_source_provenance: true,
                debugger_static_metadata_type_inventory: true,
                debugger_static_metadata_type_display: true,
                debugger_static_metadata_symbol_inventory: true,
                debugger_static_metadata_contract_inventory: true,
                debugger_static_metadata_contract_display: true,
                debugger_static_metadata_contract_validation: true,
                debugger_static_metadata_symbol_display: true,
                simulate_low_memory: true,
                memory_poll_interval: Duration::from_millis(50),
            }
        );
    }

    #[test]
    fn a_control_socket_flag_missing_its_value_is_an_error() {
        assert_eq!(
            args(&["--control-socket"]),
            Err("--control-socket requires a value".to_string())
        );
    }

    #[test]
    fn a_non_numeric_memory_poll_interval_is_an_error() {
        assert_eq!(
            args(&["--memory-poll-interval-ms", "not-a-number"]),
            Err("--memory-poll-interval-ms must be a number".to_string())
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
            args(&["--width", "not-a-number"]),
            Err("--width must be a number".to_string())
        );
    }

    #[test]
    fn a_non_numeric_height_is_an_error() {
        assert_eq!(
            args(&["--height", "not-a-number"]),
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
