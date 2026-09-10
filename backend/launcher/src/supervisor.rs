// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A generic, policy-driven registry of processes `blueice-launcher`
//! supervises the lifecycle of -- the minimal first slice of
//! `phase-8-live-core-hotswap/PLAN.md`'s fleet-memory-supervisor design,
//! itself following `research/multi-process-memory.md`'s recommendation
//! that the launcher (already `core`'s own supervisor) own idle-teardown
//! authority for the rest of the process fleet too, rather than
//! inventing a separate coordinator role.
//!
//! Deliberately one registry keyed by role name and one small policy
//! enum, not a special case per process: `core` is `AlwaysResident`
//! today, `mcp-server` is registered as a typed `IdleTeardown` slot
//! (real in the data model, not yet wired to an automatic spawn path --
//! see this crate's own module docs), and `extension`/`ai-assistant`/
//! `downloads` need no code changes here at all once they exist as real
//! processes -- each is one `register` call away.

use std::collections::HashMap;
use std::process::Child;
use std::time::{Duration, Instant};

/// How a supervised process's lifecycle should be managed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessPolicy {
    /// Never a teardown candidate, regardless of idle time or memory
    /// pressure -- `core` today (the one process that "must not
    /// crash"); `ai-gatekeeper` once Phase 7 exists (a fail-closed
    /// safety checkpoint has no meaningful "idle" state).
    AlwaysResident,
    /// Not spawned until first needed; once resident, eligible for
    /// teardown after `idle_timeout` with no activity.
    OnDemand { idle_timeout: Duration },
    /// Already resident (e.g. spawned unconditionally at launcher
    /// startup) but still eligible for idle-teardown -- differs from
    /// `OnDemand` only in *when* it first becomes resident, not in the
    /// teardown decision itself.
    IdleTeardown { idle_timeout: Duration },
}

impl ProcessPolicy {
    /// `None` for `AlwaysResident` (never eligible); the configured
    /// timeout otherwise.
    fn idle_timeout(self) -> Option<Duration> {
        match self {
            ProcessPolicy::AlwaysResident => None,
            ProcessPolicy::OnDemand { idle_timeout }
            | ProcessPolicy::IdleTeardown { idle_timeout } => Some(idle_timeout),
        }
    }
}

struct SupervisedProcess {
    policy: ProcessPolicy,
    /// `Some` while a real child process is running for this role;
    /// `None` for a not-yet-spawned `OnDemand` role, or right after
    /// [`ProcessRegistry::teardown`].
    resident: Option<Child>,
    last_active: Instant,
}

/// The fleet-wide registry: every role `blueice-launcher` supervises,
/// keyed by name (`"core"`, `"mcp-server"`, ...) rather than a fixed
/// set of fields, so a new role is one [`ProcessRegistry::register`]
/// call, not a new struct field and a new match arm everywhere.
#[derive(Default)]
pub struct ProcessRegistry {
    entries: HashMap<String, SupervisedProcess>,
}

impl ProcessRegistry {
    pub fn new() -> Self {
        ProcessRegistry {
            entries: HashMap::new(),
        }
    }

    /// Registers `role` under `policy`. `resident` is `Some` if a
    /// process is already running for this role at registration time
    /// (e.g. `core`, spawned unconditionally before this call), `None`
    /// for an inert slot not yet spawned. `now` seeds the initial
    /// activity timestamp.
    pub fn register(
        &mut self,
        role: impl Into<String>,
        policy: ProcessPolicy,
        resident: Option<Child>,
        now: Instant,
    ) {
        self.entries.insert(
            role.into(),
            SupervisedProcess {
                policy,
                resident,
                last_active: now,
            },
        );
    }

    /// Resets `role`'s idle clock -- call this on any real activity
    /// (a client message routed to it, a new connection, etc.). A
    /// no-op if `role` isn't registered.
    pub fn mark_active(&mut self, role: &str, now: Instant) {
        if let Some(entry) = self.entries.get_mut(role) {
            entry.last_active = now;
        }
    }

    /// Whether `role` currently has a live process. `AlwaysResident`
    /// roles report `true` unconditionally by definition (they're
    /// spawned once and kept forever, often by a *different* owner --
    /// `core`'s own child is owned and torn down by `SpawnedCore`, not
    /// this registry, since an `AlwaysResident` role is never a
    /// [`Self::teardown`] candidate and so never needs this registry to
    /// hold the actual [`Child`] at all); every other policy reports
    /// whether this registry itself currently holds a resident process.
    pub fn is_resident(&self, role: &str) -> bool {
        self.entries.get(role).is_some_and(|entry| {
            matches!(entry.policy, ProcessPolicy::AlwaysResident) || entry.resident.is_some()
        })
    }

