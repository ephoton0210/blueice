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
    assert_eq!(stream(&page).announcements, updated.announcements);
}

#[test]
fn delayed_native_reads_retain_updates_across_other_layouts() {
    let mut page = page("<div role='log'><span id='message'>ready</span></div>");
    replace(&mut page, "message", "first update");
    page.resize(500.0, 220.0);
    replace(&mut page, "message", "second update");
    page.resize(600.0, 240.0);
    let pending = stream(&page);
    assert_eq!(pending.revision, 2);
    assert_eq!(
        pending
            .announcements
            .iter()
            .map(|item| item.text.as_str())
            .collect::<Vec<_>>(),
        ["first update", "second update"]
    );
    assert_eq!(stream(&page), pending, "Reading is not acknowledgement");
}

#[test]
fn delayed_delivery_is_bounded_and_reports_evicted_announcements() {
    let mut page = page("<div role='log'><span id='message'>ready</span></div>");
    for index in 1..=200 {
        replace(&mut page, "message", &format!("Update {index}"));
    }
    let pending = stream(&page);
    assert_eq!(pending.revision, 200);
    assert_eq!(pending.announcements.len(), 64);
    assert_eq!(pending.announcements.first().unwrap().sequence, 137);
    assert_eq!(pending.announcements.last().unwrap().sequence, 200);
    assert!(pending.truncated);
    let delivery = blueice_ipc::accessibility::AccessibilityDelivery {
        version: 1,
        frame_source: 42,
        document_generation: pending.document_generation,
        revision: 135,
    };
    page.accessibility_acknowledge(&delivery, 42).unwrap();
    page.resize(500.0, 200.0);
    assert!(stream(&page).truncated, "Overflow survives another layout");
    page.accessibility_acknowledge(
        &blueice_ipc::accessibility::AccessibilityDelivery {
            revision: 136,
            ..delivery
        },
        42,
    )
    .unwrap();
    assert!(!stream(&page).truncated);
    assert_eq!(stream(&page).announcements.len(), 64);
}

#[test]
fn retained_updates_are_discarded_when_their_source_becomes_private() {
    let mut page = page("<div role='status'>Progress <input id='editor' value='ready'></div>");
    let editor = page.script_get_element_by_id("editor").unwrap();
    page.act(editor, NodeAction::SetValue("queued-public-value".into()));
    assert!(stream(&page).announcements[0]
        .text
        .contains("queued-public-value"));
    attribute(&mut page, "editor", "type", "password");
    assert!(stream(&page).announcements.is_empty());
    assert!(!serde_json::to_string(&stream(&page))
        .unwrap()
        .contains("queued-public-value"));
}

#[test]
fn queued_regions_are_purged_when_disabled_hidden_or_removed() {
    for (name, value) in [
        ("aria-live", "off"),
        ("aria-hidden", "true"),
        ("style", "display:none"),
    ] {
        let mut page = page("<div id='region' role='status'><span id='message'>ready</span></div>");
        replace(&mut page, "message", "queued update");
        attribute(&mut page, "region", name, value);
        assert!(stream(&page).announcements.is_empty(), "{name}={value}");
        page.resize(500.0, 200.0);
        assert!(stream(&page).announcements.is_empty());
    }
    let mut page = page("<main id='main'><div role='status' id='message'>ready</div></main>");
    replace(&mut page, "message", "queued update");
    replace(&mut page, "main", "ordinary replacement");
    assert!(stream(&page).announcements.is_empty());
}

#[test]
fn queued_text_truncation_survives_subsequent_short_updates_until_acknowledged() {
    let mut page = page("<div role='log'><span id='message'>ready</span></div>");
    replace(&mut page, "message", &"😀".repeat(5000));
    replace(&mut page, "message", "short update");
    page.resize(500.0, 200.0);
    let pending = stream(&page);
    assert_eq!(pending.announcements.len(), 2);
    assert!(
        pending.truncated,
        "A queued clipped update must still report its loss"
    );
    page.accessibility_acknowledge(
        &blueice_ipc::accessibility::AccessibilityDelivery {
            version: 1,
            frame_source: 42,
            document_generation: pending.document_generation,
            revision: 1,
        },
        42,
    )
    .unwrap();
    assert!(!stream(&page).truncated);
    assert_eq!(stream(&page).announcements[0].text, "short update");
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
    assert_eq!(stream(&page).announcements.last().unwrap().text, "two");
}

