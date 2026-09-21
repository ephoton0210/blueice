// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Core-side admission for an already-authorized direct BlueTS page script.
//!
//! This is deliberately not a page loader or a DOM binding layer. It reads a
//! live [`crate::TabManager`] only to bind a script realm to the current
//! document's tab, URL-derived origin, and replacement generation. A future
//! long-lived BlueJS process must still carry caller-authorized script kind and
//! closed source graph through the normal page pipeline. Keeping those checks
//! together prevents a caller from compiling against a host profile that the
//! runtime did not verify.

use super::host_typings::{
    core_script_host_type_catalog, GeneratedHostTypingsV1, HostRuntimeBindingV1,
    HostTypeSurfaceCatalogV1, HostTypingsError, HostTypingsManifestV1,
    CORE_SCRIPT_DOCUMENT_TEXT_PROFILE_V1,
};
use crate::{Page, TabId, TabManager};
use blueice_bluets::{
    AuthorizedModule, AuthorizedModuleLoader, CompilerOptions, RuntimePolicy, LANGUAGE_VERSION,
};
use blueice_bluets_bluejs::{
    compile_direct_module_graph, compile_direct_script, BridgeError, DirectPageRealmOwner,
};
use std::{collections::BTreeMap, fmt};

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
    /// A core-owned live tab identity, never an untrusted raw integer.
    pub tab_id: TabId,
    pub kind: DirectPageScriptKind,
    pub entry: String,
    pub loader: &'a AuthorizedModuleLoader,
    pub compiler_options: CompilerOptions,
    pub feature_profile: String,
    pub supplied_manifest: &'a HostTypingsManifestV1,
    pub supplied_declaration_source: &'a str,
    pub supplied_runtime_bindings: &'a [HostRuntimeBindingV1],
}

/// A request to execute one inline, explicitly opted-in BlueTS declaration
/// from the current core document. This deliberately has no source text,
/// `src`, module ID, or resolver records: the core host derives the exact
/// single-module graph from the live document. External declarations and
/// imports remain a future authorized-loader boundary.
pub struct DirectInlinePageScriptRequest<'a> {
    pub tab_id: TabId,
    pub ordinal: u32,
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
    live_documents: BTreeMap<TabId, LivePageIdentity>,
    bound_profiles: BTreeMap<TabId, String>,
}

/// The core-visible portion of a live page document needed to keep a BlueJS
/// realm correctly scoped. This is not a web-exposed document identity.
#[derive(Debug, Clone, PartialEq, Eq)]
struct LivePageIdentity {
    document_generation: u64,
    origin: blueice_bluejs::BlueJsPageOrigin,
}

impl DirectPageScriptHost {
    pub fn new(profiles: HostTypeSurfaceCatalogV1) -> Self {
        Self::with_realm_owner(profiles, DirectPageRealmOwner::default())
    }

    /// Creates a direct-page host with caller-selected, already validated
    /// BlueJS realm and static-debug retention limits. This lets the core
    /// process own its page-script resource policy without exposing a VM or
    /// permitting a script request to change those limits.
    pub fn with_realm_owner(
        profiles: HostTypeSurfaceCatalogV1,
        realms: DirectPageRealmOwner,
    ) -> Self {
        Self {
            profiles,
            realms,
            live_documents: BTreeMap::new(),
            bound_profiles: BTreeMap::new(),
        }
    }

    /// Synchronizes one realm with a real, currently open core tab. A changed
    /// document generation always replaces the realm, including a same-origin
    /// navigation. Only loaded HTTP(S) documents currently have a direct-page
    /// origin; blank and built-in pages fail closed rather than inheriting an
    /// arbitrary caller-supplied string.
    pub fn synchronize_tab(
        &mut self,
        tabs: &TabManager,
        tab_id: TabId,
    ) -> Result<(), DirectPageScriptError> {
        let target = match tabs.get(tab_id) {
            Some(page) => live_page_identity(tab_id, page),
            None => Err(DirectPageScriptError::UnknownTab {
                tab_id: tab_id.as_u64(),
            }),
        };
        let target = match target {
            Ok(target) => target,
            Err(error) => {
                // A missing or non-page-origin document must not leave the
                // preceding page's realm usable under this tab identity.
                self.close_page(tab_id);
                return Err(error);
            }
        };
        self.synchronize_live_document(tab_id, target)
    }

