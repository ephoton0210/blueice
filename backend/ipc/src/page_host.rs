// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Private, versioned transport between `blueice-launcher` and its isolated
//! BlueJS page host child.
//!
//! This is intentionally neither the public frontend protocol nor the narrow
//! script-to-DOM channel. A launcher creates one private Unix socket and a
//! one-time capability token for one child, then a core-owned page loader may
//! supply already-authorized source records through that connection. The
//! child never receives a filesystem path, URL to fetch, DOM handle, network
//! authority, or a resolver callback. It receives only complete source graphs
//! selected by its caller and reports only bounded, source-free outcomes.
//! Version 39 adds a source-free, exact terminal BlueTS exception-location
//! route after version 38's paused-slot, bounded plain-data value preview.
//! It is child-private and does not itself grant a public debugger client
//! value access. Version 37 adds a bounded source-free stack/scope snapshot from an exact
//! paused root or nested frame. It remains private and does not enable a
//! public debugger capability. Version 36 adds exact active nested-frame resume without reusing root
//! resume or single-instruction stepping. Version 35 adds nested-frame arm, state, and single-instruction
//! step controls for the private core/child route. The frame identity binds
//! tab, document, program, code unit, and invocation; it grants no source or
//! value inspection and is distinct from a static safe-point location.
//! Version 31 adds an authenticated, source-free child-wide actual-usage
//! snapshot for all live realms. It is private to the core/child connection,
//! separate from the launcher's conservative reservations and from public
//! debugger, frontend, and MCP protocols. Version 30 adds a child-private
//! BlueTS source-span step on a retained classic-root continuation. It
//! advances one verified root instruction per
//! owner turn, stops at a different compiler-bound source span, completion,
//! or an explicit fixed instruction-budget yield. It reveals no source text,
//! runtime value, stack, or VM frame. Public debugger v30 separately gates its
//! use with owner/client capability grants and same-stream source receipts.
//! Version 29 adds a child-private, bounded BlueTS byte-position-to-safe-point
//! binding under an exact live metadata attachment. It preserves explicitly
//! unbound lowering spans and does not itself grant a public debugger client
//! an arbitrary source-position query or execution control.
//! Version 28 adds a child-private exact safe-point-to-BlueTS byte-span lookup
//! under one live opaque metadata attachment. It returns only a compiler
//! source ID and bounded original byte range, never source text, module
//! identity, a guessed nearest span, or a public debugger capability.
//! Version 27 adds an exact-program, source-free single-root-instruction
//! debugger step and a transient `Stepping` state for the isolated child.
//! Version 26 adds bounded compiler-produced original-source UTF-16
//! coordinates to the existing child-private symbol and contract location
//! replies. Version 25 adds the compiler's export classification to the existing
//! child-private symbol display. Version 24 adds a fixed root-shape
//! classification to the existing contract-display reply for an exact
//! child-private metadata attachment.
//!
//! Version 11 retains the two fixed, core-derived document snapshots consumed
//! by the child-owned JavaScript bindings, the location-only debugger
//! inventory, a bounded exact-breakpoint configuration table, and an opt-in
//! root-classic continuation seam. The
//! transport has no profile or capability
//! selector: every accepted document contains exactly the immutable text and
//! canonical-origin copies selected by core.
//! BlueTS stays a child-fixed, direct-lowering profile with no ambient host
//! typings, compiler option, resolver, or emitted JavaScript crossing this
//! channel. Apart from the two fixed JavaScript primitive snapshot callbacks,
//! version 10 exposes only a core-proxied, source-free debugger location
//! inventory and configuration records. A core-selected document may opt in
//! to the root-classic arm/state/resume lifecycle and, since v27, one
//! root-instruction step per explicit request; the child admits no generic
//! interruption, nested-frame stepping, stack, scope,
//! bytecode, source-text, runtime-value transport, general host callback,
//! fetch/cache, or client-facing API. Version 7 additionally lets that
//! authenticated core enumerate one separately minted opaque static-metadata
//! handle for an exact BlueTS program. The handle discloses neither static
//! metadata nor a child program identity and is unusable after its realm is
//! replaced or closed. Version 8 adds only an explicitly requested,
//! handle-bound static summary: fixed compiler fingerprints and aggregate
//! counts, never a source identity/text, span, name, type display, symbol,
//! contract, bytecode, runtime value, or dereferenceable metadata record.
//! Version 9 adds only bounded compiler-minted source-record IDs for that
//! same handle; IDs carry no source identity, hash, text, or record detail.
//! Version 10 adds the separately authorized source-provenance reply for one
//! of those IDs: canonical module identity and a labeled SHA-256 digest only,
//! never source text or a general static-record read.
//! Version 11 adds only a parent-handle-bound compiler type-ID inventory.
//! Version 12 adds one exact type-ID display lookup; it remains an explicit
//! core-proxied operation, not a general static-record read or source access.
//! Version 13 adds a payload-free parent-handle-bound symbol-ID inventory.
//! Version 14 adds a payload-free parent-handle-bound contract-ID inventory.
//! Version 15 adds one exact symbol-ID display lookup; it remains an explicit
//! core-proxied operation, not a general static-record read or source access.
//! Version 16 adds one exact contract-ID display lookup; it remains an explicit
//! core-proxied operation, not a general static-record read or source access.
//! Version 23 adds an exact contract/source declaration range without a
//! contract plan, source text, module identity, or runtime value.
//! Version 22 adds the compiler declaration kind to the exact child-local
//! symbol display. It does not add a target, source read, or runtime access.
//! Version 21 adds an exact symbol-to-reifiable-contract verification under
//! one private metadata attachment. It repeats only two caller-supplied
//! opaque IDs and does not disclose the contract plan or static record.
//! Version 20 adds an exact symbol-to-static-type verification under one
//! private metadata attachment. It repeats only two caller-supplied opaque
//! IDs and does not disclose type displays or static records. Version 19 adds
//! a separately requested, opaque-handle-bound symbol
//! location. It carries only a receipted source ID and bounded half-open byte
//! range, never source text, module identity, line/column data, name, type,
//! contract, bytecode, VM object, or value. Version 18 adds a separately
//! requested, opaque-handle-bound summary of the
//! verified BlueTS-to-BlueJS lowering map. It carries only fixed ABI labels,
//! a source-set fingerprint, and an aggregate bound-entry count; source
//! identities/spans, map entries, AST nodes, and bytecode offsets remain
//! private. Version 17 adds a separately requested, data-only validation against one
//! exact prior contract ID. Its reply is only a boolean, never the input,
//! contract plan, or structural failure detail.

use crate::compiler::CompilerContractValue;
use crate::debugger::{
    DebuggerSourceCoordinates, DebuggerStaticMetadataContractRootKind,
    DebuggerStaticMetadataSymbolKind,
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::io::{self, Read, Write};

/// Independent version for the private launcher-to-BlueJS-host channel.
/// V34 carries a document-bound script handle, not a raw core NodeId, in
/// `DispatchClick` so its target matches child-owned listener identities.
pub const PAGE_HOST_PROTOCOL_VERSION: u32 = 39;

pub const PAGE_HOST_DEBUGGER_MAX_STACK_FRAMES: u32 = 64;
pub const PAGE_HOST_DEBUGGER_MAX_SCOPE_ENTRIES: u32 = 256;
pub const PAGE_HOST_DEBUGGER_MAX_VALUE_DEPTH: u32 = 4;
pub const PAGE_HOST_DEBUGGER_MAX_VALUE_CONTAINER_LENGTH: usize = 32;
pub const PAGE_HOST_DEBUGGER_MAX_VALUE_NODES: usize = 256;
pub const PAGE_HOST_DEBUGGER_MAX_VALUE_PAYLOAD_BYTES: usize = 4_096;

/// Maximum private page-host request/reply frame. The child rejects a length
/// above this cap before allocating a payload buffer or deserializing source.
/// This is separate from per-document source budgets enforced by the child.
pub const PAGE_HOST_MAX_FRAME_BYTES: usize = 12 * 1024 * 1024;

/// The exact string budgets for the two fixed core-to-child document
/// snapshots. `blueice-engine` uses the same values in its pure host-binding
/// contract inventory before it serializes a document; the child repeats the
/// byte checks before it replaces a realm. There is deliberately no generic
/// binding-value transport with caller-controlled limits.
pub const PAGE_HOST_DOCUMENT_TEXT_MAX_BYTES: usize = 1_048_576;
pub const PAGE_HOST_DOCUMENT_ORIGIN_MAX_BYTES: usize = 4 * 1_024;

/// Maximum exact instruction boundaries returned for one child-owned program
/// by the private debugger-location inventory. It is an immutable child
/// policy, not a client-provided request limit.
pub const PAGE_HOST_DEBUGGER_MAX_SAFE_POINTS_PER_PROGRAM: u32 = 4_096;

/// Maximum exact breakpoint configuration records retained for one child
/// realm. The cap is fixed by this private protocol; neither the public
/// debugger nor page code can grow the child table without bound.
pub const PAGE_HOST_DEBUGGER_MAX_BREAKPOINTS_PER_REALM: u32 = 256;

/// A document accepts at most 256 declarations and each declaration can
/// carry at most eight closed graph modules. The accounting reply cannot name
/// more live BlueJS programs than that fixed document envelope permits.
pub const PAGE_HOST_REALM_STATS_MAX_PROGRAMS: u32 = 2_048;

/// An opaque debugger program identity minted by the isolated child. It is
/// valid only with the exact tab/document generation supplied by the request;
/// it deliberately contains no source identity, BlueJS registry handle, or
/// bytecode data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PageHostDebuggerProgram {
    pub program_handle: u64,
    pub program_generation: u64,
}

impl PageHostDebuggerProgram {
    /// Private debugger identities never use zero placeholders.
    pub fn is_well_formed(self) -> bool {
        self.program_handle != 0 && self.program_generation != 0
    }
}

/// A child-minted opaque association with static BlueTS metadata for one
/// exact live direct-program generation. It is deliberately a different
/// identity namespace from [`PageHostDebuggerProgram`]: callers cannot reuse
/// a program ID as a metadata ID, nor derive source/module/type/span/contract
/// information from either field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PageHostDebuggerMetadataHandle {
    pub metadata_handle: u64,
    pub metadata_generation: u64,
}

impl PageHostDebuggerMetadataHandle {
    /// Private metadata identities never use zero placeholders.
    pub fn is_well_formed(self) -> bool {
        self.metadata_handle != 0 && self.metadata_generation != 0
    }
}

/// A bounded source-free description of a live BlueTS debug attachment.
/// This private transport structure intentionally has no child program or
/// metadata handle: the enclosing request/reply supplies those opaque
/// identities and core verifies every component before reminting the public
/// summary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageHostDebuggerBlueTsMetadataSummary {
    pub language_version: String,
    pub compiler_options_hash: String,
    pub source_count: u32,
    pub type_count: u32,
    pub symbol_count: u32,
    pub contract_count: u32,
}

/// Source-free aggregate evidence for the exact retained direct-lowering map
/// under one opaque metadata handle. The enclosing request/reply binds this
/// to a child-private program and metadata identity. It deliberately contains
/// no source/module identity, source span, map entry, AST node, code-unit ID,
/// bytecode offset, VM object/value, or static-record payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageHostDebuggerBlueTsMetadataLoweringSummary {
    pub safe_point_map_abi: String,
    pub program_abi: String,
    pub source_set_hash: String,
    pub bound_safe_point_count: u32,
}

/// One compiler-minted source-record ID for an exact private BlueTS metadata
/// attachment. It deliberately carries no module identity, source text,
/// content hash, span, symbol, type, contract, bytecode, VM object, or value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PageHostDebuggerBlueTsMetadataSourceId {
    pub source_id: u32,
}

/// One compiler-minted type-record ID for an exact private BlueTS metadata
/// attachment. It deliberately carries no display string, source identity,
/// span, symbol, contract, bytecode, VM object, or value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PageHostDebuggerBlueTsMetadataTypeId {
    pub type_id: u32,
}

/// One compiler-minted symbol-record ID for an exact private BlueTS metadata
/// attachment. It deliberately carries no name, source span, declared type,
/// contract, bytecode, VM object, or value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PageHostDebuggerBlueTsMetadataSymbolId {
    pub symbol_id: u32,
}

