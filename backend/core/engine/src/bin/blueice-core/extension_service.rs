// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[cfg(unix)]
#[derive(Clone)]
pub(super) struct PermissionControlMetadata {
    pub(super) extension_id: String,
    pub(super) name: String,
    pub(super) version: String,
    pub(super) optional: Vec<OptionalCapabilityInfo>,
    pub(super) ephemeral: Vec<EphemeralCapabilityInfo>,
}

#[cfg(unix)]
pub(super) struct ExtensionService {
    pub(super) socket: PathBuf,
    pub(super) listener: UnixListener,
    pub(super) registry: Arc<ExtensionRegistry>,
    pub(super) extension_id: String,
    pub(super) storage: ExtensionStorage,
    pub(super) required_authentication: Option<String>,
    pub(super) runtime_start: Option<Arc<Mutex<mpsc::Receiver<()>>>>,
    pub(super) runtime_events: Option<Arc<Mutex<mpsc::Receiver<ExtensionRuntimeEvent>>>>,
}

/// A core-owned response must be prompt enough not to hold an extension
/// connection forever if the frontend session has already ended, while still
/// comfortably exceeding the session loop's 25ms poll interval.
#[cfg(unix)]
pub(super) const EXTENSION_CORE_REQUEST_TIMEOUT: Duration = Duration::from_secs(1);
/// How long a navigation waits for the assistant's translation before it
/// shows the original page instead.
#[cfg(unix)]
pub(super) const DEFAULT_TRANSLATE_DEADLINE_MS: u64 = 8_000;

/// An extension cannot select or reuse this identifier. It connects a private
/// socket's short-lived declarative network rules to precisely that socket's
/// cleanup path, independent of the package's public extension identity.
#[cfg(unix)]
static NEXT_EXTENSION_CONNECTION_ID: AtomicU64 = AtomicU64::new(1);
#[cfg(unix)]
static NEXT_EXTENSION_POPUP_ID: AtomicU64 = AtomicU64::new(1);

/// Core gives each host child a fresh 256-bit credential. This binary is Unix
/// only (it already uses Unix-domain sockets), so the kernel CSPRNG is the
/// appropriate local source and avoids persisting a credential in either the
/// package manifest or a temporary file.
#[cfg(unix)]
pub(super) fn new_extension_authentication() -> Result<String, String> {
    let mut random = [0_u8; 32];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut source| source.read_exact(&mut random))
        .map_err(|error| {
            format!("could not obtain extension-host authentication entropy: {error}")
        })?;
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(random.len() * 2);
    for byte in random {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    Ok(encoded)
}

/// Starts the BlueIce-owned host with its connection credential in the child
/// environment only. In particular, the secret is never placed on the command
/// line, in the manifest, or in a listener response.
#[cfg(unix)]
pub(super) fn spawn_extension_host(
    executable: &Path,
    socket: &Path,
    manifest: &Path,
    authentication: &str,
) -> Result<Child, String> {
    Command::new(executable)
        .arg("--connect")
        .arg(socket)
        .arg("--manifest")
        .arg(manifest)
        .env("BLUEICE_EXTENSION_AUTH_TOKEN", authentication)
        // The parent's stdio may be the private permission-control pipe.
        // Never let an extension child read commands or forge replies.
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .spawn()
        .map_err(|error| {
            format!(
                "could not start extension host {}: {error}",
                executable.display()
            )
        })
}

#[cfg(unix)]
pub(super) fn stop_extension_host(mut child: Child) {
    // A normal session close first drops the lifecycle-event sender, letting a
    // host blocked in NextRuntimeEvent receive RuntimeEventStreamClosed and
    // exit on its own. Give that bounded shutdown path a brief chance before
    // falling back to process containment for a misbehaving host.
    for _ in 0..10 {
        match child.try_wait() {
            Ok(Some(_)) | Err(_) => return,
            Ok(None) => thread::sleep(Duration::from_millis(10)),
        }
    }
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(unix)]
pub(super) fn inspect_live_document(
    session_requests: &mpsc::Sender<ExtensionPageRequest>,
    tab_id: u64,
) -> Result<(u64, Option<String>), String> {
    let (reply, result) = mpsc::channel();
    session_requests
        .send(ExtensionPageRequest::InspectDocument { tab_id, reply })
        .map_err(|_| "the core session is unavailable for document inspection".to_string())?;
    result
        .recv_timeout(Duration::from_secs(2))
        .map_err(|_| "the core session did not answer document inspection".to_string())?
}

