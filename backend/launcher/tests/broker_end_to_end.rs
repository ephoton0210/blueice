// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![cfg(unix)]

//! The concrete, checkable proof `phase-8-live-core-hotswap/PLAN.md`'s
//! "Minimal first slice" checklist asks for: two independent client
//! connections through the *same* real `blueice-launcher` subprocess
//! (standing in for `frontend` and `mcp-server`) observe the *same*
//! render pass -- an action on one connection produces a `FrameReady`
//! the *other* connection also receives, and a `GetRepresentation` on
//! either connection reports the same `generation` either one observed.
//! Exercises the real compiled `blueice-launcher` (and, transitively,
//! the real `blueice-core` it spawns) as actual subprocesses over real
//! Unix sockets, the same strategy `blueice-engine`'s
//! `tests/core_binary.rs` uses one layer down. Headless and
//! display-free -- there is no reason this can't run in CI.
//!
//! Also proves `phase-8-live-core-hotswap/PLAN.md`'s cutover mechanism
//! end to end: a client connected before a `Cutover` control request
//! is never dropped by it, and sees v2's replayed tab state afterward
//! on that same, still-open connection -- and, on the failure side,
//! that a cutover which can't complete leaves v1 serving every
//! already-connected client exactly as before.

use blueice_ipc::owner_bootstrap::{
    OwnerHttpOriginRule, OwnerHttpPolicyBootstrap, OwnerHttpResource,
};
use blueice_ipc::{
    read_server_message, read_server_message_with_id, read_server_message_with_ids,
    write_client_message, write_client_message_with_id, ClientMessage, ServerMessage,
};
use blueice_launcher::control::{
    read_control_reply, write_control_request, ControlReply, ControlRequest,
};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::{Arc, Barrier, Mutex};
use std::thread;
use std::time::{Duration, Instant};

/// A per-call-unique path, not just per-process: `cargo test` runs every
/// test in one binary's tests concurrently, on separate threads of the
/// *same* process, so a PID-only path would let two concurrently-running
/// tests' `Launcher::spawn()` calls collide on the exact same rendezvous/
/// control socket or frame directory (this crate now has more than one
/// test that spawns a `Launcher`, unlike when this file had only one).
fn unique_path(label: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "blueice-launcher-e2e-{label}-{}-{n}",
        std::process::id()
    ))
}

/// The launcher delegates ordinary HTTP navigation to the core, which retains
/// the existing fail-closed gatekeeper requirement. This isolated real socket
/// lets the private-host regression prove page execution rather than merely a
/// startup handshake.
fn clearing_gatekeeper() -> PathBuf {
    let path = unique_path("gatekeeper.sock");
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path).expect("test gatekeeper must bind");
    thread::spawn(move || {
        for incoming in listener.incoming() {
            let Ok(mut stream) = incoming else { break };
            let _ = blueice_ai_gatekeeper::handle_one_check(&mut stream);
        }
    });
    path
}

// `cargo llvm-cov` instruments the launcher and the core that it starts. On
// a busy CI runner their initial dynamic linking and coverage setup can take
// longer than the ordinary test-profile startup, so this is deliberately a
// bounded readiness deadline rather than a five-second scheduling assumption.
const LAUNCHER_STARTUP_TIMEOUT: Duration = Duration::from_secs(30);

// Each case starts at least one real launcher/core tree, and some supervise a
// BlueJS child. Keep process teardown within its existing bounded deadline
// while preserving the explicit concurrency exercised inside cutover tests.
static BROKER_TEST_LOCK: Mutex<()> = Mutex::new(());

fn broker_test_guard() -> std::sync::MutexGuard<'static, ()> {
    BROKER_TEST_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Mirrors `blueice_launcher::v2_frame_dir`'s exact (private, so not
/// directly callable from here) naming scheme, so this test can predict
/// -- and deliberately block -- the frame directory a cutover attempt
/// will give v2. See `a_cutover_that_fails_during_replay_leaves_v1_serving_normally`.
fn expected_v2_frame_dir(v1_frame_dir: &Path, target_generation: u64) -> PathBuf {
    let stem = v1_frame_dir
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    v1_frame_dir.with_file_name(format!("{stem}-cutover-{target_generation}"))
}

