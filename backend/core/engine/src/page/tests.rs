// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;
use blueice_paint::PaintCommand;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;

#[test]
fn about_assistant_reads_the_shared_panel_and_refreshes_in_place() {
    use crate::assistant_page::{AssistantPanel, PanelKind};
    let panel = Arc::new(AssistantPanel::new());
    panel.set_available(true);
    let mut page = Page::new(320.0, 400.0);
    page.set_assistant_panel(Some(panel.clone()));
    assert!(page.load_built_in("about:assistant"));
    assert_eq!(page.url(), Some("about:assistant"));
    assert!(all_text(&page.render()).contains("Nothing here yet"));

    panel.push(PanelKind::Summary, None, None, Ok("a fresh summary".into()));
    assert!(page.refresh_assistant_panel());
    assert!(all_text(&page.render()).contains("a fresh summary"));
    assert_eq!(page.url(), Some("about:assistant"));
}

#[test]
fn only_a_page_showing_about_assistant_refreshes() {
    let mut page = Page::new(320.0, 400.0);
    page.load_html_str("<p>ordinary</p>", Some("https://example.com/".into()));
    assert!(!page.refresh_assistant_panel());
    assert_eq!(shown_text(&page), ["ordinary"]);
    let mut blank = Page::new(320.0, 400.0);
    assert!(!blank.refresh_assistant_panel());
}

#[test]
fn about_assistant_without_a_panel_says_it_is_not_configured() {
    let mut page = Page::new(320.0, 400.0);
    assert!(page.load_built_in("about:assistant"));
    assert!(all_text(&page.render()).contains("No local assistant is configured"));
}

#[test]
fn about_assistant_honours_the_lang_parameter() {
    let mut page = Page::new(320.0, 400.0);
    assert!(page.load_built_in("about:assistant?lang=zh-TW"));
    assert!(all_text(&page.render()).contains("助理"));
}

#[test]
fn new_page_is_blank_with_no_url() {
    let page = Page::new(320.0, 200.0);
    assert_eq!(page.url(), None);
    assert!(page.render().commands.is_empty());
}