    /// Synchronizes every already-admitted realm with its current core tab and
    /// releases realms whose tabs have closed or no longer have an HTTP(S)
    /// document. This deliberately does not allocate VMs for ordinary tabs
    /// that have never admitted a direct script. Session/page lifecycle owners
    /// call this after a lifecycle batch; [`Self::execute`] also synchronizes
    /// its addressed tab defensively.
    pub fn synchronize_tabs(&mut self, tabs: &TabManager) -> Result<(), DirectPageScriptError> {
        let tracked_tabs: Vec<_> = self.live_documents.keys().copied().collect();
        for tab_id in tracked_tabs {
            match self.synchronize_tab(tabs, tab_id) {
                Ok(())
                | Err(DirectPageScriptError::UnknownTab { .. })
                | Err(DirectPageScriptError::PageHasNoUrl { .. })
                | Err(DirectPageScriptError::InvalidPageUrl { .. }) => {}
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }

    /// Closes one realm and its generation-bound static metadata. Call this
    /// from a tab-close lifecycle event; [`Self::synchronize_tabs`] detects the
    /// same condition when a batch observer is available.
    pub fn close_page(&mut self, tab_id: TabId) -> bool {
        self.live_documents.remove(&tab_id);
        self.bound_profiles.remove(&tab_id);
        self.realms.close_realm(tab_id.as_u64())
    }

    /// Compiles, admits, and executes an opted-in TypeScript script. All host
    /// typing checks happen before BlueTS parsing or BlueJS program admission.
    pub fn execute(
        &mut self,
        tabs: &TabManager,
        request: DirectPageScriptRequest<'_>,
    ) -> Result<blueice_bluejs::Value, DirectPageScriptError> {
        // Lifecycle synchronization must precede every caller-controlled
        // compiler/profile rejection. Otherwise a malformed request arriving
        // after navigation could leave the preceding document's realm alive.
        self.synchronize_tab(tabs, request.tab_id)?;
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
        self.configure_profile_bindings(tabs, request.tab_id, &request.feature_profile, &artifact)?;
        let mut options = request.compiler_options;
        options.ambient_declaration_modules = vec![declaration];
        options.require_declared_global_calls = true;
        let origin = self
            .live_documents
            .get(&request.tab_id)
            .expect("a successful tab synchronization retains its document")
            .origin
            .clone();
        match request.kind {
            DirectPageScriptKind::Classic => {
                let script = compile_direct_script(&request.entry, request.loader, options)
                    .map_err(DirectPageScriptError::Bridge)?;
                let attachment = self
                    .realms
                    .attach_script(&script, request.tab_id.as_u64(), &origin)
                    .map_err(DirectPageScriptError::Bridge)?;
                self.realms
                    .execute_program(request.tab_id.as_u64(), &attachment)
                    .map_err(DirectPageScriptError::Bridge)
            }
            DirectPageScriptKind::Module => {
                let graph = compile_direct_module_graph(&request.entry, request.loader, options)
                    .map_err(DirectPageScriptError::Bridge)?;
                let attachment = self
                    .realms
                    .attach_module_graph(&graph, request.tab_id.as_u64(), &origin)
                    .map_err(DirectPageScriptError::Bridge)?;
                self.realms
                    .execute_module_graph(request.tab_id.as_u64(), &attachment)
                    .map_err(DirectPageScriptError::Bridge)
            }
        }
    }

    /// Admits and executes one inline script declaration from the current
    /// document. The generated module identity is scoped to the core tab and
    /// replacement-document generation, so an ordinal from an older document
    /// cannot be attached to a successor realm. An external `src` fails rather
    /// than acquiring a loader or resolution authority implicitly.
    pub fn execute_inline(
        &mut self,
        tabs: &TabManager,
        request: DirectInlinePageScriptRequest<'_>,
    ) -> Result<blueice_bluejs::Value, DirectPageScriptError> {
        // An external declaration is rejected below, but it still belongs to
        // the current document and must invalidate a prior document's realm.
        self.synchronize_tab(tabs, request.tab_id)?;
        let page = tabs
            .get(request.tab_id)
            .ok_or(DirectPageScriptError::UnknownTab {
                tab_id: request.tab_id.as_u64(),
            })?;
        let document_generation = page.document_generation();
        let declaration = page
            .blue_ts_script_declarations()
            .into_iter()
            .find(|declaration| match declaration {
                crate::script::BlueTsPageScriptDeclaration::Inline { ordinal, .. }
                | crate::script::BlueTsPageScriptDeclaration::External { ordinal, .. } => {
                    *ordinal == request.ordinal
                }
            })
            .ok_or(DirectPageScriptError::InlineScriptNotFound {
                tab_id: request.tab_id.as_u64(),
                ordinal: request.ordinal,
            })?;
        let (kind, source) = match declaration {
            crate::script::BlueTsPageScriptDeclaration::Inline { kind, source, .. } => {
                (kind, source)
            }
            crate::script::BlueTsPageScriptDeclaration::External { src, .. } => {
                return Err(DirectPageScriptError::ExternalScriptRequiresLoader {
                    tab_id: request.tab_id.as_u64(),
                    ordinal: request.ordinal,
                    src,
                });
            }
        };
        let entry = inline_module_id(request.tab_id, document_generation, request.ordinal);
        let loader =
            AuthorizedModuleLoader::new([AuthorizedModule::new(entry.clone(), source)], [])
                .expect("a single core-generated inline source always forms a valid closed loader");
        self.execute(
            tabs,
            DirectPageScriptRequest {
                tab_id: request.tab_id,
                kind,
                entry,
                loader: &loader,
                compiler_options: request.compiler_options,
                feature_profile: request.feature_profile,
                supplied_manifest: request.supplied_manifest,
                supplied_declaration_source: request.supplied_declaration_source,
                supplied_runtime_bindings: request.supplied_runtime_bindings,
            },
        )
    }

    pub fn debug_record_count(&self) -> usize {
        self.realms.debug_record_count()
    }

    /// Returns bounded runtime accounting for one currently admitted page
    /// realm. The result contains no VM handle, source text, or bytecode; it
    /// lets the core host attribute retained programs, bytecode, and heap use
    /// to the tab that caused them.
    pub fn realm_stats(
        &self,
        tab_id: TabId,
    ) -> Result<blueice_bluejs::BlueJsPageRealmStats, DirectPageScriptError> {
        self.realms
            .realm_stats(tab_id.as_u64())
            .map_err(DirectPageScriptError::Bridge)
    }

    fn synchronize_live_document(
        &mut self,
        tab_id: TabId,
        target: LivePageIdentity,
    ) -> Result<(), DirectPageScriptError> {
        let replaced = match self.live_documents.get(&tab_id) {
            Some(current) if current == &target => false,
            Some(_) => {
                self.realms
                    .navigate(tab_id.as_u64(), target.origin.clone())
                    .map_err(DirectPageScriptError::Bridge)?;
                true
            }
            None => {
                self.realms
                    .open_realm(tab_id.as_u64(), target.origin.clone())
                    .map_err(DirectPageScriptError::Bridge)?;
                true
            }
        };
        if replaced {
            self.bound_profiles.remove(&tab_id);
        }
        self.live_documents.insert(tab_id, target);
        Ok(())
    }

    fn configure_profile_bindings(
        &mut self,
        tabs: &TabManager,
        tab_id: TabId,
        profile: &str,
        artifact: &GeneratedHostTypingsV1,
    ) -> Result<(), DirectPageScriptError> {
        if let Some(active) = self.bound_profiles.get(&tab_id) {
            return if active == profile {
                Ok(())
            } else {
                Err(DirectPageScriptError::BindingProfileAlreadySelected)
            };
        }
        if artifact.runtime_bindings.is_empty() {
            self.bound_profiles.insert(tab_id, profile.to_string());
            return Ok(());
        }

        let expected = core_script_host_type_catalog()
            .generate(CORE_SCRIPT_DOCUMENT_TEXT_PROFILE_V1)
            .expect("the checked-in document-text host profile is valid");
        if profile != CORE_SCRIPT_DOCUMENT_TEXT_PROFILE_V1
            || artifact.manifest != expected.manifest
            || artifact.declaration_source != expected.declaration_source
            || artifact.runtime_bindings != expected.runtime_bindings
        {
            return Err(DirectPageScriptError::RuntimeBindingProfileUnavailable);
        }
        let document_text = tabs
            .get(tab_id)
            .ok_or(DirectPageScriptError::UnknownTab {
                tab_id: tab_id.as_u64(),
            })?
            .script_document_text_content();
        self.realms
            .configure_realm_bindings(tab_id.as_u64(), move |bindings| {
                bindings.install_global_function(
                    "blueiceDocumentText",
                    0,
                    move |args: &[blueice_bluejs::HostValue]| {
                        if !args.is_empty() {
                            return Err(blueice_bluejs::HostFunctionError::new(
                                "blueiceDocumentText requires no arguments",
                            ));
                        }
                        Ok(blueice_bluejs::HostValue::String(
                            document_text.clone().into(),
                        ))
                    },
                )
            })
            .map_err(DirectPageScriptError::Bridge)?;
        self.bound_profiles.insert(tab_id, profile.to_string());
        Ok(())
    }
}

fn inline_module_id(tab_id: TabId, document_generation: u64, ordinal: u32) -> String {
    format!(
        "blueice://page/tab-{}/document-{document_generation}/inline-{ordinal}.ts",
        tab_id.as_u64()
    )
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

fn live_page_identity(
    tab_id: TabId,
    page: &Page,
) -> Result<LivePageIdentity, DirectPageScriptError> {
    let url = page.url().ok_or(DirectPageScriptError::PageHasNoUrl {
        tab_id: tab_id.as_u64(),
    })?;
    let origin = blueice_net::canonical_http_origin(url).map_err(|error| {
        DirectPageScriptError::InvalidPageUrl {
            tab_id: tab_id.as_u64(),
            url: url.to_string(),
            message: error.to_string(),
        }
    })?;
    let origin =
        blueice_bluejs::BlueJsPageOrigin::new(origin).map_err(DirectPageScriptError::Origin)?;
    Ok(LivePageIdentity {
        document_generation: page.document_generation(),
        origin,
    })
}

#[derive(Debug)]
pub enum DirectPageScriptError {
    HostTypings(HostTypingsError),
    Origin(blueice_bluejs::BlueJsPageRuntimeError),
    Bridge(BridgeError),
    CallerSuppliedAmbientDeclarations,
    TranspileOnlyPolicy,
    LanguageVersionMismatch {
        profile: String,
    },
    RuntimeBindingProfileUnavailable,
    BindingProfileAlreadySelected,
    UnknownTab {
        tab_id: u64,
    },
    PageHasNoUrl {
        tab_id: u64,
    },
    InvalidPageUrl {
        tab_id: u64,
        url: String,
        message: String,
    },
    InlineScriptNotFound {
        tab_id: u64,
        ordinal: u32,
    },
    ExternalScriptRequiresLoader {
        tab_id: u64,
        ordinal: u32,
        src: String,
    },
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
            Self::RuntimeBindingProfileUnavailable => {
                formatter.write_str("host typing profile has no matching runtime binding installer")
            }
            Self::BindingProfileAlreadySelected => {
                formatter.write_str("page realm already selected a different host binding profile")
            }
            Self::UnknownTab { tab_id } => write!(formatter, "unknown page tab {tab_id}"),
            Self::PageHasNoUrl { tab_id } => {
                write!(formatter, "page tab {tab_id} has no loaded document URL")
            }
            Self::InvalidPageUrl {
                tab_id,
                url,
                message,
            } => write!(
                formatter,
                "page tab {tab_id} has an invalid document URL `{url}`: {message}"
            ),
            Self::InlineScriptNotFound { tab_id, ordinal } => write!(
                formatter,
                "page tab {tab_id} has no inline BlueTS script declaration {ordinal}"
            ),
            Self::ExternalScriptRequiresLoader {
                tab_id,
                ordinal,
                src,
            } => write!(
                formatter,
                "page tab {tab_id} script declaration {ordinal} uses external source `{src}` and requires an authorized loader"
            ),
        }
    }
}

impl std::error::Error for DirectPageScriptError {}

#[cfg(test)]
mod tests {
    use super::super::host_typings::{
        core_script_host_type_catalog, HostTypeSurfaceV1, CORE_SCRIPT_DOCUMENT_TEXT_PROFILE_V1,
        CORE_SCRIPT_EMPTY_PROFILE_V1,
    };
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

