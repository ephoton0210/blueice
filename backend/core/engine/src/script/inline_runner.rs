// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Explicitly configured execution of inline BlueTS page declarations.
//!
//! This is a core-owned page-pipeline seam, not a replacement module loader.
//! It observes parsed, opt-in declarations on a live [`crate::Page`], uses
//! the selected host profile that it generated itself, and executes each
//! inline declaration at most once for a tab/document generation. External
//! declarations remain rejected unless a core-owned source authorizer supplies
//! their complete closed graph. The default session loop does not construct
//! this type.

use super::{
    direct_page::{
        DirectInlinePageScriptRequest, DirectPageScriptError, DirectPageScriptHost,
        DirectPageScriptKind, DirectPageScriptRequest,
    },
    host_typings::{GeneratedHostTypingsV1, HostTypeSurfaceCatalogV1, HostTypingsError},
    page_source_authorizer::{PageScriptSourceAuthorizer, PageScriptSourceRequest},
    BlueTsPageScriptDeclaration,
};
use crate::{TabId, TabManager};
use blueice_bluets::{CompilerOptions, RuntimePolicy};
use blueice_bluets_bluejs::{BridgeError, DirectPageRealmOwner};
use std::collections::{BTreeMap, VecDeque};
use std::fmt;

/// Maximum execution reports retained for the core owner. Reports deliberately
/// contain no script source or BlueJS runtime value.
const MAX_EXECUTION_REPORTS: usize = 128;

/// One successful or rejected declaration observed by
/// [`DirectPageInlineExecutor`]. A browser-facing error/event transport is a
/// separate page-host concern; these bounded records make the lifecycle seam
/// observable without pretending that such a transport exists already.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DirectPageScriptExecutionReport {
    Executed {
        tab_id: u64,
        document_generation: u64,
        ordinal: u32,
        kind: DirectPageScriptKind,
    },
    Rejected {
        tab_id: u64,
        document_generation: u64,
        ordinal: u32,
        kind: DirectPageScriptKind,
        message: String,
    },
}

/// A core-owned, explicitly selected inline BlueTS execution profile.
///
/// The generated typing artifact is private to the executor: callers can
/// select a known profile, but cannot substitute declaration bytes, manifest,
/// or runtime binding records between documents.
pub struct DirectPageInlineExecutor {
    host: DirectPageScriptHost,
    compiler_options: CompilerOptions,
    feature_profile: String,
    generated_typings: GeneratedHostTypingsV1,
    external_source_authorizer: Option<Box<dyn PageScriptSourceAuthorizer>>,
    observed_documents: BTreeMap<TabId, u64>,
    reports: VecDeque<DirectPageScriptExecutionReport>,
}

impl DirectPageInlineExecutor {
    /// Creates an opt-in runner for one exact host feature profile. Page code
    /// can never request `transpile-only` or add ambient declaration modules.
    pub fn new(
        profiles: HostTypeSurfaceCatalogV1,
        feature_profile: impl Into<String>,
        compiler_options: CompilerOptions,
    ) -> Result<Self, DirectPageInlineExecutorError> {
        let host = DirectPageScriptHost::new(profiles.clone());
        Self::new_with_host(profiles, feature_profile, compiler_options, None, host)
    }

    /// Creates an opt-in runner with caller-selected, already validated page
    /// realm and static-debug retention limits. The limits are fixed before a
    /// document is observed and cannot be changed by its declarations.
    pub fn with_realm_owner(
        profiles: HostTypeSurfaceCatalogV1,
        feature_profile: impl Into<String>,
        compiler_options: CompilerOptions,
        realms: DirectPageRealmOwner,
    ) -> Result<Self, DirectPageInlineExecutorError> {
        let host = DirectPageScriptHost::with_realm_owner(profiles.clone(), realms);
        Self::new_with_host(profiles, feature_profile, compiler_options, None, host)
    }

