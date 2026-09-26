// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn child_bluets_safe_point_span_requires_an_exact_live_metadata_attachment() {
    let mut host = BlueJsChildHost::default();
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: document(
                1,
                vec![
                    blue_ts_classic(0, "const first: number = 1;"),
                    blue_ts_classic(1, "const second: number = 2;"),
                ],
            ),
        }),
        PageHostReply::Synchronized { reports, .. }
            if reports.iter().all(|report| report.outcome == PageHostScriptOutcome::Executed)
    ));
    let programs = match host.handle_request(PageHostRequest::ListDebuggerPrograms {
        tab_id: 7,
        document_generation: 1,
    }) {
        PageHostReply::DebuggerPrograms { programs, .. } => programs,
        reply => panic!("expected two private BlueTS programs, got {reply:?}"),
    };
    assert_eq!(programs.len(), 2);
    let program = programs[0];
    let other_program = programs[1];
    let metadata = match host.handle_request(PageHostRequest::ListDebuggerBlueTsMetadata {
        tab_id: 7,
        document_generation: 1,
        program,
    }) {
        PageHostReply::DebuggerBlueTsMetadata { metadata, .. } => metadata[0],
        reply => panic!("expected exact private metadata handle, got {reply:?}"),
    };
    let entry = {
        let handle = host.documents[&7].debugger_programs[&program.program_handle].runtime_handle;
        host.debug_registry
            .get(host.runtime.program_registry(), handle)
            .unwrap()
            .safe_point_map()
            .entries[0]
            .clone()
    };
    let safe_point = PageHostDebuggerSafePoint {
        program,
        code_unit_ordinal: entry.code_unit.ordinal(),
        bytecode_offset: entry.bytecode_offset,
    };
    let request = PageHostRequest::DescribeDebuggerBlueTsSafePointSpan {
        tab_id: 7,
        document_generation: 1,
        metadata,
        safe_point,
    };
    let PageHostReply::DebuggerBlueTsSafePointSpan {
        span,
        safe_point: echoed,
        ..
    } = host.handle_request(request.clone())
    else {
        panic!("an exact retained safe point must have its original BlueTS span")
    };
    assert_eq!(echoed, safe_point);
    assert_eq!(
        (span.start_byte, span.end_byte),
        (
            u32::try_from(entry.start_byte).unwrap(),
            u32::try_from(entry.end_byte).unwrap()
        )
    );
    assert!(span.start_byte < span.end_byte);
    assert!(span.end_byte <= DEBUGGER_STATIC_METADATA_MAX_SOURCE_SPAN_BYTES);
    assert!(span
        .coordinates
        .is_well_formed_for_range(span.start_byte, span.end_byte));
    assert!(!format!("{span:?}").contains("const first"));
    assert!(!format!("{span:?}").contains("inline-0.ts"));

    for (metadata, safe_point) in [
        (
            metadata,
            PageHostDebuggerSafePoint {
                program: other_program,
                ..safe_point
            },
        ),
        (
            PageHostDebuggerMetadataHandle {
                metadata_generation: metadata.metadata_generation + 1,
                ..metadata
            },
            safe_point,
        ),
        (
            metadata,
            PageHostDebuggerSafePoint {
                bytecode_offset: u32::MAX,
                ..safe_point
            },
        ),
    ] {
        assert!(matches!(
            host.handle_request(PageHostRequest::DescribeDebuggerBlueTsSafePointSpan {
                tab_id: 7,
                document_generation: 1,
                metadata,
                safe_point,
            }),
            PageHostReply::Error {
                code: PageHostErrorCode::InvalidRequest,
                ..
            }
        ));
    }
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: document(2, vec![blue_ts_classic(0, "const successor = 3;")]),
        }),
        PageHostReply::Synchronized { .. }
    ));
    assert!(matches!(
        host.handle_request(request),
        PageHostReply::Error {
            code: PageHostErrorCode::StaleDocument,
            ..
        }
    ));
}

