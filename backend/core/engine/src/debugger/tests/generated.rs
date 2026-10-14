// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;
use crate::script::javascript::{
    JavaScriptPageDebuggerEmittedOriginalPosition, JavaScriptPageDebuggerEmittedPausedRootLocation,
    JavaScriptPageDebuggerStackFrame, JavaScriptPageDebuggerStackSnapshot,
};

struct GeneratedLocations {
    paused: bool,
    armed: bool,
    contradictory_map: bool,
    contradictory_binding: bool,
    malformed_map_failure: bool,
}

impl PageJavaScriptDebuggerLocations for GeneratedLocations {
    fn debugger_has_live_realm(&mut self, _tab: TabId, generation: u64) -> bool {
        generation == 1
    }

    fn max_debugger_safe_points_per_program(&self) -> usize {
        1
    }

    fn debugger_programs(
        &mut self,
        _tab: TabId,
        _generation: u64,
    ) -> Result<Vec<JavaScriptPageDebuggerProgram>, JavaScriptPageDebuggerError> {
        Ok(vec![JavaScriptPageDebuggerProgram {
            program_handle: 7,
            program_generation: 3,
        }])
    }

    fn debugger_safe_points(
        &mut self,
        _tab: TabId,
        _generation: u64,
        _handle: u64,
        _program_generation: u64,
    ) -> Result<Vec<JavaScriptPageDebuggerSafePoint>, JavaScriptPageDebuggerError> {
        Ok(vec![JavaScriptPageDebuggerSafePoint {
            code_unit_ordinal: 0,
            bytecode_offset: 4,
        }])
    }

    fn validate_debugger_safe_point(
        &mut self,
        _tab: TabId,
        _generation: u64,
        handle: u64,
        program_generation: u64,
        ordinal: u32,
        offset: u32,
    ) -> Result<(), JavaScriptPageDebuggerError> {
        if (handle, program_generation, ordinal, offset) == (7, 3, 0, 4) {
            Ok(())
        } else {
            Err(JavaScriptPageDebuggerError::InvalidSafePoint)
        }
    }

    fn debugger_emitted_map_available(&self) -> bool {
        true
    }

    fn debugger_emitted_paused_root_location_available(&self) -> bool {
        true
    }

