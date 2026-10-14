// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_engine::{ObjectUrlPurpose, TabId, TabManager};
use blueice_ipc::viewport::DisplayViewport;

fn document(tabs: &mut TabManager, tab: TabId, url: &str) {
    tabs.get_mut(tab)
        .unwrap()
        .load_html_str("<p>owner</p>", Some(url.into()));
}

#[test]
fn object_urls_retain_binary_data_and_use_creator_origin() {
    let mut tabs = TabManager::new(300.0, 200.0);
    let owner = tabs.default_tab();
    document(&mut tabs, owner, "https://EXAMPLE.test:443/source");
    let url = tabs
        .create_object_url(owner, vec![0, 255, 65], "application/octet-stream")
        .unwrap();
    assert!(url.starts_with("blob:https://example.test/"));
    let body = tabs
        .resolve_object_url(owner, &url, ObjectUrlPurpose::Fetch)
        .unwrap();
    assert_eq!(body.bytes(), &[0, 255, 65]);
    assert_eq!(body.mime_type(), "application/octet-stream");
    assert_eq!(body.creator_origin(), "https://example.test");
    let next = tabs
        .create_object_url(owner, vec![1], "text/plain")
        .unwrap();
    assert_ne!(url, next);
    assert!(tabs
        .resolve_object_url(owner, &format!("{url}#section"), ObjectUrlPurpose::Fetch)
        .is_some());
    assert!(tabs
        .resolve_object_url(owner, &format!("{url}?query"), ObjectUrlPurpose::Fetch)
        .is_none());
}

#[test]
fn fetching_and_revocation_require_same_origin_and_partition() {
    let mut tabs = TabManager::new(300.0, 200.0);
    let owner = tabs.default_tab();
    document(&mut tabs, owner, "https://example.test/source");
    let peer = tabs.open_tab();
    document(&mut tabs, peer, "https://example.test/other");
    let another_window = tabs
        .create_window(DisplayViewport {
            width: 300.0,
            height: 200.0,
            device_scale: 1.0,
            backing_scale: None,
        })
        .unwrap();
    tabs.move_tab_to_window(peer, another_window).unwrap();
    let foreign = tabs.open_tab();
    document(&mut tabs, foreign, "https://other.test/");
    let context = tabs.create_context("Isolated".into()).unwrap();
    let window = tabs
        .create_window_in_context(
            context,
            DisplayViewport {
                width: 300.0,
                height: 200.0,
                device_scale: 1.0,
                backing_scale: None,
            },
        )
        .unwrap();
    let partition = tabs.open_tab_in_window(window).unwrap();
    document(&mut tabs, partition, "https://example.test/");
    let url = tabs
        .create_object_url(owner, vec![7], "text/plain")
        .unwrap();
    assert!(tabs
        .resolve_object_url(peer, &url, ObjectUrlPurpose::Fetch)
        .is_some());
    for requester in [foreign, partition] {
        assert!(tabs
            .resolve_object_url(requester, &url, ObjectUrlPurpose::Fetch)
            .is_none());
        tabs.revoke_object_url(requester, &url);
        assert!(tabs
            .resolve_object_url(owner, &url, ObjectUrlPurpose::Fetch)
            .is_some());
        assert!(tabs
            .resolve_object_url(requester, &url, ObjectUrlPurpose::Navigation)
            .is_some());
    }
    tabs.revoke_object_url(peer, &format!("{url}#fragment"));
    assert!(tabs
        .resolve_object_url(owner, &url, ObjectUrlPurpose::Fetch)
        .is_none());
    tabs.revoke_object_url(peer, &url);
    tabs.revoke_object_url(peer, "https://example.test/");
}

#[test]
fn replacing_or_closing_creator_retires_urls_but_keeps_started_reads() {
    let mut tabs = TabManager::new(300.0, 200.0);
    let owner = tabs.default_tab();
    document(&mut tabs, owner, "https://example.test/");
    let peer = tabs.open_tab();
    document(&mut tabs, peer, "https://example.test/");
    let url = tabs
        .create_object_url(owner, vec![0, 255], "text/plain")
        .unwrap();
    let started = tabs
        .resolve_object_url(peer, &url, ObjectUrlPurpose::Fetch)
        .unwrap();
    document(&mut tabs, owner, "https://example.test/new");
    assert!(tabs
        .resolve_object_url(peer, &url, ObjectUrlPurpose::Fetch)
        .is_none());
    assert_eq!(started.bytes(), &[0, 255]);
    let next = tabs.create_object_url(owner, vec![8], "").unwrap();
    assert!(tabs.close_tab(owner));
    assert!(tabs
        .resolve_object_url(peer, &next, ObjectUrlPurpose::Fetch)
        .is_none());
    assert_eq!(started.bytes(), &[0, 255]);
}

