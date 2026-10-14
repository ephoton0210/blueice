// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Independently granted, same-stream-receipted generated debugger routes.

use super::*;

fn authorize_generated_program(
    tabs: &TabManager,
    locations: &mut dyn PageJavaScriptDebuggerLocations,
    metadata_session: Option<&DebuggerMetadataSessionAuthorization>,
    program: DebuggerProgram,
    capability: DebuggerMetadataCapability,
) -> Result<TabId, Box<DebuggerReply>> {
    if !program.is_well_formed() {
        return Err(Box::new(DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            message: "invalid generated debugger program target".to_string(),
        }));
    }
    let unavailable = || match capability {
        DebuggerMetadataCapability::OpaqueGeneratedPausedLocation => {
            unavailable_generated_paused_location()
        }
        _ => unavailable_generated_breakpoint(),
    };
    let Some(session) = metadata_session else {
        return Err(Box::new(unavailable()));
    };
    if !session.permits(capability) || !session.observed_program(program) {
        return Err(Box::new(unavailable()));
    }
    let capabilities =
        describe_child_location_capabilities(tabs, locations, Some(session), program.realm);
    let DebuggerReply::Capabilities(capabilities) = capabilities else {
        return Err(Box::new(capabilities));
    };
    let Some(authorization) = capabilities.authorize_metadata(session, capability) else {
        return Err(Box::new(unavailable()));
    };
    if !authorization.permits(program.realm, capability) {
        return Err(Box::new(unavailable()));
    }
    resolve_live_realm(tabs, program.realm)
}

pub(super) fn resolve_child_generated_breakpoint(
    tabs: &TabManager,
    locations: &mut dyn PageJavaScriptDebuggerLocations,
    metadata_session: Option<&DebuggerMetadataSessionAuthorization>,
    target: DebuggerGeneratedBreakpointTarget,
) -> DebuggerReply {
    if !target.is_well_formed() {
        return DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            message: "invalid generated breakpoint target".to_string(),
        };
    }
    let program = target.program;
    let tab_id = match authorize_generated_program(
        tabs,
        locations,
        metadata_session,
        program,
        DebuggerMetadataCapability::OpaqueGeneratedBreakpoint,
    ) {
        Ok(tab_id) => tab_id,
        Err(reply) => return *reply,
    };
    let safe_point = match locations.debugger_emitted_safe_point(
        tab_id,
        program.realm.realm_generation,
        program.program_handle,
        program.program_generation,
        target.generated_line,
        target.generated_column_utf16,
    ) {
        Ok(point) => point.map(|point| DebuggerSafePoint {
            program,
            code_unit_ordinal: point.code_unit_ordinal,
            bytecode_offset: point.bytecode_offset,
        }),
        Err(error) => return debugger_program_error(error),
    };
    if let Some(safe_point) = safe_point {
        match validate_child_safe_point(tabs, locations, safe_point) {
            DebuggerReply::SafePointValidated {
                safe_point: validated,
            } if validated == safe_point => {}
            reply => return reply,
        }
    }
    let result = DebuggerGeneratedBreakpoint { target, safe_point };
    if !result.is_well_formed() {
        return DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            message: "invalid generated breakpoint binding".to_string(),
        };
    }
    DebuggerReply::GeneratedBreakpoint(result)
}

pub(super) fn arm_child_generated_breakpoint(
    tabs: &TabManager,
    locations: &mut dyn PageJavaScriptDebuggerLocations,
    metadata_session: Option<&DebuggerMetadataSessionAuthorization>,
    target: DebuggerGeneratedBreakpointTarget,
) -> DebuggerReply {
    let binding =
        match resolve_child_generated_breakpoint(tabs, locations, metadata_session, target) {
            DebuggerReply::GeneratedBreakpoint(binding) => binding,
            DebuggerReply::Unsupported { .. } => return unavailable_generated_breakpoint_arm(),
            reply => return reply,
        };
    let Some(safe_point) = binding.safe_point else {
        return DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidSafePoint,
            message: "generated position has no exact executable safe point".to_string(),
        };
    };
    if safe_point.code_unit_ordinal == 0 {
        if !locations.debugger_execution_control_available() {
            return unavailable_generated_breakpoint_arm();
        }
        arm_child_root_safe_point_breakpoint(tabs, locations, safe_point)
    } else {
        arm_child_nested_safe_point_breakpoint(tabs, locations, safe_point)
    }
}

