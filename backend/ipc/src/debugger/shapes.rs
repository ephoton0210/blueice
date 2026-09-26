// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

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

/// A fixed, source-free classification of a reifiable contract's root after
/// resolving its compiler-retained local references. A cyclic or unresolved
/// reference remains `Reference`; no field names or plan edges are exposed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DebuggerStaticMetadataContractRootKind {
    Null,
    Undefined,
    Boolean,
    Number,
    String,
    Literal,
    Array,
    Tuple,
    Record,
    Union,
    Intersection,
    Reference,
}

/// One owner-authorized display for a compiler-minted contract ID previously
/// returned by the exact stream's contract inventory. A display can contain a
/// project-authored identifier and a fixed root-shape classification, so it
/// is independently default-denied. It carries no source span, plan edges,
/// field names, validation behavior, bytecode, or static record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DebuggerStaticMetadataContractDisplay {
    pub contract: DebuggerStaticMetadataContractId,
    pub display: String,
    pub root_kind: DebuggerStaticMetadataContractRootKind,
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
/// project-authored identifier, compiler declaration kind, and export status,
/// so it is
/// independently default-denied and carries no source span, type, contract,
/// bytecode, or static record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DebuggerStaticMetadataSymbolDisplay {
    pub symbol: DebuggerStaticMetadataSymbolId,
    pub display: String,
    pub kind: DebuggerStaticMetadataSymbolKind,
    pub exported: bool,
}

/// The bounded compiler classification of a source-level declaration. It
/// carries no identifier, source position, static type, or runtime identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DebuggerStaticMetadataSymbolKind {
    Import,
    TypeAlias,
    Interface,
    Variable,
    Function,
}

impl DebuggerStaticMetadataSymbolDisplay {
    pub fn is_well_formed(&self) -> bool {
        self.symbol.is_well_formed()
            && !self.display.is_empty()
            && self.display.len() <= DEBUGGER_STATIC_METADATA_SYMBOL_DISPLAY_MAX_BYTES
    }
}

/// Bounded zero-based original-source coordinates for one compiler-produced
/// declaration or lowering range. Columns count UTF-16 code units; a caller cannot ask
/// this protocol to map an arbitrary byte offset or read source text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DebuggerSourceCoordinates {
    pub start_line: u32,
    pub start_column_utf16: u32,
    pub end_line: u32,
    pub end_column_utf16: u32,
}

impl DebuggerSourceCoordinates {
    pub fn is_well_formed_for_range(self, start_byte: u32, end_byte: u32) -> bool {
        start_byte < end_byte
            && end_byte <= DEBUGGER_STATIC_METADATA_MAX_SOURCE_SPAN_BYTES
            && self.start_line <= DEBUGGER_STATIC_METADATA_MAX_SOURCE_SPAN_BYTES
            && self.end_line <= DEBUGGER_STATIC_METADATA_MAX_SOURCE_SPAN_BYTES
            && self.start_column_utf16 <= DEBUGGER_STATIC_METADATA_MAX_SOURCE_SPAN_BYTES
            && self.end_column_utf16 <= DEBUGGER_STATIC_METADATA_MAX_SOURCE_SPAN_BYTES
            && (self.start_line, self.start_column_utf16) < (self.end_line, self.end_column_utf16)
            && self.start_line + self.start_column_utf16 <= start_byte
            && self.end_line + self.end_column_utf16 <= end_byte
    }
}

/// One owner-authorized location for a compiler-minted symbol that the exact
/// debugger stream previously inventoried. The range is a half-open UTF-8
/// byte range under a separately receipted source ID: it is not source text,
/// a module identity, an arbitrary line/column conversion, a bytecode position, or a
/// source-read capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DebuggerStaticMetadataSymbolLocation {
    pub symbol: DebuggerStaticMetadataSymbolId,
    pub source: DebuggerStaticMetadataSourceId,
    pub start_byte: u32,
    pub end_byte: u32,
    pub coordinates: DebuggerSourceCoordinates,
}

