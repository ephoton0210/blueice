// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Session tests for the Phase 7/8/9/10/16 features layered on the core
//! session loop (extension bridges, downloads, assistant, tab groups and
//! history). The page-script/BlueJS/BlueTS session tests live in `tests.rs`.

use super::*;
use std::net::TcpListener;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

fn temp_frame_dir(label: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "blueice-session-test-{label}-{}",
        std::process::id()
    ))
}

#[test]
fn a_same_url_downloads_navigation_invalidates_its_previous_render_cache() {
    let mut tabs = TabManager::new(100.0, 100.0);
    let tab_id = tabs.default_tab();
    let mut refresher = DownloadsRefresher::default();
    refresher
        .seen_url
        .insert(tab_id, "about:downloads".to_string());
    refresher
        .rendered
        .insert(tab_id, "<p>old successful list</p>".to_string());
    refresher.visit.insert(tab_id, 41);
    refresher
        .due
        .insert(tab_id, Instant::now() + Duration::from_secs(1));

    let dir = temp_frame_dir("same-downloads-url");
    std::fs::create_dir_all(&dir).unwrap();
    let (tx, _rx) = mpsc::channel();
    let mut wire = Vec::new();
    let mut generation = 0;
    begin_gated_navigation(
        &mut tabs,
        &mut wire,
        &dir,
        &mut generation,
        Some(tab_id.as_u64()),
        None,
        tab_id,
        "about:downloads".to_string(),
        PendingKind::Navigate,
        &mut HashMap::new(),
        &mut refresher,
        &tx,
        Path::new("/unused"),
        None,
    )
    .unwrap();

    assert_eq!(tabs.get(tab_id).unwrap().url(), Some("about:downloads"));
    assert_eq!(refresher.visit[&tab_id], 42);
    assert!(!refresher.rendered.contains_key(&tab_id));
    assert!(!refresher.seen_url.contains_key(&tab_id));
    assert!(!refresher.due.contains_key(&tab_id));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn one_transient_downloads_failure_keeps_the_last_successful_render() {
    let mut tabs = TabManager::new(100.0, 100.0);
    let tab_id = tabs.default_tab();
    tabs.get_mut(tab_id).unwrap().load_html_str(
        "<p>last successful list</p>",
        Some("about:downloads".to_string()),
    );
    let mut refresher = DownloadsRefresher::default();
    refresher.visit.insert(tab_id, 1);
    refresher
        .rendered
        .insert(tab_id, "<p>last successful list</p>".to_string());
    let dir = temp_frame_dir("downloads-transient-failure");
    std::fs::create_dir_all(&dir).unwrap();
    let mut wire = Vec::new();
    let mut generation = 0;

    refresher
        .apply(
            &mut tabs,
            &mut wire,
            &dir,
            &mut generation,
            DownloadsListing {
                tab_id,
                visit: 1,
                url: "about:downloads".to_string(),
                outcome: Err("temporary timeout".to_string()),
            },
        )
        .unwrap();

    assert_eq!(generation, 0);
    assert!(tabs
        .get(tab_id)
        .unwrap()
        .dom_dump()
        .contains("last successful list"));

    refresher
        .apply(
            &mut tabs,
            &mut wire,
            &dir,
            &mut generation,
            DownloadsListing {
                tab_id,
                visit: 1,
                url: "about:downloads".to_string(),
                outcome: Err("temporary timeout".to_string()),
            },
        )
        .unwrap();

    assert_eq!(generation, 1);
    assert!(tabs
        .get(tab_id)
        .unwrap()
        .dom_dump()
        .contains("The downloads service is not running"));
    let _ = std::fs::remove_dir_all(dir);
}

fn client_pair() -> (UnixStream, UnixStream) {
    UnixStream::pair().unwrap()
}

#[test]
fn extension_request_reads_the_default_tabs_real_ai_representation() {
    let (mut client, mut server) = client_pair();
    let (extension_tx, extension_rx) = mpsc::channel();
    let dir = temp_frame_dir("extension-default-tab-read");
    let cleanup_dir = dir.clone();
    std::fs::create_dir_all(&dir).unwrap();
    let gatekeeper = PathBuf::from("/not-used-for-built-in-navigation");
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        let mut generation = 0;
        run_session_with_extension_requests(
            &mut tabs,
            &mut server,
            &dir,
            &mut generation,
            &gatekeeper,
            &extension_rx,
        )
    });

    blueice_ipc::client_handshake(&mut client).unwrap();
    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::Navigate {
            url: "about:credits".to_string(),
        },
    )
    .unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::Navigated {
            url: "about:credits".to_string(),
        }
    );
    assert!(matches!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::FrameReady { .. }
    ));

    let (reply_tx, reply_rx) = mpsc::channel();
    extension_tx
        .send(ExtensionPageRequest::ReadRepresentation {
            tab_id: None,
            reply: reply_tx,
        })
        .unwrap();
    let encoded = reply_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("the live session must answer the extension read")
        .expect("a live default tab must serialize");
    let snapshot: blueice_ipc::AiSnapshot = serde_json::from_str(&encoded).unwrap();
    assert_eq!(snapshot.tab_id, 1);
    assert_eq!(snapshot.url.as_deref(), Some("about:credits"));
    assert!(
        !snapshot.nodes.is_empty(),
        "the core-backed snapshot must be from the navigated credits page, not the empty initial tab"
    );

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    handle.join().unwrap().unwrap();
    let _ = std::fs::remove_dir_all(cleanup_dir);
}

#[test]
fn extension_observes_only_the_committed_http_response_for_a_live_tab() {
    let gatekeeper = clearing_gatekeeper("extension-network-observe");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/before", listener.local_addr().unwrap());
    let final_url = format!("http://{}/after", listener.local_addr().unwrap());
    let http = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0u8; 1024];
        let _ = std::io::Read::read(&mut stream, &mut request);
        std::io::Write::write_all(
            &mut stream,
            b"HTTP/1.1 302 Found\r\nLocation: /after\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        )
        .unwrap();
        let (mut stream, _) = listener.accept().unwrap();
        let _ = std::io::Read::read(&mut stream, &mut request);
        std::io::Write::write_all(
            &mut stream,
            b"HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nSet-Cookie: secret=never-expose\r\nContent-Length: 9\r\nConnection: close\r\n\r\n<p>ok</p>",
        )
        .unwrap();
    });
    let (mut client, mut server) = client_pair();
    let (extension_tx, extension_rx) = mpsc::channel();
    let dir = temp_frame_dir("extension-network-observe");
    let cleanup_dir = dir.clone();
    std::fs::create_dir_all(&dir).unwrap();
    let session = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        let mut generation = 0;
        run_session_with_extension_requests(
            &mut tabs,
            &mut server,
            &dir,
            &mut generation,
            &gatekeeper,
            &extension_rx,
        )
    });
    handshake(&mut client);
    let (reply_tx, reply_rx) = mpsc::channel();
    extension_tx
        .send(ExtensionPageRequest::ReadNetworkResponse {
            tab_id: 1,
            reply: reply_tx,
        })
        .unwrap();
    assert_eq!(
        reply_rx
            .recv_timeout(Duration::from_secs(1))
            .unwrap()
            .unwrap(),
        None
    );
    let (trace_tx, trace_rx) = mpsc::channel();
    extension_tx
        .send(ExtensionPageRequest::ReadNetworkTrace {
            tab_id: 1,
            reply: trace_tx,
        })
        .unwrap();
    assert_eq!(
        trace_rx
            .recv_timeout(Duration::from_secs(1))
            .unwrap()
            .unwrap(),
        None
    );
    blueice_ipc::write_client_message(&mut client, &ClientMessage::Navigate { url: url.clone() })
        .unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::Navigated {
            url: final_url.clone()
        }
    );
    assert!(matches!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::FrameReady { .. }
    ));
    let (reply_tx, reply_rx) = mpsc::channel();
    extension_tx
        .send(ExtensionPageRequest::ReadNetworkResponse {
            tab_id: 1,
            reply: reply_tx,
        })
        .unwrap();
    let response = reply_rx
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(response.method, "GET");
    assert_eq!(response.final_url, final_url);
    assert_eq!(response.status, 200);
    assert_eq!(
        response.content_type.as_deref(),
        Some("text/html; charset=utf-8")
    );
    assert!(!format!("{response:?}").contains("secret"));
    let (trace_tx, trace_rx) = mpsc::channel();
    extension_tx
        .send(ExtensionPageRequest::ReadNetworkTrace {
            tab_id: 1,
            reply: trace_tx,
        })
        .unwrap();
    let trace = trace_rx
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(trace.request_url, url);
    assert_eq!(
        trace.redirects,
        vec![blueice_ipc::extension::NetworkRedirectInfo {
            request_url: url.clone(),
            status: 302,
            target_url: final_url.clone(),
        }]
    );
    assert_eq!(trace.response, response);
    assert!(!format!("{trace:?}").contains("secret"));

    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::Navigate {
            url: "about:blank".to_string(),
        },
    )
    .unwrap();
    assert!(matches!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::Navigated { .. }
    ));
    assert!(matches!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::FrameReady { .. }
    ));
    let (reply_tx, reply_rx) = mpsc::channel();
    extension_tx
        .send(ExtensionPageRequest::ReadNetworkResponse {
            tab_id: 1,
            reply: reply_tx,
        })
        .unwrap();
    assert_eq!(
        reply_rx
            .recv_timeout(Duration::from_secs(1))
            .unwrap()
            .unwrap(),
        None
    );
    let (trace_tx, trace_rx) = mpsc::channel();
    extension_tx
        .send(ExtensionPageRequest::ReadNetworkTrace {
            tab_id: 1,
            reply: trace_tx,
        })
        .unwrap();
    assert_eq!(
        trace_rx
            .recv_timeout(Duration::from_secs(1))
            .unwrap()
            .unwrap(),
        None
    );
    let (reply_tx, reply_rx) = mpsc::channel();
    extension_tx
        .send(ExtensionPageRequest::ReadNetworkResponse {
            tab_id: 99,
            reply: reply_tx,
        })
        .unwrap();
    assert!(reply_rx
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .is_err());
    let (trace_tx, trace_rx) = mpsc::channel();
    extension_tx
        .send(ExtensionPageRequest::ReadNetworkTrace {
            tab_id: 99,
            reply: trace_tx,
        })
        .unwrap();
    assert!(trace_rx
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .is_err());
    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    session.join().unwrap().unwrap();
    http.join().unwrap();
    let _ = std::fs::remove_dir_all(cleanup_dir);
}

#[test]
fn extension_toolbar_is_broadcast_clickable_and_owned_by_its_connection() {
    let (mut client, mut server) = client_pair();
    let (extension_tx, extension_rx) = mpsc::channel();
    let (event_tx, event_rx) = mpsc::sync_channel(16);
    let dir = temp_frame_dir("extension-toolbar");
    let cleanup_dir = dir.clone();
    std::fs::create_dir_all(&dir).unwrap();
    let session = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        let mut generation = 0;
        run_session_with_extension_requests_and_events(
            &mut tabs,
            &mut server,
            &dir,
            &mut generation,
            Path::new("/not-used-for-native-ui"),
            &extension_rx,
            Some(&event_tx),
        )
    });
    handshake(&mut client);
    let (reply_tx, reply_rx) = mpsc::channel();
    extension_tx
        .send(ExtensionPageRequest::SetToolbarButton {
            connection_id: 4,
            grant_generation: 0,
            label: "\u{202e}spoof".to_string(),
            reply: reply_tx,
        })
        .unwrap();
    assert!(reply_rx
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .is_err());
    let (reply_tx, reply_rx) = mpsc::channel();
    extension_tx
        .send(ExtensionPageRequest::SetToolbarButton {
            connection_id: 4,
            grant_generation: 0,
            label: "Notes".to_string(),
            reply: reply_tx,
        })
        .unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::ExtensionToolbar {
            label: Some("Notes".to_string()),
        }
    );
    reply_rx
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    blueice_ipc::write_client_message(&mut client, &ClientMessage::GetExtensionToolbar).unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::ExtensionToolbar {
            label: Some("Notes".to_string()),
        }
    );
    blueice_ipc::write_client_message_with_ids(
        &mut client,
        Some(1),
        None,
        &ClientMessage::ActivateExtensionToolbar,
    )
    .unwrap();
    assert_eq!(
        event_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
        ExtensionRuntimeEvent::ToolbarActivated {
            tab_id: 1,
            grant_generation: 0
        }
    );

    // A stale connection cannot clear a newer connection's button.
    let (reply_tx, reply_rx) = mpsc::channel();
    extension_tx
        .send(ExtensionPageRequest::SetToolbarButton {
            connection_id: 5,
            grant_generation: 0,
            label: "Tasks".to_string(),
            reply: reply_tx,
        })
        .unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::ExtensionToolbar {
            label: Some("Tasks".to_string()),
        }
    );
    reply_rx
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    let popup = ExtensionPopup {
        id: 1,
        tab_id: 1,
        title: "Tasks".to_string(),
        body: "Saved locally".to_string(),
        action_label: None,
    };
    let (reply_tx, reply_rx) = mpsc::channel();
    extension_tx
        .send(ExtensionPageRequest::ShowPopup {
            connection_id: 5,
            grant_generation: 0,
            popup: popup.clone(),
            reply: reply_tx,
        })
        .unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::ExtensionPopup {
            popup: Some(popup.clone())
        }
    );
    reply_rx
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    blueice_ipc::write_client_message(&mut client, &ClientMessage::GetExtensionPopup).unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::ExtensionPopup {
            popup: Some(popup.clone())
        }
    );
    blueice_ipc::write_client_message(&mut client, &ClientMessage::DismissExtensionPopup).unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::ExtensionPopup { popup: None }
    );
    let (reply_tx, reply_rx) = mpsc::channel();
    extension_tx
        .send(ExtensionPageRequest::ShowPopup {
            connection_id: 5,
            grant_generation: 0,
            popup: popup.clone(),
            reply: reply_tx,
        })
        .unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::ExtensionPopup { popup: Some(popup) }
    );
    reply_rx
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    let action_popup = ExtensionPopup {
        id: 2,
        tab_id: 1,
        title: "Tasks".to_string(),
        body: "Ready to open".to_string(),
        action_label: Some("Open tasks".to_string()),
    };
    let (reply_tx, reply_rx) = mpsc::channel();
    extension_tx
        .send(ExtensionPageRequest::ShowPopup {
            connection_id: 5,
            grant_generation: 0,
            popup: action_popup.clone(),
            reply: reply_tx,
        })
        .unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::ExtensionPopup {
            popup: Some(action_popup.clone())
        }
    );
    reply_rx
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::ActivateExtensionPopupAction { popup_id: 1 },
    )
    .unwrap();
    assert!(
        matches!(blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::Error { message } if message.contains("no matching live extension popup action"))
    );
    assert!(event_rx.try_recv().is_err());
    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::ActivateExtensionPopupAction { popup_id: 2 },
    )
    .unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::ExtensionPopup { popup: None }
    );
    assert_eq!(
        event_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
        ExtensionRuntimeEvent::PopupActionActivated {
            tab_id: 1,
            grant_generation: 0
        }
    );
    let (reply_tx, reply_rx) = mpsc::channel();
    extension_tx
        .send(ExtensionPageRequest::ShowPopup {
            connection_id: 5,
            grant_generation: 0,
            popup: action_popup,
            reply: reply_tx,
        })
        .unwrap();
    assert!(matches!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::ExtensionPopup { popup: Some(_) }
    ));
    reply_rx
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    let (reply_tx, reply_rx) = mpsc::channel();
    extension_tx
        .send(ExtensionPageRequest::ClearToolbarButton {
            connection_id: 4,
            reply: reply_tx,
        })
        .unwrap();
    reply_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    blueice_ipc::write_client_message(&mut client, &ClientMessage::GetExtensionToolbar).unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::ExtensionToolbar {
            label: Some("Tasks".to_string()),
        }
    );
    let (reply_tx, reply_rx) = mpsc::channel();
    extension_tx
        .send(ExtensionPageRequest::ClearToolbarButton {
            connection_id: 5,
            reply: reply_tx,
        })
        .unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::ExtensionPopup { popup: None }
    );
    assert_eq!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::ExtensionToolbar { label: None }
    );
    reply_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    blueice_ipc::write_client_message(&mut client, &ClientMessage::ActivateExtensionToolbar)
        .unwrap();
    assert!(matches!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::Error { message } if message.contains("no extension toolbar")
    ));
    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    session.join().unwrap().unwrap();
    let _ = std::fs::remove_dir_all(cleanup_dir);
}

