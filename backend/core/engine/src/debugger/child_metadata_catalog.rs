// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

pub(super) fn describe_child_location_capabilities(
    tabs: &TabManager,
    locations: &mut dyn PageJavaScriptDebuggerLocations,
    metadata_session: Option<&DebuggerMetadataSessionAuthorization>,
    realm: DebuggerPageRealm,
) -> DebuggerReply {
    if let Err(reply) = resolve_live_realm(tabs, realm) {
        return *reply;
    }
    // `Available` is stronger than "core has a similarly numbered page":
    // the authenticated child must acknowledge this exact live realm on its
    // own private socket at this session boundary.
    let locations_available =
        locations.debugger_has_live_realm(TabId::from_u64(realm.tab_id), realm.realm_generation);
    let max_safe_points_per_program = if locations_available {
        locations.max_debugger_safe_points_per_program()
    } else {
        DEFAULT_MAX_SAFE_POINTS_PER_PROGRAM
    };
    let breakpoint_configuration_available =
        locations_available && locations.debugger_breakpoint_configuration_available();
    let execution_control_available =
        locations_available && locations.debugger_execution_control_available();
    let stack_available = execution_control_available && locations.debugger_stack_available();
    let scopes_available = execution_control_available && locations.debugger_scopes_available();
    let values_available = scopes_available
        && metadata_session.is_some_and(|session| session.permits_bounded_values())
        && locations.debugger_values_available();
    // Do not advertise an installed private inventory to a peer that has no
    // negotiated grant. This makes the public capability view fail closed as
    // well as the eventual operation; a successful `Hello` still needs this
    // exact live realm to supply the inventory.
    let static_metadata_inventory_available = locations_available
        && metadata_session
            .is_some_and(|session| session.permits(DebuggerMetadataCapability::OpaqueInventory))
        && locations.debugger_static_metadata_inventory_available();
    // A summary has no target without a successful inventory request, so it
    // additionally requires that same session grant. Do not publish either
    // availability bit to an ungranted peer.
    let static_metadata_summary_available = static_metadata_inventory_available
        && metadata_session
            .is_some_and(|session| session.permits(DebuggerMetadataCapability::OpaqueSummary))
        && locations.debugger_static_metadata_summary_available();
    let static_metadata_lowering_summary_available = static_metadata_inventory_available
        && metadata_session.is_some_and(|session| {
            session.permits(DebuggerMetadataCapability::OpaqueLoweringSummary)
        })
        && locations.debugger_static_metadata_lowering_summary_available();
    let static_metadata_source_inventory_available = static_metadata_inventory_available
        && metadata_session.is_some_and(|session| {
            session.permits(DebuggerMetadataCapability::OpaqueSourceInventory)
        })
        && locations.debugger_static_metadata_source_inventory_available();
    let static_metadata_source_provenance_available = static_metadata_source_inventory_available
        && metadata_session.is_some_and(|session| {
            session.permits(DebuggerMetadataCapability::OpaqueSourceProvenance)
        })
        && locations.debugger_static_metadata_source_provenance_available();
    let static_metadata_type_inventory_available = static_metadata_inventory_available
        && metadata_session.is_some_and(|session| {
            session.permits(DebuggerMetadataCapability::OpaqueTypeInventory)
        })
        && locations.debugger_static_metadata_type_inventory_available();
    let static_metadata_type_display_available = static_metadata_type_inventory_available
        && metadata_session
            .is_some_and(|session| session.permits(DebuggerMetadataCapability::OpaqueTypeDisplay))
        && locations.debugger_static_metadata_type_display_available();
    let static_metadata_symbol_inventory_available = static_metadata_inventory_available
        && metadata_session.is_some_and(|session| {
            session.permits(DebuggerMetadataCapability::OpaqueSymbolInventory)
        })
        && locations.debugger_static_metadata_symbol_inventory_available();
    let static_metadata_contract_inventory_available = static_metadata_inventory_available
        && metadata_session.is_some_and(|session| {
            session.permits(DebuggerMetadataCapability::OpaqueContractInventory)
        })
        && locations.debugger_static_metadata_contract_inventory_available();
    let static_metadata_contract_display_available = static_metadata_contract_inventory_available
        && metadata_session.is_some_and(|session| {
            session.permits(DebuggerMetadataCapability::OpaqueContractDisplay)
        })
        && locations.debugger_static_metadata_contract_display_available();
    let static_metadata_contract_validation_available = static_metadata_contract_inventory_available
        && metadata_session.is_some_and(|session| {
            session.permits(DebuggerMetadataCapability::OpaqueContractValidation)
        })
        && locations.debugger_static_metadata_contract_validation_available();
    let static_metadata_symbol_display_available = static_metadata_symbol_inventory_available
        && metadata_session.is_some_and(|session| {
            session.permits(DebuggerMetadataCapability::OpaqueSymbolDisplay)
        })
        && locations.debugger_static_metadata_symbol_display_available();
    let static_metadata_symbol_location_available = static_metadata_source_inventory_available
        && static_metadata_symbol_inventory_available
        && metadata_session.is_some_and(|session| {
            session.permits(DebuggerMetadataCapability::OpaqueSymbolLocation)
        })
        && locations.debugger_static_metadata_symbol_location_available();
    let static_metadata_safe_point_span_available = static_metadata_source_inventory_available
        && metadata_session.is_some_and(|session| {
            session.permits(DebuggerMetadataCapability::OpaqueSafePointSpan)
        })
        && locations.debugger_static_metadata_safe_point_span_available();
    let static_metadata_source_span_step_available = static_metadata_safe_point_span_available
        && execution_control_available
        && metadata_session.is_some_and(|session| {
            session.permits(DebuggerMetadataCapability::OpaqueSourceSpanStep)
        })
        && locations.debugger_source_span_stepping_available();
    let static_metadata_source_breakpoint_available = static_metadata_source_inventory_available
        && metadata_session.is_some_and(|session| {
            session.permits(DebuggerMetadataCapability::OpaqueSourceBreakpoint)
        })
        && locations.debugger_static_metadata_source_breakpoint_available();
    let static_metadata_contract_location_available = static_metadata_source_inventory_available
        && static_metadata_contract_inventory_available
        && metadata_session.is_some_and(|session| {
            session.permits(DebuggerMetadataCapability::OpaqueContractLocation)
        })
        && locations.debugger_static_metadata_contract_location_available();
    let static_metadata_symbol_type_available = static_metadata_type_inventory_available
        && static_metadata_symbol_inventory_available
        && metadata_session
            .is_some_and(|session| session.permits(DebuggerMetadataCapability::OpaqueSymbolType))
        && locations.debugger_static_metadata_symbol_type_available();
    let static_scope_relation_available = scopes_available
        && static_metadata_type_inventory_available
        && static_metadata_symbol_inventory_available
        && metadata_session.is_some_and(|session| {
            session.permits(DebuggerMetadataCapability::OpaqueStaticScopeRelation)
        });
    let static_metadata_symbol_contract_available = static_metadata_contract_inventory_available
        && static_metadata_symbol_inventory_available
        && metadata_session.is_some_and(|session| {
            session.permits(DebuggerMetadataCapability::OpaqueSymbolContract)
        })
        && locations.debugger_static_metadata_symbol_contract_available();
    let max_breakpoints_per_realm = if breakpoint_configuration_available {
        locations.max_debugger_breakpoints_per_realm()
    } else {
        DEFAULT_MAX_BREAKPOINTS_PER_REALM
    };
    DebuggerReply::Capabilities(DebuggerCapabilities {
        protocol_version: DEBUGGER_PROTOCOL_VERSION,
        realm,
        reports: capability_reports(DebuggerCapabilityAvailability {
            program_locations_available: locations_available,
            breakpoint_configuration_available,
            entry_execution_control_available: execution_control_available,
            stepping_available: execution_control_available
                && locations.debugger_stepping_available(),
            nested_frames_available: execution_control_available
                && locations.debugger_nested_frames_available(),
            linked_modules_available: execution_control_available
                && locations.debugger_linked_frames_available(),
            stack_available,
            scopes_available,
            values_available,
            static_metadata_inventory_available,
            static_metadata_summary_available,
            static_metadata_lowering_summary_available,
            static_metadata_source_inventory_available,
            static_metadata_source_provenance_available,
            static_metadata_type_inventory_available,
            static_metadata_type_display_available,
            static_metadata_symbol_inventory_available,
            static_metadata_contract_inventory_available,
            static_metadata_contract_display_available,
            static_metadata_contract_validation_available,
            static_metadata_symbol_display_available,
            static_metadata_symbol_location_available,
            static_metadata_safe_point_span_available,
            static_metadata_source_span_step_available,
            static_metadata_source_breakpoint_available,
            static_metadata_contract_location_available,
            static_metadata_symbol_type_available,
            static_metadata_symbol_contract_available,
            static_scope_relation_available,
        }),
        max_stack_frames: MAX_STACK_FRAMES,
        max_scope_bindings: MAX_SCOPE_BINDINGS,
        max_value_preview_bytes: MAX_VALUE_PREVIEW_BYTES,
        max_safe_points_per_program: u32::try_from(max_safe_points_per_program)
            .expect("native debugger safe-point reply cap fits the wire type"),
        max_breakpoints_per_realm: u32::try_from(max_breakpoints_per_realm)
            .expect("child debugger breakpoint reply cap fits the wire type"),
    })
}