impl DebuggerStaticMetadataSymbolLocation {
    /// Rejects malformed or over-budget child locations before they reach a
    /// debugger client. An unchecked machine-sized offset can never become a
    /// wire-range or allocation ambiguity.
    pub fn is_well_formed(self) -> bool {
        self.symbol.is_well_formed()
            && self.source.is_well_formed()
            && self.symbol.metadata == self.source.metadata
            && self
                .coordinates
                .is_well_formed_for_range(self.start_byte, self.end_byte)
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

/// One exact safe point and independently inventoried source ID under the
/// same opaque BlueTS metadata parent. It cannot select a source byte offset
/// or request a nearest-position lookup.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DebuggerStaticMetadataSafePointSpanTarget {
    pub safe_point: DebuggerSafePoint,
    pub source: DebuggerStaticMetadataSourceId,
}

impl DebuggerStaticMetadataSafePointSpanTarget {
    pub fn is_well_formed(self) -> bool {
        self.safe_point.is_well_formed()
            && self.source.is_well_formed()
            && self.safe_point.program == self.source.metadata.program
    }
}

/// An exact original BlueTS half-open UTF-8 byte span and UTF-16 coordinates,
/// bound to the caller's verified safe point and separately receipted source
/// ID. This is not source text, module identity, generated source, or a
/// general source-map read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DebuggerStaticMetadataSafePointSpan {
    pub safe_point: DebuggerSafePoint,
    pub source: DebuggerStaticMetadataSourceId,
    pub start_byte: u32,
    pub end_byte: u32,
    pub coordinates: DebuggerSourceCoordinates,
}

impl DebuggerStaticMetadataSafePointSpan {
    pub fn is_well_formed(self) -> bool {
        DebuggerStaticMetadataSafePointSpanTarget {
            safe_point: self.safe_point,
            source: self.source,
        }
        .is_well_formed()
            && self
                .coordinates
                .is_well_formed_for_range(self.start_byte, self.end_byte)
    }
}

/// One terminal uncaught BlueTS location under a prior same-stream source
/// receipt. The safe point is core-reminted, not the child's private handle;
/// no error value, message, stack, source text, or generated span is present.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DebuggerExceptionLocation {
    pub source: DebuggerStaticMetadataSourceId,
    pub safe_point: DebuggerSafePoint,
    pub start_byte: u32,
    pub end_byte: u32,
    pub coordinates: DebuggerSourceCoordinates,
}

impl DebuggerExceptionLocation {
    pub fn is_well_formed(self) -> bool {
        self.source.is_well_formed()
            && self.safe_point.is_well_formed()
            && self.safe_point.program == self.source.metadata.program
            && self
                .coordinates
                .is_well_formed_for_range(self.start_byte, self.end_byte)
    }
}

/// A separately authorized original BlueTS position in one source record
/// already inventoried by this debugger stream. This is an explicit
/// source-position query, not part of the exact-span read capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DebuggerStaticMetadataSourceBreakpointTarget {
    pub source: DebuggerStaticMetadataSourceId,
    pub source_byte: u32,
}

impl DebuggerStaticMetadataSourceBreakpointTarget {
    pub fn is_well_formed(self) -> bool {
        self.source.is_well_formed()
            && self.source_byte <= DEBUGGER_STATIC_METADATA_MAX_SOURCE_SPAN_BYTES
    }
}

/// A source-position binding result under the exact live program generation.
/// `None` means that the selected lowering span has no instruction or there
/// is no following lowered span; it must not be silently retargeted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DebuggerStaticMetadataSourceBreakpoint {
    pub target: DebuggerStaticMetadataSourceBreakpointTarget,
    pub safe_point: Option<DebuggerSafePoint>,
}

impl DebuggerStaticMetadataSourceBreakpoint {
    pub fn is_well_formed(self) -> bool {
        self.target.is_well_formed()
            && self.safe_point.is_none_or(|safe_point| {
                safe_point.is_well_formed()
                    && safe_point.program == self.target.source.metadata.program
            })
    }
}