#[test]
fn acknowledgement_releases_only_observed_revisions_without_layout_changes() {
    let mut page = page("<div role='log'><span id='message'>ready</span></div>");
    replace(&mut page, "message", "first");
    replace(&mut page, "message", "second");
    let before = stream(&page);
    let delivery = blueice_ipc::accessibility::AccessibilityDelivery {
        version: 1,
        frame_source: 42,
        document_generation: before.document_generation,
        revision: 1,
    };
    let frame = page.frame_generation;
    let reply = page.accessibility_acknowledge(&delivery, 42).unwrap();
    assert_eq!(reply.revision, 1);
    let pending = stream(&page);
    assert_eq!(pending.revision, 2);
    assert_eq!(pending.acknowledged_revision, 1);
    assert_eq!(pending.announcements.len(), 1);
    assert_eq!(pending.announcements[0].text, "second");
    assert_eq!(page.frame_generation, frame);
    assert_eq!(
        page.accessibility_acknowledge(&delivery, 42).unwrap(),
        reply
    );
    assert_eq!(stream(&page), pending);
    let older = blueice_ipc::accessibility::AccessibilityDelivery {
        revision: 0,
        ..delivery
    };
    assert_eq!(
        page.accessibility_acknowledge(&older, 42).unwrap().revision,
        1
    );
    let complete = blueice_ipc::accessibility::AccessibilityDelivery {
        revision: 2,
        ..delivery
    };
    page.accessibility_acknowledge(&complete, 42).unwrap();
    assert!(stream(&page).announcements.is_empty());
}

