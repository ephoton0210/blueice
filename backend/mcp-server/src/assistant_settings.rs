// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The MCP side of assistant settings proposals
//! (`phase-7-local-ai/PLAN.md`, step R7).
//!
//! An agent can *read* the settings in force and *propose* a change. It cannot
//! apply one: a proposal is screened by the launcher's deterministic rule-base
//! and then waits for the person to approve it in BlueIce's trusted window,
//! which no MCP client can reach. Everything here talks to the launcher's
//! operator-control socket, whose protocol has no request that approves,
//! denies, or edits.

use blueice_assistant_settings::{
    AssistantSettings, BackendKind, CandleSettings, LoopbackSettings, SETTINGS_VERSION,
};
use blueice_launcher::control::{
    read_control_reply, write_control_request, ControlReply, ControlRequest,
};
use rmcp::schemars;
use std::io;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

const CONTROL_TIMEOUT: Duration = Duration::from_secs(10);

/// What the launcher answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsOutcome {
    /// The settings in force.
    InForce(Box<AssistantSettings>),
    /// The proposal passed the rule-base and waits for the person. Nothing has
    /// changed yet.
    Accepted {
        id: u64,
        digest: String,
        diff: Vec<String>,
    },
    /// The rule-base refused it; each entry is one violated rule.
    Blocked(Vec<String>),
    /// It could not be considered (one is already waiting, too many this hour,
    /// or no assistant is supervised).
    Refused(String),
    /// Where a proposal stands: pending, approved, denied, expired, stale, or
    /// unknown.
    Status(String),
}

fn ask(control_socket: &Path, request: &ControlRequest) -> io::Result<ControlReply> {
    let mut stream = UnixStream::connect(control_socket)?;
    stream.set_read_timeout(Some(CONTROL_TIMEOUT))?;
    stream.set_write_timeout(Some(CONTROL_TIMEOUT))?;
    write_control_request(&mut stream, request)?;
    read_control_reply(&mut stream)
}

fn outcome(reply: ControlReply) -> io::Result<SettingsOutcome> {
    Ok(match reply {
        ControlReply::AssistantSettingsInForce { settings } => SettingsOutcome::InForce(settings),
        ControlReply::AssistantProposalAccepted { id, digest, diff } => {
            SettingsOutcome::Accepted { id, digest, diff }
        }
        ControlReply::AssistantProposalBlocked { violations } => {
            SettingsOutcome::Blocked(violations)
        }
        ControlReply::AssistantProposalRefused { reason } => SettingsOutcome::Refused(reason),
        ControlReply::AssistantProposalStatus { status } => SettingsOutcome::Status(status),
        other => {
            return Err(io::Error::other(format!(
                "the launcher sent an unexpected reply: {other:?}"
            )))
        }
    })
}

/// The assistant's settings in force.
pub fn current(control_socket: &Path) -> io::Result<SettingsOutcome> {
    outcome(ask(
        control_socket,
        &ControlRequest::InspectAssistantSettings,
    )?)
}

/// Proposes `settings`; the person decides.
pub fn propose(control_socket: &Path, settings: AssistantSettings) -> io::Result<SettingsOutcome> {
    outcome(ask(
        control_socket,
        &ControlRequest::ProposeAssistantSettings { settings },
    )?)
}

pub fn status(control_socket: &Path, id: u64) -> io::Result<SettingsOutcome> {
    outcome(ask(
        control_socket,
        &ControlRequest::AssistantProposalStatus { id },
    )?)
}

/// What the launcher and its supervised processes are doing right now, as plain
/// lines. Read-only, and free of settings values, page data, and secrets.
pub fn launcher_status(control_socket: &Path) -> io::Result<String> {
    match ask(control_socket, &ControlRequest::Status)? {
        ControlReply::Status(status) => Ok(format_status(&status)),
        other => Err(io::Error::other(format!(
            "the launcher sent an unexpected reply: {other:?}"
        ))),
    }
}

fn format_status(status: &blueice_launcher::control::LauncherStatus) -> String {
    let pid = |pid: Option<u32>| pid.map_or("not running".to_string(), |pid| pid.to_string());
    let mut lines = vec![
        format!("launcher pid: {}", status.launcher_pid),
        format!(
            "core: pid {}, generation {}",
            pid(status.core_pid),
            status.core_generation
        ),
    ];
    match &status.assistant {
        None => lines.push("assistant: not supervised by this launcher".to_string()),
        Some(assistant) => lines.push(format!(
            "assistant: backend {}, pid {}, started {} time(s)",
            assistant.backend,
            pid(assistant.resident_pid),
            assistant.spawn_count
        )),
    }
    lines.push(match &status.pending_proposal {
        None => "settings proposal: none waiting".to_string(),
        Some(pending) => format!(
            "settings proposal: #{} waiting for the person ({}s left)",
            pending.id, pending.seconds_left
        ),
    });
    lines.join("\n")
}

