// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

pub(super) fn invalid_linked_target() -> DebuggerReply {
    DebuggerReply::Error {
        code: DebuggerErrorCode::InvalidTarget,
        message: "invalid linked debugger target".to_string(),
    }
}

pub(super) fn child_execution_state(
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
    if !locations.debugger_has_live_realm(tab_id, program.realm.realm_generation)
        || !locations.debugger_execution_control_available()
    {
        return unavailable_execution_control();
    }
    if locations.debugger_nested_frames_available() {
        let nested = match locations.debugger_nested_execution_state(
            tab_id,
            program.realm.realm_generation,
            program.program_handle,
            program.program_generation,
        ) {
            Ok(nested) => nested,
            Err(error) => return debugger_program_error(error),
        };
        if let Some(nested) = nested {
            let (frame, state) = match nested {
                JavaScriptPageDebuggerNestedExecutionState::Paused {
                    frame,
                    bytecode_offset,
                } => {
                    let public_frame = DebuggerFrame {
                        program,
                        code_unit_ordinal: frame.code_unit_ordinal,
                        core_instance: frame.core_instance,
                        frame_handle: frame.frame_handle,
                    };
                    let safe_point = DebuggerSafePoint {
                        program,
                        code_unit_ordinal: frame.code_unit_ordinal,
                        bytecode_offset,
                    };
                    if !public_frame.matches_safe_point(safe_point) {
                        return invalid_safe_point_target();
                    }
                    if let Err(error) = locations.validate_debugger_safe_point(
                        tab_id,
                        program.realm.realm_generation,
                        program.program_handle,
                        program.program_generation,
                        safe_point.code_unit_ordinal,
                        safe_point.bytecode_offset,
                    ) {
                        return debugger_program_error(error);
                    }
                    (
                        frame,
                        DebuggerExecutionState::NestedPaused {
                            frame: public_frame,
                            safe_point,
                        },
                    )
                }
                JavaScriptPageDebuggerNestedExecutionState::Stepping { frame } => {
                    let public_frame = DebuggerFrame {
                        program,
                        code_unit_ordinal: frame.code_unit_ordinal,
                        core_instance: frame.core_instance,
                        frame_handle: frame.frame_handle,
                    };
                    if !public_frame.is_well_formed() {
                        return invalid_safe_point_target();
                    }
                    (
                        frame,
                        DebuggerExecutionState::NestedStepping {
                            frame: public_frame,
                        },
                    )
                }
                JavaScriptPageDebuggerNestedExecutionState::Resuming { frame } => {
                    let public_frame = DebuggerFrame {
                        program,
                        code_unit_ordinal: frame.code_unit_ordinal,
                        core_instance: frame.core_instance,
                        frame_handle: frame.frame_handle,
                    };
                    if !public_frame.is_well_formed() {
                        return invalid_safe_point_target();
                    }
                    (
                        frame,
                        DebuggerExecutionState::NestedResuming {
                            frame: public_frame,
                        },
                    )
                }
            };
            if frame.tab_id != tab_id
                || frame.document_generation != program.realm.realm_generation
                || frame.program_handle != program.program_handle
                || frame.program_generation != program.program_generation
            {
                return DebuggerReply::Error {
                    code: DebuggerErrorCode::InvalidTarget,
                    message: "active debugger frame does not belong to this program".to_string(),
                };
            }
            return DebuggerReply::ExecutionState { program, state };
        }
    }
    match locations.debugger_execution_state(
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

pub(super) fn resume_child_execution(
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
    if !locations.debugger_has_live_realm(tab_id, program.realm.realm_generation)
        || !locations.debugger_execution_control_available()
    {
        return unavailable_execution_control();
    }
    match locations.resume_debugger_execution(
        tab_id,
        program.realm.realm_generation,
        program.program_handle,
        program.program_generation,
    ) {
        Ok(()) => DebuggerReply::ExecutionResumed { program },
        Err(error) => debugger_program_error(error),
    }
}

pub(super) fn step_child_root_instruction(
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
    if !locations.debugger_has_live_realm(tab_id, program.realm.realm_generation)
        || !locations.debugger_execution_control_available()
        || !locations.debugger_stepping_available()
    {
        return unavailable_stepping();
    }
    match locations.step_debugger_root_instruction(
        tab_id,
        program.realm.realm_generation,
        program.program_handle,
        program.program_generation,
    ) {
        Ok(()) => DebuggerReply::ExecutionStepRequested { program },
        Err(error) => debugger_program_error(error),
    }
}

pub(super) fn step_child_nested_instruction(
    tabs: &TabManager,
    locations: &mut dyn PageJavaScriptDebuggerLocations,
    frame: DebuggerFrame,
) -> DebuggerReply {
    continue_child_nested_execution(tabs, locations, frame, false)
}

pub(super) fn resume_child_nested_execution(
    tabs: &TabManager,
    locations: &mut dyn PageJavaScriptDebuggerLocations,
    frame: DebuggerFrame,
) -> DebuggerReply {
    continue_child_nested_execution(tabs, locations, frame, true)
}

pub(super) fn continue_child_nested_execution(
    tabs: &TabManager,
    locations: &mut dyn PageJavaScriptDebuggerLocations,
    frame: DebuggerFrame,
    resume: bool,
) -> DebuggerReply {
    if !frame.is_well_formed() {
        return DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            message: "invalid active debugger frame".to_string(),
        };
    }
    let realm = frame.program.realm;
    let tab_id = match resolve_live_realm(tabs, realm) {
        Ok(tab_id) => tab_id,
        Err(reply) => return *reply,
    };
    if !locations.debugger_has_live_realm(tab_id, realm.realm_generation)
        || !locations.debugger_nested_frames_available()
    {
        return unavailable_nested_frames();
    }
    let internal_frame = JavaScriptPageDebuggerFrame {
        tab_id,
        document_generation: realm.realm_generation,
        program_handle: frame.program.program_handle,
        program_generation: frame.program.program_generation,
        code_unit_ordinal: frame.code_unit_ordinal,
        core_instance: frame.core_instance,
        frame_handle: frame.frame_handle,
    };
    let result = if resume {
        locations.resume_debugger_nested_execution(internal_frame)
    } else {
        locations.step_debugger_nested_instruction(internal_frame)
    };
    match result {
        Ok(()) if resume => DebuggerReply::NestedResumeRequested { frame },
        Ok(()) => DebuggerReply::NestedStepRequested { frame },
        Err(error) => debugger_program_error(error),
    }
}

pub(super) fn resolve_child_stack_target(
    tabs: &TabManager,
    program: DebuggerProgram,
    frame: Option<DebuggerFrame>,
) -> Result<(TabId, Option<JavaScriptPageDebuggerFrame>), Box<DebuggerReply>> {
    if !program.is_well_formed()
        || frame.is_some_and(|frame| !frame.is_well_formed() || frame.program != program)
    {
        return Err(Box::new(DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            message: "invalid debugger stack target".to_string(),
        }));
    }
    let tab_id = resolve_live_realm(tabs, program.realm)?;
    let internal_frame = frame.map(|frame| JavaScriptPageDebuggerFrame {
        tab_id,
        document_generation: program.realm.realm_generation,
        program_handle: program.program_handle,
        program_generation: program.program_generation,
        code_unit_ordinal: frame.code_unit_ordinal,
        core_instance: frame.core_instance,
        frame_handle: frame.frame_handle,
    });
    Ok((tab_id, internal_frame))
}

