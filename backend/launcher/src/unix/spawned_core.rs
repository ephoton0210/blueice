// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

impl SpawnedCore {
    /// Spawns a standalone core using the conventional gatekeeper
    /// socket. Launcher-managed cores should use
    /// [`Self::spawn_with_gatekeeper`] with their private supervised
    /// gatekeeper instead.
    pub fn spawn(width: f64, height: f64, frame_dir: &Path) -> io::Result<Self> {
        Self::spawn_with_options(width, height, frame_dir, CoreLaunchOptions::default())
    }

    /// Spawns a core wired to `gatekeeper_socket`, which must be a
    /// live, launcher-supervised mandatory checkpoint for production
    /// launcher use.
    pub fn spawn_with_gatekeeper(
        width: f64,
        height: f64,
        frame_dir: &Path,
        gatekeeper_socket: &Path,
    ) -> io::Result<Self> {
        Self::spawn_with_gatekeeper_and_extension(width, height, frame_dir, gatekeeper_socket, None)
    }

    /// Starts an installed package through core's authenticated extension
    /// host when a manifest is supplied. Each cutover gets a fresh private
    /// extension socket and core-generated child credential. Optional grants
    /// are not copied to v2; a new core starts from manifest-declared grants.
    pub fn spawn_with_gatekeeper_and_extension(
        width: f64,
        height: f64,
        frame_dir: &Path,
        gatekeeper_socket: &Path,
        extension_manifest: Option<&Path>,
    ) -> io::Result<Self> {
        Self::spawn_with_assistant(
            width,
            height,
            frame_dir,
            gatekeeper_socket,
            extension_manifest,
            None,
        )
    }

