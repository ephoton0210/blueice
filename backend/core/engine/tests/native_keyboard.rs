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
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

struct Browser {
    stream: UnixStream,
    state: TextInputState,
    worker: Option<thread::JoinHandle<()>>,
    root: std::path::PathBuf,
    pixels: Vec<u8>,
}

#[test]
fn undo_redo_restore_unicode_selection_pixels_and_independent_control_histories() {
    let mut browser = Browser::new("<input value='A😀B'><input value='next'>");
    browser.key(PageKey::Tab, false);
    browser
        .input(browser.context(), TextInputAction::SelectAll)
        .unwrap();
    let before = browser.pixels.clone();
    browser
        .input(
            browser.context(),
            TextInputAction::Replace {
                text: "中文👨‍👩‍👧‍👦".into(),
                replacement: None,
            },
        )
        .unwrap();
    assert!(browser.state.focused.as_ref().unwrap().can_undo);
    browser
        .input(browser.context(), TextInputAction::Undo)
        .unwrap();
    let field = browser.state.focused.as_ref().unwrap();
    assert_eq!(field.text.as_deref(), Some("A😀B"));
    assert_eq!((field.selection.location, field.selection.length), (0, 4));
    assert!(field.can_redo);
    assert_eq!(browser.pixels, before);
    browser
        .input(browser.context(), TextInputAction::Redo)
        .unwrap();
    assert_eq!(
        browser.state.focused.as_ref().unwrap().text.as_deref(),
        Some("中文👨‍👩‍👧‍👦")
    );
    let old = browser.context();
    browser.key(PageKey::Tab, false);
    browser
        .input(
            browser.context(),
            TextInputAction::Replace {
                text: "!".into(),
                replacement: None,
            },
        )
        .unwrap();
    assert!(browser.input(old, TextInputAction::Undo).is_err());
    assert_eq!(
        browser.state.focused.as_ref().unwrap().text.as_deref(),
        Some("next!")
    );
    browser.key(PageKey::Tab, true);
    browser
        .input(browser.context(), TextInputAction::Undo)
        .unwrap();
    assert_eq!(
        browser.state.focused.as_ref().unwrap().text.as_deref(),
        Some("A😀B")
    );
}

#[test]
fn undo_groups_typing_and_committed_composition_but_drops_redo_after_a_new_edit() {
    use blueice_ipc::input::TextRange;
    let mut browser = Browser::new("<textarea>old</textarea>");
    browser.key(PageKey::Tab, false);
    for text in ["a", "b", "c"] {
        browser
            .input(
                browser.context(),
                TextInputAction::Replace {
                    text: text.into(),
                    replacement: None,
                },
            )
            .unwrap();
    }
    browser
        .input(browser.context(), TextInputAction::Undo)
        .unwrap();
    assert_eq!(
        browser.state.focused.as_ref().unwrap().text.as_deref(),
        Some("old")
    );
    browser
        .input(browser.context(), TextInputAction::SelectAll)
        .unwrap();
    for text in ["中", "中文"] {
        browser
            .input(
                browser.context(),
                TextInputAction::Compose {
                    text: text.into(),
                    selection: TextRange {
                        location: text.encode_utf16().count() as u32,
                        length: 0,
                    },
                    replacement: None,
                },
            )
            .unwrap();
    }
    browser
        .input(browser.context(), TextInputAction::FinishComposition)
        .unwrap();
    assert!(!browser.state.focused.as_ref().unwrap().can_redo);
    browser
        .input(browser.context(), TextInputAction::Undo)
        .unwrap();
    assert_eq!(
        browser.state.focused.as_ref().unwrap().text.as_deref(),
        Some("old")
    );
    browser
        .input(browser.context(), TextInputAction::Redo)
        .unwrap();
    assert_eq!(
        browser.state.focused.as_ref().unwrap().text.as_deref(),
        Some("中文")
    );
    browser
        .input(browser.context(), TextInputAction::Undo)
        .unwrap();
    browser
        .input(
            browser.context(),
            TextInputAction::Replace {
                text: "new branch".into(),
                replacement: None,
            },
        )
        .unwrap();
    assert!(!browser.state.focused.as_ref().unwrap().can_redo);
    browser
        .input(browser.context(), TextInputAction::Redo)
        .unwrap();
    assert_eq!(
        browser.state.focused.as_ref().unwrap().text.as_deref(),
        Some("new branch")
    );
}

#[test]
fn undo_password_state_never_discloses_old_or_new_plaintext() {
    let mut browser = Browser::new("<input type='password' value='old-secret'>");
    browser.key(PageKey::Tab, false);
    browser
        .input(browser.context(), TextInputAction::SelectAll)
        .unwrap();
    browser
        .input(
            browser.context(),
            TextInputAction::Replace {
                text: "new-private-password".into(),
                replacement: None,
            },
        )
        .unwrap();
    for action in [TextInputAction::Undo, TextInputAction::Redo] {
        browser.input(browser.context(), action).unwrap();
        let field = browser.state.focused.as_ref().unwrap();
        assert!(field.protected);
        assert_eq!(field.text, None);
        let state = serde_json::to_string(&browser.state).unwrap();
        assert!(!state.contains("old-secret") && !state.contains("new-private-password"));
        assert!(browser
            .snapshot()
            .nodes
            .iter()
            .all(|node| !format!("{node:?}").contains("secret")));
    }
    assert_eq!(browser.state.focused.as_ref().unwrap().text_length, 20);
}

#[test]
fn undo_external_same_value_write_and_form_reset_remove_history() {
    let mut browser = Browser::new(
        "<form><input aria-label='Editor' value='start'><button type='reset'>Reset</button></form>",
    );
    browser.key(PageKey::Tab, false);
    browser
        .input(
            browser.context(),
            TextInputAction::Replace {
                text: "!".into(),
                replacement: None,
            },
        )
        .unwrap();
    let node = browser
        .snapshot()
        .nodes
        .iter()
        .find(|n| n.name.as_deref() == Some("Editor"))
        .unwrap()
        .id;
    browser.interaction(ClientMessage::ActOn {
        id: node,
        action: NodeAction::SetValue("start!".into()),
    });
    assert!(!browser.state.focused.as_ref().unwrap().can_undo);
    browser
        .input(
            browser.context(),
            TextInputAction::Replace {
                text: "new".into(),
                replacement: None,
            },
        )
        .unwrap();
    browser.key(PageKey::Tab, false);
    browser.key(PageKey::Enter, false);
    browser.key(PageKey::Tab, true);
    assert_eq!(
        browser.state.focused.as_ref().unwrap().text.as_deref(),
        Some("start")
    );
    assert!(!browser.state.focused.as_ref().unwrap().can_undo);
}

#[test]
fn undo_bounded_history_retains_recent_atomic_edits_and_reports_eviction() {
    use blueice_ipc::input::TextRange;
    let mut browser = Browser::new("<input value='initial'>");
    browser.key(PageKey::Tab, false);
    for i in 0..140 {
        let size = browser.state.focused.as_ref().unwrap().text_length;
        browser
            .input(
                browser.context(),
                TextInputAction::Replace {
                    text: format!("value {i}"),
                    replacement: Some(TextRange {
                        location: 0,
                        length: size,
                    }),
                },
            )
            .unwrap();
    }
    assert!(browser.state.focused.as_ref().unwrap().undo_limited);
    let mut undos = 0;
    while browser.state.focused.as_ref().unwrap().can_undo {
        browser
            .input(browser.context(), TextInputAction::Undo)
            .unwrap();
        undos += 1;
    }
    assert_eq!(undos, 128);
    assert_eq!(
        browser.state.focused.as_ref().unwrap().text.as_deref(),
        Some("value 11")
    );
    while browser.state.focused.as_ref().unwrap().can_redo {
        browser
            .input(browser.context(), TextInputAction::Redo)
            .unwrap();
    }
    assert_eq!(
        browser.state.focused.as_ref().unwrap().text.as_deref(),
        Some("value 139")
    );
}

