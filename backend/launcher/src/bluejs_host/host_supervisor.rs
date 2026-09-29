// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

/// Private child-connection material a launcher may hand only to the core it
/// is supervising.
///
/// This is deliberately not a frontend setting or a page-visible capability.
/// A caller using [`SpawnedBlueJsHost::spawn_for_core`] must pass it to a
/// trusted core startup boundary and keep the returned supervisor alive for
/// at least as long as that core. The same secret authenticates the child's
/// generation-private script DOM connection when the launcher supplied that
/// socket; it must therefore never be disclosed to frontend or page code.
pub struct BlueJsHostCoreConfig {
    socket_path: PathBuf,
    session_token: String,
}

impl BlueJsHostCoreConfig {
    /// The owner-only socket created for this one child.
    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    /// The per-spawn capability for the trusted core's page-host handshake
    /// and the supervised child's matching private script socket.
    /// Callers must not forward it to frontend/page code or log it.
    pub fn session_token(&self) -> &str {
        &self.session_token
    }
}

/// A launcher-owned child and, for the legacy direct-launcher path, its one
/// authenticated private connection. Dropping this handle kills/reaps the
/// process and removes the socket, matching [`crate::SpawnedCore`]'s
/// ownership discipline. [`Self::spawn_for_core`] instead delegates the sole
/// connection to a separately spawned trusted core while retaining process
/// supervision here.
pub struct SpawnedBlueJsHost {
    child: Child,
    socket_path: PathBuf,
    session_token: String,
    stream: Option<UnixStream>,
}

impl SpawnedBlueJsHost {
    /// Spawns the sibling `blueice-bluejs-host` binary, waits for its private
    /// socket, then performs the authenticated v1 handshake before returning
    /// a usable handle. A startup failure always reaps the child and removes
    /// the private socket.
    pub fn spawn() -> io::Result<Self> {
        Self::spawn_with_runtime_limits(BlueJsHostRuntimeLimits::default())
    }

    /// Spawns an isolated child under one owner-selected immutable per-realm
    /// envelope. This is an embedding/launcher construction API, not a
    /// page-host protocol capability and not a child-wide RSS limit.
    pub fn spawn_with_runtime_limits(limits: BlueJsHostRuntimeLimits) -> io::Result<Self> {
        let mut host = Self::spawn_unconnected(limits, None)?;
        if let Err(error) = host.connect_as_launcher() {
            host.reap_after_shutdown();
            return Err(error);
        }
        Ok(host)
    }

    /// Spawns and supervises a child whose sole authenticated connection will
    /// belong to a trusted core. The returned configuration contains the
    /// one-time capability and must be conveyed through a launcher-owned
    /// startup boundary, never from a page or frontend request.
    ///
    /// This does not alter the normal `blueice-launcher` command path. It is
    /// the narrow lifecycle hand-off used by the explicitly opted-in core
    /// adapter; the caller retains this supervisor until core exits.
    pub fn spawn_for_core() -> io::Result<(Self, BlueJsHostCoreConfig)> {
        Self::spawn_for_core_with_runtime_limits(BlueJsHostRuntimeLimits::default())
    }

    /// Equivalent to [`Self::spawn_for_core`], with an immutable launcher
    /// owner-selected per-realm envelope supplied to this one child before it
    /// binds its private socket. The delegated core receives neither the
    /// limits nor an operation to change them.
    pub fn spawn_for_core_with_runtime_limits(
        limits: BlueJsHostRuntimeLimits,
    ) -> io::Result<(Self, BlueJsHostCoreConfig)> {
        let host = Self::spawn_unconnected(limits, None)?;
        let config = BlueJsHostCoreConfig {
            socket_path: host.socket_path.clone(),
            session_token: host.session_token.clone(),
        };
        Ok((host, config))
    }

