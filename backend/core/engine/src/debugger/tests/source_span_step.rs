// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

struct SourceSpanStepLocations {
    span_stepping_available: bool,
    state: Result<JavaScriptPageDebuggerExecutionState, JavaScriptPageDebuggerError>,
    step_result: Result<(), JavaScriptPageDebuggerError>,
    step_calls: usize,
}

impl PageJavaScriptDebuggerLocations for SourceSpanStepLocations {
    fn debugger_has_live_realm(&mut self, _tab_id: TabId, _document_generation: u64) -> bool {
        true
    }

    fn debugger_execution_control_available(&self) -> bool {
        true
    }

    fn max_debugger_safe_points_per_program(&self) -> usize {
        2
    }

    fn debugger_safe_points(
        &mut self,
        _tab_id: TabId,
        _document_generation: u64,
        _program_handle: u64,
        _program_generation: u64,
    ) -> Result<
        Vec<crate::script::javascript::JavaScriptPageDebuggerSafePoint>,
        JavaScriptPageDebuggerError,
    > {
        Ok(Vec::new())
    }

    fn validate_debugger_safe_point(
        &mut self,
        _tab_id: TabId,
        _document_generation: u64,
        _program_handle: u64,
        _program_generation: u64,
        _code_unit_ordinal: u32,
        _bytecode_offset: u32,
    ) -> Result<(), JavaScriptPageDebuggerError> {
        Ok(())
    }

    fn debugger_static_metadata_inventory_available(&self) -> bool {
        true
    }

    fn debugger_static_metadata_source_inventory_available(&self) -> bool {
        true
    }

    fn debugger_static_metadata_safe_point_span_available(&self) -> bool {
        true
    }

    fn debugger_source_span_stepping_available(&self) -> bool {
        self.span_stepping_available
    }

    fn debugger_programs(
        &mut self,
        _tab_id: TabId,
        _generation: u64,
    ) -> Result<Vec<JavaScriptPageDebuggerProgram>, JavaScriptPageDebuggerError> {
        Ok(vec![JavaScriptPageDebuggerProgram {
            program_handle: 7,
            program_generation: 3,
        }])
    }

    fn debugger_static_metadata(
        &mut self,
        _tab_id: TabId,
        _generation: u64,
        _program_handle: u64,
        _program_generation: u64,
    ) -> Result<
        Vec<crate::script::javascript::JavaScriptPageDebuggerStaticMetadata>,
        JavaScriptPageDebuggerError,
    > {
        Ok(vec![
            crate::script::javascript::JavaScriptPageDebuggerStaticMetadata {
                metadata_handle: 41,
                metadata_generation: 9,
            },
        ])
    }

    fn debugger_static_metadata_sources(
        &mut self,
        _tab_id: TabId,
        _generation: u64,
        _program_handle: u64,
        _program_generation: u64,
        metadata_handle: u64,
        metadata_generation: u64,
    ) -> Result<
        Vec<crate::script::javascript::JavaScriptPageDebuggerStaticMetadataSourceId>,
        JavaScriptPageDebuggerError,
    > {
        if (metadata_handle, metadata_generation) != (41, 9) {
            return Err(JavaScriptPageDebuggerError::UnknownProgram);
        }
        Ok(vec![
            crate::script::javascript::JavaScriptPageDebuggerStaticMetadataSourceId {
                source_id: 0,
            },
        ])
    }

    fn debugger_static_metadata_safe_point_span(
        &mut self,
        _tab_id: TabId,
        _generation: u64,
        target: JavaScriptPageDebuggerStaticMetadataSafePointSpanTarget,
    ) -> Result<
        crate::script::javascript::JavaScriptPageDebuggerStaticMetadataSafePointSpan,
        JavaScriptPageDebuggerError,
    > {
        if (
            target.code_unit_ordinal,
            target.bytecode_offset,
            target.source_id,
        ) != (1, 0, 0)
        {
            return Err(JavaScriptPageDebuggerError::UnknownProgram);
        }
        Ok(
            crate::script::javascript::JavaScriptPageDebuggerStaticMetadataSafePointSpan {
                source_id: target.source_id,
                start_byte: 8,
                end_byte: 40,
                coordinates: DebuggerSourceCoordinates {
                    start_line: 0,
                    start_column_utf16: 8,
                    end_line: 0,
                    end_column_utf16: 40,
                },
            },
        )
    }

