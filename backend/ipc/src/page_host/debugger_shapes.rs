// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

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

/// Exact cross-program invocation retained by the private page host. The
/// dependency child and entry caller are separate installed programs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PageHostDebuggerLinkedFrame {
    pub tab_id: u64,
    pub document_generation: u64,
    pub entry_program: PageHostDebuggerProgram,
    pub dependency_program: PageHostDebuggerProgram,
    pub code_unit_ordinal: u32,
    pub invocation_serial: u64,
}

impl PageHostDebuggerLinkedFrame {
    pub fn is_well_formed(self) -> bool {
        self.tab_id != 0
            && self.document_generation != 0
            && self.entry_program.is_well_formed()
            && self.dependency_program.is_well_formed()
            && self.entry_program != self.dependency_program
            && self.code_unit_ordinal != 0
            && self.invocation_serial != 0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageHostDebuggerLinkedStackFrame {
    pub safe_point: PageHostDebuggerSafePoint,
    pub scope_entries: Vec<PageHostDebuggerScopeEntry>,
    pub scope_truncated: bool,
}

/// Complete child-first dependency/entry stack. The original scope budget is
/// retained so an expected stack can be reacquired without truncation drift.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageHostDebuggerLinkedStackSnapshot {
    pub frames: [PageHostDebuggerLinkedStackFrame; 2],
    pub stack_truncated: bool,
    pub max_scope_entries: u32,
}

impl PageHostDebuggerLinkedStackSnapshot {
    pub fn is_well_formed(&self, frame: PageHostDebuggerLinkedFrame) -> bool {
        frame.is_well_formed()
            && (1..=PAGE_HOST_DEBUGGER_MAX_SCOPE_ENTRIES).contains(&self.max_scope_entries)
            && !self.stack_truncated
            && self.frames[0].safe_point.program == frame.dependency_program
            && self.frames[0].safe_point.code_unit_ordinal == frame.code_unit_ordinal
            && self.frames[1].safe_point.program == frame.entry_program
            && self.frames[1].safe_point.code_unit_ordinal == 0
            && self.frames.iter().all(|frame| {
                frame.scope_entries.len() <= self.max_scope_entries as usize
                    && frame.safe_point.is_well_formed()
            })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageHostDebuggerLinkedSource {
    pub metadata: PageHostDebuggerMetadataHandle,
    pub source_id: u32,
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

/// Exact paused root slot and its separately minted static metadata owner.
/// The ordinary selector can point at a root pause or the parent root of a
/// nested pause. A linked pause must use the complete-stack variant instead.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PageHostDebuggerStaticScopeTarget {
    Ordinary {
        metadata: PageHostDebuggerMetadataHandle,
        target: PageHostDebuggerValueTarget,
    },
    Linked {
        frame: PageHostDebuggerLinkedFrame,
        expected_stack: Box<PageHostDebuggerLinkedStackSnapshot>,
        /// Only the entry root at index one has a retained root-symbol map.
        frame_index: u32,
        metadata: PageHostDebuggerMetadataHandle,
        scope_entry: PageHostDebuggerScopeEntry,
    },
}

impl PageHostDebuggerStaticScopeTarget {
    /// Checks bounded, unambiguous wire shape. The child must separately
    /// reacquire the pause, compare the whole stack and check metadata/slot
    /// ownership before producing a relation.
    pub fn is_well_formed(&self) -> bool {
        match self {
            Self::Ordinary { metadata, target } => {
                metadata.is_well_formed()
                    && target.is_well_formed()
                    && target.safe_point.code_unit_ordinal == 0
                    && (target.frame.is_none() || target.frame_index == 1)
            }
            Self::Linked {
                frame,
                expected_stack,
                frame_index,
                metadata,
                scope_entry,
            } => {
                metadata.is_well_formed()
                    && *frame_index == 1
                    && expected_stack.is_well_formed(*frame)
                    && expected_stack
                        .frames
                        .iter()
                        .all(|frame| !frame.scope_truncated)
                    && expected_stack.frames[1]
                        .scope_entries
                        .iter()
                        .filter(|entry| *entry == scope_entry)
                        .count()
                        == 1
            }
        }
    }
}

/// Static compiler symbol/type IDs for exactly one echoed paused slot.
/// This reply never contains a VM value, type display, source, or name. A
/// missing, moved, or ambiguous join is a typed error, not a partial relation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageHostDebuggerStaticScopeRelation {
    pub target: PageHostDebuggerStaticScopeTarget,
    pub symbol_type: PageHostDebuggerBlueTsMetadataSymbolType,
}

impl PageHostDebuggerStaticScopeRelation {
    pub fn is_well_formed(&self) -> bool {
        self.target.is_well_formed()
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PageHostDebuggerLinkedExecutionState {
    Paused {
        safe_point: PageHostDebuggerSafePoint,
    },
    Resuming,
}

impl PageHostDebuggerSafePoint {
    /// The nested opaque program identity is required for every location.
    pub fn is_well_formed(self) -> bool {
        self.program.is_well_formed()
    }
}
