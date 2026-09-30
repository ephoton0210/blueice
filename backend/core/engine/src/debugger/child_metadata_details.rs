// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

/// Checks the exact data-only wire envelope before core forwards it to the
/// child. This iterative check avoids a recursive pre-validation walk and
/// enforces the same immutable limits again in the child before plan use.
pub(super) fn debugger_contract_value_is_within_fixed_limits(
    value: &CompilerContractValue,
) -> bool {
    use blueice_ipc::debugger::{
        DEBUGGER_STATIC_METADATA_CONTRACT_VALIDATION_MAX_COLLECTION_ENTRIES,
        DEBUGGER_STATIC_METADATA_CONTRACT_VALIDATION_MAX_DEPTH,
        DEBUGGER_STATIC_METADATA_CONTRACT_VALIDATION_MAX_NODES,
        DEBUGGER_STATIC_METADATA_CONTRACT_VALIDATION_MAX_STRING_BYTES,
    };

    let mut nodes = 0usize;
    let mut pending = vec![(value, 0usize)];
    while let Some((value, depth)) = pending.pop() {
        if depth > DEBUGGER_STATIC_METADATA_CONTRACT_VALIDATION_MAX_DEPTH
            || nodes >= DEBUGGER_STATIC_METADATA_CONTRACT_VALIDATION_MAX_NODES
        {
            return false;
        }
        nodes += 1;
        match value {
            CompilerContractValue::Null
            | CompilerContractValue::Undefined
            | CompilerContractValue::Boolean(_) => {}
            CompilerContractValue::Number(value) => {
                if value
                    .parse::<f64>()
                    .ok()
                    .is_none_or(|value| !value.is_finite())
                {
                    return false;
                }
            }
            CompilerContractValue::String(value) => {
                if value.len() > DEBUGGER_STATIC_METADATA_CONTRACT_VALIDATION_MAX_STRING_BYTES {
                    return false;
                }
            }
            CompilerContractValue::Array(values) => {
                if values.len()
                    > DEBUGGER_STATIC_METADATA_CONTRACT_VALIDATION_MAX_COLLECTION_ENTRIES
                {
                    return false;
                }
                pending.extend(values.iter().map(|value| (value, depth + 1)));
            }
            CompilerContractValue::Object(values) => {
                if values.len()
                    > DEBUGGER_STATIC_METADATA_CONTRACT_VALIDATION_MAX_COLLECTION_ENTRIES
                    || values.keys().any(|key| {
                        key.len() > DEBUGGER_STATIC_METADATA_CONTRACT_VALIDATION_MAX_STRING_BYTES
                    })
                {
                    return false;
                }
                pending.extend(values.values().map(|value| (value, depth + 1)));
            }
        }
    }
    true
}

/// Discloses one bounded compiler-produced display only after the exact type
/// ID crossed this stream's type-inventory receipt boundary. The target keeps
/// the opaque parent and all generations, so a caller-supplied number cannot
/// probe a child type table by itself.
pub(super) fn describe_child_static_metadata_type(
    tabs: &TabManager,
    locations: &mut dyn PageJavaScriptDebuggerLocations,
    metadata_session: Option<&DebuggerMetadataSessionAuthorization>,
    static_type: DebuggerStaticMetadataTypeId,
) -> DebuggerReply {
    if !static_type.is_well_formed() {
        return DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            message: "invalid debugger static metadata type display target".to_string(),
        };
    }
    let Some(metadata_session) = metadata_session else {
        return unavailable_static_metadata_type_display();
    };
    if !metadata_session.permits(DebuggerMetadataCapability::OpaqueInventory)
        || !metadata_session.permits(DebuggerMetadataCapability::OpaqueTypeInventory)
        || !metadata_session.observed_type(static_type)
    {
        return unavailable_static_metadata_type_display();
    }
    let capabilities = describe_child_location_capabilities(
        tabs,
        locations,
        Some(metadata_session),
        static_type.metadata.program.realm,
    );
    let DebuggerReply::Capabilities(capabilities) = capabilities else {
        return capabilities;
    };
    let Some(authorization) = capabilities.authorize_metadata(
        metadata_session,
        DebuggerMetadataCapability::OpaqueTypeDisplay,
    ) else {
        return unavailable_static_metadata_type_display();
    };
    if !authorization.permits(
        static_type.metadata.program.realm,
        DebuggerMetadataCapability::OpaqueTypeDisplay,
    ) {
        return unavailable_static_metadata_type_display();
    }
    let tab_id = match resolve_live_realm(tabs, static_type.metadata.program.realm) {
        Ok(tab_id) => tab_id,
        Err(reply) => return *reply,
    };
    match locations.debugger_static_metadata_type_display(
        tab_id,
        static_type.metadata.program.realm.realm_generation,
        JavaScriptPageDebuggerStaticMetadataTypeTarget {
            program_handle: static_type.metadata.program.program_handle,
            program_generation: static_type.metadata.program.program_generation,
            metadata_handle: static_type.metadata.metadata_handle,
            metadata_generation: static_type.metadata.metadata_generation,
            type_id: static_type.type_id,
        },
    ) {
        Ok(type_display) => {
            let type_display = DebuggerStaticMetadataTypeDisplay {
                static_type,
                display: type_display.display,
            };
            if !type_display.is_well_formed() {
                return DebuggerReply::Error {
                    code: DebuggerErrorCode::InvalidTarget,
                    message: "invalid debugger static metadata type display".to_string(),
                };
            }
            DebuggerReply::StaticMetadataType(type_display)
        }
        Err(error) => debugger_program_error(error),
    }
}

