// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Private parent-to-core control protocol for installed optional extension
//! permissions. This is **not** a `ClientMessage` and must never be accepted
//! on the launcher rendezvous socket: an MCP client can send those messages.
//! A launcher-owned pipe is the intended transport; possession of its write
//! end is the authority, not an extension-supplied identity or page click.

use serde::{Deserialize, Serialize};
use std::io::{self, Read, Write};

const MAX_PERMISSION_FRAME_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum PermissionControlRequest {
    Inspect,
    /// Read-only live tab identity for binding a future one-shot gesture.
    /// This does not grant any capability and is unavailable on public IPC.
    InspectDocument { tab_id: u64 },
    /// Core-parent-only one-operation lease request. Core rechecks the live
    /// tab epoch and the installed runtime-ephemeral declaration itself.
    ArmEphemeral { capability: String, tab_id: u64, document_epoch: u64 },
    Grant { capability: String },
    Revoke { capability: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OptionalCapabilityInfo {
    pub capability: String,
    pub granted: bool,
    /// Empty means the validated manifest did not constrain this capability
    /// to specific origins. These are display data, never client authority.
    pub origins: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum PermissionControlReply {
    State {
        extension_id: String,
        name: String,
        version: String,
        optional: Vec<OptionalCapabilityInfo>,
    },
    Document {
        tab_id: u64,
        document_epoch: u64,
        url: Option<String>,
    },
    EphemeralArmed {
        capability: String,
        tab_id: u64,
        document_epoch: u64,
        ticket: String,
    },
    Updated {
        capability: String,
        granted: bool,
        changed: bool,
    },
    Rejected {
        reason: String,
    },
}

fn write_frame<W: Write, T: Serialize>(writer: &mut W, value: &T) -> io::Result<()> {
    let bytes = serde_json::to_vec(value).map_err(io::Error::other)?;
    if bytes.is_empty() || bytes.len() > MAX_PERMISSION_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "permission control frame is oversized",
        ));
    }
    writer.write_all(&(bytes.len() as u32).to_le_bytes())?;
    writer.write_all(&bytes)?;
    writer.flush()
}

fn read_frame<R: Read>(reader: &mut R) -> io::Result<Option<Vec<u8>>> {
    let mut first = [0_u8; 1];
    match reader.read_exact(&mut first) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(error) => return Err(error),
    }
    let mut remaining = [0_u8; 3];
    reader.read_exact(&mut remaining)?;
    let len = u32::from_le_bytes([first[0], remaining[0], remaining[1], remaining[2]]) as usize;
    if len == 0 || len > MAX_PERMISSION_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "permission control frame has an invalid length",
        ));
    }
    let mut bytes = vec![0_u8; len];
    reader.read_exact(&mut bytes)?;
    Ok(Some(bytes))
}

pub fn write_permission_control_request<W: Write>(
    writer: &mut W,
    request: &PermissionControlRequest,
) -> io::Result<()> {
    write_frame(writer, request)
}

pub fn read_permission_control_request<R: Read>(
    reader: &mut R,
) -> io::Result<Option<PermissionControlRequest>> {
    read_frame(reader)?
        .map(|bytes| serde_json::from_slice(&bytes).map_err(io::Error::other))
        .transpose()
}

pub fn write_permission_control_reply<W: Write>(
    writer: &mut W,
    reply: &PermissionControlReply,
) -> io::Result<()> {
    write_frame(writer, reply)
}

pub fn read_permission_control_reply<R: Read>(
    reader: &mut R,
) -> io::Result<PermissionControlReply> {
    let bytes = read_frame(reader)?.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "permission control reply ended",
        )
    })?;
    serde_json::from_slice(&bytes).map_err(io::Error::other)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_permission_messages_round_trip_with_bounded_frames() {
        for request in [
            PermissionControlRequest::Inspect,
            PermissionControlRequest::InspectDocument { tab_id: 7 },
            PermissionControlRequest::ArmEphemeral {
                capability: "dom:read".into(), tab_id: 7, document_epoch: 12,
            },
            PermissionControlRequest::Grant {
                capability: "dom:read".into(),
            },
            PermissionControlRequest::Revoke {
                capability: "network:intercept".into(),
            },
        ] {
            let mut wire = Vec::new();
            write_permission_control_request(&mut wire, &request).unwrap();
            let mut reader = wire.as_slice();
            assert_eq!(
                read_permission_control_request(&mut reader).unwrap(),
                Some(request)
            );
            assert_eq!(read_permission_control_request(&mut reader).unwrap(), None);
        }
        let reply = PermissionControlReply::State {
            extension_id: "sha256:abc".into(),
            name: "Notes".into(),
            version: "1".into(),
            optional: vec![OptionalCapabilityInfo {
                capability: "dom:read".into(),
                granted: false,
                origins: vec!["https://example.test".into()],
            }],
        };
        let mut wire = Vec::new();
        write_permission_control_reply(&mut wire, &reply).unwrap();
        assert_eq!(
            read_permission_control_reply(&mut wire.as_slice()).unwrap(),
            reply
        );
        let document = PermissionControlReply::Document {
            tab_id: 7,
            document_epoch: 12,
            url: Some("https://example.test/page".into()),
        };
        let mut wire = Vec::new();
        write_permission_control_reply(&mut wire, &document).unwrap();
        assert_eq!(read_permission_control_reply(&mut wire.as_slice()).unwrap(), document);
        let armed = PermissionControlReply::EphemeralArmed {
            capability: "dom:read".into(), tab_id: 7, document_epoch: 12,
            ticket: "0123456789abcdef".repeat(4),
        };
        let mut wire = Vec::new();
        write_permission_control_reply(&mut wire, &armed).unwrap();
        assert_eq!(read_permission_control_reply(&mut wire.as_slice()).unwrap(), armed);
    }

    #[test]
    fn rejects_oversized_or_truncated_frames_before_parsing() {
        let mut oversized = ((MAX_PERMISSION_FRAME_BYTES + 1) as u32)
            .to_le_bytes()
            .to_vec();
        assert_eq!(
            read_permission_control_request(&mut oversized.as_slice())
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
        oversized = vec![5, 0, 0, 0, b'{'];
        assert_eq!(
            read_permission_control_request(&mut oversized.as_slice())
                .unwrap_err()
                .kind(),
            io::ErrorKind::UnexpectedEof
        );
        assert!(write_permission_control_request(
            &mut Vec::new(),
            &PermissionControlRequest::Grant {
                capability: "x".repeat(MAX_PERMISSION_FRAME_BYTES)
            },
        )
        .is_err());
    }
}
