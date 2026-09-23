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
//! and an opt-in root-code-unit pause/resume seam. Version nine adds the
//! default-deny source-provenance operation for a prior source ID: `Hello`
//! grants only the canonical intersection of a requested manifest and the
//! core policy, and a metadata operation may be dispatched only after the
//! exact target's capability report also grants its specific metadata
//! capability. A host must report every operation as
//! [`DebuggerCapabilityState::Available`] only after it implements the native
//! behavior; a configured breakpoint is not evidence that pause, stack,
//! scope, value inspection, or static metadata access already exists.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::io::{self, Read, Write};
use std::sync::{Arc, Mutex};

/// Independent protocol version for the private core-to-BlueJS debugger
/// channel. It does not share `crate::PROTOCOL_VERSION`, whose lifecycle is
/// the frontend control-plane protocol.
pub const DEBUGGER_PROTOCOL_VERSION: u32 = 9;

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
            Self::Unknown => None,
        }
    }

    const fn canonical_index(self) -> Option<u8> {
        match self {
            Self::OpaqueInventory => Some(0),
            Self::OpaqueSummary => Some(1),
            Self::OpaqueSourceInventory => Some(2),
            Self::OpaqueSourceProvenance => Some(3),
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
    /// Bounded per-stream receipts for source IDs emitted by the public
    /// source-inventory operation. This prevents provenance from accepting a
    /// guessed numeric ID as an independent content-oracle target.
    observed_source_identities: Arc<Mutex<BTreeSet<DebuggerMetadataSourceIdentity>>>,
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

/// One metadata session may remember at most one full source-ID page. This
/// fixed cap avoids turning a long-lived debugger stream into an unbounded
/// receipt cache; a caller can reconnect after it consumes the budget.
pub const DEBUGGER_METADATA_SESSION_MAX_OBSERVED_SOURCE_IDENTITIES: usize = 4_096;

impl DebuggerMetadataSessionAuthorization {
    /// Whether this session negotiated one exact metadata capability. A
    /// handler must also require the per-realm authorization below; session
    /// negotiation alone does not prove a realm can currently supply data.
    pub fn permits(&self, capability: DebuggerMetadataCapability) -> bool {
        self.granted.contains(capability)
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
        observed_source_identities: Arc::new(Mutex::new(BTreeSet::new())),
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
    /// Lists bounded compiler-minted source-record identities for one exact
    /// metadata attachment. It is not a source/provenance read operation.
    ListStaticMetadataSources {
        metadata: DebuggerStaticMetadataHandle,
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
    /// Reply to [`DebuggerRequest::ListStaticMetadataSources`]. IDs are
    /// parent-handle-bound and contain no source/provenance payload.
    StaticMetadataSources(Vec<DebuggerStaticMetadataSourceId>),
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
        | DebuggerRequest::ListStaticMetadataSources { .. }
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
            DebuggerReply::StaticMetadataSources(vec![DebuggerStaticMetadataSourceId {
                metadata: DebuggerStaticMetadataHandle {
                    program,
                    metadata_handle: 24,
                    metadata_generation: 7,
                },
                source_id: 0,
            }]),
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
