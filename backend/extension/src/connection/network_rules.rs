// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

pub(super) fn handle<S, R, W, N, B, C>(
    context: RequestContext<'_>,
    stream: &mut S,
    request: ExtensionRequest,
    delegates: &mut ExtensionActionDelegates<R, W, N, B, C>,
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
    let ExtensionActionDelegates {
        register_network_intercept,
        register_network_block_url,
        register_network_block_host,
        register_network_block_path_prefix,
        register_network_redirect_url,
        clear_network_block_urls,
        ..
    } = delegates;
    match request {
        ExtensionRequest::RegisterNetworkBlockUrl { url } => {
            let grant_generation = match network_registration_generation(registry, identity, 2) {
                Ok(generation) => generation,
                Err(reason) => {
                    write_extension_reply(
                        stream,
                        &ExtensionReply::CapabilityDenied {
                            capability: CAPABILITY_NETWORK_INTERCEPT.to_string(),
                            reason,
                        },
                    )?;
                    return Ok(());
                }
            };
            if url.len() > blueice_ipc::extension::MAX_NETWORK_BLOCK_URL_BYTES {
                write_extension_reply(
                    stream,
                    &ExtensionReply::OperationUnavailable {
                        capability: CAPABILITY_NETWORK_INTERCEPT.to_string(),
                        reason: format!(
                            "exact navigation-block URLs cannot exceed {} bytes",
                            blueice_ipc::extension::MAX_NETWORK_BLOCK_URL_BYTES
                        ),
                    },
                )?;
                return Ok(());
            }
            // The reviewer receives a fixed operation class rather than
            // the extension-controlled URL. The URL is parsed and
            // canonicalized only in the core-owned rule store.
            match check_extension_action(
                gatekeeper_socket,
                &identity.extension_id,
                CAPABILITY_NETWORK_INTERCEPT,
                "action=register-exact-navigation-block".to_string(),
            ) {
                Ok(GatekeeperReply::Cleared)
                    if registry.capability_generation(
                        &identity.extension_id,
                        CAPABILITY_NETWORK_INTERCEPT,
                    ) != Some(grant_generation) =>
                {
                    write_extension_reply(
                        stream,
                        &ExtensionReply::CapabilityDenied {
                            capability: CAPABILITY_NETWORK_INTERCEPT.to_string(),
                            reason: grant_changed_reason(CAPABILITY_NETWORK_INTERCEPT),
                        },
                    )?
                }
                Ok(GatekeeperReply::Cleared) => write_network_registration_reply(
                    stream,
                    register_network_block_url(url, grant_generation),
                )?,
                Ok(GatekeeperReply::Rejected { reason, category }) => {
                    write_extension_reply(
                        stream,
                        &ExtensionReply::GatekeeperBlocked {
                            capability: CAPABILITY_NETWORK_INTERCEPT.to_string(),
                            reason,
                            category,
                        },
                    )?;
                }
                Err(reason) => {
                    write_extension_reply(
                        stream,
                        &ExtensionReply::GatekeeperBlocked {
                            capability: CAPABILITY_NETWORK_INTERCEPT.to_string(),
                            reason,
                            category: "gatekeeper-unavailable".to_string(),
                        },
                    )?;
                }
            }
        }
        ExtensionRequest::RegisterNetworkBlockHost { host } => {
            let grant_generation = match network_registration_generation(registry, identity, 4) {
                Ok(generation) => generation,
                Err(reason) => {
                    write_extension_reply(
                        stream,
                        &ExtensionReply::CapabilityDenied {
                            capability: CAPABILITY_NETWORK_INTERCEPT.to_string(),
                            reason,
                        },
                    )?;
                    return Ok(());
                }
            };
            if host.len() > blueice_ipc::extension::MAX_NETWORK_BLOCK_HOST_BYTES {
                write_extension_reply(
                    stream,
                    &ExtensionReply::OperationUnavailable {
                        capability: CAPABILITY_NETWORK_INTERCEPT.to_string(),
                        reason: format!(
                            "navigation-block hosts cannot exceed {} bytes",
                            blueice_ipc::extension::MAX_NETWORK_BLOCK_HOST_BYTES
                        ),
                    },
                )?;
                return Ok(());
            }
            // The reviewer sees the operation class, never a guest-
            // controlled host. Core independently validates and stores
            // the ASCII host only after the review clears.
            match check_extension_action(
                gatekeeper_socket,
                &identity.extension_id,
                CAPABILITY_NETWORK_INTERCEPT,
                "action=register-host-navigation-block".to_string(),
            ) {
                Ok(GatekeeperReply::Cleared)
                    if registry.capability_generation(
                        &identity.extension_id,
                        CAPABILITY_NETWORK_INTERCEPT,
                    ) != Some(grant_generation) =>
                {
                    write_extension_reply(
                        stream,
                        &ExtensionReply::CapabilityDenied {
                            capability: CAPABILITY_NETWORK_INTERCEPT.to_string(),
                            reason: grant_changed_reason(CAPABILITY_NETWORK_INTERCEPT),
                        },
                    )?
                }
                Ok(GatekeeperReply::Cleared) => write_network_registration_reply(
                    stream,
                    register_network_block_host(host, grant_generation),
                )?,
                Ok(GatekeeperReply::Rejected { reason, category }) => {
                    write_extension_reply(
                        stream,
                        &ExtensionReply::GatekeeperBlocked {
                            capability: CAPABILITY_NETWORK_INTERCEPT.to_string(),
                            reason,
                            category,
                        },
                    )?;
                }
                Err(reason) => {
                    write_extension_reply(
                        stream,
                        &ExtensionReply::GatekeeperBlocked {
                            capability: CAPABILITY_NETWORK_INTERCEPT.to_string(),
                            reason,
                            category: "gatekeeper-unavailable".to_string(),
                        },
                    )?;
                }
            }
        }
        ExtensionRequest::RegisterNetworkBlockPathPrefix { host, path_prefix } => {
            let grant_generation = match network_registration_generation(registry, identity, 5) {
                Ok(generation) => generation,
                Err(reason) => {
                    write_extension_reply(
                        stream,
                        &ExtensionReply::CapabilityDenied {
                            capability: CAPABILITY_NETWORK_INTERCEPT.to_string(),
                            reason,
                        },
                    )?;
                    return Ok(());
                }
            };
            if host.len() > blueice_ipc::extension::MAX_NETWORK_BLOCK_HOST_BYTES
                || path_prefix.len() > blueice_ipc::extension::MAX_NETWORK_BLOCK_PATH_BYTES
            {
                write_extension_reply(
                    stream,
                    &ExtensionReply::OperationUnavailable {
                        capability: CAPABILITY_NETWORK_INTERCEPT.to_string(),
                        reason: "navigation-block host or path prefix exceeds its protocol bound"
                            .to_string(),
                    },
                )?;
                return Ok(());
            }
            // Only the fixed action class reaches the gatekeeper. The
            // guest-controlled host/path are validated again by core,
            // after review and before a rule can become active.
            match check_extension_action(
                gatekeeper_socket,
                &identity.extension_id,
                CAPABILITY_NETWORK_INTERCEPT,
                "action=register-path-prefix-navigation-block".to_string(),
            ) {
                Ok(GatekeeperReply::Cleared)
                    if registry.capability_generation(
                        &identity.extension_id,
                        CAPABILITY_NETWORK_INTERCEPT,
                    ) != Some(grant_generation) =>
                {
                    write_extension_reply(
                        stream,
                        &ExtensionReply::CapabilityDenied {
                            capability: CAPABILITY_NETWORK_INTERCEPT.to_string(),
                            reason: grant_changed_reason(CAPABILITY_NETWORK_INTERCEPT),
                        },
                    )?
                }
                Ok(GatekeeperReply::Cleared) => {
                    write_network_registration_reply(
                        stream,
                        register_network_block_path_prefix(host, path_prefix, grant_generation),
                    )?;
                }
                Ok(GatekeeperReply::Rejected { reason, category }) => {
                    write_extension_reply(
                        stream,
                        &ExtensionReply::GatekeeperBlocked {
                            capability: CAPABILITY_NETWORK_INTERCEPT.to_string(),
                            reason,
                            category,
                        },
                    )?;
                }
                Err(reason) => {
                    write_extension_reply(
                        stream,
                        &ExtensionReply::GatekeeperBlocked {
                            capability: CAPABILITY_NETWORK_INTERCEPT.to_string(),
                            reason,
                            category: "gatekeeper-unavailable".to_string(),
                        },
                    )?;
                }
            }
        }
        ExtensionRequest::RegisterNetworkRedirectUrl {
            source_url,
            target_url,
        } => {
            let grant_generation = match network_registration_generation(registry, identity, 6) {
                Ok(generation) => generation,
                Err(reason) => {
                    write_extension_reply(
                        stream,
                        &ExtensionReply::CapabilityDenied {
                            capability: CAPABILITY_NETWORK_INTERCEPT.to_string(),
                            reason,
                        },
                    )?;
                    return Ok(());
                }
            };
            if source_url.len() > blueice_ipc::extension::MAX_NETWORK_BLOCK_URL_BYTES
                || target_url.len() > blueice_ipc::extension::MAX_NETWORK_BLOCK_URL_BYTES
            {
                write_extension_reply(
                    stream,
                    &ExtensionReply::OperationUnavailable {
                        capability: CAPABILITY_NETWORK_INTERCEPT.to_string(),
                        reason: "navigation redirect URLs exceed the protocol bound".to_string(),
                    },
                )?;
                return Ok(());
            }
            // Only a fixed action label reaches the reviewer. Core later
            // validates both exact URLs and their shared origin; every
            // actual target still receives normal CheckUrl review.
            match check_extension_action(
                gatekeeper_socket,
                &identity.extension_id,
                CAPABILITY_NETWORK_INTERCEPT,
                "action=register-same-origin-navigation-redirect".to_string(),
            ) {
                Ok(GatekeeperReply::Cleared)
                    if registry.capability_generation(
                        &identity.extension_id,
                        CAPABILITY_NETWORK_INTERCEPT,
                    ) != Some(grant_generation) =>
                {
                    write_extension_reply(
                        stream,
                        &ExtensionReply::CapabilityDenied {
                            capability: CAPABILITY_NETWORK_INTERCEPT.to_string(),
                            reason: grant_changed_reason(CAPABILITY_NETWORK_INTERCEPT),
                        },
                    )?
                }
                Ok(GatekeeperReply::Cleared) => {
                    write_network_registration_reply(
                        stream,
                        register_network_redirect_url(source_url, target_url, grant_generation),
                    )?;
                }
                Ok(GatekeeperReply::Rejected { reason, category }) => {
                    write_extension_reply(
                        stream,
                        &ExtensionReply::GatekeeperBlocked {
                            capability: CAPABILITY_NETWORK_INTERCEPT.to_string(),
                            reason,
                            category,
                        },
                    )?;
                }
                Err(reason) => {
                    write_extension_reply(
                        stream,
                        &ExtensionReply::GatekeeperBlocked {
                            capability: CAPABILITY_NETWORK_INTERCEPT.to_string(),
                            reason,
                            category: "gatekeeper-unavailable".to_string(),
                        },
                    )?;
                }
            }
        }
        ExtensionRequest::ClearNetworkBlockUrls => {
            if let Some(reason) =
                capability_denial_reason(registry, identity, CAPABILITY_NETWORK_INTERCEPT, 3)
            {
                write_extension_reply(
                    stream,
                    &ExtensionReply::CapabilityDenied {
                        capability: CAPABILITY_NETWORK_INTERCEPT.to_string(),
                        reason,
                    },
                )?;
                return Ok(());
            }
            // A connection may only clear its own opaque rule bucket.
            // This carries no URL/body/header payload; any later
            // navigation, including one no longer rewritten, still
            // receives the ordinary mandatory URL/content reviews.
            match clear_network_block_urls() {
                Ok(()) => write_extension_reply(stream, &ExtensionReply::NetworkInterceptAck)?,
                Err(reason) => write_extension_reply(
                    stream,
                    &ExtensionReply::OperationUnavailable {
                        capability: CAPABILITY_NETWORK_INTERCEPT.to_string(),
                        reason,
                    },
                )?,
            }
        }
        ExtensionRequest::NetworkIntercept => {
            if let Some(reason) =
                capability_denial_reason(registry, identity, CAPABILITY_NETWORK_INTERCEPT, 1)
            {
                write_extension_reply(
                    stream,
                    &ExtensionReply::CapabilityDenied {
                        capability: CAPABILITY_NETWORK_INTERCEPT.to_string(),
                        reason,
                    },
                )?;
            } else {
                // Legacy v1 deliberately has no extension-defined rule.
                // Fixed metadata proves its review boundary without
                // opening an unbounded extension-to-reviewer text path.
                match check_extension_action(
                    gatekeeper_socket,
                    &identity.extension_id,
                    CAPABILITY_NETWORK_INTERCEPT,
                    "action=register-intercept".to_string(),
                ) {
                    Ok(GatekeeperReply::Cleared) => match register_network_intercept() {
                        Ok(()) => {
                            write_extension_reply(stream, &ExtensionReply::NetworkInterceptAck)?
                        }
                        Err(reason) => write_extension_reply(
                            stream,
                            &ExtensionReply::OperationUnavailable {
                                capability: CAPABILITY_NETWORK_INTERCEPT.to_string(),
                                reason,
                            },
                        )?,
                    },
                    Ok(GatekeeperReply::Rejected { reason, category }) => {
                        write_extension_reply(
                            stream,
                            &ExtensionReply::GatekeeperBlocked {
                                capability: CAPABILITY_NETWORK_INTERCEPT.to_string(),
                                reason,
                                category,
                            },
                        )?;
                    }
                    Err(reason) => {
                        write_extension_reply(
                            stream,
                            &ExtensionReply::GatekeeperBlocked {
                                capability: CAPABILITY_NETWORK_INTERCEPT.to_string(),
                                reason,
                                category: "gatekeeper-unavailable".to_string(),
                            },
                        )?;
                    }
                }
            }
        }
        _ => unreachable!("request belongs to another extension domain"),
    }
    Ok(())
}
