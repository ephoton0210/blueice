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
//! Version five adds a core-minted, fixed query-only capability manifest to
//! the per-accepted-stream session attestation, source-text-free identity,
//! check, individual static type/symbol queries, compiler-minted provenance
//! hashes, deliberately bounded reifiable static-contract
//! inspection/validation, generation-bound pages of opaque metadata IDs, and
//! source-free one-shot pages of retained compiler diagnostics. Core also
//! binds pagination cursors to the accepted, attested compiler stream that
//! received them and releases unused cursor slots when that stream closes;
//! a client cannot carry a numeric cursor to a later stream.
//! The attestation binds an MCP-side receipt to the core that accepted its
//! relay stream; the manifest makes that receipt's exact fixed operation set
//! independently verifiable. Neither grants additional authority. Build
//! artifacts, project registration/update, source reads, and output
//! transactions remain separate capability-bearing operations. In particular,
//! this module is not an MCP protocol and does not grant an MCP client any
//! authority by itself.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::{self, Read, Write};

/// Independent protocol version for registered-project compiler IPC. It does
/// not share the browser frontend protocol's lifecycle.
pub const COMPILER_PROTOCOL_VERSION: u32 = 5;

/// The maximum encoded request or reply accepted by this protocol. The engine
/// adapter applies a smaller response budget before a reply reaches this
/// transport boundary; this check also rejects a malicious length prefix
/// before it causes an unbounded allocation.
pub const MAX_COMPILER_MESSAGE_BYTES: usize = 1_024 * 1_024;

/// Source-free opaque evidence that a particular core process accepted one
/// compiler transport stream. It identifies neither a project nor a catalog,
/// and it carries no permission beyond that stream's existing query-only
/// protocol.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompilerSessionAttestation {
    /// Exactly 32 random bytes encoded as lowercase hexadecimal.
    pub id: String,
}

impl CompilerSessionAttestation {
    /// The number of hexadecimal characters in one core-minted attestation.
    pub const ID_LENGTH: usize = 64;

    /// Reject malformed evidence before a client binds its local receipt to
    /// it. A syntactically valid value is still only meaningful on the stream
    /// whose core listener minted it.
    pub fn is_well_formed(&self) -> bool {
        self.id.len() == Self::ID_LENGTH
            && self
                .id
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    }
}

/// The identity of the sole compiler capability manifest this protocol can
/// expose. Its version is independent of the transport version so a client
/// can validate the fixed query-only operation set explicitly rather than
/// inferring authority from a protocol number.
pub const COMPILER_QUERY_CAPABILITY_MANIFEST_VERSION: u32 = 2;

/// Stable, source-free identifiers for the exact read-only compiler queries
/// available over this transport. The protocol deliberately has no variants
/// for project registration, source reads, option changes, builds, artifacts,
/// paths, or output writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CompilerQueryOperationId {
    DescribeProject,
    Check,
    ListDiagnostics,
    GetStaticType,
    GetStaticSymbol,
    ListStaticMetadata,
    GetStaticProvenance,
    GetStaticContract,
    ValidateStaticContract,
}

/// Core-authored declaration of the fixed query-only compiler surface for an
/// accepted transport stream. It is source-free and contains no project,
/// generation, path, source, resolver, option, build, artifact, or write
/// authority. Its operation order is canonical so a receiver can reject
/// subsets, supersets, duplicates, and reordered claims without guessing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompilerSessionCapabilityManifest {
    pub version: u32,
    pub operation_ids: Vec<CompilerQueryOperationId>,
}

impl CompilerSessionCapabilityManifest {
    /// Returns the only capability manifest a core compiler listener may
    /// mint. It is intentionally fixed rather than negotiated with a client.
    pub fn fixed_query_only() -> Self {
        Self {
            version: COMPILER_QUERY_CAPABILITY_MANIFEST_VERSION,
            operation_ids: Self::fixed_query_operation_ids().to_vec(),
        }
    }

    /// Checks both the manifest identity and its complete, canonical operation
    /// inventory. This is suitable for an adapter that must fail closed before
    /// presenting a core-issued receipt to another protocol.
    pub fn is_well_formed(&self) -> bool {
        self.version == COMPILER_QUERY_CAPABILITY_MANIFEST_VERSION
            && self.operation_ids.as_slice() == Self::fixed_query_operation_ids()
    }

