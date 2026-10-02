// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;
use blueice_ipc::permission_control::{
    read_permission_control_request, write_permission_control_reply,
};
use blueice_ipc::*;

// ---- bounded retry (`phase-8-live-core-hotswap/PLAN.md`) ----

fn no_pause(_: Duration) {}

// ---- structural per-tab health diff ----

/// A snapshot with `n` nodes, for counting.
fn snapshot_with_nodes(n: usize) -> blueice_ipc::AiSnapshot {
    use blueice_ipc::{AiNode, Bounds, NodeState, Role};
    blueice_ipc::AiSnapshot {
        frame_source: 0,
        generation: 1,
        tab_id: 1,
        url: None,
        scroll_y: 0.0,
        nodes: (0..n as u64)
            .map(|id| AiNode {
                id,
                parent: None,
                children: vec![],
                role: Role::Paragraph,
                name: None,
                name_from: None,
                original_name: None,
                state: NodeState::default(),
                bounds: Bounds {
                    x: 0.0,
                    y: 0.0,
                    width: 1.0,
                    height: 1.0,
                },
                opacity: 1.0,
                occluded: false,
                occluded_by: None,
                occluded_fraction: 0.0,
            })
            .collect(),
    }
}

fn tab(id: u64) -> TabSummary {
    TabSummary {
        id,
        url: Some(format!("about:tab{id}")),
        group_id: None,
    }
}

/// A v2 stand-in answering each `GetRepresentation` with the next canned
/// reply, recording which tab each was addressed to.
fn fake_v2(replies: Vec<ServerMessage>) -> (UnixStream, thread::JoinHandle<Vec<Option<u64>>>) {
    let (client, mut server) = UnixStream::pair().unwrap();
    let handle = thread::spawn(move || {
        let mut asked = Vec::new();
        for reply in replies {
            let (tab_id, request_id, message) = read_client_message_with_ids(&mut server).unwrap();
            assert!(matches!(message, ClientMessage::GetRepresentation));
            asked.push(tab_id);
            write_server_message_with_id(&mut server, request_id, &reply).unwrap();
        }
        asked
    });
    (client, handle)
}

// ---- assistant settings over the trusted-window pipe ----

/// A broker around a fake core, with or without an assistant settings owner.
/// Assistant requests never touch the core, so a stand-in is enough.
fn broker_with_settings(
    service: Option<Arc<assistant_settings_service::AssistantSettingsService>>,
) -> Arc<Broker> {
    let root = std::env::temp_dir().join(format!(
        "trusted-assistant-{}-{}",
        std::process::id(),
        synthetic_request_id()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let (core_stream, _peer) = UnixStream::pair().unwrap();
    let fake_child = || Command::new("true").spawn().unwrap();
    let core = SpawnedCore {
        child: fake_child(),
        internal_socket_path: root.join("core.sock"),
        extension_socket_path: None,
        script_private_socket_path: None,
        compiler_private_socket_path: None,
        debugger_private_socket_path: None,
        options: CoreLaunchOptions::default(),
        bluejs_host: None,
        route_gate: Arc::new(Mutex::new(())),
        compiler_mcp_relay: None,
        debugger_relay: None,
        permission_control: None,
        frame_dir: root.join("frames"),
        stream: core_stream,
    };
    let (done, _done_rx) = mpsc::channel();
    Arc::new(Broker {
        core_writer: Arc::new(Mutex::new(core.stream.try_clone().unwrap())),
        clients: Arc::new(Mutex::new(Vec::new())),
        generation: Arc::new(AtomicU64::new(1)),
        cutover_gate: CutoverGate::new(),
        active_core: Mutex::new(Some(core)),
        width: 320.0,
        height: 200.0,
        frame_dir: root.join("frames"),
        gatekeeper_socket: root.join("gate.sock"),
        extension_manifest: None,
        assistant: None,
        assistant_settings: service,
        core_options: CoreLaunchOptions::default(),
        route_gate: Arc::new(Mutex::new(())),
        compiler_mcp_relay: None,
        debugger_relay: None,
        done,
    })
}

fn assistant_service() -> Arc<assistant_settings_service::AssistantSettingsService> {
    let dir = std::env::temp_dir().join(format!(
        "trusted-assistant-svc-{}-{}",
        std::process::id(),
        synthetic_request_id()
    ));
    Arc::new(
        assistant_settings_service::AssistantSettingsService::with_environment(
            dir.join("assistant-settings.json"),
            blueice_assistant_settings::AssistantSettings::default(),
            std::sync::Weak::new(),
            dir.join("models"),
            32 * 1024,
        ),
    )
}

fn unique_compiler_mcp_test_path(label: &str) -> PathBuf {
    PathBuf::from("/tmp").join(format!(
        "blueice-launcher-compiler-mcp-{label}-{}-{}.sock",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

mod permissions;

mod cutover;

mod transport;

mod tabs;

mod assistant;

mod lifecycle;

mod actions;