#[test]
fn opaque_documents_do_not_share_null_origin_authority() {
    let mut tabs = TabManager::new(300.0, 200.0);
    let owner = tabs.default_tab();
    document(&mut tabs, owner, "about:blank");
    let peer = tabs.open_tab();
    document(&mut tabs, peer, "about:blank");
    let url = tabs.create_object_url(owner, vec![9], "").unwrap();
    assert!(url.starts_with("blob:null/"));
    assert!(tabs
        .resolve_object_url(owner, &url, ObjectUrlPurpose::Fetch)
        .is_some());
    assert!(tabs
        .resolve_object_url(peer, &url, ObjectUrlPurpose::Fetch)
        .is_none());
    tabs.revoke_object_url(peer, &url);
    assert!(tabs
        .resolve_object_url(owner, &url, ObjectUrlPurpose::Fetch)
        .is_some());
    tabs.revoke_object_url(owner, &url);
    assert!(tabs
        .resolve_object_url(owner, &url, ObjectUrlPurpose::Fetch)
        .is_none());
}

#[test]
fn explicit_revocation_keeps_started_reads_and_rejects_unknown_requesters() {
    let mut tabs = TabManager::new(300.0, 200.0);
    let owner = tabs.default_tab();
    document(&mut tabs, owner, "https://example.test/");
    let url = tabs
        .create_object_url(owner, vec![7, 0, 255], "TEXT/PLAIN")
        .unwrap();
    let started = tabs
        .resolve_object_url(owner, &url, ObjectUrlPurpose::Fetch)
        .unwrap();
    assert_eq!(started.mime_type(), "text/plain");
    let unknown = TabId::from_u64(u64::MAX);
    assert!(tabs.create_object_url(unknown, vec![], "").is_err());
    assert!(tabs
        .resolve_object_url(unknown, &url, ObjectUrlPurpose::Navigation)
        .is_none());
    tabs.revoke_object_url(unknown, &url);
    assert!(tabs
        .resolve_object_url(owner, &url, ObjectUrlPurpose::Fetch)
        .is_some());
    tabs.revoke_object_url(owner, &url);
    assert!(tabs
        .resolve_object_url(owner, &url, ObjectUrlPurpose::Navigation)
        .is_none());
    drop(tabs);
    assert_eq!(started.bytes(), &[7, 0, 255]);
}

#[test]
fn staged_uploads_publish_only_complete_bytes_and_cannot_cross_documents() {
    let mut tabs = TabManager::new(300.0, 200.0);
    let owner = tabs.default_tab();
    document(&mut tabs, owner, "https://example.test/");
    let peer = tabs.open_tab();
    document(&mut tabs, peer, "https://example.test/");
    let bytes: Vec<u8> = (0..70_000).map(|index| (index % 256) as u8).collect();
    let upload = tabs
        .begin_object_url_upload(owner, bytes.len(), "application/octet-stream")
        .unwrap();
    assert!(tabs
        .append_object_url_upload(peer, upload, 0, &[1])
        .is_err());
    assert!(tabs.commit_object_url_upload(owner, upload).is_err());
    tabs.append_object_url_upload(owner, upload, 0, &bytes[..65_536])
        .unwrap();
    assert!(tabs
        .append_object_url_upload(owner, upload, 0, &[1])
        .is_err());
    tabs.append_object_url_upload(owner, upload, 65_536, &bytes[65_536..])
        .unwrap();
    let url = tabs.commit_object_url_upload(owner, upload).unwrap();
    assert_eq!(
        tabs.resolve_object_url(peer, &url, ObjectUrlPurpose::Fetch)
            .unwrap()
            .bytes(),
        bytes
    );
    assert!(tabs.commit_object_url_upload(owner, upload).is_err());
    let stale = tabs.begin_object_url_upload(owner, 1, "").unwrap();
    document(&mut tabs, owner, "https://example.test/new");
    assert!(tabs
        .append_object_url_upload(owner, stale, 0, &[1])
        .is_err());
    assert!(tabs.commit_object_url_upload(owner, stale).is_err());
}

