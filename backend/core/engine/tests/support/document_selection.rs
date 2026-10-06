// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;
use blueice_ipc::accessibility::{
    AccessibilityContext, AccessibilityTextAction as Action, AccessibilityTextResult as TextResult,
};
use blueice_ipc::input::{TextInputAction, TextMovement, TextRange};

fn page(html: &str) -> Page {
    let mut page = Page::new(450.0, 220.0);
    page.load_html_str(html, Some("about:blank".into()));
    page.advance_frame_generation();
    page
}

fn apply(page: &mut Page, action: TextInputAction) {
    let context = page.native_text_input_state(17).context();
    page.native_text_input(&context, 17, action).unwrap();
}

fn selection(page: &Page) -> serde_json::Value {
    let state = serde_json::to_value(page.native_text_input_state(17)).unwrap();
    assert!(
        state["document"].is_object(),
        "Native document text support is missing"
    );
    state["document"].clone()
}

fn context(page: &Page, node: NodeId) -> AccessibilityContext {
    AccessibilityContext {
        version: 1,
        frame_source: 17,
        document_generation: page.document_generation(),
        frame_generation: page.frame_generation(),
        node_id: node.as_u64(),
    }
}

fn inspect(page: &mut Page, node: NodeId) -> blueice_ipc::accessibility::AccessibilityTextState {
    let context = context(page, node);
    let (changed, result) = page
        .accessibility_text(&context, 17, Action::Inspect)
        .unwrap();
    assert!(!changed);
    let TextResult::State(state) = result else {
        panic!("Expected text state")
    };
    *state
}

#[test]
fn select_all_copies_rendered_unicode_inline_words_and_paragraph_boundaries() {
    let mut page = page("<h1>Title</h1><p>Alpha <b>bold</b> 😀 é</p><p>Beta 中文</p>");
    apply(&mut page, TextInputAction::SelectAll);
    let state = selection(&page);
    assert_eq!(state["selected_text"], "Title\nAlpha bold 😀 é\nBeta 中文");
    assert_eq!(state["selection"]["location"], 0);
    assert_eq!(state["selection"]["length"], state["text_length"]);
    assert_eq!(state["active"], true);
    assert!(!page.render().commands.is_empty());
}

#[test]
fn controls_protected_hidden_and_inert_content_do_not_enter_document_copy() {
    let mut page = page("<p>public</p><input value='control-secret'><textarea>textarea-secret</textarea><button>button-secret</button><select><option>option-secret</option></select><p aria-hidden='true'>ax-secret</p><p hidden>hidden-secret</p><div inert>inert-secret</div><p style='display:none'>display-secret</p><p style='opacity:0'>opacity-secret</p><p>end</p>");
    apply(&mut page, TextInputAction::SelectAll);
    assert_eq!(selection(&page)["selected_text"], "public\nend");
    assert!(page.native_text_input_state(17).focused.is_none());
}

#[test]
fn paragraph_queries_are_read_only_and_preserve_the_focused_editor() {
    let mut page = page("<p id='p'>完成 😀 é</p><input id='field' value='editor'>");
    let field = page.script_get_element_by_id("field").unwrap();
    page.focus_native_editor_at(Some(field));
    let before = page.native_text_input_state(17);
    let pixels = page.render_visible();
    let p = page.script_get_element_by_id("p").unwrap();
    let state = inspect(&mut page, p);
    assert_eq!(state.text.as_deref(), Some("完成 😀 é"));
    assert_eq!(state.text_length, 8);
    assert!(!state.writable && !state.protected && !state.focused);
    assert_eq!(state.selection, None);
    assert_eq!(page.native_text_input_state(17), before);
    assert_eq!(page.render_visible(), pixels);
}

#[test]
fn paragraph_ax_selection_has_global_copy_and_local_range_without_editor_changes() {
    let mut page = page("<p>first</p><p id='p'>完成 😀 é</p><input id='field' value='editor'>");
    let field = page.script_get_element_by_id("field").unwrap();
    page.focus_native_editor_at(Some(field));
    let editor = page.native_text_input_state(17).focused;
    let p = page.script_get_element_by_id("p").unwrap();
    let context = context(&page, p);
    let (changed, result) = page
        .accessibility_text(
            &context,
            17,
            Action::Select {
                range: TextRange {
                    location: 3,
                    length: 2,
                },
            },
        )
        .unwrap();
    assert!(changed);
    let TextResult::State(state) = result else {
        panic!()
    };
    assert_eq!(
        state.selection,
        Some(TextRange {
            location: 3,
            length: 2
        })
    );
    assert_eq!(selection(&page)["selected_text"], "😀");
    assert_eq!(page.native_text_input_state(17).focused, editor);
    assert_eq!(page.focused, Some(field));
}