/// The proposal an agent writes, in plain fields a tool schema can describe.
#[derive(Debug, Clone, serde::Deserialize, schemars::JsonSchema)]
pub struct SettingsParams {
    /// `none`, `loopback`, `candle`, or `both`.
    pub backend: String,
    /// Required for `loopback` and `both`.
    pub loopback: Option<LoopbackParams>,
    /// Required for `candle` and `both`.
    pub candle: Option<CandleParams>,
    /// Seconds with no use before the assistant may be stopped (30 to 86400).
    pub idle_timeout_secs: u64,
    /// Memory ceiling in MiB, or omit for none. An existing ceiling cannot be
    /// removed or more than doubled by a proposal.
    pub max_resident_mb: Option<u64>,
    /// Scheduling niceness, 5 to 19 (a proposal cannot ask for a higher priority).
    pub nice: i32,
}

#[derive(Debug, Clone, serde::Deserialize, schemars::JsonSchema)]
pub struct LoopbackParams {
    /// `ollama`, `huggingface`, or `llamacpp`.
    pub provider: String,
    /// Must be `http://127.0.0.1:<port>/v1/` or `http://[::1]:<port>/v1/`.
    pub base_url: String,
    pub model: String,
}

#[derive(Debug, Clone, serde::Deserialize, schemars::JsonSchema)]
pub struct CandleParams {
    /// Absolute path to a `.gguf` file inside the configured model directory or
    /// BlueIce's models directory.
    pub model_path: String,
    /// Absolute path to a `.json` tokenizer in the same places.
    pub tokenizer_path: String,
    /// Context window in tokens (64 to 131072).
    pub context: usize,
}

impl SettingsParams {
    /// Converts to real settings. An unknown backend word is an error here, not
    /// a silent default.
    pub fn into_settings(self) -> Result<AssistantSettings, String> {
        let backend = match self.backend.as_str() {
            "none" => BackendKind::None,
            "loopback" => BackendKind::Loopback,
            "candle" => BackendKind::Candle,
            "both" => BackendKind::Both,
            other => {
                return Err(format!(
                    "backend must be none, loopback, candle, or both, not {other:?}"
                ))
            }
        };
        Ok(AssistantSettings {
            version: SETTINGS_VERSION,
            backend,
            loopback: self.loopback.map(|l| LoopbackSettings {
                provider: l.provider,
                base_url: l.base_url,
                model: l.model,
            }),
            candle: self.candle.map(|c| CandleSettings {
                model_path: c.model_path,
                tokenizer_path: c.tokenizer_path,
                context: c.context,
            }),
            idle_timeout_secs: self.idle_timeout_secs,
            max_resident_mb: self.max_resident_mb,
            nice: self.nice,
        })
    }
}