    /// Creates an opt-in runner whose external declarations can be resolved
    /// only by this core-owned authorizer. The executor itself neither fetches
    /// a URL nor derives an import edge from page text.
    pub fn with_external_source_authorizer(
        profiles: HostTypeSurfaceCatalogV1,
        feature_profile: impl Into<String>,
        compiler_options: CompilerOptions,
        authorizer: impl PageScriptSourceAuthorizer + 'static,
    ) -> Result<Self, DirectPageInlineExecutorError> {
        let host = DirectPageScriptHost::new(profiles.clone());
        Self::new_with_host(
            profiles,
            feature_profile,
            compiler_options,
            Some(Box::new(authorizer)),
            host,
        )
    }

    /// Creates an opt-in runner with both a core-owned external-source
    /// authorizer and caller-selected fixed page-realm limits.
    pub fn with_external_source_authorizer_and_realm_owner(
        profiles: HostTypeSurfaceCatalogV1,
        feature_profile: impl Into<String>,
        compiler_options: CompilerOptions,
        authorizer: impl PageScriptSourceAuthorizer + 'static,
        realms: DirectPageRealmOwner,
    ) -> Result<Self, DirectPageInlineExecutorError> {
        let host = DirectPageScriptHost::with_realm_owner(profiles.clone(), realms);
        Self::new_with_host(
            profiles,
            feature_profile,
            compiler_options,
            Some(Box::new(authorizer)),
            host,
        )
    }

    fn new_with_host(
        profiles: HostTypeSurfaceCatalogV1,
        feature_profile: impl Into<String>,
        compiler_options: CompilerOptions,
        external_source_authorizer: Option<Box<dyn PageScriptSourceAuthorizer>>,
        host: DirectPageScriptHost,
    ) -> Result<Self, DirectPageInlineExecutorError> {
        if !compiler_options.ambient_declaration_modules.is_empty() {
            return Err(DirectPageInlineExecutorError::CallerSuppliedAmbientDeclarations);
        }
        if matches!(
            compiler_options.runtime_policy,
            RuntimePolicy::TranspileOnly
        ) {
            return Err(DirectPageInlineExecutorError::TranspileOnlyPolicy);
        }
        let feature_profile = feature_profile.into();
        let generated_typings = profiles
            .generate(&feature_profile)
            .map_err(DirectPageInlineExecutorError::HostTypings)?;
        Ok(Self {
            host,
            compiler_options,
            feature_profile,
            generated_typings,
            external_source_authorizer,
            observed_documents: BTreeMap::new(),
            reports: VecDeque::new(),
        })
    }

    /// Synchronizes prior realms, then executes every inline opted-in
    /// declaration in document order for each not-yet-observed document.
    /// Rejected declarations are recorded and do not stop later declarations
    /// in the same document. This mirrors the isolation required for a normal
    /// script pipeline while keeping external source loading fail-closed.
    pub fn synchronize_and_execute(
        &mut self,
        tabs: &TabManager,
    ) -> Result<(), DirectPageInlineExecutorError> {
        self.host
            .synchronize_tabs(tabs)
            .map_err(DirectPageInlineExecutorError::Lifecycle)?;
        self.observed_documents
            .retain(|tab_id, _| tabs.get(*tab_id).is_some());

        let tab_ids: Vec<_> = tabs.ids().collect();
        for tab_id in tab_ids {
            let Some(page) = tabs.get(tab_id) else {
                continue;
            };
            let document_generation = page.document_generation();
            if self.observed_documents.get(&tab_id) == Some(&document_generation) {
                continue;
            }
            let declarations = page.blue_ts_script_declarations();
            let document_url = page.url().map(str::to_string);
            // Mark before execution, so a rejected document cannot be retried
            // on every unrelated frontend/session event.
            self.observed_documents.insert(tab_id, document_generation);
            for declaration in declarations {
                self.execute_declaration(
                    tabs,
                    tab_id,
                    document_generation,
                    document_url.as_deref(),
                    declaration,
                );
            }
        }
        Ok(())
    }

    /// Returns bounded reports in execution order. The records retain neither
    /// source text nor runtime values.
    pub fn reports(&self) -> &VecDeque<DirectPageScriptExecutionReport> {
        &self.reports
    }

    /// Removes and returns every retained execution report.
    pub fn drain_reports(&mut self) -> Vec<DirectPageScriptExecutionReport> {
        self.reports.drain(..).collect()
    }

