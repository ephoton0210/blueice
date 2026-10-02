// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn new_creates_exactly_one_tab_as_the_default() {
    let tabs = TabManager::new(320.0, 200.0);
    assert_eq!(tabs.ids().collect::<Vec<_>>(), vec![tabs.default_tab()]);
    assert!(tabs.get(tabs.default_tab()).is_some());
}

#[test]
fn extension_origin_scopes_follow_the_live_tab_url_and_default_to_legacy_global_grants() {
    let mut tabs = TabManager::new(320.0, 200.0);
    let tab = tabs.default_tab();
    assert!(tabs.check_extension_origin("dom:read", tab).is_ok());
    tabs.set_extension_capability_origins(BTreeMap::from([
        (
            "dom:read".to_string(),
            BTreeSet::from(["https://example.test".to_string()]),
        ),
        (
            "dom:write".to_string(),
            BTreeSet::from(["http://127.0.0.1:4312".to_string()]),
        ),
    ]));
    assert!(tabs.check_extension_origin("dom:read", tab).is_err());
    tabs.get_mut(tab).unwrap().load_html_str(
        "<p>one</p>",
        Some("https://example.test/path?query=1".to_string()),
    );
    assert!(tabs.check_extension_origin("dom:read", tab).is_ok());
    assert!(tabs.check_extension_origin("dom:write", tab).is_err());
    assert!(tabs.check_extension_origin("network:observe", tab).is_ok());
    tabs.get_mut(tab)
        .unwrap()
        .load_html_str("<p>two</p>", Some("http://127.0.0.1:4312/page".to_string()));
    assert!(tabs.check_extension_origin("dom:read", tab).is_err());
    assert!(tabs.check_extension_origin("dom:write", tab).is_ok());
    tabs.get_mut(tab).unwrap().navigate("about:blank").unwrap();
    assert!(tabs.check_extension_origin("dom:write", tab).is_err());
    assert!(tabs
        .check_extension_origin("dom:write", TabId::from_u64(999))
        .is_err());
}

#[test]
fn scoped_network_intercept_rules_cannot_block_other_origins_even_when_host_matches() {
    let mut tabs = TabManager::new(320.0, 200.0);
    tabs.add_extension_navigation_block_host_rule(7, "127.0.0.1".to_string())
        .unwrap();
    assert!(tabs.is_extension_navigation_blocked("http://127.0.0.1:4312/page"));
    assert!(tabs.is_extension_navigation_blocked("http://127.0.0.1:4313/page"));

    tabs.set_extension_capability_origins(BTreeMap::from([(
        "network:intercept".to_string(),
        BTreeSet::from(["http://127.0.0.1:4312".to_string()]),
    )]));
    let snapshot = tabs.extension_navigation_block_rule_snapshot();
    assert!(extension_navigation_rules_block_url(
        &snapshot,
        "http://127.0.0.1:4312/page"
    ));
    assert!(!extension_navigation_rules_block_url(
        &snapshot,
        "http://127.0.0.1:4313/page"
    ));
    assert!(!extension_navigation_rules_block_url(
        &snapshot,
        "https://127.0.0.1:4312/page"
    ));
    assert!(!extension_navigation_rules_block_url(
        &snapshot,
        "about:settings"
    ));
}