/// The words a tool result gives the agent (and, through it, the person).
pub fn describe(outcome: &SettingsOutcome) -> String {
    match outcome {
        SettingsOutcome::InForce(settings) => {
            serde_json::to_string_pretty(settings).unwrap_or_else(|_| "{}".to_string())
        }
        SettingsOutcome::Accepted { id, diff, .. } => format!(
            "Proposal {id} passed the safety rules and is now waiting for the person to approve it in BlueIce's trusted window. NOTHING HAS CHANGED YET, and you cannot approve it yourself. If it is approved it will change:\n{}",
            diff.join("\n")
        ),
        SettingsOutcome::Blocked(violations) => format!(
            "The proposal was blocked by the safety rules and the person was not asked. Fix these and propose again:\n- {}",
            violations.join("\n- ")
        ),
        SettingsOutcome::Refused(reason) => format!("The proposal was not considered: {reason}."),
        SettingsOutcome::Status(status) => format!("The proposal is {status}."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use blueice_launcher::control::{read_control_request, write_control_reply};
    use std::os::unix::net::UnixListener;
    use std::thread;

    /// A fake launcher control socket answering one request with `reply` and
    /// reporting what it was asked.
    fn fake_launcher(
        reply: ControlReply,
    ) -> (std::path::PathBuf, thread::JoinHandle<ControlRequest>) {
        let path = std::env::temp_dir().join(format!(
            "mcp-ctl-{}-{}.sock",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let listener = UnixListener::bind(&path).unwrap();
        let worker = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let request = read_control_request(&mut stream).unwrap();
            write_control_reply(&mut stream, &reply).unwrap();
            request
        });
        (path, worker)
    }

    fn params(backend: &str) -> SettingsParams {
        SettingsParams {
            backend: backend.into(),
            loopback: None,
            candle: None,
            idle_timeout_secs: 600,
            max_resident_mb: Some(2048),
            nice: 10,
        }
    }

    #[test]
    fn a_proposal_is_sent_as_a_propose_request_and_the_acceptance_is_returned() {
        let (path, worker) = fake_launcher(ControlReply::AssistantProposalAccepted {
            id: 4,
            digest: "d".into(),
            diff: vec!["Priority (nice): 10 -> 12".into()],
        });
        let settings = params("none").into_settings().unwrap();
        let out = propose(&path, settings.clone()).unwrap();
        assert_eq!(
            out,
            SettingsOutcome::Accepted {
                id: 4,
                digest: "d".into(),
                diff: vec!["Priority (nice): 10 -> 12".into()]
            }
        );
        assert_eq!(
            worker.join().unwrap(),
            ControlRequest::ProposeAssistantSettings { settings }
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn reading_and_status_use_the_read_only_requests() {
        let (path, worker) = fake_launcher(ControlReply::AssistantSettingsInForce {
            settings: Box::default(),
        });
        assert_eq!(
            current(&path).unwrap(),
            SettingsOutcome::InForce(Box::default())
        );
        assert_eq!(
            worker.join().unwrap(),
            ControlRequest::InspectAssistantSettings
        );
        let (path2, worker2) = fake_launcher(ControlReply::AssistantProposalStatus {
            status: "pending".into(),
        });
        assert_eq!(
            status(&path2, 9).unwrap(),
            SettingsOutcome::Status("pending".into())
        );
        assert_eq!(
            worker2.join().unwrap(),
            ControlRequest::AssistantProposalStatus { id: 9 }
        );
    }

    #[test]
    fn every_reply_kind_maps_to_an_outcome_and_an_unrelated_reply_is_an_error() {
        let (path, _w) = fake_launcher(ControlReply::AssistantProposalBlocked {
            violations: vec!["v".into()],
        });
        assert_eq!(
            propose(&path, AssistantSettings::default()).unwrap(),
            SettingsOutcome::Blocked(vec!["v".into()])
        );
        let (path, _w) =
            fake_launcher(ControlReply::AssistantProposalRefused { reason: "r".into() });
        assert_eq!(
            propose(&path, AssistantSettings::default()).unwrap(),
            SettingsOutcome::Refused("r".into())
        );
        let (path, _w) = fake_launcher(ControlReply::CutoverBusy);
        assert!(propose(&path, AssistantSettings::default()).is_err());
    }

    #[test]
    fn a_missing_launcher_is_an_error_not_a_hang() {
        let missing = std::env::temp_dir().join("no-such-launcher.sock");
        assert!(current(&missing).is_err());
    }

    #[test]
    fn plain_fields_convert_to_settings_and_an_unknown_backend_word_is_refused() {
        let both = SettingsParams {
            backend: "both".into(),
            loopback: Some(LoopbackParams {
                provider: "llamacpp".into(),
                base_url: "http://127.0.0.1:8080/v1/".into(),
                model: "m".into(),
            }),
            candle: Some(CandleParams {
                model_path: "/m/a.gguf".into(),
                tokenizer_path: "/m/t.json".into(),
                context: 4096,
            }),
            ..params("both")
        };
        let settings = both.into_settings().unwrap();
        assert_eq!(settings.backend, BackendKind::Both);
        assert_eq!(settings.loopback.unwrap().model, "m");
        assert_eq!(settings.candle.unwrap().context, 4096);
        for word in ["none", "loopback", "candle"] {
            assert!(params(word).into_settings().is_ok(), "{word}");
        }
        assert!(params("quantum")
            .into_settings()
            .unwrap_err()
            .contains("quantum"));
    }

    #[test]
    fn the_words_tell_the_agent_it_cannot_approve_and_nothing_has_changed() {
        let accepted = describe(&SettingsOutcome::Accepted {
            id: 2,
            digest: "d".into(),
            diff: vec!["a".into(), "b".into()],
        });
        assert!(accepted.contains("NOTHING HAS CHANGED"));
        assert!(accepted.contains("cannot approve it yourself"));
        assert!(accepted.contains("a\nb"));
        let blocked = describe(&SettingsOutcome::Blocked(vec!["one".into(), "two".into()]));
        assert!(blocked.contains("person was not asked"));
        assert!(blocked.contains("- one\n- two"));
        assert!(describe(&SettingsOutcome::Refused("busy".into())).contains("busy"));
        assert!(describe(&SettingsOutcome::Status("denied".into())).contains("denied"));
        assert!(describe(&SettingsOutcome::InForce(Box::default()))
        .contains("idle_timeout_secs"));
    }
}