pub(super) fn child_stack(
    tabs: &TabManager,
    locations: &mut dyn PageJavaScriptDebuggerLocations,
    program: DebuggerProgram,
    frame: Option<DebuggerFrame>,
    max_frames: u32,
) -> DebuggerReply {
    let (tab_id, internal_frame) = match resolve_child_stack_target(tabs, program, frame) {
        Ok(target) => target,
        Err(reply) => return *reply,
    };
    if !locations.debugger_has_live_realm(tab_id, program.realm.realm_generation)
        || !locations.debugger_stack_available()
        || (frame.is_some() && !locations.debugger_nested_frames_available())
    {
        return unavailable_stack();
    }
    if !(1..=MAX_STACK_FRAMES).contains(&max_frames) {
        return DebuggerReply::Error {
            code: DebuggerErrorCode::ResourceLimit,
            message: "debugger stack frame limit exceeds its fixed positive budget".to_string(),
        };
    }
    let snapshot = match locations.debugger_stack_snapshot(
        tab_id,
        program.realm.realm_generation,
        JavaScriptPageDebuggerProgram {
            program_handle: program.program_handle,
            program_generation: program.program_generation,
        },
        internal_frame,
        max_frames,
        1,
    ) {
        Ok(snapshot) => snapshot,
        Err(error) => return debugger_program_error(error),
    };
    DebuggerReply::Stack(DebuggerStackSnapshot {
        program,
        frame,
        safe_points: snapshot
            .frames
            .into_iter()
            .map(|frame| DebuggerSafePoint {
                program,
                code_unit_ordinal: frame.code_unit_ordinal,
                bytecode_offset: frame.bytecode_offset,
            })
            .collect(),
        stack_truncated: snapshot.stack_truncated,
    })
}

