// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

/// The `blueice-core` binary's path, resolved relative to this
/// (`blueice-launcher`'s) own executable -- mirrors `blueice-mcp-
/// server`'s identical `sibling_core_binary` helper (not shared between
/// the two crates: each is ~10 lines, and the two processes' spawn
/// helpers are otherwise independent enough that a shared crate just
/// for this would be more indirection than the duplication costs).
/// Steps out of a `deps` directory first so the same lookup works both
/// for the installed binary and for a `cargo test` integration-test
/// binary, which lands one level deeper (`target/<profile>/deps/`).
pub(super) fn sibling_core_binary(this_exe: &Path) -> PathBuf {
    let name = if cfg!(windows) {
        "blueice-core.exe"
    } else {
        "blueice-core"
    };
    let dir = this_exe.parent().unwrap_or_else(|| Path::new("."));
    let dir = if dir.file_name().is_some_and(|n| n == "deps") {
        dir.parent().unwrap_or(dir)
    } else {
        dir
    };
    dir.join(name)
}

/// The authenticated extension host must be from the same installed build as
/// launcher and core; never resolve a different executable from `$PATH`.
pub(super) fn sibling_extension_host_binary(this_exe: &Path) -> PathBuf {
    let name = if cfg!(windows) {
        "blueice-extension-host.exe"
    } else {
        "blueice-extension-host"
    };
    let dir = this_exe.parent().unwrap_or_else(|| Path::new("."));
    let dir = if dir.file_name().is_some_and(|n| n == "deps") {
        dir.parent().unwrap_or(dir)
    } else {
        dir
    };
    dir.join(name)
}

/// The native frontend trusted for permission confirmation must
/// be from the same installed build. Never accept an arbitrary binary path
/// from a control-socket caller as a substitute for this child.
pub(super) fn sibling_frontend_binary(this_exe: &Path) -> PathBuf {
    // The macOS browser owns this launcher. Its permission window is still
    // an exact launcher-spawned child, isolated from public browser sockets.
    // An unpackaged launcher retains the ordinary sibling frontend contract.
    #[cfg(target_os = "macos")]
    if this_exe
        .file_name()
        .is_some_and(|name| name == "blueice-launcher")
    {
        if let Some(directory) = this_exe.parent() {
            if directory.file_name().is_some_and(|name| name == "MacOS")
                && directory.parent().is_some_and(|parent| {
                    parent.file_name().is_some_and(|name| name == "Contents")
                        && parent.parent().is_some_and(|bundle| {
                            bundle
                                .extension()
                                .is_some_and(|extension| extension == "app")
                        })
                })
            {
                return directory.join("BlueIcePanels.app/Contents/MacOS/BlueIcePanels");
            }
        }
    }
    let name = if cfg!(windows) {
        "blueice-frontend.exe"
    } else {
        "blueice-frontend"
    };
    let dir = this_exe.parent().unwrap_or_else(|| Path::new("."));
    let dir = if dir.file_name().is_some_and(|n| n == "deps") {
        dir.parent().unwrap_or(dir)
    } else {
        dir
    };
    dir.join(name)
}

/// An exact sibling native frontend launched by this launcher. Its stdin and
/// stdout are private anonymous pipes, not the shared frontend/MCP socket.
/// Only this child can ask for native confirmation. Launcher independently
/// validates and applies each decision through the active core's private pipe.
pub struct SpawnedTrustedWindow {
    child: Child,
}

impl SpawnedTrustedWindow {
    pub fn spawn(rendezvous_socket: &Path) -> io::Result<Self> {
        let frontend_bin = sibling_frontend_binary(&std::env::current_exe()?);
        let child = Command::new(&frontend_bin)
            .arg("--socket")
            .arg(rendezvous_socket)
            .arg("--trusted-window-stdio")
            .arg("--url")
            .arg("about:blank")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .map_err(|error| {
                io::Error::new(
                    error.kind(),
                    format!(
                        "failed to start trusted native frontend {}: {error}",
                        frontend_bin.display()
                    ),
                )
            })?;
        Ok(Self { child })
    }

