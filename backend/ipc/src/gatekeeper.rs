// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The internal wire protocol between `core` and `ai-gatekeeper`
//! (`phase-7-local-ai/PLAN.md`'s "Wiring design" section) -- distinct
//! from, and never spoken by, an external [`crate::ClientMessage`]/
//! [`crate::ServerMessage`] client. Lives inside `blueice-ipc` (rather
//! than a separate crate) purely to reuse this crate's private
//! length-prefixed-JSON framing primitives ([`crate::write_framed`]/
//! [`crate::read_frame_bytes`]) without changing their visibility --
//! `gatekeeper` is a descendant module of the crate root the same way
//! every other module here is, so those `fn`s (private to their
//! defining module and its descendants) are already reachable from
//! this file with no `pub(crate)` promotion needed anywhere.
//!
//! Deliberately **no handshake**, unlike the client-facing protocol's
//! `Hello`/`protocol_version` negotiation: a gatekeeper check is one
//! request, one reply, over one short-lived connection (connect ->
//! request -> reply -> disconnect -- see `blueice-engine`'s own
//! `gatekeeper_client::check_and_fetch`), built and versioned in
//! lockstep with `core` itself. There's no independent third-party
//! client of this protocol to stay forward-compatible with, so none of
//! [`crate::ClientMessage`]/[`crate::ServerMessage`]'s
//! `#[serde(other)]` fail-soft machinery is warranted here.

use serde::{Deserialize, Serialize};
use std::io::{self, Read, Write};
use std::path::PathBuf;

/// One gatekeeper review request -- a caller (`core` for navigation, the
/// downloads process for transfers) sends exactly one of these per
/// short-lived connection to `ai-gatekeeper`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum GatekeeperRequest {
    /// The URL stage, sent before any network fetch: catches known-bad
    /// domains cheaply, before spending a fetch on them.
    CheckUrl { url: String },
    /// The content stage, sent after fetch but before parse/cascade/
    /// layout -- the stage that actually addresses this phase's named
    /// primary threat (hidden/adversarial content aimed at an AI
    /// reader), which URL blocklisting alone can't catch.
    CheckContent { url: String, html: String },
    /// The download stage (`phase-10-download-manager/PLAN.md`'s
    /// "Gatekeeper integration"): sent by the downloads process after
    /// it has probed a URL that already passed [`Self::CheckUrl`] and
    /// before a single byte is transferred. The probe is what makes the
    /// file's name, type, and size known, so this is the stage that can
    /// act on "downloading an executable or otherwise dangerous file
    /// type" (`phase-7-local-ai/PLAN.md`'s risk taxonomy). Both hints
    /// are optional because a server may not send them (a chunked
    /// response with no `Content-Type`); the review still has to happen.
    CheckDownload {
        url: String,
        file_name: String,
        content_type: Option<String>,
        total_bytes: Option<u64>,
    },
    /// A high-risk action initiated by a process-isolated extension.
    /// The capability-enforcing host constructs `detail` from reviewed
    /// action metadata rather than accepting a second arbitrary detail
    /// field from the extension process. Triggers include
    /// `network:intercept` registrations, sensitive `dom:write` effects,
    /// and bounded native extension popup text. The popup text is untrusted
    /// guest input, but is validated and length-limited before review.
    CheckExtensionAction {
        extension_id: String,
        capability: String,
        detail: String,
    },
}

/// `ai-gatekeeper`'s reply to one [`GatekeeperRequest`]. Either stage
/// returning `Rejected` ends the navigation; a connection/IO failure
/// talking to the gatekeeper is treated the same way by `core`'s own
/// client (fail-closed) but is *not* itself a value of this type -- see
/// `blueice-engine`'s `gatekeeper_client` module.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum GatekeeperReply {
    Cleared,
    Rejected { reason: String, category: String },
}

/// One immutable, compiled safety rule shown to a person on the built-in
/// settings page. `mandatory` is deliberately data rather than an implication
/// of the prose: clients must be able to say exactly which baseline rules a
/// user may inspect but cannot turn off.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatekeeperRuleInfo {
    pub id: String,
    pub category: String,
    pub description: String,
    /// Shipped signatures or human-readable matching conditions, reported by the enforcing
    /// process rather than independently transcribed by the settings page.
    pub conditions: Vec<String>,
    /// How the signatures are combined. A flat conditions list alone cannot
    /// distinguish an AND from an OR, which matters for hidden-content rules.
    #[serde(default)]
    pub match_logic: String,
    /// IDs of the mandatory workflow steps at which this rule is evaluated.
    #[serde(default)]
    pub workflow_steps: Vec<String>,
    pub mandatory: bool,
}

