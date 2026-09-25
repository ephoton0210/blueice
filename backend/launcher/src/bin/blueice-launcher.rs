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

use blueice_launcher::assistant::{sibling_assistant_binary, AssistantSupervisor};
use blueice_launcher::assistant_settings_service::AssistantSettingsService;
use blueice_launcher::AssistantWiring;
use blueice_launcher::memory_pressure::{self, SystemMemorySource};
use blueice_launcher::supervisor::ProcessRegistry;
use blueice_launcher::{
    default_control_socket_path, default_rendezvous_socket_path,
    run_broker_with_options, BrokerOptions, SpawnedCore, SpawnedGatekeeper, SpawnedTrustedWindow,
};
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

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
    /// One installed package is validated and hosted by each core generation.
    /// Optional grants remain process-lifetime and do not cross a cutover.
    extension_manifest: Option<PathBuf>,
    /// Spawn the exact sibling native frontend with private anonymous pipes.
    /// Its native confirmation panel may change installed optional grants;
    /// neither shared browser IPC nor the operator socket gains that route.
    trusted_frontend: bool,
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
    /// Supervise `blueice-ai-assistant` (`phase-7-local-ai/PLAN.md`, step R2),
    /// configured by this settings file. `--assistant` selects the default
    /// path. Without either, no assistant is supervised and `core` is given
    /// no assistant socket.
    assistant_settings: Option<PathBuf>,
    /// Test/debug-only override of the assistant binary; by default it is the
    /// sibling of this launcher.
    assistant_bin: Option<PathBuf>,
    /// Poll the `blueice-core` binary this often (in seconds) and cut over to a
    /// newer one automatically (`phase-8-live-core-hotswap/PLAN.md`). Off by
    /// default, so upgrading never changes a running deployment's behavior.
    auto_update: Option<Duration>,
}