#[test]
fn child_bluets_source_breakpoint_is_bound_to_a_live_source_and_generation() {
    let mut host = BlueJsChildHost::default();
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: document(
                1,
                vec![
                    blue_ts_classic(
                        0,
                        "const first: number = 1; const second: number = 2; second;"
                    ),
                    blue_ts_classic(1, "const other: number = 3;"),
                ],
            ),
        }),
        PageHostReply::Synchronized { .. }
    ));
    let programs = match host.handle_request(PageHostRequest::ListDebuggerPrograms {
        tab_id: 7,
        document_generation: 1,
    }) {
        PageHostReply::DebuggerPrograms { programs, .. } => programs,
        reply => panic!("expected private BlueTS programs, got {reply:?}"),
    };
    let program = programs[0];
    let metadata = match host.handle_request(PageHostRequest::ListDebuggerBlueTsMetadata {
        tab_id: 7,
        document_generation: 1,
        program,
    }) {
        PageHostReply::DebuggerBlueTsMetadata { metadata, .. } => metadata[0],
        reply => panic!("expected private BlueTS metadata, got {reply:?}"),
    };
    let (source_id, first, second) = {
        let handle = host.documents[&7].debugger_programs[&program.program_handle].runtime_handle;
        let retained = host
            .debug_registry
            .get(host.runtime.program_registry(), handle)
            .unwrap();
        let mut entries = retained.safe_point_map().entries.iter().collect::<Vec<_>>();
        entries.sort_by_key(|entry| (entry.start_byte, entry.bytecode_offset));
        let first = entries[0];
        let second = entries
            .iter()
            .copied()
            .find(|entry| entry.source == first.source && entry.start_byte >= first.end_byte)
            .expect("the next distinct declaration has a bound entry");
        (
            retained
                .static_info()
                .sources
                .iter()
                .find(|source| source.module == entries[0].source)
                .expect("the lowered source must have a compiler source ID")
                .id
                .0,
            first.clone(),
            second.clone(),
        )
    };
    let request = |source_byte| PageHostRequest::ResolveDebuggerBlueTsSourceBreakpoint {
        tab_id: 7,
        document_generation: 1,
        program,
        metadata,
        source_id,
        source_byte,
    };
    for (source_byte, entry) in [
        (u32::try_from(first.start_byte).unwrap(), &first),
        (u32::try_from(first.end_byte).unwrap(), &second),
    ] {
        assert_eq!(
            host.handle_request(request(source_byte)),
            PageHostReply::DebuggerBlueTsSourceBreakpoint {
                tab_id: 7,
                document_generation: 1,
                program,
                metadata,
                source_id,
                source_byte,
                safe_point: Some(PageHostDebuggerSafePoint {
                    program,
                    code_unit_ordinal: entry.code_unit.ordinal(),
                    bytecode_offset: entry.bytecode_offset,
                }),
            }
        );
    }
    assert_eq!(
        host.handle_request(request(u32::try_from(second.end_byte).unwrap() + 10)),
        PageHostReply::DebuggerBlueTsSourceBreakpoint {
            tab_id: 7,
            document_generation: 1,
            program,
            metadata,
            source_id,
            source_byte: u32::try_from(second.end_byte).unwrap() + 10,
            safe_point: None,
        }
    );
    for forged in [
        PageHostRequest::ResolveDebuggerBlueTsSourceBreakpoint {
            tab_id: 7,
            document_generation: 1,
            program: programs[1],
            metadata,
            source_id,
            source_byte: 0,
        },
        PageHostRequest::ResolveDebuggerBlueTsSourceBreakpoint {
            tab_id: 7,
            document_generation: 1,
            program,
            metadata,
            source_id: u32::MAX,
            source_byte: 0,
        },
        PageHostRequest::ResolveDebuggerBlueTsSourceBreakpoint {
            tab_id: 7,
            document_generation: 1,
            program,
            metadata: PageHostDebuggerMetadataHandle {
                metadata_generation: metadata.metadata_generation + 1,
                ..metadata
            },
            source_id,
            source_byte: 0,
        },
        request(DEBUGGER_STATIC_METADATA_MAX_SOURCE_SPAN_BYTES + 1),
    ] {
        assert!(matches!(
            host.handle_request(forged),
            PageHostReply::Error {
                code: PageHostErrorCode::InvalidRequest,
                ..
            }
        ));
    }
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: document(2, vec![blue_ts_classic(0, "const successor = 4;")]),
        }),
        PageHostReply::Synchronized { .. }
    ));
    assert!(matches!(
        host.handle_request(request(0)),
        PageHostReply::Error {
            code: PageHostErrorCode::StaleDocument,
            ..
        }
    ));
}
