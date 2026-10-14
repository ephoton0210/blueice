// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

/// Serves one extension connection until it disconnects (or sends
/// something this minimal slice can't make sense of -- see below):
/// requires [`ExtensionRequest::Hello`] as the very first message
/// (rejecting/ending the connection otherwise, mirroring how
/// `blueice_engine::session`'s `perform_handshake` rejects a non-`Hello`
/// first message on the external client protocol), replies
/// [`ExtensionReply::HelloAck`] (including any individually unsupported
/// capability versions), then loops handling `DomRead`/
/// `DomWrite`/`NetworkIntercept` requests -- checking `registry` before executing each,
/// replying [`ExtensionReply::CapabilityDenied`] for an unauthorized
/// request rather than a silent no-op or a bare/generic error.
///
/// **A later `Hello`** (once past the initial handshake) is accepted,
/// renegotiates the independently versioned capability set, and answers
/// with another `HelloAck`, updating which `extension_id` subsequent
/// requests on this connection are checked against -- the same
/// "answered again, not re-gating the whole connection" discipline
/// `run_session`'s own docs describe for a repeat `Hello` on the
/// external client protocol, applied here to a long-lived extension
/// connection.
///
/// **Any read failure** (a clean disconnect, or bytes that don't parse
/// as a well-formed [`ExtensionRequest`]) ends the connection by
/// returning `Ok(())`, never propagating a "malformed input" case as a
/// distinguishable error and never panicking -- mirrors `run_session`'s
/// own "any non-timeout read error means disconnect" handling of the
/// external client protocol's main loop. The wire-level parsing
/// functions this calls (`blueice_ipc::extension::read_extension_
/// request`) do still surface a malformed frame as a real `io::Error`
/// to *their own* callers/tests -- this fn just chooses, deliberately,
/// to treat that the same as an ordinary disconnect rather than
/// escalate it, the same choice `run_session` already made for the
/// analogous case on the external protocol.
pub fn handle_extension_connection<S: Read + Write>(
    registry: &ExtensionRegistry,
    stream: &mut S,
) -> io::Result<()> {
    let gatekeeper_socket = default_gatekeeper_socket_path();
    handle_extension_connection_with_gatekeeper(registry, &gatekeeper_socket, stream)
}

/// Like [`handle_extension_connection`], but routes high-risk extension
/// actions through an explicit gatekeeper socket. The launcher and tests
/// pass an isolated path; the public convenience function above retains
/// the conventional standalone-development default.
pub fn handle_extension_connection_with_gatekeeper<S: Read + Write>(
    registry: &ExtensionRegistry,
    gatekeeper_socket: &Path,
    stream: &mut S,
) -> io::Result<()> {
    handle_extension_connection_with_actions(
        registry,
        gatekeeper_socket,
        stream,
        |_| Ok(PLACEHOLDER_DOM_READ_VALUE.to_string()),
        |_, _, _, _| Ok(()),
        || Ok(()),
    )
}

/// Like [`handle_extension_connection_with_gatekeeper`], but delegates a
/// capability-approved operation to the process that owns the actual page
/// state. This preserves the authorization and, where required, gatekeeper
/// checks in this host before any effect is requested. A delegate error is a
/// structured [`ExtensionReply::OperationUnavailable`] response, not a false
/// acknowledgement.
///
/// The standalone host supplies placeholder delegates through
/// [`handle_extension_connection_with_gatekeeper`]. `blueice-core` supplies
/// a synchronous channel-backed read delegate so its session thread remains
/// the sole mutable owner of `TabManager`/`Page` state.
pub fn handle_extension_connection_with_actions<S, R, W, N>(
    registry: &ExtensionRegistry,
    gatekeeper_socket: &Path,
    stream: &mut S,
    read_dom: R,
    write_dom: W,
    register_network_intercept: N,
) -> io::Result<()>
where
    S: Read + Write,
    R: FnMut(Option<u64>) -> Result<String, String>,
    W: FnMut(
        Option<(u64, u64)>,
        String,
        &blueice_ipc::extension::DomWriteTarget,
        u64,
    ) -> Result<(), String>,
    N: FnMut() -> Result<(), String>,
{
    handle_extension_connection_with_actions_and_authentication(
        registry,
        gatekeeper_socket,
        stream,
        ExtensionConnectionAuthentication::unauthenticated(),
        read_dom,
        write_dom,
        register_network_intercept,
    )
}