    fn loaded_tabs() -> (TabManager, TabId) {
        let mut tabs = TabManager::new(320.0, 200.0);
        let tab_id = tabs.default_tab();
        tabs.get_mut(tab_id).unwrap().load_html_str(
            "<main id=\"app\"></main>",
            Some("https://example.test/app/index.html".to_string()),
        );
        (tabs, tab_id)
    }

    fn request<'a>(
        tab_id: TabId,
        loader: &'a AuthorizedModuleLoader,
        artifact: &'a GeneratedHostTypingsV1,
    ) -> DirectPageScriptRequest<'a> {
        DirectPageScriptRequest {
            tab_id,
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
        let (tabs, tab_id) = loaded_tabs();
        let mut host = DirectPageScriptHost::new(profiles);
        assert_eq!(
            host.execute(&tabs, request(tab_id, &loader, &artifact))
                .unwrap(),
            blueice_bluejs::Value::Number(42.0)
        );
        assert_eq!(host.debug_record_count(), 1);
        let stats = host.realm_stats(tab_id).unwrap();
        assert_eq!(stats.tab_id, tab_id.as_u64());
        assert_eq!(stats.program_count, 1);
        assert!(stats.bytecode_bytes > 0);
    }

    #[test]
    fn configured_bytecode_limit_rejects_before_direct_page_execution() {
        let profiles = catalog();
        let artifact = profiles.generate("test-empty-v1").unwrap();
        let loader = AuthorizedModuleLoader::new(
            [AuthorizedModule::new(
                "page:///app/main.ts",
                "const answer: number = 42; answer;",
            )],
            [],
        )
        .unwrap();
        let (tabs, tab_id) = loaded_tabs();
        let realms = DirectPageRealmOwner::new(
            blueice_bluejs::BlueJsPageRuntimeConfig {
                max_bytecode_bytes_per_realm: 1,
                ..blueice_bluejs::BlueJsPageRuntimeConfig::default()
            },
            blueice_bluets_bluejs::DirectDebugRetentionLimits::default(),
        )
        .unwrap();
        let mut host = DirectPageScriptHost::with_realm_owner(profiles, realms);

        assert!(matches!(
            host.execute(&tabs, request(tab_id, &loader, &artifact)),
            Err(DirectPageScriptError::Bridge(BridgeError::PageRuntime(
                blueice_bluejs::BlueJsPageRuntimeError::BytecodeLimit {
                    tab_id: failed_tab,
                    limit: 1,
                }
            ))) if failed_tab == tab_id.as_u64()
        ));
        assert_eq!(host.debug_record_count(), 0);
        let stats = host.realm_stats(tab_id).unwrap();
        assert_eq!(stats.program_count, 0);
        assert_eq!(stats.bytecode_bytes, 0);
    }