pub(super) fn child_generated_paused_location(
    tabs: &TabManager,
    locations: &mut dyn PageJavaScriptDebuggerLocations,
    metadata_session: Option<&DebuggerMetadataSessionAuthorization>,
    program: DebuggerProgram,
) -> DebuggerReply {
    let tab_id = match authorize_generated_program(
        tabs,
        locations,
        metadata_session,
        program,
        DebuggerMetadataCapability::OpaqueGeneratedPausedLocation,
    ) {
        Ok(tab_id) => tab_id,
        Err(reply) => return *reply,
    };
    let (expected, frame) = match child_execution_state(tabs, locations, program) {
        DebuggerReply::ExecutionState {
            state: DebuggerExecutionState::Paused { safe_point },
            ..
        } => (safe_point, None),
        DebuggerReply::ExecutionState {
            state: DebuggerExecutionState::NestedPaused { frame, safe_point },
            ..
        } => (safe_point, Some(frame)),
        DebuggerReply::Error { code, message } => return DebuggerReply::Error { code, message },
        _ => {
            return DebuggerReply::Error {
                code: DebuggerErrorCode::InvalidExecutionState,
                message: "generated location requires an exact paused frame".to_string(),
            }
        }
    };
    let max_frames = if frame.is_some() { 3 } else { 1 };
    let stack = match child_stack(tabs, locations, program, frame, max_frames) {
        DebuggerReply::Stack(stack) => stack,
        reply => return reply,
    };
    if stack.stack_truncated
        || stack.frame != frame
        || stack.safe_points.first().copied() != Some(expected)
        || frame.is_none() && stack.safe_points.len() != 1
        || frame.is_some()
            && (!(2..=3).contains(&stack.safe_points.len())
                || stack
                    .safe_points
                    .last()
                    .is_none_or(|point| point.program != program || point.code_unit_ordinal != 0)
                || stack.safe_points.len() == 3
                    && (stack.safe_points[1].program != program
                        || stack.safe_points[1].code_unit_ordinal == 0))
    {
        return DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidExecutionState,
            message: "paused generated stack moved from the expected safe point".to_string(),
        };
    }
    let (point, location) = match locations.debugger_emitted_paused_root_location(
        tab_id,
        program.realm.realm_generation,
        program.program_handle,
        program.program_generation,
    ) {
        Ok(result) => result,
        Err(error) => return debugger_program_error(error),
    };
    let safe_point = DebuggerSafePoint {
        program,
        code_unit_ordinal: point.code_unit_ordinal,
        bytecode_offset: point.bytecode_offset,
    };
    if safe_point != expected {
        return DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidExecutionState,
            message: "child paused generated location moved from the observed stack".to_string(),
        };
    }
    let location = match verify_generated_projection(locations, tab_id, program, expected, location)
    {
        Ok(location) => location,
        Err(reply) => return *reply,
    };
    let result = DebuggerGeneratedPausedLocation {
        program,
        safe_point,
        location,
    };
    if !result.is_well_formed() {
        return DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            message: "invalid generated paused location".to_string(),
        };
    }
    DebuggerReply::GeneratedPausedLocation(result)
}