impl DebuggerStaticMetadataSymbolLocation {
    /// Checks whether separately authorized symbol, source, and exact-span
    /// observations identify one executable declaration. This data-only
    /// client helper grants nothing: every input must first come from its
    /// independently gated operation on the same debugger stream, and the
    /// returned safe point still needs the ordinary live arm validation.
    pub fn executable_breakpoint_candidate(
        self,
        display: &DebuggerStaticMetadataSymbolDisplay,
        binding: DebuggerStaticMetadataSourceBreakpoint,
        span: DebuggerStaticMetadataSafePointSpan,
    ) -> Option<DebuggerSafePoint> {
        if !self.is_well_formed()
            || !display.is_well_formed()
            || !binding.is_well_formed()
            || !span.is_well_formed()
            || !matches!(
                display.kind,
                DebuggerStaticMetadataSymbolKind::Variable
                    | DebuggerStaticMetadataSymbolKind::Function
            )
            || display.symbol != self.symbol
            || binding.target.source != self.source
            || binding.target.source_byte != self.start_byte
            || span.source != self.source
            || span.start_byte < self.start_byte
            || span.end_byte > self.end_byte
        {
            return None;
        }
        let point = binding.safe_point?;
        (span.safe_point == point).then_some(point)
    }
}

impl DebuggerStaticMetadataSymbolLocationTarget {
    pub fn is_well_formed(self) -> bool {
        self.symbol.is_well_formed()
            && self.source.is_well_formed()
            && self.symbol.metadata == self.source.metadata
    }
}

/// One separately authorized declaration range for a compiler-minted
/// contract. Both opaque IDs must have crossed this debugger stream's exact
/// inventories; the range is neither source text nor contract-plan access.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DebuggerStaticMetadataContractLocation {
    pub contract: DebuggerStaticMetadataContractId,
    pub source: DebuggerStaticMetadataSourceId,
    pub start_byte: u32,
    pub end_byte: u32,
    pub coordinates: DebuggerSourceCoordinates,
}

impl DebuggerStaticMetadataContractLocation {
    pub fn is_well_formed(self) -> bool {
        self.contract.is_well_formed()
            && self.source.is_well_formed()
            && self.contract.metadata == self.source.metadata
            && self
                .coordinates
                .is_well_formed_for_range(self.start_byte, self.end_byte)
    }
}

/// The exact contract/source receipt pair required to ask for its location.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DebuggerStaticMetadataContractLocationTarget {
    pub contract: DebuggerStaticMetadataContractId,
    pub source: DebuggerStaticMetadataSourceId,
}

impl DebuggerStaticMetadataContractLocationTarget {
    pub fn is_well_formed(self) -> bool {
        self.contract.is_well_formed()
            && self.source.is_well_formed()
            && self.contract.metadata == self.source.metadata
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

/// One compiler-verified relation between a symbol and its reifiable local
/// contract. Both opaque IDs must have been returned by separate inventories
/// on this stream. No contract plan, validation result, name, or static
/// record is disclosed by this operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DebuggerStaticMetadataSymbolContract {
    pub symbol: DebuggerStaticMetadataSymbolId,
    pub contract: DebuggerStaticMetadataContractId,
}

impl DebuggerStaticMetadataSymbolContract {
    pub fn is_well_formed(self) -> bool {
        self.symbol.is_well_formed()
            && self.contract.is_well_formed()
            && self.symbol.metadata == self.contract.metadata
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

/// A core-reminted identity for one actually paused nested invocation.
/// Its handle is opaque and process-unique while this core lives; a static
/// safe point may name the same code unit but can never stand in for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DebuggerFrame {
    pub program: DebuggerProgram,
    pub code_unit_ordinal: u32,
    /// A core-instance identity: PID plus 96 random bits. It prevents a
    /// successor core's fresh handle counter from aliasing its predecessor.
    pub core_instance: [u8; 16],
    pub frame_handle: u64,
}

impl DebuggerFrame {
    pub fn is_well_formed(self) -> bool {
        self.program.is_well_formed()
            && self.code_unit_ordinal != 0
            && self.core_instance != [0; 16]
            && self.frame_handle != 0
    }

