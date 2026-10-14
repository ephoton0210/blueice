// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

fn connect_ui_frontend(path: &Path, timeout: Duration) -> std::io::Result<UnixStream> {
    let deadline = Instant::now() + timeout;
    loop {
        match UnixStream::connect(path) {
            Ok(stream) => return Ok(stream),
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused
                ) && Instant::now() < deadline =>
            {
                thread::sleep(Duration::from_millis(10));
            }
            Err(error) => return Err(error),
        }
    }
}

struct UiCoreCleanup {
    child: Child,
    sockets: Vec<PathBuf>,
    directories: Vec<PathBuf>,
}

impl Drop for UiCoreCleanup {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            if let Ok(mut stream) =
                connect_ui_frontend(&self.sockets[0], Duration::from_millis(200))
            {
                let _ = stream.set_read_timeout(Some(Duration::from_millis(200)));
                let _ = stream.set_write_timeout(Some(Duration::from_millis(200)));
                if blueice_ipc::client_handshake(&mut stream).is_ok() {
                    let _ = blueice_ipc::write_client_message(
                        &mut stream,
                        &blueice_ipc::ClientMessage::Shutdown,
                    );
                }
            }
            let deadline = Instant::now() + Duration::from_secs(2);
            while self.child.try_wait().ok().flatten().is_none() && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(20));
            }
            if self.child.try_wait().ok().flatten().is_none() {
                let _ = self.child.kill();
            }
        }
        let _ = self.child.wait();
        for socket in &self.sockets {
            let _ = std::fs::remove_file(socket);
        }
        for directory in &self.directories {
            let _ = std::fs::remove_dir_all(directory);
        }
    }
}

#[test]
fn frontend_connection_waits_for_a_socket_to_become_listening() {
    let _guard = core_process_test_guard();
    let socket = unique_socket_path("ui-readiness");
    let _ = std::fs::remove_file(&socket);
    drop(UnixListener::bind(&socket).unwrap());
    let endpoint = socket.clone();
    let worker = thread::spawn(move || {
        thread::sleep(Duration::from_millis(50));
        let result = (|| -> std::io::Result<()> {
            std::fs::remove_file(&endpoint)?;
            let listener = UnixListener::bind(&endpoint)?;
            listener.set_nonblocking(true)?;
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error)
                        if error.kind() == std::io::ErrorKind::WouldBlock
                            && Instant::now() < deadline =>
                    {
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => return Err(error),
                }
            };
            stream.set_nonblocking(false)?;
            stream.set_read_timeout(Some(Duration::from_secs(5)))?;
            stream.set_write_timeout(Some(Duration::from_secs(5)))?;
            let mut request = [0];
            stream.read_exact(&mut request)?;
            assert_eq!(request, [7]);
            stream.write_all(&[8])
        })();
        let _ = std::fs::remove_file(endpoint);
        result
    });
    let result = connect_ui_frontend(&socket, Duration::from_secs(5)).and_then(|mut stream| {
        stream.set_read_timeout(Some(Duration::from_secs(5)))?;
        stream.set_write_timeout(Some(Duration::from_secs(5)))?;
        stream.write_all(&[7])?;
        let mut reply = [0];
        stream.read_exact(&mut reply)?;
        assert_eq!(reply, [8]);
        Ok(())
    });
    let server = worker.join().unwrap();
    assert!(
        result.is_ok(),
        "frontend readiness: {result:?}; server: {server:?}"
    );
    server.unwrap();
    assert!(!socket.exists());
}