    #[test]
    fn document_text_profile_executes_against_the_current_document_only() {
        let profiles = core_script_host_type_catalog();
        let artifact = profiles
            .generate(CORE_SCRIPT_DOCUMENT_TEXT_PROFILE_V1)
            .unwrap();
        let loader = AuthorizedModuleLoader::new(
            [AuthorizedModule::new(
                "page:///app/main.ts",
                "const text: string = blueiceDocumentText(); text;",
            )],
            [],
        )
        .unwrap();
        let (mut tabs, tab_id) = loaded_tabs();
        tabs.get_mut(tab_id).unwrap().load_html_str(
            "<main>first <strong>document</strong></main>",
            Some("https://example.test/app/index.html".to_string()),
        );
        let mut host = DirectPageScriptHost::new(profiles);
        assert_eq!(
            host.execute(
                &tabs,
                DirectPageScriptRequest {
                    tab_id,
                    kind: DirectPageScriptKind::Classic,
                    entry: "page:///app/main.ts".to_string(),
                    loader: &loader,
                    compiler_options: CompilerOptions::default(),
                    feature_profile: CORE_SCRIPT_DOCUMENT_TEXT_PROFILE_V1.to_string(),
                    supplied_manifest: &artifact.manifest,
                    supplied_declaration_source: &artifact.declaration_source,
                    supplied_runtime_bindings: &artifact.runtime_bindings,
                },
            )
            .unwrap(),
            blueice_bluejs::Value::String("first document".into())
        );

        tabs.get_mut(tab_id).unwrap().load_html_str(
            "<main>replacement document</main>",
            Some("https://example.test/app/next.html".to_string()),
        );
        assert_eq!(
            host.execute(
                &tabs,
                DirectPageScriptRequest {
                    tab_id,
                    kind: DirectPageScriptKind::Classic,
                    entry: "page:///app/main.ts".to_string(),
                    loader: &loader,
                    compiler_options: CompilerOptions::default(),
                    feature_profile: CORE_SCRIPT_DOCUMENT_TEXT_PROFILE_V1.to_string(),
                    supplied_manifest: &artifact.manifest,
                    supplied_declaration_source: &artifact.declaration_source,
                    supplied_runtime_bindings: &artifact.runtime_bindings,
                },
            )
            .unwrap(),
            blueice_bluejs::Value::String("replacement document".into())
        );
    }

