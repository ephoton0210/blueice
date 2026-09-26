// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![cfg(unix)]

//! Exercises the actual compiled `blueice-core` binary as a real
//! subprocess -- `main`'s own argument parsing, socket binding,
//! accept, and shutdown/cleanup wiring, none of which
//! `src/bin/blueice-core.rs`'s unit tests touch (those cover
//! `parse_args` in isolation; `blueice_engine::session`'s own tests
//! cover the message loop over an in-process pipe). This is the
//! "e2e test through the real public interface" the project's
//! Definition of Done asks for applied to a binary rather than a
//! library: `Command::new(env!("CARGO_BIN_EXE_blueice-core"))`, not a
//! function call, is the real public interface here. Manually running
//! this exact binary under a real windowed frontend (WSLg) additionally
//! confirmed the end-to-end pixels look right; this test covers the
//! process-lifecycle contract repeatably in CI, which a manual run
//! can't.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
#[path = "core_binary/compiler_debugger.rs"]
mod compiler_debugger;
#[path = "core_binary/core_session.rs"]
mod core_session;
#[path = "core_binary/multi_tab.rs"]
mod multi_tab;
#[path = "core_binary/page_execution.rs"]
mod page_execution;
#[path = "core_binary/root_control.rs"]
mod root_control;

fn spawn_private_bluejs_host(label: &str) -> (PathBuf, String, thread::JoinHandle<()>) {
    // The default macOS temporary directory can exceed the Unix-domain socket
    // pathname limit once this test's descriptive filename is appended.
    let path =
        PathBuf::from("/tmp").join(format!("blueice-oop-{label}-{}.sock", std::process::id()));
    let token = "0123456789abcdef0123456789abcdef".to_string();
    let listener = blueice_launcher::bluejs_host::bind_bluejs_host_socket(&path)
        .expect("test child host must bind its private socket");
    let child_token = token.clone();
    let child = thread::spawn(move || {
        let mut host = blueice_launcher::bluejs_host::BlueJsChildHost::default();
        blueice_launcher::bluejs_host::serve_bluejs_host_listener(listener, child_token, &mut host)
            .expect("test child host must serve its private protocol");
    });
    (path, token, child)
}

fn shutdown_private_bluejs_host(path: &std::path::Path, token: &str) {
    let mut stream =
        UnixStream::connect(path).expect("must connect to test child host for shutdown");
    blueice_ipc::page_host::write_page_host_request(
        &mut stream,
        &blueice_ipc::page_host::PageHostRequest::Hello {
            protocol_version: blueice_ipc::page_host::PAGE_HOST_PROTOCOL_VERSION,
            session_token: token.to_string(),
        },
    )
    .expect("must send child-host shutdown handshake");
    assert!(matches!(
        blueice_ipc::page_host::read_page_host_reply(&mut stream)
            .expect("child host must acknowledge shutdown handshake"),
        blueice_ipc::page_host::PageHostReply::HelloAck { .. }
    ));
    blueice_ipc::page_host::write_page_host_request(
        &mut stream,
        &blueice_ipc::page_host::PageHostRequest::Shutdown,
    )
    .expect("must request child-host shutdown");
    assert_eq!(
        blueice_ipc::page_host::read_page_host_reply(&mut stream)
            .expect("child host must acknowledge shutdown"),
        blueice_ipc::page_host::PageHostReply::ShutdownAck
    );
}

fn unique_socket_path(label: &str) -> PathBuf {
    // macOS may supply a long TMPDIR; keep the basename short enough for
    // sockaddr_un while preserving the test label and process uniqueness.
    std::env::temp_dir().join(format!("bicb-{label}-{}.sock", std::process::id()))
}

/// A gated `Navigate`/`OpenTab{url}` sent to the real subprocess needs
/// *some* `ai-gatekeeper` behind the `--gatekeeper-socket` path it's
/// given -- a genuinely unreachable one fails closed
/// (`phase-7-local-ai/PLAN.md`'s "Wiring design"), which would turn
/// this file's pre-existing "navigation always succeeds" assertions
/// false. Spins up a real listener running the actual minimal-slice
/// stub logic (`blueice_ai_gatekeeper::handle_one_check`, always
/// clears) bound to a fresh path unique to this call, mirroring
/// `blueice_engine::session`'s own test-module helper of the same
/// name/purpose.
fn clearing_gatekeeper(label: &str) -> PathBuf {
    let path = unique_socket_path(label);
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

fn wait_for(path: &std::path::Path, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if path.exists() {
            return true;
        }
        thread::sleep(Duration::from_millis(20));
    }
    false
}

fn connect_with_retry(path: &std::path::Path, timeout: Duration) -> std::io::Result<UnixStream> {
    let deadline = Instant::now() + timeout;
    loop {
        match UnixStream::connect(path) {
            Ok(stream) => return Ok(stream),
            Err(_) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
            Err(error) => return Err(error),
        }
    }
}

fn serve_html_once(body: &'static str) -> (std::net::SocketAddr, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0u8; 1024];
        let _ = stream.read(&mut request);
        stream
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            )
            .unwrap();
    });
    (address, server)
}