    pub(super) fn take_pipes(&mut self) -> io::Result<(ChildStdout, ChildStdin)> {
        let requests = self
            .child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("trusted frontend has no private request pipe"))?;
        let replies = self
            .child
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("trusted frontend has no private reply pipe"))?;
        Ok((requests, replies))
    }
}

impl Drop for SpawnedTrustedWindow {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The always-resident safety-gatekeeper daemon lives beside `core` and
/// BlueJS. Resolve it relative to the launcher rather than `$PATH`, so
/// one installed BlueIce release never launches another release's policy
/// process by accident.
pub(super) fn sibling_gatekeeper_binary(this_exe: &Path) -> PathBuf {
    let name = if cfg!(windows) {
        "blueice-ai-gatekeeper.exe"
    } else {
        "blueice-ai-gatekeeper"
    };
    let dir = this_exe.parent().unwrap_or_else(|| Path::new("."));
    let dir = if dir.file_name().is_some_and(|n| n == "deps") {
        dir.parent().unwrap_or(dir)
    } else {
        dir
    };
    dir.join(name)
}

/// A path for `core`'s *internal* socket -- never exposed to external
/// clients, which only ever see the rendezvous socket this launcher
/// itself listens on. Includes a monotonic counter alongside the PID:
/// a PID alone isn't unique enough once a single launcher process can
/// call [`SpawnedCore::spawn`] more than once in its own lifetime (v1
/// at startup, then a fresh v2 on every cutover) -- without the
/// counter, v2's internal socket path would collide with v1's still-
/// live one.
pub(super) fn unique_internal_socket_path() -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "blueice-launcher-core-{}-{n}.sock",
        std::process::id()
    ))
}

pub(super) fn unique_internal_extension_socket_path() -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    // Core's extension listener requires a private directory, and Darwin's
    // AF_UNIX path limit leaves little room for the leaf name.
    blueice_ipc::local_socket::default_socket_dir()
        .join(format!("l-ext-{}-{n}.sock", std::process::id()))
}

/// A per-launcher private socket for its one always-resident
/// gatekeeper. It is deliberately not the well-known standalone socket:
/// multiple launchers may run at once, each with its own supervised
/// process and independent shutdown lifecycle.
pub(super) fn unique_internal_gatekeeper_socket_path() -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    // `ai-gatekeeper` deliberately binds only within a private socket
    // directory. Do not use `temp_dir()` directly here: it can be an
    // OS-managed shared directory whose mode the gatekeeper must not
    // chmod merely to create one per-launcher socket.
    blueice_ipc::local_socket::default_socket_dir().join(format!(
        "blueice-launcher-gatekeeper-{}-{n}.sock",
        std::process::id()
    ))
}

/// [`wait_for_socket`], but gives up at once if `child` exits first: a core
/// that died on startup will never create its socket, and waiting out the full
/// timeout only delays reporting (and retrying) the failure.
pub(super) fn wait_for_socket_or_exit(path: &Path, child: &mut Child, timeout: Duration) -> bool {
    wait_for_socket_or_exit_while(path, child, timeout, || true)
}

pub(super) fn wait_for_socket_or_exit_while(
    path: &Path,
    child: &mut Child,
    timeout: Duration,
    keep_waiting: impl Fn() -> bool,
) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if !keep_waiting() {
            return false;
        }
        if path.exists() {
            return true;
        }
        if child.try_wait().ok().flatten().is_some() {
            return false;
        }
        thread::sleep(Duration::from_millis(20));
    }
    false
}

/// A generation-private compiler socket.  Unlike the public endpoint
/// selected by the caller, this name is launcher-generated and never
/// exposed as a CLI/MCP input.  v1 and a staged v2 therefore can each
/// bind their own listener while the public relay remains stable.
pub(super) fn unique_internal_compiler_socket_path() -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "blueice-launcher-compiler-{}-{n}.sock",
        std::process::id()
    ))
}