pub(super) fn child_stack_coordinates(
    tabs: &TabManager,
    locations: &mut dyn PageJavaScriptDebuggerLocations,
    metadata_session: Option<&DebuggerMetadataSessionAuthorization>,
    target: DebuggerStackCoordinatesTarget,
) -> DebuggerReply {
    if !target.is_well_formed() {
        return DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            message: "invalid debugger stack-coordinate target".to_string(),
        };
    }
    let Some(metadata_session) = metadata_session else {
        return unavailable_static_metadata_safe_point_span();
    };
    if !metadata_session.permits(DebuggerMetadataCapability::OpaqueInventory)
        || !metadata_session.permits(DebuggerMetadataCapability::OpaqueSourceInventory)
        || !metadata_session.observed_metadata(target.sources[0].metadata)
        || !target
            .sources
            .iter()
            .all(|source| metadata_session.observed_source(*source))
    {
        return unavailable_static_metadata_safe_point_span();
    }
    let expected = target.expected_stack;
    let realm = expected.program.realm;
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
    let current = match child_stack(
        tabs,
        locations,
        expected.program,
        expected.frame,
        expected.safe_points.len() as u32,
    ) {
        DebuggerReply::Stack(snapshot) => snapshot,
        other => return other,
    };
    if current != expected {
        return DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidExecutionState,
            message: "debugger stack moved from the expected safe points".to_string(),
        };
    }
    let mut spans = Vec::with_capacity(current.safe_points.len());
    for (safe_point, source) in current.safe_points.iter().zip(target.sources) {
        match describe_child_static_metadata_safe_point_span(
            tabs,
            locations,
            Some(metadata_session),
            DebuggerStaticMetadataSafePointSpanTarget {
                safe_point: *safe_point,
                source,
            },
        ) {
            DebuggerReply::StaticMetadataSafePointSpan(span) => spans.push(span),
            other => return other,
        }
    }
    let result = DebuggerStackCoordinates {
        stack: current,
        spans,
    };
    if !result.is_well_formed() {
        return DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            message: "invalid debugger stack-coordinate result".to_string(),
        };
    }
    DebuggerReply::StackCoordinates(result)
}

pub(super) fn child_scopes(
    tabs: &TabManager,
    locations: &mut dyn PageJavaScriptDebuggerLocations,
    program: DebuggerProgram,
    frame: Option<DebuggerFrame>,
    frame_index: u32,
    expected_safe_point: DebuggerSafePoint,
    max_scope_entries: u32,
) -> DebuggerReply {
    let (tab_id, internal_frame) = match resolve_child_stack_target(tabs, program, frame) {
        Ok(target) => target,
        Err(reply) => return *reply,
    };
    if !expected_safe_point.is_well_formed() || expected_safe_point.program != program {
        return invalid_safe_point_target();
    }
    if !locations.debugger_has_live_realm(tab_id, program.realm.realm_generation)
        || !locations.debugger_scopes_available()
        || (frame.is_some() && !locations.debugger_nested_frames_available())
    {
        return unavailable_scopes();
    }
    if frame_index >= MAX_STACK_FRAMES || !(1..=MAX_SCOPE_BINDINGS).contains(&max_scope_entries) {
        return DebuggerReply::Error {
            code: DebuggerErrorCode::ResourceLimit,
            message: "debugger scope selection or entry limit exceeds its fixed budget".to_string(),
        };
    }
    let snapshot = match locations.debugger_stack_snapshot(
        tab_id,
        program.realm.realm_generation,
        JavaScriptPageDebuggerProgram {
            program_handle: program.program_handle,
            program_generation: program.program_generation,
        },
        internal_frame,
        frame_index + 1,
        max_scope_entries,
    ) {
        Ok(snapshot) => snapshot,
        Err(error) => return debugger_program_error(error),
    };
    let Some(selected) = snapshot.frames.get(frame_index as usize) else {
        return DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            message: "debugger scope frame index is not active".to_string(),
        };
    };
    if selected.code_unit_ordinal != expected_safe_point.code_unit_ordinal
        || selected.bytecode_offset != expected_safe_point.bytecode_offset
    {
        return DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidExecutionState,
            message: "debugger scope frame moved from the expected safe point".to_string(),
        };
    }
    DebuggerReply::Scopes(DebuggerScopeSnapshot {
        program,
        frame,
        frame_index,
        safe_point: expected_safe_point,
        entries: selected
            .scope_entries
            .iter()
            .map(|entry| DebuggerScopeEntry {
                slot_ordinal: entry.slot_ordinal,
                scope_depth: entry.scope_depth,
            })
            .collect(),
        scope_truncated: selected.scope_truncated,
    })
}

