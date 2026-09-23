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
//! and an opt-in root-code-unit pause/resume seam. Version six also reserves a
//! fail-closed session and per-realm capability boundary for future static
//! metadata: `Hello` grants only the canonical intersection of a requested
//! manifest and the core policy, and a metadata operation may be dispatched
//! only after the exact target's capability report also grants its specific
//! metadata capability. A host must report every operation as
//! [`DebuggerCapabilityState::Available`] only after it implements the native
//! behavior; a configured breakpoint is not evidence that pause, stack,
//! scope, value inspection, or static metadata access already exists.

use serde::{Deserialize, Serialize};
use std::io::{self, Read, Write};

/// Independent protocol version for the private core-to-BlueJS debugger
/// channel. It does not share `crate::PROTOCOL_VERSION`, whose lifecycle is
/// the frontend control-plane protocol.
pub const DEBUGGER_PROTOCOL_VERSION: u32 = 6;

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
}

/// One narrowly scoped static-metadata operation a debugger client may ask
/// for during `Hello` and a core policy may grant for that session.
///
/// This deliberately has no broad `StaticMetadata` or `All` variant. Every
/// future metadata surface must add a distinct variant and map it to a
/// distinct [`DebuggerCapability`] before it can be requested. At version six
/// no request or reply exposes even the opaque inventory; this type establishes
/// the policy boundary before that surface is added.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DebuggerMetadataCapability {
    /// Enumerate only bounded [`DebuggerStaticMetadataHandle`] values for one
    /// exact realm. The handles themselves carry no static metadata.
    OpaqueInventory,
    /// A newer metadata capability identifier. It makes the enclosing
    /// manifest invalid instead of silently narrowing the requested set.
    #[serde(other)]
    Unknown,
}

impl DebuggerMetadataCapability {
    const fn debugger_capability(self) -> Option<DebuggerCapability> {
        match self {
            Self::OpaqueInventory => Some(DebuggerCapability::StaticMetadataInventory),
            Self::Unknown => None,
        }
    }

    const fn canonical_index(self) -> Option<u8> {
        match self {
            Self::OpaqueInventory => Some(0),
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

    /// The only non-empty manifest shape currently known to version six.
    /// Calling this does not enable any metadata request: a core still needs a
    /// matching live-realm capability report before dispatch.
    pub fn opaque_inventory() -> Self {
        Self {
            version: DEBUGGER_METADATA_CAPABILITY_MANIFEST_VERSION,
            capabilities: vec![DebuggerMetadataCapability::OpaqueInventory],
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
        true
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DebuggerMetadataSessionAuthorization {
    granted: DebuggerMetadataCapabilityManifest,
}

impl DebuggerMetadataSessionAuthorization {
    /// Whether this session negotiated one exact metadata capability. A
    /// handler must also require the per-realm authorization below; session
    /// negotiation alone does not prove a realm can currently supply data.
    pub fn permits(&self, capability: DebuggerMetadataCapability) -> bool {
        self.granted.contains(capability)
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
