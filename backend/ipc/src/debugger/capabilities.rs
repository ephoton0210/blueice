// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

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
    /// Exact pause and single-instruction step for one live synchronous
    /// nested invocation. Root-frame controls do not imply this capability.
    NestedFrames,
    /// A direct dependency/entry module pause with two separate reminted
    /// program identities and an all-or-nothing linked stack.
    LinkedModules,
    Stack,
    Scopes,
    ExceptionPolicy,
    BoundedValues,
    /// Compiler-only symbol/type relation for a receipted paused lexical
    /// slot. It does not imply the independently granted runtime Value read.
    StaticScopeRelation,
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
    /// One half-open byte range for a contract and separately receipted source
    /// ID. It contains no plan, name, source text, or runtime value.
    StaticMetadataContractLocation,
    /// Verifies a symbol's compiler-minted static type using two separately
    /// receipted IDs. It contains no name, display, source, or record payload.
    StaticMetadataSymbolType,
    /// Verifies a reifiable symbol's compiler-minted contract using two
    /// separately receipted IDs. It contains no plan or validation result.
    StaticMetadataSymbolContract,
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
    /// One exact original BlueTS byte span for a verified safe point and a
    /// separately receipted source ID. This neither reads source text nor
    /// grants nearest-offset mapping or source-level execution control.
    StaticMetadataSafePointSpan,
    /// An independently granted, bounded source-byte-position to verified
    /// safe-point binding. It can reveal lowering structure and therefore is
    /// distinct from an exact safe-point span read or execution control.
    StaticMetadataSourceBreakpoint,
    /// An exact, receipt-bound BlueTS source-span step from a paused root.
    StaticMetadataSourceSpanStep,
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
    /// Verifies a symbol-to-contract relation only for IDs returned by
    /// separate same-stream inventories. This disclosure has its own grant.
    OpaqueSymbolContract,
    /// Describes a contract declaration's bounded byte range only after
    /// separate same-stream contract and source receipts and an owner grant.
    OpaqueContractLocation,
    /// Discloses one exact direct-BlueTS safe-point span under the same
    /// stream's opaque metadata and source-ID receipts. It is independent
    /// from source provenance, symbol locations, and execution control.
    OpaqueSafePointSpan,
    /// Resolves one caller-selected bounded original byte position in a
    /// separately receipted source ID. This source-position oracle needs an
    /// independent owner policy and same-stream client grant.
    OpaqueSourceBreakpoint,
    /// Steps one paused root-classic BlueTS span only after exact metadata and
    /// source receipts and an independent owner/client execution grant.
    OpaqueSourceSpanStep,
    /// Relates one receipted paused lexical slot to separately inventoried
    /// compiler symbol/type IDs. This is independent from `OpaqueSymbolType`
    /// and bounded runtime values.
    OpaqueStaticScopeRelation,
    /// A newer metadata capability identifier. It makes the enclosing
    /// manifest invalid instead of silently narrowing the requested set.
    #[serde(other)]
    Unknown,
}

impl DebuggerMetadataCapability {
    pub(super) const fn debugger_capability(self) -> Option<DebuggerCapability> {
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
            Self::OpaqueContractLocation => {
                Some(DebuggerCapability::StaticMetadataContractLocation)
            }
            Self::OpaqueSymbolType => Some(DebuggerCapability::StaticMetadataSymbolType),
            Self::OpaqueSymbolContract => Some(DebuggerCapability::StaticMetadataSymbolContract),
            Self::OpaqueSafePointSpan => Some(DebuggerCapability::StaticMetadataSafePointSpan),
            Self::OpaqueSourceBreakpoint => {
                Some(DebuggerCapability::StaticMetadataSourceBreakpoint)
            }
            Self::OpaqueSourceSpanStep => Some(DebuggerCapability::StaticMetadataSourceSpanStep),
            Self::OpaqueStaticScopeRelation => Some(DebuggerCapability::StaticScopeRelation),
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
            Self::OpaqueSymbolContract => Some(14),
            Self::OpaqueContractLocation => Some(15),
            Self::OpaqueSafePointSpan => Some(16),
            Self::OpaqueSourceBreakpoint => Some(17),
            Self::OpaqueSourceSpanStep => Some(18),
            Self::OpaqueStaticScopeRelation => Some(19),
            Self::Unknown => None,
        }
    }
}

