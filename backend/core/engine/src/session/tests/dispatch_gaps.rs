// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Every per-tab-scoped `ClientMessage` variant has its own `None =>
// write_unknown_tab_error(...)` call site in `run_session`'s match --
// a real, independently reachable branch per variant, not a single
// shared helper call. Most were only exercised (if at all) through
// `GetRepresentation`; this file drives every other variant's own
// unknown-tab arm directly, plus the `Click`/`ActOn` click-dispatch
// branches that need a real (non-default) `PageJavaScriptExecutor` to
// reach at all.

use super::*;

const UNKNOWN_TAB: u64 = 424_242;

fn assert_unknown_tab_error(client: &mut UnixStream) {
    let (reply_tab, _, reply) = blueice_ipc::read_server_message_with_ids(client).unwrap();
    assert_eq!(reply_tab, Some(UNKNOWN_TAB));
    assert!(
        matches!(reply, ServerMessage::Error { .. }),
        "expected Error for an unknown tab, got {reply:?}"
    );
}

#[test]
fn every_remaining_per_tab_message_rejects_an_unknown_tab() {
    let dir = temp_frame_dir("dispatch-gaps-unknown-tab");
    let gatekeeper = clearing_gatekeeper("dispatch-gaps-unknown-tab");
    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        let mut generation = 0u64;
        run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
        dir
    });
    handshake(&mut client);

    blueice_ipc::write_client_message_with_ids(
        &mut client,
        Some(UNKNOWN_TAB),
        None,
        &ClientMessage::Navigate {
            url: "http://example.test".to_string(),
        },
    )
    .unwrap();
    assert_unknown_tab_error(&mut client);

    blueice_ipc::write_client_message_with_ids(
        &mut client,
        Some(UNKNOWN_TAB),
        None,
        &ClientMessage::Resize {
            width: 100,
            height: 100,
        },
    )
    .unwrap();
    assert_unknown_tab_error(&mut client);

    blueice_ipc::write_client_message_with_ids(
        &mut client,
        Some(UNKNOWN_TAB),
        None,
        &ClientMessage::Scroll { delta_y: 10.0 },
    )
    .unwrap();
    assert_unknown_tab_error(&mut client);

    blueice_ipc::write_client_message_with_ids(
        &mut client,
        Some(UNKNOWN_TAB),
        None,
        &ClientMessage::Hover { x: 1.0, y: 1.0 },
    )
    .unwrap();
    assert_unknown_tab_error(&mut client);

    blueice_ipc::write_client_message_with_ids(
        &mut client,
        Some(UNKNOWN_TAB),
        None,
        &ClientMessage::GetDom,
    )
    .unwrap();
    assert_unknown_tab_error(&mut client);

    blueice_ipc::write_client_message_with_ids(
        &mut client,
        Some(UNKNOWN_TAB),
        None,
        &ClientMessage::GetBlueTsScriptReports,
    )
    .unwrap();
    assert_unknown_tab_error(&mut client);

    blueice_ipc::write_client_message_with_ids(
        &mut client,
        Some(UNKNOWN_TAB),
        None,
        &ClientMessage::GetBlueJsScriptReports,
    )
    .unwrap();
    assert_unknown_tab_error(&mut client);

    blueice_ipc::write_client_message_with_ids(
        &mut client,
        Some(UNKNOWN_TAB),
        None,
        &ClientMessage::ActOn {
            id: 1,
            action: NodeAction::Focus,
        },
    )
    .unwrap();
    assert_unknown_tab_error(&mut client);

    blueice_ipc::write_client_message_with_ids(
        &mut client,
        Some(UNKNOWN_TAB),
        None,
        &ClientMessage::Highlight { id: None },
    )
    .unwrap();
    assert_unknown_tab_error(&mut client);

    // `Click` resolves its target through the same `tabs.get(target)`
    // as every other variant, but only reports the unknown-tab error
    // from its own separate `else if` tail (since a coordinate click
    // that hits nothing on a *live* tab is silently a no-op, not an
    // error) -- an unknown tab must still surface one.
    blueice_ipc::write_client_message_with_ids(
        &mut client,
        Some(UNKNOWN_TAB),
        None,
        &ClientMessage::Click { x: 1.0, y: 1.0 },
    )
    .unwrap();
    assert_unknown_tab_error(&mut client);

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn get_blue_ts_script_reports_rejects_a_configured_executor_without_blue_ts_support() {
    // `JavaScriptPageExecutor` (the in-process standard-JS host) never
    // overrides `supports_blue_ts_page_execution`, so it takes the
    // trait's own default (`false`) -- a distinct branch from "no
    // executor is configured at all" (already covered elsewhere).
    let dir = temp_frame_dir("dispatch-gaps-unsupported-bluets");
    let gatekeeper = clearing_gatekeeper("dispatch-gaps-unsupported-bluets");
    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        let mut generation = 0u64;
        let mut executor = JavaScriptPageExecutor::default();
        run_session_with_script_and_debugger_requests_and_inline_javascript_executor(
            &mut tabs,
            &mut server,
            &dir,
            &mut generation,
            &gatekeeper,
            CoreSessionRequests::default(),
            Some(&mut executor),
        )
        .unwrap();
        dir
    });
    handshake(&mut client);

    blueice_ipc::write_client_message(&mut client, &ClientMessage::GetBlueTsScriptReports).unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::Error {
            message: "inline BlueTS execution is not enabled".to_string(),
        }
    );

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

