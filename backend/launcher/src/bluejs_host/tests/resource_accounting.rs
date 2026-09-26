// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn child_wide_reservations_reject_new_tabs_but_allow_replacement_and_release() {
    for constrained_resource in ["programs", "bytecode", "heap"] {
        let heap_per_realm = BlueJsHostRuntimeLimits::default().max_heap_bytes_per_realm;
        let mut limits = BlueJsHostRuntimeLimits {
            max_realms: 2,
            max_programs_per_realm: 2,
            max_bytecode_bytes_per_realm: 4096,
            max_heap_bytes_per_realm: heap_per_realm,
            max_reserved_programs: 4,
            max_reserved_bytecode_bytes: 8192,
            max_reserved_heap_bytes: heap_per_realm.saturating_mul(2),
        };
        match constrained_resource {
            "programs" => limits.max_reserved_programs = limits.max_programs_per_realm,
            "bytecode" => {
                limits.max_reserved_bytecode_bytes = limits.max_bytecode_bytes_per_realm;
            }
            "heap" => limits.max_reserved_heap_bytes = limits.max_heap_bytes_per_realm,
            _ => unreachable!(),
        }
        let mut host = BlueJsChildHost::with_runtime_limits(limits).unwrap();
        let first = document(1, vec![classic(0, "globalThis.answer = 42;")]);
        assert!(matches!(
            host.handle_request(PageHostRequest::SynchronizeDocument { document: first }),
            PageHostReply::Synchronized { reports, .. }
                if matches!(reports.as_slice(), [PageHostScriptReport {
                    outcome: PageHostScriptOutcome::Executed,
                    ..
                }])
        ));

        let mut second = document(1, vec![classic(0, "globalThis.other = 7;")]);
        second.tab_id = 8;
        assert!(
            matches!(
                host.handle_request(PageHostRequest::SynchronizeDocument {
                    document: second.clone()
                }),
                PageHostReply::Error {
                    code: PageHostErrorCode::ResourceLimit,
                    ..
                }
            ),
            "{constrained_resource}"
        );
        assert!(
            matches!(
                host.handle_request(PageHostRequest::SynchronizeDocument {
                    document: document(2, vec![classic(0, "globalThis.answer = 43;")])
                }),
                PageHostReply::Synchronized { reports, .. }
                    if matches!(reports.as_slice(), [PageHostScriptReport {
                        outcome: PageHostScriptOutcome::Executed,
                        ..
                    }])
            ),
            "{constrained_resource}"
        );
        assert!(matches!(
            host.handle_request(PageHostRequest::CloseRealm {
                tab_id: 7,
                document_generation: 2,
            }),
            PageHostReply::RealmClosed { .. }
        ));
        assert!(
            matches!(
                host.handle_request(PageHostRequest::SynchronizeDocument { document: second }),
                PageHostReply::Synchronized { reports, .. }
                    if matches!(reports.as_slice(), [PageHostScriptReport {
                        outcome: PageHostScriptOutcome::Executed,
                        ..
                    }])
            ),
            "{constrained_resource}"
        );
    }
}

#[test]
fn child_wide_reservation_must_cover_one_full_realm() {
    let mut limits = BlueJsHostRuntimeLimits::default();
    limits.max_reserved_programs = limits.max_programs_per_realm - 1;
    assert!(limits.runtime_config().is_err());
    limits = BlueJsHostRuntimeLimits::default();
    limits.max_reserved_bytecode_bytes = limits.max_bytecode_bytes_per_realm - 1;
    assert!(limits.runtime_config().is_err());
    limits = BlueJsHostRuntimeLimits::default();
    limits.max_reserved_heap_bytes = limits.max_heap_bytes_per_realm - 1;
    assert!(limits.runtime_config().is_err());
}

#[test]
fn child_wide_actual_usage_tracks_live_realms_not_predecessors_or_reservations() {
    let mut host = BlueJsChildHost::default();
    assert_eq!(
        host.handle_request(PageHostRequest::GetChildStats),
        PageHostReply::ChildStats(PageHostChildStats {
            realm_count: 0,
            program_count: 0,
            bytecode_bytes: 0,
            heap_bytes: 0,
        })
    );
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: document(1, vec![classic(0, "globalThis.first = 1;")]),
        }),
        PageHostReply::Synchronized { .. }
    ));
    let mut second = document(1, vec![classic(0, "globalThis.second = 2;")]);
    second.tab_id = 8;
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument { document: second }),
        PageHostReply::Synchronized { .. }
    ));
    let PageHostReply::RealmStats(first) = host.handle_request(PageHostRequest::GetRealmStats {
        tab_id: 7,
        document_generation: 1,
    }) else {
        panic!("first child realm must remain live");
    };
    let PageHostReply::RealmStats(second) = host.handle_request(PageHostRequest::GetRealmStats {
        tab_id: 8,
        document_generation: 1,
    }) else {
        panic!("second child realm must remain live");
    };
    assert_eq!(
        host.handle_request(PageHostRequest::GetChildStats),
        PageHostReply::ChildStats(PageHostChildStats {
            realm_count: 2,
            program_count: u64::from(first.program_count + second.program_count),
            bytecode_bytes: first.bytecode_bytes + second.bytecode_bytes,
            heap_bytes: first.heap_bytes + second.heap_bytes,
        })
    );
    assert!(first.bytecode_bytes > 0 && second.bytecode_bytes > 0);
    assert!(matches!(
        host.handle_request(PageHostRequest::SynchronizeDocument {
            document: document(2, Vec::new()),
        }),
        PageHostReply::Synchronized { .. }
    ));
    let PageHostReply::RealmStats(replaced) = host.handle_request(PageHostRequest::GetRealmStats {
        tab_id: 7,
        document_generation: 2,
    }) else {
        panic!("replacement child realm must be live");
    };
    assert_eq!(replaced.program_count, 0);
    assert_eq!(
        host.handle_request(PageHostRequest::GetChildStats),
        PageHostReply::ChildStats(PageHostChildStats {
            realm_count: 2,
            program_count: u64::from(second.program_count),
            bytecode_bytes: second.bytecode_bytes,
            heap_bytes: replaced.heap_bytes + second.heap_bytes,
        })
    );
    assert!(matches!(
        host.handle_request(PageHostRequest::CloseRealm {
            tab_id: 8,
            document_generation: 1,
        }),
        PageHostReply::RealmClosed { .. }
    ));
    assert_eq!(
        host.handle_request(PageHostRequest::GetChildStats),
        PageHostReply::ChildStats(PageHostChildStats {
            realm_count: 1,
            program_count: 0,
            bytecode_bytes: 0,
            heap_bytes: replaced.heap_bytes,
        })
    );
}
