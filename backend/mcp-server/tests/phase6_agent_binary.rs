// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Exercises the compiled Phase 6 runner through a real launcher-owned core,
//! gatekeeper, stdio MCP server, and independent frame observer. The local
//! Chat Completions peer is scripted so this test is repeatable; it is not the
//! outstanding proof of a real model and a visible human window.

use blueice_ipc::{
    client_handshake, read_server_message_with_ids, write_client_message, ClientMessage,
    ServerMessage,
};
use blueice_launcher::{run_broker, SpawnedCore, SpawnedGatekeeper};
use serde_json::{json, Value};
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

static NEXT_RUN: AtomicU64 = AtomicU64::new(1);

struct SharedCore {
    directory: PathBuf,
    socket: PathBuf,
    worker: Option<JoinHandle<()>>,
}

impl SharedCore {
    fn start() -> Self {
        let directory = std::env::temp_dir().join(format!(
            "p6-e2e-{}-{}",
            std::process::id(),
            NEXT_RUN.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&directory).unwrap();
        let socket = directory.join("launcher.sock");
        let control = directory.join("control.sock");
        let frames = directory.join("frames");
        let gatekeeper = SpawnedGatekeeper::spawn().unwrap();
        let gatekeeper_socket = gatekeeper.socket_path().to_path_buf();
        let core =
            SpawnedCore::spawn_with_gatekeeper(800.0, 600.0, &frames, &gatekeeper_socket).unwrap();
        let listener = UnixListener::bind(&socket).unwrap();
        let control_listener = UnixListener::bind(&control).unwrap();
        let worker = thread::spawn(move || {
            run_broker(
                listener,
                control_listener,
                core,
                800.0,
                600.0,
                gatekeeper_socket,
            )
            .unwrap();
            drop(gatekeeper);
        });
        Self {
            directory,
            socket,
            worker: Some(worker),
        }
    }
}

impl Drop for SharedCore {
    fn drop(&mut self) {
        if let Ok(mut stream) = UnixStream::connect(&self.socket) {
            if client_handshake(&mut stream).is_ok() {
                let _ = write_client_message(&mut stream, &ClientMessage::Shutdown);
            }
        }
        if let Some(worker) = self.worker.take() {
            worker.join().unwrap();
        }
        fs::remove_dir_all(&self.directory).unwrap();
    }
}

fn accept_before(listener: &TcpListener, deadline: Instant) -> TcpStream {
    listener.set_nonblocking(true).unwrap();
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                stream.set_nonblocking(false).unwrap();
                return stream;
            }
            Err(error)
                if error.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < deadline =>
            {
                thread::sleep(Duration::from_millis(10));
            }
            Err(error) => panic!("timed out accepting loopback test connection: {error}"),
        }
    }
}

fn read_http_request(stream: &mut TcpStream) -> (String, Vec<u8>) {
    stream
        .set_read_timeout(Some(Duration::from_secs(15)))
        .unwrap();
    let mut reader = BufReader::new(&mut *stream);
    let mut first = String::new();
    reader.read_line(&mut first).unwrap();
    let mut content_length = 0;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        if line == "\r\n" {
            break;
        }
        if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
            content_length = value.trim().parse::<usize>().unwrap();
        }
    }
    assert!(content_length < 8 * 1024 * 1024);
    let mut body = vec![0; content_length];
    reader.read_exact(&mut body).unwrap();
    (first, body)
}

fn reply_json(stream: &mut TcpStream, value: &Value) {
    let body = value.to_string();
    write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
}

fn spawn_demo_site(listener: TcpListener) -> JoinHandle<()> {
    thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(30);
        for (path, body) in [
            ("/index.html", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../development/browser_core/phase-6-ai-agent-integration-demo/demo-site/index.html"))),
            ("/complete.html", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../development/browser_core/phase-6-ai-agent-integration-demo/demo-site/complete.html"))),
        ] {
            let mut stream = accept_before(&listener, deadline);
            let (request, _) = read_http_request(&mut stream);
            assert!(request.starts_with(&format!("GET {path} ")), "{request}");
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        }
    })
}

fn spawn_scripted_model(listener: TcpListener, provider: &'static str) -> JoinHandle<()> {
    thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(45);
        let actions = [
            "navigate_demo",
            "inspect_page",
            "take_screenshot",
            "set_name_to_blueice",
            "highlight_name",
            "take_screenshot",
            "continue_to_confirmation",
        ];
        for (turn, action) in actions.into_iter().enumerate() {
            let mut stream = accept_before(&listener, deadline);
            let (request, body) = read_http_request(&mut stream);
            assert!(
                request.starts_with("POST /v1/chat/completions "),
                "{request}"
            );
            let request: Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(request["tool_choice"], "auto");
            assert_eq!(request["model"], "scripted-local-model");
            let messages = request["messages"].as_array().unwrap();
            if turn > 0 {
                let expected_call_id = if provider == "huggingface" {
                    (turn - 1).to_string()
                } else {
                    format!("call-{}", turn - 1)
                };
                assert!(messages.iter().any(|message| {
                    message["role"] == "tool" && message["tool_call_id"] == expected_call_id
                }));
            }
            if turn >= 3 {
                assert!(messages.iter().any(|message| {
                    message["content"].as_array().is_some_and(|content| {
                        content.iter().any(|part| {
                            part["type"] == "image_url"
                                && part["image_url"]["url"]
                                    .as_str()
                                    .is_some_and(|url| url.starts_with("data:image/png;base64,"))
                        })
                    })
                }));
            }
            let call = if provider == "huggingface" {
                json!({
                    "id": turn, "type": "function",
                    "function": { "name": action, "parameters": {} }
                })
            } else {
                json!({
                    "id": format!("call-{turn}"), "type": "function",
                    "function": { "name": action, "arguments": "{}" }
                })
            };
            let calls = if provider == "huggingface" {
                call
            } else {
                json!([call])
            };
            reply_json(
                &mut stream,
                &json!({
                    "choices": [{ "message": {
                        "role": "assistant", "content": null,
                        "tool_calls": calls
                    }}]
                }),
            );
        }
        let mut stream = accept_before(&listener, deadline);
        let (_, body) = read_http_request(&mut stream);
        let request: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(request["tool_choice"], "none");
        reply_json(
            &mut stream,
            &json!({
                "choices": [{ "message": {
                    "role": "assistant", "content": "The shared BlueIce demo reached Task complete."
                }}]
            }),
        );
    })
}

