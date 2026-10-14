// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Direct-page BlueTS acceptance through the real supervised child process.

use super::*;

#[test]
fn real_child_executes_the_closed_emitted_bluets_module_graph() {
    const ENTRY: &str = "blueice://page/emitted-module-entry.ts";
    const DEPENDENCY: &str = "blueice://page/emitted-module-dependency.ts";
    let mut source_graph = graph(
        ENTRY,
        vec![
            PageHostSource::new(
                ENTRY,
                "import { value } from './emitted-module-dependency.ts'; if (value !== 41) throw 'wrong emitted import';",
            ),
            PageHostSource::new(DEPENDENCY, "export const value: number = 41;"),
        ],
    );
    source_graph.resolutions.push(PageHostStaticResolution {
        from_module: ENTRY.into(),
        specifier: "./emitted-module-dependency.ts".into(),
        canonical_target: DEPENDENCY.into(),
    });
    let mut host = SpawnedBlueJsHost::spawn().unwrap();
    let reply = host
        .synchronize_document(document(
            1,
            vec![PageHostScript {
                ordinal: 0,
                language: PageHostScriptLanguage::BlueTsEmitted,
                kind: PageHostScriptKind::Module,
                graph: source_graph,
            }],
        ))
        .unwrap();
    assert!(
        matches!(
            &reply,
            PageHostReply::Synchronized { reports, .. }
                if matches!(reports.as_slice(), [report]
                    if report.outcome == PageHostScriptOutcome::Executed)
        ),
        "{reply:?}"
    );
    host.shutdown().unwrap();
}

#[test]
fn real_child_pauses_and_projects_an_emitted_module_entry_artifact() {
    const ENTRY: &str = "blueice://page/emitted-debugger-module.ts";
    const SOURCE: &str = "const marker: number = 0; export const answer: number = 42;";
    let compilation = blueice_bluets::compile(
        ENTRY,
        &blueice_bluets::MapLoader::from([blueice_bluets::ModuleSource::new(ENTRY, SOURCE)]),
        blueice_bluets::CompilerOptions {
            source_map: true,
            inline_sources: true,
            ..Default::default()
        },
    );
    assert!(!compilation.has_errors(), "{:#?}", compilation.diagnostics);
    let output = compilation.output.unwrap();
    let generated = &output.artifacts[ENTRY].javascript;
    let marker_byte = generated.find("const marker").unwrap();
    let before_marker = &generated[..marker_byte];
    let generated_line = before_marker.bytes().filter(|byte| *byte == b'\n').count() as u32;
    let generated_column_utf16 = before_marker
        .rsplit('\n')
        .next()
        .unwrap()
        .encode_utf16()
        .count() as u32;
    let original_column_utf16 = SOURCE.find("const marker").unwrap() as u32;

    let mut page = document(
        1,
        vec![PageHostScript {
            ordinal: 0,
            language: PageHostScriptLanguage::BlueTsEmitted,
            kind: PageHostScriptKind::Module,
            graph: graph(ENTRY, vec![PageHostSource::new(ENTRY, SOURCE)]),
        }],
    );
    page.debugger_execution_control = true;
    let mut host = SpawnedBlueJsHost::spawn().unwrap();
    assert!(matches!(
        host.synchronize_document(page).unwrap(),
        PageHostReply::Synchronized { reports, .. } if reports.is_empty()
    ));
    let program = match host
        .request(PageHostRequest::ListDebuggerPrograms {
            tab_id: 41,
            document_generation: 1,
        })
        .unwrap()
    {
        PageHostReply::DebuggerPrograms { programs, .. } if programs.len() == 1 => programs[0],
        reply => panic!("expected one emitted module, got {reply:?}"),
    };
    assert_eq!(
        host.request(PageHostRequest::ResolveDebuggerEmittedOriginalPosition {
            tab_id: 41,
            document_generation: 1,
            program,
            generated_line,
            generated_column_utf16,
        })
        .unwrap(),
        PageHostReply::DebuggerEmittedOriginalPosition {
            tab_id: 41,
            document_generation: 1,
            program,
            generated_line,
            generated_column_utf16,
            original: Some(PageHostDebuggerEmittedOriginalPosition {
                source_index: 0,
                line: 0,
                column_utf16: original_column_utf16,
            }),
        }
    );
    let safe_point = match host
        .request(PageHostRequest::ResolveDebuggerEmittedSafePoint {
            tab_id: 41,
            document_generation: 1,
            program,
            generated_line,
            generated_column_utf16,
        })
        .unwrap()
    {
        PageHostReply::DebuggerEmittedSafePoint {
            safe_point: Some(safe_point),
            ..
        } => safe_point,
        reply => panic!("the emitted module's first mapped item must bind: {reply:?}"),
    };
    assert!(matches!(
        host.request(PageHostRequest::ArmDebuggerRootSafePointBreakpoint {
            tab_id: 41,
            document_generation: 1,
            safe_point,
        })
        .unwrap(),
        PageHostReply::DebuggerRootSafePointBreakpointArmed { .. }
    ));
    assert!(matches!(
        host.request(PageHostRequest::AdvanceDebuggerExecution {
            tab_id: 41,
            document_generation: 1,
        })
        .unwrap(),
        PageHostReply::DebuggerExecutionAdvanced { reports, .. } if reports.is_empty()
    ));
    assert_eq!(
        host.request(PageHostRequest::GetDebuggerEmittedPausedRootLocation {
            tab_id: 41,
            document_generation: 1,
            program,
        })
        .unwrap(),
        PageHostReply::DebuggerEmittedPausedRootLocation {
            tab_id: 41,
            document_generation: 1,
            program,
            safe_point,
            location: Some(PageHostDebuggerEmittedPausedRootLocation {
                generated_line,
                generated_column_utf16,
                original: Some(PageHostDebuggerEmittedOriginalPosition {
                    source_index: 0,
                    line: 0,
                    column_utf16: original_column_utf16,
                }),
            }),
        }
    );
    assert!(matches!(
        host.request(PageHostRequest::ResumeDebuggerExecution {
            tab_id: 41,
            document_generation: 1,
            program,
        })
        .unwrap(),
        PageHostReply::DebuggerExecutionResumed { .. }
    ));
    assert!(matches!(
        host.request(PageHostRequest::AdvanceDebuggerExecution {
            tab_id: 41,
            document_generation: 1,
        })
        .unwrap(),
        PageHostReply::DebuggerExecutionAdvanced { reports, .. }
            if matches!(reports.as_slice(), [report]
                if report.outcome == PageHostScriptOutcome::Executed)
    ));
    host.shutdown().unwrap();
}