#[cfg(unix)]
pub(super) fn permission_control_reply(
    request: PermissionControlRequest,
    metadata: &PermissionControlMetadata,
    registry: &ExtensionRegistry,
    session_requests: &mpsc::Sender<ExtensionPageRequest>,
    runtime_events: Option<&mpsc::SyncSender<ExtensionRuntimeEvent>>,
) -> PermissionControlReply {
    match request {
        PermissionControlRequest::Inspect => PermissionControlReply::State {
            extension_id: metadata.extension_id.clone(),
            name: metadata.name.clone(),
            version: metadata.version.clone(),
            optional: metadata
                .optional
                .iter()
                .map(|entry| OptionalCapabilityInfo {
                    capability: entry.capability.clone(),
                    granted: registry.has_capability(&metadata.extension_id, &entry.capability),
                    origins: entry.origins.clone(),
                })
                .collect(),
            runtime_ephemeral: metadata.ephemeral.clone(),
        },
        PermissionControlRequest::InspectDocument { tab_id } => {
            match inspect_live_document(session_requests, tab_id) {
                Ok((document_epoch, url)) => PermissionControlReply::Document {
                    tab_id,
                    document_epoch,
                    url,
                },
                Err(reason) => PermissionControlReply::Rejected { reason },
            }
        }
        PermissionControlRequest::ArmEphemeral {
            capability,
            tab_id,
            document_epoch,
        } => {
            if !metadata
                .ephemeral
                .iter()
                .any(|entry| entry.capability == capability)
            {
                return PermissionControlReply::Rejected {
                    reason: "capability is not an installed runtime-ephemeral declaration".into(),
                };
            }
            let Some(runtime_events) = runtime_events else {
                return PermissionControlReply::Rejected {
                    reason: "the authenticated extension runtime event channel is unavailable"
                        .into(),
                };
            };
            match inspect_live_document(session_requests, tab_id) {
                Ok((current_epoch, _)) if current_epoch == document_epoch => {}
                Ok(_) => {
                    return PermissionControlReply::Rejected {
                        reason: "the document changed before the ephemeral lease was armed".into(),
                    }
                }
                Err(reason) => return PermissionControlReply::Rejected { reason },
            }
            match registry.arm_runtime_ephemeral(
                &metadata.extension_id,
                &capability,
                tab_id,
                document_epoch,
            ) {
                Ok(ticket) => {
                    let event = ExtensionRuntimeEvent::TrustedEphemeralDomRead {
                        tab_id,
                        document_epoch,
                        ticket: ticket.clone(),
                    };
                    if runtime_events.try_send(event).is_err() {
                        let _ =
                            registry.revoke_runtime_ephemeral(&metadata.extension_id, &capability);
                        return PermissionControlReply::Rejected {
                            reason: "the authenticated extension runtime cannot accept a trusted gesture".into(),
                        };
                    }
                    PermissionControlReply::EphemeralArmed {
                        capability,
                        tab_id,
                        document_epoch,
                        ticket,
                    }
                }
                Err(reason) => PermissionControlReply::Rejected { reason },
            }
        }
        PermissionControlRequest::Grant { capability } => {
            if !metadata
                .optional
                .iter()
                .any(|entry| entry.capability == capability)
            {
                return PermissionControlReply::Rejected {
                    reason: "capability is not an installed optional declaration".into(),
                };
            }
            match registry.grant_optional(&metadata.extension_id, &capability) {
                Ok(changed) => PermissionControlReply::Updated {
                    capability,
                    granted: true,
                    changed,
                },
                Err(reason) => PermissionControlReply::Rejected { reason },
            }
        }
        PermissionControlRequest::Revoke { capability } => {
            if !metadata
                .optional
                .iter()
                .any(|entry| entry.capability == capability)
            {
                return PermissionControlReply::Rejected {
                    reason: "capability is not an installed optional declaration".into(),
                };
            }
            match session::revoke_optional_and_wait_for_cleanup(
                registry,
                &metadata.extension_id,
                &capability,
                session_requests,
            ) {
                Ok(changed) => PermissionControlReply::Updated {
                    capability,
                    granted: false,
                    changed,
                },
                Err(reason) => PermissionControlReply::Rejected { reason },
            }
        }
    }
}

