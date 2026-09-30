// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

pub(super) fn describe_capabilities(
    tabs: &TabManager,
    javascript_executor: Option<&JavaScriptPageExecutor>,
    realm: DebuggerPageRealm,
) -> DebuggerReply {
    if let Err(reply) = resolve_live_realm(tabs, realm) {
        return *reply;
    }
    let program_locations_available = javascript_executor.is_some_and(|executor| {
        executor.debugger_has_live_realm(TabId::from_u64(realm.tab_id), realm.realm_generation)
    });
    let entry_execution_control_available = javascript_executor.is_some_and(|executor| {
        executor.debugger_execution_control_available()
            && executor
                .debugger_has_live_realm(TabId::from_u64(realm.tab_id), realm.realm_generation)
    });
    let max_safe_points_per_program = javascript_executor
        .map(JavaScriptPageExecutor::max_debugger_safe_points_per_program)
        .unwrap_or(DEFAULT_MAX_SAFE_POINTS_PER_PROGRAM);
    let max_breakpoints_per_realm = javascript_executor
        .map(JavaScriptPageExecutor::max_debugger_breakpoints_per_realm)
        .unwrap_or(DEFAULT_MAX_BREAKPOINTS_PER_REALM);

    DebuggerReply::Capabilities(DebuggerCapabilities {
        protocol_version: DEBUGGER_PROTOCOL_VERSION,
        realm,
        reports: capability_reports(DebuggerCapabilityAvailability {
            program_locations_available,
            breakpoint_configuration_available: program_locations_available,
            entry_execution_control_available,
            stepping_available: entry_execution_control_available,
            nested_frames_available: false,
            linked_modules_available: false,
            stack_available: false,
            scopes_available: false,
            values_available: false,
            static_metadata_inventory_available: false,
            static_metadata_summary_available: false,
            static_metadata_lowering_summary_available: false,
            static_metadata_source_inventory_available: false,
            static_metadata_source_provenance_available: false,
            static_metadata_type_inventory_available: false,
            static_metadata_type_display_available: false,
            static_metadata_symbol_inventory_available: false,
            static_metadata_contract_inventory_available: false,
            static_metadata_contract_display_available: false,
            static_metadata_contract_validation_available: false,
            static_metadata_symbol_display_available: false,
            static_metadata_symbol_location_available: false,
            static_metadata_safe_point_span_available: false,
            static_metadata_source_span_step_available: false,
            static_metadata_source_breakpoint_available: false,
            static_metadata_contract_location_available: false,
            static_metadata_symbol_type_available: false,
            static_metadata_symbol_contract_available: false,
            static_scope_relation_available: false,
        }),
        max_stack_frames: MAX_STACK_FRAMES,
        max_scope_bindings: MAX_SCOPE_BINDINGS,
        max_value_preview_bytes: MAX_VALUE_PREVIEW_BYTES,
        max_safe_points_per_program: u32::try_from(max_safe_points_per_program)
            .expect("native debugger safe-point reply cap fits the wire type"),
        max_breakpoints_per_realm: u32::try_from(max_breakpoints_per_realm)
            .expect("native debugger breakpoint reply cap fits the wire type"),
    })
}

/// Resolves a realm for one debugger operation without copying a large public
/// wire reply through every local `Result` error path.
pub(super) fn resolve_live_realm(
    tabs: &TabManager,
    realm: DebuggerPageRealm,
) -> Result<TabId, Box<DebuggerReply>> {
    if !realm.is_well_formed() || realm.browser_context_id != DEFAULT_BROWSER_CONTEXT_ID {
        return Err(Box::new(DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            message: "invalid debugger realm target".to_string(),
        }));
    }
    let tab_id = TabId::from_u64(realm.tab_id);
    let Some(page) = tabs.get(tab_id) else {
        return Err(Box::new(DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            message: "unknown debugger tab".to_string(),
        }));
    };
    if page.document_generation() != realm.realm_generation {
        return Err(Box::new(DebuggerReply::Error {
            code: DebuggerErrorCode::StaleRealm,
            message: "stale debugger realm generation".to_string(),
        }));
    }
    Ok(tab_id)
}

pub(super) fn list_programs(
    tabs: &TabManager,
    javascript_executor: Option<&JavaScriptPageExecutor>,
    realm: DebuggerPageRealm,
) -> DebuggerReply {
    let tab_id = match resolve_live_realm(tabs, realm) {
        Ok(tab_id) => tab_id,
        Err(reply) => return *reply,
    };
    let Some(executor) = javascript_executor else {
        return unavailable_program_locations();
    };
    if !executor.debugger_has_live_realm(tab_id, realm.realm_generation) {
        return unavailable_program_locations();
    }
    match executor.debugger_programs(tab_id, realm.realm_generation) {
        Ok(programs) => DebuggerReply::Programs(
            programs
                .into_iter()
                .map(|program| DebuggerProgram {
                    realm,
                    program_handle: program.program_handle,
                    program_generation: program.program_generation,
                })
                .collect(),
        ),
        Err(error) => debugger_program_error(error),
    }
}