fn installed_extension_package() -> (PathBuf, PathBuf) {
    let package_root = unique_path("extension-package");
    std::fs::create_dir_all(&package_root).unwrap();
    let manifest = package_root.join("extension.json");
    std::fs::write(
        &manifest,
        r#"{"name":"Launcher cutover test","version":"1.0.0","blueice_api_version":1,"entry_point":"extension.wasm","capabilities":{"declared":["dom:read"],"optional":["storage"]}}"#,
    )
    .unwrap();
    std::fs::write(
        package_root.join("extension.wasm"),
        wat::parse_str(r#"(module (func (export "blueice_start")))"#).unwrap(),
    )
    .unwrap();
    (package_root, manifest)
}

struct Launcher {
    child: Child,
    rendezvous_socket: PathBuf,
    control_socket: PathBuf,
    frame_dir: PathBuf,
    /// Kept only by this lifecycle test. The launcher never publishes this
    /// child-only path to frontend or page traffic.
    private_bluejs_socket: Option<PathBuf>,
}

impl Launcher {
    fn spawn() -> Self {
        Self::spawn_with_options(None, false, None, None)
    }

    fn spawn_with_manifest(manifest: Option<&Path>) -> Self {
        Self::spawn_with_options(None, false, None, manifest)
    }

    fn spawn_with_supervised_bluejs(gatekeeper_socket: &Path) -> Self {
        Self::spawn_with_options(Some(gatekeeper_socket), true, None, None)
    }

    fn spawn_with_supervised_bluejs_and_owner_http_policy(
        gatekeeper_socket: &Path,
        policy_file: &Path,
    ) -> Self {
        Self::spawn_with_options(Some(gatekeeper_socket), true, Some(policy_file), None)
    }

    fn spawn_with_options(
        gatekeeper_socket: Option<&Path>,
        supervise_out_of_process_bluejs: bool,
        page_http_policy_file: Option<&Path>,
        extension_manifest: Option<&Path>,
    ) -> Self {
        let rendezvous_socket = unique_path("rendezvous.sock");
        let control_socket = unique_path("control.sock");
        let frame_dir = unique_path("frames");
        let _ = std::fs::remove_file(&rendezvous_socket);
        let _ = std::fs::remove_file(&control_socket);
        let _ = std::fs::remove_dir_all(&frame_dir);

        let mut command = Command::new(env!("CARGO_BIN_EXE_blueice-launcher"));
        command.args([
            "--socket",
            rendezvous_socket.to_str().unwrap(),
            "--control-socket",
            control_socket.to_str().unwrap(),
            "--width",
            "320",
            "--height",
            "200",
            "--frame-dir",
            frame_dir.to_str().unwrap(),
        ]);
        if let Some(gatekeeper_socket) = gatekeeper_socket {
            command.arg("--gatekeeper-socket").arg(gatekeeper_socket);
        }
        if supervise_out_of_process_bluejs {
            command.arg("--out-of-process-bluejs");
        }
        if let Some(path) = page_http_policy_file {
            command.arg("--page-http-policy-file").arg(path);
        }
        if let Some(manifest) = extension_manifest {
            command.arg("--extension-manifest").arg(manifest);
        }
        let child = command.spawn().expect("failed to spawn blueice-launcher");

        // Construct the RAII owner before making assertions about process
        // readiness. Previously a timeout panicked while `child` was a bare
        // local, so the launcher (and its core) could survive the failed test.
        let private_bluejs_socket = supervise_out_of_process_bluejs.then(|| {
            std::env::temp_dir().join(format!(
                "blueice-launcher-bluejs-host-{}-0.sock",
                child.id()
            ))
        });
        let mut launcher = Launcher {
            child,
            rendezvous_socket,
            control_socket,
            frame_dir,
            private_bluejs_socket,
        };
        let rendezvous_socket = launcher.rendezvous_socket.clone();
        launcher.wait_for_socket(&rendezvous_socket, "rendezvous");
        let control_socket = launcher.control_socket.clone();
        launcher.wait_for_socket(&control_socket, "control");
        launcher
    }

    fn wait_for_socket(&mut self, path: &Path, name: &str) {
        let deadline = Instant::now() + LAUNCHER_STARTUP_TIMEOUT;
        loop {
            if path.exists() {
                return;
            }
            if let Some(status) = self
                .child
                .try_wait()
                .expect("failed to observe blueice-launcher while waiting for startup")
            {
                panic!(
                    "blueice-launcher exited before creating its {name} socket; \
                     child status: {status}"
                );
            }
            if Instant::now() >= deadline {
                self.wait_or_kill(Duration::from_secs(1));
                panic!(
                    "blueice-launcher never created its {name} socket within \
                     {LAUNCHER_STARTUP_TIMEOUT:?}; child was reaped during cleanup"
                );
            }
            thread::sleep(Duration::from_millis(20));
        }
    }

    fn connect(&self) -> UnixStream {
        UnixStream::connect(&self.rendezvous_socket)
            .expect("failed to connect to the launcher's rendezvous socket")
    }

    fn connect_control(&self) -> UnixStream {
        UnixStream::connect(&self.control_socket)
            .expect("failed to connect to the launcher's control socket")
    }

    fn inspect_extension_permissions(
        &self,
    ) -> (
        u64,
        Option<blueice_launcher::control::InstalledExtensionPermissions>,
    ) {
        let mut control = self.connect_control();
        write_control_request(&mut control, &ControlRequest::InspectExtensionPermissions).unwrap();
        match read_control_reply(&mut control).unwrap() {
            ControlReply::ExtensionPermissions {
                core_generation,
                installed,
            } => (core_generation, installed),
            other => panic!("expected a read-only extension inspection, got {other:?}"),
        }
    }

    /// The internal socket path `blueice_launcher::SpawnedCore::spawn`
    /// gives v1 -- deterministic given the launcher's own PID, since
    /// `main()` calls `SpawnedCore::spawn` exactly once, before any
    /// cutover, making v1's spawn the very first call in the launcher
    /// process's lifetime (its internal per-call counter starts at 0).
    /// Used to prove v1 is actually torn down after a successful
    /// cutover: `SpawnedCore::Drop` removes this file after the process
    /// has exited and been reaped (with a bounded force-kill fallback).
    fn v1_internal_socket_path(&self) -> PathBuf {
        std::env::temp_dir().join(format!("blueice-launcher-core-{}-0.sock", self.child.id()))
    }

    /// `SpawnedCore` gives each generation a fresh private extension socket.
    /// Its counter starts at zero in each launcher process, independently of
    /// the ordinary core IPC socket counter.
    fn extension_socket_path(&self, generation_index: u64) -> PathBuf {
        blueice_ipc::local_socket::default_socket_dir()
            .join(format!("l-ext-{}-{generation_index}.sock", self.child.id()))
    }

    /// Waits for the launcher to exit on its own (e.g. after a
    /// `Shutdown` cascades through `core` and closes the launcher's own
    /// internal connection), falling back to a hard kill only if it
    /// doesn't within `timeout` -- letting the process exit normally
    /// wherever possible is what lets its coverage instrumentation
    /// actually flush (a `kill()` sends SIGKILL, which skips that).
    fn wait_or_kill(&mut self, timeout: Duration) {
        let deadline = Instant::now() + timeout;
        loop {
            if self.child.try_wait().ok().flatten().is_some() {
                return;
            }
            if Instant::now() >= deadline {
                let _ = self.child.kill();
                let _ = self.child.wait();
                return;
            }
            thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for Launcher {
    fn drop(&mut self) {
        self.wait_or_kill(Duration::from_secs(1));
        let _ = std::fs::remove_file(&self.rendezvous_socket);
        let _ = std::fs::remove_file(&self.control_socket);
        if let Some(path) = &self.private_bluejs_socket {
            let _ = std::fs::remove_file(path);
        }
        let _ = std::fs::remove_dir_all(&self.frame_dir);
    }
}

#[path = "broker_end_to_end/transport.rs"]
mod transport;

#[path = "broker_end_to_end/permissions.rs"]
mod permissions;

#[path = "broker_end_to_end/lifecycle.rs"]
mod lifecycle;

#[path = "broker_end_to_end/cutover.rs"]
mod cutover;

#[path = "broker_end_to_end/extensions.rs"]
mod extensions;