#[test]
fn snapshot_history_restoration_does_not_revive_departed_document_urls() {
    use blueice_engine::{
        session::{run_session, ReadTimeout},
        HistorySnapshotMode,
    };
    use std::io::{Cursor, Read, Write};
    struct Pipe {
        input: Cursor<Vec<u8>>,
        output: Vec<u8>,
    }
    impl Read for Pipe {
        fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
            self.input.read(bytes)
        }
    }
    impl Write for Pipe {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.output.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    impl ReadTimeout for Pipe {
        fn set_read_timeout(&self, _: Option<std::time::Duration>) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut tabs =
        TabManager::new_with_history_snapshot_mode(300.0, 200.0, HistorySnapshotMode::Snapshot);
    let owner = tabs.default_tab();
    document(&mut tabs, owner, "https://example.test/original");
    let url = tabs
        .create_object_url(owner, vec![7], "text/plain")
        .unwrap();
    let started = tabs
        .resolve_object_url(owner, &url, ObjectUrlPurpose::Fetch)
        .unwrap();
    let mut input = vec![];
    for request in [
        blueice_ipc::ClientMessage::Hello {
            protocol_version: blueice_ipc::PROTOCOL_VERSION,
        },
        blueice_ipc::ClientMessage::Navigate {
            url: "about:credits".into(),
        },
        blueice_ipc::ClientMessage::GoBack,
        blueice_ipc::ClientMessage::Shutdown,
    ] {
        blueice_ipc::write_client_message(&mut input, &request).unwrap();
    }
    let mut pipe = Pipe {
        input: Cursor::new(input),
        output: vec![],
    };
    let root = std::env::temp_dir().join(format!(
        "bi-url-history-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&root).unwrap();
    let result = run_session(
        &mut tabs,
        &mut pipe,
        &root,
        &mut 0,
        &root.join("unused-gatekeeper.sock"),
    );
    let _ = std::fs::remove_dir_all(&root);
    result.unwrap();
    let mut output = Cursor::new(pipe.output);
    let mut messages = Vec::new();
    while (output.position() as usize) < output.get_ref().len() {
        messages.push(blueice_ipc::read_server_message(&mut output).unwrap());
    }
    assert!(messages.iter().any(|message| matches!(message, blueice_ipc::ServerMessage::Navigated { url } if url == "about:credits")), "{messages:?}");
    assert_eq!(
        tabs.get(owner).unwrap().url(),
        Some("https://example.test/original")
    );
    assert!(tabs
        .resolve_object_url(owner, &url, ObjectUrlPurpose::Fetch)
        .is_none());
    assert_eq!(started.bytes(), &[7]);
    let fresh = tabs
        .create_object_url(owner, vec![8], "text/plain")
        .unwrap();
    assert_ne!(fresh, url);
}

#[test]
fn upload_reservations_are_bounded_and_only_the_creator_can_abort_them() {
    let mut tabs = TabManager::new(300.0, 200.0);
    let owner = tabs.default_tab();
    document(&mut tabs, owner, "https://example.test/");
    let peer = tabs.open_tab();
    document(&mut tabs, peer, "https://example.test/");
    assert!(tabs.begin_object_url_upload(owner, usize::MAX, "").is_err());
    let reserved = tabs
        .begin_object_url_upload(owner, 256 * 1024 * 1024, "")
        .unwrap();
    assert!(tabs.create_object_url(peer, vec![1], "").is_err());
    tabs.abort_object_url_upload(peer, reserved);
    assert!(tabs.create_object_url(owner, vec![1], "").is_err());
    tabs.abort_object_url_upload(owner, reserved);
    let url = tabs.create_object_url(owner, vec![1], "").unwrap();
    assert_eq!(
        tabs.resolve_object_url(peer, &url, ObjectUrlPurpose::Fetch)
            .unwrap()
            .bytes(),
        &[1]
    );
}
