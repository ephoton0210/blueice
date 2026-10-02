// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_engine::{TabManager, WindowId};
use blueice_ipc::viewport::DisplayViewport;

fn viewport(width: f64, height: f64, scale: f64) -> DisplayViewport {
    DisplayViewport {
        width,
        height,
        device_scale: scale,
        backing_scale: None,
    }
}

#[test]
fn window_viewports_and_new_tabs_are_independent() {
    let mut tabs = TabManager::new(300.0, 200.0);
    let first = tabs.default_tab();
    let second_window = tabs.create_window(viewport(180.0, 120.0, 2.0)).unwrap();
    let second = tabs.open_tab_in_window(second_window).unwrap();
    assert_eq!(tabs.tab_window(first), Some(WindowId::from_u64(1)));
    assert_eq!(tabs.tab_window(second), Some(second_window));
    assert_eq!(tabs.get(second).unwrap().viewport_size(), (180.0, 120.0));
    tabs.configure_window_viewport(second_window, viewport(240.0, 160.0, 1.0))
        .unwrap();
    assert_eq!(tabs.get(first).unwrap().viewport_size(), (300.0, 200.0));
    assert_eq!(tabs.get(second).unwrap().viewport_size(), (240.0, 160.0));
    let third = tabs.open_tab_in_window(second_window).unwrap();
    assert_eq!(tabs.get(third).unwrap().viewport_size(), (240.0, 160.0));
    assert_eq!(
        tabs.window_tabs(second_window).collect::<Vec<_>>(),
        [second, third]
    );
}

#[test]
fn moving_a_tab_reuses_its_page_and_group_and_closing_is_scoped() {
    let mut tabs = TabManager::new(300.0, 200.0);
    let first = tabs.default_tab();
    tabs.get_mut(first).unwrap().load_html_str(
        "<input value='retained'>",
        Some("https://window.test/retained".into()),
    );
    let group = tabs.create_group("Research".into(), "#4477cc".into());
    tabs.set_tab_group(first, Some(group));
    let epoch = tabs.document_epoch(first);
    let target = tabs.create_window(viewport(180.0, 120.0, 2.0)).unwrap();
    let other = tabs.open_tab();
    tabs.move_tab_to_window(first, target).unwrap();
    assert_eq!(
        tabs.get(first).unwrap().url(),
        Some("https://window.test/retained")
    );
    assert_eq!(tabs.document_epoch(first), epoch);
    assert_eq!(tabs.tab_group(first), Some(group));
    assert_eq!(tabs.get(first).unwrap().viewport_size(), (180.0, 120.0));
    assert_eq!(tabs.close_window(WindowId::from_u64(1)).unwrap(), [other]);
    assert!(tabs.get(first).is_some());
    assert_eq!(tabs.tab_window(first), Some(target));
    let later = tabs.create_window(viewport(200.0, 150.0, 1.0)).unwrap();
    assert!(
        later.as_u64() > target.as_u64(),
        "Closed window identities must not be reused"
    );
}

#[test]
fn rejected_window_actions_leave_membership_and_viewports_unchanged() {
    let mut tabs = TabManager::new(300.0, 200.0);
    let first = tabs.default_tab();
    assert!(tabs.create_window(viewport(200.0, 150.0, 0.0)).is_err());
    assert!(tabs
        .move_tab_to_window(first, WindowId::from_u64(99))
        .is_err());
    assert!(tabs
        .configure_window_viewport(WindowId::from_u64(1), viewport(3000.0, 200.0, 2.0))
        .is_err());
    assert_eq!(tabs.tab_window(first), Some(WindowId::from_u64(1)));
    assert_eq!(tabs.get(first).unwrap().viewport_size(), (300.0, 200.0));
    let second = tabs.create_window(viewport(200.0, 150.0, 1.0)).unwrap();
    assert_eq!(second.as_u64(), 2);
    tabs.close_window(second).unwrap();
    assert!(tabs.open_tab_in_window(second).is_err());
    assert!(tabs.close_window(second).is_err());
    assert!(tabs
        .configure_window_viewport(second, viewport(200.0, 150.0, 1.0))
        .is_err());
}

#[cfg(unix)]
mod wire {
    use super::*;
    use blueice_engine::session::run_session;
    use blueice_engine::HistorySnapshotMode;
    use blueice_ipc::input::{TextInputAction, TextInputContext, TextInputState};
    use blueice_ipc::windows::{WindowAction, WindowEvent, WindowState};
    use blueice_ipc::{ClientMessage, ServerMessage};
    use std::collections::HashMap;
    use std::os::unix::net::UnixStream;
    use std::path::PathBuf;
    use std::thread::JoinHandle;
    use std::time::Duration;

