// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

impl SpawnedCore {
    pub fn spawn(width: f64, height: f64, frame_dir: &Path) -> io::Result<Self> {
        Self::spawn_with_options(width, height, frame_dir, CoreLaunchOptions::default())
    }

    /// Exposes only the generation-private script socket pathname for
    /// lifecycle checks. The launcher-owned child capability is never
    /// returned to a frontend or caller through this accessor.
    pub fn script_socket_path(&self) -> Option<&Path> {
        self.script_private_socket_path.as_deref()
    }

    /// Starts a core under launcher-owned operational policy.
    ///
    /// When out-of-process BlueJS is selected, this method is the only
    /// place that creates the endpoint/token pair. It passes that private
    /// capability directly to the newly spawned core, then retains the
    /// child supervisor in this returned owner. Neither the public
    /// frontend broker nor a page receives either value. This v1 launch
    /// seam uses the core's existing private startup arguments; their
    /// values are launcher-generated, are never logged or forwarded over
    /// frontend IPC, and are not an operator-configurable interface. A
    /// descriptor-passing startup channel would be separate Unix process
    /// bootstrap work, not a reason to expose a second runtime protocol.
    pub fn spawn_with_options(
        width: f64,
        height: f64,
        frame_dir: &Path,
        options: CoreLaunchOptions,
    ) -> io::Result<Self> {
        if options.page_http_policy.is_some() && options.core_http_page_script_fixture {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "fixed and owner-selected HTTP page policies are mutually exclusive",
            ));
        }
        // Do this before creating either child. A malformed, partial, or
        // occupied caller-selected endpoint cannot briefly spawn a core
        // or page host that would then need cleanup.
        let route_gate = Arc::new(Mutex::new(()));
        let compiler_mcp_relay = options
            .compiler_mcp_socket
            .as_deref()
            .map(|path| {
                GenerationPinnedUnixRelay::bind(path, "compiler MCP", Arc::clone(&route_gate))
            })
            .transpose()?
            .map(Arc::new);
        let debugger_relay = options
            .debugger_socket
            .as_deref()
            .map(|path| GenerationPinnedUnixRelay::bind(path, "debugger", Arc::clone(&route_gate)))
            .transpose()?
            .map(Arc::new);
        let private_page_host = PrivatePageHostLaunch::spawn(&options)?;
        let core = Self::spawn_with_private_host(
            width,
            height,
            frame_dir,
            options,
            private_page_host,
            RelaySet {
                route_gate,
                compiler_mcp_relay,
                debugger_relay,
            },
        )?;
        core.activate_relays();
        Ok(core)
    }

    /// Stages a core during a broker cutover. The relays are existing
    /// public listeners, not new caller-selected endpoints; their
    /// targets intentionally stay on v1 until replay and health checking
    /// complete in [`cutover`].
    pub(super) fn spawn_with_options_and_relays(
        width: f64,
        height: f64,
        frame_dir: &Path,
        options: CoreLaunchOptions,
        relays: RelaySet,
    ) -> io::Result<Self> {
        let private_page_host = PrivatePageHostLaunch::spawn(&options)?;
        Self::spawn_with_private_host(width, height, frame_dir, options, private_page_host, relays)
    }

    fn spawn_with_private_host(
        width: f64,
        height: f64,
        frame_dir: &Path,
        options: CoreLaunchOptions,
        private_page_host: Option<PrivatePageHostLaunch>,
        relays: RelaySet,
    ) -> io::Result<Self> {
        let (bluejs_host, page_host_config, script_private_socket_path) = match private_page_host {
            Some(private_page_host) => (
                Some(private_page_host.host),
                Some(private_page_host.config),
                Some(private_page_host.script_socket),
            ),
            None => (None, None, None),
        };
        let this_exe = std::env::current_exe()?;
        let core_bin = sibling_core_binary(&this_exe);
        let internal_socket_path = unique_internal_socket_path();
        let _ = std::fs::remove_file(&internal_socket_path);
        if let Some(path) = &script_private_socket_path {
            remove_owned_socket_if_owned(path);
        }
        let compiler_private_socket_path = relays
            .compiler_mcp_relay
            .as_ref()
            .map(|_| unique_internal_compiler_socket_path());
        if let Some(path) = &compiler_private_socket_path {
            let _ = std::fs::remove_file(path);
        }
        let debugger_private_socket_path = relays
            .debugger_relay
            .as_ref()
            .map(|_| unique_internal_debugger_socket_path());
        if let Some(path) = &debugger_private_socket_path {
            let _ = std::fs::remove_file(path);
        }

        let mut command = Command::new(&core_bin);
        command
            .arg("--socket")
            .arg(&internal_socket_path)
            .arg("--width")
            .arg(width.to_string())
            .arg("--height")
            .arg(height.to_string())
            .arg("--frame-dir")
            .arg(frame_dir);
        if let Some(gatekeeper_socket) = &options.gatekeeper_socket {
            command.arg("--gatekeeper-socket").arg(gatekeeper_socket);
        }
        if let Some(config) = &page_host_config {
            command
                .arg("--out-of-process-bluejs-socket")
                .arg(config.socket_path())
                .arg("--out-of-process-bluejs-token")
                .arg(config.session_token());
            if let Some(script_socket) = &script_private_socket_path {
                command
                    .arg("--script-socket")
                    .arg(script_socket)
                    .arg("--script-session-token")
                    .arg(config.session_token());
            }
        }
        if options.core_http_page_script_fixture {
            command
                .arg("--out-of-process-bluejs-page-script-profile")
                .arg("core-page-http-fixture-v1");
        }
        if let Some(compiler_socket) = &compiler_private_socket_path {
            command.arg("--compiler-socket").arg(compiler_socket);
            if options.compiler_catalog.is_some() && options.page_http_policy.is_none() {
                command.arg("--compiler-catalog-stdin");
            } else if options.compiler_catalog.is_none() {
                command
                    .arg("--compiler-project-profile")
                    .arg(CORE_CLOSED_COMPILER_PROJECT_PROFILE);
            }
        }
        if options.page_http_policy.is_some() {
            command.arg("--owner-bootstrap-stdin");
        }
        if options.page_http_policy.is_some() || options.compiler_catalog.is_some() {
            command.stdin(Stdio::piped());
        }
        if let Some(debugger_socket) = &debugger_private_socket_path {
            command.arg("--debugger-socket").arg(debugger_socket);
            if options.debugger_bounded_values {
                command.arg("--debugger-bounded-values");
            }
            if options.debugger_static_metadata_inventory {
                command.arg("--debugger-static-metadata-inventory");
            }
            if options.debugger_static_metadata_summary {
                command.arg("--debugger-static-metadata-summary");
            }
            if options.debugger_static_metadata_source_inventory {
                command.arg("--debugger-static-metadata-source-inventory");
            }
            if options.debugger_static_metadata_source_provenance {
                command.arg("--debugger-static-metadata-source-provenance");
            }
            if options.debugger_static_metadata_type_inventory {
                command.arg("--debugger-static-metadata-type-inventory");
            }
            if options.debugger_static_metadata_type_display {
                command.arg("--debugger-static-metadata-type-display");
            }
            if options.debugger_static_metadata_symbol_inventory {
                command.arg("--debugger-static-metadata-symbol-inventory");
            }
            if options.debugger_static_metadata_contract_inventory {
                command.arg("--debugger-static-metadata-contract-inventory");
            }
            if options.debugger_static_metadata_contract_display {
                command.arg("--debugger-static-metadata-contract-display");
            }
            if options.debugger_static_metadata_contract_validation {
                command.arg("--debugger-static-metadata-contract-validation");
            }
            if options.debugger_static_metadata_lowering_summary {
                command.arg("--debugger-static-metadata-lowering-summary");
            }
            if options.debugger_static_metadata_symbol_display {
                command.arg("--debugger-static-metadata-symbol-display");
            }
            if options.debugger_static_metadata_symbol_location {
                command.arg("--debugger-static-metadata-symbol-location");
            }
            if options.debugger_static_metadata_safe_point_span {
                command.arg("--debugger-static-metadata-safe-point-span");
            }
            if options.debugger_static_metadata_source_breakpoint {
                command.arg("--debugger-static-metadata-source-breakpoint");
            }
            if options.debugger_static_metadata_source_span_step {
                command.arg("--debugger-static-metadata-source-span-step");
            }
            if options.debugger_static_metadata_contract_location {
                command.arg("--debugger-static-metadata-contract-location");
            }
            if options.debugger_static_metadata_symbol_type {
                command.arg("--debugger-static-metadata-symbol-type");
            }
            if options.debugger_static_metadata_symbol_contract {
                command.arg("--debugger-static-metadata-symbol-contract");
            }
            if options.debugger_static_scope_relation {
                command.arg("--debugger-static-scope-relation");
            }
        }
        let mut child = command.spawn()?;
        if options.page_http_policy.is_some() || options.compiler_catalog.is_some() {
            let send_result = child
                .stdin
                .take()
                .ok_or_else(|| io::Error::other("core catalog bootstrap pipe is missing"))
                .and_then(|mut pipe| {
                    if let Some(page_http_policy) = options.page_http_policy.as_ref() {
                        write_core_owner_bootstrap(
                            &mut pipe,
                            &CoreOwnerBootstrap {
                                version: CORE_OWNER_BOOTSTRAP_VERSION,
                                compiler_catalog: options.compiler_catalog.clone(),
                                page_http_policy: Some(page_http_policy.clone()),
                            },
                        )
                    } else {
                        write_compiler_catalog(
                            &mut pipe,
                            options
                                .compiler_catalog
                                .as_ref()
                                .expect("compiler-only pipe requires a catalog"),
                        )
                    }
                });
            if let Err(error) = send_result {
                let _ = child.kill();
                let _ = child.wait();
                let _ = std::fs::remove_file(&internal_socket_path);
                if let Some(script_socket) = &script_private_socket_path {
                    remove_owned_socket_if_owned(script_socket);
                }
                if let Some(compiler_socket) = &compiler_private_socket_path {
                    remove_compiler_mcp_socket_if_owned(compiler_socket);
                }
                if let Some(debugger_socket) = &debugger_private_socket_path {
                    remove_owned_socket_if_owned(debugger_socket);
                }
                return Err(error);
            }
        }

        if !wait_for_socket(&internal_socket_path, Duration::from_secs(5)) {
            let _ = child.kill();
            let _ = child.wait();
            let _ = std::fs::remove_file(&internal_socket_path);
            if let Some(script_socket) = &script_private_socket_path {
                remove_owned_socket_if_owned(script_socket);
            }
            if let Some(compiler_socket) = &compiler_private_socket_path {
                remove_compiler_mcp_socket_if_owned(compiler_socket);
            }
            if let Some(debugger_socket) = &debugger_private_socket_path {
                remove_owned_socket_if_owned(debugger_socket);
            }
            return Err(io::Error::other(format!(
                "blueice-core never created its socket at {}",
                internal_socket_path.display()
            )));
        }
        if let Some(debugger_socket) = &debugger_private_socket_path {
            if !wait_for_socket(debugger_socket, Duration::from_secs(5)) {
                let _ = child.kill();
                let _ = child.wait();
                let _ = std::fs::remove_file(&internal_socket_path);
                if let Some(script_socket) = &script_private_socket_path {
                    remove_owned_socket_if_owned(script_socket);
                }
                if let Some(compiler_socket) = &compiler_private_socket_path {
                    remove_compiler_mcp_socket_if_owned(compiler_socket);
                }
                remove_owned_socket_if_owned(debugger_socket);
                return Err(io::Error::other(format!(
                    "blueice-core never created its debugger socket at {}",
                    debugger_socket.display()
                )));
            }
        }
        let mut stream = match UnixStream::connect(&internal_socket_path) {
            Ok(stream) => stream,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = std::fs::remove_file(&internal_socket_path);
                if let Some(script_socket) = &script_private_socket_path {
                    remove_owned_socket_if_owned(script_socket);
                }
                if let Some(compiler_socket) = &compiler_private_socket_path {
                    remove_compiler_mcp_socket_if_owned(compiler_socket);
                }
                if let Some(debugger_socket) = &debugger_private_socket_path {
                    remove_owned_socket_if_owned(debugger_socket);
                }
                return Err(error);
            }
        };
        // `core` requires the very first message on a fresh connection
        // to be `Hello` (`phase-1-ai-representation-layer/PLAN.md` §3);
        // this launcher is the connection's one and only direct client,
        // so it satisfies that gate itself, once, here -- external
        // clients connecting through the rendezvous socket send their
        // own `Hello` too, but by the time the broker forwards it into
        // this already-past-its-handshake connection, `core` just
        // answers it again rather than re-gating (see `blueice_engine::
        // session::run_session`'s own docs).
        if let Err(error) = blueice_ipc::client_handshake(&mut stream) {
            let _ = child.kill();
            let _ = child.wait();
            let _ = std::fs::remove_file(&internal_socket_path);
            if let Some(script_socket) = &script_private_socket_path {
                remove_owned_socket_if_owned(script_socket);
            }
            if let Some(compiler_socket) = &compiler_private_socket_path {
                remove_compiler_mcp_socket_if_owned(compiler_socket);
            }
            if let Some(debugger_socket) = &debugger_private_socket_path {
                remove_owned_socket_if_owned(debugger_socket);
            }
            return Err(error);
        }
        Ok(SpawnedCore {
            child,
            internal_socket_path,
            script_private_socket_path,
            compiler_private_socket_path,
            debugger_private_socket_path,
            frame_dir: frame_dir.to_path_buf(),
            options,
            bluejs_host,
            route_gate: relays.route_gate,
            compiler_mcp_relay: relays.compiler_mcp_relay,
            debugger_relay: relays.debugger_relay,
            stream,
        })
    }

    pub(super) fn activate_relays(&self) {
        if let (Some(relay), Some(socket)) = (
            self.compiler_mcp_relay.as_ref(),
            self.compiler_private_socket_path.as_ref(),
        ) {
            relay.activate_generation(socket);
        }
        if let (Some(relay), Some(socket)) = (
            self.debugger_relay.as_ref(),
            self.debugger_private_socket_path.as_ref(),
        ) {
            relay.activate_generation(socket);
        }
    }

    /// Activates this staged core while the shared `route_gate` is held
    /// by [`perform_swap`].
    pub(super) fn activate_relays_after_handoff(&self) {
        if let (Some(relay), Some(socket)) = (
            self.compiler_mcp_relay.as_ref(),
            self.compiler_private_socket_path.as_ref(),
        ) {
            relay.activate_generation_after_handoff(socket);
        }
        if let (Some(relay), Some(socket)) = (
            self.debugger_relay.as_ref(),
            self.debugger_private_socket_path.as_ref(),
        ) {
            relay.activate_generation_after_handoff(socket);
        }
    }
}

impl Drop for SpawnedCore {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        // A delegated child may still be blocked on the core-owned
        // protocol connection. Reap it only after core is gone, rather
        // than allowing the child's private capability to outlive its
        // intended core generation.
        drop(self.bluejs_host.take());
        let _ = std::fs::remove_file(&self.internal_socket_path);
        if let Some(script_socket) = &self.script_private_socket_path {
            remove_owned_socket_if_owned(script_socket);
        }
        if let Some(compiler_socket) = &self.compiler_private_socket_path {
            remove_compiler_mcp_socket_if_owned(compiler_socket);
        }
        if let Some(debugger_socket) = &self.debugger_private_socket_path {
            remove_owned_socket_if_owned(debugger_socket);
        }
        // `child.kill()` sends SIGKILL, which never lets `blueice-core`
        // run its own graceful-exit cleanup (which would otherwise
        // remove this itself) -- matters specifically for a cutover's
        // forcefully-superseded v1, which never gets the chance to exit
        // on its own.
        let _ = std::fs::remove_dir_all(&self.frame_dir);
    }
}