/// A required point in the gatekeeper's enforcement workflow. This is part of
/// the user-visible policy, not a client-side hint: URL/content/download and
/// high-risk extension checks remain server-owned and mandatory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatekeeperWorkflowStep {
    pub id: String,
    pub trigger: String,
    pub description: String,
    /// What the enforcing caller does when this review rejects or cannot be
    /// completed. Reported by the service, not inferred by the settings UI.
    #[serde(default)]
    pub failure_behavior: String,
    pub mandatory: bool,
}

/// An explicitly enabled, self-operated Chat Completions reviewer. The
/// gatekeeper validates the endpoint as loopback-only before persistence;
/// this is never a cloud or API-key configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatekeeperLocalModel {
    pub provider: String,
    pub base_url: String,
    pub model: String,
}

/// Complete inspectable gatekeeper policy. The compiled rules and workflow are
/// immutable for a running release. Custom lists are additive controls;
/// local-model configuration is an explicitly optional second review layer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatekeeperSettings {
    pub ruleset_version: String,
    /// This release's deterministic layer is active; the separate local
    /// model-review layer has not been wired into the enforcement path yet.
    pub model_review_active: bool,
    #[serde(default)]
    pub local_model: Option<GatekeeperLocalModel>,
    pub baseline_rules: Vec<GatekeeperRuleInfo>,
    pub workflow: Vec<GatekeeperWorkflowStep>,
    pub custom_blocked_hosts: Vec<String>,
    pub custom_blocked_phrases: Vec<String>,
    #[serde(default)]
    pub custom_blocked_download_extensions: Vec<String>,
    #[serde(default)]
    pub custom_blocked_popup_phrases: Vec<String>,
}

/// A user-requested local policy adjustment. There is no operation to disable
/// or weaken a compiled rule or mandatory workflow step; only the optional
/// second model layer may be configured or disabled.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GatekeeperSettingsChange {
    ConfigureLocalModel { provider: String, base_url: String, model: String },
    DisableLocalModel,
    AddBlockedHost { host: String },
    RemoveBlockedHost { host: String },
    AddBlockedPhrase { phrase: String },
    RemoveBlockedPhrase { phrase: String },
    AddBlockedDownloadExtension { extension: String },
    RemoveBlockedDownloadExtension { extension: String },
    AddBlockedPopupPhrase { phrase: String },
    RemoveBlockedPopupPhrase { phrase: String },
}

/// The settings-control protocol shares the private gatekeeper socket with
/// ordinary review checks, but deliberately has separate request/reply types.
/// Existing review consumers can therefore never mistake configuration data
/// for a `Cleared` verdict.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GatekeeperSettingsRequest {
    Read,
    Update { change: GatekeeperSettingsChange },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GatekeeperSettingsReply {
    Settings(GatekeeperSettings),
    Rejected { reason: String },
}

/// Internal service-side multiplexing result. Ordinary callers should use the
/// typed review or settings helpers rather than constructing this enum.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum GatekeeperWireRequest {
    Review(GatekeeperRequest),
    Settings(GatekeeperSettingsRequest),
}

pub fn write_gatekeeper_request<W: Write>(w: &mut W, msg: &GatekeeperRequest) -> io::Result<()> {
    crate::write_framed(w, msg)
}

pub fn read_gatekeeper_request<R: Read>(r: &mut R) -> io::Result<GatekeeperRequest> {
    let buf = crate::read_frame_bytes(r)?;
    serde_json::from_slice(&buf).map_err(io::Error::other)
}

/// Reads either the long-standing review protocol or the settings-control
/// protocol. Only the gatekeeper service uses this multiplexer; callers use
/// the typed read/write helpers below and cannot accidentally send one kind of
/// request where the other is expected.
pub fn read_gatekeeper_wire_request<R: Read>(r: &mut R) -> io::Result<GatekeeperWireRequest> {
    let buf = crate::read_frame_bytes(r)?;
    if let Ok(request) = serde_json::from_slice::<GatekeeperRequest>(&buf) {
        return Ok(GatekeeperWireRequest::Review(request));
    }
    serde_json::from_slice::<GatekeeperSettingsRequest>(&buf)
        .map(GatekeeperWireRequest::Settings)
        .map_err(io::Error::other)
}

