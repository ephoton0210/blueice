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

/// What the launcher enforces on the assistant from outside
/// (`phase-7-local-ai/PLAN.md`, step R3). The assistant is never trusted to
/// limit itself: a runaway process is exactly the one that would not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Kill the assistant if its resident memory exceeds this many bytes.
    /// (A `cgroup` or Job Object limit would need privileges the launcher
    /// cannot assume; watching resident memory is portable and testable.)
    pub max_resident_bytes: Option<u64>,
    /// Scheduling niceness applied to the child (0 to 19), so a busy model does
    /// not starve `core` of CPU.
    pub nice: Option<i32>,
    /// How often resident memory is checked.
    pub watch_interval: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            max_resident_bytes: None,
            nice: None,
            watch_interval: Duration::from_secs(1),
        }
    }
}

/// Lowers `pid`'s scheduling priority to `nice`. Failure is reported, not
/// fatal: an assistant that could not be deprioritized still works, and the
/// launcher should not refuse to run it over a scheduling nicety.
fn apply_nice(pid: u32, nice: i32) -> io::Result<()> {
    // SAFETY: `setpriority` only reads its integer arguments; an invalid pid
    // is reported through the return value and `errno`, never as UB.
    let result = unsafe { libc::setpriority(libc::PRIO_PROCESS, pid as libc::id_t, nice) };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// Starts the assistant child listening on the given private socket path.
type Spawner = Box<dyn Fn(&Path) -> io::Result<Child> + Send + Sync>;

struct Inner {
    /// Replaceable at runtime by [`AssistantSupervisor::reconfigure`].
    spawner: Mutex<Spawner>,
    private_socket: PathBuf,
    registry: Arc<Mutex<ProcessRegistry>>,
    /// Serializes "is it running? if not, start it and wait until it listens"
    /// so many simultaneous first connections cause one spawn.
    spawn_lock: Mutex<()>,
    ready_timeout: Duration,
    limits: Mutex<Limits>,
    stop: AtomicBool,
    spawns: AtomicU64,
    relays: AtomicUsize,
}

/// The public socket, its accept thread, and the on-demand child behind it.
pub struct AssistantSupervisor {
    inner: Arc<Inner>,
    public_socket: PathBuf,
    /// The assistant binary, when started from real settings.
    assistant_bin: Option<PathBuf>,
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
        let child = (self.spawner.lock().unwrap_or_else(|e| e.into_inner()))(&self.private_socket)?;
        self.spawns.fetch_add(1, Ordering::Relaxed);
        let nice = self.limits.lock().unwrap_or_else(|e| e.into_inner()).nice;
        if let Some(nice) = nice {
            if let Err(error) = apply_nice(child.id(), nice) {
                eprintln!("blueice-launcher: could not set the assistant's priority: {error}");
            }
        }
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
            // Gone for any reason: it exited on its own, or something else (the
            // memory watchdog, a teardown) stopped it while it was still
            // starting, in which case the registry no longer holds it at all.
            let exited = {
                let mut registry = self.registry.lock().unwrap_or_else(|e| e.into_inner());
                registry.reap_exited(ROLE) || registry.resident_pid(ROLE).is_none()
            };
            if exited || Instant::now() >= deadline {
                let mut registry = self.registry.lock().unwrap_or_else(|e| e.into_inner());
                registry.teardown(ROLE);
                return Err(io::Error::other(if exited {
                    "the assistant stopped while starting"
                } else {
                    "the assistant did not start listening in time"
                }));
            }
            thread::sleep(Duration::from_millis(20));
        }
    }

    /// Stops the assistant when its resident memory passes the ceiling. Runs
    /// until the supervisor stops, reading the limit afresh every tick so a
    /// reconfiguration takes effect at once (and a ceiling can be added to an
    /// assistant that started without one); the next connection starts a fresh
    /// assistant.
    fn watch_memory(self: Arc<Self>) {
        let mut system = sysinfo::System::new();
        while !self.stop.load(Ordering::Relaxed) {
            let limits = *self.limits.lock().unwrap_or_else(|e| e.into_inner());
            thread::sleep(limits.watch_interval);
            let Some(ceiling) = limits.max_resident_bytes else {
                continue;
            };
            let pid = self
                .registry
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .resident_pid(ROLE);
            let Some(pid) = pid else { continue };
            let pid = sysinfo::Pid::from_u32(pid);
            system.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[pid]), true);
            let Some(used) = system.process(pid).map(|process| process.memory()) else {
                continue;
            };
            if used > ceiling {
                eprintln!(
                    "blueice-launcher: the assistant used {} MiB, over its {} MiB ceiling; stopping it",
                    used / (1024 * 1024),
                    ceiling / (1024 * 1024)
                );
                self.registry
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .teardown(ROLE);
            }
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
        limits: Limits,
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
            spawner: Mutex::new(spawner),
            private_socket,
            registry,
            spawn_lock: Mutex::new(()),
            ready_timeout,
            limits: Mutex::new(limits),
            stop: AtomicBool::new(false),
            spawns: AtomicU64::new(0),
            relays: AtomicUsize::new(0),
        });
        let watcher = Arc::clone(&inner);
        thread::spawn(move || watcher.watch_memory());
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
            assistant_bin: None,
        })
    }

    /// Applies new settings at once: the spawn flags, the limits, and the idle
    /// policy all change, and the assistant that is running (started under the
    /// old ones) is torn down, so the next connection starts it as configured.
    /// `spawner` is what starts the child from now on.
    pub fn reconfigure(&self, spawner: Spawner, limits: Limits, idle_timeout: Duration) {
        // Wait out a spawn in progress so its child is the one torn down below.
        let _no_spawn_in_progress = self
            .inner
            .spawn_lock
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        *self.inner.spawner.lock().unwrap_or_else(|e| e.into_inner()) = spawner;
        *self.inner.limits.lock().unwrap_or_else(|e| e.into_inner()) = limits;
        let mut registry = self
            .inner
            .registry
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        registry.set_policy(ROLE, ProcessPolicy::OnDemand { idle_timeout });
        registry.teardown(ROLE);
    }

    /// [`Self::reconfigure`] for real settings, using the assistant binary this
    /// supervisor was started with.
    pub fn reconfigure_settings(&self, settings: &AssistantSettings) -> io::Result<()> {
        settings.validate().map_err(io::Error::other)?;
        let bin = self.assistant_bin.clone().ok_or_else(|| {
            io::Error::other("this supervisor has no assistant binary to reconfigure")
        })?;
        self.reconfigure(
            settings_spawner(settings, bin),
            settings_limits(settings),
            Duration::from_secs(settings.idle_timeout_secs),
        );
        Ok(())
    }

    /// Supervises the real `blueice-ai-assistant` at `assistant_bin`, started
    /// with the flags `settings` renders to. Unconfigured settings still get a
    /// supervisor and a public socket -- every connection is simply refused --
    /// so a later approved change can turn the assistant on.
    pub fn start(
        settings: &AssistantSettings,
        assistant_bin: PathBuf,
        registry: Arc<Mutex<ProcessRegistry>>,
    ) -> io::Result<Self> {
        settings.validate().map_err(io::Error::other)?;
        let (public_socket, private_socket) = unique_socket_paths();
        let mut supervisor = Self::start_with_spawner(
            Duration::from_secs(settings.idle_timeout_secs),
            settings_spawner(settings, assistant_bin.clone()),
            registry,
            public_socket,
            private_socket,
            DEFAULT_READY_TIMEOUT,
            settings_limits(settings),
        )?;
        supervisor.assistant_bin = Some(assistant_bin);
        Ok(supervisor)
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

/// How to start the assistant for `settings`. Unconfigured settings produce a
/// spawner that always fails, so connections are refused rather than served by
/// something the person never configured.
fn settings_spawner(settings: &AssistantSettings, assistant_bin: PathBuf) -> Spawner {
    if !settings.is_configured() {
        return Box::new(|_| Err(io::Error::other("no assistant is configured")));
    }
    let args = settings.assistant_args();
    Box::new(move |private_socket| {
        Command::new(&assistant_bin)
            .arg("--socket")
            .arg(private_socket)
            .args(&args)
            .spawn()
    })
}

/// The limits `settings` asks the launcher to enforce.
fn settings_limits(settings: &AssistantSettings) -> Limits {
    Limits {
        max_resident_bytes: settings.max_resident_mb.map(|mb| mb * 1024 * 1024),
        nice: Some(settings.nice),
        ..Limits::default()
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
        rig_with_limits(tag, spawner, idle, ready, Limits::default())
    }

    fn rig_with_limits(
        tag: &str,
        spawner: Spawner,
        idle: Duration,
        ready: Duration,
        limits: Limits,
    ) -> Rig {
        let registry = Arc::new(Mutex::new(ProcessRegistry::new()));
        let (public, private) = socket_pair_paths(tag);
        let supervisor = AssistantSupervisor::start_with_spawner(
            idle,
            spawner,
            registry.clone(),
            public,
            private,
            ready,
            limits,
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

    // ---- ceilings (step R3) ----

    /// An echo server that first commits `megabytes` of resident memory, so the
    /// watchdog has a real, measurable process to judge.
    fn hungry_spawner(megabytes: usize) -> Spawner {
        Box::new(move |socket| {
            let script = format!("_hog = b'x' * ({megabytes} * 1024 * 1024)\n{ECHO_SERVER}");
            Command::new("python3")
                .args(["-c", &script])
                .arg(socket)
                .spawn()
        })
    }

    /// An echo server that only balloons *after* it is serving, so it is judged
    /// while a client is connected to it.
    fn balloons_while_serving_spawner(megabytes: usize) -> Spawner {
        Box::new(move |socket| {
            let script = ECHO_SERVER.replace(
                "while True:\n    conn, _ = server.accept()",
                &format!(
                    "hog = []\nthreading.Timer(0.3, lambda: hog.append(b'x' * ({megabytes} * 1024 * 1024))).start()\nwhile True:\n    conn, _ = server.accept()"
                ),
            );
            Command::new("python3")
                .args(["-c", &script])
                .arg(socket)
                .spawn()
        })
    }

    fn resident_pid(rig: &Rig) -> Option<u32> {
        rig.registry.lock().unwrap().resident_pid(ROLE)
    }

    fn alive(pid: u32) -> bool {
        // Signal 0 checks existence without delivering anything. A zombie still
        // counts as existing, but the registry reaps what it kills.
        Command::new("kill")
            .args(["-0", &pid.to_string()])
            .stderr(std::process::Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    }

    const FAST_WATCH: Duration = Duration::from_millis(50);

    #[test]
    fn an_assistant_over_its_memory_ceiling_is_stopped() {
        let limits = Limits {
            max_resident_bytes: Some(150 * 1024 * 1024),
            watch_interval: FAST_WATCH,
            ..Limits::default()
        };
        let rig = rig_with_limits(
            "hungry",
            hungry_spawner(300),
            Duration::from_secs(60),
            Duration::from_secs(20),
            limits,
        );
        // The connection starts it; its allocation then exceeds the ceiling.
        let _ = try_talk(rig.supervisor.public_socket(), b"hello");
        let deadline = Instant::now() + Duration::from_secs(15);
        while resident_pid(&rig).is_some() {
            assert!(
                Instant::now() < deadline,
                "the over-limit assistant was never stopped"
            );
            thread::sleep(Duration::from_millis(50));
        }
        assert!(!resident(&rig));
        // And the supervisor still works: the next connection starts another.
        let before = rig.supervisor.spawn_count();
        let _ = try_talk(rig.supervisor.public_socket(), b"again");
        assert!(
            rig.supervisor.spawn_count() > before,
            "it must respawn on demand"
        );
    }

    #[test]
    fn an_assistant_that_balloons_while_serving_is_stopped_and_its_client_is_released() {
        let limits = Limits {
            max_resident_bytes: Some(150 * 1024 * 1024),
            watch_interval: FAST_WATCH,
            ..Limits::default()
        };
        let rig = rig_with_limits(
            "balloon",
            balloons_while_serving_spawner(300),
            Duration::from_secs(60),
            Duration::from_secs(20),
            limits,
        );
        // A connected client that is simply waiting for a reply.
        let mut waiting = UnixStream::connect(rig.supervisor.public_socket()).unwrap();
        waiting
            .set_read_timeout(Some(Duration::from_secs(15)))
            .unwrap();
        let started = Instant::now();
        let mut byte = [0u8; 1];
        // The killed assistant's death must release this connection, not leave it
        // hanging: EOF or a reset, well before the read timeout.
        let outcome = waiting.read(&mut byte);
        assert!(
            matches!(outcome, Ok(0) | Err(_)),
            "no data was ever sent, so anything but a close is wrong: {outcome:?}"
        );
        assert!(
            started.elapsed() < Duration::from_secs(12),
            "the client was left waiting for {:?}",
            started.elapsed()
        );
        assert!(!resident(&rig), "the over-limit assistant must be gone");
    }

    #[test]
    fn an_assistant_under_its_ceiling_is_left_alone() {
        let limits = Limits {
            max_resident_bytes: Some(4096 * 1024 * 1024),
            watch_interval: FAST_WATCH,
            ..Limits::default()
        };
        let rig = rig_with_limits(
            "modest",
            echo_spawner(),
            Duration::from_secs(60),
            Duration::from_secs(10),
            limits,
        );
        assert_eq!(talk(rig.supervisor.public_socket(), b"hi"), b"hi");
        let pid = resident_pid(&rig).expect("resident");
        thread::sleep(Duration::from_millis(600)); // a dozen watchdog checks
        assert_eq!(resident_pid(&rig), Some(pid));
        assert!(alive(pid));
    }

    #[test]
    fn without_a_ceiling_a_large_assistant_is_left_alone() {
        let rig = rig(
            "unlimited",
            hungry_spawner(300),
            Duration::from_secs(60),
            Duration::from_secs(20),
        );
        assert_eq!(talk(rig.supervisor.public_socket(), b"hi"), b"hi");
        let pid = resident_pid(&rig).expect("resident");
        thread::sleep(Duration::from_millis(300));
        assert_eq!(resident_pid(&rig), Some(pid));
    }

    #[test]
    fn the_watchdog_ends_with_its_supervisor() {
        let limits = Limits {
            max_resident_bytes: Some(4096 * 1024 * 1024),
            watch_interval: FAST_WATCH,
            ..Limits::default()
        };
        let rig = rig_with_limits(
            "watchend",
            echo_spawner(),
            Duration::from_secs(60),
            Duration::from_secs(10),
            limits,
        );
        let registry = Arc::clone(&rig.registry);
        drop(rig);
        // The watchdog held a share of the supervisor's state; once it has
        // noticed the stop flag, only this test's handle remains.
        let deadline = Instant::now() + Duration::from_secs(5);
        while Arc::strong_count(&registry) > 1 {
            assert!(Instant::now() < deadline, "the watchdog thread never ended");
            thread::sleep(Duration::from_millis(20));
        }
    }

    /// The niceness of a live process, read the portable way.
    fn niceness(pid: u32) -> i32 {
        let output = Command::new("ps")
            .args(["-o", "ni=", "-p", &pid.to_string()])
            .output()
            .unwrap();
        String::from_utf8_lossy(&output.stdout)
            .trim()
            .parse()
            .unwrap()
    }

    #[test]
    fn the_assistants_priority_is_lowered_as_configured() {
        let limits = Limits {
            nice: Some(10),
            ..Limits::default()
        };
        let rig = rig_with_limits(
            "nice",
            echo_spawner(),
            Duration::from_secs(60),
            Duration::from_secs(10),
            limits,
        );
        assert_eq!(talk(rig.supervisor.public_socket(), b"hi"), b"hi");
        let pid = resident_pid(&rig).expect("resident");
        assert_eq!(niceness(pid), niceness(std::process::id()) + 10);
    }

    #[test]
    fn no_priority_setting_leaves_the_priority_alone() {
        let rig = rig(
            "plain",
            echo_spawner(),
            Duration::from_secs(60),
            Duration::from_secs(10),
        );
        assert_eq!(talk(rig.supervisor.public_socket(), b"hi"), b"hi");
        let pid = resident_pid(&rig).expect("resident");
        assert_eq!(niceness(pid), niceness(std::process::id()));
    }

    #[test]
    fn setting_the_priority_of_a_process_that_does_not_exist_is_an_error_not_ub() {
        // Pid 0x7fff_fff0 is far above any real pid_max.
        assert!(apply_nice(0x7fff_fff0, 5).is_err());
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
    fn an_unconfigured_assistant_is_supervised_but_every_connection_is_refused() {
        let registry = Arc::new(Mutex::new(ProcessRegistry::new()));
        let supervisor = AssistantSupervisor::start(
            &AssistantSettings::default(),
            PathBuf::from("/nonexistent"),
            registry.clone(),
        )
        .unwrap();
        assert!(is_unavailable(try_talk(supervisor.public_socket(), b"hi")));
        assert!(!registry.lock().unwrap().is_resident(ROLE));
        assert_eq!(supervisor.spawn_count(), 0);
    }

    /// An echo spawner that counts how many children it has started.
    fn counting_echo_spawner() -> (Spawner, Arc<AtomicUsize>) {
        let started = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&started);
        (
            Box::new(move |socket| {
                counter.fetch_add(1, Ordering::SeqCst);
                Command::new("python3")
                    .args(["-c", ECHO_SERVER])
                    .arg(socket)
                    .spawn()
            }),
            started,
        )
    }

    #[test]
    fn reconfiguring_tears_down_the_running_assistant_and_the_new_spawner_starts_the_next() {
        let (first, first_count) = counting_echo_spawner();
        let rig = rig(
            "reconf",
            first,
            Duration::from_secs(600),
            Duration::from_secs(10),
        );
        assert_eq!(talk(rig.supervisor.public_socket(), b"one"), b"one");
        let old_pid = resident_pid(&rig).unwrap();

        let (second, second_count) = counting_echo_spawner();
        rig.supervisor
            .reconfigure(second, Limits::default(), Duration::from_secs(600));
        assert!(!resident(&rig), "the old assistant must be gone at once");
        assert!(!alive(old_pid));

        assert_eq!(talk(rig.supervisor.public_socket(), b"two"), b"two");
        assert_eq!(first_count.load(Ordering::SeqCst), 1);
        assert_eq!(
            second_count.load(Ordering::SeqCst),
            1,
            "the new spawner started the next one"
        );
    }

    #[test]
    fn reconfiguring_to_unconfigured_refuses_connections_from_then_on() {
        let rig = rig(
            "reconf-off",
            echo_spawner(),
            Duration::from_secs(600),
            Duration::from_secs(10),
        );
        assert_eq!(talk(rig.supervisor.public_socket(), b"one"), b"one");
        rig.supervisor.reconfigure(
            settings_spawner(&AssistantSettings::default(), PathBuf::from("/x")),
            Limits::default(),
            Duration::from_secs(600),
        );
        assert!(is_unavailable(try_talk(
            rig.supervisor.public_socket(),
            b"two"
        )));
        assert!(!resident(&rig));
    }

    #[test]
    fn a_ceiling_added_by_reconfiguration_is_enforced_on_the_next_assistant() {
        // Started with no ceiling at all: the watchdog still runs and reads the
        // limit each tick, so a later ceiling applies without restarting anything.
        let limits = Limits {
            watch_interval: FAST_WATCH,
            ..Limits::default()
        };
        let rig = rig_with_limits(
            "reconf-ceiling",
            hungry_spawner(300),
            Duration::from_secs(600),
            Duration::from_secs(20),
            limits,
        );
        assert_eq!(talk(rig.supervisor.public_socket(), b"hi"), b"hi");
        let pid = resident_pid(&rig).unwrap();
        thread::sleep(Duration::from_millis(300));
        assert_eq!(
            resident_pid(&rig),
            Some(pid),
            "no ceiling yet, so it is left alone"
        );

        rig.supervisor.reconfigure(
            hungry_spawner(300),
            Limits {
                max_resident_bytes: Some(150 * 1024 * 1024),
                watch_interval: FAST_WATCH,
                ..Limits::default()
            },
            Duration::from_secs(600),
        );
        let _ = try_talk(rig.supervisor.public_socket(), b"again"); // starts the next one
        let deadline = Instant::now() + Duration::from_secs(15);
        while resident_pid(&rig).is_some() {
            assert!(
                Instant::now() < deadline,
                "the new ceiling was never enforced"
            );
            thread::sleep(Duration::from_millis(50));
        }
    }

    #[test]
    fn reconfiguring_changes_when_the_assistant_becomes_idle_eligible() {
        let rig = rig(
            "reconf-idle",
            echo_spawner(),
            Duration::from_secs(600),
            Duration::from_secs(10),
        );
        talk(rig.supervisor.public_socket(), b"hi");
        let soon = Instant::now() + Duration::from_secs(100);
        assert!(rig
            .registry
            .lock()
            .unwrap()
            .idle_eligible_for_teardown(soon)
            .is_empty());
        rig.supervisor
            .reconfigure(echo_spawner(), Limits::default(), Duration::from_secs(30));
        talk(rig.supervisor.public_socket(), b"hi"); // resident again, under the new policy
        assert_eq!(
            rig.registry
                .lock()
                .unwrap()
                .idle_eligible_for_teardown(Instant::now() + Duration::from_secs(100)),
            [ROLE]
        );
    }

    #[test]
    fn settings_reconfiguration_needs_a_supervisor_built_from_real_settings() {
        let rig = rig(
            "reconf-real",
            echo_spawner(),
            Duration::from_secs(600),
            Duration::from_secs(10),
        );
        // A test rig has no assistant binary to rebuild the spawner from.
        assert!(rig
            .supervisor
            .reconfigure_settings(&AssistantSettings::default())
            .is_err());

        let registry = Arc::new(Mutex::new(ProcessRegistry::new()));
        let supervisor = AssistantSupervisor::start(
            &AssistantSettings::default(),
            PathBuf::from("/x"),
            registry,
        )
        .unwrap();
        assert!(supervisor
            .reconfigure_settings(&AssistantSettings::default())
            .is_ok());
        let invalid = AssistantSettings {
            nice: 99,
            ..AssistantSettings::default()
        };
        assert!(
            supervisor.reconfigure_settings(&invalid).is_err(),
            "invalid settings are refused"
        );
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
