// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![cfg(unix)]

//! Real-subprocess evidence for the launcher-owned BlueJS child host. The
//! library tests cover source-graph validation; this test proves the actual
//! sibling binary, private socket readiness, authenticated handshake,
//! document generation lifecycle, source-free responses, and cleanup path.

use blueice_ipc::page_host::{
    PageHostDocument, PageHostModuleGraph, PageHostReply, PageHostScript, PageHostScriptKind,
    PageHostScriptOutcome, PageHostSource, PageHostStaticResolution,
};
use blueice_launcher::bluejs_host::SpawnedBlueJsHost;

// Ensure Cargo builds the sibling child binary before `SpawnedBlueJsHost`
// derives its path from this integration-test executable.
const CHILD_BINARY: &str = env!("CARGO_BIN_EXE_blueice-bluejs-host");

fn graph(entry: &str, modules: Vec<PageHostSource>) -> PageHostModuleGraph {
    PageHostModuleGraph {
        entry: entry.to_string(),
        modules,
        resolutions: Vec::new(),
        resolver_fingerprint: "core-page-loader-v1".to_string(),
    }
}

fn document(generation: u64, scripts: Vec<PageHostScript>) -> PageHostDocument {
    PageHostDocument {
        tab_id: 41,
        document_generation: generation,
        origin: "https://example.test".to_string(),
        scripts,
    }
}

#[test]
fn launcher_spawns_an_isolated_host_that_executes_closed_graphs_and_reaps_cleanly() {
    assert!(
        std::path::Path::new(CHILD_BINARY).exists(),
        "Cargo must build the actual sibling BlueJS child host"
    );
    let mut host = SpawnedBlueJsHost::spawn()
        .expect("launcher must start and authenticate an isolated BlueJS child");
    let private_socket = host.socket_path().to_path_buf();
    assert!(private_socket.exists());

    let entry = "blueice://page/entry.js";
    let dependency = "blueice://page/dependency.js";
    let mut module = PageHostScript {
        ordinal: 1,
        kind: PageHostScriptKind::Module,
        graph: graph(
            entry,
            vec![
                PageHostSource::new(
                    entry,
                    "import { answer } from './dependency.js'; export const result = answer;",
                ),
                PageHostSource::new(dependency, "export const answer = 42;"),
            ],
        ),
    };
    module.graph.resolutions.push(PageHostStaticResolution {
        from_module: entry.to_string(),
        specifier: "./dependency.js".to_string(),
        canonical_target: dependency.to_string(),
    });
    let classic_id = "blueice://page/classic.js";
    let classic = PageHostScript {
        ordinal: 0,
        kind: PageHostScriptKind::Classic,
        graph: graph(
            classic_id,
            vec![PageHostSource::new(classic_id, "globalThis.answer = 42;")],
        ),
    };

    let reply = host
        .synchronize_document(document(1, vec![classic, module]))
        .expect("private host request must receive a reply");
    assert!(matches!(
        reply,
        PageHostReply::Synchronized {
            already_current: false,
            reports,
            ..
        } if reports.len() == 2
            && reports.iter().all(|report| report.outcome == PageHostScriptOutcome::Executed)
    ));

    // Repeating a generation is a protocol-level idempotency guarantee: it
    // must not execute source again or offer stale callers a replay gadget.
    let replay = host
        .synchronize_document(document(1, Vec::new()))
        .expect("current-generation sync must be answered");
    assert!(matches!(
        replay,
        PageHostReply::Synchronized {
            already_current: true,
            reports,
            ..
        } if reports.is_empty()
    ));

    host.shutdown()
        .expect("launcher must obtain child shutdown acknowledgement");
    assert!(
        !private_socket.exists(),
        "launcher must clean the private child socket after a clean shutdown"
    );
}