#[test]
fn queued_network_registration_checks_original_grant_generation_at_session_commit() {
    let root = temp_frame_dir("stale-network-registration");
    std::fs::create_dir_all(&root).unwrap();
    let manifest = root.join("extension.json");
    std::fs::write(&manifest,
        r#"{"name":"Queued network","version":"1","blueice_api_version":1,"entry_point":"extension.wasm","capabilities":{"optional":["network:intercept"]}}"#
    ).unwrap();
    std::fs::write(root.join("extension.wasm"), b"\0asm\x01\0\0\0").unwrap();
    let installed = blueice_extension_host::load_installed_extension(&manifest).unwrap();
    let id = installed.extension_id().to_string();
    let registry = Arc::new(blueice_extension_host::registry_for_installed_extension(
        &installed,
    ));
    let mut tabs = TabManager::new(320.0, 200.0);
    tabs.set_extension_permission_registry(Arc::clone(&registry), id.clone());
    assert!(registry.grant_optional(&id, "network:intercept").unwrap());
    let old_generation = registry
        .capability_generation(&id, "network:intercept")
        .unwrap();
    assert!(registry.revoke_optional(&id, "network:intercept").unwrap());
    assert!(registry.grant_optional(&id, "network:intercept").unwrap());
    let new_generation = registry
        .capability_generation(&id, "network:intercept")
        .unwrap();
    assert_ne!(old_generation, new_generation);

    let mut wire = Vec::new();
    let mut frame_generation = 0;
    let mut toolbar = None;
    let mut popup = None;
    let mut submit = |tabs: &mut TabManager, grant_generation| {
        let (reply, result) = mpsc::channel();
        handle_extension_page_request(
            tabs,
            &mut wire,
            &root,
            &mut frame_generation,
            &mut toolbar,
            &mut popup,
            ExtensionPageRequest::RegisterNetworkBlockUrl {
                connection_id: 7,
                grant_generation,
                url: "https://example.test/blocked".into(),
                reply,
            },
        )
        .unwrap();
        result.recv().unwrap()
    };
    assert!(submit(&mut tabs, old_generation).is_err());
    assert!(!tabs.is_extension_navigation_blocked("https://example.test/blocked"));
    assert!(submit(&mut tabs, new_generation).is_ok());
    assert!(tabs.is_extension_navigation_blocked("https://example.test/blocked"));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn queued_dom_writes_check_original_grant_generation_at_session_commit() {
    let root = temp_frame_dir("stale-dom-write");
    std::fs::create_dir_all(&root).unwrap();
    let manifest = root.join("extension.json");
    std::fs::write(&manifest,
        r#"{"name":"Queued DOM write","version":"1","blueice_api_version":1,"entry_point":"extension.wasm","capabilities":{"optional":["dom:write"]}}"#
    ).unwrap();
    std::fs::write(root.join("extension.wasm"), b"\0asm\x01\0\0\0").unwrap();
    let installed = blueice_extension_host::load_installed_extension(&manifest).unwrap();
    let id = installed.extension_id().to_string();
    let registry = Arc::new(blueice_extension_host::registry_for_installed_extension(
        &installed,
    ));
    let mut tabs = TabManager::new(320.0, 200.0);
    let tab_id = tabs.default_tab();
    tabs.get_mut(tab_id).unwrap().load_html_str(
        "<input id='name' value='Before'><h1 id='title'>Original</h1>",
        Some("https://example.test/page".into()),
    );
    let input_id = tabs
        .get(tab_id)
        .unwrap()
        .script_get_element_by_id("name")
        .unwrap();
    let title_id = tabs
        .get(tab_id)
        .unwrap()
        .script_get_element_by_id("title")
        .unwrap();
    tabs.set_extension_permission_registry(Arc::clone(&registry), id.clone());
    registry.grant_optional(&id, "dom:write").unwrap();
    let old_generation = registry.capability_generation(&id, "dom:write").unwrap();
    registry.revoke_optional(&id, "dom:write").unwrap();
    registry.grant_optional(&id, "dom:write").unwrap();
    let new_generation = registry.capability_generation(&id, "dom:write").unwrap();
    assert_ne!(old_generation, new_generation);

    let before = tabs.get(tab_id).unwrap().dom_dump();
    let mut wire = Vec::new();
    let mut frame_generation = 0;
    let mut toolbar = None;
    let mut popup = None;
    let (reply, result) = mpsc::channel();
    handle_extension_page_request(
        &mut tabs,
        &mut wire,
        &root,
        &mut frame_generation,
        &mut toolbar,
        &mut popup,
        ExtensionPageRequest::SetTextInputValue {
            tab_id: tab_id.as_u64(),
            node_id: input_id.as_u64(),
            value: "Stale".into(),
            grant_generation: old_generation,
            reply,
        },
    )
    .unwrap();
    assert!(result.recv().unwrap().is_err());
    let (reply, result) = mpsc::channel();
    handle_extension_page_request(
        &mut tabs,
        &mut wire,
        &root,
        &mut frame_generation,
        &mut toolbar,
        &mut popup,
        ExtensionPageRequest::SetVisibleTextContent {
            tab_id: tab_id.as_u64(),
            node_id: title_id.as_u64(),
            value: "Stale".into(),
            grant_generation: old_generation,
            reply,
        },
    )
    .unwrap();
    assert!(result.recv().unwrap().is_err());
    // Every other core-backed dom:write shape shares the final grant
    // check. An invalid node would produce a different error if one arm
    // accidentally bypassed it.
    for variant in 0..6 {
        let (reply, result) = mpsc::channel();
        let request = match variant {
            0 => ExtensionPageRequest::SetCheckboxChecked {
                tab_id: tab_id.as_u64(),
                node_id: 0,
                checked: true,
                grant_generation: old_generation,
                reply,
            },
            1 => ExtensionPageRequest::SetTextareaValue {
                tab_id: tab_id.as_u64(),
                node_id: 0,
                value: "Stale".into(),
                grant_generation: old_generation,
                reply,
            },
            2 => ExtensionPageRequest::SetVisibleLeafText {
                tab_id: tab_id.as_u64(),
                node_id: 0,
                value: "Stale".into(),
                grant_generation: old_generation,
                reply,
            },
            3 => ExtensionPageRequest::SetRangeInputValue {
                tab_id: tab_id.as_u64(),
                node_id: 0,
                value: 1,
                grant_generation: old_generation,
                reply,
            },
            4 => ExtensionPageRequest::SetRadioChecked {
                tab_id: tab_id.as_u64(),
                node_id: 0,
                grant_generation: old_generation,
                reply,
            },
            _ => ExtensionPageRequest::SelectOption {
                tab_id: tab_id.as_u64(),
                node_id: 0,
                grant_generation: old_generation,
                reply,
            },
        };
        handle_extension_page_request(
            &mut tabs,
            &mut wire,
            &root,
            &mut frame_generation,
            &mut toolbar,
            &mut popup,
            request,
        )
        .unwrap();
        assert!(
            result
                .recv()
                .unwrap()
                .unwrap_err()
                .contains("grant changed"),
            "every DOM write must reject the original revoked grant"
        );
    }
    assert_eq!(tabs.get(tab_id).unwrap().dom_dump(), before);
    assert!(wire.is_empty(), "a denied write must not publish a frame");

    let (reply, result) = mpsc::channel();
    handle_extension_page_request(
        &mut tabs,
        &mut wire,
        &root,
        &mut frame_generation,
        &mut toolbar,
        &mut popup,
        ExtensionPageRequest::SetTextInputValue {
            tab_id: tab_id.as_u64(),
            node_id: input_id.as_u64(),
            value: "Fresh".into(),
            grant_generation: new_generation,
            reply,
        },
    )
    .unwrap();
    result.recv().unwrap().unwrap();
    assert_ne!(tabs.get(tab_id).unwrap().dom_dump(), before);
    assert!(!wire.is_empty());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn optional_ui_revocation_clears_published_surfaces_and_rejects_stale_publication() {
    let root = temp_frame_dir("stale-optional-ui");
    std::fs::create_dir_all(&root).unwrap();
    let manifest = root.join("extension.json");
    std::fs::write(&manifest,
        r#"{"name":"Optional UI","version":"1","blueice_api_version":1,"entry_point":"extension.wasm","capabilities":{"optional":["ui:inject"]}}"#
    ).unwrap();
    std::fs::write(root.join("extension.wasm"), b"\0asm\x01\0\0\0").unwrap();
    let installed = blueice_extension_host::load_installed_extension(&manifest).unwrap();
    let id = installed.extension_id().to_string();
    let registry = Arc::new(blueice_extension_host::registry_for_installed_extension(
        &installed,
    ));
    let mut tabs = TabManager::new(320.0, 200.0);
    tabs.set_extension_permission_registry(Arc::clone(&registry), id.clone());
    registry.grant_optional(&id, "ui:inject").unwrap();
    let old_generation = registry.capability_generation(&id, "ui:inject").unwrap();
    let mut wire = Vec::new();
    let mut frame_generation = 0;
    let mut toolbar = None;
    let mut popup = None;
    let (reply, result) = mpsc::channel();
    handle_extension_page_request(
        &mut tabs,
        &mut wire,
        &root,
        &mut frame_generation,
        &mut toolbar,
        &mut popup,
        ExtensionPageRequest::SetToolbarButton {
            connection_id: 7,
            grant_generation: old_generation,
            label: "Notes".into(),
            reply,
        },
    )
    .unwrap();
    result.recv().unwrap().unwrap();
    let shown = ExtensionPopup {
        id: 1,
        tab_id: 1,
        title: "Notes".into(),
        body: "Saved".into(),
        action_label: None,
    };
    let (reply, result) = mpsc::channel();
    handle_extension_page_request(
        &mut tabs,
        &mut wire,
        &root,
        &mut frame_generation,
        &mut toolbar,
        &mut popup,
        ExtensionPageRequest::ShowPopup {
            connection_id: 7,
            grant_generation: old_generation,
            popup: shown.clone(),
            reply,
        },
    )
    .unwrap();
    result.recv().unwrap().unwrap();

    registry.revoke_optional(&id, "ui:inject").unwrap();
    prune_stale_extension_effects(&mut tabs, &mut wire, &mut toolbar, &mut popup).unwrap();
    assert!(toolbar.is_none() && popup.is_none());
    registry.grant_optional(&id, "ui:inject").unwrap();
    let new_generation = registry.capability_generation(&id, "ui:inject").unwrap();
    assert_ne!(old_generation, new_generation);
    let (reply, result) = mpsc::channel();
    handle_extension_page_request(
        &mut tabs,
        &mut wire,
        &root,
        &mut frame_generation,
        &mut toolbar,
        &mut popup,
        ExtensionPageRequest::SetToolbarButton {
            connection_id: 7,
            grant_generation: old_generation,
            label: "Old".into(),
            reply,
        },
    )
    .unwrap();
    assert!(result.recv().unwrap().is_err());
    assert!(toolbar.is_none());
    let (reply, result) = mpsc::channel();
    handle_extension_page_request(
        &mut tabs,
        &mut wire,
        &root,
        &mut frame_generation,
        &mut toolbar,
        &mut popup,
        ExtensionPageRequest::SetToolbarButton {
            connection_id: 7,
            grant_generation: new_generation,
            label: "New".into(),
            reply,
        },
    )
    .unwrap();
    result.recv().unwrap().unwrap();
    let mut cursor = std::io::Cursor::new(wire);
    assert_eq!(
        blueice_ipc::read_server_message(&mut cursor).unwrap(),
        ServerMessage::ExtensionToolbar {
            label: Some("Notes".into())
        }
    );
    assert_eq!(
        blueice_ipc::read_server_message(&mut cursor).unwrap(),
        ServerMessage::ExtensionPopup { popup: Some(shown) }
    );
    assert_eq!(
        blueice_ipc::read_server_message(&mut cursor).unwrap(),
        ServerMessage::ExtensionPopup { popup: None }
    );
    assert_eq!(
        blueice_ipc::read_server_message(&mut cursor).unwrap(),
        ServerMessage::ExtensionToolbar { label: None }
    );
    assert_eq!(
        blueice_ipc::read_server_message(&mut cursor).unwrap(),
        ServerMessage::ExtensionToolbar {
            label: Some("New".into())
        }
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn live_session_broadcasts_optional_ui_revocation_without_client_traffic() {
    let root = temp_frame_dir("live-optional-ui-revoke");
    std::fs::create_dir_all(&root).unwrap();
    let manifest = root.join("extension.json");
    std::fs::write(&manifest,
        r#"{"name":"Live optional UI","version":"1","blueice_api_version":1,"entry_point":"extension.wasm","capabilities":{"optional":["ui:inject"]}}"#
    ).unwrap();
    std::fs::write(root.join("extension.wasm"), b"\0asm\x01\0\0\0").unwrap();
    let installed = blueice_extension_host::load_installed_extension(&manifest).unwrap();
    let id = installed.extension_id().to_string();
    let registry = Arc::new(blueice_extension_host::registry_for_installed_extension(
        &installed,
    ));
    registry.grant_optional(&id, "ui:inject").unwrap();
    let old_generation = registry.capability_generation(&id, "ui:inject").unwrap();
    let (mut client, mut server) = client_pair();
    client
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let (extension_tx, extension_rx) = mpsc::channel();
    let (event_tx, event_rx) = mpsc::sync_channel(16);
    let session_registry = Arc::clone(&registry);
    let session = thread::spawn({
        let id = id.clone();
        let root = root.clone();
        move || {
            let mut tabs = TabManager::new(320.0, 200.0);
            tabs.set_extension_permission_registry(session_registry, id);
            let mut generation = 0;
            run_session_with_extension_requests_and_events(
                &mut tabs,
                &mut server,
                &root,
                &mut generation,
                Path::new("/not-used-for-optional-ui"),
                &extension_rx,
                Some(&event_tx),
            )
        }
    });
    handshake(&mut client);
    let (reply, result) = mpsc::channel();
    extension_tx
        .send(ExtensionPageRequest::SetToolbarButton {
            connection_id: 7,
            grant_generation: old_generation,
            label: "Notes".into(),
            reply,
        })
        .unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::ExtensionToolbar {
            label: Some("Notes".into())
        }
    );
    result
        .recv_timeout(Duration::from_secs(2))
        .unwrap()
        .unwrap();
    let shown = ExtensionPopup {
        id: 1,
        tab_id: 1,
        title: "Notes".into(),
        body: "Saved".into(),
        action_label: Some("Open".into()),
    };
    let (reply, result) = mpsc::channel();
    extension_tx
        .send(ExtensionPageRequest::ShowPopup {
            connection_id: 7,
            grant_generation: old_generation,
            popup: shown.clone(),
            reply,
        })
        .unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::ExtensionPopup { popup: Some(shown) }
    );
    result
        .recv_timeout(Duration::from_secs(2))
        .unwrap()
        .unwrap();

    assert!(
        revoke_optional_and_wait_for_cleanup(&registry, &id, "ui:inject", &extension_tx,).unwrap()
    );
    // No client command follows the revoke. The private completion
    // barrier acknowledges only after both removals are published.
    assert_eq!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::ExtensionPopup { popup: None }
    );
    assert_eq!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::ExtensionToolbar { label: None }
    );
    blueice_ipc::write_client_message(&mut client, &ClientMessage::ActivateExtensionToolbar)
        .unwrap();
    assert!(
        matches!(blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::Error { message } if message.contains("no extension toolbar"))
    );
    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::ActivateExtensionPopupAction { popup_id: 1 },
    )
    .unwrap();
    assert!(
        matches!(blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::Error { message } if message.contains("no matching live extension popup action"))
    );
    assert!(event_rx.try_recv().is_err());

    registry.grant_optional(&id, "ui:inject").unwrap();
    let new_generation = registry.capability_generation(&id, "ui:inject").unwrap();
    assert_ne!(old_generation, new_generation);
    let (reply, result) = mpsc::channel();
    extension_tx
        .send(ExtensionPageRequest::SetToolbarButton {
            connection_id: 7,
            grant_generation: old_generation,
            label: "Old".into(),
            reply,
        })
        .unwrap();
    assert!(result
        .recv_timeout(Duration::from_secs(2))
        .unwrap()
        .is_err());
    let (reply, result) = mpsc::channel();
    extension_tx
        .send(ExtensionPageRequest::SetToolbarButton {
            connection_id: 7,
            grant_generation: new_generation,
            label: "New".into(),
            reply,
        })
        .unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::ExtensionToolbar {
            label: Some("New".into())
        }
    );
    result
        .recv_timeout(Duration::from_secs(2))
        .unwrap()
        .unwrap();
    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    session.join().unwrap().unwrap();
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn internal_revoke_barrier_acks_only_after_ui_and_network_effects_are_removed() {
    let root = temp_frame_dir("optional-revoke-barrier");
    std::fs::create_dir_all(&root).unwrap();
    let manifest = root.join("extension.json");
    std::fs::write(&manifest,
        r#"{"name":"Optional effects","version":"1","blueice_api_version":1,"entry_point":"extension.wasm","capabilities":{"optional":["network:intercept","ui:inject"]}}"#
    ).unwrap();
    std::fs::write(root.join("extension.wasm"), b"\0asm\x01\0\0\0").unwrap();
    let installed = blueice_extension_host::load_installed_extension(&manifest).unwrap();
    let id = installed.extension_id().to_string();
    let registry = Arc::new(blueice_extension_host::registry_for_installed_extension(
        &installed,
    ));
    registry.grant_optional(&id, "network:intercept").unwrap();
    registry.grant_optional(&id, "ui:inject").unwrap();
    let network_generation = registry
        .capability_generation(&id, "network:intercept")
        .unwrap();
    let ui_generation = registry.capability_generation(&id, "ui:inject").unwrap();
    let mut tabs = TabManager::new(320.0, 200.0);
    tabs.set_extension_permission_registry(Arc::clone(&registry), id.clone());
    let mut wire = Vec::new();
    let mut frame_generation = 0;
    let mut toolbar = None;
    let mut popup = None;

    let (reply, result) = mpsc::channel();
    handle_extension_page_request(
        &mut tabs,
        &mut wire,
        &root,
        &mut frame_generation,
        &mut toolbar,
        &mut popup,
        ExtensionPageRequest::RegisterNetworkBlockHost {
            connection_id: 7,
            grant_generation: network_generation,
            host: "old.example.test".into(),
            reply,
        },
    )
    .unwrap();
    result.recv().unwrap().unwrap();
    assert_eq!(tabs.extension_navigation_rule_owner_count(), 1);
    let (reply, result) = mpsc::channel();
    handle_extension_page_request(
        &mut tabs,
        &mut wire,
        &root,
        &mut frame_generation,
        &mut toolbar,
        &mut popup,
        ExtensionPageRequest::SetToolbarButton {
            connection_id: 7,
            grant_generation: ui_generation,
            label: "Notes".into(),
            reply,
        },
    )
    .unwrap();
    result.recv().unwrap().unwrap();
    let shown = ExtensionPopup {
        id: 1,
        tab_id: 1,
        title: "Notes".into(),
        body: "Saved".into(),
        action_label: None,
    };
    let (reply, result) = mpsc::channel();
    handle_extension_page_request(
        &mut tabs,
        &mut wire,
        &root,
        &mut frame_generation,
        &mut toolbar,
        &mut popup,
        ExtensionPageRequest::ShowPopup {
            connection_id: 7,
            grant_generation: ui_generation,
            popup: shown.clone(),
            reply,
        },
    )
    .unwrap();
    result.recv().unwrap().unwrap();

    registry.revoke_optional(&id, "ui:inject").unwrap();
    let (session_tx, session_rx) = mpsc::channel();
    thread::scope(|scope| {
        let frame_root = &root;
        let tabs = &mut tabs;
        let wire = &mut wire;
        let frame_generation = &mut frame_generation;
        let toolbar = &mut toolbar;
        let popup = &mut popup;
        let worker = scope.spawn(move || {
            let request = session_rx.recv_timeout(Duration::from_secs(2)).unwrap();
            handle_extension_page_request(
                tabs,
                wire,
                frame_root,
                frame_generation,
                toolbar,
                popup,
                request,
            )
            .unwrap();
        });
        assert!(revoke_optional_and_wait_for_cleanup(
            &registry,
            &id,
            "network:intercept",
            &session_tx,
        )
        .unwrap());
        worker.join().unwrap();
    });
    assert_eq!(tabs.extension_navigation_rule_owner_count(), 0);
    assert!(toolbar.is_none() && popup.is_none());
    let mut cursor = std::io::Cursor::new(wire);
    assert_eq!(
        blueice_ipc::read_server_message(&mut cursor).unwrap(),
        ServerMessage::ExtensionToolbar {
            label: Some("Notes".into())
        }
    );
    assert_eq!(
        blueice_ipc::read_server_message(&mut cursor).unwrap(),
        ServerMessage::ExtensionPopup { popup: Some(shown) }
    );
    assert_eq!(
        blueice_ipc::read_server_message(&mut cursor).unwrap(),
        ServerMessage::ExtensionPopup { popup: None }
    );
    assert_eq!(
        blueice_ipc::read_server_message(&mut cursor).unwrap(),
        ServerMessage::ExtensionToolbar { label: None }
    );
    registry.grant_optional(&id, "network:intercept").unwrap();
    let (unavailable_session, receiver) = mpsc::channel();
    drop(receiver);
    assert!(
        revoke_optional_and_wait_for_cleanup(
            &registry,
            &id,
            "network:intercept",
            &unavailable_session,
        )
        .is_err(),
        "a missing session acknowledgement must not look like successful cleanup"
    );
    assert_eq!(
        registry.capability_generation(&id, "network:intercept"),
        None,
        "an acknowledgement error must not restore the revoked grant"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn idle_session_physically_prunes_revoked_optional_navigation_rules() {
    let root = temp_frame_dir("idle-optional-network-revoke");
    std::fs::create_dir_all(&root).unwrap();
    let manifest = root.join("extension.json");
    std::fs::write(&manifest,
        r#"{"name":"Optional network","version":"1","blueice_api_version":1,"entry_point":"extension.wasm","capabilities":{"optional":["network:intercept"]}}"#
    ).unwrap();
    std::fs::write(root.join("extension.wasm"), b"\0asm\x01\0\0\0").unwrap();
    let installed = blueice_extension_host::load_installed_extension(&manifest).unwrap();
    let id = installed.extension_id().to_string();
    let registry = Arc::new(blueice_extension_host::registry_for_installed_extension(
        &installed,
    ));
    registry.grant_optional(&id, "network:intercept").unwrap();
    let grant_generation = registry
        .capability_generation(&id, "network:intercept")
        .unwrap();
    let (mut client, mut server) = client_pair();
    let (extension_tx, extension_rx) = mpsc::channel();
    let session_registry = Arc::clone(&registry);
    let session = thread::spawn({
        let id = id.clone();
        let root = root.clone();
        move || {
            let mut tabs = TabManager::new(320.0, 200.0);
            tabs.set_extension_permission_registry(session_registry, id);
            let mut generation = 0;
            let result = run_session_with_extension_requests(
                &mut tabs,
                &mut server,
                &root,
                &mut generation,
                Path::new("/not-used-for-optional-network"),
                &extension_rx,
            );
            (result, tabs.extension_navigation_rule_owner_count())
        }
    });
    handshake(&mut client);
    let (reply, result) = mpsc::channel();
    extension_tx
        .send(ExtensionPageRequest::RegisterNetworkBlockHost {
            connection_id: 7,
            grant_generation,
            host: "old.example.test".into(),
            reply,
        })
        .unwrap();
    result
        .recv_timeout(Duration::from_secs(2))
        .unwrap()
        .unwrap();
    registry.revoke_optional(&id, "network:intercept").unwrap();
    // No client or extension message follows. The 25 ms session poll
    // removes the now-inert rule before this client disconnects; the EOF
    // branch itself deliberately does not run cleanup.
    thread::sleep(Duration::from_millis(125));
    drop(client);
    let (result, remaining_rule_owners) = session.join().unwrap();
    result.unwrap();
    assert_eq!(remaining_rule_owners, 0);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn extension_network_rule_blocks_a_matching_navigation_before_gatekeeper_or_fetch() {
    let (mut client, mut server) = client_pair();
    let (extension_tx, extension_rx) = mpsc::channel();
    let dir = temp_frame_dir("extension-navigation-block-rule");
    let cleanup_dir = dir.clone();
    std::fs::create_dir_all(&dir).unwrap();
    // This path has no listener. A matching navigation must still produce
    // the synchronous declarative-rule error rather than attempting the
    // ordinary gatekeeper/fetch background path.
    let gatekeeper = PathBuf::from("/not-reached-for-extension-navigation-rule.sock");
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        let mut generation = 0;
        run_session_with_extension_requests(
            &mut tabs,
            &mut server,
            &dir,
            &mut generation,
            &gatekeeper,
            &extension_rx,
        )
    });

    blueice_ipc::client_handshake(&mut client).unwrap();
    let (rule_reply_tx, rule_reply_rx) = mpsc::channel();
    extension_tx
        .send(ExtensionPageRequest::RegisterNetworkBlockUrl {
            connection_id: 77,
            grant_generation: 0,
            url: "https://example.test/private#fragment".to_string(),
            reply: rule_reply_tx,
        })
        .unwrap();
    rule_reply_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("the live session must answer the extension rule request")
        .expect("a valid HTTPS rule must be installed");

    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::Navigate {
            url: "https://example.test/private".to_string(),
        },
    )
    .unwrap();
    match blueice_ipc::read_server_message(&mut client).unwrap() {
        ServerMessage::Error { message } => {
            assert!(message.contains("declarative extension rule"));
            assert!(message.contains("https://example.test/private"));
        }
        other => {
            panic!("matching extension rule must stop navigation before fetch, got {other:?}")
        }
    }

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    handle.join().unwrap().unwrap();
    let _ = std::fs::remove_dir_all(cleanup_dir);
}

