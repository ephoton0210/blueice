// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Automatic update detection (`phase-8-live-core-hotswap/PLAN.md`, "Fuller
//! hot-swap"). Opt-in: the launcher watches the `blueice-core` binary it would
//! spawn and, when a newer one appears, runs the same single-flight cutover the
//! control socket triggers -- with the bounded retry -- so an updated engine
//! replaces the running one without dropping any client.
//!
//! The decision logic is [`UpdateWatch`], a small state machine with no I/O and
//! no clock, so every rule is a plain unit test:
//!
//! * A binary counts as *changed* when its identity (modification time and
//!   length) differs from the one the running core was started from.
//! * A change must be seen on **two consecutive polls** before acting, so a
//!   binary that is still being copied over is never swapped in half-written.
//! * Scheduling is "as soon as it is stable": a cutover never drops a client,
//!   so there is no quiet-window policy to design.
//! * After a cutover succeeds, the new identity is the baseline. After it fails
//!   (retries exhausted), the launcher gives up on *that* binary and tries again
//!   only once the file changes again, rather than hammering a broken build.
//! * If another cutover is already in flight, the next poll simply tries again.

use crate::Broker;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, SystemTime};

/// What identifies one build of a binary for change detection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BinaryIdentity {
    modified: SystemTime,
    len: u64,
}

/// The identity of the file at `path`, or `None` if it cannot be read (it may
/// be momentarily absent while a package manager replaces it).
pub fn identity(path: &Path) -> Option<BinaryIdentity> {
    let metadata = std::fs::metadata(path).ok()?;
    Some(BinaryIdentity {
        modified: metadata.modified().ok()?,
        len: metadata.len(),
    })
}

/// What the watcher should do after one poll.
#[derive(Debug, PartialEq, Eq)]
pub enum Decision {
    Nothing,
    /// Cut over to the binary with this identity.
    Cutover(BinaryIdentity),
}

/// The watcher's memory between polls.
pub struct UpdateWatch {
    baseline: BinaryIdentity,
    /// A differing identity seen on the previous poll, awaiting confirmation.
    pending: Option<BinaryIdentity>,
    /// The identity of a binary a cutover already failed for.
    given_up: Option<BinaryIdentity>,
}

impl UpdateWatch {
    pub fn new(baseline: BinaryIdentity) -> Self {
        UpdateWatch {
            baseline,
            pending: None,
            given_up: None,
        }
    }

    /// Feeds one poll's observation and says whether to cut over now.
    pub fn observe(&mut self, current: Option<BinaryIdentity>) -> Decision {
        let Some(current) = current else {
            // Unreadable right now (mid-replace): forget any half-confirmed
            // change rather than trust it.
            self.pending = None;
            return Decision::Nothing;
        };
        if current == self.baseline {
            // Back to (or still on) the running build: nothing to update to.
            self.pending = None;
            self.given_up = None;
            return Decision::Nothing;
        }
        if self.given_up.as_ref() == Some(&current) {
            return Decision::Nothing;
        }
        if self.pending.as_ref() == Some(&current) {
            return Decision::Cutover(current);
        }
        self.pending = Some(current);
        Decision::Nothing
    }

    /// The cutover to `identity` completed: it is the running build now.
    pub fn cutover_succeeded(&mut self, identity: BinaryIdentity) {
        self.baseline = identity;
        self.pending = None;
        self.given_up = None;
    }

    /// The cutover to `identity` failed for good: leave it alone until it changes.
    pub fn cutover_failed(&mut self, identity: BinaryIdentity) {
        self.given_up = Some(identity);
        self.pending = None;
    }
}

