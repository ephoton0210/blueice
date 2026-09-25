// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Private permission messages between a launcher and a native frontend
//! process **spawned by that launcher**. These types are not part of the
//! rendezvous or operator-control socket protocols. The launcher accepts
//! them only from pipe handles retained for its own trusted window and
//! revalidates every mutating target against the active core generation.
//! In particular, an ordinary `--launcher` frontend and every MCP client
//! remain unable to send a grant or revoke request.

use crate::control::InstalledExtensionPermissions;
use blueice_assistant_settings::AssistantSettings;
use serde::{Deserialize, Serialize};
use std::io::{self, Read, Write};

const MAX_FRAME_BYTES: usize = 128 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionAction {
    Grant,
    Revoke,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum TrustedWindowRequest {
    /// Read the current core generation and its live installed permissions.
    Inspect,
    /// A native chrome decision for the exact package/core the person saw.
    /// The launcher must compare both expected fields under its active-core
    /// lock before asking core's private pipe to make a change. Package hash
    /// alone is insufficient because a cutover resets process-local grants.
    Change {
        expected_core_generation: u64,
        expected_extension_id: String,
        capability: String,
        action: PermissionAction,
    },
    /// First native review step. The launcher obtains the live document
    /// identity from its private core-parent pipe, not the shared client IPC.
    InspectEphemeral {
        expected_core_generation: u64,
        expected_extension_id: String,
        capability: String,
        tab_id: u64,
    },
    /// Second, distinct native confirmation. Core rejects a changed document
    /// epoch before arming, and the launcher never returns the bearer ticket.
    ArmEphemeral {
        expected_core_generation: u64,
        expected_extension_id: String,
        capability: String,
        tab_id: u64,
        document_epoch: u64,
    },
    /// Read the assistant's settings in force and any proposal an agent has
    /// made that is waiting for this person's decision
    /// (`phase-7-local-ai/PLAN.md`, R6).
    InspectAssistantSettings,
    /// The person approves the proposal they were shown. It names the id *and*
    /// the digest of the settings displayed, so a different or swapped proposal
    /// cannot ride this approval.
    ApproveAssistantProposal { id: u64, digest: String },
    DenyAssistantProposal { id: u64 },
    /// The person edits the settings directly. The rule-base that screens an
    /// agent's proposals does not apply: only the validator does.
    EditAssistantSettings { settings: AssistantSettings },
}

/// An agent's proposal, as shown to the person for a decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingAssistantProposal {
    pub id: u64,
    /// What an approval must name.
    pub digest: String,
    /// The `label: before -> after` lines, computed when the proposal was made.
    pub diff: Vec<String>,
    pub proposed: AssistantSettings,
    pub seconds_left: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum TrustedWindowReply {
    /// The assistant's settings in force and the proposal awaiting a decision.
    /// Also the reply to every successful assistant request, so a frontend
    /// shows what is true now rather than inferring it.
    AssistantSettingsState {
        // Boxed: these carry whole settings, far larger than the other replies.
        current: Box<AssistantSettings>,
        pending: Option<Box<PendingAssistantProposal>>,
    },
    /// Also returned after a successful change, with the newly inspected
    /// grant state. A frontend must not infer success from sending a request.
    State {
        core_generation: u64,
        installed: Option<InstalledExtensionPermissions>,
    },
    EphemeralReview {
        core_generation: u64,
        installed: InstalledExtensionPermissions,
        capability: String,
        tab_id: u64,
        document_epoch: u64,
        url: String,
    },
    /// Acknowledges only that core armed an internal one-shot event. The
    /// opaque bearer ticket is deliberately absent from the native pipe.
    EphemeralArmed {
        core_generation: u64,
        installed: InstalledExtensionPermissions,
        capability: String,
        tab_id: u64,
        document_epoch: u64,
    },
    Rejected { reason: String },
}

/// Rejects stale or misaddressed native-window decisions before they can
/// reach the parent-to-core permission pipe. This is necessary but not
/// sufficient: its caller must *also* be an authenticated launcher-owned
/// window, never an operator or frontend/MCP socket peer.
pub(crate) fn validate_change_target(
    current_generation: u64,
    installed: Option<&InstalledExtensionPermissions>,
    expected_generation: u64,
    expected_extension_id: &str,
    capability: &str,
) -> Result<(), &'static str> {
    if expected_generation != current_generation {
        return Err("the core changed after this permission was inspected");
    }
    let Some(installed) = installed else {
        return Err("there is no installed extension in the active core");
    };
    if installed.extension_id != expected_extension_id {
        return Err("the installed extension changed after this permission was inspected");
    }
    if !installed.optional.iter().any(|entry| entry.capability == capability) {
        return Err("the capability is not an installed optional declaration");
    }
    Ok(())
}