    pub fn matches_safe_point(self, safe_point: DebuggerSafePoint) -> bool {
        self.is_well_formed()
            && self.program == safe_point.program
            && self.code_unit_ordinal == safe_point.code_unit_ordinal
    }
}

/// Source-free ordered locations of only the actually paused continuation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DebuggerStackSnapshot {
    pub program: DebuggerProgram,
    pub frame: Option<DebuggerFrame>,
    pub safe_points: Vec<DebuggerSafePoint>,
    pub stack_truncated: bool,
}

impl DebuggerStackSnapshot {
    /// Checks only the bounded, internally consistent shape. The core must
    /// separately re-read the live continuation before trusting a snapshot.
    pub fn is_well_formed(&self) -> bool {
        if !self.program.is_well_formed()
            || self.safe_points.is_empty()
            || self.safe_points.len() > DEBUGGER_MAX_STACK_FRAMES as usize
            || !self
                .safe_points
                .iter()
                .all(|point| point.is_well_formed() && point.program == self.program)
        {
            return false;
        }
        match self.frame {
            Some(frame) => {
                frame.program == self.program
                    && frame.matches_safe_point(self.safe_points[0])
                    && (self.stack_truncated
                        || self
                            .safe_points
                            .last()
                            .is_some_and(|point| point.code_unit_ordinal == 0))
            }
            None => {
                self.safe_points.len() == 1
                    && self.safe_points[0].code_unit_ordinal == 0
                    && !self.stack_truncated
            }
        }
    }
}

/// One previously returned, bounded Stack snapshot plus a separately
/// inventoried BlueTS source ID for each frame in exact top-first order.
/// This value alone grants no source access; the core must verify every
/// receipt and compare the whole snapshot with the live paused stack.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DebuggerStackCoordinatesTarget {
    pub expected_stack: DebuggerStackSnapshot,
    pub sources: Vec<DebuggerStaticMetadataSourceId>,
}

impl DebuggerStackCoordinatesTarget {
    pub fn is_well_formed(&self) -> bool {
        self.expected_stack.is_well_formed()
            && self.sources.len() == self.expected_stack.safe_points.len()
            && self.sources.first().is_some_and(|first| {
                first.is_well_formed()
                    && first.metadata.program == self.expected_stack.program
                    && self
                        .sources
                        .iter()
                        .all(|source| source.is_well_formed() && source.metadata == first.metadata)
            })
    }
}

/// A complete original BlueTS coordinate result for that exact stack.
/// Each entry repeats its ordered safe point and receipted source ID so a
/// partial, reordered, or cross-metadata response is malformed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DebuggerStackCoordinates {
    pub stack: DebuggerStackSnapshot,
    pub spans: Vec<DebuggerStaticMetadataSafePointSpan>,
}

impl DebuggerStackCoordinates {
    pub fn is_well_formed(&self) -> bool {
        self.stack.is_well_formed()
            && self.spans.len() == self.stack.safe_points.len()
            && self.spans.first().is_some_and(|first| {
                first.is_well_formed()
                    && first.source.metadata.program == self.stack.program
                    && self
                        .spans
                        .iter()
                        .zip(&self.stack.safe_points)
                        .all(|(span, point)| {
                            span.is_well_formed()
                                && span.safe_point == *point
                                && span.source.metadata == first.source.metadata
                        })
            })
    }
}

/// One core-reminted invocation in a linked dependency/entry stack. Unlike
/// the older nested-frame type, this shape also permits the entry caller's
/// root code unit (ordinal zero). Its role is fixed by the complete stack.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DebuggerLinkedFrame {
    pub program: DebuggerProgram,
    pub code_unit_ordinal: u32,
    pub core_instance: [u8; 16],
    pub frame_handle: u64,
}

impl DebuggerLinkedFrame {
    pub fn is_well_formed(self) -> bool {
        self.program.is_well_formed() && self.core_instance != [0; 16] && self.frame_handle != 0
    }

    pub fn matches_safe_point(self, safe_point: DebuggerSafePoint) -> bool {
        self.is_well_formed()
            && self.program == safe_point.program
            && self.code_unit_ordinal == safe_point.code_unit_ordinal
    }
}

/// Source-free location of one retained frame. The top frame is the linked
/// dependency; the second is its distinct entry-module caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DebuggerLinkedStackFrame {
    pub frame: DebuggerLinkedFrame,
    pub safe_point: DebuggerSafePoint,
}

/// Complete child-first linked stack. An array prevents truncated, reordered,
/// or partial source disclosures from masquerading as a full graph pause.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DebuggerLinkedStackSnapshot {
    pub frames: [DebuggerLinkedStackFrame; 2],
}