pub(super) fn list_safe_points(
    tabs: &TabManager,
    javascript_executor: Option<&JavaScriptPageExecutor>,
    program: DebuggerProgram,
) -> DebuggerReply {
    if !program.is_well_formed() {
        return DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            message: "invalid debugger program target".to_string(),
        };
    }
    let tab_id = match resolve_live_realm(tabs, program.realm) {
        Ok(tab_id) => tab_id,
        Err(reply) => return *reply,
    };
    let Some(executor) = javascript_executor else {
        return unavailable_program_locations();
    };
    if !executor.debugger_has_live_realm(tab_id, program.realm.realm_generation) {
        return unavailable_program_locations();
    }
    match executor.debugger_safe_points(
        tab_id,
        program.realm.realm_generation,
        program.program_handle,
        program.program_generation,
    ) {
        Ok(safe_points) => DebuggerReply::SafePoints(
            safe_points
                .into_iter()
                .map(|safe_point| DebuggerSafePoint {
                    program,
                    code_unit_ordinal: safe_point.code_unit_ordinal,
                    bytecode_offset: safe_point.bytecode_offset,
                })
                .collect(),
        ),
        Err(error) => debugger_program_error(error),
    }
}

pub(super) fn validate_safe_point(
    tabs: &TabManager,
    javascript_executor: Option<&JavaScriptPageExecutor>,
    safe_point: DebuggerSafePoint,
) -> DebuggerReply {
    if !safe_point.is_well_formed() {
        return DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            message: "invalid debugger safe-point target".to_string(),
        };
    }
    let tab_id = match resolve_live_realm(tabs, safe_point.program.realm) {
        Ok(tab_id) => tab_id,
        Err(reply) => return *reply,
    };
    let Some(executor) = javascript_executor else {
        return unavailable_program_locations();
    };
    if !executor.debugger_has_live_realm(tab_id, safe_point.program.realm.realm_generation) {
        return unavailable_program_locations();
    }
    match executor.validate_debugger_safe_point(
        tab_id,
        safe_point.program.realm.realm_generation,
        safe_point.program.program_handle,
        safe_point.program.program_generation,
        safe_point.code_unit_ordinal,
        safe_point.bytecode_offset,
    ) {
        Ok(()) => DebuggerReply::SafePointValidated { safe_point },
        Err(error) => debugger_program_error(error),
    }
}

pub(super) fn set_breakpoint(
    tabs: &TabManager,
    javascript_executor: Option<&mut JavaScriptPageExecutor>,
    safe_point: DebuggerSafePoint,
) -> DebuggerReply {
    if !safe_point.is_well_formed() {
        return invalid_safe_point_target();
    }
    let tab_id = match resolve_live_realm(tabs, safe_point.program.realm) {
        Ok(tab_id) => tab_id,
        Err(reply) => return *reply,
    };
    let Some(executor) = javascript_executor else {
        return unavailable_breakpoint_configuration();
    };
    if !executor.debugger_has_live_realm(tab_id, safe_point.program.realm.realm_generation) {
        return unavailable_breakpoint_configuration();
    }
    match executor.set_debugger_breakpoint(
        tab_id,
        safe_point.program.realm.realm_generation,
        safe_point.program.program_handle,
        safe_point.program.program_generation,
        safe_point.code_unit_ordinal,
        safe_point.bytecode_offset,
    ) {
        Ok(()) => DebuggerReply::BreakpointSet { safe_point },
        Err(error) => debugger_program_error(error),
    }
}

pub(super) fn arm_entry_breakpoint(
    tabs: &TabManager,
    javascript_executor: Option<&mut JavaScriptPageExecutor>,
    safe_point: DebuggerSafePoint,
) -> DebuggerReply {
    if !safe_point.is_well_formed() {
        return invalid_safe_point_target();
    }
    let tab_id = match resolve_live_realm(tabs, safe_point.program.realm) {
        Ok(tab_id) => tab_id,
        Err(reply) => return *reply,
    };
    let Some(executor) = javascript_executor else {
        return unavailable_execution_control();
    };
    if !executor.debugger_has_live_realm(tab_id, safe_point.program.realm.realm_generation) {
        return unavailable_execution_control();
    }
    match executor.arm_debugger_entry_breakpoint(
        tab_id,
        safe_point.program.realm.realm_generation,
        safe_point.program.program_handle,
        safe_point.program.program_generation,
        safe_point.code_unit_ordinal,
        safe_point.bytecode_offset,
    ) {
        Ok(()) => DebuggerReply::BreakpointArmed { safe_point },
        Err(error) => debugger_program_error(error),
    }
}

/// Arms the bounded continuation seam at an exact root-code-unit boundary.
/// This deliberately has its own request/reply instead of making ordinary
/// breakpoint configuration appear to interrupt a synchronous VM.
pub(super) fn arm_root_safe_point_breakpoint(
    tabs: &TabManager,
    javascript_executor: Option<&mut JavaScriptPageExecutor>,
    safe_point: DebuggerSafePoint,
) -> DebuggerReply {
    if !safe_point.is_well_formed() {
        return invalid_safe_point_target();
    }
    let tab_id = match resolve_live_realm(tabs, safe_point.program.realm) {
        Ok(tab_id) => tab_id,
        Err(reply) => return *reply,
    };
    let Some(executor) = javascript_executor else {
        return unavailable_execution_control();
    };
    if !executor.debugger_has_live_realm(tab_id, safe_point.program.realm.realm_generation) {
        return unavailable_execution_control();
    }
    match executor.arm_debugger_root_safe_point_breakpoint(
        tab_id,
        safe_point.program.realm.realm_generation,
        safe_point.program.program_handle,
        safe_point.program.program_generation,
        safe_point.code_unit_ordinal,
        safe_point.bytecode_offset,
    ) {
        Ok(()) => DebuggerReply::RootSafePointBreakpointArmed { safe_point },
        Err(error) => debugger_program_error(error),
    }
}