/// Core-side linked entry-root scopes. Public dispatch mints a same-stream
/// receipt only for a complete result; this helper neither grants a static
/// relation nor returns dependency captures.
pub(super) fn staged_child_linked_scopes(
    tabs: &TabManager,
    locations: &mut dyn PageJavaScriptDebuggerLocations,
    expected_stack: DebuggerLinkedStackSnapshot,
    max_scope_entries: u32,
) -> Result<DebuggerLinkedScopeSnapshot, Box<DebuggerReply>> {
    if !expected_stack.is_well_formed() {
        return Err(Box::new(invalid_linked_target()));
    }
    if !(1..=MAX_SCOPE_BINDINGS).contains(&max_scope_entries) {
        return Err(Box::new(DebuggerReply::Error {
            code: DebuggerErrorCode::ResourceLimit,
            message: "linked debugger scope entry limit exceeds its fixed budget".to_string(),
        }));
    }
    let realm = expected_stack.frames[1].frame.program.realm;
    let tab_id = resolve_live_realm(tabs, realm)?;
    if !locations.debugger_has_live_realm(tab_id, realm.realm_generation)
        || !locations.debugger_linked_frames_available()
        || !locations.debugger_scopes_available()
    {
        return Err(Box::new(unavailable_scopes()));
    }
    let internal = internal_linked_stack(tab_id, expected_stack);
    let private = locations
        .debugger_linked_scope_snapshot(internal)
        .map_err(|error| Box::new(debugger_program_error(error)))?;
    let mut unique_slots = HashSet::with_capacity(private.scope_entries.len());
    if private.stack != internal
        || private.scope_entries.len() > MAX_SCOPE_BINDINGS as usize
        || !private
            .scope_entries
            .iter()
            .all(|entry| unique_slots.insert(entry.slot_ordinal))
    {
        return Err(Box::new(DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidExecutionState,
            message: "linked debugger scope changed or exceeded the private budget".to_string(),
        }));
    }
    let scope_truncated = private.scope_entries.len() > max_scope_entries as usize;
    let snapshot = DebuggerLinkedScopeSnapshot {
        stack: expected_stack,
        frame_index: 1,
        entries: private
            .scope_entries
            .into_iter()
            .take(max_scope_entries as usize)
            .map(|entry| DebuggerScopeEntry {
                slot_ordinal: entry.slot_ordinal,
                scope_depth: entry.scope_depth,
            })
            .collect(),
        scope_truncated,
        max_scope_entries,
    };
    if !snapshot.is_well_formed() {
        return Err(Box::new(DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidExecutionState,
            message: "invalid linked debugger scope snapshot".to_string(),
        }));
    }
    Ok(snapshot)
}