pub fn write_gatekeeper_reply<W: Write>(w: &mut W, msg: &GatekeeperReply) -> io::Result<()> {
    crate::write_framed(w, msg)
}

pub fn read_gatekeeper_reply<R: Read>(r: &mut R) -> io::Result<GatekeeperReply> {
    let buf = crate::read_frame_bytes(r)?;
    serde_json::from_slice(&buf).map_err(io::Error::other)
}

pub fn write_gatekeeper_settings_request<W: Write>(
    w: &mut W,
    msg: &GatekeeperSettingsRequest,
) -> io::Result<()> {
    crate::write_framed(w, msg)
}

pub fn read_gatekeeper_settings_request<R: Read>(
    r: &mut R,
) -> io::Result<GatekeeperSettingsRequest> {
    let buf = crate::read_frame_bytes(r)?;
    serde_json::from_slice(&buf).map_err(io::Error::other)
}

pub fn write_gatekeeper_settings_reply<W: Write>(
    w: &mut W,
    msg: &GatekeeperSettingsReply,
) -> io::Result<()> {
    crate::write_framed(w, msg)
}

pub fn read_gatekeeper_settings_reply<R: Read>(r: &mut R) -> io::Result<GatekeeperSettingsReply> {
    let buf = crate::read_frame_bytes(r)?;
    serde_json::from_slice(&buf).map_err(io::Error::other)
}