    /// [`Self::spawn_with_gatekeeper_and_extension`] plus the launcher's public
    /// assistant socket, which core is given as `--assistant-socket` so live
    /// translation and assistant tasks reach the supervised assistant.
    pub fn spawn_with_assistant(
        width: f64,
        height: f64,
        frame_dir: &Path,
        gatekeeper_socket: &Path,
        extension_manifest: Option<&Path>,
        assistant: Option<&AssistantWiring>,
    ) -> io::Result<Self> {
        let mut options =
            CoreLaunchOptions::default().with_gatekeeper_socket(gatekeeper_socket.to_path_buf());
        options.extension_manifest = extension_manifest.map(Path::to_path_buf);
        options.assistant = assistant.cloned();
        Self::spawn_with_options(width, height, frame_dir, options)
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
        if options.extension_manifest.is_some()
            && (options.page_http_policy.is_some() || options.compiler_catalog.is_some())
        {
            // Both features need the core's stdin pipe (owner bootstrap versus
            // the private permission-control channel).
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "an installed extension cannot be combined with an owner bootstrap pipe",
            ));
        }
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
        let extension_socket_path = options
            .extension_manifest
            .as_ref()
            .map(|_| unique_internal_extension_socket_path());
        if let Some(path) = extension_socket_path.as_deref() {
            let _ = std::fs::remove_file(path);
        }
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
        if let Some(assistant) = &options.assistant {
            command.arg("--assistant-socket").arg(&assistant.socket);
            if let Some(settings) = &assistant.settings_file {
                command.arg("--assistant-settings").arg(settings);
            }
        }
        if let (Some(manifest), Some(socket)) = (
            options.extension_manifest.as_deref(),
            extension_socket_path.as_deref(),
        ) {
            command
                .arg("--extension-socket")
                .arg(socket)
                .arg("--extension-manifest")
                .arg(manifest)
                .arg("--extension-host")
                .arg(sibling_extension_host_binary(&this_exe))
                .arg("--permission-control-stdio")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped());
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
                if let Some(path) = extension_socket_path.as_deref() {
                    let _ = std::fs::remove_file(path);
                }
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

        // Core itself gives its authenticated extension host five seconds to
        // connect, then stops that child on failure. Leave a little margin
        // before launcher's fallback kill so we do not interrupt that cleanup.
        let startup_timeout = if options.extension_manifest.is_some() {
            Duration::from_secs(7)
        } else {
            Duration::from_secs(5)
        };
        if !wait_for_socket_or_exit(&internal_socket_path, &mut child, startup_timeout) {
            let _ = child.kill();
            let _ = child.wait();
            let _ = std::fs::remove_file(&internal_socket_path);
            if let Some(path) = extension_socket_path.as_deref() {
                let _ = std::fs::remove_file(path);
            }
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
                if let Some(path) = extension_socket_path.as_deref() {
                    let _ = std::fs::remove_file(path);
                }
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
        let mut stream = match connect_when_listening(&internal_socket_path, Duration::from_secs(5))
        {
            Ok(stream) => stream,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = std::fs::remove_file(&internal_socket_path);
                if let Some(path) = extension_socket_path.as_deref() {
                    let _ = std::fs::remove_file(path);
                }
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
            if let Some(path) = extension_socket_path.as_deref() {
                let _ = std::fs::remove_file(path);
            }
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
        let mut core = SpawnedCore {
            child,
            internal_socket_path,
            extension_socket_path,
            permission_control: None,
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
        };
        // A responsive, authenticated host is not enough: prove that this
        // generation's private parent pipe actually reaches its registry
        // before admitting it as v1 or cutting over to it as v2.
        if core.options.extension_manifest.is_some() {
            let input = core.child.stdin.take().ok_or_else(|| {
                io::Error::other("installed core has no private permission input pipe")
            })?;
            let output = core.child.stdout.take().ok_or_else(|| {
                io::Error::other("installed core has no private permission output pipe")
            })?;
            core.permission_control = Some(PermissionControlChannel::new(input, output)?);
            core.inspect_installed_extension()?.ok_or_else(|| {
                io::Error::other("installed core did not report extension permissions")
            })?;
        }
        Ok(core)
    }

    pub(super) fn inspect_installed_extension(
        &self,
    ) -> io::Result<Option<control::InstalledExtensionPermissions>> {
        let Some(channel) = self.permission_control.as_ref() else {
            return Ok(None);
        };
        match channel.inspect()? {
            PermissionControlReply::State {
                extension_id,
                name,
                version,
                optional,
                runtime_ephemeral,
            } => Ok(Some(control::InstalledExtensionPermissions {
                extension_id,
                name,
                version,
                optional,
                runtime_ephemeral,
            })),
            _ => Err(io::Error::other(
                "core returned a non-state permission inspection reply",
            )),
        }
    }

    pub(super) fn inspect_document(&self, tab_id: u64) -> io::Result<(u64, Option<String>)> {
        let channel = self.permission_control.as_ref().ok_or_else(|| {
            io::Error::other("the active core has no installed permission channel")
        })?;
        match channel.exchange(
            PermissionControlRequest::InspectDocument { tab_id },
            PERMISSION_INSPECT_TIMEOUT,
        )? {
            PermissionControlReply::Document {
                tab_id: observed,
                document_epoch,
                url,
            } if observed == tab_id => Ok((document_epoch, url)),
            PermissionControlReply::Rejected { reason } => Err(io::Error::other(reason)),
            _ => Err(io::Error::other(
                "core returned an unexpected document inspection reply",
            )),
        }
    }

    /// A core rejection (for example, a navigation after native review) is
    /// expected and leaves the core serving. Only an uncertain transport or
    /// malformed success kills the generation, preventing a late lease from
    /// outliving its trusted window.
    pub(super) fn arm_ephemeral(
        &mut self,
        capability: &str,
        tab_id: u64,
        document_epoch: u64,
    ) -> io::Result<Result<(), String>> {
        let outcome = (|| {
            let channel = self.permission_control.as_ref().ok_or_else(|| {
                io::Error::other("the active core has no installed permission channel")
            })?;
            match channel.exchange(
                PermissionControlRequest::ArmEphemeral {
                    capability: capability.to_string(),
                    tab_id,
                    document_epoch,
                },
                PERMISSION_CHANGE_TIMEOUT,
            )? {
                PermissionControlReply::EphemeralArmed {
                    capability: armed,
                    tab_id: armed_tab,
                    document_epoch: armed_epoch,
                    ticket,
                } if armed == capability
                    && armed_tab == tab_id
                    && armed_epoch == document_epoch
                    && ticket.len() == 64
                    && ticket.bytes().all(|byte| byte.is_ascii_hexdigit()) =>
                {
                    Ok(Ok(()))
                }
                PermissionControlReply::Rejected { reason } => Ok(Err(reason)),
                _ => Err(io::Error::other(
                    "core returned an inconsistent one-shot permission reply",
                )),
            }
        })();
        if outcome.is_err() {
            let _ = self.child.kill();
        }
        outcome
    }

    /// The caller must hold the broker's cutover gate and active-core lock
    /// after validating this package/capability against a native decision.
    /// An uncertain mutating reply cannot leave a possibly granted core
    /// serving: terminate it, which also withdraws its process-local grants.
    pub(super) fn apply_optional_change(
        &mut self,
        action: trusted_window::PermissionAction,
        capability: &str,
    ) -> io::Result<control::InstalledExtensionPermissions> {
        let outcome = (|| {
            let channel = self.permission_control.as_ref().ok_or_else(|| {
                io::Error::other("the active core has no installed permission channel")
            })?;
            let (request, expected_granted) = match action {
                trusted_window::PermissionAction::Grant => (
                    PermissionControlRequest::Grant {
                        capability: capability.to_string(),
                    },
                    true,
                ),
                trusted_window::PermissionAction::Revoke => (
                    PermissionControlRequest::Revoke {
                        capability: capability.to_string(),
                    },
                    false,
                ),
            };
            match channel.exchange(request, PERMISSION_CHANGE_TIMEOUT)? {
                PermissionControlReply::Updated {
                    capability: updated,
                    granted,
                    ..
                } if updated == capability && granted == expected_granted => {}
                PermissionControlReply::Rejected { reason } => {
                    return Err(io::Error::other(reason))
                }
                _ => {
                    return Err(io::Error::other(
                        "core returned an unexpected permission change reply",
                    ))
                }
            }
            let installed = self.inspect_installed_extension()?.ok_or_else(|| {
                io::Error::other("the installed extension disappeared after a permission change")
            })?;
            if !installed
                .optional
                .iter()
                .any(|entry| entry.capability == capability && entry.granted == expected_granted)
            {
                return Err(io::Error::other(
                    "core permission state did not confirm the requested change",
                ));
            }
            Ok(installed)
        })();
        if outcome.is_err() {
            let _ = self.child.kill();
        }
        outcome
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
        // Discard this generation's private authority before disconnecting
        // it. The worker closes core's stdin on EOF; core then withdraws
        // optional grants even if the normal client-shutdown path stalls.
        self.permission_control.take();
        // Let core observe its private client disconnect and reap its own
        // extension-host child before the force-kill fallback. SIGKILL first
        // would orphan that child during every successful cutover.
        let _ = self.stream.shutdown(Shutdown::Both);
        let deadline = Instant::now() + Duration::from_millis(750);
        loop {
            match self.child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
                _ => {
                    let _ = self.child.kill();
                    let _ = self.child.wait();
                    break;
                }
            }
        }
        // A delegated child may still be blocked on the core-owned
        // protocol connection. Reap it only after core is gone, rather
        // than allowing the child's private capability to outlive its
        // intended core generation.
        drop(self.bluejs_host.take());
        let _ = std::fs::remove_file(&self.internal_socket_path);
        if let Some(path) = self.extension_socket_path.as_deref() {
            let _ = std::fs::remove_file(path);
        }
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
