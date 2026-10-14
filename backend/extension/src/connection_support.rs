// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

/// Connection-scoped identity plus the subset of capability declarations
/// whose API version was successfully negotiated. The grant lookup stays
/// in [`ExtensionRegistry`], so the extension cannot manufacture either
/// half of an authorization decision in a later request.
pub(super) struct ConnectionIdentity {
    pub(super) extension_id: String,
    pub(super) negotiated_capabilities: BTreeMap<String, u32>,
}

pub(super) fn negotiate_hello(
    registry: &ExtensionRegistry,
    extension_id: String,
    capability_versions: BTreeMap<String, u32>,
) -> (ConnectionIdentity, ExtensionReply) {
    let unsupported_capabilities = registry.unsupported_capability_versions(&capability_versions);
    let negotiated_capabilities = capability_versions
        .into_iter()
        .filter(|(capability, _)| !unsupported_capabilities.contains_key(capability))
        .collect();

    (
        ConnectionIdentity {
            extension_id,
            negotiated_capabilities,
        },
        ExtensionReply::HelloAck {
            unsupported_capabilities,
        },
    )
}

pub(super) fn capability_denial_reason(
    registry: &ExtensionRegistry,
    identity: &ConnectionIdentity,
    capability: &str,
    minimum_version: u32,
) -> Option<String> {
    let Some(version) = identity.negotiated_capabilities.get(capability) else {
        return Some(format!(
            "{} did not negotiate a supported version of {capability}",
            identity.extension_id
        ));
    };
    if *version < minimum_version {
        return Some(format!(
            "{} negotiated {capability} version {version}, but this request requires version {minimum_version}",
            identity.extension_id
        ));
    }
    if !registry.has_capability(&identity.extension_id, capability) {
        return Some(format!(
            "{} is not granted {capability}",
            identity.extension_id
        ));
    }
    None
}

pub(super) fn network_registration_generation(
    registry: &ExtensionRegistry,
    identity: &ConnectionIdentity,
    minimum_version: u32,
) -> Result<u64, String> {
    if let Some(reason) = capability_denial_reason(
        registry,
        identity,
        CAPABILITY_NETWORK_INTERCEPT,
        minimum_version,
    ) {
        return Err(reason);
    }
    registry
        .capability_generation(&identity.extension_id, CAPABILITY_NETWORK_INTERCEPT)
        .ok_or_else(|| {
            format!(
                "{} is no longer granted {CAPABILITY_NETWORK_INTERCEPT}",
                identity.extension_id
            )
        })
}

pub(super) fn commit_dom_write<W>(
    registry: &ExtensionRegistry,
    identity: &ConnectionIdentity,
    captured_generation: Option<u64>,
    write_dom: &mut W,
    target: Option<(u64, u64)>,
    value: String,
    kind: &blueice_ipc::extension::DomWriteTarget,
) -> Result<(), String>
where
    W: FnMut(
        Option<(u64, u64)>,
        String,
        &blueice_ipc::extension::DomWriteTarget,
        u64,
    ) -> Result<(), String>,
{
    let generation =
        captured_generation.ok_or_else(|| grant_changed_reason(CAPABILITY_DOM_WRITE))?;
    if registry.capability_generation(&identity.extension_id, CAPABILITY_DOM_WRITE)
        != Some(generation)
    {
        return Err(grant_changed_reason(CAPABILITY_DOM_WRITE));
    }
    // The core-backed delegate carries this original generation to the
    // session's final guarded mutation. A host-side check alone would not
    // protect a request left in the session queue after a timeout.
    write_dom(target, value, kind, generation)
}

pub(super) fn write_dom_effect_reply<S: Write>(
    stream: &mut S,
    result: Result<(), String>,
) -> io::Result<()> {
    match result {
        Ok(()) => write_extension_reply(stream, &ExtensionReply::DomWriteAck),
        Err(reason) if reason == grant_changed_reason(CAPABILITY_DOM_WRITE) => {
            write_extension_reply(
                stream,
                &ExtensionReply::CapabilityDenied {
                    capability: CAPABILITY_DOM_WRITE.to_string(),
                    reason,
                },
            )
        }
        Err(reason) => write_extension_reply(
            stream,
            &ExtensionReply::OperationUnavailable {
                capability: CAPABILITY_DOM_WRITE.to_string(),
                reason,
            },
        ),
    }
}

#[allow(clippy::result_large_err)]
pub(super) fn with_stable_storage_grant<T>(
    registry: &ExtensionRegistry,
    identity: &ConnectionIdentity,
    captured_generation: Option<u64>,
    effect: impl FnOnce() -> Result<T, String>,
) -> Result<T, ExtensionReply> {
    let Some(generation) = captured_generation else {
        return Err(ExtensionReply::CapabilityDenied {
            capability: CAPABILITY_STORAGE.to_string(),
            reason: grant_changed_reason(CAPABILITY_STORAGE),
        });
    };
    match registry.with_stable_capability(
        &identity.extension_id,
        CAPABILITY_STORAGE,
        generation,
        effect,
    ) {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(reason)) => Err(ExtensionReply::OperationUnavailable {
            capability: CAPABILITY_STORAGE.to_string(),
            reason,
        }),
        Err(reason) => Err(ExtensionReply::CapabilityDenied {
            capability: CAPABILITY_STORAGE.to_string(),
            reason,
        }),
    }
}

