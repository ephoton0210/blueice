// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The launcher's supervision of `blueice-ai-assistant`
//! (`phase-7-local-ai/PLAN.md`, step R2).
//!
//! The launcher owns the assistant's *public* socket -- the one `core` is given
//! -- and starts the real assistant on a private socket only when a connection
//! arrives ([`ProcessPolicy::OnDemand`]), then relays that connection byte for
//! byte. That one arrangement provides everything the plan asks of it without
//! a new protocol:
//!
//! * **Every connection is an activity signal.** `core` opens a connection for
//!   each translated navigation and each task, so an assistant in use is never
//!   idle -- the live-translation carve-out -- and it idles out only after
//!   `idle_timeout` with no traffic *and* under memory pressure, the registry's
//!   existing rule.
//! * **A torn-down or crashed assistant respawns** on the next connection
//!   instead of staying dead.
//! * **The launcher, not the assistant, holds the child**, so a ceiling can be
//!   enforced on it from outside.
//!
//! The price is a model reload on respawn: a loopback backend restarts in
//! milliseconds, an in-process one does not. While a respawning assistant loads,
//! the connection that triggered it is held (bounded); `core` gives up after its
//! own deadline and shows the original page, which is exactly the fail-open
//! behavior translation already has.

use crate::supervisor::{ProcessPolicy, ProcessRegistry};
use blueice_assistant_settings::AssistantSettings;
use std::io::{self, Read, Write};
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

/// The role name in the [`ProcessRegistry`].
pub const ROLE: &str = "ai-assistant";
/// How long a connection that triggered a spawn waits for the assistant to
/// start listening. Longer than `core`'s translation deadline on purpose: the
/// assistant should still come up after `core` has given up on that request.
pub const DEFAULT_READY_TIMEOUT: Duration = Duration::from_secs(30);
/// The most connections relayed at once; an excess connection is dropped, which
/// `core` treats as an unavailable assistant. Bounds what a local process that
/// floods the public socket can cost.
const MAX_CONCURRENT_RELAYS: usize = 64;
/// How often a relay with no bytes flowing still counts as activity and checks
/// whether the supervisor is stopping.
const RELAY_TICK: Duration = Duration::from_secs(1);

/// The assistant binary beside this launcher, the same way the other siblings
/// (`core`, the gatekeeper) are found.
pub fn sibling_assistant_binary(this_exe: &Path) -> PathBuf {
    let name = if cfg!(windows) {
        "blueice-ai-assistant.exe"
    } else {
        "blueice-ai-assistant"
    };
    let dir = this_exe.parent().unwrap_or_else(|| Path::new("."));
    // Under `cargo test` the running executable lives in `target/debug/deps`.
    let dir = if dir.file_name().is_some_and(|n| n == "deps") {
        dir.parent().unwrap_or(dir)
    } else {
        dir
    };
    dir.join(name)
}

/// Starts the assistant child listening on the given private socket path.
type Spawner = Box<dyn Fn(&Path) -> io::Result<Child> + Send + Sync>;

struct Inner {
    spawner: Spawner,
    private_socket: PathBuf,
    registry: Arc<Mutex<ProcessRegistry>>,
    /// Serializes "is it running? if not, start it and wait until it listens"
    /// so many simultaneous first connections cause one spawn.
    spawn_lock: Mutex<()>,
    ready_timeout: Duration,
    stop: AtomicBool,
    spawns: AtomicU64,
    relays: AtomicUsize,
}

/// The public socket, its accept thread, and the on-demand child behind it.
pub struct AssistantSupervisor {
    inner: Arc<Inner>,
    public_socket: PathBuf,
}

impl Inner {
    fn touch(&self) {
        if let Ok(mut registry) = self.registry.lock() {
            registry.mark_active(ROLE, Instant::now());
        }
    }