/// Rechecks a static-only paused-slot relation against the live child. Public
/// dispatch must first establish independent grant and same-stream receipts.
pub(super) fn private_core_static_scope_relation(
    tabs: &TabManager,
    locations: &mut dyn PageJavaScriptDebuggerLocations,
    realm: DebuggerPageRealm,
    target: JavaScriptPageDebuggerStaticScopeTarget,
) -> Result<JavaScriptPageDebuggerStaticScopeRelation, Box<DebuggerReply>> {
    let invalid = || {
        Box::new(DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidExecutionState,
            message: "static scope target is not the exact active paused slot".to_string(),
        })
    };
    let tab_id = resolve_live_realm(tabs, realm)?;
    if !locations.debugger_has_live_realm(tab_id, realm.realm_generation) {
        return Err(Box::new(unavailable_scopes()));
    }
    match target {
        JavaScriptPageDebuggerStaticScopeTarget::Ordinary { metadata, target } => {
            let frame_count = match (target.frame, target.frame_index) {
                (None, 0) => 1,
                (Some(_), 1) => 2,
                _ => return Err(invalid()),
            };
            if metadata.metadata_handle == 0
                || metadata.metadata_generation == 0
                || target.program.program_handle == 0
                || target.program.program_generation == 0
                || target.safe_point.code_unit_ordinal != 0
                || target.frame.is_some_and(|frame| {
                    frame.tab_id != tab_id
                        || frame.document_generation != realm.realm_generation
                        || frame.program_handle != target.program.program_handle
                        || frame.program_generation != target.program.program_generation
                        || frame.code_unit_ordinal == 0
                })
            {
                return Err(invalid());
            }
            let snapshot = locations
                .debugger_stack_snapshot(
                    tab_id,
                    realm.realm_generation,
                    target.program,
                    target.frame,
                    frame_count,
                    MAX_SCOPE_BINDINGS,
                )
                .map_err(|error| Box::new(debugger_program_error(error)))?;
            if snapshot.stack_truncated
                || snapshot.frames.len() != frame_count as usize
                || snapshot.frames.iter().any(|frame| frame.scope_truncated)
            {
                return Err(invalid());
            }
            let selected = &snapshot.frames[target.frame_index as usize];
            if selected.code_unit_ordinal != 0
                || selected.bytecode_offset != target.safe_point.bytecode_offset
                || selected
                    .scope_entries
                    .iter()
                    .filter(|entry| entry.slot_ordinal == target.scope_entry.slot_ordinal)
                    .count()
                    != 1
                || !selected.scope_entries.contains(&target.scope_entry)
            {
                return Err(invalid());
            }
        }
        JavaScriptPageDebuggerStaticScopeTarget::Linked {
            metadata,
            expected_stack,
            frame_index,
            ..
        } => {
            if metadata.metadata_handle == 0
                || metadata.metadata_generation == 0
                || frame_index != 1
                || public_linked_stack(tab_id, realm, expected_stack).is_none()
            {
                return Err(invalid());
            }
            let current = locations
                .debugger_linked_stack_snapshot(expected_stack.frames[0].frame, MAX_SCOPE_BINDINGS)
                .map_err(|error| Box::new(debugger_program_error(error)))?;
            if current != expected_stack {
                return Err(invalid());
            }
            // The executor also checks the complete private two-program scope
            // stack and entry-root slot before it forwards this target.
        }
    }
    let relation = locations
        .debugger_static_scope_relation(tab_id, realm.realm_generation, target)
        .map_err(|error| Box::new(debugger_program_error(error)))?;
    if relation.target != target {
        return Err(invalid());
    }
    Ok(relation)
}

