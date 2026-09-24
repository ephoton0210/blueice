// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Versioned native debugger IPC vocabulary.
//!
//! This channel is intentionally separate from page-script DOM calls and the
//! public automation protocol. It gives `core` and an out-of-process BlueJS
//! host one typed way to agree on a page realm, its generation, and executable
//! program locations. It establishes framing, handshake, capability discovery,
//! bounded opaque program-location operations, exact breakpoint configuration,
//! and an opt-in root-code-unit pause/resume seam. Version nineteen adds an
//! independently default-denied symbol-to-static-type relation that requires
//! exact symbol and type receipts on one debugger stream. Version eighteen adds the
//! independently default-deny symbol-location operation for prior opaque
//! symbol and source receipts. Version seventeen added the independently
//! default-deny lowering-map summary operation for a prior
//! opaque metadata handle. Version sixteen added the default-deny data-only
//! contract-validation operation for a prior opaque
//! contract ID
//! handle: `Hello` grants only the canonical intersection of a requested
//! manifest and the core policy, and a metadata operation may be dispatched
//! only after the exact target's capability report also grants its specific
//! metadata capability. A host must report every operation as
//! [`DebuggerCapabilityState::Available`] only after it implements the native
//! behavior; a configured breakpoint is not evidence that pause, stack,
//! scope, value inspection, or static metadata access already exists.

use crate::compiler::CompilerContractValue;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::io::{self, Read, Write};
use std::sync::{Arc, Mutex};

/// Independent protocol version for the private core-to-BlueJS debugger
/// channel. It does not share `crate::PROTOCOL_VERSION`, whose lifecycle is
/// the frontend control-plane protocol.
pub const DEBUGGER_PROTOCOL_VERSION: u32 = 19;

/// A core-owned page realm identity. The browser-context field is present from
/// from the first protocol revision even while the current core exposes only
/// its default context, so an old debugger client cannot silently retarget an
/// equally numbered tab in a future context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DebuggerPageRealm {
    pub browser_context_id: u64,
    pub tab_id: u64,
    pub realm_generation: u64,
}

impl DebuggerPageRealm {
    /// The wire format never treats zero as a valid owner-issued identity.
    pub fn is_well_formed(self) -> bool {
        self.browser_context_id != 0 && self.tab_id != 0 && self.realm_generation != 0
    }
}

/// An exact BlueJS program generation in one page realm. It is deliberately
/// separate from a source URL or an offset: a replacement program cannot
/// inherit a debugger handle merely because it has the same source identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DebuggerProgram {
    pub realm: DebuggerPageRealm,
    pub program_handle: u64,
    pub program_generation: u64,
}

impl DebuggerProgram {
    pub fn is_well_formed(self) -> bool {
        self.realm.is_well_formed() && self.program_handle != 0 && self.program_generation != 0
    }
}

/// A source-free, generation-bound handle for future debugger static-metadata
/// operations. It is deliberately not a source URL, hash, symbol name, type,
/// span, bytecode offset, or VM object. A host may issue one only after it has
/// granted the corresponding [`DebuggerMetadataCapability`] for this exact
/// program generation, and it must reject the handle after that program or
/// its enclosing realm changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DebuggerStaticMetadataHandle {
    pub program: DebuggerProgram,
    pub metadata_handle: u64,
    pub metadata_generation: u64,
}

impl DebuggerStaticMetadataHandle {
    /// The wire format never treats zero as an issuer-created handle or
    /// generation. The caller still has to check the live program owner.
    pub fn is_well_formed(self) -> bool {
        self.program.is_well_formed() && self.metadata_handle != 0 && self.metadata_generation != 0
    }
}

/// A bounded, source-free description of one static BlueTS metadata record.
///
/// This summary is deliberately distinct from the record itself. It exposes
/// only the compiler/language fingerprints and aggregate collection sizes
/// needed to identify a compilation; it contains no source identity or text,
/// span, name, type display, symbol, contract, bytecode, runtime value, or
/// dereferenceable child handle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DebuggerStaticMetadataSummary {
    /// The exact opaque record this description names.
    pub metadata: DebuggerStaticMetadataHandle,
    /// Fixed compiler language vocabulary, bounded by the protocol owner.
    pub language_version: String,
    /// A compiler-selected options fingerprint. It is an identifier, not an
    /// options/configuration dump.
    pub compiler_options_hash: String,
    pub source_count: u32,
    pub type_count: u32,
    pub symbol_count: u32,
    pub contract_count: u32,
}

impl DebuggerStaticMetadataSummary {
    /// Rejects malformed or over-budget child-proxied summaries before one
    /// reaches a debugger client. These are fixed protocol limits, never
    /// caller-selected pagination or allocation parameters.
    pub fn is_well_formed(&self) -> bool {
        self.metadata.is_well_formed()
            && !self.language_version.is_empty()
            && self.language_version.len() <= DEBUGGER_STATIC_METADATA_LANGUAGE_VERSION_MAX_BYTES
            && !self.compiler_options_hash.is_empty()
            && self.compiler_options_hash.len()
                <= DEBUGGER_STATIC_METADATA_COMPILER_OPTIONS_HASH_MAX_BYTES
            && self.source_count <= DEBUGGER_STATIC_METADATA_MAX_SOURCES
            && self.type_count <= DEBUGGER_STATIC_METADATA_MAX_TYPES
            && self.symbol_count <= DEBUGGER_STATIC_METADATA_MAX_SYMBOLS
            && self.contract_count <= DEBUGGER_STATIC_METADATA_MAX_CONTRACTS
    }
}

/// Maximum length of the fixed BlueTS language-version label exposed by the
/// summary surface.
pub const DEBUGGER_STATIC_METADATA_LANGUAGE_VERSION_MAX_BYTES: usize = 64;
/// Maximum length of the compiler-owned options fingerprint in a summary.
pub const DEBUGGER_STATIC_METADATA_COMPILER_OPTIONS_HASH_MAX_BYTES: usize = 128;
/// Retention-derived upper bounds for the summary's aggregate counts.
pub const DEBUGGER_STATIC_METADATA_MAX_SOURCES: u32 = 4_096;
pub const DEBUGGER_STATIC_METADATA_MAX_TYPES: u32 = 4_096;
pub const DEBUGGER_STATIC_METADATA_MAX_SYMBOLS: u32 = 65_536;
pub const DEBUGGER_STATIC_METADATA_MAX_CONTRACTS: u32 = 65_536;
/// Maximum end offset for one authorized static source span. This matches the
/// page host's fixed one-mebibyte per-module compiler admission limit.
pub const DEBUGGER_STATIC_METADATA_MAX_SOURCE_SPAN_BYTES: u32 = 1_048_576;
/// Maximum UTF-8 byte length for one explicitly authorized static type
/// display. This is a fixed protocol budget, not a caller-provided limit.
pub const DEBUGGER_STATIC_METADATA_TYPE_DISPLAY_MAX_BYTES: usize = 4_096;
/// Maximum UTF-8 byte length for one explicitly authorized static symbol
/// display. This is a fixed protocol budget, not a caller-provided limit.
pub const DEBUGGER_STATIC_METADATA_SYMBOL_DISPLAY_MAX_BYTES: usize = 4_096;
/// Maximum UTF-8 byte length for one explicitly authorized static contract
/// display. This is a fixed protocol budget, not a caller-provided limit.
pub const DEBUGGER_STATIC_METADATA_CONTRACT_DISPLAY_MAX_BYTES: usize = 4_096;
/// Fixed data-only contract-validation limits for the public debugger. The
/// client cannot select or relax them; core and the supervised child both
/// apply the same envelope before a contract plan is consulted.
pub const DEBUGGER_STATIC_METADATA_CONTRACT_VALIDATION_MAX_DEPTH: usize = 64;
pub const DEBUGGER_STATIC_METADATA_CONTRACT_VALIDATION_MAX_COLLECTION_ENTRIES: usize = 4_096;
pub const DEBUGGER_STATIC_METADATA_CONTRACT_VALIDATION_MAX_NODES: usize = 32_768;
pub const DEBUGGER_STATIC_METADATA_CONTRACT_VALIDATION_MAX_STRING_BYTES: usize = 256 * 1_024;
/// Maximum byte length for one fixed, compiler-owned lowering-map ABI label.
pub const DEBUGGER_STATIC_METADATA_LOWERING_ABI_MAX_BYTES: usize = 128;
/// The only safe-point-map ABI emitted by the currently supported direct
/// BlueTS-to-BlueJS bridge. A child cannot substitute an arbitrary label to
/// smuggle compiler diagnostics or source-derived data through this field.
pub const DEBUGGER_STATIC_METADATA_SAFE_POINT_MAP_ABI_V1: &str = "bluejs-safe-point-map-v1";
/// The only executable-program ABI paired with the supported direct bridge.
pub const DEBUGGER_STATIC_METADATA_PROGRAM_ABI_V1: &str = "bluejs-program-v1";
/// Maximum byte length for one deterministic source-set fingerprint. This
/// identifies a verified map input set, never an individual source or text.
pub const DEBUGGER_STATIC_METADATA_LOWERING_SOURCE_SET_HASH_MAX_BYTES: usize = 128;
/// Prefix and exact opaque digest width for a direct bridge source-set
/// fingerprint. The digest is an aggregate receipt, never a module identity.
pub const DEBUGGER_STATIC_METADATA_SOURCE_SET_HASH_PREFIX: &str = "bts-source-set-";
pub const DEBUGGER_STATIC_METADATA_SOURCE_SET_HASH_HEX_BYTES: usize = 16;
/// The public lowering summary reports only the count of verified bound map
/// entries. It never carries the entries' source spans or bytecode offsets.
pub const DEBUGGER_STATIC_METADATA_MAX_BOUND_SAFE_POINTS: u32 = 4_096;
/// Maximum compiler-canonical module identity exposed by the distinct,
/// owner-authorized provenance surface.
pub const DEBUGGER_STATIC_METADATA_MODULE_MAX_BYTES: usize = 4_096;
/// Required label for the compiler provenance digest.
pub const DEBUGGER_STATIC_METADATA_SHA256_DIGEST_PREFIX: &str = "bts-sha256:";
/// Exact byte length of a labeled lowercase SHA-256 digest.
pub const DEBUGGER_STATIC_METADATA_SHA256_DIGEST_BYTES: usize =
    DEBUGGER_STATIC_METADATA_SHA256_DIGEST_PREFIX.len() + 64;

/// One source-record identity retained by an exact static metadata attachment.
/// The identifier is compiler-minted but carries no module identity, source
/// text, hash, span, symbol, type, contract, bytecode, VM object, or value.
/// It is usable only with the enclosing opaque metadata handle, which remains
/// generation-bound to its live program.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DebuggerStaticMetadataSourceId {
    pub metadata: DebuggerStaticMetadataHandle,
    pub source_id: u32,
}

/// One compiler-minted static type identity for an exact opaque metadata
/// attachment. It exposes neither a type display nor a source, span, symbol,
/// contract, bytecode, VM object, value, or arbitrary metadata read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DebuggerStaticMetadataTypeId {
    pub metadata: DebuggerStaticMetadataHandle,
    pub type_id: u32,
}

impl DebuggerStaticMetadataTypeId {
    pub fn is_well_formed(self) -> bool {
        self.metadata.is_well_formed()
    }
}

/// One compiler-minted static symbol identity for an exact opaque metadata
/// attachment. It exposes no symbol name, source span, declared type,
/// contract, bytecode, VM object, value, or arbitrary metadata read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DebuggerStaticMetadataSymbolId {
    pub metadata: DebuggerStaticMetadataHandle,
    pub symbol_id: u32,
}

/// One compiler-minted static contract identity for an exact opaque metadata
/// attachment. It exposes no contract name, source span, plan, validation,
/// bytecode, VM object, value, or arbitrary metadata read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DebuggerStaticMetadataContractId {
    pub metadata: DebuggerStaticMetadataHandle,
    pub contract_id: u32,
}

impl DebuggerStaticMetadataContractId {
    pub fn is_well_formed(self) -> bool {
        self.metadata.is_well_formed()
    }
}

/// One owner-authorized display for a compiler-minted contract ID previously
/// returned by the exact stream's contract inventory. A display can contain a
/// project-authored identifier, so it is independently default-denied and
/// carries no source span, plan, validation behavior, bytecode, or static
/// record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DebuggerStaticMetadataContractDisplay {
    pub contract: DebuggerStaticMetadataContractId,
    pub display: String,
}

impl DebuggerStaticMetadataContractDisplay {
    pub fn is_well_formed(&self) -> bool {
        self.contract.is_well_formed()
            && !self.display.is_empty()
            && self.display.len() <= DEBUGGER_STATIC_METADATA_CONTRACT_DISPLAY_MAX_BYTES
    }
}

/// Source-free result of validating one caller-provided data-only snapshot
/// against a prior inventoried static contract. It deliberately returns only
/// the exact opaque ID and a boolean: no input echo, failure path, expected
/// shape, observed category, contract plan, source span, or runtime value can
/// cross the debugger boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DebuggerStaticMetadataContractValidation {
    pub contract: DebuggerStaticMetadataContractId,
    pub valid: bool,
}

impl DebuggerStaticMetadataContractValidation {
    pub fn is_well_formed(&self) -> bool {
        self.contract.is_well_formed()
    }
}

/// One owner-authorized, source-free summary of the verified direct
/// BlueTS-to-BlueJS lowering map paired with an exact opaque metadata handle.
/// It contains fixed ABI labels, a deterministic source-set fingerprint, and
/// only an aggregate entry count. It does not expose any source identity,
/// source span, AST node, code-unit identity, bytecode offset, map entry,
/// BlueJS object/value, or general static-record read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DebuggerStaticMetadataLoweringSummary {
    pub metadata: DebuggerStaticMetadataHandle,
    pub safe_point_map_abi: String,
    pub program_abi: String,
    pub source_set_hash: String,
    pub bound_safe_point_count: u32,
}

impl DebuggerStaticMetadataLoweringSummary {
    pub fn is_well_formed(&self) -> bool {
        self.metadata.is_well_formed()
            && self.safe_point_map_abi == DEBUGGER_STATIC_METADATA_SAFE_POINT_MAP_ABI_V1
            && self.safe_point_map_abi.len() <= DEBUGGER_STATIC_METADATA_LOWERING_ABI_MAX_BYTES
            && self.program_abi == DEBUGGER_STATIC_METADATA_PROGRAM_ABI_V1
            && self.program_abi.len() <= DEBUGGER_STATIC_METADATA_LOWERING_ABI_MAX_BYTES
            && self
                .source_set_hash
                .strip_prefix(DEBUGGER_STATIC_METADATA_SOURCE_SET_HASH_PREFIX)
                .is_some_and(|digest| {
                    digest.len() == DEBUGGER_STATIC_METADATA_SOURCE_SET_HASH_HEX_BYTES
                        && digest
                            .bytes()
                            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
                })
            && self.source_set_hash.len()
                <= DEBUGGER_STATIC_METADATA_LOWERING_SOURCE_SET_HASH_MAX_BYTES
            && self.bound_safe_point_count <= DEBUGGER_STATIC_METADATA_MAX_BOUND_SAFE_POINTS
    }
}

/// One owner-authorized display for a compiler-minted symbol ID previously
/// returned by the exact stream's symbol inventory. A display can contain a
/// project-authored identifier, so it is independently default-denied and
/// carries no source span, type, contract, bytecode, or static record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DebuggerStaticMetadataSymbolDisplay {
    pub symbol: DebuggerStaticMetadataSymbolId,
    pub display: String,
}

impl DebuggerStaticMetadataSymbolDisplay {
    pub fn is_well_formed(&self) -> bool {
        self.symbol.is_well_formed()
            && !self.display.is_empty()
            && self.display.len() <= DEBUGGER_STATIC_METADATA_SYMBOL_DISPLAY_MAX_BYTES
    }
}

/// One owner-authorized location for a compiler-minted symbol that the exact
/// debugger stream previously inventoried. The range is a half-open UTF-8
/// byte range under a separately receipted source ID: it is not source text,
/// a module identity, a line/column conversion, a bytecode position, or a
/// source-read capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DebuggerStaticMetadataSymbolLocation {
    pub symbol: DebuggerStaticMetadataSymbolId,
    pub source: DebuggerStaticMetadataSourceId,
    pub start_byte: u32,
    pub end_byte: u32,
}

impl DebuggerStaticMetadataSymbolLocation {
    /// Rejects malformed or over-budget child locations before they reach a
    /// debugger client. An unchecked machine-sized offset can never become a
    /// wire-range or allocation ambiguity.
    pub fn is_well_formed(self) -> bool {
        self.symbol.is_well_formed()
            && self.source.is_well_formed()
            && self.symbol.metadata == self.source.metadata
            && self.start_byte < self.end_byte
            && self.end_byte <= DEBUGGER_STATIC_METADATA_MAX_SOURCE_SPAN_BYTES
    }
}

/// One exact prior symbol/source receipt pair for the separately authorized
/// symbol-location operation. Core validates both IDs against the same stream
/// before forwarding a private lookup, so a child result cannot introduce an
/// unrequested source identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DebuggerStaticMetadataSymbolLocationTarget {
    pub symbol: DebuggerStaticMetadataSymbolId,
    pub source: DebuggerStaticMetadataSourceId,
}

impl DebuggerStaticMetadataSymbolLocationTarget {
    pub fn is_well_formed(self) -> bool {
        self.symbol.is_well_formed()
            && self.source.is_well_formed()
            && self.symbol.metadata == self.source.metadata
    }
}