/// Discloses one bounded compiler-produced symbol display only after the
/// exact symbol ID crossed this stream's symbol-inventory receipt boundary.
/// The target keeps its opaque parent and all generations, so a caller-supplied
/// number cannot probe a child symbol table by itself.
pub(super) fn describe_child_static_metadata_symbol(
    tabs: &TabManager,
    locations: &mut dyn PageJavaScriptDebuggerLocations,
    metadata_session: Option<&DebuggerMetadataSessionAuthorization>,
    symbol: DebuggerStaticMetadataSymbolId,
) -> DebuggerReply {
    if !symbol.is_well_formed() {
        return DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            message: "invalid debugger static metadata symbol display target".to_string(),
        };
    }
    let Some(metadata_session) = metadata_session else {
        return unavailable_static_metadata_symbol_display();
    };
    if !metadata_session.permits(DebuggerMetadataCapability::OpaqueInventory)
        || !metadata_session.permits(DebuggerMetadataCapability::OpaqueSymbolInventory)
        || !metadata_session.observed_symbol(symbol)
    {
        return unavailable_static_metadata_symbol_display();
    }
    let capabilities = describe_child_location_capabilities(
        tabs,
        locations,
        Some(metadata_session),
        symbol.metadata.program.realm,
    );
    let DebuggerReply::Capabilities(capabilities) = capabilities else {
        return capabilities;
    };
    let Some(authorization) = capabilities.authorize_metadata(
        metadata_session,
        DebuggerMetadataCapability::OpaqueSymbolDisplay,
    ) else {
        return unavailable_static_metadata_symbol_display();
    };
    if !authorization.permits(
        symbol.metadata.program.realm,
        DebuggerMetadataCapability::OpaqueSymbolDisplay,
    ) {
        return unavailable_static_metadata_symbol_display();
    }
    let tab_id = match resolve_live_realm(tabs, symbol.metadata.program.realm) {
        Ok(tab_id) => tab_id,
        Err(reply) => return *reply,
    };
    match locations.debugger_static_metadata_symbol_display(
        tab_id,
        symbol.metadata.program.realm.realm_generation,
        JavaScriptPageDebuggerStaticMetadataSymbolTarget {
            program_handle: symbol.metadata.program.program_handle,
            program_generation: symbol.metadata.program.program_generation,
            metadata_handle: symbol.metadata.metadata_handle,
            metadata_generation: symbol.metadata.metadata_generation,
            symbol_id: symbol.symbol_id,
        },
    ) {
        Ok(symbol_display) => {
            if symbol_display.symbol_id != symbol.symbol_id {
                return DebuggerReply::Error {
                    code: DebuggerErrorCode::InvalidTarget,
                    message: "mismatched debugger static metadata symbol display identity"
                        .to_string(),
                };
            }
            let symbol_display = DebuggerStaticMetadataSymbolDisplay {
                symbol,
                display: symbol_display.display,
                kind: symbol_display.kind,
                exported: symbol_display.exported,
            };
            if !symbol_display.is_well_formed() {
                return DebuggerReply::Error {
                    code: DebuggerErrorCode::InvalidTarget,
                    message: "invalid debugger static metadata symbol display".to_string(),
                };
            }
            DebuggerReply::StaticMetadataSymbol(symbol_display)
        }
        Err(error) => debugger_program_error(error),
    }
}

