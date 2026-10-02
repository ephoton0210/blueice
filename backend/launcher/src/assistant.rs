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
        let spawn_number = self.spawns.fetch_add(1, Ordering::Relaxed) + 1;
        crate::trace::event(
            "assistant.spawn",
            &format!("pid {} (spawn #{spawn_number})", child.id()),
        );
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
                crate::trace::event(
                    "assistant.ceiling",
                    &format!(
                        "used {} MiB, over its {} MiB ceiling; stopping it",
                        used / (1024 * 1024),
                        ceiling / (1024 * 1024)
                    ),
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
        crate::trace::event(
            "assistant.reconfigure",
            &format!("backend {:?}", settings.backend),
        );
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

    /// The private socket the child is started on (tests read files the fake
    /// assistant leaves beside it).
    #[cfg(test)]
    pub(crate) fn private_socket_for_tests(&self) -> PathBuf {
        self.inner.private_socket.clone()
    }

    /// The socket `core` is given as `--assistant-socket`.
    pub fn public_socket(&self) -> &Path {
        &self.public_socket
    }

    /// The pid of the assistant if it is running now.
    pub fn resident_pid(&self) -> Option<u32> {
        self.inner
            .registry
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .resident_pid(ROLE)
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
mod tests;
