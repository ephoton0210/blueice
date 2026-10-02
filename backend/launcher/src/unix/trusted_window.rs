// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

pub(super) fn serve_trusted_window_pipe<R: Read, W: Write>(
    mut requests: R,
    mut replies: W,
    broker: &Arc<Broker>,
    ready: Sender<()>,
) -> io::Result<()> {
    let mut ready = Some(ready);
    let mut reviewed_ephemeral = None;
    let result = (|| {
        while let Some(request) = trusted_window::read_request(&mut requests)? {
            let inspected = matches!(&request, trusted_window::TrustedWindowRequest::Inspect);
            let reply =
                handle_trusted_window_session_request(request, broker, &mut reviewed_ephemeral);
            trusted_window::write_reply(&mut replies, &reply)?;
            if inspected && matches!(reply, trusted_window::TrustedWindowReply::State { .. }) {
                if let Some(ready) = ready.take() {
                    let _ = ready.send(());
                }
            }
        }
        Ok(())
    })();
    // The native window is the lifetime owner of its human-approved grants.
    // EOF, malformed frames, or a broken reply pipe must tear down the
    // active core, not leave those grants serving to ordinary/MCP clients.
    let _ = broker.done.send(());
    result
}

#[derive(PartialEq, Eq)]
pub(super) struct ReviewedEphemeral {
    pub(super) core_generation: u64,
    pub(super) extension_id: String,
    pub(super) capability: String,
    pub(super) tab_id: u64,
    pub(super) document_epoch: u64,
}

/// The private native pipe itself requires a successful review immediately
/// before one matching confirmation. Even a malformed or replayed request
/// from that exact child cannot skip the review or reuse it for a second arm.
pub(super) fn handle_trusted_window_session_request(
    request: trusted_window::TrustedWindowRequest,
    broker: &Arc<Broker>,
    reviewed_ephemeral: &mut Option<ReviewedEphemeral>,
) -> trusted_window::TrustedWindowReply {
    let confirmation_matches_review = match &request {
        trusted_window::TrustedWindowRequest::ArmEphemeral {
            expected_core_generation,
            expected_extension_id,
            capability,
            tab_id,
            document_epoch,
        } => reviewed_ephemeral.as_ref().is_some_and(|review| {
            review.core_generation == *expected_core_generation
                && review.extension_id == *expected_extension_id
                && review.capability == *capability
                && review.tab_id == *tab_id
                && review.document_epoch == *document_epoch
        }),
        _ => true,
    };
    crate::trace::event(
        "trusted.request",
        &crate::trace::trusted_request_name(&request),
    );
    // Any intervening request cancels the old review. Arm consumes it before
    // reaching core, including when cutover or a changed document rejects it.
    *reviewed_ephemeral = None;
    // Assistant-settings requests are about the launcher's own settings, not the
    // core, so they are answered here without inspecting the extension state.
    if let Some(service) = broker.assistant_settings.as_ref() {
        if let Some(reply) = service.handle_trusted(request.clone()) {
            crate::trace::event("trusted.reply", &crate::trace::trusted_reply_name(&reply));
            return reply;
        }
    } else if matches!(
        request,
        trusted_window::TrustedWindowRequest::InspectAssistantSettings
            | trusted_window::TrustedWindowRequest::ApproveAssistantProposal { .. }
            | trusted_window::TrustedWindowRequest::DenyAssistantProposal { .. }
            | trusted_window::TrustedWindowRequest::EditAssistantSettings { .. }
    ) {
        return trusted_window::TrustedWindowReply::Rejected {
            reason: "this launcher supervises no assistant".to_string(),
        };
    }
    if !confirmation_matches_review {
        return trusted_window::TrustedWindowReply::Rejected {
            reason: "review the live one-shot document before confirming it".into(),
        };
    }
    let reply = handle_trusted_window_request(request, broker);
    crate::trace::event("trusted.reply", &crate::trace::trusted_reply_name(&reply));
    if let trusted_window::TrustedWindowReply::EphemeralReview {
        core_generation,
        installed,
        capability,
        tab_id,
        document_epoch,
        ..
    } = &reply
    {
        *reviewed_ephemeral = Some(ReviewedEphemeral {
            core_generation: *core_generation,
            extension_id: installed.extension_id.clone(),
            capability: capability.clone(),
            tab_id: *tab_id,
            document_epoch: *document_epoch,
        });
    }
    reply
}