pub(super) fn list_child_static_metadata(
    tabs: &TabManager,
    locations: &mut dyn PageJavaScriptDebuggerLocations,
    metadata_session: Option<&DebuggerMetadataSessionAuthorization>,
    program: DebuggerProgram,
) -> DebuggerReply {
    if !program.is_well_formed() {
        return DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            message: "invalid debugger program target".to_string(),
        };
    }
    let Some(metadata_session) = metadata_session else {
        return unavailable_static_metadata_inventory();
    };
    let capabilities = describe_child_location_capabilities(
        tabs,
        locations,
        Some(metadata_session),
        program.realm,
    );
    let DebuggerReply::Capabilities(capabilities) = capabilities else {
        return capabilities;
    };
    let Some(authorization) = capabilities.authorize_metadata(
        metadata_session,
        DebuggerMetadataCapability::OpaqueInventory,
    ) else {
        return unavailable_static_metadata_inventory();
    };
    if !authorization.permits(program.realm, DebuggerMetadataCapability::OpaqueInventory) {
        return unavailable_static_metadata_inventory();
    }

    let tab_id = match resolve_live_realm(tabs, program.realm) {
        Ok(tab_id) => tab_id,
        Err(reply) => return *reply,
    };
    match locations.debugger_static_metadata(
        tab_id,
        program.realm.realm_generation,
        program.program_handle,
        program.program_generation,
    ) {
        Ok(metadata) if metadata.len() <= MAX_STATIC_METADATA_HANDLES_PER_PROGRAM => {
            let mut identities = std::collections::BTreeSet::new();
            let mut handles = Vec::with_capacity(metadata.len());
            for metadata in metadata {
                if metadata.metadata_handle == 0
                    || metadata.metadata_generation == 0
                    || !identities.insert((metadata.metadata_handle, metadata.metadata_generation))
                {
                    return DebuggerReply::Error {
                        code: DebuggerErrorCode::InvalidTarget,
                        message: "invalid opaque debugger static metadata inventory".to_string(),
                    };
                }
                handles.push(DebuggerStaticMetadataHandle {
                    program,
                    metadata_handle: metadata.metadata_handle,
                    metadata_generation: metadata.metadata_generation,
                });
            }
            // The inventory is the only operation that can mint a parent
            // handle into this stream's local receipt ledger. Every dependent
            // metadata capability must have this receipt first; none may turn
            // a guessed numeric handle into a child query target.
            if (metadata_session.permits(DebuggerMetadataCapability::OpaqueSummary)
                || metadata_session.permits(DebuggerMetadataCapability::OpaqueSourceInventory)
                || metadata_session.permits(DebuggerMetadataCapability::OpaqueTypeInventory)
                || metadata_session.permits(DebuggerMetadataCapability::OpaqueSymbolInventory)
                || metadata_session.permits(DebuggerMetadataCapability::OpaqueContractInventory)
                || metadata_session.permits(DebuggerMetadataCapability::OpaqueLoweringSummary))
                && !metadata_session.observe_metadata(&handles)
            {
                return DebuggerReply::Error {
                    code: DebuggerErrorCode::ResourceLimit,
                    message: "debugger static metadata receipt budget is exhausted".to_string(),
                };
            }
            DebuggerReply::StaticMetadata(handles)
        }
        Ok(_) => DebuggerReply::Error {
            code: DebuggerErrorCode::ResourceLimit,
            message: "debugger static metadata inventory exceeds its fixed limit".to_string(),
        },
        Err(error) => debugger_program_error(error),
    }
}

