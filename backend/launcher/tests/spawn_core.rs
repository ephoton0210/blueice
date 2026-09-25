// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![cfg(unix)]

//! Integration test for [`blueice_launcher::SpawnedCore`] -- spawns the
//! *real* `blueice-core` binary and drives it over the real internal
//! Unix socket, the same strategy `blueice-mcp-server`'s own
//! `tests/core_process.rs` uses for its equivalent `CoreProcess`.
//! Headless and display-free -- there is no reason this can't run in CI.

use blueice_launcher::{CoreLaunchOptions, SpawnedCore};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::os::unix::net::{UnixListener, UnixStream};
use std::thread;
use std::time::{Duration, Instant};

#[test]
fn spawn_connects_to_a_real_core_and_cleans_up_on_drop() {
    let dir = std::env::temp_dir().join(format!(
        "blueice-launcher-test-spawn-core-{}",
        std::process::id()
    ));
    let core = SpawnedCore::spawn(320.0, 200.0, &dir)
        .expect("blueice-core must spawn and accept a connection");
    assert!(core.script_socket_path().is_none());

    let mut stream = core.stream.try_clone().unwrap();
    blueice_ipc::write_client_message(
        &mut stream,
        &blueice_ipc::ClientMessage::Navigate {
            url: "about:blank".to_string(),
        },
    )
    .unwrap();
    let navigated = blueice_ipc::read_server_message(&mut stream).unwrap();
    assert_eq!(
        navigated,
        blueice_ipc::ServerMessage::Navigated {
            url: "about:blank".to_string()
        }
    );

    drop(core); // must not panic or hang
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn supervised_child_gets_a_private_script_listener_that_denies_foreign_hello() {
    let dir = std::env::temp_dir().join(format!(
        "blueice-launcher-test-script-core-{}",
        std::process::id()
    ));
    let core = SpawnedCore::spawn_with_options(
        320.0,
        200.0,
        &dir,
        CoreLaunchOptions::default().supervise_out_of_process_bluejs(),
    )
    .expect("launcher must start one core and its supervised child");
    let path = core
        .script_socket_path()
        .expect("supervised core must have a private script listener")
        .to_path_buf();
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut stream = loop {
        match UnixStream::connect(&path) {
            Ok(stream) => break stream,
            Err(_) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            Err(error) => panic!("script socket never accepted a connection: {error}"),
        }
    };
    blueice_ipc::script::write_script_request(
        &mut stream,
        &blueice_ipc::script::ScriptRequest::Hello {
            protocol_version: blueice_ipc::script::SCRIPT_PROTOCOL_VERSION,
            session_token: "f".repeat(64),
        },
    )
    .unwrap();
    assert!(matches!(
        blueice_ipc::script::read_script_reply(&mut stream).unwrap(),
        blueice_ipc::script::ScriptReply::Error { .. }
    ));
    assert!(blueice_ipc::script::read_script_reply(&mut stream).is_err());
    drop(core);
    assert!(
        !path.exists(),
        "launcher must remove its private script socket"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn supervised_child_script_completes_a_synchronous_core_dom_lookup() {
    let gatekeeper_path = std::env::temp_dir().join(format!("bi-dom-{}.sock", std::process::id()));
    let _ = std::fs::remove_file(&gatekeeper_path);
    let gatekeeper = UnixListener::bind(&gatekeeper_path).unwrap();
    thread::spawn(move || {
        for incoming in gatekeeper.incoming() {
            let Ok(mut stream) = incoming else { break };
            let _ = blueice_ai_gatekeeper::handle_one_check(&mut stream);
        }
    });

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/lookup.html", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0u8; 1024];
        let _ = stream.read(&mut request).unwrap();
        let body = concat!(
            "<div id='target'>live core node</div>",
            "<script>",
            "if (typeof fetch !== 'undefined') throw 'child gained fetch';",
            "if (typeof document !== 'undefined') throw 'child gained direct document';",
            "if (!blueiceTestHasElementById('target')) throw 'missing live node';",
            "if (blueiceTestHasElementById('absent')) throw 'invented node';",
            "let node = blueiceTestGetElementById('target');",
            "if (node === null || typeof node !== 'object') throw 'missing wrapper';",
            "if (node !== blueiceTestGetElementById('target')) throw 'unstable wrapper';",
            "if (Object.keys(node).length !== 0 || node.nodeId !== undefined) throw 'leaked node ID';",
            "if (node.blueiceTestRequireLive() !== true) throw 'live wrapper rejected';",
            "try { node.blueiceTestRequireLive.call(Object.create(node)); throw 'forged receiver accepted'; } catch (error) { if (!(error instanceof TypeError)) throw error; }",
            "if (blueiceTestGetElementById('absent') !== null) throw 'invented wrapper';",
            "globalThis.domLookupCompleted = true;",
            "</script>",
            "<script>if (!globalThis.domLookupCompleted) throw 'lookup did not complete';</script>"
        );
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

    let frame_dir = std::env::temp_dir().join(format!(
        "blueice-launcher-dom-probe-frames-{}",
        std::process::id()
    ));
    let core = SpawnedCore::spawn_with_options(
        320.0,
        200.0,
        &frame_dir,
        CoreLaunchOptions::default()
            .with_gatekeeper_socket(gatekeeper_path.clone())
            .supervise_out_of_process_bluejs_with_dom_lookup_probe_fixture(),
    )
    .expect("launcher must supervise the real core and child");
    let mut stream = core.stream.try_clone().unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
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
        blueice_ipc::ServerMessage::FrameReady { .. }
    ));
    blueice_ipc::write_client_message(
        &mut stream,
        &blueice_ipc::ClientMessage::GetBlueJsScriptReports,
    )
    .unwrap();
    let blueice_ipc::ServerMessage::BlueJsScriptReports(reports) =
        blueice_ipc::read_server_message(&mut stream).unwrap()
    else {
        panic!("expected page script reports");
    };
    assert_eq!(reports.len(), 2);
    assert!(reports.iter().all(|report| matches!(
        report.outcome,
        blueice_ipc::BlueJsScriptExecutionOutcome::Executed
    )));
    server.join().unwrap();
    drop(core);
    let _ = std::fs::remove_file(&gatekeeper_path);
    let _ = std::fs::remove_dir_all(&frame_dir);
}

#[test]
fn supervised_child_dom_text_profile_executes_checked_bluets_and_renders_live_text() {
    let gatekeeper_path = std::env::temp_dir().join(format!(
        "bi-dom-text-gatekeeper-{}.sock",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&gatekeeper_path);
    let gatekeeper = UnixListener::bind(&gatekeeper_path).unwrap();
    thread::spawn(move || {
        for incoming in gatekeeper.incoming() {
            let Ok(mut stream) = incoming else { break };
            let _ = blueice_ai_gatekeeper::handle_one_check(&mut stream);
        }
    });

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/dom-text.html", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0u8; 1024];
        let _ = stream.read(&mut request).unwrap();
        let body = concat!(
            "<div id='target'><span id='inner'>before</span></div>",
            "<script>",
            "if (typeof fetch !== 'undefined' || typeof blueiceTestGetElementById !== 'undefined') throw 'ambient';",
            "let node = document.getElementById('target');",
            "let removed = document.getElementById('inner');",
            "if (node === null || node.textContent !== 'before') throw 'lookup';",
            "if (removed === null || removed.textContent !== 'before') throw 'inner lookup';",
            "if (node !== document.getElementById('target')) throw 'identity';",
            "if (Object.keys(node).length !== 0 || node.nodeId !== undefined) throw 'node ID';",
            "if (document.getElementById('absent') !== null) throw 'miss';",
            "if (typeof document.createElement !== 'undefined') throw 'unsupported DOM';",
            "node.textContent = 'from JavaScript';",
            "if (node.textContent !== 'from JavaScript') throw 'write';",
            "try { removed.textContent; throw 'removed wrapper survived'; } catch (error) { if (!(error instanceof TypeError)) throw error; }",
            "globalThis.domTextScriptCompleted = true;",
            "</script>",
            "<script type='application/x-blueice-typescript'>",
            "const typedNode = document.getElementById('target')!;",
            "typedNode.textContent = typedNode.textContent + ' / BlueTS';",
            "</script>",
            "<script type='application/x-blueice-typescript'>",
            "document.getElementById(42);",
            "</script>",
            "<script>",
            "if (!globalThis.domTextScriptCompleted) throw 'order';",
            "const finalNode = document.getElementById('target');",
            "if (finalNode === null || finalNode.textContent !== 'from JavaScript / BlueTS') throw 'order';",
            "finalNode.textContent = 'rendered text';",
            "</script>"
        );
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

    let frame_dir = std::env::temp_dir().join(format!(
        "blueice-launcher-dom-text-frames-{}",
        std::process::id()
    ));
    let core = SpawnedCore::spawn_with_options(
        320.0,
        200.0,
        &frame_dir,
        CoreLaunchOptions::default()
            .with_gatekeeper_socket(gatekeeper_path.clone())
            .supervise_out_of_process_bluejs_with_dom_text_fixture(),
    )
    .expect("launcher must supervise the real core and child");
    let mut stream = core.stream.try_clone().unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    blueice_ipc::write_client_message(
        &mut stream,
        &blueice_ipc::ClientMessage::Navigate { url: url.clone() },
    )
    .unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut stream).unwrap(),
        blueice_ipc::ServerMessage::Navigated { url }
    );
    let blueice_ipc::ServerMessage::FrameReady { shm_path, .. } =
        blueice_ipc::read_server_message(&mut stream).unwrap()
    else {
        panic!("expected a rendered frame after the DOM text mutation");
    };
    assert!(std::fs::metadata(shm_path).unwrap().len() > 0);
    blueice_ipc::write_client_message(
        &mut stream,
        &blueice_ipc::ClientMessage::GetBlueJsScriptReports,
    )
    .unwrap();
    let blueice_ipc::ServerMessage::BlueJsScriptReports(reports) =
        blueice_ipc::read_server_message(&mut stream).unwrap()
    else {
        panic!("expected page script reports");
    };
    assert_eq!(reports.len(), 2);
    assert!(
        reports.iter().all(|report| matches!(
            report.outcome,
            blueice_ipc::BlueJsScriptExecutionOutcome::Executed
        )),
        "{reports:?}"
    );
    blueice_ipc::write_client_message(
        &mut stream,
        &blueice_ipc::ClientMessage::GetBlueTsScriptReports,
    )
    .unwrap();
    let blueice_ipc::ServerMessage::BlueTsScriptReports(typed_reports) =
        blueice_ipc::read_server_message(&mut stream).unwrap()
    else {
        panic!("expected BlueTS page script reports");
    };
    assert_eq!(typed_reports.len(), 2);
    assert!(
        matches!(
            typed_reports[0].outcome,
            blueice_ipc::BlueTsScriptExecutionOutcome::Executed
        ),
        "{typed_reports:?}"
    );
    assert!(
        matches!(
            &typed_reports[1].outcome,
            blueice_ipc::BlueTsScriptExecutionOutcome::Rejected { category }
                if category == "BlueTS compilation rejected the page script"
        ),
        "{typed_reports:?}"
    );
    blueice_ipc::write_client_message_with_ids(
        &mut stream,
        Some(1),
        Some(301),
        &blueice_ipc::ClientMessage::GetDom,
    )
    .unwrap();
    let (_, reply_id, dom_reply) = blueice_ipc::read_server_message_with_ids(&mut stream).unwrap();
    assert_eq!(reply_id, Some(301));
    let blueice_ipc::ServerMessage::Dom(dom) = dom_reply else {
        panic!("expected the live DOM dump");
    };
    assert!(dom.contains("\"rendered text\""), "{dom}");
    assert!(!dom.contains("\"before\""), "{dom}");
    server.join().unwrap();
    drop(core);
    let _ = std::fs::remove_file(&gatekeeper_path);
    let _ = std::fs::remove_dir_all(&frame_dir);
}

