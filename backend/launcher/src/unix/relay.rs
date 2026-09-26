// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

impl GenerationPinnedUnixRelay {
    pub(super) fn bind(
        path: &Path,
        endpoint_label: &'static str,
        route_gate: Arc<Mutex<()>>,
    ) -> io::Result<Self> {
        prepare_stable_endpoint(path, endpoint_label)?;
        let listener = UnixListener::bind(path)?;
        // Do not depend on the process umask for a public capability
        // boundary.  This also keeps the public stable endpoint at the
        // same owner-only mode as the former core-owned listener.
        if let Err(error) = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)) {
            remove_owned_socket_if_owned(path);
            return Err(error);
        }
        if let Err(error) = listener.set_nonblocking(true) {
            remove_owned_socket_if_owned(path);
            return Err(error);
        }

        let target = Arc::new(Mutex::new(None));
        let accepting = Arc::new(AtomicBool::new(true));
        let thread_target = Arc::clone(&target);
        let thread_route_gate = Arc::clone(&route_gate);
        let thread_accepting = Arc::clone(&accepting);
        let accept_thread = thread::spawn(move || {
            while thread_accepting.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((client, _)) => {
                        // Accepted descriptors inherit the listener's
                        // nonblocking flag on some Unix platforms.  The
                        // forwarding pair intentionally does blocking
                        // byte copies, so restore the per-connection
                        // default before it can mistake EAGAIN for EOF.
                        if client.set_nonblocking(false).is_err() {
                            let _ = client.shutdown(Shutdown::Both);
                            continue;
                        }
                        // Snapshot the route while accepting.  This
                        // gate is shared with the browser-writer handoff,
                        // so a newly paired MCP adapter cannot observe
                        // different browser/compiler generations.  The
                        // forwarding pair below owns that concrete
                        // private stream and never consults `target`
                        // again, so an old cursor/request stream cannot
                        // cross a catalog-generation cutover.
                        let _route_handoff = thread_route_gate
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner());
                        let target = thread_target
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner())
                            .clone();
                        if let Some(target) = target {
                            thread::spawn(move || {
                                relay_generation_pinned_connection(client, target)
                            });
                        } else {
                            // A core is staged but not committed, or the
                            // launcher is stopping.  There is no safe
                            // fallback generation, so fail closed.
                            let _ = client.shutdown(Shutdown::Both);
                        }
                    }
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                    Err(_) => break,
                }
            }
        });

        Ok(Self {
            public_socket_path: path.to_path_buf(),
            route_gate,
            target,
            accepting,
            accept_thread: Mutex::new(Some(accept_thread)),
        })
    }

    /// Makes subsequently accepted public connections target `socket`.
    /// Existing connections keep their already-open private stream.  The
    /// ordinary initial activation has no concurrent browser handoff.
    pub(super) fn activate_generation(&self, socket: &Path) {
        let _route_handoff = self
            .route_gate
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        self.activate_generation_after_handoff(socket);
    }

    /// The caller holds [`Self::route_gate`] alongside the browser
    /// writer swap.  Kept separate so cutover never recursively locks
    /// the handoff mutex.
    pub(super) fn activate_generation_after_handoff(&self, socket: &Path) {
        *self
            .target
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(socket.to_path_buf());
    }

    pub(super) fn close(&self) {
        if !self.accepting.swap(false, Ordering::AcqRel) {
            return;
        }
        *self
            .target
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
        if let Some(thread) = self
            .accept_thread
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
        {
            let _ = thread.join();
        }
        remove_owned_socket_if_owned(&self.public_socket_path);
    }
}

impl Drop for GenerationPinnedUnixRelay {
    fn drop(&mut self) {
        self.close();
    }
}