    /// Gives the supervised child only this generation's launcher-selected
    /// core script socket and its existing per-child capability. The probe is
    /// enabled solely by the explicit DOM lookup proof profile; ordinary
    /// child realms receive no page-visible DOM callback yet.
    pub(crate) fn spawn_for_core_with_script_socket_and_runtime_limits(
        script_socket: &Path,
        limits: BlueJsHostRuntimeLimits,
        enable_dom_lookup_probe: bool,
        enable_dom_text_profile: bool,
        enable_dom_mutation_profile: bool,
        enable_dom_event_profile: bool,
    ) -> io::Result<(Self, BlueJsHostCoreConfig)> {
        if !script_socket.is_absolute()
            || [
                enable_dom_lookup_probe,
                enable_dom_text_profile,
                enable_dom_mutation_profile,
                enable_dom_event_profile,
            ]
            .into_iter()
            .filter(|enabled| *enabled)
            .count()
                > 1
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "child script socket must be absolute and DOM profiles exclusive",
            ));
        }
        let host = Self::spawn_unconnected(
            limits,
            Some((
                script_socket,
                enable_dom_lookup_probe,
                enable_dom_text_profile,
                enable_dom_mutation_profile,
                enable_dom_event_profile,
            )),
        )?;
        let config = BlueJsHostCoreConfig {
            socket_path: host.socket_path.clone(),
            session_token: host.session_token.clone(),
        };
        Ok((host, config))
    }

    fn spawn_unconnected(
        limits: BlueJsHostRuntimeLimits,
        script_socket: Option<(&Path, bool, bool, bool, bool)>,
    ) -> io::Result<Self> {
        limits
            .runtime_config()
            .map_err(|message| io::Error::new(io::ErrorKind::InvalidInput, message))?;
        let this_exe = std::env::current_exe()?;
        let binary = sibling_bluejs_host_binary(&this_exe);
        let socket_path = unique_bluejs_host_socket_path();
        let token = secure_session_token()?;
        let _ = fs::remove_file(&socket_path);
        let mut command = Command::new(&binary);
        command
            .arg("--socket")
            .arg(&socket_path)
            .arg("--session-token")
            .arg(&token)
            .arg("--max-realms")
            .arg(limits.max_realms.to_string())
            .arg("--max-programs-per-realm")
            .arg(limits.max_programs_per_realm.to_string())
            .arg("--max-bytecode-bytes-per-realm")
            .arg(limits.max_bytecode_bytes_per_realm.to_string())
            .arg("--max-heap-bytes-per-realm")
            .arg(limits.max_heap_bytes_per_realm.to_string())
            .arg("--max-reserved-programs")
            .arg(limits.max_reserved_programs.to_string())
            .arg("--max-reserved-bytecode-bytes")
            .arg(limits.max_reserved_bytecode_bytes.to_string())
            .arg("--max-reserved-heap-bytes")
            .arg(limits.max_reserved_heap_bytes.to_string());
        if let Some((
            script_socket,
            enable_dom_lookup_probe,
            enable_dom_text_profile,
            enable_dom_mutation_profile,
            enable_dom_event_profile,
        )) = script_socket
        {
            command.arg("--script-socket").arg(script_socket);
            if enable_dom_lookup_probe {
                command.arg("--enable-dom-lookup-probe");
            }
            if enable_dom_text_profile {
                command.arg("--enable-dom-text-profile");
            }
            if enable_dom_mutation_profile {
                command.arg("--enable-dom-mutation-profile");
            }
            if enable_dom_event_profile {
                command.arg("--enable-dom-event-profile");
            }
        }
        let mut child = command.spawn()?;

        let started = wait_for_child_socket(&mut child, &socket_path, STARTUP_TIMEOUT);
        if let Err(error) = started {
            let _ = child.kill();
            let _ = child.wait();
            let _ = fs::remove_file(&socket_path);
            return Err(error);
        }
        Ok(Self {
            child,
            socket_path,
            session_token: token,
            stream: None,
        })
    }

    fn connect_as_launcher(&mut self) -> io::Result<()> {
        // A Unix socket pathname becomes visible at bind(2), before listen(2)
        // has completed. Under load, the launcher can observe the path in
        // `spawn_unconnected` and race that small interval. Only retry those
        // transient startup errors; never retry a rejected handshake.
        let deadline = Instant::now() + STARTUP_TIMEOUT;
        let mut stream = loop {
            match UnixStream::connect(&self.socket_path) {
                Ok(stream) => break stream,
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::ConnectionRefused | io::ErrorKind::NotFound
                    ) =>
                {
                    if let Some(status) = self.child.try_wait()? {
                        return Err(io::Error::other(format!(
                            "BlueJS page-host child exited before accepting its connection: {status}"
                        )));
                    }
                    if Instant::now() >= deadline {
                        return Err(io::Error::new(
                            io::ErrorKind::TimedOut,
                            "BlueJS page-host child never accepted its private connection",
                        ));
                    }
                    thread::sleep(Duration::from_millis(20));
                }
                Err(error) => return Err(error),
            }
        };
        let hello = PageHostRequest::Hello {
            protocol_version: page_host::PAGE_HOST_PROTOCOL_VERSION,
            session_token: self.session_token.clone(),
        };
        let handshake = (|| -> io::Result<PageHostReply> {
            page_host::write_page_host_request(&mut stream, &hello)?;
            page_host::read_page_host_reply(&mut stream)
        })();
        match handshake {
            Ok(PageHostReply::HelloAck {
                protocol_version: page_host::PAGE_HOST_PROTOCOL_VERSION,
            }) => {
                self.stream = Some(stream);
                Ok(())
            }
            Ok(reply) => Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("BlueJS page host rejected launcher handshake: {reply:?}"),
            )),
            Err(error) => Err(error),
        }
    }

    /// Sends one post-handshake request across the private child connection.
    /// This intentionally exposes only typed, source-free protocol values,
    /// never the child VM or its program registry.
    pub fn request(&mut self, request: PageHostRequest) -> io::Result<PageHostReply> {
        if matches!(request, PageHostRequest::Hello { .. }) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "BlueJS page-host handshake is already complete",
            ));
        }
        let stream = self.stream.as_mut().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::PermissionDenied,
                "BlueJS page-host connection is delegated to the trusted core",
            )
        })?;
        page_host::write_page_host_request(stream, &request)?;
        page_host::read_page_host_reply(stream)
    }

    /// Applies one caller-authorized document to the isolated child.
    pub fn synchronize_document(
        &mut self,
        document: PageHostDocument,
    ) -> io::Result<PageHostReply> {
        self.request(PageHostRequest::SynchronizeDocument { document })
    }

    /// Requests clean child shutdown, then reaps the child. `Drop` provides a
    /// hard-kill fallback when a process is hung or the transport is broken.
    pub fn shutdown(&mut self) -> io::Result<()> {
        let reply = self.request(PageHostRequest::Shutdown)?;
        if reply != PageHostReply::ShutdownAck {
            return Err(io::Error::other("BlueJS page host rejected shutdown"));
        }
        self.reap_after_shutdown();
        Ok(())
    }

    /// The private path is observable only for lifecycle tests and launcher
    /// cleanup diagnostics; callers must not use it as a second connection.
    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    fn reap_after_shutdown(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(1);
        loop {
            match self.child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
                Ok(None) | Err(_) => {
                    let _ = self.child.kill();
                    let _ = self.child.wait();
                    break;
                }
            }
        }
        let _ = fs::remove_file(&self.socket_path);
    }
}