fn shown_text(page: &Page) -> Vec<String> {
    page.render()
        .commands
        .iter()
        .filter_map(|c| match c {
            PaintCommand::Text { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn a_translated_load_paints_the_translation_and_keeps_the_original() {
    let mut page = Page::new(320.0, 200.0);
    page.load_html_translated(
        "<p>Hello</p><p>World</p>",
        Some("https://a.example/".into()),
        &["你好".into(), "世界".into()],
    );
    assert_eq!(page.url(), Some("https://a.example/"));
    assert_eq!(shown_text(&page), ["你好", "世界"]);
    assert!(page.has_translation());
    assert!(page.translation_shown());
    let node = crate::translation::translatable_texts(page.doc())[0].node;
    assert_eq!(page.original_text(node), Some("Hello"));
}

#[test]
fn translation_lands_before_layout_so_a_longer_text_reflows() {
    let mut short = Page::new(100.0, 200.0);
    short.load_html_str("<p>hi</p>", None);
    let mut long = Page::new(100.0, 200.0);
    let translation = "supercalifragilistic expialidocious wonderful ".repeat(4);
    long.load_html_translated("<p>hi</p>", None, &[translation]);
    assert!(
        shown_text(&long).len() > shown_text(&short).len(),
        "the long translation must wrap onto more lines: laid out from the translated text, not the original"
    );
}

#[test]
fn toggling_shows_the_original_and_back_and_relayouts() {
    let mut page = Page::new(320.0, 200.0);
    page.load_html_translated("<p>Hello</p>", None, &["你好".into()]);
    assert!(page.set_translation_shown(false));
    assert_eq!(shown_text(&page), ["Hello"]);
    assert!(!page.translation_shown());
    assert!(
        !page.set_translation_shown(false),
        "no change, no new frame"
    );
    assert!(page.set_translation_shown(true));
    assert_eq!(shown_text(&page), ["你好"]);
}

#[test]
fn a_wrong_length_translation_leaves_the_original_page() {
    let mut page = Page::new(320.0, 200.0);
    page.load_html_translated("<p>a</p><p>b</p>", None, &["only one".into()]);
    assert_eq!(shown_text(&page), ["a", "b"]);
    assert!(!page.has_translation());
    assert!(!page.set_translation_shown(false));
}

#[test]
fn a_new_document_discards_the_previous_translation() {
    let mut page = Page::new(320.0, 200.0);
    page.load_html_translated("<p>Hello</p>", None, &["你好".into()]);
    page.load_html_str("<p>Fresh</p>", None);
    assert!(!page.has_translation());
    assert_eq!(shown_text(&page), ["Fresh"]);
}

#[test]
fn an_untranslated_page_has_no_originals() {
    let mut page = Page::new(320.0, 200.0);
    page.load_html_str("<p>Hello</p>", None);
    let node = crate::translation::translatable_texts(page.doc())[0].node;
    assert_eq!(page.original_text(node), None);
    assert!(!page.translation_shown());
}

#[test]
fn load_html_str_sets_url_and_renders_content() {
    let mut page = Page::new(320.0, 200.0);
    page.load_html_str("<p>hi</p>", Some("about:blank".to_string()));
    assert_eq!(page.url(), Some("about:blank"));
    assert!(page
        .render()
        .commands
        .iter()
        .any(|c| matches!(c, PaintCommand::Text { text, .. } if text == "hi")));
}

#[test]
fn page_exposes_only_explicit_blue_ts_script_declarations() {
    let mut page = Page::new(320.0, 200.0);
    page.load_html_str(
        r#"
                <script>const javascript = true;</script>
                <script type="application/x-blueice-typescript">const typed: number = 42;</script>
                <script type="application/x-blueice-typescript-module" src="/module.ts"></script>
            "#,
        Some("https://example.test/".to_string()),
    );

    assert_eq!(
        page.blue_ts_script_declarations(),
        vec![
            crate::script::BlueTsPageScriptDeclaration::Inline {
                ordinal: 0,
                kind: crate::script::direct_page::DirectPageScriptKind::Classic,
                source: "const typed: number = 42;".to_string(),
            },
            crate::script::BlueTsPageScriptDeclaration::External {
                ordinal: 1,
                kind: crate::script::direct_page::DirectPageScriptKind::Module,
                src: "/module.ts".to_string(),
            },
        ]
    );
}

#[test]
fn page_exposes_standard_javascript_declarations_separately_from_bluets() {
    let mut page = Page::new(320.0, 200.0);
    page.load_html_str(
        r#"
                <script>const javascript = true;</script>
                <script type="module">export const module = true;</script>
                <script type="application/x-blueice-typescript">const typed: number = 42;</script>
            "#,
        Some("https://example.test/".to_string()),
    );

    assert_eq!(
        page.blue_js_script_declarations(),
        vec![
            crate::script::BlueJsPageScriptDeclaration::Inline {
                ordinal: 0,
                kind: crate::script::BlueJsPageScriptKind::Classic,
                source: "const javascript = true;".to_string(),
            },
            crate::script::BlueJsPageScriptDeclaration::Inline {
                ordinal: 1,
                kind: crate::script::BlueJsPageScriptKind::Module,
                source: "export const module = true;".to_string(),
            },
        ]
    );
}

#[test]
fn navigate_to_about_credits_loads_the_built_in_credits_page_without_network() {
    let mut page = Page::new(320.0, 200.0);
    page.navigate("about:credits").unwrap();
    assert_eq!(page.url(), Some("about:credits"));
    let text = all_text(&page.render());
    assert!(
        text.contains("Chromium"),
        "must reproduce the Chromium BSD-3-Clause notice: {text}"
    );
    assert!(text.contains("Gecko"), "must credit Gecko: {text}");
    assert!(
        text.contains("DejaVu"),
        "must credit the bundled DejaVu font: {text}"
    );
}

#[test]
fn navigate_to_about_credits_with_a_lang_parameter_loads_the_localized_credits_page() {
    let mut page = Page::new(320.0, 200.0);
    page.navigate("about:credits?lang=zh-TW").unwrap();
    assert_eq!(page.url(), Some("about:credits?lang=zh-TW"));
    let text = all_text(&page.render());
    assert!(
        text.contains("關於"),
        "must render the localized page: {text}"
    );
}

#[test]
fn navigating_never_reuses_a_nodeid_from_the_previous_document() {
    // Regression: `NodeIdAllocator` used to live on `Document`
    // itself, restarting at 0 every time `load_html` built a fresh
    // `Document` -- so a client that cached a NodeId before this
    // navigation and acted on it afterward could get silently
    // redirected to whatever unrelated node the recycled ID now
    // happened to belong to, instead of a safe "doesn't exist"
    // (plan §1's stable-ID-across-mutations requirement).
    let mut page = Page::new(320.0, 200.0);
    page.load_html_str("<p>first</p>", None);
    let stale_id = page.doc().root();
    let first_next_id = page.doc().next_node_id();

    page.load_html_str("<p>second</p>", None);

    assert!(
        !page.doc().contains(stale_id),
        "a NodeId real in the previous document must not resolve to anything in the new one"
    );
    assert!(
        page.doc().next_node_id() >= first_next_id,
        "the new document's allocator must continue from where the old one left off, not restart at 0"
    );
}

fn find_by_tag(doc: &Document, root: NodeId, tag: &str) -> Option<NodeId> {
    if let NodeData::Element { tag_name, .. } = doc.data(root) {
        if tag_name == tag {
            return Some(root);
        }
    }
    doc.children(root).find_map(|c| find_by_tag(doc, c, tag))
}

#[cfg(unix)]
fn find_by_attribute(doc: &Document, root: NodeId, name: &str, value: &str) -> Option<NodeId> {
    if element_attribute(doc, root, name) == Some(value) {
        return Some(root);
    }
    doc.children(root)
        .find_map(|child| find_by_attribute(doc, child, name, value))
}

#[cfg(unix)]
#[test]
fn about_settings_shows_and_applies_the_running_gatekeepers_additive_policy() {
    use blueice_ai_gatekeeper::GatekeeperService;
    use std::os::unix::net::UnixListener;
    use std::sync::Arc;
    use std::thread;

    let socket =
        blueice_ipc::local_socket::default_socket_dir().join(format!("gks-{}", std::process::id()));
    let _ = std::fs::remove_file(&socket);
    let listener = UnixListener::bind(&socket).unwrap();
    listener.set_nonblocking(true).unwrap();
    let service = Arc::new(GatekeeperService::new(None).unwrap());
    let worker = thread::spawn({
        let service = service.clone();
        move || {
            // Ten full settings-page renders can exceed ten seconds in a
            // cold, single-test run even though every socket exchange is
            // bounded separately by the production 300 ms client timeout.
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
            let mut handled = 0;
            while handled < 10 && std::time::Instant::now() < deadline {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        // The listener is nonblocking only so this worker can
                        // honor its deadline. On macOS an accepted stream may
                        // inherit that mode and race the client's first write.
                        stream.set_nonblocking(false).unwrap();
                        service.handle_connection(&mut stream).unwrap();
                        handled += 1;
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(std::time::Duration::from_millis(5));
                    }
                    Err(error) => panic!("settings test listener failed: {error}"),
                }
            }
            assert_eq!(
                handled, 10,
                "the page must read and apply every control through the real service"
            );
        }
    });

    let mut page = Page::new(640.0, 480.0);
    page.set_gatekeeper_settings_source(Some(Arc::new(GatekeeperSettingsSource::without_default(
        &socket,
    ))));
    page.navigate("about:settings?lang=en").unwrap();
    assert!(page.dom_dump().contains("known-malicious-domain"));
    assert!(page
        .dom_dump()
        .contains("extension-visible-text-social-engineering"));
    assert!(page
        .dom_dump()
        .contains("Exact host or dot-boundary subdomain match"));
    assert!(page.dom_dump().contains("If rejected or unavailable"));
    assert!(page.dom_dump().contains("Active review order"));
    assert!(page
        .dom_dump()
        .contains("Compiled deterministic rule base (required)"));
    assert!(page.dom_dump().contains("Compiled rules at this step"));
    let model_name =
        find_element_by_id(page.doc(), page.doc().root(), "gatekeeper-model-name").unwrap();
    page.act(model_name, NodeAction::SetValue("local-model".to_string()));
    let configure_model = find_by_attribute(
        page.doc(),
        page.doc().root(),
        "data-gatekeeper-action",
        "configure-model",
    )
    .unwrap();
    assert_eq!(
        page.gatekeeper_settings_change_for(configure_model),
        Some(GatekeeperSettingsChange::ConfigureLocalModel {
            provider: "ollama".to_string(),
            base_url: "http://127.0.0.1:11434/v1/".to_string(),
            model: "local-model".to_string(),
        })
    );
    let input =
        find_element_by_id(page.doc(), page.doc().root(), "gatekeeper-custom-host").unwrap();
    let add = find_by_attribute(
        page.doc(),
        page.doc().root(),
        "data-gatekeeper-action",
        "add-host",
    )
    .unwrap();
    page.act(input, NodeAction::SetValue("tracker.example".to_string()));
    assert!(page.gatekeeper_settings.is_some());
    assert_eq!(
        page.gatekeeper_settings_change_for(add),
        Some(GatekeeperSettingsChange::AddBlockedHost {
            host: "tracker.example".to_string()
        })
    );
    assert_eq!(page.apply_gatekeeper_settings_control(add), Some(Ok(())));
    assert!(page.dom_dump().contains("tracker.example"));
    assert!(page.dom_dump().contains("Your blocked hosts"));
    assert!(page.dom_dump().contains("Gatekeeper settings saved"));
    let phrase_input =
        find_element_by_id(page.doc(), page.doc().root(), "gatekeeper-custom-phrase").unwrap();
    let add_phrase = find_by_attribute(
        page.doc(),
        page.doc().root(),
        "data-gatekeeper-action",
        "add-phrase",
    )
    .unwrap();
    page.act(
        phrase_input,
        NodeAction::SetValue("Private Code".to_string()),
    );
    assert_eq!(
        page.gatekeeper_settings_change_for(add_phrase),
        Some(GatekeeperSettingsChange::AddBlockedPhrase {
            phrase: "Private Code".to_string(),
        })
    );
    assert_eq!(
        page.apply_gatekeeper_settings_control(add_phrase),
        Some(Ok(()))
    );
    assert!(page.dom_dump().contains("private code"));
    let remove_phrase = find_by_attribute(
        page.doc(),
        page.doc().root(),
        "data-gatekeeper-action",
        "remove-phrase",
    )
    .unwrap();
    assert_eq!(
        page.apply_gatekeeper_settings_control(remove_phrase),
        Some(Ok(()))
    );
    assert!(service.settings().custom_blocked_phrases.is_empty());
    let extension_input =
        find_element_by_id(page.doc(), page.doc().root(), "gatekeeper-custom-extension").unwrap();
    let add_extension = find_by_attribute(
        page.doc(),
        page.doc().root(),
        "data-gatekeeper-action",
        "add-extension",
    )
    .unwrap();
    page.act(extension_input, NodeAction::SetValue(".zip".to_string()));
    assert_eq!(
        page.apply_gatekeeper_settings_control(add_extension),
        Some(Ok(()))
    );
    assert!(page.dom_dump().contains(".zip"));
    let remove_extension = find_by_attribute(
        page.doc(),
        page.doc().root(),
        "data-gatekeeper-action",
        "remove-extension",
    )
    .unwrap();
    assert_eq!(
        page.apply_gatekeeper_settings_control(remove_extension),
        Some(Ok(()))
    );
    assert!(service
        .settings()
        .custom_blocked_download_extensions
        .is_empty());
    let popup_input = find_element_by_id(
        page.doc(),
        page.doc().root(),
        "gatekeeper-custom-popup-phrase",
    )
    .unwrap();
    let add_popup = find_by_attribute(
        page.doc(),
        page.doc().root(),
        "data-gatekeeper-action",
        "add-popup-phrase",
    )
    .unwrap();
    page.act(
        popup_input,
        NodeAction::SetValue("send secrets".to_string()),
    );
    assert_eq!(
        page.apply_gatekeeper_settings_control(add_popup),
        Some(Ok(()))
    );
    assert!(page.dom_dump().contains("send secrets"));
    let remove_popup = find_by_attribute(
        page.doc(),
        page.doc().root(),
        "data-gatekeeper-action",
        "remove-popup-phrase",
    )
    .unwrap();
    assert_eq!(
        page.apply_gatekeeper_settings_control(remove_popup),
        Some(Ok(()))
    );
    assert!(service.settings().custom_blocked_popup_phrases.is_empty());
    let model_name =
        find_element_by_id(page.doc(), page.doc().root(), "gatekeeper-model-name").unwrap();
    page.act(model_name, NodeAction::SetValue("local-model".to_string()));
    let configure_model = find_by_attribute(
        page.doc(),
        page.doc().root(),
        "data-gatekeeper-action",
        "configure-model",
    )
    .unwrap();
    assert_eq!(
        page.apply_gatekeeper_settings_control(configure_model),
        Some(Ok(()))
    );
    assert!(service.settings().model_review_active);
    assert!(page
        .dom_dump()
        .contains("Optional local model (blocks if unavailable)"));
    let disable_model = find_by_attribute(
        page.doc(),
        page.doc().root(),
        "data-gatekeeper-action",
        "disable-model",
    )
    .unwrap();
    assert_eq!(
        page.apply_gatekeeper_settings_control(disable_model),
        Some(Ok(()))
    );
    assert!(!service.settings().model_review_active);
    worker.join().unwrap();
    let _ = std::fs::remove_file(socket);
}

#[test]
fn act_set_value_updates_the_value_attribute_and_the_painted_frame() {
    let mut page = Page::new(320.0, 200.0);
    page.load_html_str(r#"<input type="text">"#, None);
    let input_id = find_by_tag(page.doc(), page.doc().root(), "input").unwrap();

    page.act(input_id, NodeAction::SetValue("hello".to_string()));

    let NodeData::Element { attributes, .. } = page.doc().data(input_id) else {
        panic!("expected an element")
    };
    assert!(attributes.contains(&("value".to_string(), "hello".to_string())));
    let frame = page.render();
    assert!(
        frame
            .commands
            .iter()
            .any(|command| matches!(command, PaintCommand::Rect { rect, .. } if rect.width > 0.0 && rect.height > 0.0)),
        "the post-action frame must retain the visible input control box"
    );
    assert!(
        frame.commands.iter().any(|command| {
            matches!(command, PaintCommand::Text { text, .. } if text == "hello")
        }),
        "the post-action frame must visibly contain the core-owned value"
    );
    let snapshot = page.snapshot(1, 1);
    assert_eq!(snapshot.nodes[0].state.value.as_deref(), Some("hello"));
}

#[test]
fn focused_text_entry_is_limited_to_the_enabled_clicked_text_input() {
    let mut page = Page::new(320.0, 200.0);
    page.load_html_str(
        r#"<input id="host" type="text"><input id="locked" disabled><input id="secret" type="password">"#,
        None,
    );
    let host = page.script_get_element_by_id("host").unwrap();
    let locked = page.script_get_element_by_id("locked").unwrap();
    let secret = page.script_get_element_by_id("secret").unwrap();

    assert!(page.focus_text_input_at(Some(host)));
    assert!(page.insert_focused_text("tracker.example"));
    assert!(page.delete_focused_text_backward());
    assert_eq!(
        element_attribute(page.doc(), host, "value"),
        Some("tracker.exampl")
    );

    assert!(page.focus_text_input_at(Some(locked)));
    assert!(!page.insert_focused_text("must not write"));
    assert_eq!(element_attribute(page.doc(), locked, "value"), None);

    assert!(!page.focus_text_input_at(Some(secret)));
    assert!(!page.insert_focused_text("must not write"));
    assert_eq!(element_attribute(page.doc(), secret, "value"), None);
}

#[test]
fn extension_text_input_write_only_accepts_live_supported_text_inputs() {
    let mut page = Page::new(320.0, 200.0);
    page.load_html_str(
        r#"<input id="text"><input id="password" type="password"><div id="other"></div>"#,
        None,
    );
    let text = page.script_get_element_by_id("text").unwrap();
    let password = page.script_get_element_by_id("password").unwrap();
    let other = page.script_get_element_by_id("other").unwrap();

    page.set_text_input_value(text, "from extension".to_string())
        .unwrap();
    assert_eq!(
        page.snapshot(1, 1)
            .nodes
            .iter()
            .find(|node| node.id == text.as_u64())
            .and_then(|node| node.state.value.as_deref()),
        Some("from extension")
    );
    assert!(page
        .set_text_input_value(password, "must not write".to_string())
        .is_err());
    assert!(page
        .set_text_input_value(other, "must not write".to_string())
        .is_err());
    assert!(page
        .set_text_input_value(NodeId::from_u64(9_999), "stale".to_string())
        .is_err());
}

#[test]
fn extension_textarea_write_only_changes_live_enabled_native_textareas() {
    let mut page = Page::new(320.0, 200.0);
    page.load_html_str(
        r#"<textarea id="notes">before</textarea><textarea id="disabled" disabled>locked</textarea><input id="other">"#,
        None,
    );
    let textarea = page.script_get_element_by_id("notes").unwrap();
    let disabled = page.script_get_element_by_id("disabled").unwrap();
    let other = page.script_get_element_by_id("other").unwrap();

    page.set_textarea_value(textarea, "after\nwith detail".to_string())
        .unwrap();
    assert_eq!(
        page.snapshot(1, 1)
            .nodes
            .iter()
            .find(|node| node.id == textarea.as_u64())
            .and_then(|node| node.state.value.as_deref()),
        Some("after with detail")
    );
    assert!(page
        .set_textarea_value(disabled, "must not write".to_string())
        .is_err());
    assert!(page
        .set_textarea_value(other, "must not write".to_string())
        .is_err());
    assert!(page
        .set_textarea_value(NodeId::from_u64(9_999), "stale".to_string())
        .is_err());
}

#[test]
fn extension_visible_leaf_write_preserves_nested_content_and_rejects_hidden_or_builtin_pages() {
    let mut page = Page::new(320.0, 200.0);
    page.load_html_str(
        r#"<h1 id="title">Before</h1><p id="nested">Before <a href="/next">link</a></p><p id="hidden" hidden>Secret</p><p id="aria" aria-hidden="TRUE">Secret</p><p id="named" aria-label="Other">Secret</p><p id="none" style="display:none">Secret</p><input id="field" value="old">"#,
        Some("https://example.test/".to_string()),
    );
    let title = page.script_get_element_by_id("title").unwrap();
    let original_text = page.doc.children(title).next().unwrap();
    page.set_visible_leaf_text(title, "After".to_string())
        .unwrap();
    assert_eq!(page.doc.children(title).next(), Some(original_text));
    assert_eq!(
        page.snapshot(1, 1)
            .nodes
            .iter()
            .find(|node| node.id == title.as_u64())
            .and_then(|node| node.name.as_deref()),
        Some("After")
    );
    for id in ["nested", "hidden", "aria", "named", "none", "field"] {
        let node = page.script_get_element_by_id(id).unwrap();
        assert!(
            page.set_visible_leaf_text(node, "changed".to_string())
                .is_err(),
            "{id} must not be writable"
        );
    }
    assert!(page
        .set_visible_leaf_text(
            title,
            "x".repeat(blueice_ipc::extension::MAX_VISIBLE_LEAF_TEXT_BYTES + 1)
        )
        .is_err());
    assert!(page
        .set_visible_leaf_text(title, "  \n  ".to_string())
        .is_err());
    page.navigate("about:credits").unwrap();
    assert!(page
        .set_visible_leaf_text(title, "changed".to_string())
        .is_err());
}

#[test]
fn extension_visible_text_content_replaces_only_noninteractive_inline_markup() {
    let mut page = Page::new(320.0, 200.0);
    page.load_html_str(
        r#"<p id="formatted">Before <strong>bold <em>and italic</em></strong></p><p id="linked">Before <a href="/next">link</a></p><p id="event">Before <span onclick="go()">event</span></p><p id="named">Before <span id="target">named child</span></p><p id="hidden-child">Before <span style="display:none">secret</span></p><p id="hidden" hidden>Secret</p><input id="field" value="old">"#,
        Some("https://example.test/page".to_string()),
    );
    let formatted = page.script_get_element_by_id("formatted").unwrap();
    let old_child = page.doc.children(formatted).next().unwrap();
    page.set_visible_text_content(formatted, "After".to_string())
        .unwrap();
    assert!(!page.doc.contains(old_child));
    assert_eq!(page.doc.children(formatted).count(), 1);
    assert_eq!(
        page.snapshot(1, 1)
            .nodes
            .iter()
            .find(|node| node.id == formatted.as_u64())
            .and_then(|node| node.name.as_deref()),
        Some("After")
    );
    for id in [
        "linked",
        "event",
        "named",
        "hidden-child",
        "hidden",
        "field",
    ] {
        let node = page.script_get_element_by_id(id).unwrap();
        assert!(
            page.set_visible_text_content(node, "must not change".to_string())
                .is_err(),
            "{id} must not be writable"
        );
    }
    assert!(page
        .set_visible_text_content(
            formatted,
            "x".repeat(blueice_ipc::extension::MAX_VISIBLE_LEAF_TEXT_BYTES + 1)
        )
        .is_err());
    page.navigate("about:blank").unwrap();
    assert!(page
        .set_visible_text_content(formatted, "must not change".to_string())
        .is_err());
}

#[test]
fn extension_range_write_only_changes_live_enabled_integer_ranges() {
    let mut page = Page::new(320.0, 200.0);
    page.load_html_str(
        r#"
                <input id="volume" type="range" min="-5" max="5" step="2" value="-5">
                <input id="default" type="range">
                <input id="disabled" type="range" disabled>
                <input id="fractional" type="range" min="0.5" max="2">
                <input id="any" type="range" step="any">
                <input id="wide" type="range" min="-1" max="9223372036854775807">
                <input id="text" type="text">
            "#,
        None,
    );
    let volume = page.script_get_element_by_id("volume").unwrap();
    let default = page.script_get_element_by_id("default").unwrap();
    let disabled = page.script_get_element_by_id("disabled").unwrap();
    let fractional = page.script_get_element_by_id("fractional").unwrap();
    let any = page.script_get_element_by_id("any").unwrap();
    let wide = page.script_get_element_by_id("wide").unwrap();
    let text = page.script_get_element_by_id("text").unwrap();

    page.set_range_input_value(volume, 3).unwrap();
    page.set_range_input_value(default, 42).unwrap();
    page.set_range_input_value(wide, i64::MAX).unwrap();
    let snapshot = page.snapshot(1, 1);
    let value = |id: NodeId| {
        snapshot
            .nodes
            .iter()
            .find(|node| node.id == id.as_u64())
            .and_then(|node| node.state.value.as_deref())
    };
    assert_eq!(value(volume), Some("3"));
    assert_eq!(value(default), Some("42"));
    assert_eq!(value(wide), Some("9223372036854775807"));

    assert!(page.set_range_input_value(volume, 2).is_err());
    assert!(page.set_range_input_value(volume, 7).is_err());
    assert!(page.set_range_input_value(disabled, 1).is_err());
    assert!(page.set_range_input_value(fractional, 1).is_err());
    assert!(page.set_range_input_value(any, 1).is_err());
    assert!(page.set_range_input_value(text, 1).is_err());
    assert!(page
        .set_range_input_value(NodeId::from_u64(9_999), 1)
        .is_err());
}

#[test]
fn extension_checkbox_write_only_changes_live_enabled_native_checkboxes() {
    let mut page = Page::new(320.0, 200.0);
    page.load_html_str(
        r#"<input id="check" type="checkbox"><input id="disabled" type="checkbox" disabled><input id="radio" type="radio"><div id="other"></div>"#,
        None,
    );
    let checkbox = page.script_get_element_by_id("check").unwrap();
    let disabled = page.script_get_element_by_id("disabled").unwrap();
    let radio = page.script_get_element_by_id("radio").unwrap();
    let other = page.script_get_element_by_id("other").unwrap();

    page.set_checkbox_checked(checkbox, true).unwrap();
    assert_eq!(
        page.snapshot(1, 1)
            .nodes
            .iter()
            .find(|node| node.id == checkbox.as_u64())
            .and_then(|node| node.state.checked),
        Some(true)
    );
    page.set_checkbox_checked(checkbox, false).unwrap();
    assert_eq!(
        page.snapshot(2, 1)
            .nodes
            .iter()
            .find(|node| node.id == checkbox.as_u64())
            .and_then(|node| node.state.checked),
        Some(false)
    );
    assert!(page.set_checkbox_checked(disabled, true).is_err());
    assert!(page.set_checkbox_checked(radio, true).is_err());
    assert!(page.set_checkbox_checked(other, true).is_err());
    assert!(page
        .set_checkbox_checked(NodeId::from_u64(9_999), true)
        .is_err());
}

#[test]
fn extension_radio_write_selects_only_its_live_named_local_group() {
    let mut page = Page::new(320.0, 200.0);
    page.load_html_str(
        r#"
                <form id="one">
                  <input id="first" type="radio" name="choice" checked>
                  <input id="second" type="radio" name="choice">
                  <input id="other-name" type="radio" name="other" checked>
                  <input id="disabled" type="radio" name="choice" disabled>
                </form>
                <form id="two"><input id="other-form" type="radio" name="choice" checked></form>
                <input id="unnamed" type="radio">
                <input id="external" type="radio" name="choice" form="one">
                <div id="other"></div>
            "#,
        None,
    );
    let first = page.script_get_element_by_id("first").unwrap();
    let second = page.script_get_element_by_id("second").unwrap();
    let other_name = page.script_get_element_by_id("other-name").unwrap();
    let disabled = page.script_get_element_by_id("disabled").unwrap();
    let other_form = page.script_get_element_by_id("other-form").unwrap();
    let unnamed = page.script_get_element_by_id("unnamed").unwrap();
    let external = page.script_get_element_by_id("external").unwrap();
    let other = page.script_get_element_by_id("other").unwrap();

    page.set_radio_checked(second).unwrap();
    let snapshot = page.snapshot(1, 1);
    let checked = |id: NodeId| {
        snapshot
            .nodes
            .iter()
            .find(|node| node.id == id.as_u64())
            .and_then(|node| node.state.checked)
    };
    assert_eq!(checked(first), Some(false));
    assert_eq!(checked(second), Some(true));
    assert_eq!(checked(other_name), Some(true));
    assert_eq!(checked(other_form), Some(true));

    assert!(page.set_radio_checked(disabled).is_err());
    assert!(page.set_radio_checked(unnamed).is_err());
    assert!(page.set_radio_checked(external).is_err());
    assert!(page.set_radio_checked(other).is_err());
    assert!(page.set_radio_checked(NodeId::from_u64(9_999)).is_err());
}

#[test]
fn extension_select_option_selects_only_an_enabled_live_single_select_choice() {
    let mut page = Page::new(320.0, 200.0);
    page.load_html_str(
        r#"
                <label for="priority">Priority</label>
                <select id="priority">
                  <option id="first" selected>First</option>
                  <option id="second">Second</option>
                  <option id="disabled" disabled>Disabled</option>
                  <optgroup label="Locked" disabled><option id="locked">Locked</option></optgroup>
                </select>
                <select id="multiple" multiple><option id="many">Many</option></select>
                <select id="disabled-select" disabled><option id="disabled-owner">Disabled owner</option></select>
                <div id="other"></div>
            "#,
        None,
    );
    let first = page.script_get_element_by_id("first").unwrap();
    let second = page.script_get_element_by_id("second").unwrap();
    let disabled = page.script_get_element_by_id("disabled").unwrap();
    let locked = page.script_get_element_by_id("locked").unwrap();
    let many = page.script_get_element_by_id("many").unwrap();
    let disabled_owner = page.script_get_element_by_id("disabled-owner").unwrap();
    let other = page.script_get_element_by_id("other").unwrap();

    page.select_option(second).unwrap();
    let snapshot = page.snapshot(1, 1);
    let selected = |id: NodeId| {
        snapshot
            .nodes
            .iter()
            .find(|node| node.id == id.as_u64())
            .map(|node| node.state.selected)
    };
    assert_eq!(selected(first), Some(false));
    assert_eq!(selected(second), Some(true));

    assert!(page.select_option(disabled).is_err());
    assert!(page.select_option(locked).is_err());
    assert!(page.select_option(many).is_err());
    assert!(page.select_option(disabled_owner).is_err());
    assert!(page.select_option(other).is_err());
    assert!(page.select_option(NodeId::from_u64(9_999)).is_err());
}

#[test]
fn navigate_to_about_blank_loads_an_empty_page_without_network() {
    let mut page = Page::new(320.0, 200.0);
    page.navigate("about:blank").unwrap();
    assert_eq!(page.url(), Some("about:blank"));
    assert!(page.render().commands.is_empty());
}

#[test]
fn dom_dump_matches_blueice_doms_own_dump_of_the_same_document() {
    let mut page = Page::new(320.0, 200.0);
    page.load_html_str(r#"<div class="x"><p>hi</p></div>"#, None);
    assert_eq!(page.dom_dump(), blueice_dom::dump(page.doc()));
    assert!(page.dom_dump().contains("<div>"));
}

#[test]
fn dom_dump_includes_nodes_the_ai_snapshot_would_exclude() {
    // a bare <div> has no semantic role, so Page::snapshot excludes
    // it entirely -- dom_dump must not apply that filter.
    let mut page = Page::new(320.0, 200.0);
    page.load_html_str(r#"<div style="background-color: red;">x</div>"#, None);
    assert!(page.dom_dump().contains("<div>"));
    assert!(page.snapshot(0, 1).nodes.is_empty());
}

#[test]
fn script_appended_element_receives_author_style_before_rasterization() {
    let mut page = Page::new(320.0, 200.0);
    page.load_html_str(
        "<style>span { display: block; width: 100px; height: 30px; background-color: red; }</style><div id='target'></div>",
        None,
    );
    let mut static_page = Page::new(320.0, 200.0);
    static_page.load_html_str(
        "<style>span { display: block; width: 100px; height: 30px; background-color: red; }</style><div id='target'><span></span></div>",
        None,
    );
    assert_eq!(
        static_page.render_visible().get_pixel(0, 0),
        [255, 0, 0, 255]
    );
    let baseline = page.render_visible();
    let parent = page.script_get_element_by_id("target").unwrap();
    let child = page.script_create_element("span".to_string()).unwrap();
    let parent = page.script_handle_for_node(parent);
    let child = page.script_handle_for_node(child);
    page.script_append_child(parent, child).unwrap();
    let appended = page.render_visible();
    assert_ne!(baseline.get_pixel(0, 0), appended.get_pixel(0, 0));
    assert_eq!(appended.get_pixel(0, 0), [255, 0, 0, 255]);
}

fn all_text(frame: &Frame) -> String {
    frame
        .commands
        .iter()
        .filter_map(|c| match c {
            PaintCommand::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[test]
fn navigate_fetches_over_the_network_and_loads_the_body() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = [0u8; 1024];
        let _ = stream.read(&mut buf);
        let body = "<p>fetched</p>";
        stream
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            )
            .unwrap();
    });
    let mut page = Page::new(320.0, 200.0);
    page.navigate(&format!("http://{addr}")).unwrap();
    let response = page
        .network_response()
        .expect("a network page has response metadata");
    assert_eq!(response.method, "GET");
    assert_eq!(response.status, 200);
    assert_eq!(response.final_url, format!("http://{addr}"));
    assert!(page
        .render()
        .commands
        .iter()
        .any(|c| matches!(c, PaintCommand::Text { text, .. } if text == "fetched")));
    page.navigate("about:blank").unwrap();
    assert!(page.network_response().is_none());
}

fn distinct_line_count(frame: &Frame) -> usize {
    let mut ys: Vec<i64> = frame
        .commands
        .iter()
        .filter_map(|c| match c {
            PaintCommand::Text { y, .. } => Some(y.round() as i64),
            _ => None,
        })
        .collect();
    ys.sort_unstable();
    ys.dedup();
    ys.len()
}

#[test]
fn resize_relayouts_at_the_new_width() {
    let mut page = Page::new(1000.0, 200.0);
    page.load_html_str("<p>aaaa bbbb cccc dddd eeee</p>", None);
    let wide_lines = distinct_line_count(&page.render());
    page.resize(50.0, 200.0);
    let narrow_lines = distinct_line_count(&page.render());
    assert!(
        narrow_lines > wide_lines,
        "a much narrower viewport must wrap onto more lines"
    );
}

#[test]
fn scroll_clamps_to_the_content_range() {
    let mut page = Page::new(100.0, 20.0);
    page.load_html_str("<div style=\"height: 500px;\"></div>", None);
    page.scroll_by(-100.0);
    assert_eq!(page.scroll_y(), 0.0, "cannot scroll above the top");
    page.scroll_by(10_000.0);
    assert!(
        page.scroll_y() > 0.0 && page.scroll_y() <= 500.0,
        "cannot scroll past the bottom of the content"
    );
}

#[test]
fn click_on_a_link_returns_its_href() {
    let mut page = Page::new(320.0, 200.0);
    page.load_html_str(r#"<a href="https://example.com/next">click me</a>"#, None);
    // the link is the only content, at the top-left of the page
    assert_eq!(
        page.click(2.0, 2.0),
        Some("https://example.com/next".to_string())
    );
}

#[test]
fn click_on_nested_content_inside_a_link_still_finds_the_href() {
    let mut page = Page::new(320.0, 200.0);
    page.load_html_str(r#"<a href="/x"><b>bold link text</b></a>"#, None);
    assert_eq!(page.click(2.0, 2.0), Some("/x".to_string()));
}

#[test]
fn clicking_a_relative_link_resolves_it_against_the_current_page_url() {
    let mut page = Page::new(320.0, 200.0);
    page.load_html_str(
        r#"<a href="../next?q=blue#section">next</a>"#,
        Some("https://example.com/guide/start/index.html?old=query".to_string()),
    );

    assert_eq!(
        page.click(2.0, 2.0),
        Some("https://example.com/guide/next?q=blue#section".to_string())
    );
}

#[test]
fn acting_on_a_relative_link_uses_the_same_resolution_as_a_pointer_click() {
    let mut page = Page::new(320.0, 200.0);
    page.load_html_str(
        r#"<a href="/downloads/file.zip">file</a>"#,
        Some("https://example.com/guide/start".to_string()),
    );
    let link_id = page.snapshot(0, 1).nodes[0].id;

    assert_eq!(
        page.act(NodeId::from_u64(link_id), NodeAction::Click),
        Some("https://example.com/downloads/file.zip".to_string())
    );
}

#[test]
fn click_outside_any_link_returns_none() {
    let mut page = Page::new(320.0, 200.0);
    page.load_html_str("<p>no links here</p>", None);
    assert_eq!(page.click(2.0, 2.0), None);
}

#[test]
fn click_past_the_end_of_the_content_returns_none() {
    let mut page = Page::new(320.0, 200.0);
    page.load_html_str(r#"<a href="/x">hi</a>"#, None);
    assert_eq!(page.click(300.0, 190.0), None);
}

#[test]
fn render_visible_crops_to_the_viewport_size() {
    let mut page = Page::new(50.0, 30.0);
    page.load_html_str(
        "<div style=\"height: 500px; background-color: red;\"></div>",
        None,
    );
    let visible = page.render_visible();
    assert_eq!(visible.width, 50);
    assert_eq!(visible.height, 30);
}

#[test]
fn render_visible_shows_content_at_the_current_scroll_offset() {
    let mut page = Page::new(20.0, 10.0);
    page.load_html_str(
        "<div style=\"height: 10px; background-color: red;\"></div><div style=\"height: 10px; background-color: blue;\"></div>",
        None,
    );
    let top = page.render_visible();
    assert_eq!(
        top.get_pixel(0, 0),
        [255, 0, 0, 255],
        "scrolled to top, red div is visible"
    );

    page.scroll_by(10.0);
    let scrolled = page.render_visible();
    assert_eq!(
        scrolled.get_pixel(0, 0),
        [0, 0, 255, 255],
        "scrolled down 10px, blue div is now visible"
    );
}

// ---- about:downloads ------------------------------------------------

#[cfg(unix)]
use crate::downloads_page::test_support::{fake_downloads, Scratch};
#[cfg(unix)]
use crate::downloads_page::DownloadsSource;
#[cfg(unix)]
use blueice_ipc::downloads::{TransferInfo, TransferState};
use std::sync::Arc;

#[cfg(unix)]
fn transfer(id: u64, name: &str, state: TransferState) -> TransferInfo {
    TransferInfo {
        id,
        url: format!("https://example.com/{name}"),
        dest_path: format!("/d/{name}"),
        state,
        total_bytes: Some(1000),
        completed_bytes: 400,
        ..TransferInfo::default()
    }
}

#[cfg(unix)]
fn page_reading(socket: std::path::PathBuf) -> Page {
    let mut page = Page::new(400.0, 300.0);
    page.set_downloads_source(Some(Arc::new(DownloadsSource::without_spawner(socket))));
    page
}

#[cfg(unix)]
#[test]
fn navigating_to_about_downloads_renders_the_live_list_without_a_network_fetch() {
    let dir = Scratch::new("page-live");
    let _server = fake_downloads(
        &dir.socket(),
        vec![
            transfer(1, "alpha.iso", TransferState::Active),
            transfer(2, "beta.zip", TransferState::Completed),
        ],
        false,
        blueice_ipc::downloads::DOWNLOADS_PROTOCOL_VERSION,
    );
    let mut page = page_reading(dir.socket());

    page.navigate("about:downloads")
        .expect("a built-in page never fails to navigate");
    assert_eq!(page.url(), Some("about:downloads"));
    let dump = page.dom_dump();
    assert!(
        dump.contains("alpha.iso") && dump.contains("beta.zip"),
        "{dump}"
    );
    assert!(
        dump.contains("Downloading") && dump.contains("Completed"),
        "{dump}"
    );
}

#[cfg(unix)]
#[test]
fn about_downloads_says_the_service_is_not_running_when_there_is_no_source_or_it_is_unreachable() {
    let mut without = Page::new(400.0, 300.0);
    without.navigate("about:downloads").unwrap();
    assert!(
        without
            .dom_dump()
            .contains("The downloads service is not running"),
        "{}",
        without.dom_dump()
    );

    let dir = Scratch::new("page-dead");
    let mut unreachable = page_reading(dir.socket()); // nothing listens there
    unreachable.navigate("about:downloads").unwrap();
    assert!(unreachable
        .dom_dump()
        .contains("The downloads service is not running"));
    assert!(
        !unreachable.dom_dump().contains("No downloads yet."),
        "unreachable is not the same as empty"
    );
}

#[cfg(unix)]
#[test]
fn about_downloads_honors_a_lang_parameter() {
    let dir = Scratch::new("page-lang");
    let _server = fake_downloads(
        &dir.socket(),
        vec![transfer(1, "alpha.iso", TransferState::Active)],
        false,
        blueice_ipc::downloads::DOWNLOADS_PROTOCOL_VERSION,
    );
    let mut page = page_reading(dir.socket());
    page.navigate("about:downloads?lang=zh-TW").unwrap();
    assert_eq!(page.url(), Some("about:downloads?lang=zh-TW"));
    assert!(page.dom_dump().contains("下載中"), "{}", page.dom_dump());
}

#[test]
fn refreshing_keeps_the_scroll_position_a_navigation_would_reset() {
    let long: String = (0..60).map(|i| format!("<p>line {i}</p>")).collect();
    let mut page = Page::new(400.0, 100.0);
    page.load_html_str(&long, Some("about:downloads".to_string()));
    page.scroll_by(200.0);
    assert_eq!(page.scroll_y(), 200.0);

    page.refresh_html(&long);
    assert_eq!(
        page.scroll_y(),
        200.0,
        "a live refresh must not throw a reader back to the top"
    );
    page.load_html_str(&long, None);
    assert_eq!(
        page.scroll_y(),
        0.0,
        "whereas a real navigation does start at the top"
    );
}

#[test]
fn refreshing_to_shorter_content_clamps_the_scroll_to_what_still_exists() {
    let long: String = (0..60).map(|i| format!("<p>line {i}</p>")).collect();
    let mut page = Page::new(400.0, 100.0);
    page.load_html_str(&long, Some("about:downloads".to_string()));
    page.scroll_by(500.0);
    let before = page.scroll_y();
    assert!(before > 100.0);
    page.refresh_html("<p>just one line</p>");
    assert_eq!(page.scroll_y(), 0.0, "nothing left to scroll to");
}
