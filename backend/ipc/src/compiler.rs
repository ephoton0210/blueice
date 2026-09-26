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
//! Version ten adds a bounded, source-free inventory of owner-exposed
//! startup project IDs. A core listener grants subsequent project queries only after
//! that exact accepted stream received the inventory; guessed IDs cannot
//! reach the compiler cache. It adds no registration, source, or write path.
//! Version nine adds optional original-source UTF-16 coordinates to the
//! existing bounded diagnostic records. They are derived only from exact
//! authorized source bytes, carry no source text or read authority, and do
//! not add a query operation or capability. Version eight adds
//! exact-generation, receipt-bound symbol and contract declaration locations.
//! The bounded replies contain only compiler-minted
//! IDs, UTF-8 byte ranges, and original-source UTF-16 coordinates; they do
//! not offer arbitrary offset mapping or source reads. Version seven adds the
//! checker's export classification to the existing generation-bound
//! static-symbol query reply. It adds no operation or
//! authority, so the fixed query-only capability manifest remains v3.
//! Version six adds source-free, one-shot pages of incremental compiler
//! work-set module identities for an exact checked generation. Version five
//! added a core-minted, fixed query-only capability manifest to
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
pub const COMPILER_PROTOCOL_VERSION: u32 = 10;

/// A sealed catalog can expose at most this many project identities on one
/// compiler stream. Inventory is a single bounded source-free response.
pub const COMPILER_MAX_PROJECT_INVENTORY: usize = 128;

/// The code is a fixed compiler vocabulary, not project-controlled prose.
/// Keep its wire budget separate from the bounded module and message fields.
pub const COMPILER_DIAGNOSTIC_MAX_CODE_BYTES: usize = 64;

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
pub const COMPILER_QUERY_CAPABILITY_MANIFEST_VERSION: u32 = 5;

/// Stable, source-free identifiers for the exact read-only compiler queries
/// available over this transport. The protocol deliberately has no variants
/// for project registration, source reads, option changes, builds, artifacts,
/// paths, or output writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CompilerQueryOperationId {
    ListProjects,
    DescribeProject,
    Check,
    ListDiagnostics,
    ListWorkSet,
    GetStaticType,
    GetStaticSymbol,
    GetStaticSymbolLocation,
    ListStaticMetadata,
    GetStaticProvenance,
    GetStaticContract,
    GetStaticContractLocation,
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
            CompilerQueryOperationId::ListProjects,
            CompilerQueryOperationId::DescribeProject,
            CompilerQueryOperationId::Check,
            CompilerQueryOperationId::ListDiagnostics,
            CompilerQueryOperationId::ListWorkSet,
            CompilerQueryOperationId::GetStaticType,
            CompilerQueryOperationId::GetStaticSymbol,
            CompilerQueryOperationId::GetStaticSymbolLocation,
            CompilerQueryOperationId::ListStaticMetadata,
            CompilerQueryOperationId::GetStaticProvenance,
            CompilerQueryOperationId::GetStaticContract,
            CompilerQueryOperationId::GetStaticContractLocation,
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

/// Bounded, ordered IDs selected by the sealed core startup catalog. The
/// response contains no entry module, root, source, configuration, artifact,
/// resolver, option, or write target. On a real accepted stream these IDs
/// become project receipts only after this exact response was delivered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompilerProjectInventory {
    pub projects: Vec<CompilerProject>,
}

impl CompilerProjectInventory {
    pub fn is_well_formed(&self) -> bool {
        self.projects.len() <= COMPILER_MAX_PROJECT_INVENTORY
            && self.projects.iter().all(|project| project.is_well_formed())
            && self.projects.windows(2).all(|pair| pair[0].id < pair[1].id)
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

/// One of the four compiler-produced incremental-cache work-sets. Its page
/// entries are canonical module identities, not source-read capabilities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CompilerWorkSetKind {
    Parsed,
    ReusedParsed,
    Rechecked,
    ReusedChecked,
}

/// A core-minted, one-shot work-set continuation bound to one accepted stream,
/// exact generation, and work-set kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CompilerWorkSetCursor {
    pub id: u64,
}

