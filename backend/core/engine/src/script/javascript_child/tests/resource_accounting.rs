// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn core_accepts_only_well_formed_child_wide_usage_for_its_live_realm_count() {
    let (tabs, _tab_id) = loaded_tabs(
        "<script>let accounting = 1;</script>",
        "https://example.test/aggregate-accounting.html",
    );
    let valid = PageHostChildStats {
        realm_count: 1,
        program_count: 1,
        bytecode_bytes: 64,
        heap_bytes: 128,
    };
    let mut executor = OutOfProcessJavaScriptPageExecutor::new(RecordingChild {
        child_stats_reply: Some(PageHostReply::ChildStats(valid)),
        ..RecordingChild::default()
    });
    executor.synchronize_and_execute(&tabs).unwrap();
    assert_eq!(executor.child_stats().unwrap(), valid);
    executor.child.child_stats_reply = Some(PageHostReply::ChildStats(PageHostChildStats {
        realm_count: 2,
        ..valid
    }));
    assert_eq!(
        executor.child_stats().unwrap_err().kind(),
        io::ErrorKind::InvalidData
    );
    executor.child.child_stats_reply = Some(PageHostReply::ChildStats(PageHostChildStats {
        program_count: u64::MAX,
        ..valid
    }));
    assert_eq!(
        executor.child_stats().unwrap_err().kind(),
        io::ErrorKind::InvalidData
    );
    executor.child.child_stats_reply = Some(PageHostReply::RealmStats(PageHostRealmStats {
        tab_id: 1,
        document_generation: 1,
        program_count: 1,
        bytecode_bytes: 64,
        heap_bytes: 128,
    }));
    assert_eq!(
        executor.child_stats().unwrap_err().kind(),
        io::ErrorKind::InvalidData
    );
}

#[test]
fn core_caches_child_realm_accounting_only_for_the_live_generation() {
    let (mut tabs, tab_id) = loaded_tabs(
        "<script>let accounting = 1;</script>",
        "https://example.test/accounting-first.html",
    );
    let mut executor = OutOfProcessJavaScriptPageExecutor::new(RecordingChild::default());
    executor.synchronize_and_execute(&tabs).unwrap();
    assert_eq!(
        executor.realm_stats(tab_id),
        Some(&PageHostRealmStats {
            tab_id: tab_id.as_u64(),
            document_generation: 1,
            program_count: 1,
            bytecode_bytes: 64,
            heap_bytes: 128,
        })
    );

    tabs.get_mut(tab_id).unwrap().load_html_str(
        "<script>let accounting = 2;</script>",
        Some("https://example.test/accounting-successor.html".to_string()),
    );
    executor.synchronize_and_execute(&tabs).unwrap();
    assert_eq!(
        executor
            .realm_stats(tab_id)
            .map(|stats| stats.document_generation),
        Some(2),
        "a replacement must not retain the predecessor accounting record"
    );

    assert!(tabs.close_tab(tab_id));
    executor.synchronize_and_execute(&tabs).unwrap();
    assert_eq!(executor.realm_stats(tab_id), None);
    assert_eq!(executor.into_child().closes, vec![(tab_id.as_u64(), 2)]);
}

#[derive(Clone, Copy, Default)]
enum InvalidRealmStats {
    #[default]
    MismatchedTuple,
    ExcessPrograms,
    SaturatedBytecode,
    SaturatedHeap,
}

#[derive(Default)]
struct InvalidStatsChild {
    closes: Vec<(u64, u64)>,
    invalid_stats: InvalidRealmStats,
}

impl PageHostClient for InvalidStatsChild {
    fn synchronize_document(&mut self, document: PageHostDocument) -> io::Result<PageHostReply> {
        Ok(PageHostReply::Synchronized {
            tab_id: document.tab_id,
            document_generation: document.document_generation,
            already_current: false,
            reports: Vec::new(),
        })
    }

    fn close_realm(&mut self, tab_id: u64, document_generation: u64) -> io::Result<PageHostReply> {
        self.closes.push((tab_id, document_generation));
        Ok(PageHostReply::RealmClosed {
            tab_id,
            document_generation,
        })
    }

    fn debugger_realm_stats(
        &mut self,
        tab_id: u64,
        document_generation: u64,
    ) -> io::Result<PageHostReply> {
        let stats = match self.invalid_stats {
            InvalidRealmStats::MismatchedTuple => page_host::PageHostRealmStats {
                tab_id: tab_id.saturating_add(1),
                document_generation,
                program_count: 1,
                bytecode_bytes: 64,
                heap_bytes: 128,
            },
            InvalidRealmStats::ExcessPrograms => page_host::PageHostRealmStats {
                tab_id,
                document_generation,
                program_count: page_host::PAGE_HOST_REALM_STATS_MAX_PROGRAMS + 1,
                bytecode_bytes: 64,
                heap_bytes: 128,
            },
            InvalidRealmStats::SaturatedBytecode => page_host::PageHostRealmStats {
                tab_id,
                document_generation,
                program_count: 1,
                bytecode_bytes: u64::MAX,
                heap_bytes: 128,
            },
            InvalidRealmStats::SaturatedHeap => page_host::PageHostRealmStats {
                tab_id,
                document_generation,
                program_count: 1,
                bytecode_bytes: 64,
                heap_bytes: u64::MAX,
            },
        };
        Ok(PageHostReply::RealmStats(stats))
    }
}

