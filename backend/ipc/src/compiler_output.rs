// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Separate owner-granted BlueTS output protocol. Its receipt is deliberately
//! a different shape from the query-only compiler session attestation.

use crate::compiler::{CompilerGeneration, CompilerProject, CompilerProjectInventory};
use serde::{Deserialize, Serialize};
use std::io::{self, Read, Write};

pub const COMPILER_OUTPUT_PROTOCOL_VERSION: u32 = 1;
pub const MAX_COMPILER_OUTPUT_MESSAGE_BYTES: usize = 64 * 1024;
pub const MAX_COMPILER_OUTPUT_FINGERPRINT_BYTES: usize = 4_096;

/// Per-accepted-output-stream evidence, minted only by a core output owner.
/// The `ow-` domain prevents a read-only query receipt from being accepted as
/// a syntactically valid output-write receipt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompilerOutputSessionReceipt {
    pub id: String,
}

impl CompilerOutputSessionReceipt {
    pub fn is_well_formed(&self) -> bool {
        let Some(hex) = self.id.strip_prefix("ow-") else {
            return false;
        };
        hex.len() == 64
            && hex
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    }
}

/// A request never selects source, resolver, options, artifact, or destination.
/// A build must present the exact output receipt learned on this stream and a
/// project ID from its separately granted output inventory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum CompilerOutputRequest {
    Hello {
        protocol_version: u32,
    },
    ListProjects {
        receipt: CompilerOutputSessionReceipt,
    },
    Build {
        receipt: CompilerOutputSessionReceipt,
        project: CompilerProject,
    },
    #[serde(other)]
    Unknown,
}

/// A bounded, source-free publication result. The owner keeps the physical
/// output path and emitted bytes; a diagnostic-bearing build publishes nothing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompilerOutputBuildResult {
    pub generation: CompilerGeneration,
    pub project_fingerprint: String,
    pub has_errors: bool,
    pub published: bool,
}

impl CompilerOutputBuildResult {
    pub fn is_well_formed_for_project(&self, project: CompilerProject) -> bool {
        self.generation.is_well_formed()
            && self.generation.project == project
            && !self.project_fingerprint.is_empty()
            && self.project_fingerprint.len() <= MAX_COMPILER_OUTPUT_FINGERPRINT_BYTES
            && self.published != self.has_errors
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CompilerOutputErrorCode {
    ProtocolVersion,
    InvalidReceipt,
    UnobservedProject,
    NotGranted,
    BuildFailed,
    ResourceLimit,
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompilerOutputReply {
    HelloAck {
        protocol_version: u32,
        receipt: CompilerOutputSessionReceipt,
    },
    Projects(CompilerProjectInventory),
    Build(CompilerOutputBuildResult),
    Error {
        code: CompilerOutputErrorCode,
        message: String,
    },
}

/// Only an exact first Hello and a well-formed core-minted receipt can open
/// this output protocol. Later requests must be dispatched by the owner.
pub fn negotiate(
    request: &CompilerOutputRequest,
    receipt: Option<CompilerOutputSessionReceipt>,
) -> CompilerOutputReply {
    match (request, receipt) {
        (CompilerOutputRequest::Hello { protocol_version }, Some(receipt))
            if *protocol_version == COMPILER_OUTPUT_PROTOCOL_VERSION
                && receipt.is_well_formed() =>
        {
            CompilerOutputReply::HelloAck {
                protocol_version: COMPILER_OUTPUT_PROTOCOL_VERSION,
                receipt,
            }
        }
        (CompilerOutputRequest::Hello { protocol_version }, _)
            if *protocol_version != COMPILER_OUTPUT_PROTOCOL_VERSION =>
        {
            CompilerOutputReply::Error {
                code: CompilerOutputErrorCode::ProtocolVersion,
                message: "unsupported compiler output protocol version".into(),
            }
        }
        _ => CompilerOutputReply::Error {
            code: CompilerOutputErrorCode::InvalidReceipt,
            message: "compiler output Hello requires a core-minted write receipt".into(),
        },
    }
}

pub fn write_compiler_output_request<W: Write>(
    writer: &mut W,
    request: &CompilerOutputRequest,
) -> io::Result<()> {
    write_bounded(writer, request)
}

pub fn read_compiler_output_request<R: Read>(reader: &mut R) -> io::Result<CompilerOutputRequest> {
    read_bounded(reader)
}

pub fn write_compiler_output_reply<W: Write>(
    writer: &mut W,
    reply: &CompilerOutputReply,
) -> io::Result<()> {
    write_bounded(writer, reply)
}

pub fn read_compiler_output_reply<R: Read>(reader: &mut R) -> io::Result<CompilerOutputReply> {
    read_bounded(reader)
}

fn read_bounded<R: Read, T: for<'de> Deserialize<'de>>(reader: &mut R) -> io::Result<T> {
    let bytes = crate::read_frame_bytes_bounded(reader, MAX_COMPILER_OUTPUT_MESSAGE_BYTES)?;
    serde_json::from_slice(&bytes).map_err(io::Error::other)
}

fn write_bounded<W: Write, T: Serialize>(writer: &mut W, message: &T) -> io::Result<()> {
    let bytes = serde_json::to_vec(message).map_err(io::Error::other)?;
    if bytes.len() > MAX_COMPILER_OUTPUT_MESSAGE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "compiler output message exceeds protocol byte limit",
        ));
    }
    let len = u32::try_from(bytes.len()).map_err(io::Error::other)?;
    writer.write_all(&len.to_le_bytes())?;
    writer.write_all(&bytes)?;
    writer.flush()
}

#[cfg(test)]
mod tests;
