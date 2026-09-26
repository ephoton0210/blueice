// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn real_subprocess_executes_an_opted_in_inline_bluets_profile_and_reports_source_free_outcomes() {
    // This proves the process seam, rather than just the in-process runner:
    // parsed classic/module declarations travel through real HTTP navigation,
    // the core-owned profile executes them before the navigation reply, and
    // the frontend can observe only bounded outcome metadata.
    let socket_path = unique_socket_path("inline-bluets");
    let frame_dir = std::env::temp_dir().join(format!(
        "blueice-core-binary-test-inline-bluets-frames-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&socket_path);
    let _ = std::fs::remove_dir_all(&frame_dir);
    let gatekeeper_path = clearing_gatekeeper("ib-gk");

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = [0u8; 1024];
        let _ = stream.read(&mut buf);
        let body = concat!(
            "<main>inline BlueTS process proof</main>",
            "<script type=\"application/x-blueice-typescript\">",
            "blueiceDocumentText();",
            "</script>",
            "<script type=\"application/x-blueice-typescript-module\">",
            "blueiceDocumentText();",
            "</script>",
            "<script type=\"application/x-blueice-typescript\">",
            "blueiceDocumentText(1);",
            "</script>"
        );
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

    let mut child = Command::new(env!("CARGO_BIN_EXE_blueice-core"))
        .args([
            "--socket",
            socket_path.to_str().unwrap(),
            "--frame-dir",
            frame_dir.to_str().unwrap(),
            "--gatekeeper-socket",
            gatekeeper_path.to_str().unwrap(),
            "--inline-bluets-profile",
            "core-script-document-text-v1",
        ])
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn blueice-core");

    assert!(
        wait_for(&socket_path, Duration::from_secs(5)),
        "blueice-core never created its socket"
    );
    let mut stream = connect_with_retry(&socket_path, Duration::from_secs(5))
        .expect("failed to connect to the real subprocess");
    blueice_ipc::client_handshake(&mut stream)
        .expect("the real subprocess must complete the protocol_version handshake");

    let url = format!("http://{addr}");
    blueice_ipc::write_client_message(
        &mut stream,
        &blueice_ipc::ClientMessage::Navigate { url: url.clone() },
    )
    .unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut stream).unwrap(),
        blueice_ipc::ServerMessage::Navigated { url }
    );
    assert!(matches!(
        blueice_ipc::read_server_message(&mut stream).unwrap(),
        blueice_ipc::ServerMessage::FrameReady { generation: 1, .. }
    ));

    blueice_ipc::write_client_message(
        &mut stream,
        &blueice_ipc::ClientMessage::GetBlueTsScriptReports,
    )
    .unwrap();
    let reports = match blueice_ipc::read_server_message(&mut stream).unwrap() {
        blueice_ipc::ServerMessage::BlueTsScriptReports(reports) => reports,
        other => panic!("expected BlueTsScriptReports, got {other:?}"),
    };
    assert_eq!(
        reports,
        vec![
            blueice_ipc::BlueTsScriptExecutionReport {
                tab_id: 1,
                document_generation: 1,
                ordinal: 0,
                kind: blueice_ipc::BlueTsScriptKind::Classic,
                outcome: blueice_ipc::BlueTsScriptExecutionOutcome::Executed,
            },
            blueice_ipc::BlueTsScriptExecutionReport {
                tab_id: 1,
                document_generation: 1,
                ordinal: 1,
                kind: blueice_ipc::BlueTsScriptKind::Module,
                outcome: blueice_ipc::BlueTsScriptExecutionOutcome::Executed,
            },
            blueice_ipc::BlueTsScriptExecutionReport {
                tab_id: 1,
                document_generation: 1,
                ordinal: 2,
                kind: blueice_ipc::BlueTsScriptKind::Classic,
                outcome: blueice_ipc::BlueTsScriptExecutionOutcome::Rejected {
                    category: "BlueTS compilation rejected the page script".to_string(),
                },
            },
        ]
    );
    assert!(reports.iter().all(|report| match &report.outcome {
        blueice_ipc::BlueTsScriptExecutionOutcome::Executed => true,
        blueice_ipc::BlueTsScriptExecutionOutcome::Rejected { category } => {
            !category.contains("blueiceDocumentText(1)")
        }
    }));

    blueice_ipc::write_client_message(&mut stream, &blueice_ipc::ClientMessage::Shutdown).unwrap();
    let status = child
        .wait()
        .expect("failed to wait for blueice-core to exit");
    assert!(
        status.success(),
        "blueice-core must exit cleanly after Shutdown"
    );
    assert!(!socket_path.exists());
    assert!(!frame_dir.exists());
}