pub(super) fn list_breakpoints(
    tabs: &TabManager,
    javascript_executor: Option<&JavaScriptPageExecutor>,
    realm: DebuggerPageRealm,
) -> DebuggerReply {
    let tab_id = match resolve_live_realm(tabs, realm) {
        Ok(tab_id) => tab_id,
        Err(reply) => return *reply,
    };
    let Some(executor) = javascript_executor else {
        return unavailable_breakpoint_configuration();
    };
    if !executor.debugger_has_live_realm(tab_id, realm.realm_generation) {
        return unavailable_breakpoint_configuration();
    }
    match executor.debugger_breakpoints(tab_id, realm.realm_generation) {
        Ok(breakpoints) => DebuggerReply::Breakpoints(
            breakpoints
                .into_iter()
                .map(|breakpoint| DebuggerSafePoint {
                    program: DebuggerProgram {
                        realm,
                        program_handle: breakpoint.program_handle,
                        program_generation: breakpoint.program_generation,
                    },
                    code_unit_ordinal: breakpoint.code_unit_ordinal,
                    bytecode_offset: breakpoint.bytecode_offset,
                })
                .collect(),
        ),
        Err(error) => debugger_program_error(error),
    }
}

pub(super) fn clear_breakpoint(
    tabs: &TabManager,
    javascript_executor: Option<&mut JavaScriptPageExecutor>,
    safe_point: DebuggerSafePoint,
) -> DebuggerReply {
    if !safe_point.is_well_formed() {
        return invalid_safe_point_target();
    }
    let tab_id = match resolve_live_realm(tabs, safe_point.program.realm) {
        Ok(tab_id) => tab_id,
        Err(reply) => return *reply,
    };
    let Some(executor) = javascript_executor else {
        return unavailable_breakpoint_configuration();
    };
    if !executor.debugger_has_live_realm(tab_id, safe_point.program.realm.realm_generation) {
        return unavailable_breakpoint_configuration();
    }
    match executor.clear_debugger_breakpoint(
        tab_id,
        safe_point.program.realm.realm_generation,
        safe_point.program.program_handle,
        safe_point.program.program_generation,
        safe_point.code_unit_ordinal,
        safe_point.bytecode_offset,
    ) {
        Ok(was_present) => DebuggerReply::BreakpointCleared {
            safe_point,
            was_present,
        },
        Err(error) => debugger_program_error(error),
    }
}

pub(super) fn execution_state(
    tabs: &TabManager,
    javascript_executor: Option<&JavaScriptPageExecutor>,
    program: DebuggerProgram,
) -> DebuggerReply {
    if !program.is_well_formed() {
        return DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            message: "invalid debugger program target".to_string(),
        };
    }
    let tab_id = match resolve_live_realm(tabs, program.realm) {
        Ok(tab_id) => tab_id,
        Err(reply) => return *reply,
    };
    let Some(executor) = javascript_executor else {
        return unavailable_execution_control();
    };
    if !executor.debugger_has_live_realm(tab_id, program.realm.realm_generation) {
        return unavailable_execution_control();
    }
    match executor.debugger_execution_state(
        tab_id,
        program.realm.realm_generation,
        program.program_handle,
        program.program_generation,
    ) {
        Ok(state) => DebuggerReply::ExecutionState {
            program,
            state: debugger_execution_state(state, program),
        },
        Err(error) => debugger_program_error(error),
    }
}

pub(super) fn resume_execution(
    tabs: &TabManager,
    javascript_executor: Option<&mut JavaScriptPageExecutor>,
    program: DebuggerProgram,
) -> DebuggerReply {
    if !program.is_well_formed() {
        return DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            message: "invalid debugger program target".to_string(),
        };
    }
    let tab_id = match resolve_live_realm(tabs, program.realm) {
        Ok(tab_id) => tab_id,
        Err(reply) => return *reply,
    };
    let Some(executor) = javascript_executor else {
        return unavailable_execution_control();
    };
    if !executor.debugger_has_live_realm(tab_id, program.realm.realm_generation) {
        return unavailable_execution_control();
    }
    match executor.resume_debugger_execution(
        tab_id,
        program.realm.realm_generation,
        program.program_handle,
        program.program_generation,
    ) {
        Ok(()) => DebuggerReply::ExecutionResumed { program },
        Err(error) => debugger_program_error(error),
    }
}

pub(super) fn step_root_instruction(
    tabs: &TabManager,
    javascript_executor: Option<&mut JavaScriptPageExecutor>,
    program: DebuggerProgram,
) -> DebuggerReply {
    if !program.is_well_formed() {
        return DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            message: "invalid debugger program target".to_string(),
        };
    }
    let tab_id = match resolve_live_realm(tabs, program.realm) {
        Ok(tab_id) => tab_id,
        Err(reply) => return *reply,
    };
    let Some(executor) = javascript_executor else {
        return unavailable_stepping();
    };
    if !executor.debugger_has_live_realm(tab_id, program.realm.realm_generation)
        || !executor.debugger_execution_control_available()
    {
        return unavailable_stepping();
    }
    match executor.step_debugger_root_instruction(
        tab_id,
        program.realm.realm_generation,
        program.program_handle,
        program.program_generation,
    ) {
        Ok(()) => DebuggerReply::ExecutionStepRequested { program },
        Err(error) => debugger_program_error(error),
    }
}

pub(super) fn invalid_safe_point_target() -> DebuggerReply {
    DebuggerReply::Error {
        code: DebuggerErrorCode::InvalidTarget,
        message: "invalid debugger safe-point target".to_string(),
    }
}

pub(super) fn unavailable_program_locations() -> DebuggerReply {
    DebuggerReply::Error {
        code: DebuggerErrorCode::CapabilityUnavailable,
        message: "native debugger program locations require an enabled live JavaScript page realm"
            .to_string(),
    }
}

