// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Core-owned adapter for the bounded registered-project compiler IPC.
//!
//! A core owner calls [`CompilerServiceIpcAdapter::register_core_project`]
//! while it still holds authority to choose canonical roots, a closed module
//! graph, and fixed compiler options. The protocol-facing
//! [`CompilerServiceIpcAdapter::handle`] method accepts only opaque handles;
//! it cannot register, update, reconfigure, source-read, or write a project.
//!
//! A [`CoreCompilerProjectCatalog`] is the explicit startup-only authority
//! boundary: it accepts complete, owner-selected registrations and can then be
//! sealed into a [`CoreCompilerServiceSession`]. The sealed session accepts
//! only the decoded query channel below, so neither a socket worker nor an MCP
//! client can acquire registration, source, path, resolver, option, or output
//! authority after core startup.

use crate::compiler_service::{
    CompilerServiceCheck, CompilerServiceError, CompilerServiceLimits,
    RegisteredProjectCompilerService, RegisteredProjectGeneration, RegisteredProjectId,
    RegisteredProjectRegistration, StaticMetadataInventoryKind, WorkSetInventoryKind,
};
use blueice_bluets::{
    ContractId, ContractValue, DebugSourceLocation, Diagnostic, Severity, SourceId, SymbolKind,
    ValidationError,
};
use blueice_ipc::compiler::{
    CompilerCheck, CompilerContractValidation, CompilerContractValidationFailure,
    CompilerContractValue, CompilerDiagnostic, CompilerDiagnosticCursor, CompilerDiagnosticPage,
    CompilerDiagnosticSeverity, CompilerDiagnostics, CompilerErrorCode, CompilerGeneration,
    CompilerModuleList, CompilerProject, CompilerProjectIdentity, CompilerProjectInventory,
    CompilerReply, CompilerRequest, CompilerSessionAttestation, CompilerSourceCoordinates,
    CompilerStaticContract, CompilerStaticContractLocation, CompilerStaticMetadataCursor,
    CompilerStaticMetadataKind, CompilerStaticMetadataPage, CompilerStaticMetadataSummary,
    CompilerStaticProvenance, CompilerStaticSymbol, CompilerStaticSymbolLocation,
    CompilerStaticType, CompilerSymbolKind, CompilerWorkSetCursor, CompilerWorkSetKind,
    CompilerWorkSetPage, COMPILER_DIAGNOSTIC_MAX_CODE_BYTES, COMPILER_MAX_PROJECT_INVENTORY,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io;
use std::sync::mpsc;

/// A response budget smaller than the protocol's one-mebibyte transport cap.
/// The accounting uses pessimistic JSON-string expansion, so an accepted
/// adapter response has room for structural framing without depending on a
/// serializer side effect to enforce its transport bound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompilerServiceIpcLimits {
    /// Upper bound for the pessimistic encoded response payload accounting.
    pub max_response_bytes: usize,
    /// Maximum UTF-8 byte length for any one emitted identity, diagnostic
    /// message, type display, or symbol name.
    pub max_field_bytes: usize,
    /// Per-set cap for each module work-set in a check result.
    pub max_modules_per_set: usize,
    /// Cap for diagnostics in a check result, independent of the service's
    /// own retention limit.
    pub max_diagnostics: usize,
    /// Maximum diagnostics returned by one separately paginated diagnostic
    /// reply. A request can only lower this core-selected cap.
    pub max_diagnostic_page_entries: usize,
    /// Maximum module identities returned by one work-set page.
    pub max_work_set_page_entries: usize,
    /// Maximum opaque IDs returned in one static-metadata inventory page. A
    /// request may ask for fewer entries but cannot raise this core-selected
    /// cap or use a cursor as an offset.
    pub max_static_metadata_page_entries: usize,
    /// Maximum outstanding cursor receipts retained across all accepted
    /// compiler streams. This independently bounds session bookkeeping even
    /// if a service cursor was consumed before a later response-policy error.
    pub max_stream_cursor_receipts: usize,
}

impl Default for CompilerServiceIpcLimits {
    fn default() -> Self {
        Self {
            max_response_bytes: 256 * 1_024,
            max_field_bytes: 16 * 1_024,
            max_modules_per_set: 1_024,
            max_diagnostics: 256,
            max_diagnostic_page_entries: 128,
            max_work_set_page_entries: 128,
            max_static_metadata_page_entries: 128,
            max_stream_cursor_receipts: 2_048,
        }
    }
}

/// Invalid adapter policy is rejected before a core makes the protocol
/// available. It is not exposed to an IPC client as a potentially misleading
/// ordinary request failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompilerServiceIpcConfigurationError {
    ZeroResponseBytes,
    ResponseExceedsTransportLimit,
    ZeroFieldBytes,
    ZeroDiagnosticPageEntries,
    ZeroWorkSetPageEntries,
    ZeroStaticMetadataPageEntries,
    ZeroStreamCursorReceipts,
    TooManyRegisteredProjects,
}

impl fmt::Display for CompilerServiceIpcConfigurationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroResponseBytes => {
                formatter.write_str("compiler IPC response budget must be nonzero")
            }
            Self::ResponseExceedsTransportLimit => formatter
                .write_str("compiler IPC response budget exceeds the protocol transport limit"),
            Self::ZeroFieldBytes => {
                formatter.write_str("compiler IPC field budget must be nonzero")
            }
            Self::ZeroDiagnosticPageEntries => {
                formatter.write_str("compiler IPC diagnostic page cap must be nonzero")
            }
            Self::ZeroWorkSetPageEntries => {
                formatter.write_str("compiler IPC work-set page cap must be nonzero")
            }
            Self::ZeroStaticMetadataPageEntries => {
                formatter.write_str("compiler IPC static metadata page cap must be nonzero")
            }
            Self::ZeroStreamCursorReceipts => {
                formatter.write_str("compiler IPC stream cursor receipt cap must be nonzero")
            }
            Self::TooManyRegisteredProjects => {
                formatter.write_str("compiler IPC project inventory exceeds its fixed cap")
            }
        }
    }
}

impl std::error::Error for CompilerServiceIpcConfigurationError {}

/// The sole core-side owner of a registered-project compiler service exposed
/// through the v1 IPC adapter.
#[derive(Debug)]
pub struct CompilerServiceIpcAdapter {
    service: RegisteredProjectCompilerService,
    limits: CompilerServiceIpcLimits,
    /// Only owner-exposed project IDs can enter a stream inventory. A
    /// private registration is never query authority, and a project number
    /// supplied by a peer is never registration or visibility authority.
    registered_projects: BTreeSet<u64>,
    /// A successful inventory belongs to one accepted core-attested stream;
    /// another stream cannot substitute a known or guessed numeric ID.
    project_inventory_streams: BTreeMap<String, BTreeSet<u64>>,
    /// Cursor receipts belong to the accepted compiler stream that saw the
    /// preceding page. Numeric cursor IDs alone cannot grant a second stream
    /// continuation authority, even when it knows the project/generation.
    session_cursors: BTreeMap<String, BTreeSet<CompilerSessionCursorReceipt>>,
}

const MAX_PROJECT_INVENTORY_STREAMS: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum CompilerSessionCursorKind {
    Diagnostics,
    WorkSet(CompilerWorkSetKind),
    Sources,
    Types,
    Symbols,
    Contracts,
}

