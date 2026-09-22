// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Versioned IPC vocabulary for core-owned registered-project compilation.
//!
//! This is deliberately a *query* protocol. A core owner registers a closed
//! project before it exposes an adapter; no request here contains a project
//! root, source text, import map, resolver, plugin, compiler option, output
//! path, or write capability. Callers can therefore only act on opaque
//! project and generation handles minted by that owner.
//!
//! Version two exposes source-text-free identity, check, individual static
//! type/symbol queries, compiler-minted provenance hashes, and a deliberately
//! bounded subset of reifiable static-contract inspection/validation. Build
//! artifacts, project registration/update, source reads, and output
//! transactions remain separate capability-bearing operations. In particular,
//! this module is not an MCP protocol and does not grant an MCP client any
//! authority by itself.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::{self, Read, Write};

/// Independent protocol version for registered-project compiler IPC. It does
/// not share the browser frontend protocol's lifecycle.
pub const COMPILER_PROTOCOL_VERSION: u32 = 2;

/// The maximum encoded request or reply accepted by this protocol. The engine
/// adapter applies a smaller response budget before a reply reaches this
/// transport boundary; this check also rejects a malicious length prefix
/// before it causes an unbounded allocation.
pub const MAX_COMPILER_MESSAGE_BYTES: usize = 1_024 * 1_024;

/// Opaque identifier minted by the core owner when it registers a project.
/// It is an identifier, not a path or authority to create a registration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CompilerProject {
    pub id: u64,
}

impl CompilerProject {
    pub fn is_well_formed(self) -> bool {
        self.id != 0
    }
}

/// The exact compiler result generation a static query must present. A later
/// check invalidates this handle even if it has the same project ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CompilerGeneration {
    pub project: CompilerProject,
    pub sequence: u64,
}

impl CompilerGeneration {
    pub fn is_well_formed(self) -> bool {
        self.project.is_well_formed() && self.sequence != 0
    }
}

/// A bounded list from a check response. A truncated list is never evidence
/// that the omitted set is empty.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompilerModuleList {
    pub entries: Vec<String>,
    pub truncated: bool,
}

/// Stable severity vocabulary for a compiler diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CompilerDiagnosticSeverity {
    Error,
    Warning,
}

/// A source-text-free diagnostic. `module` is the core-selected source
/// identity and the range is half-open UTF-8 byte offsets; neither provides a
/// way to read source text over this protocol.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompilerDiagnostic {
    /// The stable BlueTS code such as `BTS3003`.
    pub code: String,
    pub severity: CompilerDiagnosticSeverity,
    pub module: String,
    pub start: u64,
    pub end: u64,
    /// Bounded compiler prose. The engine adapter rejects over-budget data
    /// rather than silently exposing an unbounded diagnostic.
    pub message: String,
}

/// Capped diagnostics returned by a compiler check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompilerDiagnostics {
    pub entries: Vec<CompilerDiagnostic>,
    pub truncated: bool,
}

/// Counts and fingerprints of source-text-free static metadata retained for a
/// generation. Individual types/symbols require their own generation-bound
/// query rather than an unbounded metadata dump.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompilerStaticMetadataSummary {
    pub language_version: String,
    pub compiler_options_hash: String,
    pub source_count: u32,
    pub type_count: u32,
    pub symbol_count: u32,
    pub contract_count: u32,
}

/// A registered project's non-sensitive, source-text-free description.
/// Canonical project/config/output roots intentionally do not cross IPC;
/// callers need an opaque project handle and its known entry-module identity,
/// not filesystem topology.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompilerProjectIdentity {
    pub project: CompilerProject,
    pub entry_module: String,
}

/// Bounded result of checking one already registered project. It contains no
/// emitted JavaScript, declaration text, source-map text, source text, or
/// output-write result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompilerCheck {
    pub generation: CompilerGeneration,
    pub cache_hit: bool,
    pub parsed_modules: CompilerModuleList,
    pub reused_parsed_modules: CompilerModuleList,
    pub rechecked_modules: CompilerModuleList,
    pub reused_checked_modules: CompilerModuleList,
    pub diagnostics: CompilerDiagnostics,
    pub has_errors: bool,
    pub artifact_fingerprint: Option<String>,
    pub static_metadata: Option<CompilerStaticMetadataSummary>,
}

