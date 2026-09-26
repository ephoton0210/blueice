// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn private_child_snapshots_only_terminal_uncaught_bluets_locations() {
    let mut host = BlueJsChildHost::default();
    let synchronized = host.handle_request(PageHostRequest::SynchronizeDocument {
        document: debugger_document(
            1,
            vec![
                blue_ts_classic(0, "function fail(): number { throw 7; } fail();"),
                classic(1, "globalThis.successor = 1;"),
                blue_ts_classic(
                    2,
                    "function unused(): number { throw 8; } const handled: number = 1;",
                ),
            ],
        ),
    });
    assert!(
        matches!(&synchronized, PageHostReply::Synchronized { reports, .. } if reports.is_empty()),
        "unexpected synchronization reply: {synchronized:?}"
    );
    let programs = match host.handle_request(PageHostRequest::ListDebuggerPrograms {
        tab_id: 7,
        document_generation: 1,
    }) {
        PageHostReply::DebuggerPrograms { programs, .. } => programs,
        reply => panic!("expected three private programs, got {reply:?}"),
    };
    assert_eq!(programs.len(), 3);
    let metadata = match host.handle_request(PageHostRequest::ListDebuggerBlueTsMetadata {
        tab_id: 7,
        document_generation: 1,
        program: programs[0],
    }) {
        PageHostReply::DebuggerBlueTsMetadata { metadata, .. } => metadata[0],
        reply => panic!("expected private BlueTS attachment, got {reply:?}"),
    };
    let request = PageHostRequest::DescribeDebuggerBlueTsExceptionLocation {
        tab_id: 7,
        document_generation: 1,
        program: programs[0],
        metadata,
    };
    assert!(matches!(
        host.handle_request(request.clone()),
        PageHostReply::Error {
            code: PageHostErrorCode::InvalidDebuggerState,
            ..
        }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::DebuggerExecutionAdvanced { reports, .. } if reports.len() == 3
    ));
    let first = host.documents[&7].debugger_programs[&programs[0].program_handle]
        .exception_location
        .expect("first uncaught BlueTS throw must survive successor execution");
    assert_eq!(first.safe_point.program, programs[0]);
    assert_eq!(first.safe_point.code_unit_ordinal, 1);
    assert!(first
        .span
        .coordinates
        .is_well_formed_for_range(first.span.start_byte, first.span.end_byte));
    assert_eq!(first.span.start_byte, 0);
    assert!(first.span.end_byte > first.span.start_byte);
    assert_eq!(
        host.handle_request(request.clone()),
        PageHostReply::DebuggerBlueTsExceptionLocation {
            tab_id: 7,
            document_generation: 1,
            program: programs[0],
            metadata,
            location: PageHostDebuggerBlueTsExceptionLocation {
                safe_point: first.safe_point,
                span: first.span,
            },
        }
    );
    assert!(matches!(
        host.handle_request(PageHostRequest::DescribeDebuggerBlueTsExceptionLocation {
            tab_id: 7,
            document_generation: 1,
            program: programs[1],
            metadata,
        }),
        PageHostReply::Error {
            code: PageHostErrorCode::InvalidRequest,
            ..
        }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::DescribeDebuggerBlueTsExceptionLocation {
            tab_id: 7,
            document_generation: 1,
            program: programs[0],
            metadata: PageHostDebuggerMetadataHandle {
                metadata_generation: metadata.metadata_generation + 1,
                ..metadata
            },
        }),
        PageHostReply::Error {
            code: PageHostErrorCode::InvalidRequest,
            ..
        }
    ));
    for program in &programs[1..] {
        assert!(
            host.documents[&7].debugger_programs[&program.program_handle]
                .exception_location
                .is_none()
        );
    }
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: debugger_document(2, vec![blue_ts_classic(0, "const next = 1;")]),
        }),
        PageHostReply::Synchronized { .. }
    ));
    assert!(!host.documents[&7]
        .debugger_programs
        .contains_key(&programs[0].program_handle));
    assert!(matches!(
        host.handle_request(request),
        PageHostReply::Error {
            code: PageHostErrorCode::StaleDocument,
            ..
        }
    ));
}

