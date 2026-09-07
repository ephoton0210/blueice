// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `blueice-mcp-server`: a thin MCP adapter over `core`'s existing
//! IPC control-plane protocol, per `phase-12-mcp-server/PLAN.md`'s
//! "MCP should be an adapter, not a fourth protocol" design decision.
//! Built now, ahead of Phase 12 proper, because differential testing
//! against a real Chromium (via Puppeteer) needs a stable way for an
//! external agent to drive BlueIce early -- this is deliberately the
//! *foundation* Phase 12 will later broaden (downloads, transfer
//! protocols, `bluejs_run`/`bluejs_analyze`) and harden (on-demand
//! process spawning via Phase 8's launcher, the `protocol_version`
//! handshake Phase 1/5 deferred), not the finished feature.
//!
//! **Every tool here wraps a `blueice-ipc` `ClientMessage`/
//! `ServerMessage` round trip -- no browsing logic of its own**, per
//! Phase 12's adapter-not-parallel-channel principle.
//!
//! **The pipelining trick that avoids a read-timeout hack**: several
//! `ClientMessage`s produce a *variable* number of replies (a
//! coordinate/ID `Click` that doesn't land on a link produces none at
//! all -- see `blueice_engine::session`'s own module docs). Rather
//! than guessing how many replies to wait for, every state-changing
//! tool immediately pipelines a `GetRepresentation` after its own
//! message and reads in a loop until it sees the `Representation`
//! reply -- which is *always* exactly one, deterministically, and
//! always the last message on the wire for that pipelined pair, since
//! nothing else produces one. This both resolves the ambiguity and
//! gives every tool a fresh snapshot of the resulting state to return,
//! with no timers and no risk of leaving an unread reply for the next
//! call to misinterpret.

use blueice_ipc::{AiSnapshot, ChromeCommand, ClientMessage, NodeAction, ServerMessage};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The most recent `FrameReady` seen -- cached so [`CoreConnection::screenshot`]
/// can serve the latest frame without having to trigger a new one
/// (there is no `ClientMessage` that means "just resend the current
/// frame" -- every frame comes as the side effect of a state change).
#[derive(Debug, Clone, PartialEq)]
pub struct FrameInfo {
    pub shm_path: String,
    pub width: u32,
    pub height: u32,
    pub generation: u64,
}

/// The common result shape for every state-changing tool: the
/// resulting page representation, plus an error message if `core`
/// reported one along the way (e.g. a failed `navigate`) -- `core`
/// still processes the pipelined `GetRepresentation` even after an
/// error, so `snapshot` is always populated (describing whatever page
/// is current, which is the *previous* page on a failed navigation).
#[derive(Debug, Clone, PartialEq)]
pub struct ToolOutcome {
    pub error: Option<String>,
    pub snapshot: AiSnapshot,
}

/// Wraps one `core` connection, generic over the stream so the actual
/// message-sequencing logic (the part worth testing) doesn't need a
/// real subprocess and Unix socket -- `UnixStream::pair()` plus a fake
/// responder thread is enough, the same strategy
/// `blueice_engine::session`'s own tests already use.
pub struct CoreConnection<S> {
    stream: S,
    last_frame: Option<FrameInfo>,
}

impl<S: Read + Write> CoreConnection<S> {
    pub fn new(stream: S) -> Self {
        CoreConnection { stream, last_frame: None }
    }

    /// Sends `msg`, then pipelines a `GetRepresentation` and drains
    /// until it arrives -- see the module docs for why this avoids
    /// needing to know in advance how many replies `msg` produces.
    fn send_and_drain(&mut self, msg: &ClientMessage) -> io::Result<ToolOutcome> {
        blueice_ipc::write_client_message(&mut self.stream, msg)?;
        blueice_ipc::write_client_message(&mut self.stream, &ClientMessage::GetRepresentation)?;
        let mut error = None;
        loop {
            match blueice_ipc::read_server_message(&mut self.stream)? {
                ServerMessage::Representation(snapshot) => return Ok(ToolOutcome { error, snapshot }),
                ServerMessage::Error { message } => error = Some(message),
                ServerMessage::FrameReady { shm_path, width, height, generation } => {
                    self.last_frame = Some(FrameInfo { shm_path, width, height, generation });
                }
                ServerMessage::Navigated { .. } | ServerMessage::Dom(_) => {}
            }
        }
    }

    pub fn navigate(&mut self, url: &str) -> io::Result<ToolOutcome> {
        self.send_and_drain(&ClientMessage::Navigate { url: url.to_string() })
    }

