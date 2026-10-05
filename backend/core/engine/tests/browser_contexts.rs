// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_engine::{BrowserContextId, TabManager, WindowId};
use blueice_ipc::viewport::DisplayViewport;

fn viewport() -> DisplayViewport {
    DisplayViewport {
        width: 300.0,
        height: 200.0,
        device_scale: 1.0,
        backing_scale: None,
    }
}

#[test]
fn contexts_own_windows_tabs_and_groups_without_replacing_existing_pages() {
    let mut tabs = TabManager::new(300.0, 200.0);
    let default = BrowserContextId::from_u64(1);
    let original = tabs.default_tab();
    tabs.get_mut(original).unwrap().load_html_str(
        "<input value='retained 中文'>",
        Some("https://context.test/original".into()),
    );
    let generation = tabs.document_epoch(original);
    let work = tabs.create_context(" Work ".into()).unwrap();
    assert_eq!(tabs.context(work).unwrap().name(), "Work");
    assert_eq!(tabs.context_windows(work).count(), 0);
    assert_eq!(
        tabs.ids().count(),
        1,
        "Creating a context must not create another Page"
    );
    let window = tabs.create_window_in_context(work, viewport()).unwrap();
    let tab = tabs.open_tab_in_window(window).unwrap();
    let group = tabs
        .create_group_in_context(work, "Research".into(), "#4477cc".into())
        .unwrap();
    tabs.assign_tab_group(tab, Some(group)).unwrap();
    assert_eq!(tabs.tab_context(original), Some(default));
    assert_eq!(tabs.window_context(window), Some(work));
    assert_eq!(tabs.tab_context(tab), Some(work));
    assert_eq!(tabs.group(group).unwrap().context_id(), work);
    assert!(tabs.move_tab_to_window(original, window).is_err());
    assert!(tabs.assign_tab_group(original, Some(group)).is_err());
    assert_eq!(tabs.tab_window(original), Some(WindowId::from_u64(1)));
    assert_eq!(tabs.tab_group(original), None);
    assert_eq!(tabs.document_epoch(original), generation);
    assert_eq!(
        tabs.get(original).unwrap().url(),
        Some("https://context.test/original")
    );
    let second = tabs.create_window_in_context(work, viewport()).unwrap();
    tabs.move_tab_to_window(tab, second).unwrap();
    assert_eq!(tabs.tab_group(tab), Some(group));
    assert_eq!(tabs.tab_context(tab), Some(work));
}

#[test]
fn closing_a_context_retires_only_its_members_and_never_reuses_ids() {
    let mut tabs = TabManager::new(300.0, 200.0);
    let default = BrowserContextId::from_u64(1);
    let work = tabs.create_context("Work".into()).unwrap();
    let first = tabs.create_window_in_context(work, viewport()).unwrap();
    let second = tabs.create_window_in_context(work, viewport()).unwrap();
    let tab = tabs.open_tab_in_window(first).unwrap();
    let other = tabs.open_tab_in_window(second).unwrap();
    let group = tabs
        .create_group_in_context(work, "Shared".into(), "#4477cc".into())
        .unwrap();
    tabs.assign_tab_group(tab, Some(group)).unwrap();
    let retired = tabs.close_context(work).unwrap();
    assert_eq!(retired.windows, [first, second]);
    assert_eq!(retired.tabs, [tab, other]);
    assert_eq!(retired.groups, [group]);
    assert!(tabs.get(tabs.default_tab()).is_some());
    assert!(tabs.get(tab).is_none() && tabs.get(other).is_none());
    assert!(tabs.group(group).is_none());
    assert!(tabs.window_viewport(first).is_none());
    assert!(tabs.close_context(work).is_err());
    assert!(tabs.rename_context(work, "Late".into()).is_err());
    assert!(tabs.create_window_in_context(work, viewport()).is_err());
    assert!(tabs.close_context(default).is_err());
    let later = tabs.create_context("Work".into()).unwrap();
    assert!(later.as_u64() > work.as_u64());
    let later_window = tabs.create_window_in_context(later, viewport()).unwrap();
    let later_tab = tabs.open_tab_in_window(later_window).unwrap();
    assert!(later_window.as_u64() > second.as_u64());
    assert!(later_tab.as_u64() > other.as_u64());
}