#[test]
fn malformed_child_realm_accounting_is_never_cached() {
    let (tabs, tab_id) = loaded_tabs(
        "<script>let untrustedAccounting = 1;</script>",
        "https://example.test/mismatched-accounting.html",
    );
    for invalid_stats in [
        InvalidRealmStats::MismatchedTuple,
        InvalidRealmStats::ExcessPrograms,
        InvalidRealmStats::SaturatedBytecode,
        InvalidRealmStats::SaturatedHeap,
    ] {
        let mut executor = OutOfProcessJavaScriptPageExecutor::new(InvalidStatsChild {
            invalid_stats,
            ..InvalidStatsChild::default()
        });
        executor.synchronize_and_execute(&tabs).unwrap();

        assert_eq!(executor.realm_stats(tab_id), None);
        assert!(
            !executor.debugger_has_live_realm(tab_id, 1),
            "malformed accounting must not be accepted as debugger liveness"
        );
        assert_eq!(
            executor.into_child().closes,
            vec![(tab_id.as_u64(), 1)],
            "an untrustworthy accounting record must close the newly acknowledged realm"
        );
    }
}

#[derive(Default)]
struct WrongSuccessorAckChild {
    active_generation: Option<u64>,
    closes: Vec<(u64, u64)>,
    return_error: bool,
}

impl PageHostClient for WrongSuccessorAckChild {
    fn synchronize_document(&mut self, document: PageHostDocument) -> io::Result<PageHostReply> {
        self.active_generation = Some(document.document_generation);
        if document.document_generation == 2 && self.return_error {
            return Ok(PageHostReply::Error {
                code: PageHostErrorCode::HostFailure,
                message: "child failed after admission".to_string(),
            });
        }
        Ok(PageHostReply::Synchronized {
            tab_id: document.tab_id + u64::from(document.document_generation == 2),
            document_generation: document.document_generation,
            already_current: false,
            reports: Vec::new(),
        })
    }

    fn close_realm(&mut self, tab_id: u64, document_generation: u64) -> io::Result<PageHostReply> {
        self.closes.push((tab_id, document_generation));
        if self.active_generation == Some(document_generation) {
            self.active_generation = None;
            Ok(PageHostReply::RealmClosed {
                tab_id,
                document_generation,
            })
        } else {
            Ok(PageHostReply::Error {
                code: PageHostErrorCode::StaleDocument,
                message: "stale document".to_string(),
            })
        }
    }

    fn debugger_realm_stats(
        &mut self,
        tab_id: u64,
        document_generation: u64,
    ) -> io::Result<PageHostReply> {
        if self.active_generation != Some(document_generation) {
            return Ok(PageHostReply::Error {
                code: PageHostErrorCode::StaleDocument,
                message: "stale document".to_string(),
            });
        }
        Ok(PageHostReply::RealmStats(PageHostRealmStats {
            tab_id,
            document_generation,
            program_count: 1,
            bytecode_bytes: 64,
            heap_bytes: 128,
        }))
    }

    fn child_stats(&mut self) -> io::Result<PageHostReply> {
        let has_realm = self.active_generation.is_some();
        Ok(PageHostReply::ChildStats(PageHostChildStats {
            realm_count: u32::from(has_realm),
            program_count: u64::from(has_realm),
            bytecode_bytes: if has_realm { 64 } else { 0 },
            heap_bytes: if has_realm { 128 } else { 0 },
        }))
    }
}

#[test]
fn untrusted_successor_ack_closes_the_generation_the_child_may_have_admitted() {
    for return_error in [false, true] {
        let (mut tabs, tab_id) = loaded_tabs(
            "<script>let previous = 1;</script>",
            "https://example.test/previous.html",
        );
        let mut executor = OutOfProcessJavaScriptPageExecutor::new(WrongSuccessorAckChild {
            return_error,
            ..WrongSuccessorAckChild::default()
        });
        executor.synchronize_and_execute(&tabs).unwrap();
        assert_eq!(executor.child.active_generation, Some(1));
        assert_eq!(executor.child_stats().unwrap().realm_count, 1);

        tabs.get_mut(tab_id).unwrap().load_html_str(
            "<script>let successor = 2;</script>",
            Some("https://example.test/successor.html".to_string()),
        );
        executor.synchronize_and_execute(&tabs).unwrap();
        assert_eq!(executor.child_stats().unwrap().realm_count, 0);
        let child = executor.into_child();
        assert_eq!(child.active_generation, None);
        assert_eq!(
            child.closes,
            vec![(tab_id.as_u64(), 1), (tab_id.as_u64(), 2)]
        );
    }
}