#[test]
fn extension_network_rule_blocks_a_redirect_target_before_its_connection() {
    let (mut client, mut server) = client_pair();
    let (extension_tx, extension_rx) = mpsc::channel();
    let dir = temp_frame_dir("extension-redirect-navigation-block-rule");
    let cleanup_dir = dir.clone();
    std::fs::create_dir_all(&dir).unwrap();
    let target_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    target_listener.set_nonblocking(true).unwrap();
    let blocked_url = format!("http://{}/blocked", target_listener.local_addr().unwrap());
    let redirect_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let redirect_url = format!("http://{}/before", redirect_listener.local_addr().unwrap());
    let redirect_server = thread::spawn({
        let blocked_url = blocked_url.clone();
        move || {
            let (mut stream, _) = redirect_listener.accept().unwrap();
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf);
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 302 Found\r\nLocation: {blocked_url}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    )
                    .as_bytes(),
                )
                .unwrap();
        }
    });
    let gatekeeper = clearing_gatekeeper("extension-redirect-rule");
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        let mut generation = 0;
        run_session_with_extension_requests(
            &mut tabs,
            &mut server,
            &dir,
            &mut generation,
            &gatekeeper,
            &extension_rx,
        )
    });

    blueice_ipc::client_handshake(&mut client).unwrap();
    let (rule_reply_tx, rule_reply_rx) = mpsc::channel();
    extension_tx
        .send(ExtensionPageRequest::RegisterNetworkBlockUrl {
            connection_id: 78,
            grant_generation: 0,
            url: blocked_url.clone(),
            reply: rule_reply_tx,
        })
        .unwrap();
    rule_reply_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("the live session must accept the redirect-target rule")
        .expect("the target URL is a valid rule");

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Navigate { url: redirect_url })
        .unwrap();
    match blueice_ipc::read_server_message(&mut client).unwrap() {
        ServerMessage::Error { message } => {
            assert!(message.contains("declarative extension rule"));
            assert!(message.contains(&blocked_url));
        }
        other => panic!("the redirect target must be blocked, got {other:?}"),
    }
    redirect_server.join().unwrap();
    std::thread::sleep(Duration::from_millis(25));
    assert!(matches!(
        target_listener.accept(),
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock
    ));

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    handle.join().unwrap().unwrap();
    let _ = std::fs::remove_dir_all(cleanup_dir);
}

#[test]
fn extension_host_rule_blocks_a_redirect_target_before_its_connection() {
    let (mut client, mut server) = client_pair();
    let (extension_tx, extension_rx) = mpsc::channel();
    let dir = temp_frame_dir("extension-host-redirect-rule");
    let cleanup_dir = dir.clone();
    std::fs::create_dir_all(&dir).unwrap();
    let target_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    target_listener.set_nonblocking(true).unwrap();
    let blocked_url = format!(
        "http://localhost:{}/blocked",
        target_listener.local_addr().unwrap().port()
    );
    let redirect_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let redirect_url = format!("http://{}/before", redirect_listener.local_addr().unwrap());
    let redirect_server = thread::spawn({
        let blocked_url = blocked_url.clone();
        move || {
            let (mut stream, _) = redirect_listener.accept().unwrap();
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf);
            stream.write_all(
                format!("HTTP/1.1 302 Found\r\nLocation: {blocked_url}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").as_bytes(),
            ).unwrap();
        }
    });
    let gatekeeper = clearing_gatekeeper("extension-host-redirect-rule");
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        let mut generation = 0;
        run_session_with_extension_requests(
            &mut tabs,
            &mut server,
            &dir,
            &mut generation,
            &gatekeeper,
            &extension_rx,
        )
    });

    blueice_ipc::client_handshake(&mut client).unwrap();
    let (rule_reply_tx, rule_reply_rx) = mpsc::channel();
    extension_tx
        .send(ExtensionPageRequest::RegisterNetworkBlockHost {
            connection_id: 78,
            grant_generation: 0,
            host: "localhost".to_string(),
            reply: rule_reply_tx,
        })
        .unwrap();
    rule_reply_rx
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    blueice_ipc::write_client_message(&mut client, &ClientMessage::Navigate { url: redirect_url })
        .unwrap();
    match blueice_ipc::read_server_message(&mut client).unwrap() {
        ServerMessage::Error { message } => {
            assert!(message.contains("declarative extension rule"));
            assert!(message.contains(&blocked_url));
        }
        other => panic!("the redirect target must be blocked, got {other:?}"),
    }
    redirect_server.join().unwrap();
    std::thread::sleep(Duration::from_millis(25));
    assert!(matches!(
        target_listener.accept(),
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock
    ));
    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    handle.join().unwrap().unwrap();
    let _ = std::fs::remove_dir_all(cleanup_dir);
}

#[test]
fn committed_navigation_emits_a_bounded_core_defined_extension_event() {
    let (mut client, mut server) = client_pair();
    let (_extension_tx, extension_rx) = mpsc::channel();
    let (events_tx, events_rx) = mpsc::sync_channel(1);
    let dir = temp_frame_dir("extension-navigation-event");
    let cleanup_dir = dir.clone();
    std::fs::create_dir_all(&dir).unwrap();
    let gatekeeper = PathBuf::from("/not-used-for-built-in-navigation");
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        let mut generation = 0;
        run_session_with_extension_requests_and_events(
            &mut tabs,
            &mut server,
            &dir,
            &mut generation,
            &gatekeeper,
            &extension_rx,
            Some(&events_tx),
        )
    });

    blueice_ipc::client_handshake(&mut client).unwrap();
    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::Navigate {
            url: "about:credits".to_string(),
        },
    )
    .unwrap();
    assert!(matches!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::Navigated { .. }
    ));
    assert!(matches!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::FrameReady { .. }
    ));
    assert_eq!(
        events_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
        ExtensionRuntimeEvent::NavigationCommitted { tab_id: 1 }
    );

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    handle.join().unwrap().unwrap();
    let _ = std::fs::remove_dir_all(cleanup_dir);
}