#[test]
fn paragraph_edits_and_invalid_or_stale_ranges_fail_before_any_mutation() {
    let mut page = page("<p id='p'>A😀B</p>");
    let p = page.script_get_element_by_id("p").unwrap();
    inspect(&mut page, p);
    let before = page.native_text_input_state(17);
    let context = context(&page, p);
    for action in [
        Action::SetValue { text: "bad".into() },
        Action::ReplaceSelection { text: "bad".into() },
        Action::Select {
            range: TextRange {
                location: 2,
                length: 1,
            },
        },
        Action::Select {
            range: TextRange {
                location: 0,
                length: u32::MAX,
            },
        },
    ] {
        assert!(page.accessibility_text(&context, 17, action).is_err());
        assert_eq!(page.native_text_input_state(17), before);
    }
    for stale in [
        AccessibilityContext {
            version: 2,
            ..context
        },
        AccessibilityContext {
            frame_source: 18,
            ..context
        },
        AccessibilityContext {
            frame_generation: context.frame_generation + 1,
            ..context
        },
        AccessibilityContext {
            document_generation: context.document_generation + 1,
            ..context
        },
    ] {
        assert!(page
            .accessibility_text(
                &stale,
                17,
                Action::Select {
                    range: TextRange {
                        location: 0,
                        length: 1
                    }
                }
            )
            .is_err());
        assert_eq!(page.native_text_input_state(17), before);
    }
}

#[test]
fn paragraph_grapheme_and_visual_line_queries_share_the_actual_layout() {
    let mut page = page("<p id='p' style='width:65px'>A😀B é final words</p>");
    let p = page.script_get_element_by_id("p").unwrap();
    let context = context(&page, p);
    let state = inspect(&mut page, p);
    assert!(state.line_count > 1);
    assert_eq!(
        page.accessibility_text(&context, 17, Action::RangeForIndex { index: 2 })
            .unwrap()
            .1,
        TextResult::Range(Some(TextRange {
            location: 1,
            length: 2
        }))
    );
    let TextResult::Bounds(Some(bounds)) = page
        .accessibility_text(
            &context,
            17,
            Action::Bounds {
                range: TextRange {
                    location: 1,
                    length: 2,
                },
            },
        )
        .unwrap()
        .1
    else {
        panic!()
    };
    assert!(bounds.width > 0.0 && bounds.height > 0.0);
    assert_eq!(
        page.accessibility_text(
            &context,
            17,
            Action::RangeForPosition {
                x: bounds.x + bounds.width / 2.0,
                y: bounds.y + bounds.height / 2.0
            }
        )
        .unwrap()
        .1,
        TextResult::Range(Some(TextRange {
            location: 1,
            length: 2
        }))
    );
}

#[test]
fn keyboard_selection_moves_by_grapheme_and_collapses_without_splitting_emoji() {
    let mut page = page("<p>A😀éZ</p>");
    apply(&mut page, TextInputAction::SelectAll);
    apply(
        &mut page,
        TextInputAction::Move {
            direction: TextMovement::Beginning,
            extend: false,
        },
    );
    apply(
        &mut page,
        TextInputAction::Move {
            direction: TextMovement::Forward,
            extend: true,
        },
    );
    apply(
        &mut page,
        TextInputAction::Move {
            direction: TextMovement::Forward,
            extend: true,
        },
    );
    assert_eq!(selection(&page)["selected_text"], "A😀");
    apply(
        &mut page,
        TextInputAction::Move {
            direction: TextMovement::Backward,
            extend: false,
        },
    );
    assert_eq!(
        selection(&page)["selection"],
        serde_json::json!({"location":0,"length":0})
    );
    apply(
        &mut page,
        TextInputAction::Move {
            direction: TextMovement::End,
            extend: true,
        },
    );
    assert_eq!(selection(&page)["selected_text"], "A😀éZ");
}

#[test]
fn double_click_selects_a_word_and_drag_extends_across_paragraphs() {
    let mut page = page("<p id='a'>Alpha beta</p><p id='b'>Gamma delta</p>");
    let a = page.script_get_element_by_id("a").unwrap();
    let b = page.script_get_element_by_id("b").unwrap();
    let a = find_fragment_bounds(&page.fragment, a, 0.0, 0.0).unwrap();
    let b = find_fragment_bounds(&page.fragment, b, 0.0, 0.0).unwrap();
    apply(
        &mut page,
        TextInputAction::Pointer {
            x: a.x + 5.0,
            y: a.y + 8.0,
            extend: false,
            click_count: 2,
        },
    );
    assert_eq!(selection(&page)["selected_text"], "Alpha");
    apply(
        &mut page,
        TextInputAction::Pointer {
            x: b.x + b.width,
            y: b.y + 8.0,
            extend: true,
            click_count: 1,
        },
    );
    assert_eq!(selection(&page)["selected_text"], "Alpha beta\nGamma delta");
}