/// A private parent pipe, never the public frontend/extension wire. Any EOF,
/// invalid frame, or broken reply pipe withdraws all grants this channel
/// could have made. The session's existing idle poll retires their published
/// effects even if the parent disappeared before receiving an acknowledgement.
#[cfg(unix)]
pub(super) fn serve_permission_control<R: Read, W: Write>(
    mut reader: R,
    mut writer: W,
    metadata: PermissionControlMetadata,
    registry: Arc<ExtensionRegistry>,
    session_requests: mpsc::Sender<ExtensionPageRequest>,
    runtime_events: Option<mpsc::SyncSender<ExtensionRuntimeEvent>>,
) -> io::Result<()> {
    let result = (|| {
        while let Some(request) = read_permission_control_request(&mut reader)? {
            let reply = permission_control_reply(
                request,
                &metadata,
                &registry,
                &session_requests,
                runtime_events.as_ref(),
            );
            write_permission_control_reply(&mut writer, &reply)?;
        }
        Ok(())
    })();
    for entry in &metadata.optional {
        let _ = registry.revoke_optional(&metadata.extension_id, &entry.capability);
    }
    for entry in &metadata.ephemeral {
        let _ = registry.revoke_runtime_ephemeral(&metadata.extension_id, &entry.capability);
    }
    result
}

#[cfg(unix)]
pub(super) fn request_tab_representation(
    tx: &mpsc::Sender<ExtensionPageRequest>,
    tab_id: Option<u64>,
) -> Result<String, String> {
    let (reply_tx, reply_rx) = mpsc::channel();
    tx.send(ExtensionPageRequest::ReadRepresentation {
        tab_id,
        reply: reply_tx,
    })
    .map_err(|_| "blueice-core session is no longer available".to_string())?;
    reply_rx
        .recv_timeout(EXTENSION_CORE_REQUEST_TIMEOUT)
        .map_err(|_| "blueice-core did not answer the extension request in time".to_string())?
}

#[cfg(unix)]
pub(super) fn request_ephemeral_tab_representation(
    tx: &mpsc::Sender<ExtensionPageRequest>,
    tab_id: u64,
    ticket: String,
) -> Result<String, String> {
    let (reply_tx, reply_rx) = mpsc::channel();
    tx.send(ExtensionPageRequest::ReadEphemeralRepresentation {
        tab_id,
        ticket,
        reply: reply_tx,
    })
    .map_err(|_| "blueice-core session is no longer available".to_string())?;
    reply_rx
        .recv_timeout(EXTENSION_CORE_REQUEST_TIMEOUT)
        .map_err(|_| "blueice-core did not answer the ephemeral read in time".to_string())?
}

#[cfg(unix)]
pub(super) fn request_network_response(
    tx: &mpsc::Sender<ExtensionPageRequest>,
    tab_id: u64,
) -> Result<Option<NetworkResponseInfo>, String> {
    let (reply_tx, reply_rx) = mpsc::channel();
    tx.send(ExtensionPageRequest::ReadNetworkResponse {
        tab_id,
        reply: reply_tx,
    })
    .map_err(|_| "blueice-core session is no longer available".to_string())?;
    reply_rx
        .recv_timeout(EXTENSION_CORE_REQUEST_TIMEOUT)
        .map_err(|_| "blueice-core did not answer the extension request in time".to_string())?
}

#[cfg(unix)]
pub(super) fn request_network_trace(
    tx: &mpsc::Sender<ExtensionPageRequest>,
    tab_id: u64,
) -> Result<Option<NetworkTraceInfo>, String> {
    let (reply_tx, reply_rx) = mpsc::channel();
    tx.send(ExtensionPageRequest::ReadNetworkTrace {
        tab_id,
        reply: reply_tx,
    })
    .map_err(|_| "blueice-core session is no longer available".to_string())?;
    reply_rx
        .recv_timeout(EXTENSION_CORE_REQUEST_TIMEOUT)
        .map_err(|_| "blueice-core did not answer the extension request in time".to_string())?
}

