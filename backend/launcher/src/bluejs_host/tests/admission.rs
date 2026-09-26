// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn executes_authorized_classic_and_rejects_a_later_parse_failure_independently() {
    let mut host = BlueJsChildHost::default();
    let reply = host.handle_request(PageHostRequest::SynchronizeDocument {
        document: document(
            1,
            vec![
                classic(0, "globalThis.answer = 42;"),
                classic(1, "const = ;"),
            ],
        ),
    });
    let PageHostReply::Synchronized { reports, .. } = reply else {
        panic!("expected synchronized reply");
    };
    assert_eq!(reports.len(), 2);
    assert_eq!(reports[0].outcome, PageHostScriptOutcome::Executed);
    assert!(matches!(
        reports[1].outcome,
        PageHostScriptOutcome::Rejected { .. }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::GetRealmStats {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::RealmStats(PageHostRealmStats {
            program_count: 1,
            ..
        })
    ));
}

#[test]
fn direct_bluets_and_javascript_execute_in_document_order_in_one_realm() {
    let mut host = BlueJsChildHost::default();
    let reply = host.handle_request(PageHostRequest::SynchronizeDocument {
        document: document(
            1,
            vec![
                classic(0, "globalThis.beforeBlueTs = true;"),
                // The TypeScript annotation makes this invalid JavaScript
                // source. Success therefore proves the child used direct
                // BlueTS lowering rather than emitted-JavaScript reparse.
                blue_ts_classic(1, "const sharedAnswer: number = 42;"),
                classic(
                    2,
                    "if (!globalThis.beforeBlueTs || sharedAnswer !== 42) throw 'realm/order failed';",
                ),
            ],
        ),
    });
    let PageHostReply::Synchronized { reports, .. } = reply else {
        panic!("expected synchronized reply");
    };
    assert_eq!(
        reports
            .iter()
            .map(|report| (report.ordinal, report.language, &report.outcome))
            .collect::<Vec<_>>(),
        vec![
            (
                0,
                PageHostScriptLanguage::JavaScript,
                &PageHostScriptOutcome::Executed
            ),
            (
                1,
                PageHostScriptLanguage::BlueTs,
                &PageHostScriptOutcome::Executed
            ),
            (
                2,
                PageHostScriptLanguage::JavaScript,
                &PageHostScriptOutcome::Executed
            ),
        ]
    );
    assert!(matches!(
        host.handle_request(PageHostRequest::GetRealmStats {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::RealmStats(PageHostRealmStats {
            program_count: 3,
            ..
        })
    ));
}

#[test]
fn child_installs_only_fixed_core_snapshot_callbacks_for_javascript() {
    let mut host = BlueJsChildHost::default();
    let reply = host.handle_request(PageHostRequest::SynchronizeDocument {
        document: document_with_snapshot(
            1,
            "private document snapshot".to_string(),
            "https://example.test".to_string(),
            vec![
                classic(
                    0,
                    concat!(
                        "if (blueiceDocumentText() !== 'private document snapshot') throw 'text';",
                        "if (blueiceDocumentOrigin() !== 'https://example.test') throw 'origin';",
                        "if (typeof document !== 'undefined' || typeof fetch !== 'undefined' || typeof blueiceTestHasElementById !== 'undefined' || typeof blueiceTestGetElementById !== 'undefined') throw 'ambient';"
                    ),
                ),
                classic(1, "blueiceDocumentText(1);"),
            ],
        ),
    });
    let PageHostReply::Synchronized { reports, .. } = reply else {
        panic!("expected synchronized reply");
    };
    assert_eq!(reports.len(), 2);
    assert_eq!(reports[0].outcome, PageHostScriptOutcome::Executed);
    assert!(matches!(
        reports[1].outcome,
        PageHostScriptOutcome::Rejected { .. }
    ));
    // Result records remain source/value-free even when a callback
    // returned a private snapshot inside the child VM.
    assert!(!format!("{:?}", reports).contains("private document snapshot"));
}

#[test]
fn snapshot_validation_happens_before_realm_creation_or_replacement() {
    let mut host = BlueJsChildHost::default();
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: document_with_snapshot(
                1,
                "snapshot".to_string(),
                "HTTP://EXAMPLE.test".to_string(),
                vec![classic(0, "globalThis.answer = 42;")],
            ),
        }),
        PageHostReply::Error {
            code: PageHostErrorCode::InvalidRequest,
            ..
        }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::GetRealmStats {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::Error {
            code: PageHostErrorCode::UnknownRealm,
            ..
        }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: document_with_snapshot(
                1,
                "x".repeat(PAGE_HOST_DOCUMENT_TEXT_MAX_BYTES + 1),
                "https://example.test".to_string(),
                vec![classic(0, "globalThis.answer = 42;")],
            ),
        }),
        PageHostReply::Error {
            code: PageHostErrorCode::ResourceLimit,
            ..
        }
    ));
}