impl CompilerSessionCursorKind {
    fn from_static_kind(kind: CompilerStaticMetadataKind) -> Self {
        match kind {
            CompilerStaticMetadataKind::Sources => Self::Sources,
            CompilerStaticMetadataKind::Types => Self::Types,
            CompilerStaticMetadataKind::Symbols => Self::Symbols,
            CompilerStaticMetadataKind::Contracts => Self::Contracts,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct CompilerSessionCursorReceipt {
    project_id: u64,
    generation: u64,
    kind: CompilerSessionCursorKind,
    id: u64,
}

impl CompilerSessionCursorReceipt {
    fn new(generation: CompilerGeneration, kind: CompilerSessionCursorKind, id: u64) -> Self {
        Self {
            project_id: generation.project.id,
            generation: generation.sequence,
            kind,
            id,
        }
    }
}

impl Default for CompilerServiceIpcAdapter {
    fn default() -> Self {
        Self::new(
            RegisteredProjectCompilerService::default(),
            CompilerServiceIpcLimits::default(),
        )
        .expect("default compiler IPC policy must be valid")
    }
}

impl CompilerServiceIpcAdapter {
    /// Wraps a service selected by the core owner. The adapter never exposes
    /// the service mutably, preventing an IPC caller from changing its
    /// registration inputs after construction.
    pub fn new(
        service: RegisteredProjectCompilerService,
        limits: CompilerServiceIpcLimits,
    ) -> Result<Self, CompilerServiceIpcConfigurationError> {
        validate_limits(limits)?;
        if service.registered_project_ids().count() > COMPILER_MAX_PROJECT_INVENTORY {
            return Err(CompilerServiceIpcConfigurationError::TooManyRegisteredProjects);
        }
        Ok(Self {
            service,
            limits,
            // A pre-populated service is not evidence that its projects were
            // selected for public compiler IPC. Exposure is explicit only.
            registered_projects: BTreeSet::new(),
            project_inventory_streams: BTreeMap::new(),
            session_cursors: BTreeMap::new(),
        })
    }

    /// Creates an empty core-owned service with the supplied service and IPC
    /// retention limits. Projects still have to be registered by a core owner
    /// using [`Self::register_core_project`] before the protocol can inspect
    /// them.
    pub fn with_limits(
        service_limits: CompilerServiceLimits,
        ipc_limits: CompilerServiceIpcLimits,
    ) -> Result<Self, CompilerServiceIpcConfigurationError> {
        Self::new(
            RegisteredProjectCompilerService::new(service_limits),
            ipc_limits,
        )
    }

    /// Core-only registration seam. It is intentionally not represented in
    /// [`CompilerRequest`]: remote callers have no path, source graph,
    /// resolver, plugin, compiler-option, or output-root field to influence.
    pub fn register_core_project(
        &mut self,
        registration: RegisteredProjectRegistration,
    ) -> Result<CompilerProject, CompilerServiceError> {
        self.register_core_project_with_visibility(registration, true)
    }

    /// Keeps a core-owned registration out of every public stream inventory.
    /// The service still owns it for later privileged core-only work; no
    /// compiler IPC or MCP request can promote its visibility after sealing.
    fn register_core_project_private(
        &mut self,
        registration: RegisteredProjectRegistration,
    ) -> Result<CompilerProject, CompilerServiceError> {
        self.register_core_project_with_visibility(registration, false)
    }

    fn register_core_project_with_visibility(
        &mut self,
        registration: RegisteredProjectRegistration,
        exposed: bool,
    ) -> Result<CompilerProject, CompilerServiceError> {
        if self.service.registered_project_ids().count() >= COMPILER_MAX_PROJECT_INVENTORY {
            return Err(CompilerServiceError::ProjectLimit {
                limit: COMPILER_MAX_PROJECT_INVENTORY,
            });
        }
        let project = project_to_wire(self.service.register(registration)?);
        if exposed {
            self.registered_projects.insert(project.id);
        }
        Ok(project)
    }

    /// Handles one request after the transport has successfully negotiated
    /// `Hello`. A transport owner is responsible for first-message handling;
    /// an in-band `Hello` is rejected rather than renegotiating state.
    pub fn handle(&mut self, request: CompilerRequest) -> CompilerReply {
        if let Some(project_id) = compiler_request_project_id(&request) {
            // Direct core-side callers must not bypass owner visibility. Keep
            // malformed and unknown handle categories unchanged by denying
            // only identities the underlying service actually owns.
            if !self.registered_projects.contains(&project_id)
                && self
                    .service
                    .registered_project_ids()
                    .any(|project| project.as_u64() == project_id)
            {
                return CompilerReply::Error {
                    code: CompilerErrorCode::UnobservedProject,
                    message: "compiler project was not exposed by the core owner".to_string(),
                };
            }
        }
        match request {
            CompilerRequest::ListProjects => self.project_inventory(),
            CompilerRequest::DescribeProject { project } => self.describe_project(project),
            CompilerRequest::Check { project } => self.check(project),
            CompilerRequest::ListDiagnostics {
                generation,
                cursor,
                limit,
            } => self.diagnostic_page(generation, cursor, limit),
            CompilerRequest::ListWorkSet {
                generation,
                kind,
                cursor,
                limit,
            } => self.work_set_page(generation, kind, cursor, limit),
            CompilerRequest::GetStaticType {
                generation,
                type_id,
            } => self.static_type(generation, type_id),
            CompilerRequest::GetStaticSymbol {
                generation,
                symbol_id,
            } => self.static_symbol(generation, symbol_id),
            CompilerRequest::GetStaticSymbolLocation {
                generation,
                symbol_id,
                source_id,
            } => self.static_symbol_location(generation, symbol_id, source_id),
            CompilerRequest::ListStaticMetadata {
                generation,
                kind,
                cursor,
                limit,
            } => self.static_metadata_page(generation, kind, cursor, limit),
            CompilerRequest::GetStaticProvenance {
                generation,
                source_id,
            } => self.static_provenance(generation, source_id),
            CompilerRequest::GetStaticContract {
                generation,
                contract_id,
            } => self.static_contract(generation, contract_id),
            CompilerRequest::GetStaticContractLocation {
                generation,
                contract_id,
                source_id,
            } => self.static_contract_location(generation, contract_id, source_id),
            CompilerRequest::ValidateStaticContract {
                generation,
                contract_id,
                value,
            } => self.validate_static_contract(generation, contract_id, value),
            CompilerRequest::Hello { .. } => CompilerReply::Error {
                code: CompilerErrorCode::ProtocolVersion,
                message: "compiler Hello is valid only as the first request".to_string(),
            },
            CompilerRequest::Unknown => CompilerReply::Unsupported {
                operation: "unknown compiler request".to_string(),
                reason: "this core build does not recognize the requested compiler operation"
                    .to_string(),
            },
        }
    }

    /// Applies a decoded request under the exact accepted compiler stream's
    /// core-minted attestation. A cursor is usable only if this stream
    /// previously received it as `next_cursor` for the same generation and
    /// collection. The attestation is internal hand-off data, not an IPC
    /// request field that a remote client can choose.
    fn handle_session_request(
        &mut self,
        session_id: &str,
        request: CompilerRequest,
    ) -> CompilerReply {
        if matches!(request, CompilerRequest::ListProjects) {
            if !self.project_inventory_streams.contains_key(session_id)
                && self.project_inventory_streams.len() >= MAX_PROJECT_INVENTORY_STREAMS
            {
                return CompilerReply::Error {
                    code: CompilerErrorCode::ResourceLimit,
                    message: "compiler project inventory stream limit exceeded".to_string(),
                };
            }
            let reply = self.project_inventory();
            if let CompilerReply::Projects(inventory) = &reply {
                self.project_inventory_streams.insert(
                    session_id.to_string(),
                    inventory
                        .projects
                        .iter()
                        .map(|project| project.id)
                        .collect(),
                );
            }
            return reply;
        }
        if let Some(project_id) = compiler_request_project_id(&request) {
            if !self
                .project_inventory_streams
                .get(session_id)
                .is_some_and(|projects| projects.contains(&project_id))
            {
                return CompilerReply::Error {
                    code: CompilerErrorCode::UnobservedProject,
                    message: "compiler project was not inventoried on this stream".to_string(),
                };
            }
        }
        let pagination = match &request {
            CompilerRequest::ListDiagnostics {
                generation, cursor, ..
            } => Some((
                *generation,
                CompilerSessionCursorKind::Diagnostics,
                cursor.map(|cursor| cursor.id),
            )),
            CompilerRequest::ListWorkSet {
                generation,
                kind,
                cursor,
                ..
            } => Some((
                *generation,
                CompilerSessionCursorKind::WorkSet(*kind),
                cursor.map(|cursor| cursor.id),
            )),
            CompilerRequest::ListStaticMetadata {
                generation,
                kind,
                cursor,
                ..
            } => Some((
                *generation,
                CompilerSessionCursorKind::from_static_kind(*kind),
                cursor.map(|cursor| cursor.id),
            )),
            _ => None,
        };
        let checked_project = match &request {
            CompilerRequest::Check { project } => Some(project.id),
            _ => None,
        };
        let presented_cursor = pagination.and_then(|(generation, kind, id)| {
            id.map(|id| CompilerSessionCursorReceipt::new(generation, kind, id))
        });
        if let Some(cursor) = presented_cursor {
            if !self
                .session_cursors
                .get(session_id)
                .is_some_and(|receipts| receipts.contains(&cursor))
            {
                let (code, message) = match cursor.kind {
                    CompilerSessionCursorKind::Diagnostics => (
                        CompilerErrorCode::InvalidDiagnosticCursor,
                        "compiler diagnostic cursor was not returned on this stream",
                    ),
                    CompilerSessionCursorKind::WorkSet(_) => (
                        CompilerErrorCode::InvalidWorkSetCursor,
                        "compiler work-set cursor was not returned on this stream",
                    ),
                    _ => (
                        CompilerErrorCode::InvalidMetadataCursor,
                        "static metadata cursor was not returned on this stream",
                    ),
                };
                return CompilerReply::Error {
                    code,
                    message: message.to_string(),
                };
            }
        }
        let reply = self.handle(request);
        if let Some(project_id) = checked_project {
            // A check may advance the generation before a response-budget
            // error is reported. Conservatively revoke every old cursor for
            // that project, including cursors held by another stream.
            self.revoke_project_session_cursors(project_id);
        }
        let next_cursor = match (pagination, &reply) {
            (
                Some((generation, CompilerSessionCursorKind::Diagnostics, _)),
                CompilerReply::DiagnosticPage(page),
            ) if page.generation == generation => page.next_cursor.map(|cursor| {
                CompilerSessionCursorReceipt::new(
                    generation,
                    CompilerSessionCursorKind::Diagnostics,
                    cursor.id,
                )
            }),
            (
                Some((generation, CompilerSessionCursorKind::WorkSet(kind), _)),
                CompilerReply::WorkSetPage(page),
            ) if page.generation == generation && page.kind == kind => {
                page.next_cursor.map(|cursor| {
                    CompilerSessionCursorReceipt::new(
                        generation,
                        CompilerSessionCursorKind::WorkSet(kind),
                        cursor.id,
                    )
                })
            }
            (Some((generation, kind, _)), CompilerReply::StaticMetadataPage(page))
                if page.generation == generation
                    && CompilerSessionCursorKind::from_static_kind(page.kind) == kind =>
            {
                page.next_cursor
                    .map(|cursor| CompilerSessionCursorReceipt::new(generation, kind, cursor.id))
            }
            _ => None,
        };
        if matches!(
            &reply,
            CompilerReply::DiagnosticPage(_)
                | CompilerReply::WorkSetPage(_)
                | CompilerReply::StaticMetadataPage(_)
        ) {
            let receipts = self
                .session_cursors
                .entry(session_id.to_string())
                .or_default();
            if let Some(cursor) = presented_cursor {
                receipts.remove(&cursor);
            }
            if let Some(cursor) = next_cursor {
                receipts.insert(cursor);
            }
            if receipts.is_empty() {
                self.session_cursors.remove(session_id);
            }
            if let Some(next_cursor) = next_cursor {
                let total = self
                    .session_cursors
                    .values()
                    .map(BTreeSet::len)
                    .sum::<usize>();
                if total > self.limits.max_stream_cursor_receipts {
                    let receipts = self
                        .session_cursors
                        .get_mut(session_id)
                        .expect("the newly minted cursor was just recorded on this stream");
                    receipts.remove(&next_cursor);
                    if receipts.is_empty() {
                        self.session_cursors.remove(session_id);
                    }
                    self.release_session_cursors(BTreeSet::from([next_cursor]));
                    return CompilerReply::Error {
                        code: CompilerErrorCode::ResourceLimit,
                        message: "compiler stream cursor receipt limit exceeded".to_string(),
                    };
                }
            }
        }
        reply
    }

    /// Releases unconsumed cursor slots when the accepted stream ends. This
    /// prevents a client from exhausting the core's fixed cursor budget by
    /// repeatedly abandoning first pages and reconnecting.
    fn end_session(&mut self, session_id: &str) {
        self.project_inventory_streams.remove(session_id);
        let Some(receipts) = self.session_cursors.remove(session_id) else {
            return;
        };
        self.release_session_cursors(receipts);
    }

    fn project_inventory(&self) -> CompilerReply {
        let projects = self
            .registered_projects
            .iter()
            .map(|id| CompilerProject { id: *id })
            .collect::<Vec<_>>();
        let inventory = CompilerProjectInventory { projects };
        if !inventory.is_well_formed() {
            return response_limit_reply();
        }
        let mut budget = ResponseBudget::new(self.limits.max_response_bytes);
        if !budget.reserve_fixed(128 + 32 * inventory.projects.len()) {
            return response_limit_reply();
        }
        CompilerReply::Projects(inventory)
    }

    fn revoke_project_session_cursors(&mut self, project_id: u64) {
        let mut revoked = BTreeSet::new();
        self.session_cursors.retain(|_, receipts| {
            receipts.retain(|receipt| {
                if receipt.project_id == project_id {
                    revoked.insert(*receipt);
                    false
                } else {
                    true
                }
            });
            !receipts.is_empty()
        });
        self.release_session_cursors(revoked);
    }

    fn release_session_cursors(&mut self, receipts: BTreeSet<CompilerSessionCursorReceipt>) {
        let mut metadata = Vec::new();
        let mut diagnostics = Vec::new();
        let mut work_sets = Vec::new();
        for receipt in receipts {
            match receipt.kind {
                CompilerSessionCursorKind::Diagnostics => diagnostics.push(receipt.id),
                CompilerSessionCursorKind::WorkSet(_) => work_sets.push(receipt.id),
                _ => metadata.push(receipt.id),
            }
        }
        self.service
            .revoke_inventory_cursors(&metadata, &diagnostics, &work_sets);
    }

    fn describe_project(&self, project: CompilerProject) -> CompilerReply {
        let project_id = match project_from_wire(project) {
            Ok(project_id) => project_id,
            Err(error) => return handle_error_reply(error),
        };
        let identity = match self.service.identity(project_id) {
            Ok(identity) => identity,
            Err(error) => return service_error_reply(&error),
        };
        let mut budget = ResponseBudget::new(self.limits.max_response_bytes);
        if !budget.reserve_fixed(256)
            || !budget.reserve_required_string(&identity.entry_module, self.limits.max_field_bytes)
        {
            return response_limit_reply();
        }
        CompilerReply::Project(CompilerProjectIdentity {
            project,
            entry_module: identity.entry_module,
        })
    }

    fn check(&mut self, project: CompilerProject) -> CompilerReply {
        let project_id = match project_from_wire(project) {
            Ok(project_id) => project_id,
            Err(error) => return handle_error_reply(error),
        };
        let check = match self.service.check(project_id) {
            Ok(check) => check,
            Err(error) => return service_error_reply(&error),
        };
        // A successful check is the only public way an MCP receipt learns a
        // generation. Validate every retained diagnostic field now, before
        // any one-shot diagnostic cursor exists, so a later page cannot lose
        // a cursor merely because an unseen later entry violates the fixed
        // public field policy.
        if !retained_diagnostics_fit_wire_policy(
            &check.retained_diagnostics,
            &check.retained_diagnostic_locations,
            self.limits,
        ) {
            return response_limit_reply();
        }
        if [
            &check.parsed_modules,
            &check.reused_parsed_modules,
            &check.rechecked_modules,
            &check.reused_checked_modules,
        ]
        .into_iter()
        .flatten()
        .any(|module| module.len() > self.limits.max_field_bytes)
        {
            return response_limit_reply();
        }
        match self.check_to_wire(check) {
            Ok(check) => CompilerReply::Check(check),
            Err(()) => response_limit_reply(),
        }
    }

    /// Returns one source-free diagnostic page for an exact retained
    /// generation. The adapter bounds a page using the same pessimistic JSON
    /// accounting as ordinary check replies before it asks the service to
    /// consume a one-shot cursor.
    fn diagnostic_page(
        &mut self,
        generation: CompilerGeneration,
        cursor: Option<CompilerDiagnosticCursor>,
        requested_limit: Option<u32>,
    ) -> CompilerReply {
        let generation = match generation_from_wire(generation) {
            Ok(generation) => generation,
            Err(error) => return handle_error_reply(error),
        };
        if cursor.is_some_and(|cursor| !cursor.is_well_formed()) {
            return CompilerReply::Error {
                code: CompilerErrorCode::InvalidDiagnosticCursor,
                message: "invalid compiler diagnostic cursor".to_string(),
            };
        }
        let requested_limit = match requested_limit {
            Some(0) => {
                return CompilerReply::Error {
                    code: CompilerErrorCode::InvalidDiagnosticPage,
                    message: "compiler diagnostic page limit must be positive".to_string(),
                };
            }
            Some(limit) => usize::try_from(limit).unwrap_or(usize::MAX),
            None => self.limits.max_diagnostic_page_entries,
        };
        // A diagnostic always carries two project-controlled strings (module
        // identity and prose). Reserve their worst-case JSON expansion before
        // the service accepts a cursor, so an accepted page cannot overflow
        // this adapter's response envelope merely because a field is dense in
        // escapable bytes.
        const PAGE_FIXED_BYTES: usize = 256;
        const PAGE_ENTRY_FIXED_BYTES: usize = 352;
        let max_entry_bytes = self
            .limits
            .max_field_bytes
            .checked_mul(12)
            .and_then(|bytes| {
                COMPILER_DIAGNOSTIC_MAX_CODE_BYTES
                    .checked_mul(6)
                    .and_then(|code_bytes| bytes.checked_add(code_bytes))
            })
            .and_then(|bytes| bytes.checked_add(PAGE_ENTRY_FIXED_BYTES));
        let Some(max_entry_bytes) = max_entry_bytes else {
            return response_limit_reply();
        };
        let response_cap = self
            .limits
            .max_response_bytes
            .saturating_sub(PAGE_FIXED_BYTES)
            / max_entry_bytes;
        let limit = requested_limit
            .min(self.limits.max_diagnostic_page_entries)
            .min(response_cap);
        if limit == 0 {
            return response_limit_reply();
        }
        let page = match self.service.diagnostic_inventory(
            generation,
            cursor.map(|cursor| cursor.id),
            limit,
        ) {
            Ok(page) => page,
            Err(error) => return service_error_reply(&error),
        };
        let mut budget = ResponseBudget::new(self.limits.max_response_bytes);
        if !budget.reserve_fixed(PAGE_FIXED_BYTES) {
            if let Some(id) = page.next_cursor {
                self.service.revoke_inventory_cursors(&[], &[id], &[]);
            }
            return response_limit_reply();
        }
        let entries = match diagnostic_page_entries_to_wire(
            &page.entries,
            &page.locations,
            self.limits,
            &mut budget,
        ) {
            Ok(entries) => entries,
            Err(()) => {
                if let Some(id) = page.next_cursor {
                    self.service.revoke_inventory_cursors(&[], &[id], &[]);
                }
                return response_limit_reply();
            }
        };
        CompilerReply::DiagnosticPage(CompilerDiagnosticPage {
            generation: generation_to_wire(generation),
            entries,
            next_cursor: page.next_cursor.map(|id| CompilerDiagnosticCursor { id }),
            truncated: page.truncated,
        })
    }

    fn work_set_page(
        &mut self,
        generation: CompilerGeneration,
        kind: CompilerWorkSetKind,
        cursor: Option<CompilerWorkSetCursor>,
        requested_limit: Option<u32>,
    ) -> CompilerReply {
        let generation = match generation_from_wire(generation) {
            Ok(generation) => generation,
            Err(error) => return handle_error_reply(error),
        };
        if cursor.is_some_and(|cursor| !cursor.is_well_formed()) {
            return CompilerReply::Error {
                code: CompilerErrorCode::InvalidWorkSetCursor,
                message: "invalid compiler work-set cursor".to_string(),
            };
        }
        let requested_limit = match requested_limit {
            Some(0) => {
                return CompilerReply::Error {
                    code: CompilerErrorCode::InvalidWorkSetPage,
                    message: "compiler work-set page limit must be positive".to_string(),
                };
            }
            Some(limit) => usize::try_from(limit).unwrap_or(usize::MAX),
            None => self.limits.max_work_set_page_entries,
        };
        const PAGE_FIXED_BYTES: usize = 256;
        const ENTRY_FIXED_BYTES: usize = 64;
        let Some(max_entry_bytes) = self
            .limits
            .max_field_bytes
            .checked_mul(6)
            .and_then(|bytes| bytes.checked_add(ENTRY_FIXED_BYTES + 2))
        else {
            return response_limit_reply();
        };
        let response_cap = self
            .limits
            .max_response_bytes
            .saturating_sub(PAGE_FIXED_BYTES)
            / max_entry_bytes;
        let limit = requested_limit
            .min(self.limits.max_work_set_page_entries)
            .min(response_cap);
        if limit == 0 {
            return response_limit_reply();
        }
        let page = match self.service.work_set_inventory(
            generation,
            work_set_kind_from_wire(kind),
            cursor.map(|cursor| cursor.id),
            limit,
        ) {
            Ok(page) => page,
            Err(error) => return service_error_reply(&error),
        };
        let mut budget = ResponseBudget::new(self.limits.max_response_bytes);
        let valid = budget.reserve_fixed(PAGE_FIXED_BYTES)
            && page.entries.iter().all(|entry| {
                budget.reserve_required_string(entry, self.limits.max_field_bytes)
                    && budget.reserve_fixed(ENTRY_FIXED_BYTES)
            });
        if !valid {
            if let Some(id) = page.next_cursor {
                self.service.revoke_inventory_cursors(&[], &[], &[id]);
            }
            return response_limit_reply();
        }
        CompilerReply::WorkSetPage(CompilerWorkSetPage {
            generation: generation_to_wire(generation),
            kind,
            entries: page.entries,
            next_cursor: page.next_cursor.map(|id| CompilerWorkSetCursor { id }),
            truncated: page.truncated,
        })
    }

    fn static_type(&self, generation: CompilerGeneration, type_id: u32) -> CompilerReply {
        let generation = match generation_from_wire(generation) {
            Ok(generation) => generation,
            Err(error) => return handle_error_reply(error),
        };
        let static_type = match self
            .service
            .static_type(generation, blueice_bluets::TypeId(type_id))
        {
            Ok(static_type) => static_type,
            Err(error) => return service_error_reply(&error),
        };
        let mut budget = ResponseBudget::new(self.limits.max_response_bytes);
        if !budget.reserve_fixed(256)
            || !budget.reserve_required_string(&static_type.display, self.limits.max_field_bytes)
        {
            return response_limit_reply();
        }
        CompilerReply::StaticType(CompilerStaticType {
            generation: generation_to_wire(generation),
            id: static_type.id.0,
            display: static_type.display,
        })
    }

    fn static_symbol(&self, generation: CompilerGeneration, symbol_id: u32) -> CompilerReply {
        let generation = match generation_from_wire(generation) {
            Ok(generation) => generation,
            Err(error) => return handle_error_reply(error),
        };
        let symbol = match self
            .service
            .static_symbol(generation, blueice_bluets::SymbolId(symbol_id))
        {
            Ok(symbol) => symbol,
            Err(error) => return service_error_reply(&error),
        };
        let mut budget = ResponseBudget::new(self.limits.max_response_bytes);
        if !budget.reserve_fixed(384)
            || !budget.reserve_required_string(&symbol.name, self.limits.max_field_bytes)
            || !budget.reserve_required_string(&symbol.span.module, self.limits.max_field_bytes)
        {
            return response_limit_reply();
        }
        let (start, end) = match (
            u64::try_from(symbol.span.start),
            u64::try_from(symbol.span.end),
        ) {
            (Ok(start), Ok(end)) => (start, end),
            _ => return response_limit_reply(),
        };
        CompilerReply::StaticSymbol(CompilerStaticSymbol {
            generation: generation_to_wire(generation),
            id: symbol.id.0,
            name: symbol.name,
            kind: symbol_kind_to_wire(symbol.kind),
            exported: symbol.exported,
            module: symbol.span.module,
            start,
            end,
            static_type_id: symbol.static_type.map(|id| id.0),
            source_id: symbol.source.0,
            contract_id: symbol.contract.map(|id| id.0),
        })
    }

    fn static_symbol_location(
        &self,
        generation: CompilerGeneration,
        symbol_id: u32,
        source_id: u32,
    ) -> CompilerReply {
        let generation = match generation_from_wire(generation) {
            Ok(generation) => generation,
            Err(error) => return handle_error_reply(error),
        };
        let symbol = match self
            .service
            .static_symbol(generation, blueice_bluets::SymbolId(symbol_id))
        {
            Ok(symbol) => symbol,
            Err(error) => return service_error_reply(&error),
        };
        if symbol.source.0 != source_id {
            return invalid_location_target_reply();
        }
        let Some((start_byte, end_byte, coordinates)) =
            compiler_declaration_location(&symbol.span, symbol.location)
        else {
            return response_limit_reply();
        };
        let reply = CompilerStaticSymbolLocation {
            generation: generation_to_wire(generation),
            symbol_id,
            source_id,
            start_byte,
            end_byte,
            coordinates,
        };
        if !reply.is_well_formed()
            || !ResponseBudget::new(self.limits.max_response_bytes).reserve_fixed(256)
        {
            return response_limit_reply();
        }
        CompilerReply::StaticSymbolLocation(reply)
    }

    /// Returns one source-free page of opaque static IDs. The cursor is
    /// validated by both this adapter and the core service, which binds it to
    /// the exact retained generation and consumes it after one use. The page
    /// cap has both a configured policy limit and a response-budget limit.
    fn static_metadata_page(
        &mut self,
        generation: CompilerGeneration,
        kind: CompilerStaticMetadataKind,
        cursor: Option<CompilerStaticMetadataCursor>,
        requested_limit: Option<u32>,
    ) -> CompilerReply {
        let generation = match generation_from_wire(generation) {
            Ok(generation) => generation,
            Err(error) => return handle_error_reply(error),
        };
        if cursor.is_some_and(|cursor| !cursor.is_well_formed()) {
            return CompilerReply::Error {
                code: CompilerErrorCode::InvalidMetadataCursor,
                message: "invalid static metadata cursor".to_string(),
            };
        }
        let requested_limit = match requested_limit {
            Some(0) => {
                return CompilerReply::Error {
                    code: CompilerErrorCode::InvalidMetadataPage,
                    message: "static metadata page limit must be positive".to_string(),
                };
            }
            Some(limit) => usize::try_from(limit).unwrap_or(usize::MAX),
            None => self.limits.max_static_metadata_page_entries,
        };
        // Each ID, list delimiter and conservative JSON framing are charged
        // before the service consumes a one-shot cursor. This means a tiny
        // adapter response policy returns a limit failure without losing the
        // cursor or accidentally creating a partial page.
        const PAGE_FIXED_BYTES: usize = 256;
        const PAGE_ID_BYTES: usize = 32;
        let response_cap = self
            .limits
            .max_response_bytes
            .saturating_sub(PAGE_FIXED_BYTES)
            / PAGE_ID_BYTES;
        let limit = requested_limit
            .min(self.limits.max_static_metadata_page_entries)
            .min(response_cap);
        if limit == 0 {
            return response_limit_reply();
        }
        let page = match self.service.static_metadata_inventory(
            generation,
            static_metadata_kind_from_wire(kind),
            cursor.map(|cursor| cursor.id),
            limit,
        ) {
            Ok(page) => page,
            Err(error) => return service_error_reply(&error),
        };
        let mut budget = ResponseBudget::new(self.limits.max_response_bytes);
        if !budget.reserve_fixed(PAGE_FIXED_BYTES)
            || page
                .ids
                .iter()
                .any(|_| !budget.reserve_optional_fixed(PAGE_ID_BYTES))
        {
            if let Some(id) = page.next_cursor {
                self.service.revoke_inventory_cursors(&[id], &[], &[]);
            }
            return response_limit_reply();
        }
        CompilerReply::StaticMetadataPage(CompilerStaticMetadataPage {
            generation: generation_to_wire(generation),
            kind,
            ids: page.ids,
            next_cursor: page
                .next_cursor
                .map(|id| CompilerStaticMetadataCursor { id }),
        })
    }

    fn static_provenance(&self, generation: CompilerGeneration, source_id: u32) -> CompilerReply {
        let generation = match generation_from_wire(generation) {
            Ok(generation) => generation,
            Err(error) => return handle_error_reply(error),
        };
        let provenance = match self
            .service
            .static_provenance(generation, SourceId(source_id))
        {
            Ok(provenance) => provenance,
            Err(error) => return service_error_reply(&error),
        };
        let mut budget = ResponseBudget::new(self.limits.max_response_bytes);
        if !budget.reserve_fixed(384)
            || !budget.reserve_required_string(&provenance.module, self.limits.max_field_bytes)
            || !budget
                .reserve_required_string(&provenance.content_hash, self.limits.max_field_bytes)
        {
            return response_limit_reply();
        }
        CompilerReply::StaticProvenance(CompilerStaticProvenance {
            generation: generation_to_wire(generation),
            source_id: provenance.id.0,
            module: provenance.module,
            content_hash: provenance.content_hash,
        })
    }

    fn static_contract(&self, generation: CompilerGeneration, contract_id: u32) -> CompilerReply {
        let generation = match generation_from_wire(generation) {
            Ok(generation) => generation,
            Err(error) => return handle_error_reply(error),
        };
        let contract = match self
            .service
            .static_contract(generation, ContractId(contract_id))
        {
            Ok(contract) => contract,
            Err(error) => return service_error_reply(&error),
        };
        let root = format!("{:?}", contract.plan.root);
        let definitions = format!("{:?}", contract.plan.definitions);
        let definition_count = match u32::try_from(contract.plan.definitions.len()) {
            Ok(count) => count,
            Err(_) => return response_limit_reply(),
        };
        let mut budget = ResponseBudget::new(self.limits.max_response_bytes);
        if !budget.reserve_fixed(512)
            || !budget.reserve_required_string(&contract.name, self.limits.max_field_bytes)
            || !budget
                .reserve_required_string(&contract.plan.fingerprint, self.limits.max_field_bytes)
            || !budget.reserve_required_string(&root, self.limits.max_field_bytes)
            || !budget.reserve_required_string(&definitions, self.limits.max_field_bytes)
        {
            return response_limit_reply();
        }
        CompilerReply::StaticContract(CompilerStaticContract {
            generation: generation_to_wire(generation),
            contract_id: contract.id.0,
            source_id: contract.source.0,
            name: contract.name,
            fingerprint: contract.plan.fingerprint,
            root,
            definitions,
            definition_count,
        })
    }

    fn static_contract_location(
        &self,
        generation: CompilerGeneration,
        contract_id: u32,
        source_id: u32,
    ) -> CompilerReply {
        let generation = match generation_from_wire(generation) {
            Ok(generation) => generation,
            Err(error) => return handle_error_reply(error),
        };
        let contract = match self
            .service
            .static_contract(generation, ContractId(contract_id))
        {
            Ok(contract) => contract,
            Err(error) => return service_error_reply(&error),
        };
        if contract.source.0 != source_id {
            return invalid_location_target_reply();
        }
        let Some((start_byte, end_byte, coordinates)) =
            compiler_declaration_location(&contract.span, contract.location)
        else {
            return response_limit_reply();
        };
        let reply = CompilerStaticContractLocation {
            generation: generation_to_wire(generation),
            contract_id,
            source_id,
            start_byte,
            end_byte,
            coordinates,
        };
        if !reply.is_well_formed()
            || !ResponseBudget::new(self.limits.max_response_bytes).reserve_fixed(256)
        {
            return response_limit_reply();
        }
        CompilerReply::StaticContractLocation(reply)
    }

    fn validate_static_contract(
        &self,
        generation: CompilerGeneration,
        contract_id: u32,
        value: CompilerContractValue,
    ) -> CompilerReply {
        let generation = match generation_from_wire(generation) {
            Ok(generation) => generation,
            Err(error) => return handle_error_reply(error),
        };
        let value = match wire_contract_value(value, self.service.contract_validation_limits()) {
            Ok(value) => value,
            Err(()) => {
                return CompilerReply::Error {
                    code: CompilerErrorCode::InvalidContractValue,
                    message: "invalid or over-budget data-only contract value".to_string(),
                };
            }
        };
        let outcome =
            match self
                .service
                .validate_static_contract(generation, ContractId(contract_id), &value)
            {
                Ok(outcome) => outcome,
                Err(error) => return service_error_reply(&error),
            };
        let failure = outcome.err().map(validation_failure_to_wire);
        let mut budget = ResponseBudget::new(self.limits.max_response_bytes);
        if !budget.reserve_fixed(512)
            || failure.as_ref().is_some_and(|failure| {
                !budget.reserve_required_string(&failure.path, self.limits.max_field_bytes)
                    || !budget
                        .reserve_required_string(&failure.expected, self.limits.max_field_bytes)
                    || !budget
                        .reserve_required_string(&failure.observed, self.limits.max_field_bytes)
            })
        {
            return response_limit_reply();
        }
        CompilerReply::ContractValidation(CompilerContractValidation {
            generation: generation_to_wire(generation),
            contract_id,
            valid: failure.is_none(),
            failure,
        })
    }

    fn check_to_wire(&self, check: CompilerServiceCheck) -> Result<CompilerCheck, ()> {
        let mut budget = ResponseBudget::new(self.limits.max_response_bytes);
        if !budget.reserve_fixed(2_048) {
            return Err(());
        }
        let parsed_modules = module_list(&check.parsed_modules, self.limits, &mut budget)?;
        let reused_parsed_modules =
            module_list(&check.reused_parsed_modules, self.limits, &mut budget)?;
        let rechecked_modules = module_list(&check.rechecked_modules, self.limits, &mut budget)?;
        let reused_checked_modules =
            module_list(&check.reused_checked_modules, self.limits, &mut budget)?;
        let diagnostics = diagnostics_to_wire(
            &check.diagnostics.entries,
            check
                .retained_diagnostic_locations
                .get(..check.diagnostics.entries.len())
                .ok_or(())?,
            check.diagnostics.truncated,
            self.limits,
            &mut budget,
        )?;
        let artifact_fingerprint = match check.artifact_fingerprint {
            Some(fingerprint) => {
                if !budget.reserve_required_string(&fingerprint, self.limits.max_field_bytes) {
                    return Err(());
                }
                Some(fingerprint)
            }
            None => None,
        };
        let static_metadata = match check.static_debug_info {
            Some(info) => {
                if !budget.reserve_fixed(256)
                    || !budget.reserve_required_string(
                        &info.language_version,
                        self.limits.max_field_bytes,
                    )
                    || !budget.reserve_required_string(
                        &info.compiler_options_hash,
                        self.limits.max_field_bytes,
                    )
                {
                    return Err(());
                }
                Some(CompilerStaticMetadataSummary {
                    language_version: info.language_version,
                    compiler_options_hash: info.compiler_options_hash,
                    source_count: u32::try_from(info.sources.len()).map_err(|_| ())?,
                    type_count: u32::try_from(info.types.len()).map_err(|_| ())?,
                    symbol_count: u32::try_from(info.symbols.len()).map_err(|_| ())?,
                    contract_count: u32::try_from(info.contracts.len()).map_err(|_| ())?,
                })
            }
            None => None,
        };
        Ok(CompilerCheck {
            generation: generation_to_wire(check.generation),
            cache_hit: check.cache_hit,
            parsed_modules,
            reused_parsed_modules,
            rechecked_modules,
            reused_checked_modules,
            diagnostics,
            has_errors: check.has_errors,
            artifact_fingerprint,
            static_metadata,
        })
    }
}

fn compiler_request_project_id(request: &CompilerRequest) -> Option<u64> {
    match request {
        CompilerRequest::DescribeProject { project } | CompilerRequest::Check { project } => {
            Some(project.id)
        }
        CompilerRequest::ListDiagnostics { generation, .. }
        | CompilerRequest::ListWorkSet { generation, .. }
        | CompilerRequest::GetStaticType { generation, .. }
        | CompilerRequest::GetStaticSymbol { generation, .. }
        | CompilerRequest::GetStaticSymbolLocation { generation, .. }
        | CompilerRequest::ListStaticMetadata { generation, .. }
        | CompilerRequest::GetStaticProvenance { generation, .. }
        | CompilerRequest::GetStaticContract { generation, .. }
        | CompilerRequest::GetStaticContractLocation { generation, .. }
        | CompilerRequest::ValidateStaticContract { generation, .. } => Some(generation.project.id),
        CompilerRequest::Hello { .. }
        | CompilerRequest::ListProjects
        | CompilerRequest::Unknown => None,
    }
}

/// Startup-only catalog for the closed compiler projects selected by a core
/// owner. It is intentionally separate from the long-lived query session: a
/// caller must consume this catalog with [`Self::seal`] before handing a
/// service to a listener/session loop, which makes later registration
/// structurally impossible through that runtime object.
#[derive(Debug)]
pub struct CoreCompilerProjectCatalog {
    adapter: CompilerServiceIpcAdapter,
    registered_projects: Vec<CompilerProject>,
}

impl Default for CoreCompilerProjectCatalog {
    fn default() -> Self {
        Self::new(
            RegisteredProjectCompilerService::default(),
            CompilerServiceIpcLimits::default(),
        )
        .expect("default core compiler catalog policy must be valid")
    }
}

impl CoreCompilerProjectCatalog {
    /// Creates a catalog while the trusted core startup owner still chooses
    /// its fixed project inputs. Any projects already in the supplied service
    /// are counted but remain private by default. This API is never called by
    /// a wire request or an MCP tool.
    pub fn new(
        service: RegisteredProjectCompilerService,
        limits: CompilerServiceIpcLimits,
    ) -> Result<Self, CompilerServiceIpcConfigurationError> {
        let registered_projects = service
            .registered_project_ids()
            .map(project_to_wire)
            .collect();
        Ok(Self {
            adapter: CompilerServiceIpcAdapter::new(service, limits)?,
            registered_projects,
        })
    }

    /// Registers one complete, already-authorized project during trusted core
    /// startup. A registration cannot be changed after this call, and the
    /// returned opaque ID does not reveal the roots or source graph.
    pub fn register_startup_project(
        &mut self,
        registration: RegisteredProjectRegistration,
    ) -> Result<CompilerProject, CompilerServiceError> {
        let project = self.adapter.register_core_project(registration)?;
        self.registered_projects.push(project);
        Ok(project)
    }

    /// Registers a core-owned project without granting any compiler stream a
    /// project receipt. Even after `ListProjects`, a guessed private ID is
    /// rejected before reaching the compiler cache. Visibility cannot be
    /// changed after `seal` consumes this startup-only object.
    pub fn register_startup_project_private(
        &mut self,
        registration: RegisteredProjectRegistration,
    ) -> Result<CompilerProject, CompilerServiceError> {
        let project = self.adapter.register_core_project_private(registration)?;
        self.registered_projects.push(project);
        Ok(project)
    }

    /// Returns how many registrations the trusted startup owner admitted.
    /// It is intentionally a count, not a remote project enumeration API.
    pub fn registered_project_count(&self) -> usize {
        self.registered_projects.len()
    }

    /// Closes startup registration and creates the owner-side session object
    /// used by the main core loop. `CoreCompilerServiceSession` deliberately
    /// has no registration method.
    pub fn seal(self) -> CoreCompilerServiceSession {
        CoreCompilerServiceSession {
            adapter: self.adapter,
            registered_project_count: self.registered_projects.len(),
        }
    }
}

/// The sealed, main-session owner of the registered-project compiler cache.
/// It can service only pre-negotiated opaque query requests received from a
/// [`CompilerServiceIpcRequestReceiver`].
#[derive(Debug)]
pub struct CoreCompilerServiceSession {
    adapter: CompilerServiceIpcAdapter,
    registered_project_count: usize,
}

impl CoreCompilerServiceSession {
    /// Drains a bounded batch on the core session thread. The listener worker
    /// owns framing/handshake only and never obtains this adapter or its
    /// incremental compiler state.
    pub fn dispatch_pending(&mut self, receiver: &CompilerServiceIpcRequestReceiver) -> usize {
        receiver.dispatch_pending(&mut self.adapter)
    }

    /// A startup-only count useful for core lifecycle diagnostics and tests.
    /// Only explicitly exposed project identities can reach a public caller
    /// through its inventoried opaque handle and `DescribeProject` query.
    pub fn registered_project_count(&self) -> usize {
        self.registered_project_count
    }
}

fn validate_limits(
    limits: CompilerServiceIpcLimits,
) -> Result<(), CompilerServiceIpcConfigurationError> {
    if limits.max_response_bytes == 0 {
        return Err(CompilerServiceIpcConfigurationError::ZeroResponseBytes);
    }
    if limits.max_response_bytes > blueice_ipc::compiler::MAX_COMPILER_MESSAGE_BYTES {
        return Err(CompilerServiceIpcConfigurationError::ResponseExceedsTransportLimit);
    }
    if limits.max_field_bytes == 0 {
        return Err(CompilerServiceIpcConfigurationError::ZeroFieldBytes);
    }
    if limits.max_diagnostic_page_entries == 0 {
        return Err(CompilerServiceIpcConfigurationError::ZeroDiagnosticPageEntries);
    }
    if limits.max_work_set_page_entries == 0 {
        return Err(CompilerServiceIpcConfigurationError::ZeroWorkSetPageEntries);
    }
    if limits.max_static_metadata_page_entries == 0 {
        return Err(CompilerServiceIpcConfigurationError::ZeroStaticMetadataPageEntries);
    }
    if limits.max_stream_cursor_receipts == 0 {
        return Err(CompilerServiceIpcConfigurationError::ZeroStreamCursorReceipts);
    }
    Ok(())
}

fn project_to_wire(project_id: RegisteredProjectId) -> CompilerProject {
    CompilerProject {
        id: project_id.as_u64(),
    }
}

fn generation_to_wire(generation: RegisteredProjectGeneration) -> CompilerGeneration {
    CompilerGeneration {
        project: project_to_wire(generation.project_id()),
        sequence: generation.sequence(),
    }
}

fn compiler_declaration_location(
    span: &blueice_bluets::SourceSpan,
    location: blueice_bluets::DebugSourceLocation,
) -> Option<(u64, u64, CompilerSourceCoordinates)> {
    let start_byte = u64::try_from(span.start).ok()?;
    let end_byte = u64::try_from(span.end).ok()?;
    let coordinates = CompilerSourceCoordinates {
        start_line: u32::try_from(location.start.line).ok()?,
        start_column_utf16: u32::try_from(location.start.column_utf16).ok()?,
        end_line: u32::try_from(location.end.line).ok()?,
        end_column_utf16: u32::try_from(location.end.column_utf16).ok()?,
    };
    coordinates
        .is_well_formed_for_range(start_byte, end_byte)
        .then_some((start_byte, end_byte, coordinates))
}

fn invalid_location_target_reply() -> CompilerReply {
    CompilerReply::Error {
        code: CompilerErrorCode::InvalidLocationTarget,
        message: "static declaration does not belong to the requested source ID".to_string(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CompilerHandleError {
    InvalidProject,
    InvalidGeneration,
}

fn project_from_wire(project: CompilerProject) -> Result<RegisteredProjectId, CompilerHandleError> {
    if !project.is_well_formed() {
        return Err(CompilerHandleError::InvalidProject);
    }
    RegisteredProjectId::from_wire(project.id).ok_or(CompilerHandleError::InvalidProject)
}

fn generation_from_wire(
    generation: CompilerGeneration,
) -> Result<RegisteredProjectGeneration, CompilerHandleError> {
    if !generation.is_well_formed() {
        return Err(CompilerHandleError::InvalidGeneration);
    }
    let project = project_from_wire(generation.project)
        .map_err(|_| CompilerHandleError::InvalidGeneration)?;
    RegisteredProjectGeneration::from_wire(project, generation.sequence)
        .ok_or(CompilerHandleError::InvalidGeneration)
}

fn handle_error_reply(error: CompilerHandleError) -> CompilerReply {
    match error {
        CompilerHandleError::InvalidProject => CompilerReply::Error {
            code: CompilerErrorCode::InvalidProject,
            message: "invalid registered compiler project".to_string(),
        },
        CompilerHandleError::InvalidGeneration => CompilerReply::Error {
            code: CompilerErrorCode::StaleGeneration,
            message: "invalid compiler generation".to_string(),
        },
    }
}

fn service_error_reply(error: &CompilerServiceError) -> CompilerReply {
    let (code, message) = match error {
        CompilerServiceError::UnknownProject { .. } => (
            CompilerErrorCode::InvalidProject,
            "unknown registered compiler project",
        ),
        CompilerServiceError::StaleGeneration { .. } => (
            CompilerErrorCode::StaleGeneration,
            "stale compiler generation",
        ),
        CompilerServiceError::NoStaticMetadata { .. } => (
            CompilerErrorCode::NoStaticMetadata,
            "compiler generation has no static metadata",
        ),
        CompilerServiceError::UnknownType { .. } => (
            CompilerErrorCode::UnknownType,
            "unknown static compiler type",
        ),
        CompilerServiceError::UnknownSymbol { .. } => (
            CompilerErrorCode::UnknownSymbol,
            "unknown static compiler symbol",
        ),
        CompilerServiceError::UnknownSource { .. } => (
            CompilerErrorCode::UnknownSource,
            "unknown static compiler provenance source",
        ),
        CompilerServiceError::UnknownContract { .. } => (
            CompilerErrorCode::UnknownContract,
            "unknown static compiler contract",
        ),
        CompilerServiceError::InvalidStaticMetadataCursor { .. } => (
            CompilerErrorCode::InvalidMetadataCursor,
            "invalid, consumed, stale, or mismatched static metadata cursor",
        ),
        CompilerServiceError::InvalidDiagnosticCursor { .. } => (
            CompilerErrorCode::InvalidDiagnosticCursor,
            "invalid, consumed, stale, or mismatched compiler diagnostic cursor",
        ),
        CompilerServiceError::InvalidWorkSetCursor { .. } => (
            CompilerErrorCode::InvalidWorkSetCursor,
            "invalid, consumed, stale, or mismatched compiler work-set cursor",
        ),
        CompilerServiceError::InvalidStaticMetadataPage { .. } => (
            CompilerErrorCode::InvalidMetadataPage,
            "invalid static metadata page request",
        ),
        CompilerServiceError::InvalidDiagnosticPage { .. } => (
            CompilerErrorCode::InvalidDiagnosticPage,
            "invalid compiler diagnostic page request",
        ),
        CompilerServiceError::InvalidWorkSetPage { .. } => (
            CompilerErrorCode::InvalidWorkSetPage,
            "invalid compiler work-set page request",
        ),
        CompilerServiceError::ProjectLimit { .. }
        | CompilerServiceError::GenerationExhausted { .. }
        | CompilerServiceError::StaticMetadataLimit { .. }
        | CompilerServiceError::StaticMetadataCursorLimit { .. }
        | CompilerServiceError::DiagnosticCursorLimit { .. }
        | CompilerServiceError::WorkSetCursorLimit { .. }
        | CompilerServiceError::BuildOutputLimit { .. } => (
            CompilerErrorCode::ResourceLimit,
            "compiler service resource limit reached",
        ),
        CompilerServiceError::InvalidRegistration { .. }
        | CompilerServiceError::EntryModuleNotAuthorized { .. }
        | CompilerServiceError::DuplicateRegistration
        | CompilerServiceError::ProjectIdExhausted => (
            CompilerErrorCode::Unavailable,
            "compiler operation is unavailable",
        ),
    };
    CompilerReply::Error {
        code,
        message: message.to_string(),
    }
}

fn response_limit_reply() -> CompilerReply {
    CompilerReply::Error {
        code: CompilerErrorCode::ResourceLimit,
        message: "compiler IPC response exceeds the configured core budget".to_string(),
    }
}

/// Converts the wire's data-only tree into BlueTS's pure contract value while
/// applying the core-selected bounds before a second recursive structure is
/// retained. It deliberately rejects non-finite numbers and oversized object
/// keys as malformed input rather than treating them as static type data.
fn wire_contract_value(
    value: CompilerContractValue,
    limits: blueice_bluets::ValidationLimits,
) -> Result<ContractValue, ()> {
    fn convert(
        value: CompilerContractValue,
        limits: blueice_bluets::ValidationLimits,
        depth: usize,
        nodes: &mut usize,
    ) -> Result<ContractValue, ()> {
        if depth > limits.max_depth || *nodes >= limits.max_nodes {
            return Err(());
        }
        *nodes += 1;
        match value {
            CompilerContractValue::Null => Ok(ContractValue::Null),
            CompilerContractValue::Undefined => Ok(ContractValue::Undefined),
            CompilerContractValue::Boolean(value) => Ok(ContractValue::Boolean(value)),
            CompilerContractValue::Number(value) => value
                .parse::<f64>()
                .ok()
                .filter(|value| value.is_finite())
                .map(ContractValue::Number)
                .ok_or(()),
            CompilerContractValue::String(value) => (value.len() <= limits.max_string_bytes)
                .then_some(ContractValue::String(value))
                .ok_or(()),
            CompilerContractValue::Array(values) => {
                if values.len() > limits.max_collection_entries {
                    return Err(());
                }
                values
                    .into_iter()
                    .map(|value| convert(value, limits, depth + 1, nodes))
                    .collect::<Result<Vec<_>, _>>()
                    .map(ContractValue::Array)
            }
            CompilerContractValue::Object(values) => {
                if values.len() > limits.max_collection_entries
                    || values.keys().any(|key| key.len() > limits.max_string_bytes)
                {
                    return Err(());
                }
                values
                    .into_iter()
                    .map(|(key, value)| {
                        convert(value, limits, depth + 1, nodes).map(|value| (key, value))
                    })
                    .collect::<Result<std::collections::BTreeMap<_, _>, _>>()
                    .map(ContractValue::Object)
            }
        }
    }

    let mut nodes = 0;
    convert(value, limits, 0, &mut nodes)
}

fn validation_failure_to_wire(error: ValidationError) -> CompilerContractValidationFailure {
    CompilerContractValidationFailure {
        path: error.path,
        expected: error.expected,
        observed: error.observed,
    }
}

fn module_list(
    modules: &BTreeSet<String>,
    limits: CompilerServiceIpcLimits,
    budget: &mut ResponseBudget,
) -> Result<CompilerModuleList, ()> {
    let mut entries = Vec::new();
    let mut truncated = false;
    for module in modules {
        if entries.len() >= limits.max_modules_per_set {
            truncated = true;
            break;
        }
        if !budget.reserve_optional_string(module, limits.max_field_bytes)? {
            truncated = true;
            break;
        }
        if !budget.reserve_optional_fixed(64) {
            truncated = true;
            break;
        }
        entries.push(module.clone());
    }
    truncated |= entries.len() != modules.len();
    Ok(CompilerModuleList { entries, truncated })
}

fn retained_diagnostics_fit_wire_policy(
    diagnostics: &[Diagnostic],
    locations: &[Option<DebugSourceLocation>],
    limits: CompilerServiceIpcLimits,
) -> bool {
    diagnostics.len() == locations.len()
        && diagnostics
            .iter()
            .zip(locations)
            .all(|(diagnostic, location)| {
                diagnostic.code.to_string().len() <= COMPILER_DIAGNOSTIC_MAX_CODE_BYTES
                    && diagnostic.span.module.len() <= limits.max_field_bytes
                    && diagnostic.message.len() <= limits.max_field_bytes
                    && u64::try_from(diagnostic.span.start).is_ok()
                    && u64::try_from(diagnostic.span.end).is_ok()
                    && diagnostic_coordinates_to_wire(diagnostic, *location).is_ok()
            })
}

fn diagnostic_coordinates_to_wire(
    diagnostic: &Diagnostic,
    location: Option<DebugSourceLocation>,
) -> Result<Option<CompilerSourceCoordinates>, ()> {
    let Some(location) = location else {
        return Ok(None);
    };
    let coordinates = CompilerSourceCoordinates {
        start_line: u32::try_from(location.start.line).map_err(|_| ())?,
        start_column_utf16: u32::try_from(location.start.column_utf16).map_err(|_| ())?,
        end_line: u32::try_from(location.end.line).map_err(|_| ())?,
        end_column_utf16: u32::try_from(location.end.column_utf16).map_err(|_| ())?,
    };
    let start = u64::try_from(diagnostic.span.start).map_err(|_| ())?;
    let end = u64::try_from(diagnostic.span.end).map_err(|_| ())?;
    coordinates
        .is_well_formed_for_diagnostic_range(start, end)
        .then_some(Some(coordinates))
        .ok_or(())
}

fn diagnostics_to_wire(
    diagnostics: &[Diagnostic],
    locations: &[Option<DebugSourceLocation>],
    service_truncated: bool,
    limits: CompilerServiceIpcLimits,
    budget: &mut ResponseBudget,
) -> Result<CompilerDiagnostics, ()> {
    if diagnostics.len() != locations.len() {
        return Err(());
    }
    let mut entries = Vec::new();
    let mut truncated = service_truncated;
    for (diagnostic, location) in diagnostics.iter().zip(locations) {
        if entries.len() >= limits.max_diagnostics {
            truncated = true;
            break;
        }
        let code = diagnostic.code.to_string();
        if code.len() > COMPILER_DIAGNOSTIC_MAX_CODE_BYTES
            || !budget.reserve_optional_string(&code, COMPILER_DIAGNOSTIC_MAX_CODE_BYTES)?
            || !budget.reserve_optional_string(&diagnostic.span.module, limits.max_field_bytes)?
            || !budget.reserve_optional_string(&diagnostic.message, limits.max_field_bytes)?
            || !budget.reserve_optional_fixed(320)
        {
            truncated = true;
            break;
        }
        let (start, end) = match (
            u64::try_from(diagnostic.span.start),
            u64::try_from(diagnostic.span.end),
        ) {
            (Ok(start), Ok(end)) => (start, end),
            _ => return Err(()),
        };
        entries.push(CompilerDiagnostic {
            code,
            severity: match diagnostic.severity {
                Severity::Error => CompilerDiagnosticSeverity::Error,
                Severity::Warning => CompilerDiagnosticSeverity::Warning,
            },
            module: diagnostic.span.module.clone(),
            start,
            end,
            coordinates: diagnostic_coordinates_to_wire(diagnostic, *location)?,
            message: diagnostic.message.clone(),
        });
    }
    truncated |= entries.len() != diagnostics.len();
    Ok(CompilerDiagnostics { entries, truncated })
}

/// Converts an already fixed-size diagnostic page without silently dropping
/// an entry. The caller selected its page cap from the worst-case envelope
/// before the service consumed the one-shot cursor, so `Ok` means the public
/// page is complete for that cursor rather than an unmarked partial page.
fn diagnostic_page_entries_to_wire(
    diagnostics: &[Diagnostic],
    locations: &[Option<DebugSourceLocation>],
    limits: CompilerServiceIpcLimits,
    budget: &mut ResponseBudget,
) -> Result<Vec<CompilerDiagnostic>, ()> {
    if diagnostics.len() != locations.len() {
        return Err(());
    }
    diagnostics
        .iter()
        .zip(locations)
        .map(|(diagnostic, location)| {
            let code = diagnostic.code.to_string();
            if code.len() > COMPILER_DIAGNOSTIC_MAX_CODE_BYTES
                || !budget.reserve_required_string(&code, COMPILER_DIAGNOSTIC_MAX_CODE_BYTES)
                || !budget.reserve_required_string(&diagnostic.span.module, limits.max_field_bytes)
                || !budget.reserve_required_string(&diagnostic.message, limits.max_field_bytes)
                || !budget.reserve_optional_fixed(352)
            {
                return Err(());
            }
            let (start, end) = (
                u64::try_from(diagnostic.span.start).map_err(|_| ())?,
                u64::try_from(diagnostic.span.end).map_err(|_| ())?,
            );
            Ok(CompilerDiagnostic {
                code,
                severity: match diagnostic.severity {
                    Severity::Error => CompilerDiagnosticSeverity::Error,
                    Severity::Warning => CompilerDiagnosticSeverity::Warning,
                },
                module: diagnostic.span.module.clone(),
                start,
                end,
                coordinates: diagnostic_coordinates_to_wire(diagnostic, *location)?,
                message: diagnostic.message.clone(),
            })
        })
        .collect()
}

fn symbol_kind_to_wire(kind: SymbolKind) -> CompilerSymbolKind {
    match kind {
        SymbolKind::Import => CompilerSymbolKind::Import,
        SymbolKind::TypeAlias => CompilerSymbolKind::TypeAlias,
        SymbolKind::Interface => CompilerSymbolKind::Interface,
        SymbolKind::Variable => CompilerSymbolKind::Variable,
        SymbolKind::Function => CompilerSymbolKind::Function,
    }
}

fn static_metadata_kind_from_wire(kind: CompilerStaticMetadataKind) -> StaticMetadataInventoryKind {
    match kind {
        CompilerStaticMetadataKind::Sources => StaticMetadataInventoryKind::Sources,
        CompilerStaticMetadataKind::Types => StaticMetadataInventoryKind::Types,
        CompilerStaticMetadataKind::Symbols => StaticMetadataInventoryKind::Symbols,
        CompilerStaticMetadataKind::Contracts => StaticMetadataInventoryKind::Contracts,
    }
}

fn work_set_kind_from_wire(kind: CompilerWorkSetKind) -> WorkSetInventoryKind {
    match kind {
        CompilerWorkSetKind::Parsed => WorkSetInventoryKind::Parsed,
        CompilerWorkSetKind::ReusedParsed => WorkSetInventoryKind::ReusedParsed,
        CompilerWorkSetKind::Rechecked => WorkSetInventoryKind::Rechecked,
        CompilerWorkSetKind::ReusedChecked => WorkSetInventoryKind::ReusedChecked,
    }
}

/// Pessimistic response accounting for JSON serialisation. A UTF-8 byte can
/// require at most six JSON bytes (`\\u00xx`), and object/list punctuation is
/// charged conservatively at each retained entry.
#[derive(Debug, Clone, Copy)]
struct ResponseBudget {
    remaining: usize,
}

impl ResponseBudget {
    fn new(remaining: usize) -> Self {
        Self { remaining }
    }

    fn reserve_fixed(&mut self, bytes: usize) -> bool {
        self.reserve(bytes)
    }

    fn reserve_optional_fixed(&mut self, bytes: usize) -> bool {
        self.reserve(bytes)
    }

    fn reserve_required_string(&mut self, value: &str, max_field_bytes: usize) -> bool {
        self.reserve_optional_string(value, max_field_bytes)
            .unwrap_or(false)
    }

    /// `Err` means one field violates its independent cap and the caller must
    /// reject the whole reply. `Ok(false)` means the global page budget is
    /// exhausted and callers with list semantics may return an explicitly
    /// truncated result instead.
    fn reserve_optional_string(&mut self, value: &str, max_field_bytes: usize) -> Result<bool, ()> {
        if value.len() > max_field_bytes {
            return Err(());
        }
        let encoded = value
            .len()
            .checked_mul(6)
            .and_then(|bytes| bytes.checked_add(2))
            .ok_or(())?;
        Ok(self.reserve(encoded))
    }

    fn reserve(&mut self, bytes: usize) -> bool {
        if bytes > self.remaining {
            return false;
        }
        self.remaining -= bytes;
        true
    }
}

/// A worker-thread sender for the compiler adapter. It carries decoded,
/// authority-free requests to the owner of the registered-project service.
#[derive(Clone)]
pub struct CompilerServiceIpcRequestSender(mpsc::Sender<CompilerServiceIpcRequestEnvelope>);

/// An accepted compiler stream's private hand-off handle. The core listener
/// binds it only after a valid Hello minted a unique attestation; the remote
/// request has no field with which to choose or replace this identity. Drop
/// queues cursor cleanup even when the peer disconnects mid-pagination.
pub struct CompilerServiceIpcSessionSender {
    sender: CompilerServiceIpcRequestSender,
    session_id: String,
}

/// Session-owner receiver for the compiler adapter. Only this side obtains a
/// mutable adapter and therefore the mutable incremental compiler cache.
pub struct CompilerServiceIpcRequestReceiver(mpsc::Receiver<CompilerServiceIpcRequestEnvelope>);

enum CompilerServiceIpcRequestEnvelope {
    Request {
        session_id: String,
        request: CompilerRequest,
        reply: mpsc::SyncSender<CompilerReply>,
    },
    EndSession {
        session_id: String,
    },
}

/// Builds the worker-to-owner hand-off used by the core compiler socket.
pub fn compiler_service_ipc_request_channel() -> (
    CompilerServiceIpcRequestSender,
    CompilerServiceIpcRequestReceiver,
) {
    let (sender, receiver) = mpsc::channel();
    (
        CompilerServiceIpcRequestSender(sender),
        CompilerServiceIpcRequestReceiver(receiver),
    )
}

impl CompilerServiceIpcRequestSender {
    /// Binds one accepted worker stream to the core-minted Hello attestation.
    /// Only this internal handle can present a pagination cursor receipt.
    pub fn bind_session(
        &self,
        attestation: CompilerSessionAttestation,
    ) -> io::Result<CompilerServiceIpcSessionSender> {
        if !attestation.is_well_formed() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid core compiler session attestation",
            ));
        }
        Ok(CompilerServiceIpcSessionSender {
            sender: self.clone(),
            session_id: attestation.id,
        })
    }
}

impl CompilerServiceIpcSessionSender {
    /// Routes one request through the exact accepted stream's core-side
    /// cursor ledger. A stopped owner is a transport failure, not an invented
    /// compiler reply.
    pub fn request(&self, request: CompilerRequest) -> io::Result<CompilerReply> {
        let (reply_sender, reply_receiver) = mpsc::sync_channel(1);
        self.sender
            .0
            .send(CompilerServiceIpcRequestEnvelope::Request {
                session_id: self.session_id.clone(),
                request,
                reply: reply_sender,
            })
            .map_err(|_| {
                io::Error::new(io::ErrorKind::BrokenPipe, "core compiler session ended")
            })?;
        reply_receiver.recv().map_err(|_| {
            io::Error::new(io::ErrorKind::BrokenPipe, "core compiler reply unavailable")
        })
    }
}

impl Drop for CompilerServiceIpcSessionSender {
    fn drop(&mut self) {
        let _ = self
            .sender
            .0
            .send(CompilerServiceIpcRequestEnvelope::EndSession {
                session_id: self.session_id.clone(),
            });
    }
}

impl CompilerServiceIpcRequestReceiver {
    /// Applies a bounded batch under the service owner. A disconnected worker
    /// cannot interrupt compilation, page processing, or future rendering.
    pub fn dispatch_pending(&self, adapter: &mut CompilerServiceIpcAdapter) -> usize {
        const MAX_REQUESTS_PER_TICK: usize = 64;
        let mut dispatched = 0;
        while dispatched < MAX_REQUESTS_PER_TICK {
            let Ok(envelope) = self.0.try_recv() else {
                break;
            };
            match envelope {
                CompilerServiceIpcRequestEnvelope::Request {
                    session_id,
                    request,
                    reply,
                } => {
                    let result = adapter.handle_session_request(&session_id, request);
                    let _ = reply.send(result);
                }
                CompilerServiceIpcRequestEnvelope::EndSession { session_id } => {
                    adapter.end_session(&session_id);
                }
            }
            dispatched += 1;
        }
        dispatched
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use blueice_bluets::{
        AuthorizedModule, AuthorizedModuleLoader, AuthorizedModuleResolution, CompilerOptions,
        RuntimePolicy,
    };

    mod inventory_tests {
        include!("compiler_ipc/inventory_tests.rs");
    }

    const ENTRY: &str = "project:///app/main.ts";
    const DEPENDENCY: &str = "project:///app/math.ts";

    fn registration(source: &str) -> RegisteredProjectRegistration {
        RegisteredProjectRegistration {
            canonical_project_root: "project:///app".to_string(),
            canonical_config_root: "project:///app/blue-ts.json".to_string(),
            canonical_output_root: "project:///dist".to_string(),
            entry_module: ENTRY.to_string(),
            loader: AuthorizedModuleLoader::new(
                [
                    AuthorizedModule::new(
                        ENTRY,
                        format!("import {{ answer }} from './math'; {source}"),
                    ),
                    AuthorizedModule::new(DEPENDENCY, "export const answer: number = 42;"),
                ],
                [AuthorizedModuleResolution::new(ENTRY, "./math", DEPENDENCY)],
            )
            .unwrap(),
            compiler_options: CompilerOptions {
                resolver_fingerprint: "registered-project-resolver-v1".to_string(),
                runtime_policy: RuntimePolicy::Checked,
                ..CompilerOptions::default()
            },
        }
    }

    fn adapter() -> (CompilerServiceIpcAdapter, CompilerProject) {
        let mut adapter = CompilerServiceIpcAdapter::default();
        let project = adapter
            .register_core_project(registration("export const value: number = answer;"))
            .unwrap();
        (adapter, project)
    }

    fn inventory_on_stream(
        adapter: &mut CompilerServiceIpcAdapter,
        stream: &str,
        project: CompilerProject,
    ) {
        let CompilerReply::Projects(inventory) =
            adapter.handle_session_request(stream, CompilerRequest::ListProjects)
        else {
            panic!("accepted stream must receive sealed project inventory")
        };
        assert!(inventory.is_well_formed());
        assert!(inventory.projects.contains(&project));
    }

    #[test]
    fn sealed_project_inventory_is_bounded_and_stream_local() {
        let (mut adapter, first) = adapter();
        let mut second_registration = registration("export const next: number = answer;");
        second_registration.canonical_project_root = "project:///next".to_string();
        second_registration.canonical_config_root = "project:///next/blue-ts.json".to_string();
        let second = adapter.register_core_project(second_registration).unwrap();
        let first_stream = "a".repeat(CompilerSessionAttestation::ID_LENGTH);
        let second_stream = "b".repeat(CompilerSessionAttestation::ID_LENGTH);
        for (stream, project) in [(&first_stream, first), (&second_stream, second)] {
            assert!(matches!(
                adapter
                    .handle_session_request(stream, CompilerRequest::DescribeProject { project }),
                CompilerReply::Error {
                    code: CompilerErrorCode::UnobservedProject,
                    ..
                }
            ));
        }
        let CompilerReply::Projects(inventory) =
            adapter.handle_session_request(&first_stream, CompilerRequest::ListProjects)
        else {
            panic!("sealed project inventory must be available")
        };
        assert_eq!(inventory.projects, vec![first, second]);
        let mut later_registration = registration("export const later: number = answer;");
        later_registration.canonical_project_root = "project:///later".to_string();
        later_registration.canonical_config_root = "project:///later/blue-ts.json".to_string();
        let later = adapter.register_core_project(later_registration).unwrap();
        assert!(matches!(
            adapter.handle_session_request(
                &first_stream,
                CompilerRequest::DescribeProject { project: later }
            ),
            CompilerReply::Error {
                code: CompilerErrorCode::UnobservedProject,
                ..
            }
        ));
        assert!(matches!(
            adapter.handle_session_request(
                &first_stream,
                CompilerRequest::DescribeProject { project: first }
            ),
            CompilerReply::Project(_)
        ));
        assert!(matches!(
            adapter
                .handle_session_request(&second_stream, CompilerRequest::Check { project: first }),
            CompilerReply::Error {
                code: CompilerErrorCode::UnobservedProject,
                ..
            }
        ));
        assert!(matches!(
            adapter.handle_session_request(
                &first_stream,
                CompilerRequest::Check {
                    project: CompilerProject { id: u64::MAX }
                }
            ),
            CompilerReply::Error {
                code: CompilerErrorCode::UnobservedProject,
                ..
            }
        ));
        adapter.end_session(&first_stream);
        assert!(matches!(
            adapter
                .handle_session_request(&first_stream, CompilerRequest::Check { project: first }),
            CompilerReply::Error {
                code: CompilerErrorCode::UnobservedProject,
                ..
            }
        ));
    }

    #[test]
    fn opaque_project_queries_return_generation_bound_source_free_metadata() {
        let (mut adapter, project) = adapter();
        assert_eq!(
            adapter.handle(CompilerRequest::DescribeProject { project }),
            CompilerReply::Project(CompilerProjectIdentity {
                project,
                entry_module: ENTRY.to_string(),
            })
        );

        let CompilerReply::Check(check) = adapter.handle(CompilerRequest::Check { project }) else {
            panic!("registered project must check through adapter")
        };
        assert!(!check.has_errors, "{check:#?}");
        assert!(!check.parsed_modules.truncated);
        assert!(check
            .parsed_modules
            .entries
            .iter()
            .any(|module| module == ENTRY));
        assert!(check.artifact_fingerprint.is_some());
        let metadata = check.static_metadata.as_ref().unwrap();
        assert_eq!(metadata.source_count, 2);
        assert!(metadata.type_count > 0);
        assert!(metadata.symbol_count > 0);

        let CompilerReply::StaticType(static_type) =
            adapter.handle(CompilerRequest::GetStaticType {
                generation: check.generation,
                type_id: 0,
            })
        else {
            panic!("checked type must be generation-addressable")
        };
        assert_eq!(static_type.generation, check.generation);
        // The first deterministic type belongs to the imported binding. Its
        // checked static shape is intentionally `unknown` until a richer
        // import type surface is implemented; this still proves a precise,
        // generation-bound type lookup rather than a runtime-value query.
        assert_eq!(static_type.display, "unknown");

        let CompilerReply::StaticSymbol(symbol) =
            adapter.handle(CompilerRequest::GetStaticSymbol {
                generation: check.generation,
                symbol_id: 0,
            })
        else {
            panic!("checked symbol must be generation-addressable")
        };
        assert_eq!(symbol.generation, check.generation);
        assert_eq!(symbol.kind, CompilerSymbolKind::Import);
        assert!(!symbol.exported);
        assert_eq!(symbol.module, ENTRY);
        assert_ne!(symbol.module, "import { answer } from './math';");
        let CompilerReply::StaticSymbol(exported) =
            adapter.handle(CompilerRequest::GetStaticSymbol {
                generation: check.generation,
                symbol_id: 1,
            })
        else {
            panic!("exported declaration must be generation-addressable")
        };
        assert_eq!(exported.name, "value");
        assert!(exported.exported);
    }

    #[test]
    fn declaration_locations_require_exact_generation_and_source_ownership() {
        let mut adapter = CompilerServiceIpcAdapter::default();
        let project = adapter
            .register_core_project(registration(
                "export interface Shape { value: number; }\r\n/* 🚀 */ const value: number = answer;",
            ))
            .unwrap();
        let CompilerReply::Check(check) = adapter.handle(CompilerRequest::Check { project }) else {
            panic!("registered project must check")
        };
        assert!(!check.has_errors, "{check:#?}");
        let symbol_count = check.static_metadata.as_ref().unwrap().symbol_count;
        let symbols: Vec<_> = (0..symbol_count)
            .filter_map(|symbol_id| {
                match adapter.handle(CompilerRequest::GetStaticSymbol {
                    generation: check.generation,
                    symbol_id,
                }) {
                    CompilerReply::StaticSymbol(symbol) => Some(symbol),
                    _ => None,
                }
            })
            .collect();
        let variable = symbols
            .iter()
            .find(|symbol| symbol.name == "value")
            .unwrap();
        let interface = symbols
            .iter()
            .find(|symbol| symbol.name == "Shape")
            .unwrap();
        let contract_id = interface.contract_id.expect("interface is reifiable");
        let CompilerReply::StaticSymbolLocation(location) =
            adapter.handle(CompilerRequest::GetStaticSymbolLocation {
                generation: check.generation,
                symbol_id: variable.id,
                source_id: variable.source_id,
            })
        else {
            panic!("symbol location must be available")
        };
        assert!(location.is_well_formed());
        assert_eq!(location.coordinates.start_line, 1);
        assert_eq!(location.coordinates.start_column_utf16, 9);
        assert!(!format!("{location:?}").contains("value"));
        let CompilerReply::StaticContractLocation(contract_location) =
            adapter.handle(CompilerRequest::GetStaticContractLocation {
                generation: check.generation,
                contract_id,
                source_id: interface.source_id,
            })
        else {
            panic!("contract location must be available")
        };
        assert!(contract_location.is_well_formed());
        assert_eq!(contract_location.coordinates.start_line, 0);
        assert!(!format!("{contract_location:?}").contains("Shape"));
        assert!(matches!(
            adapter.handle(CompilerRequest::GetStaticSymbolLocation {
                generation: check.generation,
                symbol_id: variable.id,
                source_id: variable.source_id.wrapping_add(1),
            }),
            CompilerReply::Error {
                code: CompilerErrorCode::InvalidLocationTarget,
                ..
            }
        ));
        assert!(matches!(
            adapter.handle(CompilerRequest::GetStaticContractLocation {
                generation: check.generation,
                contract_id,
                source_id: interface.source_id.wrapping_add(1),
            }),
            CompilerReply::Error {
                code: CompilerErrorCode::InvalidLocationTarget,
                ..
            }
        ));
        let CompilerReply::Check(next) = adapter.handle(CompilerRequest::Check { project }) else {
            panic!("second check must succeed")
        };
        assert_ne!(next.generation, check.generation);
        assert!(matches!(
            adapter.handle(CompilerRequest::GetStaticSymbolLocation {
                generation: check.generation,
                symbol_id: variable.id,
                source_id: variable.source_id,
            }),
            CompilerReply::Error {
                code: CompilerErrorCode::StaleGeneration,
                ..
            }
        ));
    }

    #[test]
    fn later_check_invalidates_an_old_generation_without_retargeting() {
        let (mut adapter, project) = adapter();
        let CompilerReply::Check(first) = adapter.handle(CompilerRequest::Check { project }) else {
            panic!("first check must succeed")
        };
        let CompilerReply::Check(second) = adapter.handle(CompilerRequest::Check { project })
        else {
            panic!("second check must succeed")
        };
        assert_ne!(first.generation, second.generation);
        assert!(matches!(
            adapter.handle(CompilerRequest::GetStaticType {
                generation: first.generation,
                type_id: 0,
            }),
            CompilerReply::Error {
                code: CompilerErrorCode::StaleGeneration,
                ..
            }
        ));
    }

    #[test]
    fn adapter_rejects_over_budget_or_non_finite_contract_snapshots() {
        let mut adapter = CompilerServiceIpcAdapter::default();
        let project = adapter
            .register_core_project(registration(
                "type Name = string; export const name: Name = 'blueice';",
            ))
            .unwrap();
        let CompilerReply::Check(check) = adapter.handle(CompilerRequest::Check { project }) else {
            panic!("registered project must check")
        };
        let CompilerReply::StaticSymbol(symbol) =
            adapter.handle(CompilerRequest::GetStaticSymbol {
                generation: check.generation,
                symbol_id: 1,
            })
        else {
            panic!("type alias symbol must be retained")
        };
        let contract_id = symbol.contract_id.unwrap();
        let oversized = CompilerContractValue::String("x".repeat(256 * 1_024 + 1));
        assert!(matches!(
            adapter.handle(CompilerRequest::ValidateStaticContract {
                generation: check.generation,
                contract_id,
                value: oversized,
            }),
            CompilerReply::Error {
                code: CompilerErrorCode::InvalidContractValue,
                ..
            }
        ));
        assert!(matches!(
            adapter.handle(CompilerRequest::ValidateStaticContract {
                generation: check.generation,
                contract_id,
                value: CompilerContractValue::Number("NaN".to_string()),
            }),
            CompilerReply::Error {
                code: CompilerErrorCode::InvalidContractValue,
                ..
            }
        ));
    }

    #[test]
    fn malformed_or_unknown_handles_never_create_or_select_a_project() {
        let (mut adapter, _) = adapter();
        assert!(matches!(
            adapter.handle(CompilerRequest::Check {
                project: CompilerProject { id: 0 },
            }),
            CompilerReply::Error {
                code: CompilerErrorCode::InvalidProject,
                ..
            }
        ));
        assert!(matches!(
            adapter.handle(CompilerRequest::DescribeProject {
                project: CompilerProject { id: 999 },
            }),
            CompilerReply::Error {
                code: CompilerErrorCode::InvalidProject,
                ..
            }
        ));
        assert!(matches!(
            adapter.handle(CompilerRequest::GetStaticSymbol {
                generation: CompilerGeneration {
                    project: CompilerProject { id: 1 },
                    sequence: 0,
                },
                symbol_id: 0,
            }),
            CompilerReply::Error {
                code: CompilerErrorCode::StaleGeneration,
                ..
            }
        ));
    }

    #[test]
    fn adapter_refuses_an_over_budget_field_without_exposing_partial_data() {
        let mut adapter = CompilerServiceIpcAdapter::new(
            RegisteredProjectCompilerService::default(),
            CompilerServiceIpcLimits {
                max_field_bytes: 4,
                ..CompilerServiceIpcLimits::default()
            },
        )
        .unwrap();
        let project = adapter
            .register_core_project(registration("export const value: number = answer;"))
            .unwrap();
        assert!(matches!(
            adapter.handle(CompilerRequest::DescribeProject { project }),
            CompilerReply::Error {
                code: CompilerErrorCode::ResourceLimit,
                ..
            }
        ));
        assert!(matches!(
            adapter.handle(CompilerRequest::Check { project }),
            CompilerReply::Error {
                code: CompilerErrorCode::ResourceLimit,
                ..
            }
        ));
    }

    #[test]
    fn check_marks_adapter_capped_work_sets_and_diagnostics_as_truncated() {
        let mut adapter = CompilerServiceIpcAdapter::new(
            RegisteredProjectCompilerService::default(),
            CompilerServiceIpcLimits {
                max_modules_per_set: 1,
                max_diagnostics: 0,
                ..CompilerServiceIpcLimits::default()
            },
        )
        .unwrap();
        let project = adapter
            .register_core_project(registration("export const value: number = 'wrong';"))
            .unwrap();
        let CompilerReply::Check(check) = adapter.handle(CompilerRequest::Check { project }) else {
            panic!("registered project must return a bounded check reply")
        };
        assert!(check.has_errors);
        assert_eq!(check.parsed_modules.entries.len(), 1);
        assert!(check.parsed_modules.truncated);
        assert!(check.rechecked_modules.truncated);
        assert!(check.diagnostics.entries.is_empty());
        assert!(check.diagnostics.truncated);
    }

    #[test]
    fn adapter_pages_diagnostics_with_one_shot_generation_bound_cursors() {
        let mut adapter = CompilerServiceIpcAdapter::new(
            RegisteredProjectCompilerService::new(CompilerServiceLimits {
                max_retained_diagnostics: 4,
                max_diagnostics: 1,
                ..CompilerServiceLimits::default()
            }),
            CompilerServiceIpcLimits {
                max_diagnostics: 0,
                max_diagnostic_page_entries: 1,
                ..CompilerServiceIpcLimits::default()
            },
        )
        .unwrap();
        let project = adapter
            .register_core_project(registration(
                "const marker = '😀';\r\nconst first: number = 'one'; \
                 const second: number = 'two'; \
                 const third: number = 'three';",
            ))
            .unwrap();
        let CompilerReply::Check(check) = adapter.handle(CompilerRequest::Check { project }) else {
            panic!("registered invalid project must return a bounded check reply")
        };
        assert!(check.has_errors);
        assert!(check.diagnostics.entries.is_empty());
        assert!(check.diagnostics.truncated);
        let CompilerReply::DiagnosticPage(first) =
            adapter.handle(CompilerRequest::ListDiagnostics {
                generation: check.generation,
                cursor: None,
                limit: Some(1),
            })
        else {
            panic!("first diagnostic page must be returned for the exact check generation")
        };
        assert_eq!(first.generation, check.generation);
        assert_eq!(first.entries.len(), 1);
        let coordinates = first.entries[0]
            .coordinates
            .expect("an authorized diagnostic must retain original source coordinates");
        assert_eq!(coordinates.start_line, 1);
        assert_eq!(coordinates.end_line, 1);
        assert!(coordinates
            .is_well_formed_for_diagnostic_range(first.entries[0].start, first.entries[0].end,));
        assert!(
            !format!("{first:?}").contains("const first"),
            "paged diagnostic output must not contain the retained project source"
        );
        let cursor = first
            .next_cursor
            .expect("fixture must require a continuation cursor");
        assert!(matches!(
            adapter.handle(CompilerRequest::ListDiagnostics {
                generation: check.generation,
                cursor: Some(cursor),
                limit: Some(0),
            }),
            CompilerReply::Error {
                code: CompilerErrorCode::InvalidDiagnosticPage,
                ..
            }
        ));
        let CompilerReply::DiagnosticPage(_) = adapter.handle(CompilerRequest::ListDiagnostics {
            generation: check.generation,
            cursor: Some(cursor),
            limit: Some(1),
        }) else {
            panic!("a rejected page-limit request must not consume its cursor")
        };
        assert!(matches!(
            adapter.handle(CompilerRequest::ListDiagnostics {
                generation: check.generation,
                cursor: Some(cursor),
                limit: Some(1),
            }),
            CompilerReply::Error {
                code: CompilerErrorCode::InvalidDiagnosticCursor,
                ..
            }
        ));
        let CompilerReply::Check(later) = adapter.handle(CompilerRequest::Check { project }) else {
            panic!("later check must create a successor generation")
        };
        assert!(matches!(
            adapter.handle(CompilerRequest::ListDiagnostics {
                generation: check.generation,
                cursor: None,
                limit: Some(1),
            }),
            CompilerReply::Error {
                code: CompilerErrorCode::StaleGeneration,
                ..
            }
        ));
        assert_ne!(later.generation, check.generation);
    }

    #[test]
    fn immediate_check_diagnostics_include_authorized_utf16_positions() {
        let mut adapter = CompilerServiceIpcAdapter::default();
        let project = adapter
            .register_core_project(registration(
                "const marker = '😀';\r\nconst invalid: number = 'wrong';",
            ))
            .unwrap();
        let CompilerReply::Check(check) = adapter.handle(CompilerRequest::Check { project }) else {
            panic!("the invalid closed project must return a check result")
        };
        let diagnostic = check
            .diagnostics
            .entries
            .iter()
            .find(|entry| entry.code == "BTS3003")
            .expect("a static type mismatch must remain observable");
        let coordinates = diagnostic.coordinates.unwrap();
        assert_eq!(coordinates.start_line, 1);
        assert_eq!(coordinates.end_line, 1);
        assert!(coordinates.is_well_formed_for_diagnostic_range(diagnostic.start, diagnostic.end,));
        assert!(!format!("{check:?}").contains("const marker"));
    }

    #[test]
    fn diagnostic_cursors_are_bound_to_the_receiving_compiler_stream() {
        let mut adapter = CompilerServiceIpcAdapter::new(
            RegisteredProjectCompilerService::new(CompilerServiceLimits {
                max_diagnostic_cursors: 1,
                ..CompilerServiceLimits::default()
            }),
            CompilerServiceIpcLimits {
                max_diagnostic_page_entries: 1,
                ..CompilerServiceIpcLimits::default()
            },
        )
        .unwrap();
        let project = adapter
            .register_core_project(registration(
                "const first: number = 'one'; \
                 const second: number = 'two'; \
                 const third: number = 'three';",
            ))
            .unwrap();
        let first_stream = "a".repeat(CompilerSessionAttestation::ID_LENGTH);
        let second_stream = "b".repeat(CompilerSessionAttestation::ID_LENGTH);
        inventory_on_stream(&mut adapter, &first_stream, project);
        inventory_on_stream(&mut adapter, &second_stream, project);
        let CompilerReply::Check(check) =
            adapter.handle_session_request(&first_stream, CompilerRequest::Check { project })
        else {
            panic!("the invalid fixture must yield a bounded check")
        };
        let CompilerReply::DiagnosticPage(first) = adapter.handle_session_request(
            &first_stream,
            CompilerRequest::ListDiagnostics {
                generation: check.generation,
                cursor: None,
                limit: Some(1),
            },
        ) else {
            panic!("the first stream must receive a diagnostic page")
        };
        let cursor = first
            .next_cursor
            .expect("three diagnostics need continuation");
        let continuation = CompilerRequest::ListDiagnostics {
            generation: check.generation,
            cursor: Some(cursor),
            limit: Some(1),
        };
        assert!(matches!(
            adapter.handle_session_request(&second_stream, continuation.clone()),
            CompilerReply::Error {
                code: CompilerErrorCode::InvalidDiagnosticCursor,
                ..
            }
        ));
        assert!(matches!(
            adapter.handle_session_request(
                &first_stream,
                CompilerRequest::ListDiagnostics {
                    generation: check.generation,
                    cursor: Some(cursor),
                    limit: Some(0),
                },
            ),
            CompilerReply::Error {
                code: CompilerErrorCode::InvalidDiagnosticPage,
                ..
            }
        ));
        adapter.end_session(&first_stream);
        inventory_on_stream(&mut adapter, &first_stream, project);
        assert!(matches!(
            adapter.handle_session_request(&second_stream, continuation),
            CompilerReply::Error {
                code: CompilerErrorCode::InvalidDiagnosticCursor,
                ..
            }
        ));
        let CompilerReply::DiagnosticPage(second) = adapter.handle_session_request(
            &second_stream,
            CompilerRequest::ListDiagnostics {
                generation: check.generation,
                cursor: None,
                limit: Some(1),
            },
        ) else {
            panic!("disconnect must release the sole diagnostic cursor slot")
        };
        let second_cursor = second.next_cursor.expect("a fresh stream can paginate");
        assert_ne!(second_cursor, cursor);
        let CompilerReply::Check(later) =
            adapter.handle_session_request(&first_stream, CompilerRequest::Check { project })
        else {
            panic!("a later check must advance the project generation")
        };
        assert_ne!(later.generation, check.generation);
        assert!(matches!(
            adapter.handle_session_request(
                &second_stream,
                CompilerRequest::ListDiagnostics {
                    generation: check.generation,
                    cursor: Some(second_cursor),
                    limit: Some(1),
                },
            ),
            CompilerReply::Error {
                code: CompilerErrorCode::InvalidDiagnosticCursor,
                ..
            }
        ));
    }

    #[test]
    fn work_set_cursors_are_stream_kind_and_generation_bound() {
        let mut adapter = CompilerServiceIpcAdapter::new(
            RegisteredProjectCompilerService::new(CompilerServiceLimits {
                max_work_set_cursors: 1,
                ..CompilerServiceLimits::default()
            }),
            CompilerServiceIpcLimits {
                max_modules_per_set: 1,
                max_work_set_page_entries: 1,
                ..CompilerServiceIpcLimits::default()
            },
        )
        .unwrap();
        let project = adapter
            .register_core_project(registration("export const value: number = answer;"))
            .unwrap();
        let first_stream = "a".repeat(CompilerSessionAttestation::ID_LENGTH);
        let second_stream = "b".repeat(CompilerSessionAttestation::ID_LENGTH);
        inventory_on_stream(&mut adapter, &first_stream, project);
        inventory_on_stream(&mut adapter, &second_stream, project);
        let CompilerReply::Check(check) =
            adapter.handle_session_request(&first_stream, CompilerRequest::Check { project })
        else {
            panic!("fixture must check under the first stream")
        };
        assert!(check.parsed_modules.truncated);
        let CompilerReply::WorkSetPage(first) = adapter.handle_session_request(
            &first_stream,
            CompilerRequest::ListWorkSet {
                generation: check.generation,
                kind: CompilerWorkSetKind::Parsed,
                cursor: None,
                limit: Some(1),
            },
        ) else {
            panic!("first work-set page must be available")
        };
        assert_eq!(first.entries.len(), 1);
        let cursor = first.next_cursor.expect("two modules need continuation");
        let continuation = CompilerRequest::ListWorkSet {
            generation: check.generation,
            kind: CompilerWorkSetKind::Parsed,
            cursor: Some(cursor),
            limit: Some(1),
        };
        for rejected in [
            adapter.handle_session_request(&second_stream, continuation.clone()),
            adapter.handle_session_request(
                &first_stream,
                CompilerRequest::ListWorkSet {
                    generation: check.generation,
                    kind: CompilerWorkSetKind::Rechecked,
                    cursor: Some(cursor),
                    limit: Some(1),
                },
            ),
        ] {
            assert!(matches!(
                rejected,
                CompilerReply::Error {
                    code: CompilerErrorCode::InvalidWorkSetCursor,
                    ..
                }
            ));
        }
        assert!(matches!(
            adapter.handle_session_request(
                &first_stream,
                CompilerRequest::ListWorkSet {
                    generation: check.generation,
                    kind: CompilerWorkSetKind::Parsed,
                    cursor: Some(cursor),
                    limit: Some(0),
                },
            ),
            CompilerReply::Error {
                code: CompilerErrorCode::InvalidWorkSetPage,
                ..
            }
        ));
        let CompilerReply::WorkSetPage(second) =
            adapter.handle_session_request(&first_stream, continuation.clone())
        else {
            panic!("owning stream must consume its cursor once")
        };
        assert!(second.next_cursor.is_none());
        assert_ne!(first.entries, second.entries);
        assert!(matches!(
            adapter.handle_session_request(&first_stream, continuation),
            CompilerReply::Error {
                code: CompilerErrorCode::InvalidWorkSetCursor,
                ..
            }
        ));
        let CompilerReply::Check(later) =
            adapter.handle_session_request(&first_stream, CompilerRequest::Check { project })
        else {
            panic!("successor generation must check")
        };
        assert_ne!(later.generation, check.generation);
        assert!(matches!(
            adapter.handle_session_request(
                &first_stream,
                CompilerRequest::ListWorkSet {
                    generation: check.generation,
                    kind: CompilerWorkSetKind::Parsed,
                    cursor: None,
                    limit: Some(1),
                }
            ),
            CompilerReply::Error {
                code: CompilerErrorCode::StaleGeneration,
                ..
            }
        ));
    }

    #[test]
    fn abandoned_work_set_cursor_slots_are_released_on_disconnect() {
        let mut adapter = CompilerServiceIpcAdapter::new(
            RegisteredProjectCompilerService::new(CompilerServiceLimits {
                max_work_set_cursors: 1,
                ..CompilerServiceLimits::default()
            }),
            CompilerServiceIpcLimits {
                max_work_set_page_entries: 1,
                ..CompilerServiceIpcLimits::default()
            },
        )
        .unwrap();
        let project = adapter
            .register_core_project(registration("export const value: number = answer;"))
            .unwrap();
        let first_stream = "a".repeat(CompilerSessionAttestation::ID_LENGTH);
        let second_stream = "b".repeat(CompilerSessionAttestation::ID_LENGTH);
        inventory_on_stream(&mut adapter, &first_stream, project);
        inventory_on_stream(&mut adapter, &second_stream, project);
        let CompilerReply::Check(check) =
            adapter.handle_session_request(&first_stream, CompilerRequest::Check { project })
        else {
            panic!("registered fixture must check")
        };
        let first_request = CompilerRequest::ListWorkSet {
            generation: check.generation,
            kind: CompilerWorkSetKind::Parsed,
            cursor: None,
            limit: Some(1),
        };
        let CompilerReply::WorkSetPage(first) =
            adapter.handle_session_request(&first_stream, first_request.clone())
        else {
            panic!("the first stream must own the sole cursor slot")
        };
        let cursor = first.next_cursor.unwrap();
        assert!(matches!(
            adapter.handle_session_request(&second_stream, first_request.clone()),
            CompilerReply::Error {
                code: CompilerErrorCode::ResourceLimit,
                ..
            }
        ));
        adapter.end_session(&first_stream);
        let CompilerReply::WorkSetPage(second) =
            adapter.handle_session_request(&second_stream, first_request)
        else {
            panic!("disconnect must release the sole work-set cursor slot")
        };
        assert_ne!(second.next_cursor, Some(cursor));
    }

    #[test]
    fn rejected_diagnostic_wire_page_does_not_leak_a_core_cursor_slot() {
        let mut adapter = CompilerServiceIpcAdapter::new(
            RegisteredProjectCompilerService::new(CompilerServiceLimits {
                max_diagnostic_cursors: 1,
                ..CompilerServiceLimits::default()
            }),
            CompilerServiceIpcLimits {
                max_field_bytes: 4,
                max_diagnostic_page_entries: 1,
                ..CompilerServiceIpcLimits::default()
            },
        )
        .unwrap();
        let project = adapter
            .register_core_project(registration(
                "const first: number = 'one'; \
                 const second: number = 'two'; \
                 const third: number = 'three';",
            ))
            .unwrap();
        assert_eq!(
            adapter.handle(CompilerRequest::Check { project }),
            response_limit_reply(),
            "the tiny wire-field budget must not disclose a check generation"
        );
        // An untrusted raw IPC peer may guess a generation number even after
        // the check reply was rejected. Both attempts must fail at the wire
        // budget, not because the first undisclosed page exhausted the sole
        // service cursor slot.
        let generation = CompilerGeneration {
            project,
            sequence: 1,
        };
        for _ in 0..2 {
            assert_eq!(
                adapter.handle(CompilerRequest::ListDiagnostics {
                    generation,
                    cursor: None,
                    limit: Some(1),
                }),
                response_limit_reply(),
            );
        }
    }

    #[test]
    fn queued_requests_are_applied_only_by_the_adapter_owner() {
        let (mut adapter, project) = adapter();
        inventory_on_stream(
            &mut adapter,
            &"a".repeat(CompilerSessionAttestation::ID_LENGTH),
            project,
        );
        let (sender, receiver) = compiler_service_ipc_request_channel();
        assert!(sender
            .bind_session(CompilerSessionAttestation {
                id: "caller-chosen-short-token".to_string(),
            })
            .is_err());
        let bound = sender
            .bind_session(CompilerSessionAttestation {
                id: "a".repeat(CompilerSessionAttestation::ID_LENGTH),
            })
            .unwrap();
        let worker = std::thread::spawn(move || bound.request(CompilerRequest::Check { project }));
        while receiver.dispatch_pending(&mut adapter) == 0 {
            std::thread::yield_now();
        }
        assert!(matches!(
            worker.join().unwrap().unwrap(),
            CompilerReply::Check(_)
        ));
    }

    #[test]
    fn startup_catalog_seals_registration_before_the_session_receives_queries() {
        let mut catalog = CoreCompilerProjectCatalog::default();
        let project = catalog
            .register_startup_project(registration("export const value: number = answer;"))
            .unwrap();
        assert_eq!(catalog.registered_project_count(), 1);

        // `seal` consumes the only object that exposes registration. The
        // resulting service exposes only this bounded query dispatch API.
        let mut session = catalog.seal();
        assert_eq!(session.registered_project_count(), 1);
        inventory_on_stream(
            &mut session.adapter,
            &"b".repeat(CompilerSessionAttestation::ID_LENGTH),
            project,
        );
        let (sender, receiver) = compiler_service_ipc_request_channel();
        let bound = sender
            .bind_session(CompilerSessionAttestation {
                id: "b".repeat(CompilerSessionAttestation::ID_LENGTH),
            })
            .unwrap();
        let worker =
            std::thread::spawn(move || bound.request(CompilerRequest::DescribeProject { project }));
        while session.dispatch_pending(&receiver) == 0 {
            std::thread::yield_now();
        }
        assert!(matches!(
            worker.join().unwrap().unwrap(),
            CompilerReply::Project(CompilerProjectIdentity { project: returned, .. })
                if returned == project
        ));
    }

    #[test]
    fn private_startup_project_is_never_inventoried_or_queryable() {
        let mut catalog = CoreCompilerProjectCatalog::default();
        let visible = catalog
            .register_startup_project(registration("export const shown: number = answer;"))
            .unwrap();
        let mut private_registration = registration("export const hidden: number = answer;");
        private_registration.canonical_project_root = "project:///private".to_string();
        private_registration.canonical_config_root = "project:///private/blue-ts.json".to_string();
        private_registration.canonical_output_root = "project:///private-dist".to_string();
        let private = catalog
            .register_startup_project_private(private_registration)
            .unwrap();
        assert_eq!(catalog.registered_project_count(), 2);
        let mut session = catalog.seal();
        let stream = "c".repeat(CompilerSessionAttestation::ID_LENGTH);
        let CompilerReply::Projects(inventory) = session
            .adapter
            .handle_session_request(&stream, CompilerRequest::ListProjects)
        else {
            panic!("accepted stream must receive a project inventory")
        };
        assert_eq!(inventory.projects, vec![visible]);
        for request in [
            CompilerRequest::DescribeProject { project: private },
            CompilerRequest::Check { project: private },
        ] {
            assert!(matches!(
                session.adapter.handle_session_request(&stream, request),
                CompilerReply::Error {
                    code: CompilerErrorCode::UnobservedProject,
                    ..
                }
            ));
        }
    }

    #[test]
    fn pre_registered_service_projects_remain_private_until_explicitly_exposed() {
        let mut service = RegisteredProjectCompilerService::default();
        let private = project_to_wire(
            service
                .register(registration("export const hidden: number = answer;"))
                .unwrap(),
        );
        let mut adapter =
            CompilerServiceIpcAdapter::new(service, CompilerServiceIpcLimits::default()).unwrap();
        let stream = "d".repeat(CompilerSessionAttestation::ID_LENGTH);
        for reply in [
            adapter.handle(CompilerRequest::ListProjects),
            adapter.handle_session_request(&stream, CompilerRequest::ListProjects),
        ] {
            let CompilerReply::Projects(inventory) = reply else {
                panic!("a private-only service must return an empty inventory")
            };
            assert!(inventory.projects.is_empty());
        }
        for request in [
            CompilerRequest::DescribeProject { project: private },
            CompilerRequest::Check { project: private },
        ] {
            assert!(matches!(
                adapter.handle(request.clone()),
                CompilerReply::Error {
                    code: CompilerErrorCode::UnobservedProject,
                    ..
                }
            ));
            assert!(matches!(
                adapter.handle_session_request(&stream, request),
                CompilerReply::Error {
                    code: CompilerErrorCode::UnobservedProject,
                    ..
                }
            ));
        }

        let mut exposed_registration = registration("export const shown: number = answer;");
        exposed_registration.canonical_project_root = "project:///shown".to_string();
        exposed_registration.canonical_config_root = "project:///shown/blue-ts.json".to_string();
        exposed_registration.canonical_output_root = "project:///shown-dist".to_string();
        let exposed = adapter.register_core_project(exposed_registration).unwrap();
        let CompilerReply::Projects(inventory) =
            adapter.handle_session_request(&stream, CompilerRequest::ListProjects)
        else {
            panic!("accepted stream must receive an inventory")
        };
        assert_eq!(inventory.projects, vec![exposed]);
        assert!(matches!(
            adapter.handle_session_request(&stream, CompilerRequest::Check { project: private }),
            CompilerReply::Error {
                code: CompilerErrorCode::UnobservedProject,
                ..
            }
        ));
    }

    #[test]
    fn pre_registered_catalog_projects_are_counted_but_never_exposed() {
        let mut service = RegisteredProjectCompilerService::default();
        service
            .register(registration("export const hidden: number = answer;"))
            .unwrap();
        let catalog =
            CoreCompilerProjectCatalog::new(service, CompilerServiceIpcLimits::default()).unwrap();
        assert_eq!(catalog.registered_project_count(), 1);
        let mut session = catalog.seal();
        assert_eq!(session.registered_project_count(), 1);
        let stream = "e".repeat(CompilerSessionAttestation::ID_LENGTH);
        let CompilerReply::Projects(inventory) = session
            .adapter
            .handle_session_request(&stream, CompilerRequest::ListProjects)
        else {
            panic!("private-only catalog must still return a valid inventory")
        };
        assert!(inventory.projects.is_empty());
    }

    #[test]
    fn invalid_ipc_limits_fail_before_an_adapter_is_available() {
        assert_eq!(
            CompilerServiceIpcAdapter::new(
                RegisteredProjectCompilerService::default(),
                CompilerServiceIpcLimits {
                    max_stream_cursor_receipts: 0,
                    ..CompilerServiceIpcLimits::default()
                },
            )
            .unwrap_err(),
            CompilerServiceIpcConfigurationError::ZeroStreamCursorReceipts
        );
        assert_eq!(
            CompilerServiceIpcAdapter::new(
                RegisteredProjectCompilerService::default(),
                CompilerServiceIpcLimits {
                    max_response_bytes: 0,
                    ..CompilerServiceIpcLimits::default()
                },
            )
            .unwrap_err(),
            CompilerServiceIpcConfigurationError::ZeroResponseBytes
        );
        assert_eq!(
            CompilerServiceIpcAdapter::new(
                RegisteredProjectCompilerService::default(),
                CompilerServiceIpcLimits {
                    max_response_bytes: blueice_ipc::compiler::MAX_COMPILER_MESSAGE_BYTES + 1,
                    ..CompilerServiceIpcLimits::default()
                },
            )
            .unwrap_err(),
            CompilerServiceIpcConfigurationError::ResponseExceedsTransportLimit
        );
        assert_eq!(
            CompilerServiceIpcAdapter::new(
                RegisteredProjectCompilerService::default(),
                CompilerServiceIpcLimits {
                    max_diagnostic_page_entries: 0,
                    ..CompilerServiceIpcLimits::default()
                },
            )
            .unwrap_err(),
            CompilerServiceIpcConfigurationError::ZeroDiagnosticPageEntries
        );
        assert_eq!(
            CompilerServiceIpcAdapter::new(
                RegisteredProjectCompilerService::default(),
                CompilerServiceIpcLimits {
                    max_work_set_page_entries: 0,
                    ..CompilerServiceIpcLimits::default()
                },
            )
            .unwrap_err(),
            CompilerServiceIpcConfigurationError::ZeroWorkSetPageEntries
        );
        assert_eq!(
            CompilerServiceIpcAdapter::new(
                RegisteredProjectCompilerService::default(),
                CompilerServiceIpcLimits {
                    max_static_metadata_page_entries: 0,
                    ..CompilerServiceIpcLimits::default()
                },
            )
            .unwrap_err(),
            CompilerServiceIpcConfigurationError::ZeroStaticMetadataPageEntries
        );
    }
}