#[test]
fn extension_v2_text_write_updates_the_addressed_input_and_pushes_a_frame() {
    let (mut client, mut server) = client_pair();
    let (extension_tx, extension_rx) = mpsc::channel();
    let dir = temp_frame_dir("extension-v2-text-write");
    let cleanup_dir = dir.clone();
    std::fs::create_dir_all(&dir).unwrap();
    let mut tabs = TabManager::new(320.0, 200.0);
    let tab_id = tabs.default_tab();
    tabs.get_mut(tab_id).unwrap().load_html_str(
        r#"<label for="shared">Shared field</label><input id="shared" type="text" value="before">"#,
        Some("https://example.test/form".to_string()),
    );
    let input_id = tabs
        .get(tab_id)
        .unwrap()
        .script_get_element_by_id("shared")
        .unwrap();
    let gatekeeper = PathBuf::from("/not-used-after-host-review");
    let handle = thread::spawn(move || {
        let mut generation = 0;
        run_session_with_extension_requests(
            &mut tabs,
            &mut server,
            &dir,
            &mut generation,
            &gatekeeper,
            &extension_rx,
        )
    });

    blueice_ipc::client_handshake(&mut client).unwrap();
    let (reply_tx, reply_rx) = mpsc::channel();
    extension_tx
        .send(ExtensionPageRequest::SetTextInputValue {
            tab_id: tab_id.as_u64(),
            node_id: input_id.as_u64(),
            value: "from extension".to_string(),
            grant_generation: 0,
            reply: reply_tx,
        })
        .unwrap();
    reply_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("the live session must answer the extension write")
        .expect("the addressed text input must accept the value");
    let (reply_tab, request_id, frame) =
        blueice_ipc::read_server_message_with_ids(&mut client).unwrap();
    assert_eq!(reply_tab, Some(tab_id.as_u64()));
    assert_eq!(request_id, None);
    assert!(matches!(frame, ServerMessage::FrameReady { .. }));

    let (read_tx, read_rx) = mpsc::channel();
    extension_tx
        .send(ExtensionPageRequest::ReadRepresentation {
            tab_id: Some(tab_id.as_u64()),
            reply: read_tx,
        })
        .unwrap();
    let snapshot: blueice_ipc::AiSnapshot = serde_json::from_str(
        &read_rx
            .recv_timeout(Duration::from_secs(1))
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(snapshot.tab_id, tab_id.as_u64());
    assert_eq!(
        snapshot
            .nodes
            .iter()
            .find(|node| node.id == input_id.as_u64())
            .and_then(|node| node.state.value.as_deref()),
        Some("from extension")
    );

    let (reply_tx, reply_rx) = mpsc::channel();
    extension_tx
        .send(ExtensionPageRequest::SetTextInputValue {
            tab_id: tab_id.as_u64(),
            node_id: input_id.as_u64(),
            value: "x".repeat(blueice_ipc::extension::MAX_TEXT_WRITE_BYTES + 1),
            grant_generation: 0,
            reply: reply_tx,
        })
        .unwrap();
    let error = reply_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("the live session must answer an oversized text-input write")
        .expect_err("the core must reject oversized text-control values");
    assert!(error.contains("4096 bytes"));

    let (read_tx, read_rx) = mpsc::channel();
    extension_tx
        .send(ExtensionPageRequest::ReadRepresentation {
            tab_id: Some(tab_id.as_u64()),
            reply: read_tx,
        })
        .unwrap();
    let snapshot: blueice_ipc::AiSnapshot = serde_json::from_str(
        &read_rx
            .recv_timeout(Duration::from_secs(1))
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        snapshot
            .nodes
            .iter()
            .find(|node| node.id == input_id.as_u64())
            .and_then(|node| node.state.value.as_deref()),
        Some("from extension")
    );

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    handle.join().unwrap().unwrap();
    let _ = std::fs::remove_dir_all(cleanup_dir);
}

#[test]
fn extension_v3_checkbox_write_updates_the_addressed_control_and_pushes_a_frame() {
    let (mut client, mut server) = client_pair();
    let (extension_tx, extension_rx) = mpsc::channel();
    let dir = temp_frame_dir("extension-v3-checkbox-write");
    let cleanup_dir = dir.clone();
    std::fs::create_dir_all(&dir).unwrap();
    let mut tabs = TabManager::new(320.0, 200.0);
    let tab_id = tabs.default_tab();
    tabs.get_mut(tab_id).unwrap().load_html_str(
        r#"<label for="agree">Agree</label><input id="agree" type="checkbox">"#,
        Some("https://example.test/form".to_string()),
    );
    let checkbox_id = tabs
        .get(tab_id)
        .unwrap()
        .script_get_element_by_id("agree")
        .unwrap();
    let gatekeeper = PathBuf::from("/not-used-after-host-review");
    let handle = thread::spawn(move || {
        let mut generation = 0;
        run_session_with_extension_requests(
            &mut tabs,
            &mut server,
            &dir,
            &mut generation,
            &gatekeeper,
            &extension_rx,
        )
    });

    blueice_ipc::client_handshake(&mut client).unwrap();
    let (reply_tx, reply_rx) = mpsc::channel();
    extension_tx
        .send(ExtensionPageRequest::SetCheckboxChecked {
            tab_id: tab_id.as_u64(),
            node_id: checkbox_id.as_u64(),
            checked: true,
            grant_generation: 0,
            reply: reply_tx,
        })
        .unwrap();
    reply_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("the live session must answer the extension checkbox write")
        .expect("the addressed checkbox must accept its checked state");
    let (reply_tab, request_id, frame) =
        blueice_ipc::read_server_message_with_ids(&mut client).unwrap();
    assert_eq!(reply_tab, Some(tab_id.as_u64()));
    assert_eq!(request_id, None);
    assert!(matches!(frame, ServerMessage::FrameReady { .. }));

    let (read_tx, read_rx) = mpsc::channel();
    extension_tx
        .send(ExtensionPageRequest::ReadRepresentation {
            tab_id: Some(tab_id.as_u64()),
            reply: read_tx,
        })
        .unwrap();
    let snapshot: blueice_ipc::AiSnapshot = serde_json::from_str(
        &read_rx
            .recv_timeout(Duration::from_secs(1))
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        snapshot
            .nodes
            .iter()
            .find(|node| node.id == checkbox_id.as_u64())
            .and_then(|node| node.state.checked),
        Some(true)
    );

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    handle.join().unwrap().unwrap();
    let _ = std::fs::remove_dir_all(cleanup_dir);
}

#[test]
fn extension_v4_textarea_write_updates_the_addressed_control_and_pushes_a_frame() {
    let (mut client, mut server) = client_pair();
    let (extension_tx, extension_rx) = mpsc::channel();
    let dir = temp_frame_dir("extension-v4-textarea-write");
    let cleanup_dir = dir.clone();
    std::fs::create_dir_all(&dir).unwrap();
    let mut tabs = TabManager::new(320.0, 200.0);
    let tab_id = tabs.default_tab();
    tabs.get_mut(tab_id).unwrap().load_html_str(
        r#"<label for="notes">Notes</label><textarea id="notes">before</textarea>"#,
        Some("https://example.test/form".to_string()),
    );
    let textarea_id = tabs
        .get(tab_id)
        .unwrap()
        .script_get_element_by_id("notes")
        .unwrap();
    let gatekeeper = PathBuf::from("/not-used-after-host-review");
    let handle = thread::spawn(move || {
        let mut generation = 0;
        run_session_with_extension_requests(
            &mut tabs,
            &mut server,
            &dir,
            &mut generation,
            &gatekeeper,
            &extension_rx,
        )
    });

    blueice_ipc::client_handshake(&mut client).unwrap();
    let (reply_tx, reply_rx) = mpsc::channel();
    extension_tx
        .send(ExtensionPageRequest::SetTextareaValue {
            tab_id: tab_id.as_u64(),
            node_id: textarea_id.as_u64(),
            value: "from extension\nwith detail".to_string(),
            grant_generation: 0,
            reply: reply_tx,
        })
        .unwrap();
    reply_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("the live session must answer the extension textarea write")
        .expect("the addressed textarea must accept its value");
    let (reply_tab, request_id, frame) =
        blueice_ipc::read_server_message_with_ids(&mut client).unwrap();
    assert_eq!(reply_tab, Some(tab_id.as_u64()));
    assert_eq!(request_id, None);
    assert!(matches!(frame, ServerMessage::FrameReady { .. }));

    let (read_tx, read_rx) = mpsc::channel();
    extension_tx
        .send(ExtensionPageRequest::ReadRepresentation {
            tab_id: Some(tab_id.as_u64()),
            reply: read_tx,
        })
        .unwrap();
    let snapshot: blueice_ipc::AiSnapshot = serde_json::from_str(
        &read_rx
            .recv_timeout(Duration::from_secs(1))
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        snapshot
            .nodes
            .iter()
            .find(|node| node.id == textarea_id.as_u64())
            .and_then(|node| node.state.value.as_deref()),
        Some("from extension with detail")
    );

    let (reply_tx, reply_rx) = mpsc::channel();
    extension_tx
        .send(ExtensionPageRequest::SetTextareaValue {
            tab_id: tab_id.as_u64(),
            node_id: textarea_id.as_u64(),
            value: "x".repeat(blueice_ipc::extension::MAX_TEXT_WRITE_BYTES + 1),
            grant_generation: 0,
            reply: reply_tx,
        })
        .unwrap();
    let error = reply_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("the live session must answer an oversized textarea write")
        .expect_err("the core must reject oversized text-control values");
    assert!(error.contains("4096 bytes"));

    let (read_tx, read_rx) = mpsc::channel();
    extension_tx
        .send(ExtensionPageRequest::ReadRepresentation {
            tab_id: Some(tab_id.as_u64()),
            reply: read_tx,
        })
        .unwrap();
    let snapshot: blueice_ipc::AiSnapshot = serde_json::from_str(
        &read_rx
            .recv_timeout(Duration::from_secs(1))
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        snapshot
            .nodes
            .iter()
            .find(|node| node.id == textarea_id.as_u64())
            .and_then(|node| node.state.value.as_deref()),
        Some("from extension with detail")
    );

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    handle.join().unwrap().unwrap();
    let _ = std::fs::remove_dir_all(cleanup_dir);
}

#[test]
fn extension_v8_visible_leaf_write_updates_the_shared_frame_and_representation() {
    let (mut client, mut server) = client_pair();
    let (extension_tx, extension_rx) = mpsc::channel();
    let dir = temp_frame_dir("extension-v8-visible-leaf");
    let cleanup_dir = dir.clone();
    std::fs::create_dir_all(&dir).unwrap();
    let mut tabs = TabManager::new(320.0, 200.0);
    let tab_id = tabs.default_tab();
    tabs.get_mut(tab_id).unwrap().load_html_str(
        "<h1 id=\"headline\">Before</h1>",
        Some("https://example.test/".to_string()),
    );
    let node_id = tabs
        .get(tab_id)
        .unwrap()
        .script_get_element_by_id("headline")
        .unwrap();
    let gatekeeper = PathBuf::from("/not-used-after-host-review");
    let handle = thread::spawn(move || {
        let mut generation = 0;
        run_session_with_extension_requests(
            &mut tabs,
            &mut server,
            &dir,
            &mut generation,
            &gatekeeper,
            &extension_rx,
        )
    });
    blueice_ipc::client_handshake(&mut client).unwrap();
    let (reply_tx, reply_rx) = mpsc::channel();
    extension_tx
        .send(ExtensionPageRequest::SetVisibleLeafText {
            tab_id: tab_id.as_u64(),
            node_id: node_id.as_u64(),
            value: "After".to_string(),
            grant_generation: 0,
            reply: reply_tx,
        })
        .unwrap();
    reply_rx
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    let (frame_tab, frame_request, frame) =
        blueice_ipc::read_server_message_with_ids(&mut client).unwrap();
    assert_eq!(frame_tab, Some(tab_id.as_u64()));
    assert_eq!(frame_request, None);
    assert!(matches!(frame, ServerMessage::FrameReady { .. }));
    let (read_tx, read_rx) = mpsc::channel();
    extension_tx
        .send(ExtensionPageRequest::ReadRepresentation {
            tab_id: Some(tab_id.as_u64()),
            reply: read_tx,
        })
        .unwrap();
    let snapshot: blueice_ipc::AiSnapshot = serde_json::from_str(
        &read_rx
            .recv_timeout(Duration::from_secs(1))
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        snapshot
            .nodes
            .iter()
            .find(|node| node.id == node_id.as_u64())
            .and_then(|node| node.name.as_deref()),
        Some("After")
    );
    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    handle.join().unwrap().unwrap();
    let _ = std::fs::remove_dir_all(cleanup_dir);
}

#[test]
fn extension_origin_scopes_are_checked_against_each_live_tab_before_reads_or_writes() {
    use std::collections::{BTreeMap, BTreeSet};
    let (mut client, mut server) = client_pair();
    let (extension_tx, extension_rx) = mpsc::channel();
    let dir = temp_frame_dir("extension-origin-scopes");
    let cleanup_dir = dir.clone();
    std::fs::create_dir_all(&dir).unwrap();
    let mut tabs = TabManager::new(320.0, 200.0);
    let allowed_tab = tabs.default_tab();
    let blocked_tab = tabs.open_tab();
    tabs.get_mut(allowed_tab).unwrap().load_html_str(
        "<h1 id=\"title\">Allowed</h1>",
        Some("https://allowed.test/page".to_string()),
    );
    tabs.get_mut(blocked_tab).unwrap().load_html_str(
        "<h1 id=\"title\">Blocked</h1>",
        Some("https://blocked.test/page".to_string()),
    );
    let allowed_node = tabs
        .get(allowed_tab)
        .unwrap()
        .script_get_element_by_id("title")
        .unwrap();
    let blocked_node = tabs
        .get(blocked_tab)
        .unwrap()
        .script_get_element_by_id("title")
        .unwrap();
    tabs.set_extension_capability_origins(BTreeMap::from([
        (
            "dom:read".to_string(),
            BTreeSet::from(["https://allowed.test".to_string()]),
        ),
        (
            "dom:write".to_string(),
            BTreeSet::from(["https://allowed.test".to_string()]),
        ),
        (
            "network:observe".to_string(),
            BTreeSet::from(["https://allowed.test".to_string()]),
        ),
    ]));
    let gatekeeper = PathBuf::from("/not-used-after-host-review");
    let handle = thread::spawn(move || {
        let mut generation = 0;
        run_session_with_extension_requests(
            &mut tabs,
            &mut server,
            &dir,
            &mut generation,
            &gatekeeper,
            &extension_rx,
        )
    });
    blueice_ipc::client_handshake(&mut client).unwrap();
    for (tab_id, permitted) in [(blocked_tab, false), (allowed_tab, true)] {
        let (reply_tx, reply_rx) = mpsc::channel();
        extension_tx
            .send(ExtensionPageRequest::ReadRepresentation {
                tab_id: Some(tab_id.as_u64()),
                reply: reply_tx,
            })
            .unwrap();
        assert_eq!(
            reply_rx
                .recv_timeout(Duration::from_secs(1))
                .unwrap()
                .is_ok(),
            permitted
        );
        let (reply_tx, reply_rx) = mpsc::channel();
        extension_tx
            .send(ExtensionPageRequest::ReadNetworkResponse {
                tab_id: tab_id.as_u64(),
                reply: reply_tx,
            })
            .unwrap();
        assert_eq!(
            reply_rx
                .recv_timeout(Duration::from_secs(1))
                .unwrap()
                .is_ok(),
            permitted
        );
    }
    let (reply_tx, reply_rx) = mpsc::channel();
    extension_tx
        .send(ExtensionPageRequest::SetVisibleLeafText {
            tab_id: blocked_tab.as_u64(),
            node_id: blocked_node.as_u64(),
            value: "must not change".to_string(),
            grant_generation: 0,
            reply: reply_tx,
        })
        .unwrap();
    assert!(reply_rx
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap_err()
        .contains("not granted"));
    let (reply_tx, reply_rx) = mpsc::channel();
    extension_tx
        .send(ExtensionPageRequest::SetVisibleLeafText {
            tab_id: allowed_tab.as_u64(),
            node_id: allowed_node.as_u64(),
            value: "Updated".to_string(),
            grant_generation: 0,
            reply: reply_tx,
        })
        .unwrap();
    reply_rx
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    let (frame_tab, _, frame) = blueice_ipc::read_server_message_with_ids(&mut client).unwrap();
    assert_eq!(
        frame_tab,
        Some(allowed_tab.as_u64()),
        "a rejected write must not publish a frame"
    );
    assert!(matches!(frame, ServerMessage::FrameReady { .. }));
    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    handle.join().unwrap().unwrap();
    let _ = std::fs::remove_dir_all(cleanup_dir);
}

/// A monotonic counter alongside the PID, so every call is unique
/// regardless of how many concurrent tests (each running on its own
/// thread, in this one test binary process) call it -- same
/// discipline `blueice-mcp-server`'s own `unique_socket_path` uses,
/// necessary here because many gatekeeper-behavior tests below each
/// need their own independent fake listener. `_label` exists purely
/// so call sites read self-documenting (`clearing_gatekeeper("foo-
/// test")`) -- deliberately *not* included in the actual path: a
/// Unix domain socket path is capped at ~100 bytes total
/// (`sockaddr_un::sun_path`, tighter on macOS than Linux), and this
/// module's already-long, already-temp-dir-prefixed test names
/// would blow that budget immediately if concatenated in.
fn unique_gatekeeper_socket_path(_label: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("bl-gk-{}-{n}.sock", std::process::id()))
}

/// Spins up a background listener that behaves exactly like `ai-
/// gatekeeper`'s own trivial minimal-slice stub (always clears),
/// bound to a fresh socket path unique to this call. Every existing
/// test below that navigates needs *some* gatekeeper behind the
/// path it gives `run_session` -- not because gating itself is
/// under test there (see the dedicated gatekeeper-behavior tests
/// further down for that), but because a genuinely unreachable
/// gatekeeper fails closed, which would turn those tests'
/// pre-existing "navigation always succeeds" assertions false. This
/// keeps every one of those assertions unmodified.
fn clearing_gatekeeper(label: &str) -> PathBuf {
    let path = unique_gatekeeper_socket_path(label);
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path).unwrap();
    thread::spawn(move || {
        for incoming in listener.incoming() {
            let Ok(mut stream) = incoming else { break };
            let _ = blueice_ai_gatekeeper::handle_one_check(&mut stream);
        }
    });
    path
}

/// Performs the `protocol_version` handshake `run_session` now
/// requires as the very first message on a fresh connection --
/// every test below drives `run_session` over a brand-new
/// connection, so every one of them needs this before its own
/// message(s), the same way a real client (`frontend`, `blueice-
/// mcp-server`) would via `blueice_ipc::client_handshake`.
fn handshake(client: &mut UnixStream) {
    blueice_ipc::client_handshake(client).unwrap();
}

/// Every test below that predates multi-tab (Phase 16) sets up its
/// fixture content on "the" page, the same single-tab shape it
/// always had -- this is just `tabs.default_tab()` resolved to its
/// `Page`, so those tests don't need to change beyond `Page::new`
/// becoming `TabManager::new`.
fn default_page(tabs: &mut TabManager) -> &mut Page {
    let default = tabs.default_tab();
    tabs.get_mut(default).unwrap()
}

