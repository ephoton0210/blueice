// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A one-line-per-event trace of what the launcher is doing, for debugging.
//!
//! Every event goes to stderr (short, so a terminal stays readable) and, if
//! `BLUEICE_TRACE=<file>` is set, is appended to that file too, which is what a
//! developer -- or Claude Code reading the file -- follows while reproducing a
//! problem. Events name *what happened* (an id, a count, an outcome), never the
//! contents of settings or pages, and never a bearer ticket or credential; the
//! text that does appear (an error reason) is stripped to one printable line, so
//! an agent-influenced string cannot forge a second trace line.

use std::fmt::Write as _;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// The longest detail kept; more is cut off with an ellipsis.
const MAX_DETAIL_CHARS: usize = 300;

/// One printable line: control characters (newlines included) become spaces, and
/// the text is bounded.
fn one_line(text: &str) -> String {
    let mut out = String::new();
    for (count, character) in text.chars().enumerate() {
        if count == MAX_DETAIL_CHARS {
            out.push('…');
            break;
        }
        out.push(if character.is_control() {
            ' '
        } else {
            character
        });
    }
    out
}

/// `[+12.345s] kind: detail`
fn format_line(elapsed: Duration, kind: &str, detail: &str) -> String {
    let mut line = String::new();
    let _ = write!(line, "[+{:.3}s] {}", elapsed.as_secs_f64(), one_line(kind));
    if !detail.is_empty() {
        let _ = write!(line, ": {}", one_line(detail));
    }
    line
}

struct Sink {
    started: Instant,
    file: Option<Mutex<std::fs::File>>,
}

fn sink() -> &'static Sink {
    static SINK: OnceLock<Sink> = OnceLock::new();
    SINK.get_or_init(|| Sink {
        started: Instant::now(),
        file: std::env::var_os("BLUEICE_TRACE")
            .map(PathBuf::from)
            .and_then(|path| OpenOptions::new().create(true).append(true).open(path).ok())
            .map(Mutex::new),
    })
}

/// Records one event. Never fails and never blocks the caller on I/O errors: a
/// debug aid must not take the launcher down.
pub fn event(kind: &str, detail: &str) {
    let sink = sink();
    let line = format_line(sink.started.elapsed(), kind, detail);
    eprintln!("blueice-launcher {line}");
    if let Some(file) = &sink.file {
        if let Ok(mut file) = file.lock() {
            let _ = writeln!(file, "{line}");
        }
    }
}

/// A short, secret-free name for a trusted-window request, for the trace.
pub fn trusted_request_name(request: &crate::trusted_window::TrustedWindowRequest) -> String {
    use crate::trusted_window::TrustedWindowRequest as R;
    match request {
        R::Inspect => "inspect".into(),
        R::Change {
            capability, action, ..
        } => format!("change {action:?} {capability}"),
        R::InspectEphemeral {
            capability, tab_id, ..
        } => format!("inspect_ephemeral {capability} tab {tab_id}"),
        R::ArmEphemeral {
            capability, tab_id, ..
        } => format!("arm_ephemeral {capability} tab {tab_id}"),
        R::InspectAssistantSettings => "inspect_assistant_settings".into(),
        R::ApproveAssistantProposal { id, .. } => format!("approve_assistant_proposal {id}"),
        R::DenyAssistantProposal { id } => format!("deny_assistant_proposal {id}"),
        R::EditAssistantSettings { .. } => "edit_assistant_settings".into(),
    }
}

/// A short, secret-free name for a trusted-window reply, for the trace.
pub fn trusted_reply_name(reply: &crate::trusted_window::TrustedWindowReply) -> String {
    use crate::trusted_window::TrustedWindowReply as R;
    match reply {
        R::State {
            core_generation,
            installed,
        } => format!(
            "state generation {core_generation} extension {}",
            if installed.is_some() {
                "installed"
            } else {
                "none"
            }
        ),
        R::EphemeralReview { tab_id, .. } => format!("ephemeral_review tab {tab_id}"),
        R::EphemeralArmed { tab_id, .. } => format!("ephemeral_armed tab {tab_id}"),
        R::AssistantSettingsState { pending, .. } => format!(
            "assistant_settings_state pending {}",
            pending
                .as_ref()
                .map_or("none".to_string(), |p| p.id.to_string())
        ),
        R::Rejected { reason } => format!("rejected: {reason}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trusted_window::{TrustedWindowReply, TrustedWindowRequest};

    #[test]
    fn a_line_carries_the_elapsed_time_kind_and_detail() {
        assert_eq!(
            format_line(Duration::from_millis(12_345), "assistant.spawn", "pid 42"),
            "[+12.345s] assistant.spawn: pid 42"
        );
        assert_eq!(
            format_line(Duration::ZERO, "started", ""),
            "[+0.000s] started"
        );
    }

    #[test]
    fn an_agent_influenced_string_cannot_forge_another_trace_line() {
        let line = format_line(
            Duration::ZERO,
            "proposal.blocked",
            "bad\n[+1.000s] approved: 7\r\u{7}x",
        );
        assert!(!line.contains('\n') && !line.contains('\r') && !line.contains('\u{7}'));
        assert_eq!(line.lines().count(), 1);
    }

    #[test]
    fn a_long_detail_is_cut_off() {
        let line = format_line(Duration::ZERO, "k", &"a".repeat(1000));
        assert!(line.chars().count() < 340, "{}", line.chars().count());
        assert!(line.ends_with('…'));
    }

    #[test]
    fn request_and_reply_names_never_include_digests_or_settings() {
        let approve = trusted_request_name(&TrustedWindowRequest::ApproveAssistantProposal {
            id: 4,
            digest: "SECRET-DIGEST".into(),
        });
        assert_eq!(approve, "approve_assistant_proposal 4");
        let edit = trusted_request_name(&TrustedWindowRequest::EditAssistantSettings {
            settings: blueice_assistant_settings::AssistantSettings::default(),
        });
        assert_eq!(edit, "edit_assistant_settings");
        assert_eq!(
            trusted_request_name(&TrustedWindowRequest::Inspect),
            "inspect"
        );
        let reply = trusted_reply_name(&TrustedWindowReply::AssistantSettingsState {
            current: Box::default(),
            pending: None,
        });
        assert_eq!(reply, "assistant_settings_state pending none");
        assert!(trusted_reply_name(&TrustedWindowReply::Rejected {
            reason: "nope".into()
        })
        .contains("nope"));
        assert!(trusted_reply_name(&TrustedWindowReply::State {
            core_generation: 3,
            installed: None
        })
        .contains("generation 3"));
    }

    #[test]
    fn recording_an_event_does_not_panic_without_a_trace_file() {
        event("test.event", "a detail");
    }
}
