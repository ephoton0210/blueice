// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn core_charges_snapshot_validation_to_the_initiating_tab_even_on_rejection() {
    let (mut tabs, first_tab) = loaded_tabs(
        "<main>hello</main><script>blueiceDocumentText();</script>",
        "https://example.test/first.html",
    );
    let second_tab = tabs.open_tab();
    let oversized = "x".repeat(page_host::PAGE_HOST_DOCUMENT_TEXT_MAX_BYTES + 1);
    tabs.get_mut(second_tab).unwrap().load_html_str(
        &format!("<main>{oversized}</main><script>blueiceDocumentText();</script>"),
        Some("https://example.test/second.html".to_string()),
    );
    let first_snapshot_bytes = tabs
        .get(first_tab)
        .unwrap()
        .script_document_text_content()
        .len() as u64;
    let second_snapshot_bytes = tabs
        .get(second_tab)
        .unwrap()
        .script_document_text_content()
        .len() as u64;
    let mut executor = OutOfProcessJavaScriptPageExecutor::new(RecordingChild::default());
    executor.synchronize_and_execute(&tabs).unwrap();

    let first = executor.validation_usage(first_tab).unwrap();
    assert_eq!(first.document_generation, 1);
    assert_eq!(first.attempts, 2);
    assert_eq!(first.visited_nodes, 2);
    assert_eq!(
        first.copied_value_bytes,
        first_snapshot_bytes + "https://example.test".len() as u64
    );
    let second = executor.validation_usage(second_tab).unwrap();
    assert_eq!(second.document_generation, 1);
    assert_eq!(second.attempts, 1);
    assert_eq!(second.visited_nodes, 1);
    assert_eq!(second.copied_value_bytes, second_snapshot_bytes);
    assert_eq!(executor.child.documents.len(), 1);
    assert_eq!(executor.child.documents[0].tab_id, first_tab.as_u64());
    assert!(matches!(
        executor.drain_reports_for_tab(second_tab).as_slice(),
        [JavaScriptPageExecutionReport::Rejected {
            category: "host binding contract rejected the page script",
            ..
        }]
    ));

    tabs.get_mut(first_tab).unwrap().load_html_str(
        "<main>next</main><script>blueiceDocumentText();</script>",
        Some("https://example.test/next.html".to_string()),
    );
    let replacement_snapshot_bytes = tabs
        .get(first_tab)
        .unwrap()
        .script_document_text_content()
        .len() as u64;
    executor.synchronize_and_execute(&tabs).unwrap();
    let replacement = executor.validation_usage(first_tab).unwrap();
    assert_eq!(replacement.document_generation, 2);
    assert_eq!(replacement.attempts, 2);
    assert_eq!(
        replacement.copied_value_bytes,
        replacement_snapshot_bytes + "https://example.test".len() as u64
    );
    assert_eq!(executor.validation_usage(second_tab), Some(second));

    assert!(tabs.close_tab(second_tab));
    executor.synchronize_and_execute(&tabs).unwrap();
    assert_eq!(executor.validation_usage(second_tab), None);
    assert_eq!(executor.validation_usage(first_tab), Some(replacement));
}

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
        static_metadata_bytes: 0,
        deferred_payload_bytes: 0,
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
            static_metadata_bytes: 32,
            deferred_payload_bytes: 64,
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