pub(crate) fn validate_ephemeral_target(
    current_generation: u64,
    installed: Option<&InstalledExtensionPermissions>,
    expected_generation: u64,
    expected_extension_id: &str,
    capability: &str,
    tab_id: u64,
) -> Result<(), &'static str> {
    if expected_generation != current_generation {
        return Err("the core changed after this one-shot permission was inspected");
    }
    let Some(installed) = installed else {
        return Err("there is no installed extension in the active core");
    };
    if installed.extension_id != expected_extension_id {
        return Err("the installed extension changed after this one-shot permission was inspected");
    }
    if tab_id == 0 || capability != "dom:read"
        || !installed.runtime_ephemeral.iter().any(|entry| entry.capability == capability)
    {
        return Err("the target is not a supported installed one-shot declaration");
    }
    Ok(())
}

fn write_frame<W: Write, T: Serialize>(writer: &mut W, value: &T) -> io::Result<()> {
    let bytes = serde_json::to_vec(value).map_err(io::Error::other)?;
    if bytes.is_empty() || bytes.len() > MAX_FRAME_BYTES {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "trusted-window frame is oversized"));
    }
    writer.write_all(&(bytes.len() as u32).to_le_bytes())?;
    writer.write_all(&bytes)?;
    writer.flush()
}

fn read_frame<R: Read, T: for<'de> Deserialize<'de>>(reader: &mut R) -> io::Result<Option<T>> {
    let mut first = [0_u8; 1];
    match reader.read_exact(&mut first) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(error) => return Err(error),
    }
    let mut rest = [0_u8; 3];
    reader.read_exact(&mut rest)?;
    let len = u32::from_le_bytes([first[0], rest[0], rest[1], rest[2]]) as usize;
    if len == 0 || len > MAX_FRAME_BYTES {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "trusted-window frame has an invalid length"));
    }
    let mut bytes = vec![0_u8; len];
    reader.read_exact(&mut bytes)?;
    serde_json::from_slice(&bytes).map(Some).map_err(io::Error::other)
}

pub fn write_request<W: Write>(writer: &mut W, request: &TrustedWindowRequest) -> io::Result<()> {
    write_frame(writer, request)
}

pub fn read_request<R: Read>(reader: &mut R) -> io::Result<Option<TrustedWindowRequest>> {
    read_frame(reader)
}

pub fn write_reply<W: Write>(writer: &mut W, reply: &TrustedWindowReply) -> io::Result<()> {
    write_frame(writer, reply)
}

pub fn read_reply<R: Read>(reader: &mut R) -> io::Result<Option<TrustedWindowReply>> {
    read_frame(reader)
}

#[cfg(test)]
mod tests {
    use super::*;
    use blueice_ipc::permission_control::{EphemeralCapabilityInfo, OptionalCapabilityInfo};

    fn installed() -> InstalledExtensionPermissions {
        InstalledExtensionPermissions {
            extension_id: "sha256:package-a".into(),
            name: "Notes".into(),
            version: "1".into(),
            optional: vec![OptionalCapabilityInfo {
                capability: "storage".into(),
                granted: false,
                origins: vec![],
            }],
            runtime_ephemeral: vec![EphemeralCapabilityInfo {
                capability: "dom:read".into(),
                origins: vec!["https://example.test".into()],
            }],
        }
    }