pub(super) fn unavailable_breakpoint_configuration() -> DebuggerReply {
    DebuggerReply::Error {
        code: DebuggerErrorCode::CapabilityUnavailable,
        message: "native breakpoint configuration requires an enabled live JavaScript page realm"
            .to_string(),
    }
}

pub(super) fn unavailable_static_metadata_inventory() -> DebuggerReply {
    DebuggerReply::Error {
        code: DebuggerErrorCode::CapabilityUnavailable,
        message: "opaque debugger static metadata inventory is not authorized for this session and live realm"
            .to_string(),
    }
}

pub(super) fn unavailable_static_metadata_summary() -> DebuggerReply {
    DebuggerReply::Error {
        code: DebuggerErrorCode::CapabilityUnavailable,
        message: "bounded debugger static metadata summaries are not authorized for this session and live realm"
            .to_string(),
    }
}

pub(super) fn unavailable_static_metadata_source_inventory() -> DebuggerReply {
    DebuggerReply::Error {
        code: DebuggerErrorCode::CapabilityUnavailable,
        message: "opaque debugger static metadata source inventory is not authorized for this session and live realm"
            .to_string(),
    }
}

pub(super) fn unavailable_static_metadata_type_inventory() -> DebuggerReply {
    DebuggerReply::Error {
        code: DebuggerErrorCode::CapabilityUnavailable,
        message: "opaque debugger static metadata type inventory is not authorized for this session and live realm"
            .to_string(),
    }
}

pub(super) fn unavailable_static_metadata_type_display() -> DebuggerReply {
    DebuggerReply::Error {
        code: DebuggerErrorCode::CapabilityUnavailable,
        message: "opaque debugger static metadata type display is not authorized for this session and live realm"
            .to_string(),
    }
}

pub(super) fn unavailable_static_metadata_symbol_inventory() -> DebuggerReply {
    DebuggerReply::Error {
        code: DebuggerErrorCode::CapabilityUnavailable,
        message: "opaque debugger static metadata symbol inventory is not authorized for this session and live realm"
            .to_string(),
    }
}

pub(super) fn unavailable_static_metadata_contract_inventory() -> DebuggerReply {
    DebuggerReply::Error {
        code: DebuggerErrorCode::CapabilityUnavailable,
        message: "opaque debugger static metadata contract inventory is not authorized for this session and live realm"
            .to_string(),
    }
}

pub(super) fn unavailable_static_metadata_contract_display() -> DebuggerReply {
    DebuggerReply::Error {
        code: DebuggerErrorCode::CapabilityUnavailable,
        message: "opaque debugger static metadata contract display is not authorized for this session and live realm"
            .to_string(),
    }
}

pub(super) fn unavailable_static_metadata_contract_validation() -> DebuggerReply {
    DebuggerReply::Error {
        code: DebuggerErrorCode::CapabilityUnavailable,
        message: "opaque debugger static metadata contract validation is not authorized for this session and live realm"
            .to_string(),
    }
}

pub(super) fn unavailable_static_metadata_lowering_summary() -> DebuggerReply {
    DebuggerReply::Error {
        code: DebuggerErrorCode::CapabilityUnavailable,
        message: "opaque debugger static metadata lowering summary is not authorized for this session and live realm"
            .to_string(),
    }
}

pub(super) fn unavailable_static_metadata_symbol_display() -> DebuggerReply {
    DebuggerReply::Error {
        code: DebuggerErrorCode::CapabilityUnavailable,
        message: "opaque debugger static metadata symbol display is not authorized for this session and live realm"
            .to_string(),
    }
}

pub(super) fn unavailable_static_metadata_symbol_location() -> DebuggerReply {
    DebuggerReply::Unsupported {
        operation: "describe static metadata symbol location".to_string(),
        reason: "static metadata symbol locations require an explicitly negotiated owner grant, prior symbol/source receipts, and a live BlueTS child program".to_string(),
    }
}

pub(super) fn unavailable_static_metadata_safe_point_span() -> DebuggerReply {
    DebuggerReply::Unsupported {
        operation: "describe static metadata safe-point span".to_string(),
        reason: "exact BlueTS safe-point spans require a separate owner/client grant, a prior same-stream source-ID receipt, and a live child attachment".to_string(),
    }
}

pub(super) fn unavailable_exception_location() -> DebuggerReply {
    DebuggerReply::Error {
        code: DebuggerErrorCode::CapabilityUnavailable,
        message: "BlueTS exception locations require a separate owner/client span grant and live child route".to_string(),
    }
}

pub(super) fn invalid_exception_location() -> DebuggerReply {
    DebuggerReply::Error {
        code: DebuggerErrorCode::InvalidTarget,
        message: "invalid BlueTS exception location target".to_string(),
    }
}

pub(super) fn unavailable_static_metadata_source_span_step() -> DebuggerReply {
    DebuggerReply::Unsupported {
        operation: "step static metadata source span".to_string(),
        reason: "BlueTS source-span stepping requires independent owner/client and span grants, prior same-stream metadata/source receipts, and a paused live child root".to_string(),
    }
}

pub(super) fn unavailable_static_metadata_source_breakpoint() -> DebuggerReply {
    DebuggerReply::Unsupported {
        operation: "resolve static metadata source breakpoint".to_string(),
        reason: "BlueTS source-position binding requires a separate owner/client grant, a prior same-stream source-ID receipt, and a live child attachment".to_string(),
    }
}