/// One compiler-minted static contract ID for an exact private BlueTS metadata
/// attachment. It deliberately carries no contract name, source span, plan,
/// validation, bytecode, VM object, or value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PageHostDebuggerBlueTsMetadataContractId {
    pub contract_id: u32,
}

/// One child-local compiler-produced contract display for an exact contract ID
/// under an opaque metadata attachment. The enclosing request/reply carries
/// the child program and metadata identities; this value never grants a
/// generic static-record read, contract-plan access, or validation authority.
/// Its fixed root-kind field has no plan edges or field names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageHostDebuggerBlueTsMetadataContractDisplay {
    pub contract_id: u32,
    pub display: String,
    pub root_kind: DebuggerStaticMetadataContractRootKind,
}

/// One child-local boolean result for validating a data-only snapshot against
/// an exact contract ID. It exposes neither the submitted value nor plan/error
/// detail; the enclosing reply binds it to one private program and metadata
/// handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageHostDebuggerBlueTsMetadataContractValidation {
    pub contract_id: u32,
    pub valid: bool,
}

/// One child-local compiler-produced symbol display for an exact symbol ID
/// under an opaque metadata attachment, with its bounded declaration kind.
/// The enclosing request/reply carries the child program and metadata
/// identities; this value never grants a generic static-record read or source
/// access.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageHostDebuggerBlueTsMetadataSymbolDisplay {
    pub symbol_id: u32,
    pub display: String,
    pub kind: DebuggerStaticMetadataSymbolKind,
    pub exported: bool,
}

/// One child-local source-text-free declaration range for an exact symbol.
/// The request/reply tuple owns the private program and metadata identities;
/// this value contains neither a module name nor source bytes and is not a
/// source-read or source-map operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageHostDebuggerBlueTsMetadataSymbolLocation {
    pub symbol_id: u32,
    pub source_id: u32,
    pub start_byte: u32,
    pub end_byte: u32,
    pub coordinates: DebuggerSourceCoordinates,
}

/// One child-local, source-text-free declaration range for a retained
/// reifiable contract. Core must check the echoed contract/source IDs against
/// the exact public stream's separate receipts before forwarding it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageHostDebuggerBlueTsMetadataContractLocation {
    pub contract_id: u32,
    pub source_id: u32,
    pub start_byte: u32,
    pub end_byte: u32,
    pub coordinates: DebuggerSourceCoordinates,
}

/// A child-private exact lowering association and original UTF-16 coordinates
/// for one verified instruction.
/// The enclosing reply repeats the live safe-point and metadata handles; this
/// payload contains no module identity, source text, AST node, or VM value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageHostDebuggerBlueTsSafePointSpan {
    pub source_id: u32,
    pub start_byte: u32,
    pub end_byte: u32,
    pub coordinates: DebuggerSourceCoordinates,
}

/// One child-private, terminal uncaught BlueTS location. Its safe point and
/// original coordinates contain no thrown value, error text, or source text.
/// Core must revalidate and remint both components before public disclosure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageHostDebuggerBlueTsExceptionLocation {
    pub safe_point: PageHostDebuggerSafePoint,
    pub span: PageHostDebuggerBlueTsSafePointSpan,
}

/// One exact child-verified relation between a compiler symbol and type.
/// Both numeric IDs came from separate core-proxied inventories; this
/// contains no name, type display, source, span, contract, or record payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageHostDebuggerBlueTsMetadataSymbolType {
    pub symbol_id: u32,
    pub type_id: u32,
}

/// One exact child-verified relation between a compiler symbol and its
/// reifiable contract. Both numeric IDs came from separate core-proxied
/// inventories; this contains no contract plan, name, or static record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageHostDebuggerBlueTsMetadataSymbolContract {
    pub symbol_id: u32,
    pub contract_id: u32,
}

/// One child-local compiler-produced type display for an exact type ID under
/// an opaque metadata attachment. The enclosing request/reply carries the
/// child program and metadata identities; this value never grants a generic
/// static-record read or source access.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageHostDebuggerBlueTsMetadataTypeDisplay {
    pub type_id: u32,
    pub display: String,
}

/// One child-local, source-text-free provenance description for a source ID
/// under an exact private metadata attachment. Core validates and remints the
/// enclosing public identities; this value never grants source access.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageHostDebuggerBlueTsMetadataSourceProvenance {
    pub source_id: u32,
    pub module: String,
    pub content_hash: String,
}

/// One exact compiler-verified instruction boundary returned without source
/// text or bytecode. The child validates this complete tuple; it never maps a
/// caller-supplied nearest offset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PageHostDebuggerSafePoint {
    pub program: PageHostDebuggerProgram,
    pub code_unit_ordinal: u32,
    pub bytecode_offset: u32,
}

/// One actually paused child invocation, not a configured code-unit point.
/// These numbers are an exact-match identity, not a secret or a VM address.
/// A child issues the tuple only after the page runtime retains the frame;
/// a successor document or another invocation cannot inherit it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PageHostDebuggerFrame {
    pub tab_id: u64,
    pub document_generation: u64,
    pub program: PageHostDebuggerProgram,
    pub code_unit_ordinal: u32,
    pub invocation_serial: u64,
}

impl PageHostDebuggerFrame {
    /// Rejects absent identity components and root code units. Callers must
    /// additionally compare the whole tuple with the current paused frame.
    pub fn is_well_formed(self) -> bool {
        self.tab_id != 0
            && self.document_generation != 0
            && self.program.is_well_formed()
            && self.code_unit_ordinal != 0
            && self.invocation_serial != 0
    }

    pub fn matches_safe_point(self, safe_point: PageHostDebuggerSafePoint) -> bool {
        self.is_well_formed()
            && self.program == safe_point.program
            && self.code_unit_ordinal == safe_point.code_unit_ordinal
    }
}

/// Source-free binding-slot inventory copied from an actually active scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageHostDebuggerScopeEntry {
    pub slot_ordinal: u32,
    pub scope_depth: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageHostDebuggerStackFrame {
    pub code_unit_ordinal: u32,
    pub bytecode_offset: u32,
    pub scope_entries: Vec<PageHostDebuggerScopeEntry>,
    pub scope_truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageHostDebuggerStackSnapshot {
    pub frames: Vec<PageHostDebuggerStackFrame>,
    pub stack_truncated: bool,
}

/// One exact active slot in a retained debugger continuation. Core and child
/// must revalidate the entire target against the live paused stack; these
/// fields are identities, not authority tokens or heap-object handles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageHostDebuggerValueTarget {
    pub tab_id: u64,
    pub document_generation: u64,
    pub program: PageHostDebuggerProgram,
    pub frame: Option<PageHostDebuggerFrame>,
    /// Zero selects the child when `frame` is present; one selects its root.
    pub frame_index: u32,
    pub safe_point: PageHostDebuggerSafePoint,
    pub scope_entry: PageHostDebuggerScopeEntry,
}

impl PageHostDebuggerValueTarget {
    pub fn is_well_formed(self) -> bool {
        self.tab_id != 0
            && self.document_generation != 0
            && self.program.is_well_formed()
            && self.safe_point.program == self.program
            && match self.frame {
                None => self.frame_index == 0 && self.safe_point.code_unit_ordinal == 0,
                Some(frame) => {
                    frame.is_well_formed()
                        && frame.tab_id == self.tab_id
                        && frame.document_generation == self.document_generation
                        && frame.program == self.program
                        && match self.frame_index {
                            0 => self.safe_point.code_unit_ordinal == frame.code_unit_ordinal,
                            1 => self.safe_point.code_unit_ordinal == 0,
                            _ => false,
                        }
                }
            }
    }
}

/// Lossless, handle-free payload copied from stored own data in a paused VM.
/// String and record keys are UTF-16 code units, including lone surrogates;
/// number bits preserve non-finite values and negative zero. Array `None` is
/// a hole, distinct from an explicit `Undefined` element.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PageHostDebuggerValuePreview {
    Undefined,
    Null,
    Bool(bool),
    NumberBits(u64),
    BigIntBytes(Vec<u8>),
    StringUnits(Vec<u16>),
    Array(Vec<Option<Self>>),
    Record(Vec<(Vec<u16>, Self)>),
}

impl PageHostDebuggerValuePreview {
    /// Checks the whole tree iteratively before any core remint or public
    /// reply. No partial/truncated preview is a valid result.
    pub fn is_well_formed(&self) -> bool {
        let mut pending = vec![(self, 0)];
        let mut nodes = 0usize;
        let mut payload_bytes = 0usize;
        while let Some((value, depth)) = pending.pop() {
            if depth > PAGE_HOST_DEBUGGER_MAX_VALUE_DEPTH {
                return false;
            }
            nodes += 1;
            if nodes > PAGE_HOST_DEBUGGER_MAX_VALUE_NODES {
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
                    if elements.len() > PAGE_HOST_DEBUGGER_MAX_VALUE_CONTAINER_LENGTH {
                        return false;
                    }
                    for element in elements {
                        if let Some(value) = element {
                            pending.push((value, depth + 1));
                        } else {
                            if depth + 1 > PAGE_HOST_DEBUGGER_MAX_VALUE_DEPTH {
                                return false;
                            }
                            nodes += 1;
                            if nodes > PAGE_HOST_DEBUGGER_MAX_VALUE_NODES {
                                return false;
                            }
                        }
                    }
                    0
                }
                Self::Record(entries) => {
                    if entries.len() > PAGE_HOST_DEBUGGER_MAX_VALUE_CONTAINER_LENGTH {
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
                        if payload_bytes > PAGE_HOST_DEBUGGER_MAX_VALUE_PAYLOAD_BYTES {
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
            if payload_bytes > PAGE_HOST_DEBUGGER_MAX_VALUE_PAYLOAD_BYTES {
                return false;
            }
        }
        true
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageHostDebuggerValueSnapshot {
    pub target: PageHostDebuggerValueTarget,
    pub preview: PageHostDebuggerValuePreview,
}

impl PageHostDebuggerValueSnapshot {
    pub fn is_well_formed(&self) -> bool {
        self.target.is_well_formed() && self.preview.is_well_formed()
    }
}

/// Source-free lifecycle state for the one-shot root-classic continuation
/// seam. A `Paused` location is always an exact child-validated root safe
/// point; no frame, scope, runtime value, source, or bytecode is serialized.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PageHostDebuggerExecutionState {
    Pending,
    Paused {
        safe_point: PageHostDebuggerSafePoint,
    },
    NestedPaused {
        frame: PageHostDebuggerFrame,
        safe_point: PageHostDebuggerSafePoint,
    },
    Stepping,
    NestedStepping {
        frame: PageHostDebuggerFrame,
    },
    NestedResuming {
        frame: PageHostDebuggerFrame,
    },
    /// The source-span step reached its fixed root-instruction budget before
    /// another bound span. The continuation remains paused at this exact
    /// verified root boundary and may be resumed or instruction-stepped.
    SourceStepLimitReached {
        safe_point: PageHostDebuggerSafePoint,
    },
    Resuming,
    Completed,
}

impl PageHostDebuggerSafePoint {
    /// The nested opaque program identity is required for every location.
    pub fn is_well_formed(self) -> bool {
        self.program.is_well_formed()
    }
}

/// A complete source record selected and fingerprinted by the caller-owned
/// page loader. `source_hash` is verified by the child against `source`; it
/// is never a client-supplied assertion that the child blindly trusts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageHostSource {
    pub canonical_module_id: String,
    pub source: String,
    pub source_hash: String,
}

impl PageHostSource {
    /// Creates a source record with the v1 deterministic FNV-1a content
    /// fingerprint. This detects accidental record mismatches and gives the
    /// program registry a stable identity; it is not cryptographic integrity
    /// and does not replace the caller's integrity, CSP, cache, or origin
    /// policy.
    pub fn new(canonical_module_id: impl Into<String>, source: impl Into<String>) -> Self {
        let source = source.into();
        Self {
            canonical_module_id: canonical_module_id.into(),
            source_hash: source_hash(&source),
            source,
        }
    }
}

/// One caller-authorized static import/export resolution. The child validates
/// this exact record against every static request; it never performs relative
/// URL, import-map, filesystem, package-manager, or network resolution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageHostStaticResolution {
    pub from_module: String,
    pub specifier: String,
    pub canonical_target: String,
}

/// A closed, caller-authorized source graph for one page declaration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageHostModuleGraph {
    pub entry: String,
    pub modules: Vec<PageHostSource>,
    pub resolutions: Vec<PageHostStaticResolution>,
    /// Caller-owned identity for the origin/integrity/cache resolver policy.
    /// It is recorded only as an opaque non-empty fingerprint in this v1
    /// transport; the child does not interpret it or gain the resolver.
    pub resolver_fingerprint: String,
}

/// Script grammar selected by the trusted page pipeline, never by a filename
/// suffix or the child host's own MIME sniffing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PageHostScriptKind {
    Classic,
    Module,
}

