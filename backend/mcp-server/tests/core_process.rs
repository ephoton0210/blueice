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

use blueice_mcp_server::CoreProcess;

#[test]
fn spawn_connects_navigates_and_cleans_up_on_drop() {
    let core = CoreProcess::spawn(320, 200).expect("blueice-core must spawn and accept a connection");

    let outcome = {
        let mut conn = core.conn.lock().unwrap();
        conn.navigate("about:blank").expect("navigate must round-trip over the real socket")
    };
    assert_eq!(outcome.error, None);
    assert_eq!(outcome.snapshot.url.as_deref(), Some("about:blank"));

    drop(core); // Drop's shutdown + child reap + socket cleanup must not panic or hang
}