    /// Transitions `role` to resident with `child` as the process this
    /// registry now owns (and will [`Self::teardown`] later if it
    /// becomes idle-eligible) -- the on-demand-spawn counterpart to
    /// `teardown`. `now` also resets the idle clock, since a freshly
    /// spawned process is by definition not idle yet.
    ///
    /// Returns `child` back if `role` isn't registered, rather than
    /// silently dropping it: `Child` doesn't kill its process on drop,
    /// so discarding it here would leak a real, still-running process
    /// with no handle left to ever clean it up.
    pub fn set_resident(&mut self, role: &str, child: Child, now: Instant) -> Option<Child> {
        match self.entries.get_mut(role) {
            Some(entry) => {
                entry.resident = Some(child);
                entry.last_active = now;
                None
            }
            None => Some(child),
        }
    }

    /// Role names that are currently resident, have a policy with an
    /// idle timeout (i.e. not `AlwaysResident`), and have been idle at
    /// least that long as of `now`. `now` is an injected parameter
    /// (never read internally via `Instant::now()`) so idle-timeout
    /// behavior is a deterministic unit test, not one that needs a
    /// real sleep.
    pub fn idle_eligible_for_teardown(&self, now: Instant) -> Vec<String> {
        self.entries
            .iter()
            .filter(|(_, entry)| entry.resident.is_some())
            .filter_map(|(role, entry)| {
                let idle_timeout = entry.policy.idle_timeout()?;
                (now.saturating_duration_since(entry.last_active) >= idle_timeout)
                    .then(|| role.clone())
            })
            .collect()
    }