impl DebuggerLinkedStackSnapshot {
    pub fn is_well_formed(self) -> bool {
        let [dependency, caller] = self.frames;
        dependency.frame.matches_safe_point(dependency.safe_point)
            && caller.frame.matches_safe_point(caller.safe_point)
            && dependency.frame.code_unit_ordinal != 0
            && caller.frame.code_unit_ordinal == 0
            && dependency.frame.program != caller.frame.program
            && dependency.frame.program.realm == caller.frame.program.realm
            && dependency.frame.core_instance == caller.frame.core_instance
            && dependency.frame.frame_handle != caller.frame.frame_handle
    }
}

/// One pending entry module and an exact verified dependency boundary. A
/// same-program nested pause uses the older request family instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DebuggerLinkedArmTarget {
    pub entry: DebuggerProgram,
    pub dependency_safe_point: DebuggerSafePoint,
}

impl DebuggerLinkedArmTarget {
    pub fn is_well_formed(self) -> bool {
        self.entry.is_well_formed()
            && self.dependency_safe_point.is_well_formed()
            && self.dependency_safe_point.code_unit_ordinal != 0
            && self.entry.realm == self.dependency_safe_point.program.realm
            && self.entry != self.dependency_safe_point.program
    }
}

/// Source-free lifecycle of one linked entry/dependency pause. A paused
/// result contains the complete two-frame stack, never just the dependency.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DebuggerLinkedExecutionState {
    Pending,
    Paused { stack: DebuggerLinkedStackSnapshot },
    Resuming { frame: DebuggerLinkedFrame },
    Completed,
}

impl DebuggerLinkedExecutionState {
    pub fn is_well_formed(self, entry: DebuggerProgram) -> bool {
        if !entry.is_well_formed() {
            return false;
        }
        match self {
            Self::Pending | Self::Completed => true,
            Self::Paused { stack } => {
                stack.is_well_formed() && stack.frames[1].frame.program == entry
            }
            Self::Resuming { frame } => {
                frame.is_well_formed()
                    && frame.code_unit_ordinal != 0
                    && frame.program.realm == entry.realm
                    && frame.program != entry
            }
        }
    }
}

/// An exact previously returned linked stack and independently inventoried
/// source ID for each frame. Possession of this value grants no span access.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DebuggerLinkedStackCoordinatesTarget {
    pub expected_stack: DebuggerLinkedStackSnapshot,
    pub sources: [DebuggerStaticMetadataSourceId; 2],
}

impl DebuggerLinkedStackCoordinatesTarget {
    pub fn is_well_formed(self) -> bool {
        self.expected_stack.is_well_formed()
            && self.sources.iter().enumerate().all(|(index, source)| {
                source.is_well_formed()
                    && source.metadata.program == self.expected_stack.frames[index].frame.program
            })
    }
}

/// All-or-nothing original coordinates for both ordered linked frames. The
/// source/metadata pair is independently bound for each returned span.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DebuggerLinkedStackCoordinates {
    pub stack: DebuggerLinkedStackSnapshot,
    pub spans: [DebuggerStaticMetadataSafePointSpan; 2],
}

impl DebuggerLinkedStackCoordinates {
    pub fn is_well_formed(self) -> bool {
        self.stack.is_well_formed()
            && self.spans.iter().enumerate().all(|(index, span)| {
                span.is_well_formed()
                    && span.safe_point == self.stack.frames[index].safe_point
                    && span.source.metadata.program == self.stack.frames[index].frame.program
            })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DebuggerScopeEntry {
    pub slot_ordinal: u32,
    pub scope_depth: u32,
}

/// One bounded entry-root scope list for a complete linked pause. It is only
/// a data shape until the separately gated public linked-scopes route lands;
/// no dependency child binding, name, value, or source is included.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DebuggerLinkedScopeSnapshot {
    pub stack: DebuggerLinkedStackSnapshot,
    pub frame_index: u32,
    pub entries: Vec<DebuggerScopeEntry>,
    pub scope_truncated: bool,
    pub max_scope_entries: u32,
}

impl DebuggerLinkedScopeSnapshot {
    pub fn is_well_formed(&self) -> bool {
        if !self.stack.is_well_formed()
            || self.frame_index != 1
            || !(1..=DEBUGGER_MAX_SCOPE_ENTRIES).contains(&self.max_scope_entries)
            || self.entries.len() > self.max_scope_entries as usize
            || (self.scope_truncated && self.entries.len() != self.max_scope_entries as usize)
        {
            return false;
        }
        let mut slots = HashSet::with_capacity(self.entries.len());
        self.entries
            .iter()
            .all(|entry| slots.insert(entry.slot_ordinal))
    }
}

/// Exact linked entry-root slot selector. An ordinary `Scopes` receipt is
/// deliberately not interchangeable with this complete-stack identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DebuggerLinkedScopeTarget {
    pub stack: DebuggerLinkedStackSnapshot,
    pub frame_index: u32,
    pub scope_entry: DebuggerScopeEntry,
}