/// One compiler-verified relation between a symbol and its static type. Both
/// opaque IDs must have been returned by separate inventories on this stream.
/// This exposes no type display, symbol name, source, span, or static record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DebuggerStaticMetadataSymbolType {
    pub symbol: DebuggerStaticMetadataSymbolId,
    pub static_type: DebuggerStaticMetadataTypeId,
}

impl DebuggerStaticMetadataSymbolType {
    pub fn is_well_formed(self) -> bool {
        self.symbol.is_well_formed()
            && self.static_type.is_well_formed()
            && self.symbol.metadata == self.static_type.metadata
    }
}

impl DebuggerStaticMetadataSymbolId {
    pub fn is_well_formed(self) -> bool {
        self.metadata.is_well_formed()
    }
}

/// One owner-authorized, bounded display for a compiler-minted type ID that
/// was previously returned by the exact stream's type inventory. A display is
/// source-text-free but can contain project-authored identifiers, so it is a
/// distinct default-deny disclosure rather than an implication of type-ID
/// inventory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DebuggerStaticMetadataTypeDisplay {
    /// The parent-bound, generation-bound type identity this display names.
    pub static_type: DebuggerStaticMetadataTypeId,
    /// A compiler-produced type rendering, never source text or a general
    /// static-record payload.
    pub display: String,
}

impl DebuggerStaticMetadataTypeDisplay {
    /// Rejects malformed and oversized child-proxied displays before a
    /// debugger client observes them.
    pub fn is_well_formed(&self) -> bool {
        self.static_type.is_well_formed()
            && !self.display.is_empty()
            && self.display.len() <= DEBUGGER_STATIC_METADATA_TYPE_DISPLAY_MAX_BYTES
    }
}

impl DebuggerStaticMetadataSourceId {
    pub fn is_well_formed(self) -> bool {
        self.metadata.is_well_formed()
    }
}

/// One owner-authorized, source-text-free provenance description for a
/// compiler-minted source ID. Module identity and a labeled SHA-256 digest
/// identify the exact compiler input, but this carries no source text, span,
/// name, type, symbol, contract, bytecode, VM object, value, or read ability.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DebuggerStaticMetadataSourceProvenance {
    pub source: DebuggerStaticMetadataSourceId,
    /// Compiler-canonical module identity, never a caller-selected path.
    pub module: String,
    /// Compiler-selected `bts-sha256:` digest, never source text.
    pub content_hash: String,
}

impl DebuggerStaticMetadataSourceProvenance {
    /// Checks the fixed disclosure budget before a child-proxied result
    /// reaches a debugger client.
    pub fn is_well_formed(&self) -> bool {
        self.source.is_well_formed()
            && !self.module.is_empty()
            && !self.module.starts_with('/')
            && !self.module.starts_with("file:")
            && self.module.len() <= DEBUGGER_STATIC_METADATA_MODULE_MAX_BYTES
            && self.content_hash.len() == DEBUGGER_STATIC_METADATA_SHA256_DIGEST_BYTES
            && self
                .content_hash
                .starts_with(DEBUGGER_STATIC_METADATA_SHA256_DIGEST_PREFIX)
            && self.content_hash[DEBUGGER_STATIC_METADATA_SHA256_DIGEST_PREFIX.len()..]
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    }
}

/// A compiler-recorded executable bytecode boundary for one exact program.
/// Hosts MUST validate this tuple against BlueJS's program registry rather
/// than translating a nearest source offset heuristically.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DebuggerSafePoint {
    pub program: DebuggerProgram,
    pub code_unit_ordinal: u32,
    pub bytecode_offset: u32,
}

impl DebuggerSafePoint {
    pub fn is_well_formed(self) -> bool {
        self.program.is_well_formed()
    }
}

/// Native debugger features that a host may explicitly advertise.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DebuggerCapability {
    /// Enumerate opaque live program identities and compiler-verified
    /// instruction boundaries, then revalidate an exact tuple. This does not
    /// pause or inspect a VM.
    ProgramLocations,
    /// Install, enumerate, and remove exact generation-bound breakpoint
    /// records. Configuration alone neither starts nor interrupts execution.
    BreakpointConfiguration,
    /// A breakpoint interrupt/pause hook. This remains distinct from
    /// [`Self::BreakpointConfiguration`] so a host cannot imply that a stored
    /// record has stopped a synchronous VM.
    Breakpoints,
    PauseResume,
    Stepping,
    Stack,
    Scopes,
    ExceptionPolicy,
    BoundedValues,
    /// A bounded inventory of source-free, generation-bound static metadata
    /// handles. This does not grant source text, source identity, spans,
    /// symbols, types, contracts, bytecode, runtime values, or a general
    /// metadata dump. Those each need their own later capability and request.
    StaticMetadataInventory,
    /// One bounded, source-free summary for an opaque static-metadata handle.
    /// It is separate from inventory so a client cannot infer a read grant
    /// merely because it may enumerate handles.
    StaticMetadataSummary,
    /// A bounded inventory of compiler-minted source-record identities for
    /// one exact opaque metadata attachment. It exposes no source identity,
    /// content hash, text, span, or record detail.
    StaticMetadataSourceInventory,
    /// One exact source-text-free provenance record for a source ID returned
    /// by the separately negotiated inventory. Module identity and a digest
    /// need a distinct default-deny policy even though neither is source text.
    StaticMetadataSourceProvenance,
    /// A bounded inventory of compiler-minted type-record identities for one
    /// exact opaque metadata attachment. It does not disclose type displays
    /// or other static records.
    StaticMetadataTypeInventory,
    /// One bounded compiler-produced display for a type ID previously
    /// returned by the exact stream's type inventory. This does not expose a
    /// source span, symbol, contract, bytecode, runtime value, or general
    /// metadata-record read.
    StaticMetadataTypeDisplay,
    /// A bounded inventory of compiler-minted symbol-record identities for
    /// one exact opaque metadata attachment. It does not disclose names,
    /// spans, types, or other static records.
    StaticMetadataSymbolInventory,
    /// A bounded inventory of compiler-minted contract identities for one
    /// exact opaque metadata attachment. It does not disclose contract
    /// names, source spans, plans, or validation behavior.
    StaticMetadataContractInventory,
    /// One bounded compiler-produced display for a symbol ID previously
    /// returned by the exact stream's symbol inventory. It does not expose a
    /// source span, type, contract, bytecode, or general static-record read.
    StaticMetadataSymbolDisplay,
    /// One half-open byte range for a symbol and a separately receipted source
    /// ID. It contains no source/module/name/type/contract/bytecode payload.
    StaticMetadataSymbolLocation,
    /// Verifies a symbol's compiler-minted static type using two separately
    /// receipted IDs. It contains no name, display, source, or record payload.
    StaticMetadataSymbolType,
    /// One bounded compiler-produced display for a contract ID previously
    /// returned by the exact stream's contract inventory. It does not expose a
    /// source span, plan, validation behavior, bytecode, or general
    /// static-record read.
    StaticMetadataContractDisplay,
    /// Validates a bounded data-only snapshot against a contract ID that the
    /// exact debugger stream previously inventoried. It returns no plan or
    /// structural failure detail.
    StaticMetadataContractValidation,
    /// One source-free aggregate summary of the verified direct
    /// BlueTS-to-BlueJS lowering map. It carries no map entry, source span,
    /// AST node, code-unit identity, or bytecode offset.
    StaticMetadataLoweringSummary,
}

/// One narrowly scoped static-metadata operation a debugger client may ask
/// for during `Hello` and a core policy may grant for that session.
///
/// This deliberately has no broad `StaticMetadata` or `All` variant. Every
/// future metadata surface must add a distinct variant and map it to a
/// distinct [`DebuggerCapability`] before it can be requested. Summary and
/// source-record inventory each depend on inventory because they accept an
/// exact opaque handle; neither exposes metadata records or a general
/// inspection operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DebuggerMetadataCapability {
    /// Enumerate only bounded [`DebuggerStaticMetadataHandle`] values for one
    /// exact realm. The handles themselves carry no static metadata.
    OpaqueInventory,
    /// Describe one exact inventory handle with compiler fingerprints and
    /// fixed aggregate counts only. This never exposes source text or
    /// identity, spans, names, type displays, symbols, contracts, bytecode,
    /// runtime values, or a metadata-record dereference.
    OpaqueSummary,
    /// Lists only source-record IDs that remain bound to one metadata handle.
    /// It depends on inventory because no source ID is valid without the
    /// parent opaque handle. Source provenance/detail remains separately
    /// default-denied and is not represented by this capability.
    OpaqueSourceInventory,
    /// Describes one source ID previously returned by
    /// [`Self::OpaqueSourceInventory`] with its compiler-canonical module
    /// identity and labeled SHA-256 digest. This is not source text or a
    /// source-read endpoint and needs an independent authorization.
    OpaqueSourceProvenance,
    /// Lists only compiler-minted type-record IDs that remain bound to one
    /// metadata handle. Type displays and static-record reads are distinct,
    /// future default-deny capabilities.
    OpaqueTypeInventory,
    /// Describes one compiler-minted type ID previously returned by
    /// [`Self::OpaqueTypeInventory`]. Type displays are source-text-free but
    /// can include project-authored identifiers, so this is independently
    /// default-denied and remains bounded to one prior receipt.
    OpaqueTypeDisplay,
    /// Lists only compiler-minted symbol-record IDs that remain bound to one
    /// metadata handle. Symbol names, spans, types, contracts, and record
    /// reads remain distinct, future default-deny capabilities.
    OpaqueSymbolInventory,
    /// Lists only compiler-minted static contract IDs that remain bound to
    /// one metadata handle. Contract name/span/plan/validation reads remain
    /// distinct, future default-deny capabilities.
    OpaqueContractInventory,
    /// Describes one compiler-minted symbol ID previously returned by
    /// [`Self::OpaqueSymbolInventory`]. Project-authored identifiers require
    /// an independent default-deny grant and exact receipt.
    OpaqueSymbolDisplay,
    /// Describes one compiler-minted contract ID previously returned by
    /// [`Self::OpaqueContractInventory`]. Project-authored identifiers require
    /// an independent default-deny grant and exact receipt; the contract plan
    /// and validation behavior remain unavailable.
    OpaqueContractDisplay,
    /// Validates a bounded data-only snapshot against one contract ID
    /// previously returned by [`Self::OpaqueContractInventory`]. The outcome
    /// is only a boolean; plan and failure detail stay independently denied.
    OpaqueContractValidation,
    /// Describes only fixed ABI labels, a deterministic source-set fingerprint,
    /// and an aggregate verified-entry count for the exact opaque metadata
    /// handle. Map entries, spans, AST nodes, and bytecode remain unavailable.
    OpaqueLoweringSummary,
    /// Describes a half-open byte range for one compiler-minted symbol that
    /// the stream previously inventoried, under a separately receipted source
    /// ID. It is source-text-free but discloses source structure, so it needs
    /// its own default-deny authorization.
    OpaqueSymbolLocation,
    /// Verifies a symbol-to-type relation only for IDs returned by separate
    /// same-stream inventories. This disclosure has its own owner grant.
    OpaqueSymbolType,
    /// A newer metadata capability identifier. It makes the enclosing
    /// manifest invalid instead of silently narrowing the requested set.
    #[serde(other)]
    Unknown,
}

impl DebuggerMetadataCapability {
    const fn debugger_capability(self) -> Option<DebuggerCapability> {
        match self {
            Self::OpaqueInventory => Some(DebuggerCapability::StaticMetadataInventory),
            Self::OpaqueSummary => Some(DebuggerCapability::StaticMetadataSummary),
            Self::OpaqueSourceInventory => Some(DebuggerCapability::StaticMetadataSourceInventory),
            Self::OpaqueSourceProvenance => {
                Some(DebuggerCapability::StaticMetadataSourceProvenance)
            }
            Self::OpaqueTypeInventory => Some(DebuggerCapability::StaticMetadataTypeInventory),
            Self::OpaqueTypeDisplay => Some(DebuggerCapability::StaticMetadataTypeDisplay),
            Self::OpaqueSymbolInventory => Some(DebuggerCapability::StaticMetadataSymbolInventory),
            Self::OpaqueContractInventory => {
                Some(DebuggerCapability::StaticMetadataContractInventory)
            }
            Self::OpaqueSymbolDisplay => Some(DebuggerCapability::StaticMetadataSymbolDisplay),
            Self::OpaqueContractDisplay => Some(DebuggerCapability::StaticMetadataContractDisplay),
            Self::OpaqueContractValidation => {
                Some(DebuggerCapability::StaticMetadataContractValidation)
            }
            Self::OpaqueLoweringSummary => Some(DebuggerCapability::StaticMetadataLoweringSummary),
            Self::OpaqueSymbolLocation => Some(DebuggerCapability::StaticMetadataSymbolLocation),
            Self::OpaqueSymbolType => Some(DebuggerCapability::StaticMetadataSymbolType),
            Self::Unknown => None,
        }
    }

    const fn canonical_index(self) -> Option<u8> {
        match self {
            Self::OpaqueInventory => Some(0),
            Self::OpaqueSummary => Some(1),
            Self::OpaqueSourceInventory => Some(2),
            Self::OpaqueSourceProvenance => Some(3),
            Self::OpaqueTypeInventory => Some(4),
            Self::OpaqueTypeDisplay => Some(5),
            Self::OpaqueSymbolInventory => Some(6),
            Self::OpaqueContractInventory => Some(7),
            Self::OpaqueSymbolDisplay => Some(8),
            Self::OpaqueContractDisplay => Some(9),
            Self::OpaqueContractValidation => Some(10),
            Self::OpaqueLoweringSummary => Some(11),
            Self::OpaqueSymbolLocation => Some(12),
            Self::OpaqueSymbolType => Some(13),
            Self::Unknown => None,
        }
    }
}

/// Independent schema version for the session-scoped debugger metadata
/// capability manifest. It is intentionally separate from the transport
/// version so future metadata operations cannot be inferred from a transport
/// upgrade alone.
pub const DEBUGGER_METADATA_CAPABILITY_MANIFEST_VERSION: u32 = 1;

/// A canonical requested or granted metadata-capability set for one debugger
/// transport session. It carries capability identifiers only: no realm,
/// source, source identity, metadata handle, bytecode, VM object, or value.
///
/// `Hello` carries the requested set and `HelloAck` carries the core policy's
/// exact intersection. Both sender and receiver must reject a malformed,
/// duplicate, reordered, or unknown set rather than treating it as a partial
/// grant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DebuggerMetadataCapabilityManifest {
    pub version: u32,
    pub capabilities: Vec<DebuggerMetadataCapability>,
}

/// Named owner-selected metadata surfaces used to construct the exact
/// canonical manifest. Keeping these choices named prevents an added
/// default-deny capability from turning call sites into unsafe positional
/// boolean lists.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DebuggerMetadataCapabilitySelection {
    pub summary: bool,
    pub source_inventory: bool,
    pub source_provenance: bool,
    pub type_inventory: bool,
    pub type_display: bool,
    pub symbol_inventory: bool,
    pub contract_inventory: bool,
    pub symbol_display: bool,
    pub contract_display: bool,
    pub contract_validation: bool,
    pub lowering_summary: bool,
    pub symbol_location: bool,
    pub symbol_type: bool,
}

impl DebuggerMetadataCapabilityManifest {
    /// The default core policy grants no debugger metadata capability.
    pub fn empty() -> Self {
        Self {
            version: DEBUGGER_METADATA_CAPABILITY_MANIFEST_VERSION,
            capabilities: Vec::new(),
        }
    }

    /// Grants the first inventory-only metadata surface.
    /// Calling this does not enable any metadata request: a core still needs a
    /// matching live-realm capability report before dispatch.
    pub fn opaque_inventory() -> Self {
        Self {
            version: DEBUGGER_METADATA_CAPABILITY_MANIFEST_VERSION,
            capabilities: vec![DebuggerMetadataCapability::OpaqueInventory],
        }
    }

    /// Grants the inventory plus its dependent, bounded summary surface.
    /// A summary cannot be requested alone: its only target is an exact
    /// handle returned by the inventory operation in this same session.
    pub fn opaque_summary() -> Self {
        Self {
            version: DEBUGGER_METADATA_CAPABILITY_MANIFEST_VERSION,
            capabilities: vec![
                DebuggerMetadataCapability::OpaqueInventory,
                DebuggerMetadataCapability::OpaqueSummary,
            ],
        }
    }

    /// Grants the opaque parent-handle inventory plus bounded source-record
    /// identities. This does not grant any source detail or provenance.
    pub fn opaque_source_inventory() -> Self {
        Self {
            version: DEBUGGER_METADATA_CAPABILITY_MANIFEST_VERSION,
            capabilities: vec![
                DebuggerMetadataCapability::OpaqueInventory,
                DebuggerMetadataCapability::OpaqueSourceInventory,
            ],
        }
    }

    /// Grants both currently independent derived surfaces for an opaque
    /// metadata handle. Keeping this constructor explicit prevents an owner
    /// that enables one bounded read from accidentally treating the other as
    /// implied by transport version or inventory access alone.
    pub fn opaque_summary_and_source_inventory() -> Self {
        Self {
            version: DEBUGGER_METADATA_CAPABILITY_MANIFEST_VERSION,
            capabilities: vec![
                DebuggerMetadataCapability::OpaqueInventory,
                DebuggerMetadataCapability::OpaqueSummary,
                DebuggerMetadataCapability::OpaqueSourceInventory,
            ],
        }
    }

