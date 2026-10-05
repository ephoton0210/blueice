// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

fn page(html: &str) -> Page {
    let mut page = Page::new(400.0, 200.0);
    page.load_html_str(html, Some("https://example.test/".into()));
    page
}

fn replace(page: &mut Page, id: &str, text: &str) {
    let node = page.script_get_element_by_id(id).unwrap();
    let handle = page.script_handle_for_node(node);
    page.script_set_text_content(handle, text.into()).unwrap();
}

fn attribute(page: &mut Page, id: &str, name: &str, value: &str) {
    let node = page.script_get_element_by_id(id).unwrap();
    let NodeData::Element { attributes, .. } = page.doc.data_mut(node) else {
        panic!()
    };
    attributes.retain(|(key, _)| key != name);
    attributes.push((name.into(), value.into()));
    page.restyle_and_relayout();
}

fn stream(page: &Page) -> AccessibilitySnapshot {
    page.snapshot(1, 2).accessibility.unwrap()
}

#[test]
fn status_is_atomic_and_reads_do_not_create_duplicate_announcements() {
    let mut page = page("<div role='status'>Count <span id='count'>1</span></div>");
    assert!(stream(&page).announcements.is_empty());
    replace(&mut page, "count", "2😀");
    let updated = stream(&page);
    assert_eq!(updated.announcements[0].text, "Count 2😀");
    assert_eq!(updated.announcements[0].politeness, LivePoliteness::Polite);
    assert_eq!(stream(&page), updated);
    page.scroll_by(100.0);
    assert_eq!(stream(&page), updated);
    page.resize(500.0, 200.0);
    assert_eq!(stream(&page).revision, updated.revision);
    assert!(stream(&page).announcements.is_empty());
}

#[test]
fn busy_updates_coalesce_until_the_region_is_ready() {
    let mut page = page("<div id='status' role='status'><span id='count'>1</span></div>");
    attribute(&mut page, "status", "aria-busy", "true");
    replace(&mut page, "count", "2");
    replace(&mut page, "count", "3");
    assert!(stream(&page).announcements.is_empty());
    attribute(&mut page, "status", "aria-busy", "false");
    assert_eq!(stream(&page).announcements[0].text, "3");
}

#[test]
fn nested_live_off_hidden_inert_and_protected_content_never_enter_announcements() {
    let mut page = page("<div role='status'><span id='public'>ready</span><span aria-live='off'>off-secret</span><span aria-hidden='true'>aria-secret</span><span inert>inert-secret</span><span hidden>hidden-secret</span><span style='opacity:0'>opacity-secret</span><span style='visibility:hidden'>visibility-secret</span><input type='password' value='password-secret'><input type='file' value='/private/file-secret'><input autocomplete='cc-number' value='card-secret'><input autocomplete='one-time-code' value='otp-secret'></div>");
    replace(&mut page, "public", "done");
    assert_eq!(stream(&page).announcements[0].text, "done");
    assert!(!serde_json::to_string(&stream(&page).announcements)
        .unwrap()
        .contains("secret"));
    // aria-live=off suppresses announcements, but remains ordinary readable
    // AX content. Hidden and protected descendants must also stay out of the
    // native name corrections carried by this same overlay.
    let native = serde_json::to_string(&stream(&page)).unwrap();
    for secret in [
        "aria-secret",
        "inert-secret",
        "hidden-secret",
        "opacity-secret",
        "visibility-secret",
        "password-secret",
        "file-secret",
        "card-secret",
        "otp-secret",
    ] {
        assert!(!native.contains(secret), "Native metadata leaked {secret}");
    }
}

#[test]
fn alert_overrides_parent_politeness_and_explicit_off_disables_it() {
    let mut page = page("<div role='status'><span id='outer'>ready</span><div id='alert' role='alert'>error</div></div>");
    replace(&mut page, "alert", "failure");
    let announcements = stream(&page).announcements;
    assert_eq!(announcements.len(), 1);
    assert_eq!(announcements[0].politeness, LivePoliteness::Assertive);
    assert_eq!(announcements[0].text, "failure");
    attribute(&mut page, "alert", "aria-live", "off");
    replace(&mut page, "alert", "silenced");
    assert!(stream(&page).announcements.is_empty());
}

#[test]
fn relevant_removals_and_non_atomic_text_use_the_changed_content() {
    let mut page =
        page("<div id='log' role='log'><span id='a'>one</span><span id='b'>two</span></div>");
    replace(&mut page, "a", "new");
    assert_eq!(stream(&page).announcements[0].text, "new");
    attribute(&mut page, "log", "aria-relevant", "removals");
    replace(&mut page, "b", "");
    assert_eq!(stream(&page).announcements[0].text, "two");
}

#[test]
fn replacement_documents_drop_previous_revisions_and_text() {
    let mut page = page("<div role='status' id='status'>old</div>");
    replace(&mut page, "status", "updated");
    let old = stream(&page);
    page.load_html_str("<div role='status'>replacement</div>", None);
    let new = stream(&page);
    assert!(new.document_generation > old.document_generation);
    assert_eq!(new.revision, 0);
    assert!(new.announcements.is_empty());
}

#[test]
fn removals_do_not_reveal_content_that_became_private_or_hidden() {
    let mut page = page("<div role='log' aria-relevant='removals'><input id='input' value='old-public-value'><span id='text'>old-visible-text</span></div>");
    attribute(&mut page, "input", "type", "password");
    assert!(stream(&page).announcements.is_empty());
    attribute(&mut page, "text", "aria-hidden", "true");
    assert!(stream(&page).announcements.is_empty());
}

