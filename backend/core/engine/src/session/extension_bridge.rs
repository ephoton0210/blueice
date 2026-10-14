// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

/// Removes connection-owned effects published under an earlier optional
/// grant generation. The session polls even without client traffic, so a
/// revoke does not need a public IPC trigger to retire UI or network rules.
pub(super) fn prune_stale_extension_effects<S: Write>(
    tabs: &mut TabManager,
    stream: &mut S,
    toolbar: &mut Option<(u64, String, u64)>,
    popup: &mut Option<(u64, ExtensionPopup, u64)>,
) -> io::Result<()> {
    tabs.prune_stale_extension_navigation_rules();
    let current = tabs.extension_capability_generation("ui:inject");
    let toolbar_is_stale = toolbar
        .as_ref()
        .is_some_and(|(_, _, generation)| current != Some(*generation));
    let popup_is_stale = popup
        .as_ref()
        .is_some_and(|(_, _, generation)| current != Some(*generation));
    if (toolbar_is_stale || popup_is_stale) && popup.take().is_some() {
        blueice_ipc::write_server_message_with_ids(
            stream,
            None,
            None,
            &ServerMessage::ExtensionPopup { popup: None },
        )?;
    }
    if toolbar_is_stale {
        toolbar.take();
        blueice_ipc::write_server_message_with_ids(
            stream,
            None,
            None,
            &ServerMessage::ExtensionToolbar { label: None },
        )?;
    }
    Ok(())
}

