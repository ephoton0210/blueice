// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The memory-pressure half of `phase-8-live-core-hotswap/PLAN.md`'s
//! fleet memory supervisor: a polled signal for "is the system low on
//! memory," and the policy of tearing down [`crate::supervisor::
//! ProcessRegistry`]'s idle-eligible roles when it is. Split from
//! [`crate::supervisor`] deliberately -- the registry is pure decision
//! state with no notion of *why* a teardown decision was made, and this
//! module is the one (of potentially several -- an explicit low-memory
//! signal today, an OS-native pressure notification later) source that
//! can trigger one.

use crate::supervisor::ProcessRegistry;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

/// A source of "how much memory is available right now." Behind a
/// trait so the actual *decision* logic ([`poll_once`]) is a
/// deterministic unit test against a fake signal -- a test that depends
/// on genuinely exhausting host memory would be flaky and dangerous to
/// run in CI, which this project's own coverage/TDD discipline
/// (`CLAUDE.md`) rules out as a primary test strategy.
pub trait MemorySource: Send + Sync {
    /// Fraction of total memory currently available, in `0.0..=1.0`.
    fn available_ratio(&self) -> f64;
}

/// The real, `sysinfo`-backed implementation -- this crate's dependency
/// tree previously had no memory-introspection capability at all, per
/// the same "this is infrastructure, don't hand-roll it" reasoning
/// already used elsewhere in this project for `serde_json`/`ureq`/
/// `fontdue`/`winit`.
pub struct SystemMemorySource {
    system: Mutex<sysinfo::System>,
}

impl SystemMemorySource {
    pub fn new() -> Self {
        SystemMemorySource {
            system: Mutex::new(sysinfo::System::new_all()),
        }
    }
}

impl Default for SystemMemorySource {
    fn default() -> Self {
        Self::new()
    }
}

impl MemorySource for SystemMemorySource {
    fn available_ratio(&self) -> f64 {
        let mut system = self
            .system
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        system.refresh_memory();
        let total = system.total_memory();
        if total == 0 {
            // Can't tell -- assume no pressure rather than tearing
            // everything down on a bogus zero reading.
            return 1.0;
        }
        system.available_memory() as f64 / total as f64
    }
}

/// A memory source reporting a fixed ratio, regardless of real host
/// conditions -- lets a caller simulate memory pressure deterministically
/// rather than waiting on (or risking) real host memory actually running
/// low. Not `#[cfg(test)]`-gated: `blueice-launcher.rs`'s own
/// `--simulate-low-memory` debug flag uses this too, for the same
/// integration-testing reason (`backend/launcher/tests/
/// supervisor_idle_teardown.rs` drives the real binary end to end and
/// needs a way to exercise the pressure-response path without depending
/// on the test machine's actual memory state).
pub struct FixedMemorySource(pub f64);

impl MemorySource for FixedMemorySource {
    fn available_ratio(&self) -> f64 {
        self.0
    }
}

/// Below this fraction of available memory, idle-eligible processes get
/// torn down.
pub const DEFAULT_PRESSURE_THRESHOLD: f64 = 0.10;

/// How often [`spawn_pressure_monitor`]'s background thread polls.
pub const DEFAULT_POLL_INTERVAL: Duration = Duration::from_secs(10);

/// One poll: if `source`'s available ratio is at or above `threshold`,
/// does nothing (the common case -- no pressure). Otherwise tears down
/// every role `registry` currently reports as idle-eligible. Split out
/// from [`spawn_pressure_monitor`]'s loop so this is a plain, fast unit
/// test -- no thread, no real sleep, no real memory query.
pub fn poll_once(
    registry: &mut ProcessRegistry,
    source: &dyn MemorySource,
    threshold: f64,
    now: Instant,
) {
    if source.available_ratio() >= threshold {
        return;
    }
    for role in registry.idle_eligible_for_teardown(now) {
        registry.teardown(&role);
    }
}

