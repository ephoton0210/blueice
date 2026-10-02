// This Source Code Form is subject to the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![cfg(unix)]

use blueice_engine::{session::run_session, HistorySnapshotMode, TabManager};
use blueice_ipc::find::{FindAction, FindState, MAX_FIND_MATCHES, MAX_FIND_QUERY_BYTES};
use blueice_ipc::input::TextInputContext;
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
    context: TextInputContext,
    request: u64,
    pixels: Vec<u8>,
    width: u32,
}

impl Browser {
    fn new(html: &str) -> Self {
        Self::with_history(html, HistorySnapshotMode::Reload)
    }
    fn with_history(html: &str, mode: HistorySnapshotMode) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "bi-find-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        let mut tabs = TabManager::new_with_history_snapshot_mode(420.0, 180.0, mode);
        tabs.get_mut(tabs.default_tab())
            .unwrap()
            .load_html_str(html, Some("https://find.test/one".into()));
        let second = tabs.open_tab();
        tabs.get_mut(second).unwrap().load_html_str(
            "<p>Other tab frost</p>",
            Some("https://find.test/two".into()),
        );
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
                &directory.join("unused.sock"),
            )
            .unwrap()
        });
        blueice_ipc::client_handshake(&mut client).unwrap();
        blueice_ipc::write_client_message_with_ids(
            &mut client,
            Some(1),
            Some(1),
            &ClientMessage::GetTextInputState,
        )
        .unwrap();
        let ServerMessage::TextInputState(state) =
            blueice_ipc::read_server_message(&mut client).unwrap()
        else {
            panic!("input context")
        };
        Self {
            client,
            worker: Some(worker),
            root,
            context: TextInputContext {
                version: state.version,
                frame_source: state.frame_source,
                document_generation: state.document_generation,
                focus_generation: state.focus_generation,
            },
            request: 1,
            pixels: Vec::new(),
            width: 0,
        }
    }
    fn request(&mut self, tab: u64, command: ClientMessage) -> ServerMessage {
        let frame_only = matches!(
            command,
            ClientMessage::Resize { .. } | ClientMessage::ActOn { .. }
        );
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
            if let ServerMessage::FrameReady {
                ref shm_path,
                width,
                height,
                ..
            } = message
            {
                if reply_tab != Some(tab) || request != Some(self.request) {
                    continue;
                }
                self.pixels = std::fs::read(shm_path).unwrap();
                self.width = width;
                assert_eq!(self.pixels.len(), width as usize * height as usize * 4);
                if frame_only {
                    return message;
                }
            } else {
                assert_eq!(reply_tab, Some(tab));
                assert_eq!(request, Some(self.request));
                return message;
            }
        }
    }
    fn find(&mut self, action: FindAction) -> FindState {
        let message = self.request(
            1,
            ClientMessage::Find {
                tab_id: 1,
                frame_source: self.context.frame_source,
                document_generation: self.context.document_generation,
                action,
            },
        );
        let ServerMessage::FindState(state) = message else {
            panic!("expected find state, got {message:?}")
        };
        state
    }
    fn update(&mut self, query: &str, case_sensitive: bool) -> FindState {
        self.find(FindAction::Update {
            query: query.into(),
            case_sensitive,
        })
    }
    fn state(&mut self, tab: u64) -> FindState {
        let ServerMessage::FindState(state) = self.request(tab, ClientMessage::GetFindState) else {
            panic!("find state")
        };
        state
    }
    fn snapshot(&mut self) -> AiSnapshot {
        let ServerMessage::Representation(state) =
            self.request(1, ClientMessage::GetRepresentation)
        else {
            panic!("snapshot")
        };
        state
    }
}
impl Drop for Browser {
    fn drop(&mut self) {
        let _ = self.client.shutdown(std::net::Shutdown::Both);
        let result = self.worker.take().unwrap().join();
        let _ = std::fs::remove_dir_all(&self.root);
        if !thread::panicking() {
            result.unwrap();
        }
    }
}

