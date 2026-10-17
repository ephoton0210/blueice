// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;
use blueice_ipc::debugger::{
    DebuggerErrorCode, DebuggerGeneratedBreakpointTarget, DebuggerMetadataCapabilityManifest,
    DebuggerMetadataCapabilitySelection, DebuggerReply, DebuggerRequest,
};
use blueice_ipc::page_host::{self, PageHostErrorCode, PageHostReply, PageHostRequest};
use std::sync::atomic::{AtomicUsize, Ordering};

fn spawn_map_fault_child() -> (PathBuf, String, Arc<AtomicUsize>, thread::JoinHandle<()>) {
    let path =
        PathBuf::from("/tmp").join(format!("blueice-oop-map-fault-{}.sock", std::process::id()));
    let token = "0123456789abcdef0123456789abcdef".to_string();
    let listener = blueice_launcher::bluejs_host::bind_bluejs_host_socket(&path).unwrap();
    let faults = Arc::new(AtomicUsize::new(0));
    let child_faults = Arc::clone(&faults);
    let child_token = token.clone();
    let child = thread::spawn(move || {
        let mut host = blueice_launcher::bluejs_host::BlueJsChildHost::default();
        for incoming in listener.incoming() {
            let mut stream = incoming.unwrap();
            let hello = page_host::read_page_host_request(&mut stream).unwrap();
            let reply = page_host::negotiate(&hello, &child_token);
            let accepted = matches!(reply, PageHostReply::HelloAck { .. });
            page_host::write_page_host_reply(&mut stream, &reply).unwrap();
            if !accepted {
                continue;
            }
            loop {
                let request = match page_host::read_page_host_request(&mut stream) {
                    Ok(request) => request,
                    Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => break,
                    Err(error) => panic!("private child request failed: {error}"),
                };
                let shutdown = matches!(&request, PageHostRequest::Shutdown);
                let reply = if matches!(
                    &request,
                    PageHostRequest::ResolveDebuggerEmittedOriginalPosition { .. }
                        | PageHostRequest::ResolveDebuggerEmittedSafePoint { .. }
                        | PageHostRequest::GetDebuggerEmittedPausedRootLocation { .. }
                ) {
                    child_faults.fetch_add(1, Ordering::SeqCst);
                    PageHostReply::Error {
                        code: PageHostErrorCode::HostFailure,
                        message: "private map decode failed".to_string(),
                    }
                    .into()
                } else {
                    host.handle_reply(request)
                };
                reply.write(&mut stream).unwrap();
                if shutdown {
                    return;
                }
            }
        }
    });
    (path, token, faults, child)
}