    fn debugger_execution_state(
        &mut self,
        _tab_id: TabId,
        _document_generation: u64,
        _program_handle: u64,
        _program_generation: u64,
    ) -> Result<JavaScriptPageDebuggerExecutionState, JavaScriptPageDebuggerError> {
        self.state
    }

    fn step_debugger_bluets_source_span(
        &mut self,
        _tab_id: TabId,
        _document_generation: u64,
        target: JavaScriptPageDebuggerStaticMetadataSafePointSpanTarget,
    ) -> Result<(), JavaScriptPageDebuggerError> {
        self.step_calls += 1;
        assert_eq!(target.program_handle, 7);
        assert_eq!(target.program_generation, 3);
        assert_eq!(target.metadata_handle, 41);
        assert_eq!(target.metadata_generation, 9);
        assert_eq!(target.source_id, 0);
        assert_eq!(target.code_unit_ordinal, 1);
        assert_eq!(target.bytecode_offset, 0);
        self.step_result
    }
}

fn granted_session() -> DebuggerMetadataSessionAuthorization {
    let manifest =
        blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::opaque_source_span_step();
    let hello = DebuggerRequest::Hello {
        protocol_version: DEBUGGER_PROTOCOL_VERSION,
        requested_bounded_values: false,
        requested_metadata_capabilities: manifest.clone(),
    };
    let reply = blueice_ipc::debugger::negotiate(&hello, &manifest);
    blueice_ipc::debugger::metadata_session_authorization(&hello, &reply)
        .expect("explicit source-span-step grant must create a session")
}

#[test]
fn malformed_source_span_step_target_is_rejected() {
    let (tabs, realm) = loaded_tabs();
    let mismatched_program = DebuggerProgram {
        realm,
        program_handle: 99,
        program_generation: 1,
    };
    let program = DebuggerProgram {
        realm,
        program_handle: 7,
        program_generation: 3,
    };
    let metadata = DebuggerStaticMetadataHandle {
        program: mismatched_program,
        metadata_handle: 41,
        metadata_generation: 9,
    };
    let target = DebuggerStaticMetadataSafePointSpanTarget {
        safe_point: DebuggerSafePoint {
            program,
            code_unit_ordinal: 1,
            bytecode_offset: 0,
        },
        source: DebuggerStaticMetadataSourceId {
            metadata,
            source_id: 0,
        },
    };
    let mut locations = SourceSpanStepLocations {
        span_stepping_available: true,
        state: Ok(JavaScriptPageDebuggerExecutionState::Paused {
            code_unit_ordinal: 1,
            bytecode_offset: 0,
        }),
        step_result: Ok(()),
        step_calls: 0,
    };
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            None,
            DebuggerRequest::StepStaticMetadataSourceSpan { target },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            message: "invalid debugger BlueTS source-span step target".to_string(),
        }
    );
    assert_eq!(locations.step_calls, 0);
}

