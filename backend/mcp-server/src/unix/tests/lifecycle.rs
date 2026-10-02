// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn go_back_uses_the_same_navigation_completion_barrier_and_returns_restored_state() {
    let (client, server) = UnixStream::pair().unwrap();
    fake_core(
        server,
        vec![
            Box::new(|msg, s| {
                assert!(matches!(msg, ClientMessage::GoBack));
                reply(
                    s,
                    &ServerMessage::Navigated {
                        url: "https://example.com/previous".to_string(),
                    },
                );
                reply_tab(
                    s,
                    7,
                    &ServerMessage::FrameReady {
                        shm_path: "/tmp/previous".to_string(),
                        width: 10,
                        height: 10,
                        generation: 4,
                    },
                );
            }),
            Box::new(|msg, s| {
                assert!(matches!(msg, ClientMessage::GetRepresentation));
                let mut snapshot = sample_snapshot(4);
                snapshot.tab_id = 7;
                snapshot.url = Some("https://example.com/previous".to_string());
                reply(s, &ServerMessage::Representation(snapshot));
            }),
        ],
    );

    let mut conn = CoreConnection::new(client);
    let outcome = conn.go_back(Some(7)).unwrap();
    assert_eq!(outcome.error, None);
    assert_eq!(outcome.snapshot.tab_id, 7);
    assert_eq!(
        outcome.snapshot.url.as_deref(),
        Some("https://example.com/previous")
    );
    assert_eq!(conn.last_frame(Some(7)).unwrap().generation, 4);
}

#[test]
fn attach_to_never_falls_back_to_a_private_core() {
    let rendezvous_path = std::env::temp_dir().join(format!(
        "blueice-mcp-test-required-rendezvous-missing-{}.sock",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&rendezvous_path);

    assert!(
        CoreProcess::attach_to(&rendezvous_path, 320, 200).is_err(),
        "a required launcher attachment must fail rather than spawn an unobserved core"
    );
}

#[test]
fn sibling_core_binary_sits_next_to_the_mcp_server_binary() {
    let exe = PathBuf::from("/some/target/debug/blueice-mcp-server");
    assert_eq!(
        sibling_core_binary(&exe),
        PathBuf::from("/some/target/debug/blueice-core")
    );
}

#[test]
fn sibling_core_binary_steps_out_of_a_deps_directory_for_integration_tests() {
    let exe = PathBuf::from("/some/target/debug/deps/core_process-abc123");
    assert_eq!(
        sibling_core_binary(&exe),
        PathBuf::from("/some/target/debug/blueice-core")
    );
}

#[test]
fn send_and_drain_reports_gatekeeper_blocked_as_an_error() {
    let (client, server) = UnixStream::pair().unwrap();
    fake_core(
        server,
        vec![
            Box::new(|msg, s| {
                assert!(matches!(msg, ClientMessage::Navigate { .. }));
                reply(
                    s,
                    &ServerMessage::GatekeeperBlocked {
                        reason: "denied".to_string(),
                        category: "policy".to_string(),
                        url: "https://bad.example".to_string(),
                    },
                );
            }),
            Box::new(|msg, s| {
                assert!(matches!(msg, ClientMessage::GetRepresentation));
                reply(s, &ServerMessage::Representation(sample_snapshot(0)));
            }),
        ],
    );

    let mut conn = CoreConnection::new(client);
    let outcome = conn.navigate("https://bad.example", None).unwrap();
    let error = outcome
        .error
        .expect("gatekeeper block must surface as an error");
    assert!(error.contains("denied"));
    assert!(error.contains("policy"));
    assert!(error.contains("https://bad.example"));
}
