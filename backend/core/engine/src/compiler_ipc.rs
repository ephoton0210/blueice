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
//! This module intentionally stops short of a process listener and MCP tool.
//! Those layers need their own launcher/session capability negotiation. The
//! request channel follows the debugger's session-owner hand-off pattern so a
//! future listener can never borrow compiler state on its worker thread.

use crate::compiler_service::{
    CompilerServiceCheck, CompilerServiceError, CompilerServiceLimits,
    RegisteredProjectCompilerService, RegisteredProjectGeneration, RegisteredProjectId,
    RegisteredProjectRegistration,
};
use blueice_bluets::{Diagnostic, Severity, SymbolKind};
use blueice_ipc::compiler::{
    CompilerCheck, CompilerDiagnostic, CompilerDiagnosticSeverity, CompilerDiagnostics,
    CompilerErrorCode, CompilerGeneration, CompilerModuleList, CompilerProject,
    CompilerProjectIdentity, CompilerReply, CompilerRequest, CompilerStaticMetadataSummary,
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
}

impl Default for CompilerServiceIpcLimits {
    fn default() -> Self {
        Self {
            max_response_bytes: 256 * 1_024,
            max_field_bytes: 16 * 1_024,
            max_modules_per_set: 1_024,
            max_diagnostics: 256,
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
            CompilerRequest::GetStaticType {
                generation,
                type_id,
            } => self.static_type(generation, type_id),
            CompilerRequest::GetStaticSymbol {
                generation,
                symbol_id,
            } => self.static_symbol(generation, symbol_id),
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
        match self.check_to_wire(check) {
            Ok(check) => CompilerReply::Check(check),
            Err(()) => response_limit_reply(),
        }
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
        CompilerServiceError::ProjectLimit { .. }
        | CompilerServiceError::GenerationExhausted { .. }
        | CompilerServiceError::StaticMetadataLimit { .. }
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
        if !budget.reserve_optional_string(&diagnostic.span.module, limits.max_field_bytes)?
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
            code: diagnostic.code.to_string(),
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

fn symbol_kind_to_wire(kind: SymbolKind) -> CompilerSymbolKind {
    match kind {
        SymbolKind::Import => CompilerSymbolKind::Import,
        SymbolKind::TypeAlias => CompilerSymbolKind::TypeAlias,
        SymbolKind::Interface => CompilerSymbolKind::Interface,
        SymbolKind::Variable => CompilerSymbolKind::Variable,
        SymbolKind::Function => CompilerSymbolKind::Function,
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
    }
}