    /// Removes and returns reports for `tab_id`, preserving reports for every
    /// other tab in their original execution order. This keeps the frontend
    /// control plane scoped to its addressed tab without making a report from
    /// one page observable to another page's client.
    pub fn drain_reports_for_tab(&mut self, tab_id: TabId) -> Vec<DirectPageScriptExecutionReport> {
        let mut matched = Vec::new();
        let mut remaining = VecDeque::with_capacity(self.reports.len());
        while let Some(report) = self.reports.pop_front() {
            let report_tab_id = match &report {
                DirectPageScriptExecutionReport::Executed { tab_id, .. }
                | DirectPageScriptExecutionReport::Rejected { tab_id, .. } => *tab_id,
            };
            if report_tab_id == tab_id.as_u64() {
                matched.push(report);
            } else {
                remaining.push_back(report);
            }
        }
        self.reports = remaining;
        matched
    }

    /// Exposes only the count of retained static debug records for lifecycle
    /// regression tests. Static metadata remains owned by the live realm.
    pub fn debug_record_count(&self) -> usize {
        self.host.debug_record_count()
    }

    /// Returns bounded per-tab program, bytecode, and heap accounting for one
    /// currently admitted realm. It deliberately exposes no runtime values,
    /// source text, or VM handle.
    pub fn realm_stats(
        &self,
        tab_id: TabId,
    ) -> Result<blueice_bluejs::BlueJsPageRealmStats, DirectPageScriptError> {
        self.host.realm_stats(tab_id)
    }

    fn execute_declaration(
        &mut self,
        tabs: &TabManager,
        tab_id: TabId,
        document_generation: u64,
        document_url: Option<&str>,
        declaration: BlueTsPageScriptDeclaration,
    ) {
        let (ordinal, kind) = match &declaration {
            BlueTsPageScriptDeclaration::Inline { ordinal, kind, .. }
            | BlueTsPageScriptDeclaration::External { ordinal, kind, .. } => (*ordinal, *kind),
        };
        let BlueTsPageScriptDeclaration::External { src, .. } = declaration else {
            self.execute_inline_declaration(tabs, tab_id, document_generation, ordinal, kind);
            return;
        };
        self.execute_external_declaration(
            tabs,
            tab_id,
            document_generation,
            ordinal,
            kind,
            document_url,
            src,
        );
    }

    fn execute_inline_declaration(
        &mut self,
        tabs: &TabManager,
        tab_id: TabId,
        document_generation: u64,
        ordinal: u32,
        kind: DirectPageScriptKind,
    ) {
        let result = self.host.execute_inline(
            tabs,
            DirectInlinePageScriptRequest {
                tab_id,
                ordinal,
                compiler_options: self.compiler_options.clone(),
                feature_profile: self.feature_profile.clone(),
                supplied_manifest: &self.generated_typings.manifest,
                supplied_declaration_source: &self.generated_typings.declaration_source,
                supplied_runtime_bindings: &self.generated_typings.runtime_bindings,
            },
        );
        self.record_execution_result(tab_id, document_generation, ordinal, kind, result);
    }

    #[allow(clippy::too_many_arguments)]
    fn execute_external_declaration(
        &mut self,
        tabs: &TabManager,
        tab_id: TabId,
        document_generation: u64,
        ordinal: u32,
        kind: DirectPageScriptKind,
        document_url: Option<&str>,
        declared_src: String,
    ) {
        let Some(document_url) = document_url else {
            self.reject(
                tab_id,
                document_generation,
                ordinal,
                kind,
                "page document has no supported script origin",
            );
            return;
        };
        if blueice_net::canonical_http_origin(document_url).is_err() {
            self.reject(
                tab_id,
                document_generation,
                ordinal,
                kind,
                "page document has no supported script origin",
            );
            return;
        }
        let Some(authorizer) = self.external_source_authorizer.as_mut() else {
            self.reject(
                tab_id,
                document_generation,
                ordinal,
                kind,
                "external BlueTS declarations require an authorized loader",
            );
            return;
        };
        let graph = match authorizer.authorize(&PageScriptSourceRequest {
            tab_id,
            document_generation,
            ordinal,
            kind,
            document_url: document_url.to_string(),
            declared_src,
        }) {
            Ok(graph) => graph,
            Err(_) => {
                self.reject(
                    tab_id,
                    document_generation,
                    ordinal,
                    kind,
                    "external BlueTS source authorization rejected the page script",
                );
                return;
            }
        };
        let mut compiler_options = self.compiler_options.clone();
        compiler_options.resolver_fingerprint = graph.resolver_fingerprint;
        let result = self.host.execute(
            tabs,
            DirectPageScriptRequest {
                tab_id,
                kind,
                entry: graph.entry,
                loader: &graph.loader,
                compiler_options,
                feature_profile: self.feature_profile.clone(),
                supplied_manifest: &self.generated_typings.manifest,
                supplied_declaration_source: &self.generated_typings.declaration_source,
                supplied_runtime_bindings: &self.generated_typings.runtime_bindings,
            },
        );
        self.record_execution_result(tab_id, document_generation, ordinal, kind, result);
    }

