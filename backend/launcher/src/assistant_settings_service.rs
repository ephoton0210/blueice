// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The launcher's owner of the assistant's settings
//! (`phase-7-local-ai/PLAN.md`, step R5c): the settings in force, the settings
//! file, the pending-proposal store, and the running supervisor to reconfigure.
//!
//! Two doors lead in, and they are deliberately different:
//!
//! * **Proposals** ([`Self::propose`], [`Self::proposal_status`]) come from an
//!   agent through the operator-control socket. They can only *propose*: the
//!   rule-base screens them, and nothing changes until the person approves.
//! * **Decisions and direct edits** ([`Self::approve`], [`Self::deny`],
//!   [`Self::edit`]) are called only by the launcher's own trusted-window
//!   handler, over the private pipes no agent can reach. A direct edit by the
//!   person passes the validator only -- the person is the authority.
//!
//! Applying a change validates it, writes the file atomically, updates the
//! settings in force, and reconfigures the supervisor, so it takes effect at
//! once rather than at the next launcher start.

use crate::assistant::AssistantSupervisor;
use crate::assistant_proposals::{
    DecisionError, PendingView, ProposalStore, ProposeOutcome, Status,
};
use crate::trusted_window::{PendingAssistantProposal, TrustedWindowReply, TrustedWindowRequest};
use blueice_assistant_settings::proposal::Environment;
use blueice_assistant_settings::{proposal, AssistantSettings};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, Weak};
use std::time::Instant;

/// The BlueIce models directory: always an allowed place for an agent to point
/// the assistant at a model file.
pub fn default_models_dir() -> PathBuf {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local").join("share"))
        })
        .unwrap_or_else(std::env::temp_dir);
    base.join("blueice").join("models")
}

/// Total physical memory in MiB, or a conservative fallback if it cannot be read
/// (a small figure makes the ceiling rule stricter, never looser).
pub fn physical_memory_mb() -> u64 {
    let mut system = sysinfo::System::new();
    system.refresh_memory();
    match system.total_memory() / (1024 * 1024) {
        0 => 1024,
        mb => mb,
    }
}

pub struct AssistantSettingsService {
    file: PathBuf,
    current: Mutex<AssistantSettings>,
    store: Mutex<ProposalStore>,
    /// Weak, so the supervisor is dropped (and its child killed) when the
    /// launcher lets go of it, not kept alive by a detached control thread.
    supervisor: Weak<AssistantSupervisor>,
    models_dir: PathBuf,
    physical_memory_mb: u64,
}

impl AssistantSettingsService {
    pub fn new(
        file: PathBuf,
        initial: AssistantSettings,
        supervisor: Weak<AssistantSupervisor>,
    ) -> Self {
        Self::with_environment(
            file,
            initial,
            supervisor,
            default_models_dir(),
            physical_memory_mb(),
        )
    }

    /// [`Self::new`] with an explicit models directory and memory size, for tests.
    pub fn with_environment(
        file: PathBuf,
        initial: AssistantSettings,
        supervisor: Weak<AssistantSupervisor>,
        models_dir: PathBuf,
        physical_memory_mb: u64,
    ) -> Self {
        AssistantSettingsService {
            file,
            current: Mutex::new(initial),
            store: Mutex::new(ProposalStore::new()),
            supervisor,
            models_dir,
            physical_memory_mb,
        }
    }