#[test]
fn real_child_and_core_refresh_two_tab_retained_totals_after_execution_and_lifecycle() {
    use blueice_launcher::bluejs_host::SpawnedBlueJsHost;

    let (host, config) = SpawnedBlueJsHost::spawn_for_core().unwrap();
    let first_source = "<script type=\"application/x-blueice-typescript\">const firstValue: number = 1; firstValue;</script>";
    let second_source = "<script type=\"application/x-blueice-typescript\">const secondLongerValue: number = 2; secondLongerValue;</script>";
    let (mut tabs, first_tab) =
        loaded_tabs(first_source, "https://example.test/first-retained.html");
    let second_tab = tabs.open_tab();
    tabs.get_mut(second_tab).unwrap().load_html_str(
        second_source,
        Some("https://example.test/second-retained.html".to_string()),
    );
    let mut executor = OutOfProcessJavaScriptPageExecutor::connect_with_debugger_execution_control(
        config.socket_path(),
        config.session_token(),
    )
    .unwrap();
    executor.synchronize_and_execute(&tabs).unwrap();
    let first_pending = executor.realm_stats(first_tab).unwrap().clone();
    let second_pending = executor.realm_stats(second_tab).unwrap().clone();
    assert_eq!(first_pending.document_generation, 1);
    assert_eq!(second_pending.document_generation, 1);
    assert!(first_pending.static_metadata_bytes > 0 && first_pending.deferred_payload_bytes > 0);
    assert!(second_pending.static_metadata_bytes > 0 && second_pending.deferred_payload_bytes > 0);
    let first_identity = executor
        .retained_debugger_identity_map_bytes(first_tab, 1)
        .unwrap();
    let second_identity = executor
        .retained_debugger_identity_map_bytes(second_tab, 1)
        .unwrap();
    assert!(first_identity > 0 && second_identity > 0);
    assert_eq!(
        executor.retained_page_cache_payload_bytes(first_tab, 1),
        Some(
            usize::try_from(first_pending.static_metadata_bytes).unwrap()
                + usize::try_from(first_pending.deferred_payload_bytes).unwrap()
                + first_identity
        )
    );
    assert_eq!(
        executor.retained_page_cache_payload_bytes(second_tab, 1),
        Some(
            usize::try_from(second_pending.static_metadata_bytes).unwrap()
                + usize::try_from(second_pending.deferred_payload_bytes).unwrap()
                + second_identity
        )
    );
    let program = executor.debugger_programs(first_tab, 1).unwrap()[0];
    let after_program_discovery = executor
        .retained_debugger_identity_map_bytes(first_tab, 1)
        .unwrap();
    assert!(after_program_discovery > first_identity);
    assert!(!executor
        .debugger_static_metadata(
            first_tab,
            1,
            program.program_handle,
            program.program_generation,
        )
        .unwrap()
        .is_empty());
    let after_metadata_discovery = executor
        .retained_debugger_identity_map_bytes(first_tab, 1)
        .unwrap();
    assert!(after_metadata_discovery > after_program_discovery);
    assert_eq!(
        executor.retained_page_cache_payload_bytes(first_tab, 1),
        Some(
            usize::try_from(first_pending.static_metadata_bytes).unwrap()
                + usize::try_from(first_pending.deferred_payload_bytes).unwrap()
                + after_metadata_discovery
        )
    );
    assert_eq!(
        executor.retained_debugger_identity_map_bytes(second_tab, 1),
        Some(second_identity)
    );

    executor.synchronize_and_execute(&tabs).unwrap();
    let first_executed = executor.realm_stats(first_tab).unwrap().clone();
    let second_executed = executor.realm_stats(second_tab).unwrap().clone();
    assert_eq!(
        first_executed.static_metadata_bytes,
        first_pending.static_metadata_bytes
    );
    assert_eq!(
        second_executed.static_metadata_bytes,
        second_pending.static_metadata_bytes
    );
    assert_eq!(first_executed.deferred_payload_bytes, 0);
    assert_eq!(second_executed.deferred_payload_bytes, 0);
    assert_eq!(
        executor.retained_page_cache_payload_bytes(first_tab, 1),
        Some(
            usize::try_from(first_executed.static_metadata_bytes).unwrap()
                + executor
                    .retained_debugger_payload_bytes(first_tab, 1)
                    .unwrap()
        )
    );

    tabs.get_mut(first_tab).unwrap().load_html_str(
        "<script type=\"application/x-blueice-typescript\">const replacement: number = 3;</script>",
        Some("https://example.test/replacement-retained.html".to_string()),
    );
    executor.synchronize_and_execute(&tabs).unwrap();
    let replacement = executor.realm_stats(first_tab).unwrap();
    assert_eq!(replacement.document_generation, 2);
    assert!(replacement.static_metadata_bytes > 0 && replacement.deferred_payload_bytes > 0);
    assert_eq!(executor.realm_stats(second_tab), Some(&second_executed));
    assert_eq!(
        executor.retained_debugger_identity_map_bytes(first_tab, 1),
        None
    );
    assert_eq!(
        executor.retained_page_cache_payload_bytes(first_tab, 1),
        None
    );
    let replacement_identity = executor
        .retained_debugger_identity_map_bytes(first_tab, 2)
        .unwrap();
    assert!(replacement_identity > 0 && replacement_identity < after_metadata_discovery);
    assert_eq!(
        executor.retained_page_cache_payload_bytes(first_tab, 2),
        Some(
            usize::try_from(replacement.static_metadata_bytes).unwrap()
                + usize::try_from(replacement.deferred_payload_bytes).unwrap()
                + replacement_identity
        )
    );
    assert_eq!(
        executor.retained_debugger_identity_map_bytes(second_tab, 1),
        Some(second_identity)
    );
    assert!(matches!(
        executor.child.debugger_realm_stats(first_tab.as_u64(), 1),
        Ok(PageHostReply::Error {
            code: PageHostErrorCode::StaleDocument,
            ..
        })
    ));

    assert!(tabs.close_tab(second_tab));
    executor.synchronize_and_execute(&tabs).unwrap();
    assert_eq!(executor.realm_stats(second_tab), None);
    assert_eq!(
        executor.retained_debugger_identity_map_bytes(second_tab, 1),
        None
    );
    assert_eq!(
        executor.retained_page_cache_payload_bytes(second_tab, 1),
        None
    );
    assert_eq!(
        executor
            .realm_stats(first_tab)
            .unwrap()
            .deferred_payload_bytes,
        0
    );
    let oversized = "x".repeat(page_host::PAGE_HOST_DOCUMENT_TEXT_MAX_BYTES + 1);
    tabs.get_mut(first_tab).unwrap().load_html_str(
        &format!("<main>{oversized}</main><script>blueiceDocumentText();</script>"),
        Some("https://example.test/rejected-retained.html".to_string()),
    );
    executor.synchronize_and_execute(&tabs).unwrap();
    assert_eq!(executor.realm_stats(first_tab), None);
    assert_eq!(
        executor.retained_debugger_identity_map_bytes(first_tab, 3),
        Some(0)
    );
    assert_eq!(
        executor.retained_page_cache_payload_bytes(first_tab, 3),
        Some(0)
    );
    assert!(matches!(
        executor.child.debugger_realm_stats(first_tab.as_u64(), 3),
        Ok(PageHostReply::Error { .. })
    ));
    assert!(tabs.close_tab(first_tab));
    executor.synchronize_and_execute(&tabs).unwrap();
    assert_eq!(executor.realm_stats(first_tab), None);
    assert_eq!(
        executor.retained_debugger_identity_map_bytes(first_tab, 3),
        None
    );
    assert_eq!(
        executor.retained_page_cache_payload_bytes(first_tab, 3),
        None
    );
    drop(executor);
    drop(host);
}