/// Session-side static relation. The `granted` flag comes only from the
/// independently negotiated owner/client capability for this exact live realm.
pub(super) fn staged_child_static_scope_relation(
    tabs: &TabManager,
    locations: &mut dyn PageJavaScriptDebuggerLocations,
    session: Option<&DebuggerMetadataSessionAuthorization>,
    granted: bool,
    pause_incarnation: u64,
    target: DebuggerStaticScopeTarget,
) -> Result<DebuggerStaticScopeRelation, Box<DebuggerReply>> {
    let unavailable = || {
        Box::new(DebuggerReply::Error {
            code: DebuggerErrorCode::CapabilityUnavailable,
            message: "static scope relation requires an independent grant and same-stream receipts"
                .to_string(),
        })
    };
    if !granted {
        return Err(unavailable());
    }
    let Some(session) = session else {
        return Err(unavailable());
    };
    if !target.is_well_formed() || !session.observed_static_scope(target, pause_incarnation) {
        return Err(Box::new(DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            message: "static scope target was not receipted on this paused stream".to_string(),
        }));
    }
    let metadata = target.metadata();
    if !session.permits(DebuggerMetadataCapability::OpaqueInventory)
        || !session.permits(DebuggerMetadataCapability::OpaqueTypeInventory)
        || !session.permits(DebuggerMetadataCapability::OpaqueSymbolInventory)
        || !session.observed_metadata(metadata)
    {
        return Err(unavailable());
    }
    let realm = metadata.program.realm;
    let tab_id = resolve_live_realm(tabs, realm)?;
    let core_metadata = JavaScriptPageDebuggerStaticMetadata {
        metadata_handle: metadata.metadata_handle,
        metadata_generation: metadata.metadata_generation,
    };
    let core_target = match target {
        DebuggerStaticScopeTarget::Ordinary { target, .. } => {
            let (_, frame) = resolve_child_stack_target(tabs, target.program, target.frame)?;
            JavaScriptPageDebuggerStaticScopeTarget::Ordinary {
                metadata: core_metadata,
                target: JavaScriptPageDebuggerValueTarget {
                    program: JavaScriptPageDebuggerProgram {
                        program_handle: target.program.program_handle,
                        program_generation: target.program.program_generation,
                    },
                    frame,
                    frame_index: target.frame_index,
                    safe_point: JavaScriptPageDebuggerSafePoint {
                        code_unit_ordinal: target.safe_point.code_unit_ordinal,
                        bytecode_offset: target.safe_point.bytecode_offset,
                    },
                    scope_entry: JavaScriptPageDebuggerScopeEntry {
                        slot_ordinal: target.scope_entry.slot_ordinal,
                        scope_depth: target.scope_entry.scope_depth,
                    },
                },
            }
        }
        DebuggerStaticScopeTarget::Linked { target, .. } => {
            JavaScriptPageDebuggerStaticScopeTarget::Linked {
                metadata: core_metadata,
                expected_stack: internal_linked_stack(tab_id, target.stack),
                frame_index: target.frame_index,
                scope_entry: JavaScriptPageDebuggerScopeEntry {
                    slot_ordinal: target.scope_entry.slot_ordinal,
                    scope_depth: target.scope_entry.scope_depth,
                },
            }
        }
    };
    let relation = private_core_static_scope_relation(tabs, locations, realm, core_target)?;
    if relation.target != core_target {
        return Err(Box::new(DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidExecutionState,
            message: "private static scope relation changed its target".to_string(),
        }));
    }
    let public = DebuggerStaticScopeRelation {
        target,
        symbol: DebuggerStaticMetadataSymbolId {
            metadata,
            symbol_id: relation.symbol_type.symbol_id,
        },
        static_type: DebuggerStaticMetadataTypeId {
            metadata,
            type_id: relation.symbol_type.type_id,
        },
    };
    if !public.is_well_formed()
        || !session.observed_symbol(public.symbol)
        || !session.observed_type(public.static_type)
    {
        return Err(unavailable());
    }
    Ok(public)
}

pub(super) fn child_static_scope_relation(
    tabs: &TabManager,
    locations: &mut dyn PageJavaScriptDebuggerLocations,
    session: Option<&DebuggerMetadataSessionAuthorization>,
    pause_incarnation: u64,
    target: DebuggerStaticScopeTarget,
) -> DebuggerReply {
    if !target.is_well_formed() {
        return DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            message: "invalid debugger static scope target".to_string(),
        };
    }
    let realm = target.metadata().program.realm;
    let granted = session.is_some_and(|session| {
        let DebuggerReply::Capabilities(capabilities) =
            describe_child_location_capabilities(tabs, locations, Some(session), realm)
        else {
            return false;
        };
        capabilities
            .authorize_metadata(
                session,
                DebuggerMetadataCapability::OpaqueStaticScopeRelation,
            )
            .is_some_and(|authorization| {
                authorization.permits(realm, DebuggerMetadataCapability::OpaqueStaticScopeRelation)
            })
    });
    match staged_child_static_scope_relation(
        tabs,
        locations,
        session,
        granted,
        pause_incarnation,
        target,
    ) {
        Ok(relation) => DebuggerReply::StaticScopeRelation(Box::new(relation)),
        Err(reply) => *reply,
    }
}

