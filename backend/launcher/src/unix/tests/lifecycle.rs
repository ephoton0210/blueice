// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn a_direct_edit_over_the_pipe_takes_effect_and_the_next_inspect_shows_it() {
    let broker = broker_with_settings(Some(assistant_service()));
    let edited = blueice_assistant_settings::AssistantSettings {
        nice: 15,
        ..Default::default()
    };
    let reply = handle_trusted_window_session_request(
        trusted_window::TrustedWindowRequest::EditAssistantSettings {
            settings: edited.clone(),
        },
        &broker,
        &mut None,
    );
    assert!(
        matches!(&reply, trusted_window::TrustedWindowReply::AssistantSettingsState { current, .. } if **current == edited)
    );
    let again = handle_trusted_window_session_request(
        trusted_window::TrustedWindowRequest::InspectAssistantSettings,
        &broker,
        &mut None,
    );
    assert!(
        matches!(&again, trusted_window::TrustedWindowReply::AssistantSettingsState { current, .. } if **current == edited)
    );
}

#[test]
fn the_new_trusted_window_messages_round_trip_over_the_pipe_format() {
    let settings = blueice_assistant_settings::AssistantSettings::default();
    for request in [
        trusted_window::TrustedWindowRequest::InspectAssistantSettings,
        trusted_window::TrustedWindowRequest::ApproveAssistantProposal {
            id: 3,
            digest: "d".into(),
        },
        trusted_window::TrustedWindowRequest::DenyAssistantProposal { id: 3 },
        trusted_window::TrustedWindowRequest::EditAssistantSettings {
            settings: settings.clone(),
        },
    ] {
        let json = serde_json::to_string(&request).unwrap();
        assert_eq!(
            serde_json::from_str::<trusted_window::TrustedWindowRequest>(&json).unwrap(),
            request
        );
    }
    let reply = trusted_window::TrustedWindowReply::AssistantSettingsState {
        current: Box::new(settings.clone()),
        pending: Some(Box::new(trusted_window::PendingAssistantProposal {
            id: 1,
            digest: "d".into(),
            diff: vec!["a".into()],
            proposed: settings,
            seconds_left: 9,
        })),
    };
    let json = serde_json::to_string(&reply).unwrap();
    assert_eq!(
        serde_json::from_str::<trusted_window::TrustedWindowReply>(&json).unwrap(),
        reply
    );
}

#[test]
fn supervise_out_of_process_bluejs_builders_set_their_own_fields() {
    let limits = BlueJsHostRuntimeLimits::default();
    let options =
        CoreLaunchOptions::default().supervise_out_of_process_bluejs_with_runtime_limits(limits);
    assert!(options.supervise_out_of_process_bluejs);
    assert_eq!(options.bluejs_host_runtime_limits, limits);

    let options =
        CoreLaunchOptions::default().supervise_out_of_process_bluejs_with_core_http_fixture();
    assert!(options.supervise_out_of_process_bluejs);
    assert!(options.core_http_page_script_fixture);
}

#[test]
fn spawn_rejects_a_fixed_and_owner_selected_http_policy_before_spawning_anything() {
    use blueice_ipc::owner_bootstrap::{OwnerHttpOriginRule, OwnerHttpResource};
    let policy = OwnerHttpPolicyBootstrap {
        origin_rule: OwnerHttpOriginRule::SameDocumentOrigin,
        resources: vec![OwnerHttpResource {
            canonical_url: "https://example.test/app.js".to_string(),
            integrity: format!("sha256:{}", "a".repeat(64)),
        }],
    };
    let options = CoreLaunchOptions::default()
        .supervise_out_of_process_bluejs_with_owner_http_policy(policy)
        .unwrap()
        .supervise_out_of_process_bluejs_with_core_http_fixture();
    let Err(error) = SpawnedCore::spawn_with_options(320.0, 200.0, &std::env::temp_dir(), options)
    else {
        panic!("a mutually exclusive HTTP page policy combination must be rejected");
    };
    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    assert_eq!(
        error.to_string(),
        "fixed and owner-selected HTTP page policies are mutually exclusive"
    );
}

#[test]
fn client_write_half_gets_a_bounded_write_timeout() {
    // Regression coverage for a real deadlock this fixes: without a
    // write timeout, a client whose writer thread stalls (its
    // kernel socket buffer fills and nobody drains it) would never
    // notice the client is gone -- its channel would just queue up
    // forever instead of eventually being pruned. See
    // `a_slow_client_does_not_block_delivery_to_another_client` for
    // the actual "doesn't block other clients" property this and
    // the channel/writer-thread split together provide.
    let (client_side, _client_observed) = UnixStream::pair().unwrap();
    let write_half = client_write_half(&client_side).unwrap();
    assert_eq!(
        write_half.write_timeout().unwrap(),
        Some(CLIENT_WRITE_TIMEOUT)
    );
}