/// Discloses one half-open source byte range only after the exact stream has
/// independently inventoried both its symbol and source IDs. The caller never
/// supplies an offset, and the reply contains no source/module/name/type/
/// contract/bytecode data or source-map translation.
pub(super) fn describe_child_static_metadata_symbol_location(
    tabs: &TabManager,
    locations: &mut dyn PageJavaScriptDebuggerLocations,
    metadata_session: Option<&DebuggerMetadataSessionAuthorization>,
    target: DebuggerStaticMetadataSymbolLocationTarget,
) -> DebuggerReply {
    if !target.is_well_formed() {
        return DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            message: "invalid debugger static metadata symbol location target".to_string(),
        };
    }
    let Some(metadata_session) = metadata_session else {
        return unavailable_static_metadata_symbol_location();
    };
    if !metadata_session.permits(DebuggerMetadataCapability::OpaqueInventory)
        || !metadata_session.permits(DebuggerMetadataCapability::OpaqueSourceInventory)
        || !metadata_session.permits(DebuggerMetadataCapability::OpaqueSymbolInventory)
        || !metadata_session.observed_symbol(target.symbol)
        || !metadata_session.observed_source(target.source)
    {
        return unavailable_static_metadata_symbol_location();
    }
    let capabilities = describe_child_location_capabilities(
        tabs,
        locations,
        Some(metadata_session),
        target.symbol.metadata.program.realm,
    );
    let DebuggerReply::Capabilities(capabilities) = capabilities else {
        return capabilities;
    };
    let Some(authorization) = capabilities.authorize_metadata(
        metadata_session,
        DebuggerMetadataCapability::OpaqueSymbolLocation,
    ) else {
        return unavailable_static_metadata_symbol_location();
    };
    if !authorization.permits(
        target.symbol.metadata.program.realm,
        DebuggerMetadataCapability::OpaqueSymbolLocation,
    ) {
        return unavailable_static_metadata_symbol_location();
    }
    let tab_id = match resolve_live_realm(tabs, target.symbol.metadata.program.realm) {
        Ok(tab_id) => tab_id,
        Err(reply) => return *reply,
    };
    match locations.debugger_static_metadata_symbol_location(
        tab_id,
        target.symbol.metadata.program.realm.realm_generation,
        JavaScriptPageDebuggerStaticMetadataSymbolLocationTarget {
            program_handle: target.symbol.metadata.program.program_handle,
            program_generation: target.symbol.metadata.program.program_generation,
            metadata_handle: target.symbol.metadata.metadata_handle,
            metadata_generation: target.symbol.metadata.metadata_generation,
            symbol_id: target.symbol.symbol_id,
            source_id: target.source.source_id,
        },
    ) {
        Ok(location) => {
            if location.symbol_id != target.symbol.symbol_id
                || location.source_id != target.source.source_id
            {
                return DebuggerReply::Error {
                    code: DebuggerErrorCode::InvalidTarget,
                    message: "mismatched debugger static metadata symbol location identity"
                        .to_string(),
                };
            }
            let location = DebuggerStaticMetadataSymbolLocation {
                symbol: target.symbol,
                source: target.source,
                start_byte: location.start_byte,
                end_byte: location.end_byte,
                coordinates: location.coordinates,
            };
            if !location.is_well_formed() {
                return DebuggerReply::Error {
                    code: DebuggerErrorCode::InvalidTarget,
                    message: "invalid debugger static metadata symbol location".to_string(),
                };
            }
            DebuggerReply::StaticMetadataSymbolLocation(location)
        }
        Err(error) => debugger_program_error(error),
    }
}