    /// Grants source inventory plus its dependent single-source provenance
    /// surface. This deliberately omits the independent summary capability.
    pub fn opaque_source_provenance() -> Self {
        Self {
            version: DEBUGGER_METADATA_CAPABILITY_MANIFEST_VERSION,
            capabilities: vec![
                DebuggerMetadataCapability::OpaqueInventory,
                DebuggerMetadataCapability::OpaqueSourceInventory,
                DebuggerMetadataCapability::OpaqueSourceProvenance,
            ],
        }
    }

    /// Grants the opaque parent-handle inventory plus compiler-minted type
    /// record identities. The IDs are not type displays or static-record
    /// reads; those require their own later capability.
    pub fn opaque_type_inventory() -> Self {
        Self {
            version: DEBUGGER_METADATA_CAPABILITY_MANIFEST_VERSION,
            capabilities: vec![
                DebuggerMetadataCapability::OpaqueInventory,
                DebuggerMetadataCapability::OpaqueTypeInventory,
            ],
        }
    }

    /// Grants a compiler-produced type display only together with its
    /// required opaque parent and prior type-ID inventory. A display request
    /// must still prove its exact type ID crossed this stream's receipt
    /// boundary before core reaches the child.
    pub fn opaque_type_display() -> Self {
        Self {
            version: DEBUGGER_METADATA_CAPABILITY_MANIFEST_VERSION,
            capabilities: vec![
                DebuggerMetadataCapability::OpaqueInventory,
                DebuggerMetadataCapability::OpaqueTypeInventory,
                DebuggerMetadataCapability::OpaqueTypeDisplay,
            ],
        }
    }

    /// Grants the opaque parent-handle inventory plus compiler-minted symbol
    /// record identities. IDs are not names, spans, declared types, or
    /// static-record reads; those require distinct later capabilities.
    pub fn opaque_symbol_inventory() -> Self {
        Self {
            version: DEBUGGER_METADATA_CAPABILITY_MANIFEST_VERSION,
            capabilities: vec![
                DebuggerMetadataCapability::OpaqueInventory,
                DebuggerMetadataCapability::OpaqueSymbolInventory,
            ],
        }
    }

    /// Grants the opaque parent-handle inventory plus compiler-minted contract
    /// identities. IDs are not names, source spans, plans, or validation
    /// operations; those require distinct later capabilities.
    pub fn opaque_contract_inventory() -> Self {
        Self {
            version: DEBUGGER_METADATA_CAPABILITY_MANIFEST_VERSION,
            capabilities: vec![
                DebuggerMetadataCapability::OpaqueInventory,
                DebuggerMetadataCapability::OpaqueContractInventory,
            ],
        }
    }

    /// Grants a compiler-produced symbol display only together with its
    /// required opaque parent and prior symbol-ID inventory. A display request
    /// must still prove its exact symbol ID crossed this stream's receipt
    /// boundary before core reaches the child.
    pub fn opaque_symbol_display() -> Self {
        Self {
            version: DEBUGGER_METADATA_CAPABILITY_MANIFEST_VERSION,
            capabilities: vec![
                DebuggerMetadataCapability::OpaqueInventory,
                DebuggerMetadataCapability::OpaqueSymbolInventory,
                DebuggerMetadataCapability::OpaqueSymbolDisplay,
            ],
        }
    }

    /// Grants a compiler-produced contract display only together with its
    /// required opaque parent and prior contract-ID inventory. A display
    /// request must still prove its exact contract ID crossed this stream's
    /// receipt boundary before core reaches the child.
    pub fn opaque_contract_display() -> Self {
        Self {
            version: DEBUGGER_METADATA_CAPABILITY_MANIFEST_VERSION,
            capabilities: vec![
                DebuggerMetadataCapability::OpaqueInventory,
                DebuggerMetadataCapability::OpaqueContractInventory,
                DebuggerMetadataCapability::OpaqueContractDisplay,
            ],
        }
    }

    /// Grants a bounded data-only contract validation only together with its
    /// required opaque parent and prior contract-ID inventory. A request must
    /// still prove the exact contract ID crossed this stream's receipt
    /// boundary before core reaches the child, and the result has no error
    /// detail beyond a boolean.
    pub fn opaque_contract_validation() -> Self {
        Self {
            version: DEBUGGER_METADATA_CAPABILITY_MANIFEST_VERSION,
            capabilities: vec![
                DebuggerMetadataCapability::OpaqueInventory,
                DebuggerMetadataCapability::OpaqueContractInventory,
                DebuggerMetadataCapability::OpaqueContractValidation,
            ],
        }
    }

    /// Grants a verified direct-lowering-map summary only with its required
    /// opaque parent inventory. The summary has no per-entry dereference.
    pub fn opaque_lowering_summary() -> Self {
        Self {
            version: DEBUGGER_METADATA_CAPABILITY_MANIFEST_VERSION,
            capabilities: vec![
                DebuggerMetadataCapability::OpaqueInventory,
                DebuggerMetadataCapability::OpaqueLoweringSummary,
            ],
        }
    }

    /// Grants one source-text-free symbol location only with its required
    /// parent, source, and symbol inventory receipts.
    pub fn opaque_symbol_location() -> Self {
        Self {
            version: DEBUGGER_METADATA_CAPABILITY_MANIFEST_VERSION,
            capabilities: vec![
                DebuggerMetadataCapability::OpaqueInventory,
                DebuggerMetadataCapability::OpaqueSourceInventory,
                DebuggerMetadataCapability::OpaqueSymbolInventory,
                DebuggerMetadataCapability::OpaqueSymbolLocation,
            ],
        }
    }

    /// Grants one symbol-to-static-type relation only with its parent,
    /// symbol, and type inventory prerequisites.
    pub fn opaque_symbol_type() -> Self {
        Self {
            version: DEBUGGER_METADATA_CAPABILITY_MANIFEST_VERSION,
            capabilities: vec![
                DebuggerMetadataCapability::OpaqueInventory,
                DebuggerMetadataCapability::OpaqueTypeInventory,
                DebuggerMetadataCapability::OpaqueSymbolInventory,
                DebuggerMetadataCapability::OpaqueSymbolType,
            ],
        }
    }

    /// Builds the exact canonical manifest selected by a trusted owner after
    /// it independently validated each prerequisite flag. Keeping this
    /// operation here avoids a caller hand-assembling a reordered manifest.
    pub fn opaque_selected(selection: DebuggerMetadataCapabilitySelection) -> Self {
        let DebuggerMetadataCapabilitySelection {
            summary,
            source_inventory,
            source_provenance,
            type_inventory,
            type_display,
            symbol_inventory,
            contract_inventory,
            symbol_display,
            contract_display,
            contract_validation,
            lowering_summary,
            symbol_location,
            symbol_type,
        } = selection;
        let any = summary
            || source_inventory
            || source_provenance
            || type_inventory
            || type_display
            || symbol_inventory
            || contract_inventory
            || symbol_display
            || contract_display
            || contract_validation
            || lowering_summary
            || symbol_location
            || symbol_type;
        let mut capabilities = Vec::new();
        if any {
            capabilities.push(DebuggerMetadataCapability::OpaqueInventory);
        }
        if summary {
            capabilities.push(DebuggerMetadataCapability::OpaqueSummary);
        }
        if source_inventory || symbol_location {
            capabilities.push(DebuggerMetadataCapability::OpaqueSourceInventory);
        }
        if source_provenance {
            capabilities.push(DebuggerMetadataCapability::OpaqueSourceProvenance);
        }
        if type_inventory || type_display || symbol_type {
            capabilities.push(DebuggerMetadataCapability::OpaqueTypeInventory);
        }
        if type_display {
            capabilities.push(DebuggerMetadataCapability::OpaqueTypeDisplay);
        }
        if symbol_inventory || symbol_display || symbol_location || symbol_type {
            capabilities.push(DebuggerMetadataCapability::OpaqueSymbolInventory);
        }
        if contract_inventory || contract_display || contract_validation {
            capabilities.push(DebuggerMetadataCapability::OpaqueContractInventory);
        }
        if symbol_display {
            capabilities.push(DebuggerMetadataCapability::OpaqueSymbolDisplay);
        }
        if contract_display {
            capabilities.push(DebuggerMetadataCapability::OpaqueContractDisplay);
        }
        if contract_validation {
            capabilities.push(DebuggerMetadataCapability::OpaqueContractValidation);
        }
        if lowering_summary {
            capabilities.push(DebuggerMetadataCapability::OpaqueLoweringSummary);
        }
        if symbol_location {
            capabilities.push(DebuggerMetadataCapability::OpaqueSymbolLocation);
        }
        if symbol_type {
            capabilities.push(DebuggerMetadataCapability::OpaqueSymbolType);
        }
        let manifest = Self {
            version: DEBUGGER_METADATA_CAPABILITY_MANIFEST_VERSION,
            capabilities,
        };
        debug_assert!(manifest.is_well_formed());
        manifest
    }

    /// Grants every currently implemented opaque static-metadata surface.
    pub fn opaque_summary_source_inventory_and_provenance() -> Self {
        Self {
            version: DEBUGGER_METADATA_CAPABILITY_MANIFEST_VERSION,
            capabilities: vec![
                DebuggerMetadataCapability::OpaqueInventory,
                DebuggerMetadataCapability::OpaqueSummary,
                DebuggerMetadataCapability::OpaqueSourceInventory,
                DebuggerMetadataCapability::OpaqueSourceProvenance,
            ],
        }
    }

    /// Validates the manifest version and strict canonical capability order.
    /// Empty is valid, which is how a caller explicitly requests no metadata.
    pub fn is_well_formed(&self) -> bool {
        if self.version != DEBUGGER_METADATA_CAPABILITY_MANIFEST_VERSION {
            return false;
        }

        let mut previous = None;
        for capability in &self.capabilities {
            let Some(index) = capability.canonical_index() else {
                return false;
            };
            if previous.is_some_and(|previous| previous >= index) {
                return false;
            }
            previous = Some(index);
        }
        (!self
            .capabilities
            .contains(&DebuggerMetadataCapability::OpaqueSummary)
            || self
                .capabilities
                .contains(&DebuggerMetadataCapability::OpaqueInventory))
            && (!self
                .capabilities
                .contains(&DebuggerMetadataCapability::OpaqueSourceInventory)
                || self
                    .capabilities
                    .contains(&DebuggerMetadataCapability::OpaqueInventory))
            && (!self
                .capabilities
                .contains(&DebuggerMetadataCapability::OpaqueSourceProvenance)
                || (self
                    .capabilities
                    .contains(&DebuggerMetadataCapability::OpaqueInventory)
                    && self
                        .capabilities
                        .contains(&DebuggerMetadataCapability::OpaqueSourceInventory)))
            && (!self
                .capabilities
                .contains(&DebuggerMetadataCapability::OpaqueTypeInventory)
                || self
                    .capabilities
                    .contains(&DebuggerMetadataCapability::OpaqueInventory))
            && (!self
                .capabilities
                .contains(&DebuggerMetadataCapability::OpaqueTypeDisplay)
                || (self
                    .capabilities
                    .contains(&DebuggerMetadataCapability::OpaqueInventory)
                    && self
                        .capabilities
                        .contains(&DebuggerMetadataCapability::OpaqueTypeInventory)))
            && (!self
                .capabilities
                .contains(&DebuggerMetadataCapability::OpaqueSymbolInventory)
                || self
                    .capabilities
                    .contains(&DebuggerMetadataCapability::OpaqueInventory))
            && (!self
                .capabilities
                .contains(&DebuggerMetadataCapability::OpaqueContractInventory)
                || self
                    .capabilities
                    .contains(&DebuggerMetadataCapability::OpaqueInventory))
            && (!self
                .capabilities
                .contains(&DebuggerMetadataCapability::OpaqueSymbolDisplay)
                || (self
                    .capabilities
                    .contains(&DebuggerMetadataCapability::OpaqueInventory)
                    && self
                        .capabilities
                        .contains(&DebuggerMetadataCapability::OpaqueSymbolInventory)))
            && (!self
                .capabilities
                .contains(&DebuggerMetadataCapability::OpaqueContractDisplay)
                || (self
                    .capabilities
                    .contains(&DebuggerMetadataCapability::OpaqueInventory)
                    && self
                        .capabilities
                        .contains(&DebuggerMetadataCapability::OpaqueContractInventory)))
            && (!self
                .capabilities
                .contains(&DebuggerMetadataCapability::OpaqueContractValidation)
                || (self
                    .capabilities
                    .contains(&DebuggerMetadataCapability::OpaqueInventory)
                    && self
                        .capabilities
                        .contains(&DebuggerMetadataCapability::OpaqueContractInventory)))
            && (!self
                .capabilities
                .contains(&DebuggerMetadataCapability::OpaqueLoweringSummary)
                || self
                    .capabilities
                    .contains(&DebuggerMetadataCapability::OpaqueInventory))
            && (!self
                .capabilities
                .contains(&DebuggerMetadataCapability::OpaqueSymbolLocation)
                || (self
                    .capabilities
                    .contains(&DebuggerMetadataCapability::OpaqueInventory)
                    && self
                        .capabilities
                        .contains(&DebuggerMetadataCapability::OpaqueSourceInventory)
                    && self
                        .capabilities
                        .contains(&DebuggerMetadataCapability::OpaqueSymbolInventory)))
            && (!self
                .capabilities
                .contains(&DebuggerMetadataCapability::OpaqueSymbolType)
                || (self
                    .capabilities
                    .contains(&DebuggerMetadataCapability::OpaqueInventory)
                    && self
                        .capabilities
                        .contains(&DebuggerMetadataCapability::OpaqueTypeInventory)
                    && self
                        .capabilities
                        .contains(&DebuggerMetadataCapability::OpaqueSymbolInventory)))
    }

    /// Whether this well-formed manifest contains one exact capability.
    pub fn contains(&self, capability: DebuggerMetadataCapability) -> bool {
        self.is_well_formed() && self.capabilities.contains(&capability)
    }

    fn intersection(&self, requested: &Self) -> Self {
        debug_assert!(self.is_well_formed());
        debug_assert!(requested.is_well_formed());
        Self {
            version: DEBUGGER_METADATA_CAPABILITY_MANIFEST_VERSION,
            capabilities: requested
                .capabilities
                .iter()
                .copied()
                .filter(|capability| self.capabilities.contains(capability))
                .collect(),
        }
    }

    fn is_subset_of(&self, requested: &Self) -> bool {
        self.is_well_formed()
            && requested.is_well_formed()
            && self
                .capabilities
                .iter()
                .all(|capability| requested.capabilities.contains(capability))
    }
}

impl Default for DebuggerMetadataCapabilityManifest {
    fn default() -> Self {
        Self::empty()
    }
}

/// Availability is per target and protocol generation. `Planned` never grants
/// a caller permission to invoke a feature; it exists so clients can render a
/// truthful disabled state without guessing from another browser's debugger.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DebuggerCapabilityState {
    Available,
    Planned,
    Unsupported,
}

/// One bounded, host-controlled capability report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DebuggerCapabilityReport {
    pub capability: DebuggerCapability,
    pub state: DebuggerCapabilityState,
    /// A stable, host-controlled reason or implementation label. It MUST NOT
    /// contain script source, runtime values, or an arbitrary thrown value.
    pub detail: String,
}

/// The discovery document for one exact realm generation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DebuggerCapabilities {
    pub protocol_version: u32,
    pub realm: DebuggerPageRealm,
    pub reports: Vec<DebuggerCapabilityReport>,
    pub max_stack_frames: u32,
    pub max_scope_bindings: u32,
    pub max_value_preview_bytes: u32,
    /// Maximum instruction boundaries returned by one `ListSafePoints`
    /// operation. This limit does not grant a caller source or bytecode.
    pub max_safe_points_per_program: u32,
    /// Maximum exact breakpoint records retained for one page realm. This
    /// does not grant a caller pause, execution, or runtime-value authority.
    pub max_breakpoints_per_realm: u32,
}

