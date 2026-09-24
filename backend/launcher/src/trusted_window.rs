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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum TrustedWindowReply {
    /// Also returned after a successful change, with the newly inspected
    /// grant state. A frontend must not infer success from sending a request.
    State {
        core_generation: u64,
        installed: Option<InstalledExtensionPermissions>,
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
    use blueice_ipc::permission_control::OptionalCapabilityInfo;

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
    }

    #[test]
    fn stale_generation_wrong_package_and_non_optional_capability_are_rejected() {
        let package = installed();
        assert_eq!(validate_change_target(4, Some(&package), 4, &package.extension_id, "storage"), Ok(()));
        assert!(validate_change_target(5, Some(&package), 4, &package.extension_id, "storage").is_err());
        assert!(validate_change_target(4, Some(&package), 4, "sha256:other", "storage").is_err());
        assert!(validate_change_target(4, Some(&package), 4, &package.extension_id, "ui:inject").is_err());
        assert!(validate_change_target(4, None, 4, &package.extension_id, "storage").is_err());
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