pub(super) fn describe_child_static_metadata(
    tabs: &TabManager,
    locations: &mut dyn PageJavaScriptDebuggerLocations,
    metadata_session: Option<&DebuggerMetadataSessionAuthorization>,
    metadata: DebuggerStaticMetadataHandle,
) -> DebuggerReply {
    if !metadata.is_well_formed() {
        return DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            message: "invalid debugger static metadata target".to_string(),
        };
    }
    let Some(metadata_session) = metadata_session else {
        return unavailable_static_metadata_summary();
    };
    // A summary can only be associated with a handle minted by inventory on
    // this same stream. A malformed/partial session or guessed numeric handle
    // can never turn the dependent summary grant into a target-probing
    // capability.
    if !metadata_session.permits(DebuggerMetadataCapability::OpaqueInventory)
        || !metadata_session.observed_metadata(metadata)
    {
        return unavailable_static_metadata_summary();
    }
    let capabilities = describe_child_location_capabilities(
        tabs,
        locations,
        Some(metadata_session),
        metadata.program.realm,
    );
    let DebuggerReply::Capabilities(capabilities) = capabilities else {
        return capabilities;
    };
    let Some(authorization) = capabilities
        .authorize_metadata(metadata_session, DebuggerMetadataCapability::OpaqueSummary)
    else {
        return unavailable_static_metadata_summary();
    };
    if !authorization.permits(
        metadata.program.realm,
        DebuggerMetadataCapability::OpaqueSummary,
    ) {
        return unavailable_static_metadata_summary();
    }

    let tab_id = match resolve_live_realm(tabs, metadata.program.realm) {
        Ok(tab_id) => tab_id,
        Err(reply) => return *reply,
    };
    match locations.debugger_static_metadata_summary(
        tab_id,
        metadata.program.realm.realm_generation,
        metadata.program.program_handle,
        metadata.program.program_generation,
        metadata.metadata_handle,
        metadata.metadata_generation,
    ) {
        Ok(summary) => {
            let summary = DebuggerStaticMetadataSummary {
                metadata,
                language_version: summary.language_version,
                compiler_options_hash: summary.compiler_options_hash,
                source_count: summary.source_count,
                type_count: summary.type_count,
                symbol_count: summary.symbol_count,
                contract_count: summary.contract_count,
            };
            if !summary.is_well_formed() {
                return DebuggerReply::Error {
                    code: DebuggerErrorCode::InvalidTarget,
                    message: "invalid bounded debugger static metadata summary".to_string(),
                };
            }
            DebuggerReply::StaticMetadataSummary(summary)
        }
        Err(error) => debugger_program_error(error),
    }
}