    pub fn act(&mut self, id: u64, action: NodeAction) -> io::Result<ToolOutcome> {
        self.send_and_drain(&ClientMessage::ActOn { id, action })
    }

    pub fn highlight(&mut self, id: Option<u64>) -> io::Result<ToolOutcome> {
        self.send_and_drain(&ClientMessage::Highlight { id })
    }

    /// Unlike the action-shaped methods above, this sends nothing but
    /// `GetRepresentation` itself -- there's no prior state change to
    /// pipeline it after.
    pub fn representation(&mut self) -> io::Result<AiSnapshot> {
        blueice_ipc::write_client_message(&mut self.stream, &ClientMessage::GetRepresentation)?;
        loop {
            match blueice_ipc::read_server_message(&mut self.stream)? {
                ServerMessage::Representation(snapshot) => return Ok(snapshot),
                ServerMessage::FrameReady { shm_path, width, height, generation } => {
                    self.last_frame = Some(FrameInfo { shm_path, width, height, generation });
                }
                ServerMessage::Error { .. } | ServerMessage::Navigated { .. } | ServerMessage::Dom(_) => {}
            }
        }
    }

    /// The full DOM tree (`blueice_dom::dump`'s canonical text format),
    /// unfiltered by the AI-representation's semantic-role/`display:
    /// none` exclusion -- what the Chromium differential-testing
    /// harness (`TEST_PLAN.md`) diffs a serialized Chromium DOM
    /// against. Same "nothing to pipeline it after" shape as
    /// [`CoreConnection::representation`].
    pub fn dom(&mut self) -> io::Result<String> {
        blueice_ipc::write_client_message(&mut self.stream, &ClientMessage::GetDom)?;
        loop {
            match blueice_ipc::read_server_message(&mut self.stream)? {
                ServerMessage::Dom(dump) => return Ok(dump),
                ServerMessage::FrameReady { shm_path, width, height, generation } => {
                    self.last_frame = Some(FrameInfo { shm_path, width, height, generation });
                }
                ServerMessage::Error { .. } | ServerMessage::Navigated { .. } | ServerMessage::Representation(_) => {}
            }
        }
    }

    /// The most recent frame, if any tool call has caused one yet --
    /// `None` before the first `navigate`.
    pub fn last_frame(&self) -> Option<&FrameInfo> {
        self.last_frame.as_ref()
    }

    pub fn shutdown(&mut self) -> io::Result<()> {
        blueice_ipc::write_client_message(&mut self.stream, &ClientMessage::Shutdown)
    }

    /// Not wired to any tool yet (Phase 12 proper owns the human/AI
    /// visibility-control surface); exposed so a future tool is a
    /// one-line addition rather than a new connection method.
    pub fn set_visible(&mut self, visible: bool) -> io::Result<()> {
        blueice_ipc::write_client_message(&mut self.stream, &ClientMessage::Chrome(ChromeCommand::SetVisible(visible)))
    }
}

/// Renders `frame` (already mapped from shared memory) to PNG bytes,
/// for the `screenshot` tool's base64-encoded MCP image content --
/// round-trips through a temp file rather than adding an in-memory PNG
/// encoder entry point to `blueice-raster`'s public API for this one
/// caller.
pub fn frame_to_png_bytes(pixels: &[u8], width: u32, height: u32) -> io::Result<Vec<u8>> {
    let pixmap = blueice_raster::Pixmap { width, height, pixels: pixels.to_vec() };
    let path = std::env::temp_dir().join(format!("blueice-mcp-screenshot-{}-{}.png", std::process::id(), fastrand_like_suffix()));
    pixmap.save_png(&path)?;
    let bytes = std::fs::read(&path)?;
    let _ = std::fs::remove_file(&path);
    Ok(bytes)
}

/// A cheap, dependency-free unique-enough suffix for the temp file
/// name above -- not a real RNG, just the low bits of the current
/// time, sufficient to avoid two concurrent screenshots on the same
/// process colliding.
fn fastrand_like_suffix() -> u128 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0)
}

/// `core` is expected to sit next to this binary in the same build
/// output directory, same convention `blueice-frontend-reference`
/// already uses for the same reason (both are workspace members
/// landing in the same `target/<profile>/`) -- except a `cargo test`
/// integration-test binary lands one level deeper, in `target/
/// <profile>/deps/`, so this steps back out of a `deps` directory
/// before joining, letting the same lookup work from either place.
fn sibling_core_binary(this_exe: &Path) -> PathBuf {
    let name = if cfg!(windows) { "blueice-core.exe" } else { "blueice-core" };
    let dir = this_exe.parent().unwrap_or_else(|| Path::new("."));
    let dir = if dir.file_name().is_some_and(|n| n == "deps") { dir.parent().unwrap_or(dir) } else { dir };
    dir.join(name)
}