#[test]
fn real_child_core_and_http_source_sum_each_live_document_once() {
    use blueice_launcher::bluejs_host::SpawnedBlueJsHost;

    const SOURCE: &str = "const remoteValue: number = 5; remoteValue;";
    let mut responses = BTreeMap::new();
    responses.insert(
        "/shared.ts".to_string(),
        HttpTestResponse::script("text/typescript", SOURCE),
    );
    let (origin, requested, server) = spawn_local_resource_server(responses, 3);
    let policy = HttpScriptResourcePolicy::new(
        HttpScriptResourceOriginRule::same_document_origin(),
        HttpScriptIntegrityManifest::new([(
            format!("{origin}/shared.ts"),
            sha256_integrity(SOURCE.as_bytes()),
        )])
        .unwrap(),
        HttpScriptResourceLimits::default(),
    )
    .unwrap();
    let html = "<script type=\"application/x-blueice-typescript\" src=\"/shared.ts\"></script>";
    let (mut tabs, first_tab) = loaded_tabs(html, &format!("{origin}/first.html"));
    let second_tab = tabs.open_tab();
    tabs.get_mut(second_tab)
        .unwrap()
        .load_html_str(html, Some(format!("{origin}/second.html")));
    let (host, config) = SpawnedBlueJsHost::spawn_for_core().unwrap();
    let mut executor = OutOfProcessJavaScriptPageExecutor::connect_with_external_source_authorizer(
        config.socket_path(),
        config.session_token(),
        HttpOutOfProcessPageScriptSourceAuthorizer::new(policy),
    )
    .unwrap();
    executor.enable_debugger_execution_control();

    executor.synchronize_and_execute(&tabs).unwrap();
    for tab_id in [first_tab, second_tab] {
        let stats = executor.realm_stats(tab_id).unwrap();
        assert!(stats.static_metadata_bytes > 0 && stats.deferred_payload_bytes > 0);
        let core_bytes = executor.retained_debugger_payload_bytes(tab_id, 1).unwrap();
        assert!(core_bytes > 0);
        assert_eq!(
            executor.retained_external_source_payload_bytes(tab_id, 1),
            Some(SOURCE.len())
        );
        assert_eq!(
            executor.retained_page_cache_payload_bytes(tab_id, 1),
            Some(
                usize::try_from(stats.static_metadata_bytes).unwrap()
                    + usize::try_from(stats.deferred_payload_bytes).unwrap()
                    + core_bytes
                    + SOURCE.len()
            )
        );
    }
    assert_eq!(requested.lock().unwrap().len(), 2);
    let second_before = executor.retained_page_cache_payload_bytes(second_tab, 1);
    let first_before = executor
        .retained_page_cache_payload_bytes(first_tab, 1)
        .unwrap();
    let program = executor.debugger_programs(first_tab, 1).unwrap()[0];
    let after_discovery = executor
        .retained_page_cache_payload_bytes(first_tab, 1)
        .unwrap();
    assert!(after_discovery > first_before);
    assert_eq!(
        executor.retained_page_cache_payload_bytes(second_tab, 1),
        second_before
    );
    assert!(!executor
        .debugger_static_metadata(
            first_tab,
            1,
            program.program_handle,
            program.program_generation,
        )
        .unwrap()
        .is_empty());
    assert!(
        executor
            .retained_page_cache_payload_bytes(first_tab, 1)
            .unwrap()
            > after_discovery
    );

    executor.synchronize_and_execute(&tabs).unwrap();
    assert_eq!(
        executor
            .realm_stats(first_tab)
            .unwrap()
            .deferred_payload_bytes,
        0
    );
    assert_eq!(
        executor
            .realm_stats(second_tab)
            .unwrap()
            .deferred_payload_bytes,
        0
    );
    assert_eq!(requested.lock().unwrap().len(), 2);
    let second_executed = executor.retained_page_cache_payload_bytes(second_tab, 1);

    tabs.get_mut(first_tab)
        .unwrap()
        .load_html_str(html, Some(format!("{origin}/replacement.html")));
    executor.synchronize_and_execute(&tabs).unwrap();
    assert_eq!(
        executor.retained_page_cache_payload_bytes(first_tab, 1),
        None
    );
    assert!(
        executor
            .retained_page_cache_payload_bytes(first_tab, 2)
            .unwrap()
            > SOURCE.len()
    );
    assert_eq!(
        executor.retained_external_source_payload_bytes(first_tab, 2),
        Some(SOURCE.len())
    );
    assert_eq!(
        executor.retained_page_cache_payload_bytes(second_tab, 1),
        second_executed
    );
    assert_eq!(requested.lock().unwrap().len(), 3);

    assert!(tabs.close_tab(second_tab));
    executor.synchronize_and_execute(&tabs).unwrap();
    assert_eq!(
        executor.retained_page_cache_payload_bytes(second_tab, 1),
        None
    );
    let oversized = "x".repeat(page_host::PAGE_HOST_DOCUMENT_TEXT_MAX_BYTES + 1);
    tabs.get_mut(first_tab).unwrap().load_html_str(
        &format!("<main>{oversized}</main><script>blueiceDocumentText();</script>"),
        Some(format!("{origin}/rejected.html")),
    );
    executor.synchronize_and_execute(&tabs).unwrap();
    assert_eq!(
        executor.retained_page_cache_payload_bytes(first_tab, 2),
        None
    );
    assert_eq!(
        executor.retained_page_cache_payload_bytes(first_tab, 3),
        Some(0)
    );
    assert!(tabs.close_tab(first_tab));
    executor.synchronize_and_execute(&tabs).unwrap();
    assert_eq!(
        executor.retained_page_cache_payload_bytes(first_tab, 3),
        None
    );
    assert_eq!(requested.lock().unwrap().len(), 3);
    server.join().unwrap();
    drop(executor);
    drop(host);
}

