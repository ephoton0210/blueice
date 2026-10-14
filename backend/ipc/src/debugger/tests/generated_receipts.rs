// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn generated_grants_are_independent_and_program_receipts_are_stream_bound() {
    let requested =
        DebuggerMetadataCapabilityManifest::opaque_selected(DebuggerMetadataCapabilitySelection {
            generated_breakpoint: true,
            generated_paused_location: true,
            ..Default::default()
        });
    assert_eq!(
        requested.capabilities,
        vec![
            DebuggerMetadataCapability::OpaqueGeneratedBreakpoint,
            DebuggerMetadataCapability::OpaqueGeneratedPausedLocation,
        ]
    );
    assert!(requested.is_well_formed());
    assert!(!requested.contains(DebuggerMetadataCapability::OpaqueInventory));
    let request = hello(requested);
    let owner = DebuggerMetadataCapabilityManifest::opaque_generated_breakpoint();
    let ack = negotiate(&request, &owner);
    let session = metadata_session_authorization(&request, &ack).unwrap();
    let separate = metadata_session_authorization(&request, &ack).unwrap();
    assert!(session.permits(DebuggerMetadataCapability::OpaqueGeneratedBreakpoint));
    assert!(!session.permits(DebuggerMetadataCapability::OpaqueGeneratedPausedLocation));
    let program = DebuggerProgram {
        realm: realm(),
        program_handle: 11,
        program_generation: 13,
    };
    assert!(!session.observed_program(program));
    assert!(session.observe_programs(&[program]));
    assert!(session.observed_program(program));
    assert!(!separate.observed_program(program));
    assert!(!session.observed_program(DebuggerProgram {
        program_generation: 14,
        ..program
    }));
    assert!(!session.observed_program(DebuggerProgram {
        realm: DebuggerPageRealm {
            realm_generation: 4,
            ..program.realm
        },
        ..program
    }));
    assert!(!session.observe_programs(&[DebuggerProgram {
        program_handle: 0,
        ..program
    }]));
    assert!(session.observed_program(program));
    let reports = capabilities(vec![capability_report(
        DebuggerCapability::GeneratedBreakpoint,
        DebuggerCapabilityState::Available,
    )]);
    assert!(reports
        .authorize_metadata(
            &session,
            DebuggerMetadataCapability::OpaqueGeneratedBreakpoint
        )
        .is_some());
    assert!(reports
        .authorize_metadata(
            &session,
            DebuggerMetadataCapability::OpaqueGeneratedPausedLocation
        )
        .is_none());
}

#[test]
fn generated_program_receipt_budget_is_atomic() {
    let manifest = DebuggerMetadataCapabilityManifest::opaque_generated_paused_location();
    let request = hello(manifest.clone());
    let ack = negotiate(&request, &manifest);
    let session = metadata_session_authorization(&request, &ack).unwrap();
    let base = DebuggerProgram {
        realm: realm(),
        program_handle: 1,
        program_generation: 1,
    };
    let page = (1..=DEBUGGER_SESSION_MAX_OBSERVED_PROGRAMS)
        .map(|handle| DebuggerProgram {
            program_handle: handle as u64,
            ..base
        })
        .collect::<Vec<_>>();
    assert!(session.observe_programs(&page));
    let overflow = DebuggerProgram {
        program_handle: (DEBUGGER_SESSION_MAX_OBSERVED_PROGRAMS + 1) as u64,
        ..base
    };
    assert!(!session.observe_programs(&[overflow]));
    assert!(session.observed_program(page[0]));
    assert!(!session.observed_program(overflow));
}