impl CompilerWorkSetCursor {
    pub fn is_well_formed(self) -> bool {
        self.id != 0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompilerWorkSetPage {
    pub generation: CompilerGeneration,
    pub kind: CompilerWorkSetKind,
    pub entries: Vec<String>,
    pub next_cursor: Option<CompilerWorkSetCursor>,
    /// True only if fixed core retention omitted later entries.
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
    /// Original-source zero-based UTF-16 coordinates, present only when the
    /// exact authorized source and both UTF-8 boundaries are available.
    pub coordinates: Option<CompilerSourceCoordinates>,
    /// Bounded compiler prose. The engine adapter rejects over-budget data
    /// rather than silently exposing an unbounded diagnostic.
    pub message: String,
}

impl CompilerDiagnostic {
    /// Structural validation at a public adapter boundary. The source bytes
    /// are intentionally unavailable here; only the core can establish that
    /// an otherwise plausible UTF-16 position really maps to this span.
    pub fn is_well_formed(&self) -> bool {
        !self.code.is_empty()
            && self.code.len() <= COMPILER_DIAGNOSTIC_MAX_CODE_BYTES
            && !self.module.is_empty()
            && self.start <= self.end
            && self.coordinates.is_none_or(|coordinates| {
                coordinates.is_well_formed_for_diagnostic_range(self.start, self.end)
            })
    }
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

impl CompilerDiagnosticPage {
    /// A page can continue only after returning at least one entry. Its
    /// generation and each coordinate must agree with the exact request.
    pub fn is_well_formed_for_generation(&self, generation: CompilerGeneration) -> bool {
        generation.is_well_formed()
            && self.generation == generation
            && (self.next_cursor.is_none() || !self.entries.is_empty())
            && self
                .next_cursor
                .is_none_or(CompilerDiagnosticCursor::is_well_formed)
            && self.entries.iter().all(CompilerDiagnostic::is_well_formed)
    }
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
    /// The BlueTS checker's exact module-export classification.
    pub exported: bool,
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

/// Zero-based original-source declaration coordinates. Columns count UTF-16
/// code units, while the paired range uses UTF-8 byte offsets. The fixed
/// 1 MiB range cap matches the BlueTS parser's per-source limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompilerSourceCoordinates {
    pub start_line: u32,
    pub start_column_utf16: u32,
    pub end_line: u32,
    pub end_column_utf16: u32,
}

pub const COMPILER_STATIC_LOCATION_MAX_SOURCE_BYTES: u64 = 1_048_576;

impl CompilerSourceCoordinates {
    pub fn is_well_formed_for_range(self, start_byte: u64, end_byte: u64) -> bool {
        start_byte < end_byte
            && end_byte <= COMPILER_STATIC_LOCATION_MAX_SOURCE_BYTES
            && (self.start_line, self.start_column_utf16) < (self.end_line, self.end_column_utf16)
            && u64::from(self.start_line) + u64::from(self.start_column_utf16) <= start_byte
            && u64::from(self.end_line) + u64::from(self.end_column_utf16) <= end_byte
    }

    /// Diagnostics may be zero-width at EOF, unlike declaration locations.
    /// Both positions must still be ordered and plausible for the byte range.
    pub fn is_well_formed_for_diagnostic_range(self, start_byte: u64, end_byte: u64) -> bool {
        start_byte <= end_byte
            && end_byte <= COMPILER_STATIC_LOCATION_MAX_SOURCE_BYTES
            && (self.start_line, self.start_column_utf16) <= (self.end_line, self.end_column_utf16)
            && u64::from(self.start_line) + u64::from(self.start_column_utf16) <= start_byte
            && u64::from(self.end_line) + u64::from(self.end_column_utf16) <= end_byte
    }
}

/// One exact-generation declaration location. Both IDs are compiler-minted;
/// the core checks that the requested source owns this exact symbol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompilerStaticSymbolLocation {
    pub generation: CompilerGeneration,
    pub symbol_id: u32,
    pub source_id: u32,
    pub start_byte: u64,
    pub end_byte: u64,
    pub coordinates: CompilerSourceCoordinates,
}

impl CompilerStaticSymbolLocation {
    pub fn is_well_formed(self) -> bool {
        self.generation.is_well_formed()
            && self
                .coordinates
                .is_well_formed_for_range(self.start_byte, self.end_byte)
    }
}

/// One exact-generation reifiable-contract declaration location. It is not
/// a runtime value, source read, or general source-map lookup.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompilerStaticContractLocation {
    pub generation: CompilerGeneration,
    pub contract_id: u32,
    pub source_id: u32,
    pub start_byte: u64,
    pub end_byte: u64,
    pub coordinates: CompilerSourceCoordinates,
}

impl CompilerStaticContractLocation {
    pub fn is_well_formed(self) -> bool {
        self.generation.is_well_formed()
            && self
                .coordinates
                .is_well_formed_for_range(self.start_byte, self.end_byte)
    }
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
    /// Lists only the sealed startup catalog's opaque project IDs. No caller
    /// can register or modify a project through this query.
    ListProjects,
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
    /// Paginates one retained incremental work-set. A cursor is not an offset
    /// and cannot be used for another set, generation, or accepted stream.
    ListWorkSet {
        generation: CompilerGeneration,
        kind: CompilerWorkSetKind,
        cursor: Option<CompilerWorkSetCursor>,
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
    /// Resolves only a compiler-minted declaration boundary, never a caller-
    /// selected byte offset. MCP requires both IDs to have been inventoried
    /// on its current session before forwarding this request.
    GetStaticSymbolLocation {
        generation: CompilerGeneration,
        symbol_id: u32,
        source_id: u32,
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
    GetStaticContractLocation {
        generation: CompilerGeneration,
        contract_id: u32,
        source_id: u32,
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
    InvalidProjectInventory,
    /// A project ID was not returned to this accepted compiler stream by
    /// `ListProjects`, even if another stream knows the same numeric ID.
    UnobservedProject,
    StaleGeneration,
    NoStaticMetadata,
    UnknownType,
    UnknownSymbol,
    UnknownSource,
    InvalidLocationTarget,
    UnknownContract,
    InvalidContractValue,
    InvalidDiagnosticCursor,
    InvalidDiagnosticPage,
    InvalidWorkSetCursor,
    InvalidWorkSetPage,
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
    Projects(CompilerProjectInventory),
    Project(CompilerProjectIdentity),
    Check(CompilerCheck),
    DiagnosticPage(CompilerDiagnosticPage),
    WorkSetPage(CompilerWorkSetPage),
    StaticType(CompilerStaticType),
    StaticSymbol(CompilerStaticSymbol),
    StaticSymbolLocation(CompilerStaticSymbolLocation),
    StaticMetadataPage(CompilerStaticMetadataPage),
    StaticProvenance(CompilerStaticProvenance),
    StaticContract(CompilerStaticContract),
    StaticContractLocation(CompilerStaticContractLocation),
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
            CompilerRequest::ListProjects
            | CompilerRequest::DescribeProject { .. }
            | CompilerRequest::Check { .. }
            | CompilerRequest::ListDiagnostics { .. }
            | CompilerRequest::ListWorkSet { .. }
            | CompilerRequest::GetStaticType { .. }
            | CompilerRequest::GetStaticSymbol { .. }
            | CompilerRequest::GetStaticSymbolLocation { .. }
            | CompilerRequest::ListStaticMetadata { .. }
            | CompilerRequest::GetStaticProvenance { .. }
            | CompilerRequest::GetStaticContract { .. }
            | CompilerRequest::GetStaticContractLocation { .. }
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
mod tests;