fn unique_socket_path() -> PathBuf {
    std::env::temp_dir().join(format!("blueice-mcp-{}.sock", std::process::id()))
}

fn wait_for_socket(path: &Path, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if path.exists() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

/// Spawns `blueice-core` and connects to it -- the process-management
/// half `main.rs` drives; kept separate from [`CoreConnection`]'s pure
/// message-sequencing logic so that logic stays testable without a
/// real subprocess.
pub struct CoreProcess {
    child: Child,
    socket_path: PathBuf,
    pub conn: Arc<Mutex<CoreConnection<std::os::unix::net::UnixStream>>>,
}

impl CoreProcess {
    pub fn spawn(width: u32, height: u32) -> io::Result<Self> {
        let this_exe = std::env::current_exe()?;
        let core_bin = sibling_core_binary(&this_exe);
        let socket_path = unique_socket_path();
        let _ = std::fs::remove_file(&socket_path);

        let child = Command::new(&core_bin).arg("--socket").arg(&socket_path).arg("--width").arg(width.to_string()).arg("--height").arg(height.to_string()).spawn()?;

        if !wait_for_socket(&socket_path, Duration::from_secs(5)) {
            return Err(io::Error::other(format!("blueice-core never created its socket at {}", socket_path.display())));
        }
        let stream = std::os::unix::net::UnixStream::connect(&socket_path)?;
        Ok(CoreProcess { child, socket_path, conn: Arc::new(Mutex::new(CoreConnection::new(stream))) })
    }
}

impl Drop for CoreProcess {
    fn drop(&mut self) {
        if let Ok(mut conn) = self.conn.lock() {
            let _ = conn.shutdown();
        }
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.socket_path);
    }
}

pub mod server;
pub use server::BlueIceMcpServer;

#[cfg(test)]
mod tests {
    use super::*;
    use blueice_ipc::{AiNode, Bounds, NameFrom, NodeState, Role};
    use std::os::unix::net::UnixStream;
    use std::thread;

    fn sample_snapshot(generation: u64) -> AiSnapshot {
        AiSnapshot {
            generation,
            url: Some("https://example.com".to_string()),
            scroll_y: 0.0,
            nodes: vec![AiNode {
                id: 1,
                parent: None,
                children: vec![],
                role: Role::Link,
                name: Some("go".to_string()),
                name_from: Some(NameFrom::Contents),
                state: NodeState::default(),
                bounds: Bounds { x: 0.0, y: 0.0, width: 10.0, height: 5.0 },
                opacity: 1.0,
                occluded: false,
                occluded_by: None,
                occluded_fraction: 0.0,
            }],
        }
    }

    type FakeCoreStep = Box<dyn FnOnce(ClientMessage, &mut UnixStream) + Send>;

    /// Spawns a fake `core` on the other end of a `UnixStream::pair()`
    /// that reads one `ClientMessage` at a time and replies according
    /// to `script` -- mirrors `blueice_engine::session`'s own test
    /// strategy of driving the real wire protocol without a real
    /// subprocess.
    fn fake_core(mut server: UnixStream, script: Vec<FakeCoreStep>) {
        thread::spawn(move || {
            for step in script {
                let msg = blueice_ipc::read_client_message(&mut server).unwrap();
                step(msg, &mut server);
            }
        });
    }

    fn reply(stream: &mut UnixStream, msg: &ServerMessage) {
        blueice_ipc::write_server_message(stream, msg).unwrap();
    }

    #[test]
    fn navigate_pipelines_get_representation_and_returns_the_resulting_snapshot() {
        let (client, server) = UnixStream::pair().unwrap();
        fake_core(
            server,
            vec![
                Box::new(|msg, s| {
                    assert!(matches!(msg, ClientMessage::Navigate { .. }));
                    reply(s, &ServerMessage::Navigated { url: "https://example.com".to_string() });
                    reply(s, &ServerMessage::FrameReady { shm_path: "/tmp/x".to_string(), width: 10, height: 10, generation: 1 });
                }),
                Box::new(|msg, s| {
                    assert!(matches!(msg, ClientMessage::GetRepresentation));
                    reply(s, &ServerMessage::Representation(sample_snapshot(1)));
                }),
            ],
        );

        let mut conn = CoreConnection::new(client);
        let outcome = conn.navigate("https://example.com").unwrap();
        assert_eq!(outcome.error, None);
        assert_eq!(outcome.snapshot.generation, 1);
        assert_eq!(conn.last_frame().unwrap().generation, 1);
    }

