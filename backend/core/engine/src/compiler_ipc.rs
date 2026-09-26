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

mod adapter;

#[cfg(test)]
mod tests;