#[test]
fn resize_then_shutdown_produces_one_frame_and_then_ends_the_session() {
    let dir = temp_frame_dir("resize");
    let gatekeeper = clearing_gatekeeper("resize");
    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        default_page(&mut tabs).load_html_str("<p>hi</p>", None);
        let mut generation = 0u64;
        run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
        dir
    });
    handshake(&mut client);

    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::Resize {
            width: 100,
            height: 50,
        },
    )
    .unwrap();
    let reply = blueice_ipc::read_server_message(&mut client).unwrap();
    assert!(matches!(
        reply,
        ServerMessage::FrameReady {
            generation: 1,
            width: 100,
            height: 50,
            ..
        }
    ));

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn navigate_replies_with_navigated_then_a_frame_reflecting_the_new_page() {
    let dir = temp_frame_dir("navigate");
    let gatekeeper = clearing_gatekeeper("navigate");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = [0u8; 1024];
        let _ = std::io::Read::read(&mut stream, &mut buf);
        let body = "<p>fetched page</p>";
        std::io::Write::write_all(
            &mut stream,
            format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .as_bytes(),
        )
        .unwrap();
    });

    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        let mut generation = 0u64;
        run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
        dir
    });
    handshake(&mut client);

    let url = format!("http://{addr}");
    blueice_ipc::write_client_message(&mut client, &ClientMessage::Navigate { url: url.clone() })
        .unwrap();
    let navigated = blueice_ipc::read_server_message(&mut client).unwrap();
    assert_eq!(navigated, ServerMessage::Navigated { url });
    let frame = blueice_ipc::read_server_message(&mut client).unwrap();
    let shm_path = match frame {
        ServerMessage::FrameReady {
            shm_path,
            generation: 1,
            ..
        } => shm_path,
        other => panic!("expected FrameReady, got {other:?}"),
    };
    assert!(
        shm::map_frame(std::path::Path::new(&shm_path)).is_ok(),
        "the frame-plane file must actually exist and be mappable"
    );

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn navigate_to_an_unreachable_host_replies_with_error_not_a_frame() {
    let dir = temp_frame_dir("navigate-error");
    let gatekeeper = clearing_gatekeeper("navigate-error");
    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        let mut generation = 0u64;
        run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
        dir
    });
    handshake(&mut client);

    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::Navigate {
            url: "not-a-valid-url".to_string(),
        },
    )
    .unwrap();
    let reply = blueice_ipc::read_server_message(&mut client).unwrap();
    assert!(matches!(reply, ServerMessage::Error { .. }));

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn click_on_a_link_navigates_and_a_click_elsewhere_produces_no_reply() {
    let dir = temp_frame_dir("click");
    let gatekeeper = clearing_gatekeeper("click");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = [0u8; 1024];
        let _ = std::io::Read::read(&mut stream, &mut buf);
        let body = "<p>landed</p>";
        std::io::Write::write_all(
            &mut stream,
            format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .as_bytes(),
        )
        .unwrap();
    });
    let url = format!("http://{addr}");

    let (mut client, mut server) = client_pair();
    let dir_for_thread = dir.clone();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        default_page(&mut tabs).load_html_str(&format!(r#"<a href="{url}">go</a>"#), None);
        let mut generation = 0u64;
        run_session(
            &mut tabs,
            &mut server,
            &dir_for_thread,
            &mut generation,
            &gatekeeper,
        )
        .unwrap();
    });
    handshake(&mut client);

    // clicking the link navigates: expect Navigated then FrameReady
    blueice_ipc::write_client_message(&mut client, &ClientMessage::Click { x: 2.0, y: 2.0 })
        .unwrap();
    let navigated = blueice_ipc::read_server_message(&mut client).unwrap();
    assert!(matches!(navigated, ServerMessage::Navigated { .. }));
    let frame = blueice_ipc::read_server_message(&mut client).unwrap();
    assert!(matches!(frame, ServerMessage::FrameReady { .. }));

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn set_visible_produces_no_reply_and_the_session_keeps_running() {
    let dir = temp_frame_dir("visible");
    let gatekeeper = clearing_gatekeeper("visible");
    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        let mut generation = 0u64;
        run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
        dir
    });
    handshake(&mut client);

    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::Chrome(blueice_ipc::ChromeCommand::SetVisible(false)),
    )
    .unwrap();
    // proven by the fact that a subsequent message still gets a
    // normal reply -- Chrome(SetVisible) didn't wedge or end the session.
    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::Resize {
            width: 10,
            height: 10,
        },
    )
    .unwrap();
    let reply = blueice_ipc::read_server_message(&mut client).unwrap();
    assert!(matches!(reply, ServerMessage::FrameReady { .. }));

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn get_representation_shares_the_current_generation_across_every_send_frame_call_site() {
    // `get_representation_shares_the_current_generation_with_the_last_frame`
    // below proves the "same render pass" invariant for `Resize`
    // alone; this extends the same proof to `Scroll`, `Highlight`,
    // and a non-navigating `ActOn` (`Focus`) -- the other distinct
    // `send_frame` call sites in `run_session` (`Click`/`ActOn`'s
    // Click variant only ever reach `send_frame` via the same
    // navigate path `Navigate` itself already exercises, so they add
    // no new coverage here). `send_frame` is a single choke point
    // every one of these routes through, so this is expected to
    // hold structurally -- but the invariant is central enough to
    // this project's premise to prove per call site, not infer from
    // one example.
    let dir = temp_frame_dir("representation-generation-all-sites");
    let gatekeeper = clearing_gatekeeper("representation-generation-all-sites");
    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        default_page(&mut tabs).load_html_str(r#"<input type="text">"#, None);
        let mut generation = 0u64;
        run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
        dir
    });
    handshake(&mut client);

    blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
    let ServerMessage::Representation(snap) =
        blueice_ipc::read_server_message(&mut client).unwrap()
    else {
        panic!("expected Representation")
    };
    let input_id = snap.nodes[0].id;

    let assert_matching_generation = |client: &mut UnixStream, send: ClientMessage| {
        blueice_ipc::write_client_message(client, &send).unwrap();
        let frame = blueice_ipc::read_server_message(client).unwrap();
        let ServerMessage::FrameReady {
            generation: frame_generation,
            ..
        } = frame
        else {
            panic!("expected FrameReady, got {frame:?}")
        };

        blueice_ipc::write_client_message(client, &ClientMessage::GetRepresentation).unwrap();
        let reply = blueice_ipc::read_server_message(client).unwrap();
        let ServerMessage::Representation(snapshot) = reply else {
            panic!("expected Representation, got {reply:?}")
        };
        assert_eq!(snapshot.generation, frame_generation);
    };

    assert_matching_generation(&mut client, ClientMessage::Scroll { delta_y: 10.0 });
    assert_matching_generation(&mut client, ClientMessage::Highlight { id: Some(input_id) });
    assert_matching_generation(
        &mut client,
        ClientMessage::ActOn {
            id: input_id,
            action: NodeAction::Focus,
        },
    );

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn get_representation_shares_the_current_generation_with_the_last_frame() {
    // the concrete, checkable "same render pass" proof
    // `phase-5-ai-representation-output/PLAN.md` asks for: a
    // Representation and the FrameReady sent alongside a prior
    // state change carry the identical generation number.
    let dir = temp_frame_dir("representation-generation");
    let gatekeeper = clearing_gatekeeper("representation-generation");
    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        default_page(&mut tabs).load_html_str(r#"<a href="/x">Go</a>"#, None);
        let mut generation = 0u64;
        run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
        dir
    });
    handshake(&mut client);

    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::Resize {
            width: 100,
            height: 50,
        },
    )
    .unwrap();
    let frame = blueice_ipc::read_server_message(&mut client).unwrap();
    let ServerMessage::FrameReady {
        generation: frame_generation,
        ..
    } = frame
    else {
        panic!("expected FrameReady, got {frame:?}")
    };

    blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
    let reply = blueice_ipc::read_server_message(&mut client).unwrap();
    let ServerMessage::Representation(snapshot) = reply else {
        panic!("expected Representation, got {reply:?}")
    };
    assert_eq!(snapshot.generation, frame_generation);
    assert!(snapshot
        .nodes
        .iter()
        .any(|n| n.name.as_deref() == Some("Go")));

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn get_dom_returns_the_full_tree_unfiltered_by_the_ai_representation() {
    let dir = temp_frame_dir("get-dom");
    let gatekeeper = clearing_gatekeeper("get-dom");
    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        default_page(&mut tabs)
            .load_html_str(r#"<div style="background-color: red;">x</div>"#, None);
        let mut generation = 0u64;
        run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
        dir
    });
    handshake(&mut client);

    blueice_ipc::write_client_message(&mut client, &ClientMessage::GetDom).unwrap();
    let reply = blueice_ipc::read_server_message(&mut client).unwrap();
    let ServerMessage::Dom(dump) = reply else {
        panic!("expected Dom, got {reply:?}")
    };
    assert!(
        dump.contains("<div>"),
        "a bare div has no AI-representation role but must still appear in the full DOM dump: {dump}"
    );

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn act_on_click_navigates_the_same_way_a_coordinate_click_does() {
    let dir = temp_frame_dir("act-on-click");
    let gatekeeper = clearing_gatekeeper("act-on-click");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = [0u8; 1024];
        let _ = std::io::Read::read(&mut stream, &mut buf);
        let body = "<p>landed via id</p>";
        std::io::Write::write_all(
            &mut stream,
            format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .as_bytes(),
        )
        .unwrap();
    });
    let url = format!("http://{addr}");

    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        default_page(&mut tabs).load_html_str(&format!(r#"<a href="{url}">go</a>"#), None);
        let mut generation = 0u64;
        run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
        dir
    });
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
    let navigated = blueice_ipc::read_server_message(&mut client).unwrap();
    assert!(matches!(navigated, ServerMessage::Navigated { .. }));
    let frame = blueice_ipc::read_server_message(&mut client).unwrap();
    assert!(matches!(frame, ServerMessage::FrameReady { .. }));

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn act_on_focus_is_reflected_in_the_next_representation() {
    let dir = temp_frame_dir("act-on-focus");
    let gatekeeper = clearing_gatekeeper("act-on-focus");
    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        default_page(&mut tabs)
            .load_html_str(r#"<input id="name" type="text" placeholder="Name">"#, None);
        let mut generation = 0u64;
        run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
        dir
    });
    handshake(&mut client);

    blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
    let ServerMessage::Representation(before) =
        blueice_ipc::read_server_message(&mut client).unwrap()
    else {
        panic!("expected Representation")
    };
    let input_id = before.nodes[0].id;
    assert!(!before.nodes[0].state.focused);

    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::ActOn {
            id: input_id,
            action: NodeAction::Focus,
        },
    )
    .unwrap();
    let frame = blueice_ipc::read_server_message(&mut client).unwrap();
    assert!(
        matches!(frame, ServerMessage::FrameReady { .. }),
        "Focus is a state change and still gets a FrameReady, per session.rs's own docs"
    );

    blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
    let ServerMessage::Representation(after) =
        blueice_ipc::read_server_message(&mut client).unwrap()
    else {
        panic!("expected Representation")
    };
    assert!(after.nodes[0].state.focused);

    // The native frontend sends these narrower keyboard messages rather
    // than guessing a DOM node ID. They are accepted only because the
    // preceding focus action selected this supported text input.
    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::InsertText {
            text: "BlueIce".to_string(),
        },
    )
    .unwrap();
    assert!(matches!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::FrameReady { .. }
    ));
    blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
    let ServerMessage::Representation(with_text) =
        blueice_ipc::read_server_message(&mut client).unwrap()
    else {
        panic!("expected Representation")
    };
    assert_eq!(with_text.nodes[0].state.value.as_deref(), Some("BlueIce"));

    blueice_ipc::write_client_message(&mut client, &ClientMessage::DeleteBackward).unwrap();
    assert!(matches!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::FrameReady { .. }
    ));
    blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
    let ServerMessage::Representation(after_delete) =
        blueice_ipc::read_server_message(&mut client).unwrap()
    else {
        panic!("expected Representation")
    };
    assert_eq!(after_delete.nodes[0].state.value.as_deref(), Some("BlueIc"));

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn act_on_set_value_is_reflected_in_the_next_representation() {
    let dir = temp_frame_dir("act-on-set-value");
    let gatekeeper = clearing_gatekeeper("act-on-set-value");
    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        default_page(&mut tabs)
            .load_html_str(r#"<input id="name" type="text" placeholder="Name">"#, None);
        let mut generation = 0u64;
        run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
        dir
    });
    handshake(&mut client);

    blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
    let ServerMessage::Representation(before) =
        blueice_ipc::read_server_message(&mut client).unwrap()
    else {
        panic!("expected Representation")
    };
    let input_id = before.nodes[0].id;
    assert_eq!(before.nodes[0].state.value, None);

    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::ActOn {
            id: input_id,
            action: NodeAction::SetValue("BlueIce".to_string()),
        },
    )
    .unwrap();
    assert!(matches!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::FrameReady { .. }
    ));

    blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
    let ServerMessage::Representation(after) =
        blueice_ipc::read_server_message(&mut client).unwrap()
    else {
        panic!("expected Representation")
    };
    assert_eq!(after.nodes[0].state.value.as_deref(), Some("BlueIce"));

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn act_on_an_unknown_id_is_a_harmless_no_op() {
    let dir = temp_frame_dir("act-on-unknown");
    let gatekeeper = clearing_gatekeeper("act-on-unknown");
    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        default_page(&mut tabs).load_html_str(r#"<a href="/x">go</a>"#, None);
        let mut generation = 0u64;
        run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
        dir
    });
    handshake(&mut client);

    // an unknown id with Click: same "no reply at all" contract as
    // a coordinate click that lands on nothing.
    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::ActOn {
            id: 999_999,
            action: NodeAction::Click,
        },
    )
    .unwrap();
    // proven by the fact that the next message still gets a normal
    // reply -- the unknown id didn't wedge or end the session.
    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::Resize {
            width: 10,
            height: 10,
        },
    )
    .unwrap();
    let reply = blueice_ipc::read_server_message(&mut client).unwrap();
    assert!(matches!(reply, ServerMessage::FrameReady { .. }));

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_stale_id_from_before_a_navigation_is_a_harmless_no_op_after_it() {
    // Unlike `act_on_an_unknown_id_is_a_harmless_no_op` (a
    // never-allocated id), this id is real -- it existed in the
    // document *before* the navigation below. Regression: NodeId
    // allocation used to restart at 0 for every freshly-parsed
    // document, so this same numeric id could be reused by an
    // unrelated node in the post-navigation document, and ActOn
    // would silently act on that unrelated node instead of safely
    // no-op'ing.
    let dir = temp_frame_dir("stale-id-across-navigation");
    let gatekeeper = clearing_gatekeeper("stale-id-across-navigation");
    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        default_page(&mut tabs).load_html_str(r#"<a href="/x">go</a>"#, None);
        let mut generation = 0u64;
        run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
        dir
    });
    handshake(&mut client);

    blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
    let ServerMessage::Representation(snap) =
        blueice_ipc::read_server_message(&mut client).unwrap()
    else {
        panic!("expected Representation")
    };
    let stale_id = snap.nodes[0].id;

    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::Navigate {
            url: "about:blank".to_string(),
        },
    )
    .unwrap();
    let navigated = blueice_ipc::read_server_message(&mut client).unwrap();
    assert_eq!(
        navigated,
        ServerMessage::Navigated {
            url: "about:blank".to_string(),
        }
    );
    let _frame = blueice_ipc::read_server_message(&mut client).unwrap();

    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::ActOn {
            id: stale_id,
            action: NodeAction::Click,
        },
    )
    .unwrap();
    // proven the same way as the never-allocated-id case: the next
    // message still gets a normal reply, so the stale id neither
    // wedged the session nor triggered a misdirected action.
    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::Resize {
            width: 10,
            height: 10,
        },
    )
    .unwrap();
    let reply = blueice_ipc::read_server_message(&mut client).unwrap();
    assert!(matches!(reply, ServerMessage::FrameReady { .. }));

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn highlight_adds_an_outline_to_the_next_frame_and_clearing_it_removes_it() {
    let dir = temp_frame_dir("highlight");
    let gatekeeper = clearing_gatekeeper("highlight");
    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        default_page(&mut tabs).load_html_str(r#"<a href="/x">go</a>"#, None);
        let mut generation = 0u64;
        run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
        dir
    });
    handshake(&mut client);

    blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
    let ServerMessage::Representation(snap) =
        blueice_ipc::read_server_message(&mut client).unwrap()
    else {
        panic!("expected Representation")
    };
    let link_id = snap.nodes[0].id;

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Highlight { id: Some(link_id) })
        .unwrap();
    let frame = blueice_ipc::read_server_message(&mut client).unwrap();
    assert!(matches!(frame, ServerMessage::FrameReady { .. }));

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn hover_updates_state_silently_with_no_reply() {
    let dir = temp_frame_dir("hover");
    let gatekeeper = clearing_gatekeeper("hover");
    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        default_page(&mut tabs).load_html_str(r#"<a href="/x">go</a>"#, None);
        let mut generation = 0u64;
        run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
        dir
    });
    handshake(&mut client);

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Hover { x: 2.0, y: 2.0 })
        .unwrap();
    // proven the same way SetVisible/Chrome is: the next message
    // still gets a normal reply, so Hover didn't wedge the session.
    blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
    let ServerMessage::Representation(snap) =
        blueice_ipc::read_server_message(&mut client).unwrap()
    else {
        panic!("expected Representation")
    };
    assert!(
        snap.nodes[0].state.hovered,
        "the hovered state must be visible via GetRepresentation"
    );

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn chrome_set_visible_does_not_change_engine_render_state() {
    // `phase-5-ai-representation-output/PLAN.md`'s "verify engine
    // state is unchanged across a hide/show cycle" checklist item,
    // made explicit and checkable rather than left implicit in
    // `Chrome`'s no-op handling: a full hide-then-show round trip
    // must leave the representation (and therefore the DOM/styles/
    // fragment tree it's derived from) byte-for-byte identical, and
    // must not cause a new frame to be rendered.
    let dir = temp_frame_dir("chrome-no-restart");
    let gatekeeper = clearing_gatekeeper("chrome-no-restart");
    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        default_page(&mut tabs).load_html_str("<p>hi</p>", None);
        let mut generation = 0u64;
        run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
        dir
    });
    handshake(&mut client);

    blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
    let ServerMessage::Representation(before) =
        blueice_ipc::read_server_message(&mut client).unwrap()
    else {
        panic!("expected Representation")
    };

    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::Chrome(blueice_ipc::ChromeCommand::SetVisible(false)),
    )
    .unwrap();
    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::Chrome(blueice_ipc::ChromeCommand::SetVisible(true)),
    )
    .unwrap();

    blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
    let ServerMessage::Representation(after) =
        blueice_ipc::read_server_message(&mut client).unwrap()
    else {
        panic!("expected Representation")
    };

    assert_eq!(
        before.nodes, after.nodes,
        "a hide/show cycle must not change the engine's render-pass state"
    );
    assert_eq!(
        before.generation, after.generation,
        "no frame is re-rendered just from a visibility toggle"
    );

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn disconnecting_without_shutdown_ends_the_session_cleanly() {
    let dir = temp_frame_dir("disconnect");
    let gatekeeper = unique_gatekeeper_socket_path("disconnect"); // never dialed
    let (client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        let mut generation = 0u64;
        run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper)
    });
    drop(client);
    assert!(handle.join().unwrap().is_ok());
}

#[test]
fn a_first_message_that_is_not_hello_is_rejected_and_ends_the_session() {
    let dir = temp_frame_dir("handshake-not-hello-first");
    let gatekeeper = unique_gatekeeper_socket_path("handshake-not-hello-first"); // never dialed
    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        let mut generation = 0u64;
        run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper)
    });

    blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
    let reply = blueice_ipc::read_server_message(&mut client).unwrap();
    assert!(
        matches!(reply, ServerMessage::Error { .. }),
        "expected an Error reply, got {reply:?}"
    );

    assert!(
        handle.join().unwrap().is_ok(),
        "the session must end cleanly, not hang, after rejecting the handshake"
    );
}

#[test]
fn an_unsupported_protocol_version_is_rejected_and_ends_the_session() {
    let dir = temp_frame_dir("handshake-bad-version");
    let gatekeeper = unique_gatekeeper_socket_path("handshake-bad-version"); // never dialed
    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        let mut generation = 0u64;
        run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper)
    });

    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::Hello {
            protocol_version: blueice_ipc::PROTOCOL_VERSION + 1,
        },
    )
    .unwrap();
    let reply = blueice_ipc::read_server_message(&mut client).unwrap();
    assert!(
        matches!(reply, ServerMessage::Error { .. }),
        "expected an Error reply, got {reply:?}"
    );

    assert!(
        handle.join().unwrap().is_ok(),
        "the session must end cleanly, not hang, after rejecting an unsupported version"
    );
}

