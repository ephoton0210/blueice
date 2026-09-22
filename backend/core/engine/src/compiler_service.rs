// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Core-owned, registered-project BlueTS compilation.
//!
//! This service deliberately receives a complete, caller-authorized module
//! graph at registration time. Subsequent check, build, and static-metadata
//! requests carry only opaque project and generation handles; they cannot add
//! paths, resolver rules, plugins, or compiler options. `build` returns
//! bounded in-memory artifacts and performs no filesystem write. A future
//! output transaction and MCP adapter must remain separate, capability-checked
//! layers above this service.

use blueice_bluets::{
    AuthorizedModuleLoader, BlueTsDebugInfo, BuildOutput, CompilerOptions, ContractId,
    ContractValue, DebugContract, DebugSource, DebugSymbol, DebugType, Diagnostic,
    IncrementalCompiler, ModuleLoader, SourceId, SymbolId, TypeId, ValidationError,
    ValidationLimits,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// Limits for core-owned compiler-service state and responses.
///
/// Source parsing, checking, and emission remain bounded by the pinned
/// [`CompilerOptions`] for each registered project. These limits bound service
/// retention and what a debugger/MCP adapter can receive in one response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompilerServiceLimits {
    pub max_projects: usize,
    pub max_diagnostics: usize,
    pub max_static_sources: usize,
    pub max_static_types: usize,
    pub max_static_symbols: usize,
    pub max_static_contracts: usize,
    /// Fixed core-selected limits for validation requests. Query callers never
    /// provide or relax these bounds.
    pub contract_validation: ValidationLimits,
    pub max_build_artifacts: usize,
    pub max_build_artifact_bytes: usize,
}

impl Default for CompilerServiceLimits {
    fn default() -> Self {
        Self {
            max_projects: 128,
            max_diagnostics: 256,
            max_static_sources: 4_096,
            max_static_types: 16_384,
            max_static_symbols: 65_536,
            max_static_contracts: 16_384,
            contract_validation: ValidationLimits {
                max_depth: 64,
                max_collection_entries: 4_096,
                max_nodes: 32_768,
                max_string_bytes: 256 * 1_024,
            },
            max_build_artifacts: 4_096,
            max_build_artifact_bytes: 16 * 1_024 * 1_024,
        }
    }
}

/// A core-minted identity for one registered, immutable compilation project.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RegisteredProjectId(u64);

impl RegisteredProjectId {
    pub fn as_u64(self) -> u64 {
        self.0
    }

    /// Reconstitutes an opaque, core-minted identifier inside this crate for
    /// an IPC adapter. This is deliberately crate-private: external callers
    /// receive the ID only as an untrusted wire value and cannot use it to
    /// register or alter a project.
    pub(crate) fn from_wire(value: u64) -> Option<Self> {
        (value != 0).then_some(Self(value))
    }
}

/// A core-minted generation for one check or build result.
///
/// Metadata queries require this exact token, preventing a client from using
/// stale symbols or types after a later request observed the project.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RegisteredProjectGeneration {
    project_id: RegisteredProjectId,
    sequence: u64,
}

impl RegisteredProjectGeneration {
    pub fn project_id(self) -> RegisteredProjectId {
        self.project_id
    }

    pub fn sequence(self) -> u64 {
        self.sequence
    }

    /// Reconstitutes a generation only for an engine-internal adapter after
    /// both opaque wire components have passed their well-formedness checks.
    pub(crate) fn from_wire(project_id: RegisteredProjectId, sequence: u64) -> Option<Self> {
        (sequence != 0).then_some(Self {
            project_id,
            sequence,
        })
    }
}

/// Core-owned, immutable inputs for a compilation project.
///
/// The three root fields are canonical identities selected by the core, not
/// paths that this service resolves or writes. Their role is to bind an
/// eventual output transaction and remote capability grant to this exact
/// registration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisteredProjectRegistration {
    pub canonical_project_root: String,
    pub canonical_config_root: String,
    pub canonical_output_root: String,
    pub entry_module: String,
    pub loader: AuthorizedModuleLoader,
    pub compiler_options: CompilerOptions,
}

/// The non-source identity a caller may inspect after registration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisteredProjectIdentity {
    pub id: RegisteredProjectId,
    pub canonical_project_root: String,
    pub canonical_config_root: String,
    pub canonical_output_root: String,
    pub entry_module: String,
}