fn observe_frames(
    socket: &Path,
    stop: Arc<AtomicBool>,
    frames: Arc<Mutex<Vec<(u64, u64)>>>,
) -> JoinHandle<()> {
    let mut stream = UnixStream::connect(socket).unwrap();
    client_handshake(&mut stream).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_millis(200)))
        .unwrap();
    thread::spawn(move || {
        while !stop.load(Ordering::Relaxed) {
            match read_server_message_with_ids(&mut stream) {
                Ok((Some(tab_id), _, ServerMessage::FrameReady { generation, .. })) => {
                    frames.lock().unwrap().push((tab_id, generation));
                }
                Ok(_) => {}
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) => {}
                Err(error) => panic!("shared frame observer lost its connection: {error}"),
            }
        }
    })
}

fn run_scripted_provider(provider: &'static str) {
    let shared = SharedCore::start();
    let demo = TcpListener::bind("127.0.0.1:0").unwrap();
    let demo_url = format!("http://{}/index.html", demo.local_addr().unwrap());
    let demo_worker = spawn_demo_site(demo);
    let model = TcpListener::bind("127.0.0.1:0").unwrap();
    let model_base = format!("http://{}/v1/", model.local_addr().unwrap());
    let model_worker = spawn_scripted_model(model, provider);
    let stop_observer = Arc::new(AtomicBool::new(false));
    let frames = Arc::new(Mutex::new(Vec::new()));
    let observer = observe_frames(&shared.socket, stop_observer.clone(), frames.clone());
    let transcript = shared.directory.join("agent.jsonl");
    let evidence = shared.directory.join("evidence");

    let mut command = Command::new(env!("CARGO_BIN_EXE_blueice-phase6-agent"));
    command
        .arg("--provider")
        .arg(provider)
        .arg("--model")
        .arg("scripted-local-model")
        .arg("--demo-url")
        .arg(demo_url)
        .arg("--launcher-socket")
        .arg(&shared.socket)
        .arg("--mcp-server")
        .arg(env!("CARGO_BIN_EXE_blueice-mcp-server"))
        .arg("--transcript")
        .arg(&transcript)
        .arg("--evidence-dir")
        .arg(&evidence)
        .arg("--highlight-hold-seconds")
        .arg("1")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if provider == "huggingface" {
        command.arg("--huggingface-base").arg(model_base);
    } else {
        command.arg("--ollama-base").arg(model_base);
    }
    let mut child = command.spawn().unwrap();
    let deadline = Instant::now() + Duration::from_secs(45);
    while child.try_wait().unwrap().is_none() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(50));
    }
    if child.try_wait().unwrap().is_none() {
        child.kill().unwrap();
    }
    let output = child.wait_with_output().unwrap();
    stop_observer.store(true, Ordering::Relaxed);
    observer.join().unwrap();
    assert!(
        output.status.success(),
        "agent failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    model_worker.join().unwrap();
    demo_worker.join().unwrap();

    let events: Vec<Value> = fs::read_to_string(&transcript)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let highlight = events
        .iter()
        .find(|event| event["kind"] == "highlight_frame")
        .unwrap();
    let tab_id = highlight["data"]["tab_id"].as_u64().unwrap();
    let generation = highlight["data"]["generation"].as_u64().unwrap();
    assert!(
        frames.lock().unwrap().contains(&(tab_id, generation)),
        "an independent launcher client never received the model's highlighted core frame"
    );
    let saved: Vec<&Value> = events
        .iter()
        .filter(|event| event["kind"] == "evidence_saved")
        .collect();
    assert_eq!(saved.len(), 2);
    assert_eq!(saved[1]["data"]["tab_id"], tab_id);
    assert_eq!(saved[1]["data"]["generation"], generation);
    for entry in saved {
        assert!(Path::new(entry["data"]["path"].as_str().unwrap()).is_file());
    }
    assert!(events.iter().any(|event| event["kind"] == "run_complete"));
}

#[test]
fn scripted_ollama_and_huggingface_peers_drive_real_mcp_and_one_shared_core_each() {
    for provider in ["ollama", "huggingface"] {
        run_scripted_provider(provider);
    }
}