/// Returns only aggregate evidence for an exact prior opaque metadata receipt.
/// This path intentionally has no per-entry source-map, source-span, AST, or
/// bytecode lookup operation; it exposes only the verified direct-map ABI and
/// aggregate count after the complete live tuple has been revalidated.
pub(super) fn describe_child_static_metadata_lowering_summary(
    tabs: &TabManager,
    locations: &mut dyn PageJavaScriptDebuggerLocations,
    metadata_session: Option<&DebuggerMetadataSessionAuthorization>,
    metadata: DebuggerStaticMetadataHandle,
) -> DebuggerReply {
    if !metadata.is_well_formed() {
        return DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            message: "invalid debugger static metadata lowering summary target".to_string(),
        };
    }
    let Some(metadata_session) = metadata_session else {
        return unavailable_static_metadata_lowering_summary();
    };
    if !metadata_session.permits(DebuggerMetadataCapability::OpaqueInventory)
        || !metadata_session.observed_metadata(metadata)
    {
        return unavailable_static_metadata_lowering_summary();
    }
    let capabilities = describe_child_location_capabilities(
        tabs,
        locations,
        Some(metadata_session),
        metadata.program.realm,
    );
    let DebuggerReply::Capabilities(capabilities) = capabilities else {
        return capabilities;
    };
    let Some(authorization) = capabilities.authorize_metadata(
        metadata_session,
        DebuggerMetadataCapability::OpaqueLoweringSummary,
    ) else {
        return unavailable_static_metadata_lowering_summary();
    };
    if !authorization.permits(
        metadata.program.realm,
        DebuggerMetadataCapability::OpaqueLoweringSummary,
    ) {
        return unavailable_static_metadata_lowering_summary();
    }
    let tab_id = match resolve_live_realm(tabs, metadata.program.realm) {
        Ok(tab_id) => tab_id,
        Err(reply) => return *reply,
    };
    match locations.debugger_static_metadata_lowering_summary(
        tab_id,
        metadata.program.realm.realm_generation,
        metadata.program.program_handle,
        metadata.program.program_generation,
        metadata.metadata_handle,
        metadata.metadata_generation,
    ) {
        Ok(summary) => {
            let summary = DebuggerStaticMetadataLoweringSummary {
                metadata,
                safe_point_map_abi: summary.safe_point_map_abi,
                program_abi: summary.program_abi,
                source_set_hash: summary.source_set_hash,
                bound_safe_point_count: summary.bound_safe_point_count,
            };
            if !summary.is_well_formed() {
                return DebuggerReply::Error {
                    code: DebuggerErrorCode::InvalidTarget,
                    message: "invalid debugger static metadata lowering summary".to_string(),
                };
            }
            DebuggerReply::StaticMetadataLoweringSummary(Box::new(summary))
        }
        Err(error) => debugger_program_error(error),
    }
}