#[cfg(unix)]
pub(super) fn request_text_input_value(
    tx: &mpsc::Sender<ExtensionPageRequest>,
    tab_id: u64,
    node_id: u64,
    value: String,
    grant_generation: u64,
) -> Result<(), String> {
    let (reply_tx, reply_rx) = mpsc::channel();
    tx.send(ExtensionPageRequest::SetTextInputValue {
        tab_id,
        node_id,
        value,
        grant_generation,
        reply: reply_tx,
    })
    .map_err(|_| "blueice-core session is no longer available".to_string())?;
    reply_rx
        .recv_timeout(EXTENSION_CORE_REQUEST_TIMEOUT)
        .map_err(|_| "blueice-core did not answer the extension request in time".to_string())?
}

#[cfg(unix)]
pub(super) fn request_checkbox_checked(
    tx: &mpsc::Sender<ExtensionPageRequest>,
    tab_id: u64,
    node_id: u64,
    checked: bool,
    grant_generation: u64,
) -> Result<(), String> {
    let (reply_tx, reply_rx) = mpsc::channel();
    tx.send(ExtensionPageRequest::SetCheckboxChecked {
        tab_id,
        node_id,
        checked,
        grant_generation,
        reply: reply_tx,
    })
    .map_err(|_| "blueice-core session is no longer available".to_string())?;
    reply_rx
        .recv_timeout(EXTENSION_CORE_REQUEST_TIMEOUT)
        .map_err(|_| "blueice-core did not answer the extension request in time".to_string())?
}

#[cfg(unix)]
pub(super) fn request_radio_checked(
    tx: &mpsc::Sender<ExtensionPageRequest>,
    tab_id: u64,
    node_id: u64,
    grant_generation: u64,
) -> Result<(), String> {
    let (reply_tx, reply_rx) = mpsc::channel();
    tx.send(ExtensionPageRequest::SetRadioChecked {
        tab_id,
        node_id,
        grant_generation,
        reply: reply_tx,
    })
    .map_err(|_| "blueice-core session is no longer available".to_string())?;
    reply_rx
        .recv_timeout(EXTENSION_CORE_REQUEST_TIMEOUT)
        .map_err(|_| "blueice-core did not answer the extension request in time".to_string())?
}

#[cfg(unix)]
pub(super) fn request_select_option(
    tx: &mpsc::Sender<ExtensionPageRequest>,
    tab_id: u64,
    node_id: u64,
    grant_generation: u64,
) -> Result<(), String> {
    let (reply_tx, reply_rx) = mpsc::channel();
    tx.send(ExtensionPageRequest::SelectOption {
        tab_id,
        node_id,
        grant_generation,
        reply: reply_tx,
    })
    .map_err(|_| "blueice-core session is no longer available".to_string())?;
    reply_rx
        .recv_timeout(EXTENSION_CORE_REQUEST_TIMEOUT)
        .map_err(|_| "blueice-core did not answer the extension request in time".to_string())?
}

#[cfg(unix)]
pub(super) fn request_textarea_value(
    tx: &mpsc::Sender<ExtensionPageRequest>,
    tab_id: u64,
    node_id: u64,
    value: String,
    grant_generation: u64,
) -> Result<(), String> {
    let (reply_tx, reply_rx) = mpsc::channel();
    tx.send(ExtensionPageRequest::SetTextareaValue {
        tab_id,
        node_id,
        value,
        grant_generation,
        reply: reply_tx,
    })
    .map_err(|_| "blueice-core session is no longer available".to_string())?;
    reply_rx
        .recv_timeout(EXTENSION_CORE_REQUEST_TIMEOUT)
        .map_err(|_| "blueice-core did not answer the extension request in time".to_string())?
}