#[test]
fn supervised_child_dom_mutation_profile_renders_created_subtree_from_js_and_bluets() {
    let gatekeeper_path = std::env::temp_dir().join(format!(
        "bi-dom-mutation-gatekeeper-{}.sock",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&gatekeeper_path);
    let gatekeeper = UnixListener::bind(&gatekeeper_path).unwrap();
    thread::spawn(move || {
        for incoming in gatekeeper.incoming() {
            let Ok(mut stream) = incoming else { break };
            let _ = blueice_ai_gatekeeper::handle_one_check(&mut stream);
        }
    });

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!(
        "http://{}/dom-mutation.html",
        listener.local_addr().unwrap()
    );
    let baseline_url = format!("http://{}/baseline.html", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        let body = concat!(
            "<style>span { display: block; width: 100px; height: 30px; background-color: red; }</style>",
            "<div id='target'></div>",
            "<script>",
            "if (typeof fetch !== 'undefined' || typeof blueiceTestGetElementById !== 'undefined') throw 'ambient';",
            "let parent = document.getElementById('target');",
            "let child = document.createElement('span');",
            "let text = document.createTextNode('JS added');",
            "if (child.appendChild(text) !== text) throw 'text identity';",
            "if (parent.appendChild(child) !== child) throw 'child identity';",
            "try { parent.appendChild(Object.create(child)); throw 'forged accepted'; } catch (error) { if (!(error instanceof TypeError)) throw error; }",
            "globalThis.jsMutationCompleted = true;",
            "</script>",
            "<script type='application/x-blueice-typescript'>",
            "const typedParent = document.getElementById('target')!;",
            "const typedChild = document.createElement('strong');",
            "const typedText = document.createTextNode(' BlueTS added');",
            "typedChild.appendChild(typedText); typedParent.appendChild(typedChild);",
            "</script>",
            "<script type='application/x-blueice-typescript'>",
            "document.getElementById('target')!.appendChild('wrong');",
            "</script>",
            "<script>",
            "if (!globalThis.jsMutationCompleted) throw 'order';",
            "let finalParent = document.getElementById('target');",
            "if (finalParent.textContent !== 'JS added BlueTS added') throw 'missing subtree';",
            "</script>"
        );
        for response_body in [
            "<style>span { display: block; width: 100px; height: 30px; background-color: red; }</style><div id='target'></div>",
            body,
        ] {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0u8; 1024];
            let _ = stream.read(&mut request).unwrap();
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response_body}",
                        response_body.len()
                    )
                    .as_bytes(),
                )
                .unwrap();
        }
    });

    let frame_dir = std::env::temp_dir().join(format!(
        "blueice-launcher-dom-mutation-frames-{}",
        std::process::id()
    ));
    let core = SpawnedCore::spawn_with_options(
        320.0,
        200.0,
        &frame_dir,
        CoreLaunchOptions::default()
            .with_gatekeeper_socket(gatekeeper_path.clone())
            .supervise_out_of_process_bluejs_with_dom_mutation_fixture(),
    )
    .expect("launcher must supervise the real core and mutation-capable child");
    let mut stream = core.stream.try_clone().unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    blueice_ipc::write_client_message(
        &mut stream,
        &blueice_ipc::ClientMessage::Navigate {
            url: baseline_url.clone(),
        },
    )
    .unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut stream).unwrap(),
        blueice_ipc::ServerMessage::Navigated { url: baseline_url }
    );
    let blueice_ipc::ServerMessage::FrameReady {
        shm_path: baseline_path,
        ..
    } = blueice_ipc::read_server_message(&mut stream).unwrap()
    else {
        panic!("expected a baseline frame before DOM append");
    };
    let baseline_pixels = std::fs::read(baseline_path).unwrap();
    blueice_ipc::write_client_message(
        &mut stream,
        &blueice_ipc::ClientMessage::Navigate { url: url.clone() },
    )
    .unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut stream).unwrap(),
        blueice_ipc::ServerMessage::Navigated { url }
    );
    let blueice_ipc::ServerMessage::FrameReady { shm_path, .. } =
        blueice_ipc::read_server_message(&mut stream).unwrap()
    else {
        panic!("expected a rendered frame after DOM append");
    };
    let appended_pixels = std::fs::read(shm_path).unwrap();
    assert!(!appended_pixels.is_empty());
    assert!(
        appended_pixels != baseline_pixels,
        "created subtree must change the rasterized frame"
    );
    blueice_ipc::write_client_message(
        &mut stream,
        &blueice_ipc::ClientMessage::GetBlueJsScriptReports,
    )
    .unwrap();
    let blueice_ipc::ServerMessage::BlueJsScriptReports(reports) =
        blueice_ipc::read_server_message(&mut stream).unwrap()
    else {
        panic!("expected JavaScript reports");
    };
    assert_eq!(reports.len(), 2);
    assert!(
        reports.iter().all(|report| matches!(
            report.outcome,
            blueice_ipc::BlueJsScriptExecutionOutcome::Executed
        )),
        "{reports:?}"
    );
    blueice_ipc::write_client_message(
        &mut stream,
        &blueice_ipc::ClientMessage::GetBlueTsScriptReports,
    )
    .unwrap();
    let blueice_ipc::ServerMessage::BlueTsScriptReports(typed_reports) =
        blueice_ipc::read_server_message(&mut stream).unwrap()
    else {
        panic!("expected BlueTS reports");
    };
    assert_eq!(typed_reports.len(), 2);
    assert!(
        matches!(
            typed_reports[0].outcome,
            blueice_ipc::BlueTsScriptExecutionOutcome::Executed
        ),
        "{typed_reports:?}"
    );
    assert!(
        matches!(
            &typed_reports[1].outcome,
            blueice_ipc::BlueTsScriptExecutionOutcome::Rejected { category }
                if category == "BlueTS compilation rejected the page script"
        ),
        "{typed_reports:?}"
    );
    blueice_ipc::write_client_message_with_ids(
        &mut stream,
        Some(1),
        Some(302),
        &blueice_ipc::ClientMessage::GetDom,
    )
    .unwrap();
    let (_, reply_id, dom_reply) = blueice_ipc::read_server_message_with_ids(&mut stream).unwrap();
    assert_eq!(reply_id, Some(302));
    let blueice_ipc::ServerMessage::Dom(dom) = dom_reply else {
        panic!("expected the live DOM dump");
    };
    assert!(dom.contains("\"JS added\""), "{dom}");
    assert!(dom.contains("\" BlueTS added\""), "{dom}");
    server.join().unwrap();
    drop(core);
    let _ = std::fs::remove_file(&gatekeeper_path);
    let _ = std::fs::remove_dir_all(&frame_dir);
}