#[test]
fn a_hello_seen_again_after_the_handshake_is_answered_without_ending_the_session() {
    // The broker-multiplexing scenario `run_session`'s own docs
    // describe: a second external client's handshake, forwarded
    // into the one already-past-its-own-handshake shared
    // connection, must not be treated as a protocol violation.
    let dir = temp_frame_dir("late-hello");
    let gatekeeper = clearing_gatekeeper("late-hello");
    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        let mut generation = 0u64;
        run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
        dir
    });
    handshake(&mut client);

    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::Hello {
            protocol_version: blueice_ipc::PROTOCOL_VERSION,
        },
    )
    .unwrap();
    let reply = blueice_ipc::read_server_message(&mut client).unwrap();
    assert_eq!(
        reply,
        ServerMessage::Hello {
            protocol_version: blueice_ipc::PROTOCOL_VERSION
        }
    );

    // proven the same way other no-special-effect messages are:
    // the session is still alive and answers normally afterward.
    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn every_reply_to_a_message_echoes_back_its_request_id() {
    let dir = temp_frame_dir("request-id-echo");
    let gatekeeper = clearing_gatekeeper("request-id-echo");
    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        default_page(&mut tabs).load_html_str("<p>hi</p>", None);
        let mut generation = 0u64;
        run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
        dir
    });
    handshake(&mut client);

    blueice_ipc::write_client_message_with_id(
        &mut client,
        Some(99),
        &ClientMessage::GetRepresentation,
    )
    .unwrap();
    let (request_id, reply) = blueice_ipc::read_server_message_with_id(&mut client).unwrap();
    assert_eq!(request_id, Some(99));
    assert!(matches!(reply, ServerMessage::Representation(_)));

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn an_unknown_client_variant_is_ignored_and_the_session_keeps_running() {
    let dir = temp_frame_dir("unknown-variant");
    let gatekeeper = clearing_gatekeeper("unknown-variant");
    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        let mut generation = 0u64;
        run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
        dir
    });
    handshake(&mut client);

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Unknown).unwrap();
    // proven the same way other no-reply messages are: the next
    // message still gets a normal reply, so Unknown didn't wedge
    // or end the session.
    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::Resize {
            width: 10,
            height: 10,
        },
    )
    .unwrap();
    let reply = blueice_ipc::read_server_message(&mut client).unwrap();
    assert!(matches!(reply, ServerMessage::FrameReady { .. }));

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn open_tab_creates_a_second_tab_visible_in_list_tabs() {
    let dir = temp_frame_dir("open-tab-list");
    let gatekeeper = clearing_gatekeeper("open-tab-list");
    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        let mut generation = 0u64;
        run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
        dir
    });
    handshake(&mut client);

    blueice_ipc::write_client_message(&mut client, &ClientMessage::ListTabs).unwrap();
    let ServerMessage::Tabs(before) = blueice_ipc::read_server_message(&mut client).unwrap() else {
        panic!("expected Tabs")
    };
    assert_eq!(
        before.len(),
        1,
        "a fresh core starts with exactly one tab, same as before Phase 16"
    );

    blueice_ipc::write_client_message(&mut client, &ClientMessage::OpenTab { url: None }).unwrap();
    let ServerMessage::TabOpened {
        tab_id: new_id,
        url,
        ..
    } = blueice_ipc::read_server_message(&mut client).unwrap()
    else {
        panic!("expected TabOpened")
    };
    assert_eq!(url, None);
    assert_ne!(new_id, before[0].id);

    blueice_ipc::write_client_message(&mut client, &ClientMessage::ListTabs).unwrap();
    let ServerMessage::Tabs(after) = blueice_ipc::read_server_message(&mut client).unwrap() else {
        panic!("expected Tabs")
    };
    assert_eq!(
        after.iter().map(|t| t.id).collect::<Vec<_>>(),
        vec![before[0].id, new_id]
    );

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn back_and_forward_restore_only_the_addressed_tabs_history() {
    let dir = temp_frame_dir("per-tab-history");
    let gatekeeper = clearing_gatekeeper("per-tab-history");
    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        let mut generation = 0u64;
        run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
        dir
    });
    handshake(&mut client);

    // Two visits in tab 1 create a real back stack. Built-in pages keep
    // this test deterministic while exercising the same history commit
    // path a cleared network navigation uses.
    for url in ["about:credits", "about:downloads"] {
        blueice_ipc::write_client_message(
            &mut client,
            &ClientMessage::Navigate {
                url: url.to_string(),
            },
        )
        .unwrap();
        assert_eq!(
            blueice_ipc::read_server_message(&mut client).unwrap(),
            ServerMessage::Navigated {
                url: url.to_string()
            }
        );
        assert!(matches!(
            blueice_ipc::read_server_message(&mut client).unwrap(),
            ServerMessage::FrameReady { .. }
        ));
    }

    blueice_ipc::write_client_message(&mut client, &ClientMessage::OpenTab { url: None }).unwrap();
    let ServerMessage::TabOpened {
        tab_id: tab_two, ..
    } = blueice_ipc::read_server_message(&mut client).unwrap()
    else {
        panic!("expected TabOpened")
    };
    blueice_ipc::write_client_message_with_ids(
        &mut client,
        Some(tab_two),
        None,
        &ClientMessage::Navigate {
            url: "about:blank".to_string(),
        },
    )
    .unwrap();
    let (reply_tab, _, navigated) = blueice_ipc::read_server_message_with_ids(&mut client).unwrap();
    assert_eq!(reply_tab, Some(tab_two));
    assert_eq!(
        navigated,
        ServerMessage::Navigated {
            url: "about:blank".to_string()
        }
    );
    assert!(matches!(
        blueice_ipc::read_server_message_with_ids(&mut client)
            .unwrap()
            .2,
        ServerMessage::FrameReady { .. }
    ));

    // Going back in tab 1 must leave tab 2's distinct visit untouched.
    blueice_ipc::write_client_message(&mut client, &ClientMessage::GoBack).unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::Navigated {
            url: "about:credits".to_string()
        }
    );
    assert!(matches!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::FrameReady { .. }
    ));

    blueice_ipc::write_client_message_with_ids(
        &mut client,
        Some(tab_two),
        None,
        &ClientMessage::GetHistoryState,
    )
    .unwrap();
    let (reply_tab, _, state) = blueice_ipc::read_server_message_with_ids(&mut client).unwrap();
    assert_eq!(reply_tab, Some(tab_two));
    assert_eq!(
        state,
        ServerMessage::HistoryState {
            can_go_back: true,
            can_go_forward: false,
        },
        "tab 1's Back must not alter tab 2's history position"
    );

    blueice_ipc::write_client_message(&mut client, &ClientMessage::GetHistoryState).unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::HistoryState {
            can_go_back: true,
            can_go_forward: true,
        }
    );

    blueice_ipc::write_client_message(&mut client, &ClientMessage::GoForward).unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::Navigated {
            url: "about:downloads".to_string()
        }
    );
    assert!(matches!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::FrameReady { .. }
    ));

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn default_history_reload_fetches_the_url_again_and_uses_fresh_content() {
    let dir = temp_frame_dir("history-reload");
    let gatekeeper = clearing_gatekeeper("history-reload");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let http = thread::spawn(move || {
        for body in ["first version", "updated version"] {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0u8; 1024];
            let _ = std::io::Read::read(&mut stream, &mut request);
            let body = format!("<button>{body}</button>");
            std::io::Write::write_all(
                &mut stream,
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            )
            .unwrap();
        }
    });

    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        let mut generation = 0u64;
        run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
        dir
    });
    handshake(&mut client);

    let url = format!("http://{addr}");
    blueice_ipc::write_client_message(&mut client, &ClientMessage::Navigate { url: url.clone() })
        .unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::Navigated { url: url.clone() }
    );
    assert!(matches!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::FrameReady { .. }
    ));

    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::Navigate {
            url: "about:credits".to_string(),
        },
    )
    .unwrap();
    let _ = blueice_ipc::read_server_message(&mut client).unwrap();
    let _ = blueice_ipc::read_server_message(&mut client).unwrap();

    blueice_ipc::write_client_message(&mut client, &ClientMessage::GoBack).unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::Navigated { url: url.clone() }
    );
    assert!(matches!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::FrameReady { .. }
    ));
    blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
    let ServerMessage::Representation(snapshot) =
        blueice_ipc::read_server_message(&mut client).unwrap()
    else {
        panic!("expected a representation after history reload")
    };
    assert!(
        snapshot
            .nodes
            .iter()
            .any(|node| node.name.as_deref() == Some("updated version")),
        "Back must fetch the URL again instead of displaying the first visit's in-memory page"
    );

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    http.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn default_history_reload_rechecks_the_live_gatekeeper_before_a_second_fetch() {
    use blueice_ipc::gatekeeper::GatekeeperSettingsChange;

    let dir = temp_frame_dir("history-live-policy");
    let gatekeeper = unique_gatekeeper_socket_path("history-live-policy");
    let _ = std::fs::remove_file(&gatekeeper);
    let listener = UnixListener::bind(&gatekeeper).unwrap();
    let service = Arc::new(blueice_ai_gatekeeper::GatekeeperService::new(None).unwrap());
    let reviewer = thread::spawn({
        let service = Arc::clone(&service);
        move || {
            // First visit: URL + content. Then one settings update. The
            // history reload must ask the same live service about its URL.
            for _ in 0..4 {
                let (mut stream, _) = listener.accept().unwrap();
                service.handle_connection(&mut stream).unwrap();
            }
        }
    });

    let http_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = http_listener.local_addr().unwrap();
    let http_peer = http_listener.try_clone().unwrap();
    let http = thread::spawn(move || {
        let (mut stream, _) = http_peer.accept().unwrap();
        let mut request = [0u8; 1024];
        let _ = std::io::Read::read(&mut stream, &mut request);
        let body = "<p>original page</p>";
        std::io::Write::write_all(
            &mut stream,
            format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .as_bytes(),
        )
        .unwrap();
    });

    let (mut client, mut server) = client_pair();
    let session_gatekeeper = gatekeeper.clone();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        let mut generation = 0u64;
        run_session(
            &mut tabs,
            &mut server,
            &dir,
            &mut generation,
            &session_gatekeeper,
        )
        .unwrap();
        dir
    });
    handshake(&mut client);
    let url = format!("http://{address}/history");
    blueice_ipc::write_client_message(&mut client, &ClientMessage::Navigate { url: url.clone() })
        .unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::Navigated { url: url.clone() }
    );
    assert!(matches!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::FrameReady { .. }
    ));
    http.join().unwrap();
    http_listener.set_nonblocking(true).unwrap();

    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::Navigate {
            url: "about:credits".into(),
        },
    )
    .unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::Navigated {
            url: "about:credits".into()
        }
    );
    assert!(matches!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::FrameReady { .. }
    ));

    let source = crate::gatekeeper_settings_page::GatekeeperSettingsSource::at(gatekeeper.clone());
    let effective = source
        .update(GatekeeperSettingsChange::AddBlockedHost {
            host: "127.0.0.1".into(),
        })
        .unwrap();
    assert_eq!(effective.custom_blocked_hosts, ["127.0.0.1"]);
    blueice_ipc::write_client_message(&mut client, &ClientMessage::GoBack).unwrap();
    assert!(
        matches!(blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::GatekeeperBlocked { url: blocked_url, category, .. }
            if blocked_url == url && category == "custom-blocked-domain")
    );
    assert_eq!(
        http_listener.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock,
        "history reload must be blocked before a second network connection"
    );

    blueice_ipc::write_client_message(&mut client, &ClientMessage::ListTabs).unwrap();
    let ServerMessage::Tabs(tabs) = blueice_ipc::read_server_message(&mut client).unwrap() else {
        panic!("expected tab state after the blocked history reload")
    };
    assert_eq!(
        tabs[0].url.as_deref(),
        Some("about:credits"),
        "a rejected reload must keep the current document visible"
    );
    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    reviewer.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
    let _ = std::fs::remove_file(gatekeeper);
}

#[test]
fn opted_in_history_snapshot_restores_when_the_original_url_is_unavailable() {
    let dir = temp_frame_dir("history-snapshot");
    let gatekeeper = clearing_gatekeeper("history-snapshot");
    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new_with_history_snapshot_mode(
            320.0,
            200.0,
            crate::HistorySnapshotMode::Snapshot,
        );
        let tab = tabs.default_tab();
        tabs.get_mut(tab).unwrap().load_html_str(
            "<button>saved historical version</button>",
            Some("https://unavailable.example.test/archive-me".to_string()),
        );
        let mut generation = 0u64;
        run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
        dir
    });
    handshake(&mut client);

    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::Navigate {
            url: "about:credits".to_string(),
        },
    )
    .unwrap();
    let _ = blueice_ipc::read_server_message(&mut client).unwrap();
    let _ = blueice_ipc::read_server_message(&mut client).unwrap();

    blueice_ipc::write_client_message(&mut client, &ClientMessage::GoBack).unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::Navigated {
            url: "https://unavailable.example.test/archive-me".to_string()
        }
    );
    assert!(matches!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::FrameReady { .. }
    ));
    blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
    let ServerMessage::Representation(snapshot) =
        blueice_ipc::read_server_message(&mut client).unwrap()
    else {
        panic!("expected a representation after snapshot restoration")
    };
    assert!(snapshot
        .nodes
        .iter()
        .any(|node| node.name.as_deref() == Some("saved historical version")));

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn failed_default_history_reload_keeps_the_current_page_and_cursor() {
    let dir = temp_frame_dir("history-reload-failure");
    let gatekeeper = clearing_gatekeeper("history-reload-failure");
    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        let tab = tabs.default_tab();
        tabs.get_mut(tab).unwrap().load_html_str(
            "<button>unavailable historical page</button>",
            Some("http://127.0.0.1:1/history-unavailable".to_string()),
        );
        let mut generation = 0u64;
        run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
        dir
    });
    handshake(&mut client);

    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::Navigate {
            url: "about:credits".to_string(),
        },
    )
    .unwrap();
    let _ = blueice_ipc::read_server_message(&mut client).unwrap();
    let _ = blueice_ipc::read_server_message(&mut client).unwrap();

    blueice_ipc::write_client_message(&mut client, &ClientMessage::GoBack).unwrap();
    assert!(matches!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::Error { .. }
    ));
    blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
    let ServerMessage::Representation(snapshot) =
        blueice_ipc::read_server_message(&mut client).unwrap()
    else {
        panic!("expected the still-current page representation")
    };
    assert_eq!(snapshot.url.as_deref(), Some("about:credits"));

    blueice_ipc::write_client_message(&mut client, &ClientMessage::GetHistoryState).unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::HistoryState {
            can_go_back: true,
            can_go_forward: false,
        },
        "a failed reload must not advance the history cursor"
    );

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn tab_groups_are_shared_session_state_and_closing_one_ungroups_its_tabs() {
    let dir = temp_frame_dir("tab-groups");
    let gatekeeper = clearing_gatekeeper("tab-groups");
    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        let mut generation = 0u64;
        run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
        dir
    });
    handshake(&mut client);

    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::CreateTabGroup {
            name: "  Research  ".to_string(),
            color: "#4F8cFf".to_string(),
        },
    )
    .unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::TabGroupCreated(TabGroupSummary {
            id: 1,
            name: "Research".to_string(),
            color: "#4f8cff".to_string(),
            collapsed: false,
        })
    );

    blueice_ipc::write_client_message_with_ids(
        &mut client,
        Some(1),
        None,
        &ClientMessage::SetTabGroup { group_id: Some(1) },
    )
    .unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::TabGroupAssigned {
            tab_id: 1,
            group_id: Some(1),
        }
    );

    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::RenameTabGroup {
            group_id: 1,
            name: "Reference".to_string(),
        },
    )
    .unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::TabGroupUpdated(TabGroupSummary {
            id: 1,
            name: "Reference".to_string(),
            color: "#4f8cff".to_string(),
            collapsed: false,
        })
    );

    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::SetTabGroupColor {
            group_id: 1,
            color: "#ff6600".to_string(),
        },
    )
    .unwrap();
    let recolored = blueice_ipc::read_server_message(&mut client).unwrap();
    assert!(matches!(
        recolored,
        ServerMessage::TabGroupUpdated(TabGroupSummary { ref color, .. }) if color == "#ff6600"
    ));

    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::SetTabGroupCollapsed {
            group_id: 1,
            collapsed: true,
        },
    )
    .unwrap();
    let collapsed = blueice_ipc::read_server_message(&mut client).unwrap();
    assert!(matches!(
        collapsed,
        ServerMessage::TabGroupUpdated(TabGroupSummary {
            collapsed: true,
            ..
        })
    ));

    blueice_ipc::write_client_message(&mut client, &ClientMessage::ListTabs).unwrap();
    assert!(matches!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::Tabs(ref tabs) if tabs == &vec![TabSummary {
            id: 1,
            url: None,
            group_id: Some(1),
        }]
    ));
    blueice_ipc::write_client_message(&mut client, &ClientMessage::ListTabGroups).unwrap();
    assert!(matches!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::TabGroups(ref groups) if groups.len() == 1 && groups[0].collapsed
    ));

    blueice_ipc::write_client_message(&mut client, &ClientMessage::CloseTabGroup { group_id: 1 })
        .unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::TabGroupClosed { group_id: 1 }
    );
    blueice_ipc::write_client_message(&mut client, &ClientMessage::ListTabs).unwrap();
    assert!(matches!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::Tabs(ref tabs) if tabs[0].group_id.is_none()
    ));

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn resize_eagerly_reflows_background_tabs_without_creating_an_active_tab() {
    let dir = temp_frame_dir("resize-background-tabs");
    let gatekeeper = clearing_gatekeeper("resize-background-tabs");
    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        let mut generation = 0u64;
        run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
        dir
    });
    handshake(&mut client);

    blueice_ipc::write_client_message(&mut client, &ClientMessage::OpenTab { url: None }).unwrap();
    assert!(matches!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::TabOpened { tab_id: 2, .. }
    ));
    blueice_ipc::write_client_message_with_ids(
        &mut client,
        Some(1),
        None,
        &ClientMessage::Resize {
            width: 640,
            height: 480,
        },
    )
    .unwrap();
    let first = blueice_ipc::read_server_message_with_ids(&mut client).unwrap();
    let second = blueice_ipc::read_server_message_with_ids(&mut client).unwrap();
    let mut resized = [first, second];
    resized.sort_by_key(|(tab_id, _, _)| *tab_id);
    assert!(matches!(
        &resized[..],
        [
            (Some(1), _, ServerMessage::FrameReady { .. }),
            (Some(2), _, ServerMessage::FrameReady { .. }),
        ]
    ));

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn open_tab_with_a_url_navigates_it_and_sends_a_frame() {
    let dir = temp_frame_dir("open-tab-with-url");
    let gatekeeper = clearing_gatekeeper("open-tab-with-url");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = [0u8; 1024];
        let _ = std::io::Read::read(&mut stream, &mut buf);
        let body = "<p>opened via url</p>";
        std::io::Write::write_all(
            &mut stream,
            format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .as_bytes(),
        )
        .unwrap();
    });

    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        let mut generation = 0u64;
        run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
        dir
    });
    handshake(&mut client);

    let url = format!("http://{addr}");
    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::OpenTab {
            url: Some(url.clone()),
        },
    )
    .unwrap();
    let ServerMessage::TabOpened {
        tab_id: new_id,
        url: opened_url,
        ..
    } = blueice_ipc::read_server_message(&mut client).unwrap()
    else {
        panic!("expected TabOpened")
    };
    assert_eq!(opened_url, Some(url));
    let frame = blueice_ipc::read_server_message(&mut client).unwrap();
    assert!(
        matches!(frame, ServerMessage::FrameReady { .. }),
        "expected FrameReady, got {frame:?}"
    );

    // The new tab's content must actually be addressable afterward.
    blueice_ipc::write_client_message_with_ids(
        &mut client,
        Some(new_id),
        None,
        &ClientMessage::GetRepresentation,
    )
    .unwrap();
    let (reply_tab, _, reply) = blueice_ipc::read_server_message_with_ids(&mut client).unwrap();
    assert_eq!(reply_tab, Some(new_id));
    let ServerMessage::Representation(snapshot) = reply else {
        panic!("expected Representation, got {reply:?}")
    };
    assert_eq!(snapshot.tab_id, new_id);
    assert!(snapshot
        .nodes
        .iter()
        .any(|n| n.name.as_deref() == Some("opened via url")));

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn open_tab_with_a_failing_url_replies_error_not_tab_opened() {
    let dir = temp_frame_dir("open-tab-failing-url");
    let gatekeeper = clearing_gatekeeper("open-tab-failing-url");
    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        let mut generation = 0u64;
        run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
        dir
    });
    handshake(&mut client);

    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::OpenTab {
            url: Some("not-a-valid-url".to_string()),
        },
    )
    .unwrap();
    let reply = blueice_ipc::read_server_message(&mut client).unwrap();
    assert!(
        matches!(reply, ServerMessage::Error { .. }),
        "expected Error, got {reply:?}"
    );

    // The session must still be alive and taking new commands
    // afterward -- proven the same way every other no-crash case
    // in this file is.
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
fn an_action_addressed_to_one_tab_never_affects_another_tabs_state() {
    let dir = temp_frame_dir("tab-isolation");
    let gatekeeper = clearing_gatekeeper("tab-isolation");
    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        default_page(&mut tabs).load_html_str("<p>tab one</p>", None);
        let mut generation = 0u64;
        run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
        dir
    });
    handshake(&mut client);

    blueice_ipc::write_client_message(&mut client, &ClientMessage::ListTabs).unwrap();
    let ServerMessage::Tabs(initial) = blueice_ipc::read_server_message(&mut client).unwrap()
    else {
        panic!("expected Tabs")
    };
    let tab_one = initial[0].id;

    blueice_ipc::write_client_message(&mut client, &ClientMessage::OpenTab { url: None }).unwrap();
    let ServerMessage::TabOpened {
        tab_id: tab_two, ..
    } = blueice_ipc::read_server_message(&mut client).unwrap()
    else {
        panic!("expected TabOpened")
    };

    // Scroll only tab_two.
    blueice_ipc::write_client_message_with_ids(
        &mut client,
        Some(tab_two),
        None,
        &ClientMessage::Scroll { delta_y: 500.0 },
    )
    .unwrap();
    let frame = blueice_ipc::read_server_message(&mut client).unwrap();
    assert!(matches!(frame, ServerMessage::FrameReady { .. }));

    // tab_one's representation must be completely unaffected --
    // still showing its own content, scroll untouched.
    blueice_ipc::write_client_message_with_ids(
        &mut client,
        Some(tab_one),
        None,
        &ClientMessage::GetRepresentation,
    )
    .unwrap();
    let (_, _, reply) = blueice_ipc::read_server_message_with_ids(&mut client).unwrap();
    let ServerMessage::Representation(snap) = reply else {
        panic!("expected Representation, got {reply:?}")
    };
    assert_eq!(snap.tab_id, tab_one);
    assert_eq!(
        snap.scroll_y, 0.0,
        "scrolling tab_two must not move tab_one's scroll position"
    );
    assert!(snap
        .nodes
        .iter()
        .any(|n| n.name.as_deref() == Some("tab one")));

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_gated_navigation_addressed_to_one_tab_never_affects_another_tabs_state() {
    // Extends `an_action_addressed_to_one_tab_never_affects_another_
    // tabs_state` (which only covers `Scroll`) to a gated `Navigate`
    // specifically, now that navigation is asynchronous: `tab_two`
    // fully navigating must leave `tab_one`'s content, generation
    // relationship, and addressability completely untouched.
    let dir = temp_frame_dir("tab-isolation-gated-navigate");
    let gatekeeper = clearing_gatekeeper("tab-isolation-gated-navigate");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = [0u8; 1024];
        let _ = std::io::Read::read(&mut stream, &mut buf);
        let body = "<p>tab two content</p>";
        std::io::Write::write_all(
            &mut stream,
            format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .as_bytes(),
        )
        .unwrap();
    });

    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        default_page(&mut tabs).load_html_str("<p>tab one</p>", None);
        let mut generation = 0u64;
        run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
        dir
    });
    handshake(&mut client);

    blueice_ipc::write_client_message(&mut client, &ClientMessage::ListTabs).unwrap();
    let ServerMessage::Tabs(initial) = blueice_ipc::read_server_message(&mut client).unwrap()
    else {
        panic!("expected Tabs")
    };
    let tab_one = initial[0].id;

    blueice_ipc::write_client_message(&mut client, &ClientMessage::OpenTab { url: None }).unwrap();
    let ServerMessage::TabOpened {
        tab_id: tab_two, ..
    } = blueice_ipc::read_server_message(&mut client).unwrap()
    else {
        panic!("expected TabOpened")
    };

    let url = format!("http://{addr}");
    blueice_ipc::write_client_message_with_ids(
        &mut client,
        Some(tab_two),
        None,
        &ClientMessage::Navigate { url: url.clone() },
    )
    .unwrap();
    let (reply_tab, _, navigated) = blueice_ipc::read_server_message_with_ids(&mut client).unwrap();
    assert_eq!(reply_tab, Some(tab_two));
    assert_eq!(navigated, ServerMessage::Navigated { url });
    let (_, _, frame) = blueice_ipc::read_server_message_with_ids(&mut client).unwrap();
    assert!(matches!(frame, ServerMessage::FrameReady { .. }));

    blueice_ipc::write_client_message_with_ids(
        &mut client,
        Some(tab_one),
        None,
        &ClientMessage::GetRepresentation,
    )
    .unwrap();
    let (_, _, reply) = blueice_ipc::read_server_message_with_ids(&mut client).unwrap();
    let ServerMessage::Representation(snap) = reply else {
        panic!("expected Representation, got {reply:?}")
    };
    assert_eq!(snap.tab_id, tab_one);
    assert!(
        snap.nodes
            .iter()
            .any(|n| n.name.as_deref() == Some("tab one")),
        "tab one's content must be untouched by tab two's gated navigation"
    );

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_message_addressed_to_an_unknown_tab_replies_error_not_a_silent_no_op() {
    let dir = temp_frame_dir("unknown-tab-error");
    let gatekeeper = clearing_gatekeeper("unknown-tab-error");
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
        Some(999_999),
        None,
        &ClientMessage::GetRepresentation,
    )
    .unwrap();
    let (reply_tab, _, reply) = blueice_ipc::read_server_message_with_ids(&mut client).unwrap();
    assert_eq!(
        reply_tab,
        Some(999_999),
        "the reply should still echo back which (nonexistent) tab was addressed"
    );
    assert!(
        matches!(reply, ServerMessage::Error { .. }),
        "expected Error, got {reply:?}"
    );

    // The session must survive an unknown-tab error, same as every
    // other error case in this file.
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
fn close_tab_removes_it_and_a_later_message_to_it_becomes_an_error() {
    let dir = temp_frame_dir("close-tab");
    let gatekeeper = clearing_gatekeeper("close-tab");
    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        let mut generation = 0u64;
        run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
        dir
    });
    handshake(&mut client);

    blueice_ipc::write_client_message(&mut client, &ClientMessage::OpenTab { url: None }).unwrap();
    let ServerMessage::TabOpened { tab_id: new_id, .. } =
        blueice_ipc::read_server_message(&mut client).unwrap()
    else {
        panic!("expected TabOpened")
    };

    blueice_ipc::write_client_message_with_ids(
        &mut client,
        Some(new_id),
        None,
        &ClientMessage::CloseTab,
    )
    .unwrap();
    let (reply_tab, _, reply) = blueice_ipc::read_server_message_with_ids(&mut client).unwrap();
    assert_eq!(reply_tab, Some(new_id));
    assert_eq!(reply, ServerMessage::TabClosed { tab_id: new_id });

    blueice_ipc::write_client_message_with_ids(
        &mut client,
        Some(new_id),
        None,
        &ClientMessage::GetRepresentation,
    )
    .unwrap();
    let (_, _, reply) = blueice_ipc::read_server_message_with_ids(&mut client).unwrap();
    assert!(
        matches!(reply, ServerMessage::Error { .. }),
        "a closed tab's id must no longer resolve, expected Error, got {reply:?}"
    );

    // Closing again is a harmless-but-reported "unknown tab" error,
    // not a panic or a second TabClosed.
    blueice_ipc::write_client_message_with_ids(
        &mut client,
        Some(new_id),
        None,
        &ClientMessage::CloseTab,
    )
    .unwrap();
    let (_, _, reply) = blueice_ipc::read_server_message_with_ids(&mut client).unwrap();
    assert!(matches!(reply, ServerMessage::Error { .. }));

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_reply_to_an_untagged_request_still_echoes_the_resolved_default_tab_id() {
    // The load-bearing property that makes broadcast-shared,
    // multi-tab connections work at all: a request that left
    // `tab_id` implicit still gets a reply that self-discloses the
    // *concrete* tab it resolved to, not `None` -- otherwise a
    // second client sharing the connection via `blueice-launcher`'s
    // broker could never tell which tab an untagged client's
    // broadcasted reply was actually about.
    let dir = temp_frame_dir("echo-resolved-default-tab");
    let gatekeeper = clearing_gatekeeper("echo-resolved-default-tab");
    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        let mut generation = 0u64;
        run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
        dir
    });
    handshake(&mut client);

    blueice_ipc::write_client_message(&mut client, &ClientMessage::ListTabs).unwrap();
    let ServerMessage::Tabs(tabs) = blueice_ipc::read_server_message(&mut client).unwrap() else {
        panic!("expected Tabs")
    };
    let default_tab_id = tabs[0].id;

    // Sent with no tab_id at all -- the envelope-level default.
    blueice_ipc::write_client_message_with_id(&mut client, None, &ClientMessage::GetRepresentation)
        .unwrap();
    let (reply_tab, _, reply) = blueice_ipc::read_server_message_with_ids(&mut client).unwrap();
    assert_eq!(
        reply_tab,
        Some(default_tab_id),
        "the reply must echo the resolved tab, not None"
    );
    assert!(matches!(reply, ServerMessage::Representation(_)));

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