#[cfg(unix)]
pub(super) fn request_visible_leaf_text(
    tx: &mpsc::Sender<ExtensionPageRequest>,
    tab_id: u64,
    node_id: u64,
    value: String,
    grant_generation: u64,
) -> Result<(), String> {
    let (reply_tx, reply_rx) = mpsc::channel();
    tx.send(ExtensionPageRequest::SetVisibleLeafText {
        tab_id,
        node_id,
        value,
        grant_generation,
        reply: reply_tx,
    })
    .map_err(|_| "blueice-core session is no longer available".to_string())?;
    reply_rx
        .recv_timeout(EXTENSION_CORE_REQUEST_TIMEOUT)
        .map_err(|_| "blueice-core did not answer the extension request in time".to_string())?
}

#[cfg(unix)]
pub(super) fn request_visible_text_content(
    tx: &mpsc::Sender<ExtensionPageRequest>,
    tab_id: u64,
    node_id: u64,
    value: String,
    grant_generation: u64,
) -> Result<(), String> {
    let (reply_tx, reply_rx) = mpsc::channel();
    tx.send(ExtensionPageRequest::SetVisibleTextContent {
        tab_id,
        node_id,
        value,
        grant_generation,
        reply: reply_tx,
    })
    .map_err(|_| "blueice-core session is no longer available".to_string())?;
    reply_rx
        .recv_timeout(EXTENSION_CORE_REQUEST_TIMEOUT)
        .map_err(|_| "blueice-core did not answer the extension request in time".to_string())?
}

#[cfg(unix)]
pub(super) fn request_range_input_value(
    tx: &mpsc::Sender<ExtensionPageRequest>,
    tab_id: u64,
    node_id: u64,
    value: i64,
    grant_generation: u64,
) -> Result<(), String> {
    let (reply_tx, reply_rx) = mpsc::channel();
    tx.send(ExtensionPageRequest::SetRangeInputValue {
        tab_id,
        node_id,
        value,
        grant_generation,
        reply: reply_tx,
    })
    .map_err(|_| "blueice-core session is no longer available".to_string())?;
    reply_rx
        .recv_timeout(EXTENSION_CORE_REQUEST_TIMEOUT)
        .map_err(|_| "blueice-core did not answer the extension request in time".to_string())?
}

#[cfg(unix)]
pub(super) fn request_network_block_url(
    tx: &mpsc::Sender<ExtensionPageRequest>,
    connection_id: u64,
    url: String,
    grant_generation: u64,
) -> Result<(), String> {
    let (reply_tx, reply_rx) = mpsc::channel();
    tx.send(ExtensionPageRequest::RegisterNetworkBlockUrl {
        connection_id,
        grant_generation,
        url,
        reply: reply_tx,
    })
    .map_err(|_| "blueice-core session is no longer available".to_string())?;
    reply_rx
        .recv_timeout(EXTENSION_CORE_REQUEST_TIMEOUT)
        .map_err(|_| "blueice-core did not answer the extension request in time".to_string())?
}

#[cfg(unix)]
pub(super) fn request_network_block_host(
    tx: &mpsc::Sender<ExtensionPageRequest>,
    connection_id: u64,
    host: String,
    grant_generation: u64,
) -> Result<(), String> {
    let (reply_tx, reply_rx) = mpsc::channel();
    tx.send(ExtensionPageRequest::RegisterNetworkBlockHost {
        connection_id,
        grant_generation,
        host,
        reply: reply_tx,
    })
    .map_err(|_| "blueice-core session is no longer available".to_string())?;
    reply_rx
        .recv_timeout(EXTENSION_CORE_REQUEST_TIMEOUT)
        .map_err(|_| "blueice-core did not answer the extension request in time".to_string())?
}

#[cfg(unix)]
pub(super) fn request_network_block_path_prefix(
    tx: &mpsc::Sender<ExtensionPageRequest>,
    connection_id: u64,
    host: String,
    path_prefix: String,
    grant_generation: u64,
) -> Result<(), String> {
    let (reply_tx, reply_rx) = mpsc::channel();
    tx.send(ExtensionPageRequest::RegisterNetworkBlockPathPrefix {
        connection_id,
        grant_generation,
        host,
        path_prefix,
        reply: reply_tx,
    })
    .map_err(|_| "blueice-core session is no longer available".to_string())?;
    reply_rx
        .recv_timeout(EXTENSION_CORE_REQUEST_TIMEOUT)
        .map_err(|_| "blueice-core did not answer the extension request in time".to_string())?
}