pub(super) fn list_child_static_metadata_sources(
    tabs: &TabManager,
    locations: &mut dyn PageJavaScriptDebuggerLocations,
    metadata_session: Option<&DebuggerMetadataSessionAuthorization>,
    metadata: DebuggerStaticMetadataHandle,
) -> DebuggerReply {
    if !metadata.is_well_formed() {
        return DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            message: "invalid debugger static metadata source inventory target".to_string(),
        };
    }
    let Some(metadata_session) = metadata_session else {
        return unavailable_static_metadata_source_inventory();
    };
    // Source inventory has the same parent receipt boundary as summary: it
    // does not admit a caller-invented metadata handle as a source-ID oracle.
    if !metadata_session.permits(DebuggerMetadataCapability::OpaqueInventory)
        || !metadata_session.observed_metadata(metadata)
    {
        return unavailable_static_metadata_source_inventory();
    }
    let capabilities = describe_child_location_capabilities(
        tabs,
        locations,
        Some(metadata_session),
        metadata.program.realm,
    );
    let DebuggerReply::Capabilities(capabilities) = capabilities else {
        // Preserve the same stale/invalid realm outcome as the parent
        // inventory and sibling summary paths. This still occurs before any
        // child source-record access or source-ID disclosure.
        return capabilities;
    };
    let Some(authorization) = capabilities.authorize_metadata(
        metadata_session,
        DebuggerMetadataCapability::OpaqueSourceInventory,
    ) else {
        return unavailable_static_metadata_source_inventory();
    };
    if !authorization.permits(
        metadata.program.realm,
        DebuggerMetadataCapability::OpaqueSourceInventory,
    ) {
        return unavailable_static_metadata_source_inventory();
    }
    let tab_id = match resolve_live_realm(tabs, metadata.program.realm) {
        Ok(tab_id) => tab_id,
        Err(reply) => return *reply,
    };
    match locations.debugger_static_metadata_sources(
        tab_id,
        metadata.program.realm.realm_generation,
        metadata.program.program_handle,
        metadata.program.program_generation,
        metadata.metadata_handle,
        metadata.metadata_generation,
    ) {
        Ok(sources)
            if sources.len() <= usize::try_from(DEBUGGER_STATIC_METADATA_MAX_SOURCES).unwrap() =>
        {
            let mut seen = std::collections::BTreeSet::new();
            let mut result = Vec::with_capacity(sources.len());
            for source in sources {
                if !seen.insert(source.source_id) {
                    return DebuggerReply::Error {
                        code: DebuggerErrorCode::InvalidTarget,
                        message: "duplicate debugger static metadata source identity".to_string(),
                    };
                }
                result.push(DebuggerStaticMetadataSourceId {
                    metadata,
                    source_id: source.source_id,
                });
            }
            // Keep only a bounded local receipt set when an enabled dependent
            // operation can consume source IDs. This preserves default deny
            // for inventory-only sessions while letting symbol-location use
            // the exact same stream-local identity boundary as provenance.
            if (metadata_session.permits(DebuggerMetadataCapability::OpaqueSourceProvenance)
                || metadata_session.permits(DebuggerMetadataCapability::OpaqueSymbolLocation)
                || metadata_session.permits(DebuggerMetadataCapability::OpaqueContractLocation)
                || metadata_session.permits(DebuggerMetadataCapability::OpaqueSafePointSpan)
                || metadata_session.permits(DebuggerMetadataCapability::OpaqueSourceBreakpoint))
                && !metadata_session.observe_sources(&result)
            {
                return DebuggerReply::Error {
                    code: DebuggerErrorCode::ResourceLimit,
                    message: "debugger static metadata source receipt budget is exhausted"
                        .to_string(),
                };
            }
            DebuggerReply::StaticMetadataSources(result)
        }
        Ok(_) => DebuggerReply::Error {
            code: DebuggerErrorCode::ResourceLimit,
            message: "debugger static metadata source inventory exceeds its fixed limit"
                .to_string(),
        },
        Err(error) => debugger_program_error(error),
    }
}

/// Lists compiler-minted type IDs under one opaque metadata parent. The
/// inventory is default-deny and payload-free: it is deliberately not a type
/// display or a static-record dereference operation.
pub(super) fn list_child_static_metadata_types(
    tabs: &TabManager,
    locations: &mut dyn PageJavaScriptDebuggerLocations,
    metadata_session: Option<&DebuggerMetadataSessionAuthorization>,
    metadata: DebuggerStaticMetadataHandle,
) -> DebuggerReply {
    if !metadata.is_well_formed() {
        return DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            message: "invalid debugger static metadata type inventory target".to_string(),
        };
    }
    let Some(metadata_session) = metadata_session else {
        return unavailable_static_metadata_type_inventory();
    };
    // No guessed parent may query a child type table. The public handle must
    // have crossed this exact stream's inventory receipt boundary first.
    if !metadata_session.permits(DebuggerMetadataCapability::OpaqueInventory)
        || !metadata_session.observed_metadata(metadata)
    {
        return unavailable_static_metadata_type_inventory();
    }
    let capabilities = describe_child_location_capabilities(
        tabs,
        locations,
        Some(metadata_session),
        metadata.program.realm,
    );
    let DebuggerReply::Capabilities(capabilities) = capabilities else {
        return capabilities;
    };
    let Some(authorization) = capabilities.authorize_metadata(
        metadata_session,
        DebuggerMetadataCapability::OpaqueTypeInventory,
    ) else {
        return unavailable_static_metadata_type_inventory();
    };
    if !authorization.permits(
        metadata.program.realm,
        DebuggerMetadataCapability::OpaqueTypeInventory,
    ) {
        return unavailable_static_metadata_type_inventory();
    }
    let tab_id = match resolve_live_realm(tabs, metadata.program.realm) {
        Ok(tab_id) => tab_id,
        Err(reply) => return *reply,
    };
    match locations.debugger_static_metadata_types(
        tab_id,
        metadata.program.realm.realm_generation,
        metadata.program.program_handle,
        metadata.program.program_generation,
        metadata.metadata_handle,
        metadata.metadata_generation,
    ) {
        Ok(types)
            if types.len() <= usize::try_from(DEBUGGER_STATIC_METADATA_MAX_TYPES).unwrap() =>
        {
            let mut seen = std::collections::BTreeSet::new();
            let mut result = Vec::with_capacity(types.len());
            for static_type in types {
                if !seen.insert(static_type.type_id) {
                    return DebuggerReply::Error {
                        code: DebuggerErrorCode::InvalidTarget,
                        message: "duplicate debugger static metadata type identity".to_string(),
                    };
                }
                result.push(DebuggerStaticMetadataTypeId {
                    metadata,
                    type_id: static_type.type_id,
                });
            }
            if !metadata_session.observe_types(&result) {
                return DebuggerReply::Error {
                    code: DebuggerErrorCode::ResourceLimit,
                    message: "debugger static metadata type receipt budget is exhausted"
                        .to_string(),
                };
            }
            DebuggerReply::StaticMetadataTypes(result)
        }
        Ok(_) => DebuggerReply::Error {
            code: DebuggerErrorCode::ResourceLimit,
            message: "debugger static metadata type inventory exceeds its fixed limit".to_string(),
        },
        Err(error) => debugger_program_error(error),
    }
}

