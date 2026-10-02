// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;
use blueice_ipc::{AiNode, Bounds, NameFrom, NodeState, Role};
use std::cell::Cell;
use std::os::unix::net::{UnixListener, UnixStream};
use std::thread;
use std::time::Duration;

fn sample_snapshot(generation: u64) -> AiSnapshot {
    AiSnapshot {
        frame_source: 0,
        generation,
        tab_id: 1,
        url: Some("https://example.com".to_string()),
        scroll_y: 0.0,
        nodes: vec![AiNode {
            id: 1,
            parent: None,
            children: vec![],
            role: Role::Link,
            name: Some("go".to_string()),
            name_from: Some(NameFrom::Contents),
            original_name: None,
            state: NodeState::default(),
            bounds: Bounds {
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 5.0,
            },
            opacity: 1.0,
            occluded: false,
            occluded_by: None,
            occluded_fraction: 0.0,
        }],
    }
}

type FakeCoreStep = Box<dyn FnOnce(ClientMessage, &mut UnixStream) + Send>;

thread_local! {
    /// The request currently being handled by [`fake_core`]. Its reply
    /// helpers mirror real core's envelope rather than silently
    /// treating an untagged broadcast as a response.
    static FAKE_REQUEST_ID: Cell<Option<u64>> = const { Cell::new(None) };
}

/// Spawns a fake `core` on the other end of a `UnixStream::pair()`
/// that reads one `ClientMessage` at a time and replies according
/// to `script` -- mirrors `blueice_engine::session`'s own test
/// strategy of driving the real wire protocol without a real
/// subprocess.
fn fake_core(mut server: UnixStream, script: Vec<FakeCoreStep>) {
    thread::spawn(move || {
        for step in script {
            let (_, request_id, msg) =
                blueice_ipc::read_client_message_with_ids(&mut server).unwrap();
            FAKE_REQUEST_ID.with(|current| current.set(request_id));
            step(msg, &mut server);
        }
    });
}

fn reply(stream: &mut UnixStream, msg: &ServerMessage) {
    FAKE_REQUEST_ID.with(|current| {
        blueice_ipc::write_server_message_with_id(stream, current.get(), msg).unwrap();
    });
}

/// Like [`reply`], but tags the reply with a concrete tab_id --
/// what a real, up-to-date `core` always does (`session.rs`'s own
/// "every reply echoes the resolved tab" guarantee), needed
/// specifically for `FrameReady` replies a test then checks via
/// `CoreConnection::last_frame`, since `record_frame` only caches a
/// frame whose reply actually carried a tab_id.
fn reply_tab(stream: &mut UnixStream, tab_id: u64, msg: &ServerMessage) {
    FAKE_REQUEST_ID.with(|current| {
        blueice_ipc::write_server_message_with_ids(stream, Some(tab_id), current.get(), msg)
            .unwrap();
    });
}

mod actions;

mod assistant;

mod lifecycle;

mod transport;

mod trust;

mod tabs;