#[test]
fn real_subprocess_executes_opted_in_standard_javascript_and_reports_source_free_outcomes() {
    // This is the Phase 13 page-host process proof: ordinary HTML scripts
    // travel through real HTTP navigation into the compiled core binary, share
    // its tab/document realm lifecycle, and expose only bounded outcomes.
    let socket_path = unique_socket_path("inline-bluejs");
    let frame_dir = std::env::temp_dir().join(format!(
        "blueice-core-binary-test-inline-bluejs-frames-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&socket_path);
    let _ = std::fs::remove_dir_all(&frame_dir);
    let gatekeeper_path = clearing_gatekeeper("ij-gk");

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = [0u8; 1024];
        let _ = stream.read(&mut buf);
        let body = concat!(
            "<main>inline JavaScript process proof</main>",
            "<script>blueiceDocumentText(); blueiceDocumentOrigin();</script>",
            "<script type=\"module\">export const moduleAnswer = 43;</script>",
            "<script>const = malformed;</script>"
        );
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

    let mut child = Command::new(env!("CARGO_BIN_EXE_blueice-core"))
        .args([
            "--socket",
            socket_path.to_str().unwrap(),
            "--frame-dir",
            frame_dir.to_str().unwrap(),
            "--gatekeeper-socket",
            gatekeeper_path.to_str().unwrap(),
            "--inline-bluejs",
        ])
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn blueice-core");

    assert!(wait_for(&socket_path, Duration::from_secs(5)));
    let mut stream = connect_with_retry(&socket_path, Duration::from_secs(5)).unwrap();
    blueice_ipc::client_handshake(&mut stream).unwrap();
    let url = format!("http://{addr}");
    blueice_ipc::write_client_message(
        &mut stream,
        &blueice_ipc::ClientMessage::Navigate { url: url.clone() },
    )
    .unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut stream).unwrap(),
        blueice_ipc::ServerMessage::Navigated { url }
    );
    assert!(matches!(
        blueice_ipc::read_server_message(&mut stream).unwrap(),
        blueice_ipc::ServerMessage::FrameReady { generation: 1, .. }
    ));

    blueice_ipc::write_client_message(
        &mut stream,
        &blueice_ipc::ClientMessage::GetBlueJsScriptReports,
    )
    .unwrap();
    let reports = match blueice_ipc::read_server_message(&mut stream).unwrap() {
        blueice_ipc::ServerMessage::BlueJsScriptReports(reports) => reports,
        other => panic!("expected BlueJsScriptReports, got {other:?}"),
    };
    assert_eq!(
        reports,
        vec![
            blueice_ipc::BlueJsScriptExecutionReport {
                tab_id: 1,
                document_generation: 1,
                ordinal: 0,
                kind: blueice_ipc::BlueJsScriptKind::Classic,
                outcome: blueice_ipc::BlueJsScriptExecutionOutcome::Executed,
            },
            blueice_ipc::BlueJsScriptExecutionReport {
                tab_id: 1,
                document_generation: 1,
                ordinal: 1,
                kind: blueice_ipc::BlueJsScriptKind::Module,
                outcome: blueice_ipc::BlueJsScriptExecutionOutcome::Executed,
            },
            blueice_ipc::BlueJsScriptExecutionReport {
                tab_id: 1,
                document_generation: 1,
                ordinal: 2,
                kind: blueice_ipc::BlueJsScriptKind::Classic,
                outcome: blueice_ipc::BlueJsScriptExecutionOutcome::Rejected {
                    category: "JavaScript parsing rejected the page script".to_string(),
                },
            },
        ]
    );
    assert!(reports.iter().all(|report| match &report.outcome {
        blueice_ipc::BlueJsScriptExecutionOutcome::Executed => true,
        blueice_ipc::BlueJsScriptExecutionOutcome::Rejected { category } => {
            !category.contains("const = malformed")
        }
    }));

    blueice_ipc::write_client_message(&mut stream, &blueice_ipc::ClientMessage::Shutdown).unwrap();
    assert!(child.wait().unwrap().success());
    assert!(!socket_path.exists());
    assert!(!frame_dir.exists());
}