impl DebuggerLinkedScopeTarget {
    pub fn is_well_formed(self) -> bool {
        self.stack.is_well_formed() && self.frame_index == 1
    }
}

/// One currently active frame's bounded lexical-slot inventory. It is not a
/// value lookup, binding name inventory, or source-position disclosure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DebuggerScopeSnapshot {
    pub program: DebuggerProgram,
    pub frame: Option<DebuggerFrame>,
    pub frame_index: u32,
    pub safe_point: DebuggerSafePoint,
    pub entries: Vec<DebuggerScopeEntry>,
    pub scope_truncated: bool,
}

/// Exact selector for a slot previously emitted by Scopes on this stream.
/// It is not yet a debugger request and carries no heap-object handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DebuggerValueTarget {
    pub program: DebuggerProgram,
    pub frame: Option<DebuggerFrame>,
    pub frame_index: u32,
    pub safe_point: DebuggerSafePoint,
    pub scope_entry: DebuggerScopeEntry,
}

/// A handle-free, lossless tree copied from one paused active binding.
/// `None` in an array is a hole, distinct from an explicit `Undefined`.
/// The public request/reply route still requires an independent session grant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DebuggerValuePreview {
    Undefined,
    Null,
    Bool(bool),
    NumberBits(u64),
    BigIntBytes(Vec<u8>),
    StringUnits(Vec<u16>),
    Array(Vec<Option<Self>>),
    Record(Vec<(Vec<u16>, Self)>),
}

impl DebuggerValuePreview {
    /// Rejects a partial or over-budget tree before it becomes a public
    /// reply. Traversal is iterative and checks duplicate record keys.
    pub fn is_well_formed(&self) -> bool {
        let mut pending = vec![(self, 0usize)];
        let mut nodes = 0usize;
        let mut payload_bytes = 0usize;
        while let Some((value, depth)) = pending.pop() {
            if depth > DEBUGGER_MAX_VALUE_DEPTH {
                return false;
            }
            nodes += 1;
            if nodes > DEBUGGER_MAX_VALUE_NODES {
                return false;
            }
            let bytes = match value {
                Self::Undefined | Self::Null | Self::Bool(_) | Self::NumberBits(_) => 0,
                Self::BigIntBytes(bytes) => bytes.len(),
                Self::StringUnits(units) => match units.len().checked_mul(2) {
                    Some(bytes) => bytes,
                    None => return false,
                },
                Self::Array(elements) => {
                    if elements.len() > DEBUGGER_MAX_VALUE_CONTAINER_LENGTH {
                        return false;
                    }
                    for element in elements {
                        if let Some(value) = element {
                            pending.push((value, depth + 1));
                        } else {
                            if depth + 1 > DEBUGGER_MAX_VALUE_DEPTH {
                                return false;
                            }
                            nodes += 1;
                            if nodes > DEBUGGER_MAX_VALUE_NODES {
                                return false;
                            }
                        }
                    }
                    0
                }
                Self::Record(entries) => {
                    if entries.len() > DEBUGGER_MAX_VALUE_CONTAINER_LENGTH {
                        return false;
                    }
                    let mut keys = HashSet::new();
                    for (key, value) in entries {
                        if !keys.insert(key.as_slice()) {
                            return false;
                        }
                        let Some(key_bytes) = key.len().checked_mul(2) else {
                            return false;
                        };
                        let Some(total) = payload_bytes.checked_add(key_bytes) else {
                            return false;
                        };
                        payload_bytes = total;
                        if payload_bytes > DEBUGGER_MAX_VALUE_PAYLOAD_BYTES {
                            return false;
                        }
                        pending.push((value, depth + 1));
                    }
                    0
                }
            };
            let Some(total) = payload_bytes.checked_add(bytes) else {
                return false;
            };
            payload_bytes = total;
            if payload_bytes > DEBUGGER_MAX_VALUE_PAYLOAD_BYTES {
                return false;
            }
        }
        true
    }
}