/// Applies a request received from the extension host. An accepted write
/// produces the same uncorrelated fresh frame that other background-originated
/// core work does, so every connected observer sees the core-owned mutation.
/// The typed one-shot response then gives the extension connection one answer
/// while keeping this session loop the only owner of page state.
pub(super) fn handle_extension_page_request<S: Write>(
    tabs: &mut TabManager,
    stream: &mut S,
    frame_dir: &Path,
    generation: &mut u64,
    extension_toolbar: &mut Option<(u64, String, u64)>,
    extension_popup: &mut Option<(u64, ExtensionPopup, u64)>,
    request: ExtensionPageRequest,
) -> io::Result<()> {
    match request {
        ExtensionPageRequest::SynchronizeRevokedEffects { reply } => {
            prune_stale_extension_effects(tabs, stream, extension_toolbar, extension_popup)?;
            let _ = reply.send(());
        }
        ExtensionPageRequest::InspectDocument { tab_id, reply } => {
            let id = TabId::from_u64(tab_id);
            let result = tabs
                .document_epoch(id)
                .map(|epoch| {
                    (
                        epoch,
                        tabs.get(id).and_then(|page| page.url()).map(str::to_string),
                    )
                })
                .ok_or_else(|| "the requested tab is not live".to_string());
            let _ = reply.send(result);
        }
        ExtensionPageRequest::ReadRepresentation { tab_id, reply } => {
            let tab_id = tab_id
                .map(TabId::from_u64)
                .unwrap_or_else(|| tabs.default_tab());
            let result = tabs
                .check_extension_origin("dom:read", tab_id)
                .and_then(|()| {
                    let page = tabs.get(tab_id).expect("the checked tab remains live");
                    let mut snapshot = page.snapshot(page.frame_generation(), tab_id.as_u64());
                    snapshot.frame_source = blueice_ipc::shm::frame_source_id(frame_dir);
                    serde_json::to_string(&snapshot).map_err(|error| {
                        format!("could not serialize the core representation: {error}")
                    })
                });
            let _ = reply.send(result);
        }
        ExtensionPageRequest::ReadEphemeralRepresentation {
            tab_id,
            ticket,
            reply,
        } => {
            let id = TabId::from_u64(tab_id);
            let result = tabs
                .check_extension_origin("dom:read", id)
                .and_then(|()| tabs.consume_extension_runtime_ephemeral("dom:read", id, &ticket))
                .and_then(|()| {
                    let page = tabs.get(id).expect("the consumed lease names a live tab");
                    let mut snapshot = page.snapshot(page.frame_generation(), tab_id);
                    snapshot.frame_source = blueice_ipc::shm::frame_source_id(frame_dir);
                    serde_json::to_string(&snapshot).map_err(|error| {
                        format!("could not serialize the core representation: {error}")
                    })
                });
            let _ = reply.send(result);
        }
        ExtensionPageRequest::ReadNetworkResponse { tab_id, reply } => {
            let tab_id = TabId::from_u64(tab_id);
            let result = tabs
                .check_extension_origin("network:observe", tab_id)
                .map(|()| {
                    tabs.get(tab_id)
                        .expect("the checked tab remains live")
                        .network_response()
                        .cloned()
                });
            let _ = reply.send(result);
        }
        ExtensionPageRequest::ReadNetworkTrace { tab_id, reply } => {
            let tab_id = TabId::from_u64(tab_id);
            let result = tabs
                .check_extension_origin("network:observe", tab_id)
                .map(|()| {
                    tabs.get(tab_id)
                        .expect("the checked tab remains live")
                        .network_trace()
                        .cloned()
                });
            let _ = reply.send(result);
        }
        ExtensionPageRequest::SetToolbarButton {
            connection_id,
            grant_generation,
            label,
            reply,
        } => {
            let result = tabs
                .with_stable_extension_capability("ui:inject", grant_generation, |_| {
                    blueice_extension_host::validate_toolbar_label(&label).and_then(|()| {
                        if extension_popup
                            .as_ref()
                            .is_some_and(|(owner, _, _)| *owner != connection_id)
                        {
                            blueice_ipc::write_server_message_with_ids(
                                stream,
                                None,
                                None,
                                &ServerMessage::ExtensionPopup { popup: None },
                            )
                            .map_err(|error| {
                                format!("could not remove old extension popup: {error}")
                            })?;
                            *extension_popup = None;
                        }
                        blueice_ipc::write_server_message_with_ids(
                            stream,
                            None,
                            None,
                            &ServerMessage::ExtensionToolbar {
                                label: Some(label.clone()),
                            },
                        )
                        .map_err(|error| format!("could not publish extension toolbar: {error}"))?;
                        *extension_toolbar = Some((connection_id, label, grant_generation));
                        Ok(())
                    })
                })
                .and_then(|result| result);
            let _ = reply.send(result);
        }
        ExtensionPageRequest::ClearToolbarButton {
            connection_id,
            reply,
        } => {
            if extension_toolbar
                .as_ref()
                .is_some_and(|(owner, _, _)| *owner == connection_id)
            {
                if extension_popup
                    .as_ref()
                    .is_some_and(|(owner, _, _)| *owner == connection_id)
                {
                    *extension_popup = None;
                    blueice_ipc::write_server_message_with_ids(
                        stream,
                        None,
                        None,
                        &ServerMessage::ExtensionPopup { popup: None },
                    )?;
                }
                *extension_toolbar = None;
                blueice_ipc::write_server_message_with_ids(
                    stream,
                    None,
                    None,
                    &ServerMessage::ExtensionToolbar { label: None },
                )?;
            }
            let _ = reply.send(());
        }
        ExtensionPageRequest::ShowPopup {
            connection_id,
            grant_generation,
            popup,
            reply,
        } => {
            let result = tabs
                .with_stable_extension_capability("ui:inject", grant_generation, |tabs| {
                    blueice_extension_host::validate_popup_text(&popup.title, &popup.body)
                        .and_then(|()| match popup.action_label.as_deref() {
                            Some(_) if popup.id == 0 => {
                                Err("a popup action needs a core-assigned ID".to_string())
                            }
                            Some(label) => blueice_extension_host::validate_toolbar_label(label),
                            None => Ok(()),
                        })
                        .and_then(|()| {
                            if !extension_toolbar
                                .as_ref()
                                .is_some_and(|(owner, _, generation)| {
                                    *owner == connection_id && *generation == grant_generation
                                })
                            {
                                return Err(
                                    "a popup requires this connection's toolbar button".to_string()
                                );
                            }
                            if tabs.get(TabId::from_u64(popup.tab_id)).is_none() {
                                return Err(format!("unknown tab {}", popup.tab_id));
                            }
                            blueice_ipc::write_server_message_with_ids(
                                stream,
                                None,
                                None,
                                &ServerMessage::ExtensionPopup {
                                    popup: Some(popup.clone()),
                                },
                            )
                            .map_err(|error| {
                                format!("could not publish extension popup: {error}")
                            })?;
                            *extension_popup = Some((connection_id, popup, grant_generation));
                            Ok(())
                        })
                })
                .and_then(|result| result);
            let _ = reply.send(result);
        }
        ExtensionPageRequest::ClearPopup {
            connection_id,
            reply,
        } => {
            if extension_popup
                .as_ref()
                .is_some_and(|(owner, _, _)| *owner == connection_id)
            {
                *extension_popup = None;
                blueice_ipc::write_server_message_with_ids(
                    stream,
                    None,
                    None,
                    &ServerMessage::ExtensionPopup { popup: None },
                )?;
            }
            let _ = reply.send(());
        }
        ExtensionPageRequest::SetTextInputValue {
            tab_id,
            node_id,
            value,
            grant_generation,
            reply,
        } => {
            let tab_id = TabId::from_u64(tab_id);
            let node_id = NodeId::from_u64(node_id);
            let result = tabs
                .with_stable_extension_capability("dom:write", grant_generation, |tabs| {
                    if value.len() > blueice_ipc::extension::MAX_TEXT_WRITE_BYTES {
                        Err(format!(
                            "text-control values cannot exceed {} bytes",
                            blueice_ipc::extension::MAX_TEXT_WRITE_BYTES
                        ))
                    } else {
                        tabs.check_extension_origin("dom:write", tab_id)
                            .and_then(|()| match tabs.get_mut(tab_id) {
                                Some(page) => page.set_text_input_value(node_id, value),
                                None => Err(format!("unknown tab {}", tab_id.as_u64())),
                            })
                    }
                })
                .and_then(|result| result);
            if result.is_ok() {
                // Mirror first-party SetValue: script listeners observe the
                // core-owned new value before observers receive its frame.
                let page = tabs
                    .get_mut(tab_id)
                    .expect("a checked extension target tab remains live");
                send_frame(
                    page,
                    stream,
                    frame_dir,
                    generation,
                    Some(tab_id.as_u64()),
                    None,
                )?;
            }
            let _ = reply.send(result);
        }
        ExtensionPageRequest::SetCheckboxChecked {
            tab_id,
            node_id,
            checked,
            grant_generation,
            reply,
        } => {
            let tab_id = TabId::from_u64(tab_id);
            let node_id = NodeId::from_u64(node_id);
            let result = tabs
                .with_stable_extension_capability("dom:write", grant_generation, |tabs| {
                    tabs.check_extension_origin("dom:write", tab_id)
                        .and_then(|()| match tabs.get_mut(tab_id) {
                            Some(page) => page.set_checkbox_checked(node_id, checked),
                            None => Err(format!("unknown tab {}", tab_id.as_u64())),
                        })
                })
                .and_then(|result| result);
            if result.is_ok() {
                // Match the text-input extension operation: page event
                // handlers see core's new state before the shared frame is
                // published to observers.
                let page = tabs
                    .get_mut(tab_id)
                    .expect("a checked extension target tab remains live");
                send_frame(
                    page,
                    stream,
                    frame_dir,
                    generation,
                    Some(tab_id.as_u64()),
                    None,
                )?;
            }
            let _ = reply.send(result);
        }
        ExtensionPageRequest::SetTextareaValue {
            tab_id,
            node_id,
            value,
            grant_generation,
            reply,
        } => {
            let tab_id = TabId::from_u64(tab_id);
            let node_id = NodeId::from_u64(node_id);
            let result = tabs
                .with_stable_extension_capability("dom:write", grant_generation, |tabs| {
                    if value.len() > blueice_ipc::extension::MAX_TEXT_WRITE_BYTES {
                        Err(format!(
                            "text-control values cannot exceed {} bytes",
                            blueice_ipc::extension::MAX_TEXT_WRITE_BYTES
                        ))
                    } else {
                        tabs.check_extension_origin("dom:write", tab_id)
                            .and_then(|()| match tabs.get_mut(tab_id) {
                                Some(page) => page.set_textarea_value(node_id, value),
                                None => Err(format!("unknown tab {}", tab_id.as_u64())),
                            })
                    }
                })
                .and_then(|result| result);
            if result.is_ok() {
                // Match the other constrained form writes: event handlers
                // see the core-owned value before observers receive a frame.
                let page = tabs
                    .get_mut(tab_id)
                    .expect("a checked extension target tab remains live");
                send_frame(
                    page,
                    stream,
                    frame_dir,
                    generation,
                    Some(tab_id.as_u64()),
                    None,
                )?;
            }
            let _ = reply.send(result);
        }
        ExtensionPageRequest::SetVisibleLeafText {
            tab_id,
            node_id,
            value,
            grant_generation,
            reply,
        } => {
            let tab_id = TabId::from_u64(tab_id);
            let node_id = NodeId::from_u64(node_id);
            let result = tabs
                .with_stable_extension_capability("dom:write", grant_generation, |tabs| {
                    tabs.check_extension_origin("dom:write", tab_id)
                        .and_then(|()| match tabs.get_mut(tab_id) {
                            Some(page) => page.set_visible_leaf_text(node_id, value),
                            None => Err(format!("unknown tab {}", tab_id.as_u64())),
                        })
                })
                .and_then(|result| result);
            if result.is_ok() {
                let page = tabs
                    .get_mut(tab_id)
                    .expect("the extension target tab remains live");
                send_frame(
                    page,
                    stream,
                    frame_dir,
                    generation,
                    Some(tab_id.as_u64()),
                    None,
                )?;
            }
            let _ = reply.send(result);
        }
        ExtensionPageRequest::SetVisibleTextContent {
            tab_id,
            node_id,
            value,
            grant_generation,
            reply,
        } => {
            let tab_id = TabId::from_u64(tab_id);
            let node_id = NodeId::from_u64(node_id);
            let result = tabs
                .with_stable_extension_capability("dom:write", grant_generation, |tabs| {
                    tabs.check_extension_origin("dom:write", tab_id)
                        .and_then(|()| match tabs.get_mut(tab_id) {
                            Some(page) => page.set_visible_text_content(node_id, value),
                            None => Err(format!("unknown tab {}", tab_id.as_u64())),
                        })
                })
                .and_then(|result| result);
            if result.is_ok() {
                let page = tabs
                    .get_mut(tab_id)
                    .expect("the extension target tab remains live");
                send_frame(
                    page,
                    stream,
                    frame_dir,
                    generation,
                    Some(tab_id.as_u64()),
                    None,
                )?;
            }
            let _ = reply.send(result);
        }
        ExtensionPageRequest::SetRangeInputValue {
            tab_id,
            node_id,
            value,
            grant_generation,
            reply,
        } => {
            let tab_id = TabId::from_u64(tab_id);
            let node_id = NodeId::from_u64(node_id);
            let result = tabs
                .with_stable_extension_capability("dom:write", grant_generation, |tabs| {
                    tabs.check_extension_origin("dom:write", tab_id)
                        .and_then(|()| match tabs.get_mut(tab_id) {
                            Some(page) => page.set_range_input_value(node_id, value),
                            None => Err(format!("unknown tab {}", tab_id.as_u64())),
                        })
                })
                .and_then(|result| result);
            if result.is_ok() {
                // A range is one constrained form control, so listeners see
                // its committed core state before the shared observer frame.
                let page = tabs
                    .get_mut(tab_id)
                    .expect("a checked extension target tab remains live");
                send_frame(
                    page,
                    stream,
                    frame_dir,
                    generation,
                    Some(tab_id.as_u64()),
                    None,
                )?;
            }
            let _ = reply.send(result);
        }
        ExtensionPageRequest::SetRadioChecked {
            tab_id,
            node_id,
            grant_generation,
            reply,
        } => {
            let tab_id = TabId::from_u64(tab_id);
            let node_id = NodeId::from_u64(node_id);
            let result = tabs
                .with_stable_extension_capability("dom:write", grant_generation, |tabs| {
                    tabs.check_extension_origin("dom:write", tab_id)
                        .and_then(|()| match tabs.get_mut(tab_id) {
                            Some(page) => page.set_radio_checked(node_id),
                            None => Err(format!("unknown tab {}", tab_id.as_u64())),
                        })
                })
                .and_then(|result| result);
            if result.is_ok() {
                // A selected radio can clear other controls in its core-owned
                // group, so publish one post-mutation input/change pair and a
                // single shared frame after the entire group update.
                let page = tabs
                    .get_mut(tab_id)
                    .expect("a checked extension target tab remains live");
                send_frame(
                    page,
                    stream,
                    frame_dir,
                    generation,
                    Some(tab_id.as_u64()),
                    None,
                )?;
            }
            let _ = reply.send(result);
        }
        ExtensionPageRequest::SelectOption {
            tab_id,
            node_id,
            grant_generation,
            reply,
        } => {
            let tab_id = TabId::from_u64(tab_id);
            let node_id = NodeId::from_u64(node_id);
            let result = tabs
                .with_stable_extension_capability("dom:write", grant_generation, |tabs| {
                    tabs.check_extension_origin("dom:write", tab_id)
                        .and_then(|()| match tabs.get_mut(tab_id) {
                            Some(page) => page.select_option(node_id),
                            None => Err(format!("unknown tab {}", tab_id.as_u64())),
                        })
                })
                .and_then(|result| result);
            if result.is_ok() {
                // A single-select transition can clear another option, so
                // dispatch one input/change pair and publish one post-update
                // frame only after core has completed the whole group change.
                let page = tabs
                    .get_mut(tab_id)
                    .expect("a checked extension target tab remains live");
                send_frame(
                    page,
                    stream,
                    frame_dir,
                    generation,
                    Some(tab_id.as_u64()),
                    None,
                )?;
            }
            let _ = reply.send(result);
        }
        ExtensionPageRequest::RegisterNetworkBlockUrl {
            connection_id,
            grant_generation,
            url,
            reply,
        } => {
            let result = tabs
                .with_stable_extension_capability("network:intercept", grant_generation, |tabs| {
                    tabs.add_extension_navigation_block_rule(connection_id, url)
                })
                .and_then(|result| result);
            let _ = reply.send(result);
        }
        ExtensionPageRequest::RegisterNetworkBlockHost {
            connection_id,
            grant_generation,
            host,
            reply,
        } => {
            let result = tabs
                .with_stable_extension_capability("network:intercept", grant_generation, |tabs| {
                    tabs.add_extension_navigation_block_host_rule(connection_id, host)
                })
                .and_then(|result| result);
            let _ = reply.send(result);
        }
        ExtensionPageRequest::RegisterNetworkBlockPathPrefix {
            connection_id,
            grant_generation,
            host,
            path_prefix,
            reply,
        } => {
            let result = tabs
                .with_stable_extension_capability("network:intercept", grant_generation, |tabs| {
                    tabs.add_extension_navigation_block_path_prefix_rule(
                        connection_id,
                        host,
                        path_prefix,
                    )
                })
                .and_then(|result| result);
            let _ = reply.send(result);
        }
        ExtensionPageRequest::RegisterNetworkRedirectUrl {
            connection_id,
            grant_generation,
            source_url,
            target_url,
            reply,
        } => {
            let result = tabs
                .with_stable_extension_capability("network:intercept", grant_generation, |tabs| {
                    tabs.add_extension_navigation_redirect_rule(
                        connection_id,
                        source_url,
                        target_url,
                    )
                })
                .and_then(|result| result);
            let _ = reply.send(result);
        }
        ExtensionPageRequest::ClearNetworkBlockUrls {
            connection_id,
            reply,
        } => {
            tabs.clear_extension_navigation_block_rules(connection_id);
            let _ = reply.send(());
        }
    }
    Ok(())
}
