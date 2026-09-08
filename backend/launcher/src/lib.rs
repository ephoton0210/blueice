// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `blueice-launcher`: the minimal first slice of
//! `phase-8-live-core-hotswap/PLAN.md`'s supervisor role -- a rendezvous
//! broker letting a human's `frontend` and an AI's `mcp-server` (or any
//! other `blueice-ipc` client) share *one* running `core` instance and
//! its one `Page`, instead of each spawning/observing a separate one.
//! That's the exact dual-track split (a human on one instance, an AI
//! driving a separate one) CLAUDE.md's core goal rejects -- just as
//! fully present *within* BlueIce's own process fleet, before this
//! crate existed, as the external "human on a real browser, AI on
//! headless Chromium" case the whole project exists to avoid.
//!
//! Deliberately not this phase's fuller hot-swap objective (spawning a
//! new `core` version, health-checking it, cutting traffic over): this
//! crate spawns exactly one `core`, once, and keeps it running for its
//! own whole lifetime. See `phase-8-live-core-hotswap/PLAN.md`'s
//! "Minimal first slice" for the full design and what's still deferred.
//!
//! No changes were needed to `blueice-core`/`blueice_engine::session`
//! for this: `core` still only ever sees one connection (this crate's),
//! exactly as it always has. The multi-client fan-out/fan-in work all
//! happens here: every external client's [`blueice_ipc::ClientMessage`]s
//! are forwarded into that one connection ([`forward_client_to_core`]),
//! and every [`blueice_ipc::ServerMessage`] `core` sends back is
//! broadcast to *every* currently-connected external client
//! ([`broadcast_core_to_clients`]) -- not just whichever one's action
//! triggered it, which is what actually delivers "same render pass."

use blueice_ipc::{read_client_message, read_server_message, write_client_message, write_server_message};
use std::io;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

/// The rendezvous socket path clients connect to, when none is given
/// explicitly: `$XDG_RUNTIME_DIR/blueice/core.sock`, falling back to
/// `/tmp/blueice-<uid>/core.sock` if `XDG_RUNTIME_DIR` isn't set. Kept
/// per-user (never a single system-wide path) so two different users on
/// the same machine -- or two independent BlueIce sessions started by
/// the same user with an explicit override -- never collide.
pub fn default_rendezvous_socket_path() -> PathBuf {
    let dir = match std::env::var_os("XDG_RUNTIME_DIR") {
        Some(dir) => PathBuf::from(dir).join("blueice"),
        None => std::env::temp_dir().join(format!("blueice-{}", unsafe { libc_getuid() })),
    };
    dir.join("core.sock")
}

// A tiny, deliberately minimal stand-in for `libc::getuid()` rather than
// adding a whole `libc` dependency for one syscall: reads the real UID
// via the `/proc/self/status` line every Linux (BlueIce's only
// currently-supported target -- see `blueice-core`'s own `UnixListener`
// dependency, already Unix-only) exposes, falling back to the process
// ID if that ever fails so the path is still unique per-process rather
// than panicking.
unsafe fn libc_getuid() -> u32 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|status| status.lines().find_map(|line| line.strip_prefix("Uid:")).and_then(|rest| rest.split_whitespace().next()).and_then(|s| s.parse().ok()))
        .unwrap_or_else(std::process::id)
}

/// Forwards every [`blueice_ipc::ClientMessage`] read from `client` into
/// `core`, until `client` disconnects or errors, or writing to `core`
/// fails (the shared core connection is gone). Runs on its own thread
/// per connected external client; `core` is behind a [`Mutex`] because
/// multiple such threads write into the same one `core` connection
/// concurrently.
pub fn forward_client_to_core(mut client: UnixStream, core: Arc<Mutex<UnixStream>>) {
    loop {
        let msg = match read_client_message(&mut client) {
            Ok(msg) => msg,
            Err(_) => return,
        };
        let mut core = core.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if write_client_message(&mut *core, &msg).is_err() {
            return;
        }
    }
}