    fn fixed_query_operation_ids() -> &'static [CompilerQueryOperationId] {
        const OPERATIONS: &[CompilerQueryOperationId] = &[
            CompilerQueryOperationId::DescribeProject,
            CompilerQueryOperationId::Check,
            CompilerQueryOperationId::ListDiagnostics,
            CompilerQueryOperationId::GetStaticType,
            CompilerQueryOperationId::GetStaticSymbol,
            CompilerQueryOperationId::ListStaticMetadata,
            CompilerQueryOperationId::GetStaticProvenance,
            CompilerQueryOperationId::GetStaticContract,
            CompilerQueryOperationId::ValidateStaticContract,
        ];
        OPERATIONS
    }
}

/// All core-authored evidence attached to an accepted compiler handshake.
/// A listener creates it only after it has accepted the exact protocol
/// `Hello`; clients have no request field through which to select either
/// component.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompilerSessionHelloEvidence {
    pub session_attestation: CompilerSessionAttestation,
    pub capability_manifest: CompilerSessionCapabilityManifest,
}

impl CompilerSessionHelloEvidence {
    /// Rejects malformed stream evidence before it becomes a `HelloAck`.
    pub fn is_well_formed(&self) -> bool {
        self.session_attestation.is_well_formed() && self.capability_manifest.is_well_formed()
    }
}

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

/// An opaque, one-shot pagination cursor minted by the core service for the
/// exact diagnostics retained by one compiler generation and the accepted
/// stream that received it. It is neither an offset nor a source position,
/// and cannot be repurposed for static metadata, a later stream, or a later
/// check generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CompilerDiagnosticCursor {
    pub id: u64,
}

impl CompilerDiagnosticCursor {
    pub fn is_well_formed(self) -> bool {
        self.id != 0
    }
}

/// One bounded page of retained compiler diagnostics. The entries include no
/// source text, path, resolver, compiler configuration, artifact, or output
/// capability. `truncated` is true only when the core's fixed retention bound
/// omitted later diagnostics; it is never a caller-selected page limit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompilerDiagnosticPage {
    pub generation: CompilerGeneration,
    pub entries: Vec<CompilerDiagnostic>,
    pub next_cursor: Option<CompilerDiagnosticCursor>,
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

/// A source-text-free metadata collection retained by a successful exact
/// generation. Entries in an inventory page are compiler-minted opaque IDs;
/// callers use the existing single-record queries to inspect one ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CompilerStaticMetadataKind {
    Sources,
    Types,
    Symbols,
    Contracts,
}

/// An opaque, one-shot pagination cursor minted by the core service. A
/// client must not construct it: the core binds it to one exact generation
/// and metadata kind, the stream that received it, consumes it once, and
/// invalidates it on a later check or stream close.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CompilerStaticMetadataCursor {
    pub id: u64,
}

impl CompilerStaticMetadataCursor {
    pub fn is_well_formed(self) -> bool {
        self.id != 0
    }
}

/// One bounded page of compiler-minted static metadata IDs. `next_cursor` is
/// absent only after the final page. It deliberately contains no source text,
/// path, resolver/configuration, artifact, or runtime-value data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompilerStaticMetadataPage {
    pub generation: CompilerGeneration,
    pub kind: CompilerStaticMetadataKind,
    pub ids: Vec<u32>,
    pub next_cursor: Option<CompilerStaticMetadataCursor>,
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
/// module identity and labeled SHA-256 digest are static metadata, not a
/// source-read endpoint.
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
    /// Lists one capped page of source-free diagnostics retained for an exact
    /// check generation. A continuation cursor is core-minted and one-shot;
    /// `None` begins a new inventory. The core clamps a positive requested
    /// limit to fixed policy and response budgets.
    ListDiagnostics {
        generation: CompilerGeneration,
        cursor: Option<CompilerDiagnosticCursor>,
        limit: Option<u32>,
    },
    GetStaticType {
        generation: CompilerGeneration,
        type_id: u32,
    },
    GetStaticSymbol {
        generation: CompilerGeneration,
        symbol_id: u32,
    },
    /// Lists one capped page of opaque static IDs from exactly one successful
    /// generation. The optional cursor is core-minted and one-shot; `None`
    /// begins a new inventory. The core clamps a positive request limit to a
    /// fixed policy cap and rejects zero or malformed values.
    ListStaticMetadata {
        generation: CompilerGeneration,
        kind: CompilerStaticMetadataKind,
        cursor: Option<CompilerStaticMetadataCursor>,
        limit: Option<u32>,
    },
    /// Gets a compiler-minted SHA-256 source digest and static module identity
    /// from one exact successful generation. No source text crosses this
    /// request.
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
    InvalidDiagnosticCursor,
    InvalidDiagnosticPage,
    InvalidMetadataCursor,
    InvalidMetadataPage,
    /// The MCP adapter has not received this exact opaque metadata ID in an
    /// inventory page for its current session and generation. This is a
    /// public-adapter boundary, not an assertion that the core has no such
    /// internal record.
    UnobservedMetadata,
    ResourceLimit,
    Unavailable,
}