#[test]
fn supervised_child_click_event_profile_runs_js_and_bluets_before_navigation() {
    let gatekeeper_path = std::env::temp_dir().join(format!(
        "bi-dom-event-gatekeeper-{}.sock",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&gatekeeper_path);
    let gatekeeper = UnixListener::bind(&gatekeeper_path).unwrap();
    thread::spawn(move || {
        for incoming in gatekeeper.incoming() {
            let Ok(mut stream) = incoming else { break };
            let _ = blueice_ai_gatekeeper::handle_one_check(&mut stream);
        }
    });

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/click.html", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        let body = concat!(
            "<style>a { display: block; width: 100px; height: 30px; }</style>",
            "<a id='js' href='/away'>JS click</a><a id='ts' href='/away'>TS click</a>",
            "<div id='status'>before</div>",
            "<script>if (typeof document.getElementById('js').dispatchEvent !== 'undefined') throw 'broad event API';",
            "document.getElementById('js').addEventListener('click', function(event) {",
            "event.preventDefault();",
            "Promise.resolve().then(function() { document.getElementById('status').textContent = 'JS ran'; });",
            "throw 'listener failure';",
            "});</script>",
            "<script type='application/x-blueice-typescript'>",
            "function onTypedClick(event: BlueIceClickEvent): void {",
            "event.preventDefault();",
            "const status = document.getElementById('status')!;",
            "status.textContent = 'TS ran';",
            "const badge = document.createElement('strong');",
            "const label = document.createTextNode(' created on click');",
            "badge.appendChild(label); status.appendChild(badge);",
            "}",
            "const typedLink = document.getElementById('ts')!;",
            "typedLink.addEventListener('click', onTypedClick);</script>"
        );
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0u8; 1024];
        let _ = stream.read(&mut request).unwrap();
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

    let frame_dir = std::env::temp_dir().join(format!(
        "blueice-launcher-dom-event-frames-{}",
        std::process::id()
    ));
    let core = SpawnedCore::spawn_with_options(
        320.0,
        200.0,
        &frame_dir,
        CoreLaunchOptions::default()
            .with_gatekeeper_socket(gatekeeper_path.clone())
            .supervise_out_of_process_bluejs_with_dom_event_fixture(),
    )
    .expect("launcher must supervise an event-capable child");
    let mut stream = core.stream.try_clone().unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    blueice_ipc::write_client_message(
        &mut stream,
        &blueice_ipc::ClientMessage::Navigate { url: url.clone() },
    )
    .unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut stream).unwrap(),
        blueice_ipc::ServerMessage::Navigated { url: url.clone() }
    );
    assert!(matches!(
        blueice_ipc::read_server_message(&mut stream).unwrap(),
        blueice_ipc::ServerMessage::FrameReady { .. }
    ));
    blueice_ipc::write_client_message(
        &mut stream,
        &blueice_ipc::ClientMessage::GetBlueTsScriptReports,
    )
    .unwrap();
    let blueice_ipc::ServerMessage::BlueTsScriptReports(typed_reports) =
        blueice_ipc::read_server_message(&mut stream).unwrap()
    else {
        panic!("expected the click page's BlueTS compilation report");
    };
    assert_eq!(typed_reports.len(), 1, "{typed_reports:?}");
    assert!(
        matches!(
            typed_reports[0].outcome,
            blueice_ipc::BlueTsScriptExecutionOutcome::Executed
        ),
        "{typed_reports:?}"
    );
    blueice_ipc::write_client_message(&mut stream, &blueice_ipc::ClientMessage::GetRepresentation)
        .unwrap();
    let blueice_ipc::ServerMessage::Representation(snapshot) =
        blueice_ipc::read_server_message(&mut stream).unwrap()
    else {
        panic!("expected a live representation");
    };
    let js_link = snapshot
        .nodes
        .iter()
        .find(|node| node.name.as_deref() == Some("JS click"))
        .expect("JavaScript link must be visible");
    let ts_link = snapshot
        .nodes
        .iter()
        .find(|node| node.name.as_deref() == Some("TS click"))
        .expect("BlueTS link must be visible");
    blueice_ipc::write_client_message(
        &mut stream,
        &blueice_ipc::ClientMessage::Click {
            x: js_link.bounds.x + js_link.bounds.width / 2.0,
            y: js_link.bounds.y + js_link.bounds.height / 2.0,
        },
    )
    .unwrap();
    assert!(matches!(
        blueice_ipc::read_server_message(&mut stream).unwrap(),
        blueice_ipc::ServerMessage::FrameReady { .. }
    ));
    blueice_ipc::write_client_message(&mut stream, &blueice_ipc::ClientMessage::GetDom).unwrap();
    let blueice_ipc::ServerMessage::Dom(dom) =
        blueice_ipc::read_server_message(&mut stream).unwrap()
    else {
        panic!("expected DOM after JavaScript click");
    };
    assert!(dom.contains("\"JS ran\""), "{dom}");

    blueice_ipc::write_client_message(
        &mut stream,
        &blueice_ipc::ClientMessage::ActOn {
            id: ts_link.id,
            action: blueice_ipc::NodeAction::Click,
        },
    )
    .unwrap();
    assert!(matches!(
        blueice_ipc::read_server_message(&mut stream).unwrap(),
        blueice_ipc::ServerMessage::FrameReady { .. }
    ));
    blueice_ipc::write_client_message(&mut stream, &blueice_ipc::ClientMessage::GetDom).unwrap();
    let blueice_ipc::ServerMessage::Dom(dom) =
        blueice_ipc::read_server_message(&mut stream).unwrap()
    else {
        panic!("expected DOM after BlueTS click");
    };
    assert!(dom.contains("\"TS ran\""), "{dom}");
    assert!(dom.contains("\" created on click\""), "{dom}");
    server.join().unwrap();
    drop(core);
    let _ = std::fs::remove_file(&gatekeeper_path);
    let _ = std::fs::remove_dir_all(&frame_dir);
}

