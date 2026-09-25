// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Pending assistant-settings proposals (`phase-7-local-ai/PLAN.md`, step R5b).
//!
//! A pure state machine with an injected clock: no I/O, no threads. It holds at
//! most **one** pending proposal, so the person is never facing a queue of
//! requests, and enforces the rate limits that stop an agent from turning the
//! consent prompt into a nuisance: at most [`MAX_ACCEPTED_PER_HOUR`] proposals
//! may reach the person per hour and at most [`MAX_ATTEMPTS_PER_HOUR`] may be
//! tried at all (a blocked proposal costs the person nothing, but it still costs
//! CPU and log space). A proposal expires after [`PENDING_LIFETIME`].
//!
//! Every proposal must pass the deterministic rule-base
//! ([`blueice_assistant_settings::proposal`]) *before* it becomes pending.
//! Approval is bound to the exact proposal: it must name the proposal's id and
//! the digest of the settings the person was shown, and it is refused if the
//! current settings have changed since the proposal was made (a human edit, or
//! an earlier approval, made it stale).

use blueice_assistant_settings::proposal::{self, Environment};
use blueice_assistant_settings::AssistantSettings;
use std::collections::VecDeque;
use std::time::{Duration, Instant};

pub const PENDING_LIFETIME: Duration = Duration::from_secs(10 * 60);
pub const MAX_ACCEPTED_PER_HOUR: usize = 5;
pub const MAX_ATTEMPTS_PER_HOUR: usize = 60;
const HOUR: Duration = Duration::from_secs(60 * 60);
/// How many finished proposals' outcomes are remembered for status queries.
const MAX_REMEMBERED: usize = 16;

/// Where a proposal stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Waiting for the person.
    Pending,
    Approved,
    Denied,
    /// Not decided within [`PENDING_LIFETIME`].
    Expired,
    /// The settings changed after the proposal was made, so it no longer
    /// describes a change from what is in force.
    Stale,
    /// No such proposal (never made, or forgotten).
    Unknown,
}

impl Status {
    /// A stable lowercase word for the wire and for logs.
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Pending => "pending",
            Status::Approved => "approved",
            Status::Denied => "denied",
            Status::Expired => "expired",
            Status::Stale => "stale",
            Status::Unknown => "unknown",
        }
    }
}