pub(super) fn unavailable_static_metadata_source_breakpoint_arm() -> DebuggerReply {
    DebuggerReply::Unsupported {
        operation: "arm static metadata source breakpoint".to_string(),
        reason: "BlueTS source-position arm requires a separate owner/client binding grant, a prior same-stream source-ID receipt, a live child attachment, and execution control".to_string(),
    }
}

pub(super) fn unavailable_static_metadata_contract_location() -> DebuggerReply {
    DebuggerReply::Unsupported {
        operation: "describe static metadata contract location".to_string(),
        reason: "static metadata contract locations require an explicitly negotiated owner grant, prior contract/source receipts, and a live BlueTS child program".to_string(),
    }
}

pub(super) fn unavailable_static_metadata_symbol_type() -> DebuggerReply {
    DebuggerReply::Unsupported {
        operation: "describe static metadata symbol type".to_string(),
        reason: "static metadata symbol types require an explicitly negotiated owner grant, prior symbol/type receipts, and a live BlueTS child program".to_string(),
    }
}

pub(super) fn unavailable_static_metadata_symbol_contract() -> DebuggerReply {
    DebuggerReply::Unsupported {
        operation: "describe static metadata symbol contract".to_string(),
        reason: "static metadata symbol contracts require an explicitly negotiated owner grant, prior symbol/contract receipts, and a live BlueTS child program".to_string(),
    }
}

pub(super) fn unavailable_static_metadata_source_provenance() -> DebuggerReply {
    DebuggerReply::Error {
        code: DebuggerErrorCode::CapabilityUnavailable,
        message: "debugger static metadata source provenance is not authorized for this session and live realm"
            .to_string(),
    }
}

pub(super) fn unavailable_execution_control() -> DebuggerReply {
    DebuggerReply::Error {
        code: DebuggerErrorCode::CapabilityUnavailable,
        message: "native debugger entry pause/resume requires an explicitly enabled JavaScript page realm"
            .to_string(),
    }
}

pub(super) fn unavailable_stepping() -> DebuggerReply {
    DebuggerReply::Error {
        code: DebuggerErrorCode::CapabilityUnavailable,
        message: "native debugger root stepping is not installed for this page-host route"
            .to_string(),
    }
}

pub(super) fn unavailable_nested_frames() -> DebuggerReply {
    DebuggerReply::Error {
        code: DebuggerErrorCode::CapabilityUnavailable,
        message: "native debugger nested-frame control is not installed for this page-host route"
            .to_string(),
    }
}

pub(super) fn unavailable_linked_modules() -> DebuggerReply {
    DebuggerReply::Error {
        code: DebuggerErrorCode::CapabilityUnavailable,
        message: "linked module debugger control is unavailable".to_string(),
    }
}

pub(super) fn unavailable_stack() -> DebuggerReply {
    DebuggerReply::Error {
        code: DebuggerErrorCode::CapabilityUnavailable,
        message: "bounded debugger stack inspection is not installed for this page-host route"
            .to_string(),
    }
}

pub(super) fn unavailable_scopes() -> DebuggerReply {
    DebuggerReply::Error {
        code: DebuggerErrorCode::CapabilityUnavailable,
        message: "bounded debugger scope inspection is not installed for this page-host route"
            .to_string(),
    }
}

pub(super) fn unavailable_values() -> DebuggerReply {
    DebuggerReply::Error {
        code: DebuggerErrorCode::CapabilityUnavailable,
        message: "bounded debugger values require an owner/client grant and a live child route"
            .to_string(),
    }
}

pub(super) fn unavailable_static_scope_relation() -> DebuggerReply {
    DebuggerReply::Error {
        code: DebuggerErrorCode::CapabilityUnavailable,
        message:
            "static scope relations require an independent owner/client grant and live child route"
                .to_string(),
    }
}

pub(super) fn debugger_program_error(error: JavaScriptPageDebuggerError) -> DebuggerReply {
    let (code, message) = match error {
        JavaScriptPageDebuggerError::NoLiveRealm => (
            DebuggerErrorCode::CapabilityUnavailable,
            "native debugger program locations require an enabled live JavaScript page realm",
        ),
        JavaScriptPageDebuggerError::UnknownProgram => {
            (DebuggerErrorCode::InvalidTarget, "unknown debugger program")
        }
        JavaScriptPageDebuggerError::StaleProgram => (
            DebuggerErrorCode::StaleProgram,
            "stale debugger program generation",
        ),
        JavaScriptPageDebuggerError::InvalidSafePoint => (
            DebuggerErrorCode::InvalidSafePoint,
            "invalid debugger instruction boundary",
        ),
        JavaScriptPageDebuggerError::ResourceLimit => (
            DebuggerErrorCode::ResourceLimit,
            "too many verified debugger safe points for one program",
        ),
        JavaScriptPageDebuggerError::BreakpointLimit => (
            DebuggerErrorCode::ResourceLimit,
            "too many native breakpoint records for one page realm",
        ),
        JavaScriptPageDebuggerError::ExecutionControlUnavailable => (
            DebuggerErrorCode::CapabilityUnavailable,
            "native debugger entry pause/resume is not enabled for this page realm",
        ),
        JavaScriptPageDebuggerError::NotExecutableEntry => (
            DebuggerErrorCode::InvalidSafePoint,
            "debugger entry pause accepts only a pending root instruction boundary",
        ),
        JavaScriptPageDebuggerError::NotResumableRootSafePoint => (
            DebuggerErrorCode::InvalidSafePoint,
            "debugger root continuation accepts only a pending classic-script root code-unit boundary",
        ),
        JavaScriptPageDebuggerError::InvalidExecutionState => (
            DebuggerErrorCode::InvalidExecutionState,
            "debugger operation is not valid for the program execution state",
        ),
    };
    DebuggerReply::Error {
        code,
        message: message.to_string(),
    }
}

