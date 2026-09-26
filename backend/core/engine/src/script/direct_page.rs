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

use super::contracts::{
    core_script_binding_contract, CoreScriptBindingContractLimits,
    CORE_SCRIPT_DOCUMENT_ORIGIN_RESULT_CONTRACT_V1, CORE_SCRIPT_DOCUMENT_TEXT_RESULT_CONTRACT_V1,
};
use super::host_typings::{
    core_script_host_type_catalog, GeneratedHostTypingsV1, HostRuntimeBindingV1,
    HostTypeSurfaceCatalogV1, HostTypingsError, HostTypingsManifestV1,
    CORE_SCRIPT_DOCUMENT_CONTEXT_PROFILE_V1, CORE_SCRIPT_DOCUMENT_TEXT_PROFILE_V1,
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
    binding_contract_limits: CoreScriptBindingContractLimits,
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
        Self::with_realm_owner_and_contract_limits(
            profiles,
            realms,
            CoreScriptBindingContractLimits::default(),
        )
    }

    /// Creates a direct-page host with fixed budgets for the current live
    /// host-to-script binding contracts. Page requests cannot alter these
    /// budgets; a result that exceeds one is rejected before compilation can
    /// admit bytecode into the realm.
    pub fn with_realm_owner_and_contract_limits(
        profiles: HostTypeSurfaceCatalogV1,
        realms: DirectPageRealmOwner,
        binding_contract_limits: CoreScriptBindingContractLimits,
    ) -> Self {
        Self {
            profiles,
            realms,
            binding_contract_limits,
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

        let expected = match profile {
            CORE_SCRIPT_DOCUMENT_TEXT_PROFILE_V1 | CORE_SCRIPT_DOCUMENT_CONTEXT_PROFILE_V1 => {
                core_script_host_type_catalog()
                    .generate(profile)
                    .expect("the checked-in core host profile is valid")
            }
            _ => return Err(DirectPageScriptError::RuntimeBindingProfileUnavailable),
        };
        if artifact.manifest != expected.manifest
            || artifact.declaration_source != expected.declaration_source
            || artifact.runtime_bindings != expected.runtime_bindings
        {
            return Err(DirectPageScriptError::RuntimeBindingProfileUnavailable);
        }
        let page = tabs.get(tab_id).ok_or(DirectPageScriptError::UnknownTab {
            tab_id: tab_id.as_u64(),
        })?;
        let document_text = page.script_document_text_content();
        let includes_document_origin = profile == CORE_SCRIPT_DOCUMENT_CONTEXT_PROFILE_V1;
        let document_origin = includes_document_origin.then(|| {
            self.live_documents
                .get(&tab_id)
                .expect("a bound profile always has a synchronized live document")
                .origin
                .as_str()
                .to_string()
        });
        let text_boundary = core_script_binding_contract("dom.document-text")
            .expect("the installed document-text binding has a contract inventory entry");
        text_boundary
            .validate_string(&document_text, self.binding_contract_limits.document_text)
            .map_err(|error| DirectPageScriptError::BindingContractViolation {
                stable_binding_id: text_boundary.stable_binding_id,
                contract_id: CORE_SCRIPT_DOCUMENT_TEXT_RESULT_CONTRACT_V1,
                error,
            })?;
        if let Some(document_origin) = document_origin.as_deref() {
            let origin_boundary = core_script_binding_contract("dom.document-origin")
                .expect("the installed document-origin binding has a contract inventory entry");
            origin_boundary
                .validate_string(
                    document_origin,
                    self.binding_contract_limits.document_origin,
                )
                .map_err(|error| DirectPageScriptError::BindingContractViolation {
                    stable_binding_id: origin_boundary.stable_binding_id,
                    contract_id: CORE_SCRIPT_DOCUMENT_ORIGIN_RESULT_CONTRACT_V1,
                    error,
                })?;
        }
        self.realms
            .configure_realm_bindings(tab_id.as_u64(), move |bindings| {
                if includes_document_origin {
                    let document_origin = document_origin
                        .expect("the document-context profile captures its validated origin");
                    bindings.install_global_function(
                        "blueiceDocumentOrigin",
                        0,
                        move |args: &[blueice_bluejs::HostValue]| {
                            require_no_arguments(args, "blueiceDocumentOrigin")?;
                            Ok(blueice_bluejs::HostValue::String(
                                document_origin.clone().into(),
                            ))
                        },
                    )?;
                }
                bindings.install_global_function(
                    "blueiceDocumentText",
                    0,
                    move |args: &[blueice_bluejs::HostValue]| {
                        require_no_arguments(args, "blueiceDocumentText")?;
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

fn require_no_arguments(
    arguments: &[blueice_bluejs::HostValue],
    function: &str,
) -> Result<(), blueice_bluejs::HostFunctionError> {
    if arguments.is_empty() {
        Ok(())
    } else {
        Err(blueice_bluejs::HostFunctionError::new(format!(
            "{function} requires no arguments"
        )))
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
    BindingContractViolation {
        stable_binding_id: &'static str,
        contract_id: &'static str,
        error: blueice_bluets::ValidationError,
    },
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
            Self::BindingContractViolation {
                stable_binding_id,
                contract_id,
                error,
            } => write!(
                formatter,
                "page host binding `{stable_binding_id}` violated contract `{contract_id}` at {}: expected {}, observed {}",
                error.path, error.expected, error.observed
            ),
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
#[path = "direct_page/tests.rs"]
mod tests;