    #[test]
    fn navigate_failure_is_reported_but_still_returns_the_current_snapshot() {
        let (client, server) = UnixStream::pair().unwrap();
        fake_core(
            server,
            vec![
                Box::new(|msg, s| {
                    assert!(matches!(msg, ClientMessage::Navigate { .. }));
                    reply(s, &ServerMessage::Error { message: "unreachable host".to_string() });
                }),
                Box::new(|msg, s| {
                    assert!(matches!(msg, ClientMessage::GetRepresentation));
                    reply(s, &ServerMessage::Representation(sample_snapshot(0)));
                }),
            ],
        );

        let mut conn = CoreConnection::new(client);
        let outcome = conn.navigate("http://bad").unwrap();
        assert_eq!(outcome.error.as_deref(), Some("unreachable host"));
        assert_eq!(outcome.snapshot.generation, 0);
    }

    #[test]
    fn act_on_click_with_no_navigable_effect_still_returns_cleanly() {
        // the real ambiguity this design exists to avoid: an ActOn
        // Click that doesn't land on a link produces zero replies of
        // its own (blueice_engine::session's documented behavior) --
        // proven here by never scripting a reply to the ActOn at all,
        // only to the pipelined GetRepresentation.
        let (client, server) = UnixStream::pair().unwrap();
        fake_core(
            server,
            vec![
                Box::new(|msg, _s| {
                    assert!(matches!(msg, ClientMessage::ActOn { .. }));
                    // no reply -- matches a Click that hit nothing
                }),
                Box::new(|msg, s| {
                    assert!(matches!(msg, ClientMessage::GetRepresentation));
                    reply(s, &ServerMessage::Representation(sample_snapshot(0)));
                }),
            ],
        );

        let mut conn = CoreConnection::new(client);
        let outcome = conn.act(1, NodeAction::Click).unwrap();
        assert_eq!(outcome.error, None);
        assert_eq!(outcome.snapshot.nodes[0].id, 1);
    }

    #[test]
    fn act_on_focus_gets_a_frame_reply_before_the_representation() {
        let (client, server) = UnixStream::pair().unwrap();
        fake_core(
            server,
            vec![
                Box::new(|msg, s| {
                    assert!(matches!(msg, ClientMessage::ActOn { action: NodeAction::Focus, .. }));
                    reply(s, &ServerMessage::FrameReady { shm_path: "/tmp/y".to_string(), width: 5, height: 5, generation: 2 });
                }),
                Box::new(|msg, s| {
                    assert!(matches!(msg, ClientMessage::GetRepresentation));
                    reply(s, &ServerMessage::Representation(sample_snapshot(2)));
                }),
            ],
        );

        let mut conn = CoreConnection::new(client);
        let outcome = conn.act(1, NodeAction::Focus).unwrap();
        assert_eq!(outcome.snapshot.generation, 2);
        assert_eq!(conn.last_frame().unwrap().shm_path, "/tmp/y");
    }

    #[test]
    fn highlight_round_trips_like_any_other_action() {
        let (client, server) = UnixStream::pair().unwrap();
        fake_core(
            server,
            vec![
                Box::new(|msg, s| {
                    assert_eq!(msg, ClientMessage::Highlight { id: Some(1) });
                    reply(s, &ServerMessage::FrameReady { shm_path: "/tmp/z".to_string(), width: 5, height: 5, generation: 3 });
                }),
                Box::new(|msg, s| {
                    assert!(matches!(msg, ClientMessage::GetRepresentation));
                    reply(s, &ServerMessage::Representation(sample_snapshot(3)));
                }),
            ],
        );

        let mut conn = CoreConnection::new(client);
        let outcome = conn.highlight(Some(1)).unwrap();
        assert_eq!(outcome.snapshot.generation, 3);
    }

    #[test]
    fn representation_alone_sends_no_prior_action() {
        let (client, server) = UnixStream::pair().unwrap();
        fake_core(
            server,
            vec![Box::new(|msg, s| {
                assert!(matches!(msg, ClientMessage::GetRepresentation));
                reply(s, &ServerMessage::Representation(sample_snapshot(0)));
            })],
        );

        let mut conn = CoreConnection::new(client);
        let snap = conn.representation().unwrap();
        assert_eq!(snap.generation, 0);
    }