/// Reads every [`blueice_ipc::ServerMessage`] `core` sends, until it
/// disconnects or errors (`core` crashed or exited), broadcasting each
/// one to every stream currently in `clients` -- not just whichever
/// client's action triggered it. A client whose write fails (it
/// disconnected) is dropped from the list rather than treated as fatal
/// to the broadcast itself.
pub fn broadcast_core_to_clients(mut core: UnixStream, clients: Arc<Mutex<Vec<UnixStream>>>) {
    loop {
        let msg = match read_server_message(&mut core) {
            Ok(msg) => msg,
            Err(_) => return,
        };
        let mut clients = clients.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        clients.retain_mut(|client| write_server_message(client, &msg).is_ok());
    }
}

/// Accepts one already-connected external client: registers its
/// write-half for broadcast and spawns a thread forwarding its incoming
/// messages into `core_writer`. Split out from the accept loop so it's
/// unit-testable with a [`UnixStream::pair`] fake client, without
/// needing a real [`std::os::unix::net::UnixListener`].
pub fn register_client(client: UnixStream, core_writer: Arc<Mutex<UnixStream>>, clients: Arc<Mutex<Vec<UnixStream>>>) -> io::Result<()> {
    let write_half = client.try_clone()?;
    clients.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).push(write_half);
    thread::spawn(move || forward_client_to_core(client, core_writer));
    Ok(())
}

/// Runs the broker for as long as `core` stays connected: a background
/// thread accepts external client connections from `listener`
/// (registering each via [`register_client`]), while the calling thread
/// runs [`broadcast_core_to_clients`] and returns once it ends -- i.e.
/// once `core` itself disconnects or errors, since a broker with no
/// `core` behind it has nothing left to broker. `listener.incoming()`
/// blocks indefinitely waiting for the next connection, so there's no
/// clean way to also stop the accept thread at that point; the caller
/// is expected to exit the process shortly after this returns, which
/// the OS reclaims regardless of that thread's blocked state (matching
/// `blueice-core`'s own "just run until the underlying connection ends"
/// simplicity level -- graceful shutdown/health-checking is the fuller
/// hot-swap work's job, not this minimal slice's).
pub fn run_broker(listener: std::os::unix::net::UnixListener, core: UnixStream) -> io::Result<()> {
    let core_writer = Arc::new(Mutex::new(core.try_clone()?));
    let clients: Arc<Mutex<Vec<UnixStream>>> = Arc::new(Mutex::new(Vec::new()));

    let accept_clients = Arc::clone(&clients);
    let accept_core_writer = Arc::clone(&core_writer);
    thread::spawn(move || {
        for incoming in listener.incoming() {
            let Ok(client) = incoming else { break };
            let _ = register_client(client, Arc::clone(&accept_core_writer), Arc::clone(&accept_clients));
        }
    });

    broadcast_core_to_clients(core, clients);
    Ok(())
}

/// The `blueice-core` binary's path, resolved relative to this
/// (`blueice-launcher`'s) own executable -- mirrors `blueice-mcp-
/// server`'s identical `sibling_core_binary` helper (not shared between
/// the two crates: each is ~10 lines, and the two processes' spawn
/// helpers are otherwise independent enough that a shared crate just
/// for this would be more indirection than the duplication costs).
/// Steps out of a `deps` directory first so the same lookup works both
/// for the installed binary and for a `cargo test` integration-test
/// binary, which lands one level deeper (`target/<profile>/deps/`).
fn sibling_core_binary(this_exe: &Path) -> PathBuf {
    let name = if cfg!(windows) { "blueice-core.exe" } else { "blueice-core" };
    let dir = this_exe.parent().unwrap_or_else(|| Path::new("."));
    let dir = if dir.file_name().is_some_and(|n| n == "deps") { dir.parent().unwrap_or(dir) } else { dir };
    dir.join(name)
}

/// A path for `core`'s *internal* socket -- never exposed to external
/// clients, which only ever see the rendezvous socket this launcher
/// itself listens on.
fn unique_internal_socket_path() -> PathBuf {
    std::env::temp_dir().join(format!("blueice-launcher-core-{}.sock", std::process::id()))
}

fn wait_for_socket(path: &Path, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if path.exists() {
            return true;
        }
        thread::sleep(Duration::from_millis(20));
    }
    false
}