/// A minimal `PageJavaScriptExecutor` whose only real behavior is its
/// configured click-dispatch outcome -- the one seam `Click`/`ActOn`
/// need a real (non-default, non-`None`) executor to reach their own
/// `Err`/prevented/not-prevented branches through at all.
enum ClickDispatch {
    Errors,
    Prevented,
    NotPrevented,
}

struct ClickDispatchExecutor(ClickDispatch);

impl PageJavaScriptExecutor for ClickDispatchExecutor {
    fn synchronize_and_execute(&mut self, _tabs: &TabManager) -> io::Result<()> {
        Ok(())
    }

    fn dispatch_click_serving_script(
        &mut self,
        _tabs: &mut TabManager,
        _tab_id: TabId,
        _node_id: u64,
        _script_requests: Option<&ScriptRequestReceiver>,
    ) -> io::Result<Option<bool>> {
        match self.0 {
            ClickDispatch::Errors => Err(io::Error::other("test click listener failure")),
            ClickDispatch::Prevented => Ok(Some(true)),
            ClickDispatch::NotPrevented => Ok(Some(false)),
        }
    }

    fn drain_reports_for_tab(&mut self, _tab_id: TabId) -> Vec<JavaScriptPageExecutionReport> {
        Vec::new()
    }
}

fn run_with_click_dispatch_executor(
    dir: std::path::PathBuf,
    gatekeeper: PathBuf,
    mut server: UnixStream,
    dispatch: ClickDispatch,
    html: &'static str,
) -> thread::JoinHandle<std::path::PathBuf> {
    thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        default_page(&mut tabs).load_html_str(html, None);
        let mut generation = 0u64;
        let mut executor = ClickDispatchExecutor(dispatch);
        run_session_with_script_and_debugger_requests_and_inline_javascript_executor(
            &mut tabs,
            &mut server,
            &dir,
            &mut generation,
            &gatekeeper,
            CoreSessionRequests::default(),
            Some(&mut executor),
        )
        .unwrap();
        dir
    })
}

