// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Core-side admission for an already-authorized direct BlueTS page script.
//!
//! This is deliberately not a page loader or a DOM binding layer. A future
//! long-lived BlueJS process must call this equivalent admission boundary only
//! after the normal page pipeline has selected a tab, origin, script kind, and
//! closed source graph. Keeping those checks together prevents a caller from
//! compiling against a host profile that the runtime did not verify.

use super::host_typings::{
    GeneratedHostTypingsV1, HostRuntimeBindingV1, HostTypeSurfaceCatalogV1, HostTypingsError,
    HostTypingsManifestV1,
};
use blueice_bluets::{AuthorizedModuleLoader, CompilerOptions, RuntimePolicy, LANGUAGE_VERSION};
use blueice_bluets_bluejs::{
    compile_direct_module_graph, compile_direct_script, BridgeError, DirectPageRealmOwner,
};
use std::fmt;

/// The only opt-in TypeScript script forms this admission boundary recognizes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectPageScriptKind {
    Classic,
    Module,
}

/// A host-authorized direct-page compilation request. The source loader is a
/// closed graph; the request does not carry a filesystem path, URL to fetch,
/// DOM capability, or an arbitrary ambient declaration source.
pub struct DirectPageScriptRequest<'a> {
    pub tab_id: u64,
    pub origin: String,
    pub kind: DirectPageScriptKind,
    pub entry: String,
    pub loader: &'a AuthorizedModuleLoader,
    pub compiler_options: CompilerOptions,
    pub feature_profile: String,
    pub supplied_manifest: &'a HostTypingsManifestV1,
    pub supplied_declaration_source: &'a str,
    pub supplied_runtime_bindings: &'a [HostRuntimeBindingV1],
}

/// A core-owned direct-page admission owner. It combines a declared host
/// profile catalog with page-realm lifetime ownership, but deliberately does
/// not provide page discovery, IPC transport, or JavaScript DOM objects.
pub struct DirectPageScriptHost {
    profiles: HostTypeSurfaceCatalogV1,
    realms: DirectPageRealmOwner,
}

impl DirectPageScriptHost {
    pub fn new(profiles: HostTypeSurfaceCatalogV1) -> Self {
        Self {
            profiles,
            realms: DirectPageRealmOwner::default(),
        }
    }

    /// Opens one host-authorized page realm. Origin canonicalization and page
    /// policy remain the caller's responsibility.
    pub fn open_page(
        &mut self,
        tab_id: u64,
        origin: impl Into<String>,
    ) -> Result<(), DirectPageScriptError> {
        self.realms
            .open_realm(tab_id, page_origin(origin.into())?)
            .map_err(DirectPageScriptError::Bridge)
    }

    /// Replaces a page realm after a caller-authorized navigation or reload.
    pub fn navigate_page(
        &mut self,
        tab_id: u64,
        origin: impl Into<String>,
    ) -> Result<(), DirectPageScriptError> {
        self.realms
            .navigate(tab_id, page_origin(origin.into())?)
            .map_err(DirectPageScriptError::Bridge)
    }

    /// Closes one realm and its generation-bound static metadata.
    pub fn close_page(&mut self, tab_id: u64) -> bool {
        self.realms.close_realm(tab_id)
    }

    /// Compiles, admits, and executes an opted-in TypeScript script. All host
    /// typing checks happen before BlueTS parsing or BlueJS program admission.
    pub fn execute(
        &mut self,
        request: DirectPageScriptRequest<'_>,
    ) -> Result<blueice_bluejs::Value, DirectPageScriptError> {
        if !request
            .compiler_options
            .ambient_declaration_modules
            .is_empty()
        {
            return Err(DirectPageScriptError::CallerSuppliedAmbientDeclarations);
        }
        if matches!(
            request.compiler_options.runtime_policy,
            RuntimePolicy::TranspileOnly
        ) {
            return Err(DirectPageScriptError::TranspileOnlyPolicy);
        }
        let artifact = self
            .profiles
            .generate(&request.feature_profile)
            .map_err(DirectPageScriptError::HostTypings)?;
        let declaration = verified_declaration(&artifact, &request)?;
        let mut options = request.compiler_options;
        options.ambient_declaration_modules = vec![declaration];
        let origin = page_origin(request.origin)?;
        match request.kind {
            DirectPageScriptKind::Classic => {
                let script = compile_direct_script(&request.entry, request.loader, options)
                    .map_err(DirectPageScriptError::Bridge)?;
                let attachment = self
                    .realms
                    .attach_script(&script, request.tab_id, &origin)
                    .map_err(DirectPageScriptError::Bridge)?;
                self.realms
                    .execute_program(request.tab_id, &attachment)
                    .map_err(DirectPageScriptError::Bridge)
            }
            DirectPageScriptKind::Module => {
                let graph = compile_direct_module_graph(&request.entry, request.loader, options)
                    .map_err(DirectPageScriptError::Bridge)?;
                let attachment = self
                    .realms
                    .attach_module_graph(&graph, request.tab_id, &origin)
                    .map_err(DirectPageScriptError::Bridge)?;
                self.realms
                    .execute_module_graph(request.tab_id, &attachment)
                    .map_err(DirectPageScriptError::Bridge)
            }
        }
    }

    pub fn debug_record_count(&self) -> usize {
        self.realms.debug_record_count()
    }
}

fn verified_declaration(
    artifact: &GeneratedHostTypingsV1,
    request: &DirectPageScriptRequest<'_>,
) -> Result<blueice_bluets::ModuleSource, DirectPageScriptError> {
    if artifact.manifest.language_version != LANGUAGE_VERSION {
        return Err(DirectPageScriptError::LanguageVersionMismatch {
            profile: artifact.manifest.language_version.clone(),
        });
    }
    artifact
        .verify_for_direct_compiler(
            format!(
                "blueice:///profiles/{}/lib.blueice.d.ts",
                request.feature_profile
            ),
            request.supplied_manifest,
            request.supplied_declaration_source,
            request.supplied_runtime_bindings,
        )
        .map_err(DirectPageScriptError::HostTypings)
}

