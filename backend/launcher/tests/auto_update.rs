// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! End-to-end proof of `phase-8-live-core-hotswap/PLAN.md`'s automatic update
//! detection through the real compiled launcher (`--auto-update-secs`): a newer
//! `blueice-core` appearing beside the launcher replaces the running one
//! without dropping the connected client, a binary still being written is not
//! swapped in, and a broken update is tried and then left alone. The "new
//! version" is a replacement wrapper script (a different length), swapped in
//! atomically the way a package manager would.

mod common;

use common::*;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

const HEALTHY: &str = "exec \"$REAL\" \"$@\"";

/// Atomically replaces the core wrapper with one running `body`, padded with a
/// unique comment so its length differs from every earlier version.
fn install_core(dir: &Path, body: &str, version: u32) {
    let current = std::fs::read_to_string(dir.join("blueice-core")).unwrap();
    // Keep the counting preamble; swap the behavior after it.
    let preamble = &current[..current.find("\nif [").or(current.find("\nexec")).unwrap()];
    let script = format!(
        "{preamble}\n{body}\n# version {version} {}\n",
        "x".repeat(version as usize * 7)
    );
    let staged = dir.join("blueice-core.staged");
    std::fs::write(&staged, script).unwrap();
    std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::rename(&staged, dir.join("blueice-core")).unwrap();
}

fn wait_for_invocations(rig: &Rig, wanted: u32, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if rig.invocations() >= wanted {
            return true;
        }
        thread::sleep(Duration::from_millis(100));
    }
    false
}

fn rig(label: &str) -> Rig {
    Rig::start_with(label, HEALTHY, &["--auto-update-secs", "1"])
}

#[test]
fn an_unchanged_core_is_never_replaced() {
    let rig = rig("steady");
    thread::sleep(Duration::from_millis(3500));
    assert_eq!(
        rig.invocations(),
        1,
        "nothing changed, so nothing may be restarted"
    );
}

#[test]
fn a_newer_core_replaces_the_running_one_and_the_connected_client_never_notices() {
    let rig = rig("newer");
    let mut client = rig.client();
    open_credits(&mut client);

    install_core(&rig.dir, HEALTHY, 1);
    assert!(
        wait_for_invocations(&rig, 2, Duration::from_secs(30)),
        "the launcher never started the updated core"
    );
    // The very same connection, opened before the update, still works and still
    // has the tab.
    assert_eq!(url_now(&mut client).as_deref(), Some("about:credits"));
    // And it settles: one update is one cutover, not a loop.
    thread::sleep(Duration::from_millis(3500));
    assert_eq!(rig.invocations(), 2);
}

#[test]
fn a_binary_still_being_written_is_not_swapped_in_half_written() {
    let rig = rig("halfwritten");
    let mut client = rig.client();
    open_credits(&mut client);

    // Three versions land within about a second, faster than confirmation.
    for version in 1..=3 {
        install_core(&rig.dir, HEALTHY, version);
        thread::sleep(Duration::from_millis(350));
    }
    assert!(wait_for_invocations(&rig, 2, Duration::from_secs(30)));
    thread::sleep(Duration::from_millis(4000));
    assert_eq!(
        rig.invocations(),
        2,
        "the settled binary is swapped in once, not once per intermediate write"
    );
    assert_eq!(url_now(&mut client).as_deref(), Some("about:credits"));
}

#[test]
fn a_broken_update_is_tried_then_left_alone_until_the_binary_changes_again() {
    let rig = rig("broken");
    let mut client = rig.client();
    open_credits(&mut client);

    // A build that cannot start: v1 keeps serving while three attempts fail.
    install_core(
        &rig.dir,
        "if [ \"$n\" -ge 2 ]; then exit 1; fi\nexec \"$REAL\" \"$@\"",
        1,
    );
    assert!(
        wait_for_invocations(&rig, 4, Duration::from_secs(40)),
        "the broken build was never tried"
    );
    let after_retries = rig.invocations();
    assert_eq!(after_retries, 4, "v1 plus exactly three attempts");
    assert_eq!(
        url_now(&mut client).as_deref(),
        Some("about:credits"),
        "v1 kept serving"
    );

    // Left alone: the same broken binary is not tried again on later polls.
    thread::sleep(Duration::from_millis(4000));
    assert_eq!(
        rig.invocations(),
        after_retries,
        "no hammering a broken build"
    );

    // A fixed build is a fresh chance and succeeds.
    install_core(&rig.dir, HEALTHY, 2);
    assert!(
        wait_for_invocations(&rig, after_retries + 1, Duration::from_secs(30)),
        "the fixed build was never picked up"
    );
    assert_eq!(url_now(&mut client).as_deref(), Some("about:credits"));
}
