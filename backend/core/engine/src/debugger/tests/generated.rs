// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;
use crate::script::javascript::{
    JavaScriptPageDebuggerEmittedOriginalPosition, JavaScriptPageDebuggerEmittedPausedRootLocation,
    JavaScriptPageDebuggerNestedExecutionState, JavaScriptPageDebuggerStackFrame,
    JavaScriptPageDebuggerStackSnapshot, JavaScriptPageDebuggerSystemStackFrame,
    JavaScriptPageDebuggerSystemStackLocations, JavaScriptPageDebuggerSystemStackSnapshot,
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

struct SystemGeneratedLocations {
    include_loader: bool,
    foreign_location: bool,
    contradictory_original: bool,
    projection_calls: usize,
}

impl PageJavaScriptDebuggerLocations for SystemGeneratedLocations {
    fn debugger_has_live_realm(&mut self, _tab: TabId, generation: u64) -> bool {
        generation == 1
    }

    fn max_debugger_safe_points_per_program(&self) -> usize {
        3
    }

    fn debugger_safe_points(
        &mut self,
        _tab: TabId,
        _generation: u64,
        _handle: u64,
        _program_generation: u64,
    ) -> Result<Vec<JavaScriptPageDebuggerSafePoint>, JavaScriptPageDebuggerError> {
        Ok(vec![])
    }

    fn debugger_programs(
        &mut self,
        _tab: TabId,
        _generation: u64,
    ) -> Result<Vec<JavaScriptPageDebuggerProgram>, JavaScriptPageDebuggerError> {
        let emitted = JavaScriptPageDebuggerProgram {
            program_handle: 7,
            program_generation: 3,
        };
        let loader = JavaScriptPageDebuggerProgram {
            program_handle: 8,
            program_generation: 4,
        };
        Ok(if self.include_loader {
            vec![emitted, loader]
        } else {
            vec![emitted]
        })
    }

    fn debugger_execution_control_available(&self) -> bool {
        true
    }

    fn debugger_stack_available(&self) -> bool {
        true
    }

    fn debugger_nested_frames_available(&self) -> bool {
        true
    }

    fn debugger_emitted_paused_root_location_available(&self) -> bool {
        true
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
        if (handle, program_generation, ordinal, offset) == (7, 3, 1, 4) {
            Ok(())
        } else {
            Err(JavaScriptPageDebuggerError::InvalidSafePoint)
        }
    }

    fn debugger_nested_execution_state(
        &mut self,
        tab_id: TabId,
        generation: u64,
        handle: u64,
        program_generation: u64,
    ) -> Result<Option<JavaScriptPageDebuggerNestedExecutionState>, JavaScriptPageDebuggerError>
    {
        if (handle, program_generation) != (7, 3) {
            return Err(JavaScriptPageDebuggerError::StaleProgram);
        }
        Ok(Some(JavaScriptPageDebuggerNestedExecutionState::Paused {
            frame: JavaScriptPageDebuggerFrame {
                tab_id,
                document_generation: generation,
                program_handle: handle,
                program_generation,
                code_unit_ordinal: 1,
                core_instance: [7; 16],
                frame_handle: 9,
            },
            bytecode_offset: 4,
        }))
    }

    fn debugger_system_stack_locations(
        &mut self,
        frame: JavaScriptPageDebuggerFrame,
        loader: JavaScriptPageDebuggerProgram,
        max_scope_entries: u32,
    ) -> Result<JavaScriptPageDebuggerSystemStackLocations, JavaScriptPageDebuggerError> {
        self.projection_calls += 1;
        let emitted = JavaScriptPageDebuggerProgram {
            program_handle: frame.program_handle,
            program_generation: frame.program_generation,
        };
        let frames = [
            JavaScriptPageDebuggerSystemStackFrame {
                program: emitted,
                safe_point: JavaScriptPageDebuggerSafePoint {
                    code_unit_ordinal: 1,
                    bytecode_offset: 4,
                },
                scope_entries: vec![],
                scope_truncated: false,
            },
            JavaScriptPageDebuggerSystemStackFrame {
                program: loader,
                safe_point: JavaScriptPageDebuggerSafePoint {
                    code_unit_ordinal: 1,
                    bytecode_offset: 2,
                },
                scope_entries: vec![],
                scope_truncated: false,
            },
            JavaScriptPageDebuggerSystemStackFrame {
                program: emitted,
                safe_point: JavaScriptPageDebuggerSafePoint {
                    code_unit_ordinal: 0,
                    bytecode_offset: 3,
                },
                scope_entries: vec![],
                scope_truncated: false,
            },
        ];
        Ok(JavaScriptPageDebuggerSystemStackLocations {
            stack: JavaScriptPageDebuggerSystemStackSnapshot {
                frame,
                frames,
                max_scope_entries,
            },
            locations: [
                Some(JavaScriptPageDebuggerEmittedPausedRootLocation {
                    generated_line: 1,
                    generated_column_utf16: 2,
                    original: Some(JavaScriptPageDebuggerEmittedOriginalPosition {
                        source_index: 0,
                        line: 3,
                        column_utf16: 4,
                    }),
                }),
                self.foreign_location
                    .then_some(JavaScriptPageDebuggerEmittedPausedRootLocation {
                        generated_line: 1,
                        generated_column_utf16: 2,
                        original: None,
                    }),
                None,
            ],
        })
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
        if (handle, program_generation, line, column) != (7, 3, 1, 2) {
            return Err(JavaScriptPageDebuggerError::StaleProgram);
        }
        Ok(Some(JavaScriptPageDebuggerEmittedOriginalPosition {
            source_index: 0,
            line: 3,
            column_utf16: if self.contradictory_original { 5 } else { 4 },
        }))
    }
}

#[test]
fn system_stack_disclosure_requires_both_same_stream_receipts_and_never_maps_loader() {
    let (tabs, realm) = loaded_tabs();
    let emitted = DebuggerProgram {
        realm,
        program_handle: 7,
        program_generation: 3,
    };
    let loader = DebuggerProgram {
        program_handle: 8,
        program_generation: 4,
        ..emitted
    };
    let frame = DebuggerFrame {
        program: emitted,
        code_unit_ordinal: 1,
        core_instance: [7; 16],
        frame_handle: 9,
    };
    let request = || DebuggerRequest::GetGeneratedSystemStackLocations { frame, loader };
    let mut locations = SystemGeneratedLocations {
        include_loader: false,
        foreign_location: false,
        contradictory_original: false,
        projection_calls: 0,
    };
    assert_eq!(
        handle_debugger_request_with_child_locations(&tabs, &mut locations, None, request()),
        unavailable_generated_paused_location()
    );
    let authorized = session(
        blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::opaque_generated_paused_location(
        ),
    );
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&authorized),
            request(),
        ),
        unavailable_generated_paused_location()
    );
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&authorized),
            DebuggerRequest::ListPrograms { realm },
        ),
        DebuggerReply::Programs(vec![emitted])
    );
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&authorized),
            request(),
        ),
        unavailable_generated_paused_location(),
        "an emitted receipt alone must not disclose a foreign loader frame"
    );
    assert_eq!(locations.projection_calls, 0);
    let breakpoint_only = session(
        blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::opaque_generated_breakpoint(),
    );
    locations.include_loader = true;
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&breakpoint_only),
            DebuggerRequest::ListPrograms { realm },
        ),
        DebuggerReply::Programs(vec![emitted, loader])
    );
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&breakpoint_only),
            request(),
        ),
        unavailable_generated_paused_location(),
        "the generated-breakpoint grant cannot disclose a paused stack"
    );
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&authorized),
            DebuggerRequest::ListPrograms { realm },
        ),
        DebuggerReply::Programs(vec![emitted, loader])
    );
    let other_stream = session(
        blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::opaque_generated_paused_location(
        ),
    );
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&other_stream),
            request(),
        ),
        unavailable_generated_paused_location(),
        "program receipts never transfer to another debugger stream"
    );
    let reply = handle_debugger_request_with_child_locations(
        &tabs,
        &mut locations,
        Some(&authorized),
        request(),
    );
    let DebuggerReply::GeneratedSystemStackLocations(result) = reply else {
        panic!("expected exact generated System stack: {reply:?}")
    };
    assert!(result.is_well_formed());
    assert_eq!(result.frame, frame);
    assert_eq!(result.loader, loader);
    assert_eq!(result.safe_points[1].program, loader);
    assert_eq!(result.locations[1], None);
    assert_eq!(result.locations[0].unwrap().original.unwrap().line, 3);
    assert_eq!(locations.projection_calls, 1);
    assert!(matches!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&authorized),
            DebuggerRequest::GetGeneratedSystemStackLocations {
                frame: DebuggerFrame {
                    frame_handle: frame.frame_handle + 1,
                    ..frame
                },
                loader,
            },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidExecutionState,
            ..
        }
    ));
    assert_eq!(locations.projection_calls, 1);
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&authorized),
            DebuggerRequest::GetGeneratedSystemStackLocations {
                frame,
                loader: DebuggerProgram {
                    program_generation: 5,
                    ..loader
                },
            },
        ),
        unavailable_generated_paused_location(),
        "a substituted loader generation cannot use the original receipt"
    );
    assert_eq!(locations.projection_calls, 1);
    locations.contradictory_original = true;
    assert!(matches!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&authorized),
            request(),
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidExecutionState,
            ..
        }
    ));
    locations.contradictory_original = false;
    locations.foreign_location = true;
    assert!(matches!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&authorized),
            request(),
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidExecutionState,
            ..
        }
    ));
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