/// Lists compiler-minted symbol IDs under one opaque metadata parent. This
/// default-deny operation is payload-free and records a same-stream receipt
/// now, so a later symbol detail operation cannot accept a guessed ID.
pub(super) fn list_child_static_metadata_symbols(
    tabs: &TabManager,
    locations: &mut dyn PageJavaScriptDebuggerLocations,
    metadata_session: Option<&DebuggerMetadataSessionAuthorization>,
    metadata: DebuggerStaticMetadataHandle,
) -> DebuggerReply {
    if !metadata.is_well_formed() {
        return DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            message: "invalid debugger static metadata symbol inventory target".to_string(),
        };
    }
    let Some(metadata_session) = metadata_session else {
        return unavailable_static_metadata_symbol_inventory();
    };
    if !metadata_session.permits(DebuggerMetadataCapability::OpaqueInventory)
        || !metadata_session.observed_metadata(metadata)
    {
        return unavailable_static_metadata_symbol_inventory();
    }
    let capabilities = describe_child_location_capabilities(
        tabs,
        locations,
        Some(metadata_session),
        metadata.program.realm,
    );
    let DebuggerReply::Capabilities(capabilities) = capabilities else {
        return capabilities;
    };
    let Some(authorization) = capabilities.authorize_metadata(
        metadata_session,
        DebuggerMetadataCapability::OpaqueSymbolInventory,
    ) else {
        return unavailable_static_metadata_symbol_inventory();
    };
    if !authorization.permits(
        metadata.program.realm,
        DebuggerMetadataCapability::OpaqueSymbolInventory,
    ) {
        return unavailable_static_metadata_symbol_inventory();
    }
    let tab_id = match resolve_live_realm(tabs, metadata.program.realm) {
        Ok(tab_id) => tab_id,
        Err(reply) => return *reply,
    };
    match locations.debugger_static_metadata_symbols(
        tab_id,
        metadata.program.realm.realm_generation,
        metadata.program.program_handle,
        metadata.program.program_generation,
        metadata.metadata_handle,
        metadata.metadata_generation,
    ) {
        Ok(symbols)
            if symbols.len() <= usize::try_from(DEBUGGER_STATIC_METADATA_MAX_SYMBOLS).unwrap() =>
        {
            let mut seen = std::collections::BTreeSet::new();
            let mut result = Vec::with_capacity(symbols.len());
            for symbol in symbols {
                if !seen.insert(symbol.symbol_id) {
                    return DebuggerReply::Error {
                        code: DebuggerErrorCode::InvalidTarget,
                        message: "duplicate debugger static metadata symbol identity".to_string(),
                    };
                }
                result.push(DebuggerStaticMetadataSymbolId {
                    metadata,
                    symbol_id: symbol.symbol_id,
                });
            }
            if !metadata_session.observe_symbols(&result) {
                return DebuggerReply::Error {
                    code: DebuggerErrorCode::ResourceLimit,
                    message: "debugger static metadata symbol receipt budget is exhausted"
                        .to_string(),
                };
            }
            DebuggerReply::StaticMetadataSymbols(result)
        }
        Ok(_) => DebuggerReply::Error {
            code: DebuggerErrorCode::ResourceLimit,
            message: "debugger static metadata symbol inventory exceeds its fixed limit"
                .to_string(),
        },
        Err(error) => debugger_program_error(error),
    }
}