/// What the person is asked to decide.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingView {
    pub id: u64,
    /// The digest an approval must name.
    pub digest: String,
    /// The `label: before -> after` lines, computed against the settings in
    /// force when the proposal was made.
    pub diff: Vec<String>,
    pub proposed: AssistantSettings,
    pub expires_in: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProposeOutcome {
    /// It reached the person and now waits for them.
    Accepted {
        id: u64,
        digest: String,
        diff: Vec<String>,
    },
    /// The rule-base refused it; each entry is one violated rule in words.
    Blocked(Vec<String>),
    /// A proposal is already waiting; decide it first.
    PendingExists,
    /// Too many proposals this hour.
    RateLimited,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecisionError {
    /// No pending proposal has that id.
    NoSuchProposal,
    /// The digest is not the one the person was shown.
    DigestMismatch,
    Expired,
    /// The settings in force changed since the proposal was made.
    Stale,
}

struct Pending {
    id: u64,
    proposed: AssistantSettings,
    digest: String,
    base_digest: String,
    diff: Vec<String>,
    created: Instant,
}

#[derive(Default)]
pub struct ProposalStore {
    pending: Option<Pending>,
    next_id: u64,
    attempts: VecDeque<Instant>,
    accepted: VecDeque<Instant>,
    finished: VecDeque<(u64, Status)>,
}

impl ProposalStore {
    pub fn new() -> Self {
        ProposalStore::default()
    }

    fn prune(queue: &mut VecDeque<Instant>, now: Instant) {
        while queue
            .front()
            .is_some_and(|t| now.saturating_duration_since(*t) >= HOUR)
        {
            queue.pop_front();
        }
    }

    fn remember(&mut self, id: u64, status: Status) {
        self.finished.push_back((id, status));
        while self.finished.len() > MAX_REMEMBERED {
            self.finished.pop_front();
        }
    }

    /// Retires the pending proposal if it has run out of time.
    fn expire(&mut self, now: Instant) {
        if self
            .pending
            .as_ref()
            .is_some_and(|p| now.saturating_duration_since(p.created) >= PENDING_LIFETIME)
        {
            let id = self.pending.take().expect("checked above").id;
            self.remember(id, Status::Expired);
        }
    }

    /// Considers an agent's proposal to change `current` into `proposed`.
    pub fn propose(
        &mut self,
        now: Instant,
        current: &AssistantSettings,
        proposed: AssistantSettings,
        env: &Environment<'_>,
    ) -> ProposeOutcome {
        Self::prune(&mut self.attempts, now);
        Self::prune(&mut self.accepted, now);
        self.expire(now);
        if self.attempts.len() >= MAX_ATTEMPTS_PER_HOUR {
            return ProposeOutcome::RateLimited;
        }
        self.attempts.push_back(now);
        if self.pending.is_some() {
            return ProposeOutcome::PendingExists;
        }
        if let Err(violations) = proposal::evaluate(current, &proposed, env) {
            return ProposeOutcome::Blocked(violations.iter().map(ToString::to_string).collect());
        }
        if self.accepted.len() >= MAX_ACCEPTED_PER_HOUR {
            return ProposeOutcome::RateLimited;
        }
        self.accepted.push_back(now);
        self.next_id += 1;
        let id = self.next_id;
        let digest = proposal::digest(&proposed);
        let diff = proposal::diff_lines(current, &proposed);
        self.pending = Some(Pending {
            id,
            proposed,
            digest: digest.clone(),
            base_digest: proposal::digest(current),
            diff: diff.clone(),
            created: now,
        });
        ProposeOutcome::Accepted { id, digest, diff }
    }

    /// The proposal waiting for the person, if any.
    pub fn pending(&mut self, now: Instant) -> Option<PendingView> {
        self.expire(now);
        self.pending.as_ref().map(|p| PendingView {
            id: p.id,
            digest: p.digest.clone(),
            diff: p.diff.clone(),
            proposed: p.proposed.clone(),
            expires_in: PENDING_LIFETIME.saturating_sub(now.saturating_duration_since(p.created)),
        })
    }

    /// Approves proposal `id`, which must be the one the person saw (`digest`).
    /// `current` is what is in force now; if it is no longer what the proposal
    /// was made against, the proposal is stale and is retired instead. On
    /// success the proposed settings are returned for the caller to apply.
    pub fn approve(
        &mut self,
        now: Instant,
        id: u64,
        digest: &str,
        current: &AssistantSettings,
    ) -> Result<AssistantSettings, DecisionError> {
        self.expire(now);
        let pending = match &self.pending {
            Some(p) if p.id == id => p,
            _ if self
                .finished
                .iter()
                .any(|(fid, s)| *fid == id && *s == Status::Expired) =>
            {
                return Err(DecisionError::Expired)
            }
            _ => return Err(DecisionError::NoSuchProposal),
        };
        if pending.digest != digest {
            // Not consumed: the person may still approve the proposal they saw.
            return Err(DecisionError::DigestMismatch);
        }
        if pending.base_digest != proposal::digest(current) {
            self.pending = None;
            self.remember(id, Status::Stale);
            return Err(DecisionError::Stale);
        }
        let approved = self.pending.take().expect("checked above").proposed;
        self.remember(id, Status::Approved);
        Ok(approved)
    }

    /// Declines proposal `id`.
    pub fn deny(&mut self, now: Instant, id: u64) -> Result<(), DecisionError> {
        self.expire(now);
        match &self.pending {
            Some(p) if p.id == id => {
                self.pending = None;
                self.remember(id, Status::Denied);
                Ok(())
            }
            _ if self
                .finished
                .iter()
                .any(|(fid, s)| *fid == id && *s == Status::Expired) =>
            {
                Err(DecisionError::Expired)
            }
            _ => Err(DecisionError::NoSuchProposal),
        }
    }

    /// Retires the pending proposal because the settings changed underneath it
    /// (for example a direct edit by the person).
    pub fn invalidate_pending(&mut self) {
        if let Some(p) = self.pending.take() {
            self.remember(p.id, Status::Stale);
        }
    }

    /// Where proposal `id` stands.
    pub fn status(&mut self, now: Instant, id: u64) -> Status {
        self.expire(now);
        if self.pending.as_ref().is_some_and(|p| p.id == id) {
            return Status::Pending;
        }
        self.finished
            .iter()
            .rev()
            .find(|(fid, _)| *fid == id)
            .map_or(Status::Unknown, |(_, status)| *status)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    fn now() -> Instant {
        Instant::now()
    }

    fn env<R>(f: impl FnOnce(&Environment<'_>) -> R) -> R {
        let exists = |_: &Path| true;
        let canon = |p: &Path| Some(p.to_path_buf());
        f(&Environment {
            is_regular_file: &exists,
            canonicalize: &canon,
            physical_memory_mb: 64 * 1024,
            models_dir: PathBuf::from("/models"),
        })
    }

    fn current() -> AssistantSettings {
        AssistantSettings {
            max_resident_mb: Some(2048),
            ..AssistantSettings::default()
        }
    }

    fn proposing(nice: i32) -> AssistantSettings {
        AssistantSettings { nice, ..current() }
    }

    fn propose(store: &mut ProposalStore, at: Instant, nice: i32) -> ProposeOutcome {
        env(|e| store.propose(at, &current(), proposing(nice), e))
    }

    fn accepted(outcome: ProposeOutcome) -> (u64, String) {
        match outcome {
            ProposeOutcome::Accepted { id, digest, .. } => (id, digest),
            other => panic!("expected Accepted, got {other:?}"),
        }
    }

    #[test]
    fn a_proposal_that_passes_the_rules_becomes_the_one_pending_proposal() {
        let mut store = ProposalStore::new();
        let t = now();
        let outcome = propose(&mut store, t, 12);
        let ProposeOutcome::Accepted { id, digest, diff } = outcome else {
            panic!("expected Accepted")
        };
        assert_eq!(diff, ["Priority (nice): 10 -> 12"]);
        let view = store.pending(t).unwrap();
        assert_eq!((view.id, view.digest.as_str()), (id, digest.as_str()));
        assert_eq!(view.proposed, proposing(12));
        assert_eq!(view.expires_in, PENDING_LIFETIME);
        assert_eq!(store.status(t, id), Status::Pending);
    }

    #[test]
    fn a_proposal_that_breaks_a_rule_is_blocked_with_every_reason_and_never_pends() {
        let mut store = ProposalStore::new();
        let t = now();
        let outcome = env(|e| {
            store.propose(
                t,
                &current(),
                AssistantSettings {
                    nice: 0,               // too high a priority
                    max_resident_mb: None, // removes the ceiling
                    ..AssistantSettings::default()
                },
                e,
            )
        });
        let ProposeOutcome::Blocked(reasons) = outcome else {
            panic!("expected Blocked")
        };
        assert_eq!(reasons.len(), 2, "{reasons:?}");
        assert!(store.pending(t).is_none());
        // A blocked proposal did not use up the person's attention quota.
        for n in 0..MAX_ACCEPTED_PER_HOUR {
            assert!(matches!(
                propose(&mut store, t, 11 + n as i32),
                ProposeOutcome::Accepted { .. }
            ));
            let id = store.pending(t).unwrap().id;
            store.deny(t, id).unwrap();
        }
    }

    #[test]
    fn a_second_proposal_waits_until_the_first_is_decided() {
        let mut store = ProposalStore::new();
        let t = now();
        let (id, _) = accepted(propose(&mut store, t, 12));
        assert_eq!(propose(&mut store, t, 13), ProposeOutcome::PendingExists);
        store.deny(t, id).unwrap();
        assert!(matches!(
            propose(&mut store, t, 13),
            ProposeOutcome::Accepted { .. }
        ));
    }

    #[test]
    fn approval_names_the_id_and_the_digest_and_returns_the_proposed_settings() {
        let mut store = ProposalStore::new();
        let t = now();
        let (id, digest) = accepted(propose(&mut store, t, 12));
        assert_eq!(
            store.approve(t, id, "not-the-digest", &current()),
            Err(DecisionError::DigestMismatch)
        );
        assert_eq!(
            store.status(t, id),
            Status::Pending,
            "a wrong digest consumes nothing"
        );
        assert_eq!(
            store.approve(t, id + 1, &digest, &current()),
            Err(DecisionError::NoSuchProposal)
        );
        assert_eq!(store.approve(t, id, &digest, &current()), Ok(proposing(12)));
        assert_eq!(store.status(t, id), Status::Approved);
        // One approval, one use.
        assert_eq!(
            store.approve(t, id, &digest, &current()),
            Err(DecisionError::NoSuchProposal)
        );
    }

    #[test]
    fn a_proposal_goes_stale_if_the_settings_in_force_changed_after_it_was_made() {
        let mut store = ProposalStore::new();
        let t = now();
        let (id, digest) = accepted(propose(&mut store, t, 12));
        let changed_since = AssistantSettings {
            idle_timeout_secs: 300,
            ..current()
        };
        assert_eq!(
            store.approve(t, id, &digest, &changed_since),
            Err(DecisionError::Stale)
        );
        assert_eq!(store.status(t, id), Status::Stale);
        assert!(store.pending(t).is_none(), "and it is retired");
    }

    #[test]
    fn a_direct_edit_retires_the_pending_proposal() {
        let mut store = ProposalStore::new();
        let t = now();
        let (id, _) = accepted(propose(&mut store, t, 12));
        store.invalidate_pending();
        assert_eq!(store.status(t, id), Status::Stale);
        assert!(store.pending(t).is_none());
        store.invalidate_pending(); // nothing pending: harmless
    }

    #[test]
    fn denying_retires_it_and_remembers_the_outcome() {
        let mut store = ProposalStore::new();
        let t = now();
        let (id, digest) = accepted(propose(&mut store, t, 12));
        assert_eq!(store.deny(t, id + 1), Err(DecisionError::NoSuchProposal));
        store.deny(t, id).unwrap();
        assert_eq!(store.status(t, id), Status::Denied);
        assert_eq!(
            store.approve(t, id, &digest, &current()),
            Err(DecisionError::NoSuchProposal)
        );
    }

    #[test]
    fn a_proposal_expires_and_can_no_longer_be_decided() {
        let mut store = ProposalStore::new();
        let t = now();
        let (id, digest) = accepted(propose(&mut store, t, 12));
        let almost = t + PENDING_LIFETIME - Duration::from_secs(1);
        assert_eq!(
            store.pending(almost).unwrap().expires_in,
            Duration::from_secs(1)
        );
        let late = t + PENDING_LIFETIME;
        assert!(store.pending(late).is_none());
        assert_eq!(store.status(late, id), Status::Expired);
        assert_eq!(
            store.approve(late, id, &digest, &current()),
            Err(DecisionError::Expired)
        );
        assert_eq!(store.deny(late, id), Err(DecisionError::Expired));
        // Expiry frees the slot for a new proposal.
        assert!(matches!(
            propose(&mut store, late, 13),
            ProposeOutcome::Accepted { .. }
        ));
    }

    #[test]
    fn at_most_five_proposals_reach_the_person_in_an_hour() {
        let mut store = ProposalStore::new();
        let t = now();
        for n in 0..MAX_ACCEPTED_PER_HOUR {
            let at = t + Duration::from_secs(n as u64);
            accepted(propose(&mut store, at, 11 + n as i32));
            let id = store.pending(at).unwrap().id;
            store.deny(at, id).unwrap();
        }
        let sixth = t + Duration::from_secs(10);
        assert_eq!(propose(&mut store, sixth, 17), ProposeOutcome::RateLimited);
        // An hour after the first, room opens up again.
        let later = t + HOUR + Duration::from_secs(1);
        assert!(matches!(
            propose(&mut store, later, 17),
            ProposeOutcome::Accepted { .. }
        ));
    }

    #[test]
    fn attempts_are_capped_even_when_every_one_is_blocked() {
        let mut store = ProposalStore::new();
        let t = now();
        for _ in 0..MAX_ATTEMPTS_PER_HOUR {
            assert!(matches!(
                propose(&mut store, t, 0),
                ProposeOutcome::Blocked(_)
            ));
        }
        assert_eq!(propose(&mut store, t, 0), ProposeOutcome::RateLimited);
        assert_eq!(
            propose(&mut store, t, 12),
            ProposeOutcome::RateLimited,
            "even a good one waits"
        );
        let later = t + HOUR + Duration::from_secs(1);
        assert!(matches!(
            propose(&mut store, later, 12),
            ProposeOutcome::Accepted { .. }
        ));
    }

    #[test]
    fn every_status_has_a_distinct_wire_word() {
        let words: std::collections::HashSet<_> = [
            Status::Pending,
            Status::Approved,
            Status::Denied,
            Status::Expired,
            Status::Stale,
            Status::Unknown,
        ]
        .into_iter()
        .map(Status::as_str)
        .collect();
        assert_eq!(words.len(), 6);
    }

    #[test]
    fn unknown_ids_and_forgotten_outcomes_report_unknown() {
        let mut store = ProposalStore::new();
        let t = now();
        assert_eq!(store.status(t, 99), Status::Unknown);
        // Outcomes are remembered only up to a bound.
        for n in 0..(MAX_REMEMBERED + 3) {
            let at = t + HOUR * (n as u32 + 1);
            accepted(propose(&mut store, at, 11));
            let id = store.pending(at).unwrap().id;
            store.deny(at, id).unwrap();
        }
        let now_late = t + HOUR * 100;
        assert_eq!(
            store.status(now_late, 1),
            Status::Unknown,
            "the oldest was forgotten"
        );
        assert_eq!(
            store.status(now_late, (MAX_REMEMBERED + 3) as u64),
            Status::Denied
        );
    }
}