/// A bounded diagnostic page. Truncation is explicit so no adapter mistakes a
/// partial report for a clean check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompilerServiceDiagnostics {
    pub entries: Vec<Diagnostic>,
    pub truncated: bool,
}

/// A check result with only static, source-text-free metadata.
#[derive(Debug, Clone)]
pub struct CompilerServiceCheck {
    pub generation: RegisteredProjectGeneration,
    pub cache_hit: bool,
    pub parsed_modules: BTreeSet<String>,
    pub reused_parsed_modules: BTreeSet<String>,
    pub rechecked_modules: BTreeSet<String>,
    pub reused_checked_modules: BTreeSet<String>,
    pub diagnostics: CompilerServiceDiagnostics,
    pub has_errors: bool,
    /// The compiler's graph/options fingerprint when artifact creation was
    /// successful. It is absent on a no-emit-on-error result.
    pub artifact_fingerprint: Option<String>,
    /// VM-independent types, symbols, source hashes, and compiler-options
    /// identity for this generation. It contains no source text or values.
    pub static_debug_info: Option<BlueTsDebugInfo>,
}

/// A build result. Its artifacts are in memory only; a higher layer must
/// obtain an explicit output-write capability before committing them anywhere.
#[derive(Debug, Clone)]
pub struct CompilerServiceBuild {
    pub check: CompilerServiceCheck,
    pub output: Option<BuildOutput>,
}

/// Fail-closed compiler-service errors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompilerServiceError {
    InvalidRegistration {
        field: &'static str,
    },
    EntryModuleNotAuthorized {
        entry_module: String,
    },
    DuplicateRegistration,
    ProjectLimit {
        limit: usize,
    },
    ProjectIdExhausted,
    GenerationExhausted {
        project_id: RegisteredProjectId,
    },
    UnknownProject {
        project_id: RegisteredProjectId,
    },
    StaleGeneration {
        generation: RegisteredProjectGeneration,
    },
    NoStaticMetadata {
        generation: RegisteredProjectGeneration,
    },
    UnknownType {
        generation: RegisteredProjectGeneration,
        type_id: TypeId,
    },
    UnknownSymbol {
        generation: RegisteredProjectGeneration,
        symbol_id: SymbolId,
    },
    UnknownSource {
        generation: RegisteredProjectGeneration,
        source_id: SourceId,
    },
    UnknownContract {
        generation: RegisteredProjectGeneration,
        contract_id: ContractId,
    },
    StaticMetadataLimit {
        resource: &'static str,
        limit: usize,
    },
    BuildOutputLimit {
        resource: &'static str,
        limit: usize,
    },
}

impl fmt::Display for CompilerServiceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRegistration { field } => {
                write!(formatter, "registered project field `{field}` is invalid")
            }
            Self::EntryModuleNotAuthorized { entry_module } => {
                write!(
                    formatter,
                    "entry module `{entry_module}` is not in the authorized graph"
                )
            }
            Self::DuplicateRegistration => formatter
                .write_str("the canonical project/config/output registration already exists"),
            Self::ProjectLimit { limit } => {
                write!(
                    formatter,
                    "registered-project limit {limit} has been reached"
                )
            }
            Self::ProjectIdExhausted => {
                formatter.write_str("registered-project identifiers exhausted")
            }
            Self::GenerationExhausted { project_id } => write!(
                formatter,
                "compiler-service generations exhausted for project {}",
                project_id.as_u64()
            ),
            Self::UnknownProject { project_id } => {
                write!(
                    formatter,
                    "unknown registered project {}",
                    project_id.as_u64()
                )
            }
            Self::StaleGeneration { generation } => write!(
                formatter,
                "stale compiler-service generation {} for project {}",
                generation.sequence(),
                generation.project_id().as_u64()
            ),
            Self::NoStaticMetadata { generation } => write!(
                formatter,
                "generation {} for project {} has no successful static metadata",
                generation.sequence(),
                generation.project_id().as_u64()
            ),
            Self::UnknownType {
                generation,
                type_id,
            } => write!(
                formatter,
                "unknown static type {} in generation {} for project {}",
                type_id.0,
                generation.sequence(),
                generation.project_id().as_u64()
            ),
            Self::UnknownSymbol {
                generation,
                symbol_id,
            } => write!(
                formatter,
                "unknown static symbol {} in generation {} for project {}",
                symbol_id.0,
                generation.sequence(),
                generation.project_id().as_u64()
            ),
            Self::UnknownSource {
                generation,
                source_id,
            } => write!(
                formatter,
                "unknown static source {} in generation {} for project {}",
                source_id.0,
                generation.sequence(),
                generation.project_id().as_u64()
            ),
            Self::UnknownContract {
                generation,
                contract_id,
            } => write!(
                formatter,
                "unknown static contract {} in generation {} for project {}",
                contract_id.0,
                generation.sequence(),
                generation.project_id().as_u64()
            ),
            Self::StaticMetadataLimit { resource, limit } => {
                write!(
                    formatter,
                    "static metadata `{resource}` exceeds service limit {limit}"
                )
            }
            Self::BuildOutputLimit { resource, limit } => {
                write!(
                    formatter,
                    "build output `{resource}` exceeds service limit {limit}"
                )
            }
        }
    }
}