/// Discloses only a compiler-verified original byte span for one exact safe
/// point, after the stream independently received its opaque metadata parent
/// and source ID. A guessed source ID or unbound instruction cannot become a
/// nearest-position source-map query.
pub(super) fn describe_child_static_metadata_safe_point_span(
    tabs: &TabManager,
    locations: &mut dyn PageJavaScriptDebuggerLocations,
    metadata_session: Option<&DebuggerMetadataSessionAuthorization>,
    target: DebuggerStaticMetadataSafePointSpanTarget,
) -> DebuggerReply {
    if !target.is_well_formed() {
        return DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            message: "invalid debugger static metadata safe-point span target".to_string(),
        };
    }
    let Some(metadata_session) = metadata_session else {
        return unavailable_static_metadata_safe_point_span();
    };
    if !metadata_session.permits(DebuggerMetadataCapability::OpaqueInventory)
        || !metadata_session.permits(DebuggerMetadataCapability::OpaqueSourceInventory)
        || !metadata_session.observed_metadata(target.source.metadata)
        || !metadata_session.observed_source(target.source)
    {
        return unavailable_static_metadata_safe_point_span();
    }
    let realm = target.safe_point.program.realm;
    let capabilities =
        describe_child_location_capabilities(tabs, locations, Some(metadata_session), realm);
    let DebuggerReply::Capabilities(capabilities) = capabilities else {
        return capabilities;
    };
    let Some(authorization) = capabilities.authorize_metadata(
        metadata_session,
        DebuggerMetadataCapability::OpaqueSafePointSpan,
    ) else {
        return unavailable_static_metadata_safe_point_span();
    };
    if !authorization.permits(realm, DebuggerMetadataCapability::OpaqueSafePointSpan) {
        return unavailable_static_metadata_safe_point_span();
    }
    let tab_id = match resolve_live_realm(tabs, realm) {
        Ok(tab_id) => tab_id,
        Err(reply) => return *reply,
    };
    match locations.debugger_static_metadata_safe_point_span(
        tab_id,
        realm.realm_generation,
        JavaScriptPageDebuggerStaticMetadataSafePointSpanTarget {
            program_handle: target.safe_point.program.program_handle,
            program_generation: target.safe_point.program.program_generation,
            metadata_handle: target.source.metadata.metadata_handle,
            metadata_generation: target.source.metadata.metadata_generation,
            source_id: target.source.source_id,
            code_unit_ordinal: target.safe_point.code_unit_ordinal,
            bytecode_offset: target.safe_point.bytecode_offset,
        },
    ) {
        Ok(span) if span.source_id == target.source.source_id => {
            let result = DebuggerStaticMetadataSafePointSpan {
                safe_point: target.safe_point,
                source: target.source,
                start_byte: span.start_byte,
                end_byte: span.end_byte,
                coordinates: span.coordinates,
            };
            if !result.is_well_formed() {
                return DebuggerReply::Error {
                    code: DebuggerErrorCode::InvalidTarget,
                    message: "invalid debugger static metadata safe-point span".to_string(),
                };
            }
            DebuggerReply::StaticMetadataSafePointSpan(result)
        }
        Ok(_) => DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            message: "mismatched debugger static metadata safe-point source identity".to_string(),
        },
        Err(error) => debugger_program_error(error),
    }
}

/// The caller supplies only one previously inventoried source ID. Core
/// separately authorizes the old safe-point-span capability, asks the child
/// for its terminal site, and remints a distinct exact exception reply.
pub(super) fn describe_child_exception_location(
    tabs: &TabManager,
    locations: &mut dyn PageJavaScriptDebuggerLocations,
    metadata_session: Option<&DebuggerMetadataSessionAuthorization>,
    source: DebuggerStaticMetadataSourceId,
) -> DebuggerReply {
    if !source.is_well_formed() {
        return invalid_exception_location();
    }
    let Some(metadata_session) = metadata_session else {
        return unavailable_exception_location();
    };
    let realm = source.metadata.program.realm;
    let capabilities =
        describe_child_location_capabilities(tabs, locations, Some(metadata_session), realm);
    let DebuggerReply::Capabilities(capabilities) = capabilities else {
        return capabilities;
    };
    let Some(authorization) = capabilities.authorize_metadata(
        metadata_session,
        DebuggerMetadataCapability::OpaqueSafePointSpan,
    ) else {
        return unavailable_exception_location();
    };
    if !authorization.permits(realm, DebuggerMetadataCapability::OpaqueSafePointSpan)
        || !locations.debugger_exception_location_available()
    {
        return unavailable_exception_location();
    }
    if !metadata_session.observed_metadata(source.metadata)
        || !metadata_session.observed_source(source)
    {
        return invalid_exception_location();
    }
    let tab_id = match resolve_live_realm(tabs, realm) {
        Ok(tab_id) => tab_id,
        Err(reply) => return *reply,
    };
    let program = source.metadata.program;
    match locations.debugger_exception_location(
        tab_id,
        realm.realm_generation,
        JavaScriptPageDebuggerExceptionLocationTarget {
            program_handle: program.program_handle,
            program_generation: program.program_generation,
            metadata_handle: source.metadata.metadata_handle,
            metadata_generation: source.metadata.metadata_generation,
            source_id: source.source_id,
        },
    ) {
        Ok(location) if location.source_id == source.source_id => {
            let result = DebuggerExceptionLocation {
                source,
                safe_point: DebuggerSafePoint {
                    program,
                    code_unit_ordinal: location.code_unit_ordinal,
                    bytecode_offset: location.bytecode_offset,
                },
                start_byte: location.start_byte,
                end_byte: location.end_byte,
                coordinates: location.coordinates,
            };
            if !result.is_well_formed() {
                return invalid_exception_location();
            }
            if locations
                .validate_debugger_safe_point(
                    tab_id,
                    realm.realm_generation,
                    program.program_handle,
                    program.program_generation,
                    result.safe_point.code_unit_ordinal,
                    result.safe_point.bytecode_offset,
                )
                .is_err()
            {
                return invalid_exception_location();
            }
            let bound_span = locations.debugger_static_metadata_safe_point_span(
                tab_id,
                realm.realm_generation,
                JavaScriptPageDebuggerStaticMetadataSafePointSpanTarget {
                    program_handle: program.program_handle,
                    program_generation: program.program_generation,
                    metadata_handle: source.metadata.metadata_handle,
                    metadata_generation: source.metadata.metadata_generation,
                    source_id: source.source_id,
                    code_unit_ordinal: result.safe_point.code_unit_ordinal,
                    bytecode_offset: result.safe_point.bytecode_offset,
                },
            );
            if !matches!(
                bound_span,
                Ok(span) if span.source_id == source.source_id
                    && span.start_byte == result.start_byte
                    && span.end_byte == result.end_byte
                    && span.coordinates == result.coordinates
            ) {
                return invalid_exception_location();
            }
            DebuggerReply::ExceptionLocation(result)
        }
        Ok(_) => invalid_exception_location(),
        Err(JavaScriptPageDebuggerError::InvalidExecutionState) => DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidExecutionState,
            message: "BlueTS program has no terminal uncaught exception location".to_string(),
        },
        Err(JavaScriptPageDebuggerError::ResourceLimit) => {
            debugger_program_error(JavaScriptPageDebuggerError::ResourceLimit)
        }
        Err(_) => invalid_exception_location(),
    }
}