#[test]
fn find_counts_cycles_wraps_and_paints_exact_core_matches() {
    let mut browser = Browser::new("<p>frost FROST frost</p>");
    let first = browser.update("frost", false);
    assert_eq!(
        (first.match_count, first.active_match, first.wrapped),
        (3, Some(1), false)
    );
    assert_eq!(first.rects.len(), 1);
    assert!(first.rects[0].width > 20.0 && first.rects[0].width < 100.0);
    assert!(
        browser
            .pixels
            .chunks_exact(4)
            .any(|pixel| pixel[0] > 220 && (100..190).contains(&pixel[1]) && pixel[2] < 70),
        "active orange outline must be rasterized in the shared frame"
    );
    let second = browser.find(FindAction::Next { backwards: false });
    assert_eq!(second.active_match, Some(2));
    assert!(second.rects[0].x > first.rects[0].x);
    assert_eq!(
        browser
            .find(FindAction::Next { backwards: true })
            .active_match,
        Some(1)
    );
    let last = browser.find(FindAction::Next { backwards: true });
    assert_eq!((last.active_match, last.wrapped), (Some(3), true));
    assert_eq!(
        browser
            .find(FindAction::Next { backwards: false })
            .active_match,
        Some(1)
    );
    assert_eq!(browser.update("frost", true).match_count, 2);
    let close = browser.find(FindAction::Close);
    assert_eq!(
        (close.match_count, close.active_match, close.query.as_str()),
        (0, None, "")
    );
    assert!(!browser
        .pixels
        .chunks_exact(4)
        .any(|pixel| pixel[0] > 220 && (100..190).contains(&pixel[1]) && pixel[2] < 70));
}

#[test]
fn find_matches_soft_wrapping_unicode_and_literal_punctuation() {
    let mut browser = Browser::new("<p style='width:100px'>snow <b>crystal</b> across lines</p><p>Cafe\u{301} CAFÉ σ ς Σ [a.*]</p><p>block</p><p>boundary</p>");
    let across = browser.update("snow   crystal", false);
    assert_eq!(across.match_count, 1);
    assert!(across.rects.len() >= 2);
    assert!(across.rects[1].y > across.rects[0].y);
    assert_eq!(browser.update("café", false).match_count, 2);
    assert_eq!(browser.update("σ", false).match_count, 3);
    assert_eq!(browser.update("[a.*]", false).match_count, 1);
    assert_eq!(browser.update("block boundary", false).match_count, 0);
    assert_eq!(browser.update("", false).active_match, None);
}

#[test]
fn find_excludes_hidden_and_protected_content_and_searches_public_controls() {
    let mut browser = Browser::new("<p>visible frost</p><div hidden>hidden-secret</div><p style='display:none'>display-secret</p><p style='opacity:0'>transparent-secret</p><script>script-secret</script><input type=password value=password-secret><input autocomplete=cc-number value=payment-secret><input type=hidden value=hidden-value><input value=public-editor><textarea>public-notes</textarea><button>public-button</button><select><option selected>public-choice</option></select>");
    for secret in [
        "hidden-secret",
        "display-secret",
        "transparent-secret",
        "script-secret",
        "password-secret",
        "payment-secret",
        "hidden-value",
    ] {
        assert_eq!(
            browser.update(secret, false).match_count,
            0,
            "must not expose {secret}"
        );
    }
    for query in [
        "public-editor",
        "public-notes",
        "public-button",
        "public-choice",
    ] {
        let state = browser.update(query, false);
        assert_eq!(state.match_count, 1, "search painted {query}");
        assert!(!state.rects.is_empty());
        let json = serde_json::to_string(&state).unwrap();
        assert!(!json.contains("password-secret") && !json.contains("payment-secret"));
    }
}

#[test]
fn find_scrolls_reflows_and_invalidates_on_navigation() {
    let mut browser =
        Browser::new("<p>frost</p><div style='height:800px'></div><p>lower frost</p>");
    let first = browser.update("frost", false);
    assert_eq!(first.match_count, 2);
    assert_eq!(browser.snapshot().scroll_y, 0.0);
    let next = browser.find(FindAction::Next { backwards: false });
    let snapshot = browser.snapshot();
    assert!(snapshot.scroll_y > 500.0);
    assert!(next.rects[0].y >= snapshot.scroll_y && next.rects[0].y < snapshot.scroll_y + 180.0);
    browser.request(
        1,
        ClientMessage::Resize {
            width: 240,
            height: 180,
        },
    );
    assert_eq!(browser.state(1).active_match, Some(2));
    assert!(matches!(
        browser.request(
            1,
            ClientMessage::Navigate {
                url: "about:credits".into()
            }
        ),
        ServerMessage::Navigated { .. }
    ));
    // Drain the navigation frame with a correlated read; navigation can also emit uncorrelated state.
    let old = browser.context;
    browser.request += 1;
    blueice_ipc::write_client_message_with_ids(
        &mut browser.client,
        Some(1),
        Some(browser.request),
        &ClientMessage::Find {
            tab_id: 1,
            frame_source: old.frame_source,
            document_generation: old.document_generation,
            action: FindAction::Next { backwards: false },
        },
    )
    .unwrap();
    loop {
        let (_, id, message) =
            blueice_ipc::read_server_message_with_ids(&mut browser.client).unwrap();
        if id == Some(browser.request) {
            assert!(
                matches!(message, ServerMessage::Error { message } if message.contains("stale"))
            );
            break;
        }
    }
    assert_eq!(browser.state(1).match_count, 0);
}

