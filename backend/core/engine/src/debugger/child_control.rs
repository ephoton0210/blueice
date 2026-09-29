// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

pub(super) fn list_child_programs(
    tabs: &TabManager,
    locations: &mut dyn PageJavaScriptDebuggerLocations,
    realm: DebuggerPageRealm,
) -> DebuggerReply {
    let tab_id = match resolve_live_realm(tabs, realm) {
        Ok(tab_id) => tab_id,
        Err(reply) => return *reply,
    };
    if !locations.debugger_has_live_realm(tab_id, realm.realm_generation) {
        return unavailable_program_locations();
    }
    match locations.debugger_programs(tab_id, realm.realm_generation) {
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

pub(super) fn list_child_safe_points(
    tabs: &TabManager,
    locations: &mut dyn PageJavaScriptDebuggerLocations,
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
    if !locations.debugger_has_live_realm(tab_id, program.realm.realm_generation) {
        return unavailable_program_locations();
    }
    match locations.debugger_safe_points(
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

pub(super) fn validate_child_safe_point(
    tabs: &TabManager,
    locations: &mut dyn PageJavaScriptDebuggerLocations,
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
    if !locations.debugger_has_live_realm(tab_id, safe_point.program.realm.realm_generation) {
        return unavailable_program_locations();
    }
    match locations.validate_debugger_safe_point(
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

pub(super) fn set_child_breakpoint(
    tabs: &TabManager,
    locations: &mut dyn PageJavaScriptDebuggerLocations,
    safe_point: DebuggerSafePoint,
) -> DebuggerReply {
    if !safe_point.is_well_formed() {
        return invalid_safe_point_target();
    }
    let tab_id = match resolve_live_realm(tabs, safe_point.program.realm) {
        Ok(tab_id) => tab_id,
        Err(reply) => return *reply,
    };
    if !locations.debugger_has_live_realm(tab_id, safe_point.program.realm.realm_generation)
        || !locations.debugger_breakpoint_configuration_available()
    {
        return unavailable_breakpoint_configuration();
    }
    match locations.set_debugger_breakpoint(
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

pub(super) fn list_child_breakpoints(
    tabs: &TabManager,
    locations: &mut dyn PageJavaScriptDebuggerLocations,
    realm: DebuggerPageRealm,
) -> DebuggerReply {
    let tab_id = match resolve_live_realm(tabs, realm) {
        Ok(tab_id) => tab_id,
        Err(reply) => return *reply,
    };
    if !locations.debugger_has_live_realm(tab_id, realm.realm_generation)
        || !locations.debugger_breakpoint_configuration_available()
    {
        return unavailable_breakpoint_configuration();
    }
    match locations.debugger_breakpoints(tab_id, realm.realm_generation) {
        Ok(breakpoints) => {
            if breakpoints.len() > locations.max_debugger_breakpoints_per_realm() {
                return DebuggerReply::Error {
                    code: DebuggerErrorCode::ResourceLimit,
                    message: "too many native breakpoint records for one page realm".to_string(),
                };
            }
            DebuggerReply::Breakpoints(
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
            )
        }
        Err(error) => debugger_program_error(error),
    }
}

pub(super) fn clear_child_breakpoint(
    tabs: &TabManager,
    locations: &mut dyn PageJavaScriptDebuggerLocations,
    safe_point: DebuggerSafePoint,
) -> DebuggerReply {
    if !safe_point.is_well_formed() {
        return invalid_safe_point_target();
    }
    let tab_id = match resolve_live_realm(tabs, safe_point.program.realm) {
        Ok(tab_id) => tab_id,
        Err(reply) => return *reply,
    };
    if !locations.debugger_has_live_realm(tab_id, safe_point.program.realm.realm_generation)
        || !locations.debugger_breakpoint_configuration_available()
    {
        return unavailable_breakpoint_configuration();
    }
    match locations.clear_debugger_breakpoint(
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

/// Routes only the one-shot child root continuation arm. The public
/// tuple is resolved by core before the child-private mapping can be used;
/// child code units and every generic VM interruption path stay unavailable.
pub(super) fn arm_child_root_safe_point_breakpoint(
    tabs: &TabManager,
    locations: &mut dyn PageJavaScriptDebuggerLocations,
    safe_point: DebuggerSafePoint,
) -> DebuggerReply {
    if !safe_point.is_well_formed() || safe_point.code_unit_ordinal != 0 {
        return invalid_safe_point_target();
    }
    let tab_id = match resolve_live_realm(tabs, safe_point.program.realm) {
        Ok(tab_id) => tab_id,
        Err(reply) => return *reply,
    };
    if !locations.debugger_has_live_realm(tab_id, safe_point.program.realm.realm_generation)
        || !locations.debugger_execution_control_available()
    {
        return unavailable_execution_control();
    }
    match locations.arm_debugger_root_safe_point_breakpoint(
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

pub(super) fn arm_child_nested_safe_point_breakpoint(
    tabs: &TabManager,
    locations: &mut dyn PageJavaScriptDebuggerLocations,
    safe_point: DebuggerSafePoint,
) -> DebuggerReply {
    if !safe_point.is_well_formed() || safe_point.code_unit_ordinal == 0 {
        return invalid_safe_point_target();
    }
    let tab_id = match resolve_live_realm(tabs, safe_point.program.realm) {
        Ok(tab_id) => tab_id,
        Err(reply) => return *reply,
    };
    if !locations.debugger_has_live_realm(tab_id, safe_point.program.realm.realm_generation)
        || !locations.debugger_nested_frames_available()
    {
        return unavailable_nested_frames();
    }
    if let Err(error) = locations.validate_debugger_safe_point(
        tab_id,
        safe_point.program.realm.realm_generation,
        safe_point.program.program_handle,
        safe_point.program.program_generation,
        safe_point.code_unit_ordinal,
        safe_point.bytecode_offset,
    ) {
        return debugger_program_error(error);
    }
    match locations.arm_debugger_nested_safe_point_breakpoint(
        tab_id,
        safe_point.program.realm.realm_generation,
        safe_point.program.program_handle,
        safe_point.program.program_generation,
        safe_point.code_unit_ordinal,
        safe_point.bytecode_offset,
    ) {
        Ok(()) => DebuggerReply::NestedSafePointBreakpointArmed { safe_point },
        Err(error) => debugger_program_error(error),
    }
}

pub(super) fn arm_child_linked_nested_safe_point_breakpoint(
    tabs: &TabManager,
    locations: &mut dyn PageJavaScriptDebuggerLocations,
    target: DebuggerLinkedArmTarget,
) -> DebuggerReply {
    if !target.is_well_formed() {
        return invalid_linked_target();
    }
    let realm = target.entry.realm;
    let tab_id = match resolve_live_realm(tabs, realm) {
        Ok(tab_id) => tab_id,
        Err(reply) => return *reply,
    };
    if !locations.debugger_has_live_realm(tab_id, realm.realm_generation)
        || !locations.debugger_linked_frames_available()
    {
        return unavailable_linked_modules();
    }
    let point = target.dependency_safe_point;
    if let Err(error) = locations.validate_debugger_safe_point(
        tab_id,
        realm.realm_generation,
        point.program.program_handle,
        point.program.program_generation,
        point.code_unit_ordinal,
        point.bytecode_offset,
    ) {
        return debugger_program_error(error);
    }
    match locations.arm_debugger_linked_nested_safe_point_breakpoint(
        tab_id,
        realm.realm_generation,
        JavaScriptPageDebuggerProgram {
            program_handle: target.entry.program_handle,
            program_generation: target.entry.program_generation,
        },
        JavaScriptPageDebuggerProgram {
            program_handle: point.program.program_handle,
            program_generation: point.program.program_generation,
        },
        JavaScriptPageDebuggerSafePoint {
            code_unit_ordinal: point.code_unit_ordinal,
            bytecode_offset: point.bytecode_offset,
        },
    ) {
        Ok(()) => DebuggerReply::LinkedNestedSafePointBreakpointArmed { target },
        Err(error) => debugger_program_error(error),
    }
}

pub(super) fn public_linked_stack(
    tab_id: TabId,
    realm: DebuggerPageRealm,
    internal: JavaScriptPageDebuggerLinkedStackSnapshot,
) -> Option<DebuggerLinkedStackSnapshot> {
    let frames = internal.frames.map(|entry| {
        let frame = entry.frame;
        if frame.tab_id != tab_id || frame.document_generation != realm.realm_generation {
            return None;
        }
        let program = DebuggerProgram {
            realm,
            program_handle: frame.program_handle,
            program_generation: frame.program_generation,
        };
        Some(DebuggerLinkedStackFrame {
            frame: DebuggerLinkedFrame {
                program,
                code_unit_ordinal: frame.code_unit_ordinal,
                core_instance: frame.core_instance,
                frame_handle: frame.frame_handle,
            },
            safe_point: DebuggerSafePoint {
                program,
                code_unit_ordinal: entry.safe_point.code_unit_ordinal,
                bytecode_offset: entry.safe_point.bytecode_offset,
            },
        })
    });
    let [Some(dependency), Some(caller)] = frames else {
        return None;
    };
    let stack = DebuggerLinkedStackSnapshot {
        frames: [dependency, caller],
    };
    stack.is_well_formed().then_some(stack)
}

pub(super) fn internal_linked_stack(
    tab_id: TabId,
    public: DebuggerLinkedStackSnapshot,
) -> JavaScriptPageDebuggerLinkedStackSnapshot {
    JavaScriptPageDebuggerLinkedStackSnapshot {
        frames: public
            .frames
            .map(|entry| JavaScriptPageDebuggerLinkedStackFrame {
                frame: JavaScriptPageDebuggerFrame {
                    tab_id,
                    document_generation: entry.frame.program.realm.realm_generation,
                    program_handle: entry.frame.program.program_handle,
                    program_generation: entry.frame.program.program_generation,
                    code_unit_ordinal: entry.frame.code_unit_ordinal,
                    core_instance: entry.frame.core_instance,
                    frame_handle: entry.frame.frame_handle,
                },
                safe_point: JavaScriptPageDebuggerSafePoint {
                    code_unit_ordinal: entry.safe_point.code_unit_ordinal,
                    bytecode_offset: entry.safe_point.bytecode_offset,
                },
            }),
    }
}

pub(super) fn child_linked_execution_state(
    tabs: &TabManager,
    locations: &mut dyn PageJavaScriptDebuggerLocations,
    entry: DebuggerProgram,
) -> DebuggerReply {
    if !entry.is_well_formed() {
        return invalid_linked_target();
    }
    let realm = entry.realm;
    let tab_id = match resolve_live_realm(tabs, realm) {
        Ok(tab_id) => tab_id,
        Err(reply) => return *reply,
    };
    if !locations.debugger_has_live_realm(tab_id, realm.realm_generation)
        || !locations.debugger_linked_frames_available()
    {
        return unavailable_linked_modules();
    }
    let state = match locations.debugger_linked_execution_state(
        tab_id,
        realm.realm_generation,
        JavaScriptPageDebuggerProgram {
            program_handle: entry.program_handle,
            program_generation: entry.program_generation,
        },
    ) {
        Ok(JavaScriptPageDebuggerLinkedExecutionState::Pending) => {
            DebuggerLinkedExecutionState::Pending
        }
        Ok(JavaScriptPageDebuggerLinkedExecutionState::Completed) => {
            DebuggerLinkedExecutionState::Completed
        }
        Ok(JavaScriptPageDebuggerLinkedExecutionState::Paused { stack }) => {
            let Some(stack) = public_linked_stack(tab_id, realm, stack) else {
                return invalid_linked_target();
            };
            DebuggerLinkedExecutionState::Paused { stack }
        }
        Ok(JavaScriptPageDebuggerLinkedExecutionState::Resuming { frame }) => {
            if frame.tab_id != tab_id || frame.document_generation != realm.realm_generation {
                return invalid_linked_target();
            }
            DebuggerLinkedExecutionState::Resuming {
                frame: DebuggerLinkedFrame {
                    program: DebuggerProgram {
                        realm,
                        program_handle: frame.program_handle,
                        program_generation: frame.program_generation,
                    },
                    code_unit_ordinal: frame.code_unit_ordinal,
                    core_instance: frame.core_instance,
                    frame_handle: frame.frame_handle,
                },
            }
        }
        Err(error) => return debugger_program_error(error),
    };
    if !state.is_well_formed(entry) {
        return invalid_linked_target();
    }
    DebuggerReply::LinkedExecutionState {
        entry,
        state: Box::new(state),
    }
}

pub(super) fn child_linked_stack(
    tabs: &TabManager,
    locations: &mut dyn PageJavaScriptDebuggerLocations,
    top_frame: DebuggerLinkedFrame,
) -> DebuggerReply {
    if !top_frame.is_well_formed() || top_frame.code_unit_ordinal == 0 {
        return invalid_linked_target();
    }
    let realm = top_frame.program.realm;
    let tab_id = match resolve_live_realm(tabs, realm) {
        Ok(tab_id) => tab_id,
        Err(reply) => return *reply,
    };
    if !locations.debugger_has_live_realm(tab_id, realm.realm_generation)
        || !locations.debugger_linked_frames_available()
    {
        return unavailable_linked_modules();
    }
    let internal = JavaScriptPageDebuggerFrame {
        tab_id,
        document_generation: realm.realm_generation,
        program_handle: top_frame.program.program_handle,
        program_generation: top_frame.program.program_generation,
        code_unit_ordinal: top_frame.code_unit_ordinal,
        core_instance: top_frame.core_instance,
        frame_handle: top_frame.frame_handle,
    };
    let stack = match locations.debugger_linked_stack_snapshot(internal, 1) {
        Ok(stack) => stack,
        Err(error) => return debugger_program_error(error),
    };
    let Some(stack) = public_linked_stack(tab_id, realm, stack) else {
        return invalid_linked_target();
    };
    if stack.frames[0].frame != top_frame {
        return DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidExecutionState,
            message: "linked debugger stack moved".to_string(),
        };
    }
    DebuggerReply::LinkedStack(Box::new(stack))
}

pub(super) fn resume_child_linked_nested_execution(
    tabs: &TabManager,
    locations: &mut dyn PageJavaScriptDebuggerLocations,
    top_frame: DebuggerLinkedFrame,
) -> DebuggerReply {
    if !top_frame.is_well_formed() || top_frame.code_unit_ordinal == 0 {
        return invalid_linked_target();
    }
    let realm = top_frame.program.realm;
    let tab_id = match resolve_live_realm(tabs, realm) {
        Ok(tab_id) => tab_id,
        Err(reply) => return *reply,
    };
    if !locations.debugger_has_live_realm(tab_id, realm.realm_generation)
        || !locations.debugger_linked_frames_available()
    {
        return unavailable_linked_modules();
    }
    let internal = JavaScriptPageDebuggerFrame {
        tab_id,
        document_generation: realm.realm_generation,
        program_handle: top_frame.program.program_handle,
        program_generation: top_frame.program.program_generation,
        code_unit_ordinal: top_frame.code_unit_ordinal,
        core_instance: top_frame.core_instance,
        frame_handle: top_frame.frame_handle,
    };
    match locations.resume_debugger_linked_nested_execution(internal) {
        Ok(()) => DebuggerReply::LinkedNestedResumeRequested { top_frame },
        Err(error) => debugger_program_error(error),
    }
}

pub(super) fn child_linked_stack_coordinates(
    tabs: &TabManager,
    locations: &mut dyn PageJavaScriptDebuggerLocations,
    metadata_session: Option<&DebuggerMetadataSessionAuthorization>,
    target: DebuggerLinkedStackCoordinatesTarget,
) -> DebuggerReply {
    if !target.is_well_formed() {
        return invalid_linked_target();
    }
    let Some(session) = metadata_session else {
        return unavailable_static_metadata_safe_point_span();
    };
    if !session.permits(DebuggerMetadataCapability::OpaqueInventory)
        || !session.permits(DebuggerMetadataCapability::OpaqueSourceInventory)
        || !target.sources.iter().all(|source| {
            session.observed_metadata(source.metadata) && session.observed_source(*source)
        })
    {
        return unavailable_static_metadata_safe_point_span();
    }
    let expected = target.expected_stack;
    let realm = expected.frames[0].frame.program.realm;
    let capabilities = describe_child_location_capabilities(tabs, locations, Some(session), realm);
    let DebuggerReply::Capabilities(capabilities) = capabilities else {
        return capabilities;
    };
    let Some(authorization) =
        capabilities.authorize_metadata(session, DebuggerMetadataCapability::OpaqueSafePointSpan)
    else {
        return unavailable_static_metadata_safe_point_span();
    };
    if !authorization.permits(realm, DebuggerMetadataCapability::OpaqueSafePointSpan) {
        return unavailable_static_metadata_safe_point_span();
    }
    let current = match child_linked_stack(tabs, locations, expected.frames[0].frame) {
        DebuggerReply::LinkedStack(stack) => *stack,
        other => return other,
    };
    if current != expected {
        return DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidExecutionState,
            message: "linked debugger stack moved from expected safe points".to_string(),
        };
    }
    let tab_id = match resolve_live_realm(tabs, realm) {
        Ok(tab_id) => tab_id,
        Err(reply) => return *reply,
    };
    let access = JavaScriptPageDebuggerLinkedSpanAccess {
        granted: true,
        metadata_receipted: [true; 2],
        source_receipted: [true; 2],
        targets: [0, 1].map(|index| {
            let frame = current.frames[index];
            let source = target.sources[index];
            JavaScriptPageDebuggerStaticMetadataSafePointSpanTarget {
                program_handle: frame.frame.program.program_handle,
                program_generation: frame.frame.program.program_generation,
                metadata_handle: source.metadata.metadata_handle,
                metadata_generation: source.metadata.metadata_generation,
                source_id: source.source_id,
                code_unit_ordinal: frame.safe_point.code_unit_ordinal,
                bytecode_offset: frame.safe_point.bytecode_offset,
            }
        }),
    };
    let spans = match locations
        .debugger_linked_stack_spans(internal_linked_stack(tab_id, current), access)
    {
        Ok(spans) => spans,
        Err(error) => return debugger_program_error(error),
    };
    let result = DebuggerLinkedStackCoordinates {
        stack: current,
        spans: [0, 1].map(|index| DebuggerStaticMetadataSafePointSpan {
            safe_point: current.frames[index].safe_point,
            source: target.sources[index],
            start_byte: spans[index].start_byte,
            end_byte: spans[index].end_byte,
            coordinates: spans[index].coordinates,
        }),
    };
    if !result.is_well_formed()
        || result
            .spans
            .iter()
            .enumerate()
            .any(|(index, span)| span.source.source_id != spans[index].source_id)
    {
        return invalid_linked_target();
    }
    DebuggerReply::LinkedStackCoordinates(Box::new(result))
}
