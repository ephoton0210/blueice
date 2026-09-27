// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Real public-socket isolation of one owner-authorized source graph across tabs.

use super::*;

const DOCUMENT: &str =
    "<script type=\"application/x-blueice-typescript-module\" src=\"/approved.ts\"></script>";
const SOURCE: &str = "export const approved: number = 41;";
const INTEGRITY: &str = "sha256:1abff90bfd5bbbe7f1198af009bd97700574504ab881755f653670406a85036a";

fn serve_policy_origin(
    listener: TcpListener,
    stop: mpsc::Receiver<()>,
) -> thread::JoinHandle<Vec<String>> {
    listener.set_nonblocking(true).unwrap();
    thread::spawn(move || {
        let mut requested = Vec::new();
        while matches!(stop.try_recv(), Err(mpsc::TryRecvError::Empty)) {
            let Ok((mut stream, _)) = listener.accept() else {
                thread::sleep(Duration::from_millis(10));
                continue;
            };
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = [0u8; 2048];
            let count = stream.read(&mut request).unwrap();
            let path = std::str::from_utf8(&request[..count])
                .unwrap()
                .split_whitespace()
                .nth(1)
                .unwrap()
                .to_string();
            let (mime, body) = match path.as_str() {
                "/" => ("text/html", DOCUMENT),
                "/approved.ts" => ("text/typescript", SOURCE),
                other => panic!("unexpected policy fixture resource {other}"),
            };
            requested.push(path);
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                )
                .unwrap();
        }
        requested
    })
}