#[test]
fn invalid_duplicate_and_excess_context_names_leave_registry_unchanged() {
    let mut tabs = TabManager::new(300.0, 200.0);
    for name in [
        "".to_string(),
        " \n ".into(),
        "Hidden\0name".into(),
        "a".repeat(257),
        "default".into(),
    ] {
        assert!(tabs.create_context(name).is_err());
    }
    let work = tabs.create_context("Cafe\u{301}".into()).unwrap();
    assert_eq!(work.as_u64(), 2);
    assert_eq!(tabs.context(work).unwrap().name(), "Café");
    assert!(tabs.create_context("CAFÉ".into()).is_err());
    assert!(tabs.rename_context(work, "Default".into()).is_err());
    assert_eq!(tabs.context(work).unwrap().name(), "Café");
    tabs.rename_context(work, " 個人 ".into()).unwrap();
    assert_eq!(tabs.context(work).unwrap().name(), "個人");
    for i in 0..14 {
        tabs.create_context(format!("Context {i}")).unwrap();
    }
    assert_eq!(tabs.contexts().count(), 16);
    assert!(tabs.create_context("Overflow".into()).is_err());
    assert_eq!(tabs.contexts().count(), 16);
}

#[test]
fn debugger_realms_report_context_ownership_and_reject_a_foreign_context_id() {
    use blueice_engine::debugger::handle_debugger_request;
    use blueice_ipc::debugger::{DebuggerErrorCode, DebuggerReply, DebuggerRequest};
    let mut tabs = TabManager::new(300.0, 200.0);
    let work = tabs.create_context("Work".into()).unwrap();
    let window = tabs.create_window_in_context(work, viewport()).unwrap();
    let tab = tabs.open_tab_in_window(window).unwrap();
    tabs.get_mut(tab)
        .unwrap()
        .load_html_str("<p>Work page</p>", Some("https://context.test/work".into()));
    let DebuggerReply::PageRealms(realms) =
        handle_debugger_request(&tabs, DebuggerRequest::ListPageRealms)
    else {
        panic!("realms")
    };
    let realm = *realms
        .iter()
        .find(|realm| realm.tab_id == tab.as_u64())
        .unwrap();
    assert_eq!(realm.browser_context_id, work.as_u64());
    assert!(!matches!(
        handle_debugger_request(&tabs, DebuggerRequest::ListPrograms { realm }),
        DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            ..
        }
    ));
    let mut foreign = realm;
    foreign.browser_context_id = 1;
    assert!(matches!(
        handle_debugger_request(&tabs, DebuggerRequest::ListPrograms { realm: foreign }),
        DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            ..
        }
    ));
}

#[cfg(unix)]
mod wire {
    use super::*;
    use blueice_engine::session::run_session;
    use blueice_ipc::browser_contexts::{ContextAction, ContextEvent};
    use blueice_ipc::windows::{WindowAction, WindowEvent};
    use blueice_ipc::{ClientMessage, ServerMessage};
    use std::os::unix::net::UnixStream;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Duration;

