// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

/// Registers the reference binary's deliberately compiled-in closed fixture.
/// Real embedders use [`CoreCompilerProjectCatalog`] directly at trusted core
/// startup, where they can supply their already-authorized graph and fixed
/// policy without ever making a path/source/configuration API available to a
/// compiler peer. Keeping this one profile in code gives the public process
/// seam a real lifecycle regression target without turning a CLI flag into a
/// filesystem project loader.
#[cfg(unix)]
pub(super) fn register_compiler_startup_profile(
    catalog: &mut CoreCompilerProjectCatalog,
    profile: &str,
) -> Result<(), String> {
    use blueice_bluets::{
        AuthorizedModule, AuthorizedModuleLoader, CompilerOptions, RuntimePolicy,
    };

    match profile {
        "core-closed-fixture-v1" => {
            let entry_module = "project:///core-fixture/main.ts";
            catalog
                .register_startup_project(
                    blueice_engine::compiler_service::RegisteredProjectRegistration {
                        canonical_project_root: "project:///core-fixture".to_string(),
                        canonical_config_root: "project:///core-fixture/blue-ts.json".to_string(),
                        canonical_output_root: "project:///core-fixture-dist".to_string(),
                        entry_module: entry_module.to_string(),
                        loader: AuthorizedModuleLoader::new(
                            [AuthorizedModule::new(
                                entry_module,
                                "interface CoreFixtureSettings { enabled: boolean; } \
                                 export const coreFixtureSettings: CoreFixtureSettings = { enabled: true }; \
                                 export const coreRegisteredAnswer: number = 42;",
                            )],
                            [],
                        )
                        .map_err(|error| {
                            format!("invalid compiled-in compiler project profile: {error}")
                        })?,
                        compiler_options: CompilerOptions {
                            resolver_fingerprint: "core-closed-fixture-v1".to_string(),
                            runtime_policy: RuntimePolicy::Checked,
                            ..CompilerOptions::default()
                        },
                    },
                )
                .map_err(|error| format!("failed to register compiler startup profile: {error}"))?;
            Ok(())
        }
        _ => Err(format!(
            "unsupported compiler project profile: {profile}; only core-owned compiled-in profiles are accepted"
        )),
    }
}

/// Consumes the trusted launcher's already-selected closed graph. This runs
/// before any core, compiler, debugger, or script listener is bound. Loader
/// construction and catalog registration reject invalid graphs atomically.
#[cfg(unix)]
pub(super) fn register_owner_compiler_catalog(
    catalog: &mut CoreCompilerProjectCatalog,
    bootstrap: blueice_ipc::compiler_catalog::CompilerCatalogBootstrap,
) -> Result<(), String> {
    use blueice_bluets::{
        AuthorizedModule, AuthorizedModuleLoader, AuthorizedModuleResolution, CompilerOptions,
        EcmaTarget, ModuleSource, RuntimePolicy,
    };
    use blueice_ipc::compiler_catalog::{CompilerCatalogRuntimePolicy, CompilerCatalogTarget};

    bootstrap.validate().map_err(|error| error.to_string())?;
    for project in bootstrap.projects {
        let expose_to_compiler_ipc = project.expose_to_compiler_ipc;
        let grant_output_write = project.grant_output_write;
        let loader = AuthorizedModuleLoader::new(
            project
                .modules
                .into_iter()
                .map(|module| AuthorizedModule::new(module.canonical_id, module.text)),
            project.resolutions.into_iter().map(|edge| {
                AuthorizedModuleResolution::new(
                    edge.from_module,
                    edge.specifier,
                    edge.target_module,
                )
            }),
        )
        .map_err(|error| format!("invalid owner compiler graph: {error}"))?;
        let compiler_options = CompilerOptions {
            target: match project.options.target {
                CompilerCatalogTarget::Es2020 => EcmaTarget::Es2020,
                CompilerCatalogTarget::Es2022 => EcmaTarget::Es2022,
            },
            runtime_policy: match project.options.runtime_policy {
                CompilerCatalogRuntimePolicy::TranspileOnly => RuntimePolicy::TranspileOnly,
                CompilerCatalogRuntimePolicy::Checked => RuntimePolicy::Checked,
                CompilerCatalogRuntimePolicy::StrictRuntime => RuntimePolicy::StrictRuntime,
            },
            source_map: project.options.source_map,
            declaration: project.options.declaration,
            resolver_fingerprint: project.options.resolver_fingerprint,
            ambient_declaration_modules: project
                .options
                .ambient_declaration_modules
                .into_iter()
                .map(|module| ModuleSource::new(module.canonical_id, module.text))
                .collect(),
            require_declared_global_calls: project.options.require_declared_global_calls,
            ..CompilerOptions::default()
        };
        let registration = blueice_engine::compiler_service::RegisteredProjectRegistration {
            canonical_project_root: project.canonical_project_root,
            canonical_config_root: project.canonical_config_root,
            canonical_output_root: project.canonical_output_root,
            entry_module: project.entry_module,
            loader,
            compiler_options,
        };
        let result = match (expose_to_compiler_ipc, grant_output_write) {
            (true, true) => catalog
                .register_startup_project_with_output_write_grant(registration)
                .map_err(|error| error.to_string()),
            (false, true) => catalog
                .register_startup_project_private_with_output_write_grant(registration)
                .map_err(|error| error.to_string()),
            (true, false) => catalog
                .register_startup_project(registration)
                .map_err(|error| error.to_string()),
            (false, false) => catalog
                .register_startup_project_private(registration)
                .map_err(|error| error.to_string()),
        };
        result.map_err(|error| format!("failed to register owner compiler project: {error}"))?;
    }
    Ok(())
}

/// Reuses the core's one HTTP(S) source-authorizer implementation. This is
/// deliberately constructed before any listener: malformed canonical URLs,
/// origin rules, integrity entries, or limits cannot create a partly live
/// browser or compiler endpoint.
#[cfg(unix)]
pub(super) fn construct_owner_http_page_policy(
    bootstrap: blueice_ipc::owner_bootstrap::OwnerHttpPolicyBootstrap,
) -> Result<script::http_resource_authorizer::HttpScriptResourcePolicy, String> {
    use blueice_ipc::owner_bootstrap::OwnerHttpOriginRule;
    use script::http_resource_authorizer::{
        HttpScriptIntegrityManifest, HttpScriptResourceLimits, HttpScriptResourceOriginRule,
        HttpScriptResourcePolicy,
    };

    bootstrap.validate().map_err(|error| error.to_string())?;
    let origin_rule = match bootstrap.origin_rule {
        OwnerHttpOriginRule::SameDocumentOrigin => {
            HttpScriptResourceOriginRule::same_document_origin()
        }
        OwnerHttpOriginRule::ExactOrigin(origin) => {
            HttpScriptResourceOriginRule::exact_origin(origin).map_err(|error| error.to_string())?
        }
    };
    let manifest = HttpScriptIntegrityManifest::new(
        bootstrap
            .resources
            .into_iter()
            .map(|resource| (resource.canonical_url, resource.integrity)),
    )
    .map_err(|error| error.to_string())?;
    HttpScriptResourcePolicy::new(origin_rule, manifest, HttpScriptResourceLimits::default())
        .map_err(|error| error.to_string())
}