impl Browser {
    fn new(html: &str) -> Self {
        Self::with_executor(html, None)
    }

    fn with_executor(html: &str, executor: Option<ClickExecutor>) -> Self {
        Self::with_page_executor(
            html,
            executor.map(|e| Box::new(e) as Box<dyn PageJavaScriptExecutor + Send>),
        )
    }

    fn with_page_executor(
        html: &str,
        executor: Option<Box<dyn PageJavaScriptExecutor + Send>>,
    ) -> Self {
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
                    Some(executor.as_mut()),
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

    fn activate(
        &mut self,
        context: TextInputContext,
        x: f64,
        y: f64,
    ) -> Result<Option<blueice_ipc::file_input::FileInputState>, String> {
        self.activate_gesture(context, 42, x, y)
    }

    fn activate_gesture(
        &mut self,
        context: TextInputContext,
        gesture: u64,
        x: f64,
        y: f64,
    ) -> Result<Option<blueice_ipc::file_input::FileInputState>, String> {
        blueice_ipc::write_client_message_with_ids(
            &mut self.stream,
            Some(1),
            Some(82),
            &ClientMessage::NativeActivate {
                gesture,
                context,
                x,
                y,
            },
        )
        .unwrap();
        blueice_ipc::write_client_message_with_ids(
            &mut self.stream,
            Some(1),
            Some(83),
            &ClientMessage::GetTextInputState,
        )
        .unwrap();
        let mut result = None;
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
                ServerMessage::NativeActivationCompleted {
                    gesture: echoed,
                    file_input,
                } => {
                    assert_eq!(echoed, gesture);
                    assert_eq!(request, Some(82));
                    assert!(result.is_none());
                    result = Some(Ok(file_input));
                }
                ServerMessage::Error { message } => {
                    assert_eq!(request, Some(82));
                    result = Some(Err(message));
                }
                ServerMessage::TextInputState(state) => {
                    assert_eq!(request, Some(83));
                    self.state = state;
                    return result.expect("activation must acknowledge even an empty point");
                }
                message => panic!("unexpected {message:?}"),
            }
        }
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

struct LabelExecutor {
    nodes: Arc<Mutex<Vec<u64>>>,
    prevent_at: Option<usize>,
    replace_at: Option<usize>,
}
impl PageJavaScriptExecutor for LabelExecutor {
    fn synchronize_and_execute(&mut self, _: &TabManager) -> std::io::Result<()> {
        Ok(())
    }
    fn dispatch_click_serving_script(
        &mut self,
        tabs: &mut TabManager,
        tab: TabId,
        node: u64,
        _: Option<&blueice_engine::script::ScriptRequestReceiver>,
    ) -> std::io::Result<Option<bool>> {
        let mut nodes = self.nodes.lock().unwrap();
        nodes.push(node);
        if self.replace_at == Some(nodes.len()) {
            tabs.get_mut(tab)
                .unwrap()
                .load_html_str("<input type='file' aria-label='Replacement'>", None);
        }
        Ok(Some(self.prevent_at == Some(nodes.len())))
    }
    fn drain_reports_for_tab(&mut self, _: TabId) -> Vec<JavaScriptPageExecutionReport> {
        Vec::new()
    }
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
fn label_click_dispatches_label_then_associated_control_once_before_default() {
    let calls = Arc::new(AtomicU64::new(0));
    let mut browser = Browser::with_executor(
        "<label for='remember' role='button' aria-label='Toggle remember'>Remember</label>\
         <input id='remember' type='checkbox' aria-label='Remember'>",
        Some(ClickExecutor {
            calls: calls.clone(),
            prevent: false,
            replacement: None,
        }),
    );
    browser.click("Toggle remember");
    assert_eq!(calls.load(Ordering::Relaxed), 2);
    assert_eq!(browser.focused_name(), "Remember");
    assert_eq!(
        browser.snapshot().nodes.last().unwrap().state.checked,
        Some(true)
    );
    browser.click("Remember");
    assert_eq!(calls.load(Ordering::Relaxed), 3);
    assert_eq!(
        browser.snapshot().nodes.last().unwrap().state.checked,
        Some(false)
    );
}

#[test]
fn cancelled_label_click_never_dispatches_or_activates_associated_control() {
    let calls = Arc::new(AtomicU64::new(0));
    let mut browser = Browser::with_executor(
        "<label for='remember' role='button' aria-label='Toggle remember'>Remember</label>\
         <input id='remember' type='checkbox' aria-label='Remember'>",
        Some(ClickExecutor {
            calls: calls.clone(),
            prevent: true,
            replacement: None,
        }),
    );
    browser.click("Toggle remember");
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    assert_eq!(browser.state.focused_node, None);
    assert_eq!(
        browser.snapshot().nodes.last().unwrap().state.checked,
        Some(false)
    );
}

#[test]
fn implicit_label_skips_hidden_input_and_resolves_button() {
    let mut browser = Browser::new(
        "<label role='button' aria-label='Implicit label' style='display:block'>Activate\
         <input type='hidden'><button type='button' aria-label='Implicit button' style='display:block'>Button</button></label>",
    );
    let label = browser
        .snapshot()
        .nodes
        .into_iter()
        .find(|n| n.name.as_deref() == Some("Implicit label"))
        .unwrap();
    browser.interaction(ClientMessage::Click {
        x: label.bounds.x + 4.0,
        y: label.bounds.y + 4.0,
    });
    assert_eq!(browser.focused_name(), "Implicit button");
}

#[test]
fn explicit_label_does_not_associate_with_focusable_nonlabelable_first_id() {
    let mut browser = Browser::new(
        "<label for='duplicate' role='button' aria-label='Unassociated label'>Activate</label>\
         <div id='duplicate' tabindex='0' role='button' aria-label='Nonlabelable'>First</div>\
         <input id='duplicate' type='checkbox' aria-label='Later duplicate'>",
    );
    browser.click("Unassociated label");
    assert_ne!(
        browser
            .snapshot()
            .nodes
            .into_iter()
            .find(|n| n.state.focused)
            .and_then(|n| n.name)
            .as_deref(),
        Some("Nonlabelable")
    );
    assert_eq!(
        browser.snapshot().nodes.last().unwrap().state.checked,
        Some(false)
    );
}

#[test]
fn interactive_label_descendant_does_not_forward_to_associated_checkbox() {
    let calls = Arc::new(AtomicU64::new(0));
    let mut browser = Browser::with_executor(
        "<label for='remember'><button type='button' aria-label='Inner button'>Inner</button></label>\
         <input id='remember' type='checkbox' aria-label='Remember'>",
        Some(ClickExecutor { calls: calls.clone(), prevent: false, replacement: None }),
    );
    browser.click("Inner button");
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    assert_eq!(browser.focused_name(), "Inner button");
    assert_eq!(
        browser.snapshot().nodes.last().unwrap().state.checked,
        Some(false)
    );
}

#[test]
fn native_label_file_activation_acknowledges_exact_owner_and_both_event_targets() {
    let nodes = Arc::new(Mutex::new(Vec::new()));
    let mut browser = Browser::with_page_executor(
        "<label for='upload' role='button' aria-label='File label' style='display:block;height:30px'>Upload</label>\
         <input id='upload' type='file' multiple accept='.txt' aria-label='Upload'>",
        Some(Box::new(LabelExecutor { nodes: nodes.clone(), prevent_at: None, replace_at: None })),
    );
    let snapshot = browser.snapshot();
    let label = snapshot
        .nodes
        .iter()
        .find(|n| n.name.as_deref() == Some("File label"))
        .unwrap();
    let upload = snapshot
        .nodes
        .iter()
        .find(|n| n.name.as_deref() == Some("Upload"))
        .unwrap();
    let context = browser.context();
    let state = browser
        .activate(context, label.bounds.x + 4.0, label.bounds.y + 4.0)
        .unwrap()
        .unwrap();
    assert_eq!(*nodes.lock().unwrap(), [label.id, upload.id]);
    assert_eq!(state.context.tab_id, 1);
    assert_eq!(state.context.frame_source, context.frame_source);
    assert_eq!(
        state.context.document_generation,
        context.document_generation
    );
    assert_eq!(state.context.node_id, upload.id);
    assert!(state.multiple);
    assert_eq!(state.accept, ".txt");
    assert!(state.names.is_empty());
    assert_eq!(browser.focused_name(), "Upload");
    assert_eq!(
        browser.activate(browser.context(), 490.0, 290.0).unwrap(),
        None
    );
}

#[test]
fn native_label_file_activation_cancellation_and_replacement_never_offer_a_picker() {
    for (prevent_at, replace_at, expected) in [
        (Some(1), None, 1),
        (Some(2), None, 2),
        (None, Some(1), 1),
        (None, Some(2), 2),
    ] {
        let nodes = Arc::new(Mutex::new(Vec::new()));
        let mut browser = Browser::with_page_executor(
            "<label for='upload' role='button' aria-label='File label' style='display:block;height:30px'>Upload</label>\
             <input id='upload' type='file' aria-label='Upload'>",
            Some(Box::new(LabelExecutor { nodes: nodes.clone(), prevent_at, replace_at })),
        );
        let label = browser
            .snapshot()
            .nodes
            .into_iter()
            .find(|n| n.name.as_deref() == Some("File label"))
            .unwrap();
        let context = browser.context();
        assert_eq!(
            browser
                .activate(context, label.bounds.x + 4.0, label.bounds.y + 4.0)
                .unwrap(),
            None
        );
        assert_eq!(nodes.lock().unwrap().len(), expected);
        if replace_at.is_some() {
            assert_ne!(
                browser.context().document_generation,
                context.document_generation
            );
        }
    }
}

#[test]
fn native_label_file_activation_rejects_stale_contexts_and_invalid_points_before_events() {
    let nodes = Arc::new(Mutex::new(Vec::new()));
    let mut browser = Browser::with_page_executor(
        "<input type='file' aria-label='Upload'>",
        Some(Box::new(LabelExecutor {
            nodes: nodes.clone(),
            prevent_at: None,
            replace_at: None,
        })),
    );
    let input = browser.context();
    for context in [
        TextInputContext {
            version: 2,
            ..input
        },
        TextInputContext {
            frame_source: input.frame_source ^ 1,
            ..input
        },
        TextInputContext {
            document_generation: input.document_generation + 1,
            ..input
        },
        TextInputContext {
            focus_generation: input.focus_generation + 1,
            ..input
        },
    ] {
        assert!(browser.activate(context, 4.0, 4.0).is_err());
    }
    assert!(browser.activate(input, 1e10, 4.0).is_err());
    assert!(browser.activate_gesture(input, 0, 4.0, 4.0).is_err());
    assert!(nodes.lock().unwrap().is_empty());
    assert!(browser.activate(input, 4.0, 4.0).unwrap().is_some());
    assert_eq!(nodes.lock().unwrap().len(), 1);
}

#[test]
fn disabled_label_control_never_receives_forwarded_event_or_picker_hint() {
    let nodes = Arc::new(Mutex::new(Vec::new()));
    let mut browser = Browser::with_page_executor(
        "<label for='upload' role='button' aria-label='Disabled file label' style='display:block;height:30px'>Upload</label>\
         <fieldset disabled><input id='upload' type='file' aria-label='Upload'></fieldset>",
        Some(Box::new(LabelExecutor { nodes: nodes.clone(), prevent_at: None, replace_at: None })),
    );
    let label = browser
        .snapshot()
        .nodes
        .into_iter()
        .find(|n| n.name.as_deref() == Some("Disabled file label"))
        .unwrap();
    assert_eq!(
        browser
            .activate(
                browser.context(),
                label.bounds.x + 4.0,
                label.bounds.y + 4.0
            )
            .unwrap(),
        None
    );
    assert_eq!(*nodes.lock().unwrap(), [label.id]);
    assert_eq!(browser.state.focused_node, None);
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
fn multiple_select_shift_keys_extend_from_the_anchor_and_skip_disabled_options() {
    let mut browser = Browser::new(
        "<select multiple aria-label='Multiple'><option selected value='a'>Alpha</option>\
         <option disabled value='x'>Disabled</option><option value='b'>Beta</option><option value='c'>Gamma</option></select>",
    );
    browser.key(PageKey::Tab, false);
    browser.key(PageKey::ArrowDown, true);
    let selected: Vec<_> = browser
        .snapshot()
        .nodes
        .into_iter()
        .filter(|node| node.state.selected)
        .filter_map(|node| node.name)
        .collect();
    assert_eq!(selected, ["Alpha", "Beta"]);
    browser.key(PageKey::ArrowDown, true);
    let selected: Vec<_> = browser
        .snapshot()
        .nodes
        .into_iter()
        .filter(|node| node.state.selected)
        .filter_map(|node| node.name)
        .collect();
    assert_eq!(selected, ["Alpha", "Beta", "Gamma"]);
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

#[test]
fn select_popup_uses_option_identity_labels_groups_and_rejects_disabled_foreign_or_stale_choices() {
    let mut browser = Browser::new("<select><option value='same'>Alpha</option><optgroup label='Group' disabled><option>Locked</option></optgroup><option value='same' label='Beta'>hidden label</option></select><select><option>Foreign</option></select>");
    browser.key(PageKey::Tab, false);
    let state = browser.state.select.clone().unwrap();
    assert!(state.popup);
    assert!(!state.multiple);
    assert_eq!(
        state
            .options
            .iter()
            .map(|o| o.label.as_str())
            .collect::<Vec<_>>(),
        ["Alpha", "Locked", "Beta"]
    );
    assert_eq!(state.options[1].group.as_deref(), Some("Group"));
    assert!(state.options[1].disabled);
    let frame = browser.state.frame_generation;
    let choice = |id, frame| TextInputAction::SelectOption {
        option_id: id,
        frame_generation: frame,
        extend: false,
        toggle: false,
    };
    assert!(browser
        .input(browser.context(), choice(state.options[1].node_id, frame))
        .is_err());
    let foreign = browser
        .snapshot()
        .nodes
        .iter()
        .find(|n| n.name.as_deref() == Some("Foreign"))
        .unwrap()
        .id;
    assert!(browser
        .input(browser.context(), choice(foreign, frame))
        .is_err());
    browser
        .input(browser.context(), choice(state.options[2].node_id, frame))
        .unwrap();
    assert!(browser.state.select.as_ref().unwrap().options[2].selected);
    assert!(browser
        .input(browser.context(), choice(state.options[0].node_id, frame))
        .is_err());
    assert!(browser.state.select.as_ref().unwrap().options[2].selected);
}

#[test]
fn select_typeahead_cycles_repeated_letters_matches_unicode_and_resets_at_focus_changes() {
    let mut browser = Browser::new("<select><option>Alpha</option><option>Beta</option><option disabled>Blocked</option><option>Bravo</option><option>中文</option></select><input>");
    browser.key(PageKey::Tab, false);
    let type_text = |text: &str| TextInputAction::Replace {
        text: text.into(),
        replacement: None,
    };
    browser.input(browser.context(), type_text("b")).unwrap();
    assert_eq!(
        browser
            .state
            .select
            .as_ref()
            .unwrap()
            .options
            .iter()
            .find(|o| o.selected)
            .unwrap()
            .label,
        "Beta"
    );
    browser.input(browser.context(), type_text("b")).unwrap();
    assert_eq!(
        browser
            .state
            .select
            .as_ref()
            .unwrap()
            .options
            .iter()
            .find(|o| o.selected)
            .unwrap()
            .label,
        "Bravo"
    );
    browser.key(PageKey::Tab, false);
    browser.key(PageKey::Tab, true);
    browser.input(browser.context(), type_text("中")).unwrap();
    browser.input(browser.context(), type_text("文")).unwrap();
    assert_eq!(
        browser
            .state
            .select
            .as_ref()
            .unwrap()
            .options
            .iter()
            .find(|o| o.selected)
            .unwrap()
            .label,
        "中文"
    );
    assert!(browser.state.focused.is_none());
}

#[test]
fn multiple_select_command_focus_space_toggle_select_all_and_shift_range_keep_core_state() {
    let mut browser = Browser::new("<select multiple size='2'><option selected>A</option><option disabled>Locked</option><option>B</option><option>C</option></select><input>");
    browser.key(PageKey::Tab, false);
    let selected = |browser: &Browser| {
        browser
            .state
            .select
            .as_ref()
            .unwrap()
            .options
            .iter()
            .filter(|o| o.selected)
            .map(|o| o.label.as_str())
            .collect::<Vec<_>>()
            .join(",")
    };
    browser
        .input(
            browser.context(),
            TextInputAction::SelectKey {
                key: PageKey::ArrowDown,
                extend: false,
                toggle: true,
            },
        )
        .unwrap();
    assert_eq!(selected(&browser), "A");
    browser.key(PageKey::Space, false);
    assert_eq!(selected(&browser), "A,B");
    browser.key(PageKey::Space, false);
    assert_eq!(selected(&browser), "A");
    browser.key(PageKey::ArrowDown, true);
    assert_eq!(selected(&browser), "B,C");
    browser
        .input(browser.context(), TextInputAction::SelectAll)
        .unwrap();
    assert_eq!(selected(&browser), "A,B,C");
    let old = browser.context();
    browser.key(PageKey::Tab, false);
    assert!(browser.input(old, TextInputAction::SelectAll).is_err());
}

#[test]
fn listbox_size_has_no_implicit_selection_and_pointer_modifiers_use_visible_option_bounds() {
    let mut browser = Browser::new("<select multiple size='2'><option>A</option><option>B</option><option>C</option><option>D</option></select><select size='3'><option>Empty choice</option></select>");
    let snapshot = browser.snapshot();
    assert!(snapshot
        .nodes
        .iter()
        .filter(|n| n.role == blueice_ipc::Role::ComboBox)
        .all(|n| n.state.value.is_none()));
    browser.key(PageKey::Tab, false);
    let b = browser
        .snapshot()
        .nodes
        .iter()
        .find(|n| n.name.as_deref() == Some("B"))
        .unwrap()
        .bounds;
    browser
        .input(
            browser.context(),
            TextInputAction::SelectPointer {
                x: b.x + 2.0,
                y: b.y + 2.0 - browser.state.scroll_y,
                extend: false,
                toggle: true,
            },
        )
        .unwrap();
    assert!(browser.state.select.as_ref().unwrap().options[1].selected);
    browser.key(PageKey::End, true);
    assert_eq!(
        browser
            .state
            .select
            .as_ref()
            .unwrap()
            .options
            .iter()
            .filter(|o| o.selected)
            .map(|o| o.label.as_str())
            .collect::<Vec<_>>(),
        ["B", "C", "D"]
    );
    let snapshot = browser.snapshot();
    let a = snapshot
        .nodes
        .iter()
        .find(|n| n.name.as_deref() == Some("A"))
        .unwrap();
    let d = snapshot
        .nodes
        .iter()
        .find(|n| n.name.as_deref() == Some("D"))
        .unwrap();
    assert_eq!(a.bounds.height, 0.0);
    assert!(d.bounds.height > 0.0);
    browser.key(PageKey::Tab, false);
    assert!(!browser.state.select.as_ref().unwrap().options[0].selected);
    browser.key(PageKey::Home, false);
    let option = browser.state.select.as_ref().unwrap().options[0].node_id;
    browser
        .input(
            browser.context(),
            TextInputAction::SelectOption {
                option_id: option,
                frame_generation: browser.state.frame_generation,
                extend: false,
                toggle: true,
            },
        )
        .unwrap();
    assert!(!browser.state.select.as_ref().unwrap().options[0].selected);
}

#[test]
fn multiple_select_value_is_the_first_selected_option_and_reset_restores_defaults_and_anchor() {
    let mut browser = Browser::new("<form><select multiple aria-label='Choice'><option selected value='a'>A</option><option value='b'>B</option><option selected value='c'>C</option></select><button type='reset' aria-label='Reset'>Reset</button></form>");
    assert_eq!(
        browser
            .snapshot()
            .nodes
            .iter()
            .find(|n| n.name.as_deref() == Some("Choice"))
            .unwrap()
            .state
            .value
            .as_deref(),
        Some("a")
    );
    browser.key(PageKey::Tab, false);
    browser.key(PageKey::End, false);
    browser.key(PageKey::Tab, false);
    browser.key(PageKey::Enter, false);
    browser.key(PageKey::Tab, true);
    assert_eq!(
        browser
            .state
            .select
            .as_ref()
            .unwrap()
            .options
            .iter()
            .filter(|o| o.selected)
            .map(|o| o.label.as_str())
            .collect::<Vec<_>>(),
        ["A", "C"]
    );
    browser.key(PageKey::ArrowDown, true);
    assert_eq!(
        browser
            .state
            .select
            .as_ref()
            .unwrap()
            .options
            .iter()
            .filter(|o| o.selected)
            .map(|o| o.label.as_str())
            .collect::<Vec<_>>(),
        ["A", "B"]
    );
}

#[test]
fn select_scroll_without_focus_changes_only_visible_options_and_page_keys_reveal_the_active_choice()
{
    let mut browser = Browser::new("<select multiple size='2' aria-label='Scrollable'><option>A</option><option>B</option><option>C</option><option>D</option><option>E</option></select>");
    let control = browser
        .snapshot()
        .nodes
        .iter()
        .find(|n| n.name.as_deref() == Some("Scrollable"))
        .unwrap()
        .bounds;
    browser
        .input(
            browser.context(),
            TextInputAction::SelectScroll {
                x: control.x + 3.0,
                y: control.y + 3.0,
                rows: 3,
            },
        )
        .unwrap();
    assert!(browser.state.focused_node.is_none());
    let snapshot = browser.snapshot();
    assert!(
        snapshot
            .nodes
            .iter()
            .find(|n| n.name.as_deref() == Some("E"))
            .unwrap()
            .bounds
            .height
            > 0.0
    );
    assert_eq!(
        snapshot
            .nodes
            .iter()
            .find(|n| n.name.as_deref() == Some("A"))
            .unwrap()
            .bounds
            .height,
        0.0
    );
    browser.key(PageKey::Tab, false);
    browser.key(PageKey::PageDown, true);
    browser.key(PageKey::PageDown, true);
    assert_eq!(
        browser
            .state
            .select
            .as_ref()
            .unwrap()
            .options
            .iter()
            .filter(|o| o.selected)
            .map(|o| o.label.as_str())
            .collect::<Vec<_>>(),
        ["A", "B", "C"]
    );
    browser.key(PageKey::PageUp, true);
    assert_eq!(
        browser
            .state
            .select
            .as_ref()
            .unwrap()
            .options
            .iter()
            .filter(|o| o.selected)
            .map(|o| o.label.as_str())
            .collect::<Vec<_>>(),
        ["A", "B"]
    );
}

#[test]
fn select_presentation_is_bounded_without_losing_keyboard_access_to_later_options() {
    let mut html = String::from("<select>");
    for index in 0..1025 {
        html.push_str(&format!("<option value='{index}'>Choice {index}</option>"));
    }
    html.push_str("</select>");
    let mut browser = Browser::new(&html);
    browser.key(PageKey::Tab, false);
    let select = browser.state.select.as_ref().unwrap();
    assert!(select.limited);
    assert_eq!(select.options.len(), 1024);
    browser.key(PageKey::End, false);
    assert_eq!(
        browser
            .snapshot()
            .nodes
            .iter()
            .find(|n| n.role == blueice_ipc::Role::ComboBox)
            .unwrap()
            .state
            .value
            .as_deref(),
        Some("1024")
    );
}

impl Browser {
    fn accessibility_context(
        &mut self,
        name: &str,
    ) -> blueice_ipc::accessibility::AccessibilityTextContext {
        blueice_ipc::accessibility::AccessibilityTextContext {
            version: 1,
            frame_source: self.state.frame_source,
            document_generation: self.state.document_generation,
            frame_generation: self.state.frame_generation,
            node_id: self
                .snapshot()
                .nodes
                .iter()
                .find(|n| n.name.as_deref() == Some(name))
                .unwrap()
                .id,
        }
    }
    fn accessibility(
        &mut self,
        context: blueice_ipc::accessibility::AccessibilityTextContext,
        action: blueice_ipc::accessibility::AccessibilityTextAction,
    ) -> Result<blueice_ipc::accessibility::AccessibilityTextReply, String> {
        blueice_ipc::write_client_message_with_ids(
            &mut self.stream,
            Some(1),
            Some(95),
            &ClientMessage::AccessibilityText { context, action },
        )
        .unwrap();
        let result = loop {
            let (tab, request, message) =
                blueice_ipc::read_server_message_with_ids(&mut self.stream).unwrap();
            assert_eq!((tab, request), (Some(1), Some(95)));
            match message {
                ServerMessage::FrameReady { shm_path, .. } => {
                    self.pixels = blueice_ipc::shm::map_frame(std::path::Path::new(&shm_path))
                        .unwrap()
                        .to_vec()
                }
                ServerMessage::AccessibilityTextState(reply) => break Ok(reply),
                ServerMessage::Error { message } => break Err(message),
                other => panic!("unexpected {other:?}"),
            }
        };
        blueice_ipc::write_client_message_with_ids(
            &mut self.stream,
            Some(1),
            Some(96),
            &ClientMessage::GetTextInputState,
        )
        .unwrap();
        let (tab, request, reply) =
            blueice_ipc::read_server_message_with_ids(&mut self.stream).unwrap();
        assert_eq!((tab, request), (Some(1), Some(96)));
        if let ServerMessage::TextInputState(state) = reply {
            self.state = state;
        } else {
            panic!("unexpected {reply:?}");
        }
        result
    }
}

#[test]
fn accessibility_text_ranges_use_utf16_graphemes_and_core_geometry_without_read_side_effects() {
    use blueice_ipc::accessibility::{
        AccessibilityTextAction as Action, AccessibilityTextResult as Result,
    };
    use blueice_ipc::input::TextRange;
    let mut browser = Browser::new("<input aria-label='Editor' value='A😀B' style='width:180px;height:32px'><textarea aria-label='Notes' style='width:200px;height:80px'>first\n中文😀\nlast</textarea>");
    let context = browser.accessibility_context("Editor");
    let reply = browser.accessibility(context, Action::Inspect).unwrap();
    let Result::State(state) = reply.result else {
        panic!("text state expected")
    };
    assert_eq!(state.text.as_deref(), Some("A😀B"));
    assert_eq!(state.text_length, 4);
    assert!(!state.focused);
    assert!(state.selection.is_none());
    assert!(browser.state.focused_node.is_none());
    let glyph = browser
        .accessibility(context, Action::RangeForIndex { index: 2 })
        .unwrap();
    assert_eq!(
        glyph.result,
        Result::Range(Some(TextRange {
            location: 1,
            length: 2
        }))
    );
    browser
        .accessibility(
            context,
            Action::Select {
                range: TextRange {
                    location: 1,
                    length: 2,
                },
            },
        )
        .unwrap();
    let context = browser.accessibility_context("Editor");
    let before = browser.pixels.clone();
    let focus = browser.state.focus_generation;
    let reply = browser
        .accessibility(
            context,
            Action::Bounds {
                range: TextRange {
                    location: 1,
                    length: 2,
                },
            },
        )
        .unwrap();
    let Result::Bounds(Some(bounds)) = reply.result else {
        panic!("range bounds expected")
    };
    assert!(bounds.width > 1.0 && bounds.height > 0.0);
    let point = browser
        .accessibility(
            context,
            Action::RangeForPosition {
                x: bounds.x + 0.1,
                y: bounds.y + bounds.height / 2.0,
            },
        )
        .unwrap();
    assert_eq!(
        point.result,
        Result::Range(Some(TextRange {
            location: 1,
            length: 2
        }))
    );
    assert_eq!(browser.pixels, before);
    assert_eq!(browser.state.focus_generation, focus);
    assert!(browser
        .accessibility(
            context,
            Action::Select {
                range: TextRange {
                    location: 2,
                    length: 0
                }
            }
        )
        .is_err());
    assert_eq!(
        browser.state.focused.as_ref().unwrap().selection,
        TextRange {
            location: 1,
            length: 2
        }
    );
    let notes = browser.accessibility_context("Notes");
    assert_eq!(
        browser
            .accessibility(notes, Action::LineForIndex { index: 7 })
            .unwrap()
            .result,
        Result::Index(Some(1))
    );
    let line = browser
        .accessibility(notes, Action::RangeForLine { line: 1 })
        .unwrap();
    assert_eq!(
        line.result,
        Result::Range(Some(TextRange {
            location: 6,
            length: 5
        }))
    );
    assert_eq!(
        browser.focused_name(),
        "Editor",
        "Range queries cannot move focus"
    );
}

#[test]
fn accessibility_text_writes_are_atomic_undoable_private_readonly_and_frame_fenced() {
    use blueice_ipc::accessibility::{
        AccessibilityTextAction as Action, AccessibilityTextResult as Result,
    };
    use blueice_ipc::input::TextRange;
    let mut browser = Browser::new("<input aria-label='Editor' value='A😀B'><input aria-label='Secret' type='password' value='fixture-secret'><input aria-label='Readonly' readonly value='locked'><input aria-label='Disabled' disabled value='disabled'>");
    let old = browser.accessibility_context("Editor");
    browser
        .accessibility(
            old,
            Action::Select {
                range: TextRange {
                    location: 1,
                    length: 2,
                },
            },
        )
        .unwrap();
    assert!(browser
        .accessibility(
            old,
            Action::SetValue {
                text: "late".into()
            }
        )
        .is_err());
    let context = browser.accessibility_context("Editor");
    browser
        .accessibility(
            context,
            Action::ReplaceSelection {
                text: "中文".into(),
            },
        )
        .unwrap();
    assert_eq!(
        browser.state.focused.as_ref().unwrap().text.as_deref(),
        Some("A中文B")
    );
    browser
        .input(browser.context(), TextInputAction::Undo)
        .unwrap();
    assert_eq!(
        browser.state.focused.as_ref().unwrap().text.as_deref(),
        Some("A😀B")
    );
    let secret = browser.accessibility_context("Secret");
    let reply = browser
        .accessibility(
            secret,
            Action::SetValue {
                text: "new-private-secret".into(),
            },
        )
        .unwrap();
    let json = serde_json::to_string(&reply).unwrap();
    assert!(!json.contains("fixture-secret") && !json.contains("new-private-secret"));
    let Result::State(state) = reply.result else {
        panic!("text state expected")
    };
    assert!(state.protected);
    assert!(state.text.is_none());
    let readonly = browser.accessibility_context("Readonly");
    let focus = browser.state.focused_node;
    assert!(browser
        .accessibility(readonly, Action::SetValue { text: "bad".into() })
        .is_err());
    assert_eq!(
        browser.state.focused_node, focus,
        "Rejected writes cannot change focus"
    );
    browser
        .accessibility(
            readonly,
            Action::Select {
                range: TextRange {
                    location: 0,
                    length: 3,
                },
            },
        )
        .unwrap();
    assert_eq!(
        browser.state.focused.as_ref().unwrap().text.as_deref(),
        Some("locked")
    );
    let disabled = browser.accessibility_context("Disabled");
    assert!(browser.accessibility(disabled, Action::Inspect).is_err());
}

#[test]
fn accessibility_text_scroll_reveals_long_ranges_without_changing_focus_or_selection() {
    use blueice_ipc::accessibility::{
        AccessibilityTextAction as Action, AccessibilityTextResult as Result,
    };
    use blueice_ipc::input::TextRange;
    let text = format!("{}尾😀", "x".repeat(1500));
    let mut browser = Browser::new(&format!("<input aria-label='Long' style='width:120px;height:32px' value='{text}'><textarea aria-label='Notes' style='width:160px;height:60px'>{}</textarea>", "row\n".repeat(50)));
    let context = browser.accessibility_context("Long");
    let before_scroll = browser.pixels.clone();
    let range = TextRange {
        location: 1500,
        length: 3,
    };
    browser
        .accessibility(context, Action::ScrollToRange { range })
        .unwrap();
    assert_ne!(
        browser.pixels, before_scroll,
        "Revealing the tail must update the rendered text"
    );
    assert!(browser.state.focused_node.is_none());
    let context = browser.accessibility_context("Long");
    let reply = browser
        .accessibility(context, Action::Bounds { range })
        .unwrap();
    let Result::Bounds(Some(bounds)) = reply.result else {
        panic!("visible long-text bounds expected")
    };
    let control = browser
        .snapshot()
        .nodes
        .iter()
        .find(|n| n.name.as_deref() == Some("Long"))
        .unwrap()
        .bounds;
    assert!(bounds.x >= control.x && bounds.x + bounds.width <= control.x + control.width);
    let reply = browser.accessibility(context, Action::Inspect).unwrap();
    let Result::State(state) = reply.result else {
        panic!("text state expected")
    };
    assert!(state.visible_range.location > 1000);
    assert!(state.selection.is_none());
    let notes = browser.accessibility_context("Notes");
    browser
        .accessibility(
            notes,
            Action::Select {
                range: TextRange {
                    location: 180,
                    length: 3,
                },
            },
        )
        .unwrap();
    let notes = browser.accessibility_context("Notes");
    let reply = browser
        .accessibility(
            notes,
            Action::Bounds {
                range: TextRange {
                    location: 180,
                    length: 3,
                },
            },
        )
        .unwrap();
    let Result::Bounds(Some(bounds)) = reply.result else {
        panic!("visible textarea bounds expected")
    };
    let control = browser
        .snapshot()
        .nodes
        .iter()
        .find(|n| n.name.as_deref() == Some("Notes"))
        .unwrap()
        .bounds;
    assert!(bounds.y >= control.y && bounds.y + bounds.height <= control.y + control.height);
    assert_eq!(
        browser.state.focused.as_ref().unwrap().selection,
        TextRange {
            location: 180,
            length: 3
        }
    );
}

#[test]
fn accessibility_text_rejects_invalid_identity_ranges_and_oversize_edits_before_focus_changes() {
    use blueice_ipc::accessibility::AccessibilityTextAction as Action;
    use blueice_ipc::input::TextRange;
    let mut browser = Browser::new(
        "<input aria-label='First' value='safe'><input aria-label='Other' value='other'>",
    );
    browser.key(PageKey::Tab, false);
    let context = browser.accessibility_context("Other");
    let before = browser.pixels.clone();
    let focus = browser.state.focus_generation;
    for invalid in [
        blueice_ipc::accessibility::AccessibilityTextContext {
            version: 2,
            ..context
        },
        blueice_ipc::accessibility::AccessibilityTextContext {
            frame_source: context.frame_source.wrapping_add(1),
            ..context
        },
        blueice_ipc::accessibility::AccessibilityTextContext {
            document_generation: context.document_generation + 1,
            ..context
        },
        blueice_ipc::accessibility::AccessibilityTextContext {
            node_id: u64::MAX,
            ..context
        },
    ] {
        assert!(browser
            .accessibility(
                invalid,
                Action::SetValue {
                    text: "wrong".into()
                }
            )
            .is_err());
    }
    for action in [
        Action::Select {
            range: TextRange {
                location: u32::MAX,
                length: 3,
            },
        },
        Action::Bounds {
            range: TextRange {
                location: 4,
                length: u32::MAX,
            },
        },
        Action::ReplaceSelection {
            text: "x".repeat(65_536),
        },
        Action::SetValue {
            text: "x".repeat(65_537),
        },
    ] {
        assert!(browser.accessibility(context, action).is_err());
    }
    assert_eq!(browser.state.focus_generation, focus);
    assert_eq!(browser.focused_name(), "First");
    assert_eq!(
        browser.state.focused.as_ref().unwrap().text.as_deref(),
        Some("safe")
    );
    assert_eq!(browser.pixels, before);
}

#[test]
fn accessibility_text_composed_graphemes_and_marked_text_replacements_keep_atomic_undo() {
    use blueice_ipc::accessibility::{
        AccessibilityTextAction as Action, AccessibilityTextResult as Result,
    };
    use blueice_ipc::input::TextRange;
    let mut browser = Browser::new("<input aria-label='Editor' value='a👨‍👩‍👧‍👦é'>");
    let context = browser.accessibility_context("Editor");
    assert_eq!(
        browser
            .accessibility(context, Action::RangeForIndex { index: 6 })
            .unwrap()
            .result,
        Result::Range(Some(TextRange {
            location: 1,
            length: 11
        }))
    );
    assert_eq!(
        browser
            .accessibility(context, Action::RangeForIndex { index: 13 })
            .unwrap()
            .result,
        Result::Range(Some(TextRange {
            location: 12,
            length: 2
        }))
    );
    browser.key(PageKey::Tab, false);
    browser
        .input(browser.context(), TextInputAction::SelectAll)
        .unwrap();
    browser
        .input(
            browser.context(),
            TextInputAction::Compose {
                text: "注音".into(),
                selection: TextRange {
                    location: 2,
                    length: 0,
                },
                replacement: None,
            },
        )
        .unwrap();
    let context = browser.accessibility_context("Editor");
    browser
        .accessibility(
            context,
            Action::SetValue {
                text: "committed".into(),
            },
        )
        .unwrap();
    assert!(browser.state.focused.as_ref().unwrap().marked.is_none());
    browser
        .input(browser.context(), TextInputAction::Undo)
        .unwrap();
    assert_eq!(
        browser.state.focused.as_ref().unwrap().text.as_deref(),
        Some("注音")
    );
    browser
        .input(browser.context(), TextInputAction::Undo)
        .unwrap();
    assert_eq!(
        browser.state.focused.as_ref().unwrap().text.as_deref(),
        Some("a👨‍👩‍👧‍👦é")
    );
}

#[test]
fn nonrendered_label_file_controls_receive_owned_hints_without_focus() {
    for attributes in [
        "style='display:none'",
        "hidden",
        "hidden style='display:none'",
    ] {
        for implicit in [false, true] {
            let control =
                format!("<input id='upload' type='file' multiple accept='.bin' {attributes}>");
            let html = if implicit {
                format!("<label role='button' aria-label='File label' style='display:block;height:30px'>Upload{control}</label>")
            } else {
                format!("<label for='upload' role='button' aria-label='File label' style='display:block;height:30px'>Upload</label>{control}")
            };
            let nodes = Arc::new(Mutex::new(Vec::new()));
            let mut browser = Browser::with_page_executor(
                &html,
                Some(Box::new(LabelExecutor {
                    nodes: nodes.clone(),
                    prevent_at: None,
                    replace_at: None,
                })),
            );
            let label = browser
                .snapshot()
                .nodes
                .into_iter()
                .find(|node| node.name.as_deref() == Some("File label"))
                .unwrap();
            let context = browser.context();
            let state = browser
                .activate(context, label.bounds.x + 4.0, label.bounds.y + 4.0)
                .unwrap()
                .expect("A visible label can activate its enabled nonrendered file control");
            assert_eq!(*nodes.lock().unwrap(), [label.id, state.context.node_id]);
            assert_ne!(state.context.node_id, label.id);
            assert_eq!(state.context.tab_id, 1);
            assert_eq!(state.context.frame_source, context.frame_source);
            assert_eq!(
                state.context.document_generation,
                context.document_generation
            );
            assert!(state.multiple);
            assert_eq!(state.accept, ".bin");
            assert!(state.names.is_empty());
            assert_eq!(browser.state.focused_node, None);
        }
    }
}

#[test]
fn nonrendered_label_controls_keep_disabled_and_inert_restrictions() {
    for control in [
        "<input id='upload' type='file' disabled style='display:none'>",
        "<fieldset disabled><input id='upload' type='file' style='display:none'></fieldset>",
        "<input id='upload' type='file' inert style='display:none'>",
        "<div inert><input id='upload' type='file' style='display:none'></div>",
        "<input id='upload' type='hidden'>",
    ] {
        let nodes = Arc::new(Mutex::new(Vec::new()));
        let mut browser = Browser::with_page_executor(&format!("<label for='upload' role='button' aria-label='File label' style='display:block;height:30px'>Upload</label>{control}"), Some(Box::new(LabelExecutor {
            nodes: nodes.clone(), prevent_at: None, replace_at: None,
        })));
        let label = browser
            .snapshot()
            .nodes
            .into_iter()
            .find(|node| node.name.as_deref() == Some("File label"))
            .unwrap();
        assert_eq!(
            browser
                .activate(
                    browser.context(),
                    label.bounds.x + 4.0,
                    label.bounds.y + 4.0
                )
                .unwrap(),
            None
        );
        assert_eq!(*nodes.lock().unwrap(), [label.id]);
        assert_eq!(browser.state.focused_node, None);
    }
}

#[test]
fn nonrendered_label_file_listeners_cancel_or_replace_before_picker_hint() {
    for (prevent_at, replace_at, expected) in [
        (Some(1), None, 1),
        (Some(2), None, 2),
        (None, Some(1), 1),
        (None, Some(2), 2),
    ] {
        let nodes = Arc::new(Mutex::new(Vec::new()));
        let mut browser = Browser::with_page_executor("<label for='upload' role='button' aria-label='File label' style='display:block;height:30px'>Upload</label><input id='upload' type='file' style='display:none'>", Some(Box::new(LabelExecutor {
            nodes: nodes.clone(), prevent_at, replace_at,
        })));
        let label = browser
            .snapshot()
            .nodes
            .into_iter()
            .find(|node| node.name.as_deref() == Some("File label"))
            .unwrap();
        let context = browser.context();
        assert_eq!(
            browser
                .activate(context, label.bounds.x + 4.0, label.bounds.y + 4.0)
                .unwrap(),
            None
        );
        assert_eq!(nodes.lock().unwrap().len(), expected);
        if replace_at.is_some() {
            assert_ne!(
                browser.context().document_generation,
                context.document_generation
            );
        }
    }
}

impl Browser {
    fn file_input_roundtrip(
        &mut self,
        action: blueice_ipc::file_input::FileInputAction,
    ) -> Result<blueice_ipc::file_input::FileInputState, String> {
        blueice_ipc::write_client_message_with_ids(
            &mut self.stream,
            Some(1),
            Some(90),
            &ClientMessage::FileInput(action),
        )
        .unwrap();
        blueice_ipc::write_client_message_with_ids(
            &mut self.stream,
            Some(1),
            Some(91),
            &ClientMessage::GetTextInputState,
        )
        .unwrap();
        let mut result = None;
        loop {
            let (tab, request, message) =
                blueice_ipc::read_server_message_with_ids(&mut self.stream).unwrap();
            assert_eq!(tab, Some(1));
            match message {
                ServerMessage::FrameReady { shm_path, .. } => {
                    assert_eq!(request, Some(90));
                    self.pixels = blueice_ipc::shm::map_frame(std::path::Path::new(&shm_path))
                        .unwrap()
                        .to_vec();
                }
                ServerMessage::FileInputState(state) => {
                    assert_eq!(request, Some(90));
                    assert!(result.is_none());
                    result = Some(Ok(state));
                }
                ServerMessage::Error { message } => {
                    assert_eq!(request, Some(90));
                    assert!(result.is_none());
                    result = Some(Err(message));
                }
                ServerMessage::TextInputState(state) => {
                    assert_eq!(request, Some(91));
                    self.state = state;
                    return result.expect("File operation must return an owned result");
                }
                message => panic!("unexpected {message:?}"),
            }
        }
    }
}

#[test]
fn nonrendered_file_selection_validates_cancels_and_resets_by_owned_context() {
    use blueice_ipc::file_input::{FileData, FileInputAction};
    let mut browser = Browser::new("<form><label for='upload' role='button' aria-label='File label' style='display:block;height:30px'>Upload</label><input id='upload' type='file' style='display:none'><button type='reset' aria-label='Reset hidden upload'>Reset</button></form>");
    let label = browser
        .snapshot()
        .nodes
        .into_iter()
        .find(|node| node.name.as_deref() == Some("File label"))
        .unwrap();
    let prepared = browser
        .activate(
            browser.context(),
            label.bounds.x + 4.0,
            label.bounds.y + 4.0,
        )
        .unwrap()
        .unwrap();
    assert_eq!(
        browser
            .file_input_roundtrip(FileInputAction::Validate {
                context: prepared.context
            })
            .unwrap(),
        prepared
    );
    let selected = browser
        .file_input_roundtrip(FileInputAction::Set {
            context: prepared.context,
            files: vec![FileData {
                name: "hidden-中文.bin".into(),
                media_type: "application/octet-stream".into(),
                last_modified: 123,
                bytes: vec![0, 255, 13, 10, 7],
            }],
        })
        .unwrap();
    assert_eq!(selected.names, ["hidden-中文.bin"]);
    assert_ne!(selected.context.revision, prepared.context.revision);
    assert_eq!(
        browser
            .file_input_roundtrip(FileInputAction::Cancel {
                context: selected.context
            })
            .unwrap(),
        selected
    );
    assert_eq!(
        browser
            .file_input_roundtrip(FileInputAction::Validate {
                context: selected.context
            })
            .unwrap(),
        selected
    );
    assert_ne!(browser.state.focused_node, Some(selected.context.node_id));
    browser.click("Reset hidden upload");
    assert!(browser
        .file_input_roundtrip(FileInputAction::Validate {
            context: selected.context
        })
        .is_err());
    let cleared = browser
        .file_input_roundtrip(FileInputAction::Prepare {
            frame_source: selected.context.frame_source,
            document_generation: selected.context.document_generation,
            node_id: selected.context.node_id,
        })
        .unwrap();
    assert!(cleared.names.is_empty());
    assert_ne!(browser.state.focused_node, Some(cleared.context.node_id));
}

struct ControlDumpExecutor {
    dumps: Arc<Mutex<Vec<String>>>,
}
impl PageJavaScriptExecutor for ControlDumpExecutor {
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
        self.dumps
            .lock()
            .unwrap()
            .push(tabs.get(tab).unwrap().dom_dump());
        Ok(Some(false))
    }
    fn drain_reports_for_tab(&mut self, _: TabId) -> Vec<JavaScriptPageExecutionReport> {
        Vec::new()
    }
}

#[test]
fn labels_activate_nonrendered_checkbox_and_radio_defaults_without_control_focus() {
    for kind in ["checkbox", "radio"] {
        for tabindex in ["", "tabindex='0'"] {
            let dumps = Arc::new(Mutex::new(Vec::new()));
            let mut browser = Browser::with_page_executor(&format!("<label for='box' role='button' aria-label='Toggle' {tabindex} style='display:block;height:30px'>Toggle</label><input id='box' type='{kind}' style='display:none'><button type='button' aria-label='Inspect'>Inspect</button>"), Some(Box::new(ControlDumpExecutor { dumps: dumps.clone() })));
            let label = browser
                .snapshot()
                .nodes
                .into_iter()
                .find(|node| node.name.as_deref() == Some("Toggle"))
                .unwrap();
            let expected_focus = (!tabindex.is_empty()).then_some(label.id);
            browser.click("Toggle");
            assert_eq!(browser.state.focused_node, expected_focus);
            assert_eq!(dumps.lock().unwrap().len(), 2);
            assert!(
                dumps
                    .lock()
                    .unwrap()
                    .iter()
                    .all(|dump| !dump.contains("checked")),
                "Both click listeners run before the native default"
            );
            browser.click("Inspect");
            assert!(dumps.lock().unwrap().last().unwrap().contains("checked"));
            if kind == "checkbox" {
                browser.click("Toggle");
                assert_eq!(browser.state.focused_node, expected_focus);
                browser.click("Inspect");
                assert!(!dumps.lock().unwrap().last().unwrap().contains("checked"));
            }
        }
    }
}

#[test]
fn inert_pointer_label_sources_do_not_activate_external_file_controls() {
    for (source, inert) in [
        ("<label for='upload' style='display:block;width:220px;height:40px'>Upload</label>", false),
        ("<label inert for='upload' style='display:block;width:220px;height:40px'>Upload</label>", true),
        ("<div inert><label for='upload' style='display:block;width:220px;height:40px'><span>Upload</span></label></div>", true),
        ("<label inert='false' for='upload' style='display:block;width:220px;height:40px'>Upload</label>", true),
    ] {
        let nodes = Arc::new(Mutex::new(Vec::new()));
        let mut browser = Browser::with_page_executor(
            &format!("<body style='margin:0'>{source}<input id='upload' type='file' aria-label='Upload'></body>"),
            Some(Box::new(LabelExecutor { nodes: nodes.clone(), prevent_at: None, replace_at: None })),
        );
        let upload = browser.snapshot().nodes.into_iter().find(|node| node.name.as_deref() == Some("Upload")).unwrap();
        let hint = browser.activate(browser.context(), 4.0, 20.0).unwrap();
        let dispatched = nodes.lock().unwrap().clone();
        if inert {
            assert!(hint.is_none(), "An inert label source cannot mint a native picker hint");
            assert!(!dispatched.contains(&upload.id));
            assert!(dispatched.len() <= 1, "Only the non-inert containing page can receive this click");
            assert_ne!(browser.state.focused_node, Some(upload.id));
        } else {
            assert_eq!(hint.unwrap().context.node_id, upload.id, "The control case establishes the pointer point");
            assert_eq!(dispatched.len(), 2);
            assert_eq!(dispatched[1], upload.id);
        }
    }
}

#[test]
fn inert_descendants_retarget_pointer_activation_to_an_active_label() {
    for child in [
        "<span inert style='display:block;width:220px;height:40px'>Inert text</span>",
        "<button inert type='button' style='width:220px;height:40px'>Inert button</button>",
        "<div inert><span style='display:block;width:220px;height:40px'>Inherited inert text</span></div>",
    ] {
        let nodes = Arc::new(Mutex::new(Vec::new()));
        let mut browser = Browser::with_page_executor(
            &format!("<body style='margin:0'><label for='upload' role='button' aria-label='Active file label' style='display:block;width:220px;height:40px'>{child}</label><input id='upload' type='file' aria-label='Upload'></body>"),
            Some(Box::new(LabelExecutor { nodes: nodes.clone(), prevent_at: None, replace_at: None })),
        );
        let snapshot = browser.snapshot();
        let label = snapshot.nodes.iter().find(|node| node.name.as_deref() == Some("Active file label")).unwrap();
        let upload = snapshot.nodes.iter().find(|node| node.name.as_deref() == Some("Upload")).unwrap();
        let hint = browser.activate(browser.context(), 4.0, 20.0).unwrap().expect("Pointer-transparent descendants leave the active label operable");
        assert_eq!(hint.context.node_id, upload.id);
        assert_eq!(*nodes.lock().unwrap(), [label.id, upload.id]);
    }
}

#[test]
fn inert_link_subtrees_are_transparent_to_public_pointer_hit_testing() {
    for (markup, navigates) in [
        ("<a href='/next' style='display:block;width:220px;height:40px'>Active</a>", true),
        ("<a inert href='/next' style='display:block;width:220px;height:40px'>Inert</a>", false),
        ("<div inert><a href='/next' style='display:block;width:220px;height:40px'>Inherited inert</a></div>", false),
        ("<a href='/next' style='display:block;width:220px;height:40px'><span inert style='display:block;width:220px;height:40px'>Active ancestor</span></a>", true),
    ] {
        let mut page = Page::new(500.0, 300.0);
        page.load_html_str(&format!("<body style='margin:0'>{markup}</body>"), Some("https://example.test/form".into()));
        assert_eq!(page.click(4.0, 20.0), navigates.then(|| "https://example.test/next".to_string()));
    }
}

#[test]
fn inert_overlays_leave_the_next_live_fragment_available_for_pointer_activation() {
    for overlay in [
        "<a inert href='/inert-must-not-navigate' style='display:block;width:220px;height:40px;margin-top:-40px;background-color:#0000ff'>Inert overlay</a>",
        "<div inert style='display:block;width:220px;height:40px;margin-top:-40px;background-color:#0000ff'><button type='button' style='width:120px;height:40px'>Inert button</button></div>",
    ] {
        let mut page = Page::new(500.0, 300.0);
        page.load_html_str(
            &format!("<body style='margin:0'><div style='width:220px;height:40px'><a href='/live-underlay' style='display:block;width:220px;height:40px;background-color:#00ff00'>Live underlay</a>{overlay}</div></body>"),
            Some("https://example.test/form".into()),
        );
        assert_eq!(page.render_visible().get_pixel(180, 20), [0, 0, 255, 255], "The inert layer remains painted above the live link");
        assert_eq!(page.click(180.0, 20.0), Some("https://example.test/live-underlay".into()));
    }
}

#[test]
fn inert_document_roots_do_not_dispatch_pointer_listeners() {
    for html in [
        "<html inert><body style='margin:0'><input type='file' style='display:block;width:220px;height:40px'></body></html>",
        "<html inert><body style='margin:0'><label for='upload' style='display:block;width:220px;height:40px'>Upload</label><input id='upload' type='file'></body></html>",
    ] {
        let nodes = Arc::new(Mutex::new(Vec::new()));
        let mut browser = Browser::with_page_executor(html, Some(Box::new(LabelExecutor {
            nodes: nodes.clone(), prevent_at: None, replace_at: None,
        })));
        assert!(browser.activate(browser.context(), 4.0, 20.0).unwrap().is_none());
        assert!(nodes.lock().unwrap().is_empty());
        assert_eq!(browser.state.focused_node, None);
    }
}

#[test]
fn pointer_hit_testing_prioritizes_later_painted_siblings_and_skips_inert_layers() {
    for (inert, path) in [("", "/painted-top"), ("inert", "/underlay")] {
        let mut page = Page::new(500.0, 300.0);
        page.load_html_str(
            &format!("<body style='margin:0'><div style='width:220px;height:40px'><a href='/underlay' style='display:block;width:220px;height:40px;background-color:#00ff00'>Underlay</a><a {inert} href='/painted-top' style='display:block;width:220px;height:40px;margin-top:-40px;background-color:#0000ff'>Top</a></div></body>"),
            Some("https://example.test/form".into()),
        );
        assert_eq!(
            page.render_visible().get_pixel(180, 20),
            [0, 0, 255, 255],
            "The same pointer point lies on the later-painted blue layer"
        );
        assert_eq!(
            page.click(180.0, 20.0),
            Some(format!("https://example.test{path}"))
        );
    }
}