    fn debugger_emitted_safe_point(
        &mut self,
        _tab: TabId,
        _generation: u64,
        handle: u64,
        program_generation: u64,
        line: u32,
        column: u32,
    ) -> Result<Option<JavaScriptPageDebuggerSafePoint>, JavaScriptPageDebuggerError> {
        if self.malformed_map_failure {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        if (handle, program_generation) != (7, 3) {
            return Err(JavaScriptPageDebuggerError::StaleProgram);
        }
        Ok(
            ((line, column) == (1, 2) && !self.contradictory_binding).then_some(
                JavaScriptPageDebuggerSafePoint {
                    code_unit_ordinal: 0,
                    bytecode_offset: 4,
                },
            ),
        )
    }

    fn debugger_emitted_original_position(
        &mut self,
        _tab: TabId,
        _generation: u64,
        handle: u64,
        program_generation: u64,
        line: u32,
        column: u32,
    ) -> Result<Option<JavaScriptPageDebuggerEmittedOriginalPosition>, JavaScriptPageDebuggerError>
    {
        if self.malformed_map_failure {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        if (handle, program_generation, line, column) != (7, 3, 1, 2) {
            return Err(JavaScriptPageDebuggerError::StaleProgram);
        }
        Ok(Some(JavaScriptPageDebuggerEmittedOriginalPosition {
            source_index: 0,
            line: 3,
            column_utf16: if self.contradictory_map { 5 } else { 4 },
        }))
    }

    fn debugger_execution_control_available(&self) -> bool {
        true
    }

    fn debugger_stack_available(&self) -> bool {
        true
    }

    fn arm_debugger_root_safe_point_breakpoint(
        &mut self,
        _tab: TabId,
        _generation: u64,
        _handle: u64,
        _program_generation: u64,
        _ordinal: u32,
        _offset: u32,
    ) -> Result<(), JavaScriptPageDebuggerError> {
        self.armed = true;
        Ok(())
    }

    fn debugger_execution_state(
        &mut self,
        _tab: TabId,
        _generation: u64,
        _handle: u64,
        _program_generation: u64,
    ) -> Result<JavaScriptPageDebuggerExecutionState, JavaScriptPageDebuggerError> {
        Ok(if self.paused {
            JavaScriptPageDebuggerExecutionState::Paused {
                code_unit_ordinal: 0,
                bytecode_offset: 4,
            }
        } else {
            JavaScriptPageDebuggerExecutionState::Pending
        })
    }

    fn debugger_stack_snapshot(
        &mut self,
        _tab: TabId,
        _generation: u64,
        _program: JavaScriptPageDebuggerProgram,
        _frame: Option<JavaScriptPageDebuggerFrame>,
        _max_frames: u32,
        _max_scope_entries: u32,
    ) -> Result<JavaScriptPageDebuggerStackSnapshot, JavaScriptPageDebuggerError> {
        Ok(JavaScriptPageDebuggerStackSnapshot {
            frames: vec![JavaScriptPageDebuggerStackFrame {
                code_unit_ordinal: 0,
                bytecode_offset: 4,
                scope_entries: Vec::new(),
                scope_truncated: false,
            }],
            stack_truncated: false,
        })
    }

    fn debugger_emitted_paused_root_location(
        &mut self,
        _tab: TabId,
        _generation: u64,
        _handle: u64,
        _program_generation: u64,
    ) -> Result<
        (
            JavaScriptPageDebuggerSafePoint,
            Option<JavaScriptPageDebuggerEmittedPausedRootLocation>,
        ),
        JavaScriptPageDebuggerError,
    > {
        if self.malformed_map_failure {
            return Err(JavaScriptPageDebuggerError::NoLiveRealm);
        }
        Ok((
            JavaScriptPageDebuggerSafePoint {
                code_unit_ordinal: 0,
                bytecode_offset: 4,
            },
            Some(JavaScriptPageDebuggerEmittedPausedRootLocation {
                generated_line: 1,
                generated_column_utf16: 2,
                original: Some(JavaScriptPageDebuggerEmittedOriginalPosition {
                    source_index: 0,
                    line: 3,
                    column_utf16: 4,
                }),
            }),
        ))
    }
}

fn session(
    manifest: blueice_ipc::debugger::DebuggerMetadataCapabilityManifest,
) -> DebuggerMetadataSessionAuthorization {
    let hello = DebuggerRequest::Hello {
        protocol_version: DEBUGGER_PROTOCOL_VERSION,
        requested_bounded_values: false,
        requested_metadata_capabilities: manifest.clone(),
    };
    let ack = blueice_ipc::debugger::negotiate(&hello, &manifest);
    blueice_ipc::debugger::metadata_session_authorization(&hello, &ack).unwrap()
}

#[test]
fn generated_public_routes_require_independent_grants_and_same_stream_program_receipt() {
    let (tabs, realm) = loaded_tabs();
    let mut locations = GeneratedLocations {
        paused: false,
        armed: false,
        contradictory_map: false,
        contradictory_binding: false,
        malformed_map_failure: false,
    };
    let program = DebuggerProgram {
        realm,
        program_handle: 7,
        program_generation: 3,
    };
    let target = DebuggerGeneratedBreakpointTarget {
        program,
        generated_line: 1,
        generated_column_utf16: 2,
    };
    let request = || DebuggerRequest::ResolveGeneratedBreakpoint { target };
    assert_eq!(
        handle_debugger_request_with_child_locations(&tabs, &mut locations, None, request()),
        unavailable_generated_breakpoint()
    );
    let breakpoint_session = session(
        blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::opaque_generated_breakpoint(),
    );
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&breakpoint_session),
            request(),
        ),
        unavailable_generated_breakpoint(),
        "a grant without a program inventory receipt cannot resolve a position"
    );
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&breakpoint_session),
            DebuggerRequest::ListPrograms { realm },
        ),
        DebuggerReply::Programs(vec![program])
    );
    let point = DebuggerSafePoint {
        program,
        code_unit_ordinal: 0,
        bytecode_offset: 4,
    };
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&breakpoint_session),
            request(),
        ),
        DebuggerReply::GeneratedBreakpoint(DebuggerGeneratedBreakpoint {
            target,
            safe_point: Some(point),
        })
    );
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&breakpoint_session),
            DebuggerRequest::ResolveGeneratedBreakpoint {
                target: DebuggerGeneratedBreakpointTarget {
                    generated_column_utf16: 3,
                    ..target
                },
            },
        ),
        DebuggerReply::GeneratedBreakpoint(DebuggerGeneratedBreakpoint {
            target: DebuggerGeneratedBreakpointTarget {
                generated_column_utf16: 3,
                ..target
            },
            safe_point: None,
        })
    );
    assert!(matches!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&breakpoint_session),
            DebuggerRequest::ArmGeneratedBreakpoint { target },
        ),
        DebuggerReply::RootSafePointBreakpointArmed { .. }
    ));
    assert!(locations.armed);
    let stale = DebuggerGeneratedBreakpointTarget {
        program: DebuggerProgram {
            program_generation: 4,
            ..program
        },
        ..target
    };
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&breakpoint_session),
            DebuggerRequest::ResolveGeneratedBreakpoint { target: stale },
        ),
        unavailable_generated_breakpoint()
    );

    let paused_session = session(
        blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::opaque_generated_paused_location(
        ),
    );
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&breakpoint_session),
            DebuggerRequest::GetGeneratedPausedLocation { program },
        ),
        unavailable_generated_paused_location()
    );
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&paused_session),
            DebuggerRequest::GetGeneratedPausedLocation { program },
        ),
        unavailable_generated_paused_location(),
        "another stream's program receipt must not transfer"
    );
    let _ = handle_debugger_request_with_child_locations(
        &tabs,
        &mut locations,
        Some(&paused_session),
        DebuggerRequest::ListPrograms { realm },
    );
    locations.paused = true;
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&paused_session),
            DebuggerRequest::GetGeneratedPausedLocation { program },
        ),
        DebuggerReply::GeneratedPausedLocation(DebuggerGeneratedPausedLocation {
            program,
            safe_point: point,
            location: Some(DebuggerGeneratedPausedSourceLocation {
                generated_line: 1,
                generated_column_utf16: 2,
                original: Some(DebuggerGeneratedOriginalPosition {
                    source_index: 0,
                    line: 3,
                    column_utf16: 4,
                }),
            }),
        })
    );
    for (map, binding) in [(true, false), (false, true)] {
        locations.contradictory_map = map;
        locations.contradictory_binding = binding;
        assert!(matches!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&paused_session),
                DebuggerRequest::GetGeneratedPausedLocation { program },
            ),
            DebuggerReply::Error {
                code: DebuggerErrorCode::InvalidExecutionState,
                ..
            }
        ));
    }
}