    fn record_execution_result(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        ordinal: u32,
        kind: DirectPageScriptKind,
        result: Result<blueice_bluejs::Value, DirectPageScriptError>,
    ) {
        match result {
            Ok(_) => self.push_report(DirectPageScriptExecutionReport::Executed {
                tab_id: tab_id.as_u64(),
                document_generation,
                ordinal,
                kind,
            }),
            Err(error) => self.push_report(DirectPageScriptExecutionReport::Rejected {
                tab_id: tab_id.as_u64(),
                document_generation,
                ordinal,
                kind,
                message: report_error_message(error).to_string(),
            }),
        }
    }

    fn reject(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        ordinal: u32,
        kind: DirectPageScriptKind,
        message: &'static str,
    ) {
        self.push_report(DirectPageScriptExecutionReport::Rejected {
            tab_id: tab_id.as_u64(),
            document_generation,
            ordinal,
            kind,
            message: message.to_string(),
        });
    }

    fn push_report(&mut self, report: DirectPageScriptExecutionReport) {
        if self.reports.len() == MAX_EXECUTION_REPORTS {
            self.reports.pop_front();
        }
        self.reports.push_back(report);
    }
}

/// Maps detailed compiler/runtime failures to stable, source-free report
/// categories. In particular, a thrown JavaScript value and diagnostics can
/// contain page-provided data, so neither can cross this owner-observation API.
fn report_error_message(error: DirectPageScriptError) -> &'static str {
    match error {
        DirectPageScriptError::HostTypings(_) => {
            "host typing verification rejected the page script"
        }
        DirectPageScriptError::Origin(_) => "page origin rejected the page script",
        DirectPageScriptError::Bridge(BridgeError::BlueTs(_)) => {
            "BlueTS compilation rejected the page script"
        }
        DirectPageScriptError::Bridge(BridgeError::UnsupportedRuntimeTarget { .. }) => {
            "BlueTS lowering rejected the page script"
        }
        DirectPageScriptError::Bridge(BridgeError::BlueJs(_)) => {
            "BlueJS compilation rejected the page script"
        }
        DirectPageScriptError::Bridge(BridgeError::BlueJsDebug(_))
        | DirectPageScriptError::Bridge(BridgeError::ProvenanceAttachment(_))
        | DirectPageScriptError::Bridge(BridgeError::DebugAttachment(_)) => {
            "BlueJS debug attachment rejected the page script"
        }
        DirectPageScriptError::Bridge(BridgeError::PageRuntime(_)) => {
            "BlueJS page execution failed"
        }
        DirectPageScriptError::Bridge(BridgeError::InvalidSourceIdentity(_)) => {
            "page script source identity was rejected"
        }
        DirectPageScriptError::CallerSuppliedAmbientDeclarations => {
            "caller-supplied ambient declarations are not allowed"
        }
        DirectPageScriptError::TranspileOnlyPolicy => {
            "transpile-only policy is not allowed for page scripts"
        }
        DirectPageScriptError::LanguageVersionMismatch { .. } => {
            "host language version rejected the page script"
        }
        DirectPageScriptError::RuntimeBindingProfileUnavailable
        | DirectPageScriptError::BindingProfileAlreadySelected => {
            "host binding profile rejected the page script"
        }
        DirectPageScriptError::BindingContractViolation { .. } => {
            "host binding contract rejected the page script"
        }
        DirectPageScriptError::UnknownTab { .. } => "page tab is no longer available",
        DirectPageScriptError::PageHasNoUrl { .. }
        | DirectPageScriptError::InvalidPageUrl { .. } => {
            "page document has no supported script origin"
        }
        DirectPageScriptError::InlineScriptNotFound { .. } => {
            "inline page script declaration is no longer available"
        }
        DirectPageScriptError::ExternalScriptRequiresLoader { .. } => {
            "external BlueTS declarations require an authorized loader"
        }
    }
}