#[cfg(unix)]
pub(super) fn request_network_redirect_url(
    tx: &mpsc::Sender<ExtensionPageRequest>,
    connection_id: u64,
    source_url: String,
    target_url: String,
    grant_generation: u64,
) -> Result<(), String> {
    let (reply_tx, reply_rx) = mpsc::channel();
    tx.send(ExtensionPageRequest::RegisterNetworkRedirectUrl {
        connection_id,
        grant_generation,
        source_url,
        target_url,
        reply: reply_tx,
    })
    .map_err(|_| "blueice-core session is no longer available".to_string())?;
    reply_rx
        .recv_timeout(EXTENSION_CORE_REQUEST_TIMEOUT)
        .map_err(|_| "blueice-core did not answer the extension request in time".to_string())?
}

#[cfg(unix)]
pub(super) fn clear_network_block_urls(
    tx: &mpsc::Sender<ExtensionPageRequest>,
    connection_id: u64,
) -> Result<(), String> {
    let (reply_tx, reply_rx) = mpsc::channel();
    tx.send(ExtensionPageRequest::ClearNetworkBlockUrls {
        connection_id,
        reply: reply_tx,
    })
    .map_err(|_| "blueice-core session is no longer available".to_string())?;
    reply_rx
        .recv_timeout(EXTENSION_CORE_REQUEST_TIMEOUT)
        .map_err(|_| "blueice-core did not answer the extension request in time".to_string())
}

#[cfg(unix)]
pub(super) fn request_toolbar_button(
    tx: &mpsc::Sender<ExtensionPageRequest>,
    connection_id: u64,
    label: String,
    grant_generation: u64,
) -> Result<(), String> {
    let (reply_tx, reply_rx) = mpsc::channel();
    tx.send(ExtensionPageRequest::SetToolbarButton {
        connection_id,
        grant_generation,
        label,
        reply: reply_tx,
    })
    .map_err(|_| "blueice-core session is no longer available".to_string())?;
    reply_rx
        .recv_timeout(EXTENSION_CORE_REQUEST_TIMEOUT)
        .map_err(|_| "blueice-core did not answer the extension request in time".to_string())?
}

#[cfg(unix)]
pub(super) fn clear_toolbar_button(
    tx: &mpsc::Sender<ExtensionPageRequest>,
    connection_id: u64,
) -> Result<(), String> {
    let (reply_tx, reply_rx) = mpsc::channel();
    tx.send(ExtensionPageRequest::ClearToolbarButton {
        connection_id,
        reply: reply_tx,
    })
    .map_err(|_| "blueice-core session is no longer available".to_string())?;
    reply_rx
        .recv_timeout(EXTENSION_CORE_REQUEST_TIMEOUT)
        .map_err(|_| "blueice-core did not answer the extension request in time".to_string())
}

#[cfg(unix)]
pub(super) fn request_show_popup(
    tx: &mpsc::Sender<ExtensionPageRequest>,
    connection_id: u64,
    tab_id: u64,
    title: String,
    body: String,
    action_label: Option<String>,
    grant_generation: u64,
) -> Result<(), String> {
    let (reply_tx, reply_rx) = mpsc::channel();
    tx.send(ExtensionPageRequest::ShowPopup {
        connection_id,
        grant_generation,
        popup: blueice_ipc::ExtensionPopup {
            id: NEXT_EXTENSION_POPUP_ID.fetch_add(1, Ordering::Relaxed),
            tab_id,
            title,
            body,
            action_label,
        },
        reply: reply_tx,
    })
    .map_err(|_| "blueice-core session is no longer available".to_string())?;
    reply_rx
        .recv_timeout(EXTENSION_CORE_REQUEST_TIMEOUT)
        .map_err(|_| "blueice-core did not answer the extension request in time".to_string())?
}

