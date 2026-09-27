// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Direct-page BlueTS acceptance through the real supervised child process.

use super::*;

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