/// Starts the watcher thread. It ends once `stop` is set.
pub(crate) fn spawn_update_watcher(
    broker: Arc<Broker>,
    binary: PathBuf,
    interval: Duration,
    stop: Arc<AtomicBool>,
) {
    let Some(baseline) = identity(&binary) else {
        eprintln!(
            "blueice-launcher: cannot watch {} for updates: it cannot be read",
            binary.display()
        );
        return;
    };
    thread::spawn(move || {
        let mut watch = UpdateWatch::new(baseline);
        while !stop.load(Ordering::Relaxed) {
            thread::sleep(interval);
            if stop.load(Ordering::Relaxed) {
                break;
            }
            let Decision::Cutover(new) = watch.observe(identity(&binary)) else {
                continue;
            };
            match crate::cutover(&broker) {
                crate::control::ControlReply::CutoverDone { tabs_migrated } => {
                    eprintln!(
                        "blueice-launcher: updated blueice-core ({tabs_migrated} tab(s) carried over)"
                    );
                    watch.cutover_succeeded(new);
                }
                // Another cutover is running; the next poll asks again.
                crate::control::ControlReply::CutoverBusy => {}
                crate::control::ControlReply::CutoverFailed { reason } => {
                    eprintln!(
                        "blueice-launcher: not updating blueice-core: {reason}; will retry only when the binary changes again"
                    );
                    watch.cutover_failed(new);
                }
                _ => {}
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::UNIX_EPOCH;

    fn id(seconds: u64, len: u64) -> BinaryIdentity {
        BinaryIdentity {
            modified: UNIX_EPOCH + Duration::from_secs(seconds),
            len,
        }
    }

    fn watch() -> UpdateWatch {
        UpdateWatch::new(id(100, 1000))
    }

    #[test]
    fn an_unchanged_binary_never_triggers_anything() {
        let mut w = watch();
        for _ in 0..5 {
            assert_eq!(w.observe(Some(id(100, 1000))), Decision::Nothing);
        }
    }

    #[test]
    fn a_change_must_be_seen_twice_in_a_row_before_acting() {
        let mut w = watch();
        assert_eq!(
            w.observe(Some(id(200, 1500))),
            Decision::Nothing,
            "first sighting only"
        );
        assert_eq!(
            w.observe(Some(id(200, 1500))),
            Decision::Cutover(id(200, 1500)),
            "stable across two polls"
        );
    }

    #[test]
    fn a_binary_still_changing_is_never_swapped_in_half_written() {
        let mut w = watch();
        // Growing while it is being copied: never the same twice in a row.
        for len in [100, 200, 300, 400] {
            assert_eq!(w.observe(Some(id(200, len))), Decision::Nothing);
        }
        // Then it settles.
        assert_eq!(
            w.observe(Some(id(200, 400))),
            Decision::Cutover(id(200, 400))
        );
    }

    #[test]
    fn a_length_change_alone_or_a_time_change_alone_counts() {
        for changed in [id(100, 2000), id(300, 1000)] {
            let mut w = watch();
            assert_eq!(w.observe(Some(changed.clone())), Decision::Nothing);
            assert_eq!(w.observe(Some(changed.clone())), Decision::Cutover(changed));
        }
    }

    #[test]
    fn an_unreadable_binary_forgets_a_half_confirmed_change() {
        let mut w = watch();
        assert_eq!(w.observe(Some(id(200, 1500))), Decision::Nothing);
        assert_eq!(w.observe(None), Decision::Nothing, "mid-replace");
        // The earlier sighting no longer counts: a fresh one is needed.
        assert_eq!(w.observe(Some(id(200, 1500))), Decision::Nothing);
        assert_eq!(
            w.observe(Some(id(200, 1500))),
            Decision::Cutover(id(200, 1500))
        );
    }

    #[test]
    fn a_change_that_reverts_is_dropped() {
        let mut w = watch();
        assert_eq!(w.observe(Some(id(200, 1500))), Decision::Nothing);
        assert_eq!(
            w.observe(Some(id(100, 1000))),
            Decision::Nothing,
            "back to the running build"
        );
        assert_eq!(
            w.observe(Some(id(200, 1500))),
            Decision::Nothing,
            "must be re-confirmed"
        );
    }

    #[test]
    fn after_a_success_the_new_build_is_the_baseline() {
        let mut w = watch();
        w.observe(Some(id(200, 1500)));
        assert!(matches!(
            w.observe(Some(id(200, 1500))),
            Decision::Cutover(_)
        ));
        w.cutover_succeeded(id(200, 1500));
        for _ in 0..3 {
            assert_eq!(w.observe(Some(id(200, 1500))), Decision::Nothing);
        }
        // A later build is detected against the new baseline.
        assert_eq!(w.observe(Some(id(300, 1600))), Decision::Nothing);
        assert_eq!(
            w.observe(Some(id(300, 1600))),
            Decision::Cutover(id(300, 1600))
        );
    }

    #[test]
    fn after_a_failure_that_build_is_left_alone_until_it_changes_again() {
        let mut w = watch();
        w.observe(Some(id(200, 1500)));
        assert!(matches!(
            w.observe(Some(id(200, 1500))),
            Decision::Cutover(_)
        ));
        w.cutover_failed(id(200, 1500));
        for _ in 0..5 {
            assert_eq!(
                w.observe(Some(id(200, 1500))),
                Decision::Nothing,
                "no hammering a broken build"
            );
        }
        // A new build is a fresh chance.
        assert_eq!(w.observe(Some(id(400, 1700))), Decision::Nothing);
        assert_eq!(
            w.observe(Some(id(400, 1700))),
            Decision::Cutover(id(400, 1700))
        );
    }

    #[test]
    fn a_busy_cutover_is_simply_asked_again_on_the_next_poll() {
        let mut w = watch();
        w.observe(Some(id(200, 1500)));
        assert!(matches!(
            w.observe(Some(id(200, 1500))),
            Decision::Cutover(_)
        ));
        // The caller saw `CutoverBusy` and told the watch nothing.
        assert_eq!(
            w.observe(Some(id(200, 1500))),
            Decision::Cutover(id(200, 1500))
        );
    }

    #[test]
    fn the_identity_of_a_real_file_changes_with_its_length_and_a_missing_file_has_none() {
        let path = std::env::temp_dir().join(format!("identity-{}", std::process::id()));
        assert_eq!(identity(&path), None);
        std::fs::write(&path, b"one").unwrap();
        let first = identity(&path).unwrap();
        std::fs::write(&path, b"longer").unwrap();
        let second = identity(&path).unwrap();
        assert_ne!(first, second);
        let _ = std::fs::remove_file(&path);
    }
}