/// A single static type, bound to the exact check/build generation that
/// produced it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompilerStaticType {
    pub generation: CompilerGeneration,
    pub id: u32,
    pub display: String,
}

/// Static symbol categories exported by the bounded compiler query. These are
/// source-level classifications, never BlueJS object or runtime-value types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CompilerSymbolKind {
    Import,
    TypeAlias,
    Interface,
    Variable,
    Function,
}

/// A single source-text-free static symbol, bound to its exact generation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompilerStaticSymbol {
    pub generation: CompilerGeneration,
    pub id: u32,
    pub name: String,
    pub kind: CompilerSymbolKind,
    pub module: String,
    pub start: u64,
    pub end: u64,
    pub static_type_id: Option<u32>,
    /// Compiler-minted source provenance handle. It can be used only with the
    /// exact generation in a `GetStaticProvenance` request.
    pub source_id: u32,
    /// Compiler-minted contract handle for a reifiable local declaration.
    /// `None` deliberately means no exact static plan was retained.
    pub contract_id: Option<u32>,
}

/// One source-text-free provenance record from an exact compilation. The
/// module identity and hash are static metadata, not a source-read endpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompilerStaticProvenance {
    pub generation: CompilerGeneration,
    pub source_id: u32,
    pub module: String,
    pub content_hash: String,
}

/// A bounded, source-text-free summary of one pure static contract plan. The
/// `root` and `definitions` are deterministic debug shapes of the retained
/// pure plan; `fingerprint` binds both for the exact generation. These strings
/// are compiler metadata, not source text, and the core response budget caps
/// each independently.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompilerStaticContract {
    pub generation: CompilerGeneration,
    pub contract_id: u32,
    pub source_id: u32,
    pub name: String,
    pub fingerprint: String,
    pub root: String,
    pub definitions: String,
    pub definition_count: u32,
}

/// The only data shapes accepted for static contract validation. It cannot
/// encode a JavaScript object, function, getter, proxy, host handle, source
/// graph, or compiler configuration. Numbers use canonical finite decimal
/// text so the JSON transport preserves a fully comparable `Eq` wire value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "kebab-case")]
pub enum CompilerContractValue {
    Null,
    Undefined,
    Boolean(bool),
    Number(String),
    String(String),
    Array(Vec<CompilerContractValue>),
    Object(BTreeMap<String, CompilerContractValue>),
}

/// A rejected static contract snapshot is a normal validation result, rather
/// than a transport failure. No request input is echoed in this result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompilerContractValidationFailure {
    pub path: String,
    pub expected: String,
    pub observed: String,
}

/// Exact-generation outcome of validation against one compiler-retained
/// static plan. It never describes a live BlueJS runtime value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompilerContractValidation {
    pub generation: CompilerGeneration,
    pub contract_id: u32,
    pub valid: bool,
    pub failure: Option<CompilerContractValidationFailure>,
}

/// Requests sent after a successful [`CompilerRequest::Hello`] handshake.
/// None can register, reconfigure, update, write, or source-read a project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompilerRequest {
    Hello {
        protocol_version: u32,
    },
    DescribeProject {
        project: CompilerProject,
    },
    Check {
        project: CompilerProject,
    },
    GetStaticType {
        generation: CompilerGeneration,
        type_id: u32,
    },
    GetStaticSymbol {
        generation: CompilerGeneration,
        symbol_id: u32,
    },
    /// Gets a compiler-minted source hash and static module identity from one
    /// exact successful generation. No source text crosses this request.
    GetStaticProvenance {
        generation: CompilerGeneration,
        source_id: u32,
    },
    /// Gets the bounded static summary for one retained reifiable contract.
    GetStaticContract {
        generation: CompilerGeneration,
        contract_id: u32,
    },
    /// Validates a caller-provided data-only snapshot against one exact static
    /// contract. Core-selected limits apply and the snapshot is never echoed.
    ValidateStaticContract {
        generation: CompilerGeneration,
        contract_id: u32,
        value: CompilerContractValue,
    },
    /// A forward-compatible unknown request fails as unsupported without
    /// changing connection framing or falling back to another operation.
    #[serde(other)]
    Unknown,
}

