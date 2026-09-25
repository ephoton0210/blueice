// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! End-to-end proof of `phase-7-local-ai/PLAN.md`'s step R9 through the real
//! compiled launcher: an agent's settings proposal is screened by the rule-base,
//! stays pending, and takes effect only when approved over the private pipe of
//! the launcher's own trusted window -- and then at once, on the real assistant
//! process. The native window itself needs a display, so a small stand-in speaks
//! its exact pipe protocol (length-prefixed JSON on stdin/stdout) and is driven
//! by the test through a command directory; everything else is the real
//! launcher, core, gatekeeper, and assistant.

use blueice_assistant_settings::{AssistantSettings, BackendKind, LoopbackSettings};
use blueice_ipc::{
    client_handshake, read_server_message, write_client_message, AiSnapshot, ClientMessage,
    ServerMessage,
};
use blueice_launcher::control::{
    read_control_reply, write_control_request, ControlReply, ControlRequest,
};
use blueice_launcher::trusted_window::{TrustedWindowReply, TrustedWindowRequest};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::os::unix::fs::PermissionsExt;
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

fn names(snapshot: &AiSnapshot) -> Vec<String> {
    snapshot
        .nodes
        .iter()
        .filter_map(|n| n.name.clone())
        .collect()
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

/// The stand-in native window: it speaks the launcher's private pipe protocol
/// and forwards whatever request the test drops into its command directory.
const FAKE_WINDOW: &str = r#"#!/usr/bin/env python3
import json, os, struct, sys, time
d = os.environ["FAKE_WINDOW_DIR"]
out, inp = sys.stdout.buffer, sys.stdin.buffer
def send(obj):
    b = json.dumps(obj).encode()
    out.write(struct.pack("<I", len(b)) + b)
    out.flush()
def recv():
    h = inp.read(4)
    if len(h) < 4:
        sys.exit(0)
    return json.loads(inp.read(struct.unpack("<I", h)[0]))
send("inspect")
recv()
open(os.path.join(d, "ready"), "w").close()
while True:
    for name in sorted(os.listdir(d)):
        if name.startswith("cmd-") and name.endswith(".json"):
            path = os.path.join(d, name)
            request = json.load(open(path))
            os.remove(path)
            send(request)
            reply = recv()
            n = name[len("cmd-"):-len(".json")]
            tmp = os.path.join(d, "reply-%s.tmp" % n)
            json.dump(reply, open(tmp, "w"))
            os.rename(tmp, os.path.join(d, "reply-%s.json" % n))
    time.sleep(0.05)
"#;

struct Stack {
    dir: PathBuf,
    child: Child,
    rendezvous: PathBuf,
    control: PathBuf,
    window: PathBuf,
    next_command: AtomicU64,
}

impl Stack {
    fn start(settings: &Path) -> Stack {
        let dir = unique_path("stack");
        std::fs::create_dir_all(&dir).unwrap();
        let window = dir.join("window");
        std::fs::create_dir_all(&window).unwrap();
        let built = PathBuf::from(env!("CARGO_BIN_EXE_blueice-launcher"));
        let built_dir = built.parent().unwrap();
        let place = |from: &Path, to: &str| {
            let to = dir.join(to);
            if std::fs::hard_link(from, &to).is_err() {
                std::fs::copy(from, &to).unwrap();
            }
        };
        place(&built, "blueice-launcher");
        for sibling in [
            "blueice-core",
            "bluejs",
            "blueice-ai-gatekeeper",
            "blueice-ai-assistant",
        ] {
            let from = built_dir.join(sibling);
            assert!(
                from.exists(),
                "build the workspace first: {} is missing",
                from.display()
            );
            place(&from, sibling);
        }
        let frontend = dir.join("blueice-frontend");
        std::fs::write(&frontend, FAKE_WINDOW).unwrap();
        std::fs::set_permissions(&frontend, std::fs::Permissions::from_mode(0o755)).unwrap();

        let rendezvous = dir.join("rv.sock");
        let control = dir.join("ctl.sock");
        let child = Command::new(dir.join("blueice-launcher"))
            .args(["--socket", rendezvous.to_str().unwrap()])
            .args(["--control-socket", control.to_str().unwrap()])
            .args(["--width", "320", "--height", "200"])
            .args(["--frame-dir", dir.join("frames").to_str().unwrap()])
            .arg("--trusted-frontend")
            .args(["--assistant-settings", settings.to_str().unwrap()])
            .args([
                "--assistant-bin",
                dir.join("blueice-ai-assistant").to_str().unwrap(),
            ])
            .env("FAKE_WINDOW_DIR", &window)
            .env("BLUEICE_TRACE", dir.join("trace.log"))
            .stderr(Stdio::piped())
            .spawn()
            .expect("failed to start the launcher");
        assert!(
            wait_for(&window.join("ready")),
            "the stand-in window never inspected its core"
        );
        assert!(wait_for(&rendezvous) && wait_for(&control));
        Stack {
            dir,
            child,
            rendezvous,
            control,
            window,
            next_command: AtomicU64::new(0),
        }
    }

    fn client(&self) -> Client {
        let mut stream = UnixStream::connect(&self.rendezvous).unwrap();
        client_handshake(&mut stream).unwrap();
        Client(stream)
    }

    /// A request on the operator-control socket: all an agent can reach.
    fn control(&self, request: &ControlRequest) -> ControlReply {
        let mut stream = UnixStream::connect(&self.control).unwrap();
        write_control_request(&mut stream, request).unwrap();
        read_control_reply(&mut stream).unwrap()
    }

    /// A request the person makes in the trusted window (the private pipe).
    fn window(&self, request: &TrustedWindowRequest) -> TrustedWindowReply {
        let n = self.next_command.fetch_add(1, Ordering::SeqCst);
        let staged = self.window.join(format!("stage-{n}"));
        std::fs::write(&staged, serde_json::to_string(request).unwrap()).unwrap();
        std::fs::rename(&staged, self.window.join(format!("cmd-{n}.json"))).unwrap();
        let reply = self.window.join(format!("reply-{n}.json"));
        assert!(wait_for(&reply), "the window never answered");
        serde_json::from_str(&std::fs::read_to_string(reply).unwrap()).unwrap()
    }

    /// Live pids of the real assistant, children of this launcher.
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

impl Drop for Stack {
    fn drop(&mut self) {
        if let Ok(mut stream) = UnixStream::connect(&self.rendezvous) {
            let _ = client_handshake(&mut stream);
            let _ = write_client_message(&mut stream, &ClientMessage::Shutdown);
            let deadline = Instant::now() + Duration::from_secs(10);
            while Instant::now() < deadline && self.child.try_wait().ok().flatten().is_none() {
                thread::sleep(Duration::from_millis(50));
            }
        }
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn proposing(model_url: &str, nice: i32) -> AssistantSettings {
    AssistantSettings {
        backend: BackendKind::Loopback,
        loopback: Some(LoopbackSettings {
            provider: "llamacpp".into(),
            base_url: model_url.into(),
            model: "local".into(),
        }),
        nice,
        ..AssistantSettings::default()
    }
}

/// Makes the assistant run (a translation needs it) and returns its pid.
fn use_the_assistant(stack: &Stack, client: &mut Client, url: &str) -> u32 {
    client.translate_to("zh-TW");
    client.navigate(url);
    assert_eq!(names(&client.snapshot()), ["你好", "世界"]);
    let pids = stack.assistant_pids();
    assert_eq!(pids.len(), 1, "exactly one assistant: {pids:?}");
    pids[0]
}

fn accepted(reply: ControlReply) -> (u64, String) {
    match reply {
        ControlReply::AssistantProposalAccepted { id, digest, .. } => (id, digest),
        other => panic!("expected the proposal to be accepted, got {other:?}"),
    }
}

fn status(stack: &Stack, id: u64) -> String {
    match stack.control(&ControlRequest::AssistantProposalStatus { id }) {
        ControlReply::AssistantProposalStatus { status } => status,
        other => panic!("expected a status, got {other:?}"),
    }
}

#[test]
fn an_agents_proposal_takes_effect_only_after_the_person_approves_it_and_then_at_once() {
    let model = model_base_url();
    let settings = settings_file(&model);
    let stack = Stack::start(settings.path());
    let mut client = stack.client();
    let url = page_url();
    let launcher_nice = niceness(stack.child.id());

    let before = use_the_assistant(&stack, &mut client, &url);
    assert_eq!(niceness(before), launcher_nice + 10);

    // The rule-base blocks a request for higher priority; the person is never asked.
    match stack.control(&ControlRequest::ProposeAssistantSettings {
        settings: proposing(&model, 0),
    }) {
        ControlReply::AssistantProposalBlocked { violations } => {
            assert!(
                violations.iter().any(|v| v.contains("higher priority")),
                "{violations:?}"
            )
        }
        other => panic!("expected Blocked, got {other:?}"),
    }
    let TrustedWindowReply::AssistantSettingsState { pending, .. } =
        stack.window(&TrustedWindowRequest::InspectAssistantSettings)
    else {
        panic!("expected the state")
    };
    assert!(
        pending.is_none(),
        "a blocked proposal never reaches the window"
    );

    // An acceptable one is accepted -- and changes nothing.
    let (id, digest) = accepted(stack.control(&ControlRequest::ProposeAssistantSettings {
        settings: proposing(&model, 12),
    }));
    assert_eq!(status(&stack, id), "pending");
    assert_eq!(
        blueice_assistant_settings::load(settings.path())
            .unwrap()
            .nice,
        10
    );
    assert_eq!(
        stack.assistant_pids(),
        [before],
        "the same assistant, untouched"
    );

    // The agent has no way to approve. The trusted-window request, sent as a
    // frame to the socket an agent can reach, is not even a valid request there.
    {
        let mut stream = UnixStream::connect(&stack.control).unwrap();
        let forged = serde_json::to_vec(&TrustedWindowRequest::ApproveAssistantProposal {
            id,
            digest: digest.clone(),
        })
        .unwrap();
        stream
            .write_all(&(forged.len() as u32).to_le_bytes())
            .unwrap();
        stream.write_all(&forged).unwrap();
        assert!(
            read_control_reply(&mut stream).is_err(),
            "the control socket must not understand an approval"
        );
    }
    assert_eq!(
        status(&stack, id),
        "pending",
        "still waiting for the person"
    );
    assert_eq!(
        blueice_assistant_settings::load(settings.path())
            .unwrap()
            .nice,
        10
    );

    // The person's window sees exactly this proposal.
    let TrustedWindowReply::AssistantSettingsState { current, pending } =
        stack.window(&TrustedWindowRequest::InspectAssistantSettings)
    else {
        panic!("expected the state")
    };
    let pending = pending.expect("the proposal is waiting");
    assert_eq!((pending.id, pending.digest.as_str()), (id, digest.as_str()));
    assert_eq!(pending.diff, ["Priority (nice): 10 -> 12"]);
    assert_eq!(current.nice, 10);

    // An approval naming a different digest is refused and leaves it waiting.
    let refused = stack.window(&TrustedWindowRequest::ApproveAssistantProposal {
        id,
        digest: "0".repeat(64),
    });
    assert!(
        matches!(refused, TrustedWindowReply::Rejected { .. }),
        "{refused:?}"
    );
    assert_eq!(status(&stack, id), "pending");

    // The real approval.
    let approved = stack.window(&TrustedWindowRequest::ApproveAssistantProposal { id, digest });
    let TrustedWindowReply::AssistantSettingsState { current, pending } = approved else {
        panic!("expected the new state, got {approved:?}")
    };
    assert_eq!(current.nice, 12);
    assert!(pending.is_none());
    assert_eq!(status(&stack, id), "approved");
    assert_eq!(
        blueice_assistant_settings::load(settings.path())
            .unwrap()
            .nice,
        12,
        "persisted"
    );

    // At once: the assistant that was running under the old settings is gone, and
    // the next one starts under the new priority.
    let deadline = Instant::now() + Duration::from_secs(5);
    while stack.assistant_pids().contains(&before) && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(50));
    }
    assert!(
        !stack.assistant_pids().contains(&before),
        "the old assistant must be torn down"
    );
    let after = use_the_assistant(&stack, &mut client, &url);
    assert_ne!(after, before);
    assert_eq!(niceness(after), launcher_nice + 12);
}

#[test]
fn a_denied_proposal_changes_nothing_and_a_direct_edit_by_the_person_may_exceed_what_an_agent_can()
{
    let model = model_base_url();
    let settings = settings_file(&model);
    let stack = Stack::start(settings.path());
    let mut client = stack.client();
    let url = page_url();
    let launcher_nice = niceness(stack.child.id());
    let before = use_the_assistant(&stack, &mut client, &url);

    // Denied: nothing changes and the assistant is not restarted.
    let (id, _) = accepted(stack.control(&ControlRequest::ProposeAssistantSettings {
        settings: proposing(&model, 15),
    }));
    let denied = stack.window(&TrustedWindowRequest::DenyAssistantProposal { id });
    assert!(
        matches!(
            denied,
            TrustedWindowReply::AssistantSettingsState { pending: None, .. }
        ),
        "{denied:?}"
    );
    assert_eq!(status(&stack, id), "denied");
    assert_eq!(
        blueice_assistant_settings::load(settings.path())
            .unwrap()
            .nice,
        10
    );
    assert_eq!(stack.assistant_pids(), [before]);

    // A proposal is waiting when the person edits directly: it goes stale.
    let (stale_id, stale_digest) =
        accepted(stack.control(&ControlRequest::ProposeAssistantSettings {
            settings: proposing(&model, 14),
        }));
    // The person may ask for the priority an agent may not (niceness 0).
    let edit = stack.window(&TrustedWindowRequest::EditAssistantSettings {
        settings: proposing(&model, 0),
    });
    let TrustedWindowReply::AssistantSettingsState { current, pending } = edit else {
        panic!("expected the new state, got {edit:?}")
    };
    assert_eq!(current.nice, 0);
    assert!(
        pending.is_none(),
        "an edit retires what an agent proposed against the old settings"
    );
    assert_eq!(status(&stack, stale_id), "stale");
    let refused = stack.window(&TrustedWindowRequest::ApproveAssistantProposal {
        id: stale_id,
        digest: stale_digest,
    });
    assert!(
        matches!(refused, TrustedWindowReply::Rejected { .. }),
        "{refused:?}"
    );

    // And it took effect on the real assistant.
    let deadline = Instant::now() + Duration::from_secs(5);
    while stack.assistant_pids().contains(&before) && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(50));
    }
    let after = use_the_assistant(&stack, &mut client, &url);
    assert_ne!(after, before);
    assert_eq!(niceness(after), launcher_nice);
}