/// The source language selected by the trusted page pipeline. An ordinary
/// JavaScript declaration never becomes BlueTS merely because its source is
/// syntactically accepted by the compiler.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PageHostScriptLanguage {
    JavaScript,
    BlueTs,
}

/// One declaration in document order. Its graph is fully supplied before the
/// child parses or executes anything.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageHostScript {
    pub ordinal: u32,
    pub language: PageHostScriptLanguage,
    pub kind: PageHostScriptKind,
    pub graph: PageHostModuleGraph,
}

/// The only core-to-child values made available to ordinary JavaScript page
/// code. They are copied primitive snapshots, not DOM handles, URL objects,
/// resolver capabilities, or a page-selected binding profile.
///
/// `document_origin` must be the canonical HTTP(S) tuple origin. The child
/// checks its canonical spelling before using it as either realm identity or
/// callback result; a missing/empty or over-budget snapshot fails closed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageHostDocumentSnapshot {
    pub document_text: String,
    pub document_origin: String,
}

/// One caller-authorized document snapshot. `snapshot` is supplied only by
/// the core-owned page lifecycle adapter after it has validated the live DOM
/// text and canonical origin. A page cannot add, remove, rename, or widen
/// these bindings through this private protocol.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageHostDocument {
    pub tab_id: u64,
    pub document_generation: u64,
    pub snapshot: PageHostDocumentSnapshot,
    /// Set only by the authenticated core lifecycle owner. When true, the
    /// child holds document-order declarations until its next explicit
    /// advance turn so core can inspect and arm an exact root-classic safe
    /// point. Page content and frontend IPC cannot select this mode.
    pub debugger_execution_control: bool,
    pub scripts: Vec<PageHostScript>,
}

/// Source-free outcome for one attempted declaration. These records are safe
/// to return to the launcher/core but are not a runtime-value or diagnostic
/// transport.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageHostScriptReport {
    pub tab_id: u64,
    pub document_generation: u64,
    pub ordinal: u32,
    pub language: PageHostScriptLanguage,
    pub kind: PageHostScriptKind,
    pub outcome: PageHostScriptOutcome,
}

/// Bounded outcome category. The child owns these fixed labels; it never
/// forwards parser/compiler/runtime error text or page source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PageHostScriptOutcome {
    Executed,
    Rejected { category: String },
}

/// Safe, aggregate accounting for one live child-owned realm. Heap object
/// identities and bytecode/source remain private to the child.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageHostRealmStats {
    pub tab_id: u64,
    pub document_generation: u64,
    pub program_count: u32,
    pub bytecode_bytes: u64,
    pub heap_bytes: u64,
}

impl PageHostRealmStats {
    /// Checks the fixed, source-free accounting envelope before core caches a
    /// child report. `u32::MAX`/`u64::MAX` are conversion-failure sentinels in
    /// the child adapter, never credible live accounting values. This does
    /// not disclose the launcher's private resource limits.
    pub fn is_well_formed(&self) -> bool {
        self.tab_id != 0
            && self.document_generation != 0
            && self.program_count <= PAGE_HOST_REALM_STATS_MAX_PROGRAMS
            && self.bytecode_bytes != u64::MAX
            && self.heap_bytes != u64::MAX
    }
}

/// Actual source-free usage across every currently live child-owned realm.
/// This excludes Rust allocation, source/cache copies, registry overhead,
/// process RSS, and other children. It conveys no tab or program identity and
/// is never a public debugger/frontend/MCP reply.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageHostChildStats {
    pub realm_count: u32,
    pub program_count: u64,
    pub bytecode_bytes: u64,
    pub heap_bytes: u64,
}

impl PageHostChildStats {
    /// Rejects conversion sentinels, impossible program counts, and a
    /// nonempty charge for an empty child before core trusts the snapshot.
    pub fn is_well_formed(self) -> bool {
        self.realm_count != u32::MAX
            && self.program_count != u64::MAX
            && self.bytecode_bytes != u64::MAX
            && self.heap_bytes != u64::MAX
            && self.program_count
                <= u64::from(self.realm_count) * u64::from(PAGE_HOST_REALM_STATS_MAX_PROGRAMS)
            && (self.program_count != 0 || self.bytecode_bytes == 0)
            && (self.realm_count != 0
                || (self.program_count == 0 && self.bytecode_bytes == 0 && self.heap_bytes == 0))
    }
}

/// Launcher/core requests to the private host. `Hello` carries the per-spawn
/// secret capability so a same-user process that guesses a socket pathname
/// cannot claim the child before its launcher does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PageHostRequest {
    Hello {
        protocol_version: u32,
        session_token: String,
    },
    /// Applies a new document only if its generation succeeds the tab's
    /// current child-owned generation. Repeating the current generation is
    /// idempotent and never re-runs page code.
    SynchronizeDocument { document: PageHostDocument },
    /// Delivers one core-hit-tested node to the exact live child document.
    /// `node_id` is the child-private script handle for that hit, not its raw
    /// core DOM NodeId. The child returns only whether a listener canceled
    /// default navigation.
    DispatchClick {
        tab_id: u64,
        document_generation: u64,
        node_id: u64,
    },
    /// Releases one exact live realm. A stale generation cannot close its
    /// successor after navigation.
    CloseRealm {
        tab_id: u64,
        document_generation: u64,
    },
    /// Returns only bounded, aggregate accounting for one exact live realm.
    GetRealmStats {
        tab_id: u64,
        document_generation: u64,
    },
    /// Returns one authenticated child-wide actual-usage snapshot. It has no
    /// caller-selected realm, source, or runtime-object target.
    GetChildStats,
    /// Lists only opaque program identities retained by one exact live child
    /// realm. This discovery operation cannot pause, resume, inspect, or
    /// mutate that realm.
    ListDebuggerPrograms {
        tab_id: u64,
        document_generation: u64,
    },
    /// Lists separately minted opaque static-metadata handles for one exact
    /// private program. A JavaScript program, a BlueTS program without a live
    /// registry attachment, or an invalidated attachment returns no handles.
    /// The reply contains no static metadata, source/module/name/type/span,
    /// contract, aggregate count, runtime handle, or VM object.
    ListDebuggerBlueTsMetadata {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
    },
    /// Describes one previously minted private BlueTS metadata handle. The
    /// request repeats its owning program so the child can reject a handle
    /// from another live program without probing its registry. It returns
    /// only a bounded fingerprint/count summary, never a metadata record.
    DescribeDebuggerBlueTsMetadata {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
    },
    /// Returns only aggregate ABI/fingerprint evidence for the verified direct
    /// lowering map paired with one exact opaque metadata handle. This is not
    /// an entry, source span, AST-node, or bytecode inspection operation.
    DescribeDebuggerBlueTsMetadataLoweringSummary {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
    },
    /// Lists compiler-minted source-record IDs for a prior exact private
    /// metadata handle. This is not a source/provenance record read.
    ListDebuggerBlueTsMetadataSources {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
    },
    /// Lists compiler-minted type-record IDs for a prior exact private
    /// metadata handle. This is not a type display or static-record read.
    ListDebuggerBlueTsMetadataTypes {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
    },
    /// Describes one compiler-minted type ID under a prior exact private
    /// metadata handle. It returns one bounded display only; source text,
    /// spans, symbols, contracts, bytecode, VM objects, and values remain in
    /// the child.
    DescribeDebuggerBlueTsMetadataType {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        type_id: u32,
    },
    /// Lists compiler-minted symbol-record IDs for a prior exact private
    /// metadata handle. This is not a symbol-name, span, type, or record read.
    ListDebuggerBlueTsMetadataSymbols {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
    },
    /// Lists compiler-minted contract IDs for a prior exact private metadata
    /// handle. This is not a contract-name, span, plan, or validation read.
    ListDebuggerBlueTsMetadataContracts {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
    },
    /// Describes a compiler-minted contract ID under a prior exact private
    /// metadata handle. This is not a contract span, plan, validation, or
    /// record read.
    DescribeDebuggerBlueTsMetadataContract {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        contract_id: u32,
    },
    /// Validates a core-forwarded data-only snapshot against one exact prior
    /// contract ID. The child applies fixed limits and returns only a boolean.
    ValidateDebuggerBlueTsMetadataContract {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        contract_id: u32,
        value: CompilerContractValue,
    },
    /// Describes a compiler-minted symbol ID under a prior exact private
    /// metadata handle. This is not a symbol span/type/contract or record read.
    DescribeDebuggerBlueTsMetadataSymbol {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        symbol_id: u32,
    },
    /// Describes one bounded half-open source byte range for a prior exact
    /// symbol ID. It has no source/module/name/type/contract/bytecode payload.
    DescribeDebuggerBlueTsMetadataSymbolLocation {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        symbol_id: u32,
    },
    /// Describes one bounded contract declaration range under the exact
    /// child-local program and metadata attachment; no plan or source bytes.
    DescribeDebuggerBlueTsMetadataContractLocation {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        contract_id: u32,
    },
    /// Resolves one already-verified child safe point to an exact retained
    /// BlueTS lowering span under its live private metadata attachment.
    /// There is no caller-selected source offset or nearest-match behavior.
    DescribeDebuggerBlueTsSafePointSpan {
        tab_id: u64,
        document_generation: u64,
        metadata: PageHostDebuggerMetadataHandle,
        safe_point: PageHostDebuggerSafePoint,
    },
    /// Reads only the terminal uncaught location retained for this exact
    /// private BlueTS program and metadata attachment, never a VM value.
    DescribeDebuggerBlueTsExceptionLocation {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
    },
    /// Resolves one bounded original BlueTS byte position in a child-minted
    /// source record to the first lowering span at or after it. A bound reply
    /// names only a compiler-verified safe point; an unbound span or missing
    /// following span returns `None` instead of guessing another instruction.
    ResolveDebuggerBlueTsSourceBreakpoint {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        source_id: u32,
        source_byte: u32,
    },
    /// Verifies exactly one symbol-to-static-type pair under the same private
    /// attachment. The reply does not return an unrequested type ID.
    DescribeDebuggerBlueTsMetadataSymbolType {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        symbol_id: u32,
        type_id: u32,
    },
    /// Verifies exactly one symbol-to-contract pair under the same private
    /// attachment. The reply does not return an unrequested contract ID.
    DescribeDebuggerBlueTsMetadataSymbolContract {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        symbol_id: u32,
        contract_id: u32,
    },
    /// Describes exactly one prior compiler-minted source ID. This private
    /// request returns module identity and a digest only, never source text.
    DescribeDebuggerBlueTsMetadataSource {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        source_id: u32,
    },
    /// Lists the child's bounded compiler-verified safe points for one exact
    /// opaque program. No source, bytecode, VM, or value crosses this channel.
    ListDebuggerSafePoints {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
    },
    /// Revalidates one exact source-free child safe-point tuple. It does not
    /// execute, pause, or otherwise alter the child VM.
    ValidateDebuggerSafePoint {
        tab_id: u64,
        document_generation: u64,
        safe_point: PageHostDebuggerSafePoint,
    },
    /// Stores one exact compiler-verified safe point in the child-owned
    /// configuration table. It neither executes nor interrupts a realm.
    SetDebuggerBreakpoint {
        tab_id: u64,
        document_generation: u64,
        safe_point: PageHostDebuggerSafePoint,
    },
    /// Lists the exact child-private breakpoint records for one live realm.
    /// The core re-mints every program identity before returning it publicly.
    ListDebuggerBreakpoints {
        tab_id: u64,
        document_generation: u64,
    },
    /// Removes one exact compiler-verified child-private breakpoint record.
    /// Removal is idempotent, but the tuple must remain valid for the current
    /// realm rather than naming a successor or nearest instruction boundary.
    ClearDebuggerBreakpoint {
        tab_id: u64,
        document_generation: u64,
        safe_point: PageHostDebuggerSafePoint,
    },
    /// Arms one exact root-code-unit location for a classic declaration that
    /// remains pending in a core-selected execution-control document. This is
    /// distinct from breakpoint configuration: it starts execution only on a
    /// later explicit advance turn and can retain one root-frame continuation.
    ArmDebuggerRootSafePointBreakpoint {
        tab_id: u64,
        document_generation: u64,
        safe_point: PageHostDebuggerSafePoint,
    },
    /// Arms a verified child-code-unit target on one still-pending classic
    /// or BlueTS entry-module declaration. It does not itself mint a frame.
    ArmDebuggerNestedSafePointBreakpoint {
        tab_id: u64,
        document_generation: u64,
        safe_point: PageHostDebuggerSafePoint,
    },
    /// Reads only source-free lifecycle state for one exact child-private
    /// program generation in the opt-in root-classic continuation seam.
    GetDebuggerExecutionState {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
    },
    /// Marks one paused root-classic continuation for execution on the next
    /// core-owned advance turn. It cannot inject a value or exception.
    ResumeDebuggerExecution {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
    },
    /// Schedules one root instruction on the retained classic continuation;
    /// the next advance yields a verified root boundary or completion.
    StepDebuggerRootInstruction {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
    },
    /// Steps only the exact currently paused nested invocation on a later
    /// child advance turn; a static safe point is never sufficient.
    StepDebuggerNestedInstruction { frame: PageHostDebuggerFrame },
    /// Resumes only the exact retained nested invocation on a later child
    /// advance turn; the waiting root remains separately paused on return.
    ResumeDebuggerNestedExecution { frame: PageHostDebuggerFrame },
    /// Inspects only the exact paused root (`None`) or the currently active
    /// nested invocation (`Some`). Zero or over-cap limits are invalid.
    GetDebuggerStackSnapshot {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        frame: Option<PageHostDebuggerFrame>,
        max_frames: u32,
        max_scope_entries: u32,
    },
    /// Copies only one exact active lexical slot in a paused BlueTS frame.
    /// This private route does not accept an object handle or run JavaScript.
    GetDebuggerValueSnapshot { target: PageHostDebuggerValueTarget },
    /// Requires an exact paused BlueTS classic safe point, live metadata,
    /// and its compiler-minted source ID. The child derives the current span
    /// from its retained map; no source text or caller-selected stop span is
    /// accepted. This private operation does not authorize a public client.
    StepDebuggerBlueTsSourceSpan {
        tab_id: u64,
        document_generation: u64,
        metadata: PageHostDebuggerMetadataHandle,
        source_id: u32,
        safe_point: PageHostDebuggerSafePoint,
    },
    /// Advances one exact opted-in realm in document order. It returns only
    /// fixed execution categories and never exposes a completion value.
    AdvanceDebuggerExecution {
        tab_id: u64,
        document_generation: u64,
    },
    /// Ends the child process after its acknowledgement.
    Shutdown,
    /// A newer request must not be interpreted as an existing operation.
    #[serde(other)]
    Unknown,
}