// -- Gatekeeper-specific behavior --------------------------------

#[test]
fn content_stage_rejection_blocks_navigation_and_leaves_the_page_unchanged() {
    let dir = temp_frame_dir("content-stage-block");
    let gatekeeper_path = unique_gatekeeper_socket_path("content-stage-block");
    let _ = std::fs::remove_file(&gatekeeper_path);
    let listener = UnixListener::bind(&gatekeeper_path).unwrap();
    thread::spawn(move || {
        for incoming in listener.incoming() {
            let Ok(mut stream) = incoming else { break };
            let Ok(req) = blueice_ipc::gatekeeper::read_gatekeeper_request(&mut stream) else {
                continue;
            };
            let reply = match req {
                blueice_ipc::gatekeeper::GatekeeperRequest::CheckUrl { .. } => {
                    blueice_ipc::gatekeeper::GatekeeperReply::Cleared
                }
                blueice_ipc::gatekeeper::GatekeeperRequest::CheckContent { .. } => {
                    blueice_ipc::gatekeeper::GatekeeperReply::Rejected {
                        reason: "hidden instruction-shaped text".to_string(),
                        category: "prompt-injection".to_string(),
                    }
                }
                blueice_ipc::gatekeeper::GatekeeperRequest::CheckDownload { .. } => {
                    unreachable!(
                        "navigation never sends a download check; that stage belongs to the downloads process"
                    )
                }
                blueice_ipc::gatekeeper::GatekeeperRequest::CheckExtensionAction { .. } => {
                    unreachable!(
                        "navigation never sends an extension action check; that stage belongs to the extension host"
                    )
                }
            };
            let _ = blueice_ipc::gatekeeper::write_gatekeeper_reply(&mut stream, &reply);
        }
    });

    let http = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = http.local_addr().unwrap();
    thread::spawn(move || {
        let (mut stream, _) = http.accept().unwrap();
        let mut buf = [0u8; 1024];
        let _ = std::io::Read::read(&mut stream, &mut buf);
        let body = "<p>malicious page</p>";
        std::io::Write::write_all(
            &mut stream,
            format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .as_bytes(),
        )
        .unwrap();
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

    let url = format!("http://{addr}");
    blueice_ipc::write_client_message(&mut client, &ClientMessage::Navigate { url: url.clone() })
        .unwrap();
    let reply = blueice_ipc::read_server_message(&mut client).unwrap();
    assert_eq!(
        reply,
        ServerMessage::GatekeeperBlocked {
            reason: "hidden instruction-shaped text".to_string(),
            category: "prompt-injection".to_string(),
            url: url.clone()
        }
    );

    // The page must not have changed: a follow-up GetRepresentation
    // shows no trace of the blocked page's content (no `FrameReady`
    // was ever produced for it either, since the only reply so far
    // was the GatekeeperBlocked above).
    blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
    let ServerMessage::Representation(snap) =
        blueice_ipc::read_server_message(&mut client).unwrap()
    else {
        panic!("expected Representation")
    };
    assert!(!snap
        .nodes
        .iter()
        .any(|n| n.name.as_deref() == Some("malicious page")));

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn navigation_fails_closed_when_the_gatekeeper_is_unreachable() {
    let dir = temp_frame_dir("gatekeeper-unreachable");
    let gatekeeper_path = unique_gatekeeper_socket_path("gatekeeper-unreachable"); // nothing listens here
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

    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::Navigate {
            url: "http://example.invalid/".to_string(),
        },
    )
    .unwrap();
    let reply = blueice_ipc::read_server_message(&mut client).unwrap();
    assert!(
        matches!(reply, ServerMessage::GatekeeperBlocked { .. }),
        "an unreachable gatekeeper must fail closed, got {reply:?}"
    );

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn navigation_fails_closed_when_the_gatekeeper_accepts_then_drops_the_connection() {
    let dir = temp_frame_dir("gatekeeper-drops-connection");
    let gatekeeper_path = unique_gatekeeper_socket_path("gatekeeper-drops-connection");
    let _ = std::fs::remove_file(&gatekeeper_path);
    let listener = UnixListener::bind(&gatekeeper_path).unwrap();
    thread::spawn(move || {
        for incoming in listener.incoming() {
            drop(incoming); // accept, then immediately disconnect -- no reply ever sent
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

    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::Navigate {
            url: "http://example.invalid/".to_string(),
        },
    )
    .unwrap();
    let reply = blueice_ipc::read_server_message(&mut client).unwrap();
    assert!(
        matches!(reply, ServerMessage::GatekeeperBlocked { .. }),
        "a gatekeeper that drops the connection must fail closed, got {reply:?}"
    );

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_stalled_gatekeeper_check_for_one_tab_does_not_block_a_reply_to_another_tab() {
    // The single most important proof of the property this whole
    // mechanism exists for: a slow/stuck gatekeeper review for one
    // tab must never stall the one shared connection other tabs
    // (or clients sharing it via `blueice-launcher`'s broker) are
    // also using.
    let dir = temp_frame_dir("non-blocking-concurrency");
    let gatekeeper_path = unique_gatekeeper_socket_path("non-blocking-concurrency");
    let _ = std::fs::remove_file(&gatekeeper_path);
    let listener = UnixListener::bind(&gatekeeper_path).unwrap();
    thread::spawn(move || {
        for incoming in listener.incoming() {
            let Ok(mut stream) = incoming else { break };
            thread::spawn(move || {
                if let Ok(req) = blueice_ipc::gatekeeper::read_gatekeeper_request(&mut stream) {
                    if matches!(&req, blueice_ipc::gatekeeper::GatekeeperRequest::CheckUrl { url } if url.contains("slow-tab"))
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
    let ServerMessage::TabOpened { tab_id: tab_b, .. } =
        blueice_ipc::read_server_message(&mut client).unwrap()
    else {
        panic!("expected TabOpened")
    };

    // Kick off the default tab's navigation, whose gatekeeper check
    // stalls for 300ms -- fire-and-forget, its own reply isn't
    // waited on here.
    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::Navigate {
            url: "http://127.0.0.1:1/slow-tab".to_string(),
        },
    )
    .unwrap();

    // Immediately address tab_b with an unrelated message.
    let start = Instant::now();
    blueice_ipc::write_client_message_with_ids(
        &mut client,
        Some(tab_b),
        None,
        &ClientMessage::GetRepresentation,
    )
    .unwrap();
    let (reply_tab, _, reply) = blueice_ipc::read_server_message_with_ids(&mut client).unwrap();
    assert_eq!(reply_tab, Some(tab_b));
    assert!(matches!(reply, ServerMessage::Representation(_)));
    assert!(
        start.elapsed() < Duration::from_millis(150),
        "tab_b's reply must arrive well before tab_a's stalled gatekeeper check resolves, took {:?}",
        start.elapsed()
    );

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_second_navigation_supersedes_a_still_pending_first_one() {
    let dir = temp_frame_dir("supersede");
    let gatekeeper_path = unique_gatekeeper_socket_path("supersede");
    let _ = std::fs::remove_file(&gatekeeper_path);
    let listener = UnixListener::bind(&gatekeeper_path).unwrap();
    thread::spawn(move || {
        for incoming in listener.incoming() {
            let Ok(mut stream) = incoming else { break };
            thread::spawn(move || {
                if let Ok(req) = blueice_ipc::gatekeeper::read_gatekeeper_request(&mut stream) {
                    if matches!(&req, blueice_ipc::gatekeeper::GatekeeperRequest::CheckUrl { url } if url.contains("first"))
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

    let second_http = TcpListener::bind("127.0.0.1:0").unwrap();
    let second_addr = second_http.local_addr().unwrap();
    thread::spawn(move || {
        let (mut stream, _) = second_http.accept().unwrap();
        let mut buf = [0u8; 1024];
        let _ = std::io::Read::read(&mut stream, &mut buf);
        let body = "<p>second page</p>";
        std::io::Write::write_all(
            &mut stream,
            format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .as_bytes(),
        )
        .unwrap();
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

    // First navigation: stalls 300ms on its own CheckUrl stage, and
    // even once cleared points nowhere reachable -- must never
    // become visible.
    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::Navigate {
            url: "http://127.0.0.1:1/first-slow".to_string(),
        },
    )
    .unwrap();
    // Second navigation to the same (default) tab, sent immediately
    // after, well before the first's gatekeeper check resolves.
    let second_url = format!("http://{second_addr}");
    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::Navigate {
            url: second_url.clone(),
        },
    )
    .unwrap();

    let navigated = blueice_ipc::read_server_message(&mut client).unwrap();
    assert_eq!(navigated, ServerMessage::Navigated { url: second_url });
    let frame = blueice_ipc::read_server_message(&mut client).unwrap();
    assert!(matches!(frame, ServerMessage::FrameReady { .. }));

    // No further reply ever arrives for the stale first navigation,
    // even after waiting past its stall -- proven the same way
    // every other "harmless no-op" case in this file is: the next
    // real message still gets exactly one, normal reply.
    thread::sleep(Duration::from_millis(400));
    blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
    let ServerMessage::Representation(snap) =
        blueice_ipc::read_server_message(&mut client).unwrap()
    else {
        panic!("expected Representation")
    };
    assert!(snap
        .nodes
        .iter()
        .any(|n| n.name.as_deref() == Some("second page")));

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn open_tab_with_a_url_the_gatekeeper_blocks_replies_gatekeeper_blocked_not_tab_opened() {
    // `OpenTab{url: Some(_)}` goes through the same gated path
    // `Navigate` does (`PendingKind::OpenTab`) -- this is the
    // `OpenTab`-specific proof that a blocked outcome there reports
    // `GatekeeperBlocked`, not a bare `TabOpened`/`Error`, and that
    // no orphaned-but-blank tab id is leaked into a reply shape a
    // caller wouldn't expect.
    let dir = temp_frame_dir("open-tab-gatekeeper-blocked");
    let gatekeeper_path = unique_gatekeeper_socket_path("open-tab-gatekeeper-blocked");
    let _ = std::fs::remove_file(&gatekeeper_path);
    let listener = UnixListener::bind(&gatekeeper_path).unwrap();
    thread::spawn(move || {
        for incoming in listener.incoming() {
            let Ok(mut stream) = incoming else { break };
            let Ok(_req) = blueice_ipc::gatekeeper::read_gatekeeper_request(&mut stream) else {
                continue;
            };
            let _ = blueice_ipc::gatekeeper::write_gatekeeper_reply(
                &mut stream,
                &blueice_ipc::gatekeeper::GatekeeperReply::Rejected {
                    reason: "known-bad domain".to_string(),
                    category: "blocklist".to_string(),
                },
            );
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

    let url = "http://example.invalid/".to_string();
    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::OpenTab {
            url: Some(url.clone()),
        },
    )
    .unwrap();
    let reply = blueice_ipc::read_server_message(&mut client).unwrap();
    assert_eq!(
        reply,
        ServerMessage::GatekeeperBlocked {
            reason: "known-bad domain".to_string(),
            category: "blocklist".to_string(),
            url
        }
    );

    // The session must still be alive afterward, same as every
    // other error/blocked case in this file -- and `ListTabs` must
    // still show the new (blank) tab `OpenTab` always creates,
    // per `ServerMessage::TabOpened`'s own documented limitation.
    blueice_ipc::write_client_message(&mut client, &ClientMessage::ListTabs).unwrap();
    let ServerMessage::Tabs(tabs) = blueice_ipc::read_server_message(&mut client).unwrap() else {
        panic!("expected Tabs")
    };
    assert_eq!(
        tabs.len(),
        2,
        "OpenTab always creates the tab, even though its requested navigation was blocked"
    );

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_non_navigating_message_to_a_tab_with_a_pending_navigation_applies_immediately() {
    // `phase-7-local-ai/PLAN.md`'s "Wiring design" is explicit that
    // this must work the way a real browser reflows/scrolls a
    // still-displayed old page while a new one loads: `Resize`
    // addressed to a tab whose gated navigation hasn't resolved yet
    // must apply immediately against that tab's *current*
    // (pre-navigation) `Page` state, not queue up behind it.
    let dir = temp_frame_dir("resize-during-pending-nav");
    let gatekeeper_path = unique_gatekeeper_socket_path("resize-during-pending-nav");
    let _ = std::fs::remove_file(&gatekeeper_path);
    let listener = UnixListener::bind(&gatekeeper_path).unwrap();
    thread::spawn(move || {
        for incoming in listener.incoming() {
            let Ok(mut stream) = incoming else { break };
            thread::spawn(move || {
                if let Ok(_req) = blueice_ipc::gatekeeper::read_gatekeeper_request(&mut stream) {
                    // Stalls every stage, so the navigation this
                    // test kicks off never resolves within the
                    // test's own lifetime -- the point is proving
                    // `Resize` doesn't wait on it at all.
                    thread::sleep(Duration::from_secs(5));
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
        default_page(&mut tabs).load_html_str("<p>still the old page</p>", None);
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

    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::Navigate {
            url: "http://127.0.0.1:1/never-resolves".to_string(),
        },
    )
    .unwrap();

    let start = Instant::now();
    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::Resize {
            width: 111,
            height: 222,
        },
    )
    .unwrap();
    let reply = blueice_ipc::read_server_message(&mut client).unwrap();
    assert!(
        matches!(
            reply,
            ServerMessage::FrameReady {
                width: 111,
                height: 222,
                ..
            }
        ),
        "expected an immediate FrameReady for the resize, got {reply:?}"
    );
    assert!(
        start.elapsed() < Duration::from_millis(500),
        "Resize must apply immediately, not wait behind the pending navigation, took {:?}",
        start.elapsed()
    );

    // The old page's content is still what's shown -- the pending
    // navigation never actually applied.
    blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
    let ServerMessage::Representation(snap) =
        blueice_ipc::read_server_message(&mut client).unwrap()
    else {
        panic!("expected Representation")
    };
    assert!(snap
        .nodes
        .iter()
        .any(|n| n.name.as_deref() == Some("still the old page")));

    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

// ---- about:downloads: navigation and live refresh -----------------------

use crate::downloads_page::test_support::{
    fake_downloads_live, FakeState, Scratch as DownloadsScratch,
};
use crate::downloads_page::DownloadsSource;
use blueice_ipc::downloads::{TransferInfo, TransferState, DOWNLOADS_PROTOCOL_VERSION};
use std::sync::{Arc, Mutex};

fn dl(id: u64, name: &str, state: TransferState, done: u64) -> TransferInfo {
    TransferInfo {
        id,
        url: format!("https://example.com/{name}"),
        dest_path: format!("/d/{name}"),
        state,
        total_bytes: Some(1000),
        completed_bytes: done,
        ..TransferInfo::default()
    }
}

/// A session whose tabs read `about:downloads` from `socket`; returns
/// the client end and the session thread.
fn downloads_session(label: &str, socket: PathBuf) -> (UnixStream, thread::JoinHandle<()>) {
    let dir = temp_frame_dir(label);
    let gatekeeper = clearing_gatekeeper(label);
    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(400.0, 300.0);
        tabs.set_downloads_source(Arc::new(DownloadsSource::without_spawner(socket)));
        let mut generation = 0u64;
        run_session(&mut tabs, &mut server, &dir, &mut generation, &gatekeeper).unwrap();
    });
    handshake(&mut client);
    (client, handle)
}

fn navigate_to(client: &mut UnixStream, url: &str) -> u64 {
    client
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    blueice_ipc::write_client_message(
        client,
        &ClientMessage::Navigate {
            url: url.to_string(),
        },
    )
    .unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(client).unwrap(),
        ServerMessage::Navigated {
            url: url.to_string(),
        }
    );
    let ServerMessage::FrameReady { generation, .. } =
        blueice_ipc::read_server_message(client).unwrap()
    else {
        panic!("expected the frame after Navigated")
    };
    generation
}

/// The next `FrameReady` the session pushes within `wait`, if any.
fn next_pushed_frame(client: &mut UnixStream, wait: Duration) -> Option<u64> {
    client.set_read_timeout(Some(wait)).unwrap();
    match blueice_ipc::read_server_message_with_ids(client) {
        Ok((_, request_id, ServerMessage::FrameReady { generation, .. })) => {
            assert_eq!(
                request_id, None,
                "a refresh is unsolicited, so it carries no request id"
            );
            Some(generation)
        }
        Ok((_, _, other)) => panic!("unexpected message {other:?}"),
        Err(e) if is_timeout(&e) => None,
        Err(e) => panic!("{e}"),
    }
}

/// Wait for the next background refresh frame without relying on a fixed
/// "let background work settle" interval.
fn next_refresh(client: &mut UnixStream) -> u64 {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(generation) = next_pushed_frame(client, Duration::from_millis(50)) {
            return generation;
        }
        assert!(
            Instant::now() < deadline,
            "the downloads refresher never returned a frame"
        );
    }
}

fn dom_text(client: &mut UnixStream) -> String {
    client
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    blueice_ipc::write_client_message(client, &ClientMessage::GetDom).unwrap();
    loop {
        match blueice_ipc::read_server_message(client).unwrap() {
            ServerMessage::Dom(text) => return text,
            ServerMessage::FrameReady { .. } => {} // a refresh landed in between
            other => panic!("unexpected {other:?}"),
        }
    }
}

fn finish_session(mut client: UnixStream, handle: thread::JoinHandle<()>) {
    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    handle.join().unwrap();
}

#[test]
fn navigating_to_about_downloads_replies_at_once_and_shows_the_live_list() {
    let dir = DownloadsScratch::new("sess-nav");
    let state = FakeState {
        transfers: Arc::new(Mutex::new(vec![dl(
            1,
            "alpha.iso",
            TransferState::Active,
            400,
        )])),
        ..FakeState::default()
    };
    let _server = fake_downloads_live(&dir.socket(), state, false, DOWNLOADS_PROTOCOL_VERSION);
    let (mut client, handle) = downloads_session("sess-nav", dir.socket());

    navigate_to(&mut client, "about:downloads");
    let dom = dom_text(&mut client);
    assert!(
        dom.contains("alpha.iso") && dom.contains("Downloading"),
        "{dom}"
    );
    finish_session(client, handle);
}

#[test]
fn the_page_updates_itself_and_pushes_a_frame_when_a_transfer_changes() {
    let dir = DownloadsScratch::new("sess-live");
    let state = FakeState {
        transfers: Arc::new(Mutex::new(vec![dl(
            1,
            "alpha.iso",
            TransferState::Active,
            400,
        )])),
        ..FakeState::default()
    };
    let live = state.transfers.clone();
    let lists = state.lists.clone();
    let _server = fake_downloads_live(&dir.socket(), state, false, DOWNLOADS_PROTOCOL_VERSION);
    let (mut client, handle) = downloads_session("sess-live", dir.socket());
    let initial_lists = lists.load(Ordering::SeqCst);
    let first = navigate_to(&mut client, "about:downloads");
    let initial_refresh = next_refresh(&mut client);
    assert!(initial_refresh > first);
    assert!(lists.load(Ordering::SeqCst) > initial_lists);

    *live.lock().unwrap() = vec![
        dl(1, "alpha.iso", TransferState::Completed, 1000),
        dl(2, "beta.zip", TransferState::Active, 100),
    ];
    let pushed = next_refresh(&mut client);
    assert!(
        pushed > first,
        "the pushed frame is newer: {pushed} vs {first}"
    );

    // The human-visible frame and the AI-facing representation come from the same render pass.
    blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
    let ServerMessage::Representation(snapshot) =
        blueice_ipc::read_server_message(&mut client).unwrap()
    else {
        panic!("expected Representation")
    };
    assert_eq!(
        snapshot.generation, pushed,
        "the frame just pushed and the representation share one generation"
    );
    let dom = dom_text(&mut client);
    assert!(
        dom.contains("Completed") && dom.contains("beta.zip"),
        "{dom}"
    );
    finish_session(client, handle);
}

#[test]
fn a_busy_downloads_tab_cannot_age_out_another_tabs_frame_or_generation() {
    let dir = DownloadsScratch::new("sess-tab-frame-isolation");
    let state = FakeState {
        transfers: Arc::new(Mutex::new(vec![dl(
            1,
            "progressing.iso",
            TransferState::Active,
            400,
        )])),
        ..FakeState::default()
    };
    let _server = fake_downloads_live(&dir.socket(), state, false, DOWNLOADS_PROTOCOL_VERSION);
    let (mut client, handle) = downloads_session("sess-tab-frame-isolation", dir.socket());

    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::Navigate {
            url: "about:credits".to_string(),
        },
    )
    .unwrap();
    assert!(matches!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::Navigated { .. }
    ));
    let ServerMessage::FrameReady {
        shm_path: mut quiet_path,
        generation: mut quiet_generation,
        ..
    } = blueice_ipc::read_server_message(&mut client).unwrap()
    else {
        panic!("expected the credits tab's initial frame")
    };
    assert_eq!(quiet_generation, 1);

    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::OpenTab {
            url: Some("about:downloads".to_string()),
        },
    )
    .unwrap();
    let ServerMessage::TabOpened {
        tab_id: downloads_tab,
        ..
    } = blueice_ipc::read_server_message(&mut client).unwrap()
    else {
        panic!("expected downloads tab")
    };
    loop {
        let (tab_id, _, message) = blueice_ipc::read_server_message_with_ids(&mut client).unwrap();
        if matches!(message, ServerMessage::FrameReady { .. }) && tab_id == Some(downloads_tab) {
            break;
        }
    }

    // Resizing the one physical window now eagerly reflows both tabs.
    // More than the old global retention window's worth of downloads-tab
    // renders must still leave tab one's *latest* frame on disk. The fake
    // process has an active transfer, so this is the same two-tab shape
    // as a live panel; deterministic resizes avoid a wall-clock wait for
    // five poll ticks.
    for width in 201..=205 {
        blueice_ipc::write_client_message_with_ids(
            &mut client,
            Some(downloads_tab),
            None,
            &ClientMessage::Resize { width, height: 300 },
        )
        .unwrap();
        let mut saw_downloads_frame = false;
        let mut saw_quiet_frame = false;
        while !saw_downloads_frame || !saw_quiet_frame {
            let (tab_id, _, message) =
                blueice_ipc::read_server_message_with_ids(&mut client).unwrap();
            match message {
                ServerMessage::FrameReady {
                    width: rendered_width,
                    height: 300,
                    ..
                } if tab_id == Some(downloads_tab) && rendered_width == width => {
                    saw_downloads_frame = true;
                }
                ServerMessage::FrameReady {
                    shm_path,
                    width: rendered_width,
                    height: 300,
                    generation,
                } if tab_id == Some(1) && rendered_width == width => {
                    quiet_path = shm_path;
                    quiet_generation = generation;
                    saw_quiet_frame = true;
                }
                ServerMessage::FrameReady { .. } => {}
                other => panic!("unexpected message while resizing tabs: {other:?}"),
            }
        }
    }

    assert!(
        blueice_ipc::shm::map_frame(Path::new(&quiet_path)).is_ok(),
        "another tab's frame must survive its busy neighbor"
    );
    blueice_ipc::write_client_message(&mut client, &ClientMessage::GetRepresentation).unwrap();
    loop {
        match blueice_ipc::read_server_message(&mut client).unwrap() {
            ServerMessage::Representation(snapshot) => {
                assert_eq!(
                    snapshot.generation, quiet_generation,
                    "tab one's snapshot must still name its own last frame"
                );
                break;
            }
            ServerMessage::FrameReady { .. } => {} // a downloads refresh may arrive first
            other => panic!("unexpected message {other:?}"),
        }
    }

    finish_session(client, handle);
}

#[test]
fn an_unchanged_list_does_not_keep_pushing_frames() {
    let dir = DownloadsScratch::new("sess-quiet");
    let state = FakeState {
        transfers: Arc::new(Mutex::new(vec![dl(
            1,
            "alpha.iso",
            TransferState::Paused,
            400,
        )])),
        ..FakeState::default()
    };
    let lists = state.lists.clone();
    let _server = fake_downloads_live(&dir.socket(), state, false, DOWNLOADS_PROTOCOL_VERSION);
    let (mut client, handle) = downloads_session("sess-quiet", dir.socket());
    let initial_lists = lists.load(Ordering::SeqCst);
    navigate_to(&mut client, "about:downloads");
    next_refresh(&mut client);
    assert!(lists.load(Ordering::SeqCst) > initial_lists);

    let polled_before = lists.load(Ordering::SeqCst);
    let deadline = Instant::now() + Duration::from_secs(5);
    while lists.load(Ordering::SeqCst) <= polled_before {
        assert!(
            next_pushed_frame(&mut client, Duration::from_millis(50)).is_none(),
            "nothing changed, so nothing should be pushed"
        );
        assert!(
            Instant::now() < deadline,
            "the refresher never made another list request"
        );
    }
    finish_session(client, handle);
}

#[test]
fn leaving_the_downloads_page_forgets_its_future_polling_state() {
    let tabs = TabManager::new(100.0, 100.0);
    let tab_id = tabs.default_tab();
    let mut refresher = DownloadsRefresher::default();
    refresher
        .seen_url
        .insert(tab_id, "about:downloads".to_string());
    refresher.due.insert(tab_id, Instant::now());
    refresher
        .rendered
        .insert(tab_id, "<p>last render</p>".to_string());
    refresher.fresh_visit.insert(tab_id);
    refresher.visit.insert(tab_id, 1);
    refresher.consecutive_failures.insert(tab_id, 1);
    let (tx, _rx) = mpsc::channel();

    // The default page is about:blank. No helper thread or sleep is
    // needed to prove it cannot schedule another socket read.
    refresher.tick(&tabs, &tx, Instant::now());

    assert!(!refresher.seen_url.contains_key(&tab_id));
    assert!(!refresher.due.contains_key(&tab_id));
    assert!(!refresher.rendered.contains_key(&tab_id));
    assert!(!refresher.fresh_visit.contains(&tab_id));
    assert!(!refresher.consecutive_failures.contains_key(&tab_id));
}

#[test]
fn an_absent_service_shows_the_not_running_page_and_the_list_appears_when_it_starts() {
    let dir = DownloadsScratch::new("sess-late");
    let (mut client, handle) = downloads_session("sess-late", dir.socket());
    navigate_to(&mut client, "about:downloads");
    assert!(dom_text(&mut client).contains("The downloads service is not running"));

    // The service comes up later; the open page notices without being reloaded.
    let state = FakeState {
        transfers: Arc::new(Mutex::new(vec![dl(
            3,
            "gamma.bin",
            TransferState::Active,
            10,
        )])),
        ..FakeState::default()
    };
    let _server = fake_downloads_live(&dir.socket(), state, false, DOWNLOADS_PROTOCOL_VERSION);
    let deadline = Instant::now() + Duration::from_secs(6);
    loop {
        let _ = next_pushed_frame(&mut client, Duration::from_millis(50));
        let dom = dom_text(&mut client);
        if dom.contains("gamma.bin") && !dom.contains("not running") {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the open page never showed the recovered service: {dom}"
        );
    }
    finish_session(client, handle);
}

#[test]
fn a_hung_downloads_service_cannot_stall_the_session() {
    let dir = DownloadsScratch::new("sess-hung");
    let state = FakeState::default();
    let stalls = state.stalls.clone();
    let _server = fake_downloads_live(&dir.socket(), state, true, DOWNLOADS_PROTOCOL_VERSION);
    let (mut client, handle) = downloads_session("sess-hung", dir.socket());
    navigate_to(&mut client, "about:credits");

    navigate_to(&mut client, "about:downloads");
    assert!(
        dom_text(&mut client).contains("not running"),
        "a service that does not answer is shown as unavailable"
    );

    // One connection is the navigation's short read; the second is the
    // open page's background poll. Wait until the latter has genuinely
    // reached its non-responsive peer before asking the session to
    // resize. This proves the non-blocking boundary without making an
    // assertion about wall-clock scheduling or font-loading speed.
    let deadline = Instant::now() + Duration::from_secs(6);
    while stalls.load(Ordering::SeqCst) < 2 {
        assert!(
            Instant::now() < deadline,
            "the downloads refresher never reached the hung service"
        );
        thread::sleep(Duration::from_millis(10));
    }

    // The background fetch is now known to be blocked in the fake
    // service. An ordinary request still gets its normal, correlated
    // frame reply from the session loop.
    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::Resize {
            width: 123,
            height: 234,
        },
    )
    .unwrap();
    loop {
        match blueice_ipc::read_server_message(&mut client).unwrap() {
            ServerMessage::FrameReady {
                width: 123,
                height: 234,
                ..
            } => break,
            ServerMessage::FrameReady { .. } => {}
            other => panic!("unexpected {other:?}"),
        }
    }
    finish_session(client, handle);
}

#[test]
fn a_link_to_about_downloads_is_followed_like_any_built_in_page() {
    let dir = DownloadsScratch::new("sess-link");
    let state = FakeState {
        transfers: Arc::new(Mutex::new(vec![dl(
            1,
            "alpha.iso",
            TransferState::Completed,
            1000,
        )])),
        ..FakeState::default()
    };
    let _server = fake_downloads_live(&dir.socket(), state, false, DOWNLOADS_PROTOCOL_VERSION);
    let frame_dir = temp_frame_dir("sess-link");
    let gatekeeper = clearing_gatekeeper("sess-link");
    let (mut client, mut server) = client_pair();
    let socket = dir.socket();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(400.0, 300.0);
        tabs.set_downloads_source(Arc::new(DownloadsSource::without_spawner(socket)));
        default_page(&mut tabs)
            .load_html_str(r#"<a href="about:downloads">Open downloads</a>"#, None);
        let mut generation = 0u64;
        run_session(
            &mut tabs,
            &mut server,
            &frame_dir,
            &mut generation,
            &gatekeeper,
        )
        .unwrap();
    });
    handshake(&mut client);
    blueice_ipc::write_client_message(&mut client, &ClientMessage::Click { x: 2.0, y: 2.0 })
        .unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    let ServerMessage::Navigated { url, .. } =
        blueice_ipc::read_server_message(&mut client).unwrap()
    else {
        panic!("expected Navigated")
    };
    assert_eq!(url, "about:downloads");
    assert!(matches!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::FrameReady { .. }
    ));
    blueice_ipc::write_client_message(&mut client, &ClientMessage::GetDom).unwrap();
    loop {
        match blueice_ipc::read_server_message(&mut client).unwrap() {
            ServerMessage::Dom(text) => {
                assert!(
                    text.contains("alpha.iso"),
                    "the built-in link shows the downloads list: {text}"
                );
                break;
            }
            ServerMessage::FrameReady { .. } => {}
            other => panic!("unexpected {other:?}"),
        }
    }
    finish_session(client, handle);
}