#[test]
fn real_child_binds_emitted_map_lookup_to_one_live_program_generation() {
    const ENTRY: &str = "blueice://page/emitted-entry.ts";
    const SOURCE: &str = "const answer: number = 42; answer;";
    assert!(std::path::Path::new(CHILD_BINARY).exists());
    let mut page = document(
        1,
        vec![PageHostScript {
            ordinal: 0,
            language: PageHostScriptLanguage::BlueTsEmitted,
            kind: PageHostScriptKind::Classic,
            graph: graph(ENTRY, vec![PageHostSource::new(ENTRY, SOURCE)]),
        }],
    );
    page.debugger_execution_control = true;
    let mut host = SpawnedBlueJsHost::spawn().unwrap();
    assert!(matches!(
        host.synchronize_document(page).unwrap(),
        PageHostReply::Synchronized { reports, .. } if reports.is_empty()
    ));
    let programs = match host
        .request(PageHostRequest::ListDebuggerPrograms {
            tab_id: 41,
            document_generation: 1,
        })
        .unwrap()
    {
        PageHostReply::DebuggerPrograms { programs, .. } => programs,
        reply => panic!("expected generated program inventory, got {reply:?}"),
    };
    assert_eq!(programs.len(), 1);
    let program = programs[0];
    assert_eq!(
        host.request(PageHostRequest::ResolveDebuggerEmittedOriginalPosition {
            tab_id: 41,
            document_generation: 1,
            program,
            generated_line: 0,
            generated_column_utf16: 0,
        })
        .unwrap(),
        PageHostReply::DebuggerEmittedOriginalPosition {
            tab_id: 41,
            document_generation: 1,
            program,
            generated_line: 0,
            generated_column_utf16: 0,
            original: Some(PageHostDebuggerEmittedOriginalPosition {
                source_index: 0,
                line: 0,
                column_utf16: 0,
            }),
        }
    );
    let safe_point = match host
        .request(PageHostRequest::ResolveDebuggerEmittedSafePoint {
            tab_id: 41,
            document_generation: 1,
            program,
            generated_line: 0,
            generated_column_utf16: 0,
        })
        .unwrap()
    {
        PageHostReply::DebuggerEmittedSafePoint {
            safe_point: Some(safe_point),
            ..
        } => safe_point,
        reply => panic!("the exact mapped statement must bind, got {reply:?}"),
    };
    assert_eq!(
        host.request(PageHostRequest::ResolveDebuggerEmittedSafePoint {
            tab_id: 41,
            document_generation: 1,
            program,
            generated_line: 0,
            generated_column_utf16: 6,
        })
        .unwrap(),
        PageHostReply::DebuggerEmittedSafePoint {
            tab_id: 41,
            document_generation: 1,
            program,
            generated_line: 0,
            generated_column_utf16: 6,
            safe_point: None,
        },
        "a mapped inner token must not borrow its enclosing statement's safe point"
    );
    assert_eq!(safe_point.program, program);
    assert!(matches!(
        host.request(PageHostRequest::GetDebuggerEmittedPausedRootLocation {
            tab_id: 41,
            document_generation: 1,
            program,
        })
        .unwrap(),
        PageHostReply::Error {
            code: PageHostErrorCode::InvalidDebuggerState,
            ..
        }
    ));
    assert_eq!(
        host.request(PageHostRequest::ValidateDebuggerSafePoint {
            tab_id: 41,
            document_generation: 1,
            safe_point,
        })
        .unwrap(),
        PageHostReply::DebuggerSafePointValidated {
            tab_id: 41,
            document_generation: 1,
            safe_point,
        }
    );
    assert!(matches!(
        host.request(PageHostRequest::ArmDebuggerRootSafePointBreakpoint {
            tab_id: 41,
            document_generation: 1,
            safe_point,
        })
        .unwrap(),
        PageHostReply::DebuggerRootSafePointBreakpointArmed {
            safe_point: armed,
            ..
        } if armed == safe_point
    ));
    assert!(matches!(
        host.request(PageHostRequest::AdvanceDebuggerExecution {
            tab_id: 41,
            document_generation: 1,
        })
        .unwrap(),
        PageHostReply::DebuggerExecutionAdvanced { reports, .. } if reports.is_empty()
    ));
    assert_eq!(
        host.request(PageHostRequest::GetDebuggerExecutionState {
            tab_id: 41,
            document_generation: 1,
            program,
        })
        .unwrap(),
        PageHostReply::DebuggerExecutionState {
            tab_id: 41,
            document_generation: 1,
            program,
            state: PageHostDebuggerExecutionState::Paused { safe_point },
        }
    );
    assert_eq!(
        host.request(PageHostRequest::GetDebuggerEmittedPausedRootLocation {
            tab_id: 41,
            document_generation: 1,
            program,
        })
        .unwrap(),
        PageHostReply::DebuggerEmittedPausedRootLocation {
            tab_id: 41,
            document_generation: 1,
            program,
            safe_point,
            location: Some(PageHostDebuggerEmittedPausedRootLocation {
                generated_line: 0,
                generated_column_utf16: 0,
                original: Some(PageHostDebuggerEmittedOriginalPosition {
                    source_index: 0,
                    line: 0,
                    column_utf16: 0,
                }),
            }),
        }
    );
    let PageHostReply::DebuggerStackSnapshot { snapshot, .. } = host
        .request(PageHostRequest::GetDebuggerStackSnapshot {
            tab_id: 41,
            document_generation: 1,
            program,
            frame: None,
            max_frames: 2,
            max_scope_entries: 8,
        })
        .unwrap()
    else {
        panic!("the exact generated statement must have a paused stack");
    };
    assert_eq!(snapshot.frames.len(), 1);
    assert_eq!(
        snapshot.frames[0].code_unit_ordinal,
        safe_point.code_unit_ordinal
    );
    assert_eq!(
        snapshot.frames[0].bytecode_offset,
        safe_point.bytecode_offset
    );
    assert_eq!(
        host.request(PageHostRequest::StepDebuggerRootInstruction {
            tab_id: 41,
            document_generation: 1,
            program,
        })
        .unwrap(),
        PageHostReply::DebuggerExecutionStepRequested {
            tab_id: 41,
            document_generation: 1,
            program,
        }
    );
    assert!(matches!(
        host.request(PageHostRequest::AdvanceDebuggerExecution {
            tab_id: 41,
            document_generation: 1,
        })
        .unwrap(),
        PageHostReply::DebuggerExecutionAdvanced { reports, .. } if reports.is_empty()
    ));
    let PageHostReply::DebuggerExecutionState {
        state: PageHostDebuggerExecutionState::Paused { safe_point: next },
        ..
    } = host
        .request(PageHostRequest::GetDebuggerExecutionState {
            tab_id: 41,
            document_generation: 1,
            program,
        })
        .unwrap()
    else {
        panic!("one generated root instruction must yield a verified next pause");
    };
    assert_eq!(next.program, program);
    assert_ne!(next.bytecode_offset, safe_point.bytecode_offset);
    assert_eq!(
        host.request(PageHostRequest::GetDebuggerEmittedPausedRootLocation {
            tab_id: 41,
            document_generation: 1,
            program,
        })
        .unwrap(),
        PageHostReply::DebuggerEmittedPausedRootLocation {
            tab_id: 41,
            document_generation: 1,
            program,
            safe_point: next,
            location: None,
        },
        "an instruction inside a statement must not inherit its start location"
    );
    assert!(matches!(
        host.request(PageHostRequest::ResolveDebuggerEmittedOriginalPosition {
            tab_id: 41,
            document_generation: 1,
            program,
            generated_line: u32::MAX,
            generated_column_utf16: 0,
        })
        .unwrap(),
        PageHostReply::Error {
            code: PageHostErrorCode::InvalidRequest,
            ..
        }
    ));
    host.synchronize_document(document(2, vec![])).unwrap();
    assert!(matches!(
        host.request(PageHostRequest::GetDebuggerEmittedPausedRootLocation {
            tab_id: 41,
            document_generation: 1,
            program,
        })
        .unwrap(),
        PageHostReply::Error {
            code: PageHostErrorCode::StaleDocument,
            ..
        }
    ));
    assert!(matches!(
        host.request(PageHostRequest::ResolveDebuggerEmittedOriginalPosition {
            tab_id: 41,
            document_generation: 1,
            program,
            generated_line: 0,
            generated_column_utf16: 0,
        })
        .unwrap(),
        PageHostReply::Error {
            code: PageHostErrorCode::StaleDocument,
            ..
        }
    ));
}