    /// A connection to the private socket, starting the assistant first if it
    /// is not running (or died since it last was).
    fn upstream(&self) -> io::Result<UnixStream> {
        let _spawning = self.spawn_lock.lock().unwrap_or_else(|e| e.into_inner());
        // Checked under the lock, after any spawn in progress has finished: a
        // connection accepted just before shutdown began must not start a child
        // that shutdown's teardown has already passed.
        if self.stop.load(Ordering::Relaxed) {
            return Err(io::Error::other("the assistant supervisor is stopping"));
        }
        let running = {
            let mut registry = self.registry.lock().unwrap_or_else(|e| e.into_inner());
            registry.reap_exited(ROLE);
            registry.mark_active(ROLE, Instant::now());
            registry.is_resident(ROLE)
        };
        if !running {
            self.start_child()?;
        }
        UnixStream::connect(&self.private_socket)
    }

    fn start_child(&self) -> io::Result<()> {
        // A killed assistant leaves its socket file behind; a stale one would
        // make the readiness probe below succeed against nothing.
        let _ = std::fs::remove_file(&self.private_socket);
        let child = (self.spawner)(&self.private_socket)?;
        self.spawns.fetch_add(1, Ordering::Relaxed);
        let leftover = {
            let mut registry = self.registry.lock().unwrap_or_else(|e| e.into_inner());
            registry.set_resident(ROLE, child, Instant::now())
        };
        if let Some(mut orphan) = leftover {
            let _ = orphan.kill();
            let _ = orphan.wait();
            return Err(io::Error::other("the assistant role is not registered"));
        }
        let deadline = Instant::now() + self.ready_timeout;
        loop {
            if UnixStream::connect(&self.private_socket).is_ok() {
                return Ok(());
            }
            let exited = {
                let mut registry = self.registry.lock().unwrap_or_else(|e| e.into_inner());
                registry.reap_exited(ROLE)
            };
            if exited || Instant::now() >= deadline {
                let mut registry = self.registry.lock().unwrap_or_else(|e| e.into_inner());
                registry.teardown(ROLE);
                return Err(io::Error::other(if exited {
                    "the assistant exited while starting"
                } else {
                    "the assistant did not start listening in time"
                }));
            }
            thread::sleep(Duration::from_millis(20));
        }
    }

    fn relay(self: &Arc<Self>, client: UnixStream) {
        // The assistant being unavailable is not an error to report to the
        // client beyond closing the connection: `core` reads that as "not
        // available" and keeps the original page.
        let Ok(upstream) = self.upstream() else {
            return;
        };
        let (Ok(client_reader), Ok(upstream_reader)) = (client.try_clone(), upstream.try_clone())
        else {
            return;
        };
        let to_upstream = {
            let inner = Arc::clone(self);
            thread::spawn(move || inner.pump(client_reader, upstream))
        };
        self.pump(upstream_reader, client);
        let _ = to_upstream.join();
    }