#[test]
fn coordinate_click_reports_an_error_when_the_configured_listener_fails() {
    let dir = temp_frame_dir("dispatch-gaps-click-listener-error");
    let gatekeeper = clearing_gatekeeper("dispatch-gaps-click-listener-error");
    let (mut client, server) = client_pair();
    let handle = run_with_click_dispatch_executor(
        dir,
        gatekeeper,
        server,
        ClickDispatch::Errors,
        r#"<a href="http://example.test">go</a>"#,
    );
    handshake(&mut client);

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Click { x: 5.0, y: 5.0 })
        .unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::Error {
            message: "page click listener unavailable".to_string(),
        }
    );

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn act_on_click_reports_an_error_when_the_configured_listener_fails() {
    let dir = temp_frame_dir("dispatch-gaps-act-on-listener-error");
    let gatekeeper = clearing_gatekeeper("dispatch-gaps-act-on-listener-error");
    let (mut client, server) = client_pair();
    let handle = run_with_click_dispatch_executor(
        dir,
        gatekeeper,
        server,
        ClickDispatch::Errors,
        r#"<a href="http://example.test">go</a>"#,
    );
    handshake(&mut client);

    blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
    let ServerMessage::Representation(snapshot) =
        blueice_ipc::read_server_message(&mut client).unwrap()
    else {
        panic!("expected Representation")
    };
    let link_id = snapshot
        .nodes
        .iter()
        .find(|n| n.name.as_deref() == Some("go"))
        .unwrap()
        .id;

    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::ActOn {
            id: link_id,
            action: NodeAction::Click,
        },
    )
    .unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::Error {
            message: "page click listener unavailable".to_string(),
        }
    );

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn coordinate_click_on_a_link_prevented_by_script_repaints_without_navigating() {
    let dir = temp_frame_dir("dispatch-gaps-click-prevented");
    let gatekeeper = clearing_gatekeeper("dispatch-gaps-click-prevented");
    let (mut client, server) = client_pair();
    let handle = run_with_click_dispatch_executor(
        dir,
        gatekeeper,
        server,
        ClickDispatch::Prevented,
        r#"<a href="http://example.test">go</a>"#,
    );
    handshake(&mut client);

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Click { x: 5.0, y: 5.0 })
        .unwrap();
    assert!(matches!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::FrameReady { .. }
    ));

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn coordinate_click_on_plain_text_not_prevented_by_script_still_repaints() {
    // A click that lands on non-link, non-actionable content still
    // repaints once the (real, configured) listener has explicitly
    // reported "not prevented" -- distinct from the silent no-op a
    // click with *no* listener configured at all gets for the same
    // coordinates.
    let dir = temp_frame_dir("dispatch-gaps-click-not-prevented");
    let gatekeeper = clearing_gatekeeper("dispatch-gaps-click-not-prevented");
    let (mut client, server) = client_pair();
    let handle = run_with_click_dispatch_executor(
        dir,
        gatekeeper,
        server,
        ClickDispatch::NotPrevented,
        "<p>plain text</p>",
    );
    handshake(&mut client);

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Click { x: 5.0, y: 5.0 })
        .unwrap();
    assert!(matches!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::FrameReady { .. }
    ));

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn act_on_click_on_plain_text_not_prevented_by_script_still_repaints() {
    let dir = temp_frame_dir("dispatch-gaps-act-on-not-prevented");
    let gatekeeper = clearing_gatekeeper("dispatch-gaps-act-on-not-prevented");
    let (mut client, server) = client_pair();
    let handle = run_with_click_dispatch_executor(
        dir,
        gatekeeper,
        server,
        ClickDispatch::NotPrevented,
        r#"<p id="target">plain text</p>"#,
    );
    handshake(&mut client);

    blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
    let ServerMessage::Representation(snapshot) =
        blueice_ipc::read_server_message(&mut client).unwrap()
    else {
        panic!("expected Representation")
    };
    let node_id = snapshot.nodes[0].id;

    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::ActOn {
            id: node_id,
            action: NodeAction::Click,
        },
    )
    .unwrap();
    assert!(matches!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::FrameReady { .. }
    ));

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_completion_for_a_tab_closed_while_its_navigation_was_pending_is_silently_discarded() {
    // `apply_completion` must never reply to, or apply state for, a
    // navigation whose tab has since closed -- the background thread
    // that produced the completion has no way to know that happened.
    let dir = temp_frame_dir("dispatch-gaps-completion-after-close");
    let gatekeeper_path = unique_gatekeeper_socket_path("dispatch-gaps-completion-after-close");
    let _ = std::fs::remove_file(&gatekeeper_path);
    let listener = UnixListener::bind(&gatekeeper_path).unwrap();
    thread::spawn(move || {
        for incoming in listener.incoming() {
            let Ok(mut stream) = incoming else { break };
            thread::spawn(move || {
                if let Ok(req) = blueice_ipc::gatekeeper::read_gatekeeper_request(&mut stream) {
                    if matches!(&req, blueice_ipc::gatekeeper::GatekeeperRequest::CheckUrl { url } if url.contains("closed-tab"))
                    {
                        thread::sleep(Duration::from_millis(300));
                    }
                    let _ = blueice_ipc::gatekeeper::write_gatekeeper_reply(
                        &mut stream,
                        &blueice_ipc::gatekeeper::GatekeeperReply::Cleared,
                    );
                }
            });
        }
    });

    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        let mut generation = 0u64;
        run_session(
            &mut tabs,
            &mut server,
            &dir,
            &mut generation,
            &gatekeeper_path,
        )
        .unwrap();
        dir
    });
    handshake(&mut client);

    blueice_ipc::write_client_message(&mut client, &ClientMessage::OpenTab { url: None }).unwrap();
    let ServerMessage::TabOpened { tab_id, .. } =
        blueice_ipc::read_server_message(&mut client).unwrap()
    else {
        panic!("expected TabOpened")
    };

    // Kick off a navigation whose gatekeeper check stalls 300ms --
    // fire-and-forget, its own reply is never read.
    blueice_ipc::write_client_message_with_ids(
        &mut client,
        Some(tab_id),
        None,
        &ClientMessage::Navigate {
            url: "http://127.0.0.1:1/closed-tab".to_string(),
        },
    )
    .unwrap();

    // Close the tab well before the stalled check resolves.
    blueice_ipc::write_client_message_with_ids(
        &mut client,
        Some(tab_id),
        None,
        &ClientMessage::CloseTab,
    )
    .unwrap();
    let (reply_tab, _, reply) = blueice_ipc::read_server_message_with_ids(&mut client).unwrap();
    assert_eq!(reply_tab, Some(tab_id));
    assert_eq!(reply, ServerMessage::TabClosed { tab_id });

    // Give the stalled gatekeeper check time to resolve and its
    // completion time to reach the poll loop, then prove nothing
    // stray arrived for it: the very next reply on the wire must be
    // this fresh, unrelated request's own.
    thread::sleep(Duration::from_millis(500));
    blueice_ipc::write_client_message(&mut client, &ClientMessage::ListTabs).unwrap();
    let ServerMessage::Tabs(remaining) = blueice_ipc::read_server_message(&mut client).unwrap()
    else {
        panic!("expected Tabs")
    };
    assert!(
        !remaining.iter().any(|summary| summary.id == tab_id),
        "the closed tab must stay closed"
    );

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_navigation_the_gatekeeper_clears_but_whose_fetch_fails_replies_error() {
    let dir = temp_frame_dir("dispatch-gaps-fetch-failed");
    let gatekeeper = clearing_gatekeeper("dispatch-gaps-fetch-failed");
    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        let mut generation = 0u64;
        run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
        dir
    });
    handshake(&mut client);

    // Nothing listens on port 1 -- the URL-stage gatekeeper check
    // clears, then `blueice_net::fetch` itself fails to connect.
    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::Navigate {
            url: "http://127.0.0.1:1/unreachable".to_string(),
        },
    )
    .unwrap();
    let reply = blueice_ipc::read_server_message(&mut client).unwrap();
    assert!(
        matches!(reply, ServerMessage::Error { .. }),
        "expected Error for a cleared navigation whose fetch failed, got {reply:?}"
    );

    // The session must survive a fetch failure exactly like every
    // other gated-navigation error outcome.
    blueice_ipc::write_client_message(&mut client, &ClientMessage::ListTabs).unwrap();
    assert!(matches!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::Tabs(_)
    ));

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_connection_whose_first_message_is_not_hello_is_rejected_and_ends_the_session() {
    let dir = temp_frame_dir("dispatch-gaps-non-hello-first-message");
    let gatekeeper = clearing_gatekeeper("dispatch-gaps-non-hello-first-message");
    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        let mut generation = 0u64;
        run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
        dir
    });

    // No handshake -- straight to an ordinary request as the very
    // first message on the connection.
    blueice_ipc::write_client_message(&mut client, &ClientMessage::ListTabs).unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::Error {
            message: "the first message on a connection must be Hello".to_string(),
        }
    );

    // The session ends without ever entering the main loop -- a
    // further message gets no reply at all, and the thread must still
    // exit cleanly on its own.
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}