/// Takes an injectable argument iterator for the same reason
/// `blueice-core.rs`'s `parse_args` does: every flag-parsing branch is a
/// plain unit test, not something only exercisable by actually spawning
/// the binary.
fn parse_args(args: impl Iterator<Item = String>) -> Result<Args, String> {
    let mut rendezvous_socket = None;
    let mut control_socket = None;
    let mut width = 800.0;
    let mut height = 600.0;
    let mut frame_dir = None;
    let mut extension_manifest = None;
    let mut trusted_frontend = false;
    let mut simulate_low_memory = false;
    let mut memory_poll_interval = memory_pressure::DEFAULT_POLL_INTERVAL;
    let mut assistant_settings = None;
    let mut assistant_bin = None;
    let mut auto_update = None;

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
            "--extension-manifest" => extension_manifest = Some(PathBuf::from(value()?)),
            "--trusted-frontend" => trusted_frontend = true,
            "--assistant" => {
                assistant_settings.get_or_insert_with(blueice_assistant_settings::default_settings_path);
            }
            "--assistant-settings" => assistant_settings = Some(PathBuf::from(value()?)),
            "--assistant-bin" => assistant_bin = Some(PathBuf::from(value()?)),
            "--auto-update-secs" => {
                let secs: u64 = value()?
                    .parse()
                    .ok()
                    .filter(|secs| *secs > 0)
                    .ok_or_else(|| "--auto-update-secs must be a positive number".to_string())?;
                auto_update = Some(Duration::from_secs(secs));
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
    Ok(Args {
        rendezvous_socket,
        control_socket,
        width,
        height,
        frame_dir,
        extension_manifest,
        trusted_frontend,
        simulate_low_memory,
        memory_poll_interval,
        assistant_settings,
        assistant_bin,
        auto_update,
    })
}

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

    let gatekeeper = match SpawnedGatekeeper::spawn() {
        Ok(gatekeeper) => gatekeeper,
        Err(e) => {
            eprintln!("blueice-launcher: failed to spawn blueice-ai-gatekeeper: {e}");
            return ExitCode::FAILURE;
        }
    };

    // The registry exists before `core` so the assistant supervisor can register
    // itself first: `core` is started knowing the assistant's public socket.
    // `phase-8-live-core-hotswap/PLAN.md`'s fleet-memory-supervisor minimal
    // first slice: the roles this launcher supervises are registered in one
    // place (`ProcessRegistry::default_fleet`: `core` and `ai-gatekeeper`
    // `AlwaysResident`; `mcp-server` an inert `IdleTeardown` slot), plus
    // `ai-assistant` `OnDemand` when configured. Downloads stays outside this
    // generic time-only supervisor until it exposes an idle query that proves
    // no transfer is active or queued.
    let registry = Arc::new(Mutex::new(ProcessRegistry::default_fleet(Instant::now())));

    // The supervisor and the service that owns its settings. (The service holds
    // only a weak reference, so `drop(assistant)` below really stops the child.)
    let mut assistant_settings_service = None;
    let assistant = match args.assistant_settings.as_deref() {
        None => None,
        Some(path) => {
            let settings = match blueice_assistant_settings::load(path) {
                Ok(settings) => settings,
                Err(message) => {
                    eprintln!("blueice-launcher: {message}");
                    return ExitCode::FAILURE;
                }
            };
            let binary = args.assistant_bin.clone().unwrap_or_else(|| {
                sibling_assistant_binary(&std::env::current_exe().unwrap_or_default())
            });
            match AssistantSupervisor::start(&settings, binary, Arc::clone(&registry)) {
                Ok(assistant) => {
                    let assistant = Arc::new(assistant);
                    assistant_settings_service = Some(Arc::new(AssistantSettingsService::new(
                        path.to_path_buf(),
                        settings.clone(),
                        Arc::downgrade(&assistant),
                    )));
                    Some(assistant)
                }
                Err(e) => {
                    eprintln!("blueice-launcher: could not supervise the assistant: {e}");
                    return ExitCode::FAILURE;
                }
            }
        }
    };

    // What every core (v1 and each cutover's replacement) is told about it.
    let assistant_wiring = assistant.as_ref().map(|supervisor| AssistantWiring {
        socket: supervisor.public_socket().to_path_buf(),
        settings_file: args.assistant_settings.clone(),
    });

    let core = match SpawnedCore::spawn_with_assistant(
        args.width,
        args.height,
        &frame_dir,
        gatekeeper.socket_path(),
        args.extension_manifest.as_deref(),
        assistant_wiring.as_ref(),
    ) {
        Ok(core) => core,
        Err(e) => {
            eprintln!("blueice-launcher: failed to spawn blueice-core: {e}");
            return ExitCode::FAILURE;
        }
    };

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

    let trusted_window = if args.trusted_frontend {
        match SpawnedTrustedWindow::spawn(&args.rendezvous_socket) {
            Ok(window) => Some(window),
            Err(error) => {
                eprintln!("blueice-launcher: could not start trusted frontend: {error}");
                let _ = std::fs::remove_file(&args.rendezvous_socket);
                let _ = std::fs::remove_file(&args.control_socket);
                return ExitCode::FAILURE;
            }
        }
    } else {
        None
    };

    // The broker takes full ownership of `core` from here on --
    // including spawning/health-checking/swapping in a fresh v2 on a
    // `Cutover` control request, and tearing down whichever `core` is
    // currently active before it returns.
    let result = run_broker_with_options(
        listener,
        control_listener,
        core,
        args.width,
        args.height,
        gatekeeper.socket_path().to_path_buf(),
        BrokerOptions {
            trusted_window,
            auto_update_interval: args.auto_update,
            assistant_settings: assistant_settings_service,
        },
    );

    let _ = std::fs::remove_file(&args.rendezvous_socket);
    let _ = std::fs::remove_file(&args.control_socket);
    drop(assistant);
    drop(gatekeeper);

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
        assert_eq!(parsed.control_socket, default_control_socket_path());
        assert_eq!(parsed.width, 800.0);
        assert_eq!(parsed.height, 600.0);
        assert_eq!(parsed.frame_dir, None);
        assert_eq!(parsed.extension_manifest, None);
        assert!(!parsed.trusted_frontend);
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
            "--extension-manifest",
            "/tmp/extension.json",
            "--trusted-frontend",
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
                extension_manifest: Some(PathBuf::from("/tmp/extension.json")),
                trusted_frontend: true,
                simulate_low_memory: true,
                memory_poll_interval: Duration::from_millis(50),
                assistant_settings: None,
                assistant_bin: None,
                auto_update: None,
            }
        );
    }

    #[test]
    fn assistant_flags_are_parsed_and_the_default_path_is_the_shared_default() {
        let none = args(&[]).unwrap();
        assert_eq!(none.assistant_settings, None);
        assert_eq!(none.assistant_bin, None);
        let explicit = args(&[
            "--assistant-settings",
            "/tmp/a.json",
            "--assistant-bin",
            "/tmp/assistant",
        ])
        .unwrap();
        assert_eq!(explicit.assistant_settings, Some(PathBuf::from("/tmp/a.json")));
        assert_eq!(explicit.assistant_bin, Some(PathBuf::from("/tmp/assistant")));
        let default = args(&["--assistant"]).unwrap();
        assert_eq!(
            default.assistant_settings,
            Some(blueice_assistant_settings::default_settings_path())
        );
        // An explicit path wins over `--assistant`, in either order.
        for flags in [
            ["--assistant", "--assistant-settings", "/tmp/a.json"],
            ["--assistant-settings", "/tmp/a.json", "--assistant"],
        ] {
            assert_eq!(
                args(&flags).unwrap().assistant_settings,
                Some(PathBuf::from("/tmp/a.json")),
                "{flags:?}"
            );
        }
        assert!(args(&["--assistant-settings"]).is_err());
    }

    #[test]
    fn the_auto_update_interval_is_opt_in_and_must_be_positive() {
        assert_eq!(args(&[]).unwrap().auto_update, None);
        assert_eq!(
            args(&["--auto-update-secs", "30"]).unwrap().auto_update,
            Some(Duration::from_secs(30))
        );
        for bad in ["0", "soon", "-5"] {
            assert!(args(&["--auto-update-secs", bad]).is_err(), "{bad}");
        }
        assert!(args(&["--auto-update-secs"]).is_err());
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