#[test]
fn bluets_uses_the_verified_snapshot_profile_and_rejects_untyped_globals() {
    let mut host = BlueJsChildHost::default();
    let reply = host.handle_request(PageHostRequest::SynchronizeDocument {
        document: document(
            1,
            vec![
                // The annotation is invalid JavaScript syntax, so this
                // success also preserves direct BlueTS-to-BlueJS lowering.
                blue_ts_classic(
                    0,
                    concat!(
                        "const snapshotText: string = blueiceDocumentText();",
                        "const snapshotOrigin: string = blueiceDocumentOrigin();"
                    ),
                ),
                classic(
                    1,
                    concat!(
                        "if (snapshotText !== 'test document snapshot') throw 'text';",
                        "if (snapshotOrigin !== 'https://example.test') throw 'origin';"
                    ),
                ),
                // The fixed declaration contains neither fetch nor a
                // general document object. All three compile attempts fail
                // before a program is admitted to the child realm.
                blue_ts_classic(2, "fetch('https://example.test/');"),
                blue_ts_classic(3, "blueiceDocumentText(1);"),
                blue_ts_classic(4, "document.getElementById('target');"),
            ],
        ),
    });
    assert!(matches!(
        reply,
        PageHostReply::Synchronized {
            reports,
            ..
        } if reports.len() == 5
            && reports[0].language == PageHostScriptLanguage::BlueTs
            && reports[0].outcome == PageHostScriptOutcome::Executed
            && reports[1].language == PageHostScriptLanguage::JavaScript
            && reports[1].outcome == PageHostScriptOutcome::Executed
            && matches!(reports[2].outcome, PageHostScriptOutcome::Rejected { .. })
            && matches!(reports[3].outcome, PageHostScriptOutcome::Rejected { .. })
            && matches!(reports[4].outcome, PageHostScriptOutcome::Rejected { .. })
    ));
}

#[test]
fn module_graph_uses_only_the_explicit_static_resolution_records() {
    let entry = "blueice://page/entry.js";
    let dependency = "blueice://page/dependency.js";
    let mut module = PageHostScript {
        ordinal: 0,
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
    let mut host = BlueJsChildHost::default();
    let reply = host.handle_request(PageHostRequest::SynchronizeDocument {
        document: document(1, vec![module]),
    });
    assert!(matches!(
        reply,
        PageHostReply::Synchronized {
            reports,
            ..
        } if reports[0].outcome == PageHostScriptOutcome::Executed
    ));
}

#[test]
fn direct_bluets_module_graph_uses_only_the_explicit_static_resolution_records() {
    let entry = "blueice://page/entry.ts";
    let dependency = "blueice://page/dependency.ts";
    let mut module = PageHostScript {
        ordinal: 0,
        language: PageHostScriptLanguage::BlueTs,
        kind: PageHostScriptKind::Module,
        graph: graph(
            entry,
            vec![
                PageHostSource::new(
                    entry,
                    "import { answer } from './dependency.ts'; export const result: number = answer;",
                ),
                PageHostSource::new(dependency, "export const answer: number = 42;"),
            ],
        ),
    };
    module.graph.resolutions.push(PageHostStaticResolution {
        from_module: entry.to_string(),
        specifier: "./dependency.ts".to_string(),
        canonical_target: dependency.to_string(),
    });
    let mut host = BlueJsChildHost::default();
    let reply = host.handle_request(PageHostRequest::SynchronizeDocument {
        document: document(1, vec![module]),
    });
    assert!(matches!(
        reply,
        PageHostReply::Synchronized {
            reports,
            ..
        } if matches!(
            reports.as_slice(),
            [PageHostScriptReport {
                language: PageHostScriptLanguage::BlueTs,
                kind: PageHostScriptKind::Module,
                outcome: PageHostScriptOutcome::Executed,
                ..
            }]
        )
    ));
    assert_eq!(host.debug_registry.len(), 2);
    let mut retained_modules: Vec<_> = host
        .documents
        .get(&7)
        .expect("the child realm remains live")
        .debugger_programs
        .values()
        .filter_map(|record| {
            host.debug_registry
                .get(host.runtime.program_registry(), record.runtime_handle)
                .ok()
        })
        .map(|metadata| {
            let sources = &metadata.static_info().sources;
            sources
                .iter()
                .find(|source| source.module == entry || source.module == dependency)
                .expect("each module keeps its own static source metadata")
                .module
                .clone()
        })
        .collect();
    retained_modules.sort();
    assert_eq!(
        retained_modules,
        vec![dependency.to_string(), entry.to_string()]
    );
}

#[test]
fn a_missing_module_resolution_admits_no_partial_graph_programs() {
    let entry = "blueice://page/entry.js";
    let dependency = "blueice://page/dependency.js";
    let script = PageHostScript {
        ordinal: 0,
        language: PageHostScriptLanguage::JavaScript,
        kind: PageHostScriptKind::Module,
        graph: graph(
            entry,
            vec![
                PageHostSource::new(entry, "import './dependency.js';"),
                PageHostSource::new(dependency, "export const answer = 42;"),
            ],
        ),
    };
    let mut host = BlueJsChildHost::default();
    let reply = host.handle_request(PageHostRequest::SynchronizeDocument {
        document: document(1, vec![script]),
    });
    assert!(matches!(
        reply,
        PageHostReply::Synchronized {
            reports,
            ..
        } if matches!(reports[0].outcome, PageHostScriptOutcome::Rejected { .. })
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::GetRealmStats {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::RealmStats(PageHostRealmStats {
            program_count: 0,
            bytecode_bytes: 0,
            ..
        })
    ));
}

#[test]
fn document_generations_are_idempotent_and_cannot_close_a_successor() {
    let mut host = BlueJsChildHost::default();
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: document(1, vec![classic(0, "globalThis.first = true;")]),
        }),
        PageHostReply::Synchronized {
            already_current: false,
            ..
        }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: document(1, vec![classic(0, "throw new Error('must not replay');")]),
        }),
        PageHostReply::Synchronized {
            already_current: true,
            reports,
            ..
        } if reports.is_empty()
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: document(2, vec![classic(0, "globalThis.second = true;")]),
        }),
        PageHostReply::Synchronized {
            already_current: false,
            ..
        }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::CloseRealm {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::Error {
            code: PageHostErrorCode::StaleDocument,
            ..
        }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::GetRealmStats {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::Error {
            code: PageHostErrorCode::StaleDocument,
            ..
        }
    ));
}