/// Child replies for [`PageHostRequest`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PageHostReply {
    HelloAck {
        protocol_version: u32,
    },
    Synchronized {
        tab_id: u64,
        document_generation: u64,
        /// `true` when the child already owned this exact document and did
        /// not execute any declaration again.
        already_current: bool,
        reports: Vec<PageHostScriptReport>,
    },
    ClickDispatched {
        tab_id: u64,
        document_generation: u64,
        default_prevented: bool,
    },
    RealmClosed {
        tab_id: u64,
        document_generation: u64,
    },
    RealmStats(PageHostRealmStats),
    ChildStats(PageHostChildStats),
    DebuggerPrograms {
        tab_id: u64,
        document_generation: u64,
        programs: Vec<PageHostDebuggerProgram>,
    },
    /// A bounded inventory of child-minted opaque static BlueTS metadata
    /// associations for one exact private program. This is a handle-discovery
    /// operation only; the handle itself carries no metadata payload.
    DebuggerBlueTsMetadata {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: Vec<PageHostDebuggerMetadataHandle>,
    },
    /// Bounded source-free summary for one exact private metadata handle.
    /// It is valid only while the matching child realm, program, and retained
    /// BlueTS registry attachment remain live.
    DebuggerBlueTsMetadataSummary {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        summary: PageHostDebuggerBlueTsMetadataSummary,
    },
    /// Aggregate ABI/fingerprint evidence for the exact verified direct
    /// lowering map retained under this opaque metadata handle. It carries no
    /// map entries, source spans, AST nodes, or bytecode offsets.
    DebuggerBlueTsMetadataLoweringSummary {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        summary: Box<PageHostDebuggerBlueTsMetadataLoweringSummary>,
    },
    DebuggerBlueTsMetadataSources {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        sources: Vec<PageHostDebuggerBlueTsMetadataSourceId>,
    },
    /// Bounded compiler-minted static type identities. The IDs are local to
    /// the exact metadata attachment and carry no type display or record
    /// payload.
    DebuggerBlueTsMetadataTypes {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        types: Vec<PageHostDebuggerBlueTsMetadataTypeId>,
    },
    /// One bounded child-local compiler-produced type display under the exact
    /// opaque metadata attachment.
    DebuggerBlueTsMetadataType {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        static_type: PageHostDebuggerBlueTsMetadataTypeDisplay,
    },
    /// Bounded compiler-minted symbol identities. The IDs are local to the
    /// exact metadata attachment and carry no symbol-record payload.
    DebuggerBlueTsMetadataSymbols {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        symbols: Vec<PageHostDebuggerBlueTsMetadataSymbolId>,
    },
    /// Bounded compiler-minted contract identities. The IDs are local to the
    /// exact metadata attachment and carry no contract-record payload.
    DebuggerBlueTsMetadataContracts {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        contracts: Vec<PageHostDebuggerBlueTsMetadataContractId>,
    },
    /// One bounded compiler-produced contract display. The enclosing tuple
    /// keeps it bound to an exact child program and metadata attachment.
    DebuggerBlueTsMetadataContract {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        contract: PageHostDebuggerBlueTsMetadataContractDisplay,
    },
    /// A data-only exact-contract validation outcome. It contains no caller
    /// value, plan, failure path, expected shape, or runtime object.
    DebuggerBlueTsMetadataContractValidation {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        validation: PageHostDebuggerBlueTsMetadataContractValidation,
    },
    /// One bounded compiler-produced symbol display. The enclosing tuple keeps
    /// it bound to an exact child program and metadata attachment.
    DebuggerBlueTsMetadataSymbol {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        symbol: PageHostDebuggerBlueTsMetadataSymbolDisplay,
    },
    /// One bounded child-local source-text-free declaration range. The
    /// enclosing tuple retains the exact private program and metadata binding.
    DebuggerBlueTsMetadataSymbolLocation {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        location: PageHostDebuggerBlueTsMetadataSymbolLocation,
    },
    /// One exact child-local contract/source pair and bounded byte range.
    DebuggerBlueTsMetadataContractLocation {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        location: PageHostDebuggerBlueTsMetadataContractLocation,
    },
    /// Exact original BlueTS byte span for the echoed verified child safe
    /// point. This private reply is not exposed to a debugger socket client.
    DebuggerBlueTsSafePointSpan {
        tab_id: u64,
        document_generation: u64,
        metadata: PageHostDebuggerMetadataHandle,
        safe_point: PageHostDebuggerSafePoint,
        span: PageHostDebuggerBlueTsSafePointSpan,
    },
    /// Complete private location under the echoed document/program/metadata
    /// tuple. A missing, stale, or unbound site returns a typed error instead.
    DebuggerBlueTsExceptionLocation {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        location: PageHostDebuggerBlueTsExceptionLocation,
    },
    /// Repeats the exact private request tuple and returns only a verified
    /// child safe point or an explicit unbound result. Core must separately
    /// authorize and remint this before any public debugger disclosure.
    DebuggerBlueTsSourceBreakpoint {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        source_id: u32,
        source_byte: u32,
        safe_point: Option<PageHostDebuggerSafePoint>,
    },
    /// A verified relation that repeats only the two requested opaque IDs.
    DebuggerBlueTsMetadataSymbolType {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        symbol_type: PageHostDebuggerBlueTsMetadataSymbolType,
    },
    /// A verified relation that repeats only the two requested opaque IDs.
    DebuggerBlueTsMetadataSymbolContract {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        symbol_contract: PageHostDebuggerBlueTsMetadataSymbolContract,
    },
    DebuggerBlueTsMetadataSourceProvenance {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        provenance: PageHostDebuggerBlueTsMetadataSourceProvenance,
    },
    DebuggerSafePoints {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        safe_points: Vec<PageHostDebuggerSafePoint>,
    },
    DebuggerSafePointValidated {
        tab_id: u64,
        document_generation: u64,
        safe_point: PageHostDebuggerSafePoint,
    },
    DebuggerBreakpointSet {
        tab_id: u64,
        document_generation: u64,
        safe_point: PageHostDebuggerSafePoint,
    },
    DebuggerBreakpoints {
        tab_id: u64,
        document_generation: u64,
        safe_points: Vec<PageHostDebuggerSafePoint>,
    },
    DebuggerBreakpointCleared {
        tab_id: u64,
        document_generation: u64,
        safe_point: PageHostDebuggerSafePoint,
        was_present: bool,
    },
    DebuggerRootSafePointBreakpointArmed {
        tab_id: u64,
        document_generation: u64,
        safe_point: PageHostDebuggerSafePoint,
    },
    DebuggerNestedSafePointBreakpointArmed {
        tab_id: u64,
        document_generation: u64,
        safe_point: PageHostDebuggerSafePoint,
    },
    DebuggerExecutionState {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        state: PageHostDebuggerExecutionState,
    },
    DebuggerExecutionResumed {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
    },
    DebuggerExecutionStepRequested {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
    },
    DebuggerNestedStepRequested {
        frame: PageHostDebuggerFrame,
    },
    DebuggerNestedResumeRequested {
        frame: PageHostDebuggerFrame,
    },
    DebuggerStackSnapshot {
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        frame: Option<PageHostDebuggerFrame>,
        snapshot: PageHostDebuggerStackSnapshot,
    },
    DebuggerValueSnapshot(Box<PageHostDebuggerValueSnapshot>),
    DebuggerBlueTsSourceStepRequested {
        tab_id: u64,
        document_generation: u64,
        metadata: PageHostDebuggerMetadataHandle,
        source_id: u32,
        safe_point: PageHostDebuggerSafePoint,
    },
    DebuggerExecutionAdvanced {
        tab_id: u64,
        document_generation: u64,
        reports: Vec<PageHostScriptReport>,
    },
    ShutdownAck,
    Error {
        code: PageHostErrorCode,
        /// Fixed host-owned prose only; callers branch on `code`.
        message: String,
    },
}

/// Stable transport/lifecycle failure categories.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PageHostErrorCode {
    ProtocolVersion,
    Authentication,
    InvalidRequest,
    StaleDocument,
    UnknownRealm,
    ResourceLimit,
    HostFailure,
    /// The exact request is structurally valid but not eligible for the
    /// bounded root-classic lifecycle in this document/program state.
    InvalidDebuggerState,
}

/// Builds the only valid first reply. The caller must check this before it
/// dispatches a request to a live realm owner.
pub fn negotiate(request: &PageHostRequest, expected_session_token: &str) -> PageHostReply {
    match request {
        PageHostRequest::Hello {
            protocol_version,
            session_token,
        } if *protocol_version != PAGE_HOST_PROTOCOL_VERSION => PageHostReply::Error {
            code: PageHostErrorCode::ProtocolVersion,
            message: "unsupported BlueJS page-host protocol version".to_string(),
        },
        PageHostRequest::Hello { session_token, .. } if session_token != expected_session_token => {
            PageHostReply::Error {
                code: PageHostErrorCode::Authentication,
                message: "BlueJS page-host session capability was rejected".to_string(),
            }
        }
        PageHostRequest::Hello { .. } => PageHostReply::HelloAck {
            protocol_version: PAGE_HOST_PROTOCOL_VERSION,
        },
        _ => PageHostReply::Error {
            code: PageHostErrorCode::ProtocolVersion,
            message: "BlueJS page-host protocol requires Hello as its first request".to_string(),
        },
    }
}

