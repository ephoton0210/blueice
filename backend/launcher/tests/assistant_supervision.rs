// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! End-to-end proof of `phase-7-local-ai/PLAN.md`'s step R2 through the real
//! compiled `blueice-launcher`: it owns the assistant's public socket, starts
//! the real `blueice-ai-assistant` only when translation first needs it,
//! reuses it, replaces one that dies, and keeps `core` connected to it across
//! a cutover. The model is a scripted loopback server and the page a local web
//! server, so nothing leaves the machine.

use blueice_assistant_settings::{AssistantSettings, BackendKind, LoopbackSettings};
use blueice_ipc::{
    client_handshake, read_server_message, write_client_message, AiSnapshot, ClientMessage,
    ServerMessage,
};
use blueice_launcher::control::{
    read_control_reply, write_control_request, ControlReply, ControlRequest,
};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

const PAGE: &str = "<h1>Hello</h1><p>World</p>";

fn unique_path(label: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("l-as-e2e-{label}-{}-{n}", std::process::id()))
}

fn wait_for(path: &Path) -> bool {
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        if path.exists() {
            return true;
        }
        thread::sleep(Duration::from_millis(20));
    }
    false
}

/// Reads one HTTP request (headers and any body) so a reply can follow.
fn read_request(stream: &mut std::net::TcpStream) -> bool {
    let mut bytes = Vec::new();
    let header_end = loop {
        let mut chunk = [0u8; 4096];
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return false,
            Ok(n) => bytes.extend_from_slice(&chunk[..n]),
        }
        if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
            break end + 4;
        }
    };
    let length: usize = String::from_utf8_lossy(&bytes[..header_end])
        .lines()
        .find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.eq_ignore_ascii_case("content-length")
                .then(|| v.trim().parse().unwrap_or(0))
        })
        .unwrap_or(0);
    while bytes.len() - header_end < length {
        let mut chunk = [0u8; 4096];
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return false,
            Ok(n) => bytes.extend_from_slice(&chunk[..n]),
        }
    }
    true
}

fn serve(body: &'static str, content_type: &'static str) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            thread::spawn(move || {
                let mut stream = stream;
                if read_request(&mut stream) {
                    let _ = stream.write_all(
                        format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                            body.len()
                        )
                        .as_bytes(),
                    );
                }
            });
        }
    });
    format!("127.0.0.1:{}", addr.port())
}

/// A scripted OpenAI-compatible model that always "translates" the two-text page.
fn model_base_url() -> String {
    // The chat reply's `content` is itself the JSON array the assistant expects.
    let addr = serve(
        r#"{"choices":[{"message":{"content":"[\"你好\",\"世界\"]"}}]}"#,
        "application/json",
    );
    format!("http://{addr}/v1/")
}

/// A settings file removed when dropped.
struct SettingsFile(PathBuf);

impl SettingsFile {
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for SettingsFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn settings_file(model_url: &str) -> SettingsFile {
    let path = unique_path("settings.json");
    let settings = AssistantSettings {
        backend: BackendKind::Loopback,
        loopback: Some(LoopbackSettings {
            provider: "llamacpp".into(),
            base_url: model_url.into(),
            model: "local".into(),
        }),
        ..AssistantSettings::default()
    };
    blueice_assistant_settings::save(&path, &settings).unwrap();
    SettingsFile(path)
}

fn sibling_assistant() -> PathBuf {
    let launcher = PathBuf::from(env!("CARGO_BIN_EXE_blueice-launcher"));
    let binary = launcher.parent().unwrap().join("blueice-ai-assistant");
    assert!(
        binary.exists(),
        "build the workspace first (cargo build --workspace): {} is missing",
        binary.display()
    );
    binary
}

struct Launcher {
    child: Child,
    rendezvous: PathBuf,
    control: PathBuf,
}

impl Launcher {
    fn spawn(extra: &[&str]) -> Launcher {
        let rendezvous = unique_path("rv.sock");
        let control = unique_path("ctl.sock");
        let frames = unique_path("frames");
        let child = Command::new(env!("CARGO_BIN_EXE_blueice-launcher"))
            .args(["--socket", rendezvous.to_str().unwrap()])
            .args(["--control-socket", control.to_str().unwrap()])
            .args(["--width", "320", "--height", "200"])
            .args(["--frame-dir", frames.to_str().unwrap()])
            .args(extra)
            .stderr(Stdio::piped())
            .spawn()
            .expect("failed to spawn blueice-launcher");
        assert!(wait_for(&rendezvous), "the launcher never listened");
        assert!(
            wait_for(&control),
            "the launcher never opened its control socket"
        );
        Launcher {
            child,
            rendezvous,
            control,
        }
    }

