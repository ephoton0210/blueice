// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Each composite `run_session_with_*` entry point is a thin, otherwise
//! untested pass-through that fills in a different subset of
//! [`CoreSessionRequests`]/[`PageScriptRuntime`] fields before delegating to
//! the one shared, thoroughly-tested `run_session_with_script_runtime`. This
//! module drives each such wrapper directly through one minimal
//! handshake-then-shutdown round trip, so the wrapper's own glue (not the
//! shared loop it delegates to) is what's actually exercised.

use super::*;

#[test]
fn run_session_with_script_requests_handshakes_and_shuts_down() {
    let dir = temp_frame_dir("wrapper-script-requests");
    let gatekeeper = clearing_gatekeeper("wrapper-script-requests");
    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        default_page(&mut tabs).load_html_str("<p>hi</p>", None);
        let mut generation = 0u64;
        run_session_with_script_requests(
            &mut tabs,
            &mut server,
            &dir,
            &mut generation,
            &gatekeeper,
            None,
        )
        .unwrap();
        dir
    });
    handshake(&mut client);
    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn run_session_with_script_and_debugger_requests_handshakes_and_shuts_down() {
    let dir = temp_frame_dir("wrapper-script-and-debugger");
    let gatekeeper = clearing_gatekeeper("wrapper-script-and-debugger");
    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        default_page(&mut tabs).load_html_str("<p>hi</p>", None);
        let mut generation = 0u64;
        run_session_with_script_and_debugger_requests(
            &mut tabs,
            &mut server,
            &dir,
            &mut generation,
            &gatekeeper,
            None,
            None,
        )
        .unwrap();
        dir
    });
    handshake(&mut client);
    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn run_session_with_core_session_requests_handshakes_and_shuts_down() {
    let dir = temp_frame_dir("wrapper-core-session-requests");
    let gatekeeper = clearing_gatekeeper("wrapper-core-session-requests");
    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        default_page(&mut tabs).load_html_str("<p>hi</p>", None);
        let mut generation = 0u64;
        run_session_with_core_session_requests(
            &mut tabs,
            &mut server,
            &dir,
            &mut generation,
            &gatekeeper,
            CoreSessionRequests::default(),
        )
        .unwrap();
        dir
    });
    handshake(&mut client);
    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn run_session_with_script_and_debugger_requests_and_inline_page_executor_handshakes_and_shuts_down(
) {
    let dir = temp_frame_dir("wrapper-inline-page-executor");
    let gatekeeper = clearing_gatekeeper("wrapper-inline-page-executor");
    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        default_page(&mut tabs).load_html_str("<p>hi</p>", None);
        let mut generation = 0u64;
        run_session_with_script_and_debugger_requests_and_inline_page_executor(
            &mut tabs,
            &mut server,
            &dir,
            &mut generation,
            &gatekeeper,
            CoreSessionRequests::default(),
            None,
        )
        .unwrap();
        dir
    });
    handshake(&mut client);
    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn run_session_with_script_and_debugger_requests_and_out_of_process_javascript_executor_handshakes_and_shuts_down(
) {
    let dir = temp_frame_dir("wrapper-out-of-process-executor");
    let gatekeeper = clearing_gatekeeper("wrapper-out-of-process-executor");
    let (mut client, mut server) = client_pair();
    let handle = thread::spawn(move || {
        let mut tabs = TabManager::new(320.0, 200.0);
        default_page(&mut tabs).load_html_str("<p>hi</p>", None);
        let mut generation = 0u64;
        run_session_with_script_and_debugger_requests_and_out_of_process_javascript_executor(
            &mut tabs,
            &mut server,
            &dir,
            &mut generation,
            &gatekeeper,
            CoreSessionRequests::default(),
            None,
        )
        .unwrap();
        dir
    });
    handshake(&mut client);
    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let dir = handle.join().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}