#[test]
fn resize_and_scroll_preserve_selection_but_public_text_changes_clear_it() {
    let mut page = page("<p id='p'>Alpha beta gamma delta</p><div style='height:800px'></div>");
    apply(&mut page, TextInputAction::SelectAll);
    let selected = selection(&page)["selected_text"].clone();
    page.resize(130.0, 180.0);
    page.scroll_by(100.0);
    assert_eq!(selection(&page)["selected_text"], selected);
    let p = page.script_get_element_by_id("p").unwrap();
    let handle = page.script_handle_for_node(p);
    page.script_set_text_content(handle, "replacement".into())
        .unwrap();
    assert_eq!(selection(&page)["active"], false);
    assert!(selection(&page)["selected_text"].is_null());
}

#[test]
fn hiding_a_selected_source_purges_copy_and_document_replacement_resets_selection() {
    let mut page = page("<p id='p'>retained-public</p><p>end</p>");
    apply(&mut page, TextInputAction::SelectAll);
    let p = page.script_get_element_by_id("p").unwrap();
    let NodeData::Element { attributes, .. } = page.doc.data_mut(p) else {
        panic!()
    };
    attributes.push(("aria-hidden".into(), "true".into()));
    page.restyle_and_relayout();
    assert!(selection(&page)["selected_text"].is_null());
    assert!(!serde_json::to_string(&selection(&page))
        .unwrap()
        .contains("retained-public"));
    page.load_html_str("<p>next</p>", Some("about:next".into()));
    assert_eq!(selection(&page)["active"], false);
}

#[test]
fn focused_controls_keep_select_all_and_document_text_cannot_be_edited() {
    let mut page = page("<p>page</p><input id='field' value='editor'>");
    let field = page.script_get_element_by_id("field").unwrap();
    page.focus_native_editor_at(Some(field));
    apply(&mut page, TextInputAction::SelectAll);
    assert_eq!(
        page.native_text_input_state(17)
            .focused
            .unwrap()
            .selection
            .length,
        6
    );
    assert_eq!(selection(&page)["active"], false);
    page.focus_native_editor_at(None);
    apply(&mut page, TextInputAction::SelectAll);
    assert_eq!(selection(&page)["selected_text"], "page");
    let before = selection(&page);
    let context = page.native_text_input_state(17).context();
    assert!(page
        .native_text_input(
            &context,
            17,
            TextInputAction::Replace {
                text: "bad".into(),
                replacement: None
            }
        )
        .is_err());
    assert_eq!(selection(&page), before);
}

#[test]
fn long_documents_exceed_control_limits_and_report_collection_bounds() {
    let text = "a".repeat(70_000);
    let mut page = page(&format!("<p id='p'>{text}</p>"));
    apply(&mut page, TextInputAction::SelectAll);
    assert_eq!(selection(&page)["text_length"], 70_000);
    assert_eq!(
        selection(&page)["selected_text"].as_str().unwrap().len(),
        70_000
    );
    assert_eq!(selection(&page)["limited"], false);
    let p = page.script_get_element_by_id("p").unwrap();
    assert_eq!(inspect(&mut page, p).text_length, 70_000);
}

#[test]
fn explicit_document_commands_preserve_editor_and_reject_every_editing_action() {
    let mut page = page("<p id='p'>public 😀</p><input id='field' value='editor'>");
    let field = page.script_get_element_by_id("field").unwrap();
    page.focus_native_editor_at(Some(field));
    apply(
        &mut page,
        TextInputAction::Compose {
            text: "中".into(),
            selection: TextRange {
                location: 1,
                length: 0,
            },
            replacement: None,
        },
    );
    let editor = page.native_text_input_state(17).focused;
    apply(&mut page, TextInputAction::DocumentSelectAll);
    assert_eq!(selection(&page)["selected_text"], "public 😀");
    for action in [
        TextInputAction::Replace {
            text: "bad".into(),
            replacement: None,
        },
        TextInputAction::Compose {
            text: "bad".into(),
            selection: TextRange {
                location: 0,
                length: 0,
            },
            replacement: None,
        },
        TextInputAction::FinishComposition,
        TextInputAction::CancelComposition,
        TextInputAction::Delete { forward: false },
        TextInputAction::Undo,
        TextInputAction::Redo,
    ] {
        let context = page.native_text_input_state(17).context();
        assert!(page.native_text_input(&context, 17, action).is_err());
        assert_eq!(page.native_text_input_state(17).focused, editor);
    }
    for key in [
        blueice_ipc::input::PageKey::Enter,
        blueice_ipc::input::PageKey::Space,
    ] {
        apply(&mut page, TextInputAction::Key { key, shift: false });
        assert_eq!(page.native_text_input_state(17).focused, editor);
    }
    assert!(
        page.native_focus_at(Some(field)),
        "Returning to the same focused control must refresh selection paint"
    );
    assert!(!selection(&page)["active"].as_bool().unwrap());
}

