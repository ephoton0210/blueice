// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

pub(super) fn unique_socket_path() -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("blueice-mcp-{}-{n}.sock", std::process::id()))
}

pub(super) fn wait_for_socket(path: &Path, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if path.exists() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

/// Whether this [`CoreProcess`] owns a private `core` it spawned itself
/// (and must tear down), or is merely attached to one shared via
/// `blueice-launcher`'s rendezvous socket (and must *not* tear it down
/// -- other clients, e.g. a human's `frontend`, may depend on it
/// staying up). See [`CoreProcess::connect`].
pub(super) enum CoreOwnership {
    PrivatelySpawned { child: Child, socket_path: PathBuf },
    Shared,
}

/// Connects to (or spawns) `blueice-core` -- the process-management
/// half `main.rs` drives; kept separate from [`CoreConnection`]'s pure
/// message-sequencing logic so that logic stays testable without a
/// real subprocess.
pub struct CoreProcess {
    pub(super) ownership: CoreOwnership,
    pub conn: Arc<Mutex<CoreConnection<std::os::unix::net::UnixStream>>>,
}

/// Connects to a socket a child process is still setting up. The socket
/// file appears at `bind` but only accepts connections after `listen`, so
/// `wait_for_socket` seeing the path does not mean the child is ready: on a
/// loaded machine a connect in between is refused. Retry that briefly.
pub(super) fn connect_when_listening(
    path: &Path,
    timeout: Duration,
) -> io::Result<std::os::unix::net::UnixStream> {
    let deadline = Instant::now() + timeout;
    loop {
        match std::os::unix::net::UnixStream::connect(path) {
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::ConnectionRefused | io::ErrorKind::NotFound
                ) && Instant::now() < deadline =>
            {
                std::thread::sleep(Duration::from_millis(10));
            }
            result => return result,
        }
    }
}

impl CoreProcess {
    /// Tries `blueice-launcher`'s well-known rendezvous socket first --
    /// sharing whatever `core` instance (and `Page`) is already running
    /// there, the same one a human's `frontend` may be watching, per
    /// `phase-8-live-core-hotswap/PLAN.md`'s "Minimal first slice" --
    /// falling back to [`CoreProcess::spawn`]'s private, unshared
    /// `core` only if nothing is listening there (no launcher running,
    /// e.g. a standalone dev/test workflow). `BlueIceMcpServer` calls
    /// this lazily from its first browser-facing tool; `spawn` itself
    /// stays available directly for callers (and tests) that
    /// specifically want a private instance regardless.
    pub fn connect(width: u32, height: u32) -> io::Result<Self> {
        Self::connect_to(
            &blueice_launcher::default_rendezvous_socket_path(),
            width,
            height,
        )
    }

    /// Attaches to one exact launcher rendezvous socket without ever spawning
    /// a private replacement core. Phase 6 uses this stricter form so a
    /// missing shared session cannot silently invalidate the human-and-agent
    /// same-render-pass proof.
    pub fn attach_to(rendezvous_socket: &Path, _width: u32, _height: u32) -> io::Result<Self> {
        let stream = std::os::unix::net::UnixStream::connect(rendezvous_socket)?;
        Self::from_shared_stream(stream)
    }

    /// Connects to one already-running core control socket without
    /// spawning a fallback process.  This is for an embedding that owns
    /// both the browser and compiler endpoints and must keep them bound
    /// to the same core lifetime.
    pub(crate) fn connect_existing(core_socket: &Path) -> io::Result<Self> {
        let stream = std::os::unix::net::UnixStream::connect(core_socket)?;
        Self::from_shared_stream(stream)
    }

    fn from_shared_stream(stream: std::os::unix::net::UnixStream) -> io::Result<Self> {
        let mut conn = CoreConnection::new(stream);
        conn.handshake()?;
        Ok(CoreProcess {
            ownership: CoreOwnership::Shared,
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// The testable half of [`CoreProcess::connect`], taking the
    /// rendezvous path as a parameter instead of always resolving
    /// [`blueice_launcher::default_rendezvous_socket_path`] -- lets a
    /// test point it at a real (temp-path) listener standing in for
    /// `blueice-launcher`, without mutating the process-wide
    /// `XDG_RUNTIME_DIR` environment variable (unsafe to do under
    /// parallel test execution, since env vars are global process
    /// state).
    pub(super) fn connect_to(
        rendezvous_socket: &Path,
        width: u32,
        height: u32,
    ) -> io::Result<Self> {
        match std::os::unix::net::UnixStream::connect(rendezvous_socket) {
            Ok(stream) => Self::from_shared_stream(stream),
            Err(_) => Self::spawn(width, height),
        }
    }

    pub fn spawn(width: u32, height: u32) -> io::Result<Self> {
        let this_exe = std::env::current_exe()?;
        let core_bin = sibling_core_binary(&this_exe);
        let socket_path = unique_socket_path();
        let _ = std::fs::remove_file(&socket_path);

        let child = Command::new(&core_bin)
            .arg("--socket")
            .arg(&socket_path)
            .arg("--width")
            .arg(width.to_string())
            .arg("--height")
            .arg(height.to_string())
            .spawn()?;

        if !wait_for_socket(&socket_path, Duration::from_secs(5)) {
            return Err(io::Error::other(format!(
                "blueice-core never created its socket at {}",
                socket_path.display()
            )));
        }
        let stream = connect_when_listening(&socket_path, Duration::from_secs(5))?;
        let mut conn = CoreConnection::new(stream);
        conn.handshake()?;
        Ok(CoreProcess {
            ownership: CoreOwnership::PrivatelySpawned { child, socket_path },
            conn: Arc::new(Mutex::new(conn)),
        })
    }
}

impl Drop for CoreProcess {
    fn drop(&mut self) {
        match &mut self.ownership {
            // A privately-spawned `core` is ours alone: tell it to
            // shut down, then reap it and clean up its socket, exactly
            // as before this constructor grew a second mode.
            CoreOwnership::PrivatelySpawned { child, socket_path } => {
                if let Ok(mut conn) = self.conn.lock() {
                    let _ = conn.shutdown();
                }
                let _ = child.wait();
                let _ = std::fs::remove_file(socket_path);
            }
            // A shared `core` belongs to the launcher and whatever
            // other clients are attached to it (e.g. a human's
            // `frontend`) -- sending it `Shutdown` here would end the
            // render pass for all of them, not just disconnect this
            // one client. Just let the connection close naturally.
            CoreOwnership::Shared => {}
        }
    }
}