#[test]
fn real_classic_bluets_page_elides_type_import_and_maps_original_declaration() {
    const ENTRY: &str = "blueice://page/classic-entry.ts";
    const TYPES: &str = "blueice://page/types.d.ts";
    const SOURCE: &str = "import type { Shape } from './types.d.ts';\n/* 🚀 */ const typedValue: Shape = 9; globalThis.answer = typedValue;";
    const DECLARATION: &str = "const typedValue: Shape = 9;";
    assert!(std::path::Path::new(CHILD_BINARY).exists());
    let mut source_graph = graph(
        ENTRY,
        vec![
            PageHostSource::new(ENTRY, SOURCE),
            PageHostSource::new(TYPES, "export type Shape = number;"),
        ],
    );
    source_graph.resolver_fingerprint = "reviewed-classic-type-graph-v1".into();
    source_graph.resolutions.push(PageHostStaticResolution {
        from_module: ENTRY.into(),
        specifier: "./types.d.ts".into(),
        canonical_target: TYPES.into(),
    });
    let script = PageHostScript {
        ordinal: 0,
        language: PageHostScriptLanguage::BlueTs,
        kind: PageHostScriptKind::Classic,
        graph: source_graph,
    };
    let mut page = document(1, vec![script]);
    page.debugger_execution_control = true;
    let mut host = SpawnedBlueJsHost::spawn().unwrap();
    let admitted = host.synchronize_document(page).unwrap();
    assert!(
        matches!(&admitted, PageHostReply::Synchronized { reports, .. } if reports.is_empty()),
        "classic type-only graph admission returned {admitted:?}"
    );
    let PageHostReply::DebuggerPrograms { programs, .. } = host
        .request(PageHostRequest::ListDebuggerPrograms {
            tab_id: 41,
            document_generation: 1,
        })
        .unwrap()
    else {
        panic!("the classic entry must install a debugger program")
    };
    assert_eq!(programs.len(), 1, "the type-only module must not execute");
    let program = programs[0];
    let PageHostReply::DebuggerBlueTsMetadata { metadata, .. } = host
        .request(PageHostRequest::ListDebuggerBlueTsMetadata {
            tab_id: 41,
            document_generation: 1,
            program,
        })
        .unwrap()
    else {
        panic!("the classic entry must retain compiler metadata")
    };
    assert_eq!(metadata.len(), 1);
    let metadata = metadata[0];
    let PageHostReply::DebuggerSafePoints { safe_points, .. } = host
        .request(PageHostRequest::ListDebuggerSafePoints {
            tab_id: 41,
            document_generation: 1,
            program,
        })
        .unwrap()
    else {
        panic!("the classic entry must expose verified safe points")
    };
    let (point, span) = safe_points
        .into_iter()
        .find_map(|safe_point| {
            match host
                .request(PageHostRequest::DescribeDebuggerBlueTsSafePointSpan {
                    tab_id: 41,
                    document_generation: 1,
                    metadata,
                    safe_point,
                })
                .unwrap()
            {
                PageHostReply::DebuggerBlueTsSafePointSpan {
                    safe_point: echoed,
                    span,
                    ..
                } if echoed == safe_point
                    && SOURCE.get(span.start_byte as usize..span.end_byte as usize)
                        == Some(DECLARATION) =>
                {
                    Some((safe_point, span))
                }
                PageHostReply::DebuggerBlueTsSafePointSpan { .. }
                | PageHostReply::Error {
                    code: PageHostErrorCode::InvalidRequest,
                    ..
                } => None,
                reply => panic!("unexpected original span reply: {reply:?}"),
            }
        })
        .expect("the original typed declaration must bind to an exact safe point");
    assert_eq!(point.program, program);
    let expected_start = SOURCE.find(DECLARATION).unwrap();
    assert_eq!(span.start_byte as usize, expected_start);
    assert_eq!(span.end_byte as usize, expected_start + DECLARATION.len());
    assert_eq!(span.coordinates.start_line, 1);
    assert_eq!(
        span.coordinates.start_column_utf16,
        SOURCE[SOURCE.find('\n').unwrap() + 1..expected_start]
            .encode_utf16()
            .count() as u32
    );
    let PageHostReply::DebuggerBlueTsMetadataSourceProvenance { provenance, .. } = host
        .request(PageHostRequest::DescribeDebuggerBlueTsMetadataSource {
            tab_id: 41,
            document_generation: 1,
            program,
            metadata,
            source_id: span.source_id,
        })
        .unwrap()
    else {
        panic!("the mapped span must identify its original source")
    };
    assert_eq!(provenance.source_id, span.source_id);
    assert_eq!(provenance.module, ENTRY);
    assert!(!format!("{span:?}").contains(SOURCE));

    let PageHostReply::DebuggerExecutionAdvanced { reports, .. } = host
        .request(PageHostRequest::AdvanceDebuggerExecution {
            tab_id: 41,
            document_generation: 1,
        })
        .unwrap()
    else {
        panic!("the classic entry must execute after debugger admission")
    };
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].ordinal, 0);
    assert_eq!(reports[0].outcome, PageHostScriptOutcome::Executed);

    let runtime_entry = "blueice://page/runtime-import.ts";
    let runtime_dependency = "blueice://page/runtime-dependency.ts";
    let mut runtime_graph = graph(
        runtime_entry,
        vec![
            PageHostSource::new(
                runtime_entry,
                "import { value } from './runtime-dependency.ts'; const answer: number = value;",
            ),
            PageHostSource::new(runtime_dependency, "export const value: number = 9;"),
        ],
    );
    runtime_graph.resolutions.push(PageHostStaticResolution {
        from_module: runtime_entry.into(),
        specifier: "./runtime-dependency.ts".into(),
        canonical_target: runtime_dependency.into(),
    });
    let replacement = document(
        2,
        vec![PageHostScript {
            ordinal: 0,
            language: PageHostScriptLanguage::BlueTs,
            kind: PageHostScriptKind::Classic,
            graph: runtime_graph,
        }],
    );
    let reply = host.synchronize_document(replacement).unwrap();
    assert!(
        matches!(
            &reply,
            PageHostReply::Synchronized { reports, .. }
                if matches!(reports.as_slice(), [PageHostScriptReport {
                    outcome: PageHostScriptOutcome::Rejected { .. },
                    ..
                }])
        ),
        "classic runtime import must remain unavailable: {reply:?}"
    );
    assert!(matches!(
        host.request(PageHostRequest::ListDebuggerPrograms {
            tab_id: 41,
            document_generation: 2,
        })
        .unwrap(),
        PageHostReply::DebuggerPrograms { programs, .. } if programs.is_empty()
    ));
    host.shutdown().unwrap();
}