pub(super) fn debugger_execution_state(
    state: JavaScriptPageDebuggerExecutionState,
    program: DebuggerProgram,
) -> DebuggerExecutionState {
    match state {
        JavaScriptPageDebuggerExecutionState::Pending => DebuggerExecutionState::Pending,
        JavaScriptPageDebuggerExecutionState::Paused {
            code_unit_ordinal,
            bytecode_offset,
        } => DebuggerExecutionState::Paused {
            safe_point: DebuggerSafePoint {
                program,
                code_unit_ordinal,
                bytecode_offset,
            },
        },
        JavaScriptPageDebuggerExecutionState::SourceStepLimitReached {
            code_unit_ordinal,
            bytecode_offset,
        } => DebuggerExecutionState::SourceStepLimitReached {
            safe_point: DebuggerSafePoint {
                program,
                code_unit_ordinal,
                bytecode_offset,
            },
        },
        JavaScriptPageDebuggerExecutionState::Stepping => DebuggerExecutionState::Stepping,
        JavaScriptPageDebuggerExecutionState::Resuming => DebuggerExecutionState::Resuming,
        JavaScriptPageDebuggerExecutionState::Completed => DebuggerExecutionState::Completed,
    }
}

pub(super) struct DebuggerCapabilityAvailability {
    pub(super) program_locations_available: bool,
    pub(super) breakpoint_configuration_available: bool,
    pub(super) entry_execution_control_available: bool,
    pub(super) stepping_available: bool,
    pub(super) nested_frames_available: bool,
    pub(super) linked_modules_available: bool,
    pub(super) stack_available: bool,
    pub(super) scopes_available: bool,
    pub(super) values_available: bool,
    pub(super) static_metadata_inventory_available: bool,
    pub(super) static_metadata_summary_available: bool,
    pub(super) static_metadata_lowering_summary_available: bool,
    pub(super) static_metadata_source_inventory_available: bool,
    pub(super) static_metadata_source_provenance_available: bool,
    pub(super) static_metadata_type_inventory_available: bool,
    pub(super) static_metadata_type_display_available: bool,
    pub(super) static_metadata_symbol_inventory_available: bool,
    pub(super) static_metadata_contract_inventory_available: bool,
    pub(super) static_metadata_contract_display_available: bool,
    pub(super) static_metadata_contract_validation_available: bool,
    pub(super) static_metadata_symbol_display_available: bool,
    pub(super) static_metadata_symbol_location_available: bool,
    pub(super) static_metadata_safe_point_span_available: bool,
    pub(super) static_metadata_source_span_step_available: bool,
    pub(super) static_metadata_source_breakpoint_available: bool,
    pub(super) static_metadata_contract_location_available: bool,
    pub(super) static_metadata_symbol_type_available: bool,
    pub(super) static_metadata_symbol_contract_available: bool,
    pub(super) static_scope_relation_available: bool,
}