pub(super) fn handle_trusted_window_request(
    request: trusted_window::TrustedWindowRequest,
    broker: &Arc<Broker>,
) -> trusted_window::TrustedWindowReply {
    let result = (|| -> Result<trusted_window::TrustedWindowReply, String> {
        let mutating = matches!(
            &request,
            trusted_window::TrustedWindowRequest::Change { .. }
                | trusted_window::TrustedWindowRequest::ArmEphemeral { .. }
        );
        // A mutation must not race a capture/replay/swap. A busy cutover
        // rejects it; the person can inspect the new generation afterward.
        let _cutover_guard = if mutating {
            Some(broker.cutover_gate.try_acquire().ok_or_else(|| {
                "a core cutover is in progress; inspect permissions again".to_string()
            })?)
        } else {
            None
        };
        let mut active = broker
            .active_core
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let generation = broker.generation.load(Ordering::SeqCst);
        let core = active
            .as_mut()
            .ok_or_else(|| "the active core is unavailable".to_string())?;
        let installed = core
            .inspect_installed_extension()
            .map_err(|error| format!("the active core permission state is unavailable: {error}"))?;
        match request {
            trusted_window::TrustedWindowRequest::Inspect => {
                Ok(trusted_window::TrustedWindowReply::State {
                    core_generation: generation,
                    installed,
                })
            }
            trusted_window::TrustedWindowRequest::Change {
                expected_core_generation,
                expected_extension_id,
                capability,
                action,
            } => {
                trusted_window::validate_change_target(
                    generation,
                    installed.as_ref(),
                    expected_core_generation,
                    &expected_extension_id,
                    &capability,
                )
                .map_err(str::to_string)?;
                let updated = core
                    .apply_optional_change(action, &capability)
                    .map_err(|error| {
                        format!("the active core could not confirm the permission change: {error}")
                    })?;
                if updated.extension_id != expected_extension_id {
                    let _ = core.child.kill();
                    return Err(
                        "the installed extension changed during permission confirmation".into(),
                    );
                }
                Ok(trusted_window::TrustedWindowReply::State {
                    core_generation: generation,
                    installed: Some(updated),
                })
            }
            trusted_window::TrustedWindowRequest::InspectEphemeral {
                expected_core_generation,
                expected_extension_id,
                capability,
                tab_id,
            } => {
                trusted_window::validate_ephemeral_target(
                    generation,
                    installed.as_ref(),
                    expected_core_generation,
                    &expected_extension_id,
                    &capability,
                    tab_id,
                )
                .map_err(str::to_string)?;
                let (document_epoch, url) = core
                    .inspect_document(tab_id)
                    .map_err(|error| format!("the live document is unavailable: {error}"))?;
                let url = url
                    .filter(|url| url.starts_with("http://") || url.starts_with("https://"))
                    .ok_or_else(|| "one-shot DOM reads require a live HTTP(S) page".to_string())?;
                Ok(trusted_window::TrustedWindowReply::EphemeralReview {
                    core_generation: generation,
                    installed: installed
                        .expect("a validated one-shot target has an installed package"),
                    capability,
                    tab_id,
                    document_epoch,
                    url,
                })
            }
            trusted_window::TrustedWindowRequest::ArmEphemeral {
                expected_core_generation,
                expected_extension_id,
                capability,
                tab_id,
                document_epoch,
            } => {
                trusted_window::validate_ephemeral_target(
                    generation,
                    installed.as_ref(),
                    expected_core_generation,
                    &expected_extension_id,
                    &capability,
                    tab_id,
                )
                .map_err(str::to_string)?;
                let (current_epoch, url) = core
                    .inspect_document(tab_id)
                    .map_err(|error| format!("the live document is unavailable: {error}"))?;
                if current_epoch != document_epoch
                    || !url.as_deref().is_some_and(|url| {
                        url.starts_with("http://") || url.starts_with("https://")
                    })
                {
                    return Err("the reviewed HTTP(S) document changed before confirmation".into());
                }
                core.arm_ephemeral(&capability, tab_id, document_epoch)
                    .map_err(|error| {
                        format!("the active core could not confirm the one-shot read: {error}")
                    })??;
                Ok(trusted_window::TrustedWindowReply::EphemeralArmed {
                    core_generation: generation,
                    installed: installed
                        .expect("a validated one-shot target has an installed package"),
                    capability,
                    tab_id,
                    document_epoch,
                })
            }
            // Answered before this handler (`handle_trusted_window_session_request`).
            trusted_window::TrustedWindowRequest::InspectAssistantSettings
            | trusted_window::TrustedWindowRequest::ApproveAssistantProposal { .. }
            | trusted_window::TrustedWindowRequest::DenyAssistantProposal { .. }
            | trusted_window::TrustedWindowRequest::EditAssistantSettings { .. } => {
                Err("assistant settings are not handled here".to_string())
            }
        }
    })();
    match result {
        Ok(reply) => reply,
        Err(reason) => trusted_window::TrustedWindowReply::Rejected { reason },
    }
}