/// A core-local authorization for the metadata capabilities granted by one
/// successfully negotiated debugger `Hello`. It intentionally has no public
/// constructor and is not serializable: it is an implementation guard for a
/// future core/host dispatcher, never a client-supplied wire token.
#[derive(Debug, Clone)]
pub struct DebuggerMetadataSessionAuthorization {
    granted: DebuggerMetadataCapabilityManifest,
    /// Bounded per-stream receipts for opaque metadata handles emitted by the
    /// public inventory operation. A handle must not become a summary or
    /// source-inventory target merely because its numeric fields are guessed.
    observed_metadata_identities: Arc<Mutex<BTreeSet<DebuggerMetadataIdentity>>>,
    /// Bounded per-stream receipts for source IDs emitted by the public
    /// source-inventory operation. This prevents provenance from accepting a
    /// guessed numeric ID as an independent content-oracle target.
    observed_source_identities: Arc<Mutex<BTreeSet<DebuggerMetadataSourceIdentity>>>,
    /// Bounded per-stream receipts for type IDs emitted by type inventory.
    /// This remains local and source-free so a future type display operation
    /// cannot turn a guessed ID into a child metadata probe.
    observed_type_identities: Arc<Mutex<BTreeSet<DebuggerMetadataTypeIdentity>>>,
    /// Bounded per-stream receipts for symbol IDs emitted by symbol inventory.
    /// Kept payload-free now so a future symbol read cannot turn a guessed ID
    /// into a child metadata probe.
    observed_symbol_identities: Arc<Mutex<BTreeSet<DebuggerMetadataSymbolIdentity>>>,
    /// Bounded per-stream receipts for contract IDs emitted by contract
    /// inventory. Kept payload-free so a future plan or validation operation
    /// cannot turn a guessed ID into a child metadata probe.
    observed_contract_identities: Arc<Mutex<BTreeSet<DebuggerMetadataContractIdentity>>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct DebuggerMetadataIdentity {
    browser_context_id: u64,
    tab_id: u64,
    realm_generation: u64,
    program_handle: u64,
    program_generation: u64,
    metadata_handle: u64,
    metadata_generation: u64,
}

impl From<DebuggerStaticMetadataHandle> for DebuggerMetadataIdentity {
    fn from(metadata: DebuggerStaticMetadataHandle) -> Self {
        Self {
            browser_context_id: metadata.program.realm.browser_context_id,
            tab_id: metadata.program.realm.tab_id,
            realm_generation: metadata.program.realm.realm_generation,
            program_handle: metadata.program.program_handle,
            program_generation: metadata.program.program_generation,
            metadata_handle: metadata.metadata_handle,
            metadata_generation: metadata.metadata_generation,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct DebuggerMetadataSourceIdentity {
    browser_context_id: u64,
    tab_id: u64,
    realm_generation: u64,
    program_handle: u64,
    program_generation: u64,
    metadata_handle: u64,
    metadata_generation: u64,
    source_id: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct DebuggerMetadataTypeIdentity {
    browser_context_id: u64,
    tab_id: u64,
    realm_generation: u64,
    program_handle: u64,
    program_generation: u64,
    metadata_handle: u64,
    metadata_generation: u64,
    type_id: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct DebuggerMetadataSymbolIdentity {
    browser_context_id: u64,
    tab_id: u64,
    realm_generation: u64,
    program_handle: u64,
    program_generation: u64,
    metadata_handle: u64,
    metadata_generation: u64,
    symbol_id: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct DebuggerMetadataContractIdentity {
    browser_context_id: u64,
    tab_id: u64,
    realm_generation: u64,
    program_handle: u64,
    program_generation: u64,
    metadata_handle: u64,
    metadata_generation: u64,
    contract_id: u32,
}

impl From<DebuggerStaticMetadataTypeId> for DebuggerMetadataTypeIdentity {
    fn from(static_type: DebuggerStaticMetadataTypeId) -> Self {
        Self {
            browser_context_id: static_type.metadata.program.realm.browser_context_id,
            tab_id: static_type.metadata.program.realm.tab_id,
            realm_generation: static_type.metadata.program.realm.realm_generation,
            program_handle: static_type.metadata.program.program_handle,
            program_generation: static_type.metadata.program.program_generation,
            metadata_handle: static_type.metadata.metadata_handle,
            metadata_generation: static_type.metadata.metadata_generation,
            type_id: static_type.type_id,
        }
    }
}

impl From<DebuggerStaticMetadataSymbolId> for DebuggerMetadataSymbolIdentity {
    fn from(symbol: DebuggerStaticMetadataSymbolId) -> Self {
        Self {
            browser_context_id: symbol.metadata.program.realm.browser_context_id,
            tab_id: symbol.metadata.program.realm.tab_id,
            realm_generation: symbol.metadata.program.realm.realm_generation,
            program_handle: symbol.metadata.program.program_handle,
            program_generation: symbol.metadata.program.program_generation,
            metadata_handle: symbol.metadata.metadata_handle,
            metadata_generation: symbol.metadata.metadata_generation,
            symbol_id: symbol.symbol_id,
        }
    }
}

impl From<DebuggerStaticMetadataContractId> for DebuggerMetadataContractIdentity {
    fn from(contract: DebuggerStaticMetadataContractId) -> Self {
        Self {
            browser_context_id: contract.metadata.program.realm.browser_context_id,
            tab_id: contract.metadata.program.realm.tab_id,
            realm_generation: contract.metadata.program.realm.realm_generation,
            program_handle: contract.metadata.program.program_handle,
            program_generation: contract.metadata.program.program_generation,
            metadata_handle: contract.metadata.metadata_handle,
            metadata_generation: contract.metadata.metadata_generation,
            contract_id: contract.contract_id,
        }
    }
}

impl From<DebuggerStaticMetadataSourceId> for DebuggerMetadataSourceIdentity {
    fn from(source: DebuggerStaticMetadataSourceId) -> Self {
        Self {
            browser_context_id: source.metadata.program.realm.browser_context_id,
            tab_id: source.metadata.program.realm.tab_id,
            realm_generation: source.metadata.program.realm.realm_generation,
            program_handle: source.metadata.program.program_handle,
            program_generation: source.metadata.program.program_generation,
            metadata_handle: source.metadata.metadata_handle,
            metadata_generation: source.metadata.metadata_generation,
            source_id: source.source_id,
        }
    }
}

/// One metadata session may remember at most 4,096 opaque parent handles.
/// This fixed cap avoids turning a long-lived debugger stream into an
/// unbounded metadata-receipt cache; a caller can reconnect after it consumes
/// the budget.
pub const DEBUGGER_METADATA_SESSION_MAX_OBSERVED_METADATA_IDENTITIES: usize = 4_096;

/// One metadata session may remember at most one full source-ID page. This
/// fixed cap avoids turning a long-lived debugger stream into an unbounded
/// receipt cache; a caller can reconnect after it consumes the budget.
pub const DEBUGGER_METADATA_SESSION_MAX_OBSERVED_SOURCE_IDENTITIES: usize = 4_096;

/// One metadata session may remember at most one full type-ID page. This is
/// separate from source receipts so a future type-display capability cannot
/// obtain an unbounded guessed-ID oracle from a long-lived stream.
pub const DEBUGGER_METADATA_SESSION_MAX_OBSERVED_TYPE_IDENTITIES: usize = 4_096;

/// One metadata session may remember at most one complete symbol-ID inventory
/// at the public symbol count limit. The fixed budget preserves a receipt
/// boundary without making a long-lived stream an unbounded cache.
pub const DEBUGGER_METADATA_SESSION_MAX_OBSERVED_SYMBOL_IDENTITIES: usize = 65_536;

/// One metadata session may remember at most one complete contract-ID
/// inventory at the public contract count limit. The fixed budget preserves a
/// receipt boundary without making a long-lived stream an unbounded cache.
pub const DEBUGGER_METADATA_SESSION_MAX_OBSERVED_CONTRACT_IDENTITIES: usize = 65_536;

impl DebuggerMetadataSessionAuthorization {
    /// Whether this session negotiated one exact metadata capability. A
    /// handler must also require the per-realm authorization below; session
    /// negotiation alone does not prove a realm can currently supply data.
    pub fn permits(&self, capability: DebuggerMetadataCapability) -> bool {
        self.granted.contains(capability)
    }

    /// Records the exact handles that static metadata inventory actually
    /// returned on this stream. The insertion is atomic with respect to the
    /// fixed session budget and stores no metadata payload.
    pub fn observe_metadata(&self, metadata: &[DebuggerStaticMetadataHandle]) -> bool {
        let Ok(mut observed) = self.observed_metadata_identities.lock() else {
            return false;
        };
        let new_count = metadata
            .iter()
            .map(|metadata| DebuggerMetadataIdentity::from(*metadata))
            .filter(|metadata| !observed.contains(metadata))
            .collect::<BTreeSet<_>>()
            .len();
        if observed.len().saturating_add(new_count)
            > DEBUGGER_METADATA_SESSION_MAX_OBSERVED_METADATA_IDENTITIES
        {
            return false;
        }
        observed.extend(metadata.iter().copied().map(DebuggerMetadataIdentity::from));
        true
    }

    /// Whether this exact parent handle was emitted by static metadata
    /// inventory on this session. Failure to access the local receipt store
    /// fails closed.
    pub fn observed_metadata(&self, metadata: DebuggerStaticMetadataHandle) -> bool {
        self.observed_metadata_identities
            .lock()
            .is_ok_and(|observed| observed.contains(&metadata.into()))
    }

    /// Records the exact IDs that the inventory operation actually returned
    /// on this stream. The insertion is atomic with respect to the fixed
    /// session budget and stores no source/module/digest payload.
    pub fn observe_sources(&self, sources: &[DebuggerStaticMetadataSourceId]) -> bool {
        let Ok(mut observed) = self.observed_source_identities.lock() else {
            return false;
        };
        let new_count = sources
            .iter()
            .map(|source| DebuggerMetadataSourceIdentity::from(*source))
            .filter(|source| !observed.contains(source))
            .collect::<BTreeSet<_>>()
            .len();
        if observed.len().saturating_add(new_count)
            > DEBUGGER_METADATA_SESSION_MAX_OBSERVED_SOURCE_IDENTITIES
        {
            return false;
        }
        observed.extend(
            sources
                .iter()
                .copied()
                .map(DebuggerMetadataSourceIdentity::from),
        );
        true
    }

    /// Whether this exact source ID was emitted by source inventory on this
    /// session. Failure to access the local receipt store fails closed.
    pub fn observed_source(&self, source: DebuggerStaticMetadataSourceId) -> bool {
        self.observed_source_identities
            .lock()
            .is_ok_and(|observed| observed.contains(&source.into()))
    }

    /// Records exact type IDs emitted by type inventory on this stream.
    pub fn observe_types(&self, types: &[DebuggerStaticMetadataTypeId]) -> bool {
        let Ok(mut observed) = self.observed_type_identities.lock() else {
            return false;
        };
        let new_count = types
            .iter()
            .copied()
            .map(DebuggerMetadataTypeIdentity::from)
            .filter(|static_type| !observed.contains(static_type))
            .collect::<BTreeSet<_>>()
            .len();
        if observed.len().saturating_add(new_count)
            > DEBUGGER_METADATA_SESSION_MAX_OBSERVED_TYPE_IDENTITIES
        {
            return false;
        }
        observed.extend(
            types
                .iter()
                .copied()
                .map(DebuggerMetadataTypeIdentity::from),
        );
        true
    }

    /// Whether this exact type ID was emitted by type inventory on this
    /// session. Kept now as the future static type display's receipt boundary.
    pub fn observed_type(&self, static_type: DebuggerStaticMetadataTypeId) -> bool {
        self.observed_type_identities
            .lock()
            .is_ok_and(|observed| observed.contains(&static_type.into()))
    }

    /// Records exact symbol IDs emitted by symbol inventory on this stream.
    pub fn observe_symbols(&self, symbols: &[DebuggerStaticMetadataSymbolId]) -> bool {
        let Ok(mut observed) = self.observed_symbol_identities.lock() else {
            return false;
        };
        let new_count = symbols
            .iter()
            .copied()
            .map(DebuggerMetadataSymbolIdentity::from)
            .filter(|symbol| !observed.contains(symbol))
            .collect::<BTreeSet<_>>()
            .len();
        if observed.len().saturating_add(new_count)
            > DEBUGGER_METADATA_SESSION_MAX_OBSERVED_SYMBOL_IDENTITIES
        {
            return false;
        }
        observed.extend(
            symbols
                .iter()
                .copied()
                .map(DebuggerMetadataSymbolIdentity::from),
        );
        true
    }

    /// Whether this exact symbol ID was emitted by symbol inventory on this
    /// session. This establishes the opaque boundary for future symbol reads.
    pub fn observed_symbol(&self, symbol: DebuggerStaticMetadataSymbolId) -> bool {
        self.observed_symbol_identities
            .lock()
            .is_ok_and(|observed| observed.contains(&symbol.into()))
    }

    /// Records exact contract IDs emitted by contract inventory on this
    /// stream. The local receipt establishes a future plan/validation boundary.
    pub fn observe_contracts(&self, contracts: &[DebuggerStaticMetadataContractId]) -> bool {
        let Ok(mut observed) = self.observed_contract_identities.lock() else {
            return false;
        };
        let new_count = contracts
            .iter()
            .copied()
            .map(DebuggerMetadataContractIdentity::from)
            .filter(|contract| !observed.contains(contract))
            .collect::<BTreeSet<_>>()
            .len();
        if observed.len().saturating_add(new_count)
            > DEBUGGER_METADATA_SESSION_MAX_OBSERVED_CONTRACT_IDENTITIES
        {
            return false;
        }
        observed.extend(
            contracts
                .iter()
                .copied()
                .map(DebuggerMetadataContractIdentity::from),
        );
        true
    }

    /// Whether this exact contract ID was emitted by contract inventory on
    /// this session. This establishes the opaque boundary for future plan or
    /// validation reads.
    pub fn observed_contract(&self, contract: DebuggerStaticMetadataContractId) -> bool {
        self.observed_contract_identities
            .lock()
            .is_ok_and(|observed| observed.contains(&contract.into()))
    }
}

/// Reconstructs the core-local session authorization from the exact `Hello`
/// request and `HelloAck` reply a transport just exchanged. The caller must
/// invoke this only on a reply emitted by [`negotiate`], retain it per stream,
/// and discard it when that stream closes. A malformed reply, an unsupported
/// protocol version, or a grant not requested by the client fails closed.
pub fn metadata_session_authorization(
    request: &DebuggerRequest,
    reply: &DebuggerReply,
) -> Option<DebuggerMetadataSessionAuthorization> {
    let DebuggerRequest::Hello {
        protocol_version,
        requested_metadata_capabilities,
    } = request
    else {
        return None;
    };
    let DebuggerReply::HelloAck {
        protocol_version: acknowledged_version,
        granted_metadata_capabilities,
    } = reply
    else {
        return None;
    };
    if *protocol_version != DEBUGGER_PROTOCOL_VERSION
        || *acknowledged_version != DEBUGGER_PROTOCOL_VERSION
        || !requested_metadata_capabilities.is_well_formed()
        || !granted_metadata_capabilities.is_subset_of(requested_metadata_capabilities)
    {
        return None;
    }

    Some(DebuggerMetadataSessionAuthorization {
        granted: granted_metadata_capabilities.clone(),
        observed_metadata_identities: Arc::new(Mutex::new(BTreeSet::new())),
        observed_source_identities: Arc::new(Mutex::new(BTreeSet::new())),
        observed_type_identities: Arc::new(Mutex::new(BTreeSet::new())),
        observed_symbol_identities: Arc::new(Mutex::new(BTreeSet::new())),
        observed_contract_identities: Arc::new(Mutex::new(BTreeSet::new())),
    })
}

/// A core-local authorization derived from a negotiated session and one exact
/// live-realm capability report. It intentionally has no public constructor
/// and is not serializable: it is an implementation guard for a future
/// core/host dispatcher, never a client-supplied wire token. The dispatcher
/// must additionally verify that the realm remains live before every
/// operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DebuggerMetadataAuthorization {
    realm: DebuggerPageRealm,
    capability: DebuggerMetadataCapability,
}

impl DebuggerMetadataAuthorization {
    /// Checks that a future metadata operation uses precisely the granted
    /// capability and the same realm generation that discovery authorized.
    pub fn permits(self, realm: DebuggerPageRealm, capability: DebuggerMetadataCapability) -> bool {
        self.realm == realm && self.capability == capability && realm.is_well_formed()
    }
}

impl DebuggerCapabilities {
    /// Returns a core-local authorization only for one exact, unambiguous
    /// `Available` report for the requested metadata capability after that
    /// capability was granted for this exact session.
    ///
    /// Missing reports, duplicate reports, `Planned`/`Unsupported` states,
    /// malformed realm identities, and replies from another protocol revision
    /// all deny by default. Future metadata request handlers must retain this
    /// session grant after `Hello`, retain the resulting authorization after
    /// `DescribeCapabilities`, require
    /// [`DebuggerMetadataAuthorization::permits`] for their target, and still
    /// verify the live realm at dispatch time.
    pub fn authorize_metadata(
        &self,
        session: &DebuggerMetadataSessionAuthorization,
        capability: DebuggerMetadataCapability,
    ) -> Option<DebuggerMetadataAuthorization> {
        if self.protocol_version != DEBUGGER_PROTOCOL_VERSION
            || !self.realm.is_well_formed()
            || !session.permits(capability)
        {
            return None;
        }

        let required = capability.debugger_capability()?;
        let mut reports = self
            .reports
            .iter()
            .filter(|report| report.capability == required);
        let report = reports.next()?;
        if reports.next().is_some() || report.state != DebuggerCapabilityState::Available {
            return None;
        }

        Some(DebuggerMetadataAuthorization {
            realm: self.realm,
            capability,
        })
    }
}

/// Source-free state of one native-debugger controlled declaration. A paused
/// state identifies only an already-validated opaque instruction boundary;
/// it never carries source text, bytecode, a stack, a scope, or a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DebuggerExecutionState {
    Pending,
    Paused { safe_point: DebuggerSafePoint },
    Resuming,
    Completed,
}

/// Core's requests to the out-of-process BlueJS debugger host.
///
/// Command families are deliberately added only with real native behavior.
/// The first non-discovery family resolves opaque programs and exact
/// compiler-verified instruction boundaries. The second is exact, bounded
/// breakpoint configuration. The root-entry arm/resume operations are
/// separately named because normal configuration must not be mistaken for a
/// VM interruption hook.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DebuggerRequest {
    Hello {
        protocol_version: u32,
        /// The exact canonical static-metadata capabilities this debugger
        /// client asks the core to consider for this transport session.
        /// Requesting one grants nothing; the core replies with only its
        /// policy intersection in [`DebuggerReply::HelloAck`].
        requested_metadata_capabilities: DebuggerMetadataCapabilityManifest,
    },
    /// Lists bounded, currently loaded page realm identities. The reply carries
    /// no URL, source, program, bytecode, or runtime object; clients use the
    /// returned generation only to make a subsequent target-bound discovery
    /// request.
    ListPageRealms,
    DescribeCapabilities {
        realm: DebuggerPageRealm,
    },
    /// Lists opaque program identities currently retained by one exact page
    /// realm. It never returns a source identity, bytecode, VM object, or
    /// completion value.
    ListPrograms {
        realm: DebuggerPageRealm,
    },
    /// Lists a bounded source-free inventory of core-minted static-metadata
    /// handles for one exact live program. It exposes neither metadata nor a
    /// way to dereference a handle. Dispatch requires both the session grant
    /// negotiated in `Hello` and the exact realm's advertised capability.
    ListStaticMetadata {
        program: DebuggerProgram,
    },
    /// Returns a bounded source-free summary for one exact opaque handle
    /// obtained from [`Self::ListStaticMetadata`]. This is not a metadata
    /// record read or dereference operation.
    DescribeStaticMetadata {
        metadata: DebuggerStaticMetadataHandle,
    },
    /// Returns one separately authorized, aggregate summary of the verified
    /// direct BlueTS-to-BlueJS lowering map for an exact inventory handle.
    /// It never exposes source spans, map entries, AST nodes, code-unit IDs,
    /// bytecode offsets, or a generic map-read operation.
    DescribeStaticMetadataLoweringSummary {
        metadata: DebuggerStaticMetadataHandle,
    },
    /// Lists bounded compiler-minted source-record identities for one exact
    /// metadata attachment. It is not a source/provenance read operation.
    ListStaticMetadataSources {
        metadata: DebuggerStaticMetadataHandle,
    },
    /// Lists only compiler-minted type-record identities for one exact
    /// metadata attachment. It is not a type display or metadata-record read.
    ListStaticMetadataTypes {
        metadata: DebuggerStaticMetadataHandle,
    },
    /// Describes one type ID previously returned by
    /// [`Self::ListStaticMetadataTypes`]. This separately authorized
    /// operation returns only a bounded compiler-produced type display.
    DescribeStaticMetadataType {
        static_type: DebuggerStaticMetadataTypeId,
    },
    /// Lists only compiler-minted symbol-record identities for one exact
    /// metadata attachment. It is not a symbol name/span/type or record read.
    ListStaticMetadataSymbols {
        metadata: DebuggerStaticMetadataHandle,
    },
    /// Describes one symbol ID previously returned by
    /// [`Self::ListStaticMetadataSymbols`]. This separately authorized
    /// operation returns only a bounded compiler-produced symbol display.
    DescribeStaticMetadataSymbol {
        symbol: DebuggerStaticMetadataSymbolId,
    },
    /// Describes one half-open byte range for a symbol previously returned by
    /// [`Self::ListStaticMetadataSymbols`]. The reply requires a separately
    /// receipted source ID and carries neither text nor module identity.
    DescribeStaticMetadataSymbolLocation {
        target: DebuggerStaticMetadataSymbolLocationTarget,
    },
    /// Verifies one compiler-produced symbol-to-type relation. Both IDs must
    /// have crossed this debugger stream's separate opaque inventories.
    DescribeStaticMetadataSymbolType {
        target: DebuggerStaticMetadataSymbolType,
    },
    /// Lists only compiler-minted contract identities for one exact metadata
    /// attachment. It is not a contract name/span/plan/validation read.
    ListStaticMetadataContracts {
        metadata: DebuggerStaticMetadataHandle,
    },
    /// Describes one contract ID previously returned by
    /// [`Self::ListStaticMetadataContracts`]. This separately authorized
    /// operation returns only a bounded compiler-produced contract display.
    DescribeStaticMetadataContract {
        contract: DebuggerStaticMetadataContractId,
    },
    /// Validates a caller-provided data-only value against one contract ID
    /// previously returned by [`Self::ListStaticMetadataContracts`]. The
    /// independently authorized reply contains only a boolean; it never
    /// echoes input or exposes contract plan/failure details.
    ValidateStaticMetadataContract {
        contract: DebuggerStaticMetadataContractId,
        value: CompilerContractValue,
    },
    /// Describes one source ID previously returned by
    /// [`Self::ListStaticMetadataSources`]. This separately authorized
    /// operation returns compiler-canonical module identity and a labeled
    /// SHA-256 digest only; it cannot read source text.
    DescribeStaticMetadataSource {
        source: DebuggerStaticMetadataSourceId,
    },
    /// Lists bounded, compiler-verified instruction boundaries for one exact
    /// live program generation. A caller must not infer or substitute offsets.
    ListSafePoints {
        program: DebuggerProgram,
    },
    /// Checks one supplied location against the exact current BlueJS program
    /// generation. This operation does not execute or pause the program.
    ValidateSafePoint {
        safe_point: DebuggerSafePoint,
    },
    /// Stores one exact, already compiler-verified breakpoint record for the
    /// current program generation. It does not execute or interrupt a VM.
    SetBreakpoint {
        safe_point: DebuggerSafePoint,
    },
    /// Arms one compiler-verified root-code-unit entry boundary for a program
    /// that has been admitted but has not entered BlueJS execution. The host
    /// rejects any non-entry safe point or program that is no longer pending.
    ArmEntryBreakpoint {
        safe_point: DebuggerSafePoint,
    },
    /// Arms one exact root-code-unit safe point for a pending classic script.
    /// Unlike `SetBreakpoint`, this starts the script and retains a real
    /// BlueJS continuation when the requested non-entry boundary is reached.
    /// It rejects modules and child code units rather than claiming generic
    /// interpreter interruption.
    ArmRootSafePointBreakpoint {
        safe_point: DebuggerSafePoint,
    },
    /// Lists only exact breakpoint records currently retained by one live
    /// realm. The records carry no source, bytecode, VM object, or value.
    ListBreakpoints {
        realm: DebuggerPageRealm,
    },
    /// Removes one exact current breakpoint record. Removal is idempotent,
    /// but its target must still name a live compiler-verified boundary.
    ClearBreakpoint {
        safe_point: DebuggerSafePoint,
    },
    /// Reads source-free pending/paused/resuming/completed state for one
    /// exact program generation in the opt-in entry-pause scheduler.
    GetExecutionState {
        program: DebuggerProgram,
    },
    /// Allows a currently entry-paused program to enter the ordinary BlueJS
    /// VM. It cannot resume a non-paused program or inject a value/exception.
    ResumeExecution {
        program: DebuggerProgram,
    },
    /// Catch-all for a newer request variant. Like the frontend protocol, a
    /// receiver preserves connection framing and replies with `Unsupported`
    /// rather than deserializing an unknown command as an unrelated request.
    #[serde(other)]
    Unknown,
}

/// BlueJS host replies on the debugger channel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DebuggerReply {
    HelloAck {
        protocol_version: u32,
        /// The canonical intersection of the client's requested metadata
        /// capabilities and the core-owned allow policy for this one stream.
        /// An empty set is a successful handshake with no metadata authority.
        granted_metadata_capabilities: DebuggerMetadataCapabilityManifest,
    },
    /// Reply to [`DebuggerRequest::ListPageRealms`].
    PageRealms(Vec<DebuggerPageRealm>),
    Capabilities(DebuggerCapabilities),
    Programs(Vec<DebuggerProgram>),
    /// Reply to [`DebuggerRequest::ListStaticMetadata`]. Every handle is
    /// opaque and bound to the exact program generation supplied by the
    /// request; this is not a metadata payload or a read capability.
    StaticMetadata(Vec<DebuggerStaticMetadataHandle>),
    /// Reply to [`DebuggerRequest::DescribeStaticMetadata`]. The summary is
    /// bounded and source-free; individual metadata records remain private.
    StaticMetadataSummary(DebuggerStaticMetadataSummary),
    /// Reply to [`DebuggerRequest::DescribeStaticMetadataLoweringSummary`].
    /// The result is parent-bound and contains only fixed ABI/fingerprint
    /// evidence and an aggregate verified-entry count.
    StaticMetadataLoweringSummary(Box<DebuggerStaticMetadataLoweringSummary>),
    /// Reply to [`DebuggerRequest::ListStaticMetadataSources`]. IDs are
    /// parent-handle-bound and contain no source/provenance payload.
    StaticMetadataSources(Vec<DebuggerStaticMetadataSourceId>),
    /// Reply to [`DebuggerRequest::ListStaticMetadataTypes`]. IDs remain
    /// parent-handle-bound and contain no type display or record payload.
    StaticMetadataTypes(Vec<DebuggerStaticMetadataTypeId>),
    /// Reply to [`DebuggerRequest::DescribeStaticMetadataType`]. The type
    /// display remains parent-bound and source-text-free.
    StaticMetadataType(DebuggerStaticMetadataTypeDisplay),
    /// Reply to [`DebuggerRequest::ListStaticMetadataSymbols`]. IDs remain
    /// parent-bound and contain no symbol name, span, type, or record payload.
    StaticMetadataSymbols(Vec<DebuggerStaticMetadataSymbolId>),
    /// Reply to [`DebuggerRequest::DescribeStaticMetadataSymbol`]. The symbol
    /// display remains parent-bound, receipted, and source-text-free.
    StaticMetadataSymbol(DebuggerStaticMetadataSymbolDisplay),
    /// Reply to [`DebuggerRequest::DescribeStaticMetadataSymbolLocation`].
    /// The location is parent-bound, receipted, source-text-free, and does
    /// not include a module identity or a bytecode/source-map translation.
    StaticMetadataSymbolLocation(DebuggerStaticMetadataSymbolLocation),
    /// Reply to [`DebuggerRequest::DescribeStaticMetadataSymbolType`]. It
    /// repeats only the two previously receipted opaque IDs.
    StaticMetadataSymbolType(DebuggerStaticMetadataSymbolType),
    /// Reply to [`DebuggerRequest::ListStaticMetadataContracts`]. IDs remain
    /// parent-bound and contain no contract name, span, plan, or validation
    /// payload.
    StaticMetadataContracts(Vec<DebuggerStaticMetadataContractId>),
    /// Reply to [`DebuggerRequest::DescribeStaticMetadataContract`]. The
    /// contract display remains parent-bound, receipted, and source-text-free.
    StaticMetadataContract(DebuggerStaticMetadataContractDisplay),
    /// Reply to [`DebuggerRequest::ValidateStaticMetadataContract`]. The
    /// result remains parent-bound and receipted, and contains no caller data
    /// or contract-plan/failure detail.
    StaticMetadataContractValidation(DebuggerStaticMetadataContractValidation),
    /// Reply to [`DebuggerRequest::DescribeStaticMetadataSource`]. This is a
    /// bounded owner-authorized provenance disclosure, never source text.
    StaticMetadataSourceProvenance(DebuggerStaticMetadataSourceProvenance),
    SafePoints(Vec<DebuggerSafePoint>),
    SafePointValidated {
        safe_point: DebuggerSafePoint,
    },
    BreakpointSet {
        safe_point: DebuggerSafePoint,
    },
    BreakpointArmed {
        safe_point: DebuggerSafePoint,
    },
    RootSafePointBreakpointArmed {
        safe_point: DebuggerSafePoint,
    },
    Breakpoints(Vec<DebuggerSafePoint>),
    BreakpointCleared {
        safe_point: DebuggerSafePoint,
        was_present: bool,
    },
    ExecutionState {
        program: DebuggerProgram,
        state: DebuggerExecutionState,
    },
    ExecutionResumed {
        program: DebuggerProgram,
    },
    Unsupported {
        operation: String,
        reason: String,
    },
    Error {
        code: DebuggerErrorCode,
        message: String,
    },
}

/// Stable error categories for native debugger implementations and their
/// adapters. `message` remains diagnostic prose; clients branch on this code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DebuggerErrorCode {
    ProtocolVersion,
    InvalidCapabilityManifest,
    InvalidTarget,
    StaleRealm,
    StaleProgram,
    InvalidSafePoint,
    CapabilityUnavailable,
    InvalidExecutionState,
    ResourceLimit,
}

/// Builds the only valid reply to the connection's first request. A caller
/// must still reject any non-`Hello` first request before it dispatches the
/// connection to a realm owner.
pub fn negotiate(
    request: &DebuggerRequest,
    allowed_metadata_capabilities: &DebuggerMetadataCapabilityManifest,
) -> DebuggerReply {
    match request {
        DebuggerRequest::Hello {
            protocol_version,
            requested_metadata_capabilities,
        } if *protocol_version == DEBUGGER_PROTOCOL_VERSION
            && requested_metadata_capabilities.is_well_formed()
            && allowed_metadata_capabilities.is_well_formed() =>
        {
            DebuggerReply::HelloAck {
                protocol_version: DEBUGGER_PROTOCOL_VERSION,
                granted_metadata_capabilities: allowed_metadata_capabilities
                    .intersection(requested_metadata_capabilities),
            }
        }
        DebuggerRequest::Hello {
            protocol_version, ..
        } if *protocol_version == DEBUGGER_PROTOCOL_VERSION => DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidCapabilityManifest,
            message: "debugger metadata capability manifest is malformed".to_string(),
        },
        DebuggerRequest::Hello { .. } => DebuggerReply::Error {
            code: DebuggerErrorCode::ProtocolVersion,
            message: "unsupported debugger protocol version".to_string(),
        },
        DebuggerRequest::ListPageRealms
        | DebuggerRequest::DescribeCapabilities { .. }
        | DebuggerRequest::ListPrograms { .. }
        | DebuggerRequest::ListStaticMetadata { .. }
        | DebuggerRequest::DescribeStaticMetadata { .. }
        | DebuggerRequest::DescribeStaticMetadataLoweringSummary { .. }
        | DebuggerRequest::ListStaticMetadataSources { .. }
        | DebuggerRequest::ListStaticMetadataTypes { .. }
        | DebuggerRequest::DescribeStaticMetadataType { .. }
        | DebuggerRequest::ListStaticMetadataSymbols { .. }
        | DebuggerRequest::DescribeStaticMetadataSymbol { .. }
        | DebuggerRequest::DescribeStaticMetadataSymbolLocation { .. }
        | DebuggerRequest::DescribeStaticMetadataSymbolType { .. }
        | DebuggerRequest::ListStaticMetadataContracts { .. }
        | DebuggerRequest::DescribeStaticMetadataContract { .. }
        | DebuggerRequest::ValidateStaticMetadataContract { .. }
        | DebuggerRequest::DescribeStaticMetadataSource { .. }
        | DebuggerRequest::ListSafePoints { .. }
        | DebuggerRequest::ValidateSafePoint { .. }
        | DebuggerRequest::SetBreakpoint { .. }
        | DebuggerRequest::ArmEntryBreakpoint { .. }
        | DebuggerRequest::ArmRootSafePointBreakpoint { .. }
        | DebuggerRequest::ListBreakpoints { .. }
        | DebuggerRequest::ClearBreakpoint { .. }
        | DebuggerRequest::GetExecutionState { .. }
        | DebuggerRequest::ResumeExecution { .. }
        | DebuggerRequest::Unknown => DebuggerReply::Error {
            code: DebuggerErrorCode::ProtocolVersion,
            message: "debugger protocol requires Hello as its first request".to_string(),
        },
    }
}

pub fn write_debugger_request<W: Write>(
    writer: &mut W,
    request: &DebuggerRequest,
) -> io::Result<()> {
    crate::write_framed(writer, request)
}

pub fn read_debugger_request<R: Read>(reader: &mut R) -> io::Result<DebuggerRequest> {
    let bytes = crate::read_frame_bytes(reader)?;
    serde_json::from_slice(&bytes).map_err(io::Error::other)
}

pub fn write_debugger_reply<W: Write>(writer: &mut W, reply: &DebuggerReply) -> io::Result<()> {
    crate::write_framed(writer, reply)
}

pub fn read_debugger_reply<R: Read>(reader: &mut R) -> io::Result<DebuggerReply> {
    let bytes = crate::read_frame_bytes(reader)?;
    serde_json::from_slice(&bytes).map_err(io::Error::other)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::net::UnixStream;

    fn realm() -> DebuggerPageRealm {
        DebuggerPageRealm {
            browser_context_id: 1,
            tab_id: 7,
            realm_generation: 3,
        }
    }

    fn capabilities(reports: Vec<DebuggerCapabilityReport>) -> DebuggerCapabilities {
        DebuggerCapabilities {
            protocol_version: DEBUGGER_PROTOCOL_VERSION,
            realm: realm(),
            reports,
            max_stack_frames: 64,
            max_scope_bindings: 256,
            max_value_preview_bytes: 4_096,
            max_safe_points_per_program: 4_096,
            max_breakpoints_per_realm: 256,
        }
    }

    fn capability_report(
        capability: DebuggerCapability,
        state: DebuggerCapabilityState,
    ) -> DebuggerCapabilityReport {
        DebuggerCapabilityReport {
            capability,
            state,
            detail: "test capability report".to_string(),
        }
    }

    #[test]
    fn request_and_reply_round_trip_on_a_real_socket() {
        for request in [
            DebuggerRequest::Hello {
                protocol_version: DEBUGGER_PROTOCOL_VERSION,
                requested_metadata_capabilities: DebuggerMetadataCapabilityManifest::empty(),
            },
            DebuggerRequest::ListPageRealms,
            DebuggerRequest::DescribeCapabilities { realm: realm() },
            DebuggerRequest::ListPrograms { realm: realm() },
            DebuggerRequest::ListStaticMetadata {
                program: DebuggerProgram {
                    realm: realm(),
                    program_handle: 12,
                    program_generation: 5,
                },
            },
            DebuggerRequest::DescribeStaticMetadata {
                metadata: DebuggerStaticMetadataHandle {
                    program: DebuggerProgram {
                        realm: realm(),
                        program_handle: 12,
                        program_generation: 5,
                    },
                    metadata_handle: 24,
                    metadata_generation: 7,
                },
            },
            DebuggerRequest::DescribeStaticMetadataLoweringSummary {
                metadata: DebuggerStaticMetadataHandle {
                    program: DebuggerProgram {
                        realm: realm(),
                        program_handle: 12,
                        program_generation: 5,
                    },
                    metadata_handle: 24,
                    metadata_generation: 7,
                },
            },
            DebuggerRequest::ListStaticMetadataSources {
                metadata: DebuggerStaticMetadataHandle {
                    program: DebuggerProgram {
                        realm: realm(),
                        program_handle: 12,
                        program_generation: 5,
                    },
                    metadata_handle: 24,
                    metadata_generation: 7,
                },
            },
            DebuggerRequest::ListStaticMetadataTypes {
                metadata: DebuggerStaticMetadataHandle {
                    program: DebuggerProgram {
                        realm: realm(),
                        program_handle: 12,
                        program_generation: 5,
                    },
                    metadata_handle: 24,
                    metadata_generation: 7,
                },
            },
            DebuggerRequest::DescribeStaticMetadataType {
                static_type: DebuggerStaticMetadataTypeId {
                    metadata: DebuggerStaticMetadataHandle {
                        program: DebuggerProgram {
                            realm: realm(),
                            program_handle: 12,
                            program_generation: 5,
                        },
                        metadata_handle: 24,
                        metadata_generation: 7,
                    },
                    type_id: 0,
                },
            },
            DebuggerRequest::ListStaticMetadataSymbols {
                metadata: DebuggerStaticMetadataHandle {
                    program: DebuggerProgram {
                        realm: realm(),
                        program_handle: 12,
                        program_generation: 5,
                    },
                    metadata_handle: 24,
                    metadata_generation: 7,
                },
            },
            DebuggerRequest::DescribeStaticMetadataSymbol {
                symbol: DebuggerStaticMetadataSymbolId {
                    metadata: DebuggerStaticMetadataHandle {
                        program: DebuggerProgram {
                            realm: realm(),
                            program_handle: 12,
                            program_generation: 5,
                        },
                        metadata_handle: 24,
                        metadata_generation: 7,
                    },
                    symbol_id: 0,
                },
            },
            DebuggerRequest::DescribeStaticMetadataSymbolLocation {
                target: DebuggerStaticMetadataSymbolLocationTarget {
                    symbol: DebuggerStaticMetadataSymbolId {
                        metadata: DebuggerStaticMetadataHandle {
                            program: DebuggerProgram {
                                realm: realm(),
                                program_handle: 12,
                                program_generation: 5,
                            },
                            metadata_handle: 24,
                            metadata_generation: 7,
                        },
                        symbol_id: 0,
                    },
                    source: DebuggerStaticMetadataSourceId {
                        metadata: DebuggerStaticMetadataHandle {
                            program: DebuggerProgram {
                                realm: realm(),
                                program_handle: 12,
                                program_generation: 5,
                            },
                            metadata_handle: 24,
                            metadata_generation: 7,
                        },
                        source_id: 0,
                    },
                },
            },
            DebuggerRequest::ListStaticMetadataContracts {
                metadata: DebuggerStaticMetadataHandle {
                    program: DebuggerProgram {
                        realm: realm(),
                        program_handle: 12,
                        program_generation: 5,
                    },
                    metadata_handle: 24,
                    metadata_generation: 7,
                },
            },
            DebuggerRequest::DescribeStaticMetadataContract {
                contract: DebuggerStaticMetadataContractId {
                    metadata: DebuggerStaticMetadataHandle {
                        program: DebuggerProgram {
                            realm: realm(),
                            program_handle: 12,
                            program_generation: 5,
                        },
                        metadata_handle: 24,
                        metadata_generation: 7,
                    },
                    contract_id: 0,
                },
            },
            DebuggerRequest::ValidateStaticMetadataContract {
                contract: DebuggerStaticMetadataContractId {
                    metadata: DebuggerStaticMetadataHandle {
                        program: DebuggerProgram {
                            realm: realm(),
                            program_handle: 12,
                            program_generation: 5,
                        },
                        metadata_handle: 24,
                        metadata_generation: 7,
                    },
                    contract_id: 0,
                },
                value: CompilerContractValue::Object(
                    [("enabled".to_string(), CompilerContractValue::Boolean(true))]
                        .into_iter()
                        .collect(),
                ),
            },
            DebuggerRequest::ListSafePoints {
                program: DebuggerProgram {
                    realm: realm(),
                    program_handle: 12,
                    program_generation: 5,
                },
            },
            DebuggerRequest::ValidateSafePoint {
                safe_point: DebuggerSafePoint {
                    program: DebuggerProgram {
                        realm: realm(),
                        program_handle: 12,
                        program_generation: 5,
                    },
                    code_unit_ordinal: 0,
                    bytecode_offset: 0,
                },
            },
            DebuggerRequest::SetBreakpoint {
                safe_point: DebuggerSafePoint {
                    program: DebuggerProgram {
                        realm: realm(),
                        program_handle: 12,
                        program_generation: 5,
                    },
                    code_unit_ordinal: 0,
                    bytecode_offset: 0,
                },
            },
            DebuggerRequest::ArmEntryBreakpoint {
                safe_point: DebuggerSafePoint {
                    program: DebuggerProgram {
                        realm: realm(),
                        program_handle: 12,
                        program_generation: 5,
                    },
                    code_unit_ordinal: 0,
                    bytecode_offset: 0,
                },
            },
            DebuggerRequest::ArmRootSafePointBreakpoint {
                safe_point: DebuggerSafePoint {
                    program: DebuggerProgram {
                        realm: realm(),
                        program_handle: 12,
                        program_generation: 5,
                    },
                    code_unit_ordinal: 0,
                    bytecode_offset: 5,
                },
            },
            DebuggerRequest::ListBreakpoints { realm: realm() },
            DebuggerRequest::ClearBreakpoint {
                safe_point: DebuggerSafePoint {
                    program: DebuggerProgram {
                        realm: realm(),
                        program_handle: 12,
                        program_generation: 5,
                    },
                    code_unit_ordinal: 0,
                    bytecode_offset: 0,
                },
            },
            DebuggerRequest::GetExecutionState {
                program: DebuggerProgram {
                    realm: realm(),
                    program_handle: 12,
                    program_generation: 5,
                },
            },
            DebuggerRequest::ResumeExecution {
                program: DebuggerProgram {
                    realm: realm(),
                    program_handle: 12,
                    program_generation: 5,
                },
            },
            DebuggerRequest::Unknown,
        ] {
            let (mut sender, mut receiver) = UnixStream::pair().unwrap();
            write_debugger_request(&mut sender, &request).unwrap();
            assert_eq!(read_debugger_request(&mut receiver).unwrap(), request);
        }

        let reply = DebuggerReply::Capabilities(capabilities(vec![
            DebuggerCapabilityReport {
                capability: DebuggerCapability::BreakpointConfiguration,
                state: DebuggerCapabilityState::Available,
                detail: "exact breakpoint configuration is installed".to_string(),
            },
            capability_report(
                DebuggerCapability::StaticMetadataInventory,
                DebuggerCapabilityState::Planned,
            ),
            capability_report(
                DebuggerCapability::StaticMetadataSummary,
                DebuggerCapabilityState::Planned,
            ),
        ]));
        let (mut sender, mut receiver) = UnixStream::pair().unwrap();
        write_debugger_reply(&mut sender, &reply).unwrap();
        assert_eq!(read_debugger_reply(&mut receiver).unwrap(), reply);

        let reply = DebuggerReply::PageRealms(vec![realm()]);
        let (mut sender, mut receiver) = UnixStream::pair().unwrap();
        write_debugger_reply(&mut sender, &reply).unwrap();
        assert_eq!(read_debugger_reply(&mut receiver).unwrap(), reply);

        let program = DebuggerProgram {
            realm: realm(),
            program_handle: 12,
            program_generation: 5,
        };
        let safe_point = DebuggerSafePoint {
            program,
            code_unit_ordinal: 0,
            bytecode_offset: 0,
        };
        for reply in [
            DebuggerReply::Programs(vec![program]),
            DebuggerReply::StaticMetadata(vec![DebuggerStaticMetadataHandle {
                program,
                metadata_handle: 24,
                metadata_generation: 7,
            }]),
            DebuggerReply::StaticMetadataSummary(DebuggerStaticMetadataSummary {
                metadata: DebuggerStaticMetadataHandle {
                    program,
                    metadata_handle: 24,
                    metadata_generation: 7,
                },
                language_version: "blue-ts-0.1".to_string(),
                compiler_options_hash: "0123456789abcdef".to_string(),
                source_count: 1,
                type_count: 2,
                symbol_count: 3,
                contract_count: 4,
            }),
            DebuggerReply::StaticMetadataLoweringSummary(Box::new(
                DebuggerStaticMetadataLoweringSummary {
                    metadata: DebuggerStaticMetadataHandle {
                        program,
                        metadata_handle: 24,
                        metadata_generation: 7,
                    },
                    safe_point_map_abi: "bluejs-safe-point-map-v1".to_string(),
                    program_abi: "bluejs-program-v1".to_string(),
                    source_set_hash: "bts-source-set-0123456789abcdef".to_string(),
                    bound_safe_point_count: 1,
                },
            )),
            DebuggerReply::StaticMetadataSources(vec![DebuggerStaticMetadataSourceId {
                metadata: DebuggerStaticMetadataHandle {
                    program,
                    metadata_handle: 24,
                    metadata_generation: 7,
                },
                source_id: 0,
            }]),
            DebuggerReply::StaticMetadataTypes(vec![DebuggerStaticMetadataTypeId {
                metadata: DebuggerStaticMetadataHandle {
                    program,
                    metadata_handle: 24,
                    metadata_generation: 7,
                },
                type_id: 0,
            }]),
            DebuggerReply::StaticMetadataType(DebuggerStaticMetadataTypeDisplay {
                static_type: DebuggerStaticMetadataTypeId {
                    metadata: DebuggerStaticMetadataHandle {
                        program,
                        metadata_handle: 24,
                        metadata_generation: 7,
                    },
                    type_id: 0,
                },
                display: "number".to_string(),
            }),
            DebuggerReply::StaticMetadataSymbols(vec![DebuggerStaticMetadataSymbolId {
                metadata: DebuggerStaticMetadataHandle {
                    program,
                    metadata_handle: 24,
                    metadata_generation: 7,
                },
                symbol_id: 0,
            }]),
            DebuggerReply::StaticMetadataSymbol(DebuggerStaticMetadataSymbolDisplay {
                symbol: DebuggerStaticMetadataSymbolId {
                    metadata: DebuggerStaticMetadataHandle {
                        program,
                        metadata_handle: 24,
                        metadata_generation: 7,
                    },
                    symbol_id: 0,
                },
                display: "ProjectControlledName".to_string(),
            }),
            DebuggerReply::StaticMetadataSymbolLocation(DebuggerStaticMetadataSymbolLocation {
                symbol: DebuggerStaticMetadataSymbolId {
                    metadata: DebuggerStaticMetadataHandle {
                        program,
                        metadata_handle: 24,
                        metadata_generation: 7,
                    },
                    symbol_id: 0,
                },
                source: DebuggerStaticMetadataSourceId {
                    metadata: DebuggerStaticMetadataHandle {
                        program,
                        metadata_handle: 24,
                        metadata_generation: 7,
                    },
                    source_id: 0,
                },
                start_byte: 6,
                end_byte: 31,
            }),
            DebuggerReply::StaticMetadataContracts(vec![DebuggerStaticMetadataContractId {
                metadata: DebuggerStaticMetadataHandle {
                    program,
                    metadata_handle: 24,
                    metadata_generation: 7,
                },
                contract_id: 0,
            }]),
            DebuggerReply::StaticMetadataContract(DebuggerStaticMetadataContractDisplay {
                contract: DebuggerStaticMetadataContractId {
                    metadata: DebuggerStaticMetadataHandle {
                        program,
                        metadata_handle: 24,
                        metadata_generation: 7,
                    },
                    contract_id: 0,
                },
                display: "ProjectControlledContract".to_string(),
            }),
            DebuggerReply::StaticMetadataContractValidation(
                DebuggerStaticMetadataContractValidation {
                    contract: DebuggerStaticMetadataContractId {
                        metadata: DebuggerStaticMetadataHandle {
                            program,
                            metadata_handle: 24,
                            metadata_generation: 7,
                        },
                        contract_id: 0,
                    },
                    valid: true,
                },
            ),
            DebuggerReply::SafePoints(vec![safe_point]),
            DebuggerReply::SafePointValidated { safe_point },
            DebuggerReply::BreakpointSet { safe_point },
            DebuggerReply::BreakpointArmed { safe_point },
            DebuggerReply::RootSafePointBreakpointArmed { safe_point },
            DebuggerReply::Breakpoints(vec![safe_point]),
            DebuggerReply::BreakpointCleared {
                safe_point,
                was_present: true,
            },
            DebuggerReply::ExecutionState {
                program,
                state: DebuggerExecutionState::Paused { safe_point },
            },
            DebuggerReply::ExecutionResumed { program },
        ] {
            let (mut sender, mut receiver) = UnixStream::pair().unwrap();
            write_debugger_reply(&mut sender, &reply).unwrap();
            assert_eq!(read_debugger_reply(&mut receiver).unwrap(), reply);
        }
    }

    fn hello(
        requested_metadata_capabilities: DebuggerMetadataCapabilityManifest,
    ) -> DebuggerRequest {
        DebuggerRequest::Hello {
            protocol_version: DEBUGGER_PROTOCOL_VERSION,
            requested_metadata_capabilities,
        }
    }

    #[test]
    fn handshake_rejects_wrong_or_missing_versions_before_dispatch() {
        let empty_policy = DebuggerMetadataCapabilityManifest::empty();
        assert_eq!(
            negotiate(
                &hello(DebuggerMetadataCapabilityManifest::empty()),
                &empty_policy
            ),
            DebuggerReply::HelloAck {
                protocol_version: DEBUGGER_PROTOCOL_VERSION,
                granted_metadata_capabilities: DebuggerMetadataCapabilityManifest::empty(),
            }
        );
        for unsupported_version in [
            1,
            2,
            DEBUGGER_PROTOCOL_VERSION - 1,
            DEBUGGER_PROTOCOL_VERSION + 1,
        ] {
            assert!(matches!(
                negotiate(
                    &DebuggerRequest::Hello {
                        protocol_version: unsupported_version,
                        requested_metadata_capabilities: DebuggerMetadataCapabilityManifest::empty(
                        ),
                    },
                    &empty_policy,
                ),
                DebuggerReply::Error {
                    code: DebuggerErrorCode::ProtocolVersion,
                    ..
                }
            ));
        }
        assert!(matches!(
            negotiate(&DebuggerRequest::ListPageRealms, &empty_policy),
            DebuggerReply::Error {
                code: DebuggerErrorCode::ProtocolVersion,
                ..
            }
        ));
        assert!(matches!(
            negotiate(
                &DebuggerRequest::DescribeCapabilities { realm: realm() },
                &empty_policy,
            ),
            DebuggerReply::Error {
                code: DebuggerErrorCode::ProtocolVersion,
                ..
            }
        ));
    }

    #[test]
    fn metadata_handshake_grants_only_the_canonical_policy_intersection() {
        let request = hello(DebuggerMetadataCapabilityManifest::opaque_inventory());
        let deny_reply = negotiate(&request, &DebuggerMetadataCapabilityManifest::empty());
        assert_eq!(
            deny_reply,
            DebuggerReply::HelloAck {
                protocol_version: DEBUGGER_PROTOCOL_VERSION,
                granted_metadata_capabilities: DebuggerMetadataCapabilityManifest::empty(),
            }
        );
        let denied_session = metadata_session_authorization(&request, &deny_reply)
            .expect("a valid empty grant is still a negotiated session");
        assert!(!denied_session.permits(DebuggerMetadataCapability::OpaqueInventory));

        let allow_reply = negotiate(
            &request,
            &DebuggerMetadataCapabilityManifest::opaque_inventory(),
        );
        let allowed_session = metadata_session_authorization(&request, &allow_reply)
            .expect("the matching canonical requested and allowed sets must negotiate");
        assert!(allowed_session.permits(DebuggerMetadataCapability::OpaqueInventory));
        assert_eq!(
            allow_reply,
            DebuggerReply::HelloAck {
                protocol_version: DEBUGGER_PROTOCOL_VERSION,
                granted_metadata_capabilities: DebuggerMetadataCapabilityManifest::opaque_inventory(
                ),
            }
        );

        let empty_request = hello(DebuggerMetadataCapabilityManifest::empty());
        assert_eq!(
            negotiate(
                &empty_request,
                &DebuggerMetadataCapabilityManifest::opaque_inventory(),
            ),
            DebuggerReply::HelloAck {
                protocol_version: DEBUGGER_PROTOCOL_VERSION,
                granted_metadata_capabilities: DebuggerMetadataCapabilityManifest::empty(),
            }
        );

        for malformed in [
            DebuggerMetadataCapabilityManifest {
                version: 0,
                capabilities: Vec::new(),
            },
            DebuggerMetadataCapabilityManifest {
                version: DEBUGGER_METADATA_CAPABILITY_MANIFEST_VERSION,
                capabilities: vec![
                    DebuggerMetadataCapability::OpaqueInventory,
                    DebuggerMetadataCapability::OpaqueInventory,
                ],
            },
            DebuggerMetadataCapabilityManifest {
                version: DEBUGGER_METADATA_CAPABILITY_MANIFEST_VERSION,
                capabilities: vec![DebuggerMetadataCapability::Unknown],
            },
            DebuggerMetadataCapabilityManifest {
                version: DEBUGGER_METADATA_CAPABILITY_MANIFEST_VERSION,
                capabilities: vec![DebuggerMetadataCapability::OpaqueSummary],
            },
            DebuggerMetadataCapabilityManifest {
                version: DEBUGGER_METADATA_CAPABILITY_MANIFEST_VERSION,
                capabilities: vec![DebuggerMetadataCapability::OpaqueSourceInventory],
            },
        ] {
            assert!(matches!(
                negotiate(
                    &hello(malformed),
                    &DebuggerMetadataCapabilityManifest::empty()
                ),
                DebuggerReply::Error {
                    code: DebuggerErrorCode::InvalidCapabilityManifest,
                    ..
                }
            ));
        }
        let unknown_wire_request: DebuggerRequest = serde_json::from_value(serde_json::json!({
            "Hello": {
                "protocol_version": DEBUGGER_PROTOCOL_VERSION,
                "requested_metadata_capabilities": {
                    "version": DEBUGGER_METADATA_CAPABILITY_MANIFEST_VERSION,
                    "capabilities": ["future-metadata-surface"],
                },
            },
        }))
        .expect("an unknown capability must preserve handshake framing");
        assert!(matches!(
            negotiate(
                &unknown_wire_request,
                &DebuggerMetadataCapabilityManifest::empty(),
            ),
            DebuggerReply::Error {
                code: DebuggerErrorCode::InvalidCapabilityManifest,
                ..
            }
        ));
        assert!(matches!(
            negotiate(
                &request,
                &DebuggerMetadataCapabilityManifest {
                    version: 0,
                    capabilities: Vec::new(),
                },
            ),
            DebuggerReply::Error {
                code: DebuggerErrorCode::InvalidCapabilityManifest,
                ..
            }
        ));

        assert!(metadata_session_authorization(
            &empty_request,
            &DebuggerReply::HelloAck {
                protocol_version: DEBUGGER_PROTOCOL_VERSION,
                granted_metadata_capabilities: DebuggerMetadataCapabilityManifest::opaque_inventory(
                ),
            },
        )
        .is_none());
    }

    #[test]
    fn static_metadata_authorization_requires_session_and_one_exact_live_realm_grant() {
        let inventory = DebuggerMetadataCapability::OpaqueInventory;
        let request = hello(DebuggerMetadataCapabilityManifest::opaque_inventory());
        let denied_reply = negotiate(&request, &DebuggerMetadataCapabilityManifest::empty());
        let denied_session = metadata_session_authorization(&request, &denied_reply).unwrap();
        let allowed_reply = negotiate(
            &request,
            &DebuggerMetadataCapabilityManifest::opaque_inventory(),
        );
        let allowed_session = metadata_session_authorization(&request, &allowed_reply).unwrap();

        let available = capabilities(vec![capability_report(
            DebuggerCapability::StaticMetadataInventory,
            DebuggerCapabilityState::Available,
        )]);
        assert!(available
            .authorize_metadata(&denied_session, inventory)
            .is_none());
        assert!(capabilities(Vec::new())
            .authorize_metadata(&allowed_session, inventory)
            .is_none());
        assert!(capabilities(vec![capability_report(
            DebuggerCapability::StaticMetadataInventory,
            DebuggerCapabilityState::Planned,
        )])
        .authorize_metadata(&allowed_session, inventory)
        .is_none());
        assert!(capabilities(vec![capability_report(
            DebuggerCapability::StaticMetadataInventory,
            DebuggerCapabilityState::Unsupported,
        )])
        .authorize_metadata(&allowed_session, inventory)
        .is_none());
        assert!(capabilities(vec![
            capability_report(
                DebuggerCapability::StaticMetadataInventory,
                DebuggerCapabilityState::Available,
            ),
            capability_report(
                DebuggerCapability::StaticMetadataInventory,
                DebuggerCapabilityState::Available,
            ),
        ])
        .authorize_metadata(&allowed_session, inventory)
        .is_none());

        let mut wrong_version = available.clone();
        wrong_version.protocol_version -= 1;
        assert!(wrong_version
            .authorize_metadata(&allowed_session, inventory)
            .is_none());

        let mut malformed_realm = available.clone();
        malformed_realm.realm.realm_generation = 0;
        assert!(malformed_realm
            .authorize_metadata(&allowed_session, inventory)
            .is_none());

        let authorization = available
            .authorize_metadata(&allowed_session, inventory)
            .expect("one exact session and realm grant must authorize only that realm");
        assert!(authorization.permits(realm(), inventory));
        assert!(!authorization.permits(
            DebuggerPageRealm {
                realm_generation: realm().realm_generation + 1,
                ..realm()
            },
            inventory,
        ));
    }

    #[test]
    fn metadata_inventory_receipts_are_exact_and_stream_local() {
        let request = hello(DebuggerMetadataCapabilityManifest::opaque_summary());
        let reply = negotiate(
            &request,
            &DebuggerMetadataCapabilityManifest::opaque_summary(),
        );
        let session = metadata_session_authorization(&request, &reply)
            .expect("the canonical summary grant must create a session receipt ledger");
        let metadata = DebuggerStaticMetadataHandle {
            program: DebuggerProgram {
                realm: realm(),
                program_handle: 12,
                program_generation: 4,
            },
            metadata_handle: 24,
            metadata_generation: 7,
        };
        assert!(
            !session.observed_metadata(metadata),
            "a numerically well-formed handle is not a receipt before inventory"
        );
        assert!(session.observe_metadata(&[metadata]));
        assert!(session.observed_metadata(metadata));
        assert!(
            !session.observed_metadata(DebuggerStaticMetadataHandle {
                metadata_generation: metadata.metadata_generation + 1,
                ..metadata
            }),
            "a changed generation cannot borrow a prior receipt"
        );

        let other_session = metadata_session_authorization(&request, &reply)
            .expect("a second handshake has its own receipt ledger");
        assert!(
            !other_session.observed_metadata(metadata),
            "a metadata receipt must not cross debugger streams"
        );

        let budget_session = metadata_session_authorization(&request, &reply)
            .expect("a new stream must start with an empty receipt ledger");
        let full_budget = (1..=DEBUGGER_METADATA_SESSION_MAX_OBSERVED_METADATA_IDENTITIES)
            .map(|metadata_handle| DebuggerStaticMetadataHandle {
                metadata_handle: u64::try_from(metadata_handle).unwrap(),
                ..metadata
            })
            .collect::<Vec<_>>();
        assert!(budget_session.observe_metadata(&full_budget));
        let overflow = DebuggerStaticMetadataHandle {
            metadata_handle: u64::try_from(
                DEBUGGER_METADATA_SESSION_MAX_OBSERVED_METADATA_IDENTITIES + 1,
            )
            .unwrap(),
            ..metadata
        };
        assert!(
            !budget_session.observe_metadata(&[overflow]),
            "the bounded insertion must reject rather than partially grow the receipt ledger"
        );
        assert!(
            !budget_session.observed_metadata(overflow),
            "a rejected batch must not mint its overflow handle"
        );
    }

    #[test]
    fn static_metadata_summary_requires_its_own_dependent_capability_grant() {
        let request = hello(DebuggerMetadataCapabilityManifest::opaque_summary());
        let reply = negotiate(
            &request,
            &DebuggerMetadataCapabilityManifest::opaque_summary(),
        );
        let session = metadata_session_authorization(&request, &reply)
            .expect("the canonical dependent metadata grant must negotiate");
        assert!(session.permits(DebuggerMetadataCapability::OpaqueInventory));
        assert!(session.permits(DebuggerMetadataCapability::OpaqueSummary));

        let inventory_only_request = hello(DebuggerMetadataCapabilityManifest::opaque_inventory());
        let inventory_only_reply = negotiate(
            &inventory_only_request,
            &DebuggerMetadataCapabilityManifest::opaque_summary(),
        );
        let inventory_only =
            metadata_session_authorization(&inventory_only_request, &inventory_only_reply).unwrap();
        assert!(!inventory_only.permits(DebuggerMetadataCapability::OpaqueSummary));

        let summary_available = capabilities(vec![capability_report(
            DebuggerCapability::StaticMetadataSummary,
            DebuggerCapabilityState::Available,
        )]);
        let authorization = summary_available
            .authorize_metadata(&session, DebuggerMetadataCapability::OpaqueSummary)
            .expect("summary requires its exact available report and session grant");
        assert!(authorization.permits(realm(), DebuggerMetadataCapability::OpaqueSummary));
        assert!(summary_available
            .authorize_metadata(&inventory_only, DebuggerMetadataCapability::OpaqueSummary)
            .is_none());
    }

    #[test]
    fn static_metadata_source_inventory_requires_its_own_dependent_grant() {
        let request = hello(DebuggerMetadataCapabilityManifest::opaque_source_inventory());
        let reply = negotiate(
            &request,
            &DebuggerMetadataCapabilityManifest::opaque_source_inventory(),
        );
        let session = metadata_session_authorization(&request, &reply)
            .expect("the canonical dependent source-inventory grant must negotiate");
        assert!(session.permits(DebuggerMetadataCapability::OpaqueInventory));
        assert!(session.permits(DebuggerMetadataCapability::OpaqueSourceInventory));

        let inventory_only_request = hello(DebuggerMetadataCapabilityManifest::opaque_inventory());
        let inventory_only_reply = negotiate(
            &inventory_only_request,
            &DebuggerMetadataCapabilityManifest::opaque_source_inventory(),
        );
        let inventory_only =
            metadata_session_authorization(&inventory_only_request, &inventory_only_reply).unwrap();
        assert!(!inventory_only.permits(DebuggerMetadataCapability::OpaqueSourceInventory));

        let source_inventory_available = capabilities(vec![capability_report(
            DebuggerCapability::StaticMetadataSourceInventory,
            DebuggerCapabilityState::Available,
        )]);
        let authorization = source_inventory_available
            .authorize_metadata(&session, DebuggerMetadataCapability::OpaqueSourceInventory)
            .expect("source inventory requires its exact available report and session grant");
        assert!(authorization.permits(realm(), DebuggerMetadataCapability::OpaqueSourceInventory));
        assert!(source_inventory_available
            .authorize_metadata(
                &inventory_only,
                DebuggerMetadataCapability::OpaqueSourceInventory
            )
            .is_none());

        assert_eq!(
            DebuggerMetadataCapabilityManifest::opaque_summary_and_source_inventory().capabilities,
            vec![
                DebuggerMetadataCapability::OpaqueInventory,
                DebuggerMetadataCapability::OpaqueSummary,
                DebuggerMetadataCapability::OpaqueSourceInventory,
            ]
        );
        assert!(
            DebuggerMetadataCapabilityManifest::opaque_summary_and_source_inventory()
                .is_well_formed()
        );
    }

    #[test]
    fn static_metadata_source_provenance_requires_source_inventory_and_sha256_format() {
        let request = hello(DebuggerMetadataCapabilityManifest::opaque_source_provenance());
        let reply = negotiate(
            &request,
            &DebuggerMetadataCapabilityManifest::opaque_source_provenance(),
        );
        let session = metadata_session_authorization(&request, &reply)
            .expect("the canonical provenance grant must negotiate");
        assert!(session.permits(DebuggerMetadataCapability::OpaqueInventory));
        assert!(session.permits(DebuggerMetadataCapability::OpaqueSourceInventory));
        assert!(session.permits(DebuggerMetadataCapability::OpaqueSourceProvenance));

        let source_inventory_request =
            hello(DebuggerMetadataCapabilityManifest::opaque_source_inventory());
        let source_inventory_reply = negotiate(
            &source_inventory_request,
            &DebuggerMetadataCapabilityManifest::opaque_source_provenance(),
        );
        let source_inventory =
            metadata_session_authorization(&source_inventory_request, &source_inventory_reply)
                .unwrap();
        assert!(!source_inventory.permits(DebuggerMetadataCapability::OpaqueSourceProvenance));

        let provenance_available = capabilities(vec![capability_report(
            DebuggerCapability::StaticMetadataSourceProvenance,
            DebuggerCapabilityState::Available,
        )]);
        assert!(provenance_available
            .authorize_metadata(&session, DebuggerMetadataCapability::OpaqueSourceProvenance)
            .is_some());
        assert!(provenance_available
            .authorize_metadata(
                &source_inventory,
                DebuggerMetadataCapability::OpaqueSourceProvenance
            )
            .is_none());

        let source = DebuggerStaticMetadataSourceId {
            metadata: DebuggerStaticMetadataHandle {
                program: DebuggerProgram {
                    realm: realm(),
                    program_handle: 12,
                    program_generation: 4,
                },
                metadata_handle: 24,
                metadata_generation: 7,
            },
            source_id: 0,
        };
        let provenance = DebuggerStaticMetadataSourceProvenance {
            source,
            module: "page-inline:///0.ts".to_string(),
            content_hash:
                "bts-sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
                    .to_string(),
        };
        assert!(provenance.is_well_formed());
        assert!(!DebuggerStaticMetadataSourceProvenance {
            content_hash: "bts-fnv:0000000000000000".to_string(),
            ..provenance.clone()
        }
        .is_well_formed());
        assert!(!DebuggerStaticMetadataSourceProvenance {
            module: "/private/source.ts".to_string(),
            ..provenance.clone()
        }
        .is_well_formed());
        assert!(!DebuggerStaticMetadataSourceProvenance {
            module: "file:///private/source.ts".to_string(),
            ..provenance
        }
        .is_well_formed());
    }

    #[test]
    fn static_metadata_handles_are_opaque_and_generation_bound() {
        let handle = DebuggerStaticMetadataHandle {
            program: DebuggerProgram {
                realm: realm(),
                program_handle: 12,
                program_generation: 5,
            },
            metadata_handle: 41,
            metadata_generation: 9,
        };
        assert!(handle.is_well_formed());
        assert_eq!(
            serde_json::to_value(handle).unwrap(),
            serde_json::json!({
                "program": {
                    "realm": {
                        "browser_context_id": 1,
                        "tab_id": 7,
                        "realm_generation": 3,
                    },
                    "program_handle": 12,
                    "program_generation": 5,
                },
                "metadata_handle": 41,
                "metadata_generation": 9,
            })
        );
        assert!(!DebuggerStaticMetadataHandle {
            metadata_handle: 0,
            ..handle
        }
        .is_well_formed());
        assert!(!DebuggerStaticMetadataHandle {
            metadata_generation: 0,
            ..handle
        }
        .is_well_formed());
    }

    #[test]
    fn static_metadata_summary_is_bounded_and_source_free() {
        let metadata = DebuggerStaticMetadataHandle {
            program: DebuggerProgram {
                realm: realm(),
                program_handle: 12,
                program_generation: 5,
            },
            metadata_handle: 41,
            metadata_generation: 9,
        };
        let summary = DebuggerStaticMetadataSummary {
            metadata,
            language_version: "blue-ts-0.1".to_string(),
            compiler_options_hash: "0123456789abcdef".to_string(),
            source_count: 1,
            type_count: 2,
            symbol_count: 3,
            contract_count: 4,
        };
        assert!(summary.is_well_formed());
        assert!(!DebuggerStaticMetadataSummary {
            compiler_options_hash: String::new(),
            ..summary.clone()
        }
        .is_well_formed());
        assert!(!DebuggerStaticMetadataSummary {
            symbol_count: DEBUGGER_STATIC_METADATA_MAX_SYMBOLS + 1,
            ..summary
        }
        .is_well_formed());
    }

    #[test]
    fn type_inventory_is_parent_bound_and_receipted_without_a_type_display() {
        let metadata = DebuggerStaticMetadataHandle {
            program: DebuggerProgram {
                realm: realm(),
                program_handle: 12,
                program_generation: 5,
            },
            metadata_handle: 41,
            metadata_generation: 9,
        };
        let static_type = DebuggerStaticMetadataTypeId {
            metadata,
            type_id: 0,
        };
        assert!(static_type.is_well_formed());
        let request = hello(DebuggerMetadataCapabilityManifest::opaque_type_inventory());
        let reply = negotiate(
            &request,
            &DebuggerMetadataCapabilityManifest::opaque_type_inventory(),
        );
        let session = metadata_session_authorization(&request, &reply).unwrap();
        assert!(session.permits(DebuggerMetadataCapability::OpaqueTypeInventory));
        assert!(!session.permits(DebuggerMetadataCapability::OpaqueTypeDisplay));
        assert!(!session.observed_type(static_type));
        assert!(session.observe_types(&[static_type]));
        assert!(session.observed_type(static_type));
        assert!(!session.observed_type(DebuggerStaticMetadataTypeId {
            type_id: 1,
            ..static_type
        }));
        assert_eq!(
            serde_json::to_value(static_type).unwrap(),
            serde_json::json!({
                "metadata": {
                    "program": {
                        "realm": {
                            "browser_context_id": 1,
                            "tab_id": 7,
                            "realm_generation": 3,
                        },
                        "program_handle": 12,
                        "program_generation": 5,
                    },
                    "metadata_handle": 41,
                    "metadata_generation": 9,
                },
                "type_id": 0,
            })
        );
    }

    #[test]
    fn type_display_requires_type_inventory_and_respects_its_fixed_budget() {
        let metadata = DebuggerStaticMetadataHandle {
            program: DebuggerProgram {
                realm: realm(),
                program_handle: 12,
                program_generation: 5,
            },
            metadata_handle: 41,
            metadata_generation: 9,
        };
        let static_type = DebuggerStaticMetadataTypeId {
            metadata,
            type_id: 0,
        };
        let request = hello(DebuggerMetadataCapabilityManifest::opaque_type_display());
        let reply = negotiate(
            &request,
            &DebuggerMetadataCapabilityManifest::opaque_type_display(),
        );
        let session = metadata_session_authorization(&request, &reply).unwrap();
        assert!(session.permits(DebuggerMetadataCapability::OpaqueInventory));
        assert!(session.permits(DebuggerMetadataCapability::OpaqueTypeInventory));
        assert!(session.permits(DebuggerMetadataCapability::OpaqueTypeDisplay));
        assert!(!session.observed_type(static_type));
        assert!(session.observe_types(&[static_type]));
        assert!(session.observed_type(static_type));

        let display = DebuggerStaticMetadataTypeDisplay {
            static_type,
            display: "ProjectControlledName".to_string(),
        };
        assert!(display.is_well_formed());
        assert!(!DebuggerStaticMetadataTypeDisplay {
            display: String::new(),
            ..display.clone()
        }
        .is_well_formed());
        assert!(!DebuggerStaticMetadataTypeDisplay {
            display: "x".repeat(DEBUGGER_STATIC_METADATA_TYPE_DISPLAY_MAX_BYTES + 1),
            ..display
        }
        .is_well_formed());

        let malformed = DebuggerMetadataCapabilityManifest {
            version: DEBUGGER_METADATA_CAPABILITY_MANIFEST_VERSION,
            capabilities: vec![DebuggerMetadataCapability::OpaqueTypeDisplay],
        };
        assert!(!malformed.is_well_formed());
    }

    #[test]
    fn symbol_inventory_is_parent_bound_and_receipted_without_symbol_detail() {
        let metadata = DebuggerStaticMetadataHandle {
            program: DebuggerProgram {
                realm: realm(),
                program_handle: 12,
                program_generation: 5,
            },
            metadata_handle: 41,
            metadata_generation: 9,
        };
        let symbol = DebuggerStaticMetadataSymbolId {
            metadata,
            symbol_id: 0,
        };
        assert!(symbol.is_well_formed());
        let request = hello(DebuggerMetadataCapabilityManifest::opaque_symbol_inventory());
        let reply = negotiate(
            &request,
            &DebuggerMetadataCapabilityManifest::opaque_symbol_inventory(),
        );
        let session = metadata_session_authorization(&request, &reply).unwrap();
        assert!(session.permits(DebuggerMetadataCapability::OpaqueSymbolInventory));
        assert!(!session.observed_symbol(symbol));
        assert!(session.observe_symbols(&[symbol]));
        assert!(session.observed_symbol(symbol));
        assert!(!session.observed_symbol(DebuggerStaticMetadataSymbolId {
            symbol_id: 1,
            ..symbol
        }));

        let malformed = DebuggerMetadataCapabilityManifest {
            version: DEBUGGER_METADATA_CAPABILITY_MANIFEST_VERSION,
            capabilities: vec![DebuggerMetadataCapability::OpaqueSymbolInventory],
        };
        assert!(!malformed.is_well_formed());
    }

    #[test]
    fn contract_inventory_is_parent_bound_and_receipted_without_contract_detail() {
        let metadata = DebuggerStaticMetadataHandle {
            program: DebuggerProgram {
                realm: realm(),
                program_handle: 12,
                program_generation: 5,
            },
            metadata_handle: 41,
            metadata_generation: 9,
        };
        let contract = DebuggerStaticMetadataContractId {
            metadata,
            contract_id: 0,
        };
        assert!(contract.is_well_formed());
        let request = hello(DebuggerMetadataCapabilityManifest::opaque_contract_inventory());
        let reply = negotiate(
            &request,
            &DebuggerMetadataCapabilityManifest::opaque_contract_inventory(),
        );
        let session = metadata_session_authorization(&request, &reply).unwrap();
        assert!(session.permits(DebuggerMetadataCapability::OpaqueContractInventory));
        assert!(!session.observed_contract(contract));
        assert!(session.observe_contracts(&[contract]));
        assert!(session.observed_contract(contract));
        assert!(
            !session.observed_contract(DebuggerStaticMetadataContractId {
                contract_id: 1,
                ..contract
            })
        );

        let malformed = DebuggerMetadataCapabilityManifest {
            version: DEBUGGER_METADATA_CAPABILITY_MANIFEST_VERSION,
            capabilities: vec![DebuggerMetadataCapability::OpaqueContractInventory],
        };
        assert!(!malformed.is_well_formed());
    }

    #[test]
    fn symbol_display_requires_symbol_inventory_and_respects_its_fixed_budget() {
        let metadata = DebuggerStaticMetadataHandle {
            program: DebuggerProgram {
                realm: realm(),
                program_handle: 12,
                program_generation: 5,
            },
            metadata_handle: 41,
            metadata_generation: 9,
        };
        let symbol = DebuggerStaticMetadataSymbolId {
            metadata,
            symbol_id: 0,
        };
        let request = hello(DebuggerMetadataCapabilityManifest::opaque_symbol_display());
        let reply = negotiate(
            &request,
            &DebuggerMetadataCapabilityManifest::opaque_symbol_display(),
        );
        let session = metadata_session_authorization(&request, &reply).unwrap();
        assert!(session.permits(DebuggerMetadataCapability::OpaqueInventory));
        assert!(session.permits(DebuggerMetadataCapability::OpaqueSymbolInventory));
        assert!(session.permits(DebuggerMetadataCapability::OpaqueSymbolDisplay));
        assert!(!session.observed_symbol(symbol));
        assert!(session.observe_symbols(&[symbol]));
        assert!(session.observed_symbol(symbol));

        let display = DebuggerStaticMetadataSymbolDisplay {
            symbol,
            display: "ProjectControlledName".to_string(),
        };
        assert!(display.is_well_formed());
        assert!(!DebuggerStaticMetadataSymbolDisplay {
            display: String::new(),
            ..display.clone()
        }
        .is_well_formed());
        assert!(!DebuggerStaticMetadataSymbolDisplay {
            display: "x".repeat(DEBUGGER_STATIC_METADATA_SYMBOL_DISPLAY_MAX_BYTES + 1),
            ..display
        }
        .is_well_formed());

        let malformed = DebuggerMetadataCapabilityManifest {
            version: DEBUGGER_METADATA_CAPABILITY_MANIFEST_VERSION,
            capabilities: vec![DebuggerMetadataCapability::OpaqueSymbolDisplay],
        };
        assert!(!malformed.is_well_formed());
    }

    #[test]
    fn contract_display_requires_contract_inventory_and_respects_its_fixed_budget() {
        let metadata = DebuggerStaticMetadataHandle {
            program: DebuggerProgram {
                realm: realm(),
                program_handle: 12,
                program_generation: 5,
            },
            metadata_handle: 41,
            metadata_generation: 9,
        };
        let contract = DebuggerStaticMetadataContractId {
            metadata,
            contract_id: 0,
        };
        let request = hello(DebuggerMetadataCapabilityManifest::opaque_contract_display());
        let reply = negotiate(
            &request,
            &DebuggerMetadataCapabilityManifest::opaque_contract_display(),
        );
        let session = metadata_session_authorization(&request, &reply).unwrap();
        assert!(session.permits(DebuggerMetadataCapability::OpaqueInventory));
        assert!(session.permits(DebuggerMetadataCapability::OpaqueContractInventory));
        assert!(session.permits(DebuggerMetadataCapability::OpaqueContractDisplay));
        assert!(!session.observed_contract(contract));
        assert!(session.observe_contracts(&[contract]));
        assert!(session.observed_contract(contract));

        let display = DebuggerStaticMetadataContractDisplay {
            contract,
            display: "ProjectControlledContract".to_string(),
        };
        assert!(display.is_well_formed());
        assert!(!DebuggerStaticMetadataContractDisplay {
            display: String::new(),
            ..display.clone()
        }
        .is_well_formed());
        assert!(!DebuggerStaticMetadataContractDisplay {
            display: "x".repeat(DEBUGGER_STATIC_METADATA_CONTRACT_DISPLAY_MAX_BYTES + 1),
            ..display
        }
        .is_well_formed());

        let malformed = DebuggerMetadataCapabilityManifest {
            version: DEBUGGER_METADATA_CAPABILITY_MANIFEST_VERSION,
            capabilities: vec![DebuggerMetadataCapability::OpaqueContractDisplay],
        };
        assert!(!malformed.is_well_formed());
    }

    #[test]
    fn contract_validation_requires_contract_inventory_and_returns_only_a_boolean() {
        let metadata = DebuggerStaticMetadataHandle {
            program: DebuggerProgram {
                realm: realm(),
                program_handle: 12,
                program_generation: 5,
            },
            metadata_handle: 41,
            metadata_generation: 9,
        };
        let contract = DebuggerStaticMetadataContractId {
            metadata,
            contract_id: 0,
        };
        let request = hello(DebuggerMetadataCapabilityManifest::opaque_contract_validation());
        let reply = negotiate(
            &request,
            &DebuggerMetadataCapabilityManifest::opaque_contract_validation(),
        );
        let session = metadata_session_authorization(&request, &reply).unwrap();
        assert!(session.permits(DebuggerMetadataCapability::OpaqueInventory));
        assert!(session.permits(DebuggerMetadataCapability::OpaqueContractInventory));
        assert!(session.permits(DebuggerMetadataCapability::OpaqueContractValidation));
        assert!(!session.observed_contract(contract));
        assert!(session.observe_contracts(&[contract]));
        assert!(session.observed_contract(contract));

        assert!(DebuggerStaticMetadataContractValidation {
            contract,
            valid: false,
        }
        .is_well_formed());

        let malformed = DebuggerMetadataCapabilityManifest {
            version: DEBUGGER_METADATA_CAPABILITY_MANIFEST_VERSION,
            capabilities: vec![DebuggerMetadataCapability::OpaqueContractValidation],
        };
        assert!(!malformed.is_well_formed());
    }

    #[test]
    fn lowering_summary_requires_inventory_and_contains_no_map_entries() {
        let metadata = DebuggerStaticMetadataHandle {
            program: DebuggerProgram {
                realm: realm(),
                program_handle: 12,
                program_generation: 5,
            },
            metadata_handle: 41,
            metadata_generation: 9,
        };
        let request = hello(DebuggerMetadataCapabilityManifest::opaque_lowering_summary());
        let reply = negotiate(
            &request,
            &DebuggerMetadataCapabilityManifest::opaque_lowering_summary(),
        );
        let session = metadata_session_authorization(&request, &reply).unwrap();
        assert!(session.permits(DebuggerMetadataCapability::OpaqueInventory));
        assert!(session.permits(DebuggerMetadataCapability::OpaqueLoweringSummary));
        assert!(DebuggerStaticMetadataLoweringSummary {
            metadata,
            safe_point_map_abi: DEBUGGER_STATIC_METADATA_SAFE_POINT_MAP_ABI_V1.to_string(),
            program_abi: DEBUGGER_STATIC_METADATA_PROGRAM_ABI_V1.to_string(),
            source_set_hash: "bts-source-set-0123456789abcdef".to_string(),
            bound_safe_point_count: 1,
        }
        .is_well_formed());
        assert!(
            !DebuggerStaticMetadataLoweringSummary {
                metadata,
                safe_point_map_abi: "child-controlled-label".to_string(),
                program_abi: DEBUGGER_STATIC_METADATA_PROGRAM_ABI_V1.to_string(),
                source_set_hash: "bts-source-set-0123456789abcdef".to_string(),
                bound_safe_point_count: 1,
            }
            .is_well_formed(),
            "ABI labels are a fixed protocol vocabulary, never child-controlled text"
        );
        assert!(
            !DebuggerStaticMetadataLoweringSummary {
                metadata,
                safe_point_map_abi: DEBUGGER_STATIC_METADATA_SAFE_POINT_MAP_ABI_V1.to_string(),
                program_abi: DEBUGGER_STATIC_METADATA_PROGRAM_ABI_V1.to_string(),
                source_set_hash: "bts-source-set-0123456789ABCDEf".to_string(),
                bound_safe_point_count: 1,
            }
            .is_well_formed(),
            "source-set receipts must remain canonical lowercase opaque digests"
        );

        let malformed = DebuggerMetadataCapabilityManifest {
            version: DEBUGGER_METADATA_CAPABILITY_MANIFEST_VERSION,
            capabilities: vec![DebuggerMetadataCapability::OpaqueLoweringSummary],
        };
        assert!(!malformed.is_well_formed());
    }

    #[test]
    fn symbol_type_requires_two_receipted_ids_and_round_trips_on_a_socket() {
        let metadata = DebuggerStaticMetadataHandle {
            program: DebuggerProgram {
                realm: realm(),
                program_handle: 12,
                program_generation: 5,
            },
            metadata_handle: 41,
            metadata_generation: 9,
        };
        let relation = DebuggerStaticMetadataSymbolType {
            symbol: DebuggerStaticMetadataSymbolId {
                metadata,
                symbol_id: 1,
            },
            static_type: DebuggerStaticMetadataTypeId {
                metadata,
                type_id: 2,
            },
        };
        assert!(relation.is_well_formed());
        assert!(!DebuggerStaticMetadataSymbolType {
            static_type: DebuggerStaticMetadataTypeId {
                metadata: DebuggerStaticMetadataHandle {
                    metadata_generation: 10,
                    ..metadata
                },
                ..relation.static_type
            },
            ..relation
        }
        .is_well_formed());
        let manifest = DebuggerMetadataCapabilityManifest::opaque_symbol_type();
        assert!(manifest.is_well_formed());
        let request = hello(manifest.clone());
        let reply = negotiate(&request, &manifest);
        let session = metadata_session_authorization(&request, &reply).unwrap();
        assert!(session.permits(DebuggerMetadataCapability::OpaqueSymbolType));
        assert!(!session.observed_symbol(relation.symbol));
        assert!(!session.observed_type(relation.static_type));
        assert!(session.observe_symbols(&[relation.symbol]));
        assert!(session.observe_types(&[relation.static_type]));
        assert!(session.observed_symbol(relation.symbol));
        assert!(session.observed_type(relation.static_type));
        let separate_stream = metadata_session_authorization(&request, &reply).unwrap();
        assert!(!separate_stream.observed_symbol(relation.symbol));
        assert!(!separate_stream.observed_type(relation.static_type));
        let inventory_only = DebuggerMetadataCapabilityManifest::opaque_selected(
            DebuggerMetadataCapabilitySelection {
                type_inventory: true,
                symbol_inventory: true,
                ..DebuggerMetadataCapabilitySelection::default()
            },
        );
        let hello_without_relation = hello(manifest);
        let granted_without_relation = negotiate(&hello_without_relation, &inventory_only);
        let denied_session =
            metadata_session_authorization(&hello_without_relation, &granted_without_relation)
                .unwrap();
        assert!(!denied_session.permits(DebuggerMetadataCapability::OpaqueSymbolType));
        for missing in [
            vec![DebuggerMetadataCapability::OpaqueSymbolType],
            vec![
                DebuggerMetadataCapability::OpaqueInventory,
                DebuggerMetadataCapability::OpaqueSymbolInventory,
                DebuggerMetadataCapability::OpaqueSymbolType,
            ],
            vec![
                DebuggerMetadataCapability::OpaqueInventory,
                DebuggerMetadataCapability::OpaqueTypeInventory,
                DebuggerMetadataCapability::OpaqueSymbolType,
            ],
        ] {
            assert!(!DebuggerMetadataCapabilityManifest {
                version: DEBUGGER_METADATA_CAPABILITY_MANIFEST_VERSION,
                capabilities: missing,
            }
            .is_well_formed());
        }
        let request = DebuggerRequest::DescribeStaticMetadataSymbolType { target: relation };
        let (mut sender, mut receiver) = UnixStream::pair().unwrap();
        write_debugger_request(&mut sender, &request).unwrap();
        assert_eq!(read_debugger_request(&mut receiver).unwrap(), request);
        let reply = DebuggerReply::StaticMetadataSymbolType(relation);
        let (mut sender, mut receiver) = UnixStream::pair().unwrap();
        write_debugger_reply(&mut sender, &reply).unwrap();
        assert_eq!(read_debugger_reply(&mut receiver).unwrap(), reply);
    }

    #[test]
    fn generation_bound_target_and_safe_point_ids_reject_zero_placeholders() {
        let valid_program = DebuggerProgram {
            realm: realm(),
            program_handle: 12,
            program_generation: 5,
        };
        assert!(valid_program.is_well_formed());
        assert!(DebuggerSafePoint {
            program: valid_program,
            code_unit_ordinal: 0,
            bytecode_offset: 0,
        }
        .is_well_formed());
        assert!(!DebuggerPageRealm {
            browser_context_id: 1,
            tab_id: 7,
            realm_generation: 0,
        }
        .is_well_formed());
        assert!(!DebuggerProgram {
            realm: realm(),
            program_handle: 0,
            program_generation: 5,
        }
        .is_well_formed());
    }

    #[test]
    fn malformed_debugger_frames_fail_without_a_panic() {
        let mut bytes = Vec::new();
        let malformed = b"not json";
        bytes.extend_from_slice(&(malformed.len() as u32).to_le_bytes());
        bytes.extend_from_slice(malformed);
        assert!(read_debugger_request(&mut std::io::Cursor::new(bytes)).is_err());
    }
}
