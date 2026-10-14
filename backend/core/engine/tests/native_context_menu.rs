// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![cfg(unix)]

use blueice_engine::{session::run_session, TabManager};
use blueice_ipc::context_menu::{ContextMenuContext, ContextMenuLinkAction, ContextMenuState};
use blueice_ipc::input::{TextInputAction, TextInputContext};
use blueice_ipc::{AiSnapshot, ClientMessage, ServerMessage};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

struct Browser {
    client: UnixStream,
    worker: Option<JoinHandle<()>>,
    root: PathBuf,
    request: u64,
}
impl Browser {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "bi-menu-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        let mut tabs = TabManager::new(480.0, 400.0);
        tabs.get_mut(tabs.default_tab()).unwrap().load_html_str(
            "<a aria-label='Nested link' href='/next' style='display:block;width:240px'><b>Nested link</b></a><a href='about:credits' style='display:block'>Credits</a><a href='javascript:alert(1)' style='display:block'>Unsafe</a><input aria-label='Editor' value='hello' style='display:block;width:240px;height:30px'><input aria-label='Secret' type='password' value='private-menu-secret' style='display:block;width:240px;height:30px'><input aria-label='Readonly' readonly value='read only' style='display:block;width:240px;height:30px'><input aria-label='Disabled' disabled value='disabled' style='display:block;width:240px;height:30px'><a href='/hidden' style='opacity:0;display:block'>Invisible</a>",
            Some("https://menu.test/start".into()));
        tabs.open_tab();
        let (mut client, mut server) = UnixStream::pair().unwrap();
        client
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let directory = root.clone();
        let worker = thread::spawn(move || {
            run_session(
                &mut tabs,
                &mut server,
                &directory,
                &mut 0,
                &directory.join("missing-review.sock"),
            )
            .unwrap()
        });
        blueice_ipc::client_handshake(&mut client).unwrap();
        Self {
            client,
            worker: Some(worker),
            root,
            request: 0,
        }
    }
    fn request(&mut self, tab: u64, command: ClientMessage) -> ServerMessage {
        self.request += 1;
        blueice_ipc::write_client_message_with_ids(
            &mut self.client,
            Some(tab),
            Some(self.request),
            &command,
        )
        .unwrap();
        loop {
            let (reply_tab, request, message) =
                blueice_ipc::read_server_message_with_ids(&mut self.client).unwrap();
            if matches!(
                message,
                ServerMessage::FrameReady { .. } | ServerMessage::NavigationStarted { .. }
            ) {
                continue;
            }
            assert_eq!(request, Some(self.request));
            if !matches!(
                message,
                ServerMessage::TabOpened { .. } | ServerMessage::Tabs(_)
            ) {
                assert_eq!(reply_tab, Some(tab));
            }
            return message;
        }
    }
    fn snapshot(&mut self) -> AiSnapshot {
        let ServerMessage::Representation(value) =
            self.request(1, ClientMessage::GetRepresentation)
        else {
            panic!("snapshot")
        };
        value
    }
    fn menu(&mut self, name: &str) -> ContextMenuState {
        let snapshot = self.snapshot();
        let node = snapshot
            .nodes
            .iter()
            .find(|node| node.name.as_deref() == Some(name))
            .unwrap();
        let ServerMessage::ContextMenu(value) = self.request(
            1,
            ClientMessage::GetContextMenu {
                tab_id: 1,
                frame_source: snapshot.frame_source,
                frame_generation: snapshot.generation,
                // Hit the nested text itself rather than empty anchor width.
                x: node.bounds.x + 2.0,
                y: node.bounds.y + node.bounds.height / 2.0 - snapshot.scroll_y,
            },
        ) else {
            panic!("menu state for {name}")
        };
        value
    }
    fn link(
        &mut self,
        tab: u64,
        context: ContextMenuContext,
        action: ContextMenuLinkAction,
    ) -> ServerMessage {
        self.request(tab, ClientMessage::ContextMenuLink { context, action })
    }
}
impl Drop for Browser {
    fn drop(&mut self) {
        let _ = self.client.shutdown(std::net::Shutdown::Both);
        self.worker.take().unwrap().join().unwrap();
        std::fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn nested_links_copy_without_navigation_and_open_retains_review() {
    let mut browser = Browser::new();
    let menu = browser.menu("Nested link");
    assert_eq!(menu.link_url.as_deref(), Some("https://menu.test/next"));
    assert!(menu.input.is_none());
    let ServerMessage::ContextMenuLink { context, url } =
        browser.link(1, menu.context, ContextMenuLinkAction::Copy)
    else {
        panic!("copy reply")
    };
    assert_eq!(context, menu.context);
    assert_eq!(url, "https://menu.test/next");
    assert_eq!(
        browser.snapshot().url.as_deref(),
        Some("https://menu.test/start")
    );
    assert!(matches!(
        browser.link(1, context, ContextMenuLinkAction::Open),
        ServerMessage::Error { .. } | ServerMessage::GatekeeperBlocked { .. }
    ));
    assert_eq!(
        browser.snapshot().url.as_deref(),
        Some("https://menu.test/start")
    );
    assert!(browser.menu("Unsafe").link_url.is_none());
}

#[test]
fn editor_menu_focus_is_not_click_and_preserves_selection_and_privacy() {
    let mut browser = Browser::new();
    let first = browser.menu("Editor");
    let input = first.input.unwrap();
    let context = TextInputContext {
        version: input.version,
        frame_source: input.frame_source,
        document_generation: input.document_generation,
        focus_generation: input.focus_generation,
    };
    let ServerMessage::TextInputState(selected) = browser.request(
        1,
        ClientMessage::TextInput {
            context,
            action: TextInputAction::SelectAll,
        },
    ) else {
        panic!("selection")
    };
    let second = browser.menu("Editor");
    assert_eq!(
        second.input.unwrap().focused.unwrap().selection,
        selected.focused.unwrap().selection
    );
    let password = browser.menu("Secret").input.unwrap().focused.unwrap();
    assert!(password.protected);
    assert!(password.writable);
    assert!(password.text.is_none());
    let readonly = browser.menu("Readonly").input.unwrap().focused.unwrap();
    assert!(!readonly.writable);
    assert_eq!(readonly.text.as_deref(), Some("read only"));
    assert!(browser.menu("Disabled").input.is_none());
}

#[test]
fn stale_frame_document_source_cross_tab_and_closed_tab_cannot_act() {
    let mut browser = Browser::new();
    let menu = browser.menu("Nested link");
    let mut wrong = menu.context;
    wrong.frame_source ^= 1;
    assert!(matches!(
        browser.link(1, wrong, ContextMenuLinkAction::Copy),
        ServerMessage::Error { .. }
    ));
    assert!(matches!(
        browser.link(2, menu.context, ContextMenuLinkAction::Open),
        ServerMessage::Error { .. }
    ));
    let _ = browser.menu("Editor");
    assert!(matches!(
        browser.link(1, menu.context, ContextMenuLinkAction::Copy),
        ServerMessage::Error { .. }
    ));
    let fresh = browser.menu("Nested link");
    assert!(matches!(
        browser.request(
            1,
            ClientMessage::Navigate {
                url: "about:credits".into()
            }
        ),
        ServerMessage::Navigated { .. }
    ));
    assert!(matches!(
        browser.link(1, fresh.context, ContextMenuLinkAction::Open),
        ServerMessage::Error { .. }
    ));
    assert!(matches!(
        browser.request(2, ClientMessage::CloseTab),
        ServerMessage::TabClosed { .. }
    ));
    assert!(matches!(
        browser.link(2, fresh.context, ContextMenuLinkAction::Copy),
        ServerMessage::Error { .. }
    ));
}

#[test]
fn opening_link_in_new_tab_keeps_source_page_and_current_tab_open_works() {
    let mut browser = Browser::new();
    let menu = browser.menu("Credits");
    let ServerMessage::TabOpened { tab_id, url } =
        browser.link(1, menu.context, ContextMenuLinkAction::OpenInNewTab)
    else {
        panic!("new tab")
    };
    assert_eq!(tab_id, 3);
    assert_eq!(url.as_deref(), Some("about:credits"));
    assert_eq!(
        browser.snapshot().url.as_deref(),
        Some("https://menu.test/start")
    );
    let fresh = browser.menu("Credits");
    assert!(matches!(
        browser.link(1, fresh.context, ContextMenuLinkAction::Open),
        ServerMessage::Navigated { .. }
    ));
    assert_eq!(browser.snapshot().url.as_deref(), Some("about:credits"));
}

#[test]
fn context_query_rejects_outside_viewport_and_wrong_frame() {
    let mut browser = Browser::new();
    let snapshot = browser.snapshot();
    for (tab_id, source, generation, x, y) in [
        (2, snapshot.frame_source, snapshot.generation, 5.0, 5.0),
        (1, snapshot.frame_source ^ 1, snapshot.generation, 5.0, 5.0),
        (1, snapshot.frame_source, snapshot.generation + 1, 5.0, 5.0),
        (1, snapshot.frame_source, snapshot.generation, -1.0, 5.0),
        (1, snapshot.frame_source, snapshot.generation, 5.0, 401.0),
    ] {
        assert!(matches!(
            browser.request(
                1,
                ClientMessage::GetContextMenu {
                    tab_id,
                    frame_source: source,
                    frame_generation: generation,
                    x,
                    y
                }
            ),
            ServerMessage::Error { .. }
        ));
    }
}

#[test]
fn document_selection_round_trips_without_editor_leaks_and_rejects_foreign_tabs() {
    let mut browser = Browser::new();
    let editor = browser.menu("Editor").input.unwrap();
    let context = editor.context();
    let ServerMessage::TextInputState(state) = browser.request(
        1,
        ClientMessage::TextInput {
            context,
            action: TextInputAction::DocumentSelectAll,
        },
    ) else {
        panic!("document selection")
    };
    let text = state
        .document
        .as_ref()
        .unwrap()
        .selected_text
        .as_ref()
        .unwrap();
    assert!(text.contains("Nested link") && text.contains("Credits"));
    assert!(
        !text.contains("hello")
            && !text.contains("private-menu-secret")
            && !text.contains("Invisible")
    );
    assert_eq!(state.focused, editor.focused);
    let menu = browser.menu("Nested link");
    assert!(menu.input.is_none());
    assert_eq!(menu.document, state.document);
    assert!(matches!(
        browser.request(
            2,
            ClientMessage::TextInput {
                context: state.context(),
                action: TextInputAction::DocumentSelectAll,
            }
        ),
        ServerMessage::Error { .. }
    ));
    let ServerMessage::TextInputState(retained) =
        browser.request(1, ClientMessage::GetTextInputState)
    else {
        panic!("retained selection")
    };
    assert_eq!(retained.document, state.document);
    assert!(matches!(
        browser.request(
            1,
            ClientMessage::Navigate {
                url: "about:credits".into()
            }
        ),
        ServerMessage::Navigated { .. }
    ));
    assert!(matches!(
        browser.request(
            1,
            ClientMessage::TextInput {
                context: state.context(),
                action: TextInputAction::DocumentSelectAll,
            }
        ),
        ServerMessage::Error { .. }
    ));
    let ServerMessage::TextInputState(replaced) =
        browser.request(1, ClientMessage::GetTextInputState)
    else {
        panic!("new document")
    };
    assert!(!replaced.document.unwrap().active);
}

#[test]
fn deferred_native_click_rejects_stale_owners_and_keeps_reviewed_activation() {
    let mut browser = Browser::new();
    let ServerMessage::TextInputState(state) = browser.request(1, ClientMessage::GetTextInputState)
    else {
        panic!("input ownership")
    };
    let snapshot = browser.snapshot();
    let credits = snapshot
        .nodes
        .iter()
        .find(|node| node.name.as_deref() == Some("Credits"))
        .unwrap();
    let x = credits.bounds.x + 2.0;
    let y = credits.bounds.y + credits.bounds.height / 2.0;
    for context in [
        TextInputContext {
            version: 2,
            ..state.context()
        },
        TextInputContext {
            frame_source: state.frame_source ^ 1,
            ..state.context()
        },
        TextInputContext {
            document_generation: state.document_generation + 1,
            ..state.context()
        },
        TextInputContext {
            focus_generation: state.focus_generation + 1,
            ..state.context()
        },
    ] {
        assert!(matches!(
            browser.request(1, ClientMessage::NativeClick { context, x, y }),
            ServerMessage::Error { .. }
        ));
        assert_eq!(browser.snapshot().url, snapshot.url);
    }
    assert!(matches!(
        browser.request(
            1,
            ClientMessage::NativeClick {
                context: state.context(),
                x: 1e10,
                y
            }
        ),
        ServerMessage::Error { .. }
    ));
    let link = browser.menu("Nested link");
    assert!(matches!(
        browser.request(
            1,
            ClientMessage::NativeClick {
                context: state.context(),
                x: link.context.x,
                y: link.context.y
            }
        ),
        ServerMessage::Error { .. } | ServerMessage::GatekeeperBlocked { .. }
    ));
    assert_eq!(browser.snapshot().url, snapshot.url);
    let ServerMessage::TextInputState(current) =
        browser.request(1, ClientMessage::GetTextInputState)
    else {
        panic!("focus after reviewed activation")
    };
    assert!(matches!(
        browser.request(
            1,
            ClientMessage::NativeClick {
                context: current.context(),
                x,
                y
            }
        ),
        ServerMessage::Navigated { .. }
    ));
    assert!(matches!(
        browser.request(
            1,
            ClientMessage::NativeClick {
                context: state.context(),
                x,
                y
            }
        ),
        ServerMessage::Error { .. }
    ));
}
