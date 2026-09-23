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
    RegisteredProjectRegistration, StaticMetadataInventoryKind,
};
use blueice_bluets::{
    ContractId, ContractValue, Diagnostic, Severity, SourceId, SymbolKind, ValidationError,
};
use blueice_ipc::compiler::{
    CompilerCheck, CompilerContractValidation, CompilerContractValidationFailure,
    CompilerContractValue, CompilerDiagnostic, CompilerDiagnosticCursor, CompilerDiagnosticPage,
    CompilerDiagnosticSeverity, CompilerDiagnostics, CompilerErrorCode, CompilerGeneration,
    CompilerModuleList, CompilerProject, CompilerProjectIdentity, CompilerReply, CompilerRequest,
    CompilerStaticContract, CompilerStaticMetadataCursor, CompilerStaticMetadataKind,
    CompilerStaticMetadataPage, CompilerStaticMetadataSummary, CompilerStaticProvenance,
    CompilerStaticSymbol, CompilerStaticType, CompilerSymbolKind,
};
use std::collections::BTreeSet;
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
    /// Maximum opaque IDs returned in one static-metadata inventory page. A
    /// request may ask for fewer entries but cannot raise this core-selected
    /// cap or use a cursor as an offset.
    pub max_static_metadata_page_entries: usize,
}