/// A `core` process this launcher spawned and owns privately: killed and
/// cleaned up on [`Drop`], the same lifetime discipline `mcp-server`'s
/// `CoreProcess` already established for its own (today, unshared)
/// spawned `core`.
pub struct SpawnedCore {
    child: Child,
    internal_socket_path: PathBuf,
    pub stream: UnixStream,
}

impl SpawnedCore {
    pub fn spawn(width: f64, height: f64, frame_dir: &Path) -> io::Result<Self> {
        let this_exe = std::env::current_exe()?;
        let core_bin = sibling_core_binary(&this_exe);
        let internal_socket_path = unique_internal_socket_path();
        let _ = std::fs::remove_file(&internal_socket_path);

        let child = Command::new(&core_bin)
            .arg("--socket")
            .arg(&internal_socket_path)
            .arg("--width")
            .arg(width.to_string())
            .arg("--height")
            .arg(height.to_string())
            .arg("--frame-dir")
            .arg(frame_dir)
            .spawn()?;

        if !wait_for_socket(&internal_socket_path, Duration::from_secs(5)) {
            return Err(io::Error::other(format!("blueice-core never created its socket at {}", internal_socket_path.display())));
        }
        let stream = UnixStream::connect(&internal_socket_path)?;
        Ok(SpawnedCore { child, internal_socket_path, stream })
    }
}

impl Drop for SpawnedCore {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.internal_socket_path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use blueice_ipc::{ClientMessage, ServerMessage};

    #[test]
    fn forward_client_to_core_relays_one_message_then_stops_on_disconnect() {
        let (client_side, mut client_observed) = UnixStream::pair().unwrap();
        let (core_side, mut core_observed) = UnixStream::pair().unwrap();
        let core = Arc::new(Mutex::new(core_side));

        write_client_message(&mut client_observed, &ClientMessage::Resize { width: 10, height: 20 }).unwrap();
        drop(client_observed); // triggers a clean disconnect after the one message

        forward_client_to_core(client_side, Arc::clone(&core));

        assert_eq!(read_client_message(&mut core_observed).unwrap(), ClientMessage::Resize { width: 10, height: 20 });
    }

    #[test]
    fn forward_client_to_core_stops_once_the_core_connection_is_gone() {
        let (client_side, mut client_observed) = UnixStream::pair().unwrap();
        let (core_side, core_observed) = UnixStream::pair().unwrap();
        drop(core_observed); // core is "gone" before any message arrives
        let core = Arc::new(Mutex::new(core_side));

        write_client_message(&mut client_observed, &ClientMessage::Shutdown).unwrap();

        // Must return (not hang or panic) once the write to `core` fails.
        forward_client_to_core(client_side, core);
    }

    #[test]
    fn broadcast_core_to_clients_relays_one_message_to_every_registered_client() {
        let (core_side, mut core_observed) = UnixStream::pair().unwrap();
        let (client1_side, mut client1_observed) = UnixStream::pair().unwrap();
        let (client2_side, mut client2_observed) = UnixStream::pair().unwrap();
        let clients = Arc::new(Mutex::new(vec![client1_side, client2_side]));

        write_server_message(&mut core_observed, &ServerMessage::Navigated { url: "about:blank".to_string() }).unwrap();
        drop(core_observed); // ends the broadcaster loop after the one message

        broadcast_core_to_clients(core_side, clients);

        let expected = ServerMessage::Navigated { url: "about:blank".to_string() };
        assert_eq!(read_server_message(&mut client1_observed).unwrap(), expected);
        assert_eq!(read_server_message(&mut client2_observed).unwrap(), expected);
    }