    fn with_environment_view<R>(&self, f: impl FnOnce(&Environment<'_>) -> R) -> R {
        let is_regular_file = |path: &Path| std::fs::metadata(path).is_ok_and(|m| m.is_file());
        let canonicalize = |path: &Path| std::fs::canonicalize(path).ok();
        f(&Environment {
            is_regular_file: &is_regular_file,
            canonicalize: &canonicalize,
            physical_memory_mb: self.physical_memory_mb,
            models_dir: self.models_dir.clone(),
        })
    }

    /// The settings in force.
    pub fn current(&self) -> AssistantSettings {
        self.current
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// An agent's proposal: screened by the rule-base, then held for the person.
    pub fn propose(&self, proposed: AssistantSettings) -> ProposeOutcome {
        let current = self.current();
        let mut store = self.store.lock().unwrap_or_else(|e| e.into_inner());
        self.with_environment_view(|env| store.propose(Instant::now(), &current, proposed, env))
    }

    pub fn proposal_status(&self, id: u64) -> Status {
        self.store
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .status(Instant::now(), id)
    }

    /// What the person is being asked to decide, if anything.
    pub fn pending(&self) -> Option<PendingView> {
        self.store
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .pending(Instant::now())
    }

    /// The person approves proposal `id`, naming the digest they were shown.
    pub fn approve(&self, id: u64, digest: &str) -> Result<(), String> {
        let current = self.current();
        let approved = self
            .store
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .approve(Instant::now(), id, digest, &current)
            .map_err(|error| match error {
                DecisionError::NoSuchProposal => "there is no such pending proposal".to_string(),
                DecisionError::DigestMismatch => {
                    "that is not the proposal that was shown".to_string()
                }
                DecisionError::Expired => "the proposal expired".to_string(),
                DecisionError::Stale => {
                    "the settings changed after the proposal was made".to_string()
                }
            })?;
        self.apply(approved)
    }

    /// The person declines proposal `id`.
    pub fn deny(&self, id: u64) -> Result<(), String> {
        self.store
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .deny(Instant::now(), id)
            .map_err(|error| match error {
                DecisionError::Expired => "the proposal expired".to_string(),
                _ => "there is no such pending proposal".to_string(),
            })
    }

    /// The person edits the settings directly. Only the validator applies.
    pub fn edit(&self, settings: AssistantSettings) -> Result<(), String> {
        settings.validate()?;
        self.apply(settings)?;
        // Whatever an agent proposed was made against settings that are gone.
        self.store
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .invalidate_pending();
        Ok(())
    }

    /// Validates, persists, records, and reconfigures -- in that order, so a
    /// failed write leaves everything as it was.
    fn apply(&self, settings: AssistantSettings) -> Result<(), String> {
        settings.validate()?;
        blueice_assistant_settings::save(&self.file, &settings)?;
        *self.current.lock().unwrap_or_else(|e| e.into_inner()) = settings.clone();
        if let Some(supervisor) = self.supervisor.upgrade() {
            supervisor
                .reconfigure_settings(&settings)
                .map_err(|error| {
                    format!("saved, but could not reconfigure the assistant: {error}")
                })?;
        }
        Ok(())
    }

    /// What the trusted window shows: the settings in force and the proposal
    /// waiting for a decision.
    pub fn trusted_state(&self) -> TrustedWindowReply {
        TrustedWindowReply::AssistantSettingsState {
            current: Box::new(self.current()),
            pending: self.pending().map(|view| {
                Box::new(PendingAssistantProposal {
                    id: view.id,
                    digest: view.digest,
                    diff: view.diff,
                    proposed: view.proposed,
                    seconds_left: view.expires_in.as_secs(),
                })
            }),
        }
    }

    /// Answers a trusted-window request about the assistant's settings, or
    /// `None` if `request` is about something else. Only the launcher's own
    /// trusted-window handler calls this: these requests travel on private
    /// pipes, never on a socket an agent can reach.
    pub fn handle_trusted(&self, request: TrustedWindowRequest) -> Option<TrustedWindowReply> {
        let outcome = match request {
            TrustedWindowRequest::InspectAssistantSettings => Ok(()),
            TrustedWindowRequest::ApproveAssistantProposal { id, digest } => {
                self.approve(id, &digest)
            }
            TrustedWindowRequest::DenyAssistantProposal { id } => self.deny(id),
            TrustedWindowRequest::EditAssistantSettings { settings } => self.edit(settings),
            _ => return None,
        };
        Some(match outcome {
            Ok(()) => self.trusted_state(),
            Err(reason) => TrustedWindowReply::Rejected { reason },
        })
    }

    /// The `label: before -> after` lines between what is in force and `other`.
    pub fn diff_from_current(&self, other: &AssistantSettings) -> Vec<String> {
        proposal::diff_lines(&self.current(), other)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assistant::ROLE;
    use crate::supervisor::ProcessRegistry;
    use blueice_assistant_settings::{BackendKind, LoopbackSettings};
    use std::io::{Read, Write};
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::net::UnixStream;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    /// A fake "assistant" executable: it records its own arguments to
    /// `<socket>.argv` and then echoes on the socket it is told to listen on, so
    /// tests can see exactly what the launcher started it with.
    const FAKE_ASSISTANT: &str = r#"#!/usr/bin/env python3
import socket, sys, threading
path = sys.argv[sys.argv.index("--socket") + 1]
open(path + ".argv", "w").write(" ".join(sys.argv[1:]))
server = socket.socket(socket.AF_UNIX)
server.bind(path)
server.listen(8)
def serve(conn):
    while True:
        data = conn.recv(4096)
        if not data:
            break
        conn.sendall(data)
    conn.close()
while True:
    conn, _ = server.accept()
    threading.Thread(target=serve, args=(conn,), daemon=True).start()
"#;

    struct Rig {
        dir: PathBuf,
        service: AssistantSettingsService,
        supervisor: Arc<AssistantSupervisor>,
    }

    fn unique_dir(tag: &str) -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "as-svc-{tag}-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn rig(tag: &str, initial: AssistantSettings) -> Rig {
        let dir = unique_dir(tag);
        let bin = dir.join("fake-assistant");
        std::fs::write(&bin, FAKE_ASSISTANT).unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        let file = dir.join("assistant-settings.json");
        blueice_assistant_settings::save(&file, &initial).unwrap();
        let registry = Arc::new(Mutex::new(ProcessRegistry::new()));
        let supervisor = Arc::new(AssistantSupervisor::start(&initial, bin, registry).unwrap());
        let service = AssistantSettingsService::with_environment(
            file,
            initial,
            Arc::downgrade(&supervisor),
            dir.join("models"),
            32 * 1024,
        );
        Rig {
            dir,
            service,
            supervisor,
        }
    }

    impl Drop for Rig {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    fn loopback(model: &str) -> LoopbackSettings {
        LoopbackSettings {
            provider: "llamacpp".into(),
            base_url: "http://127.0.0.1:8080/v1/".into(),
            model: model.into(),
        }
    }

    fn configured(model: &str) -> AssistantSettings {
        AssistantSettings {
            backend: BackendKind::Loopback,
            loopback: Some(loopback(model)),
            max_resident_mb: Some(2048),
            ..AssistantSettings::default()
        }
    }

    /// Talks to the assistant through the launcher's public socket and returns
    /// the arguments the running fake assistant was started with.
    fn started_with(rig: &Rig) -> String {
        let mut stream = UnixStream::connect(rig.supervisor.public_socket()).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        stream.write_all(b"ping").unwrap();
        let mut reply = [0u8; 4];
        stream.read_exact(&mut reply).unwrap();
        assert_eq!(&reply, b"ping");
        let pid_socket = rig.supervisor_private_socket();
        std::fs::read_to_string(format!("{}.argv", pid_socket.display())).unwrap()
    }

    impl Rig {
        fn supervisor_private_socket(&self) -> PathBuf {
            self.supervisor.private_socket_for_tests()
        }
    }

    #[test]
    fn an_accepted_proposal_changes_nothing_until_the_person_approves_it() {
        let rig = rig("propose", configured("first"));
        let proposed = AssistantSettings {
            nice: 12,
            ..configured("first")
        };
        let ProposeOutcome::Accepted { id, digest, diff } = rig.service.propose(proposed.clone())
        else {
            panic!("the proposal should have reached the person")
        };
        assert_eq!(diff, ["Priority (nice): 10 -> 12"]);
        assert_eq!(
            rig.service.current(),
            configured("first"),
            "still the old settings"
        );
        assert_eq!(
            blueice_assistant_settings::load(&rig.service.file).unwrap(),
            configured("first")
        );
        assert_eq!(rig.service.proposal_status(id), Status::Pending);
        assert_eq!(rig.service.pending().unwrap().digest, digest);
        assert!(started_with(&rig).contains("--model-name first"));
    }

    #[test]
    fn approval_saves_reconfigures_and_takes_effect_at_once() {
        let rig = rig("approve", configured("first"));
        started_with(&rig); // the first assistant is running with "first"
        let proposed = AssistantSettings {
            loopback: Some(loopback("second")),
            ..configured("first")
        };
        let ProposeOutcome::Accepted { id, digest, .. } = rig.service.propose(proposed.clone())
        else {
            panic!("accepted")
        };
        rig.service.approve(id, &digest).unwrap();
        assert_eq!(rig.service.current(), proposed);
        assert_eq!(
            blueice_assistant_settings::load(&rig.service.file).unwrap(),
            proposed
        );
        assert_eq!(rig.service.proposal_status(id), Status::Approved);
        // The old assistant was torn down; the next one starts with the new flags.
        assert!(started_with(&rig).contains("--model-name second"));
        assert!(rig.supervisor.spawn_count() >= 2);
        let _ = ROLE;
    }

    #[test]
    fn a_wrong_digest_or_id_approves_nothing() {
        let rig = rig("wrong", configured("first"));
        let ProposeOutcome::Accepted { id, digest, .. } = rig.service.propose(AssistantSettings {
            nice: 12,
            ..configured("first")
        }) else {
            panic!("accepted")
        };
        assert!(rig
            .service
            .approve(id, "0000")
            .unwrap_err()
            .contains("not the proposal"));
        assert!(rig
            .service
            .approve(id + 9, &digest)
            .unwrap_err()
            .contains("no such"));
        assert_eq!(rig.service.current(), configured("first"));
        rig.service.approve(id, &digest).unwrap(); // the real one still works
        assert_eq!(rig.service.current().nice, 12);
    }

    #[test]
    fn a_direct_edit_by_the_person_passes_only_the_validator_and_retires_a_pending_proposal() {
        let rig = rig("edit", configured("first"));
        let ProposeOutcome::Accepted { id, digest, .. } = rig.service.propose(AssistantSettings {
            nice: 12,
            ..configured("first")
        }) else {
            panic!("accepted")
        };
        // The person may do what an agent may not: remove the ceiling, nice 0.
        let edited = AssistantSettings {
            max_resident_mb: None,
            nice: 0,
            ..configured("first")
        };
        rig.service.edit(edited.clone()).unwrap();
        assert_eq!(rig.service.current(), edited);
        assert_eq!(rig.service.proposal_status(id), Status::Stale);
        assert!(rig.service.approve(id, &digest).is_err());
        // Invalid settings are still refused, and change nothing.
        let invalid = AssistantSettings {
            nice: 99,
            ..edited.clone()
        };
        assert!(rig.service.edit(invalid).is_err());
        assert_eq!(rig.service.current(), edited);
    }

    #[test]
    fn denying_leaves_everything_as_it_was() {
        let rig = rig("deny", configured("first"));
        let ProposeOutcome::Accepted { id, .. } = rig.service.propose(AssistantSettings {
            nice: 12,
            ..configured("first")
        }) else {
            panic!("accepted")
        };
        rig.service.deny(id).unwrap();
        assert_eq!(rig.service.proposal_status(id), Status::Denied);
        assert_eq!(rig.service.current(), configured("first"));
        assert!(rig.service.deny(id).is_err(), "already decided");
        assert!(rig.service.pending().is_none());
    }

    #[test]
    fn the_rule_base_blocks_an_agent_but_not_the_person() {
        let rig = rig("rules", configured("first"));
        let risky = AssistantSettings {
            max_resident_mb: None, // removing an existing ceiling
            ..configured("first")
        };
        match rig.service.propose(risky.clone()) {
            ProposeOutcome::Blocked(reasons) => {
                assert!(
                    reasons.iter().any(|r| r.contains("may not be removed")),
                    "{reasons:?}"
                )
            }
            other => panic!("expected Blocked, got {other:?}"),
        }
        assert!(
            rig.service.pending().is_none(),
            "a blocked proposal never reaches the person"
        );
        rig.service.edit(risky).unwrap();
    }

    #[test]
    fn a_failed_save_changes_nothing_and_a_dropped_supervisor_does_not_stop_a_save() {
        let rig = rig("save", configured("first"));
        // Make the settings file's directory unwritable by replacing the file
        // with a directory: the atomic rename onto it must fail.
        std::fs::remove_file(&rig.service.file).unwrap();
        std::fs::create_dir(&rig.service.file).unwrap();
        let attempt = AssistantSettings {
            nice: 15,
            ..configured("first")
        };
        assert!(rig.service.edit(attempt).is_err());
        assert_eq!(
            rig.service.current(),
            configured("first"),
            "memory still matches what is on disk"
        );
        std::fs::remove_dir(&rig.service.file).unwrap();

        // With the supervisor gone the file is still written.
        let dir = unique_dir("gone");
        let file = dir.join("assistant-settings.json");
        let service = AssistantSettingsService::with_environment(
            file.clone(),
            AssistantSettings::default(),
            Weak::new(),
            dir.join("models"),
            32 * 1024,
        );
        let settings = AssistantSettings {
            nice: 15,
            ..AssistantSettings::default()
        };
        service.edit(settings.clone()).unwrap();
        assert_eq!(blueice_assistant_settings::load(&file).unwrap(), settings);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_trusted_window_sees_the_pending_proposal_and_decides_it_by_id_and_digest() {
        let rig = rig("trusted", configured("first"));
        let inspect = |rig: &Rig| {
            rig.service
                .handle_trusted(TrustedWindowRequest::InspectAssistantSettings)
                .unwrap()
        };
        let TrustedWindowReply::AssistantSettingsState { current, pending } = inspect(&rig) else {
            panic!("expected the state")
        };
        assert_eq!(*current, configured("first"));
        assert!(pending.is_none());

        let ProposeOutcome::Accepted { id, digest, .. } = rig.service.propose(AssistantSettings {
            nice: 12,
            ..configured("first")
        }) else {
            panic!("accepted")
        };
        let TrustedWindowReply::AssistantSettingsState { pending, .. } = inspect(&rig) else {
            panic!("expected the state")
        };
        let pending = pending.expect("the proposal is waiting");
        assert_eq!((pending.id, pending.digest.as_str()), (id, digest.as_str()));
        assert_eq!(pending.diff, ["Priority (nice): 10 -> 12"]);
        assert!(pending.seconds_left > 500);

        // A wrong digest is refused and leaves the proposal waiting.
        let refused = rig
            .service
            .handle_trusted(TrustedWindowRequest::ApproveAssistantProposal {
                id,
                digest: "0".repeat(64),
            })
            .unwrap();
        assert!(matches!(refused, TrustedWindowReply::Rejected { .. }));
        assert!(rig.service.pending().is_some());

        let approved = rig
            .service
            .handle_trusted(TrustedWindowRequest::ApproveAssistantProposal { id, digest })
            .unwrap();
        let TrustedWindowReply::AssistantSettingsState { current, pending } = approved else {
            panic!("expected the new state")
        };
        assert_eq!(current.nice, 12);
        assert!(pending.is_none());
    }

    #[test]
    fn denying_and_editing_over_the_trusted_pipe_reply_with_the_new_truth() {
        let rig = rig("trusted-more", configured("first"));
        let ProposeOutcome::Accepted { id, .. } = rig.service.propose(AssistantSettings {
            nice: 12,
            ..configured("first")
        }) else {
            panic!("accepted")
        };
        let denied = rig
            .service
            .handle_trusted(TrustedWindowRequest::DenyAssistantProposal { id })
            .unwrap();
        assert!(matches!(
            denied,
            TrustedWindowReply::AssistantSettingsState { pending: None, .. }
        ));
        assert!(matches!(
            rig.service
                .handle_trusted(TrustedWindowRequest::DenyAssistantProposal { id })
                .unwrap(),
            TrustedWindowReply::Rejected { .. }
        ));

        let edited = AssistantSettings {
            nice: 0,
            max_resident_mb: None,
            ..configured("first")
        };
        let reply = rig
            .service
            .handle_trusted(TrustedWindowRequest::EditAssistantSettings {
                settings: edited.clone(),
            })
            .unwrap();
        assert!(
            matches!(&reply, TrustedWindowReply::AssistantSettingsState { current, .. } if **current == edited)
        );
        let invalid = AssistantSettings { nice: 99, ..edited };
        assert!(matches!(
            rig.service
                .handle_trusted(TrustedWindowRequest::EditAssistantSettings { settings: invalid })
                .unwrap(),
            TrustedWindowReply::Rejected { .. }
        ));
    }

    #[test]
    fn requests_that_are_not_about_the_assistant_are_left_to_the_permission_handler() {
        let rig = rig("not-mine", configured("first"));
        assert!(rig
            .service
            .handle_trusted(TrustedWindowRequest::Inspect)
            .is_none());
    }

    #[test]
    fn the_diff_from_current_names_only_what_would_change() {
        let rig = rig("diff", configured("first"));
        let other = AssistantSettings {
            nice: 15,
            ..configured("first")
        };
        assert_eq!(
            rig.service.diff_from_current(&other),
            ["Priority (nice): 10 -> 15"]
        );
    }

    #[test]
    fn the_real_environment_values_are_sane() {
        assert!(physical_memory_mb() >= 256);
        assert_eq!(default_models_dir().file_name().unwrap(), "models");
        assert_eq!(
            default_models_dir().parent().unwrap().file_name().unwrap(),
            "blueice"
        );
    }
}
