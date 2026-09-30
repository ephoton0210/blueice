// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Every other test in this module drives `DebuggerRequestReceiver` by hand
//! (constructing a `DebuggerRequestEnvelope` directly and calling
//! `dispatch_pending` synchronously in the same thread), which never
//! exercises `DebuggerRequestSender`'s own two public blocking wrappers
//! (`request`, `request_with_metadata_session_authorization`) -- those are
//! the actual worker-thread-facing API a real socket worker uses. This
//! module drives them for real, across a genuine sender/receiver thread
//! pair, including their `BrokenPipe` failure modes.

use super::*;
use blueice_ipc::debugger::{metadata_session_authorization, negotiate_with_values};
use std::thread;

fn run_receiver_until_one_dispatch(
    receiver: DebuggerRequestReceiver,
    tabs: TabManager,
) -> thread::JoinHandle<DebuggerRequestReceiver> {
    thread::spawn(move || loop {
        if receiver.dispatch_pending(&tabs, None) > 0 {
            return receiver;
        }
        thread::yield_now();
    })
}

#[test]
fn request_blocks_until_the_session_thread_dispatches_and_replies() {
    let tabs = TabManager::new(320.0, 200.0);
    let (sender, receiver) = debugger_request_channel();
    let handle = run_receiver_until_one_dispatch(receiver, tabs);

    let reply = sender.request(DebuggerRequest::ListPageRealms).unwrap();
    assert!(matches!(reply, DebuggerReply::PageRealms(_)));

    handle.join().unwrap();
}

#[test]
fn request_fails_closed_with_a_broken_pipe_once_the_session_thread_is_gone() {
    let (sender, receiver) = debugger_request_channel();
    drop(receiver);

    let error = sender.request(DebuggerRequest::ListPageRealms).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
}

#[test]
fn request_with_metadata_session_authorization_blocks_until_dispatched_and_replies() {
    let tabs = TabManager::new(320.0, 200.0);
    let (sender, receiver) = debugger_request_channel();
    let handle = run_receiver_until_one_dispatch(receiver, tabs);

    let hello = DebuggerRequest::Hello {
        protocol_version: DEBUGGER_PROTOCOL_VERSION,
        requested_metadata_capabilities: Default::default(),
        requested_bounded_values: false,
    };
    let ack = negotiate_with_values(&hello, &Default::default(), false);
    let session = metadata_session_authorization(&hello, &ack).unwrap();

    let reply = sender
        .request_with_metadata_session_authorization(DebuggerRequest::ListPageRealms, session)
        .unwrap();
    assert!(matches!(reply, DebuggerReply::PageRealms(_)));

    handle.join().unwrap();
}

#[test]
fn request_with_metadata_session_authorization_fails_closed_with_a_broken_pipe() {
    let (sender, receiver) = debugger_request_channel();
    drop(receiver);

    let hello = DebuggerRequest::Hello {
        protocol_version: DEBUGGER_PROTOCOL_VERSION,
        requested_metadata_capabilities: Default::default(),
        requested_bounded_values: false,
    };
    let ack = negotiate_with_values(&hello, &Default::default(), false);
    let session = metadata_session_authorization(&hello, &ack).unwrap();

    let error = sender
        .request_with_metadata_session_authorization(DebuggerRequest::ListPageRealms, session)
        .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
}