#[derive(Clone, Copy, Default)]
enum InvalidRealmStats {
    #[default]
    MismatchedTuple,
    ExcessPrograms,
    SaturatedBytecode,
    SaturatedHeap,
    SaturatedStaticMetadata,
    SaturatedDeferredPayload,
    ExcessCombinedRetainedPayload,
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
                static_metadata_bytes: 0,
                deferred_payload_bytes: 0,
            },
            InvalidRealmStats::ExcessPrograms => page_host::PageHostRealmStats {
                tab_id,
                document_generation,
                program_count: page_host::PAGE_HOST_REALM_STATS_MAX_PROGRAMS + 1,
                bytecode_bytes: 64,
                heap_bytes: 128,
                static_metadata_bytes: 0,
                deferred_payload_bytes: 0,
            },
            InvalidRealmStats::SaturatedBytecode => page_host::PageHostRealmStats {
                tab_id,
                document_generation,
                program_count: 1,
                bytecode_bytes: u64::MAX,
                heap_bytes: 128,
                static_metadata_bytes: 0,
                deferred_payload_bytes: 0,
            },
            InvalidRealmStats::SaturatedHeap => page_host::PageHostRealmStats {
                tab_id,
                document_generation,
                program_count: 1,
                bytecode_bytes: 64,
                heap_bytes: u64::MAX,
                static_metadata_bytes: 0,
                deferred_payload_bytes: 0,
            },
            InvalidRealmStats::SaturatedStaticMetadata => page_host::PageHostRealmStats {
                tab_id,
                document_generation,
                program_count: 1,
                bytecode_bytes: 64,
                heap_bytes: 128,
                static_metadata_bytes: u64::MAX,
                deferred_payload_bytes: 0,
            },
            InvalidRealmStats::SaturatedDeferredPayload => page_host::PageHostRealmStats {
                tab_id,
                document_generation,
                program_count: 1,
                bytecode_bytes: 64,
                heap_bytes: 128,
                static_metadata_bytes: 0,
                deferred_payload_bytes: u64::MAX,
            },
            InvalidRealmStats::ExcessCombinedRetainedPayload => page_host::PageHostRealmStats {
                tab_id,
                document_generation,
                program_count: 1,
                bytecode_bytes: 64,
                heap_bytes: 128,
                static_metadata_bytes: page_host::PAGE_HOST_REALM_RETAINED_PAYLOAD_MAX_BYTES,
                deferred_payload_bytes: 1,
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
        InvalidRealmStats::SaturatedStaticMetadata,
        InvalidRealmStats::SaturatedDeferredPayload,
        InvalidRealmStats::ExcessCombinedRetainedPayload,
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
            static_metadata_bytes: 0,
            deferred_payload_bytes: 0,
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