/// Private core-side half of the public value route. The caller must
/// supply the combined owner/client grant and this core session's current
/// pause incarnation; a client never supplies either authority directly.
pub(super) fn child_value_snapshot(
    tabs: &TabManager,
    locations: &mut dyn PageJavaScriptDebuggerLocations,
    session: Option<&DebuggerMetadataSessionAuthorization>,
    granted: bool,
    pause_incarnation: u64,
    target: DebuggerValueTarget,
) -> Result<DebuggerValueSnapshot, Box<DebuggerReply>> {
    if !granted {
        return Err(Box::new(unavailable_values()));
    }
    let Some(session) = session else {
        return Err(Box::new(unavailable_values()));
    };
    if !target.is_well_formed() || !session.observed_scope(target, pause_incarnation) {
        return Err(Box::new(DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            message: "debugger value target was not returned by Scopes on this stream".to_string(),
        }));
    }
    if !locations.debugger_values_available() {
        return Err(Box::new(unavailable_values()));
    }
    let scopes = match child_scopes(
        tabs,
        locations,
        target.program,
        target.frame,
        target.frame_index,
        target.safe_point,
        MAX_SCOPE_BINDINGS,
    ) {
        DebuggerReply::Scopes(scopes) => scopes,
        other => return Err(Box::new(other)),
    };
    if !scopes.entries.contains(&target.scope_entry) {
        return Err(Box::new(DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidExecutionState,
            message: "debugger value slot is no longer active at this pause".to_string(),
        }));
    }
    let (tab_id, frame) = resolve_child_stack_target(tabs, target.program, target.frame)?;
    let core_target = JavaScriptPageDebuggerValueTarget {
        program: JavaScriptPageDebuggerProgram {
            program_handle: target.program.program_handle,
            program_generation: target.program.program_generation,
        },
        frame,
        frame_index: target.frame_index,
        safe_point: JavaScriptPageDebuggerSafePoint {
            code_unit_ordinal: target.safe_point.code_unit_ordinal,
            bytecode_offset: target.safe_point.bytecode_offset,
        },
        scope_entry: JavaScriptPageDebuggerScopeEntry {
            slot_ordinal: target.scope_entry.slot_ordinal,
            scope_depth: target.scope_entry.scope_depth,
        },
    };
    let preview = locations
        .debugger_value_snapshot(tab_id, target.program.realm.realm_generation, core_target)
        .map_err(|error| Box::new(debugger_program_error(error)))?;
    let mut budget = ValueRemintBudget::default();
    let preview = remint_core_debugger_value(preview, 0, &mut budget).ok_or_else(|| {
        Box::new(DebuggerReply::Error {
            code: DebuggerErrorCode::ResourceLimit,
            message: "debugger value exceeds the complete public preview budget".to_string(),
        })
    })?;
    let snapshot = DebuggerValueSnapshot { target, preview };
    if !snapshot.is_well_formed() {
        return Err(Box::new(DebuggerReply::Error {
            code: DebuggerErrorCode::ResourceLimit,
            message: "invalid complete debugger value preview".to_string(),
        }));
    }
    Ok(snapshot)
}

#[derive(Default)]
pub(super) struct ValueRemintBudget {
    nodes: usize,
    payload_bytes: usize,
}

impl ValueRemintBudget {
    fn node(&mut self, depth: usize) -> Option<()> {
        if depth > DEBUGGER_MAX_VALUE_DEPTH {
            return None;
        }
        self.nodes = self.nodes.checked_add(1)?;
        (self.nodes <= DEBUGGER_MAX_VALUE_NODES).then_some(())
    }

    fn bytes(&mut self, bytes: usize) -> Option<()> {
        self.payload_bytes = self.payload_bytes.checked_add(bytes)?;
        (self.payload_bytes <= DEBUGGER_MAX_VALUE_PAYLOAD_BYTES).then_some(())
    }
}

pub(super) fn remint_core_debugger_value(
    value: JavaScriptPageDebuggerValuePreview,
    depth: usize,
    budget: &mut ValueRemintBudget,
) -> Option<DebuggerValuePreview> {
    budget.node(depth)?;
    Some(match value {
        JavaScriptPageDebuggerValuePreview::Undefined => DebuggerValuePreview::Undefined,
        JavaScriptPageDebuggerValuePreview::Null => DebuggerValuePreview::Null,
        JavaScriptPageDebuggerValuePreview::Bool(value) => DebuggerValuePreview::Bool(value),
        JavaScriptPageDebuggerValuePreview::NumberBits(bits) => {
            DebuggerValuePreview::NumberBits(bits)
        }
        JavaScriptPageDebuggerValuePreview::BigIntBytes(bytes) => {
            budget.bytes(bytes.len())?;
            DebuggerValuePreview::BigIntBytes(bytes)
        }
        JavaScriptPageDebuggerValuePreview::StringUnits(units) => {
            budget.bytes(units.len().checked_mul(2)?)?;
            DebuggerValuePreview::StringUnits(units)
        }
        JavaScriptPageDebuggerValuePreview::Array(elements) => {
            if elements.len() > DEBUGGER_MAX_VALUE_CONTAINER_LENGTH {
                return None;
            }
            let mut result = Vec::with_capacity(elements.len());
            for element in elements {
                result.push(match element {
                    Some(value) => Some(remint_core_debugger_value(value, depth + 1, budget)?),
                    None => {
                        budget.node(depth + 1)?;
                        None
                    }
                });
            }
            DebuggerValuePreview::Array(result)
        }
        JavaScriptPageDebuggerValuePreview::Record(entries) => {
            if entries.len() > DEBUGGER_MAX_VALUE_CONTAINER_LENGTH {
                return None;
            }
            let mut keys = HashSet::new();
            let mut result = Vec::with_capacity(entries.len());
            for (key, value) in entries {
                budget.bytes(key.len().checked_mul(2)?)?;
                if !keys.insert(key.clone()) {
                    return None;
                }
                result.push((key, remint_core_debugger_value(value, depth + 1, budget)?));
            }
            DebuggerValuePreview::Record(result)
        }
    })
}

