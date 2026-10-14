// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
#![cfg(unix)]

#[path = "../../../net/tests/common/mod.rs"]
mod common;
#[allow(dead_code)]
#[path = "../../../net/tests/fixtures/navigation_response.rs"]
mod response_fixture;

use blueice_downloads::manager::{ManagerConfig, TransferManager};
use blueice_engine::{session::run_session, TabManager};
use blueice_ipc::downloads::TransferState;
use blueice_ipc::navigation_session::NavigationSessionAction;
use blueice_ipc::{ClientMessage, ServerMessage};
use common::{FakeGatekeeper, TempDir};
use response_fixture::ResponseServer;
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

struct Browser {
    client: UnixStream,
    core: Option<JoinHandle<()>>,
    downloads: Option<JoinHandle<()>>,
    manager: Arc<TransferManager>,
    stop: Arc<AtomicBool>,
    _gate: FakeGatekeeper,
    _frames: TempDir,
    _files: TempDir,
    _data: TempDir,
    _sockets: TempDir,
}
impl Browser {
    fn new() -> Self {
        let gate = FakeGatekeeper::clear_all();
        let frames = TempDir::new();
        let files = TempDir::new();
        let data = TempDir::new();
        let sockets = TempDir::new();
        let socket = sockets.join("d.sock");
        let manager =
            TransferManager::open(ManagerConfig::new(files.path(), data.path(), &gate.socket))
                .unwrap();
        let listener = UnixListener::bind(&socket).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let (owned_manager, owned_stop) = (manager.clone(), stop.clone());
        let downloads = thread::spawn(move || {
            blueice_downloads::server::serve(listener, owned_manager, owned_stop)
        });
        let mut tabs = TabManager::new(400.0, 300.0);
        tabs.set_downloads_source(Arc::new(
            blueice_engine::downloads_page::DownloadsSource::without_spawner(socket),
        ));
        let (mut client, mut server) = UnixStream::pair().unwrap();
        client
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let frame_path = frames.path().to_owned();
        let gate_path = gate.socket.clone();
        let core = thread::spawn(move || {
            run_session(&mut tabs, &mut server, &frame_path, &mut 0, &gate_path).unwrap()
        });
        blueice_ipc::client_handshake(&mut client).unwrap();
        let mut browser = Self {
            client,
            core: Some(core),
            downloads: Some(downloads),
            manager,
            stop,
            _gate: gate,
            _frames: frames,
            _files: files,
            _data: data,
            _sockets: sockets,
        };
        browser.send(
            6,
            ClientMessage::Navigate {
                url: "about:credits".into(),
            },
        );
        assert!(matches!(browser.reply(6), ServerMessage::Navigated { .. }));
        browser
    }
    fn send(&mut self, id: u64, message: ClientMessage) {
        blueice_ipc::write_client_message_with_ids(&mut self.client, Some(1), Some(id), &message)
            .unwrap();
    }
    fn reply(&mut self, id: u64) -> ServerMessage {
        loop {
            let (_, request, message) =
                blueice_ipc::read_server_message_with_ids(&mut self.client).unwrap();
            if request == Some(id) {
                return message;
            }
        }
    }
    fn snapshot(&mut self, id: u64) -> ServerMessage {
        self.send(
            id,
            ClientMessage::NavigationSession(NavigationSessionAction::Inspect),
        );
        let snapshot = self.reply(id);
        assert!(matches!(
            snapshot,
            ServerMessage::NavigationSessionState { .. }
        ));
        snapshot
    }
    fn offer(&mut self, server: &ResponseServer) -> u64 {
        self.send(
            8,
            ClientMessage::Navigate {
                url: server.url.clone(),
            },
        );
        match self.reply(8) {
            ServerMessage::NavigationDownloadOffered {
                navigation_id,
                current_url,
            } => {
                assert_eq!(current_url.as_deref(), Some("about:credits"));
                navigation_id
            }
            other => panic!("expected download offer: {other:?}"),
        }
    }
}
impl Drop for Browser {
    fn drop(&mut self) {
        let _ = self.client.shutdown(std::net::Shutdown::Both);
        if let Some(core) = self.core.take() {
            core.join().unwrap();
        }
        self.manager.shutdown();
        self.stop.store(true, Ordering::SeqCst);
        if let Some(downloads) = self.downloads.take() {
            downloads.join().unwrap();
        }
    }
}
fn attachment() -> ResponseServer {
    ResponseServer::start(
        "200 OK",
        &[
            ("Content-Type", "application/octet-stream"),
            ("Content-Disposition", "attachment; filename=original.bin"),
        ],
        &[0xff, 0, 0x80, 42],
        false,
    )
}

