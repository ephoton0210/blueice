// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

struct TypeDisplayLocations {
    type_display_available: bool,
    display_result: Result<
        crate::script::javascript::JavaScriptPageDebuggerStaticMetadataTypeDisplay,
        JavaScriptPageDebuggerError,
    >,
    display_calls: usize,
}

impl PageJavaScriptDebuggerLocations for TypeDisplayLocations {
    fn debugger_has_live_realm(&mut self, _tab_id: TabId, _document_generation: u64) -> bool {
        true
    }

    fn max_debugger_safe_points_per_program(&self) -> usize {
        1
    }

    fn debugger_programs(
        &mut self,
        _tab_id: TabId,
        _document_generation: u64,
    ) -> Result<Vec<JavaScriptPageDebuggerProgram>, JavaScriptPageDebuggerError> {
        Ok(vec![JavaScriptPageDebuggerProgram {
            program_handle: 7,
            program_generation: 3,
        }])
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

    fn debugger_static_metadata_type_inventory_available(&self) -> bool {
        true
    }

    fn debugger_static_metadata_type_display_available(&self) -> bool {
        self.type_display_available
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

    fn debugger_static_metadata_types(
        &mut self,
        _tab_id: TabId,
        _generation: u64,
        _program_handle: u64,
        _program_generation: u64,
        metadata_handle: u64,
        metadata_generation: u64,
    ) -> Result<
        Vec<crate::script::javascript::JavaScriptPageDebuggerStaticMetadataTypeId>,
        JavaScriptPageDebuggerError,
    > {
        if (metadata_handle, metadata_generation) != (41, 9) {
            return Err(JavaScriptPageDebuggerError::UnknownProgram);
        }
        Ok(vec![
            crate::script::javascript::JavaScriptPageDebuggerStaticMetadataTypeId { type_id: 5 },
        ])
    }

    fn debugger_static_metadata_type_display(
        &mut self,
        _tab_id: TabId,
        _generation: u64,
        target: crate::script::javascript::JavaScriptPageDebuggerStaticMetadataTypeTarget,
    ) -> Result<
        crate::script::javascript::JavaScriptPageDebuggerStaticMetadataTypeDisplay,
        JavaScriptPageDebuggerError,
    > {
        self.display_calls += 1;
        assert_eq!(target.program_handle, 7);
        assert_eq!(target.program_generation, 3);
        assert_eq!(target.metadata_handle, 41);
        assert_eq!(target.metadata_generation, 9);
        assert_eq!(target.type_id, 5);
        self.display_result.clone()
    }
}

fn granted_type_display_session() -> DebuggerMetadataSessionAuthorization {
    let manifest = blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::opaque_type_display();
    let hello = DebuggerRequest::Hello {
        protocol_version: DEBUGGER_PROTOCOL_VERSION,
        requested_bounded_values: false,
        requested_metadata_capabilities: manifest.clone(),
    };
    let reply = blueice_ipc::debugger::negotiate(&hello, &manifest);
    blueice_ipc::debugger::metadata_session_authorization(&hello, &reply)
        .expect("explicit type-display grant must create a session")
}

#[test]
fn malformed_type_display_target_is_rejected() {
    let (tabs, realm) = loaded_tabs();
    let program = DebuggerProgram {
        realm,
        program_handle: 7,
        program_generation: 3,
    };
    let malformed_metadata = DebuggerStaticMetadataHandle {
        program,
        metadata_handle: 0,
        metadata_generation: 9,
    };
    let mut locations = TypeDisplayLocations {
        type_display_available: true,
        display_result: Ok(
            crate::script::javascript::JavaScriptPageDebuggerStaticMetadataTypeDisplay {
                type_id: 5,
                display: "number".to_string(),
            },
        ),
        display_calls: 0,
    };
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            None,
            DebuggerRequest::DescribeStaticMetadataType {
                static_type: DebuggerStaticMetadataTypeId {
                    metadata: malformed_metadata,
                    type_id: 5,
                },
            },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            message: "invalid debugger static metadata type display target".to_string(),
        }
    );
    assert_eq!(locations.display_calls, 0);
}