/// Independent schema version for the session-scoped debugger metadata
/// capability manifest. It is intentionally separate from the transport
/// version so future metadata operations cannot be inferred from a transport
/// upgrade alone.
pub const DEBUGGER_METADATA_CAPABILITY_MANIFEST_VERSION: u32 = 5;

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
    pub contract_location: bool,
    pub symbol_type: bool,
    pub symbol_contract: bool,
    pub safe_point_span: bool,
    pub source_breakpoint: bool,
    pub source_span_step: bool,
    pub static_scope_relation: bool,
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

    /// Grants one source-text-free contract location only with its required
    /// parent, source, and contract inventory receipts.
    pub fn opaque_contract_location() -> Self {
        Self {
            version: DEBUGGER_METADATA_CAPABILITY_MANIFEST_VERSION,
            capabilities: vec![
                DebuggerMetadataCapability::OpaqueInventory,
                DebuggerMetadataCapability::OpaqueSourceInventory,
                DebuggerMetadataCapability::OpaqueContractInventory,
                DebuggerMetadataCapability::OpaqueContractLocation,
            ],
        }
    }

    /// Grants only the parent/source inventories and the exact verified
    /// safe-point span operation. Source provenance and other metadata reads
    /// remain independently denied.
    pub fn opaque_safe_point_span() -> Self {
        Self {
            version: DEBUGGER_METADATA_CAPABILITY_MANIFEST_VERSION,
            capabilities: vec![
                DebuggerMetadataCapability::OpaqueInventory,
                DebuggerMetadataCapability::OpaqueSourceInventory,
                DebuggerMetadataCapability::OpaqueSafePointSpan,
            ],
        }
    }

    /// Grants only the parent/source inventories and the independently
    /// authorized original-position binding operation. It grants neither
    /// exact-span reads nor breakpoint installation/execution control.
    pub fn opaque_source_breakpoint() -> Self {
        Self {
            version: DEBUGGER_METADATA_CAPABILITY_MANIFEST_VERSION,
            capabilities: vec![
                DebuggerMetadataCapability::OpaqueInventory,
                DebuggerMetadataCapability::OpaqueSourceInventory,
                DebuggerMetadataCapability::OpaqueSourceBreakpoint,
            ],
        }
    }

    /// Grants the exact source-span step only with inventory and span-read
    /// prerequisites; execution control remains a separate live capability.
    pub fn opaque_source_span_step() -> Self {
        Self {
            version: DEBUGGER_METADATA_CAPABILITY_MANIFEST_VERSION,
            capabilities: vec![
                DebuggerMetadataCapability::OpaqueInventory,
                DebuggerMetadataCapability::OpaqueSourceInventory,
                DebuggerMetadataCapability::OpaqueSafePointSpan,
                DebuggerMetadataCapability::OpaqueSourceSpanStep,
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

    /// Grants one symbol-to-contract relation only with its parent, symbol,
    /// and contract inventory prerequisites.
    pub fn opaque_symbol_contract() -> Self {
        Self {
            version: DEBUGGER_METADATA_CAPABILITY_MANIFEST_VERSION,
            capabilities: vec![
                DebuggerMetadataCapability::OpaqueInventory,
                DebuggerMetadataCapability::OpaqueSymbolInventory,
                DebuggerMetadataCapability::OpaqueContractInventory,
                DebuggerMetadataCapability::OpaqueSymbolContract,
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
            contract_location,
            symbol_type,
            symbol_contract,
            safe_point_span,
            source_breakpoint,
            source_span_step,
            static_scope_relation,
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
            || contract_location
            || symbol_type
            || symbol_contract
            || safe_point_span
            || source_breakpoint
            || source_span_step
            || static_scope_relation;
        let mut capabilities = Vec::new();
        if any {
            capabilities.push(DebuggerMetadataCapability::OpaqueInventory);
        }
        if summary {
            capabilities.push(DebuggerMetadataCapability::OpaqueSummary);
        }
        if source_inventory
            || symbol_location
            || contract_location
            || safe_point_span
            || source_breakpoint
            || source_span_step
        {
            capabilities.push(DebuggerMetadataCapability::OpaqueSourceInventory);
        }
        if source_provenance {
            capabilities.push(DebuggerMetadataCapability::OpaqueSourceProvenance);
        }
        if type_inventory || type_display || symbol_type || static_scope_relation {
            capabilities.push(DebuggerMetadataCapability::OpaqueTypeInventory);
        }
        if type_display {
            capabilities.push(DebuggerMetadataCapability::OpaqueTypeDisplay);
        }
        if symbol_inventory
            || symbol_display
            || symbol_location
            || symbol_type
            || symbol_contract
            || static_scope_relation
        {
            capabilities.push(DebuggerMetadataCapability::OpaqueSymbolInventory);
        }
        if contract_inventory
            || contract_display
            || contract_validation
            || symbol_contract
            || contract_location
        {
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
        if symbol_contract {
            capabilities.push(DebuggerMetadataCapability::OpaqueSymbolContract);
        }
        if contract_location {
            capabilities.push(DebuggerMetadataCapability::OpaqueContractLocation);
        }
        if safe_point_span || source_span_step {
            capabilities.push(DebuggerMetadataCapability::OpaqueSafePointSpan);
        }
        if source_breakpoint {
            capabilities.push(DebuggerMetadataCapability::OpaqueSourceBreakpoint);
        }
        if source_span_step {
            capabilities.push(DebuggerMetadataCapability::OpaqueSourceSpanStep);
        }
        if static_scope_relation {
            capabilities.push(DebuggerMetadataCapability::OpaqueStaticScopeRelation);
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
            && (!self
                .capabilities
                .contains(&DebuggerMetadataCapability::OpaqueSymbolContract)
                || (self
                    .capabilities
                    .contains(&DebuggerMetadataCapability::OpaqueInventory)
                    && self
                        .capabilities
                        .contains(&DebuggerMetadataCapability::OpaqueSymbolInventory)
                    && self
                        .capabilities
                        .contains(&DebuggerMetadataCapability::OpaqueContractInventory)))
            && (!self
                .capabilities
                .contains(&DebuggerMetadataCapability::OpaqueContractLocation)
                || (self
                    .capabilities
                    .contains(&DebuggerMetadataCapability::OpaqueInventory)
                    && self
                        .capabilities
                        .contains(&DebuggerMetadataCapability::OpaqueSourceInventory)
                    && self
                        .capabilities
                        .contains(&DebuggerMetadataCapability::OpaqueContractInventory)))
            && (!self
                .capabilities
                .contains(&DebuggerMetadataCapability::OpaqueSafePointSpan)
                || (self
                    .capabilities
                    .contains(&DebuggerMetadataCapability::OpaqueInventory)
                    && self
                        .capabilities
                        .contains(&DebuggerMetadataCapability::OpaqueSourceInventory)))
            && (!self
                .capabilities
                .contains(&DebuggerMetadataCapability::OpaqueSourceBreakpoint)
                || (self
                    .capabilities
                    .contains(&DebuggerMetadataCapability::OpaqueInventory)
                    && self
                        .capabilities
                        .contains(&DebuggerMetadataCapability::OpaqueSourceInventory)))
            && (!self
                .capabilities
                .contains(&DebuggerMetadataCapability::OpaqueSourceSpanStep)
                || (self
                    .capabilities
                    .contains(&DebuggerMetadataCapability::OpaqueInventory)
                    && self
                        .capabilities
                        .contains(&DebuggerMetadataCapability::OpaqueSourceInventory)
                    && self
                        .capabilities
                        .contains(&DebuggerMetadataCapability::OpaqueSafePointSpan)))
            && (!self
                .capabilities
                .contains(&DebuggerMetadataCapability::OpaqueStaticScopeRelation)
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

    pub(super) fn intersection(&self, requested: &Self) -> Self {
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

    pub(super) fn is_subset_of(&self, requested: &Self) -> bool {
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