#[cfg(unix)]
pub(super) fn clear_popup(
    tx: &mpsc::Sender<ExtensionPageRequest>,
    connection_id: u64,
) -> Result<(), String> {
    let (reply_tx, reply_rx) = mpsc::channel();
    tx.send(ExtensionPageRequest::ClearPopup {
        connection_id,
        reply: reply_tx,
    })
    .map_err(|_| "blueice-core session is no longer available".to_string())?;
    reply_rx
        .recv_timeout(EXTENSION_CORE_REQUEST_TIMEOUT)
        .map_err(|_| "blueice-core did not answer the extension request in time".to_string())
}

/// Serves extension connections outside the session thread, but asks that
/// thread for the one piece of real `Page` data Phase 9 currently supports.
/// This keeps a `Page` single-thread-owned just like navigation and frontend
/// IPC do; no mutable DOM state is shared with an extension handler.
#[cfg(unix)]
pub(super) fn spawn_extension_listener(
    service: ExtensionService,
    gatekeeper_socket: PathBuf,
    request_tx: mpsc::Sender<ExtensionPageRequest>,
    authenticated_ready: Option<mpsc::Sender<()>>,
) {
    thread::spawn(move || {
        for incoming in service.listener.incoming() {
            let Ok(mut stream) = incoming else { break };
            let registry = Arc::clone(&service.registry);
            let storage = service.storage.clone();
            let gatekeeper_socket = gatekeeper_socket.clone();
            let request_tx = request_tx.clone();
            let required_authentication = service.required_authentication.clone();
            let authenticated_ready = authenticated_ready.clone();
            let runtime_start = service.runtime_start.clone();
            let runtime_events = service.runtime_events.clone();
            thread::spawn(move || {
                let connection_id = NEXT_EXTENSION_CONNECTION_ID.fetch_add(1, Ordering::Relaxed);
                let authentication = match required_authentication.as_deref() {
                    Some(expected) => ExtensionConnectionAuthentication::required(expected),
                    None => ExtensionConnectionAuthentication::unauthenticated(),
                };
                let authentication = match authenticated_ready {
                    Some(ready) => authentication.with_ready_notification(ready),
                    None => authentication,
                };
                let authentication = match runtime_start {
                    Some(receiver) => authentication.with_runtime_start_receiver(receiver),
                    None => authentication,
                };
                let authentication = match runtime_events {
                    Some(receiver) => authentication.with_runtime_event_receiver(receiver),
                    None => authentication,
                };
                let read_tx = request_tx.clone();
                let ephemeral_read_tx = request_tx.clone();
                let observe_tx = request_tx.clone();
                let observe_trace_tx = request_tx.clone();
                let write_tx = request_tx.clone();
                let rule_tx = request_tx.clone();
                let host_rule_tx = request_tx.clone();
                let path_rule_tx = request_tx.clone();
                let redirect_rule_tx = request_tx.clone();
                let clear_tx = request_tx.clone();
                let toolbar_tx = request_tx.clone();
                let toolbar_clear_tx = request_tx.clone();
                let popup_tx = request_tx.clone();
                let popup_action_tx = request_tx.clone();
                let popup_clear_tx = request_tx.clone();
                let _ =
                    handle_extension_connection_with_actions_and_authentication_and_network_rules(
                        &registry,
                        &gatekeeper_socket,
                        &mut stream,
                        authentication,
                        ExtensionActionDelegates::new(
                            move |tab_id| request_tab_representation(&read_tx, tab_id),
                            move |target,
                                  value,
                                  write_target: &blueice_ipc::extension::DomWriteTarget,
                                  grant_generation| {
                                match target {
                                Some((tab_id, node_id)) => match write_target {
                                    blueice_ipc::extension::DomWriteTarget::FormInput {
                                        input_type,
                                    } if input_type.eq_ignore_ascii_case("checkbox") => {
                                        request_checkbox_checked(
                                            &write_tx,
                                            tab_id,
                                            node_id,
                                            value == "true",
                                            grant_generation,
                                        )
                                    }
                                    blueice_ipc::extension::DomWriteTarget::FormInput {
                                        input_type,
                                    } if input_type.eq_ignore_ascii_case("radio") => {
                                        request_radio_checked(&write_tx, tab_id, node_id, grant_generation)
                                    }
                                    blueice_ipc::extension::DomWriteTarget::FormInput {
                                        input_type,
                                    } if input_type.eq_ignore_ascii_case("select") => {
                                        request_select_option(&write_tx, tab_id, node_id, grant_generation)
                                    }
                                    blueice_ipc::extension::DomWriteTarget::FormInput {
                                        input_type,
                                    } if input_type.eq_ignore_ascii_case("textarea") => {
                                        request_textarea_value(&write_tx, tab_id, node_id, value, grant_generation)
                                    }
                                    blueice_ipc::extension::DomWriteTarget::FormInput {
                                        input_type,
                                    } if input_type.eq_ignore_ascii_case("range") => {
                                        let value = value.parse::<i64>().map_err(|_| {
                                            "core-backed range input values must be integers"
                                                .to_string()
                                        })?;
                                        request_range_input_value(&write_tx, tab_id, node_id, value, grant_generation)
                                    }
                                    blueice_ipc::extension::DomWriteTarget::VisibleTextLeaf => {
                                        request_visible_leaf_text(&write_tx, tab_id, node_id, value, grant_generation)
                                    }
                                    blueice_ipc::extension::DomWriteTarget::VisibleTextContent => {
                                        request_visible_text_content(&write_tx, tab_id, node_id, value, grant_generation)
                                    }
                                    _ => request_text_input_value(&write_tx, tab_id, node_id, value, grant_generation),
                                },
                                None => Err(
                                    "core-backed legacy dom:write has no stable target node; negotiate dom:write version 2 or 3 and use an explicit control operation"
                                        .to_string(),
                                ),
                            }
                            },
                            || {
                                Err(
                                    "core-backed network:intercept needs a declarative rule format; the current extension wire protocol does not carry one"
                                        .to_string(),
                                )
                            },
                            move |url, grant_generation| request_network_block_url(
                                &rule_tx, connection_id, url, grant_generation,
                            ),
                            move || clear_network_block_urls(&clear_tx, connection_id),
                        )
                        .with_storage(storage)
                        .with_network_observer(move |tab_id| {
                            request_network_response(&observe_tx, tab_id)
                        })
                        .with_ephemeral_dom_reader(move |tab_id, ticket| {
                            request_ephemeral_tab_representation(&ephemeral_read_tx, tab_id, ticket)
                        })
                        .with_network_trace_observer(move |tab_id| {
                            request_network_trace(&observe_trace_tx, tab_id)
                        })
                        .with_network_block_host(move |host, grant_generation| {
                            request_network_block_host(&host_rule_tx, connection_id, host, grant_generation)
                        })
                        .with_network_block_path_prefix(move |host, path_prefix, grant_generation| {
                            request_network_block_path_prefix(
                                &path_rule_tx, connection_id, host, path_prefix, grant_generation,
                            )
                        })
                        .with_network_redirect_url(move |source_url, target_url, grant_generation| {
                            request_network_redirect_url(
                                &redirect_rule_tx, connection_id, source_url, target_url, grant_generation,
                            )
                        })
                        .with_toolbar_button(move |label, grant_generation| {
                            request_toolbar_button(&toolbar_tx, connection_id, label, grant_generation)
                        })
                        .with_toolbar_clearer(move || {
                            clear_toolbar_button(&toolbar_clear_tx, connection_id)
                        })
                        .with_popup(
                            move |tab_id, title, body, grant_generation| {
                                request_show_popup(&popup_tx, connection_id, tab_id, title, body, None, grant_generation)
                            },
                            move || clear_popup(&popup_clear_tx, connection_id),
                        )
                        .with_popup_action(move |tab_id, title, body, action_label, grant_generation| {
                            request_show_popup(
                                &popup_action_tx,
                                connection_id,
                                tab_id,
                                title,
                                body,
                                Some(action_label),
                                grant_generation,
                            )
                        }),
                    );
                let _ = clear_network_block_urls(&request_tx, connection_id);
                let _ = clear_popup(&request_tx, connection_id);
                let _ = clear_toolbar_button(&request_tx, connection_id);
            });
        }
    });
}