#[test]
fn private_child_maps_exact_nested_module_throw_sites_without_guessing() {
    for source in [
        "function inner(): number { throw 9; } export const result: number = inner();",
        "function inner(): number { throw 'x'; } export const result: number = inner();",
    ] {
        let entry = "blueice://page/throwing-module.ts";
        let module = PageHostScript {
            ordinal: 0,
            language: PageHostScriptLanguage::BlueTs,
            kind: PageHostScriptKind::Module,
            graph: graph(entry, vec![PageHostSource::new(entry, source)]),
        };
        let mut host = BlueJsChildHost::default();
        assert!(matches!(
            host.handle_request(PageHostRequest::SynchronizeDocument {
                document: debugger_document(1, vec![module]),
            }),
            PageHostReply::Synchronized { reports, .. } if reports.is_empty()
        ));
        let program = match host.handle_request(PageHostRequest::ListDebuggerPrograms {
            tab_id: 7,
            document_generation: 1,
        }) {
            PageHostReply::DebuggerPrograms { programs, .. } => programs[0],
            reply => panic!("expected private module program, got {reply:?}"),
        };
        assert!(matches!(
            host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
                tab_id: 7,
                document_generation: 1,
            }),
            PageHostReply::DebuggerExecutionAdvanced { reports, .. } if reports.len() == 1
        ));
        let location = host.documents[&7].debugger_programs[&program.program_handle]
            .exception_location
            .expect("exact module throw must retain a private location");
        assert_eq!(location.safe_point.program, program);
        assert_eq!(location.safe_point.code_unit_ordinal, 1);
        assert!(location.span.start_byte < location.span.end_byte);
        assert!(location
            .span
            .coordinates
            .is_well_formed_for_range(location.span.start_byte, location.span.end_byte));
        let runtime_handle =
            host.documents[&7].debugger_programs[&program.program_handle].runtime_handle;
        let retained = host
            .debug_registry
            .get(host.runtime.program_registry(), runtime_handle)
            .unwrap();
        assert!(exact_bluets_span_for_site(
            retained,
            PageHostDebuggerSafePoint {
                bytecode_offset: u32::MAX,
                ..location.safe_point
            }
        )
        .is_none());
    }
}

#[test]
fn private_child_attributes_dependency_throw_to_its_own_bluets_program() {
    let entry = "blueice://page/entry-throw.ts";
    let dependency = "blueice://page/dependency-throw.ts";
    let mut module = PageHostScript {
        ordinal: 0,
        language: PageHostScriptLanguage::BlueTs,
        kind: PageHostScriptKind::Module,
        graph: graph(
            entry,
            vec![
                PageHostSource::new(
                    entry,
                    "import { answer } from './dependency-throw.ts'; export const result: number = answer;",
                ),
                PageHostSource::new(
                    dependency,
                    "function fail(): number { throw 9; } export const answer: number = fail();",
                ),
            ],
        ),
    };
    module.graph.resolutions.push(PageHostStaticResolution {
        from_module: entry.to_string(),
        specifier: "./dependency-throw.ts".to_string(),
        canonical_target: dependency.to_string(),
    });
    let mut host = BlueJsChildHost::default();
    let synchronized = host.handle_request(PageHostRequest::SynchronizeDocument {
        document: debugger_document(1, vec![module]),
    });
    assert!(
        matches!(&synchronized, PageHostReply::Synchronized { reports, .. } if reports.is_empty()),
        "unexpected synchronization reply: {synchronized:?}"
    );
    assert!(matches!(
        host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::DebuggerExecutionAdvanced { reports, .. } if reports.len() == 1
    ));
    let locations = host.documents[&7]
        .debugger_programs
        .iter()
        .filter_map(|(handle, record)| record.exception_location.map(|location| (handle, location)))
        .collect::<Vec<_>>();
    assert_eq!(locations.len(), 1);
    let (handle, location) = locations[0];
    assert_eq!(location.safe_point.program.program_handle, *handle);
    assert_eq!(location.safe_point.code_unit_ordinal, 1);
    let runtime_handle = host.documents[&7].debugger_programs[handle].runtime_handle;
    assert_eq!(
        host.runtime
            .program_registry()
            .get(runtime_handle)
            .unwrap()
            .source()
            .canonical_module_id(),
        dependency
    );
}
