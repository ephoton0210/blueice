// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;
mod admission;
mod debugger_control;
mod dom_transport;
mod exception_sites;
mod linked_module;
mod metadata_inventory;
mod metadata_roots;
mod module_stepping;
mod nested_scheduler;
mod resource_accounting;
mod source_spans;
mod static_scope_values;

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