impl Drop for SpawnedBlueJsHost {
    fn drop(&mut self) {
        self.reap_after_shutdown();
    }
}

fn sibling_bluejs_host_binary(this_exe: &Path) -> PathBuf {
    let name = if cfg!(windows) {
        "blueice-bluejs-host.exe"
    } else {
        "blueice-bluejs-host"
    };
    let directory = this_exe.parent().unwrap_or_else(|| Path::new("."));
    let directory = if directory.file_name().is_some_and(|name| name == "deps") {
        directory.parent().unwrap_or(directory)
    } else {
        directory
    };
    directory.join(name)
}

fn unique_bluejs_host_socket_path() -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let count = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "blueice-launcher-bluejs-host-{}-{count}.sock",
        std::process::id()
    ))
}

fn secure_session_token() -> io::Result<String> {
    use std::io::Read;

    let mut bytes = [0u8; 32];
    fs::File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    let mut token = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write;
        write!(token, "{byte:02x}").expect("writing to a String cannot fail");
    }
    Ok(token)
}

fn wait_for_child_socket(child: &mut Child, path: &Path, timeout: Duration) -> io::Result<()> {
    let deadline = Instant::now() + timeout;
    loop {
        if path.exists() {
            return Ok(());
        }
        if let Some(status) = child.try_wait()? {
            return Err(io::Error::other(format!(
                "BlueJS page-host child exited before binding its socket: {status}"
            )));
        }
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!(
                    "BlueJS page-host child never created its socket at {}",
                    path.display()
                ),
            ));
        }
        thread::sleep(Duration::from_millis(20));
    }
}
