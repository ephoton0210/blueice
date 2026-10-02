// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;
use blueice_ipc::input::{TextInputAction, TextInputContext, TextMovement, TextRange};

fn editor(html: &str) -> (Page, NodeId) {
    let mut page = Page::new(500.0, 300.0);
    page.load_html_str(html, Some("about:blank".to_string()));
    let id = page.script_get_element_by_id("field").unwrap();
    page.focus_native_editor_at(Some(id));
    (page, id)
}

fn context(page: &Page) -> TextInputContext {
    TextInputContext {
        version: 1,
        frame_source: 17,
        document_generation: page.document_generation(),
        focus_generation: page.native_focus_generation,
    }
}

fn apply(page: &mut Page, action: TextInputAction) {
    let context = context(page);
    page.native_text_input(&context, 17, action).unwrap();
}

fn range(location: u32, length: u32) -> TextRange {
    TextRange { location, length }
}

#[test]
fn textarea_public_value_preserves_whitespace_and_masked_values_remain_absent() {
    let (page, id) = editor("<textarea id='field'>中文\nשלום  next\n</textarea><input id='secret' type='password' value='hidden'>");
    assert_eq!(
        page.native_control_public_value(id).as_deref(),
        Some("中文\nשלום  next\n")
    );
    let snapshot = crate::ai_snapshot::build(&page, 0, 1);
    assert!(snapshot
        .nodes
        .iter()
        .any(|node| node.state.value.as_deref() == Some("中文\nשלום  next\n")));
    let secret = page.script_get_element_by_id("secret").unwrap();
    assert_eq!(page.native_control_public_value(secret), None);
}

#[test]
fn scrolled_editor_and_marked_paint_clip_to_the_resolved_content_box() {
    let (mut page, _) = editor("<input id='field' value='long long long long' style='width:90px;padding:6px;border:2px solid black'>");
    apply(
        &mut page,
        TextInputAction::Compose {
            text: "中文".into(),
            selection: range(2, 0),
            replacement: None,
        },
    );
    let geometry = geometry::Geometry::new(
        &page,
        &page.live_editor().unwrap().0,
        page.live_editor().unwrap().1,
    )
    .unwrap();
    assert!(geometry.clip.x > geometry.bounds.x);
    assert!(geometry.clip.width < geometry.bounds.width);
    assert!(
        geometry.caret(page.live_editor().unwrap().0.cursor).x
            < geometry.clip.x + geometry.clip.width
    );
    let frame = page.render();
    assert!(frame
        .commands
        .iter()
        .any(|command| matches!(command, PaintCommand::PushClip { rect }
        if rect.x == geometry.clip.x && rect.width == geometry.clip.width)));
    assert!(frame.commands.iter().any(
        |command| matches!(command, PaintCommand::Rect { rect, color: Color::Rgba(25,25,25,255) }
        if rect.height == 1.0)
    ));
}

