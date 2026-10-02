// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

/// `about:downloads`, end to end through two real processes: the compiled
/// `blueice-core` renders the page, and -- because nothing is listening on
/// the downloads socket it is pointed at -- opening the page makes `core`
/// start the real `blueice-downloads` on that socket, after which the open
/// page updates itself without being reloaded.
#[test]
fn opening_about_downloads_starts_the_downloads_process_and_the_open_page_follows_it() {
    let _guard = core_process_test_guard();
    use blueice_ipc::downloads::{
        read_downloads_reply, write_downloads_request, DownloadsClient, DownloadsRequest,
        DOWNLOADS_PROTOCOL_VERSION,
    };
    use blueice_ipc::{ClientMessage, ServerMessage};

    // Private directories for everything the two processes would otherwise
    // put in the user's home or runtime dir.
    let root = std::env::temp_dir().join(format!("bc-dl-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let (runtime, data, downloads) = (root.join("run"), root.join("data"), root.join("dl"));
    for dir in [&runtime, &data, &downloads] {
        std::fs::create_dir_all(dir).unwrap();
    }
    let core_socket = root.join("core.sock");
    let downloads_socket = root.join("dl.sock");

    let core = Command::new(env!("CARGO_BIN_EXE_blueice-core"))
        .arg("--socket")
        .arg(&core_socket)
        .arg("--downloads-socket")
        .arg(&downloads_socket)
        .env("XDG_RUNTIME_DIR", &runtime)
        .env("XDG_DATA_HOME", &data)
        .env("BLUEICE_DOWNLOAD_DIR", &downloads)
        .stdin(Stdio::null())
        .spawn()
        .expect("spawn blueice-core");
    let mut cleanup = DownloadsPageCleanup {
        core,
        downloads_socket: downloads_socket.clone(),
        root: root.clone(),
    };
    assert!(
        wait_for(&core_socket, Duration::from_secs(5)),
        "core never bound its socket"
    );
    let mut client = UnixStream::connect(&core_socket).unwrap();
    // Starting the separate downloads binary can take longer after the
    // compiled-core suite has exercised several real processes. Keep this
    // deadline above that cold-start interval while still bounding a hang.
    client
        .set_read_timeout(Some(Duration::from_secs(45)))
        .unwrap();
    blueice_ipc::client_handshake(&mut client).unwrap();

    let dom = |client: &mut UnixStream| -> String {
        blueice_ipc::write_client_message(client, &ClientMessage::GetDom).unwrap();
        loop {
            match blueice_ipc::read_server_message(client).unwrap() {
                ServerMessage::Dom(text) => return text,
                ServerMessage::FrameReady { .. } => {}
                other => panic!("unexpected {other:?}"),
            }
        }
    };

    blueice_ipc::write_client_message(
        &mut client,
        &ClientMessage::Navigate {
            url: "about:downloads".to_string(),
        },
    )
    .unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut client).unwrap(),
        ServerMessage::Navigated {
            url: "about:downloads".to_string()
        }
    );
    assert!(
        dom(&mut client).contains("The downloads service is not running"),
        "nothing was listening yet"
    );

    // Opening the page started the real downloads process, and the page notices.
    assert!(
        wait_for(&downloads_socket, Duration::from_secs(15)),
        "opening the page never started blueice-downloads"
    );
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let text = dom(&mut client);
        if text.contains("No downloads yet.") {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the open page never picked up the downloads service: {text}"
        );
        thread::sleep(Duration::from_millis(100));
    }

    // A transfer started by anyone appears in the open page. There is no
    // gatekeeper in this test, so it is blocked (fail-closed) -- which is
    // itself something the page should say.
    let mut downloads =
        DownloadsClient::connect(UnixStream::connect(&downloads_socket).unwrap()).unwrap();
    downloads
        .start("http://127.0.0.1:1/never-fetched.bin", None, false)
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let text = dom(&mut client);
        if text.contains("Blocked") && text.contains("Blocked by the safety gatekeeper") {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the open page never showed the new transfer: {text}"
        );
        thread::sleep(Duration::from_millis(100));
    }

    // Tidy up: the downloads process was started on the page's behalf and
    // outlives it by design, so stop it explicitly.
    drop(downloads);
    let mut raw = UnixStream::connect(&downloads_socket).unwrap();
    write_downloads_request(
        &mut raw,
        Some(1),
        &DownloadsRequest::Hello {
            protocol_version: DOWNLOADS_PROTOCOL_VERSION,
        },
    )
    .unwrap();
    read_downloads_reply(&mut raw).unwrap();
    write_downloads_request(&mut raw, Some(2), &DownloadsRequest::Shutdown).unwrap();
    read_downloads_reply(&mut raw).unwrap();
    blueice_ipc::write_client_message(&mut client, &ClientMessage::Shutdown).unwrap();
    let _ = cleanup.core.wait();
}