#[derive(Debug)]
pub enum DirectPageInlineExecutorError {
    HostTypings(HostTypingsError),
    Lifecycle(DirectPageScriptError),
    CallerSuppliedAmbientDeclarations,
    TranspileOnlyPolicy,
}

impl fmt::Display for DirectPageInlineExecutorError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::HostTypings(error) => write!(formatter, "host profile is unavailable: {error}"),
            Self::Lifecycle(error) => write!(formatter, "page-script lifecycle failed: {error}"),
            Self::CallerSuppliedAmbientDeclarations => formatter.write_str(
                "inline page executor must not accept caller-supplied ambient declarations",
            ),
            Self::TranspileOnlyPolicy => {
                formatter.write_str("inline page executor cannot use transpile-only policy")
            }
        }
    }
}

impl std::error::Error for DirectPageInlineExecutorError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::script::host_typings::{
        core_script_host_type_catalog, HostTypeSurfaceV1, CORE_SCRIPT_DOCUMENT_CONTEXT_PROFILE_V1,
        CORE_SCRIPT_DOCUMENT_TEXT_PROFILE_V1,
    };
    use crate::script::page_source_authorizer::{
        AuthorizedPageScriptGraph, PageScriptSourceAuthorizationError,
    };
    use crate::Page;
    use blueice_bluets::{
        AuthorizedModule, AuthorizedModuleLoader, AuthorizedModuleResolution, LANGUAGE_VERSION,
    };
    use std::cell::RefCell;
    use std::rc::Rc;

    fn profiles() -> HostTypeSurfaceCatalogV1 {
        HostTypeSurfaceCatalogV1::new([HostTypeSurfaceV1::new(
            LANGUAGE_VERSION,
            "inline-runner-v1",
            "inline-runner-empty-v1",
            Vec::new(),
        )])
        .unwrap()
    }

    fn executor() -> DirectPageInlineExecutor {
        DirectPageInlineExecutor::new(
            profiles(),
            "inline-runner-empty-v1",
            CompilerOptions::default(),
        )
        .unwrap()
    }

    struct StaticExternalAuthorizer {
        requests: Rc<RefCell<Vec<PageScriptSourceRequest>>>,
    }

    impl PageScriptSourceAuthorizer for StaticExternalAuthorizer {
        fn authorize(
            &mut self,
            request: &PageScriptSourceRequest,
        ) -> Result<
            super::super::page_source_authorizer::AuthorizedPageScriptGraph,
            PageScriptSourceAuthorizationError,
        > {
            self.requests.borrow_mut().push(request.clone());
            let entry = "https://example.test/assets/main.ts";
            let value = "https://example.test/assets/value.ts";
            let loader = AuthorizedModuleLoader::new(
                [
                    AuthorizedModule::new(
                        entry,
                        "import { value } from './value.ts'; export const answer: number = value + 1; answer;",
                    ),
                    AuthorizedModule::new(value, "export const value: number = 41;"),
                ],
                [AuthorizedModuleResolution::new(entry, "./value.ts", value)],
            )
            .unwrap();
            AuthorizedPageScriptGraph::new(entry, loader, "external-policy-v1")
                .map_err(|error| PageScriptSourceAuthorizationError::new(error.to_string()))
        }
    }

    #[test]
    fn executes_opted_in_inline_classic_and_module_declarations_once_in_order() {
        let mut tabs = TabManager::new(320.0, 200.0);
        let tab_id = tabs.default_tab();
        tabs.get_mut(tab_id).unwrap().load_html_str(
            concat!(
                "<script type=\"application/x-blueice-typescript\">42;</script>",
                "<script type=\"application/x-blueice-typescript-module\">43;</script>"
            ),
            Some("https://example.test/app/index.html".to_string()),
        );
        let mut executor = executor();
        executor.synchronize_and_execute(&tabs).unwrap();

        assert_eq!(executor.debug_record_count(), 2);
        assert_eq!(executor.realm_stats(tab_id).unwrap().program_count, 2);
        assert_eq!(
            executor.reports(),
            &VecDeque::from([
                DirectPageScriptExecutionReport::Executed {
                    tab_id: tab_id.as_u64(),
                    document_generation: 1,
                    ordinal: 0,
                    kind: DirectPageScriptKind::Classic,
                },
                DirectPageScriptExecutionReport::Executed {
                    tab_id: tab_id.as_u64(),
                    document_generation: 1,
                    ordinal: 1,
                    kind: DirectPageScriptKind::Module,
                },
            ])
        );

        executor.synchronize_and_execute(&tabs).unwrap();
        assert_eq!(executor.reports().len(), 2, "the document runs only once");
    }

    #[test]
    fn draining_one_tab_reports_does_not_expose_or_discard_another_tabs_reports() {
        let mut executor = executor();
        executor.reports = VecDeque::from([
            DirectPageScriptExecutionReport::Executed {
                tab_id: 1,
                document_generation: 3,
                ordinal: 0,
                kind: DirectPageScriptKind::Classic,
            },
            DirectPageScriptExecutionReport::Rejected {
                tab_id: 2,
                document_generation: 4,
                ordinal: 1,
                kind: DirectPageScriptKind::Module,
                message: "BlueTS compilation rejected the page script".to_string(),
            },
        ]);

        assert_eq!(
            executor.drain_reports_for_tab(TabId::from_u64(1)),
            vec![DirectPageScriptExecutionReport::Executed {
                tab_id: 1,
                document_generation: 3,
                ordinal: 0,
                kind: DirectPageScriptKind::Classic,
            }]
        );
        assert_eq!(
            executor.reports(),
            &VecDeque::from([DirectPageScriptExecutionReport::Rejected {
                tab_id: 2,
                document_generation: 4,
                ordinal: 1,
                kind: DirectPageScriptKind::Module,
                message: "BlueTS compilation rejected the page script".to_string(),
            }])
        );
    }

    #[test]
    fn configured_realm_limits_reject_inline_code_without_retaining_programs() {
        let mut tabs = TabManager::new(320.0, 200.0);
        let tab_id = tabs.default_tab();
        tabs.get_mut(tab_id).unwrap().load_html_str(
            "<script type=\"application/x-blueice-typescript\">42;</script>",
            Some("https://example.test/app/index.html".to_string()),
        );
        let realms = DirectPageRealmOwner::new(
            blueice_bluejs::BlueJsPageRuntimeConfig {
                max_bytecode_bytes_per_realm: 1,
                ..blueice_bluejs::BlueJsPageRuntimeConfig::default()
            },
            blueice_bluets_bluejs::DirectDebugRetentionLimits::default(),
        )
        .unwrap();
        let mut executor = DirectPageInlineExecutor::with_realm_owner(
            profiles(),
            "inline-runner-empty-v1",
            CompilerOptions::default(),
            realms,
        )
        .unwrap();

        executor.synchronize_and_execute(&tabs).unwrap();

        assert_eq!(
            executor.reports(),
            &VecDeque::from([DirectPageScriptExecutionReport::Rejected {
                tab_id: tab_id.as_u64(),
                document_generation: 1,
                ordinal: 0,
                kind: DirectPageScriptKind::Classic,
                message: "BlueJS page execution failed".to_string(),
            }])
        );
        assert_eq!(executor.debug_record_count(), 0);
        let stats = executor.realm_stats(tab_id).unwrap();
        assert_eq!(stats.program_count, 0);
        assert_eq!(stats.bytecode_bytes, 0);
    }

    #[test]
    fn inline_executor_runs_the_document_text_profile_through_page_lifecycle() {
        let mut tabs = TabManager::new(320.0, 200.0);
        let tab_id = tabs.default_tab();
        tabs.get_mut(tab_id).unwrap().load_html_str(
            concat!(
                "<main>current document</main>",
                "<script type=\"application/x-blueice-typescript\">",
                "const text: string = blueiceDocumentText(); text;",
                "</script>"
            ),
            Some("https://example.test/app/index.html".to_string()),
        );
        let mut executor = DirectPageInlineExecutor::new(
            core_script_host_type_catalog(),
            CORE_SCRIPT_DOCUMENT_TEXT_PROFILE_V1,
            CompilerOptions::default(),
        )
        .unwrap();
        executor.synchronize_and_execute(&tabs).unwrap();
        assert_eq!(
            executor.reports(),
            &VecDeque::from([DirectPageScriptExecutionReport::Executed {
                tab_id: tab_id.as_u64(),
                document_generation: 1,
                ordinal: 0,
                kind: DirectPageScriptKind::Classic,
            }])
        );
    }

    #[test]
    fn inline_executor_redacts_a_host_binding_contract_rejection() {
        let mut tabs = TabManager::new(320.0, 200.0);
        let tab_id = tabs.default_tab();
        tabs.get_mut(tab_id).unwrap().load_html_str(
            concat!(
                "<main>page-controlled oversized document text</main>",
                "<script type=\"application/x-blueice-typescript\">",
                "blueiceDocumentText();",
                "</script>"
            ),
            Some("https://example.test/app/index.html".to_string()),
        );
        let profiles = core_script_host_type_catalog();
        let host = DirectPageScriptHost::with_realm_owner_and_contract_limits(
            profiles.clone(),
            DirectPageRealmOwner::default(),
            crate::script::contracts::CoreScriptBindingContractLimits {
                document_text: blueice_bluets::ValidationLimits {
                    max_string_bytes: 8,
                    ..blueice_bluets::ValidationLimits::default()
                },
                ..crate::script::contracts::CoreScriptBindingContractLimits::default()
            },
        );
        let mut executor = DirectPageInlineExecutor::new_with_host(
            profiles,
            CORE_SCRIPT_DOCUMENT_TEXT_PROFILE_V1,
            CompilerOptions::default(),
            None,
            host,
        )
        .unwrap();

        executor.synchronize_and_execute(&tabs).unwrap();

        assert_eq!(
            executor.reports(),
            &VecDeque::from([DirectPageScriptExecutionReport::Rejected {
                tab_id: tab_id.as_u64(),
                document_generation: 1,
                ordinal: 0,
                kind: DirectPageScriptKind::Classic,
                message: "host binding contract rejected the page script".to_string(),
            }])
        );
        assert_eq!(executor.debug_record_count(), 0);
        let stats = executor.realm_stats(tab_id).unwrap();
        assert_eq!(stats.program_count, 0);
        assert_eq!(stats.bytecode_bytes, 0);
    }

    #[test]
    fn inline_executor_runs_the_document_context_profile_through_page_lifecycle() {
        let mut tabs = TabManager::new(320.0, 200.0);
        let tab_id = tabs.default_tab();
        tabs.get_mut(tab_id).unwrap().load_html_str(
            concat!(
                "<main>current context</main>",
                "<script type=\"application/x-blueice-typescript\">",
                "const origin: string = blueiceDocumentOrigin(); ",
                "const text: string = blueiceDocumentText(); origin + text;",
                "</script>"
            ),
            Some("https://example.test/app/index.html".to_string()),
        );
        let mut executor = DirectPageInlineExecutor::new(
            core_script_host_type_catalog(),
            CORE_SCRIPT_DOCUMENT_CONTEXT_PROFILE_V1,
            CompilerOptions::default(),
        )
        .unwrap();
        executor.synchronize_and_execute(&tabs).unwrap();

        assert_eq!(
            executor.reports(),
            &VecDeque::from([DirectPageScriptExecutionReport::Executed {
                tab_id: tab_id.as_u64(),
                document_generation: 1,
                ordinal: 0,
                kind: DirectPageScriptKind::Classic,
            }])
        );
    }

    #[test]
    fn rejects_external_source_without_reflecting_its_page_controlled_url() {
        let mut tabs = TabManager::new(320.0, 200.0);
        let tab_id = tabs.default_tab();
        tabs.get_mut(tab_id).unwrap().load_html_str(
            "<script type=\"application/x-blueice-typescript\" src=\"https://untrusted.test/a.ts\"></script>",
            Some("https://example.test/app/index.html".to_string()),
        );
        let mut executor = executor();
        executor.synchronize_and_execute(&tabs).unwrap();

        assert_eq!(executor.debug_record_count(), 0);
        let Some(DirectPageScriptExecutionReport::Rejected { message, .. }) =
            executor.reports().front()
        else {
            panic!("the external declaration must produce one rejection")
        };
        assert_eq!(
            message,
            "external BlueTS declarations require an authorized loader"
        );
        assert!(!message.contains("untrusted"));
    }

    #[test]
    fn executes_an_external_module_only_from_a_core_authorized_closed_graph() {
        let mut tabs = TabManager::new(320.0, 200.0);
        let tab_id = tabs.default_tab();
        tabs.get_mut(tab_id).unwrap().load_html_str(
            "<script type=\"application/x-blueice-typescript-module\" src=\"/assets/main.ts\"></script>",
            Some("https://example.test/app/index.html".to_string()),
        );
        let requests = Rc::new(RefCell::new(Vec::new()));
        let mut executor = DirectPageInlineExecutor::with_external_source_authorizer(
            profiles(),
            "inline-runner-empty-v1",
            CompilerOptions::default(),
            StaticExternalAuthorizer {
                requests: Rc::clone(&requests),
            },
        )
        .unwrap();
        executor.synchronize_and_execute(&tabs).unwrap();

        assert_eq!(executor.debug_record_count(), 2);
        assert!(matches!(
            executor.reports().front(),
            Some(DirectPageScriptExecutionReport::Executed {
                ordinal: 0,
                kind: DirectPageScriptKind::Module,
                ..
            })
        ));
        assert_eq!(
            requests.borrow().as_slice(),
            &[PageScriptSourceRequest {
                tab_id,
                document_generation: 1,
                ordinal: 0,
                kind: DirectPageScriptKind::Module,
                document_url: "https://example.test/app/index.html".to_string(),
                declared_src: "/assets/main.ts".to_string(),
            }]
        );
    }

    #[test]
    fn does_not_reflect_a_thrown_page_value_into_an_execution_report() {
        let mut tabs = TabManager::new(320.0, 200.0);
        let tab_id = tabs.default_tab();
        tabs.get_mut(tab_id).unwrap().load_html_str(
            "<script type=\"application/x-blueice-typescript\">throw \"page secret\";</script>",
            Some("https://example.test/app/index.html".to_string()),
        );
        let mut executor = executor();
        executor.synchronize_and_execute(&tabs).unwrap();

        let Some(DirectPageScriptExecutionReport::Rejected { message, .. }) =
            executor.reports().front()
        else {
            panic!("the throwing declaration must produce one rejection")
        };
        assert_eq!(message, "BlueTS lowering rejected the page script");
        assert!(!message.contains("page secret"));
    }

    #[test]
    fn a_document_replacement_executes_its_new_declarations_and_discards_old_metadata() {
        let mut tabs = TabManager::new(320.0, 200.0);
        let tab_id = tabs.default_tab();
        let page: &mut Page = tabs.get_mut(tab_id).unwrap();
        page.load_html_str(
            "<script type=\"application/x-blueice-typescript\">42;</script>",
            Some("https://example.test/app/first.html".to_string()),
        );
        let mut executor = executor();
        executor.synchronize_and_execute(&tabs).unwrap();
        assert_eq!(executor.debug_record_count(), 1);

        tabs.get_mut(tab_id).unwrap().load_html_str(
            "<script type=\"application/x-blueice-typescript\">43;</script>",
            Some("https://example.test/app/next.html".to_string()),
        );
        executor.synchronize_and_execute(&tabs).unwrap();
        assert_eq!(executor.debug_record_count(), 1);
        assert_eq!(executor.reports().len(), 2);
    }
}