#[test]
fn core_spawned_extension_toolbar_reaches_client_and_activation_reaches_host() {
    let _guard = core_process_test_guard();
    let core_socket = unique_socket_path("uit");
    let extension_socket = unique_private_extension_socket_path("ui");
    let frame_dir = std::env::temp_dir().join(format!(
        "blueice-core-ui-extension-frames-{}",
        std::process::id()
    ));
    let (package_root, manifest, _) = extension_manifest_package("native-ui", &["ui:inject"]);
    let gatekeeper_socket = clearing_gatekeeper("ui-gk");
    let extension_host = core_extension_host_probe_script(
        &package_root,
        "extension_host_probe_child_publishes_toolbar_and_handles_activation",
    );
    let _ = std::fs::remove_file(&core_socket);
    let _ = std::fs::remove_file(&extension_socket);
    let _ = std::fs::remove_dir_all(&frame_dir);
    let child = Command::new(env!("CARGO_BIN_EXE_blueice-core"))
        .args([
            "--socket",
            core_socket.to_str().unwrap(),
            "--extension-socket",
            extension_socket.to_str().unwrap(),
            "--extension-manifest",
            manifest.to_str().unwrap(),
            "--extension-host",
            extension_host.to_str().unwrap(),
            "--gatekeeper-socket",
            gatekeeper_socket.to_str().unwrap(),
            "--frame-dir",
            frame_dir.to_str().unwrap(),
        ])
        .spawn()
        .expect("failed to spawn core with its native UI extension host");
    let mut core = UiCoreCleanup {
        child,
        sockets: vec![
            core_socket.clone(),
            extension_socket.clone(),
            gatekeeper_socket,
        ],
        directories: vec![frame_dir.clone(), package_root],
    };
    let mut frontend = connect_ui_frontend(&core_socket, Duration::from_secs(15)).unwrap();
    frontend
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    blueice_ipc::client_handshake(&mut frontend).unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut frontend).unwrap(),
        blueice_ipc::ServerMessage::ExtensionToolbar {
            label: Some("Notes".to_string()),
        }
    );
    blueice_ipc::write_client_message(
        &mut frontend,
        &blueice_ipc::ClientMessage::ActivateExtensionToolbar,
    )
    .unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut frontend).unwrap(),
        blueice_ipc::ServerMessage::ExtensionToolbar {
            label: Some("Clicked".to_string()),
        }
    );
    assert_eq!(
        blueice_ipc::read_server_message(&mut frontend).unwrap(),
        blueice_ipc::ServerMessage::ExtensionPopup {
            popup: Some(blueice_ipc::ExtensionPopup {
                id: 1,
                tab_id: 1,
                title: "Notes".to_string(),
                body: "Saved locally".to_string(),
                action_label: None,
            }),
        }
    );
    assert_eq!(
        blueice_ipc::read_server_message(&mut frontend).unwrap(),
        blueice_ipc::ServerMessage::ExtensionPopup { popup: None }
    );
    assert_eq!(
        blueice_ipc::read_server_message(&mut frontend).unwrap(),
        blueice_ipc::ServerMessage::ExtensionPopup {
            popup: Some(blueice_ipc::ExtensionPopup {
                id: 2,
                tab_id: 1,
                title: "Notes".to_string(),
                body: "Ready to open".to_string(),
                action_label: Some("Open notes".to_string()),
            }),
        }
    );
    blueice_ipc::write_client_message(
        &mut frontend,
        &blueice_ipc::ClientMessage::ActivateExtensionPopupAction { popup_id: 1 },
    )
    .unwrap();
    assert!(
        matches!(blueice_ipc::read_server_message(&mut frontend).unwrap(),
        blueice_ipc::ServerMessage::Error { message }
            if message.contains("no matching live extension popup action"))
    );
    blueice_ipc::write_client_message(
        &mut frontend,
        &blueice_ipc::ClientMessage::ActivateExtensionPopupAction { popup_id: 2 },
    )
    .unwrap();
    assert_eq!(
        blueice_ipc::read_server_message(&mut frontend).unwrap(),
        blueice_ipc::ServerMessage::ExtensionPopup { popup: None }
    );
    assert_eq!(
        blueice_ipc::read_server_message(&mut frontend).unwrap(),
        blueice_ipc::ServerMessage::ExtensionToolbar {
            label: Some("Actioned".to_string()),
        }
    );
    assert_eq!(
        blueice_ipc::read_server_message(&mut frontend).unwrap(),
        blueice_ipc::ServerMessage::ExtensionToolbar { label: None }
    );
    blueice_ipc::write_client_message(&mut frontend, &blueice_ipc::ClientMessage::Shutdown)
        .unwrap();
    assert!(core.child.wait_timeout_or_kill().success());
    assert!(!core_socket.exists());
    assert!(!extension_socket.exists());
    assert!(!frame_dir.exists());
}
