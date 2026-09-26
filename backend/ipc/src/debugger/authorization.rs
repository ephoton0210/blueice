// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

/// A core-local authorization for the metadata and bounded-value capabilities
/// granted by one successfully negotiated debugger `Hello`. It intentionally
/// has no public constructor and is not serializable: it is a dispatcher
/// guard, never a client-supplied wire token.
#[derive(Debug, Clone)]
pub struct DebuggerMetadataSessionAuthorization {
    granted: DebuggerMetadataCapabilityManifest,
    granted_bounded_values: bool,
    /// Exact ordinary and linked Scopes entries emitted on this stream during
    /// one core-owned pause incarnation. `Value` consumes ordinary receipts
    /// only; a future static relation additionally needs its own grant.
    observed_scope_entries: Arc<Mutex<DebuggerScopeReceipts>>,
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

#[derive(Debug, Default)]
struct DebuggerScopeReceipts {
    pause_incarnation: u64,
    targets: HashSet<DebuggerValueTarget>,
    linked_targets: HashSet<DebuggerLinkedScopeTarget>,
}

pub const DEBUGGER_SESSION_MAX_OBSERVED_SCOPE_ENTRIES: usize = 4_096;

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
    /// An independent owner/client grant; scope and metadata grants do not
    /// imply permission to read a runtime value.
    pub fn permits_bounded_values(&self) -> bool {
        self.granted_bounded_values
    }

    /// Atomically receipt every slot actually returned by one Scopes reply.
    /// A changed core pause incarnation drops old receipts first, even when
    /// the new reply is malformed or would exceed the fixed stream budget.
    pub fn observe_scopes(&self, snapshot: &DebuggerScopeSnapshot, incarnation: u64) -> bool {
        let Ok(mut observed) = self.observed_scope_entries.lock() else {
            return false;
        };
        if incarnation == 0 || incarnation < observed.pause_incarnation {
            return false;
        }
        if observed.pause_incarnation != incarnation {
            observed.targets.clear();
            observed.linked_targets.clear();
            observed.pause_incarnation = incarnation;
        }
        let Some(targets) = snapshot.receipt_targets() else {
            return false;
        };
        let new_count = targets.difference(&observed.targets).count();
        if observed
            .targets
            .len()
            .saturating_add(observed.linked_targets.len())
            .saturating_add(new_count)
            > DEBUGGER_SESSION_MAX_OBSERVED_SCOPE_ENTRIES
        {
            return false;
        }
        observed.targets.extend(targets);
        true
    }

    /// Checks one exact same-stream scope receipt from the current pause.
    pub fn observed_scope(&self, target: DebuggerValueTarget, incarnation: u64) -> bool {
        incarnation != 0
            && target.is_well_formed()
            && self.observed_scope_entries.lock().is_ok_and(|observed| {
                observed.pause_incarnation == incarnation && observed.targets.contains(&target)
            })
    }

    /// Receipts the complete entry-root slot list from one exact linked
    /// Scopes-family reply. Truncated or malformed replies create no receipt;
    /// a changed pause incarnation still clears both ordinary and linked
    /// receipts before refusing the reply.
    pub fn observe_linked_scopes(
        &self,
        snapshot: &DebuggerLinkedScopeSnapshot,
        incarnation: u64,
    ) -> bool {
        let Ok(mut observed) = self.observed_scope_entries.lock() else {
            return false;
        };
        if incarnation == 0 || incarnation < observed.pause_incarnation {
            return false;
        }
        if observed.pause_incarnation != incarnation {
            observed.targets.clear();
            observed.linked_targets.clear();
            observed.pause_incarnation = incarnation;
        }
        if !snapshot.is_well_formed() || snapshot.scope_truncated {
            return false;
        }
        let targets = snapshot
            .entries
            .iter()
            .map(|scope_entry| DebuggerLinkedScopeTarget {
                stack: snapshot.stack,
                frame_index: snapshot.frame_index,
                scope_entry: *scope_entry,
            })
            .collect::<HashSet<_>>();
        let new_count = targets.difference(&observed.linked_targets).count();
        if observed
            .targets
            .len()
            .saturating_add(observed.linked_targets.len())
            .saturating_add(new_count)
            > DEBUGGER_SESSION_MAX_OBSERVED_SCOPE_ENTRIES
        {
            return false;
        }
        observed.linked_targets.extend(targets);
        true
    }

    /// Staged receipt check only. A future public handler must separately
    /// prove the independent static-scope grant, metadata/ID receipts, and
    /// current live pause before it may return a relation.
    pub fn observed_static_scope(
        &self,
        target: DebuggerStaticScopeTarget,
        incarnation: u64,
    ) -> bool {
        incarnation != 0
            && target.is_well_formed()
            && self.observed_scope_entries.lock().is_ok_and(|observed| {
                if observed.pause_incarnation != incarnation {
                    return false;
                }
                match target {
                    DebuggerStaticScopeTarget::Ordinary { target, .. } => {
                        observed.targets.contains(&target)
                    }
                    DebuggerStaticScopeTarget::Linked { target, .. } => {
                        observed.linked_targets.contains(&target)
                    }
                }
            })
    }

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
        requested_bounded_values,
    } = request
    else {
        return None;
    };
    let DebuggerReply::HelloAck {
        protocol_version: acknowledged_version,
        granted_metadata_capabilities,
        granted_bounded_values,
    } = reply
    else {
        return None;
    };
    if *protocol_version != DEBUGGER_PROTOCOL_VERSION
        || *acknowledged_version != DEBUGGER_PROTOCOL_VERSION
        || !requested_metadata_capabilities.is_well_formed()
        || !granted_metadata_capabilities.is_subset_of(requested_metadata_capabilities)
        || (*granted_bounded_values && !*requested_bounded_values)
    {
        return None;
    }

    Some(DebuggerMetadataSessionAuthorization {
        granted: granted_metadata_capabilities.clone(),
        granted_bounded_values: *granted_bounded_values,
        observed_scope_entries: Arc::new(Mutex::new(DebuggerScopeReceipts::default())),
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