/// Resolves a caller-selected original BlueTS byte position only under its
/// own owner/client grant and an exact same-stream source-ID receipt. The
/// child cannot mint a public safe point: core remints and revalidates a
/// bound instruction before replying, and preserves an explicit unbound
/// result rather than guessing a later executable statement.
pub(super) fn resolve_child_static_metadata_source_breakpoint(
    tabs: &TabManager,
    locations: &mut dyn PageJavaScriptDebuggerLocations,
    metadata_session: Option<&DebuggerMetadataSessionAuthorization>,
    target: DebuggerStaticMetadataSourceBreakpointTarget,
) -> DebuggerReply {
    if !target.is_well_formed() {
        return DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            message: "invalid debugger static metadata source breakpoint target".to_string(),
        };
    }
    let Some(metadata_session) = metadata_session else {
        return unavailable_static_metadata_source_breakpoint();
    };
    if !metadata_session.permits(DebuggerMetadataCapability::OpaqueInventory)
        || !metadata_session.permits(DebuggerMetadataCapability::OpaqueSourceInventory)
        || !metadata_session.observed_metadata(target.source.metadata)
        || !metadata_session.observed_source(target.source)
    {
        return unavailable_static_metadata_source_breakpoint();
    }
    let realm = target.source.metadata.program.realm;
    let capabilities =
        describe_child_location_capabilities(tabs, locations, Some(metadata_session), realm);
    let DebuggerReply::Capabilities(capabilities) = capabilities else {
        return capabilities;
    };
    let Some(authorization) = capabilities.authorize_metadata(
        metadata_session,
        DebuggerMetadataCapability::OpaqueSourceBreakpoint,
    ) else {
        return unavailable_static_metadata_source_breakpoint();
    };
    if !authorization.permits(realm, DebuggerMetadataCapability::OpaqueSourceBreakpoint) {
        return unavailable_static_metadata_source_breakpoint();
    }
    let tab_id = match resolve_live_realm(tabs, realm) {
        Ok(tab_id) => tab_id,
        Err(reply) => return *reply,
    };
    let program = target.source.metadata.program;
    let binding = match locations.debugger_static_metadata_source_breakpoint(
        tab_id,
        realm.realm_generation,
        JavaScriptPageDebuggerStaticMetadataSourceBreakpointTarget {
            program_handle: program.program_handle,
            program_generation: program.program_generation,
            metadata_handle: target.source.metadata.metadata_handle,
            metadata_generation: target.source.metadata.metadata_generation,
            source_id: target.source.source_id,
            source_byte: target.source_byte,
        },
    ) {
        Ok(binding) => binding,
        Err(error) => return debugger_program_error(error),
    };
    let safe_point = binding.map(|binding| DebuggerSafePoint {
        program,
        code_unit_ordinal: binding.code_unit_ordinal,
        bytecode_offset: binding.bytecode_offset,
    });
    if let Some(safe_point) = safe_point {
        match validate_child_safe_point(tabs, locations, safe_point) {
            DebuggerReply::SafePointValidated {
                safe_point: validated,
            } if validated == safe_point => {}
            reply => return reply,
        }
    }
    let result = DebuggerStaticMetadataSourceBreakpoint { target, safe_point };
    if !result.is_well_formed() {
        return DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            message: "invalid debugger static metadata source breakpoint result".to_string(),
        };
    }
    DebuggerReply::StaticMetadataSourceBreakpoint(result)
}