impl std::error::Error for CompilerServiceError {}

/// The native, core-owned compiler authority for closed registered projects.
#[derive(Debug, Default)]
pub struct RegisteredProjectCompilerService {
    limits: CompilerServiceLimits,
    next_project_id: u64,
    projects: BTreeMap<RegisteredProjectId, RegisteredProject>,
}

#[derive(Debug)]
struct RegisteredProject {
    registration: RegisteredProjectRegistration,
    compiler: IncrementalCompiler,
    next_generation: u64,
    latest: Option<RetainedCompilation>,
}

#[derive(Debug, Clone)]
struct RetainedCompilation {
    generation: RegisteredProjectGeneration,
    static_debug_info: Option<BlueTsDebugInfo>,
}

impl RegisteredProjectCompilerService {
    pub fn new(limits: CompilerServiceLimits) -> Self {
        Self {
            limits,
            next_project_id: 0,
            projects: BTreeMap::new(),
        }
    }

    /// Returns the immutable validation budget selected by the core owner.
    /// IPC adapters use it only to reject an over-budget data shape before
    /// allocating a second internal representation; clients cannot mutate it.
    pub fn contract_validation_limits(&self) -> ValidationLimits {
        self.limits.contract_validation
    }

    /// Registers a complete project exactly once. No subsequent operation can
    /// alter its source graph, roots, resolver, or compiler options.
    pub fn register(
        &mut self,
        registration: RegisteredProjectRegistration,
    ) -> Result<RegisteredProjectId, CompilerServiceError> {
        validate_registration(&registration)?;
        registration
            .loader
            .load(&registration.entry_module)
            .map_err(|_| CompilerServiceError::EntryModuleNotAuthorized {
                entry_module: registration.entry_module.clone(),
            })?;
        if self.projects.len() >= self.limits.max_projects {
            return Err(CompilerServiceError::ProjectLimit {
                limit: self.limits.max_projects,
            });
        }
        if self
            .projects
            .values()
            .any(|project| same_registration(&project.registration, &registration))
        {
            return Err(CompilerServiceError::DuplicateRegistration);
        }
        let id = RegisteredProjectId(
            self.next_project_id
                .checked_add(1)
                .ok_or(CompilerServiceError::ProjectIdExhausted)?,
        );
        self.next_project_id = id.0;
        self.projects.insert(
            id,
            RegisteredProject {
                registration,
                compiler: IncrementalCompiler::new(),
                next_generation: 0,
                latest: None,
            },
        );
        Ok(id)
    }

    /// Returns the immutable, source-text-free identity selected at
    /// registration time.
    pub fn identity(
        &self,
        project_id: RegisteredProjectId,
    ) -> Result<RegisteredProjectIdentity, CompilerServiceError> {
        let project = self.project(project_id)?;
        Ok(RegisteredProjectIdentity {
            id: project_id,
            canonical_project_root: project.registration.canonical_project_root.clone(),
            canonical_config_root: project.registration.canonical_config_root.clone(),
            canonical_output_root: project.registration.canonical_output_root.clone(),
            entry_module: project.registration.entry_module.clone(),
        })
    }