#[test]
fn source_span_step_requires_its_own_grant_prior_observation_and_a_matching_pause() {
    let (tabs, realm) = loaded_tabs();
    let program = DebuggerProgram {
        realm,
        program_handle: 7,
        program_generation: 3,
    };
    let metadata = DebuggerStaticMetadataHandle {
        program,
        metadata_handle: 41,
        metadata_generation: 9,
    };
    let source = DebuggerStaticMetadataSourceId {
        metadata,
        source_id: 0,
    };
    let safe_point = DebuggerSafePoint {
        program,
        code_unit_ordinal: 1,
        bytecode_offset: 0,
    };
    let target = DebuggerStaticMetadataSafePointSpanTarget { safe_point, source };
    let request = DebuggerRequest::StepStaticMetadataSourceSpan { target };

    // No metadata session at all.
    let mut locations = SourceSpanStepLocations {
        span_stepping_available: true,
        state: Ok(JavaScriptPageDebuggerExecutionState::Paused {
            code_unit_ordinal: 1,
            bytecode_offset: 0,
        }),
        step_result: Ok(()),
        step_calls: 0,
    };
    assert_eq!(
        handle_debugger_request_with_child_locations(&tabs, &mut locations, None, request.clone()),
        unavailable_static_metadata_source_span_step()
    );

    // A granted session that has not yet observed the metadata/source.
    let session = granted_session();
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&session),
            request.clone(),
        ),
        unavailable_static_metadata_source_span_step()
    );
    assert_eq!(locations.step_calls, 0);

    // Observe the metadata and source through the ordinary inventory routes.
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&session),
            DebuggerRequest::ListStaticMetadata { program },
        ),
        DebuggerReply::StaticMetadata(vec![metadata])
    );
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&session),
            DebuggerRequest::ListStaticMetadataSources { metadata },
        ),
        DebuggerReply::StaticMetadataSources(vec![source])
    );

    // Observed, but the live child does not report span-stepping as
    // available, so the capability itself is never `Available`.
    let mut unstepping_locations = SourceSpanStepLocations {
        span_stepping_available: false,
        state: Ok(JavaScriptPageDebuggerExecutionState::Paused {
            code_unit_ordinal: 1,
            bytecode_offset: 0,
        }),
        step_result: Ok(()),
        step_calls: 0,
    };
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut unstepping_locations,
            Some(&session),
            DebuggerRequest::ListStaticMetadata { program },
        ),
        DebuggerReply::StaticMetadata(vec![metadata])
    );
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut unstepping_locations,
            Some(&session),
            DebuggerRequest::ListStaticMetadataSources { metadata },
        ),
        DebuggerReply::StaticMetadataSources(vec![source])
    );
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut unstepping_locations,
            Some(&session),
            request.clone(),
        ),
        unavailable_static_metadata_source_span_step()
    );
    assert_eq!(unstepping_locations.step_calls, 0);

    // Observed and capable, but the paused state does not match the
    // requested safe point exactly.
    let mut wrong_state = SourceSpanStepLocations {
        span_stepping_available: true,
        state: Ok(JavaScriptPageDebuggerExecutionState::Paused {
            code_unit_ordinal: 1,
            bytecode_offset: 4,
        }),
        step_result: Ok(()),
        step_calls: 0,
    };
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut wrong_state,
            Some(&session),
            DebuggerRequest::ListStaticMetadata { program },
        ),
        DebuggerReply::StaticMetadata(vec![metadata])
    );
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut wrong_state,
            Some(&session),
            DebuggerRequest::ListStaticMetadataSources { metadata },
        ),
        DebuggerReply::StaticMetadataSources(vec![source])
    );
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut wrong_state,
            Some(&session),
            request.clone(),
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidExecutionState,
            message: "BlueTS source-span step requires the exact paused root safe point"
                .to_string(),
        }
    );
    assert_eq!(wrong_state.step_calls, 0);

    // Fully observed, capable, and paused at the exact safe point: the
    // request reaches the child and propagates both its success...
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&session),
            request.clone(),
        ),
        DebuggerReply::ExecutionSourceSpanStepRequested { safe_point }
    );
    assert_eq!(locations.step_calls, 1);

    // ...and its failure.
    let mut failing_locations = SourceSpanStepLocations {
        span_stepping_available: true,
        state: Ok(JavaScriptPageDebuggerExecutionState::Paused {
            code_unit_ordinal: 1,
            bytecode_offset: 0,
        }),
        step_result: Err(JavaScriptPageDebuggerError::UnknownProgram),
        step_calls: 0,
    };
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut failing_locations,
            Some(&session),
            DebuggerRequest::ListStaticMetadata { program },
        ),
        DebuggerReply::StaticMetadata(vec![metadata])
    );
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut failing_locations,
            Some(&session),
            DebuggerRequest::ListStaticMetadataSources { metadata },
        ),
        DebuggerReply::StaticMetadataSources(vec![source])
    );
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut failing_locations,
            Some(&session),
            request,
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            message: "unknown debugger program".to_string(),
        }
    );
    assert_eq!(failing_locations.step_calls, 1);
}
