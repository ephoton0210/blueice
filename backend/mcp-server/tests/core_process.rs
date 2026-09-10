// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Integration test for [`blueice_mcp_server::CoreProcess`] -- spawns
//! the *real* `blueice-core` binary (expected to sit next to this test
//! binary in the workspace's shared `target/` dir, same assumption
//! `blueice-frontend-reference` already makes) and drives it over the
//! real Unix socket, the same "drive the real protocol with a test
//! client" strategy `blueice_engine::session`'s own tests use one
//! layer down. Headless and display-free, unlike
//! `frontend-reference`'s own GUI integration -- there is no reason
//! this can't run in CI.

use blueice_mcp_server::{CoreProcess, OpenTabOutcome};

#[test]
fn spawn_connects_navigates_and_cleans_up_on_drop() {
    let core =
        CoreProcess::spawn(320, 200).expect("blueice-core must spawn and accept a connection");

    let outcome = {
        let mut conn = core.conn.lock().unwrap();
        conn.navigate("about:blank", None)
            .expect("navigate must round-trip over the real socket")
    };
    assert_eq!(outcome.error, None);
    assert_eq!(outcome.snapshot.url.as_deref(), Some("about:blank"));

    drop(core); // Drop's shutdown + child reap + socket cleanup must not panic or hang
}

#[test]
fn open_tab_list_tabs_and_close_tab_round_trip_over_a_real_core() {
    // The end-to-end proof `phase-16-multi-tab-and-tab-groups/PLAN.md`'s
    // MCP layer needs: `blueice-mcp-server`'s own tab tools driving a
    // real, separately-compiled `blueice-core` subprocess, not just the
    // fake-responder unit tests in `lib.rs`.
    let core =
        CoreProcess::spawn(320, 200).expect("blueice-core must spawn and accept a connection");
    let mut conn = core.conn.lock().unwrap();

    let initial = conn.list_tabs().expect("list_tabs must round-trip");
    assert_eq!(initial.len(), 1, "a fresh core starts with exactly one tab");
    let default_tab = initial[0].id;

    let opened = conn.open_tab(None).expect("open_tab must round-trip");
    let OpenTabOutcome::Opened {
        tab_id: new_tab,
        url,
    } = opened
    else {
        panic!("expected Opened, got {opened:?}")
    };
    assert_eq!(url, None);
    assert_ne!(new_tab, default_tab);

    let after_open = conn.list_tabs().unwrap();
    assert_eq!(
        after_open.iter().map(|t| t.id).collect::<Vec<_>>(),
        vec![default_tab, new_tab]
    );

    // The default tab must be completely unaffected by the new one existing.
    let default_outcome = conn
        .navigate("about:blank", Some(default_tab))
        .expect("navigate on the default tab must still work");
    assert_eq!(default_outcome.snapshot.tab_id, default_tab);

    let closed = conn.close_tab(new_tab).expect("close_tab must round-trip");
    assert_eq!(closed, blueice_mcp_server::CloseTabOutcome::Closed);

    let after_close = conn.list_tabs().unwrap();
    assert_eq!(
        after_close.iter().map(|t| t.id).collect::<Vec<_>>(),
        vec![default_tab]
    );
}
