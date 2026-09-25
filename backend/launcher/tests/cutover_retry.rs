// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! End-to-end proof of `phase-8-live-core-hotswap/PLAN.md`'s bounded cutover
//! retry through the real compiled launcher. The launcher finds `core` beside
//! its own executable, so each test builds a private directory holding a
//! launcher and a `blueice-core` that is a shell wrapper around the real one,
//! and the wrapper misbehaves on chosen invocations (the first is v1, the
//! next ones are cutover attempts).

mod common;

use blueice_launcher::control::ControlReply;
use common::*;
use std::time::{Duration, Instant};

#[test]
fn a_cutover_whose_first_v2_fails_to_start_succeeds_on_a_retry() {
    // Invocation 1 is v1; invocation 2 (the first cutover attempt's v2) dies at
    // once; invocation 3 (the retry) is healthy.
    let rig = Rig::start(
        "transient",
        "if [ \"$n\" = \"2\" ]; then exit 1; fi\nexec \"$REAL\" \"$@\"",
    );
    let mut client = rig.client();
    open_credits(&mut client);

    let started = Instant::now();
    match rig.cutover() {
        ControlReply::CutoverDone { tabs_migrated } => assert_eq!(tabs_migrated, 1),
        other => panic!("expected the retry to succeed, got {other:?}"),
    }
    assert_eq!(
        rig.invocations(),
        3,
        "v1, the failed attempt, and the retry"
    );
    // The failed start was noticed at once, not after the 5 s startup wait.
    assert!(
        started.elapsed() < Duration::from_secs(15),
        "{:?}",
        started.elapsed()
    );

    // The same, still-open client connection now talks to v2 and sees the tab.
    assert_eq!(url_now(&mut client).as_deref(), Some("about:credits"));
}

#[test]
fn a_cutover_that_never_succeeds_gives_up_after_three_attempts_and_v1_keeps_serving() {
    let rig = Rig::start(
        "permanent",
        "if [ \"$n\" -ge 2 ]; then exit 1; fi\nexec \"$REAL\" \"$@\"",
    );
    let mut client = rig.client();
    open_credits(&mut client);

    match rig.cutover() {
        ControlReply::CutoverFailed { reason } => {
            assert!(reason.contains("gave up after 3 attempts"), "{reason}");
            assert!(reason.contains("failed to spawn v2"), "{reason}");
        }
        other => panic!("expected CutoverFailed, got {other:?}"),
    }
    assert_eq!(rig.invocations(), 4, "v1 and exactly three attempts");

    // v1 was never touched: the client still works, and a later cutover is
    // accepted (not stuck busy).
    assert_eq!(url_now(&mut client).as_deref(), Some("about:credits"));
    assert!(matches!(rig.cutover(), ControlReply::CutoverFailed { .. }));
}