    fn with_assistant(settings: &Path) -> Launcher {
        let assistant = sibling_assistant();
        Self::spawn(&[
            "--assistant-settings",
            settings.to_str().unwrap(),
            "--assistant-bin",
            assistant.to_str().unwrap(),
        ])
    }

    fn client(&self) -> Client {
        let mut stream = UnixStream::connect(&self.rendezvous).unwrap();
        client_handshake(&mut stream).unwrap();
        Client(stream)
    }

    /// Live pids of `blueice-ai-assistant` children of this launcher.
    fn assistant_pids(&self) -> Vec<u32> {
        let output = Command::new("pgrep")
            .args([
                "-P",
                &self.child.id().to_string(),
                "-f",
                "blueice-ai-assistant",
            ])
            .output()
            .expect("pgrep is needed for this test");
        String::from_utf8_lossy(&output.stdout)
            .split_whitespace()
            .filter_map(|pid| pid.parse().ok())
            .collect()
    }
}

impl Drop for Launcher {
    fn drop(&mut self) {
        // Shutdown reaches core, which ends the launcher; fall back to a kill.
        // (The sockets are removed only afterwards: the shutdown travels
        // through the rendezvous socket.)
        let _connection = UnixStream::connect(&self.rendezvous)
            .ok()
            .map(|mut stream| {
                let _ = client_handshake(&mut stream);
                let _ = write_client_message(&mut stream, &ClientMessage::Shutdown);
                stream
            });
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if self.child.try_wait().ok().flatten().is_some() {
                break;
            }
            thread::sleep(Duration::from_millis(50));
        }
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
        let _ = std::fs::remove_file(&self.rendezvous);
        let _ = std::fs::remove_file(&self.control);
    }
}

struct Client(UnixStream);

impl Client {
    fn send(&mut self, message: &ClientMessage) {
        write_client_message(&mut self.0, message).unwrap();
    }

    fn read(&mut self) -> ServerMessage {
        read_server_message(&mut self.0).unwrap()
    }

    fn navigate(&mut self, url: &str) {
        self.send(&ClientMessage::Navigate { url: url.into() });
        loop {
            match self.read() {
                ServerMessage::Navigated { .. } => break,
                ServerMessage::Error { message }
                | ServerMessage::GatekeeperBlocked {
                    reason: message, ..
                } => {
                    panic!("navigation failed: {message}")
                }
                _ => {}
            }
        }
    }

    fn snapshot(&mut self) -> AiSnapshot {
        self.send(&ClientMessage::GetRepresentation);
        loop {
            if let ServerMessage::Representation(snapshot) = self.read() {
                return snapshot;
            }
        }
    }

    fn translate_to(&mut self, tag: &str) {
        self.send(&ClientMessage::SetTranslationLanguage {
            target_language: Some(tag.into()),
        });
        loop {
            match self.read() {
                ServerMessage::TranslationState { .. } => return,
                ServerMessage::Error { message } => panic!("translation refused: {message}"),
                _ => {}
            }
        }
    }
}

fn names(snapshot: &AiSnapshot) -> Vec<String> {
    snapshot
        .nodes
        .iter()
        .filter_map(|n| n.name.clone())
        .collect()
}

/// A live process's niceness, read the portable way.
fn niceness(pid: u32) -> i32 {
    let output = Command::new("ps")
        .args(["-o", "ni=", "-p", &pid.to_string()])
        .output()
        .expect("ps is needed for this test");
    String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .expect("a niceness")
}

fn page_url() -> String {
    format!("http://{}", serve(PAGE, "text/html"))
}

