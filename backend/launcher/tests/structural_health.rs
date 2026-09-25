// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! End-to-end proof of `phase-8-live-core-hotswap/PLAN.md`'s structural health
//! bar through the real compiled launcher: v2 must render something comparable
//! to what v1 showed for each replayed tab. A web server that returns a full
//! page to v1 and an almost empty one to v2's replay makes v2 "render blank";
//! the cutover must then fail with v1 untouched, and succeed once the server
//! serves the full page again.

mod common;

use blueice_ipc::{read_server_message, write_client_message, ClientMessage, ServerMessage};
use blueice_launcher::control::ControlReply;
use common::*;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;

/// Ten paragraphs, so the represented tree is comfortably larger than a blank one.
fn full_page() -> String {
    (0..10).map(|n| format!("<p>paragraph {n}</p>")).collect()
}

/// A web server whose answer flips between a full page and a nearly empty one.
fn switchable_server(serve_full: Arc<AtomicBool>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let serve_full = Arc::clone(&serve_full);
            thread::spawn(move || {
                let mut stream = stream;
                let mut buffer = [0u8; 2048];
                let _ = stream.read(&mut buffer);
                let body = if serve_full.load(Ordering::SeqCst) {
                    full_page()
                } else {
                    "<span>.</span>".to_string() // no represented nodes at all
                };
                let _ = stream.write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                );
            });
        }
    });
    format!("http://{addr}/")
}

fn navigate(stream: &mut UnixStream, url: &str) {
    write_client_message(stream, &ClientMessage::Navigate { url: url.into() }).unwrap();
    loop {
        match read_server_message(stream).unwrap() {
            ServerMessage::Navigated { .. } => break,
            ServerMessage::Error { message } => panic!("navigation failed: {message}"),
            ServerMessage::GatekeeperBlocked { reason, .. } => panic!("blocked: {reason}"),
            _ => {}
        }
    }
}

fn node_count(stream: &mut UnixStream) -> usize {
    write_client_message(stream, &ClientMessage::GetRepresentation).unwrap();
    loop {
        if let ServerMessage::Representation(snapshot) = read_server_message(stream).unwrap() {
            return snapshot.nodes.len();
        }
    }
}

#[test]
fn a_v2_that_renders_far_less_than_v1_fails_the_cutover_and_a_comparable_one_succeeds() {
    let serve_full = Arc::new(AtomicBool::new(true));
    let url = switchable_server(Arc::clone(&serve_full));
    let rig = Rig::start("structural", "exec \"$REAL\" \"$@\"");
    let mut client = rig.client();

    navigate(&mut client, &url);
    let v1_nodes = node_count(&mut client);
    assert!(
        v1_nodes >= 10,
        "the full page must be represented: {v1_nodes}"
    );

    // From now on the server hands v2's replay a page that renders as nothing.
    serve_full.store(false, Ordering::SeqCst);
    match rig.cutover() {
        ControlReply::CutoverFailed { reason } => {
            assert!(reason.contains("structurally different"), "{reason}");
            assert!(reason.contains("failed the health check twice"), "{reason}");
        }
        other => panic!("a blank v2 must not be cut over to, got {other:?}"),
    }
    // v1 was never touched: the same connection still shows the full page.
    assert_eq!(node_count(&mut client), v1_nodes);
    assert_eq!(
        rig.invocations(),
        3,
        "v1 plus the health failure and its single retry"
    );

    // Once v2 can render the page, the very next cutover goes through.
    serve_full.store(true, Ordering::SeqCst);
    match rig.cutover() {
        ControlReply::CutoverDone { tabs_migrated } => assert_eq!(tabs_migrated, 1),
        other => panic!("a comparable v2 must be accepted, got {other:?}"),
    }
    assert_eq!(node_count(&mut client), v1_nodes, "v2 shows the same page");
}