    /// Checks the registered project without exposing emitted artifacts or
    /// performing output writes.
    pub fn check(
        &mut self,
        project_id: RegisteredProjectId,
    ) -> Result<CompilerServiceCheck, CompilerServiceError> {
        let (check, _) = self.compile(project_id)?;
        Ok(check)
    }

    /// Builds the registered project into bounded in-memory artifacts. A
    /// diagnostic-bearing compile result returns `None` output, preserving
    /// BlueTSC's no-emit-on-error invariant.
    pub fn build(
        &mut self,
        project_id: RegisteredProjectId,
    ) -> Result<CompilerServiceBuild, CompilerServiceError> {
        let (check, output) = self.compile(project_id)?;
        if let Some(output) = output.as_ref() {
            validate_build_output(output, self.limits)?;
        }
        Ok(CompilerServiceBuild { check, output })
    }

    /// Returns the latest static metadata only when the caller presents the
    /// exact generation produced by `check` or `build`.
    pub fn static_debug_info(
        &self,
        generation: RegisteredProjectGeneration,
    ) -> Result<BlueTsDebugInfo, CompilerServiceError> {
        Ok(self.static_info(generation)?.clone())
    }

    /// Looks up one static type by its compiler-minted ID. This never attempts
    /// to inspect a BlueJS runtime value.
    pub fn static_type(
        &self,
        generation: RegisteredProjectGeneration,
        type_id: TypeId,
    ) -> Result<DebugType, CompilerServiceError> {
        self.static_info(generation)?
            .types
            .iter()
            .find(|static_type| static_type.id == type_id)
            .cloned()
            .ok_or(CompilerServiceError::UnknownType {
                generation,
                type_id,
            })
    }

    /// Looks up one static symbol by its compiler-minted ID. Its span remains
    /// a canonical source identity and byte range, not a source-text read.
    pub fn static_symbol(
        &self,
        generation: RegisteredProjectGeneration,
        symbol_id: SymbolId,
    ) -> Result<DebugSymbol, CompilerServiceError> {
        self.static_info(generation)?
            .symbols
            .iter()
            .find(|symbol| symbol.id == symbol_id)
            .cloned()
            .ok_or(CompilerServiceError::UnknownSymbol {
                generation,
                symbol_id,
            })
    }

    /// Returns one compiler-minted source identity and content hash. This is
    /// provenance metadata only: no method on this service reads the source
    /// represented by the returned handle.
    pub fn static_provenance(
        &self,
        generation: RegisteredProjectGeneration,
        source_id: SourceId,
    ) -> Result<DebugSource, CompilerServiceError> {
        self.static_info(generation)?
            .sources
            .iter()
            .find(|source| source.id == source_id)
            .cloned()
            .ok_or(CompilerServiceError::UnknownSource {
                generation,
                source_id,
            })
    }

    /// Returns an exact-generation pure contract plan retained by BlueTS.
    /// The plan remains static data; it has no relationship to a live BlueJS
    /// value or page realm.
    pub fn static_contract(
        &self,
        generation: RegisteredProjectGeneration,
        contract_id: ContractId,
    ) -> Result<DebugContract, CompilerServiceError> {
        self.static_info(generation)?
            .contracts
            .iter()
            .find(|contract| contract.id == contract_id)
            .cloned()
            .ok_or(CompilerServiceError::UnknownContract {
                generation,
                contract_id,
            })
    }

    /// Validates one caller-provided, data-only snapshot against an exact
    /// retained static plan. The core's immutable service limits apply; the
    /// input cannot inspect JavaScript objects, run callbacks, or select a
    /// different compiler generation.
    pub fn validate_static_contract(
        &self,
        generation: RegisteredProjectGeneration,
        contract_id: ContractId,
        value: &ContractValue,
    ) -> Result<Result<(), ValidationError>, CompilerServiceError> {
        let contract = self.static_contract(generation, contract_id)?;
        Ok(contract
            .plan
            .validate_with_limits(value, self.limits.contract_validation))
    }