#[test]
fn original_response_core_handoff_preserves_document_history_and_exact_bytes() {
    let server = attachment();
    let mut browser = Browser::new();
    let before = browser.snapshot(7);
    let sequence = browser.offer(&server);
    assert_eq!(browser.snapshot(9), before);
    browser.send(
        10,
        ClientMessage::ContinueNavigationDownload {
            navigation_id: sequence,
            accept: true,
        },
    );
    let ServerMessage::NavigationDownloadStarted { transfer_id } = browser.reply(10) else {
        panic!("expected owned manager handoff")
    };
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let info = browser.manager.get(transfer_id).unwrap();
        if info.state == TransferState::Completed {
            assert!(info.original_response);
            assert_eq!(std::fs::read(info.dest_path).unwrap(), [0xff, 0, 0x80, 42]);
            break;
        }
        assert!(
            Instant::now() < deadline,
            "original response failed: {info:?}"
        );
        thread::sleep(Duration::from_millis(10));
    }
    let request = server.finish();
    assert_eq!(request.method, "GET");
    assert_eq!(browser.snapshot(11), before);
    browser.send(
        12,
        ClientMessage::ContinueNavigationDownload {
            navigation_id: sequence,
            accept: true,
        },
    );
    assert!(matches!(browser.reply(12), ServerMessage::Error { .. }));
    assert_eq!(
        browser.manager.list(None).len(),
        1,
        "duplicate decisions must not duplicate a download"
    );
}

#[test]
fn original_response_superseded_offer_cannot_start_or_publish() {
    let server = attachment();
    let mut browser = Browser::new();
    let sequence = browser.offer(&server);
    browser.send(
        9,
        ClientMessage::Navigate {
            url: "about:credits?lang=zh-TW".into(),
        },
    );
    assert!(matches!(browser.reply(9), ServerMessage::Navigated { .. }));
    browser.send(
        10,
        ClientMessage::ContinueNavigationDownload {
            navigation_id: sequence,
            accept: true,
        },
    );
    assert!(matches!(browser.reply(10), ServerMessage::Error { .. }));
    assert!(browser.manager.list(None).is_empty());
    let _ = server.finish();
}

#[test]
fn original_response_declined_offer_retires_its_body_without_a_transfer() {
    let server = attachment();
    let mut browser = Browser::new();
    let before = browser.snapshot(7);
    let sequence = browser.offer(&server);
    browser.send(
        9,
        ClientMessage::ContinueNavigationDownload {
            navigation_id: sequence,
            accept: false,
        },
    );
    assert!(matches!(browser.reply(9), ServerMessage::Error { .. }));
    browser.send(
        10,
        ClientMessage::ContinueNavigationDownload {
            navigation_id: sequence,
            accept: true,
        },
    );
    assert!(matches!(browser.reply(10), ServerMessage::Error { .. }));
    assert!(browser.manager.list(None).is_empty());
    assert_eq!(browser.snapshot(11), before);
    let _ = server.finish();
}

#[test]
fn original_response_cancel_closes_the_upstream_http_body_before_more_bytes_arrive() {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::mpsc;
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}/stalled-original", listener.local_addr().unwrap());
    let (tx, rx) = mpsc::channel();
    let origin = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error)
                    if error.kind() == std::io::ErrorKind::WouldBlock
                        && Instant::now() < deadline =>
                {
                    thread::sleep(Duration::from_millis(5))
                }
                other => panic!("fixture did not receive its original request: {other:?}"),
            }
        };
        stream.set_nonblocking(false).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_millis(100)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        let mut request = Vec::new();
        while !request.windows(4).any(|part| part == b"\r\n\r\n") {
            let mut buffer = [0; 1024];
            let count = stream.read(&mut buffer).unwrap();
            assert!(count > 0 && request.len() + count <= 16384);
            request.extend_from_slice(&buffer[..count]);
        }
        stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Disposition: attachment; filename=stalled.bin\r\nContent-Length: 1\r\nConnection: close\r\n\r\n").unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let closed = loop {
            let mut byte = [0];
            match stream.read(&mut byte) {
                Ok(0) => break true,
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::BrokenPipe
                    ) =>
                {
                    break true
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                    ) && Instant::now() < deadline =>
                {
                    continue
                }
                _ => break false,
            }
        };
        // Release the old uncancellable reader before joining/any assertions.
        // This keeps the failure fixture bounded and reaps its owned worker.
        if !closed {
            let _ = stream.write_all(b"x");
        }
        let _ = tx.send(closed);
    });
    let mut browser = Browser::new();
    browser.send(8, ClientMessage::Navigate { url });
    let ServerMessage::NavigationDownloadOffered { navigation_id, .. } = browser.reply(8) else {
        panic!("expected original-response offer")
    };
    browser.send(
        9,
        ClientMessage::ContinueNavigationDownload {
            navigation_id,
            accept: true,
        },
    );
    let ServerMessage::NavigationDownloadStarted { transfer_id } = browser.reply(9) else {
        panic!("expected stream acceptance")
    };
    let deadline = Instant::now() + Duration::from_secs(5);
    while browser.manager.get(transfer_id).unwrap().state != TransferState::Active {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(10));
    }
    let info = browser.manager.cancel(transfer_id).unwrap();
    assert_eq!(info.state, TransferState::Cancelled);
    let early_close = rx.recv_timeout(Duration::from_secs(2));
    origin.join().unwrap();
    assert_eq!(early_close, Ok(true), "cancelling the owned transfer must close the stalled original HTTP response without waiting for another body byte");
    assert!(!std::path::Path::new(&info.dest_path).exists());
}