    #[test]
    fn private_protocol_round_trips_inspection_and_exactly_scoped_decisions() {
        for request in [
            TrustedWindowRequest::Inspect,
            TrustedWindowRequest::Change {
                expected_core_generation: 4,
                expected_extension_id: "sha256:package-a".into(),
                capability: "storage".into(),
                action: PermissionAction::Grant,
            },
            TrustedWindowRequest::Change {
                expected_core_generation: 4,
                expected_extension_id: "sha256:package-a".into(),
                capability: "storage".into(),
                action: PermissionAction::Revoke,
            },
            TrustedWindowRequest::InspectEphemeral {
                expected_core_generation: 4,
                expected_extension_id: "sha256:package-a".into(),
                capability: "dom:read".into(), tab_id: 7,
            },
            TrustedWindowRequest::ArmEphemeral {
                expected_core_generation: 4,
                expected_extension_id: "sha256:package-a".into(),
                capability: "dom:read".into(), tab_id: 7, document_epoch: 12,
            },
        ] {
            let mut wire = Vec::new();
            write_request(&mut wire, &request).unwrap();
            assert_eq!(read_request(&mut wire.as_slice()).unwrap(), Some(request));
        }
        let reply = TrustedWindowReply::State {
            core_generation: 4,
            installed: Some(installed()),
        };
        let mut wire = Vec::new();
        write_reply(&mut wire, &reply).unwrap();
        assert_eq!(read_reply(&mut wire.as_slice()).unwrap(), Some(reply));
        for reply in [
            TrustedWindowReply::EphemeralReview {
                core_generation: 4, installed: installed(),
                capability: "dom:read".into(), tab_id: 7, document_epoch: 12,
                url: "https://example.test/page".into(),
            },
            TrustedWindowReply::EphemeralArmed {
                core_generation: 4, installed: installed(),
                capability: "dom:read".into(), tab_id: 7, document_epoch: 12,
            },
        ] {
            let mut wire = Vec::new();
            write_reply(&mut wire, &reply).unwrap();
            assert_eq!(read_reply(&mut wire.as_slice()).unwrap(), Some(reply));
            let wire_text = String::from_utf8_lossy(&wire);
            assert!(!wire_text.contains("ticket"), "the native reply must not expose a bearer");
        }
    }

    #[test]
    fn stale_generation_wrong_package_and_non_optional_capability_are_rejected() {
        let package = installed();
        assert_eq!(validate_change_target(4, Some(&package), 4, &package.extension_id, "storage"), Ok(()));
        assert!(validate_change_target(5, Some(&package), 4, &package.extension_id, "storage").is_err());
        assert!(validate_change_target(4, Some(&package), 4, "sha256:other", "storage").is_err());
        assert!(validate_change_target(4, Some(&package), 4, &package.extension_id, "ui:inject").is_err());
        assert!(validate_change_target(4, None, 4, &package.extension_id, "storage").is_err());
        assert_eq!(validate_ephemeral_target(4, Some(&package), 4,
            &package.extension_id, "dom:read", 7), Ok(()));
        assert!(validate_ephemeral_target(5, Some(&package), 4,
            &package.extension_id, "dom:read", 7).is_err());
        assert!(validate_ephemeral_target(4, Some(&package), 4,
            "sha256:other", "dom:read", 7).is_err());
        assert!(validate_ephemeral_target(4, Some(&package), 4,
            &package.extension_id, "storage", 7).is_err());
        assert!(validate_ephemeral_target(4, Some(&package), 4,
            &package.extension_id, "dom:read", 0).is_err());
    }

    #[test]
    fn malformed_or_oversized_frames_never_decode_as_a_decision() {
        assert_eq!(read_request(&mut [].as_slice()).unwrap(), None);
        let mut oversized = ((MAX_FRAME_BYTES + 1) as u32).to_le_bytes().to_vec();
        assert_eq!(read_request(&mut oversized.as_slice()).unwrap_err().kind(), io::ErrorKind::InvalidData);
        oversized = vec![5, 0, 0, 0, b'{'];
        assert_eq!(read_request(&mut oversized.as_slice()).unwrap_err().kind(), io::ErrorKind::UnexpectedEof);
        assert!(write_request(&mut Vec::new(), &TrustedWindowRequest::Change {
            expected_core_generation: 0,
            expected_extension_id: "sha256:a".into(),
            capability: "x".repeat(MAX_FRAME_BYTES),
            action: PermissionAction::Grant,
        }).is_err());
    }
}