#[test]
fn the_assistant_starts_on_first_use_translates_and_is_then_reused() {
    let model = model_base_url();
    let settings = settings_file(&model);
    let launcher = Launcher::with_assistant(settings.path());
    let mut client = launcher.client();
    let url = page_url();

    // Nothing is running until something needs it.
    assert!(
        launcher.assistant_pids().is_empty(),
        "the assistant must start on demand"
    );
    client.translate_to("zh-TW");
    assert!(
        launcher.assistant_pids().is_empty(),
        "choosing a language is not use"
    );

    client.navigate(&url);
    assert_eq!(names(&client.snapshot()), ["你好", "世界"]);
    let first = launcher.assistant_pids();
    assert_eq!(first.len(), 1, "exactly one assistant: {first:?}");
    // The default setting lowers its priority (niceness 10) relative to the
    // launcher that started it.
    assert_eq!(niceness(first[0]), niceness(launcher.child.id()) + 10);

    client.navigate(&url);
    assert_eq!(names(&client.snapshot()), ["你好", "世界"]);
    assert_eq!(
        launcher.assistant_pids(),
        first,
        "the same assistant serves the next page"
    );
}

#[test]
fn an_assistant_that_dies_is_replaced_by_the_next_translation() {
    let model = model_base_url();
    let settings = settings_file(&model);
    let launcher = Launcher::with_assistant(settings.path());
    let mut client = launcher.client();
    let url = page_url();
    client.translate_to("zh-TW");
    client.navigate(&url);
    let first = launcher.assistant_pids();
    assert_eq!(first.len(), 1);

    let killed = Command::new("kill")
        .args(["-9", &first[0].to_string()])
        .status()
        .unwrap();
    assert!(killed.success());
    let deadline = Instant::now() + Duration::from_secs(5);
    while launcher.assistant_pids().contains(&first[0]) && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(20));
    }

    client.navigate(&url);
    assert_eq!(
        names(&client.snapshot()),
        ["你好", "世界"],
        "translation resumes"
    );
    let second = launcher.assistant_pids();
    assert_eq!(second.len(), 1);
    assert_ne!(second, first, "a new assistant replaced the dead one");
}

#[test]
fn a_cutover_keeps_the_replacement_core_connected_to_the_same_assistant() {
    let model = model_base_url();
    let settings = settings_file(&model);
    let launcher = Launcher::with_assistant(settings.path());
    let mut client = launcher.client();
    let url = page_url();
    client.translate_to("zh-TW");
    client.navigate(&url);
    assert_eq!(names(&client.snapshot()), ["你好", "世界"]);

    let mut control = UnixStream::connect(&launcher.control).unwrap();
    write_control_request(&mut control, &ControlRequest::Cutover).unwrap();
    match read_control_reply(&mut control).unwrap() {
        ControlReply::CutoverDone { .. } => {}
        other => panic!("expected CutoverDone, got {other:?}"),
    }

    // The new core starts with translation off (the setting is per core), but
    // it has the assistant: turning it on works and translates.
    client.translate_to("zh-TW");
    client.navigate(&url);
    assert_eq!(names(&client.snapshot()), ["你好", "世界"]);
}

#[test]
fn without_assistant_settings_translation_is_unavailable() {
    let launcher = Launcher::spawn(&[]);
    let mut client = launcher.client();
    client.send(&ClientMessage::SetTranslationLanguage {
        target_language: Some("zh-TW".into()),
    });
    match client.read() {
        ServerMessage::Error { message } => assert!(message.contains("unavailable"), "{message}"),
        other => panic!("expected Error, got {other:?}"),
    }
    assert!(launcher.assistant_pids().is_empty());
}

#[test]
fn a_settings_file_that_is_present_but_invalid_stops_the_launcher() {
    let path = unique_path("bad-settings.json");
    std::fs::write(
        &path,
        r#"{"version":1,"backend":"loopback","idle_timeout_secs":600,"nice":10}"#,
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_blueice-launcher"))
        .args(["--socket", unique_path("rv.sock").to_str().unwrap()])
        .args([
            "--control-socket",
            unique_path("ctl.sock").to_str().unwrap(),
        ])
        .args(["--assistant-settings", path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("assistant settings"), "{stderr}");
}
