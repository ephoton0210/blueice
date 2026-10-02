// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![cfg(unix)]

use blueice_engine::script::javascript::{JavaScriptPageExecutionReport, PageJavaScriptExecutor};
use blueice_engine::session::{
    run_session, run_session_with_script_and_debugger_requests_and_inline_javascript_executor,
    CoreSessionRequests,
};
use blueice_engine::{Page, TabId, TabManager};
use blueice_ipc::input::{
    FocusDirection, PageKey, TextInputAction, TextInputContext, TextInputState,
};
use blueice_ipc::{AiSnapshot, ClientMessage, NodeAction, ServerMessage};
use std::io::Write;
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

struct Browser {
    stream: UnixStream,
    state: TextInputState,
    worker: Option<thread::JoinHandle<()>>,
    root: std::path::PathBuf,
    pixels: Vec<u8>,
}

impl Browser {
    fn new(html: &str) -> Self {
        Self::with_executor(html, None)
    }

    fn with_executor(html: &str, executor: Option<ClickExecutor>) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "blueice-keyboard-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        let mut tabs = TabManager::new(500.0, 300.0);
        let tab = tabs.default_tab();
        tabs.get_mut(tab)
            .unwrap()
            .load_html_str(html, Some("https://example.test/form".into()));
        let (mut stream, mut server) = UnixStream::pair().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let directory = root.clone();
        let worker = thread::spawn(move || {
            if let Some(mut executor) = executor {
                run_session_with_script_and_debugger_requests_and_inline_javascript_executor(
                    &mut tabs,
                    &mut server,
                    &directory,
                    &mut 0,
                    &directory.join("unavailable-gatekeeper.sock"),
                    CoreSessionRequests::default(),
                    Some(&mut executor),
                )
                .unwrap();
            } else {
                run_session(
                    &mut tabs,
                    &mut server,
                    &directory,
                    &mut 0,
                    &directory.join("unavailable-gatekeeper.sock"),
                )
                .unwrap();
            }
        });
        blueice_ipc::write_client_message(
            &mut stream,
            &ClientMessage::Hello {
                protocol_version: 2,
            },
        )
        .unwrap();
        assert!(matches!(
            blueice_ipc::read_server_message(&mut stream).unwrap(),
            ServerMessage::Hello { .. }
        ));
        blueice_ipc::write_client_message(&mut stream, &ClientMessage::GetTextInputState).unwrap();
        let state = match blueice_ipc::read_server_message(&mut stream).unwrap() {
            ServerMessage::TextInputState(state) => state,
            message => panic!("unexpected {message:?}"),
        };
        Self {
            stream,
            state,
            worker: Some(worker),
            root,
            pixels: Vec::new(),
        }
    }

    fn key(&mut self, key: PageKey, shift: bool) {
        self.input(self.context(), TextInputAction::Key { key, shift })
            .unwrap();
    }

    fn context(&self) -> TextInputContext {
        TextInputContext {
            version: self.state.version,
            frame_source: self.state.frame_source,
            document_generation: self.state.document_generation,
            focus_generation: self.state.focus_generation,
        }
    }

    fn input(&mut self, context: TextInputContext, action: TextInputAction) -> Result<(), String> {
        blueice_ipc::write_client_message_with_ids(
            &mut self.stream,
            Some(1),
            Some(72),
            &ClientMessage::TextInput { context, action },
        )
        .unwrap();
        self.stream.flush().unwrap();
        loop {
            let (tab, request, message) =
                blueice_ipc::read_server_message_with_ids(&mut self.stream).unwrap();
            assert_eq!((tab, request), (Some(1), Some(72)));
            match message {
                ServerMessage::TextInputState(state) => {
                    self.state = state;
                    return Ok(());
                }
                ServerMessage::FrameReady { shm_path, .. } => {
                    self.pixels = blueice_ipc::shm::map_frame(std::path::Path::new(&shm_path))
                        .unwrap()
                        .to_vec();
                }
                ServerMessage::Error { message } => return Err(message),
                message => panic!("unexpected {message:?}"),
            }
        }
    }

    fn interaction(&mut self, message: ClientMessage) {
        blueice_ipc::write_client_message_with_ids(&mut self.stream, Some(1), Some(82), &message)
            .unwrap();
        blueice_ipc::write_client_message_with_ids(
            &mut self.stream,
            Some(1),
            Some(83),
            &ClientMessage::GetTextInputState,
        )
        .unwrap();
        loop {
            let (tab, request, message) =
                blueice_ipc::read_server_message_with_ids(&mut self.stream).unwrap();
            assert_eq!(tab, Some(1));
            match message {
                ServerMessage::FrameReady { shm_path, .. } => {
                    assert_eq!(request, Some(82));
                    self.pixels = blueice_ipc::shm::map_frame(std::path::Path::new(&shm_path))
                        .unwrap()
                        .to_vec();
                }
                ServerMessage::TextInputState(state) => {
                    assert_eq!(request, Some(83));
                    self.state = state;
                    return;
                }
                message => panic!("unexpected {message:?}"),
            }
        }
    }

    fn click(&mut self, name: &str) {
        let snapshot = self.snapshot();
        let node = snapshot
            .nodes
            .iter()
            .find(|node| node.name.as_deref() == Some(name))
            .unwrap();
        self.interaction(ClientMessage::Click {
            x: node.bounds.x + node.bounds.width / 2.0,
            y: node.bounds.y + node.bounds.height / 2.0 - snapshot.scroll_y,
        });
    }

    fn focus(&mut self, name: &str) {
        let id = self
            .snapshot()
            .nodes
            .iter()
            .find(|node| node.name.as_deref() == Some(name))
            .unwrap()
            .id;
        self.interaction(ClientMessage::ActOn {
            id,
            action: NodeAction::Focus,
        });
    }

    fn set_value(&mut self, name: &str, value: &str) {
        let id = self
            .snapshot()
            .nodes
            .iter()
            .find(|node| node.name.as_deref() == Some(name))
            .unwrap()
            .id;
        self.interaction(ClientMessage::ActOn {
            id,
            action: NodeAction::SetValue(value.into()),
        });
    }

    fn snapshot(&mut self) -> AiSnapshot {
        blueice_ipc::write_client_message(&mut self.stream, &ClientMessage::GetRepresentation)
            .unwrap();
        match blueice_ipc::read_server_message(&mut self.stream).unwrap() {
            ServerMessage::Representation(value) => value,
            message => panic!("unexpected {message:?}"),
        }
    }

    fn focused_name(&mut self) -> String {
        self.snapshot()
            .nodes
            .into_iter()
            .find(|node| node.state.focused)
            .unwrap()
            .name
            .unwrap()
    }
}