fn read_tab_dom(frontend: &mut UnixStream, tab_id: u64, request_id: u64) -> String {
    blueice_ipc::write_client_message_with_ids(
        frontend,
        Some(tab_id),
        Some(request_id),
        &blueice_ipc::ClientMessage::GetDom,
    )
    .unwrap();
    loop {
        let (reply_tab, reply_id, reply) =
            blueice_ipc::read_server_message_with_ids(frontend).unwrap();
        if reply_id != Some(request_id) {
            assert!(matches!(
                reply,
                blueice_ipc::ServerMessage::FrameReady { .. }
            ));
            continue;
        }
        assert_eq!(reply_tab, Some(tab_id));
        let blueice_ipc::ServerMessage::Dom(dom) = reply else {
            panic!("expected a DOM dump, got {reply:?}");
        };
        return dom;
    }
}

fn script_exchange(
    script: &mut UnixStream,
    next_call_id: &mut u64,
    request: blueice_ipc::script::ScriptRequest,
) -> blueice_ipc::script::ScriptReply {
    use blueice_ipc::script::{ScriptReply, ScriptRequest};
    let Some(target) = request.document_target() else {
        assert!(matches!(request, ScriptRequest::Hello { .. }));
        blueice_ipc::script::write_script_request(script, &request).unwrap();
        return blueice_ipc::script::read_script_reply(script).unwrap();
    };
    let request_id = *next_call_id;
    *next_call_id = next_call_id.checked_add(1).unwrap();
    blueice_ipc::script::write_script_request(
        script,
        &ScriptRequest::Call {
            request_id,
            request: Box::new(request),
        },
    )
    .unwrap();
    match blueice_ipc::script::read_script_reply(script).unwrap() {
        ScriptReply::CallResult {
            request_id: actual_id,
            target: actual_target,
            reply,
        } if actual_id == request_id && actual_target == target => *reply,
        reply => panic!("script reply must echo call ID and document target: {reply:?}"),
    }
}

fn navigate_default_tab(frontend: &mut UnixStream, url: String) {
    blueice_ipc::write_client_message(
        frontend,
        &blueice_ipc::ClientMessage::Navigate { url: url.clone() },
    )
    .unwrap();
    loop {
        let reply = blueice_ipc::read_server_message(frontend)
            .unwrap_or_else(|error| panic!("navigation to {url} failed: {error}"));
        match reply {
            blueice_ipc::ServerMessage::Navigated { url: navigated } => {
                assert_eq!(navigated, url);
                break;
            }
            blueice_ipc::ServerMessage::FrameReady { .. } => {}
            other => panic!("unexpected navigation reply: {other:?}"),
        }
    }
    assert!(matches!(
        blueice_ipc::read_server_message(frontend).unwrap(),
        blueice_ipc::ServerMessage::FrameReady { .. }
    ));
}

/// Sends one complete native-debugger request/response pair over the real
/// socket and asserts that the raw public reply has not reflected this
/// fixture's page-controlled secret, a VM/completion representation, or a
/// known BlueJS opcode before decoding it. The protocol intentionally permits
/// an opaque instruction *offset*; that is not bytecode disclosure.
fn debugger_request(
    stream: &mut UnixStream,
    request: &blueice_ipc::debugger::DebuggerRequest,
    page_secret: &str,
) -> blueice_ipc::debugger::DebuggerReply {
    blueice_ipc::debugger::write_debugger_request(stream, request)
        .expect("must write debugger request to real core socket");
    let mut length = [0u8; 4];
    stream
        .read_exact(&mut length)
        .expect("real core must write a debugger reply length");
    let mut payload = vec![0u8; u32::from_le_bytes(length) as usize];
    stream
        .read_exact(&mut payload)
        .expect("real core must write a complete debugger reply payload");
    let rendered =
        std::str::from_utf8(&payload).expect("debugger reply framing must contain UTF-8 JSON");
    assert!(
        !rendered.contains(page_secret),
        "debugger reply must not disclose page source or its completion value: {rendered}"
    );
    assert!(
        !rendered.contains("Value(")
            && !rendered.contains("StoreBinding")
            && !rendered.contains("\"value\"")
            && !rendered.contains("\"completion\"")
            && !rendered.contains("\"opcode\""),
        "debugger reply must not disclose a VM value, completion payload, or bytecode opcode: {rendered}"
    );
    serde_json::from_slice(&payload).expect("real core must return a debugger reply JSON shape")
}

fn wait_for_debugger_state(
    stream: &mut UnixStream,
    program: blueice_ipc::debugger::DebuggerProgram,
    state: blueice_ipc::debugger::DebuggerExecutionState,
    page_secret: &str,
) {
    let expected = blueice_ipc::debugger::DebuggerReply::ExecutionState { program, state };
    let mut observed = None;
    for _ in 0..20 {
        let reply = debugger_request(
            stream,
            &blueice_ipc::debugger::DebuggerRequest::GetExecutionState { program },
            page_secret,
        );
        if reply == expected {
            return;
        }
        observed = Some(reply);
        thread::sleep(Duration::from_millis(50));
    }
    panic!("expected debugger state {expected:?}, last observed {observed:?}");
}

trait WaitTimeoutOrKill {
    fn wait_timeout_or_kill(&mut self) -> std::process::ExitStatus;
}

impl WaitTimeoutOrKill for std::process::Child {
    fn wait_timeout_or_kill(&mut self) -> std::process::ExitStatus {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(status) = self.try_wait().expect("failed to poll child status") {
                return status;
            }
            if Instant::now() >= deadline {
                let _ = self.kill();
                panic!("blueice-core did not exit within the timeout");
            }
            thread::sleep(Duration::from_millis(20));
        }
    }
}