pub(super) fn capability_reports(
    DebuggerCapabilityAvailability {
        program_locations_available,
        breakpoint_configuration_available,
        entry_execution_control_available,
        stepping_available,
        nested_frames_available,
        linked_modules_available,
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
    }: DebuggerCapabilityAvailability,
) -> Vec<DebuggerCapabilityReport> {
    [
        (
            DebuggerCapability::ProgramLocations,
            if program_locations_available {
                DebuggerCapabilityState::Available
            } else {
                DebuggerCapabilityState::Planned
            },
            if program_locations_available {
                "opaque program and exact safe-point validation are installed"
            } else {
                "exact program-location validation requires an enabled JavaScript page realm"
            },
        ),
        (
            DebuggerCapability::BreakpointConfiguration,
            if breakpoint_configuration_available {
                DebuggerCapabilityState::Available
            } else {
                DebuggerCapabilityState::Planned
            },
            if breakpoint_configuration_available {
                "bounded exact breakpoint configuration is installed; it does not interrupt execution"
            } else {
                "native breakpoint configuration is not installed for this page-host route"
            },
        ),
        (
            DebuggerCapability::Breakpoints,
            if entry_execution_control_available {
                DebuggerCapabilityState::Available
            } else {
                DebuggerCapabilityState::Planned
            },
            if entry_execution_control_available {
                "compiler-verified root-code-unit breakpoints pause pending classic page scripts"
            } else {
                "native breakpoint interruption is not installed"
            },
        ),
        (
            DebuggerCapability::PauseResume,
            if entry_execution_control_available {
                DebuggerCapabilityState::Available
            } else {
                DebuggerCapabilityState::Planned
            },
            if entry_execution_control_available {
                "resume preserves an exactly paused root frame; nested-frame resume is separately gated"
            } else {
                "native pause and resume are not installed"
            },
        ),
        (
            DebuggerCapability::Stepping,
            if stepping_available {
                DebuggerCapabilityState::Available
            } else {
                DebuggerCapabilityState::Planned
            },
            if stepping_available {
                "one root instruction step retains the same BlueJS continuation; nested frames require their distinct capability"
            } else {
                "native stepping is not installed for this page-host route"
            },
        ),
        (
            DebuggerCapability::NestedFrames,
            if nested_frames_available {
                DebuggerCapabilityState::Available
            } else {
                DebuggerCapabilityState::Planned
            },
            if nested_frames_available {
                "one exact synchronous nested invocation can pause, step, and resume under a core-owned frame handle"
            } else {
                "nested-frame pause, step, and resume are not installed for this page-host route"
            },
        ),
        (
            DebuggerCapability::LinkedModules,
            if linked_modules_available {
                DebuggerCapabilityState::Available
            } else {
                DebuggerCapabilityState::Planned
            },
            if linked_modules_available {
                "one linked dependency/entry pause retains two separate core-owned program frames"
            } else {
                "linked module pause and complete two-frame stack are not installed"
            },
        ),
        (
            DebuggerCapability::Stack,
            if stack_available {
                DebuggerCapabilityState::Available
            } else {
                DebuggerCapabilityState::Planned
            },
            if stack_available {
                "bounded paused stack locations are installed without scope or values"
            } else {
                "native stack inspection is not installed"
            },
        ),
        (
            DebuggerCapability::Scopes,
            if scopes_available {
                DebuggerCapabilityState::Available
            } else {
                DebuggerCapabilityState::Planned
            },
            if scopes_available {
                "bounded active lexical slot inspection is installed without names or values"
            } else {
                "native scope inspection is not installed"
            },
        ),
        (
            DebuggerCapability::ExceptionPolicy,
            DebuggerCapabilityState::Planned,
            "native exception policy is not installed",
        ),
        (
            DebuggerCapability::BoundedValues,
            if values_available {
                DebuggerCapabilityState::Available
            } else {
                DebuggerCapabilityState::Planned
            },
            if values_available {
                "bounded paused-slot value reads require this stream's grant and Scopes receipt"
            } else {
                "native value inspection requires a separate owner/client grant and live child route"
            },
        ),
        (
            DebuggerCapability::StaticMetadataInventory,
            if static_metadata_inventory_available {
                DebuggerCapabilityState::Available
            } else {
                DebuggerCapabilityState::Planned
            },
            if static_metadata_inventory_available {
                "bounded opaque static-metadata handle inventory is installed; metadata remains unreadable"
            } else {
                "static metadata inventory requires an explicitly negotiated session grant and a live BlueTS child program"
            },
        ),
        (
            DebuggerCapability::StaticMetadataSummary,
            if static_metadata_summary_available {
                DebuggerCapabilityState::Available
            } else {
                DebuggerCapabilityState::Planned
            },
            if static_metadata_summary_available {
                "bounded source-free static-metadata summaries are installed; records remain unreadable"
            } else {
                "static metadata summaries require explicit inventory and summary session grants plus a live BlueTS child program"
            },
        ),
        (
            DebuggerCapability::StaticMetadataLoweringSummary,
            if static_metadata_lowering_summary_available {
                DebuggerCapabilityState::Available
            } else {
                DebuggerCapabilityState::Planned
            },
            if static_metadata_lowering_summary_available {
                "verified direct-lowering-map ABI and aggregate evidence is installed; map entries remain unreadable"
            } else {
                "static metadata lowering summaries require explicit inventory and lowering-summary session grants plus a live BlueTS child program"
            },
        ),
        (
            DebuggerCapability::StaticMetadataSourceInventory,
            if static_metadata_source_inventory_available {
                DebuggerCapabilityState::Available
            } else {
                DebuggerCapabilityState::Planned
            },
            if static_metadata_source_inventory_available {
                "bounded opaque static-metadata source identities are installed; source details remain unreadable"
            } else {
                "static metadata source identities require explicit inventory and source-inventory session grants plus a live BlueTS child program"
            },
        ),
        (
            DebuggerCapability::StaticMetadataSourceProvenance,
            if static_metadata_source_provenance_available {
                DebuggerCapabilityState::Available
            } else {
                DebuggerCapabilityState::Planned
            },
            if static_metadata_source_provenance_available {
                "owner-authorized source-free module identity and SHA-256 provenance are installed"
            } else {
                "source provenance requires explicit inventory, source-inventory, and provenance session grants plus a live BlueTS child program"
            },
        ),
        (
            DebuggerCapability::StaticMetadataTypeInventory,
            if static_metadata_type_inventory_available {
                DebuggerCapabilityState::Available
            } else {
                DebuggerCapabilityState::Planned
            },
            if static_metadata_type_inventory_available {
                "bounded opaque static-metadata type identities are installed; type displays remain unreadable"
            } else {
                "static metadata type identities require explicit inventory and type-inventory session grants plus a live BlueTS child program"
            },
        ),
        (
            DebuggerCapability::StaticMetadataTypeDisplay,
            if static_metadata_type_display_available {
                DebuggerCapabilityState::Available
            } else {
                DebuggerCapabilityState::Planned
            },
            if static_metadata_type_display_available {
                "bounded compiler-produced static type displays are installed for prior type-ID receipts"
            } else {
                "static metadata type displays require explicit inventory, type-inventory, and type-display session grants plus a live BlueTS child program"
            },
        ),
        (
            DebuggerCapability::StaticMetadataSymbolInventory,
            if static_metadata_symbol_inventory_available {
                DebuggerCapabilityState::Available
            } else {
                DebuggerCapabilityState::Planned
            },
            if static_metadata_symbol_inventory_available {
                "bounded opaque static-metadata symbol identities are installed; symbol records remain unreadable"
            } else {
                "static metadata symbol identities require explicit inventory and symbol-inventory session grants plus a live BlueTS child program"
            },
        ),
        (
            DebuggerCapability::StaticMetadataContractInventory,
            if static_metadata_contract_inventory_available {
                DebuggerCapabilityState::Available
            } else {
                DebuggerCapabilityState::Planned
            },
            if static_metadata_contract_inventory_available {
                "bounded opaque static-metadata contract identities are installed; contract records remain unreadable"
            } else {
                "static metadata contract identities require explicit inventory and contract-inventory session grants plus a live BlueTS child program"
            },
        ),
        (
            DebuggerCapability::StaticMetadataContractDisplay,
            if static_metadata_contract_display_available {
                DebuggerCapabilityState::Available
            } else {
                DebuggerCapabilityState::Planned
            },
            if static_metadata_contract_display_available {
                "bounded compiler-produced static contract displays are installed for prior contract-ID receipts"
            } else {
                "static metadata contract displays require explicit inventory, contract-inventory, and contract-display session grants plus a live BlueTS child program"
            },
        ),
        (
            DebuggerCapability::StaticMetadataContractValidation,
            if static_metadata_contract_validation_available {
                DebuggerCapabilityState::Available
            } else {
                DebuggerCapabilityState::Planned
            },
            if static_metadata_contract_validation_available {
                "bounded data-only static contract validation is installed for prior contract-ID receipts; it returns only a boolean"
            } else {
                "static metadata contract validation requires explicit inventory, contract-inventory, and contract-validation session grants plus a live BlueTS child program"
            },
        ),
        (
            DebuggerCapability::StaticMetadataSymbolDisplay,
            if static_metadata_symbol_display_available {
                DebuggerCapabilityState::Available
            } else {
                DebuggerCapabilityState::Planned
            },
            if static_metadata_symbol_display_available {
                "bounded compiler-produced static symbol displays are installed for prior symbol-ID receipts"
            } else {
                "static metadata symbol displays require explicit inventory, symbol-inventory, and symbol-display session grants plus a live BlueTS child program"
            },
        ),
        (
            DebuggerCapability::StaticMetadataSymbolLocation,
            if static_metadata_symbol_location_available {
                DebuggerCapabilityState::Available
            } else {
                DebuggerCapabilityState::Planned
            },
            if static_metadata_symbol_location_available {
                "bounded source-text-free static symbol locations are installed for prior symbol and source-ID receipts"
            } else {
                "static metadata symbol locations require explicit inventory, source-inventory, symbol-inventory, and symbol-location session grants plus a live BlueTS child program"
            },
        ),
        (
            DebuggerCapability::StaticMetadataSafePointSpan,
            if static_metadata_safe_point_span_available {
                DebuggerCapabilityState::Available
            } else {
                DebuggerCapabilityState::Planned
            },
            if static_metadata_safe_point_span_available {
                "exact BlueTS safe-point spans are installed for prior metadata and source-ID receipts"
            } else {
                "exact BlueTS safe-point spans require separate inventory, source-inventory, and span grants plus a live child attachment"
            },
        ),
        (
            DebuggerCapability::StaticMetadataSourceSpanStep,
            if static_metadata_source_span_step_available {
                DebuggerCapabilityState::Available
            } else {
                DebuggerCapabilityState::Planned
            },
            if static_metadata_source_span_step_available {
                "exact BlueTS source-span stepping is installed for paused root safe points under separate metadata and execution grants"
            } else {
                "BlueTS source-span stepping requires independent owner/client and safe-point-span grants plus a live stepping child"
            },
        ),
        (
            DebuggerCapability::StaticMetadataSourceBreakpoint,
            if static_metadata_source_breakpoint_available {
                DebuggerCapabilityState::Available
            } else {
                DebuggerCapabilityState::Planned
            },
            if static_metadata_source_breakpoint_available {
                "bounded BlueTS source positions resolve to exact safe points or explicit unbound results under separate receipts; atomic classic/module root arm also requires execution control"
            } else {
                "BlueTS source-position binding requires independent inventory, source-inventory, and binding grants plus a live child attachment"
            },
        ),
        (
            DebuggerCapability::StaticMetadataContractLocation,
            if static_metadata_contract_location_available {
                DebuggerCapabilityState::Available
            } else {
                DebuggerCapabilityState::Planned
            },
            if static_metadata_contract_location_available {
                "bounded source-text-free static contract locations are installed for prior contract and source-ID receipts"
            } else {
                "static metadata contract locations require explicit inventory, source-inventory, contract-inventory, and contract-location session grants plus a live BlueTS child program"
            },
        ),
        (
            DebuggerCapability::StaticMetadataSymbolType,
            if static_metadata_symbol_type_available {
                DebuggerCapabilityState::Available
            } else {
                DebuggerCapabilityState::Planned
            },
            if static_metadata_symbol_type_available {
                "compiler-verified symbol-to-type relations are installed for prior symbol and type-ID receipts"
            } else {
                "static metadata symbol types require explicit inventory, symbol-inventory, type-inventory, and symbol-type session grants plus a live BlueTS child program"
            },
        ),
        (
            DebuggerCapability::StaticMetadataSymbolContract,
            if static_metadata_symbol_contract_available {
                DebuggerCapabilityState::Available
            } else {
                DebuggerCapabilityState::Planned
            },
            if static_metadata_symbol_contract_available {
                "compiler-verified symbol-to-contract relations are installed for prior symbol and contract-ID receipts"
            } else {
                "static metadata symbol contracts require explicit inventory, symbol-inventory, contract-inventory, and symbol-contract session grants plus a live BlueTS child program"
            },
        ),
        (
            DebuggerCapability::StaticScopeRelation,
            if static_scope_relation_available {
                DebuggerCapabilityState::Available
            } else {
                DebuggerCapabilityState::Planned
            },
            if static_scope_relation_available {
                "compiler-only relations for receipted paused lexical slots are installed"
            } else {
                "static scope relations require independent owner/client and inventory grants plus a live child scope route"
            },
        ),
    ]
    .into_iter()
    .map(|(capability, state, detail)| DebuggerCapabilityReport {
        capability,
        state,
        detail: detail.to_string(),
    })
    .collect()
}