    /// Kills and reaps `role`'s resident process, if any, then marks it
    /// no-longer-resident -- the registry entry (and its policy) stays,
    /// so a future respawn can reuse it. A no-op if `role` isn't
    /// registered or has no resident process.
    pub fn teardown(&mut self, role: &str) {
        if let Some(entry) = self.entries.get_mut(role) {
            if let Some(mut child) = entry.resident.take() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    /// A real, cheap, long-lived-enough child process to stand in for
    /// "a resident process" without needing any of `blueice-launcher`'s
    /// own binaries -- this project's established "drive the real
    /// thing, not a mock" testing philosophy, applied at the smallest
    /// scale that still exercises a genuine OS process.
    fn spawn_dummy_child() -> Child {
        Command::new("sleep").arg("300").spawn().unwrap()
    }

    #[test]
    fn always_resident_is_never_eligible_regardless_of_idle_time() {
        // Registered with `resident: None` -- matches real usage:
        // `core`'s actual child is owned and torn down by `SpawnedCore`,
        // never by this registry (see `is_resident`'s own docs for why).
        let mut registry = ProcessRegistry::new();
        let start = Instant::now();
        registry.register("core", ProcessPolicy::AlwaysResident, None, start);

        let far_future = start + Duration::from_secs(1_000_000);
        assert!(registry.idle_eligible_for_teardown(far_future).is_empty());
    }

    #[test]
    fn always_resident_reports_resident_even_without_an_owned_child() {
        let mut registry = ProcessRegistry::new();
        registry.register("core", ProcessPolicy::AlwaysResident, None, Instant::now());
        assert!(registry.is_resident("core"));
    }

    #[test]
    fn set_resident_transitions_an_on_demand_role_to_resident_and_resets_its_idle_clock() {
        let mut registry = ProcessRegistry::new();
        let start = Instant::now();
        registry.register(
            "mcp-server",
            ProcessPolicy::OnDemand {
                idle_timeout: Duration::from_secs(60),
            },
            None,
            start,
        );
        assert!(!registry.is_resident("mcp-server"));

        let spawned_at = start + Duration::from_secs(500); // long past what would matter if the idle clock weren't reset
        assert!(
            registry
                .set_resident("mcp-server", spawn_dummy_child(), spawned_at)
                .is_none(),
            "a registered role's child must be accepted, not handed back"
        );

        assert!(registry.is_resident("mcp-server"));
        assert!(
            registry
                .idle_eligible_for_teardown(spawned_at + Duration::from_secs(1))
                .is_empty(),
            "freshly spawned must not be immediately idle-eligible"
        );
        assert_eq!(
            registry.idle_eligible_for_teardown(spawned_at + Duration::from_secs(61)),
            vec!["mcp-server".to_string()]
        );

        registry.teardown("mcp-server");
    }

    #[test]
    fn set_resident_on_an_unregistered_role_hands_the_child_back_instead_of_leaking_it() {
        let mut registry = ProcessRegistry::new();
        let mut child = registry
            .set_resident("nonexistent", spawn_dummy_child(), Instant::now())
            .expect("must hand the child back rather than silently dropping (and leaking) it");
        assert!(
            !registry.is_resident("nonexistent"),
            "must not create a phantom entry"
        );
        let _ = child.kill();
        let _ = child.wait();
    }

    #[test]
    fn idle_teardown_role_becomes_eligible_only_after_its_timeout_elapses() {
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

        assert!(
            registry
                .idle_eligible_for_teardown(start + Duration::from_secs(30))
                .is_empty(),
            "not yet past the timeout"
        );
        assert_eq!(
            registry.idle_eligible_for_teardown(start + Duration::from_secs(61)),
            vec!["mcp-server".to_string()]
        );

        registry.teardown("mcp-server");
    }

    #[test]
    fn mark_active_resets_the_idle_clock() {
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

        let later = start + Duration::from_secs(50);
        registry.mark_active("mcp-server", later);

        // 70s after registration, but only 20s after the reset activity -- still not eligible.
        assert!(registry
            .idle_eligible_for_teardown(start + Duration::from_secs(70))
            .is_empty());
        assert_eq!(
            registry.idle_eligible_for_teardown(later + Duration::from_secs(61)),
            vec!["mcp-server".to_string()]
        );

        registry.teardown("mcp-server");
    }

    #[test]
    fn a_role_with_no_resident_process_is_never_eligible() {
        // An `OnDemand` role not yet spawned -- nothing to tear down.
        let mut registry = ProcessRegistry::new();
        let start = Instant::now();
        registry.register(
            "mcp-server",
            ProcessPolicy::OnDemand {
                idle_timeout: Duration::from_secs(1),
            },
            None,
            start,
        );

        assert!(registry
            .idle_eligible_for_teardown(start + Duration::from_secs(1_000_000))
            .is_empty());
    }

    #[test]
    fn mark_active_on_an_unregistered_role_is_a_harmless_no_op() {
        let mut registry = ProcessRegistry::new();
        registry.mark_active("nonexistent", Instant::now()); // must not panic
    }

    #[test]
    fn teardown_on_an_unregistered_role_is_a_harmless_no_op() {
        let mut registry = ProcessRegistry::new();
        registry.teardown("nonexistent"); // must not panic
    }

    #[test]
    fn teardown_on_an_already_torn_down_role_is_a_harmless_no_op() {
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

        registry.teardown("mcp-server");
        assert!(!registry.is_resident("mcp-server"));
        registry.teardown("mcp-server"); // must not panic on the second call
    }

    #[test]
    fn teardown_actually_kills_the_real_child_process() {
        let mut registry = ProcessRegistry::new();
        let child = spawn_dummy_child();
        let pid = child.id();
        registry.register(
            "mcp-server",
            ProcessPolicy::IdleTeardown {
                idle_timeout: Duration::from_secs(1),
            },
            Some(child),
            Instant::now(),
        );

        registry.teardown("mcp-server");

        // `kill -0 <pid>` is a real, non-destructive liveness probe (no
        // signal actually delivered, just existence-checked) -- proves
        // the process this registry entry owned is genuinely gone, not
        // just forgotten by the registry's own bookkeeping. `teardown`'s
        // `Child::wait()` blocks until the OS has fully reaped it, so
        // this check is deterministic, not a race.
        let still_alive = Command::new("kill")
            .args(["-0", &pid.to_string()])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false);
        assert!(
            !still_alive,
            "the real child process must actually be terminated, not just marked non-resident"
        );
        assert!(!registry.is_resident("mcp-server"));
    }

    #[test]
    fn teardown_of_one_role_does_not_affect_another_roles_process() {
        let mut registry = ProcessRegistry::new();
        let start = Instant::now();
        registry.register("core", ProcessPolicy::AlwaysResident, None, start);
        registry.register(
            "mcp-server",
            ProcessPolicy::IdleTeardown {
                idle_timeout: Duration::from_secs(1),
            },
            Some(spawn_dummy_child()),
            start,
        );

        registry.teardown("mcp-server");

        assert!(
            registry.is_resident("core"),
            "tearing down one role must not affect an unrelated resident role"
        );
    }

    #[test]
    fn a_role_with_an_idle_timeout_but_not_yet_past_it_stays_resident() {
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

        assert!(registry.is_resident("mcp-server"));
        registry.teardown("mcp-server");
    }
}