#[test]
fn a_slow_client_does_not_block_delivery_to_another_client() {
    // The actual bug this channel/writer-thread design fixes: a
    // single shared thread writing to every registered client's
    // socket directly, one after another, meant one client that
    // stalls (its kernel socket buffer fills, e.g. a large message
    // nobody drains) blocked delivery to every *other* client too,
    // for as long as that stalled write took to time out --
    // confirmed against a real launcher/core pair before this fix
    // existed. Proven here with a real filled socket buffer, not a
    // mock: `slow_client`'s peer end is never read; `live_client`'s
    // is read immediately after this call returns, and must have
    // its message waiting regardless of the slow client's state.
    let (core_side, mut core_observed) = UnixStream::pair().unwrap();
    let (slow_client_side, _never_read) = UnixStream::pair().unwrap();
    let (live_client_side, mut live_client_observed) = UnixStream::pair().unwrap();
    let (core_writer_side, _unused) = UnixStream::pair().unwrap();
    let core_writer = Arc::new(Mutex::new(core_writer_side));
    let clients: Arc<Mutex<Vec<Sender<TaggedServerMessage>>>> = Arc::new(Mutex::new(Vec::new()));

    register_client(
        slow_client_side,
        Arc::clone(&core_writer),
        Arc::clone(&clients),
    )
    .unwrap();
    register_client(
        live_client_side,
        Arc::clone(&core_writer),
        Arc::clone(&clients),
    )
    .unwrap();

    // Comfortably larger than any realistic default kernel socket
    // buffer, so the write to `slow_client`'s writer thread genuinely
    // blocks rather than merely being slow to observe. Written on
    // its own thread since *this* write can itself block until
    // `broadcast_core_to_clients` below is actively reading
    // `core_side` -- `core_observed`'s own send buffer is no bigger
    // than any other socket's here.
    let big_message = ServerMessage::Dom("x".repeat(4 * 1024 * 1024));
    let sent = big_message.clone();
    thread::spawn(move || {
        write_server_message(&mut core_observed, &sent).unwrap();
        drop(core_observed); // ends the broadcaster loop after the one message
    });

    let start = Instant::now();
    broadcast_core_to_clients(core_side, clients);
    assert!(
        start.elapsed() < Duration::from_secs(1),
        "fanning out must be a cheap non-blocking queue push regardless of any client's own writer-thread state"
    );

    let received = read_server_message(&mut live_client_observed).unwrap();
    assert_eq!(
        received, big_message,
        "the live client must receive its own copy promptly, not stalled behind the slow one"
    );
}

#[test]
fn sibling_core_binary_sits_next_to_the_launcher_binary() {
    let exe = PathBuf::from("/some/target/debug/blueice-launcher");
    assert_eq!(
        sibling_core_binary(&exe),
        PathBuf::from("/some/target/debug/blueice-core")
    );
}

#[test]
fn sibling_core_binary_steps_out_of_a_deps_directory_for_integration_tests() {
    let exe = PathBuf::from("/some/target/debug/deps/broker_end_to_end-abc123");
    assert_eq!(
        sibling_core_binary(&exe),
        PathBuf::from("/some/target/debug/blueice-core")
    );
}

#[test]
fn compiler_mcp_endpoint_validation_happens_before_core_child_spawn() {
    let path = unique_compiler_mcp_test_path("preflight");
    std::fs::write(&path, b"must survive preflight").unwrap();
    let frame_dir = std::env::temp_dir().join(format!(
        "blueice-launcher-compiler-mcp-preflight-{}",
        std::process::id()
    ));
    let result = SpawnedCore::spawn_with_options(
        1.0,
        1.0,
        &frame_dir,
        CoreLaunchOptions::default().with_core_closed_compiler_mcp_endpoint(path.clone()),
    );
    let error = match result {
        Ok(_) => panic!("a non-socket compiler endpoint must reject before spawning core"),
        Err(error) => error,
    };
    assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
    assert_eq!(std::fs::read(&path).unwrap(), b"must survive preflight");
    assert!(
        !frame_dir.exists(),
        "a rejected compiler endpoint must not start a core that creates a frame directory"
    );

    let _ = std::fs::remove_file(path);
}