#[test]
fn type_display_requires_its_own_grant_prior_observation_and_a_well_formed_result() {
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
    let static_type = DebuggerStaticMetadataTypeId {
        metadata,
        type_id: 5,
    };
    let request = DebuggerRequest::DescribeStaticMetadataType { static_type };

    // No metadata session.
    let mut locations = TypeDisplayLocations {
        type_display_available: true,
        display_result: Ok(
            crate::script::javascript::JavaScriptPageDebuggerStaticMetadataTypeDisplay {
                type_id: 5,
                display: "number".to_string(),
            },
        ),
        display_calls: 0,
    };
    assert_eq!(
        handle_debugger_request_with_child_locations(&tabs, &mut locations, None, request.clone()),
        unavailable_static_metadata_type_display()
    );

    // A granted session that has not yet observed this type ID.
    let session = granted_type_display_session();
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&session),
            request.clone(),
        ),
        unavailable_static_metadata_type_display()
    );
    assert_eq!(locations.display_calls, 0);

    // Observe the type through the ordinary inventory routes.
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
            DebuggerRequest::ListStaticMetadataTypes { metadata },
        ),
        DebuggerReply::StaticMetadataTypes(vec![static_type])
    );

    // Observed, but the live child does not report the display capability
    // itself as available, so the capability is never `Available`.
    let mut undisplayable = TypeDisplayLocations {
        type_display_available: false,
        display_result: Ok(
            crate::script::javascript::JavaScriptPageDebuggerStaticMetadataTypeDisplay {
                type_id: 5,
                display: "number".to_string(),
            },
        ),
        display_calls: 0,
    };
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut undisplayable,
            Some(&session),
            DebuggerRequest::ListStaticMetadata { program },
        ),
        DebuggerReply::StaticMetadata(vec![metadata])
    );
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut undisplayable,
            Some(&session),
            DebuggerRequest::ListStaticMetadataTypes { metadata },
        ),
        DebuggerReply::StaticMetadataTypes(vec![static_type])
    );
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut undisplayable,
            Some(&session),
            request.clone(),
        ),
        unavailable_static_metadata_type_display()
    );
    assert_eq!(undisplayable.display_calls, 0);

    // Fully observed and capable: the request reaches the child and its
    // successful, well-formed display is returned...
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut locations,
            Some(&session),
            request.clone(),
        ),
        DebuggerReply::StaticMetadataType(DebuggerStaticMetadataTypeDisplay {
            static_type,
            display: "number".to_string(),
        })
    );
    assert_eq!(locations.display_calls, 1);

    // ...a malformed (empty) display from the child is rejected rather than
    // forwarded...
    let mut malformed_display = TypeDisplayLocations {
        type_display_available: true,
        display_result: Ok(
            crate::script::javascript::JavaScriptPageDebuggerStaticMetadataTypeDisplay {
                type_id: 5,
                display: String::new(),
            },
        ),
        display_calls: 0,
    };
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut malformed_display,
            Some(&session),
            DebuggerRequest::ListStaticMetadata { program },
        ),
        DebuggerReply::StaticMetadata(vec![metadata])
    );
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut malformed_display,
            Some(&session),
            DebuggerRequest::ListStaticMetadataTypes { metadata },
        ),
        DebuggerReply::StaticMetadataTypes(vec![static_type])
    );
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut malformed_display,
            Some(&session),
            request.clone(),
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            message: "invalid debugger static metadata type display".to_string(),
        }
    );
    assert_eq!(malformed_display.display_calls, 1);

    // ...and a child-reported error propagates as one too.
    let mut failing = TypeDisplayLocations {
        type_display_available: true,
        display_result: Err(JavaScriptPageDebuggerError::UnknownProgram),
        display_calls: 0,
    };
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut failing,
            Some(&session),
            DebuggerRequest::ListStaticMetadata { program },
        ),
        DebuggerReply::StaticMetadata(vec![metadata])
    );
    assert_eq!(
        handle_debugger_request_with_child_locations(
            &tabs,
            &mut failing,
            Some(&session),
            DebuggerRequest::ListStaticMetadataTypes { metadata },
        ),
        DebuggerReply::StaticMetadataTypes(vec![static_type])
    );
    assert_eq!(
        handle_debugger_request_with_child_locations(&tabs, &mut failing, Some(&session), request),
        DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            message: "unknown debugger program".to_string(),
        }
    );
    assert_eq!(failing.display_calls, 1);
}
