// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

pub(super) fn handle<S, R, W, N, B, C>(
    context: RequestContext<'_>,
    stream: &mut S,
    request: ExtensionRequest,
    delegates: &mut ExtensionActionDelegates<R, W, N, B, C>,
    dom_write_generation: Option<u64>,
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
    let RequestContext {
        registry,
        gatekeeper_socket,
        identity,
    } = context;
    let ExtensionActionDelegates { write_dom, .. } = delegates;
    match request {
        ExtensionRequest::DomWrite { value, target } => {
            if let Some(reason) =
                capability_denial_reason(registry, identity, CAPABILITY_DOM_WRITE, 1)
            {
                write_extension_reply(
                    stream,
                    &ExtensionReply::CapabilityDenied {
                        capability: CAPABILITY_DOM_WRITE.to_string(),
                        reason,
                    },
                )?;
            } else if matches!(
                target,
                blueice_ipc::extension::DomWriteTarget::VisibleTextLeaf
                    | blueice_ipc::extension::DomWriteTarget::VisibleTextContent
            ) {
                write_extension_reply(
                    stream,
                    &ExtensionReply::OperationUnavailable {
                        capability: CAPABILITY_DOM_WRITE.to_string(),
                        reason:
                            "visible text requires its explicit versioned request and live node ID"
                                .to_string(),
                    },
                )?;
            } else if target.requires_gatekeeper_review() {
                match check_extension_action(
                    gatekeeper_socket,
                    &identity.extension_id,
                    CAPABILITY_DOM_WRITE,
                    target
                        .gatekeeper_detail()
                        .expect("only gatekeeper-triggering targets reach extension action review"),
                ) {
                    Ok(GatekeeperReply::Cleared) => write_dom_effect_reply(
                        stream,
                        commit_dom_write(
                            registry,
                            identity,
                            dom_write_generation,
                            write_dom,
                            None,
                            value,
                            &target,
                        ),
                    )?,
                    Ok(GatekeeperReply::Rejected { reason, category }) => {
                        write_extension_reply(
                            stream,
                            &ExtensionReply::GatekeeperBlocked {
                                capability: CAPABILITY_DOM_WRITE.to_string(),
                                reason,
                                category,
                            },
                        )?;
                    }
                    Err(reason) => {
                        write_extension_reply(
                            stream,
                            &ExtensionReply::GatekeeperBlocked {
                                capability: CAPABILITY_DOM_WRITE.to_string(),
                                reason,
                                category: "gatekeeper-unavailable".to_string(),
                            },
                        )?;
                    }
                }
            } else {
                write_dom_effect_reply(
                    stream,
                    commit_dom_write(
                        registry,
                        identity,
                        dom_write_generation,
                        write_dom,
                        None,
                        value,
                        &target,
                    ),
                )?;
            }
        }
        ExtensionRequest::SetTextInputValue {
            tab_id,
            node_id,
            value,
        } => {
            if let Some(reason) =
                capability_denial_reason(registry, identity, CAPABILITY_DOM_WRITE, 2)
            {
                write_extension_reply(
                    stream,
                    &ExtensionReply::CapabilityDenied {
                        capability: CAPABILITY_DOM_WRITE.to_string(),
                        reason,
                    },
                )?;
                return Ok(());
            }
            if value.len() > blueice_ipc::extension::MAX_TEXT_WRITE_BYTES {
                write_extension_reply(
                    stream,
                    &ExtensionReply::OperationUnavailable {
                        capability: CAPABILITY_DOM_WRITE.to_string(),
                        reason: format!(
                            "text-control values cannot exceed {} bytes",
                            blueice_ipc::extension::MAX_TEXT_WRITE_BYTES
                        ),
                    },
                )?;
                return Ok(());
            }
            // The v2 operation has one safe semantic shape: set a
            // text-input value. Review is unconditional, so an extension
            // cannot under-classify a sensitive target through metadata.
            match check_extension_action(
                gatekeeper_socket,
                &identity.extension_id,
                CAPABILITY_DOM_WRITE,
                "action=set-text-input-value".to_string(),
            ) {
                Ok(GatekeeperReply::Cleared) => {
                    let target = blueice_ipc::extension::DomWriteTarget::FormInput {
                        input_type: "text".to_string(),
                    };
                    write_dom_effect_reply(
                        stream,
                        commit_dom_write(
                            registry,
                            identity,
                            dom_write_generation,
                            write_dom,
                            Some((tab_id, node_id)),
                            value,
                            &target,
                        ),
                    )?;
                }
                Ok(GatekeeperReply::Rejected { reason, category }) => {
                    write_extension_reply(
                        stream,
                        &ExtensionReply::GatekeeperBlocked {
                            capability: CAPABILITY_DOM_WRITE.to_string(),
                            reason,
                            category,
                        },
                    )?;
                }
                Err(reason) => {
                    write_extension_reply(
                        stream,
                        &ExtensionReply::GatekeeperBlocked {
                            capability: CAPABILITY_DOM_WRITE.to_string(),
                            reason,
                            category: "gatekeeper-unavailable".to_string(),
                        },
                    )?;
                }
            }
        }
        ExtensionRequest::SetCheckboxChecked {
            tab_id,
            node_id,
            checked,
        } => {
            if let Some(reason) =
                capability_denial_reason(registry, identity, CAPABILITY_DOM_WRITE, 3)
            {
                write_extension_reply(
                    stream,
                    &ExtensionReply::CapabilityDenied {
                        capability: CAPABILITY_DOM_WRITE.to_string(),
                        reason,
                    },
                )?;
                return Ok(());
            }
            // The v3 operation is a deliberately bounded checkbox state
            // change. The action label and form-control class are owned
            // here, never selected by the extension, so review cannot be
            // weakened by untrusted metadata.
            match check_extension_action(
                gatekeeper_socket,
                &identity.extension_id,
                CAPABILITY_DOM_WRITE,
                "action=set-checkbox-checked".to_string(),
            ) {
                Ok(GatekeeperReply::Cleared) => {
                    let target = blueice_ipc::extension::DomWriteTarget::FormInput {
                        input_type: "checkbox".to_string(),
                    };
                    let checked = if checked { "true" } else { "false" }.to_string();
                    write_dom_effect_reply(
                        stream,
                        commit_dom_write(
                            registry,
                            identity,
                            dom_write_generation,
                            write_dom,
                            Some((tab_id, node_id)),
                            checked,
                            &target,
                        ),
                    )?;
                }
                Ok(GatekeeperReply::Rejected { reason, category }) => {
                    write_extension_reply(
                        stream,
                        &ExtensionReply::GatekeeperBlocked {
                            capability: CAPABILITY_DOM_WRITE.to_string(),
                            reason,
                            category,
                        },
                    )?;
                }
                Err(reason) => {
                    write_extension_reply(
                        stream,
                        &ExtensionReply::GatekeeperBlocked {
                            capability: CAPABILITY_DOM_WRITE.to_string(),
                            reason,
                            category: "gatekeeper-unavailable".to_string(),
                        },
                    )?;
                }
            }
        }
        ExtensionRequest::SetTextareaValue {
            tab_id,
            node_id,
            value,
        } => {
            if let Some(reason) =
                capability_denial_reason(registry, identity, CAPABILITY_DOM_WRITE, 4)
            {
                write_extension_reply(
                    stream,
                    &ExtensionReply::CapabilityDenied {
                        capability: CAPABILITY_DOM_WRITE.to_string(),
                        reason,
                    },
                )?;
                return Ok(());
            }
            if value.len() > blueice_ipc::extension::MAX_TEXT_WRITE_BYTES {
                write_extension_reply(
                    stream,
                    &ExtensionReply::OperationUnavailable {
                        capability: CAPABILITY_DOM_WRITE.to_string(),
                        reason: format!(
                            "text-control values cannot exceed {} bytes",
                            blueice_ipc::extension::MAX_TEXT_WRITE_BYTES
                        ),
                    },
                )?;
                return Ok(());
            }
            // The v4 operation is one reviewed, core-defined textarea
            // write. The extension never controls the action label or
            // target class passed into policy review.
            match check_extension_action(
                gatekeeper_socket,
                &identity.extension_id,
                CAPABILITY_DOM_WRITE,
                "action=set-textarea-value".to_string(),
            ) {
                Ok(GatekeeperReply::Cleared) => {
                    let target = blueice_ipc::extension::DomWriteTarget::FormInput {
                        input_type: "textarea".to_string(),
                    };
                    write_dom_effect_reply(
                        stream,
                        commit_dom_write(
                            registry,
                            identity,
                            dom_write_generation,
                            write_dom,
                            Some((tab_id, node_id)),
                            value,
                            &target,
                        ),
                    )?;
                }
                Ok(GatekeeperReply::Rejected { reason, category }) => {
                    write_extension_reply(
                        stream,
                        &ExtensionReply::GatekeeperBlocked {
                            capability: CAPABILITY_DOM_WRITE.to_string(),
                            reason,
                            category,
                        },
                    )?;
                }
                Err(reason) => {
                    write_extension_reply(
                        stream,
                        &ExtensionReply::GatekeeperBlocked {
                            capability: CAPABILITY_DOM_WRITE.to_string(),
                            reason,
                            category: "gatekeeper-unavailable".to_string(),
                        },
                    )?;
                }
            }
        }
        ExtensionRequest::SetRangeInputValue {
            tab_id,
            node_id,
            value,
        } => {
            if let Some(reason) =
                capability_denial_reason(registry, identity, CAPABILITY_DOM_WRITE, 7)
            {
                write_extension_reply(
                    stream,
                    &ExtensionReply::CapabilityDenied {
                        capability: CAPABILITY_DOM_WRITE.to_string(),
                        reason,
                    },
                )?;
                return Ok(());
            }
            // The v7 operation carries only a signed integer. Core owns
            // the target's live type, disabled state, min/max/step, and
            // resulting value; the extension cannot provide a numeric
            // constraint or arbitrary attribute to weaken that boundary.
            match check_extension_action(
                gatekeeper_socket,
                &identity.extension_id,
                CAPABILITY_DOM_WRITE,
                "action=set-range-input-value".to_string(),
            ) {
                Ok(GatekeeperReply::Cleared) => {
                    let target = blueice_ipc::extension::DomWriteTarget::FormInput {
                        input_type: "range".to_string(),
                    };
                    write_dom_effect_reply(
                        stream,
                        commit_dom_write(
                            registry,
                            identity,
                            dom_write_generation,
                            write_dom,
                            Some((tab_id, node_id)),
                            value.to_string(),
                            &target,
                        ),
                    )?;
                }
                Ok(GatekeeperReply::Rejected { reason, category }) => {
                    write_extension_reply(
                        stream,
                        &ExtensionReply::GatekeeperBlocked {
                            capability: CAPABILITY_DOM_WRITE.to_string(),
                            reason,
                            category,
                        },
                    )?;
                }
                Err(reason) => {
                    write_extension_reply(
                        stream,
                        &ExtensionReply::GatekeeperBlocked {
                            capability: CAPABILITY_DOM_WRITE.to_string(),
                            reason,
                            category: "gatekeeper-unavailable".to_string(),
                        },
                    )?;
                }
            }
        }
        request @ (ExtensionRequest::SetVisibleLeafText { .. }
        | ExtensionRequest::SetVisibleTextContent { .. }) => {
            let (tab_id, node_id, value, required_version, action, target) = match request {
                ExtensionRequest::SetVisibleLeafText {
                    tab_id,
                    node_id,
                    value,
                } => (
                    tab_id,
                    node_id,
                    value,
                    8,
                    "set-visible-leaf-text",
                    blueice_ipc::extension::DomWriteTarget::VisibleTextLeaf,
                ),
                ExtensionRequest::SetVisibleTextContent {
                    tab_id,
                    node_id,
                    value,
                } => (
                    tab_id,
                    node_id,
                    value,
                    9,
                    "set-visible-text-content",
                    blueice_ipc::extension::DomWriteTarget::VisibleTextContent,
                ),
                _ => unreachable!("the matched request is a visible-text write"),
            };
            if let Some(reason) =
                capability_denial_reason(registry, identity, CAPABILITY_DOM_WRITE, required_version)
            {
                write_extension_reply(
                    stream,
                    &ExtensionReply::CapabilityDenied {
                        capability: CAPABILITY_DOM_WRITE.to_string(),
                        reason,
                    },
                )?;
                return Ok(());
            }
            if value.trim().is_empty()
                || value.len() > blueice_ipc::extension::MAX_VISIBLE_LEAF_TEXT_BYTES
                || value
                    .chars()
                    .any(|ch| ch.is_control() && ch != '\n' && ch != '\t')
            {
                write_extension_reply(stream, &ExtensionReply::OperationUnavailable {
                    capability: CAPABILITY_DOM_WRITE.to_string(),
                    reason: "visible text must be nonempty, within its limit, and free of control characters".to_string(),
                })?;
                return Ok(());
            }
            // Unlike form values, visible replacement text is itself a
            // public-facing effect. Review the exact bounded payload;
            // core separately verifies the live target and URL.
            let detail = format!("action={action}; text={value}");
            match check_extension_action(
                gatekeeper_socket,
                &identity.extension_id,
                CAPABILITY_DOM_WRITE,
                detail,
            ) {
                Ok(GatekeeperReply::Cleared) => {
                    write_dom_effect_reply(
                        stream,
                        commit_dom_write(
                            registry,
                            identity,
                            dom_write_generation,
                            write_dom,
                            Some((tab_id, node_id)),
                            value,
                            &target,
                        ),
                    )?;
                }
                Ok(GatekeeperReply::Rejected { reason, category }) => {
                    write_extension_reply(
                        stream,
                        &ExtensionReply::GatekeeperBlocked {
                            capability: CAPABILITY_DOM_WRITE.to_string(),
                            reason,
                            category,
                        },
                    )?;
                }
                Err(reason) => {
                    write_extension_reply(
                        stream,
                        &ExtensionReply::GatekeeperBlocked {
                            capability: CAPABILITY_DOM_WRITE.to_string(),
                            reason,
                            category: "gatekeeper-unavailable".to_string(),
                        },
                    )?;
                }
            }
        }
        ExtensionRequest::SetRadioChecked { tab_id, node_id } => {
            if let Some(reason) =
                capability_denial_reason(registry, identity, CAPABILITY_DOM_WRITE, 5)
            {
                write_extension_reply(
                    stream,
                    &ExtensionReply::CapabilityDenied {
                        capability: CAPABILITY_DOM_WRITE.to_string(),
                        reason,
                    },
                )?;
                return Ok(());
            }
            // Version 5 deliberately represents only a radio selection:
            // group identity and the corresponding unchecks are derived
            // by core from the live document, never from extension input.
            match check_extension_action(
                gatekeeper_socket,
                &identity.extension_id,
                CAPABILITY_DOM_WRITE,
                "action=set-radio-checked".to_string(),
            ) {
                Ok(GatekeeperReply::Cleared) => {
                    let target = blueice_ipc::extension::DomWriteTarget::FormInput {
                        input_type: "radio".to_string(),
                    };
                    write_dom_effect_reply(
                        stream,
                        commit_dom_write(
                            registry,
                            identity,
                            dom_write_generation,
                            write_dom,
                            Some((tab_id, node_id)),
                            "true".to_string(),
                            &target,
                        ),
                    )?;
                }
                Ok(GatekeeperReply::Rejected { reason, category }) => {
                    write_extension_reply(
                        stream,
                        &ExtensionReply::GatekeeperBlocked {
                            capability: CAPABILITY_DOM_WRITE.to_string(),
                            reason,
                            category,
                        },
                    )?;
                }
                Err(reason) => {
                    write_extension_reply(
                        stream,
                        &ExtensionReply::GatekeeperBlocked {
                            capability: CAPABILITY_DOM_WRITE.to_string(),
                            reason,
                            category: "gatekeeper-unavailable".to_string(),
                        },
                    )?;
                }
            }
        }
        ExtensionRequest::SelectOption { tab_id, node_id } => {
            if let Some(reason) =
                capability_denial_reason(registry, identity, CAPABILITY_DOM_WRITE, 6)
            {
                write_extension_reply(
                    stream,
                    &ExtensionReply::CapabilityDenied {
                        capability: CAPABILITY_DOM_WRITE.to_string(),
                        reason,
                    },
                )?;
                return Ok(());
            }
            // Version 6 deliberately represents only selection of one
            // option. The extension cannot nominate an owning select,
            // clear a particular peer, or classify its own target for
            // review; core derives all of that from live DOM state.
            match check_extension_action(
                gatekeeper_socket,
                &identity.extension_id,
                CAPABILITY_DOM_WRITE,
                "action=select-option".to_string(),
            ) {
                Ok(GatekeeperReply::Cleared) => {
                    let target = blueice_ipc::extension::DomWriteTarget::FormInput {
                        input_type: "select".to_string(),
                    };
                    write_dom_effect_reply(
                        stream,
                        commit_dom_write(
                            registry,
                            identity,
                            dom_write_generation,
                            write_dom,
                            Some((tab_id, node_id)),
                            "true".to_string(),
                            &target,
                        ),
                    )?;
                }
                Ok(GatekeeperReply::Rejected { reason, category }) => {
                    write_extension_reply(
                        stream,
                        &ExtensionReply::GatekeeperBlocked {
                            capability: CAPABILITY_DOM_WRITE.to_string(),
                            reason,
                            category,
                        },
                    )?;
                }
                Err(reason) => {
                    write_extension_reply(
                        stream,
                        &ExtensionReply::GatekeeperBlocked {
                            capability: CAPABILITY_DOM_WRITE.to_string(),
                            reason,
                            category: "gatekeeper-unavailable".to_string(),
                        },
                    )?;
                }
            }
        }
        _ => unreachable!("request belongs to another extension domain"),
    }
    Ok(())
}