#[test]
fn original_response_post_is_handed_off_once_without_changing_document_or_history() {
    let bytes = [0xff, 0, 0x80, 13, 10, 42];
    let post = ResponseServer::start(
        "201 Created",
        &[
            ("Content-Type", "application/octet-stream"),
            ("Content-Disposition", "attachment; filename=receipt.bin"),
        ],
        &bytes,
        false,
    );
    let html = format!("<form method=post action='{}'><input name=token value=one-shot><button aria-label='Download original'>Download</button></form>", post.url);
    let form = ResponseServer::start(
        "200 OK",
        &[("Content-Type", "text/html")],
        html.as_bytes(),
        false,
    );
    let mut browser = Browser::new();
    browser.send(
        20,
        ClientMessage::Navigate {
            url: form.url.clone(),
        },
    );
    assert!(matches!(browser.reply(20), ServerMessage::Navigated { .. }));
    let before = browser.snapshot(7);
    browser.send(8, ClientMessage::GetRepresentation);
    let ServerMessage::Representation(snapshot) = browser.reply(8) else {
        panic!("expected source form")
    };
    let id = snapshot
        .nodes
        .iter()
        .find(|node| node.name.as_deref() == Some("Download original"))
        .unwrap()
        .id;
    browser.send(
        9,
        ClientMessage::ActOn {
            id,
            action: blueice_ipc::NodeAction::Click,
        },
    );
    let sequence = loop {
        match browser.reply(9) {
            ServerMessage::NavigationStarted { method, .. } => assert_eq!(method, "POST"),
            ServerMessage::NavigationDownloadOffered {
                navigation_id,
                current_url,
            } => {
                assert_eq!(current_url.as_deref(), Some(form.url.as_str()));
                break navigation_id;
            }
            other => panic!("unexpected form reply: {other:?}"),
        }
    };
    browser.send(
        10,
        ClientMessage::ContinueNavigationDownload {
            navigation_id: sequence,
            accept: true,
        },
    );
    let ServerMessage::NavigationDownloadStarted { transfer_id } = browser.reply(10) else {
        panic!("expected original POST stream")
    };
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let info = browser.manager.get(transfer_id).unwrap();
        if info.state == TransferState::Completed {
            assert_eq!(std::fs::read(info.dest_path).unwrap(), bytes);
            break;
        }
        assert!(Instant::now() < deadline, "POST response failed: {info:?}");
        thread::sleep(Duration::from_millis(10));
    }
    let request = post.finish();
    assert_eq!(request.method, "POST");
    assert_eq!(request.body, b"token=one-shot");
    assert_eq!(form.finish().method, "GET");
    assert_eq!(browser.snapshot(11), before);
    assert_eq!(browser.manager.list(None).len(), 1);
}

#[test]
fn original_response_closed_pending_tab_cannot_publish_its_offer() {
    let server = attachment();
    let mut browser = Browser::new();
    browser.send(7, ClientMessage::OpenTab { url: None });
    assert!(matches!(browser.reply(7), ServerMessage::TabOpened { .. }));
    let sequence = browser.offer(&server);
    browser.send(9, ClientMessage::CloseTab);
    assert!(matches!(browser.reply(9), ServerMessage::TabClosed { .. }));
    browser.send(
        10,
        ClientMessage::ContinueNavigationDownload {
            navigation_id: sequence,
            accept: true,
        },
    );
    assert!(matches!(browser.reply(10), ServerMessage::Error { .. }));
    assert!(browser.manager.list(None).is_empty());
    let _ = server.finish();
}