pub(super) fn step_child_static_metadata_source_span(
    tabs: &TabManager,
    locations: &mut dyn PageJavaScriptDebuggerLocations,
    metadata_session: Option<&DebuggerMetadataSessionAuthorization>,
    target: DebuggerStaticMetadataSafePointSpanTarget,
) -> DebuggerReply {
    if !target.is_well_formed() {
        return DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            message: "invalid debugger BlueTS source-span step target".to_string(),
        };
    }
    let Some(metadata_session) = metadata_session else {
        return unavailable_static_metadata_source_span_step();
    };
    if !metadata_session.observed_metadata(target.source.metadata)
        || !metadata_session.observed_source(target.source)
    {
        return unavailable_static_metadata_source_span_step();
    }
    let realm = target.safe_point.program.realm;
    let capabilities =
        describe_child_location_capabilities(tabs, locations, Some(metadata_session), realm);
    let DebuggerReply::Capabilities(capabilities) = capabilities else {
        return capabilities;
    };
    let Some(authorization) = capabilities.authorize_metadata(
        metadata_session,
        DebuggerMetadataCapability::OpaqueSourceSpanStep,
    ) else {
        return unavailable_static_metadata_source_span_step();
    };
    if !authorization.permits(realm, DebuggerMetadataCapability::OpaqueSourceSpanStep) {
        return unavailable_static_metadata_source_span_step();
    }
    // Revalidate the compiler's exact source binding in the same core turn,
    // under the independently authorized span-read capability, before any
    // child execution transition is requested.
    if !matches!(
        describe_child_static_metadata_safe_point_span(
            tabs,
            locations,
            Some(metadata_session),
            target,
        ),
        DebuggerReply::StaticMetadataSafePointSpan(_)
    ) {
        return unavailable_static_metadata_source_span_step();
    }
    let tab_id = match resolve_live_realm(tabs, realm) {
        Ok(tab_id) => tab_id,
        Err(reply) => return *reply,
    };
    if !locations.debugger_has_live_realm(tab_id, realm.realm_generation)
        || !locations.debugger_execution_control_available()
        || !locations.debugger_source_span_stepping_available()
    {
        return unavailable_static_metadata_source_span_step();
    }
    let program = target.safe_point.program;
    let state = locations.debugger_execution_state(
        tab_id,
        realm.realm_generation,
        program.program_handle,
        program.program_generation,
    );
    if !matches!(
        state,
        Ok(JavaScriptPageDebuggerExecutionState::Paused {
            code_unit_ordinal,
            bytecode_offset,
        } | JavaScriptPageDebuggerExecutionState::SourceStepLimitReached {
            code_unit_ordinal,
            bytecode_offset,
        }) if code_unit_ordinal == target.safe_point.code_unit_ordinal
            && bytecode_offset == target.safe_point.bytecode_offset
    ) {
        return DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidExecutionState,
            message: "BlueTS source-span step requires the exact paused root safe point"
                .to_string(),
        };
    }
    match locations.step_debugger_bluets_source_span(
        tab_id,
        realm.realm_generation,
        JavaScriptPageDebuggerStaticMetadataSafePointSpanTarget {
            program_handle: program.program_handle,
            program_generation: program.program_generation,
            metadata_handle: target.source.metadata.metadata_handle,
            metadata_generation: target.source.metadata.metadata_generation,
            source_id: target.source.source_id,
            code_unit_ordinal: target.safe_point.code_unit_ordinal,
            bytecode_offset: target.safe_point.bytecode_offset,
        },
    ) {
        Ok(()) => DebuggerReply::ExecutionSourceSpanStepRequested {
            safe_point: target.safe_point,
        },
        Err(error) => debugger_program_error(error),
    }
}