#[test]
fn control_drag_stays_in_editor_and_explicit_document_pointer_changes_owner() {
    let mut page = page("<p id='p'>public text</p><input id='field' value='editor'>");
    let field = page.script_get_element_by_id("field").unwrap();
    page.focus_native_editor_at(Some(field));
    let p = page.script_get_element_by_id("p").unwrap();
    let bounds = find_fragment_bounds(&page.fragment, p, 0.0, 0.0).unwrap();
    apply(
        &mut page,
        TextInputAction::Pointer {
            x: bounds.x,
            y: bounds.y,
            extend: true,
            click_count: 1,
        },
    );
    assert!(!selection(&page)["active"].as_bool().unwrap());
    assert!(page.native_text_input_state(17).focused.is_some());
    apply(
        &mut page,
        TextInputAction::DocumentPointer {
            x: bounds.x,
            y: bounds.y,
            extend: false,
            click_count: 3,
        },
    );
    assert_eq!(selection(&page)["selected_text"], "public text");
    assert!(page.native_text_input_state(17).focused.is_none());
}

#[test]
fn ax_selection_is_local_to_intersecting_nodes_and_input_fences_reject_stale_owners() {
    let mut page = page("<p id='a'>Alpha</p><p id='b'>Beta</p>");
    let a = page.script_get_element_by_id("a").unwrap();
    let b = page.script_get_element_by_id("b").unwrap();
    let context = context(&page, a);
    page.accessibility_text(
        &context,
        17,
        Action::Select {
            range: TextRange {
                location: 0,
                length: 5,
            },
        },
    )
    .unwrap();
    let sibling = inspect(&mut page, b);
    assert!(!sibling.focused);
    assert!(sibling.selection.is_none() && sibling.insertion_line.is_none());
    let live = page.native_text_input_state(17).context();
    let before = page.native_text_input_state(17);
    for stale in [
        blueice_ipc::input::TextInputContext { version: 2, ..live },
        blueice_ipc::input::TextInputContext {
            frame_source: 18,
            ..live
        },
        blueice_ipc::input::TextInputContext {
            document_generation: live.document_generation + 1,
            ..live
        },
        blueice_ipc::input::TextInputContext {
            focus_generation: live.focus_generation + 1,
            ..live
        },
    ] {
        assert!(page
            .native_text_input(&stale, 17, TextInputAction::DocumentSelectAll)
            .is_err());
        assert_eq!(page.native_text_input_state(17), before);
    }
}

#[test]
fn public_context_menu_offers_document_copy_without_exposing_unrelated_editor() {
    let mut page = page("<p id='p'>public text</p><input id='field' value='editor-secret'><input id='secret' type='password' value='private-secret'>");
    let field = page.script_get_element_by_id("field").unwrap();
    page.focus_native_editor_at(Some(field));
    apply(&mut page, TextInputAction::DocumentSelectAll);
    let before = page.native_text_input_state(17);
    let p = page.script_get_element_by_id("p").unwrap();
    let bounds = find_fragment_bounds(&page.fragment, p, 0.0, 0.0).unwrap();
    assert!(!page.prepare_context_menu(bounds.x + 1.0, bounds.y + 1.0));
    let state = page.context_menu_state(17, 1, bounds.x + 1.0, bounds.y + 1.0);
    assert!(state.input.is_none());
    assert_eq!(
        state.document.unwrap().selected_text.as_deref(),
        Some("public text")
    );
    assert_eq!(page.native_text_input_state(17), before);
    let secret = page.script_get_element_by_id("secret").unwrap();
    let bounds = find_fragment_bounds(&page.fragment, secret, 0.0, 0.0).unwrap();
    let state = page.context_menu_state(17, 1, bounds.x + 1.0, bounds.y + 1.0);
    assert!(state.document.is_none());
    assert!(!serde_json::to_string(&state)
        .unwrap()
        .contains("private-secret"));
}