    #[test]
    fn non_empty_profiles_require_a_matching_runtime_installer() {
        let profiles = HostTypeSurfaceCatalogV1::new([HostTypeSurfaceV1::new(
            LANGUAGE_VERSION,
            "test-page-v1",
            "uninstalled-binding-v1",
            vec![super::super::host_typings::HostTypeBindingV1::new(
                "test.uninstalled",
                "declare function unavailable(): number;",
                super::super::host_typings::HostBindingRoleV1::Value,
                "global.unavailable",
                "test",
                "test",
                "test-page-v1",
            )],
        )])
        .unwrap();
        let artifact = profiles.generate("uninstalled-binding-v1").unwrap();
        let loader = AuthorizedModuleLoader::new(
            [AuthorizedModule::new(
                "page:///app/main.ts",
                "unavailable();",
            )],
            [],
        )
        .unwrap();
        let (tabs, tab_id) = loaded_tabs();
        let mut host = DirectPageScriptHost::new(profiles);
        assert!(matches!(
            host.execute(
                &tabs,
                DirectPageScriptRequest {
                    tab_id,
                    kind: DirectPageScriptKind::Classic,
                    entry: "page:///app/main.ts".to_string(),
                    loader: &loader,
                    compiler_options: CompilerOptions::default(),
                    feature_profile: "uninstalled-binding-v1".to_string(),
                    supplied_manifest: &artifact.manifest,
                    supplied_declaration_source: &artifact.declaration_source,
                    supplied_runtime_bindings: &artifact.runtime_bindings,
                },
            ),
            Err(DirectPageScriptError::RuntimeBindingProfileUnavailable)
        ));
    }

    #[test]
    fn document_text_profile_rejects_arguments_during_page_profile_checking() {
        let profiles = core_script_host_type_catalog();
        let artifact = profiles
            .generate(CORE_SCRIPT_DOCUMENT_TEXT_PROFILE_V1)
            .unwrap();
        let loader = AuthorizedModuleLoader::new(
            [AuthorizedModule::new(
                "page:///app/main.ts",
                "blueiceDocumentText(1);",
            )],
            [],
        )
        .unwrap();
        let (tabs, tab_id) = loaded_tabs();
        let mut host = DirectPageScriptHost::new(profiles);
        assert!(matches!(
            host.execute(
                &tabs,
                DirectPageScriptRequest {
                    tab_id,
                    kind: DirectPageScriptKind::Classic,
                    entry: "page:///app/main.ts".to_string(),
                    loader: &loader,
                    compiler_options: CompilerOptions::default(),
                    feature_profile: CORE_SCRIPT_DOCUMENT_TEXT_PROFILE_V1.to_string(),
                    supplied_manifest: &artifact.manifest,
                    supplied_declaration_source: &artifact.declaration_source,
                    supplied_runtime_bindings: &artifact.runtime_bindings,
                },
            ),
            Err(DirectPageScriptError::Bridge(BridgeError::BlueTs(diagnostics)))
                if diagnostics.iter().any(|diagnostic| {
                    diagnostic.code == blueice_bluets::DiagnosticCode::TypeMismatch
                })
        ));
    }

