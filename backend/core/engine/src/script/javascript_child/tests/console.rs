// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;
fn batch(tab: TabId) -> blueice_ipc::console::ConsoleMessages {
    blueice_ipc::console::ConsoleMessages {
        tab_id: tab.as_u64(),
        document_generation: 1,
        dropped_records: 0,
        records: vec![blueice_ipc::console::ConsoleRecord {
            sequence: 1,
            timestamp_ms: 0,
            method: "log".into(),
            level: "log".into(),
            group_depth: 0,
            message: "page".into(),
            truncated: false,
            source: Some(blueice_ipc::console::ConsoleSourceLocation {
                filename: "page:///console.js".into(),
                line: 7,
                column: 11,
            }),
        }],
    }
}
#[test]
fn console_read_uses_the_exact_live_document_and_refreshes_retained_payload() {
    let (tabs, tab) = loaded_tabs("<p>console</p>", "https://example.test/console");
    let mut executor = OutOfProcessJavaScriptPageExecutor::new(RecordingChild::default());
    assert!(executor.drain_console(&tabs, tab).is_err());
    assert!(executor.child.console_requests.is_empty());
    executor.synchronize_and_execute(&tabs).unwrap();
    executor.child.console_reply = Some(PageHostReply::ConsoleMessages {
        messages: batch(tab),
        scheduling_payload_bytes: 32,
    });
    assert_eq!(executor.drain_console(&tabs, tab).unwrap(), batch(tab));
    assert_eq!(executor.child.console_requests, [(tab.as_u64(), 1)]);
    assert_eq!(executor.realm_stats[&tab].scheduling_payload_bytes, 32);
    assert_eq!(executor.child.documents.len(), 1);
}
#[test]
fn malformed_console_replies_revoke_only_the_addressed_realm_without_replaying_scripts() {
    for failure in 0..10 {
        let (mut tabs, tab) = loaded_tabs("<p>console</p>", "https://example.test/console");
        let other = tabs.open_tab();
        tabs.get_mut(other).unwrap().load_html_str(
            "<p>healthy</p>",
            Some("https://example.test/healthy".into()),
        );
        let mut executor = OutOfProcessJavaScriptPageExecutor::new(RecordingChild::default());
        executor.synchronize_and_execute(&tabs).unwrap();
        let mut messages = batch(tab);
        let mut bytes = 0;
        match failure {
            0 => messages.tab_id += 1,
            1 => messages.document_generation += 1,
            2 => messages.records[0].message = "x".repeat(8193),
            3 => bytes = u64::MAX,
            4 => messages.records[0].level = "forged".into(),
            5 => messages.records[0].source.as_mut().unwrap().line = 0,
            6 => messages.records[0].source.as_mut().unwrap().column = 0,
            7 => messages.records[0].source.as_mut().unwrap().filename = "x".repeat(8193),
            8 => messages.records[0]
                .source
                .as_mut()
                .unwrap()
                .filename
                .clear(),
            _ => {
                let mut record = messages.records.pop().unwrap();
                record.message = "m".repeat(4096);
                record.source.as_mut().unwrap().filename = "f".repeat(4096);
                messages.records = (1..=33)
                    .map(|sequence| {
                        let mut record = record.clone();
                        record.sequence = sequence;
                        record
                    })
                    .collect();
            }
        }
        executor.child.console_reply = Some(PageHostReply::ConsoleMessages {
            messages,
            scheduling_payload_bytes: bytes,
        });
        assert!(executor.drain_console(&tabs, tab).is_err());
        assert_eq!(executor.child.closes, [(tab.as_u64(), 1)]);
        assert!(!executor.realm_stats.contains_key(&tab));
        assert!(executor.realm_stats.contains_key(&other));
        executor.synchronize_and_execute(&tabs).unwrap();
        assert_eq!(executor.child.documents.len(), 2);
    }
}