#[test]
fn queued_edits_cannot_follow_focus_into_a_different_control() {
    let (mut page, first) =
        editor(r#"<input id="field" value="first"><input id="next" value="next">"#);
    let old = context(&page);
    let next = page.script_get_element_by_id("next").unwrap();
    page.focus_native_editor_at(Some(next));
    assert!(page
        .native_text_input(
            &old,
            17,
            TextInputAction::Replace {
                text: "stale".into(),
                replacement: None,
            }
        )
        .is_err());
    assert_eq!(element_attribute(page.doc(), first, "value"), Some("first"));
    assert_eq!(element_attribute(page.doc(), next, "value"), Some("next"));
}

#[test]
fn replacement_uses_core_selection_and_utf16_offsets() {
    let (mut page, id) = editor(r#"<input id="field" value="A😀B">"#);
    apply(&mut page, TextInputAction::Select { range: range(1, 2) });
    apply(
        &mut page,
        TextInputAction::Replace {
            text: "中文".into(),
            replacement: None,
        },
    );
    assert_eq!(element_attribute(page.doc(), id, "value"), Some("A中文B"));
    let state = page.native_text_input_state(17);
    assert_eq!(state.focused.unwrap().selection, range(3, 0));
    assert!(page
        .render()
        .commands
        .iter()
        .any(|command| matches!(command,
        PaintCommand::Text { text, .. } if text == "A中文B")));
}

#[test]
fn marked_updates_replace_previous_composition_and_cancel_restores_original_selection() {
    let (mut page, id) = editor(r#"<input id="field" value="left right">"#);
    apply(&mut page, TextInputAction::Select { range: range(5, 5) });
    apply(
        &mut page,
        TextInputAction::Compose {
            text: "中".into(),
            selection: range(1, 0),
            replacement: None,
        },
    );
    assert_eq!(element_attribute(page.doc(), id, "value"), Some("left 中"));
    apply(
        &mut page,
        TextInputAction::Compose {
            text: "中文".into(),
            selection: range(2, 0),
            replacement: None,
        },
    );
    assert_eq!(
        element_attribute(page.doc(), id, "value"),
        Some("left 中文")
    );
    assert_eq!(
        page.native_text_input_state(17).focused.unwrap().marked,
        Some(range(5, 2))
    );
    apply(&mut page, TextInputAction::CancelComposition);
    assert_eq!(
        element_attribute(page.doc(), id, "value"),
        Some("left right")
    );
    let state = page.native_text_input_state(17).focused.unwrap();
    assert_eq!(state.selection, range(5, 5));
    assert_eq!(state.marked, None);
}

#[test]
fn committing_marked_text_is_one_replacement_including_rtl_text() {
    let (mut page, id) = editor(r#"<input id="field" value="AB">"#);
    apply(&mut page, TextInputAction::Select { range: range(1, 0) });
    apply(
        &mut page,
        TextInputAction::Compose {
            text: "abc".into(),
            selection: range(3, 0),
            replacement: None,
        },
    );
    apply(
        &mut page,
        TextInputAction::Replace {
            text: "שלום".into(),
            replacement: None,
        },
    );
    assert_eq!(element_attribute(page.doc(), id, "value"), Some("AשלוםB"));
    assert_eq!(
        page.native_text_input_state(17).focused.unwrap().marked,
        None
    );
}

#[test]
fn grapheme_movement_and_deletion_do_not_split_combining_or_zwj_sequences() {
    let (mut page, id) = editor(r#"<input id="field" value="Aé👩‍👩‍👧‍👦">"#);
    apply(&mut page, TextInputAction::Delete { forward: false });
    assert_eq!(element_attribute(page.doc(), id, "value"), Some("Aé"));
    apply(
        &mut page,
        TextInputAction::Move {
            direction: TextMovement::Backward,
            extend: true,
        },
    );
    assert_eq!(
        page.native_text_input_state(17).focused.unwrap().selection,
        range(1, 2)
    );
    apply(&mut page, TextInputAction::Delete { forward: false });
    assert_eq!(element_attribute(page.doc(), id, "value"), Some("A"));
}

#[test]
fn invalid_surrogate_ranges_and_stale_contexts_do_not_mutate_document() {
    let (mut page, id) = editor(r#"<input id="field" value="A😀B">"#);
    let ctx = context(&page);
    assert!(page
        .native_text_input(&ctx, 17, TextInputAction::Select { range: range(2, 0) })
        .is_err());
    assert!(page
        .native_text_input(
            &ctx,
            18,
            TextInputAction::Replace {
                text: "bad".into(),
                replacement: None
            }
        )
        .is_err());
    assert_eq!(element_attribute(page.doc(), id, "value"), Some("A😀B"));
    page.load_html_str(r#"<input id="field" value="new">"#, None);
    let next = page.script_get_element_by_id("field").unwrap();
    page.focus_native_editor_at(Some(next));
    assert!(page
        .native_text_input(
            &ctx,
            17,
            TextInputAction::Replace {
                text: "bad".into(),
                replacement: None
            }
        )
        .is_err());
    assert_eq!(element_attribute(page.doc(), next, "value"), Some("new"));
}

#[test]
fn protected_inputs_edit_and_render_masks_without_inspection_or_extension_disclosure() {
    let (mut page, id) = editor(r#"<input id="field" type="password">"#);
    apply(
        &mut page,
        TextInputAction::Replace {
            text: "private-password".into(),
            replacement: None,
        },
    );
    assert_eq!(
        element_attribute(page.doc(), id, "value"),
        Some("private-password")
    );
    let state = page.native_text_input_state(17).focused.unwrap();
    assert!(state.protected);
    assert_eq!(state.text, None);
    assert_eq!(page.snapshot(1, 1).nodes[0].state.value, None);
    assert!(page.set_text_input_value(id, "extension".into()).is_err());
    let frame = page.render();
    assert!(!frame.commands.iter().any(|command| matches!(command, PaintCommand::Text { text, .. } if text.contains("private-password"))));
    assert!(frame
        .commands
        .iter()
        .any(|command| matches!(command, PaintCommand::Text { text, .. } if text.contains('•'))));
}

#[test]
fn textarea_edits_preserve_line_breaks_and_emit_multiline_geometry() {
    let (mut page, id) = editor(r#"<textarea id="field">first</textarea>"#);
    apply(
        &mut page,
        TextInputAction::Replace {
            text: "\n中文\nשלום".into(),
            replacement: None,
        },
    );
    assert_eq!(node_text_content(page.doc(), id), "first\n中文\nשלום");
    let state = page.native_text_input_state(17).focused.unwrap();
    assert!(state.multiline);
    assert!(state.carets.first().unwrap().bounds.y < state.caret.y);
    assert!(state.caret.width > 0.0 && state.caret.height > 0.0);
}

#[test]
fn readonly_is_selectable_but_not_writable_and_disabled_cannot_focus() {
    let (mut page, id) = editor(r#"<input id="field" readonly value="copyable">"#);
    let state = page.native_text_input_state(17).focused.unwrap();
    assert!(!state.writable);
    apply(&mut page, TextInputAction::Select { range: range(0, 8) });
    let ctx = context(&page);
    assert!(page
        .native_text_input(
            &ctx,
            17,
            TextInputAction::Replace {
                text: "bad".into(),
                replacement: None
            }
        )
        .is_err());
    assert_eq!(element_attribute(page.doc(), id, "value"), Some("copyable"));
    let (page, _) = editor(r#"<input id="field" disabled value="locked">"#);
    assert!(page.native_text_input_state(17).focused.is_none());
}

#[test]
fn focus_and_navigation_end_composition_without_reusing_old_editor_state() {
    let (mut page, id) = editor(r#"<input id="field" value="A"><input id="next" value="B">"#);
    apply(
        &mut page,
        TextInputAction::Compose {
            text: "中".into(),
            selection: range(1, 0),
            replacement: None,
        },
    );
    let next = page.script_get_element_by_id("next").unwrap();
    page.focus_native_editor_at(Some(next));
    assert_eq!(element_attribute(page.doc(), id, "value"), Some("A中"));
    let state = page.native_text_input_state(17).focused.unwrap();
    assert_eq!(state.node_id, next.as_u64());
    assert_eq!(state.text.as_deref(), Some("B"));
    assert_eq!(state.marked, None);
    page.load_html_str("<p>new document</p>", None);
    assert!(page.native_text_input_state(17).focused.is_none());
}
