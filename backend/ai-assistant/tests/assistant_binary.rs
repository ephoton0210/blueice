// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Drives the real `blueice-ai-assistant` binary over its private socket with
//! a scripted loopback model server: the whole path a `core` client will use
//! (connect, `Hello`, translate) without any mocking of the process boundary.

use blueice_ipc::assistant::{
    read_assistant_reply, write_assistant_request, AssistantReply, AssistantRequest,
    ASSISTANT_PROTOCOL_VERSION,
};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::thread;
use std::time::{Duration, Instant};

fn unique_socket_path() -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    blueice_ipc::local_socket::default_socket_dir().join(format!("as9-{}-{n}", std::process::id()))
}

fn wait_for(path: &Path) -> bool {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if UnixStream::connect(path).is_ok() {
            return true;
        }
        thread::sleep(Duration::from_millis(20));
    }
    false
}

struct Assistant(Child, PathBuf);

impl Drop for Assistant {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
        let _ = std::fs::remove_file(&self.1);
    }
}

fn spawn(extra: &[String]) -> (Assistant, UnixStream) {
    let socket = unique_socket_path();
    let child = Command::new(env!("CARGO_BIN_EXE_blueice-ai-assistant"))
        .arg("--socket")
        .arg(&socket)
        .args(extra)
        .spawn()
        .unwrap();
    let assistant = Assistant(child, socket.clone());
    assert!(wait_for(&socket), "assistant did not start listening");
    let mut client = UnixStream::connect(&socket).unwrap();
    write_assistant_request(
        &mut client,
        &AssistantRequest::Hello {
            protocol_version: ASSISTANT_PROTOCOL_VERSION,
        },
    )
    .unwrap();
    assert!(matches!(
        read_assistant_reply(&mut client).unwrap(),
        AssistantReply::HelloAck { .. }
    ));
    (assistant, client)
}

fn translate(request_id: u64) -> AssistantRequest {
    AssistantRequest::Translate {
        request_id,
        target_language: "zh-TW".into(),
        texts: vec!["Hello".into(), "World".into()],
    }
}

/// A one-shot OpenAI-compatible server answering with `content`.
fn scripted_model(content: &'static str) -> (String, thread::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let worker = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut bytes = Vec::new();
        let header_end = loop {
            let mut chunk = [0u8; 4096];
            let n = stream.read(&mut chunk).unwrap();
            assert!(n > 0);
            bytes.extend_from_slice(&chunk[..n]);
            if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                break end + 4;
            }
        };
        let headers = String::from_utf8_lossy(&bytes[..header_end]).to_string();
        let length: usize = headers
            .lines()
            .find_map(|l| {
                let (k, v) = l.split_once(':')?;
                k.eq_ignore_ascii_case("content-length")
                    .then(|| v.trim().parse().unwrap())
            })
            .unwrap();
        while bytes.len() - header_end < length {
            let mut chunk = [0u8; 4096];
            let n = stream.read(&mut chunk).unwrap();
            assert!(n > 0);
            bytes.extend_from_slice(&chunk[..n]);
        }
        let body = serde_json::json!({"choices":[{"message":{"content":content}}]}).to_string();
        stream
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            )
            .unwrap();
        String::from_utf8_lossy(&bytes[header_end..]).to_string()
    });
    (format!("http://127.0.0.1:{port}/v1/"), worker)
}

#[test]
fn translates_through_the_real_process_and_a_loopback_model() {
    let (base_url, model) = scripted_model(r#"["你好","世界"]"#);
    let (_assistant, mut client) = spawn(&[
        "--model-provider".into(),
        "llamacpp".into(),
        "--model-base-url".into(),
        base_url,
        "--model-name".into(),
        "local".into(),
    ]);
    write_assistant_request(&mut client, &translate(11)).unwrap();
    assert_eq!(
        read_assistant_reply(&mut client).unwrap(),
        AssistantReply::Translated {
            request_id: 11,
            texts: vec!["你好".into(), "世界".into()]
        }
    );
    let sent: serde_json::Value = serde_json::from_str(&model.join().unwrap()).unwrap();
    assert_eq!(sent["messages"][1]["content"], r#"["Hello","World"]"#);
}

#[test]
fn without_a_model_every_task_fails_open_with_a_reason() {
    let (_assistant, mut client) = spawn(&[]);
    write_assistant_request(&mut client, &translate(5)).unwrap();
    match read_assistant_reply(&mut client).unwrap() {
        AssistantReply::Failed { request_id, reason } => {
            assert_eq!(request_id, 5);
            assert!(reason.contains("no local model"));
        }
        other => panic!("expected Failed, got {other:?}"),
    }
}

#[test]
fn a_non_loopback_model_endpoint_stops_the_process_at_startup() {
    let status = Command::new(env!("CARGO_BIN_EXE_blueice-ai-assistant"))
        .args([
            "--model-provider",
            "ollama",
            "--model-base-url",
            "http://example.com:80/v1/",
            "--model-name",
            "m",
        ])
        .status()
        .unwrap();
    assert!(!status.success());
}

/// Startup refusals report a reason on stderr and exit unsuccessfully; none of
/// them may leave a listening assistant behind.
fn refused_at_startup(args: &[&str]) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_blueice-ai-assistant"))
        .args(args)
        .output()
        .unwrap();
    assert!(!output.status.success(), "{args:?} must not start");
    String::from_utf8_lossy(&output.stderr).to_string()
}

#[test]
fn both_model_kinds_without_a_named_backend_stop_startup() {
    let stderr = refused_at_startup(&[
        "--model-provider",
        "llamacpp",
        "--model-base-url",
        "http://127.0.0.1:8080/v1/",
        "--model-name",
        "m",
        "--candle-model",
        "/nonexistent/m.gguf",
        "--candle-tokenizer",
        "/nonexistent/t.json",
    ]);
    assert!(stderr.contains("double the resources"), "{stderr}");
}

#[test]
fn a_candle_model_that_cannot_be_loaded_stops_startup_in_every_build() {
    // Without the `candle` feature the build says so; with it, the missing
    // file is reported. Either way the process refuses rather than serving
    // requests it can only fail.
    let stderr = refused_at_startup(&[
        "--backend",
        "candle",
        "--candle-model",
        "/nonexistent/m.gguf",
        "--candle-tokenizer",
        "/nonexistent/t.json",
    ]);
    assert!(
        stderr.contains("--features candle") || stderr.contains("cannot open the model file"),
        "{stderr}"
    );
}

#[test]
fn simultaneous_mode_that_cannot_load_its_candle_half_stops_startup() {
    // The loopback half alone would have started; the pair must not silently
    // degrade to it.
    let stderr = refused_at_startup(&[
        "--backend",
        "both",
        "--model-provider",
        "llamacpp",
        "--model-base-url",
        "http://127.0.0.1:8080/v1/",
        "--model-name",
        "m",
        "--candle-model",
        "/nonexistent/m.gguf",
        "--candle-tokenizer",
        "/nonexistent/t.json",
    ]);
    assert!(
        stderr.contains("--features candle") || stderr.contains("cannot open the model file"),
        "{stderr}"
    );
}