/// A script-DOM listener unique to one supervised core/child pair. The
/// child capability, not knowledge of this pathname, grants access.
pub(super) fn unique_internal_script_socket_path() -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "blueice-launcher-script-{}-{n}.sock",
        std::process::id()
    ))
}

/// A generation-private debugger socket. Like the compiler socket, this
/// name is launcher-generated and cannot be supplied through the public
/// debugger transport.
pub(super) fn unique_internal_debugger_socket_path() -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "blueice-launcher-debugger-{}-{n}.sock",
        std::process::id()
    ))
}

/// Connects to a socket a child process is still setting up. The socket
/// file appears at `bind` but only accepts connections after `listen`, so
/// `wait_for_socket` seeing the path does not mean the child is ready: on a
/// loaded machine a connect in between is refused. Retry that briefly.
pub(super) fn connect_when_listening(path: &Path, timeout: Duration) -> io::Result<UnixStream> {
    connect_when_listening_while(path, timeout, || true)
}

pub(super) fn connect_when_listening_while(
    path: &Path,
    timeout: Duration,
    keep_waiting: impl Fn() -> bool,
) -> io::Result<UnixStream> {
    let deadline = Instant::now() + timeout;
    loop {
        if !keep_waiting() {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "browser broker is shutting down",
            ));
        }
        match UnixStream::connect(path) {
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::ConnectionRefused | io::ErrorKind::NotFound
                ) && Instant::now() < deadline =>
            {
                thread::sleep(Duration::from_millis(10));
            }
            result => return result,
        }
    }
}

pub(super) fn wait_for_socket(path: &Path, timeout: Duration) -> bool {
    wait_for_socket_while(path, timeout, || true)
}

pub(super) fn wait_for_socket_while(
    path: &Path,
    timeout: Duration,
    keep_waiting: impl Fn() -> bool,
) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if !keep_waiting() {
            return false;
        }
        if path.exists() {
            return true;
        }
        thread::sleep(Duration::from_millis(20));
    }
    false
}

/// The local deterministic rule-base process a launcher owns for its
/// full lifetime. It is intentionally separate from [`SpawnedCore`]:
/// a core hot-swap must preserve the same mandatory checkpoint instead
/// of creating a safety gap between v1 and v2.
pub struct SpawnedGatekeeper {
    child: Child,
    socket_path: PathBuf,
}

impl SpawnedGatekeeper {
    pub fn spawn() -> io::Result<Self> {
        Self::spawn_at(unique_internal_gatekeeper_socket_path())
    }

    /// An owner-selected endpoint for the same supervised checkpoint. Never
    /// remove an existing listener/file while establishing a new session.
    pub fn spawn_at(socket_path: PathBuf) -> io::Result<Self> {
        if std::fs::symlink_metadata(&socket_path).is_ok() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "The owned gatekeeper endpoint already exists",
            ));
        }
        let this_exe = std::env::current_exe()?;
        let gatekeeper_bin = sibling_gatekeeper_binary(&this_exe);

        let mut child = Command::new(&gatekeeper_bin)
            .arg("--socket")
            .arg(&socket_path)
            .spawn()?;
        if !wait_for_socket(&socket_path, Duration::from_secs(5)) {
            let _ = child.kill();
            let _ = child.wait();
            let _ = std::fs::remove_file(&socket_path);
            return Err(io::Error::other(format!(
                "blueice-ai-gatekeeper never created its socket at {}",
                socket_path.display()
            )));
        }
        Ok(Self { child, socket_path })
    }

    /// The private socket path every core managed by this launcher must
    /// use for its mandatory gatekeeper checks.
    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }
}

impl Drop for SpawnedGatekeeper {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.socket_path);
    }
}

pub(super) const PERMISSION_INSPECT_TIMEOUT: Duration = Duration::from_secs(2);
pub(super) const PERMISSION_CHANGE_TIMEOUT: Duration = Duration::from_secs(5);