#[test]
fn source_hash_tampering_is_rejected_before_realm_program_admission() {
    let mut source = PageHostSource::new("blueice://page/main.js", "globalThis.answer = 42;");
    source.source_hash = "fnv1a64:0000000000000000".to_string();
    let script = PageHostScript {
        ordinal: 0,
        language: PageHostScriptLanguage::JavaScript,
        kind: PageHostScriptKind::Classic,
        graph: graph("blueice://page/main.js", vec![source]),
    };
    let mut host = BlueJsChildHost::default();
    let reply = host.handle_request(PageHostRequest::SynchronizeDocument {
        document: document(1, vec![script]),
    });
    assert!(matches!(
        reply,
        PageHostReply::Synchronized {
            reports,
            ..
        } if matches!(reports[0].outcome, PageHostScriptOutcome::Rejected { .. })
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::GetRealmStats {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::RealmStats(PageHostRealmStats {
            program_count: 0,
            ..
        })
    ));
}

#[test]
fn document_source_budget_rejects_before_realm_replacement() {
    let source = PageHostSource::new(
        "blueice://page/too-large.js",
        "x".repeat(MAX_SOURCE_BYTES_PER_DOCUMENT + 1),
    );
    let script = PageHostScript {
        ordinal: 0,
        language: PageHostScriptLanguage::JavaScript,
        kind: PageHostScriptKind::Classic,
        graph: graph("blueice://page/too-large.js", vec![source]),
    };
    let mut host = BlueJsChildHost::default();
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: document(1, vec![script]),
        }),
        PageHostReply::Error {
            code: PageHostErrorCode::ResourceLimit,
            ..
        }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::GetRealmStats {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::Error {
            code: PageHostErrorCode::UnknownRealm,
            ..
        }
    ));
}

#[test]
fn duplicate_script_ordinals_are_rejected_before_realm_replacement() {
    let mut host = BlueJsChildHost::default();
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: document(
                1,
                vec![
                    classic(0, "globalThis.first = true;"),
                    classic(0, "globalThis.second = true;"),
                ],
            ),
        }),
        PageHostReply::Error {
            code: PageHostErrorCode::InvalidRequest,
            ..
        }
    ));
    assert!(matches!(
        host.handle_request(PageHostRequest::GetRealmStats {
            tab_id: 7,
            document_generation: 1,
        }),
        PageHostReply::Error {
            code: PageHostErrorCode::UnknownRealm,
            ..
        }
    ));
}

#[test]
fn private_socket_is_owner_only() {
    let path = PathBuf::from("/tmp").join(format!(
        "blueice-launcher-owner-only-test-{}.sock",
        std::process::id()
    ));
    let listener = bind_bluejs_host_socket(&path).unwrap();
    let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
    drop(listener);
    let _ = fs::remove_file(path);
}