#[test]
fn acknowledgement_rejects_foreign_future_and_replaced_document_requests() {
    let mut page = page("<div id='message' role='status'>ready</div>");
    replace(&mut page, "message", "update");
    let before = stream(&page);
    let delivery = blueice_ipc::accessibility::AccessibilityDelivery {
        version: 1,
        frame_source: 42,
        document_generation: before.document_generation,
        revision: 1,
    };
    for bad in [
        blueice_ipc::accessibility::AccessibilityDelivery {
            version: 2,
            ..delivery
        },
        blueice_ipc::accessibility::AccessibilityDelivery {
            frame_source: 43,
            ..delivery
        },
        blueice_ipc::accessibility::AccessibilityDelivery {
            document_generation: before.document_generation + 1,
            ..delivery
        },
        blueice_ipc::accessibility::AccessibilityDelivery {
            revision: 2,
            ..delivery
        },
    ] {
        assert!(page.accessibility_acknowledge(&bad, 42).is_err());
        assert_eq!(stream(&page), before);
    }
    page.load_html_str("<div role='status'>replacement</div>", None);
    assert!(page.accessibility_acknowledge(&delivery, 42).is_err());
    assert_eq!(stream(&page).acknowledged_revision, 0);
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
    page.accessibility_acknowledge(
        &blueice_ipc::accessibility::AccessibilityDelivery {
            version: 1,
            frame_source: 42,
            document_generation: stream(&page).document_generation,
            revision: stream(&page).revision,
        },
        42,
    )
    .unwrap();
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

#[test]
fn queued_sources_becoming_live_off_are_not_delivered_as_updates_or_removals() {
    let mut page =
        page("<div role='log' aria-relevant='all'><span id='message'>ready</span></div>");
    replace(&mut page, "message", "queued update");
    assert_eq!(stream(&page).announcements[0].text, "queued update ready");
    attribute(&mut page, "message", "aria-live", "off");
    assert!(stream(&page).announcements.is_empty());
    assert_eq!(
        stream(&page).revision,
        1,
        "Disabling a source must not announce its previous text as a removal"
    );
    replace(&mut page, "message", "silenced text");
    page.resize(500.0, 200.0);
    assert!(stream(&page).announcements.is_empty());
    attribute(&mut page, "message", "aria-live", "polite");
    assert!(stream(&page)
        .announcements
        .iter()
        .all(|item| !item.text.contains("queued update")));
}

#[test]
fn retained_explicit_live_region_updates_override_an_off_ancestor() {
    let mut page = page("<div aria-live='off'><div role='log' aria-live='polite'><span id='message'>ready</span></div></div>");
    replace(&mut page, "message", "explicit update");
    page.resize(500.0, 200.0);
    assert_eq!(stream(&page).announcements.len(), 1);
    assert_eq!(stream(&page).announcements[0].text, "explicit update");
}

#[test]
fn atomic_descendants_include_their_public_group_and_author_label() {
    let mut page = page("<div role='log'>Outer <span aria-atomic='true' aria-label='Score'>Count <input id='value' value='1'></span></div>");
    let value = page.script_get_element_by_id("value").unwrap();
    page.act(value, NodeAction::SetValue("2😀".into()));
    assert_eq!(stream(&page).announcements[0].text, "Score Count 2😀");
}

#[test]
fn explicit_non_atomic_descendants_stop_an_atomic_owner() {
    let mut page = page("<div role='status'>Outer <span aria-atomic='false'>Changed <input id='value' value='1'></span></div>");
    let value = page.script_get_element_by_id("value").unwrap();
    page.act(value, NodeAction::SetValue("2".into()));
    assert_eq!(stream(&page).announcements[0].text, "2");
}

#[test]
fn nearest_atomic_boundary_selects_the_containing_element_once() {
    let mut page = page("<div role='log' aria-atomic='true'>Outer <div aria-atomic='false'>Intermediate <div aria-atomic='true' aria-label='Scores'><span id='first'>1</span><span id='second'>2</span><span aria-atomic='false'><input id='exception' value='3'></span></div></div></div>");
    for (id, text) in [("first", "10"), ("second", "20")] {
        let element = page.script_get_element_by_id(id).unwrap();
        let text_node = page.doc.children(element).next().unwrap();
        let NodeData::Text { data } = page.doc.data_mut(text_node) else {
            panic!("text")
        };
        *data = text.into();
    }
    page.restyle_and_relayout();
    let batch = stream(&page);
    assert_eq!(batch.announcements.len(), 1);
    assert_eq!(batch.announcements[0].text, "Scores 10 20 3");
    let exception = page.script_get_element_by_id("exception").unwrap();
    page.act(exception, NodeAction::SetValue("4".into()));
    assert_eq!(stream(&page).announcements[1].text, "4");
}

#[test]
fn descendant_relevant_removals_override_additions_and_text() {
    let mut page = page("<div role='log'><div id='scope' aria-relevant='removals'><input id='value' value='ready'></div></div>");
    let value = page.script_get_element_by_id("value").unwrap();
    page.act(value, NodeAction::SetValue("latest".into()));
    assert!(stream(&page).announcements.is_empty());
    replace(&mut page, "scope", "");
    assert_eq!(stream(&page).announcements[0].text, "latest");
}

#[test]
fn descendant_relevant_text_overrides_all_without_old_text_replay() {
    let mut page = page("<div role='log' aria-relevant='all'><span id='message' aria-relevant='text'>ready</span></div>");
    replace(&mut page, "message", "new text");
    assert_eq!(stream(&page).announcements[0].text, "new text");
    replace(&mut page, "message", "");
    assert_eq!(stream(&page).revision, 1);
    assert_eq!(stream(&page).announcements.len(), 1);
}

#[test]
fn changing_relevant_scope_does_not_replay_suppressed_edits() {
    let mut page = page("<div role='log'><div id='scope' aria-relevant='removals'><input id='value' value='ready'></div></div>");
    let value = page.script_get_element_by_id("value").unwrap();
    page.act(value, NodeAction::SetValue("suppressed".into()));
    assert!(stream(&page).announcements.is_empty());
    attribute(&mut page, "scope", "aria-relevant", "text");
    assert!(stream(&page).announcements.is_empty());
    page.act(value, NodeAction::SetValue("fresh value".into()));
    assert_eq!(stream(&page).announcements[0].text, "fresh value");
}

#[test]
fn removal_uses_the_original_relevant_scope_through_plain_descendants() {
    let mut page = page("<div role='log' aria-relevant='all'><div aria-relevant='text'><span id='first'>ready</span></div><div aria-relevant='removals'><span id='second'><input id='value' value='other'></span></div></div>");
    replace(&mut page, "first", "");
    assert!(stream(&page).announcements.is_empty());
    let value = page.script_get_element_by_id("value").unwrap();
    page.act(value, NodeAction::SetValue("other latest".into()));
    assert!(stream(&page).announcements.is_empty());
    replace(&mut page, "second", "");
    assert_eq!(stream(&page).announcements[0].text, "other latest");
}

#[test]
fn atomic_group_content_excludes_hidden_protected_and_off_contributors() {
    let mut page = page("<div role='log'><div aria-atomic='true' aria-label='Progress'><span id='public'>ready</span><input type='password' value='password-secret'><span aria-hidden='true'>hidden-secret</span><span aria-live='off'>off-secret</span></div></div>");
    replace(&mut page, "public", "done");
    assert_eq!(stream(&page).announcements[0].text, "Progress done");
    assert!(!serde_json::to_string(&stream(&page).announcements)
        .unwrap()
        .contains("secret"));
    attribute(&mut page, "public", "aria-live", "off");
    assert!(stream(&page).announcements.is_empty());
}

#[test]
fn atomic_labels_follow_public_idrefs_without_outside_region_notifications() {
    let mut page = page("<span id='label'>Score</span><div role='log'><div aria-atomic='true' aria-labelledby='label' aria-label='Fallback'><input id='value' value='1'></div></div>");
    let value = page.script_get_element_by_id("value").unwrap();
    page.act(value, NodeAction::SetValue("2".into()));
    let first = stream(&page);
    assert_eq!(first.announcements[0].text, "Score 2");
    page.accessibility_acknowledge(
        &blueice_ipc::accessibility::AccessibilityDelivery {
            version: 1,
            frame_source: 42,
            document_generation: first.document_generation,
            revision: first.revision,
        },
        42,
    )
    .unwrap();
    replace(&mut page, "label", "Other");
    assert!(stream(&page).announcements.is_empty());
    assert_eq!(stream(&page).revision, first.revision);
    page.act(value, NodeAction::SetValue("3".into()));
    assert_eq!(stream(&page).announcements[0].text, "Other 3");
}

#[test]
fn retained_atomic_labels_are_purged_when_external_label_becomes_private() {
    let mut page = page("<span id='label'>Score</span><div role='log'><div aria-atomic='true' aria-labelledby='label'><input id='value' value='1'></div></div>");
    let value = page.script_get_element_by_id("value").unwrap();
    page.act(value, NodeAction::SetValue("2".into()));
    assert_eq!(stream(&page).announcements[0].text, "Score 2");
    attribute(&mut page, "label", "aria-hidden", "true");
    assert!(stream(&page).announcements.is_empty());
    page.resize(500.0, 200.0);
    assert!(stream(&page).announcements.is_empty());
}

#[test]
fn atomic_label_clipping_survives_later_short_labels_until_prefix_release() {
    let html = format!("<div role='log'><div id='scope' aria-atomic='true' aria-label='{}'><input id='value' value='1'></div></div>", "😀".repeat(5000));
    let mut page = page(&html);
    let value = page.script_get_element_by_id("value").unwrap();
    page.act(value, NodeAction::SetValue("2".into()));
    let first = stream(&page);
    assert_eq!(first.announcements[0].text.chars().count(), 4096);
    assert!(first.truncated);
    attribute(&mut page, "scope", "aria-label", "Short");
    let pending = stream(&page);
    assert_eq!(pending.announcements.len(), 2);
    assert_eq!(pending.announcements[1].text, "Short 2");
    assert!(pending.truncated);
    page.accessibility_acknowledge(
        &blueice_ipc::accessibility::AccessibilityDelivery {
            version: 1,
            frame_source: 42,
            document_generation: pending.document_generation,
            revision: first.revision,
        },
        42,
    )
    .unwrap();
    assert!(!stream(&page).truncated);
}

#[test]
fn busy_atomic_descendants_coalesce_with_their_latest_public_label() {
    let mut page = page("<div role='log'><div id='scope' aria-atomic='true' aria-label='Score'><input id='value' value='1'></div></div>");
    let value = page.script_get_element_by_id("value").unwrap();
    attribute(&mut page, "scope", "aria-busy", "true");
    attribute(&mut page, "scope", "aria-label", "Current label");
    page.act(value, NodeAction::SetValue("2".into()));
    page.act(value, NodeAction::SetValue("3".into()));
    assert!(stream(&page).announcements.is_empty());
    attribute(&mut page, "scope", "aria-busy", "false");
    assert_eq!(stream(&page).announcements.len(), 1);
    assert_eq!(stream(&page).announcements[0].text, "Current label 3");
}

#[test]
fn simultaneous_atomic_and_plain_updates_keep_document_order_without_duplicates() {
    let mut page = page("<div role='log'><div aria-atomic='true' aria-label='First'><input id='first' value='1'></div><input id='plain' value='2'><div aria-atomic='true' aria-label='Last'><input id='last' value='3'></div></div>");
    for (id, value) in [("first", "10"), ("plain", "20"), ("last", "30")] {
        let node = page.script_get_element_by_id(id).unwrap();
        let NodeData::Element { attributes, .. } = page.doc.data_mut(node) else {
            panic!()
        };
        attributes.retain(|(name, _)| name != "value");
        attributes.push(("value".into(), value.into()));
    }
    page.restyle_and_relayout();
    assert_eq!(stream(&page).announcements[0].text, "First 10 20 Last 30");
}

#[test]
fn an_activated_outer_group_covers_simultaneous_inner_and_non_atomic_updates_once() {
    let mut page = page("<div role='status' aria-label='Whole'><input id='outer' value='1'><div aria-atomic='true' aria-label='Inner'><input id='inner' value='2'></div><div aria-atomic='false'><input id='plain' value='3'></div></div>");
    for (id, value) in [("outer", "10"), ("inner", "20"), ("plain", "30")] {
        let node = page.script_get_element_by_id(id).unwrap();
        let NodeData::Element { attributes, .. } = page.doc.data_mut(node) else {
            panic!()
        };
        attributes.retain(|(name, _)| name != "value");
        attributes.push(("value".into(), value.into()));
    }
    page.restyle_and_relayout();
    assert_eq!(stream(&page).announcements[0].text, "Whole 10 20 30");
}

#[test]
fn atomic_group_budget_reports_omitted_groups_without_partial_updates() {
    let html = format!(
        "<div role='log'>{}</div>",
        (0..257)
            .map(|i| format!(
                "<div aria-atomic='true' aria-label='Group'><input id='n{i}' value='ready'></div>"
            ))
            .collect::<String>()
    );
    let mut page = page(&html);
    assert!(stream(&page).truncated);
    let omitted = page.script_get_element_by_id("n256").unwrap();
    assert!(page
        .act(omitted, blueice_ipc::NodeAction::SetValue("omitted".into()))
        .is_none());
    assert!(stream(&page).announcements.is_empty());
    let retained = page.script_get_element_by_id("n255").unwrap();
    assert!(page
        .act(
            retained,
            blueice_ipc::NodeAction::SetValue("included".into())
        )
        .is_none());
    assert_eq!(stream(&page).announcements[0].text, "Group included");
}

#[test]
fn native_names_share_public_idref_priority_and_skip_private_references() {
    let mut page = page("<span id='first'>First</span><span id='secret' aria-hidden='true'>private-reference</span><span id='last'>Last</span><input id='field' aria-labelledby='first secret first last absent' aria-label='Fallback' value='1'>");
    let node = page.script_get_element_by_id("field").unwrap();
    assert_eq!(
        crate::ai_snapshot::native_name(&page, node).as_deref(),
        Some("First Last")
    );
    attribute(&mut page, "first", "aria-hidden", "true");
    attribute(&mut page, "last", "aria-hidden", "true");
    assert_eq!(
        crate::ai_snapshot::native_name(&page, node).as_deref(),
        Some("Fallback")
    );
}

#[test]
fn atomic_names_allow_external_off_references_but_exclude_off_content_inside_the_region() {
    let mut page = page("<span id='external' aria-live='off'>External</span><div role='log'><div aria-atomic='true' aria-labelledby='external internal'><span id='internal' aria-live='off'>off-name-secret</span><input id='value' value='1'></div></div>");
    let value = page.script_get_element_by_id("value").unwrap();
    page.act(value, NodeAction::SetValue("2".into()));
    assert_eq!(stream(&page).announcements[0].text, "External 2");
    attribute(&mut page, "external", "aria-hidden", "true");
    assert!(stream(&page).announcements.is_empty());
}