/// Exact selected Scopes slot and its complete validated preview. The target
/// is echoed to let the client reject a mismatched reply without a heap ID.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DebuggerValueSnapshot {
    pub target: DebuggerValueTarget,
    pub preview: DebuggerValuePreview,
}

impl DebuggerValueSnapshot {
    pub fn is_well_formed(&self) -> bool {
        self.target.is_well_formed() && self.preview.is_well_formed()
    }
}

impl DebuggerValueTarget {
    pub fn is_well_formed(self) -> bool {
        self.program.is_well_formed()
            && self.safe_point.is_well_formed()
            && self.safe_point.program == self.program
            && self.frame_index < DEBUGGER_MAX_STACK_FRAMES
            && match self.frame {
                None => self.frame_index == 0 && self.safe_point.code_unit_ordinal == 0,
                Some(frame) => {
                    frame.is_well_formed()
                        && frame.program == self.program
                        && (self.frame_index != 0 || frame.matches_safe_point(self.safe_point))
                }
            }
    }
}

/// A static-only selector for one exact ordinary root/parent or linked entry
/// root slot. The existing `Value` route has a separate grant and reply.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DebuggerStaticScopeTarget {
    Ordinary {
        metadata: DebuggerStaticMetadataHandle,
        target: DebuggerValueTarget,
    },
    Linked {
        metadata: DebuggerStaticMetadataHandle,
        target: DebuggerLinkedScopeTarget,
    },
}

impl DebuggerStaticScopeTarget {
    pub fn metadata(self) -> DebuggerStaticMetadataHandle {
        match self {
            Self::Ordinary { metadata, .. } | Self::Linked { metadata, .. } => metadata,
        }
    }

    pub fn is_well_formed(self) -> bool {
        match self {
            Self::Ordinary { metadata, target } => {
                metadata.is_well_formed()
                    && target.is_well_formed()
                    && metadata.program == target.program
                    && target.safe_point.code_unit_ordinal == 0
                    && matches!((target.frame, target.frame_index), (None, 0) | (Some(_), 1))
            }
            Self::Linked { metadata, target } => {
                metadata.is_well_formed()
                    && target.is_well_formed()
                    && metadata.program == target.stack.frames[1].frame.program
            }
        }
    }
}

/// An all-or-nothing compiler relation for a previously receipted active
/// slot. Both IDs stay opaque and parent-bound; no display or runtime value
/// can appear in this data-only reply shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DebuggerStaticScopeRelation {
    pub target: DebuggerStaticScopeTarget,
    pub symbol: DebuggerStaticMetadataSymbolId,
    pub static_type: DebuggerStaticMetadataTypeId,
}

impl DebuggerStaticScopeRelation {
    pub fn is_well_formed(self) -> bool {
        self.target.is_well_formed()
            && self.symbol.is_well_formed()
            && self.static_type.is_well_formed()
            && self.symbol.metadata == self.target.metadata()
            && self.static_type.metadata == self.target.metadata()
    }
}

impl DebuggerScopeSnapshot {
    pub(super) fn receipt_targets(&self) -> Option<HashSet<DebuggerValueTarget>> {
        if self.entries.len() > DEBUGGER_MAX_SCOPE_ENTRIES as usize {
            return None;
        }
        let mut targets = HashSet::with_capacity(self.entries.len());
        for scope_entry in &self.entries {
            let target = DebuggerValueTarget {
                program: self.program,
                frame: self.frame,
                frame_index: self.frame_index,
                safe_point: self.safe_point,
                scope_entry: *scope_entry,
            };
            if !target.is_well_formed() || !targets.insert(target) {
                return None;
            }
        }
        Some(targets)
    }
}
