// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![cfg(unix)]

//! Real-subprocess evidence for the launcher-owned BlueJS child host. The
//! library tests cover source-graph validation; this test proves the actual
//! sibling binary, private socket readiness, authenticated handshake,
//! document generation lifecycle, source-free responses, and cleanup path.

use blueice_ipc::page_host::{
    PageHostDocument, PageHostDocumentSnapshot, PageHostErrorCode, PageHostModuleGraph,
    PageHostReply, PageHostRequest, PageHostScript, PageHostScriptKind, PageHostScriptLanguage,
    PageHostScriptOutcome, PageHostScriptReport, PageHostSource, PageHostStaticResolution,
};
use blueice_launcher::bluejs_host::{BlueJsHostRuntimeLimits, SpawnedBlueJsHost};
use std::os::unix::net::UnixStream;

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
        snapshot: PageHostDocumentSnapshot {
            document_text: "test document snapshot".to_string(),
            document_origin: "https://example.test".to_string(),
        },
        debugger_execution_control: false,
        scripts,
    }
}

fn blue_ts_classic(ordinal: u32, source: &str) -> PageHostScript {
    let source_id = format!("blueice://page/typed-{ordinal}.ts");
    PageHostScript {
        ordinal,
        language: PageHostScriptLanguage::BlueTs,
        kind: PageHostScriptKind::Classic,
        graph: graph(
            &source_id,
            vec![PageHostSource::new(source_id.clone(), source)],
        ),
    }
}

