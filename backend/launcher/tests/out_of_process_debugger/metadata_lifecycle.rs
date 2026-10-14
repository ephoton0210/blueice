// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Public metadata invalidation after a browser tab closes.

use super::*;

#[test]
fn launcher_discards_receipted_bluets_metadata_when_its_tab_closes() {
    let gatekeeper_socket = clearing_gatekeeper();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let fixture = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0_u8; 1024];
        let _ = stream.read(&mut request);
        let body = "<script type=\"application/x-blueice-typescript\">const rootValue: number = 9;</script>";
        stream
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            )
            .unwrap();
    });
    let mut launcher = LauncherProcess::spawn_with_static_metadata_policy(
        &gatekeeper_socket,
        StaticMetadataPolicy {
            inventory: true,
            type_inventory: true,
            type_display: true,
            ..Default::default()
        },
    );
    let mut browser = launcher.connect_browser();
    browser
        .set_read_timeout(Some(Duration::from_secs(15)))
        .unwrap();
    blueice_ipc::client_handshake(&mut browser).unwrap();
    let manifest =
        DebuggerMetadataCapabilityManifest::opaque_selected(DebuggerMetadataCapabilitySelection {
            type_display: true,
            ..Default::default()
        });
    let mut debugger = UnixStream::connect(&launcher.debugger_socket).unwrap();
    debugger
        .set_read_timeout(Some(Duration::from_secs(15)))
        .unwrap();
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::Hello {
                protocol_version: DEBUGGER_PROTOCOL_VERSION,
                requested_bounded_values: false,
                requested_metadata_capabilities: manifest.clone(),
            },
        ),
        DebuggerReply::HelloAck {
            protocol_version: DEBUGGER_PROTOCOL_VERSION,
            granted_bounded_values: false,
            granted_metadata_capabilities: manifest,
        }
    );
    navigate(&mut browser, &url);
    fixture.join().unwrap();
    let realm = one_realm(debugger_request(
        &mut debugger,
        DebuggerRequest::ListPageRealms,
    ));
    let program = one_program(
        debugger_request(&mut debugger, DebuggerRequest::ListPrograms { realm }),
        realm,
    );
    let DebuggerReply::StaticMetadata(metadata) = debugger_request(
        &mut debugger,
        DebuggerRequest::ListStaticMetadata { program },
    ) else {
        panic!("live BlueTS tab must expose metadata")
    };
    assert_eq!(metadata.len(), 1);
    let metadata = metadata[0];
    let DebuggerReply::StaticMetadataTypes(types) = debugger_request(
        &mut debugger,
        DebuggerRequest::ListStaticMetadataTypes { metadata },
    ) else {
        panic!("live tab must expose its receipted compiler types")
    };
    let static_type = types
        .into_iter()
        .find(|static_type| {
            matches!(
                debugger_request(
                    &mut debugger,
                    DebuggerRequest::DescribeStaticMetadataType {
                        static_type: *static_type,
                    },
                ),
                DebuggerReply::StaticMetadataType(display) if display.display == "number"
            )
        })
        .expect("live root binding must have a compiler number type");

    write_client_message(&mut browser, &ClientMessage::CloseTab).unwrap();
    assert_eq!(
        read_server_message(&mut browser).unwrap(),
        ServerMessage::TabClosed {
            tab_id: realm.tab_id,
        }
    );
    assert_eq!(
        debugger_request(&mut debugger, DebuggerRequest::ListPageRealms),
        DebuggerReply::PageRealms(Vec::new())
    );
    for request in [
        DebuggerRequest::ListStaticMetadata { program },
        DebuggerRequest::ListStaticMetadataTypes { metadata },
        DebuggerRequest::DescribeStaticMetadataType { static_type },
    ] {
        let reply = debugger_request(&mut debugger, request.clone());
        assert!(
            matches!(
                reply,
                DebuggerReply::Error {
                    code: DebuggerErrorCode::StaleRealm | DebuggerErrorCode::InvalidTarget,
                    ..
                }
            ),
            "closed tab metadata request {request:?} returned {reply:?}"
        );
    }
    drop(debugger);
    launcher.shutdown();
    let _ = std::fs::remove_file(gatekeeper_socket);
}