/// Lists compiler-minted contract IDs under one opaque metadata parent. This
/// default-deny operation is payload-free and records a same-stream receipt
/// now, so a later plan or validation operation cannot accept a guessed ID.
pub(super) fn list_child_static_metadata_contracts(
    tabs: &TabManager,
    locations: &mut dyn PageJavaScriptDebuggerLocations,
    metadata_session: Option<&DebuggerMetadataSessionAuthorization>,
    metadata: DebuggerStaticMetadataHandle,
) -> DebuggerReply {
    if !metadata.is_well_formed() {
        return DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            message: "invalid debugger static metadata contract inventory target".to_string(),
        };
    }
    let Some(metadata_session) = metadata_session else {
        return unavailable_static_metadata_contract_inventory();
    };
    if !metadata_session.permits(DebuggerMetadataCapability::OpaqueInventory)
        || !metadata_session.observed_metadata(metadata)
    {
        return unavailable_static_metadata_contract_inventory();
    }
    let capabilities = describe_child_location_capabilities(
        tabs,
        locations,
        Some(metadata_session),
        metadata.program.realm,
    );
    let DebuggerReply::Capabilities(capabilities) = capabilities else {
        return capabilities;
    };
    let Some(authorization) = capabilities.authorize_metadata(
        metadata_session,
        DebuggerMetadataCapability::OpaqueContractInventory,
    ) else {
        return unavailable_static_metadata_contract_inventory();
    };
    if !authorization.permits(
        metadata.program.realm,
        DebuggerMetadataCapability::OpaqueContractInventory,
    ) {
        return unavailable_static_metadata_contract_inventory();
    }
    let tab_id = match resolve_live_realm(tabs, metadata.program.realm) {
        Ok(tab_id) => tab_id,
        Err(reply) => return *reply,
    };
    match locations.debugger_static_metadata_contracts(
        tab_id,
        metadata.program.realm.realm_generation,
        metadata.program.program_handle,
        metadata.program.program_generation,
        metadata.metadata_handle,
        metadata.metadata_generation,
    ) {
        Ok(contracts)
            if contracts.len()
                <= usize::try_from(DEBUGGER_STATIC_METADATA_MAX_CONTRACTS).unwrap() =>
        {
            let mut seen = std::collections::BTreeSet::new();
            let mut result = Vec::with_capacity(contracts.len());
            for contract in contracts {
                if !seen.insert(contract.contract_id) {
                    return DebuggerReply::Error {
                        code: DebuggerErrorCode::InvalidTarget,
                        message: "duplicate debugger static metadata contract identity".to_string(),
                    };
                }
                result.push(DebuggerStaticMetadataContractId {
                    metadata,
                    contract_id: contract.contract_id,
                });
            }
            if !metadata_session.observe_contracts(&result) {
                return DebuggerReply::Error {
                    code: DebuggerErrorCode::ResourceLimit,
                    message: "debugger static metadata contract receipt budget is exhausted"
                        .to_string(),
                };
            }
            DebuggerReply::StaticMetadataContracts(result)
        }
        Ok(_) => DebuggerReply::Error {
            code: DebuggerErrorCode::ResourceLimit,
            message: "debugger static metadata contract inventory exceeds its fixed limit"
                .to_string(),
        },
        Err(error) => debugger_program_error(error),
    }
}

