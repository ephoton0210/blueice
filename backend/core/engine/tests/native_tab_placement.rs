// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_engine::{BrowserContextId, GroupId, TabId, TabManager, WindowId};
use blueice_ipc::viewport::DisplayViewport;

fn viewport() -> DisplayViewport {
    DisplayViewport {
        width: 180.0,
        height: 120.0,
        device_scale: 2.0,
        backing_scale: Some(2.0),
    }
}

#[test]
fn placement_reorders_existing_tabs_and_preserves_the_live_document() {
    let mut tabs = TabManager::new(300.0, 200.0);
    let first = tabs.default_tab();
    let second = tabs.open_tab();
    let third = tabs.open_tab();
    let window = WindowId::from_u64(1);
    tabs.get_mut(first).unwrap().load_html_str(
        "<input value='retained'>",
        Some("https://placement.test/retained".into()),
    );
    let epoch = tabs.document_epoch(first);
    tabs.place_tab(first, window, None, None).unwrap();
    assert_eq!(
        tabs.window_tabs(window).collect::<Vec<_>>(),
        [second, third, first]
    );
    tabs.place_tab(first, window, Some(second), None).unwrap();
    assert_eq!(
        tabs.window_tabs(window).collect::<Vec<_>>(),
        [first, second, third]
    );
    tabs.place_tab(third, window, Some(second), None).unwrap();
    assert_eq!(
        tabs.window_tabs(window).collect::<Vec<_>>(),
        [first, third, second]
    );
    tabs.place_tab(first, window, Some(first), None).unwrap();
    assert_eq!(
        tabs.window_tabs(window).collect::<Vec<_>>(),
        [first, third, second]
    );
    assert_eq!(tabs.document_epoch(first), epoch);
    assert_eq!(
        tabs.get(first).unwrap().url(),
        Some("https://placement.test/retained")
    );
    assert_eq!(tabs.get(first).unwrap().viewport_size(), (300.0, 200.0));
}

#[test]
fn placement_updates_group_and_window_atomically_without_creating_a_page() {
    let mut tabs = TabManager::new(300.0, 200.0);
    let first = tabs.default_tab();
    let old = tabs.create_group("Source".into(), "#4477cc".into());
    let target_group = tabs.create_group("Destination".into(), "#228844".into());
    tabs.assign_tab_group(first, Some(old)).unwrap();
    tabs.get_mut(first).unwrap().load_html_str(
        "<p>Retained page</p>",
        Some("https://placement.test/page".into()),
    );
    let epoch = tabs.document_epoch(first);
    let destination = tabs.create_window(viewport()).unwrap();
    let anchor = tabs.open_tab_in_window(destination).unwrap();
    tabs.assign_tab_group(anchor, Some(target_group)).unwrap();
    tabs.place_tab(first, destination, Some(anchor), Some(target_group))
        .unwrap();
    assert!(tabs.window_tabs(WindowId::from_u64(1)).next().is_none());
    assert_eq!(
        tabs.window_tabs(destination).collect::<Vec<_>>(),
        [first, anchor]
    );
    assert_eq!(tabs.tab_window(first), Some(destination));
    assert_eq!(tabs.tab_group(first), Some(target_group));
    assert_eq!(tabs.document_epoch(first), epoch);
    assert_eq!(
        tabs.get(first).unwrap().url(),
        Some("https://placement.test/page")
    );
    assert_eq!(tabs.get(first).unwrap().viewport_size(), (180.0, 120.0));
    tabs.place_tab(first, destination, None, None).unwrap();
    assert_eq!(
        tabs.window_tabs(destination).collect::<Vec<_>>(),
        [anchor, first]
    );
    assert_eq!(tabs.tab_group(first), None);
}

#[test]
fn invalid_placement_never_partially_moves_or_regroups_a_tab() {
    let mut tabs = TabManager::new(300.0, 200.0);
    let first = tabs.default_tab();
    let second = tabs.open_tab();
    let source = WindowId::from_u64(1);
    let group = tabs.create_group("Retained".into(), "#4477cc".into());
    tabs.assign_tab_group(first, Some(group)).unwrap();
    let destination = tabs.create_window(viewport()).unwrap();
    let anchor = tabs.open_tab_in_window(destination).unwrap();
    let foreign = tabs.create_context("Other".into()).unwrap();
    let foreign_window = tabs.create_window_in_context(foreign, viewport()).unwrap();
    let foreign_group = tabs
        .create_group_in_context(foreign, "Foreign".into(), "#cc3344".into())
        .unwrap();
    for (tab, window, before, target_group) in [
        (first, WindowId::from_u64(999), None, None),
        (TabId::from_u64(999), destination, None, None),
        (first, destination, Some(second), None),
        (first, destination, Some(TabId::from_u64(999)), None),
        (
            first,
            destination,
            Some(anchor),
            Some(GroupId::from_u64(999)),
        ),
        (first, destination, Some(anchor), Some(foreign_group)),
        (first, foreign_window, None, None),
    ] {
        assert!(tabs.place_tab(tab, window, before, target_group).is_err());
        assert_eq!(
            tabs.window_tabs(source).collect::<Vec<_>>(),
            [first, second]
        );
        assert_eq!(tabs.window_tabs(destination).collect::<Vec<_>>(), [anchor]);
        assert_eq!(tabs.tab_group(first), Some(group));
        assert_eq!(tabs.tab_context(first), Some(BrowserContextId::from_u64(1)));
        assert_eq!(tabs.get(first).unwrap().viewport_size(), (300.0, 200.0));
    }
    tabs.close_tab(anchor);
    assert!(tabs
        .place_tab(first, destination, Some(anchor), None)
        .is_err());
    assert_eq!(
        tabs.window_tabs(source).collect::<Vec<_>>(),
        [first, second]
    );
    tabs.close_window(destination).unwrap();
    assert!(tabs.place_tab(first, destination, None, None).is_err());
}
