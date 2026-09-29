// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Real-child tab and aggregate resource attribution for BlueTS page realms.

use super::*;

#[test]
fn supervised_child_attributes_two_live_bluets_tabs_and_releases_each_charge() {
    let per_realm_heap = BlueJsHostRuntimeLimits::default().max_heap_bytes_per_realm;
    let limits = BlueJsHostRuntimeLimits {
        max_realms: 2,
        max_programs_per_realm: 1,
        max_bytecode_bytes_per_realm: 4096,
        max_heap_bytes_per_realm: per_realm_heap,
        max_reserved_programs: 2,
        max_reserved_bytecode_bytes: 8192,
        max_reserved_heap_bytes: per_realm_heap.saturating_mul(2),
    };
    let mut host = SpawnedBlueJsHost::spawn_with_runtime_limits(limits).unwrap();
    let first = document(1, vec![blue_ts_classic(0, "const first: number = 41;")]);
    assert!(matches!(
        host.synchronize_document(first).unwrap(),
        PageHostReply::Synchronized { reports, .. }
            if matches!(reports.as_slice(), [PageHostScriptReport {
                outcome: PageHostScriptOutcome::Executed,
                ..
            }])
    ));
    let mut second = document(1, vec![blue_ts_classic(0, "const second: number = 42;")]);
    second.tab_id = 42;
    assert!(matches!(
        host.synchronize_document(second).unwrap(),
        PageHostReply::Synchronized { reports, .. }
            if matches!(reports.as_slice(), [PageHostScriptReport {
                outcome: PageHostScriptOutcome::Executed,
                ..
            }])
    ));

    let realm_stats = |host: &mut SpawnedBlueJsHost, tab_id| {
        let PageHostReply::RealmStats(stats) = host
            .request(PageHostRequest::GetRealmStats {
                tab_id,
                document_generation: 1,
            })
            .unwrap()
        else {
            panic!("each live tab must have its own child realm charge")
        };
        assert_eq!(stats.tab_id, tab_id);
        assert_eq!(stats.document_generation, 1);
        assert_eq!(stats.program_count, 1);
        assert!(stats.bytecode_bytes > 0 && stats.heap_bytes > 0);
        stats
    };
    let first_stats = realm_stats(&mut host, 41);
    let second_stats = realm_stats(&mut host, 42);
    let PageHostReply::ChildStats(total) = host.request(PageHostRequest::GetChildStats).unwrap()
    else {
        panic!("the supervised child must return aggregate actual usage")
    };
    assert_eq!(total.realm_count, 2);
    assert_eq!(total.program_count, 2);
    assert_eq!(
        total.bytecode_bytes,
        first_stats.bytecode_bytes + second_stats.bytecode_bytes
    );
    assert_eq!(
        total.heap_bytes,
        first_stats.heap_bytes + second_stats.heap_bytes
    );

    assert!(matches!(
        host.request(PageHostRequest::CloseRealm {
            tab_id: 41,
            document_generation: 1,
        })
        .unwrap(),
        PageHostReply::RealmClosed { tab_id: 41, .. }
    ));
    assert!(matches!(
        host.request(PageHostRequest::GetRealmStats {
            tab_id: 41,
            document_generation: 1,
        })
        .unwrap(),
        PageHostReply::Error { .. }
    ));
    assert_eq!(realm_stats(&mut host, 42), second_stats);
    let PageHostReply::ChildStats(remaining) =
        host.request(PageHostRequest::GetChildStats).unwrap()
    else {
        panic!("the child must retain only the second tab's charge")
    };
    assert_eq!(remaining.realm_count, 1);
    assert_eq!(remaining.program_count, 1);
    assert_eq!(remaining.bytecode_bytes, second_stats.bytecode_bytes);
    assert_eq!(remaining.heap_bytes, second_stats.heap_bytes);

    assert!(matches!(
        host.request(PageHostRequest::CloseRealm {
            tab_id: 42,
            document_generation: 1,
        })
        .unwrap(),
        PageHostReply::RealmClosed { tab_id: 42, .. }
    ));
    assert!(matches!(
        host.request(PageHostRequest::GetChildStats).unwrap(),
        PageHostReply::ChildStats(stats)
            if stats.realm_count == 0
                && stats.program_count == 0
                && stats.bytecode_bytes == 0
                && stats.heap_bytes == 0
    ));
    host.shutdown().unwrap();
}