fn blue_ts_module_with_dependency(ordinal: u32) -> PageHostScript {
    let entry = "https://cdn.example.test/assets/external.ts";
    let dependency = "https://cdn.example.test/assets/answer.ts";
    let mut graph = graph(
        entry,
        vec![
            PageHostSource::new(
                entry,
                "import { answer } from './answer.ts'; export const result: number = answer;",
            ),
            PageHostSource::new(dependency, "export const answer: number = 41;"),
        ],
    );
    graph.resolver_fingerprint = "core-external-bluets-policy-v1".to_string();
    graph.resolutions.push(PageHostStaticResolution {
        from_module: entry.to_string(),
        specifier: "./answer.ts".to_string(),
        canonical_target: dependency.to_string(),
    });
    PageHostScript {
        ordinal,
        language: PageHostScriptLanguage::BlueTs,
        kind: PageHostScriptKind::Module,
        graph,
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
        language: PageHostScriptLanguage::JavaScript,
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
        language: PageHostScriptLanguage::JavaScript,
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
        &reply,
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

#[test]
fn launcher_owner_runtime_limits_reach_the_real_child_before_program_admission() {
    assert!(
        std::path::Path::new(CHILD_BINARY).exists(),
        "Cargo must build the actual sibling BlueJS child host"
    );
    let limits = BlueJsHostRuntimeLimits {
        max_realms: 1,
        max_programs_per_realm: 1,
        max_bytecode_bytes_per_realm: 1,
        max_heap_bytes_per_realm: BlueJsHostRuntimeLimits::default().max_heap_bytes_per_realm,
        max_reserved_programs: 1,
        max_reserved_bytecode_bytes: 1,
        max_reserved_heap_bytes: BlueJsHostRuntimeLimits::default().max_heap_bytes_per_realm,
    };
    let mut host = SpawnedBlueJsHost::spawn_with_runtime_limits(limits)
        .expect("the launcher must bootstrap the child with owner limits");
    let source_id = "blueice://page/over-bytecode-budget.js";
    let reply = host
        .synchronize_document(document(
            1,
            vec![PageHostScript {
                ordinal: 0,
                language: PageHostScriptLanguage::JavaScript,
                kind: PageHostScriptKind::Classic,
                graph: graph(
                    source_id,
                    vec![PageHostSource::new(source_id, "globalThis.answer = 42;")],
                ),
            }],
        ))
        .expect("the child must answer the bounded document request");
    assert!(matches!(
        reply,
        PageHostReply::Synchronized { reports, .. }
            if matches!(
                reports.as_slice(),
                [PageHostScriptReport {
                    outcome: PageHostScriptOutcome::Rejected { .. },
                    ..
                }]
            )
    ));
    assert!(matches!(
        host.request(blueice_ipc::page_host::PageHostRequest::GetRealmStats {
            tab_id: 41,
            document_generation: 1,
        })
        .expect("the child must report its source-free accounting"),
        PageHostReply::RealmStats(stats)
            if stats.program_count == 0 && stats.bytecode_bytes == 0
    ));
    host.shutdown()
        .expect("the owner must cleanly stop the bounded child");
}

#[test]
fn launcher_child_reserves_aggregate_capacity_before_admitting_a_second_tab() {
    assert!(std::path::Path::new(CHILD_BINARY).exists());
    let per_realm_heap = BlueJsHostRuntimeLimits::default().max_heap_bytes_per_realm;
    let limits = BlueJsHostRuntimeLimits {
        max_realms: 2,
        max_programs_per_realm: 1,
        max_bytecode_bytes_per_realm: 4096,
        max_heap_bytes_per_realm: per_realm_heap,
        max_reserved_programs: 1,
        max_reserved_bytecode_bytes: 8192,
        max_reserved_heap_bytes: per_realm_heap.saturating_mul(2),
    };
    let mut host = SpawnedBlueJsHost::spawn_with_runtime_limits(limits)
        .expect("launcher must pass the aggregate envelope to the real child");
    let first = document(1, vec![blue_ts_classic(0, "let answer: number = 42;")]);
    assert!(matches!(
        host.synchronize_document(first).unwrap(),
        PageHostReply::Synchronized { reports, .. }
            if matches!(reports.as_slice(), [PageHostScriptReport {
                outcome: PageHostScriptOutcome::Executed,
                ..
            }])
    ));

    let mut second = document(1, vec![blue_ts_classic(0, "let other: number = 7;")]);
    second.tab_id = 42;
    assert!(matches!(
        host.synchronize_document(second.clone()).unwrap(),
        PageHostReply::Error {
            code: PageHostErrorCode::ResourceLimit,
            ..
        }
    ));
    assert!(matches!(
        host.request(PageHostRequest::GetRealmStats {
            tab_id: 41,
            document_generation: 1,
        }).unwrap(),
        PageHostReply::RealmStats(stats) if stats.program_count == 1
    ));
    assert!(matches!(
        host.request(PageHostRequest::CloseRealm {
            tab_id: 41,
            document_generation: 1,
        })
        .unwrap(),
        PageHostReply::RealmClosed { .. }
    ));
    assert!(matches!(
        host.synchronize_document(second).unwrap(),
        PageHostReply::Synchronized { .. }
    ));
    host.shutdown().unwrap();
}

#[test]
fn launcher_child_executes_external_language_graphs_rejects_missing_edges_and_invalidates_navigation(
) {
    assert!(
        std::path::Path::new(CHILD_BINARY).exists(),
        "Cargo must build the actual sibling BlueJS child host"
    );
    let mut host = SpawnedBlueJsHost::spawn()
        .expect("launcher must start and authenticate an isolated BlueJS child");
    let external_js = "https://cdn.example.test/assets/external.js";
    let missing_edge = "https://cdn.example.test/assets/missing-edge.js";
    let mut missing_edge_graph = graph(
        missing_edge,
        vec![PageHostSource::new(
            missing_edge,
            "import './not-authorized.js';",
        )],
    );
    missing_edge_graph.resolver_fingerprint = "core-no-fallback-policy-v1".to_string();
    let reply = host
        .synchronize_document(document(
            1,
            vec![
                PageHostScript {
                    ordinal: 0,
                    language: PageHostScriptLanguage::JavaScript,
                    kind: PageHostScriptKind::Classic,
                    graph: PageHostModuleGraph {
                        resolver_fingerprint: "core-external-javascript-policy-v1".to_string(),
                        ..graph(
                            external_js,
                            vec![PageHostSource::new(
                                external_js,
                                "globalThis.externalFirstGeneration = 1;",
                            )],
                        )
                    },
                },
                blue_ts_module_with_dependency(1),
                PageHostScript {
                    ordinal: 2,
                    language: PageHostScriptLanguage::JavaScript,
                    kind: PageHostScriptKind::Module,
                    graph: missing_edge_graph,
                },
            ],
        ))
        .expect("the child must execute only supplied external graphs");
    assert!(matches!(
        reply,
        PageHostReply::Synchronized { reports, .. }
            if reports.len() == 3
                && reports[0].outcome == PageHostScriptOutcome::Executed
                && reports[1].outcome == PageHostScriptOutcome::Executed
                && matches!(reports[2].outcome, PageHostScriptOutcome::Rejected { .. })
    ));

    let second = "https://cdn.example.test/assets/second.js";
    let reply = host
        .synchronize_document(document(
            2,
            vec![PageHostScript {
                ordinal: 0,
                language: PageHostScriptLanguage::JavaScript,
                kind: PageHostScriptKind::Classic,
                graph: PageHostModuleGraph {
                    resolver_fingerprint: "core-navigation-policy-two-v1".to_string(),
                    ..graph(
                        second,
                        vec![PageHostSource::new(
                            second,
                            concat!(
                                "if (typeof globalThis.externalFirstGeneration !== 'undefined') ",
                                "throw new Error('stale realm');",
                                "globalThis.externalSecondGeneration = 2;"
                            ),
                        )],
                    )
                },
            }],
        ))
        .expect("the child must replace its old realm on navigation");
    assert!(matches!(
        reply,
        PageHostReply::Synchronized { reports, .. }
            if matches!(
                reports.as_slice(),
                [report] if report.outcome == PageHostScriptOutcome::Executed
            )
    ));
    host.shutdown()
        .expect("launcher must obtain child shutdown acknowledgement");
}

#[test]
fn launcher_child_binds_only_the_core_document_snapshots_and_returns_no_value() {
    assert!(
        std::path::Path::new(CHILD_BINARY).exists(),
        "Cargo must build the actual sibling BlueJS child host"
    );
    let mut host = SpawnedBlueJsHost::spawn()
        .expect("launcher must start and authenticate an isolated BlueJS child");
    let source_id = "blueice://page/snapshot.js";
    let reply = host
        .synchronize_document(document(
            1,
            vec![PageHostScript {
                ordinal: 0,
                language: PageHostScriptLanguage::JavaScript,
                kind: PageHostScriptKind::Classic,
                graph: graph(
                    source_id,
                    vec![PageHostSource::new(
                        source_id,
                        concat!(
                            "if (blueiceDocumentText() !== 'test document snapshot') throw 'text';",
                            "if (blueiceDocumentOrigin() !== 'https://example.test') throw 'origin';",
                            "if (typeof document !== 'undefined' || typeof fetch !== 'undefined') throw 'ambient';"
                        ),
                    )],
                ),
            }],
        ))
        .expect("private host request must receive a reply");
    assert!(matches!(
        &reply,
        PageHostReply::Synchronized {
            reports,
            ..
        } if matches!(
            reports.as_slice(),
            [report] if report.outcome == PageHostScriptOutcome::Executed
        )
    ));
    assert!(
        !format!("{reply:?}").contains("test document snapshot"),
        "the source-free protocol reply must not disclose a callback result"
    );
    host.shutdown()
        .expect("launcher must obtain child shutdown acknowledgement");
}

#[test]
fn launcher_child_types_only_the_verified_snapshot_callbacks_for_bluets() {
    assert!(
        std::path::Path::new(CHILD_BINARY).exists(),
        "Cargo must build the actual sibling BlueJS child host"
    );
    let mut host = SpawnedBlueJsHost::spawn()
        .expect("launcher must start and authenticate an isolated BlueJS child");
    let verifier_id = "blueice://page/typed-snapshot-verifier.js";
    let reply = host
        .synchronize_document(document(
            1,
            vec![
                // The type annotation prevents a JavaScript parser from
                // accepting this source; execution proves direct lowering.
                blue_ts_classic(
                    0,
                    concat!(
                        "const typedSnapshot: string = blueiceDocumentText();",
                        "const typedOrigin: string = blueiceDocumentOrigin();"
                    ),
                ),
                PageHostScript {
                    ordinal: 1,
                    language: PageHostScriptLanguage::JavaScript,
                    kind: PageHostScriptKind::Classic,
                    graph: graph(
                        verifier_id,
                        vec![PageHostSource::new(
                            verifier_id,
                            concat!(
                                "if (typedSnapshot !== 'test document snapshot') throw 'text';",
                                "if (typedOrigin !== 'https://example.test') throw 'origin';"
                            ),
                        )],
                    ),
                },
                blue_ts_classic(2, "fetch('https://example.test/');"),
            ],
        ))
        .expect("private host request must receive a reply");
    assert!(matches!(
        &reply,
        PageHostReply::Synchronized { reports, .. }
            if reports.len() == 3
                && reports[0].language == PageHostScriptLanguage::BlueTs
                && reports[0].outcome == PageHostScriptOutcome::Executed
                && reports[1].language == PageHostScriptLanguage::JavaScript
                && reports[1].outcome == PageHostScriptOutcome::Executed
                && reports[2].language == PageHostScriptLanguage::BlueTs
                && matches!(reports[2].outcome, PageHostScriptOutcome::Rejected { .. })
    ));
    assert!(
        !format!("{reply:?}").contains("test document snapshot"),
        "source-free reports must not disclose typed callback results"
    );
    host.shutdown()
        .expect("launcher must obtain child shutdown acknowledgement");
}

#[test]
fn launcher_child_mints_source_free_bluets_metadata_handles_only_for_live_typed_programs() {
    assert!(
        std::path::Path::new(CHILD_BINARY).exists(),
        "Cargo must build the actual sibling BlueJS child host"
    );
    let mut host = SpawnedBlueJsHost::spawn()
        .expect("launcher must start and authenticate an isolated BlueJS child");
    let javascript_id = "blueice://page/private-metadata.js";
    let reply = host
        .synchronize_document(document(
            1,
            vec![
                PageHostScript {
                    ordinal: 0,
                    language: PageHostScriptLanguage::JavaScript,
                    kind: PageHostScriptKind::Classic,
                    graph: graph(
                        javascript_id,
                        vec![PageHostSource::new(
                            javascript_id,
                            "globalThis.javaScriptOnly = true;",
                        )],
                    ),
                },
                blue_ts_classic(1, "const processTypedAnswer: number = 42;"),
            ],
        ))
        .expect("the child must execute the authorized document");
    assert!(matches!(
        reply,
        PageHostReply::Synchronized { reports, .. }
            if reports.iter().all(|report| report.outcome == PageHostScriptOutcome::Executed)
    ));
    let programs = match host
        .request(PageHostRequest::ListDebuggerPrograms {
            tab_id: 41,
            document_generation: 1,
        })
        .expect("the child must return private program IDs")
    {
        PageHostReply::DebuggerPrograms { programs, .. } => programs,
        reply => panic!("expected private program inventory, got {reply:?}"),
    };
    assert_eq!(programs.len(), 2);

    let mut metadata_handle = None;
    for program in programs {
        let reply = host
            .request(PageHostRequest::ListDebuggerBlueTsMetadata {
                tab_id: 41,
                document_generation: 1,
                program,
            })
            .expect("the child must answer the private metadata inventory");
        let PageHostReply::DebuggerBlueTsMetadata { metadata, .. } = &reply else {
            panic!("expected private BlueTS metadata inventory, got {reply:?}");
        };
        assert!(
            !format!("{reply:?}").contains("processTypedAnswer")
                && !format!("{reply:?}").contains("private-metadata")
                && !format!("{reply:?}").contains("number"),
            "the subprocess reply must carry only opaque metadata handles"
        );
        if let [metadata] = metadata.as_slice() {
            assert_ne!(metadata.metadata_handle, program.program_handle);
            metadata_handle = Some((program, *metadata));
        } else {
            assert!(metadata.is_empty(), "the JavaScript program is ineligible");
        }
    }
    let (typed_program, metadata_handle) =
        metadata_handle.expect("the direct BlueTS program must have a live attachment");
    assert!(metadata_handle.is_well_formed());

    let replacement = host
        .synchronize_document(document(
            2,
            vec![blue_ts_classic(
                0,
                "const successorTypedAnswer: number = 43;",
            )],
        ))
        .expect("the child must replace the first realm");
    assert!(matches!(replacement, PageHostReply::Synchronized { .. }));
    assert!(matches!(
        host.request(PageHostRequest::ListDebuggerBlueTsMetadata {
            tab_id: 41,
            document_generation: 1,
            program: typed_program,
        })
        .expect("the child must reject a stale metadata request"),
        PageHostReply::Error {
            code: blueice_ipc::page_host::PageHostErrorCode::StaleDocument,
            ..
        }
    ));
    host.shutdown()
        .expect("launcher must obtain child shutdown acknowledgement");
}

#[test]
fn launcher_can_delegate_the_single_authenticated_connection_to_a_trusted_core() {
    assert!(
        std::path::Path::new(CHILD_BINARY).exists(),
        "Cargo must build the actual sibling BlueJS child host"
    );
    let (mut host, connection) = SpawnedBlueJsHost::spawn_for_core()
        .expect("launcher must supervise a child before delegating it to core");
    let private_socket = connection.socket_path().to_path_buf();
    assert!(private_socket.exists());
    assert!(
        host.request(blueice_ipc::page_host::PageHostRequest::Shutdown)
            .is_err(),
        "the supervisor must not retain a second protocol connection after hand-off"
    );

    let mut core = UnixStream::connect(connection.socket_path())
        .expect("the trusted core must reach the delegated private socket");
    blueice_ipc::page_host::write_page_host_request(
        &mut core,
        &blueice_ipc::page_host::PageHostRequest::Hello {
            protocol_version: blueice_ipc::page_host::PAGE_HOST_PROTOCOL_VERSION,
            session_token: connection.session_token().to_string(),
        },
    )
    .expect("the trusted core must send its capability handshake");
    assert!(matches!(
        blueice_ipc::page_host::read_page_host_reply(&mut core)
            .expect("the delegated child must answer the capability handshake"),
        PageHostReply::HelloAck { .. }
    ));
    let source_id = "blueice://page/delegated-classic.js";
    blueice_ipc::page_host::write_page_host_request(
        &mut core,
        &blueice_ipc::page_host::PageHostRequest::SynchronizeDocument {
            document: document(
                1,
                vec![PageHostScript {
                    ordinal: 0,
                    language: PageHostScriptLanguage::JavaScript,
                    kind: PageHostScriptKind::Classic,
                    graph: graph(
                        source_id,
                        vec![PageHostSource::new(source_id, "globalThis.answer = 42;")],
                    ),
                }],
            ),
        },
    )
    .expect("the trusted core must send an authorized document");
    assert!(matches!(
        blueice_ipc::page_host::read_page_host_reply(&mut core)
            .expect("the child must execute the delegated document"),
        PageHostReply::Synchronized { reports, .. }
            if reports.len() == 1 && reports[0].outcome == PageHostScriptOutcome::Executed
    ));
    blueice_ipc::page_host::write_page_host_request(
        &mut core,
        &blueice_ipc::page_host::PageHostRequest::Shutdown,
    )
    .expect("the trusted core must stop its delegated child");
    assert_eq!(
        blueice_ipc::page_host::read_page_host_reply(&mut core)
            .expect("the child must acknowledge delegated shutdown"),
        PageHostReply::ShutdownAck
    );
    drop(core);
    drop(host);
    assert!(
        !private_socket.exists(),
        "supervisor teardown must remove the delegated child socket"
    );
}

#[test]
fn delegated_child_is_reaped_when_core_startup_never_claims_its_connection() {
    assert!(
        std::path::Path::new(CHILD_BINARY).exists(),
        "Cargo must build the actual sibling BlueJS child host"
    );
    let (host, connection) = SpawnedBlueJsHost::spawn_for_core()
        .expect("launcher must supervise the child before core startup");
    let private_socket = connection.socket_path().to_path_buf();
    assert!(private_socket.exists());

    // This models a core spawn/early-handshake failure: no trusted core ever
    // claims the one authenticated connection. The launcher owner must still
    // reap the child and unlink its private endpoint rather than leaving an
    // orphaned capability-bearing listener behind.
    drop(host);
    assert!(
        !private_socket.exists(),
        "dropping an unclaimed delegated child must remove its private socket"
    );
}