/// Combines the separately authorized source-position binding with the
/// existing classic/module root arm in one owning session turn. No worker or peer
/// can substitute a safe point between those checks, and an unbound or child
/// code-unit result never reaches the execution-control call.
pub(super) fn arm_child_static_metadata_source_breakpoint(
    tabs: &TabManager,
    locations: &mut dyn PageJavaScriptDebuggerLocations,
    metadata_session: Option<&DebuggerMetadataSessionAuthorization>,
    target: DebuggerStaticMetadataSourceBreakpointTarget,
) -> DebuggerReply {
    let binding = match resolve_child_static_metadata_source_breakpoint(
        tabs,
        locations,
        metadata_session,
        target,
    ) {
        DebuggerReply::StaticMetadataSourceBreakpoint(binding) => binding,
        DebuggerReply::Unsupported { .. } => {
            return unavailable_static_metadata_source_breakpoint_arm()
        }
        reply => return reply,
    };
    let Some(safe_point) = binding.safe_point else {
        return DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidSafePoint,
            message: "source position has no executable root safe point".to_string(),
        };
    };
    if !locations.debugger_execution_control_available() {
        return unavailable_execution_control();
    }
    arm_child_root_safe_point_breakpoint(tabs, locations, safe_point)
}

/// A contract location is resolved only after this stream received the exact
/// parent, contract ID, and source ID and the live child reports the distinct
/// location capability. No source identity, text, plan, or value crosses it.
pub(super) fn describe_child_static_metadata_contract_location(
    tabs: &TabManager,
    locations: &mut dyn PageJavaScriptDebuggerLocations,
    metadata_session: Option<&DebuggerMetadataSessionAuthorization>,
    target: DebuggerStaticMetadataContractLocationTarget,
) -> DebuggerReply {
    if !target.is_well_formed() {
        return DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            message: "invalid debugger static metadata contract location target".to_string(),
        };
    }
    let Some(metadata_session) = metadata_session else {
        return unavailable_static_metadata_contract_location();
    };
    if !metadata_session.permits(DebuggerMetadataCapability::OpaqueInventory)
        || !metadata_session.permits(DebuggerMetadataCapability::OpaqueSourceInventory)
        || !metadata_session.permits(DebuggerMetadataCapability::OpaqueContractInventory)
        || !metadata_session.observed_contract(target.contract)
        || !metadata_session.observed_source(target.source)
    {
        return unavailable_static_metadata_contract_location();
    }
    let capabilities = describe_child_location_capabilities(
        tabs,
        locations,
        Some(metadata_session),
        target.contract.metadata.program.realm,
    );
    let DebuggerReply::Capabilities(capabilities) = capabilities else {
        return capabilities;
    };
    let Some(authorization) = capabilities.authorize_metadata(
        metadata_session,
        DebuggerMetadataCapability::OpaqueContractLocation,
    ) else {
        return unavailable_static_metadata_contract_location();
    };
    if !authorization.permits(
        target.contract.metadata.program.realm,
        DebuggerMetadataCapability::OpaqueContractLocation,
    ) {
        return unavailable_static_metadata_contract_location();
    }
    let tab_id = match resolve_live_realm(tabs, target.contract.metadata.program.realm) {
        Ok(tab_id) => tab_id,
        Err(reply) => return *reply,
    };
    match locations.debugger_static_metadata_contract_location(
        tab_id,
        target.contract.metadata.program.realm.realm_generation,
        JavaScriptPageDebuggerStaticMetadataContractLocationTarget {
            program_handle: target.contract.metadata.program.program_handle,
            program_generation: target.contract.metadata.program.program_generation,
            metadata_handle: target.contract.metadata.metadata_handle,
            metadata_generation: target.contract.metadata.metadata_generation,
            contract_id: target.contract.contract_id,
            source_id: target.source.source_id,
        },
    ) {
        Ok(location) => {
            if location.contract_id != target.contract.contract_id
                || location.source_id != target.source.source_id
            {
                return DebuggerReply::Error {
                    code: DebuggerErrorCode::InvalidTarget,
                    message: "mismatched debugger static metadata contract location identity"
                        .to_string(),
                };
            }
            let location = DebuggerStaticMetadataContractLocation {
                contract: target.contract,
                source: target.source,
                start_byte: location.start_byte,
                end_byte: location.end_byte,
                coordinates: location.coordinates,
            };
            if !location.is_well_formed() {
                return DebuggerReply::Error {
                    code: DebuggerErrorCode::InvalidTarget,
                    message: "invalid debugger static metadata contract location".to_string(),
                };
            }
            DebuggerReply::StaticMetadataContractLocation(location)
        }
        Err(error) => debugger_program_error(error),
    }
}