    /// Copies `from` to `to` until either side ends, counting every chunk (and
    /// every quiet second while the connection stays open) as activity.
    fn pump(&self, mut from: UnixStream, mut to: UnixStream) {
        let _ = from.set_read_timeout(Some(RELAY_TICK));
        let mut buffer = [0u8; 16 * 1024];
        loop {
            match from.read(&mut buffer) {
                Ok(0) => break,
                Ok(n) => {
                    self.touch();
                    if to.write_all(&buffer[..n]).is_err() {
                        break;
                    }
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                    ) =>
                {
                    self.touch();
                    if self.stop.load(Ordering::Relaxed) {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
        let _ = to.shutdown(Shutdown::Write);
        let _ = from.shutdown(Shutdown::Read);
    }
}

impl AssistantSupervisor {
    /// Starts supervising with an explicit way to spawn the child. Registers the
    /// role as [`ProcessPolicy::OnDemand`] with `idle_timeout` and binds the
    /// public socket. Nothing is spawned until the first connection.
    pub fn start_with_spawner(
        idle_timeout: Duration,
        spawner: Spawner,
        registry: Arc<Mutex<ProcessRegistry>>,
        public_socket: PathBuf,
        private_socket: PathBuf,
        ready_timeout: Duration,
    ) -> io::Result<Self> {
        if let Some(parent) = public_socket.parent() {
            blueice_ipc::local_socket::ensure_private_socket_dir(parent)?;
        }
        let _ = std::fs::remove_file(&public_socket);
        let listener = blueice_ipc::local_socket::bind_private_listener(&public_socket)?;
        registry.lock().unwrap_or_else(|e| e.into_inner()).register(
            ROLE,
            ProcessPolicy::OnDemand { idle_timeout },
            None,
            Instant::now(),
        );
        let inner = Arc::new(Inner {
            spawner,
            private_socket,
            registry,
            spawn_lock: Mutex::new(()),
            ready_timeout,
            stop: AtomicBool::new(false),
            spawns: AtomicU64::new(0),
            relays: AtomicUsize::new(0),
        });
        let accept_inner = Arc::clone(&inner);
        thread::spawn(move || {
            for incoming in listener.incoming() {
                if accept_inner.stop.load(Ordering::Relaxed) {
                    break;
                }
                let Ok(client) = incoming else { continue };
                if accept_inner.relays.fetch_add(1, Ordering::SeqCst) >= MAX_CONCURRENT_RELAYS {
                    accept_inner.relays.fetch_sub(1, Ordering::SeqCst);
                    continue; // dropped: core sees an unavailable assistant
                }
                let inner = Arc::clone(&accept_inner);
                thread::spawn(move || {
                    inner.relay(client);
                    inner.relays.fetch_sub(1, Ordering::SeqCst);
                });
            }
        });
        Ok(AssistantSupervisor {
            inner,
            public_socket,
        })
    }

    /// Supervises the real `blueice-ai-assistant` at `assistant_bin`, started
    /// with the flags `settings` renders to. `None` when no assistant is
    /// configured, so callers give `core` no assistant socket at all.
    pub fn start(
        settings: &AssistantSettings,
        assistant_bin: PathBuf,
        registry: Arc<Mutex<ProcessRegistry>>,
    ) -> io::Result<Option<Self>> {
        settings.validate().map_err(io::Error::other)?;
        if !settings.is_configured() {
            return Ok(None);
        }
        let args = settings.assistant_args();
        let spawner: Spawner = Box::new(move |private_socket| {
            Command::new(&assistant_bin)
                .arg("--socket")
                .arg(private_socket)
                .args(&args)
                .spawn()
        });
        let (public_socket, private_socket) = unique_socket_paths();
        Self::start_with_spawner(
            Duration::from_secs(settings.idle_timeout_secs),
            spawner,
            registry,
            public_socket,
            private_socket,
            DEFAULT_READY_TIMEOUT,
        )
        .map(Some)
    }

    /// The socket `core` is given as `--assistant-socket`.
    pub fn public_socket(&self) -> &Path {
        &self.public_socket
    }

    /// How many times the assistant has been started (observable for tests and
    /// diagnostics).
    pub fn spawn_count(&self) -> u64 {
        self.inner.spawns.load(Ordering::Relaxed)
    }
}

impl Drop for AssistantSupervisor {
    fn drop(&mut self) {
        self.inner.stop.store(true, Ordering::Relaxed);
        // Wake the accept loop so it can see the flag.
        let _ = UnixStream::connect(&self.public_socket);
        // Wait out any spawn already in progress so the teardown below sees, and
        // kills, the child it produced; every later attempt sees `stop`.
        let _no_spawn_in_progress = self
            .inner
            .spawn_lock
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if let Ok(mut registry) = self.inner.registry.lock() {
            registry.teardown(ROLE);
        }
        let _ = std::fs::remove_file(&self.public_socket);
        let _ = std::fs::remove_file(&self.inner.private_socket);
    }
}

/// Per-launcher sockets, in the private socket directory the other internal
/// sockets use.
fn unique_socket_paths() -> (PathBuf, PathBuf) {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = blueice_ipc::local_socket::default_socket_dir();
    let pid = std::process::id();
    (
        dir.join(format!("l-as-pub-{pid}-{n}.sock")),
        dir.join(format!("l-as-prv-{pid}-{n}.sock")),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixStream;

    /// A real child process standing in for the assistant: it listens on the
    /// socket it is given and echoes every byte back, so the relay is tested
    /// against a genuine OS process and a genuine Unix socket.
    const ECHO_SERVER: &str = r#"
import os, socket, sys, threading
path = sys.argv[1]
server = socket.socket(socket.AF_UNIX)
server.bind(path)
server.listen(16)
def serve(conn):
    while True:
        data = conn.recv(4096)
        if not data:
            break
        conn.sendall(data)
    conn.close()
while True:
    conn, _ = server.accept()
    threading.Thread(target=serve, args=(conn,), daemon=True).start()
"#;

    fn socket_pair_paths(tag: &str) -> (PathBuf, PathBuf) {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = blueice_ipc::local_socket::default_socket_dir();
        let pid = std::process::id();
        (
            dir.join(format!("t-{tag}-pub-{pid}-{n}.sock")),
            dir.join(format!("t-{tag}-prv-{pid}-{n}.sock")),
        )
    }

    fn echo_spawner() -> Spawner {
        Box::new(|socket| {
            Command::new("python3")
                .args(["-c", ECHO_SERVER])
                .arg(socket)
                .spawn()
        })
    }

    struct Rig {
        supervisor: AssistantSupervisor,
        registry: Arc<Mutex<ProcessRegistry>>,
    }

    fn rig(tag: &str, spawner: Spawner, idle: Duration, ready: Duration) -> Rig {
        let registry = Arc::new(Mutex::new(ProcessRegistry::new()));
        let (public, private) = socket_pair_paths(tag);
        let supervisor = AssistantSupervisor::start_with_spawner(
            idle,
            spawner,
            registry.clone(),
            public,
            private,
            ready,
        )
        .unwrap();
        Rig {
            supervisor,
            registry,
        }
    }

    /// One request/reply exchange. `Err` is a reset or broken connection, which
    /// `core` reads the same as an empty reply: "the assistant is unavailable".
    fn try_talk(public: &Path, message: &[u8]) -> io::Result<Vec<u8>> {
        let mut stream = UnixStream::connect(public)?;
        stream.set_read_timeout(Some(Duration::from_secs(10)))?;
        stream.write_all(message)?;
        stream.shutdown(Shutdown::Write)?;
        let mut reply = Vec::new();
        stream.read_to_end(&mut reply)?;
        Ok(reply)
    }

    fn talk(public: &Path, message: &[u8]) -> Vec<u8> {
        try_talk(public, message).unwrap()
    }

    /// The client saw a closed connection and no assistant reply.
    fn is_unavailable(outcome: io::Result<Vec<u8>>) -> bool {
        outcome.map(|reply| reply.is_empty()).unwrap_or(true)
    }

    fn resident(rig: &Rig) -> bool {
        rig.registry.lock().unwrap().is_resident(ROLE)
    }

    #[test]
    fn nothing_is_spawned_until_the_first_connection() {
        let rig = rig(
            "lazy",
            echo_spawner(),
            Duration::from_secs(60),
            Duration::from_secs(10),
        );
        assert_eq!(rig.supervisor.spawn_count(), 0);
        assert!(!resident(&rig));
        assert_eq!(talk(rig.supervisor.public_socket(), b"ping"), b"ping");
        assert_eq!(rig.supervisor.spawn_count(), 1);
        assert!(resident(&rig));
    }

    #[test]
    fn bytes_are_relayed_both_ways_unchanged() {
        let rig = rig(
            "bytes",
            echo_spawner(),
            Duration::from_secs(60),
            Duration::from_secs(10),
        );
        let payload: Vec<u8> = (0..200_000u32).map(|n| (n % 251) as u8).collect();
        assert_eq!(talk(rig.supervisor.public_socket(), &payload), payload);
    }

    #[test]
    fn the_assistant_is_reused_across_connections_and_started_once_under_a_stampede() {
        let rig = Arc::new(rig(
            "stampede",
            echo_spawner(),
            Duration::from_secs(60),
            Duration::from_secs(10),
        ));
        let workers: Vec<_> = (0..8)
            .map(|n| {
                let rig = Arc::clone(&rig);
                thread::spawn(move || {
                    let message = format!("message {n}");
                    assert_eq!(
                        talk(rig.supervisor.public_socket(), message.as_bytes()),
                        message.as_bytes()
                    );
                })
            })
            .collect();
        for worker in workers {
            worker.join().unwrap();
        }
        assert_eq!(
            rig.supervisor.spawn_count(),
            1,
            "one spawn for eight simultaneous first connections"
        );
        assert_eq!(talk(rig.supervisor.public_socket(), b"again"), b"again");
        assert_eq!(rig.supervisor.spawn_count(), 1);
    }

    #[test]
    fn a_connection_counts_as_activity() {
        let rig = rig(
            "activity",
            echo_spawner(),
            Duration::from_secs(60),
            Duration::from_secs(10),
        );
        talk(rig.supervisor.public_socket(), b"hello");
        // Immediately after use it is not idle-eligible, even long after the
        // registration time, because the connection reset the idle clock.
        let registry = rig.registry.lock().unwrap();
        assert!(registry
            .idle_eligible_for_teardown(Instant::now() + Duration::from_secs(30))
            .is_empty());
        // ...but it does become eligible once the idle timeout has passed.
        assert_eq!(
            registry.idle_eligible_for_teardown(Instant::now() + Duration::from_secs(120)),
            vec![ROLE.to_string()]
        );
    }

    #[test]
    fn a_torn_down_assistant_is_started_again_by_the_next_connection() {
        let rig = rig(
            "respawn",
            echo_spawner(),
            Duration::from_secs(60),
            Duration::from_secs(10),
        );
        assert_eq!(talk(rig.supervisor.public_socket(), b"one"), b"one");
        rig.registry.lock().unwrap().teardown(ROLE);
        assert!(!resident(&rig));
        assert_eq!(talk(rig.supervisor.public_socket(), b"two"), b"two");
        assert_eq!(rig.supervisor.spawn_count(), 2);
        assert!(resident(&rig));
    }

    #[test]
    fn a_crashed_assistant_is_noticed_and_replaced() {
        let rig = rig(
            "crash",
            echo_spawner(),
            Duration::from_secs(60),
            Duration::from_secs(10),
        );
        assert_eq!(talk(rig.supervisor.public_socket(), b"one"), b"one");
        // Kill it behind the supervisor's back, without going through teardown.
        // (Take the child out, kill it, and put an already-dead one back.)
        {
            let mut registry = rig.registry.lock().unwrap();
            registry.teardown(ROLE);
            let dead = Command::new("true").spawn().unwrap();
            assert!(registry.set_resident(ROLE, dead, Instant::now()).is_none());
        }
        thread::sleep(Duration::from_millis(200));
        assert_eq!(talk(rig.supervisor.public_socket(), b"two"), b"two");
        assert_eq!(rig.supervisor.spawn_count(), 2);
    }

    #[test]
    fn an_assistant_that_never_listens_is_abandoned_and_the_client_sees_a_closed_connection() {
        let never: Spawner = Box::new(|_| Command::new("sleep").arg("300").spawn());
        let rig = rig(
            "never",
            never,
            Duration::from_secs(60),
            Duration::from_millis(300),
        );
        let started = Instant::now();
        assert!(is_unavailable(try_talk(
            rig.supervisor.public_socket(),
            b"hello"
        )));
        assert!(started.elapsed() < Duration::from_secs(5));
        assert!(!resident(&rig), "the stuck child must not be left resident");
    }

    #[test]
    fn an_assistant_that_exits_while_starting_fails_fast() {
        let exits: Spawner = Box::new(|_| Command::new("false").spawn());
        let rig = rig(
            "exits",
            exits,
            Duration::from_secs(60),
            Duration::from_secs(20),
        );
        let started = Instant::now();
        assert!(is_unavailable(try_talk(
            rig.supervisor.public_socket(),
            b"hello"
        )));
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "an early exit must not wait out the ready timeout"
        );
        assert!(!resident(&rig));
    }

    #[test]
    fn a_spawner_that_fails_is_a_closed_connection_not_a_crash() {
        let broken: Spawner = Box::new(|_| Err(io::Error::other("no such binary")));
        let rig = rig(
            "broken",
            broken,
            Duration::from_secs(60),
            Duration::from_secs(5),
        );
        assert!(is_unavailable(try_talk(
            rig.supervisor.public_socket(),
            b"hello"
        )));
        // The supervisor is still serving: a later connection tries again.
        assert!(is_unavailable(try_talk(
            rig.supervisor.public_socket(),
            b"again"
        )));
    }

    #[test]
    fn dropping_the_supervisor_kills_the_child_and_removes_its_sockets() {
        let rig = rig(
            "drop",
            echo_spawner(),
            Duration::from_secs(60),
            Duration::from_secs(10),
        );
        let public = rig.supervisor.public_socket().to_path_buf();
        let private = rig.supervisor.inner.private_socket.clone();
        talk(&public, b"hello");
        assert!(resident(&rig));
        let registry = Arc::clone(&rig.registry);
        drop(rig);
        assert!(!registry.lock().unwrap().is_resident(ROLE));
        assert!(!public.exists());
        assert!(!private.exists());
    }

    #[test]
    fn a_connection_that_arrives_once_stopping_has_begun_never_starts_an_assistant() {
        let rig = rig(
            "stopping",
            echo_spawner(),
            Duration::from_secs(60),
            Duration::from_secs(10),
        );
        rig.supervisor.inner.stop.store(true, Ordering::Relaxed);
        assert!(is_unavailable(try_talk(
            rig.supervisor.public_socket(),
            b"late"
        )));
        assert_eq!(rig.supervisor.spawn_count(), 0);
        assert!(!resident(&rig));
        // Restore the flag so the rig's own drop can run its normal path.
        rig.supervisor.inner.stop.store(false, Ordering::Relaxed);
    }

    /// Live processes whose command line mentions `needle`, excluding this
    /// pgrep itself.
    fn processes_mentioning(needle: &str) -> usize {
        let output = Command::new("pgrep").args(["-f", needle]).output().unwrap();
        String::from_utf8_lossy(&output.stdout)
            .split_whitespace()
            .count()
    }

    #[test]
    fn dropping_while_connections_are_arriving_leaves_no_assistant_behind() {
        for round in 0..5 {
            let rig = rig(
                &format!("orphan{round}"),
                echo_spawner(),
                Duration::from_secs(60),
                Duration::from_secs(10),
            );
            let public = rig.supervisor.public_socket().to_path_buf();
            let private = rig
                .supervisor
                .inner
                .private_socket
                .to_string_lossy()
                .to_string();
            // A connection is in flight (and may be spawning) as the drop begins.
            let racer = {
                let public = public.clone();
                thread::spawn(move || {
                    let _ = try_talk(&public, b"racing the drop");
                })
            };
            drop(rig);
            racer.join().unwrap();
            // Give a wrongly late spawn time to appear, then look for it by the
            // unique private socket path it was started with.
            thread::sleep(Duration::from_millis(300));
            assert_eq!(
                processes_mentioning(&private),
                0,
                "round {round}: an assistant outlived its supervisor"
            );
        }
    }

    #[test]
    fn the_public_socket_is_private_to_its_owner() {
        use std::os::unix::fs::PermissionsExt;
        let rig = rig(
            "mode",
            echo_spawner(),
            Duration::from_secs(60),
            Duration::from_secs(10),
        );
        let mode = std::fs::metadata(rig.supervisor.public_socket())
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(
            mode & 0o077,
            0,
            "group and other must have no access: {mode:o}"
        );
    }

    #[test]
    fn the_assistant_binary_is_found_beside_the_launcher_even_from_a_test_binary() {
        assert_eq!(
            sibling_assistant_binary(Path::new("/build/debug/blueice-launcher")),
            Path::new("/build/debug/blueice-ai-assistant")
        );
        assert_eq!(
            sibling_assistant_binary(Path::new("/build/debug/deps/launcher-abc123")),
            Path::new("/build/debug/blueice-ai-assistant")
        );
    }

    #[test]
    fn an_unconfigured_assistant_is_never_supervised() {
        let registry = Arc::new(Mutex::new(ProcessRegistry::new()));
        let settings = AssistantSettings::default();
        let supervisor =
            AssistantSupervisor::start(&settings, PathBuf::from("/nonexistent"), registry.clone())
                .unwrap();
        assert!(supervisor.is_none());
        assert!(!registry.lock().unwrap().is_resident(ROLE));
    }

    #[test]
    fn invalid_settings_are_refused_before_anything_starts() {
        let registry = Arc::new(Mutex::new(ProcessRegistry::new()));
        let settings = AssistantSettings {
            idle_timeout_secs: 1,
            ..AssistantSettings::default()
        };
        assert!(AssistantSupervisor::start(&settings, PathBuf::from("/x"), registry).is_err());
    }
}