#[test]
fn real_subprocess_routes_an_explicit_page_lifecycle_to_the_private_bluejs_host() {
    // Exercise the production core binary on its explicit child-host route.
    // The server is the launcher's real child-host state machine; unlike the
    // regular in-process `--inline-bluejs` fixture, core reaches it only over
    // the capability-authenticated page-host protocol.
    let socket_path = unique_socket_path("oopj");
    let frame_dir = std::env::temp_dir().join(format!(
        "blueice-core-binary-test-out-of-process-bluejs-frames-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&socket_path);
    let _ = std::fs::remove_dir_all(&frame_dir);
    let gatekeeper_path = clearing_gatekeeper("oopj-gk");
    let (host_socket, host_token, host) = spawn_private_bluejs_host("out-of-process-host");

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = [0u8; 1024];
        let _ = stream.read(&mut buf);
        let body = concat!(
            "<main>out-of-process JavaScript process proof</main>",
            "<script>globalThis.answer = 42;</script>",
            "<script type=\"module\">export const moduleAnswer = 43;</script>",
            "<script src=\"untrusted.js\"></script>"
        );
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
            "--frame-dir",
            frame_dir.to_str().unwrap(),
            "--gatekeeper-socket",
            gatekeeper_path.to_str().unwrap(),
            "--out-of-process-bluejs-socket",
            host_socket.to_str().unwrap(),
            "--out-of-process-bluejs-token",
            &host_token,
        ])
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn blueice-core");

    assert!(wait_for(&socket_path, Duration::from_secs(5)));
    let mut stream = connect_with_retry(&socket_path, Duration::from_secs(5)).unwrap();
    blueice_ipc::client_handshake(&mut stream).unwrap();
    let url = format!("http://{addr}");
    blueice_ipc::write_client_message(
        &mut stream,
        &blueice_ipc::ClientMessage::Navigate { url: url.clone() },
    )
    .unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut stream).unwrap(),
        blueice_ipc::ServerMessage::Navigated { url }
    );
    assert!(matches!(
        blueice_ipc::read_server_message(&mut stream).unwrap(),
        blueice_ipc::ServerMessage::FrameReady { generation: 1, .. }
    ));

    blueice_ipc::write_client_message(
        &mut stream,
        &blueice_ipc::ClientMessage::GetBlueJsScriptReports,
    )
    .unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut stream).unwrap(),
        blueice_ipc::ServerMessage::BlueJsScriptReports(vec![
            blueice_ipc::BlueJsScriptExecutionReport {
                tab_id: 1,
                document_generation: 1,
                ordinal: 0,
                kind: blueice_ipc::BlueJsScriptKind::Classic,
                outcome: blueice_ipc::BlueJsScriptExecutionOutcome::Executed,
            },
            blueice_ipc::BlueJsScriptExecutionReport {
                tab_id: 1,
                document_generation: 1,
                ordinal: 1,
                kind: blueice_ipc::BlueJsScriptKind::Module,
                outcome: blueice_ipc::BlueJsScriptExecutionOutcome::Executed,
            },
            blueice_ipc::BlueJsScriptExecutionReport {
                tab_id: 1,
                document_generation: 1,
                ordinal: 2,
                kind: blueice_ipc::BlueJsScriptKind::Classic,
                outcome: blueice_ipc::BlueJsScriptExecutionOutcome::Rejected {
                    category: "external JavaScript declarations require an authorized loader"
                        .to_string(),
                },
            },
        ])
    );

    blueice_ipc::write_client_message(&mut stream, &blueice_ipc::ClientMessage::Shutdown).unwrap();
    assert!(core.wait().unwrap().success());
    shutdown_private_bluejs_host(&host_socket, &host_token);
    host.join().unwrap();
    let _ = std::fs::remove_file(&host_socket);
    assert!(!socket_path.exists());
    assert!(!frame_dir.exists());
}