#[test]
fn receipted_public_generated_routes_hide_a_private_malformed_map_failure() {
    let (tabs, realm) = loaded_tabs();
    let mut locations = GeneratedLocations {
        paused: true,
        armed: false,
        contradictory_map: false,
        contradictory_binding: false,
        malformed_map_failure: true,
    };
    let program = DebuggerProgram {
        realm,
        program_handle: 7,
        program_generation: 3,
    };
    let target = DebuggerGeneratedBreakpointTarget {
        program,
        generated_line: 1,
        generated_column_utf16: 2,
    };
    let grant = session(
        blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::opaque_selected(
            blueice_ipc::debugger::DebuggerMetadataCapabilitySelection {
                generated_breakpoint: true,
                generated_paused_location: true,
                ..Default::default()
            },
        ),
    );
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&grant),
            DebuggerRequest::ListPrograms { realm },
        ),
        DebuggerReply::Programs(vec![program])
    );
    let unavailable = debugger_program_error(JavaScriptPageDebuggerError::NoLiveRealm);
    for request in [
        DebuggerRequest::ResolveGeneratedBreakpoint { target },
        DebuggerRequest::ArmGeneratedBreakpoint { target },
        DebuggerRequest::GetGeneratedPausedLocation { program },
    ] {
        assert_eq!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&grant),
                request
            ),
            unavailable,
            "private map failure must disclose neither coordinates nor its error text"
        );
    }
    assert!(!locations.armed);
}