    fn compile(
        &mut self,
        project_id: RegisteredProjectId,
    ) -> Result<(CompilerServiceCheck, Option<BuildOutput>), CompilerServiceError> {
        let limits = self.limits;
        let project = self.project_mut(project_id)?;
        let result = project.compiler.compile(
            &project.registration.entry_module,
            &project.registration.loader,
            project.registration.compiler_options.clone(),
        );
        let static_debug_info = result.compilation.debug_info.clone();
        if let Some(static_debug_info) = static_debug_info.as_ref() {
            validate_static_debug_info(static_debug_info, limits)?;
        }
        let sequence = project
            .next_generation
            .checked_add(1)
            .ok_or(CompilerServiceError::GenerationExhausted { project_id })?;
        project.next_generation = sequence;
        let generation = RegisteredProjectGeneration {
            project_id,
            sequence,
        };
        let diagnostics =
            capped_diagnostics(&result.compilation.diagnostics, limits.max_diagnostics);
        let artifact_fingerprint = result
            .compilation
            .output
            .as_ref()
            .map(|output| output.fingerprint.clone());
        let check = CompilerServiceCheck {
            generation,
            cache_hit: result.cache_hit,
            parsed_modules: result.parsed_modules,
            reused_parsed_modules: result.reused_parsed_modules,
            rechecked_modules: result.rechecked_modules,
            reused_checked_modules: result.reused_checked_modules,
            diagnostics,
            has_errors: result.compilation.has_errors(),
            artifact_fingerprint,
            static_debug_info: static_debug_info.clone(),
        };
        project.latest = Some(RetainedCompilation {
            generation,
            static_debug_info,
        });
        Ok((check, result.compilation.output))
    }

    fn project(
        &self,
        project_id: RegisteredProjectId,
    ) -> Result<&RegisteredProject, CompilerServiceError> {
        self.projects
            .get(&project_id)
            .ok_or(CompilerServiceError::UnknownProject { project_id })
    }

    fn project_mut(
        &mut self,
        project_id: RegisteredProjectId,
    ) -> Result<&mut RegisteredProject, CompilerServiceError> {
        self.projects
            .get_mut(&project_id)
            .ok_or(CompilerServiceError::UnknownProject { project_id })
    }

    fn static_info(
        &self,
        generation: RegisteredProjectGeneration,
    ) -> Result<&BlueTsDebugInfo, CompilerServiceError> {
        let project = self.project(generation.project_id())?;
        let retained = project
            .latest
            .as_ref()
            .filter(|retained| retained.generation == generation)
            .ok_or(CompilerServiceError::StaleGeneration { generation })?;
        retained
            .static_debug_info
            .as_ref()
            .ok_or(CompilerServiceError::NoStaticMetadata { generation })
    }
}

fn validate_registration(
    registration: &RegisteredProjectRegistration,
) -> Result<(), CompilerServiceError> {
    for (field, value) in [
        (
            "canonical_project_root",
            &registration.canonical_project_root,
        ),
        ("canonical_config_root", &registration.canonical_config_root),
        ("canonical_output_root", &registration.canonical_output_root),
        ("entry_module", &registration.entry_module),
    ] {
        if value.is_empty() || value.contains('\0') {
            return Err(CompilerServiceError::InvalidRegistration { field });
        }
    }
    if registration
        .compiler_options
        .resolver_fingerprint
        .is_empty()
        || registration
            .compiler_options
            .resolver_fingerprint
            .contains('\0')
    {
        return Err(CompilerServiceError::InvalidRegistration {
            field: "compiler_options.resolver_fingerprint",
        });
    }
    Ok(())
}

fn same_registration(
    left: &RegisteredProjectRegistration,
    right: &RegisteredProjectRegistration,
) -> bool {
    left.canonical_project_root == right.canonical_project_root
        && left.canonical_config_root == right.canonical_config_root
        && left.canonical_output_root == right.canonical_output_root
}

fn capped_diagnostics(diagnostics: &[Diagnostic], limit: usize) -> CompilerServiceDiagnostics {
    CompilerServiceDiagnostics {
        entries: diagnostics.iter().take(limit).cloned().collect(),
        truncated: diagnostics.len() > limit,
    }
}

fn validate_static_debug_info(
    info: &BlueTsDebugInfo,
    limits: CompilerServiceLimits,
) -> Result<(), CompilerServiceError> {
    for (resource, actual, limit) in [
        ("sources", info.sources.len(), limits.max_static_sources),
        ("types", info.types.len(), limits.max_static_types),
        ("symbols", info.symbols.len(), limits.max_static_symbols),
        (
            "contracts",
            info.contracts.len(),
            limits.max_static_contracts,
        ),
    ] {
        if actual > limit {
            return Err(CompilerServiceError::StaticMetadataLimit { resource, limit });
        }
    }
    Ok(())
}