#[test]
fn real_subprocess_installs_the_fixed_core_http_profile_in_the_private_page_host() {
    // This covers the actual production ownership chain: a trusted startup
    // selector reaches core, core constructs its fixed manifest authorizer,
    // and only the resulting closed graph crosses the private page-host
    // protocol. The dynamically chosen loopback port is page data, not a
    // profile input: the compiled policy permits just its fixed path and
    // integrity at the live document's same canonical origin.
    const FIXTURE_PATH: &str = "/.blueice/core-page-http-fixture-v1.js";
    const FIXTURE_SOURCE: &str = "globalThis.corePageHttpFixture = 42;";
    let socket_path = unique_socket_path("ohp");
    let frame_dir = std::env::temp_dir().join(format!(
        "blueice-core-binary-test-oop-http-profile-frames-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&socket_path);
    let _ = std::fs::remove_dir_all(&frame_dir);
    let gatekeeper_path = clearing_gatekeeper("ohp-gk");
    let (host_socket, host_token, host) = spawn_private_bluejs_host("oop-http-profile");

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let requested_paths = Arc::new(Mutex::new(Vec::new()));
    let observed_paths = Arc::clone(&requested_paths);
    let server = thread::spawn(move || {
        for _ in 0..2 {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0u8; 1024];
            let bytes = stream.read(&mut request).unwrap();
            let path = std::str::from_utf8(&request[..bytes])
                .unwrap()
                .lines()
                .next()
                .and_then(|line| line.split_whitespace().nth(1))
                .unwrap()
                .to_string();
            observed_paths.lock().unwrap().push(path.clone());
            let (content_type, body) = match path.as_str() {
                "/app/index.html" => (
                    "text/html",
                    concat!(
                        "<main>core HTTP profile</main>",
                        "<script src=\"/.blueice/core-page-http-fixture-v1.js\"></script>",
                        "<script>if (globalThis.corePageHttpFixture !== 42) throw new Error('fixture');</script>"
                    ),
                ),
                FIXTURE_PATH => ("application/javascript", FIXTURE_SOURCE),
                _ => ("text/plain", "missing fixture resource"),
            };
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                )
                .unwrap();
        }
    });

    let mut core = Command::new(env!("CARGO_BIN_EXE_blueice-core"))
        .args([
            "--socket",
            socket_path.to_str().unwrap(),
            "--frame-dir",
            frame_dir.to_str().unwrap(),
            "--gatekeeper-socket",
            gatekeeper_path.to_str().unwrap(),
            "--out-of-process-bluejs-socket",
            host_socket.to_str().unwrap(),
            "--out-of-process-bluejs-token",
            &host_token,
            "--out-of-process-bluejs-page-script-profile",
            "core-page-http-fixture-v1",
        ])
        .stderr(Stdio::piped())
        .spawn()
        .expect("must spawn core with the fixed HTTP page-script profile");

    assert!(wait_for(&socket_path, Duration::from_secs(5)));
    let mut stream = connect_with_retry(&socket_path, Duration::from_secs(5)).unwrap();
    blueice_ipc::client_handshake(&mut stream).unwrap();
    let url = format!("http://{addr}/app/index.html");
    blueice_ipc::write_client_message(
        &mut stream,
        &blueice_ipc::ClientMessage::Navigate { url: url.clone() },
    )
    .unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut stream).unwrap(),
        blueice_ipc::ServerMessage::Navigated { url }
    );
    assert!(matches!(
        blueice_ipc::read_server_message(&mut stream).unwrap(),
        blueice_ipc::ServerMessage::FrameReady { generation: 1, .. }
    ));
    blueice_ipc::write_client_message(
        &mut stream,
        &blueice_ipc::ClientMessage::GetBlueJsScriptReports,
    )
    .unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut stream).unwrap(),
        blueice_ipc::ServerMessage::BlueJsScriptReports(vec![
            blueice_ipc::BlueJsScriptExecutionReport {
                tab_id: 1,
                document_generation: 1,
                ordinal: 0,
                kind: blueice_ipc::BlueJsScriptKind::Classic,
                outcome: blueice_ipc::BlueJsScriptExecutionOutcome::Executed,
            },
            blueice_ipc::BlueJsScriptExecutionReport {
                tab_id: 1,
                document_generation: 1,
                ordinal: 1,
                kind: blueice_ipc::BlueJsScriptKind::Classic,
                outcome: blueice_ipc::BlueJsScriptExecutionOutcome::Executed,
            },
        ])
    );
    assert_eq!(
        requested_paths.lock().unwrap().as_slice(),
        ["/app/index.html", FIXTURE_PATH]
    );

    blueice_ipc::write_client_message(&mut stream, &blueice_ipc::ClientMessage::Shutdown).unwrap();
    assert!(core.wait().unwrap().success());
    server.join().unwrap();
    shutdown_private_bluejs_host(&host_socket, &host_token);
    host.join().unwrap();
    let _ = std::fs::remove_file(&host_socket);
    assert!(!socket_path.exists());
    assert!(!frame_dir.exists());
}