#[test]
fn real_esm_bluets_page_uses_authorized_target_and_maps_both_original_modules() {
    const ENTRY: &str = "https://example.test/scripts/page-entry.ts";
    const DEPENDENCY: &str = "blueice://authorized/owner-chosen.ts";
    const SPECIFIER: &str = "./answer.ts";
    const ENTRY_SOURCE: &str = "import { chosen } from './answer.ts';\n/* 🚀 */ export const typedResult: number = chosen;";
    const DEPENDENCY_SOURCE: &str = "/* 🌕 */ export const chosen: number = 41;";
    const ENTRY_DECLARATION: &str = "export const typedResult: number = chosen;";
    const DEPENDENCY_DECLARATION: &str = "export const chosen: number = 41;";

    let authorized_graph = |fingerprint: &str, include_resolution: bool| {
        let mut source_graph = graph(
            ENTRY,
            vec![
                PageHostSource::new(ENTRY, ENTRY_SOURCE),
                PageHostSource::new(DEPENDENCY, DEPENDENCY_SOURCE),
            ],
        );
        source_graph.resolver_fingerprint = fingerprint.into();
        if include_resolution {
            source_graph.resolutions.push(PageHostStaticResolution {
                from_module: ENTRY.into(),
                specifier: SPECIFIER.into(),
                canonical_target: DEPENDENCY.into(),
            });
        }
        source_graph
    };
    let module_script = |source_graph| PageHostScript {
        ordinal: 0,
        language: PageHostScriptLanguage::BlueTs,
        kind: PageHostScriptKind::Module,
        graph: source_graph,
    };

    let mut host = SpawnedBlueJsHost::spawn().unwrap();
    let mut page = document(
        1,
        vec![module_script(authorized_graph(
            "reviewed-esm-resolver-v1",
            true,
        ))],
    );
    page.debugger_execution_control = true;
    assert!(matches!(
        host.synchronize_document(page).unwrap(),
        PageHostReply::Synchronized { reports, .. } if reports.is_empty()
    ));
    let PageHostReply::DebuggerPrograms { programs, .. } = host
        .request(PageHostRequest::ListDebuggerPrograms {
            tab_id: 41,
            document_generation: 1,
        })
        .unwrap()
    else {
        panic!("both authorized ESM modules must install debugger programs")
    };
    assert_eq!(programs.len(), 2);
    let mut mapped_modules = std::collections::BTreeSet::new();
    let mut original_options_hash = None;
    for program in programs {
        let PageHostReply::DebuggerBlueTsMetadata { metadata, .. } = host
            .request(PageHostRequest::ListDebuggerBlueTsMetadata {
                tab_id: 41,
                document_generation: 1,
                program,
            })
            .unwrap()
        else {
            panic!("each ESM module must retain BlueTS metadata")
        };
        assert_eq!(metadata.len(), 1);
        let metadata = metadata[0];
        let PageHostReply::DebuggerBlueTsMetadataSummary { summary, .. } = host
            .request(PageHostRequest::DescribeDebuggerBlueTsMetadata {
                tab_id: 41,
                document_generation: 1,
                program,
                metadata,
            })
            .unwrap()
        else {
            panic!("each ESM module must expose its options fingerprint")
        };
        if let Some(previous) = &original_options_hash {
            assert_eq!(previous, &summary.compiler_options_hash);
        } else {
            original_options_hash = Some(summary.compiler_options_hash);
        }
        let PageHostReply::DebuggerSafePoints { safe_points, .. } = host
            .request(PageHostRequest::ListDebuggerSafePoints {
                tab_id: 41,
                document_generation: 1,
                program,
            })
            .unwrap()
        else {
            panic!("each ESM module must retain verified safe points")
        };
        let mut mapped = false;
        for safe_point in safe_points {
            let reply = host
                .request(PageHostRequest::DescribeDebuggerBlueTsSafePointSpan {
                    tab_id: 41,
                    document_generation: 1,
                    metadata,
                    safe_point,
                })
                .unwrap();
            let PageHostReply::DebuggerBlueTsSafePointSpan { span, .. } = reply else {
                assert!(matches!(
                    reply,
                    PageHostReply::Error {
                        code: PageHostErrorCode::InvalidRequest,
                        ..
                    }
                ));
                continue;
            };
            let PageHostReply::DebuggerBlueTsMetadataSourceProvenance { provenance, .. } = host
                .request(PageHostRequest::DescribeDebuggerBlueTsMetadataSource {
                    tab_id: 41,
                    document_generation: 1,
                    program,
                    metadata,
                    source_id: span.source_id,
                })
                .unwrap()
            else {
                panic!("the mapped ESM source must have exact provenance")
            };
            let (source, declaration) = match provenance.module.as_str() {
                ENTRY => (ENTRY_SOURCE, ENTRY_DECLARATION),
                DEPENDENCY => (DEPENDENCY_SOURCE, DEPENDENCY_DECLARATION),
                other => panic!("unexpected unapproved ESM source: {other}"),
            };
            if source.get(span.start_byte as usize..span.end_byte as usize) != Some(declaration) {
                continue;
            }
            assert_eq!(span.start_byte as usize, source.find(declaration).unwrap());
            assert_eq!(
                span.coordinates.start_column_utf16,
                source[source[..span.start_byte as usize]
                    .rfind('\n')
                    .map_or(0, |newline| newline + 1)
                    ..span.start_byte as usize]
                    .encode_utf16()
                    .count() as u32
            );
            mapped_modules.insert(provenance.module);
            mapped = true;
            break;
        }
        assert!(
            mapped,
            "each ESM program must map an original typed declaration"
        );
    }
    assert_eq!(
        mapped_modules,
        [ENTRY.to_string(), DEPENDENCY.to_string()].into()
    );
    assert!(matches!(
        host.request(PageHostRequest::AdvanceDebuggerExecution {
            tab_id: 41,
            document_generation: 1,
        })
        .unwrap(),
        PageHostReply::DebuggerExecutionAdvanced { reports, .. }
            if matches!(reports.as_slice(), [PageHostScriptReport {
                outcome: PageHostScriptOutcome::Executed,
                ..
            }])
    ));

    let mut second = document(
        2,
        vec![module_script(authorized_graph(
            "reviewed-esm-resolver-v2",
            true,
        ))],
    );
    second.debugger_execution_control = true;
    assert!(matches!(
        host.synchronize_document(second).unwrap(),
        PageHostReply::Synchronized { reports, .. } if reports.is_empty()
    ));
    let PageHostReply::DebuggerPrograms { programs, .. } = host
        .request(PageHostRequest::ListDebuggerPrograms {
            tab_id: 41,
            document_generation: 2,
        })
        .unwrap()
    else {
        panic!("the successor ESM graph must install programs")
    };
    let program = programs[0];
    let PageHostReply::DebuggerBlueTsMetadata { metadata, .. } = host
        .request(PageHostRequest::ListDebuggerBlueTsMetadata {
            tab_id: 41,
            document_generation: 2,
            program,
        })
        .unwrap()
    else {
        panic!("the successor ESM program must retain metadata")
    };
    let PageHostReply::DebuggerBlueTsMetadataSummary { summary, .. } = host
        .request(PageHostRequest::DescribeDebuggerBlueTsMetadata {
            tab_id: 41,
            document_generation: 2,
            program,
            metadata: metadata[0],
        })
        .unwrap()
    else {
        panic!("the successor ESM graph must expose its options fingerprint")
    };
    assert_ne!(
        Some(summary.compiler_options_hash),
        original_options_hash,
        "the authorized resolver fingerprint must affect compiler identity"
    );

    let missing_edge = document(
        3,
        vec![module_script(authorized_graph(
            "reviewed-esm-resolver-v2",
            false,
        ))],
    );
    assert!(matches!(
        host.synchronize_document(missing_edge).unwrap(),
        PageHostReply::Synchronized { reports, .. }
            if matches!(reports.as_slice(), [PageHostScriptReport {
                outcome: PageHostScriptOutcome::Rejected { .. },
                ..
            }])
    ));
    assert!(matches!(
        host.request(PageHostRequest::ListDebuggerPrograms {
            tab_id: 41,
            document_generation: 3,
        })
        .unwrap(),
        PageHostReply::DebuggerPrograms { programs, .. } if programs.is_empty()
    ));
    host.shutdown().unwrap();
}