    #[test]
    fn dom_alone_sends_no_prior_action() {
        let (client, server) = UnixStream::pair().unwrap();
        fake_core(
            server,
            vec![Box::new(|msg, s| {
                assert!(matches!(msg, ClientMessage::GetDom));
                reply(s, &ServerMessage::Dom("| <html>\n".to_string()));
            })],
        );

        let mut conn = CoreConnection::new(client);
        assert_eq!(conn.dom().unwrap(), "| <html>\n");
    }

    #[test]
    fn dom_still_caches_a_frame_ready_seen_along_the_way() {
        let (client, server) = UnixStream::pair().unwrap();
        fake_core(
            server,
            vec![Box::new(|msg, s| {
                assert!(matches!(msg, ClientMessage::GetDom));
                reply(s, &ServerMessage::FrameReady { shm_path: "/tmp/dom".to_string(), width: 1, height: 1, generation: 5 });
                reply(s, &ServerMessage::Dom("| <html>\n".to_string()));
            })],
        );

        let mut conn = CoreConnection::new(client);
        conn.dom().unwrap();
        assert_eq!(conn.last_frame().unwrap().generation, 5);
    }

    #[test]
    fn representation_alone_still_caches_a_frame_ready_seen_along_the_way() {
        let (client, server) = UnixStream::pair().unwrap();
        fake_core(
            server,
            vec![Box::new(|msg, s| {
                assert!(matches!(msg, ClientMessage::GetRepresentation));
                reply(s, &ServerMessage::FrameReady { shm_path: "/tmp/w".to_string(), width: 1, height: 1, generation: 9 });
                reply(s, &ServerMessage::Representation(sample_snapshot(9)));
            })],
        );

        let mut conn = CoreConnection::new(client);
        conn.representation().unwrap();
        assert_eq!(conn.last_frame().unwrap().generation, 9);
    }

    #[test]
    fn shutdown_sends_the_shutdown_message() {
        let (client, server) = UnixStream::pair().unwrap();
        fake_core(
            server,
            vec![Box::new(|msg, _s| {
                assert!(matches!(msg, ClientMessage::Shutdown));
            })],
        );

        let mut conn = CoreConnection::new(client);
        conn.shutdown().unwrap();
    }

    #[test]
    fn set_visible_sends_a_chrome_command() {
        let (client, server) = UnixStream::pair().unwrap();
        fake_core(
            server,
            vec![Box::new(|msg, _s| {
                assert_eq!(msg, ClientMessage::Chrome(ChromeCommand::SetVisible(true)));
            })],
        );

        let mut conn = CoreConnection::new(client);
        conn.set_visible(true).unwrap();
    }

    #[test]
    fn last_frame_is_none_before_any_action_produces_one() {
        let (client, _server) = UnixStream::pair().unwrap();
        let conn = CoreConnection::new(client);
        assert!(conn.last_frame().is_none());
    }

    #[test]
    fn sibling_core_binary_sits_next_to_the_mcp_server_binary() {
        let exe = PathBuf::from("/some/target/debug/blueice-mcp-server");
        assert_eq!(sibling_core_binary(&exe), PathBuf::from("/some/target/debug/blueice-core"));
    }

    #[test]
    fn sibling_core_binary_steps_out_of_a_deps_directory_for_integration_tests() {
        let exe = PathBuf::from("/some/target/debug/deps/core_process-abc123");
        assert_eq!(sibling_core_binary(&exe), PathBuf::from("/some/target/debug/blueice-core"));
    }

    #[test]
    fn unique_socket_path_stays_short_enough_for_af_unix() {
        assert!(unique_socket_path().to_string_lossy().len() < 100);
    }

    #[test]
    fn wait_for_socket_returns_true_once_the_path_exists() {
        let path = std::env::temp_dir().join(format!("blueice-mcp-wait-test-{}", std::process::id()));
        let _ = std::fs::remove_file(&path);
        std::fs::write(&path, b"x").unwrap();
        assert!(wait_for_socket(&path, Duration::from_millis(50)));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn wait_for_socket_times_out_if_the_path_never_appears() {
        let path = std::env::temp_dir().join("blueice-mcp-never-appears.sock");
        let _ = std::fs::remove_file(&path);
        assert!(!wait_for_socket(&path, Duration::from_millis(50)));
    }

    #[test]
    fn frame_to_png_bytes_produces_a_real_decodable_png() {
        let pixels = vec![255u8, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 0, 255];
        let png = frame_to_png_bytes(&pixels, 2, 2).unwrap();
        assert_eq!(&png[0..8], &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
    }
}