    struct Browser {
        client: UnixStream,
        worker: Option<std::thread::JoinHandle<()>>,
        root: std::path::PathBuf,
        request: u64,
    }
    impl Browser {
        fn new() -> Self {
            Self::with_editor(None)
        }
        fn with_editor(html: Option<&'static str>) -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let root = std::env::temp_dir().join(format!(
                "blueice-context-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&root).unwrap();
            let (mut client, mut server) = UnixStream::pair().unwrap();
            client
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let directory = root.clone();
            let worker = std::thread::spawn(move || {
                let mut tabs = TabManager::new(300.0, 200.0);
                if let Some(html) = html {
                    tabs.get_mut(tabs.default_tab())
                        .unwrap()
                        .load_html_str(html, None);
                }
                run_session(
                    &mut tabs,
                    &mut server,
                    &directory,
                    &mut 0,
                    &directory.join("unavailable.sock"),
                )
                .unwrap();
            });
            blueice_ipc::client_handshake(&mut client).unwrap();
            Self {
                client,
                worker: Some(worker),
                root,
                request: 0,
            }
        }
        fn request(&mut self, tab: Option<u64>, message: ClientMessage) -> ServerMessage {
            self.request += 1;
            blueice_ipc::write_client_message_with_ids(
                &mut self.client,
                tab,
                Some(self.request),
                &message,
            )
            .unwrap();
            loop {
                let (_, request, reply) =
                    blueice_ipc::read_server_message_with_ids(&mut self.client).unwrap();
                if let ServerMessage::FrameReady {
                    shm_path,
                    width,
                    height,
                    ..
                } = &reply
                {
                    assert_eq!(
                        std::fs::read(shm_path).unwrap().len(),
                        *width as usize * *height as usize * 4
                    );
                    continue;
                }
                if request == Some(self.request) {
                    return reply;
                }
            }
        }
        fn context(
            &mut self,
            action: ContextAction,
        ) -> blueice_ipc::browser_contexts::ContextState {
            let ServerMessage::BrowserContextState(state) =
                self.request(None, ClientMessage::BrowserContext(action))
            else {
                panic!("context state")
            };
            state
        }
        fn scoped(
            &mut self,
            context: u64,
            tab: Option<u64>,
            message: ClientMessage,
        ) -> ServerMessage {
            self.request(
                tab,
                ClientMessage::BrowserContext(ContextAction::Command {
                    context_id: context,
                    message: Box::new(message),
                }),
            )
        }
    }
    impl Drop for Browser {
        fn drop(&mut self) {
            let _ = blueice_ipc::write_client_message(&mut self.client, &ClientMessage::Shutdown);
            if let Some(worker) = self.worker.take() {
                worker.join().unwrap();
            }
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }
    #[test]
    fn accessibility_text_queries_preserve_context_and_window_ownership() {
        use blueice_ipc::accessibility::{
            AccessibilityTextAction, AccessibilityTextContext, AccessibilityTextResult,
        };
        let mut b = Browser::with_editor(Some("<input aria-label='Editor' value='owned'>"));
        let ServerMessage::TextInputState(state) =
            b.request(Some(1), ClientMessage::GetTextInputState)
        else {
            panic!("input state")
        };
        let ServerMessage::Representation(snapshot) =
            b.request(Some(1), ClientMessage::GetRepresentation)
        else {
            panic!("snapshot")
        };
        let node = snapshot
            .nodes
            .iter()
            .find(|n| n.name.as_deref() == Some("Editor"))
            .unwrap()
            .id;
        let message = ClientMessage::AccessibilityText {
            context: AccessibilityTextContext {
                version: 1,
                frame_source: state.frame_source,
                document_generation: state.document_generation,
                frame_generation: state.frame_generation,
                node_id: node,
            },
            action: AccessibilityTextAction::Inspect,
        };
        let wrapped = |window| {
            ClientMessage::Window(WindowAction::Command {
                window_id: window,
                message: Box::new(message.clone()),
            })
        };
        let ServerMessage::AccessibilityTextState(reply) = b.scoped(1, Some(1), wrapped(1)) else {
            panic!("owned AX query")
        };
        let AccessibilityTextResult::State(text) = reply.result else {
            panic!("text")
        };
        assert_eq!(text.text.as_deref(), Some("owned"));
        assert!(matches!(
            b.scoped(2, Some(1), wrapped(1)),
            ServerMessage::Error { .. }
        ));
        assert!(matches!(
            b.scoped(1, Some(1), wrapped(2)),
            ServerMessage::Error { .. }
        ));
    }

