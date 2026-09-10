// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The end-to-end proof `phase-8-live-core-hotswap/PLAN.md`'s fleet
//! memory supervisor checklist asks for: driving the real compiled
//! `blueice-launcher` binary (and, transitively, the real `blueice-core`
//! it spawns) under simulated memory pressure, and confirming `core` --
//! registered `AlwaysResident` -- survives regardless. The pure
//! decision logic (`ProcessRegistry`/`poll_once`) is already unit- and
//! real-child-process-tested in `blueice_launcher`'s own `supervisor`/
//! `memory_pressure` modules; this is the "drive the real thing, not a
//! mock" boundary test one layer up, matching `broker_end_to_end.rs`'s
//! existing strategy.

use blueice_ipc::{read_server_message, write_client_message, ClientMessage, ServerMessage};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::thread;
use std::time::{Duration, Instant};

fn unique_path(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "blueice-launcher-supervisor-{label}-{}",
        std::process::id()
    ))
}

fn wait_for(path: &std::path::Path, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if path.exists() {
            return true;
        }
        thread::sleep(Duration::from_millis(20));
    }
    false
}

struct Launcher {
    child: Child,
    rendezvous_socket: PathBuf,
    frame_dir: PathBuf,
}

impl Launcher {
    fn spawn_under_simulated_pressure() -> Self {
        let rendezvous_socket = unique_path("rendezvous.sock");
        let frame_dir = unique_path("frames");
        let _ = std::fs::remove_file(&rendezvous_socket);
        let _ = std::fs::remove_dir_all(&frame_dir);

        let child = Command::new(env!("CARGO_BIN_EXE_blueice-launcher"))
            .args([
                "--socket",
                rendezvous_socket.to_str().unwrap(),
                "--width",
                "320",
                "--height",
                "200",
                "--frame-dir",
                frame_dir.to_str().unwrap(),
                "--simulate-low-memory",
                "--memory-poll-interval-ms",
                "50",
            ])
            .spawn()
            .expect("failed to spawn blueice-launcher");

        assert!(
            wait_for(&rendezvous_socket, Duration::from_secs(5)),
            "blueice-launcher never created its rendezvous socket"
        );
        Launcher {
            child,
            rendezvous_socket,
            frame_dir,
        }
    }

    fn connect(&self) -> UnixStream {
        UnixStream::connect(&self.rendezvous_socket)
            .expect("failed to connect to the launcher's rendezvous socket")
    }

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
        let _ = std::fs::remove_dir_all(&self.frame_dir);
    }
}

#[test]
fn core_survives_many_simulated_low_memory_polls_since_it_is_always_resident() {
    let mut launcher = Launcher::spawn_under_simulated_pressure();

    // The 50ms poll interval means several real pressure-response
    // cycles run well within this sleep -- long enough to prove
    // survival isn't just "the poll hasn't fired yet."
    thread::sleep(Duration::from_millis(400));

    // `core` being alive and answering is the only externally-observable
    // proof that matters: the registry itself is launcher-internal
    // state with no IPC exposure, but if `core`'s `AlwaysResident` entry
    // had been (incorrectly) torn down, this connection -- routed
    // through the launcher into the one `core` connection -- would find
    // nothing on the other end.
    let mut client = launcher.connect();
    write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
    let reply = read_server_message(&mut client).unwrap();
    assert!(
        matches!(reply, ServerMessage::Representation(_)),
        "expected a real Representation reply from a still-alive core, got {reply:?}"
    );

    write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    launcher.wait_or_kill(Duration::from_secs(5));
}