#[test]
fn real_subprocess_rebinds_inline_javascript_document_context_after_replacement() {
    // The current document origin is a copied, realm-local value. Exercise two
    // real navigations so a stale first-document callback would turn the second
    // document's explicit origin assertion into a source-free runtime failure.
    let socket_path = unique_socket_path("ij-repl");
    let frame_dir = std::env::temp_dir().join(format!(
        "blueice-core-binary-test-inline-bluejs-replacement-frames-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&socket_path);
    let _ = std::fs::remove_dir_all(&frame_dir);
    let gatekeeper_path = clearing_gatekeeper("ijr-gk");

    let listener_one = TcpListener::bind("127.0.0.1:0").unwrap();
    let url_one = format!("http://{}", listener_one.local_addr().unwrap());
    let first_document_origin = url_one.clone();
    thread::spawn(move || {
        let (mut stream, _) = listener_one.accept().unwrap();
        let mut buf = [0u8; 1024];
        let _ = stream.read(&mut buf);
        let body = format!(
            "<main>first JavaScript replacement document</main>\
             <script>if (blueiceDocumentOrigin() !== '{first_document_origin}') \
             {{ throw 'stale origin'; }}</script>"
        );
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

    let listener_two = TcpListener::bind("127.0.0.1:0").unwrap();
    let url_two = format!("http://{}", listener_two.local_addr().unwrap());
    let second_document_origin = url_two.clone();
    thread::spawn(move || {
        let (mut stream, _) = listener_two.accept().unwrap();
        let mut buf = [0u8; 1024];
        let _ = stream.read(&mut buf);
        let body = format!(
            "<main>second JavaScript replacement document</main>\
             <script>if (blueiceDocumentOrigin() !== '{second_document_origin}') \
             {{ throw 'stale origin'; }}</script>"
        );
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

    let mut child = Command::new(env!("CARGO_BIN_EXE_blueice-core"))
        .args([
            "--socket",
            socket_path.to_str().unwrap(),
            "--frame-dir",
            frame_dir.to_str().unwrap(),
            "--gatekeeper-socket",
            gatekeeper_path.to_str().unwrap(),
            "--inline-bluejs",
        ])
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn blueice-core");

    assert!(wait_for(&socket_path, Duration::from_secs(5)));
    let mut stream = connect_with_retry(&socket_path, Duration::from_secs(5)).unwrap();
    blueice_ipc::client_handshake(&mut stream).unwrap();

    for (expected_generation, url) in [(1, url_one), (2, url_two)] {
        blueice_ipc::write_client_message(
            &mut stream,
            &blueice_ipc::ClientMessage::Navigate { url },
        )
        .unwrap();
        assert!(matches!(
            blueice_ipc::read_server_message(&mut stream).unwrap(),
            blueice_ipc::ServerMessage::Navigated { .. }
        ));
        assert!(matches!(
            blueice_ipc::read_server_message(&mut stream).unwrap(),
            blueice_ipc::ServerMessage::FrameReady {
                generation,
                ..
            } if generation == expected_generation
        ));

        blueice_ipc::write_client_message(
            &mut stream,
            &blueice_ipc::ClientMessage::GetBlueJsScriptReports,
        )
        .unwrap();
        assert_eq!(
            blueice_ipc::read_server_message(&mut stream).unwrap(),
            blueice_ipc::ServerMessage::BlueJsScriptReports(vec![
                blueice_ipc::BlueJsScriptExecutionReport {
                    tab_id: 1,
                    document_generation: expected_generation,
                    ordinal: 0,
                    kind: blueice_ipc::BlueJsScriptKind::Classic,
                    outcome: blueice_ipc::BlueJsScriptExecutionOutcome::Executed,
                },
            ])
        );
    }

    blueice_ipc::write_client_message(&mut stream, &blueice_ipc::ClientMessage::Shutdown).unwrap();
    assert!(child.wait().unwrap().success());
    assert!(!socket_path.exists());
    assert!(!frame_dir.exists());
}

#[test]
fn real_subprocess_rejects_an_oversized_document_text_binding_before_inline_admission() {
    // The profile's document-text boundary has a core-selected 1 MiB contract
    // limit. Exercise it through real navigation and process IPC so the page
    // cannot turn a rejection into either source/diagnostic disclosure or a
    // partially admitted BlueJS program.
    let socket_path = unique_socket_path("inline-contract");
    let frame_dir = std::env::temp_dir().join(format!(
        "blueice-core-binary-test-inline-contract-frames-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&socket_path);
    let _ = std::fs::remove_dir_all(&frame_dir);
    let gatekeeper_path = clearing_gatekeeper("ic-gk");

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = [0u8; 1024];
        let _ = stream.read(&mut buf);
        let oversized_text = "x".repeat(1_048_577);
        let body = format!("<main>{oversized_text}</main>",)
            + concat!(
                "<script type=\"application/x-blueice-typescript\">",
                "blueiceDocumentText();",
                "</script>"
            );
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

    let mut child = Command::new(env!("CARGO_BIN_EXE_blueice-core"))
        .args([
            "--socket",
            socket_path.to_str().unwrap(),
            "--frame-dir",
            frame_dir.to_str().unwrap(),
            "--gatekeeper-socket",
            gatekeeper_path.to_str().unwrap(),
            "--inline-bluets-profile",
            "core-script-document-text-v1",
        ])
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn blueice-core");

    assert!(wait_for(&socket_path, Duration::from_secs(5)));
    let mut stream = connect_with_retry(&socket_path, Duration::from_secs(5)).unwrap();
    blueice_ipc::client_handshake(&mut stream).unwrap();
    blueice_ipc::write_client_message(
        &mut stream,
        &blueice_ipc::ClientMessage::Navigate {
            url: format!("http://{addr}"),
        },
    )
    .unwrap();
    assert!(matches!(
        blueice_ipc::read_server_message(&mut stream).unwrap(),
        blueice_ipc::ServerMessage::Navigated { .. }
    ));
    assert!(matches!(
        blueice_ipc::read_server_message(&mut stream).unwrap(),
        blueice_ipc::ServerMessage::FrameReady { generation: 1, .. }
    ));

    blueice_ipc::write_client_message(
        &mut stream,
        &blueice_ipc::ClientMessage::GetBlueTsScriptReports,
    )
    .unwrap();
    let reports = match blueice_ipc::read_server_message(&mut stream).unwrap() {
        blueice_ipc::ServerMessage::BlueTsScriptReports(reports) => reports,
        other => panic!("expected BlueTsScriptReports, got {other:?}"),
    };
    assert_eq!(reports.len(), 1);
    let blueice_ipc::BlueTsScriptExecutionOutcome::Rejected { category } = &reports[0].outcome
    else {
        panic!("the oversized document snapshot must reject before admission")
    };
    assert_eq!(category, "host binding contract rejected the page script");
    assert!(!category.contains('x'));

    blueice_ipc::write_client_message(&mut stream, &blueice_ipc::ClientMessage::Shutdown).unwrap();
    assert!(child.wait().unwrap().success());
    assert!(!socket_path.exists());
    assert!(!frame_dir.exists());
}

#[test]
fn real_subprocess_keeps_opted_in_inline_bluets_reports_isolated_by_tab() {
    // The report query is an observation boundary, so prove it through the
    // compiled process rather than relying only on the executor's queue test:
    // two independently navigated tabs may execute, but draining one must not
    // disclose or discard the other tab's report.
    let socket_path = unique_socket_path("inline-bluets-tabs");
    let frame_dir = std::env::temp_dir().join(format!(
        "blueice-core-binary-test-inline-bluets-tabs-frames-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&socket_path);
    let _ = std::fs::remove_dir_all(&frame_dir);
    let gatekeeper_path = clearing_gatekeeper("ibt-gk");

    let listener_one = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr_one = listener_one.local_addr().unwrap();
    thread::spawn(move || {
        let (mut stream, _) = listener_one.accept().unwrap();
        let mut buf = [0u8; 1024];
        let _ = stream.read(&mut buf);
        let body = concat!(
            "<main>first inline page</main>",
            "<script type=\"application/x-blueice-typescript\">",
            "blueiceDocumentText();",
            "</script>"
        );
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
    let listener_two = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr_two = listener_two.local_addr().unwrap();
    thread::spawn(move || {
        let (mut stream, _) = listener_two.accept().unwrap();
        let mut buf = [0u8; 1024];
        let _ = stream.read(&mut buf);
        let body = concat!(
            "<main>second inline page</main>",
            "<script type=\"application/x-blueice-typescript\">",
            "blueiceDocumentText();",
            "</script>"
        );
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

    let mut child = Command::new(env!("CARGO_BIN_EXE_blueice-core"))
        .args([
            "--socket",
            socket_path.to_str().unwrap(),
            "--frame-dir",
            frame_dir.to_str().unwrap(),
            "--gatekeeper-socket",
            gatekeeper_path.to_str().unwrap(),
            "--inline-bluets-profile",
            "core-script-document-text-v1",
        ])
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn blueice-core");

    assert!(wait_for(&socket_path, Duration::from_secs(5)));
    let mut stream = connect_with_retry(&socket_path, Duration::from_secs(5)).unwrap();
    blueice_ipc::client_handshake(&mut stream).unwrap();

    blueice_ipc::write_client_message(
        &mut stream,
        &blueice_ipc::ClientMessage::Navigate {
            url: format!("http://{addr_one}"),
        },
    )
    .unwrap();
    assert!(matches!(
        blueice_ipc::read_server_message(&mut stream).unwrap(),
        blueice_ipc::ServerMessage::Navigated { .. }
    ));
    assert!(matches!(
        blueice_ipc::read_server_message(&mut stream).unwrap(),
        blueice_ipc::ServerMessage::FrameReady { .. }
    ));

    blueice_ipc::write_client_message(
        &mut stream,
        &blueice_ipc::ClientMessage::OpenTab {
            url: Some(format!("http://{addr_two}")),
        },
    )
    .unwrap();
    let tab_two = match blueice_ipc::read_server_message(&mut stream).unwrap() {
        blueice_ipc::ServerMessage::TabOpened { tab_id, .. } => tab_id,
        other => panic!("expected TabOpened, got {other:?}"),
    };
    assert!(matches!(
        blueice_ipc::read_server_message(&mut stream).unwrap(),
        blueice_ipc::ServerMessage::FrameReady { .. }
    ));

    blueice_ipc::write_client_message(
        &mut stream,
        &blueice_ipc::ClientMessage::GetBlueTsScriptReports,
    )
    .unwrap();
    let (reply_tab, _, first_reports) =
        blueice_ipc::read_server_message_with_ids(&mut stream).unwrap();
    assert_eq!(reply_tab, Some(1));
    assert_eq!(
        first_reports,
        blueice_ipc::ServerMessage::BlueTsScriptReports(vec![
            blueice_ipc::BlueTsScriptExecutionReport {
                tab_id: 1,
                document_generation: 1,
                ordinal: 0,
                kind: blueice_ipc::BlueTsScriptKind::Classic,
                outcome: blueice_ipc::BlueTsScriptExecutionOutcome::Executed,
            },
        ])
    );

    blueice_ipc::write_client_message_with_ids(
        &mut stream,
        Some(tab_two),
        None,
        &blueice_ipc::ClientMessage::GetBlueTsScriptReports,
    )
    .unwrap();
    let (reply_tab, _, second_reports) =
        blueice_ipc::read_server_message_with_ids(&mut stream).unwrap();
    assert_eq!(reply_tab, Some(tab_two));
    assert_eq!(
        second_reports,
        blueice_ipc::ServerMessage::BlueTsScriptReports(vec![
            blueice_ipc::BlueTsScriptExecutionReport {
                tab_id: tab_two,
                document_generation: 1,
                ordinal: 0,
                kind: blueice_ipc::BlueTsScriptKind::Classic,
                outcome: blueice_ipc::BlueTsScriptExecutionOutcome::Executed,
            },
        ])
    );

    blueice_ipc::write_client_message(&mut stream, &blueice_ipc::ClientMessage::Shutdown).unwrap();
    assert!(child.wait().unwrap().success());
    assert!(!socket_path.exists());
    assert!(!frame_dir.exists());
}

#[test]
fn real_subprocess_reexecutes_opted_in_inline_bluets_for_a_replacement_document() {
    // A replacement must receive a new page generation and a fresh execution
    // observation. Drive that lifecycle through the binary so it covers the
    // fetch, gatekeeper, session, realm-owner, and frontend IPC boundaries.
    // Keep the Unix-domain socket leaf short enough for macOS's
    // `sockaddr_un::sun_path` limit.
    let socket_path = unique_socket_path("ib-repl");
    let frame_dir = std::env::temp_dir().join(format!(
        "blueice-core-binary-test-inline-bluets-replacement-frames-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&socket_path);
    let _ = std::fs::remove_dir_all(&frame_dir);
    let gatekeeper_path = clearing_gatekeeper("ibr-gk");

    let listener_one = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr_one = listener_one.local_addr().unwrap();
    thread::spawn(move || {
        let (mut stream, _) = listener_one.accept().unwrap();
        let mut buf = [0u8; 1024];
        let _ = stream.read(&mut buf);
        let body = concat!(
            "<main>first replacement document</main>",
            "<script type=\"application/x-blueice-typescript\">",
            "const text: string = blueiceDocumentText(); text;",
            "</script>"
        );
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
    let listener_two = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr_two = listener_two.local_addr().unwrap();
    thread::spawn(move || {
        let (mut stream, _) = listener_two.accept().unwrap();
        let mut buf = [0u8; 1024];
        let _ = stream.read(&mut buf);
        let body = concat!(
            "<main>second replacement document</main>",
            "<script type=\"application/x-blueice-typescript\">",
            "const text: string = blueiceDocumentText(); text;",
            "</script>"
        );
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

    let mut child = Command::new(env!("CARGO_BIN_EXE_blueice-core"))
        .args([
            "--socket",
            socket_path.to_str().unwrap(),
            "--frame-dir",
            frame_dir.to_str().unwrap(),
            "--gatekeeper-socket",
            gatekeeper_path.to_str().unwrap(),
            "--inline-bluets-profile",
            "core-script-document-text-v1",
        ])
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn blueice-core");

    assert!(wait_for(&socket_path, Duration::from_secs(5)));
    let mut stream = connect_with_retry(&socket_path, Duration::from_secs(5)).unwrap();
    blueice_ipc::client_handshake(&mut stream).unwrap();

    for (expected_generation, address) in [(1, addr_one), (2, addr_two)] {
        blueice_ipc::write_client_message(
            &mut stream,
            &blueice_ipc::ClientMessage::Navigate {
                url: format!("http://{address}"),
            },
        )
        .unwrap();
        assert!(matches!(
            blueice_ipc::read_server_message(&mut stream).unwrap(),
            blueice_ipc::ServerMessage::Navigated { .. }
        ));
        assert!(matches!(
            blueice_ipc::read_server_message(&mut stream).unwrap(),
            blueice_ipc::ServerMessage::FrameReady {
                generation,
                ..
            } if generation == expected_generation
        ));

        blueice_ipc::write_client_message(
            &mut stream,
            &blueice_ipc::ClientMessage::GetBlueTsScriptReports,
        )
        .unwrap();
        assert_eq!(
            blueice_ipc::read_server_message(&mut stream).unwrap(),
            blueice_ipc::ServerMessage::BlueTsScriptReports(vec![
                blueice_ipc::BlueTsScriptExecutionReport {
                    tab_id: 1,
                    document_generation: expected_generation,
                    ordinal: 0,
                    kind: blueice_ipc::BlueTsScriptKind::Classic,
                    outcome: blueice_ipc::BlueTsScriptExecutionOutcome::Executed,
                },
            ])
        );
    }

    blueice_ipc::write_client_message(&mut stream, &blueice_ipc::ClientMessage::Shutdown).unwrap();
    assert!(child.wait().unwrap().success());
    assert!(!socket_path.exists());
    assert!(!frame_dir.exists());
}