/// Where `ai-gatekeeper` listens, and where `core`'s own gatekeeper-
/// check background threads connect by default in production.
/// Mirrors `blueice-launcher`'s `default_rendezvous_socket_path`
/// location/convention (a per-user temp dir, never a single
/// system-wide path, so two different users -- or two independent
/// BlueIce sessions -- never collide) but a distinct filename: this is
/// a different process with a different socket, not the same
/// rendezvous point `core` and its external clients share. Production
/// code is the only caller that uses this default directly -- tests
/// thread an explicit socket path through instead (see
/// `blueice_engine::session::run_session`'s own `gatekeeper_socket`
/// parameter), since many gatekeeper-behavior tests need their own
/// independent fake listener running concurrently in the same test
/// binary process.
pub fn default_gatekeeper_socket_path() -> PathBuf {
    crate::local_socket::default_socket_dir().join("ai-gatekeeper.sock")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixStream;

    #[test]
    fn gatekeeper_request_round_trips_over_a_real_socket() {
        for req in [
            GatekeeperRequest::CheckUrl {
                url: "https://example.com".to_string(),
            },
            GatekeeperRequest::CheckContent {
                url: "https://example.com".to_string(),
                html: "<p>hi</p>".to_string(),
            },
            GatekeeperRequest::CheckDownload {
                url: "https://example.com/setup.exe".to_string(),
                file_name: "setup.exe".to_string(),
                content_type: Some("application/x-msdownload".to_string()),
                total_bytes: Some(1_048_576),
            },
            // The probe may not learn a type or a size (a chunked response
            // with no `Content-Type`), and the review still has to happen.
            GatekeeperRequest::CheckDownload {
                url: "https://example.com/blob".to_string(),
                file_name: "blob".to_string(),
                content_type: None,
                total_bytes: None,
            },
            GatekeeperRequest::CheckExtensionAction {
                extension_id: "minimal-slice-extension".to_string(),
                capability: "dom:write".to_string(),
                detail: "target=form-input; input_type=password".to_string(),
            },
        ] {
            let (mut a, mut b) = UnixStream::pair().unwrap();
            write_gatekeeper_request(&mut a, &req).unwrap();
            assert_eq!(read_gatekeeper_request(&mut b).unwrap(), req);
        }
    }

    #[test]
    fn gatekeeper_reply_round_trips_over_a_real_socket() {
        for reply in [
            GatekeeperReply::Cleared,
            GatekeeperReply::Rejected {
                reason: "phishing-shaped domain".to_string(),
                category: "known-bad-domain".to_string(),
            },
        ] {
            let (mut a, mut b) = UnixStream::pair().unwrap();
            write_gatekeeper_reply(&mut a, &reply).unwrap();
            assert_eq!(read_gatekeeper_reply(&mut b).unwrap(), reply);
        }
    }

    #[test]
    fn settings_protocol_round_trips_and_never_decodes_as_a_review() {
        let settings_request = GatekeeperSettingsRequest::Update {
            change: GatekeeperSettingsChange::AddBlockedHost {
                host: "tracker.example".to_string(),
            },
        };
        let (mut a, mut b) = UnixStream::pair().unwrap();
        write_gatekeeper_settings_request(&mut a, &settings_request).unwrap();
        assert_eq!(
            read_gatekeeper_wire_request(&mut b).unwrap(),
            GatekeeperWireRequest::Settings(settings_request)
        );

        let settings_reply = GatekeeperSettingsReply::Rejected {
            reason: "invalid hostname".to_string(),
        };
        let (mut a, mut b) = UnixStream::pair().unwrap();
        write_gatekeeper_settings_reply(&mut a, &settings_reply).unwrap();
        assert_eq!(
            read_gatekeeper_settings_reply(&mut b).unwrap(),
            settings_reply
        );
    }

    #[test]
    fn local_model_settings_changes_round_trip_separately_from_reviews() {
        for change in [
            GatekeeperSettingsChange::ConfigureLocalModel {
                provider: "huggingface".into(),
                base_url: "http://127.0.0.1:8080/v1/".into(),
                model: "repo/model".into(),
            },
            GatekeeperSettingsChange::DisableLocalModel,
        ] {
            let request = GatekeeperSettingsRequest::Update { change };
            let (mut a, mut b) = UnixStream::pair().unwrap();
            write_gatekeeper_settings_request(&mut a, &request).unwrap();
            assert_eq!(read_gatekeeper_wire_request(&mut b).unwrap(), GatekeeperWireRequest::Settings(request));
        }
    }

    #[test]
    fn older_settings_replies_without_a_model_configuration_remain_readable() {
        let old = r#"{"ruleset_version":"2026.09.24.1","model_review_active":false,"baseline_rules":[],"workflow":[],"custom_blocked_hosts":[],"custom_blocked_phrases":[]}"#;
        let settings: GatekeeperSettings = serde_json::from_str(old).unwrap();
        assert!(settings.local_model.is_none());
        assert!(settings.custom_blocked_download_extensions.is_empty());
        assert!(settings.custom_blocked_popup_phrases.is_empty());
    }

    #[test]
    fn multiple_requests_can_be_written_and_read_in_sequence_on_one_stream() {
        let mut buf = Vec::new();
        write_gatekeeper_request(
            &mut buf,
            &GatekeeperRequest::CheckUrl {
                url: "https://a.example".to_string(),
            },
        )
        .unwrap();
        write_gatekeeper_request(
            &mut buf,
            &GatekeeperRequest::CheckUrl {
                url: "https://b.example".to_string(),
            },
        )
        .unwrap();
        let mut cursor = std::io::Cursor::new(buf);
        assert_eq!(
            read_gatekeeper_request(&mut cursor).unwrap(),
            GatekeeperRequest::CheckUrl {
                url: "https://a.example".to_string()
            }
        );
        assert_eq!(
            read_gatekeeper_request(&mut cursor).unwrap(),
            GatekeeperRequest::CheckUrl {
                url: "https://b.example".to_string()
            }
        );
    }

    #[test]
    fn default_gatekeeper_socket_path_is_distinct_from_the_rendezvous_socket_path() {
        // Same per-user-temp-dir convention `blueice-launcher` uses for
        // its own rendezvous socket, but a different filename -- must
        // never resolve to the same path `core`'s external clients
        // connect to.
        let path = default_gatekeeper_socket_path();
        assert_eq!(path.file_name().unwrap(), "ai-gatekeeper.sock");
        assert_ne!(path.file_name().unwrap(), "core.sock");
    }

    #[test]
    fn reading_malformed_json_is_an_error_not_a_panic() {
        let mut buf = Vec::new();
        let bad_payload = b"not json";
        buf.extend_from_slice(&(bad_payload.len() as u32).to_le_bytes());
        buf.extend_from_slice(bad_payload);
        let mut cursor = std::io::Cursor::new(buf);
        assert!(read_gatekeeper_request(&mut cursor).is_err());
    }
}