    struct Browser {
        client: UnixStream,
        worker: Option<JoinHandle<()>>,
        root: PathBuf,
        request: u64,
        frames: HashMap<u64, usize>,
        last_windows: Option<WindowState>,
    }
    impl Browser {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "bi-windows-{}-{}",
                std::process::id(),
                rand_id()
            ));
            std::fs::create_dir(&root).unwrap();
            let (mut client, mut server) = UnixStream::pair().unwrap();
            client
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut tabs = TabManager::new_with_history_snapshot_mode(
                300.0,
                200.0,
                HistorySnapshotMode::Snapshot,
            );
            tabs.get_mut(tabs.default_tab()).unwrap().load_html_str(
                "<input aria-label='Editor' value='frost' style='width:100px;height:32px'>",
                Some("https://window.test/editor".into()),
            );
            let directory = root.clone();
            let worker = std::thread::spawn(move || {
                run_session(
                    &mut tabs,
                    &mut server,
                    &directory,
                    &mut 0,
                    &directory.join("unused.sock"),
                )
                .unwrap()
            });
            blueice_ipc::client_handshake(&mut client).unwrap();
            Self {
                client,
                worker: Some(worker),
                root,
                request: 0,
                frames: HashMap::new(),
                last_windows: None,
            }
        }
        fn request(&mut self, tab: Option<u64>, command: ClientMessage) -> ServerMessage {
            let payload =
                if let ClientMessage::Window(WindowAction::Command { message, .. }) = &command {
                    message.as_ref()
                } else {
                    &command
                };
            let expected = match payload {
                ClientMessage::Window(WindowAction::OpenTab { .. }) => 0,
                ClientMessage::Window(_) => 1,
                ClientMessage::CloseTab => 6,
                ClientMessage::GetViewportState
                | ClientMessage::SetViewport { .. }
                | ClientMessage::SetPageZoom { .. }
                | ClientMessage::Resize { .. }
                | ClientMessage::Click { .. } => 2,
                ClientMessage::GetTextInputState | ClientMessage::TextInput { .. } => 3,
                ClientMessage::GetRepresentation => 4,
                _ => 5,
            };
            self.request += 1;
            blueice_ipc::write_client_message_with_ids(
                &mut self.client,
                tab,
                Some(self.request),
                &command,
            )
            .unwrap();
            loop {
                let (id, request, message) =
                    blueice_ipc::read_server_message_with_ids(&mut self.client)
                        .unwrap_or_else(|error| panic!("{command:?}: {error}"));
                if let ServerMessage::FrameReady {
                    shm_path,
                    width,
                    height,
                    ..
                } = message
                {
                    assert_eq!(
                        std::fs::read(shm_path).unwrap().len(),
                        width as usize * height as usize * 4
                    );
                    *self.frames.entry(id.unwrap()).or_default() += 1;
                    continue;
                }
                if let ServerMessage::WindowState(state) = &message {
                    self.last_windows = Some(state.clone());
                }
                if request != Some(self.request) {
                    continue;
                }
                if matches!(
                    message,
                    ServerMessage::Error { .. } | ServerMessage::GatekeeperBlocked { .. }
                ) {
                    return message;
                }
                let actual = match message {
                    ServerMessage::TabOpened { .. } => 0,
                    ServerMessage::WindowState(_) => 1,
                    ServerMessage::ViewportState(_) => 2,
                    ServerMessage::TextInputState(_) => 3,
                    ServerMessage::Representation(_) => 4,
                    ServerMessage::Navigated { .. } => 5,
                    ServerMessage::TabClosed { .. } => 6,
                    _ => continue,
                };
                if actual == expected {
                    return message;
                }
            }
        }
        fn window(&mut self, action: WindowAction, tab: Option<u64>) -> WindowState {
            let ServerMessage::WindowState(state) =
                self.request(tab, ClientMessage::Window(action))
            else {
                panic!("window state")
            };
            state
        }
        fn state(&mut self, tab: u64) -> blueice_ipc::viewport::ViewportState {
            let ServerMessage::ViewportState(state) =
                self.request(Some(tab), ClientMessage::GetViewportState)
            else {
                panic!("viewport")
            };
            state
        }
        fn input(&mut self, tab: u64) -> TextInputState {
            let ServerMessage::TextInputState(state) =
                self.request(Some(tab), ClientMessage::GetTextInputState)
            else {
                panic!("input")
            };
            state
        }
        fn context(state: &TextInputState) -> TextInputContext {
            TextInputContext {
                version: state.version,
                frame_source: state.frame_source,
                document_generation: state.document_generation,
                focus_generation: state.focus_generation,
            }
        }
    }
    fn rand_id() -> u64 {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        NEXT.fetch_add(1, Ordering::Relaxed)
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
    fn core_window_viewport_and_legacy_resize_repaint_only_the_addressed_window() {
        let mut b = Browser::new();
        let first = b.window(WindowAction::List, None);
        assert_eq!(first.windows[0].tabs[0].id, 1);
        let created = b.window(
            WindowAction::Create {
                viewport: viewport(180.0, 120.0, 2.0),
            },
            None,
        );
        assert_eq!(created.event, WindowEvent::Created { window_id: 2 });
        assert!(matches!(
            b.request(
                None,
                ClientMessage::Window(WindowAction::OpenTab {
                    window_id: 2,
                    url: Some("about:credits".into())
                })
            ),
            ServerMessage::TabOpened { tab_id: 2, .. }
        ));
        let second = b.state(2);
        assert_eq!((second.width, second.pixel_width), (180.0, 360));
        let frames = b.frames[&2];
        b.window(
            WindowAction::Resize {
                window_id: 1,
                viewport: viewport(320.0, 220.0, 1.0),
            },
            None,
        );
        assert_eq!(b.state(1).width, 320.0);
        assert_eq!(b.state(2).width, 180.0);
        assert_eq!(
            b.frames[&2], frames,
            "Other windows must not receive a resize frame"
        );
        b.request(
            Some(2),
            ClientMessage::Resize {
                width: 240,
                height: 160,
            },
        );
        assert_eq!(b.state(2).width, 240.0);
        assert_eq!(b.state(1).width, 320.0);
        b.request(
            Some(2),
            ClientMessage::SetViewport {
                viewport: viewport(260.0, 170.0, 2.0),
            },
        );
        assert_eq!(b.state(2).pixel_width, 520);
        assert_eq!(b.state(1).width, 320.0);
    }
    #[test]
    fn wire_transfer_preserves_document_selection_zoom_history_and_fences_old_input() {
        let mut b = Browser::new();
        b.window(WindowAction::List, None);
        b.request(
            Some(1),
            ClientMessage::SetViewport {
                viewport: viewport(300.0, 200.0, 1.0),
            },
        );
        b.input(1); // Native clients opt into input metadata before pointer editing.
        b.request(Some(1), ClientMessage::Click { x: 10.0, y: 10.0 });
        let context = Browser::context(&b.input(1));
        b.request(
            Some(1),
            ClientMessage::TextInput {
                context,
                action: TextInputAction::SelectAll,
            },
        );
        b.request(
            Some(1),
            ClientMessage::TextInput {
                context,
                action: TextInputAction::Replace {
                    text: "Moved 中文".into(),
                    replacement: None,
                },
            },
        );
        b.request(
            Some(1),
            ClientMessage::Navigate {
                url: "about:credits".into(),
            },
        );
        b.request(Some(1), ClientMessage::GoBack);
        b.request(Some(1), ClientMessage::Click { x: 10.0, y: 10.0 });
        b.request(Some(1), ClientMessage::SetPageZoom { zoom: 1.5 });
        let before = b.input(1);
        assert_eq!(
            before.focused.as_ref().unwrap().text.as_deref(),
            Some("Moved 中文")
        );
        let context = Browser::context(&before);
        b.window(
            WindowAction::Create {
                viewport: viewport(180.0, 120.0, 2.0),
            },
            None,
        );
        let moved = b.window(WindowAction::MoveTab { window_id: 2 }, Some(1));
        assert_eq!(
            moved.event,
            WindowEvent::TabMoved {
                tab_id: 1,
                from_window: 1,
                to_window: 2
            }
        );
        assert!(moved.windows[0].tabs.is_empty());
        assert_eq!(moved.windows[1].tabs[0].id, 1);
        for message in [
            ClientMessage::Click { x: 10.0, y: 10.0 },
            ClientMessage::GetTextInputState,
        ] {
            assert!(
                matches!(
                    b.request(
                        Some(1),
                        ClientMessage::Window(WindowAction::Command {
                            window_id: 1,
                            message: Box::new(message)
                        })
                    ),
                    ServerMessage::Error { .. }
                ),
                "Queued source-window commands must not reach the moved page"
            );
        }
        assert!(
            matches!(
                b.request(
                    Some(1),
                    ClientMessage::Window(WindowAction::Command {
                        window_id: 2,
                        message: Box::new(ClientMessage::Window(WindowAction::List))
                    })
                ),
                ServerMessage::Error { .. }
            ),
            "Nested window commands must be rejected"
        );
        assert!(matches!(
            b.request(
                Some(1),
                ClientMessage::Window(WindowAction::Command {
                    window_id: 2,
                    message: Box::new(ClientMessage::GetTextInputState)
                })
            ),
            ServerMessage::TextInputState(_)
        ));
        let after = b.input(1);
        assert_eq!(after.document_generation, before.document_generation);
        assert_eq!(
            after.focused.as_ref().unwrap().text,
            before.focused.as_ref().unwrap().text
        );
        assert_eq!(
            after.focused.as_ref().unwrap().selection,
            before.focused.as_ref().unwrap().selection
        );
        assert_ne!(after.focus_generation, before.focus_generation);
        let state = b.state(1);
        assert_eq!(
            (state.width, state.zoom, state.pixel_width),
            (180.0, 1.5, 360)
        );
        assert!(matches!(
            b.request(
                Some(1),
                ClientMessage::TextInput {
                    context,
                    action: TextInputAction::Replace {
                        text: "late key".into(),
                        replacement: None
                    }
                }
            ),
            ServerMessage::Error { .. }
        ));
        assert_eq!(
            b.input(1).focused.unwrap().text.as_deref(),
            Some("Moved 中文")
        );
        b.request(Some(1), ClientMessage::GoForward);
        assert_eq!(b.state(1).width, 180.0);
        assert_eq!(b.state(1).zoom, 1.5);
        b.request(Some(1), ClientMessage::GoBack);
        assert_eq!(b.state(1).width, 180.0);
        assert_eq!(
            b.input(1).focused.unwrap().text.as_deref(),
            Some("Moved 中文")
        );
    }
    #[test]
    fn denied_new_page_publishes_ownership_before_its_navigation_rejection() {
        let mut b = Browser::new();
        b.window(
            WindowAction::Create {
                viewport: viewport(180.0, 120.0, 1.0),
            },
            None,
        );
        let gate = std::os::unix::net::UnixListener::bind(b.root.join("unused.sock")).unwrap();
        let review = std::thread::spawn(move || {
            let (mut stream, _) = gate.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let request = blueice_ipc::gatekeeper::read_gatekeeper_request(&mut stream).unwrap();
            assert!(
                matches!(request, blueice_ipc::gatekeeper::GatekeeperRequest::CheckUrl { url } if url == "http://127.0.0.1:9/never-fetch")
            );
            blueice_ipc::gatekeeper::write_gatekeeper_reply(
                &mut stream,
                &blueice_ipc::gatekeeper::GatekeeperReply::Rejected {
                    reason: "Window policy test".into(),
                    category: "test".into(),
                },
            )
            .unwrap();
        });
        assert!(matches!(
            b.request(
                None,
                ClientMessage::Window(WindowAction::OpenTab {
                    window_id: 2,
                    url: Some("http://127.0.0.1:9/never-fetch".into())
                })
            ),
            ServerMessage::GatekeeperBlocked { .. }
        ));
        review.join().unwrap();
        let state = b
            .last_windows
            .as_ref()
            .expect("Native ownership must precede denied review");
        assert_eq!(
            state.event,
            WindowEvent::TabOpened {
                tab_id: 2,
                window_id: 2
            }
        );
        assert_eq!(state.windows[1].tabs[0].id, 2);
        assert!(state.windows[1].tabs[0].url.is_none());
        b.request(Some(2), ClientMessage::CloseTab);
        let state = b.window(WindowAction::List, None);
        assert!(state.windows[1].tabs.is_empty());
        assert_eq!(state.windows[0].tabs[0].id, 1);
    }
    #[test]
    fn closed_and_invalid_windows_leave_live_membership_and_allow_new_monotonic_windows() {
        let mut b = Browser::new();
        b.window(WindowAction::List, None);
        b.window(
            WindowAction::Create {
                viewport: viewport(180.0, 120.0, 1.0),
            },
            None,
        );
        b.request(
            None,
            ClientMessage::Window(WindowAction::OpenTab {
                window_id: 2,
                url: None,
            }),
        );
        for action in [
            WindowAction::MoveTab { window_id: 99 },
            WindowAction::Resize {
                window_id: 1,
                viewport: viewport(3000.0, 200.0, 2.0),
            },
            WindowAction::Close { window_id: 99 },
        ] {
            assert!(matches!(
                b.request(Some(1), ClientMessage::Window(action)),
                ServerMessage::Error { .. }
            ));
        }
        let closed = b.window(WindowAction::Close { window_id: 1 }, None);
        assert_eq!(closed.windows.len(), 1);
        assert_eq!(closed.windows[0].tabs[0].id, 2);
        assert!(matches!(
            b.request(None, ClientMessage::OpenTab { url: None }),
            ServerMessage::Error { .. }
        ));
        assert!(matches!(
            b.request(Some(1), ClientMessage::GetViewportState),
            ServerMessage::Error { .. }
        ));
        b.window(WindowAction::Close { window_id: 2 }, None);
        let created = b.window(
            WindowAction::Create {
                viewport: viewport(200.0, 150.0, 1.0),
            },
            None,
        );
        assert_eq!(created.windows[0].id, 3);
    }
}