    #[test]
    fn empty_profile_rejects_document_text_before_vm_admission() {
        let profiles = core_script_host_type_catalog();
        let artifact = profiles.generate(CORE_SCRIPT_EMPTY_PROFILE_V1).unwrap();
        let loader = AuthorizedModuleLoader::new(
            [AuthorizedModule::new(
                "page:///app/main.ts",
                "blueiceDocumentText();",
            )],
            [],
        )
        .unwrap();
        let (tabs, tab_id) = loaded_tabs();
        let mut host = DirectPageScriptHost::new(profiles);

        assert!(matches!(
            host.execute(
                &tabs,
                DirectPageScriptRequest {
                    tab_id,
                    kind: DirectPageScriptKind::Classic,
                    entry: "page:///app/main.ts".to_string(),
                    loader: &loader,
                    compiler_options: CompilerOptions::default(),
                    feature_profile: CORE_SCRIPT_EMPTY_PROFILE_V1.to_string(),
                    supplied_manifest: &artifact.manifest,
                    supplied_declaration_source: &artifact.declaration_source,
                    supplied_runtime_bindings: &artifact.runtime_bindings,
                },
            ),
            Err(DirectPageScriptError::Bridge(BridgeError::BlueTs(diagnostics)))
                if diagnostics.iter().any(|diagnostic| {
                    diagnostic.code == blueice_bluets::DiagnosticCode::UnknownName
                        && diagnostic.message
                            == "function blueiceDocumentText is not declared by this page profile"
                })
        ));
        assert_eq!(host.debug_record_count(), 0);
    }

    #[test]
    fn a_page_realm_cannot_switch_binding_profiles_before_navigation() {
        let profiles = core_script_host_type_catalog();
        let empty = profiles.generate(CORE_SCRIPT_EMPTY_PROFILE_V1).unwrap();
        let document_text = profiles
            .generate(CORE_SCRIPT_DOCUMENT_TEXT_PROFILE_V1)
            .unwrap();
        let (mut tabs, tab_id) = loaded_tabs();
        let mut host = DirectPageScriptHost::new(profiles);
        host.synchronize_tab(&tabs, tab_id).unwrap();
        host.configure_profile_bindings(&tabs, tab_id, CORE_SCRIPT_EMPTY_PROFILE_V1, &empty)
            .unwrap();
        assert!(matches!(
            host.configure_profile_bindings(
                &tabs,
                tab_id,
                CORE_SCRIPT_DOCUMENT_TEXT_PROFILE_V1,
                &document_text,
            ),
            Err(DirectPageScriptError::BindingProfileAlreadySelected)
        ));

        tabs.get_mut(tab_id).unwrap().load_html_str(
            "<main>replacement</main>",
            Some("https://example.test/app/next.html".to_string()),
        );
        host.synchronize_tab(&tabs, tab_id).unwrap();
        host.configure_profile_bindings(
            &tabs,
            tab_id,
            CORE_SCRIPT_DOCUMENT_TEXT_PROFILE_V1,
            &document_text,
        )
        .unwrap();
    }