#[test]
fn public_tabs_cannot_borrow_another_origins_child_source_authorization() {
    let gatekeeper_socket = clearing_gatekeeper();
    let first_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let first_origin = format!("http://{}", first_listener.local_addr().unwrap());
    let second_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let second_origin = format!("http://{}", second_listener.local_addr().unwrap());
    let (first_stop, first_stopped) = mpsc::channel();
    let (second_stop, second_stopped) = mpsc::channel();
    let first_fixture = serve_policy_origin(first_listener, first_stopped);
    let second_fixture = serve_policy_origin(second_listener, second_stopped);
    let policy = OwnerHttpPolicyBootstrap {
        origin_rule: OwnerHttpOriginRule::SameDocumentOrigin,
        resources: vec![OwnerHttpResource {
            canonical_url: format!("{first_origin}/approved.ts"),
            integrity: INTEGRITY.into(),
        }],
    };
    let policy_file = unique_path("tab-source-policy");
    std::fs::write(&policy_file, serde_json::to_vec(&policy).unwrap()).unwrap();
    let mut launcher = LauncherProcess::spawn_with_owner_http_policy(
        &gatekeeper_socket,
        StaticMetadataPolicy::default(),
        Some(&policy_file),
    );
    let mut browser = launcher.connect_browser();
    blueice_ipc::client_handshake(&mut browser).unwrap();
    navigate(&mut browser, &format!("{first_origin}/"));
    write_client_message(
        &mut browser,
        &ClientMessage::OpenTab {
            url: Some(format!("{second_origin}/")),
        },
    )
    .unwrap();
    let second_tab = match read_server_message(&mut browser).unwrap() {
        ServerMessage::TabOpened { tab_id, .. } => tab_id,
        reply => panic!("the second public tab must open: {reply:?}"),
    };
    assert_ne!(second_tab, 1);
    assert!(matches!(
        read_server_message(&mut browser).unwrap(),
        ServerMessage::FrameReady { .. }
    ));

    write_client_message(&mut browser, &ClientMessage::GetBlueTsScriptReports).unwrap();
    let (reply_tab, _, reply) = blueice_ipc::read_server_message_with_ids(&mut browser).unwrap();
    assert_eq!(reply_tab, Some(1));
    assert_eq!(
        reply,
        ServerMessage::BlueTsScriptReports(vec![blueice_ipc::BlueTsScriptExecutionReport {
            tab_id: 1,
            document_generation: 1,
            ordinal: 0,
            kind: blueice_ipc::BlueTsScriptKind::Module,
            outcome: blueice_ipc::BlueTsScriptExecutionOutcome::Executed,
        }])
    );
    blueice_ipc::write_client_message_with_ids(
        &mut browser,
        Some(second_tab),
        None,
        &ClientMessage::GetBlueTsScriptReports,
    )
    .unwrap();
    let (reply_tab, _, reply) = blueice_ipc::read_server_message_with_ids(&mut browser).unwrap();
    assert_eq!(reply_tab, Some(second_tab));
    assert_eq!(
        reply,
        ServerMessage::BlueTsScriptReports(vec![blueice_ipc::BlueTsScriptExecutionReport {
            tab_id: second_tab,
            document_generation: 1,
            ordinal: 0,
            kind: blueice_ipc::BlueTsScriptKind::Module,
            outcome: blueice_ipc::BlueTsScriptExecutionOutcome::Rejected {
                category: "external BlueTS source authorization rejected the page script".into(),
            },
        }])
    );

    // A supervised core/BlueJS child cutover must not let the second tab use
    // the first tab's owner-authorized URL in the successor process either.
    let mut control = UnixStream::connect(&launcher.control_socket).unwrap();
    write_control_request(&mut control, &ControlRequest::Cutover).unwrap();
    assert_eq!(
        read_control_reply(&mut control).unwrap(),
        ControlReply::CutoverDone { tabs_migrated: 2 }
    );
    drop(browser);
    let mut successor = launcher.connect_browser();
    blueice_ipc::client_handshake(&mut successor).unwrap();
    navigate(&mut successor, &format!("{first_origin}/"));
    write_client_message(&mut successor, &ClientMessage::GetBlueTsScriptReports).unwrap();
    let (reply_tab, _, reply) = blueice_ipc::read_server_message_with_ids(&mut successor).unwrap();
    assert_eq!(reply_tab, Some(1));
    assert!(matches!(
        reply,
        ServerMessage::BlueTsScriptReports(reports)
            if reports.iter().any(|report|
                report.tab_id == 1
                    && report.outcome == blueice_ipc::BlueTsScriptExecutionOutcome::Executed)
    ));
    blueice_ipc::write_client_message_with_ids(
        &mut successor,
        Some(second_tab),
        None,
        &ClientMessage::Navigate {
            url: format!("{second_origin}/"),
        },
    )
    .unwrap();
    assert!(matches!(
        blueice_ipc::read_server_message_with_ids(&mut successor).unwrap(),
        (Some(tab), _, ServerMessage::Navigated { .. }) if tab == second_tab
    ));
    assert!(matches!(
        blueice_ipc::read_server_message_with_ids(&mut successor).unwrap(),
        (Some(tab), _, ServerMessage::FrameReady { .. }) if tab == second_tab
    ));
    blueice_ipc::write_client_message_with_ids(
        &mut successor,
        Some(second_tab),
        None,
        &ClientMessage::GetBlueTsScriptReports,
    )
    .unwrap();
    let (reply_tab, _, reply) = blueice_ipc::read_server_message_with_ids(&mut successor).unwrap();
    assert_eq!(reply_tab, Some(second_tab));
    assert!(matches!(
        reply,
        ServerMessage::BlueTsScriptReports(reports)
            if reports.iter().any(|report|
                report.tab_id == second_tab
                    && matches!(&report.outcome,
                        blueice_ipc::BlueTsScriptExecutionOutcome::Rejected { category }
                            if category == "external BlueTS source authorization rejected the page script"))
    ));

    drop(successor);
    launcher.shutdown();
    first_stop.send(()).unwrap();
    second_stop.send(()).unwrap();
    let first_requests = first_fixture.join().unwrap();
    assert!(first_requests.iter().any(|path| path == "/approved.ts"));
    assert!(first_requests
        .iter()
        .all(|path| path == "/" || path == "/approved.ts"));
    let second_requests = second_fixture.join().unwrap();
    assert!(second_requests.len() >= 2);
    assert!(second_requests.iter().all(|path| path == "/"));
    let _ = std::fs::remove_file(policy_file);
    let _ = std::fs::remove_file(gatekeeper_socket);
}