/// Writes one length-prefixed request frame.
pub fn write_page_host_request<W: Write>(
    writer: &mut W,
    request: &PageHostRequest,
) -> io::Result<()> {
    crate::write_framed(writer, request)
}

/// Reads one length-prefixed request frame.
pub fn read_page_host_request<R: Read>(reader: &mut R) -> io::Result<PageHostRequest> {
    let bytes = crate::read_frame_bytes_with_limit(reader, PAGE_HOST_MAX_FRAME_BYTES)?;
    serde_json::from_slice(&bytes).map_err(io::Error::other)
}

/// Writes one length-prefixed child reply frame.
pub fn write_page_host_reply<W: Write>(writer: &mut W, reply: &PageHostReply) -> io::Result<()> {
    crate::write_framed(writer, reply)
}

/// Reads one length-prefixed child reply frame.
pub fn read_page_host_reply<R: Read>(reader: &mut R) -> io::Result<PageHostReply> {
    let bytes = crate::read_frame_bytes_with_limit(reader, PAGE_HOST_MAX_FRAME_BYTES)?;
    serde_json::from_slice(&bytes).map_err(io::Error::other)
}

/// The deterministic v1 source-content fingerprint shared by loader and
/// child. It detects accidental transport substitution but is not a
/// cryptographic integrity check; the caller's loader must enforce any
/// cryptographic integrity policy before it authorizes a record.
pub fn source_hash(source: &str) -> String {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in source.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("fnv1a64:{hash:016x}")
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::net::UnixStream;

    #[test]
    fn private_value_target_requires_exact_root_or_nested_frame_shape() {
        let program = PageHostDebuggerProgram {
            program_handle: 11,
            program_generation: 13,
        };
        let frame = PageHostDebuggerFrame {
            tab_id: 7,
            document_generation: 3,
            program,
            code_unit_ordinal: 1,
            invocation_serial: 19,
        };
        let mut target = PageHostDebuggerValueTarget {
            tab_id: 7,
            document_generation: 3,
            program,
            frame: None,
            frame_index: 0,
            safe_point: PageHostDebuggerSafePoint {
                program,
                code_unit_ordinal: 0,
                bytecode_offset: 4,
            },
            scope_entry: PageHostDebuggerScopeEntry {
                slot_ordinal: 2,
                scope_depth: 0,
            },
        };
        assert!(target.is_well_formed());
        let snapshot = PageHostDebuggerValueSnapshot {
            target,
            preview: PageHostDebuggerValuePreview::Undefined,
        };
        assert!(snapshot.is_well_formed());
        assert_eq!(
            serde_json::from_slice::<PageHostDebuggerValueSnapshot>(
                &serde_json::to_vec(&snapshot).unwrap()
            )
            .unwrap(),
            snapshot
        );
        target.frame_index = 1;
        assert!(!target.is_well_formed());
        target.frame_index = 0;
        target.frame = Some(frame);
        assert!(!target.is_well_formed());
        target.safe_point.code_unit_ordinal = 1;
        assert!(target.is_well_formed());
        target.frame_index = 1;
        assert!(!target.is_well_formed());
        target.safe_point.code_unit_ordinal = 0;
        assert!(target.is_well_formed());
        target.document_generation += 1;
        assert!(!target.is_well_formed());
        target.document_generation -= 1;
        target.safe_point.program.program_generation += 1;
        assert!(!target.is_well_formed());
        target.safe_point.program.program_generation -= 1;
        target.frame_index = 2;
        assert!(!target.is_well_formed());
        assert_eq!(PAGE_HOST_PROTOCOL_VERSION, 39);
    }

    #[test]
    fn private_value_preview_validates_lossless_bounded_trees() {
        let preview = PageHostDebuggerValuePreview::Record(vec![(
            vec![0xd800],
            PageHostDebuggerValuePreview::Array(vec![
                Some(PageHostDebuggerValuePreview::NumberBits(
                    (-0.0_f64).to_bits(),
                )),
                None,
                Some(PageHostDebuggerValuePreview::StringUnits(vec![0xdc00])),
            ]),
        )]);
        assert!(preview.is_well_formed());
        assert_eq!(
            serde_json::from_slice::<PageHostDebuggerValuePreview>(
                &serde_json::to_vec(&preview).unwrap()
            )
            .unwrap(),
            preview
        );
        let mut too_deep = PageHostDebuggerValuePreview::Null;
        for _ in 0..5 {
            too_deep = PageHostDebuggerValuePreview::Array(vec![Some(too_deep)]);
        }
        assert!(!too_deep.is_well_formed());
        assert!(!PageHostDebuggerValuePreview::Array(vec![None; 33]).is_well_formed());
        assert!(!PageHostDebuggerValuePreview::Array(vec![
            Some(
                PageHostDebuggerValuePreview::Array(vec![None; 32])
            );
            9
        ])
        .is_well_formed());
        assert!(!PageHostDebuggerValuePreview::BigIntBytes(vec![0; 4_097]).is_well_formed());
        assert!(PageHostDebuggerValuePreview::StringUnits(vec![0; 2_048]).is_well_formed());
        assert!(!PageHostDebuggerValuePreview::Record(vec![
            (
                vec![b'a' as u16],
                PageHostDebuggerValuePreview::StringUnits(vec![0; 1_024])
            ),
            (
                vec![b'b' as u16],
                PageHostDebuggerValuePreview::StringUnits(vec![0; 1_024])
            ),
        ])
        .is_well_formed());
        assert!(!PageHostDebuggerValuePreview::Record(vec![(
            vec![b'x' as u16; 2_049],
            PageHostDebuggerValuePreview::Null,
        )])
        .is_well_formed());
        assert!(!PageHostDebuggerValuePreview::Record(vec![
            (vec![b'a' as u16], PageHostDebuggerValuePreview::Null),
            (vec![b'a' as u16], PageHostDebuggerValuePreview::Bool(true)),
        ])
        .is_well_formed());
    }

    #[test]
    fn active_frame_wire_identity_is_exact_source_free_and_nonzero() {
        let program = PageHostDebuggerProgram {
            program_handle: 11,
            program_generation: 13,
        };
        let frame = PageHostDebuggerFrame {
            tab_id: 7,
            document_generation: 3,
            program,
            code_unit_ordinal: 1,
            invocation_serial: 19,
        };
        let point = PageHostDebuggerSafePoint {
            program,
            code_unit_ordinal: 1,
            bytecode_offset: 4,
        };
        assert!(frame.is_well_formed());
        assert!(frame.matches_safe_point(point));
        let wire = serde_json::to_vec(&frame).unwrap();
        assert_eq!(
            serde_json::from_slice::<PageHostDebuggerFrame>(&wire).unwrap(),
            frame
        );
        assert!(!String::from_utf8(wire).unwrap().contains("source"));

        for invalid in [
            PageHostDebuggerFrame { tab_id: 0, ..frame },
            PageHostDebuggerFrame {
                document_generation: 0,
                ..frame
            },
            PageHostDebuggerFrame {
                program: PageHostDebuggerProgram {
                    program_generation: 0,
                    ..program
                },
                ..frame
            },
            PageHostDebuggerFrame {
                code_unit_ordinal: 0,
                ..frame
            },
            PageHostDebuggerFrame {
                invocation_serial: 0,
                ..frame
            },
        ] {
            assert!(!invalid.is_well_formed());
            assert!(!invalid.matches_safe_point(point));
        }
        assert!(!frame.matches_safe_point(PageHostDebuggerSafePoint {
            code_unit_ordinal: 2,
            ..point
        }));
        assert!(!frame.matches_safe_point(PageHostDebuggerSafePoint {
            program: PageHostDebuggerProgram {
                program_handle: 12,
                ..program
            },
            ..point
        }));
        for different in [
            PageHostDebuggerFrame { tab_id: 8, ..frame },
            PageHostDebuggerFrame {
                document_generation: 4,
                ..frame
            },
            PageHostDebuggerFrame {
                program: PageHostDebuggerProgram {
                    program_generation: 14,
                    ..program
                },
                ..frame
            },
            PageHostDebuggerFrame {
                code_unit_ordinal: 2,
                ..frame
            },
            PageHostDebuggerFrame {
                invocation_serial: 20,
                ..frame
            },
        ] {
            assert!(different.is_well_formed());
            assert_ne!(different, frame);
        }
    }

    #[test]
    fn nested_frame_commands_and_states_round_trip_on_private_wire() {
        let program = PageHostDebuggerProgram {
            program_handle: 11,
            program_generation: 13,
        };
        let safe_point = PageHostDebuggerSafePoint {
            program,
            code_unit_ordinal: 1,
            bytecode_offset: 4,
        };
        let frame = PageHostDebuggerFrame {
            tab_id: 7,
            document_generation: 3,
            program,
            code_unit_ordinal: 1,
            invocation_serial: 19,
        };
        let value_target = PageHostDebuggerValueTarget {
            tab_id: 7,
            document_generation: 3,
            program,
            frame: Some(frame),
            frame_index: 0,
            safe_point,
            scope_entry: PageHostDebuggerScopeEntry {
                slot_ordinal: 2,
                scope_depth: 0,
            },
        };
        for request in [
            PageHostRequest::ArmDebuggerNestedSafePointBreakpoint {
                tab_id: 7,
                document_generation: 3,
                safe_point,
            },
            PageHostRequest::StepDebuggerNestedInstruction { frame },
            PageHostRequest::ResumeDebuggerNestedExecution { frame },
            PageHostRequest::GetDebuggerStackSnapshot {
                tab_id: 7,
                document_generation: 3,
                program,
                frame: Some(frame),
                max_frames: 1,
                max_scope_entries: 2,
            },
            PageHostRequest::GetDebuggerValueSnapshot {
                target: value_target,
            },
        ] {
            let (mut writer, mut reader) = UnixStream::pair().unwrap();
            write_page_host_request(&mut writer, &request).unwrap();
            assert_eq!(read_page_host_request(&mut reader).unwrap(), request);
        }
        for reply in [
            PageHostReply::DebuggerNestedSafePointBreakpointArmed {
                tab_id: 7,
                document_generation: 3,
                safe_point,
            },
            PageHostReply::DebuggerExecutionState {
                tab_id: 7,
                document_generation: 3,
                program,
                state: PageHostDebuggerExecutionState::NestedPaused { frame, safe_point },
            },
            PageHostReply::DebuggerNestedStepRequested { frame },
            PageHostReply::DebuggerNestedResumeRequested { frame },
            PageHostReply::DebuggerStackSnapshot {
                tab_id: 7,
                document_generation: 3,
                program,
                frame: Some(frame),
                snapshot: PageHostDebuggerStackSnapshot {
                    frames: vec![PageHostDebuggerStackFrame {
                        code_unit_ordinal: 1,
                        bytecode_offset: 4,
                        scope_entries: vec![PageHostDebuggerScopeEntry {
                            slot_ordinal: 2,
                            scope_depth: 0,
                        }],
                        scope_truncated: true,
                    }],
                    stack_truncated: true,
                },
            },
            PageHostReply::DebuggerValueSnapshot(Box::new(PageHostDebuggerValueSnapshot {
                target: value_target,
                preview: PageHostDebuggerValuePreview::Array(vec![
                    Some(PageHostDebuggerValuePreview::NumberBits(7.0_f64.to_bits())),
                    None,
                ]),
            })),
            PageHostReply::DebuggerExecutionState {
                tab_id: 7,
                document_generation: 3,
                program,
                state: PageHostDebuggerExecutionState::NestedStepping { frame },
            },
            PageHostReply::DebuggerExecutionState {
                tab_id: 7,
                document_generation: 3,
                program,
                state: PageHostDebuggerExecutionState::NestedResuming { frame },
            },
        ] {
            let (mut writer, mut reader) = UnixStream::pair().unwrap();
            write_page_host_reply(&mut writer, &reply).unwrap();
            assert_eq!(read_page_host_reply(&mut reader).unwrap(), reply);
        }
    }

    #[test]
    fn symbol_type_relation_round_trips_on_private_socket() {
        let program = PageHostDebuggerProgram {
            program_handle: 11,
            program_generation: 13,
        };
        let metadata = PageHostDebuggerMetadataHandle {
            metadata_handle: 17,
            metadata_generation: 19,
        };
        let request = PageHostRequest::DescribeDebuggerBlueTsMetadataSymbolType {
            tab_id: 7,
            document_generation: 3,
            program,
            metadata,
            symbol_id: 1,
            type_id: 2,
        };
        let (mut writer, mut reader) = UnixStream::pair().unwrap();
        write_page_host_request(&mut writer, &request).unwrap();
        assert_eq!(read_page_host_request(&mut reader).unwrap(), request);
        let reply = PageHostReply::DebuggerBlueTsMetadataSymbolType {
            tab_id: 7,
            document_generation: 3,
            program,
            metadata,
            symbol_type: PageHostDebuggerBlueTsMetadataSymbolType {
                symbol_id: 1,
                type_id: 2,
            },
        };
        let (mut writer, mut reader) = UnixStream::pair().unwrap();
        write_page_host_reply(&mut writer, &reply).unwrap();
        assert_eq!(read_page_host_reply(&mut reader).unwrap(), reply);

        let click_reply = PageHostReply::ClickDispatched {
            tab_id: 7,
            document_generation: 3,
            default_prevented: true,
        };
        let (mut writer, mut reader) = UnixStream::pair().unwrap();
        write_page_host_reply(&mut writer, &click_reply).unwrap();
        assert_eq!(read_page_host_reply(&mut reader).unwrap(), click_reply);
    }

    #[test]
    fn symbol_contract_relation_round_trips_on_private_socket() {
        let program = PageHostDebuggerProgram {
            program_handle: 11,
            program_generation: 13,
        };
        let metadata = PageHostDebuggerMetadataHandle {
            metadata_handle: 17,
            metadata_generation: 19,
        };
        let request = PageHostRequest::DescribeDebuggerBlueTsMetadataSymbolContract {
            tab_id: 7,
            document_generation: 3,
            program,
            metadata,
            symbol_id: 1,
            contract_id: 2,
        };
        let (mut writer, mut reader) = UnixStream::pair().unwrap();
        write_page_host_request(&mut writer, &request).unwrap();
        assert_eq!(read_page_host_request(&mut reader).unwrap(), request);
        let reply = PageHostReply::DebuggerBlueTsMetadataSymbolContract {
            tab_id: 7,
            document_generation: 3,
            program,
            metadata,
            symbol_contract: PageHostDebuggerBlueTsMetadataSymbolContract {
                symbol_id: 1,
                contract_id: 2,
            },
        };
        let (mut writer, mut reader) = UnixStream::pair().unwrap();
        write_page_host_reply(&mut writer, &reply).unwrap();
        assert_eq!(read_page_host_reply(&mut reader).unwrap(), reply);
    }

    fn source() -> PageHostSource {
        PageHostSource::new("blueice://page/main.js", "globalThis.answer = 42;")
    }

    fn document() -> PageHostDocument {
        PageHostDocument {
            tab_id: 7,
            document_generation: 3,
            snapshot: PageHostDocumentSnapshot {
                document_text: "snapshot text".to_string(),
                document_origin: "https://example.test".to_string(),
            },
            debugger_execution_control: true,
            scripts: vec![PageHostScript {
                ordinal: 0,
                language: PageHostScriptLanguage::JavaScript,
                kind: PageHostScriptKind::Classic,
                graph: PageHostModuleGraph {
                    entry: "blueice://page/main.js".to_string(),
                    modules: vec![source()],
                    resolutions: vec![],
                    resolver_fingerprint: "core-loader-v1".to_string(),
                },
            }],
        }
    }

    #[test]
    fn requests_and_replies_round_trip_over_a_real_socket() {
        let requests = [
            PageHostRequest::Hello {
                protocol_version: PAGE_HOST_PROTOCOL_VERSION,
                session_token: "not-a-real-token".to_string(),
            },
            PageHostRequest::SynchronizeDocument {
                document: document(),
            },
            PageHostRequest::DispatchClick {
                tab_id: 7,
                document_generation: 3,
                node_id: 42,
            },
            PageHostRequest::CloseRealm {
                tab_id: 7,
                document_generation: 3,
            },
            PageHostRequest::GetRealmStats {
                tab_id: 7,
                document_generation: 3,
            },
            PageHostRequest::GetChildStats,
            PageHostRequest::ListDebuggerPrograms {
                tab_id: 7,
                document_generation: 3,
            },
            PageHostRequest::ListDebuggerBlueTsMetadata {
                tab_id: 7,
                document_generation: 3,
                program: PageHostDebuggerProgram {
                    program_handle: 11,
                    program_generation: 13,
                },
            },
            PageHostRequest::DescribeDebuggerBlueTsMetadata {
                tab_id: 7,
                document_generation: 3,
                program: PageHostDebuggerProgram {
                    program_handle: 11,
                    program_generation: 13,
                },
                metadata: PageHostDebuggerMetadataHandle {
                    metadata_handle: 17,
                    metadata_generation: 19,
                },
            },
            PageHostRequest::DescribeDebuggerBlueTsMetadataLoweringSummary {
                tab_id: 7,
                document_generation: 3,
                program: PageHostDebuggerProgram {
                    program_handle: 11,
                    program_generation: 13,
                },
                metadata: PageHostDebuggerMetadataHandle {
                    metadata_handle: 17,
                    metadata_generation: 19,
                },
            },
            PageHostRequest::ListDebuggerBlueTsMetadataSources {
                tab_id: 7,
                document_generation: 3,
                program: PageHostDebuggerProgram {
                    program_handle: 11,
                    program_generation: 13,
                },
                metadata: PageHostDebuggerMetadataHandle {
                    metadata_handle: 17,
                    metadata_generation: 19,
                },
            },
            PageHostRequest::ListDebuggerBlueTsMetadataTypes {
                tab_id: 7,
                document_generation: 3,
                program: PageHostDebuggerProgram {
                    program_handle: 11,
                    program_generation: 13,
                },
                metadata: PageHostDebuggerMetadataHandle {
                    metadata_handle: 17,
                    metadata_generation: 19,
                },
            },
            PageHostRequest::DescribeDebuggerBlueTsMetadataType {
                tab_id: 7,
                document_generation: 3,
                program: PageHostDebuggerProgram {
                    program_handle: 11,
                    program_generation: 13,
                },
                metadata: PageHostDebuggerMetadataHandle {
                    metadata_handle: 17,
                    metadata_generation: 19,
                },
                type_id: 0,
            },
            PageHostRequest::ListDebuggerBlueTsMetadataSymbols {
                tab_id: 7,
                document_generation: 3,
                program: PageHostDebuggerProgram {
                    program_handle: 11,
                    program_generation: 13,
                },
                metadata: PageHostDebuggerMetadataHandle {
                    metadata_handle: 17,
                    metadata_generation: 19,
                },
            },
            PageHostRequest::ListDebuggerBlueTsMetadataContracts {
                tab_id: 7,
                document_generation: 3,
                program: PageHostDebuggerProgram {
                    program_handle: 11,
                    program_generation: 13,
                },
                metadata: PageHostDebuggerMetadataHandle {
                    metadata_handle: 17,
                    metadata_generation: 19,
                },
            },
            PageHostRequest::DescribeDebuggerBlueTsMetadataContract {
                tab_id: 7,
                document_generation: 3,
                program: PageHostDebuggerProgram {
                    program_handle: 11,
                    program_generation: 13,
                },
                metadata: PageHostDebuggerMetadataHandle {
                    metadata_handle: 17,
                    metadata_generation: 19,
                },
                contract_id: 0,
            },
            PageHostRequest::ValidateDebuggerBlueTsMetadataContract {
                tab_id: 7,
                document_generation: 3,
                program: PageHostDebuggerProgram {
                    program_handle: 11,
                    program_generation: 13,
                },
                metadata: PageHostDebuggerMetadataHandle {
                    metadata_handle: 17,
                    metadata_generation: 19,
                },
                contract_id: 0,
                value: CompilerContractValue::Object(
                    [("enabled".to_string(), CompilerContractValue::Boolean(true))]
                        .into_iter()
                        .collect(),
                ),
            },
            PageHostRequest::DescribeDebuggerBlueTsMetadataSymbol {
                tab_id: 7,
                document_generation: 3,
                program: PageHostDebuggerProgram {
                    program_handle: 11,
                    program_generation: 13,
                },
                metadata: PageHostDebuggerMetadataHandle {
                    metadata_handle: 17,
                    metadata_generation: 19,
                },
                symbol_id: 0,
            },
            PageHostRequest::DescribeDebuggerBlueTsMetadataSymbolLocation {
                tab_id: 7,
                document_generation: 3,
                program: PageHostDebuggerProgram {
                    program_handle: 11,
                    program_generation: 13,
                },
                metadata: PageHostDebuggerMetadataHandle {
                    metadata_handle: 17,
                    metadata_generation: 19,
                },
                symbol_id: 0,
            },
            PageHostRequest::DescribeDebuggerBlueTsSafePointSpan {
                tab_id: 7,
                document_generation: 3,
                metadata: PageHostDebuggerMetadataHandle {
                    metadata_handle: 17,
                    metadata_generation: 19,
                },
                safe_point: PageHostDebuggerSafePoint {
                    program: PageHostDebuggerProgram {
                        program_handle: 11,
                        program_generation: 13,
                    },
                    code_unit_ordinal: 0,
                    bytecode_offset: 4,
                },
            },
            PageHostRequest::DescribeDebuggerBlueTsExceptionLocation {
                tab_id: 7,
                document_generation: 3,
                program: PageHostDebuggerProgram {
                    program_handle: 11,
                    program_generation: 13,
                },
                metadata: PageHostDebuggerMetadataHandle {
                    metadata_handle: 17,
                    metadata_generation: 19,
                },
            },
            PageHostRequest::ResolveDebuggerBlueTsSourceBreakpoint {
                tab_id: 7,
                document_generation: 3,
                program: PageHostDebuggerProgram {
                    program_handle: 11,
                    program_generation: 13,
                },
                metadata: PageHostDebuggerMetadataHandle {
                    metadata_handle: 17,
                    metadata_generation: 19,
                },
                source_id: 0,
                source_byte: 4,
            },
            PageHostRequest::ListDebuggerSafePoints {
                tab_id: 7,
                document_generation: 3,
                program: PageHostDebuggerProgram {
                    program_handle: 11,
                    program_generation: 13,
                },
            },
            PageHostRequest::ValidateDebuggerSafePoint {
                tab_id: 7,
                document_generation: 3,
                safe_point: PageHostDebuggerSafePoint {
                    program: PageHostDebuggerProgram {
                        program_handle: 11,
                        program_generation: 13,
                    },
                    code_unit_ordinal: 0,
                    bytecode_offset: 4,
                },
            },
            PageHostRequest::SetDebuggerBreakpoint {
                tab_id: 7,
                document_generation: 3,
                safe_point: PageHostDebuggerSafePoint {
                    program: PageHostDebuggerProgram {
                        program_handle: 11,
                        program_generation: 13,
                    },
                    code_unit_ordinal: 0,
                    bytecode_offset: 4,
                },
            },
            PageHostRequest::ListDebuggerBreakpoints {
                tab_id: 7,
                document_generation: 3,
            },
            PageHostRequest::ClearDebuggerBreakpoint {
                tab_id: 7,
                document_generation: 3,
                safe_point: PageHostDebuggerSafePoint {
                    program: PageHostDebuggerProgram {
                        program_handle: 11,
                        program_generation: 13,
                    },
                    code_unit_ordinal: 0,
                    bytecode_offset: 4,
                },
            },
            PageHostRequest::ArmDebuggerRootSafePointBreakpoint {
                tab_id: 7,
                document_generation: 3,
                safe_point: PageHostDebuggerSafePoint {
                    program: PageHostDebuggerProgram {
                        program_handle: 11,
                        program_generation: 13,
                    },
                    code_unit_ordinal: 0,
                    bytecode_offset: 4,
                },
            },
            PageHostRequest::GetDebuggerExecutionState {
                tab_id: 7,
                document_generation: 3,
                program: PageHostDebuggerProgram {
                    program_handle: 11,
                    program_generation: 13,
                },
            },
            PageHostRequest::ResumeDebuggerExecution {
                tab_id: 7,
                document_generation: 3,
                program: PageHostDebuggerProgram {
                    program_handle: 11,
                    program_generation: 13,
                },
            },
            PageHostRequest::StepDebuggerRootInstruction {
                tab_id: 7,
                document_generation: 3,
                program: PageHostDebuggerProgram {
                    program_handle: 11,
                    program_generation: 13,
                },
            },
            PageHostRequest::StepDebuggerBlueTsSourceSpan {
                tab_id: 7,
                document_generation: 3,
                metadata: PageHostDebuggerMetadataHandle {
                    metadata_handle: 17,
                    metadata_generation: 19,
                },
                source_id: 0,
                safe_point: PageHostDebuggerSafePoint {
                    program: PageHostDebuggerProgram {
                        program_handle: 11,
                        program_generation: 13,
                    },
                    code_unit_ordinal: 0,
                    bytecode_offset: 4,
                },
            },
            PageHostRequest::AdvanceDebuggerExecution {
                tab_id: 7,
                document_generation: 3,
            },
            PageHostRequest::Shutdown,
            PageHostRequest::Unknown,
        ];
        for request in requests {
            let (mut writer, mut reader) = UnixStream::pair().unwrap();
            write_page_host_request(&mut writer, &request).unwrap();
            assert_eq!(read_page_host_request(&mut reader).unwrap(), request);
        }

        let reply = PageHostReply::Synchronized {
            tab_id: 7,
            document_generation: 3,
            already_current: false,
            reports: vec![PageHostScriptReport {
                tab_id: 7,
                document_generation: 3,
                ordinal: 0,
                language: PageHostScriptLanguage::JavaScript,
                kind: PageHostScriptKind::Classic,
                outcome: PageHostScriptOutcome::Executed,
            }],
        };
        let (mut writer, mut reader) = UnixStream::pair().unwrap();
        write_page_host_reply(&mut writer, &reply).unwrap();
        assert_eq!(read_page_host_reply(&mut reader).unwrap(), reply);

        let debugger_reply = PageHostReply::DebuggerBreakpointCleared {
            tab_id: 7,
            document_generation: 3,
            safe_point: PageHostDebuggerSafePoint {
                program: PageHostDebuggerProgram {
                    program_handle: 11,
                    program_generation: 13,
                },
                code_unit_ordinal: 0,
                bytecode_offset: 4,
            },
            was_present: true,
        };
        let (mut writer, mut reader) = UnixStream::pair().unwrap();
        write_page_host_reply(&mut writer, &debugger_reply).unwrap();
        assert_eq!(read_page_host_reply(&mut reader).unwrap(), debugger_reply);

        let debugger_reply = PageHostReply::DebuggerBlueTsSourceStepRequested {
            tab_id: 7,
            document_generation: 3,
            metadata: PageHostDebuggerMetadataHandle {
                metadata_handle: 17,
                metadata_generation: 19,
            },
            source_id: 0,
            safe_point: PageHostDebuggerSafePoint {
                program: PageHostDebuggerProgram {
                    program_handle: 11,
                    program_generation: 13,
                },
                code_unit_ordinal: 0,
                bytecode_offset: 4,
            },
        };
        let (mut writer, mut reader) = UnixStream::pair().unwrap();
        write_page_host_reply(&mut writer, &debugger_reply).unwrap();
        assert_eq!(read_page_host_reply(&mut reader).unwrap(), debugger_reply);

        let debugger_reply = PageHostReply::DebuggerBlueTsSafePointSpan {
            tab_id: 7,
            document_generation: 3,
            metadata: PageHostDebuggerMetadataHandle {
                metadata_handle: 17,
                metadata_generation: 19,
            },
            safe_point: PageHostDebuggerSafePoint {
                program: PageHostDebuggerProgram {
                    program_handle: 11,
                    program_generation: 13,
                },
                code_unit_ordinal: 0,
                bytecode_offset: 4,
            },
            span: PageHostDebuggerBlueTsSafePointSpan {
                source_id: 0,
                start_byte: 0,
                end_byte: 25,
                coordinates: DebuggerSourceCoordinates {
                    start_line: 0,
                    start_column_utf16: 0,
                    end_line: 0,
                    end_column_utf16: 25,
                },
            },
        };
        let (mut writer, mut reader) = UnixStream::pair().unwrap();
        write_page_host_reply(&mut writer, &debugger_reply).unwrap();
        assert_eq!(read_page_host_reply(&mut reader).unwrap(), debugger_reply);

        let debugger_reply = PageHostReply::DebuggerBlueTsExceptionLocation {
            tab_id: 7,
            document_generation: 3,
            program: PageHostDebuggerProgram {
                program_handle: 11,
                program_generation: 13,
            },
            metadata: PageHostDebuggerMetadataHandle {
                metadata_handle: 17,
                metadata_generation: 19,
            },
            location: PageHostDebuggerBlueTsExceptionLocation {
                safe_point: PageHostDebuggerSafePoint {
                    program: PageHostDebuggerProgram {
                        program_handle: 11,
                        program_generation: 13,
                    },
                    code_unit_ordinal: 1,
                    bytecode_offset: 4,
                },
                span: PageHostDebuggerBlueTsSafePointSpan {
                    source_id: 0,
                    start_byte: 2,
                    end_byte: 25,
                    coordinates: DebuggerSourceCoordinates {
                        start_line: 0,
                        start_column_utf16: 2,
                        end_line: 0,
                        end_column_utf16: 25,
                    },
                },
            },
        };
        let (mut writer, mut reader) = UnixStream::pair().unwrap();
        write_page_host_reply(&mut writer, &debugger_reply).unwrap();
        assert_eq!(read_page_host_reply(&mut reader).unwrap(), debugger_reply);

        let debugger_reply = PageHostReply::DebuggerBlueTsSourceBreakpoint {
            tab_id: 7,
            document_generation: 3,
            program: PageHostDebuggerProgram {
                program_handle: 11,
                program_generation: 13,
            },
            metadata: PageHostDebuggerMetadataHandle {
                metadata_handle: 17,
                metadata_generation: 19,
            },
            source_id: 0,
            source_byte: 4,
            safe_point: Some(PageHostDebuggerSafePoint {
                program: PageHostDebuggerProgram {
                    program_handle: 11,
                    program_generation: 13,
                },
                code_unit_ordinal: 0,
                bytecode_offset: 4,
            }),
        };
        let (mut writer, mut reader) = UnixStream::pair().unwrap();
        write_page_host_reply(&mut writer, &debugger_reply).unwrap();
        assert_eq!(read_page_host_reply(&mut reader).unwrap(), debugger_reply);

        let debugger_reply = PageHostReply::DebuggerBlueTsSourceBreakpoint {
            tab_id: 7,
            document_generation: 3,
            program: PageHostDebuggerProgram {
                program_handle: 11,
                program_generation: 13,
            },
            metadata: PageHostDebuggerMetadataHandle {
                metadata_handle: 17,
                metadata_generation: 19,
            },
            source_id: 0,
            source_byte: 30,
            safe_point: None,
        };
        let (mut writer, mut reader) = UnixStream::pair().unwrap();
        write_page_host_reply(&mut writer, &debugger_reply).unwrap();
        assert_eq!(read_page_host_reply(&mut reader).unwrap(), debugger_reply);

        let debugger_reply = PageHostReply::DebuggerBlueTsMetadata {
            tab_id: 7,
            document_generation: 3,
            program: PageHostDebuggerProgram {
                program_handle: 11,
                program_generation: 13,
            },
            metadata: vec![PageHostDebuggerMetadataHandle {
                metadata_handle: 1 << 63,
                metadata_generation: 1 << 63,
            }],
        };
        let (mut writer, mut reader) = UnixStream::pair().unwrap();
        write_page_host_reply(&mut writer, &debugger_reply).unwrap();
        assert_eq!(read_page_host_reply(&mut reader).unwrap(), debugger_reply);

        let debugger_reply = PageHostReply::DebuggerBlueTsMetadataSummary {
            tab_id: 7,
            document_generation: 3,
            program: PageHostDebuggerProgram {
                program_handle: 11,
                program_generation: 13,
            },
            metadata: PageHostDebuggerMetadataHandle {
                metadata_handle: 17,
                metadata_generation: 19,
            },
            summary: PageHostDebuggerBlueTsMetadataSummary {
                language_version: "blue-ts-0.1".to_string(),
                compiler_options_hash: "0123456789abcdef".to_string(),
                source_count: 1,
                type_count: 2,
                symbol_count: 3,
                contract_count: 4,
            },
        };
        let (mut writer, mut reader) = UnixStream::pair().unwrap();
        write_page_host_reply(&mut writer, &debugger_reply).unwrap();
        assert_eq!(read_page_host_reply(&mut reader).unwrap(), debugger_reply);

        let debugger_reply = PageHostReply::DebuggerBlueTsMetadataLoweringSummary {
            tab_id: 7,
            document_generation: 3,
            program: PageHostDebuggerProgram {
                program_handle: 11,
                program_generation: 13,
            },
            metadata: PageHostDebuggerMetadataHandle {
                metadata_handle: 17,
                metadata_generation: 19,
            },
            summary: Box::new(PageHostDebuggerBlueTsMetadataLoweringSummary {
                safe_point_map_abi: "bluejs-safe-point-map-v1".to_string(),
                program_abi: "bluejs-program-v1".to_string(),
                source_set_hash: "bts-source-set-0123456789abcdef".to_string(),
                bound_safe_point_count: 1,
            }),
        };
        let (mut writer, mut reader) = UnixStream::pair().unwrap();
        write_page_host_reply(&mut writer, &debugger_reply).unwrap();
        assert_eq!(read_page_host_reply(&mut reader).unwrap(), debugger_reply);

        let debugger_reply = PageHostReply::DebuggerBlueTsMetadataSources {
            tab_id: 7,
            document_generation: 3,
            program: PageHostDebuggerProgram {
                program_handle: 11,
                program_generation: 13,
            },
            metadata: PageHostDebuggerMetadataHandle {
                metadata_handle: 17,
                metadata_generation: 19,
            },
            sources: vec![PageHostDebuggerBlueTsMetadataSourceId { source_id: 0 }],
        };
        let (mut writer, mut reader) = UnixStream::pair().unwrap();
        write_page_host_reply(&mut writer, &debugger_reply).unwrap();
        assert_eq!(read_page_host_reply(&mut reader).unwrap(), debugger_reply);

        let debugger_reply = PageHostReply::DebuggerBlueTsMetadataTypes {
            tab_id: 7,
            document_generation: 3,
            program: PageHostDebuggerProgram {
                program_handle: 11,
                program_generation: 13,
            },
            metadata: PageHostDebuggerMetadataHandle {
                metadata_handle: 17,
                metadata_generation: 19,
            },
            types: vec![PageHostDebuggerBlueTsMetadataTypeId { type_id: 0 }],
        };
        let (mut writer, mut reader) = UnixStream::pair().unwrap();
        write_page_host_reply(&mut writer, &debugger_reply).unwrap();
        assert_eq!(read_page_host_reply(&mut reader).unwrap(), debugger_reply);

        let debugger_reply = PageHostReply::DebuggerBlueTsMetadataSymbols {
            tab_id: 7,
            document_generation: 3,
            program: PageHostDebuggerProgram {
                program_handle: 11,
                program_generation: 13,
            },
            metadata: PageHostDebuggerMetadataHandle {
                metadata_handle: 17,
                metadata_generation: 19,
            },
            symbols: vec![PageHostDebuggerBlueTsMetadataSymbolId { symbol_id: 0 }],
        };
        let (mut writer, mut reader) = UnixStream::pair().unwrap();
        write_page_host_reply(&mut writer, &debugger_reply).unwrap();
        assert_eq!(read_page_host_reply(&mut reader).unwrap(), debugger_reply);

        let debugger_reply = PageHostReply::DebuggerBlueTsMetadataSymbol {
            tab_id: 7,
            document_generation: 3,
            program: PageHostDebuggerProgram {
                program_handle: 11,
                program_generation: 13,
            },
            metadata: PageHostDebuggerMetadataHandle {
                metadata_handle: 17,
                metadata_generation: 19,
            },
            symbol: PageHostDebuggerBlueTsMetadataSymbolDisplay {
                symbol_id: 0,
                display: "ProjectControlledName".to_string(),
                kind: DebuggerStaticMetadataSymbolKind::Interface,
                exported: true,
            },
        };
        let (mut writer, mut reader) = UnixStream::pair().unwrap();
        write_page_host_reply(&mut writer, &debugger_reply).unwrap();
        assert_eq!(read_page_host_reply(&mut reader).unwrap(), debugger_reply);

        let debugger_reply = PageHostReply::DebuggerBlueTsMetadataSymbolLocation {
            tab_id: 7,
            document_generation: 3,
            program: PageHostDebuggerProgram {
                program_handle: 11,
                program_generation: 13,
            },
            metadata: PageHostDebuggerMetadataHandle {
                metadata_handle: 17,
                metadata_generation: 19,
            },
            location: PageHostDebuggerBlueTsMetadataSymbolLocation {
                symbol_id: 0,
                source_id: 0,
                start_byte: 6,
                end_byte: 31,
                coordinates: DebuggerSourceCoordinates {
                    start_line: 0,
                    start_column_utf16: 6,
                    end_line: 0,
                    end_column_utf16: 31,
                },
            },
        };
        let (mut writer, mut reader) = UnixStream::pair().unwrap();
        write_page_host_reply(&mut writer, &debugger_reply).unwrap();
        assert_eq!(read_page_host_reply(&mut reader).unwrap(), debugger_reply);

        let debugger_reply = PageHostReply::DebuggerBlueTsMetadataContract {
            tab_id: 7,
            document_generation: 3,
            program: PageHostDebuggerProgram {
                program_handle: 11,
                program_generation: 13,
            },
            metadata: PageHostDebuggerMetadataHandle {
                metadata_handle: 17,
                metadata_generation: 19,
            },
            contract: PageHostDebuggerBlueTsMetadataContractDisplay {
                contract_id: 0,
                display: "ProjectControlledContract".to_string(),
                root_kind: DebuggerStaticMetadataContractRootKind::Record,
            },
        };
        let (mut writer, mut reader) = UnixStream::pair().unwrap();
        write_page_host_reply(&mut writer, &debugger_reply).unwrap();
        assert_eq!(read_page_host_reply(&mut reader).unwrap(), debugger_reply);

        let debugger_reply = PageHostReply::DebuggerBlueTsMetadataContractValidation {
            tab_id: 7,
            document_generation: 3,
            program: PageHostDebuggerProgram {
                program_handle: 11,
                program_generation: 13,
            },
            metadata: PageHostDebuggerMetadataHandle {
                metadata_handle: 17,
                metadata_generation: 19,
            },
            validation: PageHostDebuggerBlueTsMetadataContractValidation {
                contract_id: 0,
                valid: true,
            },
        };
        let (mut writer, mut reader) = UnixStream::pair().unwrap();
        write_page_host_reply(&mut writer, &debugger_reply).unwrap();
        assert_eq!(read_page_host_reply(&mut reader).unwrap(), debugger_reply);

        let debugger_reply = PageHostReply::DebuggerBlueTsMetadataContracts {
            tab_id: 7,
            document_generation: 3,
            program: PageHostDebuggerProgram {
                program_handle: 11,
                program_generation: 13,
            },
            metadata: PageHostDebuggerMetadataHandle {
                metadata_handle: 17,
                metadata_generation: 19,
            },
            contracts: vec![PageHostDebuggerBlueTsMetadataContractId { contract_id: 0 }],
        };
        let (mut writer, mut reader) = UnixStream::pair().unwrap();
        write_page_host_reply(&mut writer, &debugger_reply).unwrap();
        assert_eq!(read_page_host_reply(&mut reader).unwrap(), debugger_reply);

        let debugger_reply = PageHostReply::DebuggerExecutionState {
            tab_id: 7,
            document_generation: 3,
            program: PageHostDebuggerProgram {
                program_handle: 11,
                program_generation: 13,
            },
            state: PageHostDebuggerExecutionState::Paused {
                safe_point: PageHostDebuggerSafePoint {
                    program: PageHostDebuggerProgram {
                        program_handle: 11,
                        program_generation: 13,
                    },
                    code_unit_ordinal: 0,
                    bytecode_offset: 4,
                },
            },
        };
        let (mut writer, mut reader) = UnixStream::pair().unwrap();
        write_page_host_reply(&mut writer, &debugger_reply).unwrap();
        assert_eq!(read_page_host_reply(&mut reader).unwrap(), debugger_reply);
        for debugger_reply in [
            PageHostReply::DebuggerExecutionState {
                tab_id: 7,
                document_generation: 3,
                program: PageHostDebuggerProgram {
                    program_handle: 11,
                    program_generation: 13,
                },
                state: PageHostDebuggerExecutionState::Stepping,
            },
            PageHostReply::DebuggerExecutionStepRequested {
                tab_id: 7,
                document_generation: 3,
                program: PageHostDebuggerProgram {
                    program_handle: 11,
                    program_generation: 13,
                },
            },
        ] {
            let (mut writer, mut reader) = UnixStream::pair().unwrap();
            write_page_host_reply(&mut writer, &debugger_reply).unwrap();
            assert_eq!(read_page_host_reply(&mut reader).unwrap(), debugger_reply);
        }
    }

    #[test]
    fn handshake_requires_the_exact_version_and_capability() {
        let token = "launcher-secret";
        assert_eq!(
            negotiate(
                &PageHostRequest::Hello {
                    protocol_version: PAGE_HOST_PROTOCOL_VERSION,
                    session_token: token.to_string(),
                },
                token,
            ),
            PageHostReply::HelloAck {
                protocol_version: PAGE_HOST_PROTOCOL_VERSION,
            }
        );
        assert!(matches!(
            negotiate(
                &PageHostRequest::Hello {
                    protocol_version: PAGE_HOST_PROTOCOL_VERSION - 1,
                    session_token: token.to_string(),
                },
                token,
            ),
            PageHostReply::Error {
                code: PageHostErrorCode::ProtocolVersion,
                ..
            }
        ));
        assert!(matches!(
            negotiate(
                &PageHostRequest::Hello {
                    protocol_version: PAGE_HOST_PROTOCOL_VERSION + 1,
                    session_token: token.to_string(),
                },
                token,
            ),
            PageHostReply::Error {
                code: PageHostErrorCode::ProtocolVersion,
                ..
            }
        ));
        assert!(matches!(
            negotiate(
                &PageHostRequest::Hello {
                    protocol_version: PAGE_HOST_PROTOCOL_VERSION,
                    session_token: "wrong".to_string(),
                },
                token,
            ),
            PageHostReply::Error {
                code: PageHostErrorCode::Authentication,
                ..
            }
        ));
        assert!(matches!(
            negotiate(
                &PageHostRequest::SynchronizeDocument {
                    document: document(),
                },
                token,
            ),
            PageHostReply::Error {
                code: PageHostErrorCode::ProtocolVersion,
                ..
            }
        ));
    }

    #[test]
    fn contract_location_round_trips_without_a_plan_or_source_record() {
        let program = PageHostDebuggerProgram {
            program_handle: 11,
            program_generation: 13,
        };
        let metadata = PageHostDebuggerMetadataHandle {
            metadata_handle: 17,
            metadata_generation: 19,
        };
        let request = PageHostRequest::DescribeDebuggerBlueTsMetadataContractLocation {
            tab_id: 7,
            document_generation: 3,
            program,
            metadata,
            contract_id: 0,
        };
        let (mut writer, mut reader) = UnixStream::pair().unwrap();
        write_page_host_request(&mut writer, &request).unwrap();
        assert_eq!(read_page_host_request(&mut reader).unwrap(), request);
        let reply = PageHostReply::DebuggerBlueTsMetadataContractLocation {
            tab_id: 7,
            document_generation: 3,
            program,
            metadata,
            location: PageHostDebuggerBlueTsMetadataContractLocation {
                contract_id: 0,
                source_id: 0,
                start_byte: 6,
                end_byte: 31,
                coordinates: DebuggerSourceCoordinates {
                    start_line: 0,
                    start_column_utf16: 6,
                    end_line: 0,
                    end_column_utf16: 31,
                },
            },
        };
        assert!(!format!("{reply:?}").contains("PrivateContract"));
        let (mut writer, mut reader) = UnixStream::pair().unwrap();
        write_page_host_reply(&mut writer, &reply).unwrap();
        assert_eq!(read_page_host_reply(&mut reader).unwrap(), reply);
    }

    #[test]
    fn source_constructor_fingerprints_exact_bytes() {
        let source = PageHostSource::new("blueice://page/main.js", "let answer = 42;");
        assert_eq!(source.source_hash, source_hash(&source.source));
        assert_ne!(source.source_hash, source_hash("let answer = 43;"));
    }

    #[test]
    fn realm_stats_reject_conversion_sentinels_and_program_overflow() {
        let stats = PageHostRealmStats {
            tab_id: 7,
            document_generation: 3,
            program_count: 2,
            bytecode_bytes: 64,
            heap_bytes: 128,
        };
        assert!(stats.is_well_formed());
        assert!(!PageHostRealmStats {
            tab_id: 0,
            ..stats.clone()
        }
        .is_well_formed());
        assert!(!PageHostRealmStats {
            program_count: PAGE_HOST_REALM_STATS_MAX_PROGRAMS + 1,
            ..stats.clone()
        }
        .is_well_formed());
        assert!(!PageHostRealmStats {
            bytecode_bytes: u64::MAX,
            ..stats.clone()
        }
        .is_well_formed());
        assert!(!PageHostRealmStats {
            heap_bytes: u64::MAX,
            ..stats
        }
        .is_well_formed());
    }

    #[test]
    fn child_stats_round_trip_and_reject_impossible_totals() {
        let stats = PageHostChildStats {
            realm_count: 2,
            program_count: 3,
            bytecode_bytes: 64,
            heap_bytes: 128,
        };
        assert!(stats.is_well_formed());
        for invalid in [
            PageHostChildStats {
                realm_count: 0,
                ..stats
            },
            PageHostChildStats {
                realm_count: u32::MAX,
                ..stats
            },
            PageHostChildStats {
                program_count: u64::from(PAGE_HOST_REALM_STATS_MAX_PROGRAMS) * 2 + 1,
                ..stats
            },
            PageHostChildStats {
                bytecode_bytes: u64::MAX,
                ..stats
            },
            PageHostChildStats {
                program_count: 0,
                ..stats
            },
            PageHostChildStats {
                heap_bytes: u64::MAX,
                ..stats
            },
        ] {
            assert!(!invalid.is_well_formed());
        }
        let reply = PageHostReply::ChildStats(stats);
        let (mut writer, mut reader) = UnixStream::pair().unwrap();
        write_page_host_reply(&mut writer, &reply).unwrap();
        assert_eq!(read_page_host_reply(&mut reader).unwrap(), reply);
    }

    #[test]
    fn page_host_rejects_an_oversized_frame_before_payload_allocation() {
        let oversized = u32::try_from(PAGE_HOST_MAX_FRAME_BYTES + 1).unwrap();
        let mut bytes = oversized.to_le_bytes().to_vec();
        assert!(read_page_host_request(&mut std::io::Cursor::new(&mut bytes)).is_err());
    }

    #[test]
    fn version_five_document_requires_the_fixed_core_snapshot() {
        let mut value = serde_json::to_value(PageHostRequest::SynchronizeDocument {
            document: document(),
        })
        .unwrap();
        value["SynchronizeDocument"]["document"]
            .as_object_mut()
            .unwrap()
            .remove("snapshot");
        assert!(serde_json::from_value::<PageHostRequest>(value).is_err());
    }
}