fn verify_generated_projection(
    locations: &mut dyn PageJavaScriptDebuggerLocations,
    tab_id: TabId,
    program: DebuggerProgram,
    expected: DebuggerSafePoint,
    location: Option<JavaScriptPageDebuggerEmittedPausedRootLocation>,
) -> Result<Option<DebuggerGeneratedPausedSourceLocation>, Box<DebuggerReply>> {
    if let Some(projected) = location {
        let original = match locations.debugger_emitted_original_position(
            tab_id,
            program.realm.realm_generation,
            program.program_handle,
            program.program_generation,
            projected.generated_line,
            projected.generated_column_utf16,
        ) {
            Ok(original) => original,
            Err(error) => return Err(Box::new(debugger_program_error(error))),
        };
        let binding = match locations.debugger_emitted_safe_point(
            tab_id,
            program.realm.realm_generation,
            program.program_handle,
            program.program_generation,
            projected.generated_line,
            projected.generated_column_utf16,
        ) {
            Ok(binding) => binding,
            Err(error) => return Err(Box::new(debugger_program_error(error))),
        };
        let binding = binding.map(|point| DebuggerSafePoint {
            program,
            code_unit_ordinal: point.code_unit_ordinal,
            bytecode_offset: point.bytecode_offset,
        });
        if original != projected.original || binding != projected.original.map(|_| expected) {
            return Err(Box::new(DebuggerReply::Error {
                code: DebuggerErrorCode::InvalidExecutionState,
                message: "child generated projection disagrees with its bound map".to_string(),
            }));
        }
    }
    Ok(
        location.map(|location| DebuggerGeneratedPausedSourceLocation {
            generated_line: location.generated_line,
            generated_column_utf16: location.generated_column_utf16,
            original: location
                .original
                .map(|original| DebuggerGeneratedOriginalPosition {
                    source_index: original.source_index,
                    line: original.line,
                    column_utf16: original.column_utf16,
                }),
        }),
    )
}

pub(super) fn child_generated_linked_stack_locations(
    tabs: &TabManager,
    locations: &mut dyn PageJavaScriptDebuggerLocations,
    metadata_session: Option<&DebuggerMetadataSessionAuthorization>,
    expected_stack: DebuggerLinkedStackSnapshot,
) -> DebuggerReply {
    if !expected_stack.is_well_formed() {
        return DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            message: "invalid generated linked stack target".to_string(),
        };
    }
    let entry = expected_stack.frames[1].frame.program;
    let mut tab_id = None;
    for frame in expected_stack.frames {
        match authorize_generated_program(
            tabs,
            locations,
            metadata_session,
            frame.frame.program,
            DebuggerMetadataCapability::OpaqueGeneratedPausedLocation,
        ) {
            Ok(authorized) if tab_id.is_none_or(|prior| prior == authorized) => {
                tab_id = Some(authorized);
            }
            Ok(_) => return invalid_linked_target(),
            Err(reply) => return *reply,
        }
    }
    let tab_id = tab_id.expect("well-formed linked stack has two frames");
    let paused_stack = |tabs: &TabManager, locations: &mut dyn PageJavaScriptDebuggerLocations| {
        match child_linked_execution_state(tabs, locations, entry) {
            DebuggerReply::LinkedExecutionState {
                entry: actual,
                state,
            } if actual == entry => match *state {
                DebuggerLinkedExecutionState::Paused { stack } if stack == expected_stack => Ok(()),
                _ => Err(Box::new(DebuggerReply::Error {
                    code: DebuggerErrorCode::InvalidExecutionState,
                    message: "generated linked stack is no longer the exact pause".to_string(),
                })),
            },
            reply => Err(Box::new(reply)),
        }
    };
    if let Err(reply) = paused_stack(tabs, locations) {
        return *reply;
    }
    let mut projected = [None; 2];
    for (index, frame) in expected_stack.frames.iter().enumerate() {
        let program = frame.frame.program;
        let (point, location) = match locations.debugger_emitted_paused_root_location(
            tab_id,
            program.realm.realm_generation,
            program.program_handle,
            program.program_generation,
        ) {
            Ok(result) => result,
            Err(error) => return debugger_program_error(error),
        };
        let safe_point = DebuggerSafePoint {
            program,
            code_unit_ordinal: point.code_unit_ordinal,
            bytecode_offset: point.bytecode_offset,
        };
        if safe_point != frame.safe_point {
            return DebuggerReply::Error {
                code: DebuggerErrorCode::InvalidExecutionState,
                message: "child generated linked frame moved from the expected stack".to_string(),
            };
        }
        projected[index] =
            match verify_generated_projection(locations, tab_id, program, safe_point, location) {
                Ok(location) => location,
                Err(reply) => return *reply,
            };
    }
    if let Err(reply) = paused_stack(tabs, locations) {
        return *reply;
    }
    let result = DebuggerGeneratedLinkedStackLocations {
        stack: expected_stack,
        locations: projected,
    };
    if !result.is_well_formed() {
        return invalid_linked_target();
    }
    DebuggerReply::GeneratedLinkedStackLocations(result)
}