#[test]
fn supervised_child_event_profile_catches_unsupported_bluets_any_calls() {
    let gatekeeper_path = std::env::temp_dir().join(format!(
        "bi-dom-event-any-gatekeeper-{}.sock",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&gatekeeper_path);
    let gatekeeper = UnixListener::bind(&gatekeeper_path).unwrap();
    thread::spawn(move || {
        for incoming in gatekeeper.incoming() {
            let Ok(mut stream) = incoming else { break };
            let _ = blueice_ai_gatekeeper::handle_one_check(&mut stream);
        }
    });

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!(
        "http://{}/unsupported-any.html",
        listener.local_addr().unwrap()
    );
    let server = thread::spawn(move || {
        let body = concat!(
            "<a id='link' href='/away'>Click</a><div id='status'>before</div>",
            "<script type='application/x-blueice-typescript'>",
            "function tryQuery(value: any): void { value.querySelector('#link'); }",
            "function tryStop(value: any): void { value.stopPropagation(); }",
            "</script>",
            "<script>",
            "try { tryQuery(document); throw 'querySelector returned'; }",
            "catch (error) { if (!(error instanceof TypeError)) throw error; }",
            "document.getElementById('status').textContent = 'query caught';",
            "document.getElementById('link').addEventListener('click', function(event) {",
            "event.preventDefault();",
            "try { tryStop(event); throw 'stopPropagation returned'; }",
            "catch (error) { if (!(error instanceof TypeError)) throw error; }",
            "document.getElementById('status').textContent = 'both caught';",
            "});",
            "</script>"
        );
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0u8; 1024];
        let _ = stream.read(&mut request).unwrap();
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

    let frame_dir = std::env::temp_dir().join(format!(
        "blueice-launcher-dom-event-any-frames-{}",
        std::process::id()
    ));
    let core = SpawnedCore::spawn_with_options(
        320.0,
        200.0,
        &frame_dir,
        CoreLaunchOptions::default()
            .with_gatekeeper_socket(gatekeeper_path.clone())
            .supervise_out_of_process_bluejs_with_dom_event_fixture(),
    )
    .expect("launcher must supervise an event-capable child");
    let mut stream = core.stream.try_clone().unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
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
        blueice_ipc::ServerMessage::FrameReady { .. }
    ));
    blueice_ipc::write_client_message(
        &mut stream,
        &blueice_ipc::ClientMessage::GetBlueTsScriptReports,
    )
    .unwrap();
    let blueice_ipc::ServerMessage::BlueTsScriptReports(typed_reports) =
        blueice_ipc::read_server_message(&mut stream).unwrap()
    else {
        panic!("expected the BlueTS execution report");
    };
    assert_eq!(typed_reports.len(), 1, "{typed_reports:?}");
    assert!(
        matches!(
            typed_reports[0].outcome,
            blueice_ipc::BlueTsScriptExecutionOutcome::Executed
        ),
        "{typed_reports:?}"
    );
    blueice_ipc::write_client_message(
        &mut stream,
        &blueice_ipc::ClientMessage::GetBlueJsScriptReports,
    )
    .unwrap();
    let blueice_ipc::ServerMessage::BlueJsScriptReports(js_reports) =
        blueice_ipc::read_server_message(&mut stream).unwrap()
    else {
        panic!("expected the JavaScript execution report");
    };
    assert_eq!(js_reports.len(), 1, "{js_reports:?}");
    assert!(matches!(
        js_reports[0].outcome,
        blueice_ipc::BlueJsScriptExecutionOutcome::Executed
    ));
    blueice_ipc::write_client_message(&mut stream, &blueice_ipc::ClientMessage::GetDom).unwrap();
    let blueice_ipc::ServerMessage::Dom(dom) =
        blueice_ipc::read_server_message(&mut stream).unwrap()
    else {
        panic!("expected DOM after the caught querySelector failure");
    };
    assert!(dom.contains("\"query caught\""), "{dom}");

    blueice_ipc::write_client_message(&mut stream, &blueice_ipc::ClientMessage::GetRepresentation)
        .unwrap();
    let blueice_ipc::ServerMessage::Representation(snapshot) =
        blueice_ipc::read_server_message(&mut stream).unwrap()
    else {
        panic!("expected the link representation");
    };
    let link = snapshot
        .nodes
        .iter()
        .find(|node| node.name.as_deref() == Some("Click"))
        .expect("the link must be visible");
    blueice_ipc::write_client_message(
        &mut stream,
        &blueice_ipc::ClientMessage::ActOn {
            id: link.id,
            action: blueice_ipc::NodeAction::Click,
        },
    )
    .unwrap();
    assert!(matches!(
        blueice_ipc::read_server_message(&mut stream).unwrap(),
        blueice_ipc::ServerMessage::FrameReady { .. }
    ));
    blueice_ipc::write_client_message(&mut stream, &blueice_ipc::ClientMessage::GetDom).unwrap();
    let blueice_ipc::ServerMessage::Dom(dom) =
        blueice_ipc::read_server_message(&mut stream).unwrap()
    else {
        panic!("expected DOM after the caught stopPropagation failure");
    };
    assert!(dom.contains("\"both caught\""), "{dom}");
    server.join().unwrap();
    drop(core);
    let _ = std::fs::remove_file(&gatekeeper_path);
    let _ = std::fs::remove_dir_all(&frame_dir);
}
