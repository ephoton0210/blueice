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

fn client_pair() -> (UnixStream, UnixStream) {
    UnixStream::pair().unwrap()
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

// -- Gatekeeper-specific behavior --------------------------------

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

mod downloads;

mod extension_dom;

mod extensions;

mod extension_ui;

mod permissions;

mod extension_network;

mod transport;

mod actions;

mod cutover;

mod lifecycle;

mod tabs;

mod tabs_2;