/// Verifies a compiler-recorded symbol/type relation after both opaque IDs
/// crossed this stream's separate inventories. The requested pair and the
/// child reply must agree exactly; no unrequested type or display is emitted.
pub(super) fn describe_child_static_metadata_symbol_type(
    tabs: &TabManager,
    locations: &mut dyn PageJavaScriptDebuggerLocations,
    metadata_session: Option<&DebuggerMetadataSessionAuthorization>,
    target: DebuggerStaticMetadataSymbolType,
) -> DebuggerReply {
    if !target.is_well_formed() {
        return DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            message: "invalid debugger static metadata symbol type target".to_string(),
        };
    }
    let Some(metadata_session) = metadata_session else {
        return unavailable_static_metadata_symbol_type();
    };
    if !metadata_session.permits(DebuggerMetadataCapability::OpaqueInventory)
        || !metadata_session.permits(DebuggerMetadataCapability::OpaqueTypeInventory)
        || !metadata_session.permits(DebuggerMetadataCapability::OpaqueSymbolInventory)
        || !metadata_session.observed_symbol(target.symbol)
        || !metadata_session.observed_type(target.static_type)
    {
        return unavailable_static_metadata_symbol_type();
    }
    let capabilities = describe_child_location_capabilities(
        tabs,
        locations,
        Some(metadata_session),
        target.symbol.metadata.program.realm,
    );
    let DebuggerReply::Capabilities(capabilities) = capabilities else {
        return capabilities;
    };
    let Some(authorization) = capabilities.authorize_metadata(
        metadata_session,
        DebuggerMetadataCapability::OpaqueSymbolType,
    ) else {
        return unavailable_static_metadata_symbol_type();
    };
    if !authorization.permits(
        target.symbol.metadata.program.realm,
        DebuggerMetadataCapability::OpaqueSymbolType,
    ) {
        return unavailable_static_metadata_symbol_type();
    }
    let tab_id = match resolve_live_realm(tabs, target.symbol.metadata.program.realm) {
        Ok(tab_id) => tab_id,
        Err(reply) => return *reply,
    };
    match locations.debugger_static_metadata_symbol_type(
        tab_id,
        target.symbol.metadata.program.realm.realm_generation,
        JavaScriptPageDebuggerStaticMetadataSymbolTypeTarget {
            program_handle: target.symbol.metadata.program.program_handle,
            program_generation: target.symbol.metadata.program.program_generation,
            metadata_handle: target.symbol.metadata.metadata_handle,
            metadata_generation: target.symbol.metadata.metadata_generation,
            symbol_id: target.symbol.symbol_id,
            type_id: target.static_type.type_id,
        },
    ) {
        Ok(symbol_type)
            if symbol_type.symbol_id == target.symbol.symbol_id
                && symbol_type.type_id == target.static_type.type_id =>
        {
            DebuggerReply::StaticMetadataSymbolType(target)
        }
        Ok(_) => DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            message: "mismatched debugger static metadata symbol type identity".to_string(),
        },
        Err(error) => debugger_program_error(error),
    }
}