#[test]
fn busy_descendants_defer_the_containing_region_and_ancestor_settings_inherit() {
    let mut page = page("<div aria-atomic='true'><div aria-live='polite'>Count <span id='count'>1</span></div></div>");
    attribute(&mut page, "count", "aria-busy", "true");
    replace(&mut page, "count", "2");
    assert!(stream(&page).announcements.is_empty());
    attribute(&mut page, "count", "aria-busy", "false");
    assert_eq!(stream(&page).announcements[0].text, "Count 2");
}

#[test]
fn resource_limits_are_reported_and_unicode_is_not_split() {
    let mut page = page("<div role='status' id='status'>ready</div>");
    replace(&mut page, "status", &"😀".repeat(5000));
    let stream = stream(&page);
    assert!(stream.truncated);
    assert_eq!(stream.announcements[0].text.chars().count(), 4096);
    assert_eq!(stream.announcements[0].text.encode_utf16().count(), 8192);
}

#[test]
fn native_names_exclude_hidden_descendants_and_hidden_associated_labels() {
    let page = page("<h1>Visible <span aria-hidden='true'>hidden-name-secret</span></h1><label for='field' aria-hidden='true'>hidden-label-secret</label><input id='field' placeholder='Public placeholder'>");
    let snapshot = page.snapshot(1, 2);
    let heading = snapshot
        .nodes
        .iter()
        .find(|node| matches!(node.role, blueice_ipc::Role::Heading { .. }))
        .unwrap();
    assert!(
        heading
            .name
            .as_ref()
            .unwrap()
            .contains("hidden-name-secret"),
        "Inspection retains its existing source observability"
    );
    let names = snapshot.accessibility.unwrap().names;
    assert_eq!(
        names
            .iter()
            .find(|name| name.node_id == heading.id)
            .unwrap()
            .name
            .as_deref(),
        Some("Visible")
    );
    let field = page.script_get_element_by_id("field").unwrap();
    assert_eq!(
        names
            .iter()
            .find(|name| name.node_id == field.as_u64())
            .unwrap()
            .name
            .as_deref(),
        Some("Public placeholder")
    );
    assert!(!serde_json::to_string(&names).unwrap().contains("secret"));
}

#[test]
fn text_relevance_includes_new_text_while_additions_requires_a_new_element() {
    let mut page =
        page("<div id='log' role='log' aria-relevant='text'><span id='message'>ready</span></div>");
    replace(&mut page, "message", "changed");
    assert_eq!(stream(&page).announcements[0].text, "changed");
    attribute(&mut page, "log", "aria-relevant", "additions");
    replace(&mut page, "message", "another text replacement");
    assert!(stream(&page).announcements.is_empty());
    let child = page.script_create_element("p".into()).unwrap();
    let text = page.script_create_text_node("Added element".into());
    let child_handle = page.script_handle_for_node(child);
    let text_handle = page.script_handle_for_node(text);
    page.script_append_child(child_handle, text_handle).unwrap();
    let parent = page.script_get_element_by_id("log").unwrap();
    let parent_handle = page.script_handle_for_node(parent);
    page.script_append_child(parent_handle, child_handle)
        .unwrap();
    assert_eq!(stream(&page).announcements[0].text, "Added element");
}

#[test]
fn reveal_scrolls_without_activation_focus_or_editor_selection_changes() {
    let mut page = page("<input id='editor' value='hello'><div style='height:1000px'></div><h2 id='target'>Lower heading</h2><a id='link' href='/danger' style='display:block'>Do not follow</a><p id='hidden' aria-hidden='true'>Hidden</p>");
    let editor = page.script_get_element_by_id("editor").unwrap();
    page.act(editor, NodeAction::Focus);
    page.frame_generation = 7;
    let target = page.script_get_element_by_id("target").unwrap();
    let context = AccessibilityContext {
        version: 1,
        frame_source: 19,
        document_generation: page.document_generation,
        frame_generation: 7,
        node_id: target.as_u64(),
    };
    let bounds = page.accessibility_reveal(&context, 19).unwrap();
    assert!(bounds.y > 900.0);
    assert!(page.scroll_y() > 800.0);
    assert_eq!(page.focused, Some(editor));
    let selection = page
        .native_text_input_state(1)
        .focused
        .map(|control| control.selection);
    let link = page.script_get_element_by_id("link").unwrap();
    assert!(page
        .accessibility_reveal(
            &AccessibilityContext {
                node_id: link.as_u64(),
                ..context
            },
            19
        )
        .is_ok());
    assert_eq!(page.url(), Some("https://example.test/"));
    assert_eq!(
        page.native_text_input_state(1)
            .focused
            .map(|control| control.selection),
        selection
    );
    assert!(page
        .accessibility_reveal(
            &AccessibilityContext {
                frame_generation: 6,
                ..context
            },
            19
        )
        .is_err());
    assert!(page.accessibility_reveal(&context, 20).is_err());
    let hidden = page.script_get_element_by_id("hidden").unwrap();
    assert!(page
        .accessibility_reveal(
            &AccessibilityContext {
                node_id: hidden.as_u64(),
                ..context
            },
            19
        )
        .is_err());
    page.load_html_str("<h2>replacement</h2>", None);
    assert!(page.accessibility_reveal(&context, 19).is_err());
}