    #[test]
    fn scoped_commands_and_legacy_mutations_cannot_cross_context_membership() {
        let mut b = Browser::new();
        let context = b.context(ContextAction::Create {
            name: "Work".into(),
        });
        let ContextEvent::Created { context_id } = context.event else {
            panic!("created")
        };
        let ServerMessage::WindowState(state) = b.scoped(
            context_id,
            None,
            ClientMessage::Window(WindowAction::Create {
                viewport: viewport(),
            }),
        ) else {
            panic!("window")
        };
        let WindowEvent::Created { window_id } = state.event else {
            panic!("created window")
        };
        let ServerMessage::TabOpened { tab_id, .. } = b.scoped(
            context_id,
            None,
            ClientMessage::Window(WindowAction::OpenTab {
                window_id,
                url: Some("about:credits".into()),
            }),
        ) else {
            panic!("opened")
        };
        let ServerMessage::TabGroupCreated(group) = b.scoped(
            context_id,
            None,
            ClientMessage::CreateTabGroup {
                name: "Research".into(),
                color: "#4477cc".into(),
            },
        ) else {
            panic!("group")
        };
        assert!(matches!(
            b.scoped(
                context_id,
                Some(tab_id),
                ClientMessage::SetTabGroup {
                    group_id: Some(group.id)
                }
            ),
            ServerMessage::TabGroupAssigned { .. }
        ));
        for (tab, message) in [
            (Some(1), ClientMessage::GetRepresentation),
            (
                None,
                ClientMessage::RenameTabGroup {
                    group_id: group.id,
                    name: "Foreign".into(),
                },
            ),
            (
                Some(tab_id),
                ClientMessage::Window(WindowAction::MoveTab { window_id: 1 }),
            ),
            (
                Some(tab_id),
                ClientMessage::BrowserContext(ContextAction::List),
            ),
            (None, ClientMessage::Shutdown),
        ] {
            let scope = if matches!(message, ClientMessage::RenameTabGroup { .. }) {
                1
            } else {
                context_id
            };
            assert!(matches!(
                b.scoped(scope, tab, message),
                ServerMessage::Error { .. }
            ));
        }
        assert!(matches!(
            b.request(
                Some(1),
                ClientMessage::SetTabGroup {
                    group_id: Some(group.id)
                }
            ),
            ServerMessage::Error { .. }
        ));
        assert!(matches!(
            b.request(
                Some(1),
                ClientMessage::Window(WindowAction::MoveTab { window_id })
            ),
            ServerMessage::Error { .. }
        ));
        let ServerMessage::Tabs(tabs) = b.scoped(context_id, None, ClientMessage::ListTabs) else {
            panic!("tabs")
        };
        assert_eq!(tabs.iter().map(|tab| tab.id).collect::<Vec<_>>(), [tab_id]);
        let ServerMessage::TabGroups(groups) =
            b.scoped(context_id, None, ClientMessage::ListTabGroups)
        else {
            panic!("groups")
        };
        assert_eq!(
            groups.iter().map(|group| group.id).collect::<Vec<_>>(),
            [group.id]
        );
        let ServerMessage::TabGroups(groups) = b.request(None, ClientMessage::ListTabGroups) else {
            panic!("default groups")
        };
        assert!(groups.is_empty());
        let state = b.context(ContextAction::Close { context_id });
        assert!(matches!(state.event, ContextEvent::Closed { .. }));
        assert_eq!(state.contexts.len(), 1);
        assert!(matches!(
            b.scoped(context_id, Some(tab_id), ClientMessage::GetRepresentation),
            ServerMessage::Error { .. }
        ));
        assert!(matches!(
            b.request(Some(1), ClientMessage::GetHistoryState),
            ServerMessage::HistoryState { .. }
        ));
        let state = b.context(ContextAction::Create {
            name: "Work".into(),
        });
        let ContextEvent::Created { context_id: later } = state.event else {
            panic!("later")
        };
        assert!(later > context_id);
    }
}
