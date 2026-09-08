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
//!
//! Also proves `phase-8-live-core-hotswap/PLAN.md`'s cutover mechanism
//! end to end: a client connected before a `Cutover` control request
//! is never dropped by it, and sees v2's replayed tab state afterward
//! on that same, still-open connection -- and, on the failure side,
//! that a cutover which can't complete leaves v1 serving every
//! already-connected client exactly as before.

use blueice_ipc::{read_server_message, read_server_message_with_id, write_client_message, write_client_message_with_id, ClientMessage, ServerMessage};
use blueice_launcher::control::{read_control_reply, write_control_request, ControlReply, ControlRequest};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
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
    std::env::temp_dir().join(format!("blueice-launcher-e2e-{label}-{}-{n}", std::process::id()))
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

/// Mirrors `blueice_launcher::v2_frame_dir`'s exact (private, so not
/// directly callable from here) naming scheme, so this test can predict
/// -- and deliberately block -- the frame directory a cutover attempt
/// will give v2. See `a_cutover_that_fails_during_replay_leaves_v1_serving_normally`.
fn expected_v2_frame_dir(v1_frame_dir: &Path, target_generation: u64) -> PathBuf {
    let stem = v1_frame_dir.file_name().unwrap().to_string_lossy().into_owned();
    v1_frame_dir.with_file_name(format!("{stem}-cutover-{target_generation}"))
}

struct Launcher {
    child: Child,
    rendezvous_socket: PathBuf,
    control_socket: PathBuf,
    frame_dir: PathBuf,
}

impl Launcher {
    fn spawn() -> Self {
        let rendezvous_socket = unique_path("rendezvous.sock");
        let control_socket = unique_path("control.sock");
        let frame_dir = unique_path("frames");
        let _ = std::fs::remove_file(&rendezvous_socket);
        let _ = std::fs::remove_file(&control_socket);
        let _ = std::fs::remove_dir_all(&frame_dir);

        let child = Command::new(env!("CARGO_BIN_EXE_blueice-launcher"))
            .args([
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
            ])
            .spawn()
            .expect("failed to spawn blueice-launcher");

        assert!(wait_for(&rendezvous_socket, Duration::from_secs(5)), "blueice-launcher never created its rendezvous socket");
        assert!(wait_for(&control_socket, Duration::from_secs(5)), "blueice-launcher never created its control socket");
        Launcher { child, rendezvous_socket, control_socket, frame_dir }
    }

    fn connect(&self) -> UnixStream {
        UnixStream::connect(&self.rendezvous_socket).expect("failed to connect to the launcher's rendezvous socket")
    }

    fn connect_control(&self) -> UnixStream {
        UnixStream::connect(&self.control_socket).expect("failed to connect to the launcher's control socket")
    }