#[test]
fn the_trace_file_tells_the_story_of_a_proposal_without_leaking_settings_or_digests() {
    let model = model_base_url();
    let settings = settings_file(&model);
    let stack = Stack::start(settings.path());
    let mut client = stack.client();
    let url = page_url();
    use_the_assistant(&stack, &mut client, &url);

    let _ = stack.control(&ControlRequest::ProposeAssistantSettings {
        settings: proposing(&model, 0),
    });
    let (id, digest) = accepted(stack.control(&ControlRequest::ProposeAssistantSettings {
        settings: proposing(&model, 12),
    }));
    let TrustedWindowReply::AssistantSettingsState { .. } =
        stack.window(&TrustedWindowRequest::ApproveAssistantProposal {
            id,
            digest: digest.clone(),
        })
    else {
        panic!("the approval was refused")
    };

    let trace = std::fs::read_to_string(stack.dir.join("trace.log")).unwrap();
    for expected in [
        "assistant.spawn",
        "proposal.blocked",
        &format!("proposal.accepted: id {id}"),
        &format!("trusted.request: approve_assistant_proposal {id}"),
        &format!("proposal.approved: id {id}"),
        "assistant.reconfigure",
    ] {
        assert!(trace.contains(expected), "missing {expected:?} in:\n{trace}");
    }
    assert!(!trace.contains(&digest), "the digest leaked into the trace");
    assert!(!trace.contains(&model), "settings leaked into the trace");
    assert!(trace.lines().all(|line| line.starts_with("[+")), "{trace}");
}
