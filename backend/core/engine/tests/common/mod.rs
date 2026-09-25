// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Shared rig for the compiled-`blueice-core` assistant tests: a real HTTP
//! server, a gatekeeper that records what it reviewed, the real
//! `AssistantService` on a private socket, and a `Core` handle.

#![allow(dead_code)] // each test binary uses a different part of the rig

use blueice_ai_assistant::backend::InferenceBackend;
use blueice_ai_assistant::AssistantService;
use blueice_ipc::gatekeeper::{read_gatekeeper_request, write_gatekeeper_reply};
use blueice_ipc::gatekeeper::{GatekeeperReply, GatekeeperRequest};
use blueice_ipc::{AiSnapshot, ClientMessage, ServerMessage};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

pub const PAGE: &str = "<h1>Hello</h1><p>World</p>";

pub fn short_path(label: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("lt-{label}-{}-{n}", std::process::id()))
}

pub fn wait_for(path: &Path) -> bool {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if path.exists() {
            return true;
        }
        thread::sleep(Duration::from_millis(20));
    }
    false
}

/// Serves `PAGE` to every request, on a fresh loopback port.
pub fn web_server() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut stream = stream;
            let mut buf = [0u8; 2048];
            let _ = stream.read(&mut buf);
            let _ = stream.write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{PAGE}",
                    PAGE.len()
                )
                .as_bytes(),
            );
        }
    });
    format!("http://{addr}")
}

/// A gatekeeper that clears everything and records the HTML it reviewed.
pub fn recording_gatekeeper() -> (PathBuf, Arc<Mutex<Vec<String>>>) {
    let path = short_path("gk");
    let listener = UnixListener::bind(&path).unwrap();
    let reviewed = Arc::new(Mutex::new(Vec::new()));
    let record = reviewed.clone();
    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let record = record.clone();
            thread::spawn(move || {
                let mut stream = stream;
                if let Ok(request) = read_gatekeeper_request(&mut stream) {
                    if let GatekeeperRequest::CheckContent { html, .. } = &request {
                        record.lock().unwrap().push(html.clone());
                    }
                    let _ = write_gatekeeper_reply(&mut stream, &GatekeeperReply::Cleared);
                }
            });
        }
    });
    (path, reviewed)
}

/// The real assistant service, backed by `backend`, on a private socket.
pub fn assistant_serving<B: InferenceBackend + 'static>(backend: B) -> PathBuf {
    let path = short_path("as");
    let listener = UnixListener::bind(&path).unwrap();
    thread::spawn(move || {
        let service = Arc::new(AssistantService::new(backend));
        for stream in listener.incoming().flatten() {
            let service = service.clone();
            thread::spawn(move || {
                let mut stream = stream;
                let _ = service.handle_connection(&mut stream);
            });
        }
    });
    path
}

/// An assistant that accepts and acknowledges `Hello`, then never answers.
pub fn silent_assistant() -> PathBuf {
    use blueice_ipc::assistant::{
        read_assistant_request, write_assistant_reply, AssistantReply, ASSISTANT_PROTOCOL_VERSION,
    };
    let path = short_path("sa");
    let listener = UnixListener::bind(&path).unwrap();
    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            thread::spawn(move || {
                let mut stream = stream;
                let _ = read_assistant_request(&mut stream);
                let _ = write_assistant_reply(
                    &mut stream,
                    &AssistantReply::HelloAck {
                        protocol_version: ASSISTANT_PROTOCOL_VERSION,
                    },
                );
                thread::sleep(Duration::from_secs(20));
            });
        }
    });
    path
}

pub struct Core {
    pub child: Child,
    pub stream: UnixStream,
    pub socket: PathBuf,
    pub frames: PathBuf,
}

impl Core {
    pub fn start(gatekeeper: &Path, extra: &[&str]) -> Core {
        let socket = short_path("core");
        let frames = short_path("fr");
        let child = Command::new(env!("CARGO_BIN_EXE_blueice-core"))
            .args(["--socket", socket.to_str().unwrap()])
            .args(["--width", "400", "--height", "200"])
            .args(["--frame-dir", frames.to_str().unwrap()])
            .args(["--gatekeeper-socket", gatekeeper.to_str().unwrap()])
            .args(extra)
            .stderr(Stdio::piped())
            .spawn()
            .expect("failed to spawn blueice-core");
        assert!(wait_for(&socket), "blueice-core never created its socket");
        let mut stream = UnixStream::connect(&socket).unwrap();
        blueice_ipc::client_handshake(&mut stream).unwrap();
        Core {
            child,
            stream,
            socket,
            frames,
        }
    }

    pub fn send(&mut self, message: &ClientMessage) {
        blueice_ipc::write_client_message(&mut self.stream, message).unwrap();
    }

    pub fn read(&mut self) -> ServerMessage {
        blueice_ipc::read_server_message(&mut self.stream).unwrap()
    }

    /// Navigates and returns the generation of the frame that followed.
    pub fn navigate(&mut self, url: &str) -> u64 {
        self.send(&ClientMessage::Navigate { url: url.into() });
        assert!(matches!(self.read(), ServerMessage::Navigated { .. }));
        match self.read() {
            ServerMessage::FrameReady { generation, .. } => generation,
            other => panic!("expected FrameReady, got {other:?}"),
        }
    }

    pub fn snapshot(&mut self) -> AiSnapshot {
        self.send(&ClientMessage::GetRepresentation);
        match self.read() {
            ServerMessage::Representation(snapshot) => snapshot,
            other => panic!("expected Representation, got {other:?}"),
        }
    }
}

impl Drop for Core {
    fn drop(&mut self) {
        let _ = blueice_ipc::write_client_message(&mut self.stream, &ClientMessage::Shutdown);
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.socket);
        let _ = std::fs::remove_dir_all(&self.frames);
    }
}

pub fn names(snapshot: &AiSnapshot) -> Vec<String> {
    snapshot
        .nodes
        .iter()
        .filter_map(|n| n.name.clone())
        .collect()
}