#[test]
fn optional_intercept_revocation_invalidates_captured_rules_and_regrant_does_not_resurrect_them() {
    let root = std::env::temp_dir().join(format!(
        "blueice-optional-network-generation-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let manifest = root.join("extension.json");
    std::fs::write(&manifest,
        r#"{"name":"Optional network","version":"1","blueice_api_version":1,"entry_point":"extension.wasm","capabilities":{"optional":["network:intercept"]}}"#
    ).unwrap();
    std::fs::write(root.join("extension.wasm"), b"\0asm\x01\0\0\0").unwrap();
    let installed = blueice_extension_host::load_installed_extension(&manifest).unwrap();
    let id = installed.extension_id().to_string();
    let registry = Arc::new(blueice_extension_host::registry_for_installed_extension(
        &installed,
    ));
    let mut tabs = TabManager::new(320.0, 200.0);
    tabs.set_extension_permission_registry(Arc::clone(&registry), id.clone());
    assert!(tabs
        .add_extension_navigation_block_host_rule(7, "old.example.test".into())
        .is_err());

    assert!(registry.grant_optional(&id, "network:intercept").unwrap());
    tabs.add_extension_navigation_block_host_rule(7, "old.example.test".into())
        .unwrap();
    tabs.add_extension_navigation_redirect_rule(
        7,
        "https://example.test/old".into(),
        "https://example.test/new".into(),
    )
    .unwrap();
    let captured = tabs.extension_navigation_block_rule_snapshot();
    assert!(extension_navigation_rules_block_url(
        &captured,
        "https://old.example.test/page"
    ));
    assert_eq!(
        extension_navigation_rules_redirect_url(&captured, "https://example.test/old"),
        Some("https://example.test/new".into())
    );

    assert!(registry.revoke_optional(&id, "network:intercept").unwrap());
    assert!(!extension_navigation_rules_block_url(
        &captured,
        "https://old.example.test/page"
    ));
    assert_eq!(
        extension_navigation_rules_redirect_url(&captured, "https://example.test/old"),
        None
    );
    assert_eq!(
        tabs.extension_navigation_block_rules.len(),
        1,
        "revocation invalidates the captured rules before session cleanup"
    );
    tabs.prune_stale_extension_navigation_rules();
    assert!(
        tabs.extension_navigation_block_rules.is_empty(),
        "the next session tick must physically remove revoked rules"
    );
    assert!(registry.grant_optional(&id, "network:intercept").unwrap());
    assert!(!extension_navigation_rules_block_url(
        &captured,
        "https://old.example.test/page"
    ));
    assert_eq!(
        extension_navigation_rules_redirect_url(&captured, "https://example.test/old"),
        None
    );

    assert!(
        tabs.with_stable_extension_capability("network:intercept", 0, |tabs| {
            tabs.add_extension_navigation_block_host_rule(7, "stale.example.test".into())
        })
        .is_err(),
        "a queued registration cannot borrow the new grant"
    );
    let fresh_generation = registry
        .capability_generation(&id, "network:intercept")
        .unwrap();
    tabs.with_stable_extension_capability("network:intercept", fresh_generation, |tabs| {
        tabs.add_extension_navigation_block_host_rule(7, "new.example.test".into())
    })
    .unwrap()
    .unwrap();
    let renewed = tabs.extension_navigation_block_rule_snapshot();
    assert!(!extension_navigation_rules_block_url(
        &renewed,
        "https://old.example.test/page"
    ));
    assert!(!extension_navigation_rules_block_url(
        &renewed,
        "https://stale.example.test/page"
    ));
    assert!(extension_navigation_rules_block_url(
        &renewed,
        "https://new.example.test/page"
    ));
    assert_eq!(
        extension_navigation_rules_redirect_url(&renewed, "https://example.test/old"),
        None
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn extension_navigation_block_rules_match_canonical_urls_and_clear_per_connection() {
    let mut tabs = TabManager::new(320.0, 200.0);
    tabs.add_extension_navigation_block_rule(
        41,
        "HTTPS://EXAMPLE.test/private#client-fragment".to_string(),
    )
    .unwrap();
    assert!(tabs.is_extension_navigation_blocked("https://example.test/private"));
    assert!(tabs.is_extension_navigation_blocked("https://example.test/private#another"));
    assert!(!tabs.is_extension_navigation_blocked("https://example.test/other"));

    tabs.clear_extension_navigation_block_rules(40);
    assert!(tabs.is_extension_navigation_blocked("https://example.test/private"));
    tabs.clear_extension_navigation_block_rules(41);
    assert!(!tabs.is_extension_navigation_blocked("https://example.test/private"));
}

#[test]
fn extension_navigation_block_rules_reject_non_http_and_credentialed_urls() {
    let mut tabs = TabManager::new(320.0, 200.0);
    assert!(tabs
        .add_extension_navigation_block_rule(1, "about:blank".to_string())
        .is_err());
    assert!(tabs
        .add_extension_navigation_block_rule(1, "https://user:secret@example.test/".to_string())
        .is_err());
    assert!(!tabs.is_extension_navigation_blocked("about:blank"));
}

#[test]
fn exact_navigation_redirects_are_same_origin_scoped_conflict_free_and_connection_owned() {
    let mut tabs = TabManager::new(320.0, 200.0);
    let source = "https://example.test/old?x=1";
    let target = "https://example.test/new?x=1";
    tabs.add_extension_navigation_redirect_rule(41, source.into(), target.into())
        .unwrap();
    let snapshot = tabs.extension_navigation_block_rule_snapshot();
    assert_eq!(
        extension_navigation_rules_redirect_url(&snapshot, source),
        Some(target.into())
    );
    assert_eq!(
        extension_navigation_rules_redirect_url(&snapshot, "https://example.test/other"),
        None
    );
    assert_eq!(
        extension_navigation_rules_redirect_url(
            &snapshot,
            "https://user:secret@example.test/old?x=1"
        ),
        None
    );
    assert!(tabs
        .add_extension_navigation_redirect_rule(
            42,
            source.into(),
            "https://example.test/other".into()
        )
        .is_err());
    assert!(tabs
        .add_extension_navigation_redirect_rule(
            41,
            source.into(),
            "https://outside.test/new".into()
        )
        .is_err());
    assert!(tabs
        .add_extension_navigation_redirect_rule(41, source.into(), source.into())
        .is_err());
    assert!(tabs
        .add_extension_navigation_redirect_rule(41, "about:blank".into(), target.into())
        .is_err());
    assert!(tabs
        .add_extension_navigation_redirect_rule(
            41,
            source.into(),
            format!("https://example.test/{}", "x".repeat(2048)),
        )
        .is_err());
    tabs.set_extension_capability_origins(BTreeMap::from([(
        "network:intercept".to_string(),
        BTreeSet::from(["https://other.test".to_string()]),
    )]));
    assert_eq!(
        extension_navigation_rules_redirect_url(
            &tabs.extension_navigation_block_rule_snapshot(),
            source
        ),
        None
    );
    tabs.clear_extension_navigation_block_rules(42);
    tabs.set_extension_capability_origins(BTreeMap::new());
    assert_eq!(
        extension_navigation_rules_redirect_url(
            &tabs.extension_navigation_block_rule_snapshot(),
            source
        ),
        Some(target.into())
    );
    tabs.clear_extension_navigation_block_rules(41);
    assert_eq!(
        extension_navigation_rules_redirect_url(
            &tabs.extension_navigation_block_rule_snapshot(),
            source
        ),
        None
    );
}

#[test]
fn extension_host_rules_match_exact_hosts_and_subdomains_but_not_suffix_tricks() {
    let mut tabs = TabManager::new(320.0, 200.0);
    tabs.add_extension_navigation_block_host_rule(41, "EXAMPLE.test.".to_string())
        .unwrap();
    assert!(tabs.is_extension_navigation_blocked("https://example.test/path"));
    assert!(tabs.is_extension_navigation_blocked("http://sub.example.test/other"));
    assert!(!tabs.is_extension_navigation_blocked("https://notexample.test/"));
    assert!(!tabs.is_extension_navigation_blocked("https://example.test.evil/"));
    assert!(!tabs.is_extension_navigation_blocked("about:blank"));
    tabs.add_extension_navigation_block_host_rule(41, "127.0.0.1".to_string())
        .unwrap();
    assert!(tabs.is_extension_navigation_blocked("http://127.0.0.1/"));
    assert!(!tabs.is_extension_navigation_blocked("http://sub.127.0.0.1/"));
    tabs.clear_extension_navigation_block_rules(40);
    assert!(tabs.is_extension_navigation_blocked("https://example.test/"));
    tabs.clear_extension_navigation_block_rules(41);
    assert!(!tabs.is_extension_navigation_blocked("https://example.test/"));
}

#[test]
fn extension_host_rules_reject_urls_wildcards_unicode_and_invalid_dns_labels() {
    let mut tabs = TabManager::new(320.0, 200.0);
    for host in [
        "https://example.test",
        "*.example.test",
        "example..test",
        "-example.test",
        "example-.test",
        "ex ample.test",
        "exämple.test",
        "example.test:443",
        "127.000.000.001",
    ] {
        assert!(
            tabs.add_extension_navigation_block_host_rule(7, host.to_string())
                .is_err(),
            "accepted invalid host {host:?}"
        );
    }
}

#[test]
fn extension_path_prefix_rules_use_host_and_path_segment_boundaries() {
    let mut tabs = TabManager::new(320.0, 200.0);
    tabs.add_extension_navigation_block_path_prefix_rule(
        41,
        "EXAMPLE.test.".into(),
        "/private".into(),
    )
    .unwrap();
    for url in [
        "https://example.test/private",
        "https://sub.example.test/private/report?download=1#section",
    ] {
        assert!(tabs.is_extension_navigation_blocked(url), "missed {url}");
    }
    for url in [
        "https://example.test/privateer",
        "https://notexample.test/private",
        "https://example.test.evil/private",
        "https://example.test/public/private",
        "https://example.test/%70rivate",
    ] {
        assert!(
            !tabs.is_extension_navigation_blocked(url),
            "overmatched {url}"
        );
    }
    tabs.clear_extension_navigation_block_rules(41);
    assert!(!tabs.is_extension_navigation_blocked("https://example.test/private"));
    tabs.add_extension_navigation_block_path_prefix_rule(42, "127.0.0.1".into(), "/private".into())
        .unwrap();
    assert!(tabs.is_extension_navigation_blocked("http://127.0.0.1/private/child"));
    assert!(!tabs.is_extension_navigation_blocked("http://sub.127.0.0.1/private/child"));
}

#[test]
fn extension_path_prefix_rules_reject_ambiguous_paths_and_share_the_quota() {
    let mut tabs = TabManager::new(320.0, 200.0);
    for path in [
        "", "private", "/a//b", "/./x", "/../x", "/a?x=1", "/a#frag", "/a%2fb", "/é",
    ] {
        assert!(
            tabs.add_extension_navigation_block_path_prefix_rule(
                7,
                "example.test".into(),
                path.into(),
            )
            .is_err(),
            "accepted {path:?}"
        );
    }
    assert!(tabs
        .add_extension_navigation_block_path_prefix_rule(
            7,
            "example.test".into(),
            format!("/{}", "x".repeat(512)),
        )
        .is_err());
    for index in 0..MAX_EXTENSION_NAVIGATION_RULES_PER_CONNECTION {
        tabs.add_extension_navigation_block_path_prefix_rule(
            7,
            "example.test".into(),
            format!("/private/{index}"),
        )
        .unwrap();
    }
    assert!(tabs
        .add_extension_navigation_block_host_rule(7, "overflow.test".into())
        .is_err());
}

#[test]
fn extension_url_and_host_rules_share_a_quota_and_clear_together() {
    let mut tabs = TabManager::new(320.0, 200.0);
    tabs.add_extension_navigation_block_rule(7, "https://exact.example/".to_string())
        .unwrap();
    for index in 1..MAX_EXTENSION_NAVIGATION_RULES_PER_CONNECTION {
        tabs.add_extension_navigation_block_host_rule(7, format!("host{index}.example"))
            .unwrap();
    }
    assert!(tabs
        .add_extension_navigation_block_host_rule(7, "overflow.example".to_string())
        .is_err());
    assert!(tabs.is_extension_navigation_blocked("https://exact.example/"));
    assert!(tabs.is_extension_navigation_blocked("https://host1.example/"));
    tabs.clear_extension_navigation_block_rules(7);
    assert!(!tabs.is_extension_navigation_blocked("https://exact.example/"));
    assert!(!tabs.is_extension_navigation_blocked("https://host1.example/"));
}

#[test]
fn open_tab_returns_a_distinct_id_and_the_tab_becomes_gettable() {
    let mut tabs = TabManager::new(320.0, 200.0);
    let new_id = tabs.open_tab();
    assert_ne!(new_id, tabs.default_tab());
    assert!(tabs.get(new_id).is_some());
    assert_eq!(
        tabs.ids().collect::<Vec<_>>(),
        vec![tabs.default_tab(), new_id]
    );
}

#[test]
fn open_tab_never_reuses_an_id_even_after_the_tab_that_had_it_closes() {
    let mut tabs = TabManager::new(320.0, 200.0);
    let first = tabs.open_tab();
    assert!(tabs.close_tab(first));
    let second = tabs.open_tab();
    assert_ne!(
        first, second,
        "a closed tab's id must never be reissued to an unrelated later tab"
    );
}

#[test]
fn close_tab_on_an_unknown_id_returns_false_and_changes_nothing() {
    let mut tabs = TabManager::new(320.0, 200.0);
    assert!(!tabs.close_tab(TabId::from_u64(999_999)));
    assert_eq!(tabs.ids().count(), 1);
}

#[test]
fn close_tab_on_an_already_closed_id_returns_false_the_second_time() {
    let mut tabs = TabManager::new(320.0, 200.0);
    let id = tabs.open_tab();
    assert!(tabs.close_tab(id));
    assert!(!tabs.close_tab(id));
}

#[test]
fn closing_every_tab_including_the_default_is_allowed() {
    let mut tabs = TabManager::new(320.0, 200.0);
    let default = tabs.default_tab();
    assert!(tabs.close_tab(default));
    assert_eq!(tabs.ids().count(), 0);
    // The default identity itself doesn't change -- it just no
    // longer resolves to anything, the same failure mode any other
    // stale TabId already has.
    assert_eq!(tabs.default_tab(), default);
    assert!(tabs.get(default).is_none());
}

#[test]
fn get_mut_on_an_unknown_id_is_none_not_a_panic() {
    let mut tabs = TabManager::new(320.0, 200.0);
    assert!(tabs.get_mut(TabId::from_u64(999_999)).is_none());
}

#[test]
fn a_newly_opened_tab_starts_at_the_current_window_size() {
    let mut tabs = TabManager::new(320.0, 200.0);
    tabs.set_window_size(640.0, 480.0);
    let id = tabs.open_tab();
    assert_eq!(tabs.get(id).unwrap().viewport_size(), (640.0, 480.0));
}

#[test]
fn tab_id_round_trips_through_as_u64_and_from_u64() {
    let id = TabId::from_u64(42);
    assert_eq!(TabId::from_u64(id.as_u64()), id);
}

#[test]
fn the_assistant_panel_reaches_every_tab_and_follows_the_endpoint() {
    use crate::assistant_page::PanelKind;
    let mut tabs = TabManager::new(320.0, 200.0);
    assert!(!tabs.assistant_panel().is_available());
    tabs.set_translation_endpoint("/tmp/as.sock".into(), std::time::Duration::from_secs(1));
    assert!(tabs.assistant_panel().is_available());

    let first = tabs.default_tab();
    let second = tabs.open_tab();
    for id in [first, second] {
        assert!(tabs.navigate_to_built_in(id, "about:assistant"));
    }
    // A tab that is not on the panel is left alone.
    let third = tabs.open_tab();
    tabs.get_mut(third)
        .unwrap()
        .load_html_str("<p>x</p>", Some("https://example.com/".into()));

    tabs.assistant_panel()
        .push(PanelKind::Summary, None, None, Ok("shared result".into()));
    let refreshed = tabs.refresh_assistant_panels();
    assert_eq!(refreshed, vec![first, second]);
    for id in [first, second] {
        let page = tabs.get(id).unwrap();
        let text: String = page
            .render()
            .commands
            .iter()
            .filter_map(|c| match c {
                blueice_paint::PaintCommand::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" ");
        assert!(text.contains("shared"), "{text}");
    }
}

#[test]
fn a_page_from_history_reload_also_reads_the_shared_panel() {
    let mut tabs = TabManager::new(320.0, 200.0);
    let id = tabs.default_tab();
    assert!(tabs.navigate_to_built_in(id, "about:assistant"));
    assert!(tabs.navigate_to_built_in(id, "about:blank"));
    assert!(tabs.navigate_history_to_built_in(id, HistoryDirection::Back, "about:assistant"));
    tabs.assistant_panel().set_available(true);
    assert_eq!(tabs.refresh_assistant_panels(), vec![id]);
}

#[test]
fn the_downloads_source_reaches_existing_and_newly_opened_tabs() {
    let mut tabs = TabManager::new(300.0, 200.0);
    let before = tabs.open_tab();
    assert!(
        tabs.downloads_source().is_none() && tabs.get(before).unwrap().downloads_source().is_none()
    );

    let source = Arc::new(DownloadsSource::without_spawner(std::path::PathBuf::from(
        "/nonexistent/downloads.sock",
    )));
    tabs.set_downloads_source(source.clone());
    let after = tabs.open_tab();
    for id in [tabs.default_tab(), before, after] {
        let page_source = tabs
            .get(id)
            .unwrap()
            .downloads_source()
            .unwrap_or_else(|| panic!("tab {id:?} has no source"));
        assert!(
            Arc::ptr_eq(page_source, &source),
            "every tab shares the one source"
        );
    }
    assert!(Arc::ptr_eq(tabs.downloads_source().unwrap(), &source));
}

#[test]
fn groups_are_ordered_and_tab_membership_is_independent_of_page_state() {
    let mut tabs = TabManager::new(300.0, 200.0);
    let second = tabs.open_tab();
    let research = tabs.create_group("Research".to_string(), "#4f8cff".to_string());
    let reference = tabs.create_group("Reference".to_string(), "#f06a6a".to_string());
    tabs.set_tab_group(second, Some(research));

    assert_eq!(tabs.tab_group(second), Some(research));
    assert_eq!(
        tabs.groups().map(|group| group.id()).collect::<Vec<_>>(),
        vec![research, reference]
    );
    assert_eq!(tabs.group(research).unwrap().name(), "Research");
    assert_eq!(tabs.group(research).unwrap().color(), "#4f8cff");
    assert!(!tabs.group(research).unwrap().collapsed());
    assert!(
        tabs.get(second).is_some(),
        "grouping never replaces the page"
    );
}

#[test]
fn closing_a_group_ungroups_but_never_closes_its_tabs() {
    let mut tabs = TabManager::new(300.0, 200.0);
    let second = tabs.open_tab();
    let group = tabs.create_group("Work".to_string(), "#2fa36b".to_string());
    tabs.set_tab_group(tabs.default_tab(), Some(group));
    tabs.set_tab_group(second, Some(group));

    assert!(tabs.close_group(group));
    assert!(tabs.get(tabs.default_tab()).is_some());
    assert!(tabs.get(second).is_some());
    assert_eq!(tabs.tab_group(tabs.default_tab()), None);
    assert_eq!(tabs.tab_group(second), None);
    assert!(tabs.groups().next().is_none());
    assert!(!tabs.close_group(group));
}

#[test]
fn resize_all_keeps_background_tabs_ready_for_selection() {
    let mut tabs = TabManager::new(300.0, 200.0);
    let background = tabs.open_tab();
    tabs.resize_all(640.0, 480.0);
    assert_eq!(
        tabs.get(tabs.default_tab()).unwrap().viewport_size(),
        (640.0, 480.0)
    );
    assert_eq!(
        tabs.get(background).unwrap().viewport_size(),
        (640.0, 480.0)
    );
}

fn traverse_built_in_history(tabs: &mut TabManager, tab: TabId, direction: HistoryDirection) {
    match tabs.history_destination(tab, direction) {
        Some(HistoryDestination::Snapshot) => {
            assert!(tabs.restore_history_snapshot(tab, direction))
        }
        Some(HistoryDestination::Reload(None)) => {
            assert!(tabs.navigate_history_to_blank(tab, direction))
        }
        Some(HistoryDestination::Reload(Some(url))) => {
            assert!(tabs.navigate_history_to_built_in(tab, direction, &url))
        }
        Some(HistoryDestination::Post { .. }) => panic!("expected a GET history entry"),
        None => panic!("expected a history entry"),
    }
}

#[test]
fn snapshot_history_preserves_tab_frame_and_document_generations() {
    let mut tabs =
        TabManager::new_with_history_snapshot_mode(300.0, 200.0, HistorySnapshotMode::Snapshot);
    let tab = tabs.default_tab();
    let quiet = tabs.open_tab();
    assert!(tabs.navigate_to_built_in(tab, "about:credits"));
    assert_eq!(tabs.get_mut(tab).unwrap().advance_frame_generation(), 1);
    assert!(tabs.navigate_to_built_in(tab, "about:credits"));
    assert_eq!(tabs.get_mut(tab).unwrap().advance_frame_generation(), 2);
    let mut document = tabs.get(tab).unwrap().document_generation();

    for (direction, expected_frame) in [
        (HistoryDirection::Back, 3),
        (HistoryDirection::Forward, 4),
        (HistoryDirection::Back, 5),
    ] {
        assert!(tabs.restore_history_snapshot(tab, direction));
        let page = tabs.get_mut(tab).unwrap();
        assert_eq!(page.document_generation(), document + 1);
        document = page.document_generation();
        assert_eq!(page.advance_frame_generation(), expected_frame);
    }
    assert_eq!(tabs.get(quiet).unwrap().frame_generation(), 0);
    assert_eq!(tabs.get(quiet).unwrap().document_generation(), 0);
}

#[test]
fn document_epoch_tracks_replacement_not_repaint_and_never_reuses_a_closed_tab() {
    let mut tabs =
        TabManager::new_with_history_snapshot_mode(300.0, 200.0, HistorySnapshotMode::Snapshot);
    let first = tabs.default_tab();
    let second = tabs.open_tab();
    assert_eq!(tabs.document_epoch(first), Some(0));
    assert_eq!(tabs.document_epoch(second), Some(0));
    tabs.resize_all(640.0, 480.0);
    assert_eq!(tabs.document_epoch(first), Some(0));
    assert!(!tabs.navigate_to_built_in(first, "about:not-a-page"));
    assert_eq!(tabs.document_epoch(first), Some(0));

    assert!(tabs.navigate_to_built_in(first, "about:credits"));
    assert_eq!(tabs.document_epoch(first), Some(1));
    assert!(tabs.navigate_to_built_in(first, "about:credits"));
    assert_eq!(
        tabs.document_epoch(first),
        Some(2),
        "even the same URL commits a different document"
    );
    assert!(tabs.restore_history_snapshot(first, HistoryDirection::Back));
    assert_eq!(
        tabs.document_epoch(first),
        Some(3),
        "restoring a saved page must not restore its former gesture identity"
    );
    assert_eq!(tabs.document_epoch(second), Some(0));

    assert!(tabs.close_tab(first));
    assert_eq!(tabs.document_epoch(first), None);
    let third = tabs.open_tab();
    assert_ne!(first, third);
    assert_eq!(tabs.document_epoch(third), Some(0));
}

#[test]
fn url_only_history_reload_advances_document_epoch() {
    let mut tabs = TabManager::new(300.0, 200.0);
    let tab = tabs.default_tab();
    assert!(tabs.navigate_to_built_in(tab, "about:credits"));
    assert!(tabs.navigate_to_built_in(tab, "about:credits"));
    assert_eq!(tabs.document_epoch(tab), Some(2));
    assert!(tabs.navigate_history_to_built_in(tab, HistoryDirection::Back, "about:credits"));
    assert_eq!(tabs.document_epoch(tab), Some(3));
    assert!(!tabs.navigate_history_to_built_in(tab, HistoryDirection::Back, "about:not-a-page"));
    assert_eq!(tabs.document_epoch(tab), Some(3));
}

#[test]
fn each_tab_owns_an_independent_back_and_forward_stack() {
    let mut tabs = TabManager::new(300.0, 200.0);
    let first = tabs.default_tab();
    let second = tabs.open_tab();

    assert!(tabs.navigate_to_built_in(first, "about:credits"));
    assert!(tabs.navigate_to_built_in(second, "about:downloads"));
    assert_eq!(tabs.can_go_back(first), Some(true));
    assert_eq!(tabs.can_go_back(second), Some(true));
    assert_eq!(tabs.can_go_forward(first), Some(false));

    assert_eq!(
        tabs.history_destination(first, HistoryDirection::Back),
        Some(HistoryDestination::Reload(None)),
        "the default history policy rebuilds the initial blank page"
    );
    traverse_built_in_history(&mut tabs, first, HistoryDirection::Back);
    assert_eq!(tabs.get(first).unwrap().url(), None);
    assert_eq!(tabs.can_go_back(first), Some(false));
    assert_eq!(tabs.can_go_forward(first), Some(true));
    assert_eq!(
        tabs.get(second).unwrap().url(),
        Some("about:downloads"),
        "going back in one tab must not move another tab"
    );
    assert_eq!(tabs.can_go_forward(second), Some(false));

    traverse_built_in_history(&mut tabs, first, HistoryDirection::Forward);
    assert_eq!(tabs.get(first).unwrap().url(), Some("about:credits"));
    assert_eq!(tabs.can_go_forward(first), Some(false));
}

#[test]
fn a_fresh_navigation_after_back_discards_only_that_tabs_forward_entries() {
    let mut tabs = TabManager::new(300.0, 200.0);
    let tab = tabs.default_tab();
    assert!(tabs.navigate_to_built_in(tab, "about:credits"));
    assert!(tabs.navigate_to_built_in(tab, "about:downloads"));
    traverse_built_in_history(&mut tabs, tab, HistoryDirection::Back);
    assert_eq!(tabs.get(tab).unwrap().url(), Some("about:credits"));
    assert_eq!(tabs.can_go_forward(tab), Some(true));

    assert!(tabs.navigate_to_built_in(tab, "about:blank"));
    assert_eq!(tabs.get(tab).unwrap().url(), Some("about:blank"));
    assert_eq!(tabs.can_go_forward(tab), Some(false));
    assert_eq!(
        tabs.history_destination(tab, HistoryDirection::Forward),
        None
    );
}

#[test]
fn default_history_entries_retain_urls_and_require_a_reload() {
    let mut tabs = TabManager::new(300.0, 200.0);
    let tab = tabs.default_tab();
    tabs.get_mut(tab).unwrap().load_html_str(
        "<input id='draft' value='kept locally'><p>original document</p>",
        Some("https://example.test/original".to_string()),
    );
    assert!(tabs.navigate_to_built_in(tab, "about:credits"));
    assert_eq!(tabs.history_snapshot_mode(), HistorySnapshotMode::Reload);
    assert_eq!(
        tabs.history_destination(tab, HistoryDirection::Back),
        Some(HistoryDestination::Reload(Some(
            "https://example.test/original".to_string()
        )))
    );
    assert!(
        !tabs.restore_history_snapshot(tab, HistoryDirection::Back),
        "the default policy must not silently use the stale in-memory DOM"
    );
}

#[test]
fn explicit_snapshot_history_restores_the_left_pages_dom_state() {
    let mut tabs =
        TabManager::new_with_history_snapshot_mode(300.0, 200.0, HistorySnapshotMode::Snapshot);
    let tab = tabs.default_tab();
    tabs.get_mut(tab).unwrap().load_html_str(
        "<input id='draft' value='kept locally'><p>original document</p>",
        Some("https://example.test/original".to_string()),
    );
    assert!(tabs.navigate_to_built_in(tab, "about:credits"));
    assert_eq!(
        tabs.history_destination(tab, HistoryDirection::Back),
        Some(HistoryDestination::Snapshot)
    );
    assert!(tabs.restore_history_snapshot(tab, HistoryDirection::Back));
    let dom = tabs.get(tab).unwrap().dom_dump();
    assert!(dom.contains("kept locally"));
    assert!(dom.contains("original document"));
    assert_eq!(
        tabs.get(tab).unwrap().url(),
        Some("https://example.test/original")
    );
}

#[test]
fn snapshot_history_documents_never_reuse_node_ids_from_retained_entries() {
    let mut tabs =
        TabManager::new_with_history_snapshot_mode(300.0, 200.0, HistorySnapshotMode::Snapshot);
    let tab = tabs.default_tab();
    tabs.get_mut(tab).unwrap().load_html_str(
        "<button>first</button>",
        Some("https://example.test/first".to_string()),
    );
    let first_ids: std::collections::HashSet<u64> = tabs
        .get(tab)
        .unwrap()
        .snapshot(0, tab.as_u64())
        .nodes
        .into_iter()
        .map(|node| node.id)
        .collect();

    assert!(tabs.navigate_to_built_in(tab, "about:credits"));
    let second_ids: std::collections::HashSet<u64> = tabs
        .get(tab)
        .unwrap()
        .snapshot(0, tab.as_u64())
        .nodes
        .into_iter()
        .map(|node| node.id)
        .collect();
    assert!(first_ids.is_disjoint(&second_ids));

    // A new branch after Back clears the forward *history*, but must still
    // allocate beyond the retained document it just displaced; an old
    // client-side NodeId cannot be redirected to this replacement page.
    assert!(tabs.restore_history_snapshot(tab, HistoryDirection::Back));
    assert!(tabs.navigate_to_built_in(tab, "about:credits?lang=zh-TW"));
    let branch_ids: std::collections::HashSet<u64> = tabs
        .get(tab)
        .unwrap()
        .snapshot(0, tab.as_u64())
        .nodes
        .into_iter()
        .map(|node| node.id)
        .collect();
    assert!(first_ids.is_disjoint(&branch_ids));
    assert!(second_ids.is_disjoint(&branch_ids));
}

#[test]
fn retained_snapshot_entries_reflow_with_the_shared_window() {
    let mut tabs =
        TabManager::new_with_history_snapshot_mode(300.0, 200.0, HistorySnapshotMode::Snapshot);
    let tab = tabs.default_tab();
    assert!(tabs.navigate_to_built_in(tab, "about:credits"));
    tabs.resize_all(640.0, 480.0);
    assert!(tabs.restore_history_snapshot(tab, HistoryDirection::Back));
    assert_eq!(tabs.get(tab).unwrap().viewport_size(), (640.0, 480.0));
    assert!(tabs.restore_history_snapshot(tab, HistoryDirection::Forward));
    assert_eq!(tabs.get(tab).unwrap().viewport_size(), (640.0, 480.0));
}

#[test]
fn group_ids_never_reuse_a_closed_groups_identity() {
    let mut tabs = TabManager::new(300.0, 200.0);
    let first = tabs.create_group("First".to_string(), "#111111".to_string());
    assert!(tabs.close_group(first));
    let second = tabs.create_group("Second".to_string(), "#222222".to_string());
    assert_ne!(first, second);
}