fn validate_build_output(
    output: &BuildOutput,
    limits: CompilerServiceLimits,
) -> Result<(), CompilerServiceError> {
    let artifact_count = output.artifacts.len() + output.declaration_modules.len();
    if artifact_count > limits.max_build_artifacts {
        return Err(CompilerServiceError::BuildOutputLimit {
            resource: "artifact count",
            limit: limits.max_build_artifacts,
        });
    }
    let mut total_bytes = output.fingerprint.len();
    for artifact in output.artifacts.values() {
        total_bytes = add_response_bytes(total_bytes, artifact.module_id.len(), limits)?;
        total_bytes = add_response_bytes(total_bytes, artifact.javascript.len(), limits)?;
        if let Some(source_map) = artifact.source_map.as_ref() {
            total_bytes = add_response_bytes(total_bytes, source_map.file.len(), limits)?;
            total_bytes = add_response_bytes(total_bytes, source_map.mappings.len(), limits)?;
            for source in &source_map.sources {
                total_bytes = add_response_bytes(total_bytes, source.len(), limits)?;
            }
            for source in &source_map.sources_content {
                total_bytes = add_response_bytes(total_bytes, source.len(), limits)?;
            }
        }
        if let Some(declaration) = artifact.declaration.as_ref() {
            total_bytes = add_response_bytes(total_bytes, declaration.len(), limits)?;
        }
    }
    for (module_id, declaration) in &output.declaration_modules {
        total_bytes = add_response_bytes(total_bytes, module_id.len(), limits)?;
        total_bytes = add_response_bytes(total_bytes, declaration.len(), limits)?;
    }
    if total_bytes > limits.max_build_artifact_bytes {
        return Err(CompilerServiceError::BuildOutputLimit {
            resource: "bytes",
            limit: limits.max_build_artifact_bytes,
        });
    }
    Ok(())
}