/// Replies emitted by the core-owned compiler adapter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompilerReply {
    HelloAck {
        protocol_version: u32,
        session_attestation: CompilerSessionAttestation,
        capability_manifest: CompilerSessionCapabilityManifest,
    },
    Project(CompilerProjectIdentity),
    Check(CompilerCheck),
    DiagnosticPage(CompilerDiagnosticPage),
    StaticType(CompilerStaticType),
    StaticSymbol(CompilerStaticSymbol),
    StaticMetadataPage(CompilerStaticMetadataPage),
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
pub fn negotiate(
    request: &CompilerRequest,
    session_evidence: Option<CompilerSessionHelloEvidence>,
) -> CompilerReply {
    match (request, session_evidence) {
        (CompilerRequest::Hello { protocol_version }, Some(session_evidence))
            if *protocol_version == COMPILER_PROTOCOL_VERSION
                && session_evidence.is_well_formed() =>
        {
            CompilerReply::HelloAck {
                protocol_version: COMPILER_PROTOCOL_VERSION,
                session_attestation: session_evidence.session_attestation,
                capability_manifest: session_evidence.capability_manifest,
            }
        }
        (CompilerRequest::Hello { protocol_version }, _)
            if *protocol_version == COMPILER_PROTOCOL_VERSION =>
        {
            CompilerReply::Error {
                code: CompilerErrorCode::Unavailable,
                message: "compiler listener did not mint valid session evidence".to_string(),
            }
        }
        (CompilerRequest::Hello { .. }, _) => CompilerReply::Error {
            code: CompilerErrorCode::ProtocolVersion,
            message: "unsupported compiler protocol version".to_string(),
        },
        (
            CompilerRequest::DescribeProject { .. }
            | CompilerRequest::Check { .. }
            | CompilerRequest::ListDiagnostics { .. }
            | CompilerRequest::GetStaticType { .. }
            | CompilerRequest::GetStaticSymbol { .. }
            | CompilerRequest::ListStaticMetadata { .. }
            | CompilerRequest::GetStaticProvenance { .. }
            | CompilerRequest::GetStaticContract { .. }
            | CompilerRequest::ValidateStaticContract { .. }
            | CompilerRequest::Unknown,
            _,
        ) => CompilerReply::Error {
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

    fn session_attestation() -> CompilerSessionAttestation {
        CompilerSessionAttestation {
            id: "a1".repeat(32),
        }
    }

    fn session_evidence() -> CompilerSessionHelloEvidence {
        CompilerSessionHelloEvidence {
            session_attestation: session_attestation(),
            capability_manifest: CompilerSessionCapabilityManifest::fixed_query_only(),
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
            CompilerRequest::ListDiagnostics {
                generation: generation(),
                cursor: Some(CompilerDiagnosticCursor { id: 6 }),
                limit: Some(2),
            },
            CompilerRequest::GetStaticType {
                generation: generation(),
                type_id: 2,
            },
            CompilerRequest::GetStaticSymbol {
                generation: generation(),
                symbol_id: 5,
            },
            CompilerRequest::ListStaticMetadata {
                generation: generation(),
                kind: CompilerStaticMetadataKind::Symbols,
                cursor: Some(CompilerStaticMetadataCursor { id: 9 }),
                limit: Some(2),
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

        let diagnostic_page = CompilerReply::DiagnosticPage(CompilerDiagnosticPage {
            generation: generation(),
            entries: vec![CompilerDiagnostic {
                code: "BTS3003".to_string(),
                severity: CompilerDiagnosticSeverity::Error,
                module: "project:///app/main.ts".to_string(),
                start: 3,
                end: 7,
                message: "fixture diagnostic".to_string(),
            }],
            next_cursor: Some(CompilerDiagnosticCursor { id: 6 }),
            truncated: false,
        });
        let (mut sender, mut receiver) = UnixStream::pair().unwrap();
        write_compiler_reply(&mut sender, &diagnostic_page).unwrap();
        assert_eq!(read_compiler_reply(&mut receiver).unwrap(), diagnostic_page);

        let hello_ack = CompilerReply::HelloAck {
            protocol_version: COMPILER_PROTOCOL_VERSION,
            session_attestation: session_attestation(),
            capability_manifest: CompilerSessionCapabilityManifest::fixed_query_only(),
        };
        let (mut sender, mut receiver) = UnixStream::pair().unwrap();
        write_compiler_reply(&mut sender, &hello_ack).unwrap();
        assert_eq!(read_compiler_reply(&mut receiver).unwrap(), hello_ack);

        let page = CompilerReply::StaticMetadataPage(CompilerStaticMetadataPage {
            generation: generation(),
            kind: CompilerStaticMetadataKind::Symbols,
            ids: vec![0, 5],
            next_cursor: Some(CompilerStaticMetadataCursor { id: 9 }),
        });
        let (mut sender, mut receiver) = UnixStream::pair().unwrap();
        write_compiler_reply(&mut sender, &page).unwrap();
        assert_eq!(read_compiler_reply(&mut receiver).unwrap(), page);
    }

    #[test]
    fn negotiation_requires_the_exact_version_and_first_request() {
        assert_eq!(
            negotiate(
                &CompilerRequest::Hello {
                    protocol_version: COMPILER_PROTOCOL_VERSION,
                },
                Some(session_evidence()),
            ),
            CompilerReply::HelloAck {
                protocol_version: COMPILER_PROTOCOL_VERSION,
                session_attestation: session_attestation(),
                capability_manifest: CompilerSessionCapabilityManifest::fixed_query_only(),
            }
        );
        assert!(matches!(
            negotiate(
                &CompilerRequest::Hello {
                    protocol_version: COMPILER_PROTOCOL_VERSION + 1,
                },
                Some(session_evidence()),
            ),
            CompilerReply::Error {
                code: CompilerErrorCode::ProtocolVersion,
                ..
            }
        ));
        assert!(matches!(
            negotiate(
                &CompilerRequest::Hello {
                    protocol_version: COMPILER_PROTOCOL_VERSION,
                },
                Some(CompilerSessionHelloEvidence {
                    session_attestation: session_attestation(),
                    capability_manifest: CompilerSessionCapabilityManifest {
                        version: 0,
                        operation_ids: Vec::new(),
                    },
                }),
            ),
            CompilerReply::Error {
                code: CompilerErrorCode::Unavailable,
                ..
            }
        ));
        assert!(matches!(
            negotiate(
                &CompilerRequest::Hello {
                    protocol_version: 1,
                },
                Some(session_evidence()),
            ),
            CompilerReply::Error {
                code: CompilerErrorCode::ProtocolVersion,
                ..
            }
        ));
        assert!(matches!(
            negotiate(&CompilerRequest::Check { project: project() }, None),
            CompilerReply::Error {
                code: CompilerErrorCode::ProtocolVersion,
                ..
            }
        ));
        assert!(matches!(
            negotiate(
                &CompilerRequest::Hello {
                    protocol_version: COMPILER_PROTOCOL_VERSION,
                },
                Some(CompilerSessionHelloEvidence {
                    session_attestation: CompilerSessionAttestation {
                        id: "not-a-core-attestation".to_string(),
                    },
                    capability_manifest: CompilerSessionCapabilityManifest::fixed_query_only(),
                }),
            ),
            CompilerReply::Error {
                code: CompilerErrorCode::Unavailable,
                ..
            }
        ));
    }

    #[test]
    fn capability_manifest_requires_the_complete_canonical_query_inventory() {
        let manifest = CompilerSessionCapabilityManifest::fixed_query_only();
        assert!(manifest.is_well_formed());
        assert_eq!(manifest.version, COMPILER_QUERY_CAPABILITY_MANIFEST_VERSION);
        assert_eq!(
            manifest.operation_ids,
            vec![
                CompilerQueryOperationId::DescribeProject,
                CompilerQueryOperationId::Check,
                CompilerQueryOperationId::ListDiagnostics,
                CompilerQueryOperationId::GetStaticType,
                CompilerQueryOperationId::GetStaticSymbol,
                CompilerQueryOperationId::ListStaticMetadata,
                CompilerQueryOperationId::GetStaticProvenance,
                CompilerQueryOperationId::GetStaticContract,
                CompilerQueryOperationId::ValidateStaticContract,
            ]
        );

        let mut reordered = manifest.clone();
        reordered.operation_ids.swap(0, 1);
        assert!(!reordered.is_well_formed());

        let mut subset = manifest.clone();
        subset.operation_ids.pop();
        assert!(!subset.is_well_formed());

        let mut duplicate = manifest.clone();
        duplicate
            .operation_ids
            .push(CompilerQueryOperationId::Check);
        assert!(!duplicate.is_well_formed());

        let mut unknown_version = manifest;
        unknown_version.version += 1;
        assert!(!unknown_version.is_well_formed());
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
        assert!(!CompilerStaticMetadataCursor { id: 0 }.is_well_formed());
        assert!(CompilerStaticMetadataCursor { id: 1 }.is_well_formed());
        assert!(!CompilerDiagnosticCursor { id: 0 }.is_well_formed());
        assert!(CompilerDiagnosticCursor { id: 1 }.is_well_formed());
        assert!(generation().is_well_formed());
    }
}