#[test]
fn find_is_tab_and_source_fenced() {
    let mut browser = Browser::new("<p>frost frost</p>");
    assert_eq!(browser.update("frost", false).match_count, 2);
    assert_eq!(browser.state(2).match_count, 0);
    assert!(
        matches!(browser.request(1, ClientMessage::Find { tab_id: 1, frame_source: browser.context.frame_source.wrapping_add(1), document_generation: browser.context.document_generation, action: FindAction::Close }), ServerMessage::Error { message } if message.contains("stale"))
    );
    assert_eq!(browser.state(1).match_count, 2);
    let state = browser.state(2);
    let ServerMessage::FindState(other) = browser.request(
        2,
        ClientMessage::Find {
            tab_id: 2,
            frame_source: state.frame_source,
            document_generation: state.document_generation,
            action: FindAction::Update {
                query: "frost".into(),
                case_sensitive: false,
            },
        },
    ) else {
        panic!("second tab find")
    };
    assert_eq!((other.tab_id, other.match_count), (2, 1));
    assert_eq!(browser.state(1).match_count, 2);
}

#[test]
fn find_limits_are_explicit_and_invalid_queries_preserve_state() {
    let mut browser = Browser::new(&format!("<p>{}</p>", "x ".repeat(MAX_FIND_MATCHES + 1)));
    let state = browser.update("x", false);
    assert_eq!(state.match_count, MAX_FIND_MATCHES as u32);
    assert!(state.limited);
    let command = ClientMessage::Find {
        tab_id: 1,
        frame_source: browser.context.frame_source,
        document_generation: browser.context.document_generation,
        action: FindAction::Update {
            query: "a".repeat(MAX_FIND_QUERY_BYTES + 1),
            case_sensitive: false,
        },
    };
    assert!(
        matches!(browser.request(1, command), ServerMessage::Error { message } if message.contains("long"))
    );
    assert_eq!(browser.state(1), state);
}

#[test]
fn find_reflow_and_live_control_mutations_rebuild_geometry_and_counts() {
    let mut browser = Browser::new("<p>snow crystal across lines</p><input aria-label=Editor value=editor style='display:block;width:200px'>");
    let first = browser.update("snow crystal across lines", false);
    assert_eq!(first.rects.len(), 1);
    browser.request(
        1,
        ClientMessage::Resize {
            width: 120,
            height: 180,
        },
    );
    let wrapped = browser.state(1);
    assert_eq!(wrapped.match_count, 1);
    assert!(wrapped.revision > first.revision && wrapped.rects.len() > 1);
    let snapshot = browser.snapshot();
    let id = snapshot
        .nodes
        .iter()
        .find(|node| node.name.as_deref() == Some("Editor"))
        .unwrap()
        .id;
    assert_eq!(browser.update("editor", false).match_count, 1);
    browser.request(
        1,
        ClientMessage::ActOn {
            id,
            action: blueice_ipc::NodeAction::SetValue("changed".into()),
        },
    );
    assert_eq!(browser.state(1).match_count, 0);
    assert_eq!(browser.update("changed", false).match_count, 1);
}

#[test]
fn restored_history_snapshot_does_not_restore_hidden_find_highlights() {
    let mut browser = Browser::with_history("<p>frost frost</p>", HistorySnapshotMode::Snapshot);
    assert_eq!(browser.update("frost", false).match_count, 2);
    assert!(matches!(
        browser.request(
            1,
            ClientMessage::Navigate {
                url: "about:credits".into()
            }
        ),
        ServerMessage::Navigated { .. }
    ));
    assert_eq!(browser.state(1).match_count, 0);
    assert!(matches!(
        browser.request(1, ClientMessage::GoBack),
        ServerMessage::Navigated { .. }
    ));
    let restored = browser.state(1);
    assert_eq!((restored.query.as_str(), restored.match_count), ("", 0));
}

#[test]
fn find_rejects_cross_tab_contexts_and_closed_tabs_without_mutating_live_state() {
    let mut browser = Browser::new("<p>frost frost</p>");
    let first = browser.update("frost", false);
    let command = ClientMessage::Find {
        tab_id: 1,
        frame_source: first.frame_source,
        document_generation: first.document_generation,
        action: FindAction::Update {
            query: "frost".into(),
            case_sensitive: false,
        },
    };
    assert!(
        matches!(browser.request(2, command), ServerMessage::Error { message } if message.contains("stale"))
    );
    assert_eq!(browser.state(2).match_count, 0);
    assert!(matches!(
        browser.request(2, ClientMessage::CloseTab),
        ServerMessage::TabClosed { tab_id: 2 }
    ));
    assert!(matches!(
        browser.request(2, ClientMessage::GetFindState),
        ServerMessage::Error { .. }
    ));
    assert_eq!(browser.state(1), first);
}