/// Keeps an optional read's original grant live through delivery, not only
/// through the core lookup: a completed revoke must not be followed by a
/// reply containing data from that old generation.
pub(super) fn write_stable_read_reply<S: Write>(
    stream: &mut S,
    registry: &ExtensionRegistry,
    identity: &ConnectionIdentity,
    capability: &str,
    captured_generation: Option<u64>,
    effect: impl FnOnce() -> ExtensionReply,
) -> io::Result<()> {
    let Some(generation) = captured_generation else {
        return write_extension_reply(
            stream,
            &ExtensionReply::CapabilityDenied {
                capability: capability.to_string(),
                reason: grant_changed_reason(capability),
            },
        );
    };
    match registry.with_stable_capability(&identity.extension_id, capability, generation, || {
        let reply = effect();
        write_extension_reply(stream, &reply)
    }) {
        Ok(result) => result,
        Err(reason) => write_extension_reply(
            stream,
            &ExtensionReply::CapabilityDenied {
                capability: capability.to_string(),
                reason,
            },
        ),
    }
}

pub(super) fn write_network_registration_reply<S: Write>(
    stream: &mut S,
    result: Result<(), String>,
) -> io::Result<()> {
    match result {
        Ok(()) => write_extension_reply(stream, &ExtensionReply::NetworkInterceptAck),
        Err(reason) if reason == grant_changed_reason(CAPABILITY_NETWORK_INTERCEPT) => {
            write_extension_reply(
                stream,
                &ExtensionReply::CapabilityDenied {
                    capability: CAPABILITY_NETWORK_INTERCEPT.to_string(),
                    reason,
                },
            )
        }
        Err(reason) => write_extension_reply(
            stream,
            &ExtensionReply::OperationUnavailable {
                capability: CAPABILITY_NETWORK_INTERCEPT.to_string(),
                reason,
            },
        ),
    }
}

#[cfg(unix)]
pub(super) fn check_extension_action(
    gatekeeper_socket: &Path,
    extension_id: &str,
    capability: &str,
    detail: String,
) -> Result<GatekeeperReply, String> {
    let mut stream = UnixStream::connect(gatekeeper_socket)
        .map_err(|error| format!("could not connect to the gatekeeper: {error}"))?;
    stream
        .set_read_timeout(Some(GATEKEEPER_CHECK_TIMEOUT))
        .map_err(|error| format!("could not configure the gatekeeper read deadline: {error}"))?;
    stream
        .set_write_timeout(Some(GATEKEEPER_CHECK_TIMEOUT))
        .map_err(|error| format!("could not configure the gatekeeper write deadline: {error}"))?;
    write_gatekeeper_request(
        &mut stream,
        &GatekeeperRequest::CheckExtensionAction {
            extension_id: extension_id.to_string(),
            capability: capability.to_string(),
            detail,
        },
    )
    .map_err(|error| format!("could not send the extension action for review: {error}"))?;
    read_gatekeeper_reply(&mut stream)
        .map_err(|error| format!("could not read the gatekeeper decision: {error}"))
}

/// Turns one wire handshake into its server-side identity claim only if it is
/// acceptable for this connection mode. The plain hello remains valid for an
/// explicitly unauthenticated development server, while a core-spawned host
/// must prove the secret on every identity-changing hello.
pub(super) fn authenticated_hello(
    expected_authentication: Option<&str>,
    request: ExtensionRequest,
) -> Option<(String, BTreeMap<String, u32>)> {
    match request {
        ExtensionRequest::Hello {
            extension_id,
            capability_versions,
        } if expected_authentication.is_none() => Some((extension_id, capability_versions)),
        ExtensionRequest::HelloAuthenticated {
            extension_id,
            capability_versions,
            authentication,
        } if expected_authentication.is_none_or(|expected| {
            constant_time_authentication_matches(expected, &authentication)
        }) =>
        {
            Some((extension_id, capability_versions))
        }
        _ => None,
    }
}

/// Compares a supplied credential without exiting early on its contents. The
/// generated core credential has a fixed ASCII length, and this helper also
/// mixes a length mismatch into the result instead of indexing beyond the
/// supplied buffer.
pub(super) fn constant_time_authentication_matches(expected: &str, supplied: &str) -> bool {
    let expected = expected.as_bytes();
    let supplied = supplied.as_bytes();
    let mut difference = u8::from(expected.len() != supplied.len());
    for (index, expected_byte) in expected.iter().enumerate() {
        difference |= expected_byte ^ supplied.get(index).copied().unwrap_or_default();
    }
    difference == 0
}

#[cfg(not(unix))]
pub(super) fn check_extension_action(
    _socket: &Path,
    _id: &str,
    _capability: &str,
    _detail: String,
) -> Result<GatekeeperReply, String> {
    Err("the gatekeeper transport is unavailable on this platform".into())
}