/// Stable machine-readable failure classes. `message` is host-controlled
/// prose; clients must branch on this code rather than error text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CompilerErrorCode {
    ProtocolVersion,
    InvalidProject,
    StaleGeneration,
    NoStaticMetadata,
    UnknownType,
    UnknownSymbol,
    UnknownSource,
    UnknownContract,
    InvalidContractValue,
    ResourceLimit,
    Unavailable,
}

/// Replies emitted by the core-owned compiler adapter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompilerReply {
    HelloAck {
        protocol_version: u32,
    },
    Project(CompilerProjectIdentity),
    Check(CompilerCheck),
    StaticType(CompilerStaticType),
    StaticSymbol(CompilerStaticSymbol),
    StaticProvenance(CompilerStaticProvenance),
    StaticContract(CompilerStaticContract),
    ContractValidation(CompilerContractValidation),
    Unsupported {
        operation: String,
        reason: String,
    },
    Error {
        code: CompilerErrorCode,
        message: String,
    },
}

/// Produces the one valid reply to a new connection's first request. The
/// transport owner must not dispatch any other request after a failed reply.
pub fn negotiate(request: &CompilerRequest) -> CompilerReply {
    match request {
        CompilerRequest::Hello { protocol_version }
            if *protocol_version == COMPILER_PROTOCOL_VERSION =>
        {
            CompilerReply::HelloAck {
                protocol_version: COMPILER_PROTOCOL_VERSION,
            }
        }
        CompilerRequest::Hello { .. } => CompilerReply::Error {
            code: CompilerErrorCode::ProtocolVersion,
            message: "unsupported compiler protocol version".to_string(),
        },
        CompilerRequest::DescribeProject { .. }
        | CompilerRequest::Check { .. }
        | CompilerRequest::GetStaticType { .. }
        | CompilerRequest::GetStaticSymbol { .. }
        | CompilerRequest::GetStaticProvenance { .. }
        | CompilerRequest::GetStaticContract { .. }
        | CompilerRequest::ValidateStaticContract { .. }
        | CompilerRequest::Unknown => CompilerReply::Error {
            code: CompilerErrorCode::ProtocolVersion,
            message: "compiler protocol requires Hello as its first request".to_string(),
        },
    }
}

pub fn write_compiler_request<W: Write>(
    writer: &mut W,
    request: &CompilerRequest,
) -> io::Result<()> {
    write_bounded(writer, request)
}

pub fn read_compiler_request<R: Read>(reader: &mut R) -> io::Result<CompilerRequest> {
    let bytes = crate::read_frame_bytes_bounded(reader, MAX_COMPILER_MESSAGE_BYTES)?;
    serde_json::from_slice(&bytes).map_err(io::Error::other)
}

pub fn write_compiler_reply<W: Write>(writer: &mut W, reply: &CompilerReply) -> io::Result<()> {
    write_bounded(writer, reply)
}

pub fn read_compiler_reply<R: Read>(reader: &mut R) -> io::Result<CompilerReply> {
    let bytes = crate::read_frame_bytes_bounded(reader, MAX_COMPILER_MESSAGE_BYTES)?;
    serde_json::from_slice(&bytes).map_err(io::Error::other)
}

