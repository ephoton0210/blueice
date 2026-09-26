// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

pub(super) fn validate_registration(
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

pub(super) fn same_registration(
    left: &RegisteredProjectRegistration,
    right: &RegisteredProjectRegistration,
) -> bool {
    left.canonical_project_root == right.canonical_project_root
        && left.canonical_config_root == right.canonical_config_root
        && left.canonical_output_root == right.canonical_output_root
}

pub(super) fn diagnostic_locations(
    loader: &AuthorizedModuleLoader,
    diagnostics: &[Diagnostic],
) -> Vec<Option<DebugSourceLocation>> {
    let mut locations = vec![None; diagnostics.len()];
    for (module, source) in loader.authorized_modules() {
        let matching = diagnostics
            .iter()
            .enumerate()
            .filter(|(_, diagnostic)| diagnostic.span.module == module)
            .map(|(index, diagnostic)| (index, &diagnostic.span))
            .collect::<Vec<_>>();
        let mapped = source_locations_for_spans(source, matching.iter().map(|(_, span)| *span));
        for ((index, _), location) in matching.into_iter().zip(mapped) {
            locations[index] = location;
        }
    }
    locations
}

pub(super) fn capped_diagnostics(
    diagnostics: &[Diagnostic],
    limit: usize,
    retention_truncated: bool,
) -> CompilerServiceDiagnostics {
    CompilerServiceDiagnostics {
        entries: diagnostics.iter().take(limit).cloned().collect(),
        truncated: retention_truncated || diagnostics.len() > limit,
    }
}

/// Materializes only compiler-minted numeric handles for pagination. The
/// returned page never includes source text, module identities, names, type
/// displays, contract plans, compiler options, or runtime values.
pub(super) fn static_metadata_ids(
    info: &BlueTsDebugInfo,
    kind: StaticMetadataInventoryKind,
) -> Vec<u32> {
    match kind {
        StaticMetadataInventoryKind::Sources => {
            info.sources.iter().map(|source| source.id.0).collect()
        }
        StaticMetadataInventoryKind::Types => info
            .types
            .iter()
            .map(|static_type| static_type.id.0)
            .collect(),
        StaticMetadataInventoryKind::Symbols => {
            info.symbols.iter().map(|symbol| symbol.id.0).collect()
        }
        StaticMetadataInventoryKind::Contracts => info
            .contracts
            .iter()
            .map(|contract| contract.id.0)
            .collect(),
    }
}

pub(super) fn validate_static_debug_info(
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

pub(super) fn validate_build_output(
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

pub(super) fn add_response_bytes(
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
