// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn linked_public_route_requires_both_receipts_and_never_returns_partial_spans() {
    let (tabs, realm) = loaded_tabs();
    let dependency = DebuggerProgram {
        realm,
        program_handle: 7,
        program_generation: 3,
    };
    let entry = DebuggerProgram {
        realm,
        program_handle: 8,
        program_generation: 4,
    };
    let point = DebuggerSafePoint {
        program: dependency,
        code_unit_ordinal: 1,
        bytecode_offset: 0,
    };
    let arm = DebuggerLinkedArmTarget {
        entry,
        dependency_safe_point: point,
    };
    let mut locations = LinkedLocations {
        moved_caller: false,
        bad_second_source: false,
        arm_calls: 0,
        span_calls: 0,
        resume_calls: 0,
    };
    let DebuggerReply::Capabilities(capabilities) = handle_debugger_request_with_child_locations(
        &tabs,
        &mut locations,
        None,
        DebuggerRequest::DescribeCapabilities { realm },
    ) else {
        panic!("linked capability report is required")
    };
    assert!(capabilities.reports.iter().any(|report| report.capability
        == DebuggerCapability::LinkedModules
        && report.state == DebuggerCapabilityState::Available));
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            None,
            DebuggerRequest::ArmLinkedNestedSafePointBreakpoint {
                target: DebuggerLinkedArmTarget {
                    entry: dependency,
                    ..arm
                }
            }
        ),
        invalid_linked_target()
    );
    assert_eq!(locations.arm_calls, 0);
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            None,
            DebuggerRequest::ArmLinkedNestedSafePointBreakpoint { target: arm }
        ),
        DebuggerReply::LinkedNestedSafePointBreakpointArmed { target: arm }
    );
    assert_eq!(locations.arm_calls, 1);
    let DebuggerReply::LinkedExecutionState { state, .. } =
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            None,
            DebuggerRequest::GetLinkedExecutionState { entry },
        )
    else {
        panic!("linked state must carry the complete stack")
    };
    let DebuggerLinkedExecutionState::Paused { stack } = *state else {
        panic!("expected paused linked stack")
    };
    assert_eq!(stack.frames[0].frame.program, dependency);
    assert_eq!(stack.frames[1].frame.program, entry);
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            None,
            DebuggerRequest::GetLinkedStack {
                top_frame: stack.frames[0].frame
            }
        ),
        DebuggerReply::LinkedStack(Box::new(stack))
    );
    let sources = [dependency, entry].map(|program| DebuggerStaticMetadataSourceId {
        metadata: DebuggerStaticMetadataHandle {
            program,
            metadata_handle: program.program_handle + 40,
            metadata_generation: 9,
        },
        source_id: 0,
    });
    let target = DebuggerLinkedStackCoordinatesTarget {
        expected_stack: stack,
        sources,
    };
    let request = DebuggerRequest::GetLinkedStackCoordinates { target };
    let manifest =
        blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::opaque_safe_point_span();
    let hello = DebuggerRequest::Hello {
        protocol_version: DEBUGGER_PROTOCOL_VERSION,
        requested_bounded_values: false,
        requested_metadata_capabilities: manifest.clone(),
    };
    let reply = blueice_ipc::debugger::negotiate(&hello, &manifest);
    let session = blueice_ipc::debugger::metadata_session_authorization(&hello, &reply).unwrap();
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&session),
            request.clone()
        ),
        unavailable_static_metadata_safe_point_span()
    );
    assert_eq!(locations.span_calls, 0);
    for (index, (program, source)) in [(dependency, sources[0]), (entry, sources[1])]
        .into_iter()
        .enumerate()
    {
        assert_eq!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&session),
                DebuggerRequest::ListStaticMetadata { program }
            ),
            DebuggerReply::StaticMetadata(vec![source.metadata])
        );
        assert_eq!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&session),
                DebuggerRequest::ListStaticMetadataSources {
                    metadata: source.metadata
                }
            ),
            DebuggerReply::StaticMetadataSources(vec![source])
        );
        if index == 0 {
            assert_eq!(
                handle_debugger_request_with_child_locations(
                    &tabs,
                    &mut locations,
                    Some(&session),
                    request.clone()
                ),
                unavailable_static_metadata_safe_point_span()
            );
            assert_eq!(locations.span_calls, 0);
        }
    }
    let DebuggerReply::LinkedStackCoordinates(result) =
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&session),
            request.clone(),
        )
    else {
        panic!("both independently receipted sources must yield both spans")
    };
    assert_eq!(result.stack, stack);
    assert_eq!(result.spans.map(|span| span.source), sources);
    assert_eq!(locations.span_calls, 1);
    let inventory_manifest =
        blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::opaque_source_inventory();
    let inventory_hello = DebuggerRequest::Hello {
        protocol_version: DEBUGGER_PROTOCOL_VERSION,
        requested_bounded_values: false,
        requested_metadata_capabilities: inventory_manifest.clone(),
    };
    let inventory_reply = blueice_ipc::debugger::negotiate(&inventory_hello, &inventory_manifest);
    let no_span_session =
        blueice_ipc::debugger::metadata_session_authorization(&inventory_hello, &inventory_reply)
            .unwrap();
    for (program, source) in [(dependency, sources[0]), (entry, sources[1])] {
        assert_eq!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&no_span_session),
                DebuggerRequest::ListStaticMetadata { program }
            ),
            DebuggerReply::StaticMetadata(vec![source.metadata])
        );
        assert_eq!(
            handle_debugger_request_with_child_locations(
                &tabs,
                &mut locations,
                Some(&no_span_session),
                DebuggerRequest::ListStaticMetadataSources {
                    metadata: source.metadata
                }
            ),
            DebuggerReply::StaticMetadataSources(vec![source])
        );
    }
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&no_span_session),
            request.clone()
        ),
        unavailable_static_metadata_safe_point_span()
    );
    assert_eq!(locations.span_calls, 1);
    let other_session =
        blueice_ipc::debugger::metadata_session_authorization(&hello, &reply).unwrap();
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&other_session),
            request.clone()
        ),
        unavailable_static_metadata_safe_point_span()
    );
    assert_eq!(locations.span_calls, 1);
    locations.moved_caller = true;
    assert!(matches!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&session),
            request.clone()
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidExecutionState,
            ..
        }
    ));
    assert_eq!(locations.span_calls, 1);
    locations.moved_caller = false;
    locations.bad_second_source = true;
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&session),
            request
        ),
        invalid_linked_target()
    );
    assert_eq!(locations.span_calls, 2);
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            None,
            DebuggerRequest::ResumeLinkedNestedExecution {
                top_frame: stack.frames[0].frame
            }
        ),
        DebuggerReply::LinkedNestedResumeRequested {
            top_frame: stack.frames[0].frame
        }
    );
    assert_eq!(locations.resume_calls, 1);
}

