// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;
mod admission;
mod debugger_control;
mod diagnostic_profiles;
mod dom_transport;
mod error_paths;
mod exception_sites;
mod linked_module;
mod metadata_inventory;
mod metadata_roots;
mod module_stepping;
mod nested_scheduler;
mod resource_accounting;
mod source_spans;
mod static_scope_values;

#[test]
fn script_cleanup_runs_jobs_before_the_next_child_declaration_even_after_a_throw() {
    for debugger in [false, true] {
        for throws in [false, true] {
            let mut host = BlueJsChildHost::default();
            let first = format!(
                "var checkpoint = 0; Promise.resolve().then(function () {{ checkpoint = 1; }}); {}",
                if throws { "throw 'script';" } else { "" },
            );
            let mut document = document(
                1,
                vec![
                    classic(0, &first),
                    classic(
                        1,
                        "if (checkpoint !== 1) { throw 'missing script cleanup'; }",
                    ),
                ],
            );
            document.debugger_execution_control = debugger;
            let PageHostReply::Synchronized { mut reports, .. } =
                host.handle_request(PageHostRequest::SynchronizeDocument { document })
            else {
                panic!("document must synchronize");
            };
            if debugger {
                assert!(reports.is_empty());
                let PageHostReply::DebuggerExecutionAdvanced {
                    reports: advanced, ..
                } = host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
                    tab_id: 7,
                    document_generation: 1,
                })
                else {
                    panic!("debugger declarations must advance");
                };
                reports = advanced;
            }
            assert_eq!(reports.len(), 2);
            assert_eq!(reports[1].outcome, PageHostScriptOutcome::Executed);
            assert_eq!(
                matches!(reports[0].outcome, PageHostScriptOutcome::Rejected { .. }),
                throws
            );
        }
    }
}

fn graph(entry: &str, modules: Vec<PageHostSource>) -> PageHostModuleGraph {
    PageHostModuleGraph {
        entry: entry.to_string(),
        modules,
        resolutions: Vec::new(),
        resolver_fingerprint: "core-page-loader-v1".to_string(),
    }
}

fn document(generation: u64, scripts: Vec<PageHostScript>) -> PageHostDocument {
    document_with_snapshot(
        generation,
        "test document snapshot".to_string(),
        "https://example.test".to_string(),
        scripts,
    )
}

fn document_with_snapshot(
    generation: u64,
    document_text: String,
    document_origin: String,
    scripts: Vec<PageHostScript>,
) -> PageHostDocument {
    PageHostDocument {
        tab_id: 7,
        document_generation: generation,
        snapshot: PageHostDocumentSnapshot {
            document_text,
            document_origin,
        },
        debugger_execution_control: false,
        emitted_bluets_profile: PageHostBlueTsEmissionProfile::Standard,
        emitted_module_page_console_ambient: false,
        emitted_classic_page_console_ambient: false,
        scripts,
    }
}

fn debugger_document(generation: u64, scripts: Vec<PageHostScript>) -> PageHostDocument {
    let mut document = document(generation, scripts);
    document.debugger_execution_control = true;
    document
}

fn classic(ordinal: u32, source: &str) -> PageHostScript {
    let id = format!("blueice://page/inline-{ordinal}.js");
    PageHostScript {
        ordinal,
        language: PageHostScriptLanguage::JavaScript,
        kind: PageHostScriptKind::Classic,
        graph: graph(&id, vec![PageHostSource::new(id.clone(), source)]),
    }
}

fn blue_ts_classic(ordinal: u32, source: &str) -> PageHostScript {
    let id = format!("blueice://page/inline-{ordinal}.ts");
    PageHostScript {
        ordinal,
        language: PageHostScriptLanguage::BlueTs,
        kind: PageHostScriptKind::Classic,
        graph: graph(&id, vec![PageHostSource::new(id.clone(), source)]),
    }
}

fn blue_ts_emitted_classic(ordinal: u32, source: &str) -> PageHostScript {
    let id = format!("blueice://page/inline-{ordinal}.ts");
    PageHostScript {
        ordinal,
        language: PageHostScriptLanguage::BlueTsEmitted,
        kind: PageHostScriptKind::Classic,
        graph: graph(&id, vec![PageHostSource::new(id.clone(), source)]),
    }
}

fn paused_value_targets(
    host: &mut BlueJsChildHost,
    program: PageHostDebuggerProgram,
    frame: Option<PageHostDebuggerFrame>,
    frame_index: usize,
) -> Vec<PageHostDebuggerValueTarget> {
    let snapshot = match host.handle_request(PageHostRequest::GetDebuggerStackSnapshot {
        tab_id: 7,
        document_generation: 1,
        program,
        frame,
        max_frames: 2,
        max_scope_entries: 256,
    }) {
        PageHostReply::DebuggerStackSnapshot { snapshot, .. } => snapshot,
        reply => panic!("expected active stack: {reply:?}"),
    };
    let selected = &snapshot.frames[frame_index];
    selected
        .scope_entries
        .iter()
        .copied()
        .map(|scope_entry| PageHostDebuggerValueTarget {
            tab_id: 7,
            document_generation: 1,
            program,
            frame,
            frame_index: frame_index as u32,
            safe_point: PageHostDebuggerSafePoint {
                program,
                code_unit_ordinal: selected.code_unit_ordinal,
                bytecode_offset: selected.bytecode_offset,
            },
            scope_entry,
        })
        .collect()
}