struct ClickExecutor {
    calls: Arc<AtomicU64>,
    prevent: bool,
    replacement: Option<&'static str>,
}

impl PageJavaScriptExecutor for ClickExecutor {
    fn synchronize_and_execute(&mut self, _: &TabManager) -> std::io::Result<()> {
        Ok(())
    }

    fn dispatch_click_serving_script(
        &mut self,
        tabs: &mut TabManager,
        tab: TabId,
        _: u64,
        _: Option<&blueice_engine::script::ScriptRequestReceiver>,
    ) -> std::io::Result<Option<bool>> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        if let Some(html) = self.replacement {
            tabs.get_mut(tab).unwrap().load_html_str(html, None);
        }
        Ok(Some(self.prevent))
    }

    fn drain_reports_for_tab(&mut self, _: TabId) -> Vec<JavaScriptPageExecutionReport> {
        Vec::new()
    }
}

impl Drop for Browser {
    fn drop(&mut self) {
        let _ = self.stream.shutdown(std::net::Shutdown::Both);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn tab_order_excludes_disabled_hidden_inert_and_negative_controls_and_exits_page() {
    let mut browser = Browser::new(
        r#"
        <input aria-label='Ordinary'><input aria-label='Negative' tabindex='-1'>
        <input aria-label='Second' tabindex='2'><input aria-label='First' tabindex='1'>
        <input aria-label='Disabled' disabled><div hidden><input aria-label='Hidden'></div>
        <div inert><input aria-label='Inert'></div><input style='display:none' aria-label='Gone'>
        <fieldset disabled><legend><input aria-label='Legend'></legend><input aria-label='Fieldset'></fieldset>
        <button aria-label='Last'>Last</button>"#,
    );
    for label in ["First", "Second", "Ordinary", "Legend", "Last"] {
        browser.key(PageKey::Tab, false);
        assert_eq!(browser.focused_name(), label);
        assert!(browser.state.focused_node.is_some());
    }
    browser.key(PageKey::Tab, false);
    assert_eq!(browser.state.focus_exit, Some(FocusDirection::Forward));
    assert_eq!(browser.state.focused_node, None);
    browser.key(PageKey::Tab, true);
    assert_eq!(browser.focused_name(), "Last");
    for label in ["Legend", "Ordinary", "Second", "First"] {
        browser.key(PageKey::Tab, true);
        assert_eq!(browser.focused_name(), label);
    }
    browser.key(PageKey::Tab, true);
    assert_eq!(browser.state.focus_exit, Some(FocusDirection::Backward));
}

#[test]
fn space_toggles_checkbox_and_radio_arrows_preserve_form_owner_groups() {
    let mut browser = Browser::new(
        r#"
        <input type='checkbox' aria-label='Remember'>
        <form id='one'><input type='radio' name='choice' aria-label='Alpha' checked>
        <input type='radio' name='choice' aria-label='Beta'>
        <input type='radio' name='choice' aria-label='Disabled' disabled></form>
        <form id='two'><input type='radio' name='choice' aria-label='Other' checked></form>
        <input type='radio' form='one' name='choice' aria-label='External'>"#,
    );
    browser.key(PageKey::Tab, false);
    let before = browser.pixels.clone();
    browser.key(PageKey::Space, false);
    assert_ne!(
        before, browser.pixels,
        "Checkbox state must change visible core pixels"
    );
    assert_eq!(
        browser
            .snapshot()
            .nodes
            .iter()
            .find(|node| node.name.as_deref() == Some("Remember"))
            .unwrap()
            .state
            .checked,
        Some(true)
    );
    browser.key(PageKey::Space, false);
    browser.key(PageKey::Tab, false);
    assert_eq!(browser.focused_name(), "Alpha");
    browser.key(PageKey::ArrowRight, false);
    assert_eq!(browser.focused_name(), "Beta");
    browser.key(PageKey::ArrowRight, false);
    assert_eq!(browser.focused_name(), "External");
    let snapshot = browser.snapshot();
    for label in ["External", "Other"] {
        assert_eq!(
            snapshot
                .nodes
                .iter()
                .find(|node| node.name.as_deref() == Some(label))
                .unwrap()
                .state
                .checked,
            Some(true)
        );
    }
    for label in ["Alpha", "Beta", "Remember"] {
        assert_eq!(
            snapshot
                .nodes
                .iter()
                .find(|node| node.name.as_deref() == Some(label))
                .unwrap()
                .state
                .checked,
            Some(false)
        );
    }
}

#[test]
fn select_and_decimal_range_keys_update_core_state_and_skip_disabled_options() {
    let mut browser = Browser::new(
        r#"
        <select aria-label='Region'><option value='a'>Alpha</option><option disabled>Disabled</option>
        <optgroup disabled><option>Hidden choice</option></optgroup><option value='b'>Beta</option></select>
        <input type='range' aria-label='Level' min='0.1' max='0.9' step='0.2' value='0.3'>"#,
    );
    browser.key(PageKey::Tab, false);
    let before = browser.pixels.clone();
    browser.key(PageKey::ArrowDown, false);
    assert_ne!(
        before, browser.pixels,
        "Select state must change visible core pixels"
    );
    let state = browser.snapshot();
    assert_eq!(
        state
            .nodes
            .iter()
            .find(|node| node.name.as_deref() == Some("Region"))
            .unwrap()
            .state
            .value
            .as_deref(),
        Some("b")
    );
    browser.key(PageKey::Tab, false);
    browser.key(PageKey::ArrowRight, false);
    let state = browser.snapshot();
    assert_eq!(
        state
            .nodes
            .iter()
            .find(|node| node.name.as_deref() == Some("Level"))
            .unwrap()
            .state
            .value
            .as_deref(),
        Some("0.5")
    );
    browser.key(PageKey::End, false);
    let state = browser.snapshot();
    assert_eq!(
        state
            .nodes
            .iter()
            .find(|node| node.name.as_deref() == Some("Level"))
            .unwrap()
            .state
            .value
            .as_deref(),
        Some("0.9")
    );
}

#[test]
fn label_click_and_keyboard_space_share_checkbox_focus_and_visible_state() {
    let mut browser = Browser::new(
        "<label for='remember' role='button' aria-label='Toggle remember'>Remember</label>\
         <input id='remember' type='checkbox' aria-label='Remember'>",
    );
    browser.click("Toggle remember");
    assert_eq!(browser.focused_name(), "Remember");
    assert_eq!(
        browser.snapshot().nodes.last().unwrap().state.checked,
        Some(true)
    );
    let clicked = browser.pixels.clone();
    browser.key(PageKey::Space, false);
    assert_eq!(
        browser.snapshot().nodes.last().unwrap().state.checked,
        Some(false)
    );
    assert_ne!(clicked, browser.pixels);
}

#[test]
fn stale_keyboard_context_is_rejected_before_dispatching_any_click_listener() {
    let calls = Arc::new(AtomicU64::new(0));
    let mut browser = Browser::with_executor(
        "<input type='checkbox' aria-label='First'><input type='checkbox' aria-label='Second'>",
        Some(ClickExecutor {
            calls: calls.clone(),
            prevent: false,
            replacement: None,
        }),
    );
    browser.key(PageKey::Tab, false);
    let old = browser.context();
    browser.key(PageKey::Tab, false);
    let error = browser
        .input(
            old,
            TextInputAction::Key {
                key: PageKey::Space,
                shift: false,
            },
        )
        .unwrap_err();
    assert!(error.contains("Stale"), "{error}");
    assert_eq!(calls.load(Ordering::Relaxed), 0);
    assert!(browser
        .snapshot()
        .nodes
        .iter()
        .filter_map(|n| n.state.checked)
        .all(|checked| !checked));
    browser.key(PageKey::Space, false);
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    assert_eq!(
        browser.snapshot().nodes.last().unwrap().state.checked,
        Some(true)
    );
}

#[test]
fn cancelled_keyboard_activation_and_listener_document_replacement_suppress_defaults() {
    for replacement in [
        None,
        Some("<input type='checkbox' aria-label='Replacement'>"),
    ] {
        let calls = Arc::new(AtomicU64::new(0));
        let mut browser = Browser::with_executor(
            "<input type='checkbox' aria-label='Remember'>",
            Some(ClickExecutor {
                calls: calls.clone(),
                prevent: replacement.is_none(),
                replacement,
            }),
        );
        browser.key(PageKey::Tab, false);
        let document = browser.context().document_generation;
        browser.key(PageKey::Space, false);
        assert_eq!(calls.load(Ordering::Relaxed), 1);
        assert_eq!(
            browser.snapshot().nodes.last().unwrap().state.checked,
            Some(false)
        );
        if replacement.is_some() {
            assert_ne!(document, browser.context().document_generation);
            assert_eq!(browser.state.focused_node, None);
        } else {
            assert_eq!(browser.focused_name(), "Remember");
        }
    }
}

#[test]
fn tab_from_negative_focus_uses_document_position_and_fieldset_does_not_disable_links() {
    let mut browser = Browser::new(
        "<input aria-label='Before'><input aria-label='Negative' tabindex='-1'>\
         <fieldset disabled><a href='/next' aria-label='Enabled link'>Next</a>\
         <input aria-label='Disabled'></fieldset><input aria-label='After'>",
    );
    let id = browser
        .snapshot()
        .nodes
        .iter()
        .find(|n| n.name.as_deref() == Some("Negative"))
        .unwrap()
        .id;
    browser.interaction(ClientMessage::ActOn {
        id,
        action: NodeAction::Focus,
    });
    browser.key(PageKey::Tab, false);
    assert_eq!(browser.focused_name(), "Enabled link");
    browser.interaction(ClientMessage::ActOn {
        id,
        action: NodeAction::Focus,
    });
    browser.key(PageKey::Tab, true);
    assert_eq!(browser.focused_name(), "Before");
}

#[test]
fn multiline_default_keys_and_non_aligned_range_end_use_current_core_control() {
    let mut browser = Browser::new(
        "<textarea aria-label='Notes'></textarea>\
         <input type='range' aria-label='Steps' min='0' max='1' step='0.3' value='0.3'>",
    );
    browser.key(PageKey::Tab, false);
    browser.key(PageKey::Space, false);
    browser.key(PageKey::Enter, false);
    assert_eq!(
        browser.state.focused.as_ref().unwrap().text.as_deref(),
        Some(" \n")
    );
    browser.key(PageKey::Tab, false);
    browser.key(PageKey::End, false);
    assert_eq!(
        browser
            .snapshot()
            .nodes
            .last()
            .unwrap()
            .state
            .value
            .as_deref(),
        Some("0.9")
    );
    browser.key(PageKey::Home, false);
    assert_eq!(
        browser
            .snapshot()
            .nodes
            .last()
            .unwrap()
            .state
            .value
            .as_deref(),
        Some("0")
    );
}

#[test]
fn single_select_keys_do_not_destroy_existing_multiple_selections() {
    let mut browser = Browser::new(
        "<select multiple aria-label='Multiple'><option selected value='a'>Alpha</option>\
         <option selected value='b'>Beta</option><option value='c'>Gamma</option></select>",
    );
    browser.key(PageKey::Tab, false);
    let before = browser.snapshot().nodes;
    browser.key(PageKey::ArrowDown, false);
    assert_eq!(browser.snapshot().nodes, before);
}

#[test]
fn disabled_select_keeps_its_displayed_value_and_is_skipped_by_tab() {
    let mut browser = Browser::new(
        "<select disabled aria-label='Disabled select'><option disabled>Disabled</option>\
         <option value='a'>Alpha</option></select>\
         <fieldset disabled><select aria-label='Inherited disabled'><option value='b'>Beta</option></select></fieldset>\
         <input aria-label='Editable'>",
    );
    let snapshot = browser.snapshot();
    for (name, value) in [("Disabled select", "a"), ("Inherited disabled", "b")] {
        let node = snapshot
            .nodes
            .iter()
            .find(|node| node.name.as_deref() == Some(name))
            .unwrap();
        assert!(node.state.disabled);
        assert!(!node.state.native_focusable);
        assert_eq!(node.state.value.as_deref(), Some(value));
    }
    browser.key(PageKey::Tab, false);
    assert_eq!(browser.focused_name(), "Editable");
}

#[test]
fn static_and_interactive_paint_share_native_form_control_pixels() {
    let html = "<input type='checkbox' checked><input type='radio' checked>\
                <select><option>Alpha</option><option selected>Beta</option></select>\
                <input type='range' min='0' max='10' value='3'>";
    let mut page = Page::new(500.0, 300.0);
    page.load_html_str(html, None);
    assert_eq!(
        blueice_paint::dump_frame(&page.render()),
        blueice_paint::dump_frame(&blueice_engine::render(html, "", 500.0)),
    );
}

#[test]
fn reset_restores_original_text_checkbox_radio_select_and_range_values() {
    let mut browser = Browser::new(
        r#"
      <form><input aria-label='Text' value='start'><input aria-label='Check' type='checkbox' checked>
      <input aria-label='Alpha' type='radio' name='group' checked><input aria-label='Beta' type='radio' name='group'>
      <select aria-label='Region'><option value='a'>Alpha</option><option value='b'>Beta</option></select>
      <input aria-label='Level' type='range' value='25'><button type='reset' aria-label='Reset'>Reset</button></form>"#,
    );
    browser.focus("Text");
    browser
        .input(
            browser.context(),
            TextInputAction::Replace {
                text: "edited".into(),
                replacement: Some(blueice_ipc::input::TextRange {
                    location: 0,
                    length: 5,
                }),
            },
        )
        .unwrap();
    browser.focus("Check");
    browser.key(PageKey::Space, false);
    browser.focus("Alpha");
    browser.key(PageKey::ArrowRight, false);
    browser.focus("Region");
    browser.key(PageKey::ArrowDown, false);
    browser.focus("Level");
    browser.key(PageKey::End, false);
    browser.focus("Reset");
    browser.key(PageKey::Enter, false);
    let snapshot = browser.snapshot();
    let state = |name: &str| {
        &snapshot
            .nodes
            .iter()
            .find(|node| node.name.as_deref() == Some(name))
            .unwrap()
            .state
    };
    assert_eq!(state("Text").value.as_deref(), Some("start"));
    assert_eq!(state("Check").checked, Some(true));
    assert_eq!(state("Alpha").checked, Some(true));
    assert_eq!(state("Beta").checked, Some(false));
    assert_eq!(state("Region").value.as_deref(), Some("a"));
    assert_eq!(state("Level").value.as_deref(), Some("25"));
    assert_eq!(snapshot.url.as_deref(), Some("https://example.test/form"));
}

#[test]
fn reset_uses_explicit_form_owners_and_restores_disabled_and_readonly_values() {
    let mut browser = Browser::new(
        r#"
        <form id='first'><input aria-label='Inside' value='one'>
        <input form='second' aria-label='Reassigned' value='two'>
        <input form='missing' aria-label='Orphan' value='orphan'>
        <input aria-label='Readonly' readonly value='locked'>
        <fieldset disabled><input aria-label='Disabled' value='disabled'></fieldset>
        <button type='reset' aria-label='Reset first'>Reset first</button></form>
        <form id='second'><input aria-label='Other' value='other'></form>
        <textarea form='first' aria-label='External'>line one
line two</textarea>
        <button type='ReSeT' form='first' aria-label='Reset external'><span>Reset external</span></button>"#,
    );
    for name in [
        "Inside",
        "Reassigned",
        "Orphan",
        "Readonly",
        "Disabled",
        "Other",
    ] {
        browser.set_value(name, "changed");
    }
    browser.focus("External");
    browser
        .input(
            browser.context(),
            TextInputAction::Replace {
                text: "changed notes".into(),
                replacement: Some(blueice_ipc::input::TextRange {
                    location: 0,
                    length: 17,
                }),
            },
        )
        .unwrap();
    browser.click("Reset external");
    let snapshot = browser.snapshot();
    for (name, value) in [
        ("Inside", "one"),
        ("Readonly", "locked"),
        ("Disabled", "disabled"),
        ("Reassigned", "changed"),
        ("Orphan", "changed"),
        ("Other", "changed"),
        ("External", "line one\nline two"),
    ] {
        assert_eq!(
            snapshot
                .nodes
                .iter()
                .find(|n| n.name.as_deref() == Some(name))
                .unwrap()
                .state
                .value
                .as_deref(),
            Some(value),
            "{name}"
        );
    }
    assert_eq!(browser.focused_name(), "Reset external");
}

#[test]
fn reset_clears_marked_text_and_rejects_pre_reset_context_without_changing_document() {
    let mut browser = Browser::new("<form><textarea aria-label='Notes'>original</textarea><button type='reset' aria-label='Reset'>Reset</button></form>");
    browser.focus("Notes");
    browser
        .input(
            browser.context(),
            TextInputAction::Compose {
                text: "中文".into(),
                selection: blueice_ipc::input::TextRange {
                    location: 2,
                    length: 0,
                },
                replacement: Some(blueice_ipc::input::TextRange {
                    location: 0,
                    length: 8,
                }),
            },
        )
        .unwrap();
    assert!(browser.state.focused.as_ref().unwrap().marked.is_some());
    browser.focus("Reset");
    let old = browser.context();
    browser.key(PageKey::Enter, false);
    assert_eq!(
        browser.context().document_generation,
        old.document_generation
    );
    assert_ne!(browser.context().focus_generation, old.focus_generation);
    assert_eq!(browser.focused_name(), "Reset");
    let pixels = browser.pixels.clone();
    let error = browser
        .input(
            old,
            TextInputAction::Key {
                key: PageKey::Tab,
                shift: false,
            },
        )
        .unwrap_err();
    assert!(error.contains("Stale"), "{error}");
    assert_eq!(pixels, browser.pixels);
    browser.focus("Notes");
    assert_eq!(
        browser.state.focused.as_ref().unwrap().text.as_deref(),
        Some("original")
    );
    assert!(browser.state.focused.as_ref().unwrap().marked.is_none());
    browser
        .input(browser.context(), TextInputAction::CancelComposition)
        .unwrap();
    assert_eq!(
        browser.state.focused.as_ref().unwrap().text.as_deref(),
        Some("original")
    );
}

#[test]
fn prevented_reset_and_document_replacement_keep_click_listener_ordering() {
    for (prevent, replacement) in [(true, None), (false, Some("<form><input aria-label='Text' value='replacement'><button type='reset' aria-label='New reset'>New reset</button></form>"))] {
        let calls = Arc::new(AtomicU64::new(0));
        let mut browser = Browser::with_executor("<form><input aria-label='Text' value='original'><button type='reset' aria-label='Reset'>Reset</button></form>",
            Some(ClickExecutor { calls: calls.clone(), prevent, replacement }));
        browser.set_value("Text", "changed");
        browser.focus("Reset");
        browser.key(PageKey::Space, false);
        assert_eq!(calls.load(Ordering::Relaxed), 1);
        let snapshot = browser.snapshot();
        assert_eq!(snapshot.nodes.iter().find(|n| n.name.as_deref() == Some("Text")).unwrap().state.value.as_deref(), Some(if prevent { "changed" } else { "replacement" }));
    }
}

#[test]
fn disabled_or_unowned_reset_has_no_default_and_act_on_click_shares_native_reset() {
    let mut browser = Browser::new("<form id='f'><input aria-label='Text' value='original'><button type='reset' disabled aria-label='Disabled reset'>Disabled reset</button></form><button type='reset' form='missing' aria-label='Unowned reset'>Unowned reset</button><button type='reset' form='f' aria-label='Reset'>Reset</button>");
    browser.set_value("Text", "changed");
    for name in ["Disabled reset", "Unowned reset"] {
        browser.click(name);
    }
    assert_eq!(
        browser
            .snapshot()
            .nodes
            .iter()
            .find(|n| n.name.as_deref() == Some("Text"))
            .unwrap()
            .state
            .value
            .as_deref(),
        Some("changed")
    );
    let id = browser
        .snapshot()
        .nodes
        .iter()
        .find(|n| n.name.as_deref() == Some("Reset"))
        .unwrap()
        .id;
    browser.interaction(ClientMessage::ActOn {
        id,
        action: NodeAction::Click,
    });
    assert_eq!(
        browser
            .snapshot()
            .nodes
            .iter()
            .find(|n| n.name.as_deref() == Some("Text"))
            .unwrap()
            .state
            .value
            .as_deref(),
        Some("original")
    );
}

#[test]
fn input_reset_exposes_and_paints_its_default_and_explicit_button_captions() {
    use blueice_ipc::Role;
    use blueice_paint::PaintCommand;
    let html = "<form><input aria-label='Text' value='start'><input type='ReSeT'><input type='reset' value='Clear form'></form>";
    let rendered = blueice_engine::render(html, "", 500.0);
    for caption in ["Reset", "Clear form"] {
        assert!(
            rendered.commands.iter().any(
                |command| matches!(command, PaintCommand::Text { text, .. } if text == caption)
            ),
            "Missing painted {caption}"
        );
    }
    let mut browser = Browser::new(html);
    for name in ["Reset", "Clear form"] {
        let snapshot = browser.snapshot();
        assert_eq!(
            snapshot
                .nodes
                .iter()
                .find(|n| n.name.as_deref() == Some(name))
                .unwrap()
                .role,
            Role::Button
        );
        browser.set_value("Text", "changed");
        browser.click(name);
        assert_eq!(
            browser
                .snapshot()
                .nodes
                .iter()
                .find(|n| n.name.as_deref() == Some("Text"))
                .unwrap()
                .state
                .value
                .as_deref(),
            Some("start")
        );
    }
}

#[test]
fn reset_keeps_original_and_edited_passwords_out_of_native_observation() {
    let mut browser = Browser::new("<form><input type='password' aria-label='Secret' value='original-private'><button type='reset' aria-label='Reset'>Reset</button></form>");
    browser.focus("Secret");
    browser
        .input(
            browser.context(),
            TextInputAction::Replace {
                text: "edited-private-value".into(),
                replacement: Some(blueice_ipc::input::TextRange {
                    location: 0,
                    length: 16,
                }),
            },
        )
        .unwrap();
    assert_eq!(browser.state.focused.as_ref().unwrap().text_length, 20);
    assert!(browser.state.focused.as_ref().unwrap().text.is_none());
    browser.focus("Reset");
    browser.key(PageKey::Enter, false);
    browser.focus("Secret");
    assert_eq!(browser.state.focused.as_ref().unwrap().text_length, 16);
    assert!(browser.state.focused.as_ref().unwrap().text.is_none());
    let snapshot = browser.snapshot();
    assert!(snapshot
        .nodes
        .iter()
        .find(|n| n.name.as_deref() == Some("Secret"))
        .unwrap()
        .state
        .value
        .is_none());
    let observation = format!("{:?} {snapshot:?}", browser.state);
    assert!(!observation.contains("original-private"));
    assert!(!observation.contains("edited-private-value"));
}