/// Like [`handle_extension_connection_with_actions`], but applies the supplied
/// [`ExtensionConnectionAuthentication`] before it acknowledges a handshake.
/// `blueice-core` uses a required credential for a host it spawned itself; the
/// standalone server deliberately uses the unauthenticated development mode.
pub fn handle_extension_connection_with_actions_and_authentication<S, R, W, N>(
    registry: &ExtensionRegistry,
    gatekeeper_socket: &Path,
    stream: &mut S,
    authentication: ExtensionConnectionAuthentication<'_>,
    read_dom: R,
    write_dom: W,
    register_network_intercept: N,
) -> io::Result<()>
where
    S: Read + Write,
    R: FnMut(Option<u64>) -> Result<String, String>,
    W: FnMut(
        Option<(u64, u64)>,
        String,
        &blueice_ipc::extension::DomWriteTarget,
        u64,
    ) -> Result<(), String>,
    N: FnMut() -> Result<(), String>,
{
    handle_extension_connection_with_actions_and_authentication_and_network_rules(
        registry,
        gatekeeper_socket,
        stream,
        authentication,
        ExtensionActionDelegates::new(
            read_dom,
            write_dom,
            register_network_intercept,
            |_, _| {
                Err(
                    "network:intercept version 2 needs a core-backed declarative rule handler"
                        .to_string(),
                )
            },
            || {
                Err(
                    "network:intercept version 3 needs a core-backed rule-clear handler"
                        .to_string(),
                )
            },
        ),
    )
}

/// Like [`handle_extension_connection_with_actions_and_authentication`], with
/// separate delegates for the version-2 exact navigation-block registration
/// and version-3 caller-owned rule clear. Keeping both distinct from the
/// legacy v1 acknowledgement preserves its isolated protocol-test behavior
/// while making each core-backed effect explicit.
pub fn handle_extension_connection_with_actions_and_authentication_and_network_rules<
    S,
    R,
    W,
    N,
    B,
    C,