fn write_bounded<W: Write, T: Serialize>(writer: &mut W, message: &T) -> io::Result<()> {
    let bytes = serde_json::to_vec(message).map_err(io::Error::other)?;
    if bytes.len() > MAX_COMPILER_MESSAGE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "compiler message exceeds protocol byte limit",
        ));
    }
    let len = u32::try_from(bytes.len()).map_err(io::Error::other)?;
    writer.write_all(&len.to_le_bytes())?;
    writer.write_all(&bytes)?;
    writer.flush()
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::net::UnixStream;

    fn project() -> CompilerProject {
        CompilerProject { id: 7 }
    }

    fn generation() -> CompilerGeneration {
        CompilerGeneration {
            project: project(),
            sequence: 3,
        }
    }

    #[test]
    fn requests_and_replies_round_trip_on_a_real_socket() {
        for request in [
            CompilerRequest::Hello {
                protocol_version: COMPILER_PROTOCOL_VERSION,
            },
            CompilerRequest::DescribeProject { project: project() },
            CompilerRequest::Check { project: project() },
            CompilerRequest::GetStaticType {
                generation: generation(),
                type_id: 2,
            },
            CompilerRequest::GetStaticSymbol {
                generation: generation(),
                symbol_id: 5,
            },
            CompilerRequest::GetStaticProvenance {
                generation: generation(),
                source_id: 7,
            },
            CompilerRequest::GetStaticContract {
                generation: generation(),
                contract_id: 8,
            },
            CompilerRequest::ValidateStaticContract {
                generation: generation(),
                contract_id: 8,
                value: CompilerContractValue::Object(BTreeMap::from([(
                    "enabled".to_string(),
                    CompilerContractValue::Boolean(true),
                )])),
            },
            CompilerRequest::Unknown,
        ] {
            let (mut sender, mut receiver) = UnixStream::pair().unwrap();
            write_compiler_request(&mut sender, &request).unwrap();
            assert_eq!(read_compiler_request(&mut receiver).unwrap(), request);
        }

        let reply = CompilerReply::Check(CompilerCheck {
            generation: generation(),
            cache_hit: false,
            parsed_modules: CompilerModuleList {
                entries: vec!["project:///app/main.ts".to_string()],
                truncated: false,
            },
            reused_parsed_modules: CompilerModuleList {
                entries: Vec::new(),
                truncated: false,
            },
            rechecked_modules: CompilerModuleList {
                entries: vec!["project:///app/main.ts".to_string()],
                truncated: false,
            },
            reused_checked_modules: CompilerModuleList {
                entries: Vec::new(),
                truncated: false,
            },
            diagnostics: CompilerDiagnostics {
                entries: Vec::new(),
                truncated: false,
            },
            has_errors: false,
            artifact_fingerprint: Some("bts-1234".to_string()),
            static_metadata: Some(CompilerStaticMetadataSummary {
                language_version: "blue-ts-v1".to_string(),
                compiler_options_hash: "bts-options-1234".to_string(),
                source_count: 1,
                type_count: 1,
                symbol_count: 1,
                contract_count: 1,
            }),
        });
        let (mut sender, mut receiver) = UnixStream::pair().unwrap();
        write_compiler_reply(&mut sender, &reply).unwrap();
        assert_eq!(read_compiler_reply(&mut receiver).unwrap(), reply);
    }

    #[test]
    fn negotiation_requires_the_exact_version_and_first_request() {
        assert_eq!(
            negotiate(&CompilerRequest::Hello {
                protocol_version: COMPILER_PROTOCOL_VERSION,
            }),
            CompilerReply::HelloAck {
                protocol_version: COMPILER_PROTOCOL_VERSION,
            }
        );
        assert!(matches!(
            negotiate(&CompilerRequest::Hello {
                protocol_version: COMPILER_PROTOCOL_VERSION + 1,
            }),
            CompilerReply::Error {
                code: CompilerErrorCode::ProtocolVersion,
                ..
            }
        ));
        assert!(matches!(
            negotiate(&CompilerRequest::Hello {
                protocol_version: 1,
            }),
            CompilerReply::Error {
                code: CompilerErrorCode::ProtocolVersion,
                ..
            }
        ));
        assert!(matches!(
            negotiate(&CompilerRequest::Check { project: project() }),
            CompilerReply::Error {
                code: CompilerErrorCode::ProtocolVersion,
                ..
            }
        ));
    }

    #[test]
    fn oversized_length_prefix_is_rejected_before_payload_allocation() {
        let mut frame = Vec::new();
        frame.extend_from_slice(&((MAX_COMPILER_MESSAGE_BYTES as u32) + 1).to_le_bytes());
        let error = read_compiler_request(&mut std::io::Cursor::new(frame)).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn oversized_reply_is_rejected_before_it_is_written() {
        let reply = CompilerReply::Unsupported {
            operation: "x".repeat(MAX_COMPILER_MESSAGE_BYTES),
            reason: "too large".to_string(),
        };
        let error = write_compiler_reply(&mut Vec::new(), &reply).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn zero_identifiers_are_not_well_formed() {
        assert!(!CompilerProject { id: 0 }.is_well_formed());
        assert!(!CompilerGeneration {
            project: project(),
            sequence: 0,
        }
        .is_well_formed());
        assert!(generation().is_well_formed());
    }
}