#[test]
fn batched_stack_coordinates_recheck_receipts_and_the_entire_live_stack() {
    let (tabs, realm) = loaded_tabs();
    let program = DebuggerProgram {
        realm,
        program_handle: 7,
        program_generation: 3,
    };
    let frame = DebuggerFrame {
        program,
        code_unit_ordinal: 1,
        core_instance: [7; 16],
        frame_handle: 19,
    };
    let child = DebuggerSafePoint {
        program,
        code_unit_ordinal: 1,
        bytecode_offset: 0,
    };
    let root = DebuggerSafePoint {
        program,
        code_unit_ordinal: 0,
        bytecode_offset: 41,
    };
    let metadata = DebuggerStaticMetadataHandle {
        program,
        metadata_handle: 41,
        metadata_generation: 9,
    };
    let sources = [0, 1].map(|source_id| DebuggerStaticMetadataSourceId {
        metadata,
        source_id,
    });
    let target = DebuggerStackCoordinatesTarget {
        expected_stack: DebuggerStackSnapshot {
            program,
            frame: Some(frame),
            safe_points: vec![child, root],
            stack_truncated: false,
        },
        sources: sources.to_vec(),
    };
    let request = DebuggerRequest::GetStackCoordinates {
        target: target.clone(),
    };
    let manifest =
        blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::opaque_safe_point_span();
    let hello = DebuggerRequest::Hello {
        protocol_version: DEBUGGER_PROTOCOL_VERSION,
        requested_bounded_values: false,
        requested_metadata_capabilities: manifest.clone(),
    };
    let reply = blueice_ipc::debugger::negotiate(&hello, &manifest);
    let session = blueice_ipc::debugger::metadata_session_authorization(&hello, &reply).unwrap();
    let mut locations = StackCoordinateLocations {
        moved_root_offset: 41,
        unbound_root: false,
        span_calls: 0,
        value_preview: None,
        value_calls: 0,
    };
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&session),
            request.clone()
        ),
        unavailable_static_metadata_safe_point_span()
    );
    assert_eq!(locations.span_calls, 0);
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&session),
            DebuggerRequest::ListStaticMetadata { program }
        ),
        DebuggerReply::StaticMetadata(vec![metadata])
    );
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&session),
            request.clone()
        ),
        unavailable_static_metadata_safe_point_span()
    );
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&session),
            DebuggerRequest::ListStaticMetadataSources { metadata }
        ),
        DebuggerReply::StaticMetadataSources(sources.to_vec())
    );
    let DebuggerReply::StackCoordinates(result) = handle_debugger_request_with_child_locations(
        &tabs,
        &mut locations,
        Some(&session),
        request.clone(),
    ) else {
        panic!("a fully receipted exact stack must receive all coordinates");
    };
    assert_eq!(result.stack, target.expected_stack);
    assert_eq!(
        result
            .spans
            .iter()
            .map(|span| span.source)
            .collect::<Vec<_>>(),
        sources
    );
    assert_eq!(locations.span_calls, 2);

    let separate_session =
        blueice_ipc::debugger::metadata_session_authorization(&hello, &reply).unwrap();
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&separate_session),
            request.clone()
        ),
        unavailable_static_metadata_safe_point_span()
    );
    let mut guessed = target.clone();
    guessed.sources[1].source_id = 2;
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&session),
            DebuggerRequest::GetStackCoordinates { target: guessed }
        ),
        unavailable_static_metadata_safe_point_span()
    );
    locations.moved_root_offset = 42;
    assert!(matches!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&session),
            request.clone()
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidExecutionState,
            ..
        }
    ));
    assert_eq!(locations.span_calls, 2);
    locations.moved_root_offset = 41;
    locations.unbound_root = true;
    assert!(!matches!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&session),
            request
        ),
        DebuggerReply::StackCoordinates(_)
    ));
    assert_eq!(locations.span_calls, 4);
}

#[test]
fn source_text_probe_and_unknown_command_have_target_independent_typed_refusals() {
    let (tabs, realm) = loaded_tabs();
    let live_realm_program = DebuggerProgram {
        realm,
        program_handle: 7,
        program_generation: 3,
    };
    let foreign_realm_program = DebuggerProgram {
        realm: DebuggerPageRealm {
            tab_id: realm.tab_id + 1,
            ..realm
        },
        program_handle: u64::MAX,
        program_generation: u64::MAX,
    };
    let expected = DebuggerReply::Error {
        code: DebuggerErrorCode::CapabilityUnavailable,
        message: "debugger operation is unavailable".to_string(),
    };
    for request in [
        DebuggerRequest::GetSourceText {
            program: live_realm_program,
        },
        DebuggerRequest::GetSourceText {
            program: foreign_realm_program,
        },
        DebuggerRequest::Unknown,
    ] {
        assert_eq!(
            handle_debugger_request_with_javascript_executor(&tabs, None, request),
            expected
        );
    }
}