>(
    registry: &ExtensionRegistry,
    gatekeeper_socket: &Path,
    stream: &mut S,
    authentication: ExtensionConnectionAuthentication<'_>,
    mut delegates: ExtensionActionDelegates<R, W, N, B, C>,
) -> io::Result<()>
where
    S: Read + Write,
    R: FnMut(Option<u64>) -> Result<String, String>,
    W: FnMut(
        Option<(u64, u64)>,
        String,
        &blueice_ipc::extension::DomWriteTarget,
        u64,
    ) -> Result<(), String>,
    N: FnMut() -> Result<(), String>,
    B: FnMut(String, u64) -> Result<(), String>,
    C: FnMut() -> Result<(), String>,
{
    let mut identity = match read_extension_request(stream) {
        Ok(request) => match authenticated_hello(authentication.expected(), request) {
            Some((extension_id, capability_versions)) => {
                let (identity, reply) =
                    negotiate_hello(registry, extension_id, capability_versions);
                write_extension_reply(stream, &reply)?;
                identity
            }
            None => return Ok(()), // not an allowed first handshake: reject without an acknowledgement
        },
        Err(_) => return Ok(()), // disconnected, or sent something unparseable, before ever completing the handshake
    };
    authentication.signal_ready();
    let mut runtime_started = false;
    let mut toolbar_visible = false;
    let mut popup_visible = false;

    loop {
        let request = match read_extension_request(stream) {
            Ok(request) => request,
            Err(_) => return Ok(()),
        };
        // Capture the grant that existed when this request arrived. A later
        // revoke/regrant cannot lend its authority to a reviewed write.
        let dom_write_generation =
            registry.capability_generation(&identity.extension_id, CAPABILITY_DOM_WRITE);
        let storage_generation =
            registry.capability_generation(&identity.extension_id, CAPABILITY_STORAGE);
        let dom_read_generation =
            registry.capability_generation(&identity.extension_id, CAPABILITY_DOM_READ);
        let network_observe_generation =
            registry.capability_generation(&identity.extension_id, CAPABILITY_NETWORK_OBSERVE);
        match request {
            ExtensionRequest::Hello {
                extension_id,
                capability_versions,
            } => {
                if authentication.expected().is_some() {
                    return Ok(());
                }
                if popup_visible {
                    if (delegates.clear_popup)().is_err() {
                        return Ok(());
                    }
                    popup_visible = false;
                }
                if toolbar_visible {
                    if (delegates.clear_toolbar_button)().is_err() {
                        return Ok(()); // fail closed before acknowledging a changed grant
                    }
                    toolbar_visible = false;
                }
                let (new_identity, reply) =
                    negotiate_hello(registry, extension_id, capability_versions);
                write_extension_reply(stream, &reply)?;
                identity = new_identity;
            }
            ExtensionRequest::HelloAuthenticated {
                extension_id,
                capability_versions,
                authentication: provided_authentication,
            } => {
                if authentication.expected().is_some_and(|expected| {
                    !constant_time_authentication_matches(expected, &provided_authentication)
                }) {
                    return Ok(());
                }
                if popup_visible {
                    if (delegates.clear_popup)().is_err() {
                        return Ok(());
                    }
                    popup_visible = false;
                }
                if toolbar_visible {
                    if (delegates.clear_toolbar_button)().is_err() {
                        return Ok(());
                    }
                    toolbar_visible = false;
                }
                let (new_identity, reply) =
                    negotiate_hello(registry, extension_id, capability_versions);
                write_extension_reply(stream, &reply)?;
                identity = new_identity;
            }
            ExtensionRequest::RuntimeReady => {
                let result = if runtime_started {
                    Err("the extension runtime has already started on this connection".to_string())
                } else if authentication.expected().is_none() {
                    Err(
                        "RuntimeReady is reserved for a core-spawned authenticated host"
                            .to_string(),
                    )
                } else {
                    authentication.wait_for_runtime_start()
                };
                match result {
                    Ok(()) => {
                        runtime_started = true;
                        write_extension_reply(stream, &ExtensionReply::RuntimeStart)?;
                    }
                    Err(reason) => write_extension_reply(
                        stream,
                        &ExtensionReply::OperationUnavailable {
                            capability: "runtime".to_string(),
                            reason,
                        },
                    )?,
                }
            }
            ExtensionRequest::NextRuntimeEvent => {
                let result = if !runtime_started {
                    Err("the extension runtime has not started on this connection".to_string())
                } else if authentication.expected().is_none() {
                    Err(
                        "NextRuntimeEvent is reserved for a core-spawned authenticated host"
                            .to_string(),
                    )
                } else {
                    loop {
                        match authentication.wait_for_runtime_event() {
                            Ok(Some(
                                ExtensionRuntimeEvent::ToolbarActivated {
                                    grant_generation, ..
                                }
                                | ExtensionRuntimeEvent::PopupActionActivated {
                                    grant_generation,
                                    ..
                                },
                            )) if registry.capability_generation(
                                &identity.extension_id,
                                CAPABILITY_UI_INJECT,
                            ) != Some(grant_generation) =>
                            {
                                continue
                            }
                            result => break result,
                        }
                    }
                };
                match result {
                    Ok(Some(event)) => {
                        write_extension_reply(stream, &ExtensionReply::RuntimeEvent(event))?
                    }
                    Ok(None) => {
                        write_extension_reply(stream, &ExtensionReply::RuntimeEventStreamClosed)?
                    }
                    Err(reason) => write_extension_reply(
                        stream,
                        &ExtensionReply::OperationUnavailable {
                            capability: "runtime".to_string(),
                            reason,
                        },
                    )?,
                }
            }
            ExtensionRequest::DomRead => {
                if let Some(reason) =
                    capability_denial_reason(registry, &identity, CAPABILITY_DOM_READ, 1)
                {
                    write_extension_reply(
                        stream,
                        &ExtensionReply::CapabilityDenied {
                            capability: CAPABILITY_DOM_READ.to_string(),
                            reason,
                        },
                    )?;
                } else {
                    write_stable_read_reply(
                        stream,
                        registry,
                        &identity,
                        CAPABILITY_DOM_READ,
                        dom_read_generation,
                        || match (delegates.read_dom)(None) {
                            Ok(value) => ExtensionReply::DomReadResult { value },
                            Err(reason) => ExtensionReply::OperationUnavailable {
                                capability: CAPABILITY_DOM_READ.to_string(),
                                reason,
                            },
                        },
                    )?;
                }
            }
            ExtensionRequest::DomReadTab { tab_id } => {
                if let Some(reason) =
                    capability_denial_reason(registry, &identity, CAPABILITY_DOM_READ, 2)
                {
                    write_extension_reply(
                        stream,
                        &ExtensionReply::CapabilityDenied {
                            capability: CAPABILITY_DOM_READ.to_string(),
                            reason,
                        },
                    )?;
                } else {
                    write_stable_read_reply(
                        stream,
                        registry,
                        &identity,
                        CAPABILITY_DOM_READ,
                        dom_read_generation,
                        || match (delegates.read_dom)(Some(tab_id)) {
                            Ok(value) => ExtensionReply::DomReadResult { value },
                            Err(reason) => ExtensionReply::OperationUnavailable {
                                capability: CAPABILITY_DOM_READ.to_string(),
                                reason,
                            },
                        },
                    )?;
                }
            }
            ExtensionRequest::DomReadTabEphemeral { tab_id, ticket } => {
                let authorized_mode = authentication.expected().is_some()
                    && registry.has_runtime_ephemeral_declaration(
                        &identity.extension_id,
                        CAPABILITY_DOM_READ,
                    )
                    && identity
                        .negotiated_capabilities
                        .get(CAPABILITY_DOM_READ)
                        .is_some_and(|version| *version >= 3);
                let reply = if !authorized_mode {
                    ExtensionReply::CapabilityDenied {
                        capability: CAPABILITY_DOM_READ.to_string(),
                        reason: "runtime-ephemeral dom:read requires an authenticated installed declaration and API v3".to_string(),
                    }
                } else {
                    match (delegates.read_ephemeral_dom)(tab_id, ticket) {
                        Ok(value) => ExtensionReply::DomReadResult { value },
                        Err(reason) => ExtensionReply::CapabilityDenied {
                            capability: CAPABILITY_DOM_READ.to_string(),
                            reason,
                        },
                    }
                };
                write_extension_reply(stream, &reply)?;
            }
            ExtensionRequest::ReadNetworkResponse { tab_id } => {
                if let Some(reason) =
                    capability_denial_reason(registry, &identity, CAPABILITY_NETWORK_OBSERVE, 1)
                {
                    write_extension_reply(
                        stream,
                        &ExtensionReply::CapabilityDenied {
                            capability: CAPABILITY_NETWORK_OBSERVE.to_string(),
                            reason,
                        },
                    )?;
                } else {
                    write_stable_read_reply(
                        stream,
                        registry,
                        &identity,
                        CAPABILITY_NETWORK_OBSERVE,
                        network_observe_generation,
                        || match (delegates.observe_network)(tab_id) {
                            Ok(response)
                                if response.as_ref().is_some_and(|value| {
                                    serde_json::to_vec(value).is_ok_and(|bytes| {
                                        bytes.len()
                                            > blueice_ipc::extension::MAX_NETWORK_OBSERVATION_BYTES
                                    })
                                }) =>
                            {
                                ExtensionReply::OperationUnavailable {
                                    capability: CAPABILITY_NETWORK_OBSERVE.to_string(),
                                    reason: "network response metadata exceeds the 4096-byte limit"
                                        .to_string(),
                                }
                            }
                            Ok(response) => ExtensionReply::NetworkResponseResult { response },
                            Err(reason) => ExtensionReply::OperationUnavailable {
                                capability: CAPABILITY_NETWORK_OBSERVE.to_string(),
                                reason,
                            },
                        },
                    )?;
                }
            }
            ExtensionRequest::ReadNetworkTrace { tab_id } => {
                if let Some(reason) =
                    capability_denial_reason(registry, &identity, CAPABILITY_NETWORK_OBSERVE, 2)
                {
                    write_extension_reply(
                        stream,
                        &ExtensionReply::CapabilityDenied {
                            capability: CAPABILITY_NETWORK_OBSERVE.to_string(),
                            reason,
                        },
                    )?;
                } else {
                    write_stable_read_reply(
                        stream,
                        registry,
                        &identity,
                        CAPABILITY_NETWORK_OBSERVE,
                        network_observe_generation,
                        || match (delegates.observe_network_trace)(tab_id) {
                            Ok(trace)
                                if trace.as_ref().is_some_and(|value| {
                                    serde_json::to_vec(value).is_ok_and(|bytes| {
                                        bytes.len()
                                            > blueice_ipc::extension::MAX_NETWORK_TRACE_BYTES
                                    })
                                }) =>
                            {
                                ExtensionReply::OperationUnavailable {
                                    capability: CAPABILITY_NETWORK_OBSERVE.to_string(),
                                    reason: "network trace metadata exceeds the 32768-byte limit"
                                        .to_string(),
                                }
                            }
                            Ok(trace) => ExtensionReply::NetworkTraceResult { trace },
                            Err(reason) => ExtensionReply::OperationUnavailable {
                                capability: CAPABILITY_NETWORK_OBSERVE.to_string(),
                                reason,
                            },
                        },
                    )?;
                }
            }
            ExtensionRequest::SetToolbarButton { label } => {
                if let Some(reason) =
                    capability_denial_reason(registry, &identity, CAPABILITY_UI_INJECT, 1)
                {
                    write_extension_reply(
                        stream,
                        &ExtensionReply::CapabilityDenied {
                            capability: CAPABILITY_UI_INJECT.to_string(),
                            reason,
                        },
                    )?;
                } else {
                    if let Err(reason) = validate_toolbar_label(&label) {
                        write_extension_reply(
                            stream,
                            &ExtensionReply::OperationUnavailable {
                                capability: CAPABILITY_UI_INJECT.to_string(),
                                reason,
                            },
                        )?;
                        continue;
                    }
                    let Some(generation) = registry
                        .capability_generation(&identity.extension_id, CAPABILITY_UI_INJECT)
                    else {
                        write_extension_reply(
                            stream,
                            &ExtensionReply::CapabilityDenied {
                                capability: CAPABILITY_UI_INJECT.to_string(),
                                reason: grant_changed_reason(CAPABILITY_UI_INJECT),
                            },
                        )?;
                        continue;
                    };
                    let detail = format!("action=set-native-toolbar-button; label={label:?}");
                    let reply = match check_extension_action(
                        gatekeeper_socket,
                        &identity.extension_id,
                        CAPABILITY_UI_INJECT,
                        detail,
                    ) {
                        Ok(GatekeeperReply::Cleared)
                            if registry.capability_generation(
                                &identity.extension_id,
                                CAPABILITY_UI_INJECT,
                            ) != Some(generation) =>
                        {
                            ExtensionReply::CapabilityDenied {
                                capability: CAPABILITY_UI_INJECT.to_string(),
                                reason: grant_changed_reason(CAPABILITY_UI_INJECT),
                            }
                        }
                        Ok(GatekeeperReply::Cleared) => {
                            match (delegates.set_toolbar_button)(label, generation) {
                                Ok(()) => {
                                    toolbar_visible = true;
                                    ExtensionReply::UiInjectAck
                                }
                                Err(reason)
                                    if reason == grant_changed_reason(CAPABILITY_UI_INJECT) =>
                                {
                                    ExtensionReply::CapabilityDenied {
                                        capability: CAPABILITY_UI_INJECT.to_string(),
                                        reason,
                                    }
                                }
                                Err(reason) => ExtensionReply::OperationUnavailable {
                                    capability: CAPABILITY_UI_INJECT.to_string(),
                                    reason,
                                },
                            }
                        }
                        Ok(GatekeeperReply::Rejected { reason, category }) => {
                            ExtensionReply::GatekeeperBlocked {
                                capability: CAPABILITY_UI_INJECT.to_string(),
                                reason,
                                category,
                            }
                        }
                        Err(reason) => ExtensionReply::GatekeeperBlocked {
                            capability: CAPABILITY_UI_INJECT.to_string(),
                            reason,
                            category: "gatekeeper-unavailable".to_string(),
                        },
                    };
                    write_extension_reply(stream, &reply)?;
                }
            }
            ExtensionRequest::ClearToolbarButton => {
                if let Some(reason) =
                    capability_denial_reason(registry, &identity, CAPABILITY_UI_INJECT, 1)
                {
                    write_extension_reply(
                        stream,
                        &ExtensionReply::CapabilityDenied {
                            capability: CAPABILITY_UI_INJECT.to_string(),
                            reason,
                        },
                    )?;
                } else {
                    let reply = match (delegates.clear_toolbar_button)() {
                        Ok(()) => {
                            toolbar_visible = false;
                            popup_visible = false;
                            ExtensionReply::UiInjectAck
                        }
                        Err(reason) => ExtensionReply::OperationUnavailable {
                            capability: CAPABILITY_UI_INJECT.to_string(),
                            reason,
                        },
                    };
                    write_extension_reply(stream, &reply)?;
                }
            }
            ExtensionRequest::ShowPopup {
                tab_id,
                title,
                body,
            } => {
                if let Some(reason) =
                    capability_denial_reason(registry, &identity, CAPABILITY_UI_INJECT, 2)
                {
                    write_extension_reply(
                        stream,
                        &ExtensionReply::CapabilityDenied {
                            capability: CAPABILITY_UI_INJECT.to_string(),
                            reason,
                        },
                    )?;
                    continue;
                }
                let Some(generation) =
                    registry.capability_generation(&identity.extension_id, CAPABILITY_UI_INJECT)
                else {
                    write_extension_reply(
                        stream,
                        &ExtensionReply::CapabilityDenied {
                            capability: CAPABILITY_UI_INJECT.to_string(),
                            reason: grant_changed_reason(CAPABILITY_UI_INJECT),
                        },
                    )?;
                    continue;
                };
                if let Err(reason) = validate_popup_text(&title, &body) {
                    write_extension_reply(
                        stream,
                        &ExtensionReply::OperationUnavailable {
                            capability: CAPABILITY_UI_INJECT.to_string(),
                            reason,
                        },
                    )?;
                    continue;
                }
                if !toolbar_visible {
                    write_extension_reply(
                        stream,
                        &ExtensionReply::OperationUnavailable {
                            capability: CAPABILITY_UI_INJECT.to_string(),
                            reason: "a native popup requires this connection's toolbar button"
                                .to_string(),
                        },
                    )?;
                    continue;
                }
                // Native popup prose can influence a human too. Review the
                // bounded, validated text itself before publishing; no
                // guest-selected HTML or link target crosses.
                let detail = format!("action=show-native-popup; title={title:?}; body={body:?}");
                let reply = match check_extension_action(
                    gatekeeper_socket,
                    &identity.extension_id,
                    CAPABILITY_UI_INJECT,
                    detail,
                ) {
                    Ok(GatekeeperReply::Cleared)
                        if registry.capability_generation(
                            &identity.extension_id,
                            CAPABILITY_UI_INJECT,
                        ) != Some(generation) =>
                    {
                        ExtensionReply::CapabilityDenied {
                            capability: CAPABILITY_UI_INJECT.to_string(),
                            reason: grant_changed_reason(CAPABILITY_UI_INJECT),
                        }
                    }
                    Ok(GatekeeperReply::Cleared) => {
                        match (delegates.show_popup)(tab_id, title, body, generation) {
                            Ok(()) => {
                                popup_visible = true;
                                ExtensionReply::UiInjectAck
                            }
                            Err(reason) if reason == grant_changed_reason(CAPABILITY_UI_INJECT) => {
                                ExtensionReply::CapabilityDenied {
                                    capability: CAPABILITY_UI_INJECT.to_string(),
                                    reason,
                                }
                            }
                            Err(reason) => ExtensionReply::OperationUnavailable {
                                capability: CAPABILITY_UI_INJECT.to_string(),
                                reason,
                            },
                        }
                    }
                    Ok(GatekeeperReply::Rejected { reason, category }) => {
                        ExtensionReply::GatekeeperBlocked {
                            capability: CAPABILITY_UI_INJECT.to_string(),
                            reason,
                            category,
                        }
                    }
                    Err(reason) => ExtensionReply::GatekeeperBlocked {
                        capability: CAPABILITY_UI_INJECT.to_string(),
                        reason,
                        category: "gatekeeper-unavailable".to_string(),
                    },
                };
                write_extension_reply(stream, &reply)?;
            }
            ExtensionRequest::ShowPopupAction {
                tab_id,
                title,
                body,
                action_label,
            } => {
                if let Some(reason) =
                    capability_denial_reason(registry, &identity, CAPABILITY_UI_INJECT, 3)
                {
                    write_extension_reply(
                        stream,
                        &ExtensionReply::CapabilityDenied {
                            capability: CAPABILITY_UI_INJECT.to_string(),
                            reason,
                        },
                    )?;
                    continue;
                }
                let Some(generation) =
                    registry.capability_generation(&identity.extension_id, CAPABILITY_UI_INJECT)
                else {
                    write_extension_reply(
                        stream,
                        &ExtensionReply::CapabilityDenied {
                            capability: CAPABILITY_UI_INJECT.to_string(),
                            reason: grant_changed_reason(CAPABILITY_UI_INJECT),
                        },
                    )?;
                    continue;
                };
                if let Err(reason) = validate_popup_text(&title, &body)
                    .and_then(|()| validate_toolbar_label(&action_label))
                {
                    write_extension_reply(
                        stream,
                        &ExtensionReply::OperationUnavailable {
                            capability: CAPABILITY_UI_INJECT.to_string(),
                            reason,
                        },
                    )?;
                    continue;
                }
                if !toolbar_visible {
                    write_extension_reply(
                        stream,
                        &ExtensionReply::OperationUnavailable {
                            capability: CAPABILITY_UI_INJECT.to_string(),
                            reason:
                                "a native popup action requires this connection's toolbar button"
                                    .to_string(),
                        },
                    )?;
                    continue;
                }
                // Every guest-visible word on the interactive surface reaches
                // policy review before a button can appear in browser chrome.
                let detail = format!(
                    "action=show-native-popup; title={title:?}; body={body:?}; action_label={action_label:?}"
                );
                let reply = match check_extension_action(
                    gatekeeper_socket,
                    &identity.extension_id,
                    CAPABILITY_UI_INJECT,
                    detail,
                ) {
                    Ok(GatekeeperReply::Cleared)
                        if registry.capability_generation(
                            &identity.extension_id,
                            CAPABILITY_UI_INJECT,
                        ) != Some(generation) =>
                    {
                        ExtensionReply::CapabilityDenied {
                            capability: CAPABILITY_UI_INJECT.to_string(),
                            reason: grant_changed_reason(CAPABILITY_UI_INJECT),
                        }
                    }
                    Ok(GatekeeperReply::Cleared) => {
                        match (delegates.show_popup_action)(
                            tab_id,
                            title,
                            body,
                            action_label,
                            generation,
                        ) {
                            Ok(()) => {
                                popup_visible = true;
                                ExtensionReply::UiInjectAck
                            }
                            Err(reason) if reason == grant_changed_reason(CAPABILITY_UI_INJECT) => {
                                ExtensionReply::CapabilityDenied {
                                    capability: CAPABILITY_UI_INJECT.to_string(),
                                    reason,
                                }
                            }
                            Err(reason) => ExtensionReply::OperationUnavailable {
                                capability: CAPABILITY_UI_INJECT.to_string(),
                                reason,
                            },
                        }
                    }
                    Ok(GatekeeperReply::Rejected { reason, category }) => {
                        ExtensionReply::GatekeeperBlocked {
                            capability: CAPABILITY_UI_INJECT.to_string(),
                            reason,
                            category,
                        }
                    }
                    Err(reason) => ExtensionReply::GatekeeperBlocked {
                        capability: CAPABILITY_UI_INJECT.to_string(),
                        reason,
                        category: "gatekeeper-unavailable".to_string(),
                    },
                };
                write_extension_reply(stream, &reply)?;
            }
            ExtensionRequest::ClearPopup => {
                if let Some(reason) =
                    capability_denial_reason(registry, &identity, CAPABILITY_UI_INJECT, 2)
                {
                    write_extension_reply(
                        stream,
                        &ExtensionReply::CapabilityDenied {
                            capability: CAPABILITY_UI_INJECT.to_string(),
                            reason,
                        },
                    )?;
                } else {
                    let reply = match (delegates.clear_popup)() {
                        Ok(()) => {
                            popup_visible = false;
                            ExtensionReply::UiInjectAck
                        }
                        Err(reason) => ExtensionReply::OperationUnavailable {
                            capability: CAPABILITY_UI_INJECT.to_string(),
                            reason,
                        },
                    };
                    write_extension_reply(stream, &reply)?;
                }
            }
            request @ (ExtensionRequest::DomWrite { .. }
            | ExtensionRequest::SetTextInputValue { .. }
            | ExtensionRequest::SetCheckboxChecked { .. }
            | ExtensionRequest::SetTextareaValue { .. }
            | ExtensionRequest::SetRangeInputValue { .. }
            | ExtensionRequest::SetVisibleLeafText { .. }
            | ExtensionRequest::SetVisibleTextContent { .. }
            | ExtensionRequest::SetRadioChecked { .. }
            | ExtensionRequest::SelectOption { .. }) => {
                dom_write::handle(
                    RequestContext {
                        registry,
                        gatekeeper_socket,
                        identity: &identity,
                    },
                    stream,
                    request,
                    &mut delegates,
                    dom_write_generation,
                )?;
            }
            request @ (ExtensionRequest::RegisterNetworkBlockUrl { .. }
            | ExtensionRequest::RegisterNetworkBlockHost { .. }
            | ExtensionRequest::RegisterNetworkBlockPathPrefix { .. }
            | ExtensionRequest::RegisterNetworkRedirectUrl { .. }
            | ExtensionRequest::ClearNetworkBlockUrls
            | ExtensionRequest::NetworkIntercept) => {
                network_rules::handle(
                    RequestContext {
                        registry,
                        gatekeeper_socket,
                        identity: &identity,
                    },
                    stream,
                    request,
                    &mut delegates,
                )?;
            }
            request @ (ExtensionRequest::StorageGet { .. }
            | ExtensionRequest::StorageSet { .. }
            | ExtensionRequest::StorageRemove { .. }
            | ExtensionRequest::DurableStorageGet { .. }
            | ExtensionRequest::DurableStorageSet { .. }
            | ExtensionRequest::DurableStorageRemove { .. }
            | ExtensionRequest::DurableStorageListKeys) => {
                storage::handle(
                    RequestContext {
                        registry,
                        gatekeeper_socket,
                        identity: &identity,
                    },
                    stream,
                    request,
                    &mut delegates,
                    storage_generation,
                )?;
            }
        }
    }
}

struct RequestContext<'a> {
    registry: &'a ExtensionRegistry,
    gatekeeper_socket: &'a Path,
    identity: &'a ConnectionIdentity,
}

mod dom_write;
mod network_rules;
mod storage;
