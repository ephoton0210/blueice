// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Exercises the real `blueice-ai-gatekeeper` binary's private socket
//! override and extension-action request path. The unit rules tests
//! cover classification; this test proves a separately spawned process
//! receives the framed Phase 9 request and returns the reviewed result.

use blueice_ipc::gatekeeper::{
    read_gatekeeper_reply, read_gatekeeper_settings_reply, write_gatekeeper_request,
    write_gatekeeper_settings_request, GatekeeperReply, GatekeeperRequest,
    GatekeeperSettingsChange, GatekeeperSettingsReply, GatekeeperSettingsRequest,
};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::thread;
use std::time::{Duration, Instant};

fn unique_socket_path() -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    blueice_ipc::local_socket::default_socket_dir().join(format!(
        // Darwin Unix-domain sockets have a small SUN_LEN path limit;
        // keep the leaf deliberately terse because the private temp-dir
        // prefix already provides user/session isolation.
        "gk9-{}-{n}",
        std::process::id()
    ))
}

/// A socket pathname can be visible between `bind()` and the process entering
/// `accept()`. Readiness must therefore mean a real client can connect, not
/// merely that the filesystem entry exists.
fn wait_for(path: &std::path::Path, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        match UnixStream::connect(path) {
            Ok(stream) => {
                drop(stream);
                return true;
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused
                ) => {}
            Err(_) => return false,
        }
        thread::sleep(Duration::from_millis(20));
    }
    false
}

struct GatekeeperProcess {
    child: Child,
    socket: PathBuf,
    settings_path: PathBuf,
}

impl GatekeeperProcess {
    fn spawn() -> Self {
        let socket = unique_socket_path();
        let settings_path = socket.with_extension("json");
        let _ = std::fs::remove_file(&socket);
        let child = Command::new(env!("CARGO_BIN_EXE_blueice-ai-gatekeeper"))
            .args([
                "--socket", socket.to_str().unwrap(),
                "--settings", settings_path.to_str().unwrap(),
            ])
            .spawn()
            .expect("failed to spawn blueice-ai-gatekeeper");
        assert!(
            wait_for(&socket, Duration::from_secs(5)),
            "blueice-ai-gatekeeper never created its private socket"
        );
        Self { child, socket, settings_path }
    }

    fn check(&self, detail: &str) -> GatekeeperReply {
        self.review(GatekeeperRequest::CheckExtensionAction {
            extension_id: "minimal-slice-extension".to_string(),
            capability: "dom:write".to_string(),
            detail: detail.to_string(),
        })
    }

    fn review(&self, request: GatekeeperRequest) -> GatekeeperReply {
        let mut stream =
            UnixStream::connect(&self.socket).expect("failed to connect to gatekeeper");
        write_gatekeeper_request(&mut stream, &request).unwrap();
        read_gatekeeper_reply(&mut stream).unwrap()
    }

    fn update(&self, change: GatekeeperSettingsChange) -> GatekeeperSettingsReply {
        let mut stream =
            UnixStream::connect(&self.socket).expect("failed to connect to gatekeeper");
        write_gatekeeper_settings_request(
            &mut stream,
            &GatekeeperSettingsRequest::Update { change },
        ).unwrap();
        read_gatekeeper_settings_reply(&mut stream).unwrap()
    }
}

impl Drop for GatekeeperProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.socket);
        let _ = std::fs::remove_file(&self.settings_path);
    }
}

#[test]
fn real_gatekeeper_process_applies_persisted_settings_to_the_next_review() {
    let gatekeeper = GatekeeperProcess::spawn();
    assert!(matches!(gatekeeper.update(GatekeeperSettingsChange::AddBlockedDownloadExtension {
        extension: ".zip".to_string(),
    }), GatekeeperSettingsReply::Settings(settings)
        if settings.custom_blocked_download_extensions == [".zip"]));
    assert!(gatekeeper.settings_path.exists());
    assert!(matches!(gatekeeper.review(GatekeeperRequest::CheckDownload {
        url: "https://safe.example/file.zip".to_string(),
        file_name: "file.zip".to_string(),
        content_type: None,
        total_bytes: None,
    }), GatekeeperReply::Rejected { category, .. }
        if category == "custom-blocked-download-extension"));
    assert!(matches!(gatekeeper.update(GatekeeperSettingsChange::AddBlockedPopupPhrase {
        phrase: "send secrets".to_string(),
    }), GatekeeperSettingsReply::Settings(settings)
        if settings.custom_blocked_popup_phrases == ["send secrets"]));
    assert!(matches!(gatekeeper.review(GatekeeperRequest::CheckExtensionAction {
        extension_id: "minimal-slice-extension".to_string(),
        capability: "ui:inject".to_string(),
        detail: "action=show-native-popup; title=Send secrets; body=Now".to_string(),
    }), GatekeeperReply::Rejected { category, .. }
        if category == "custom-blocked-popup-phrase"));
}

#[test]
fn real_gatekeeper_process_reviews_extension_actions_on_an_overridden_private_socket() {
    let gatekeeper = GatekeeperProcess::spawn();
    assert_eq!(
        gatekeeper.check("target=form-input; input_type=email"),
        GatekeeperReply::Cleared
    );
    assert!(matches!(
        gatekeeper.check("target=form-input; input_type=password"),
        GatekeeperReply::Rejected { category, .. } if category == "sensitive-extension-action"
    ));
}