    #[test]
    fn broadcast_core_to_clients_drops_a_client_whose_write_fails_without_affecting_the_others() {
        let (core_side, mut core_observed) = UnixStream::pair().unwrap();
        let (dead_client_side, dead_client_observed) = UnixStream::pair().unwrap();
        drop(dead_client_observed); // the "other end" is gone, so writes to dead_client_side fail
        let (live_client_side, mut live_client_observed) = UnixStream::pair().unwrap();
        let clients = Arc::new(Mutex::new(vec![dead_client_side, live_client_side]));

        write_server_message(&mut core_observed, &ServerMessage::Navigated { url: "about:blank".to_string() }).unwrap();
        drop(core_observed);

        broadcast_core_to_clients(core_side, Arc::clone(&clients));

        assert_eq!(read_server_message(&mut live_client_observed).unwrap(), ServerMessage::Navigated { url: "about:blank".to_string() });
        // the dead client's stream must have been pruned from the list.
        assert_eq!(clients.lock().unwrap().len(), 1);
    }

    #[test]
    fn register_client_forwards_its_messages_and_receives_broadcasts() {
        let (client_side, mut client_observed) = UnixStream::pair().unwrap();
        let (core_side, mut core_observed) = UnixStream::pair().unwrap();
        let core_writer = Arc::new(Mutex::new(core_side));
        let clients: Arc<Mutex<Vec<UnixStream>>> = Arc::new(Mutex::new(Vec::new()));

        register_client(client_side, Arc::clone(&core_writer), Arc::clone(&clients)).unwrap();

        // fan-in: a message the "client" sends must reach core.
        write_client_message(&mut client_observed, &ClientMessage::GetRepresentation).unwrap();
        assert_eq!(read_client_message(&mut core_observed).unwrap(), ClientMessage::GetRepresentation);

        // fan-out: a message written directly to the registered write-half
        // (standing in for the broadcaster) must reach the client.
        let mut registered = clients.lock().unwrap().pop().unwrap();
        write_server_message(&mut registered, &ServerMessage::Navigated { url: "x".to_string() }).unwrap();
        assert_eq!(read_server_message(&mut client_observed).unwrap(), ServerMessage::Navigated { url: "x".to_string() });
    }

    #[test]
    fn default_rendezvous_socket_path_is_per_user_not_system_wide() {
        let path = default_rendezvous_socket_path();
        assert_eq!(path.file_name().unwrap(), "core.sock");
        // must not resolve to a single fixed system-wide path regardless
        // of environment -- it has to vary by runtime dir or uid.
        assert_ne!(path, PathBuf::from("/core.sock"));
    }

    #[test]
    fn sibling_core_binary_sits_next_to_the_launcher_binary() {
        let exe = PathBuf::from("/some/target/debug/blueice-launcher");
        assert_eq!(sibling_core_binary(&exe), PathBuf::from("/some/target/debug/blueice-core"));
    }

    #[test]
    fn sibling_core_binary_steps_out_of_a_deps_directory_for_integration_tests() {
        let exe = PathBuf::from("/some/target/debug/deps/broker_end_to_end-abc123");
        assert_eq!(sibling_core_binary(&exe), PathBuf::from("/some/target/debug/blueice-core"));
    }

    #[test]
    fn unique_internal_socket_path_stays_short_enough_for_af_unix() {
        assert!(unique_internal_socket_path().to_string_lossy().len() < 100);
    }

    #[test]
    fn wait_for_socket_returns_true_once_the_path_exists() {
        let path = std::env::temp_dir().join(format!("blueice-launcher-wait-test-{}", std::process::id()));
        let _ = std::fs::remove_file(&path);
        std::fs::write(&path, b"x").unwrap();
        assert!(wait_for_socket(&path, Duration::from_millis(50)));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn wait_for_socket_times_out_if_the_path_never_appears() {
        let path = std::env::temp_dir().join(format!("blueice-launcher-wait-test-missing-{}", std::process::id()));
        let _ = std::fs::remove_file(&path);
        assert!(!wait_for_socket(&path, Duration::from_millis(50)));
    }

    #[test]
    fn run_broker_ends_once_the_core_connection_closes() {
        let dir = std::env::temp_dir().join(format!("blueice-launcher-test-run-broker-{}", std::process::id()));
        let _ = std::fs::remove_file(&dir);
        let listener = std::os::unix::net::UnixListener::bind(&dir).unwrap();
        let (core_side, core_observed) = UnixStream::pair().unwrap();
        drop(core_observed); // "core" is already gone

        run_broker(listener, core_side).unwrap();

        let _ = std::fs::remove_file(&dir);
    }
}