impl Default for CompilerServiceIpcLimits {
    fn default() -> Self {
        Self {
            max_response_bytes: 256 * 1_024,
            max_field_bytes: 16 * 1_024,
            max_modules_per_set: 1_024,
            max_diagnostics: 256,
            max_diagnostic_page_entries: 128,
            max_static_metadata_page_entries: 128,
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
    ZeroStaticMetadataPageEntries,
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
            Self::ZeroStaticMetadataPageEntries => {
                formatter.write_str("compiler IPC static metadata page cap must be nonzero")
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
        Ok(Self { service, limits })
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
        self.service.register(registration).map(project_to_wire)
    }

    /// Handles one request after the transport has successfully negotiated
    /// `Hello`. A transport owner is responsible for first-message handling;
    /// an in-band `Hello` is rejected rather than renegotiating state.
    pub fn handle(&mut self, request: CompilerRequest) -> CompilerReply {
        match request {
            CompilerRequest::DescribeProject { project } => self.describe_project(project),
            CompilerRequest::Check { project } => self.check(project),
            CompilerRequest::ListDiagnostics {
                generation,
                cursor,
                limit,
            } => self.diagnostic_page(generation, cursor, limit),
            CompilerRequest::GetStaticType {
                generation,
                type_id,
            } => self.static_type(generation, type_id),
            CompilerRequest::GetStaticSymbol {
                generation,
                symbol_id,
            } => self.static_symbol(generation, symbol_id),
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
        if !retained_diagnostics_fit_wire_policy(&check.retained_diagnostics, self.limits) {
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
        const PAGE_ENTRY_FIXED_BYTES: usize = 192;
        let max_entry_bytes = self
            .limits
            .max_field_bytes
            .checked_mul(12)
            .and_then(|bytes| {
                MAX_COMPILER_DIAGNOSTIC_CODE_BYTES
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
            return response_limit_reply();
        }
        let entries = match diagnostic_page_entries_to_wire(&page.entries, self.limits, &mut budget)
        {
            Ok(entries) => entries,
            Err(()) => return response_limit_reply(),
        };
        CompilerReply::DiagnosticPage(CompilerDiagnosticPage {
            generation: generation_to_wire(generation),
            entries,
            next_cursor: page.next_cursor.map(|id| CompilerDiagnosticCursor { id }),
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
            module: symbol.span.module,
            start,
            end,
            static_type_id: symbol.static_type.map(|id| id.0),
            source_id: symbol.source.0,
            contract_id: symbol.contract.map(|id| id.0),
        })
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
    /// Creates an empty catalog while the trusted core startup owner still
    /// chooses its fixed project inputs. This API is never called by a wire
    /// request or an MCP tool.
    pub fn new(
        service: RegisteredProjectCompilerService,
        limits: CompilerServiceIpcLimits,
    ) -> Result<Self, CompilerServiceIpcConfigurationError> {
        Ok(Self {
            adapter: CompilerServiceIpcAdapter::new(service, limits)?,
            registered_projects: Vec::new(),
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
    /// Project identities themselves remain available only through a caller's
    /// pre-existing opaque handle and `DescribeProject` query.
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
    if limits.max_static_metadata_page_entries == 0 {
        return Err(CompilerServiceIpcConfigurationError::ZeroStaticMetadataPageEntries);
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
        CompilerServiceError::InvalidStaticMetadataPage { .. } => (
            CompilerErrorCode::InvalidMetadataPage,
            "invalid static metadata page request",
        ),
        CompilerServiceError::InvalidDiagnosticPage { .. } => (
            CompilerErrorCode::InvalidDiagnosticPage,
            "invalid compiler diagnostic page request",
        ),
        CompilerServiceError::ProjectLimit { .. }
        | CompilerServiceError::GenerationExhausted { .. }
        | CompilerServiceError::StaticMetadataLimit { .. }
        | CompilerServiceError::StaticMetadataCursorLimit { .. }
        | CompilerServiceError::DiagnosticCursorLimit { .. }
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

/// Compiler diagnostic codes are a fixed compiler vocabulary, not
/// project-controlled prose. Keep their wire budget separately small so the
/// diagnostic-page envelope can be proved before a one-shot cursor is used.
const MAX_COMPILER_DIAGNOSTIC_CODE_BYTES: usize = 64;

fn retained_diagnostics_fit_wire_policy(
    diagnostics: &[Diagnostic],
    limits: CompilerServiceIpcLimits,
) -> bool {
    diagnostics.iter().all(|diagnostic| {
        diagnostic.code.to_string().len() <= MAX_COMPILER_DIAGNOSTIC_CODE_BYTES
            && diagnostic.span.module.len() <= limits.max_field_bytes
            && diagnostic.message.len() <= limits.max_field_bytes
            && u64::try_from(diagnostic.span.start).is_ok()
            && u64::try_from(diagnostic.span.end).is_ok()
    })
}

fn diagnostics_to_wire(
    diagnostics: &[Diagnostic],
    service_truncated: bool,
    limits: CompilerServiceIpcLimits,
    budget: &mut ResponseBudget,
) -> Result<CompilerDiagnostics, ()> {
    let mut entries = Vec::new();
    let mut truncated = service_truncated;
    for diagnostic in diagnostics {
        if entries.len() >= limits.max_diagnostics {
            truncated = true;
            break;
        }
        let code = diagnostic.code.to_string();
        if code.len() > MAX_COMPILER_DIAGNOSTIC_CODE_BYTES
            || !budget.reserve_optional_string(&code, MAX_COMPILER_DIAGNOSTIC_CODE_BYTES)?
            || !budget.reserve_optional_string(&diagnostic.span.module, limits.max_field_bytes)?
            || !budget.reserve_optional_string(&diagnostic.message, limits.max_field_bytes)?
            || !budget.reserve_optional_fixed(160)
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
    limits: CompilerServiceIpcLimits,
    budget: &mut ResponseBudget,
) -> Result<Vec<CompilerDiagnostic>, ()> {
    diagnostics
        .iter()
        .map(|diagnostic| {
            let code = diagnostic.code.to_string();
            if code.len() > MAX_COMPILER_DIAGNOSTIC_CODE_BYTES
                || !budget.reserve_required_string(&code, MAX_COMPILER_DIAGNOSTIC_CODE_BYTES)
                || !budget.reserve_required_string(&diagnostic.span.module, limits.max_field_bytes)
                || !budget.reserve_required_string(&diagnostic.message, limits.max_field_bytes)
                || !budget.reserve_optional_fixed(192)
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

/// Session-owner receiver for the compiler adapter. Only this side obtains a
/// mutable adapter and therefore the mutable incremental compiler cache.
pub struct CompilerServiceIpcRequestReceiver(mpsc::Receiver<CompilerServiceIpcRequestEnvelope>);

struct CompilerServiceIpcRequestEnvelope {
    request: CompilerRequest,
    reply: mpsc::SyncSender<CompilerReply>,
}

/// Builds the worker-to-owner hand-off used by a future compiler socket.
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
    /// Routes one already-negotiated request to the compiler owner. A stopped
    /// core session is a transport failure, not an invented compiler reply.
    pub fn request(&self, request: CompilerRequest) -> io::Result<CompilerReply> {
        let (reply_sender, reply_receiver) = mpsc::sync_channel(1);
        self.0
            .send(CompilerServiceIpcRequestEnvelope {
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
            let reply = adapter.handle(envelope.request);
            let _ = envelope.reply.send(reply);
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
        assert_eq!(symbol.module, ENTRY);
        assert_ne!(symbol.module, "import { answer } from './math';");
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
                "const first: number = 'one'; \
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
    fn queued_requests_are_applied_only_by_the_adapter_owner() {
        let (mut adapter, project) = adapter();
        let (sender, receiver) = compiler_service_ipc_request_channel();
        let worker = std::thread::spawn(move || sender.request(CompilerRequest::Check { project }));
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
        let (sender, receiver) = compiler_service_ipc_request_channel();
        let worker = std::thread::spawn(move || {
            sender.request(CompilerRequest::DescribeProject { project })
        });
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
    fn invalid_ipc_limits_fail_before_an_adapter_is_available() {
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
                    max_static_metadata_page_entries: 0,
                    ..CompilerServiceIpcLimits::default()
                },
            )
            .unwrap_err(),
            CompilerServiceIpcConfigurationError::ZeroStaticMetadataPageEntries
        );
    }
}