    /// The internal socket path `blueice_launcher::SpawnedCore::spawn`
    /// gives v1 -- deterministic given the launcher's own PID, since
    /// `main()` calls `SpawnedCore::spawn` exactly once, before any
    /// cutover, making v1's spawn the very first call in the launcher
    /// process's lifetime (its internal per-call counter starts at 0).
    /// Used to prove v1 is actually torn down after a successful
    /// cutover: `SpawnedCore::Drop` removes this file, and only runs
    /// once the process has actually been killed and reaped.
    fn v1_internal_socket_path(&self) -> PathBuf {
        std::env::temp_dir().join(format!("blueice-launcher-core-{}-0.sock", self.child.id()))
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

#[test]
fn shutdown_with_no_cutover_still_ends_the_whole_launcher_cleanly() {
    // A focused regression test that the generation-counter/cutover
    // restructuring didn't change this pre-existing behavior: with no
    // `Cutover` ever sent, a single client's `Shutdown` must still
    // cascade through `core` and end the launcher process, exactly as
    // it did before this phase's cutover mechanism existed (see
    // `two_clients_through_the_same_launcher_observe_the_same_render_pass`,
    // which also exercises this as a side effect -- this test isolates
    // it as its own explicit assertion).
    let mut launcher = Launcher::spawn();
    let mut client = launcher.connect();

    write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();

    let deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        if let Some(status) = launcher.child.try_wait().unwrap() {
            break status;
        }
        assert!(Instant::now() < deadline, "the launcher must exit on its own after a Shutdown with no cutover involved");
        thread::sleep(Duration::from_millis(20));
    };
    assert!(status.success(), "the launcher must exit cleanly (not be killed) after an ordinary Shutdown cascade");
}

#[test]
fn a_client_survives_a_cutover_and_sees_v2s_replayed_state() {
    let mut launcher = Launcher::spawn();
    let mut client = launcher.connect();

    // Give v1 two tabs with distinct, real (built-in, no network
    // needed) content, so cutover has something non-trivial to capture
    // and replay.
    write_client_message(&mut client, &ClientMessage::Navigate { url: "about:credits".to_string() }).unwrap();
    assert!(matches!(read_server_message(&mut client).unwrap(), ServerMessage::Navigated { .. }));
    assert!(matches!(read_server_message(&mut client).unwrap(), ServerMessage::FrameReady { .. }));

    write_client_message(&mut client, &ClientMessage::OpenTab { url: Some("about:blank".to_string()) }).unwrap();
    let opened = read_server_message(&mut client).unwrap();
    assert!(matches!(opened, ServerMessage::TabOpened { url: Some(_), .. }), "expected the second tab to have navigated, got {opened:?}");
    assert!(matches!(read_server_message(&mut client).unwrap(), ServerMessage::FrameReady { .. }));

    let v1_internal_socket = launcher.v1_internal_socket_path();
    assert!(v1_internal_socket.exists(), "sanity check: v1 must actually be up before cutover");

    // Trigger the cutover, on a *separate* connection to the control
    // socket -- the client above never touches this socket at all.
    let mut control = launcher.connect_control();
    write_control_request(&mut control, &ControlRequest::Cutover).unwrap();
    let reply = read_control_reply(&mut control).unwrap();
    let ControlReply::CutoverDone { tabs_migrated } = reply else { panic!("expected CutoverDone, got {reply:?}") };
    assert_eq!(tabs_migrated, 2);

    // (c) v1's process is actually dead: `SpawnedCore::Drop` removes
    // its internal socket file, and only runs once the child has
    // actually been killed and reaped.
    assert!(!v1_internal_socket.exists(), "v1's internal socket must be gone once cutover has torn it down");

    // (a) the *original* client connection was never dropped by the
    // cutover -- it can still send and receive on the exact same
    // socket, now served by v2. Tagged with its own request_id and
    // filtered accordingly: the tab-capture step's own `ListTabs`
    // (tagged with a synthetic id) was also broadcast to this same
    // client as a side effect of sharing the broker, and must not be
    // mistaken for this request's own reply -- exactly the id-based
    // filtering discipline Part 1's fix exists to make possible through
    // a shared broker connection.
    write_client_message_with_id(&mut client, Some(777), &ClientMessage::ListTabs).unwrap();
    let tabs = loop {
        let (reply_id, message) = read_server_message_with_id(&mut client).unwrap();
        if matches!(reply_id, Some(id) if id != 777) {
            continue;
        }
        match message {
            ServerMessage::Tabs(tabs) => break tabs,
            other => panic!("expected Tabs, got {other:?}"),
        }
    };

    // (b) v2's replayed state matches what was set up on v1 before
    // cutover -- same URLs, same order, ids may legitimately differ
    // (v2 assigns its own fresh ones).
    let urls: Vec<Option<String>> = tabs.iter().map(|t| t.url.clone()).collect();
    assert_eq!(urls, vec![Some("about:credits".to_string()), Some("about:blank".to_string())]);

    write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    launcher.wait_or_kill(Duration::from_secs(5));
}

#[test]
fn a_cutover_that_fails_during_replay_leaves_v1_serving_normally() {
    let mut launcher = Launcher::spawn();
    let mut client = launcher.connect();

    // Give v1 a real (built-in) current URL, so replay actually
    // attempts a `Navigate` into v2 rather than trivially skipping a
    // still-blank default tab.
    write_client_message(&mut client, &ClientMessage::Navigate { url: "about:credits".to_string() }).unwrap();
    assert!(matches!(read_server_message(&mut client).unwrap(), ServerMessage::Navigated { .. }));
    assert!(matches!(read_server_message(&mut client).unwrap(), ServerMessage::FrameReady { .. }));

    // Block v2's frame directory with a plain file where a directory is
    // expected. `blueice-core` never touches `frame_dir` until its
    // first paint (`shm::write_frame`'s `create_dir_all`) -- which only
    // happens partway through handling the replayed `Navigate` -- so v2
    // still spawns and completes its own handshake successfully; the
    // failure surfaces specifically as v2's connection breaking mid-
    // replay, which is exactly the "any failure here aborts the whole
    // cutover" path this test exercises. This is the very first cutover
    // attempt against a freshly-started launcher (generation 0 -> target
    // generation 1), so the frame_dir name is fully predictable.
    let blocked_v2_frame_dir = expected_v2_frame_dir(&launcher.frame_dir, 1);
    let _ = std::fs::remove_file(&blocked_v2_frame_dir);
    std::fs::write(&blocked_v2_frame_dir, b"not a directory").expect("failed to pre-create the blocking file");

    let mut control = launcher.connect_control();
    write_control_request(&mut control, &ControlRequest::Cutover).unwrap();
    let reply = read_control_reply(&mut control).unwrap();
    assert!(matches!(reply, ControlReply::CutoverFailed { .. }), "expected CutoverFailed, got {reply:?}");

    // v1 must still be serving this already-connected client completely
    // normally, with no observable disruption from the failed attempt.
    // Tagged and filtered by request_id, same as the happy-path test:
    // the aborted cutover's own tab-capture step already broadcast a
    // `Tabs` reply to every registered client (including this one)
    // before replay ever failed, and that stray message must not be
    // mistaken for this request's own reply.
    write_client_message_with_id(&mut client, Some(555), &ClientMessage::GetRepresentation).unwrap();
    let rep = loop {
        let (reply_id, message) = read_server_message_with_id(&mut client).unwrap();
        if matches!(reply_id, Some(id) if id != 555) {
            continue;
        }
        break message;
    };
    assert!(matches!(rep, ServerMessage::Representation(_)), "expected v1 to keep answering normally, got {rep:?}");

    let _ = std::fs::remove_file(&blocked_v2_frame_dir);
    write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    launcher.wait_or_kill(Duration::from_secs(5));
}