fn add_response_bytes(
    total: usize,
    additional: usize,
    limits: CompilerServiceLimits,
) -> Result<usize, CompilerServiceError> {
    let total = total
        .checked_add(additional)
        .ok_or(CompilerServiceError::BuildOutputLimit {
            resource: "bytes",
            limit: limits.max_build_artifact_bytes,
        })?;
    if total > limits.max_build_artifact_bytes {
        return Err(CompilerServiceError::BuildOutputLimit {
            resource: "bytes",
            limit: limits.max_build_artifact_bytes,
        });
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use blueice_bluets::{AuthorizedModule, AuthorizedModuleResolution, RuntimePolicy};

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
                source_map: true,
                declaration: true,
                resolver_fingerprint: "registered-project-resolver-v1".to_string(),
                runtime_policy: RuntimePolicy::Checked,
                ..CompilerOptions::default()
            },
        }
    }

    #[test]
    fn registered_projects_pin_authorized_inputs_and_expose_generation_bound_metadata() {
        let mut service = RegisteredProjectCompilerService::default();
        let id = service
            .register(registration("export const value: number = answer;"))
            .unwrap();
        assert_eq!(
            service.identity(id).unwrap(),
            RegisteredProjectIdentity {
                id,
                canonical_project_root: "project:///app".to_string(),
                canonical_config_root: "project:///app/blue-ts.json".to_string(),
                canonical_output_root: "project:///dist".to_string(),
                entry_module: ENTRY.to_string(),
            }
        );

        let first = service.check(id).unwrap();
        assert!(!first.has_errors, "{:#?}", first.diagnostics.entries);
        assert!(!first.cache_hit);
        assert!(first.artifact_fingerprint.is_some());
        let info = first.static_debug_info.as_ref().unwrap();
        let static_type = service
            .static_type(first.generation, info.types[0].id)
            .unwrap();
        let symbol = service
            .static_symbol(first.generation, info.symbols[0].id)
            .unwrap();
        assert_eq!(static_type.id, info.types[0].id);
        assert_eq!(symbol.id, info.symbols[0].id);
        assert!(service.static_debug_info(first.generation).is_ok());

        let second = service.check(id).unwrap();
        assert!(second.cache_hit);
        assert!(matches!(
            service.static_debug_info(first.generation),
            Err(CompilerServiceError::StaleGeneration { .. })
        ));
    }

    #[test]
    fn retained_contracts_and_provenance_are_exact_generation_bound() {
        let mut service = RegisteredProjectCompilerService::default();
        let id = service
            .register(registration(
                "interface Settings { enabled: boolean; } \
                 export const settings: Settings = { enabled: true };",
            ))
            .unwrap();
        let check = service.check(id).unwrap();
        let info = check.static_debug_info.as_ref().unwrap();
        let contract = info
            .contracts
            .iter()
            .find(|contract| contract.name == "Settings")
            .expect("a non-generic local interface must be reifiable");
        let provenance = service
            .static_provenance(check.generation, contract.source)
            .unwrap();
        assert_eq!(provenance.id, contract.source);
        assert_ne!(provenance.content_hash, "Settings");
        assert!(service
            .validate_static_contract(
                check.generation,
                contract.id,
                &ContractValue::Object(BTreeMap::from([(
                    "enabled".to_string(),
                    ContractValue::Boolean(true),
                )])),
            )
            .unwrap()
            .is_ok());
        let invalid = service
            .validate_static_contract(
                check.generation,
                contract.id,
                &ContractValue::Object(BTreeMap::from([(
                    "enabled".to_string(),
                    ContractValue::String("not-a-boolean".to_string()),
                )])),
            )
            .unwrap()
            .unwrap_err();
        assert_eq!(invalid.path, "$.enabled");

        let later = service.check(id).unwrap();
        assert_ne!(check.generation, later.generation);
        assert!(matches!(
            service.static_contract(check.generation, contract.id),
            Err(CompilerServiceError::StaleGeneration { .. })
        ));
    }

    #[test]
    fn build_keeps_artifacts_in_memory_and_preserves_no_emit_on_error() {
        let mut service = RegisteredProjectCompilerService::default();
        let valid = service
            .register(registration("export const value: number = answer;"))
            .unwrap();
        let build = service.build(valid).unwrap();
        let output = build.output.expect("valid build must return artifacts");
        assert_eq!(output.artifacts.len(), 2);
        assert!(build.check.artifact_fingerprint.is_some());

        let invalid = service
            .register(RegisteredProjectRegistration {
                canonical_project_root: "project:///invalid".to_string(),
                canonical_config_root: "project:///invalid/blue-ts.json".to_string(),
                canonical_output_root: "project:///invalid-dist".to_string(),
                ..registration("export const value: number = 'wrong';")
            })
            .unwrap();
        let failed_build = service.build(invalid).unwrap();
        assert!(failed_build.check.has_errors);
        assert!(failed_build.output.is_none());
        assert!(failed_build.check.artifact_fingerprint.is_none());
    }

    #[test]
    fn registration_and_build_response_limits_fail_closed() {
        let mut service = RegisteredProjectCompilerService::new(CompilerServiceLimits {
            max_projects: 1,
            max_build_artifact_bytes: 1,
            ..CompilerServiceLimits::default()
        });
        let id = service
            .register(registration("export const value: number = answer;"))
            .unwrap();
        assert!(matches!(
            service.register(RegisteredProjectRegistration {
                canonical_project_root: "project:///second".to_string(),
                canonical_config_root: "project:///second/blue-ts.json".to_string(),
                canonical_output_root: "project:///second-dist".to_string(),
                ..registration("export const value: number = answer;")
            }),
            Err(CompilerServiceError::ProjectLimit { limit: 1 })
        ));
        assert!(matches!(
            service.build(id),
            Err(CompilerServiceError::BuildOutputLimit {
                resource: "bytes",
                limit: 1,
            })
        ));
    }

    #[test]
    fn registration_rejects_an_entry_outside_the_closed_module_graph() {
        let mut service = RegisteredProjectCompilerService::default();
        let mut registration = registration("export const value: number = answer;");
        registration.entry_module = "project:///app/unapproved.ts".to_string();
        assert!(matches!(
            service.register(registration),
            Err(CompilerServiceError::EntryModuleNotAuthorized { .. })
        ));
    }
}