fn page_origin(value: String) -> Result<blueice_bluejs::BlueJsPageOrigin, DirectPageScriptError> {
    blueice_bluejs::BlueJsPageOrigin::new(value).map_err(DirectPageScriptError::Origin)
}

#[derive(Debug)]
pub enum DirectPageScriptError {
    HostTypings(HostTypingsError),
    Origin(blueice_bluejs::BlueJsPageRuntimeError),
    Bridge(BridgeError),
    CallerSuppliedAmbientDeclarations,
    TranspileOnlyPolicy,
    LanguageVersionMismatch { profile: String },
}

impl fmt::Display for DirectPageScriptError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::HostTypings(error) => {
                write!(formatter, "host typings rejected the page script: {error}")
            }
            Self::Origin(error) => write!(formatter, "page origin is invalid: {error}"),
            Self::Bridge(error) => write!(
                formatter,
                "direct BlueTS bridge rejected the page script: {error}"
            ),
            Self::CallerSuppliedAmbientDeclarations => {
                formatter.write_str("page script request must not supply ambient declarations")
            }
            Self::TranspileOnlyPolicy => {
                formatter.write_str("direct page scripts cannot use transpile-only policy")
            }
            Self::LanguageVersionMismatch { profile } => write!(
                formatter,
                "host typing profile targets unsupported BlueTS language version `{profile}`"
            ),
        }
    }
}

impl std::error::Error for DirectPageScriptError {}

#[cfg(test)]
mod tests {
    use super::super::host_typings::HostTypeSurfaceV1;
    use super::*;
    use blueice_bluets::{AuthorizedModule, AuthorizedModuleResolution};

    fn catalog() -> HostTypeSurfaceCatalogV1 {
        HostTypeSurfaceCatalogV1::new([HostTypeSurfaceV1::new(
            LANGUAGE_VERSION,
            "test-page-v1",
            "test-empty-v1",
            Vec::new(),
        )])
        .unwrap()
    }

    fn request<'a>(
        loader: &'a AuthorizedModuleLoader,
        artifact: &'a GeneratedHostTypingsV1,
    ) -> DirectPageScriptRequest<'a> {
        DirectPageScriptRequest {
            tab_id: 7,
            origin: "https://example.test".to_string(),
            kind: DirectPageScriptKind::Classic,
            entry: "page:///app/main.ts".to_string(),
            loader,
            compiler_options: CompilerOptions::default(),
            feature_profile: "test-empty-v1".to_string(),
            supplied_manifest: &artifact.manifest,
            supplied_declaration_source: &artifact.declaration_source,
            supplied_runtime_bindings: &artifact.runtime_bindings,
        }
    }

    #[test]
    fn admits_only_a_verified_profile_and_closed_graph_into_its_open_realm() {
        let profiles = catalog();
        let artifact = profiles.generate("test-empty-v1").unwrap();
        let loader = AuthorizedModuleLoader::new(
            [AuthorizedModule::new(
                "page:///app/main.ts",
                "const answer: number = 40 + 2; answer;",
            )],
            [],
        )
        .unwrap();
        let mut host = DirectPageScriptHost::new(profiles);
        host.open_page(7, "https://example.test").unwrap();
        assert_eq!(
            host.execute(request(&loader, &artifact)).unwrap(),
            blueice_bluejs::Value::Number(42.0)
        );
        assert_eq!(host.debug_record_count(), 1);
    }

    #[test]
    fn rejects_callers_attempt_to_bypass_the_verified_profile() {
        let profiles = catalog();
        let artifact = profiles.generate("test-empty-v1").unwrap();
        let loader =
            AuthorizedModuleLoader::new([AuthorizedModule::new("page:///app/main.ts", "1;")], [])
                .unwrap();
        let mut host = DirectPageScriptHost::new(profiles);
        host.open_page(7, "https://example.test").unwrap();
        let mut request = request(&loader, &artifact);
        request.compiler_options.ambient_declaration_modules.push(
            blueice_bluets::ModuleSource::new(
                "page:///untrusted.d.ts",
                "declare const unsafe: any;",
            ),
        );
        assert!(matches!(
            host.execute(request),
            Err(DirectPageScriptError::CallerSuppliedAmbientDeclarations)
        ));
        assert_eq!(host.debug_record_count(), 0);
    }

    #[test]
    fn admits_a_verified_authorized_module_graph_into_the_same_page_realm() {
        let profiles = catalog();
        let artifact = profiles.generate("test-empty-v1").unwrap();
        let loader = AuthorizedModuleLoader::new(
            [
                AuthorizedModule::new(
                    "page:///app/main.ts",
                    "import { value } from './value'; export const answer: number = value + 1; answer;",
                ),
                AuthorizedModule::new("page:///app/value.ts", "export const value: number = 41;"),
            ],
            [AuthorizedModuleResolution::new(
                "page:///app/main.ts",
                "./value",
                "page:///app/value.ts",
            )],
        )
        .unwrap();
        let mut host = DirectPageScriptHost::new(profiles);
        host.open_page(7, "https://example.test").unwrap();
        let mut request = request(&loader, &artifact);
        request.kind = DirectPageScriptKind::Module;
        assert_eq!(
            host.execute(request).unwrap(),
            blueice_bluejs::Value::Number(42.0)
        );
        assert_eq!(host.debug_record_count(), 2);
    }
}