/// Spawns a background thread polling `source` every `interval`,
/// applying [`poll_once`] against `registry`. Returns the `JoinHandle`
/// for a caller that wants explicit control; `blueice-launcher`'s own
/// binary just lets it run for the process's whole lifetime, the same
/// way every other background thread this crate spawns already does
/// (`forward_client_to_core`'s per-client threads are never joined
/// either -- the process exiting reclaims them).
pub fn spawn_pressure_monitor(
    registry: Arc<Mutex<ProcessRegistry>>,
    source: Arc<dyn MemorySource>,
    threshold: f64,
    interval: Duration,
) -> thread::JoinHandle<()> {
    thread::spawn(move || loop {
        {
            let mut registry = registry
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            poll_once(&mut registry, source.as_ref(), threshold, Instant::now());
        }
        thread::sleep(interval);
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::supervisor::ProcessPolicy;
    use std::process::Command;

    fn spawn_dummy_child() -> std::process::Child {
        Command::new("sleep").arg("300").spawn().unwrap()
    }

    #[test]
    fn plenty_of_memory_leaves_idle_eligible_roles_alone() {
        let mut registry = ProcessRegistry::new();
        let start = Instant::now();
        registry.register(
            "mcp-server",
            ProcessPolicy::IdleTeardown {
                idle_timeout: Duration::from_secs(1),
            },
            Some(spawn_dummy_child()),
            start,
        );
        let source = FixedMemorySource(0.90); // well above any reasonable threshold

        poll_once(
            &mut registry,
            &source,
            DEFAULT_PRESSURE_THRESHOLD,
            start + Duration::from_secs(1_000_000),
        );

        assert!(
            registry.is_resident("mcp-server"),
            "no pressure means no teardown, even though the role is otherwise idle-eligible"
        );
        registry.teardown("mcp-server");
    }

    #[test]
    fn low_memory_tears_down_idle_eligible_roles() {
        let mut registry = ProcessRegistry::new();
        let start = Instant::now();
        registry.register(
            "mcp-server",
            ProcessPolicy::IdleTeardown {
                idle_timeout: Duration::from_secs(1),
            },
            Some(spawn_dummy_child()),
            start,
        );
        let source = FixedMemorySource(0.02); // well below any reasonable threshold

        poll_once(
            &mut registry,
            &source,
            DEFAULT_PRESSURE_THRESHOLD,
            start + Duration::from_secs(1_000_000),
        );

        assert!(!registry.is_resident("mcp-server"));
    }

    #[test]
    fn low_memory_never_tears_down_an_always_resident_role() {
        let mut registry = ProcessRegistry::new();
        let start = Instant::now();
        registry.register("core", ProcessPolicy::AlwaysResident, None, start);
        let source = FixedMemorySource(0.0); // as much pressure as this signal can express

        poll_once(
            &mut registry,
            &source,
            DEFAULT_PRESSURE_THRESHOLD,
            start + Duration::from_secs(1_000_000),
        );

        assert!(
            registry.is_resident("core"),
            "AlwaysResident must survive even maximal simulated pressure"
        );
    }

    #[test]
    fn low_memory_does_not_tear_down_a_role_thats_not_yet_idle() {
        let mut registry = ProcessRegistry::new();
        let start = Instant::now();
        registry.register(
            "mcp-server",
            ProcessPolicy::IdleTeardown {
                idle_timeout: Duration::from_secs(60),
            },
            Some(spawn_dummy_child()),
            start,
        );
        let source = FixedMemorySource(0.0);

        // Only 1s after registration -- nowhere near the 60s idle_timeout, even under maximal pressure.
        poll_once(
            &mut registry,
            &source,
            DEFAULT_PRESSURE_THRESHOLD,
            start + Duration::from_secs(1),
        );

        assert!(
            registry.is_resident("mcp-server"),
            "pressure alone must not evict a role that isn't actually idle yet"
        );
        registry.teardown("mcp-server");
    }

    #[test]
    fn system_memory_source_reports_a_ratio_between_zero_and_one() {
        // The one test that touches the real `sysinfo`-backed source --
        // proves it doesn't panic and returns a sane value on this
        // machine, without asserting any specific number (which would
        // be inherently flaky across environments).
        let source = SystemMemorySource::new();
        let ratio = source.available_ratio();
        assert!(
            (0.0..=1.0).contains(&ratio),
            "expected a ratio in 0.0..=1.0, got {ratio}"
        );
    }
}