    #[test]
    fn rejects_callers_attempt_to_bypass_the_verified_profile() {
        let profiles = catalog();
        let artifact = profiles.generate("test-empty-v1").unwrap();
        let loader =
            AuthorizedModuleLoader::new([AuthorizedModule::new("page:///app/main.ts", "1;")], [])
                .unwrap();
        let (tabs, tab_id) = loaded_tabs();
        let mut host = DirectPageScriptHost::new(profiles);
        let mut request = request(tab_id, &loader, &artifact);
        request.compiler_options.ambient_declaration_modules.push(
            blueice_bluets::ModuleSource::new(
                "page:///untrusted.d.ts",
                "declare const unsafe: any;",
            ),
        );
        assert!(matches!(
            host.execute(&tabs, request),
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
        let (tabs, tab_id) = loaded_tabs();
        let mut host = DirectPageScriptHost::new(profiles);
        let mut request = request(tab_id, &loader, &artifact);
        request.kind = DirectPageScriptKind::Module;
        assert_eq!(
            host.execute(&tabs, request).unwrap(),
            blueice_bluejs::Value::Number(42.0)
        );
        assert_eq!(host.debug_record_count(), 2);
    }

    #[test]
    fn same_origin_document_replacement_recreates_the_page_realm() {
        let profiles = catalog();
        let artifact = profiles.generate("test-empty-v1").unwrap();
        let first = AuthorizedModuleLoader::new(
            [AuthorizedModule::new("page:///app/main.ts", "40 + 2;")],
            [],
        )
        .unwrap();
        let second = AuthorizedModuleLoader::new(
            [AuthorizedModule::new("page:///app/main.ts", "40 + 3;")],
            [],
        )
        .unwrap();
        let (mut tabs, tab_id) = loaded_tabs();
        let mut host = DirectPageScriptHost::new(profiles);

        assert_eq!(
            host.execute(&tabs, request(tab_id, &first, &artifact))
                .unwrap(),
            blueice_bluejs::Value::Number(42.0)
        );
        assert_eq!(host.debug_record_count(), 1);

        tabs.get_mut(tab_id).unwrap().load_html_str(
            "<main id=\"next\"></main>",
            Some("https://example.test/app/next.html".to_string()),
        );
        assert_eq!(
            host.execute(&tabs, request(tab_id, &second, &artifact))
                .unwrap(),
            blueice_bluejs::Value::Number(43.0)
        );
        assert_eq!(
            host.debug_record_count(),
            1,
            "a same-origin replacement must prune the preceding realm metadata"
        );
        assert_eq!(host.realm_stats(tab_id).unwrap().program_count, 1);
    }

    #[test]
    fn navigation_releases_a_realm_bytecode_charge_before_the_next_script_admission() {
        let profiles = catalog();
        let artifact = profiles.generate("test-empty-v1").unwrap();
        let loader =
            AuthorizedModuleLoader::new([AuthorizedModule::new("page:///app/main.ts", "42;")], [])
                .unwrap();
        let (mut tabs, tab_id) = loaded_tabs();
        let mut host = DirectPageScriptHost::new(profiles);
        host.execute(&tabs, request(tab_id, &loader, &artifact))
            .unwrap();
        assert!(host.realm_stats(tab_id).unwrap().bytecode_bytes > 0);

        tabs.get_mut(tab_id).unwrap().load_html_str(
            "<main id=\"next\"></main>",
            Some("https://example.test/app/next.html".to_string()),
        );
        host.synchronize_tab(&tabs, tab_id).unwrap();
        let stats = host.realm_stats(tab_id).unwrap();
        assert_eq!(stats.program_count, 0);
        assert_eq!(stats.bytecode_bytes, 0);
        assert_eq!(host.debug_record_count(), 0);
    }

    #[test]
    fn synchronizing_tabs_prunes_a_closed_tabs_realm() {
        let profiles = catalog();
        let artifact = profiles.generate("test-empty-v1").unwrap();
        let loader =
            AuthorizedModuleLoader::new([AuthorizedModule::new("page:///app/main.ts", "42;")], [])
                .unwrap();
        let (mut tabs, tab_id) = loaded_tabs();
        let mut host = DirectPageScriptHost::new(profiles);
        host.execute(&tabs, request(tab_id, &loader, &artifact))
            .unwrap();
        assert_eq!(host.debug_record_count(), 1);

        assert!(tabs.close_tab(tab_id));
        host.synchronize_tabs(&tabs).unwrap();
        assert_eq!(host.debug_record_count(), 0);
        assert!(matches!(
            host.synchronize_tab(&tabs, tab_id),
            Err(DirectPageScriptError::UnknownTab { .. })
        ));
    }

    #[test]
    fn synchronizing_tabs_does_not_allocate_a_realm_for_an_unadmitted_page() {
        let (tabs, _) = loaded_tabs();
        let mut host = DirectPageScriptHost::new(catalog());
        host.synchronize_tabs(&tabs).unwrap();
        assert_eq!(host.debug_record_count(), 0);
    }

    #[test]
    fn blank_or_builtin_pages_do_not_gain_a_caller_selected_origin() {
        let profiles = catalog();
        let artifact = profiles.generate("test-empty-v1").unwrap();
        let loader =
            AuthorizedModuleLoader::new([AuthorizedModule::new("page:///app/main.ts", "42;")], [])
                .unwrap();
        let mut tabs = TabManager::new(320.0, 200.0);
        let tab_id = tabs.default_tab();
        let mut host = DirectPageScriptHost::new(profiles);
        assert!(matches!(
            host.execute(&tabs, request(tab_id, &loader, &artifact)),
            Err(DirectPageScriptError::PageHasNoUrl { .. })
        ));

        tabs.get_mut(tab_id)
            .unwrap()
            .load_html_str("", Some("about:blank".to_string()));
        assert!(matches!(
            host.execute(&tabs, request(tab_id, &loader, &artifact)),
            Err(DirectPageScriptError::InvalidPageUrl { .. })
        ));
        assert_eq!(host.debug_record_count(), 0);
    }

    #[test]
    fn executes_an_inline_document_declaration_as_a_closed_single_module() {
        let profiles = catalog();
        let artifact = profiles.generate("test-empty-v1").unwrap();
        let mut tabs = TabManager::new(320.0, 200.0);
        let tab_id = tabs.default_tab();
        tabs.get_mut(tab_id).unwrap().load_html_str(
            "<script type=\"application/x-blueice-typescript\">const answer: number = 40 + 2; answer;</script>",
            Some("https://example.test/app/index.html".to_string()),
        );
        let mut host = DirectPageScriptHost::new(profiles);

        assert_eq!(
            host.execute_inline(
                &tabs,
                DirectInlinePageScriptRequest {
                    tab_id,
                    ordinal: 0,
                    compiler_options: CompilerOptions::default(),
                    feature_profile: "test-empty-v1".to_string(),
                    supplied_manifest: &artifact.manifest,
                    supplied_declaration_source: &artifact.declaration_source,
                    supplied_runtime_bindings: &artifact.runtime_bindings,
                },
            )
            .unwrap(),
            blueice_bluejs::Value::Number(42.0)
        );
        assert_eq!(host.debug_record_count(), 1);
    }

    #[test]
    fn executes_an_inline_module_declaration_without_a_second_resolver() {
        let profiles = catalog();
        let artifact = profiles.generate("test-empty-v1").unwrap();
        let mut tabs = TabManager::new(320.0, 200.0);
        let tab_id = tabs.default_tab();
        tabs.get_mut(tab_id).unwrap().load_html_str(
            "<script type=\"application/x-blueice-typescript-module\">export const answer: number = 40 + 2; answer;</script>",
            Some("https://example.test/app/index.html".to_string()),
        );
        let mut host = DirectPageScriptHost::new(profiles);

        assert_eq!(
            host.execute_inline(
                &tabs,
                DirectInlinePageScriptRequest {
                    tab_id,
                    ordinal: 0,
                    compiler_options: CompilerOptions::default(),
                    feature_profile: "test-empty-v1".to_string(),
                    supplied_manifest: &artifact.manifest,
                    supplied_declaration_source: &artifact.declaration_source,
                    supplied_runtime_bindings: &artifact.runtime_bindings,
                },
            )
            .unwrap(),
            blueice_bluejs::Value::Number(42.0)
        );
        assert_eq!(host.debug_record_count(), 1);
    }

    #[test]
    fn inline_execution_rejects_external_source_without_loading_it() {
        let profiles = catalog();
        let artifact = profiles.generate("test-empty-v1").unwrap();
        let mut tabs = TabManager::new(320.0, 200.0);
        let tab_id = tabs.default_tab();
        tabs.get_mut(tab_id).unwrap().load_html_str(
            "<script type=\"application/x-blueice-typescript\" src=\"/app.ts\"></script>",
            Some("https://example.test/app/index.html".to_string()),
        );
        let mut host = DirectPageScriptHost::new(profiles);

        assert!(matches!(
            host.execute_inline(
                &tabs,
                DirectInlinePageScriptRequest {
                    tab_id,
                    ordinal: 0,
                    compiler_options: CompilerOptions::default(),
                    feature_profile: "test-empty-v1".to_string(),
                    supplied_manifest: &artifact.manifest,
                    supplied_declaration_source: &artifact.declaration_source,
                    supplied_runtime_bindings: &artifact.runtime_bindings,
                },
            ),
            Err(DirectPageScriptError::ExternalScriptRequiresLoader { .. })
        ));
        assert_eq!(host.debug_record_count(), 0);
    }

    #[test]
    fn rejected_external_declaration_still_retires_the_prior_document_realm() {
        let profiles = catalog();
        let artifact = profiles.generate("test-empty-v1").unwrap();
        let mut tabs = TabManager::new(320.0, 200.0);
        let tab_id = tabs.default_tab();
        tabs.get_mut(tab_id).unwrap().load_html_str(
            "<script type=\"application/x-blueice-typescript\">42;</script>",
            Some("https://example.test/app/first.html".to_string()),
        );
        let mut host = DirectPageScriptHost::new(profiles);
        host.execute_inline(
            &tabs,
            DirectInlinePageScriptRequest {
                tab_id,
                ordinal: 0,
                compiler_options: CompilerOptions::default(),
                feature_profile: "test-empty-v1".to_string(),
                supplied_manifest: &artifact.manifest,
                supplied_declaration_source: &artifact.declaration_source,
                supplied_runtime_bindings: &artifact.runtime_bindings,
            },
        )
        .unwrap();
        assert_eq!(host.debug_record_count(), 1);

        tabs.get_mut(tab_id).unwrap().load_html_str(
            "<script type=\"application/x-blueice-typescript\" src=\"/second.ts\"></script>",
            Some("https://example.test/app/second.html".to_string()),
        );
        assert!(matches!(
            host.execute_inline(
                &tabs,
                DirectInlinePageScriptRequest {
                    tab_id,
                    ordinal: 0,
                    compiler_options: CompilerOptions::default(),
                    feature_profile: "test-empty-v1".to_string(),
                    supplied_manifest: &artifact.manifest,
                    supplied_declaration_source: &artifact.declaration_source,
                    supplied_runtime_bindings: &artifact.runtime_bindings,
                },
            ),
            Err(DirectPageScriptError::ExternalScriptRequiresLoader { .. })
        ));
        assert_eq!(host.debug_record_count(), 0);
    }
}