/// Discloses one bounded compiler-produced contract display only after the
/// exact contract ID crossed this stream's contract-inventory receipt boundary.
/// The target keeps its opaque parent and all generations, so a caller-supplied
/// number cannot probe a child contract table by itself.
pub(super) fn describe_child_static_metadata_contract(
    tabs: &TabManager,
    locations: &mut dyn PageJavaScriptDebuggerLocations,
    metadata_session: Option<&DebuggerMetadataSessionAuthorization>,
    contract: DebuggerStaticMetadataContractId,
) -> DebuggerReply {
    if !contract.is_well_formed() {
        return DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            message: "invalid debugger static metadata contract display target".to_string(),
        };
    }
    let Some(metadata_session) = metadata_session else {
        return unavailable_static_metadata_contract_display();
    };
    if !metadata_session.permits(DebuggerMetadataCapability::OpaqueInventory)
        || !metadata_session.permits(DebuggerMetadataCapability::OpaqueContractInventory)
        || !metadata_session.observed_contract(contract)
    {
        return unavailable_static_metadata_contract_display();
    }
    let capabilities = describe_child_location_capabilities(
        tabs,
        locations,
        Some(metadata_session),
        contract.metadata.program.realm,
    );
    let DebuggerReply::Capabilities(capabilities) = capabilities else {
        return capabilities;
    };
    let Some(authorization) = capabilities.authorize_metadata(
        metadata_session,
        DebuggerMetadataCapability::OpaqueContractDisplay,
    ) else {
        return unavailable_static_metadata_contract_display();
    };
    if !authorization.permits(
        contract.metadata.program.realm,
        DebuggerMetadataCapability::OpaqueContractDisplay,
    ) {
        return unavailable_static_metadata_contract_display();
    }
    let tab_id = match resolve_live_realm(tabs, contract.metadata.program.realm) {
        Ok(tab_id) => tab_id,
        Err(reply) => return *reply,
    };
    match locations.debugger_static_metadata_contract_display(
        tab_id,
        contract.metadata.program.realm.realm_generation,
        JavaScriptPageDebuggerStaticMetadataContractTarget {
            program_handle: contract.metadata.program.program_handle,
            program_generation: contract.metadata.program.program_generation,
            metadata_handle: contract.metadata.metadata_handle,
            metadata_generation: contract.metadata.metadata_generation,
            contract_id: contract.contract_id,
        },
    ) {
        Ok(contract_display) => {
            if contract_display.contract_id != contract.contract_id {
                return DebuggerReply::Error {
                    code: DebuggerErrorCode::InvalidTarget,
                    message: "mismatched debugger static metadata contract display identity"
                        .to_string(),
                };
            }
            let contract_display = DebuggerStaticMetadataContractDisplay {
                contract,
                display: contract_display.display,
                root_kind: contract_display.root_kind,
            };
            if !contract_display.is_well_formed() {
                return DebuggerReply::Error {
                    code: DebuggerErrorCode::InvalidTarget,
                    message: "invalid debugger static metadata contract display".to_string(),
                };
            }
            DebuggerReply::StaticMetadataContract(contract_display)
        }
        Err(error) => debugger_program_error(error),
    }
}

/// Validates a bounded data-only snapshot only after its exact contract ID
/// crossed this stream's contract-inventory receipt boundary. The public reply
/// intentionally carries just a boolean: plan and structural failure detail
/// remain private to the supervised child.
pub(super) fn validate_child_static_metadata_contract(
    tabs: &TabManager,
    locations: &mut dyn PageJavaScriptDebuggerLocations,
    metadata_session: Option<&DebuggerMetadataSessionAuthorization>,
    contract: DebuggerStaticMetadataContractId,
    value: CompilerContractValue,
) -> DebuggerReply {
    if !contract.is_well_formed() || !debugger_contract_value_is_within_fixed_limits(&value) {
        return DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            message: "invalid debugger static metadata contract validation target".to_string(),
        };
    }
    let Some(metadata_session) = metadata_session else {
        return unavailable_static_metadata_contract_validation();
    };
    if !metadata_session.permits(DebuggerMetadataCapability::OpaqueInventory)
        || !metadata_session.permits(DebuggerMetadataCapability::OpaqueContractInventory)
        || !metadata_session.observed_contract(contract)
    {
        return unavailable_static_metadata_contract_validation();
    }
    let capabilities = describe_child_location_capabilities(
        tabs,
        locations,
        Some(metadata_session),
        contract.metadata.program.realm,
    );
    let DebuggerReply::Capabilities(capabilities) = capabilities else {
        return capabilities;
    };
    let Some(authorization) = capabilities.authorize_metadata(
        metadata_session,
        DebuggerMetadataCapability::OpaqueContractValidation,
    ) else {
        return unavailable_static_metadata_contract_validation();
    };
    if !authorization.permits(
        contract.metadata.program.realm,
        DebuggerMetadataCapability::OpaqueContractValidation,
    ) {
        return unavailable_static_metadata_contract_validation();
    }
    let tab_id = match resolve_live_realm(tabs, contract.metadata.program.realm) {
        Ok(tab_id) => tab_id,
        Err(reply) => return *reply,
    };
    match locations.debugger_static_metadata_contract_validation(
        tab_id,
        contract.metadata.program.realm.realm_generation,
        JavaScriptPageDebuggerStaticMetadataContractTarget {
            program_handle: contract.metadata.program.program_handle,
            program_generation: contract.metadata.program.program_generation,
            metadata_handle: contract.metadata.metadata_handle,
            metadata_generation: contract.metadata.metadata_generation,
            contract_id: contract.contract_id,
        },
        value,
    ) {
        Ok(validation) => {
            if validation.contract_id != contract.contract_id {
                return DebuggerReply::Error {
                    code: DebuggerErrorCode::InvalidTarget,
                    message: "mismatched debugger static metadata contract validation identity"
                        .to_string(),
                };
            }
            let validation = DebuggerStaticMetadataContractValidation {
                contract,
                valid: validation.valid,
            };
            if !validation.is_well_formed() {
                return DebuggerReply::Error {
                    code: DebuggerErrorCode::InvalidTarget,
                    message: "invalid debugger static metadata contract validation".to_string(),
                };
            }
            DebuggerReply::StaticMetadataContractValidation(validation)
        }
        Err(error) => debugger_program_error(error),
    }
}