/// Verifies one reifiable symbol/contract relation after both opaque IDs
/// crossed this stream's separate inventories. The child may only confirm
/// the exact requested pair; it cannot introduce a contract ID or plan.
pub(super) fn describe_child_static_metadata_symbol_contract(
    tabs: &TabManager,
    locations: &mut dyn PageJavaScriptDebuggerLocations,
    metadata_session: Option<&DebuggerMetadataSessionAuthorization>,
    target: DebuggerStaticMetadataSymbolContract,
) -> DebuggerReply {
    if !target.is_well_formed() {
        return DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            message: "invalid debugger static metadata symbol contract target".to_string(),
        };
    }
    let Some(metadata_session) = metadata_session else {
        return unavailable_static_metadata_symbol_contract();
    };
    if !metadata_session.permits(DebuggerMetadataCapability::OpaqueInventory)
        || !metadata_session.permits(DebuggerMetadataCapability::OpaqueSymbolInventory)
        || !metadata_session.permits(DebuggerMetadataCapability::OpaqueContractInventory)
        || !metadata_session.observed_symbol(target.symbol)
        || !metadata_session.observed_contract(target.contract)
    {
        return unavailable_static_metadata_symbol_contract();
    }
    let capabilities = describe_child_location_capabilities(
        tabs,
        locations,
        Some(metadata_session),
        target.symbol.metadata.program.realm,
    );
    let DebuggerReply::Capabilities(capabilities) = capabilities else {
        return capabilities;
    };
    let Some(authorization) = capabilities.authorize_metadata(
        metadata_session,
        DebuggerMetadataCapability::OpaqueSymbolContract,
    ) else {
        return unavailable_static_metadata_symbol_contract();
    };
    if !authorization.permits(
        target.symbol.metadata.program.realm,
        DebuggerMetadataCapability::OpaqueSymbolContract,
    ) {
        return unavailable_static_metadata_symbol_contract();
    }
    let tab_id = match resolve_live_realm(tabs, target.symbol.metadata.program.realm) {
        Ok(tab_id) => tab_id,
        Err(reply) => return *reply,
    };
    match locations.debugger_static_metadata_symbol_contract(
        tab_id,
        target.symbol.metadata.program.realm.realm_generation,
        JavaScriptPageDebuggerStaticMetadataSymbolContractTarget {
            program_handle: target.symbol.metadata.program.program_handle,
            program_generation: target.symbol.metadata.program.program_generation,
            metadata_handle: target.symbol.metadata.metadata_handle,
            metadata_generation: target.symbol.metadata.metadata_generation,
            symbol_id: target.symbol.symbol_id,
            contract_id: target.contract.contract_id,
        },
    ) {
        Ok(symbol_contract)
            if symbol_contract.symbol_id == target.symbol.symbol_id
                && symbol_contract.contract_id == target.contract.contract_id =>
        {
            DebuggerReply::StaticMetadataSymbolContract(target)
        }
        Ok(_) => DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            message: "mismatched debugger static metadata symbol contract identity".to_string(),
        },
        Err(error) => debugger_program_error(error),
    }
}

pub(super) fn describe_child_static_metadata_source_provenance(
    tabs: &TabManager,
    locations: &mut dyn PageJavaScriptDebuggerLocations,
    metadata_session: Option<&DebuggerMetadataSessionAuthorization>,
    source: DebuggerStaticMetadataSourceId,
) -> DebuggerReply {
    if !source.is_well_formed() {
        return DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            message: "invalid debugger static metadata source provenance target".to_string(),
        };
    }
    let Some(metadata_session) = metadata_session else {
        return unavailable_static_metadata_source_provenance();
    };
    // Provenance is deliberately dependent on source inventory: callers must
    // present an exact source ID under an opaque parent, not invent a source
    // lookup key or obtain a standalone content oracle.
    if !metadata_session.permits(DebuggerMetadataCapability::OpaqueInventory)
        || !metadata_session.permits(DebuggerMetadataCapability::OpaqueSourceInventory)
        || !metadata_session.observed_source(source)
    {
        return unavailable_static_metadata_source_provenance();
    }
    let capabilities = describe_child_location_capabilities(
        tabs,
        locations,
        Some(metadata_session),
        source.metadata.program.realm,
    );
    let DebuggerReply::Capabilities(capabilities) = capabilities else {
        return capabilities;
    };
    let Some(authorization) = capabilities.authorize_metadata(
        metadata_session,
        DebuggerMetadataCapability::OpaqueSourceProvenance,
    ) else {
        return unavailable_static_metadata_source_provenance();
    };
    if !authorization.permits(
        source.metadata.program.realm,
        DebuggerMetadataCapability::OpaqueSourceProvenance,
    ) {
        return unavailable_static_metadata_source_provenance();
    }
    let tab_id = match resolve_live_realm(tabs, source.metadata.program.realm) {
        Ok(tab_id) => tab_id,
        Err(reply) => return *reply,
    };
    match locations.debugger_static_metadata_source_provenance(
        tab_id,
        source.metadata.program.realm.realm_generation,
        JavaScriptPageDebuggerStaticMetadataSourceTarget {
            program_handle: source.metadata.program.program_handle,
            program_generation: source.metadata.program.program_generation,
            metadata_handle: source.metadata.metadata_handle,
            metadata_generation: source.metadata.metadata_generation,
            source_id: source.source_id,
        },
    ) {
        Ok(provenance) => {
            let provenance = DebuggerStaticMetadataSourceProvenance {
                source,
                module: provenance.module,
                content_hash: provenance.content_hash,
            };
            if !provenance.is_well_formed() {
                return DebuggerReply::Error {
                    code: DebuggerErrorCode::InvalidTarget,
                    message: "invalid debugger static metadata source provenance".to_string(),
                };
            }
            DebuggerReply::StaticMetadataSourceProvenance(provenance)
        }
        Err(error) => debugger_program_error(error),
    }
}
