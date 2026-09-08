// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

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

use blueice_ipc::{read_server_message, write_client_message, ClientMessage, ServerMessage};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::thread;
use std::time::{Duration, Instant};

fn unique_path(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!("blueice-launcher-e2e-{label}-{}", std::process::id()))
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
    fn spawn() -> Self {
        let rendezvous_socket = unique_path("rendezvous.sock");
        let frame_dir = unique_path("frames");
        let _ = std::fs::remove_file(&rendezvous_socket);
        let _ = std::fs::remove_dir_all(&frame_dir);

        let child = Command::new(env!("CARGO_BIN_EXE_blueice-launcher"))
            .args(["--socket", rendezvous_socket.to_str().unwrap(), "--width", "320", "--height", "200", "--frame-dir", frame_dir.to_str().unwrap()])
            .spawn()
            .expect("failed to spawn blueice-launcher");

        assert!(wait_for(&rendezvous_socket, Duration::from_secs(5)), "blueice-launcher never created its rendezvous socket");
        Launcher { child, rendezvous_socket, frame_dir }
    }

    fn connect(&self) -> UnixStream {
        UnixStream::connect(&self.rendezvous_socket).expect("failed to connect to the launcher's rendezvous socket")
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
        let _ = std::fs::remove_dir_all(&self.frame_dir);
    }
}

#[test]
fn an_unrecognized_argument_exits_with_failure_and_creates_no_socket() {
    let rendezvous_socket = unique_path("bad-args-rendezvous.sock");
    let _ = std::fs::remove_file(&rendezvous_socket);

    let output = Command::new(env!("CARGO_BIN_EXE_blueice-launcher")).args(["--socket", rendezvous_socket.to_str().unwrap(), "--bogus"]).output().expect("failed to run blueice-launcher");

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("--bogus"));
    assert!(!rendezvous_socket.exists(), "a launcher that failed argument parsing must never spawn core or bind a socket");
}

#[test]
fn two_clients_through_the_same_launcher_observe_the_same_render_pass() {
    let mut launcher = Launcher::spawn();

    // Two independent connections to the *one* rendezvous socket,
    // standing in for a human's `frontend` and an AI's `mcp-server`.
    let mut human = launcher.connect();
    let mut ai = launcher.connect();

    // Navigate to a real (built-in, no network needed) page first --
    // `blueice-core` starts with a completely blank `Page`, whose
    // rendered content has no boxes at all, so its cropped-to-viewport
    // pixmap width would be clamped to a content-derived minimum rather
    // than actually reflecting the resize below.
    write_client_message(&mut human, &ClientMessage::Navigate { url: "about:credits".to_string() }).unwrap();
    let navigated = read_server_message(&mut human).unwrap();
    assert!(matches!(navigated, ServerMessage::Navigated { .. }));
    let initial_frame = read_server_message(&mut human).unwrap();
    assert!(matches!(initial_frame, ServerMessage::FrameReady { .. }));
    // The other connection must see this initial navigation too.
    assert!(matches!(read_server_message(&mut ai).unwrap(), ServerMessage::Navigated { .. }));
    assert!(matches!(read_server_message(&mut ai).unwrap(), ServerMessage::FrameReady { .. }));

    // The AI's action (an ActOn-free, plain Resize here for simplicity)
    // must produce a FrameReady the *human* connection also receives,
    // even though the human connection never sent anything itself --
    // this is the actual "same render pass" property, not just "both
    // connections work independently."
    write_client_message(&mut ai, &ClientMessage::Resize { width: 111, height: 222 }).unwrap();

    let ai_frame = read_server_message(&mut ai).unwrap();
    let ServerMessage::FrameReady { generation: ai_generation, width: 111, height: 222, .. } = ai_frame else { panic!("expected FrameReady, got {ai_frame:?}") };

    let human_frame = read_server_message(&mut human).unwrap();
    let ServerMessage::FrameReady { generation: human_generation, width: 111, height: 222, .. } = human_frame else { panic!("expected the human connection to also see the resize's FrameReady, got {human_frame:?}") };
    assert_eq!(ai_generation, human_generation, "both connections must see the identical generation for the same state change");

    // Now the *human* connection acts (Scroll), and the AI connection
    // -- via GetRepresentation -- must report that exact same new
    // generation, proving the sharing holds in both directions.
    write_client_message(&mut human, &ClientMessage::Scroll { delta_y: 5.0 }).unwrap();
    let human_frame2 = read_server_message(&mut human).unwrap();
    let ServerMessage::FrameReady { generation: scroll_generation, .. } = human_frame2 else { panic!("expected FrameReady, got {human_frame2:?}") };
    assert!(scroll_generation > human_generation);

    // Drain the same broadcast off the AI connection first (it must see
    // it too, unprompted).
    let ai_frame2 = read_server_message(&mut ai).unwrap();
    assert!(matches!(ai_frame2, ServerMessage::FrameReady { generation, .. } if generation == scroll_generation));

    write_client_message(&mut ai, &ClientMessage::GetRepresentation).unwrap();
    let reply = read_server_message(&mut ai).unwrap();
    let ServerMessage::Representation(snapshot) = reply else { panic!("expected Representation, got {reply:?}") };
    assert_eq!(snapshot.generation, scroll_generation, "GetRepresentation must reflect the state the *other* connection's action just produced");

    // This broadcasts to *human* too, same as every other reply in this
    // test -- drain it the same way a real always-reading client
    // (`frontend`'s own background reader thread) would, rather than
    // leaving it unread: `about:credits`'s `Representation` is large
    // enough that leaving it sitting in the kernel socket buffer can
    // exhaust it, and `register_client`'s bounded write timeout would
    // then have to kick in as `broadcast_core_to_clients` blocks
    // delivering it -- correct, but a needless multi-second stall this
    // test doesn't need to exercise.
    let human_reply = read_server_message(&mut human).unwrap();
    assert!(matches!(human_reply, ServerMessage::Representation(_)));

    // `Shutdown` from *either* client is forwarded into the one shared
    // `core` connection like any other message, ending `core`'s own
    // session loop; that in turn closes the launcher's internal
    // connection to it, ending the broadcaster loop and letting
    // `main()` return and exit normally -- proving the cascade, and
    // letting the process exit on its own rather than being killed.
    write_client_message(&mut human, &ClientMessage::Shutdown).unwrap();
    launcher.wait_or_kill(Duration::from_secs(5));
}