#[test]
fn replacing_identical_text_provenance_discards_selection() {
    let mut page = page("<p id='p'>same text</p>");
    apply(&mut page, TextInputAction::DocumentSelectAll);
    let p = page.script_get_element_by_id("p").unwrap();
    let handle = page.script_handle_for_node(p);
    page.script_set_text_content(handle, "same text".into())
        .unwrap();
    assert_eq!(selection(&page)["active"], false);
}

#[test]
fn large_unicode_documents_fit_the_wire_and_report_omitted_text() {
    let mut page = page(&format!("<p>{}</p>", "中".repeat(1_500_000)));
    apply(&mut page, TextInputAction::DocumentSelectAll);
    let state = page.native_text_input_state(17);
    let document = state.document.as_ref().unwrap();
    assert!(document.limited);
    assert!(document.text_length > 65_536 && document.text_length < 1_500_000);
    assert_eq!(
        document
            .selected_text
            .as_ref()
            .unwrap()
            .encode_utf16()
            .count(),
        document.selection.length as usize
    );
    let mut bytes = Vec::new();
    blueice_ipc::write_server_message(
        &mut bytes,
        &blueice_ipc::ServerMessage::TextInputState(state),
    )
    .unwrap();
    assert!(bytes.len() <= blueice_ipc::MAX_FRAME_BYTES);
}

#[test]
fn word_movement_skips_paragraph_and_space_separators() {
    let mut page = page("<p>Alpha beta</p><p>Gamma</p>");
    apply(&mut page, TextInputAction::DocumentSelectAll);
    apply(
        &mut page,
        TextInputAction::Move {
            direction: TextMovement::Beginning,
            extend: false,
        },
    );
    for expected in [5, 10, 16] {
        apply(
            &mut page,
            TextInputAction::Move {
                direction: TextMovement::WordForward,
                extend: false,
            },
        );
        assert_eq!(selection(&page)["selection"]["location"], expected);
    }
    for expected in [11, 6, 0] {
        apply(
            &mut page,
            TextInputAction::Move {
                direction: TextMovement::WordBackward,
                extend: false,
            },
        );
        assert_eq!(selection(&page)["selection"]["location"], expected);
    }
}

#[test]
fn document_keyboard_cannot_activate_a_preserved_control_or_submit_its_form() {
    use blueice_ipc::input::PageKey;
    let mut page = page("<p>public</p><form action='about:credits'><input id='field' value='editor'><button id='submit' type='button'>Send</button></form>");
    let field = page.script_get_element_by_id("field").unwrap();
    page.native_focus_at(Some(field));
    let enter = TextInputAction::Key {
        key: PageKey::Enter,
        shift: false,
    };
    assert!(page.native_implicit_form(&enter).is_some());
    apply(&mut page, TextInputAction::DocumentSelectAll);
    assert!(page.native_implicit_form(&enter).is_none());
    let submit = page.script_get_element_by_id("submit").unwrap();
    page.native_focus_at(Some(submit));
    assert_eq!(page.native_key_activation(&enter), Some(submit));
    apply(&mut page, TextInputAction::DocumentSelectAll);
    assert!(page.native_key_activation(&enter).is_none());
    assert!(page
        .native_key_activation(&TextInputAction::Key {
            key: PageKey::Space,
            shift: false
        })
        .is_none());
}

#[test]
fn document_selection_blocks_preserved_select_commands_before_mutation() {
    use blueice_ipc::input::PageKey;
    let mut page = page(
        "<p>public</p><select id='select'><option selected>A</option><option>B</option></select>",
    );
    let select = page.script_get_element_by_id("select").unwrap();
    page.native_focus_at(Some(select));
    apply(&mut page, TextInputAction::DocumentSelectAll);
    let before = page.native_text_input_state(17);
    let option = before.select.as_ref().unwrap().options[1].node_id;
    for action in [
        TextInputAction::SelectKey {
            key: PageKey::ArrowDown,
            extend: false,
            toggle: false,
        },
        TextInputAction::SelectOption {
            option_id: option,
            frame_generation: before.frame_generation,
            extend: false,
            toggle: false,
        },
        TextInputAction::SelectPointer {
            x: 2.0,
            y: 2.0,
            extend: false,
            toggle: false,
        },
        TextInputAction::SelectScroll {
            x: 2.0,
            y: 2.0,
            rows: 1,
        },
    ] {
        assert!(page
            .native_text_input(&before.context(), 17, action)
            .unwrap_err()
            .contains("read-only"));
        assert_eq!(page.native_text_input_state(17), before);
    }
}