#[test]
fn real_public_generated_breakpoint_fails_closed_on_private_map_decode_fault() {
    const PRIVATE_FAULT_TEXT: &str = "private map decode failed";
    let socket_path = unique_socket_path("gmap-core");
    let debugger_path = unique_socket_path("gmap-debugger");
    let frame_dir = std::env::temp_dir().join(format!(
        "blueice-core-generated-map-fault-frames-{}",
        std::process::id()
    ));
    let gatekeeper_path = clearing_gatekeeper("gmap-gk");
    let (child_path, child_token, faults, child) = spawn_map_fault_child();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let page = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0_u8; 1024];
        let _ = stream.read(&mut request);
        let body = "<main>map fault</main><script type=\"application/x-blueice-typescript\">const answer: number = 42;</script>";
        stream
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            )
            .unwrap();
    });
    let mut core = Command::new(env!("CARGO_BIN_EXE_blueice-core"))
        .args([
            "--socket",
            socket_path.to_str().unwrap(),
            "--debugger-socket",
            debugger_path.to_str().unwrap(),
            "--frame-dir",
            frame_dir.to_str().unwrap(),
            "--gatekeeper-socket",
            gatekeeper_path.to_str().unwrap(),
            "--out-of-process-bluejs-socket",
            child_path.to_str().unwrap(),
            "--out-of-process-bluejs-token",
            &child_token,
            "--out-of-process-bluejs-page-script-profile",
            "core-page-http-fixture-v1",
            "--out-of-process-bluejs-emitted-blue-ts-classic",
            "--debugger-generated-breakpoint",
        ])
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    assert!(wait_for(&socket_path, Duration::from_secs(10)));
    assert!(wait_for(&debugger_path, Duration::from_secs(10)));
    let mut browser = connect_with_retry(&socket_path, Duration::from_secs(5)).unwrap();
    blueice_ipc::client_handshake(&mut browser).unwrap();
    let mut debugger = connect_with_retry(&debugger_path, Duration::from_secs(5)).unwrap();
    let grant =
        DebuggerMetadataCapabilityManifest::opaque_selected(DebuggerMetadataCapabilitySelection {
            generated_breakpoint: true,
            ..Default::default()
        });
    assert_eq!(
        debugger_request(
            &mut debugger,
            &DebuggerRequest::Hello {
                protocol_version: blueice_ipc::debugger::DEBUGGER_PROTOCOL_VERSION,
                requested_bounded_values: false,
                requested_metadata_capabilities: grant.clone(),
            },
            PRIVATE_FAULT_TEXT,
        ),
        DebuggerReply::HelloAck {
            protocol_version: blueice_ipc::debugger::DEBUGGER_PROTOCOL_VERSION,
            granted_bounded_values: false,
            granted_metadata_capabilities: grant,
        }
    );
    assert_eq!(
        debugger_request(
            &mut debugger,
            &DebuggerRequest::HoldNextDocument { tab_id: 1 },
            PRIVATE_FAULT_TEXT,
        ),
        DebuggerReply::NextDocumentHoldAcquired { tab_id: 1 }
    );
    blueice_ipc::write_client_message(
        &mut browser,
        &blueice_ipc::ClientMessage::Navigate {
            url: format!("http://{address}"),
        },
    )
    .unwrap();
    assert!(matches!(
        blueice_ipc::read_server_message(&mut browser).unwrap(),
        blueice_ipc::ServerMessage::Navigated { .. }
    ));
    assert!(matches!(
        blueice_ipc::read_server_message(&mut browser).unwrap(),
        blueice_ipc::ServerMessage::FrameReady { .. }
    ));
    let realm = match debugger_request(
        &mut debugger,
        &DebuggerRequest::ListPageRealms,
        PRIVATE_FAULT_TEXT,
    ) {
        DebuggerReply::PageRealms(realms) if realms.len() == 1 => realms[0],
        reply => panic!("expected one real page realm: {reply:?}"),
    };
    let program = match debugger_request(
        &mut debugger,
        &DebuggerRequest::ListPrograms { realm },
        PRIVATE_FAULT_TEXT,
    ) {
        DebuggerReply::Programs(programs) if programs.len() == 1 => programs[0],
        reply => panic!("expected one emitted program: {reply:?}"),
    };
    let target = DebuggerGeneratedBreakpointTarget {
        program,
        generated_line: 0,
        generated_column_utf16: 0,
    };
    for request in [
        DebuggerRequest::ResolveGeneratedBreakpoint { target },
        DebuggerRequest::ArmGeneratedBreakpoint { target },
    ] {
        assert!(matches!(
            debugger_request(&mut debugger, &request, PRIVATE_FAULT_TEXT),
            DebuggerReply::Error {
                code: DebuggerErrorCode::CapabilityUnavailable,
                ..
            }
        ));
    }
    assert_eq!(faults.load(Ordering::SeqCst), 2);
    assert_eq!(
        debugger_request(
            &mut debugger,
            &DebuggerRequest::ListBreakpoints { realm },
            PRIVATE_FAULT_TEXT,
        ),
        DebuggerReply::Breakpoints(Vec::new())
    );
    blueice_ipc::write_client_message(&mut browser, &blueice_ipc::ClientMessage::Shutdown).unwrap();
    assert!(core.wait().unwrap().success());
    shutdown_private_bluejs_host(&child_path, &child_token);
    child.join().unwrap();
    page.join().unwrap();
    let _ = std::fs::remove_file(&child_path);
    assert!(!socket_path.exists());
    assert!(!debugger_path.exists());
    assert!(!frame_dir.exists());
}
