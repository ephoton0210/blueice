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
        inline_module_id, DirectInlinePageScriptRequest, DirectPageScriptError,
        DirectPageScriptHost, DirectPageScriptKind, DirectPageScriptRequest,
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
        policy: RuntimePolicy,
        source_position: Option<InlineBlueTsSourcePosition>,
    },
    Rejected {
        tab_id: u64,
        document_generation: u64,
        ordinal: u32,
        kind: DirectPageScriptKind,
        policy: RuntimePolicy,
        source_position: Option<InlineBlueTsSourcePosition>,
        message: String,
    },
}

/// Verified half-open byte range in the original inline BlueTS source.
/// Module identity and source text are discarded before a report is retained.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InlineBlueTsSourcePosition {
    pub start: u32,
    pub end: u32,
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
        self.observed_documents.retain(|tab_id, generation| {
            let keep = tabs.get(*tab_id).is_some();
            if !keep {
                if let Some(authorizer) = self.external_source_authorizer.as_mut() {
                    authorizer.release_document(*tab_id, *generation);
                }
            }
            keep
        });

        let tab_ids: Vec<_> = tabs.ids().collect();
        for tab_id in tab_ids {
            let Some(page) = tabs.get(tab_id) else {
                continue;
            };
            let document_generation = page.document_generation();
            if self.observed_documents.get(&tab_id) == Some(&document_generation) {
                continue;
            }
            if let Some(previous_generation) = self.observed_documents.get(&tab_id).copied() {
                if let Some(authorizer) = self.external_source_authorizer.as_mut() {
                    authorizer.release_document(tab_id, previous_generation);
                }
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

    /// Retained verified external source for one observed document.
    pub fn retained_external_source_payload_bytes(
        &self,
        tab_id: TabId,
        document_generation: u64,
    ) -> Option<usize> {
        if self.observed_documents.get(&tab_id) != Some(&document_generation) {
            return None;
        }
        self.external_source_authorizer
            .as_ref()
            .map_or(Some(0), |authorizer| {
                authorizer.retained_source_payload_bytes(tab_id, document_generation)
            })
    }

    /// Counts direct-realm debug metadata and retained verified source for
    /// one observed document; bytecode and VM heap remain separate realm use.
    pub fn retained_page_cache_payload_bytes(
        &self,
        tab_id: TabId,
        document_generation: u64,
    ) -> Result<Option<usize>, DirectPageInlineExecutorError> {
        let Some(source_bytes) =
            self.retained_external_source_payload_bytes(tab_id, document_generation)
        else {
            return Ok(None);
        };
        let debug_bytes = self
            .host
            .retained_debug_payload_bytes_for_document(tab_id, document_generation)
            .map_err(DirectPageInlineExecutorError::Lifecycle)?;
        Ok(debug_bytes.and_then(|debug_bytes| debug_bytes.checked_add(source_bytes)))
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
        match declaration {
            BlueTsPageScriptDeclaration::Inline {
                ordinal,
                kind,
                source,
            } => self.execute_inline_declaration(
                tabs,
                tab_id,
                document_generation,
                ordinal,
                kind,
                &source,
            ),
            BlueTsPageScriptDeclaration::External { ordinal, kind, src } => self
                .execute_external_declaration(
                    tabs,
                    tab_id,
                    document_generation,
                    ordinal,
                    kind,
                    document_url,
                    src,
                ),
        }
    }

    fn execute_inline_declaration(
        &mut self,
        tabs: &TabManager,
        tab_id: TabId,
        document_generation: u64,
        ordinal: u32,
        kind: DirectPageScriptKind,
        source: &str,
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
        let module_id = inline_module_id(tab_id, document_generation, ordinal);
        self.record_execution_result(
            tab_id,
            document_generation,
            ordinal,
            kind,
            result,
            Some((&module_id, source)),
        );
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
        self.record_execution_result(tab_id, document_generation, ordinal, kind, result, None);
    }

    fn record_execution_result(
        &mut self,
        tab_id: TabId,
        document_generation: u64,
        ordinal: u32,
        kind: DirectPageScriptKind,
        result: Result<blueice_bluejs::Value, DirectPageScriptError>,
        inline_source: Option<(&str, &str)>,
    ) {
        match result {
            Ok(_) => self.push_report(DirectPageScriptExecutionReport::Executed {
                tab_id: tab_id.as_u64(),
                document_generation,
                ordinal,
                kind,
                policy: self.compiler_options.runtime_policy,
                source_position: None,
            }),
            Err(error) => {
                let source_position = inline_source.and_then(|(module_id, source)| {
                    source_position_from_error(&error, module_id, source)
                });
                self.push_report(DirectPageScriptExecutionReport::Rejected {
                    tab_id: tab_id.as_u64(),
                    document_generation,
                    ordinal,
                    kind,
                    policy: self.compiler_options.runtime_policy,
                    source_position,
                    message: report_error_message(error).to_string(),
                });
            }
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
            policy: self.compiler_options.runtime_policy,
            source_position: None,
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

fn source_position_from_error(
    error: &DirectPageScriptError,
    inline_module_id: &str,
    source: &str,
) -> Option<InlineBlueTsSourcePosition> {
    let span = match error {
        DirectPageScriptError::Bridge(BridgeError::BlueTs(diagnostics)) => diagnostics
            .iter()
            .find(|diagnostic| diagnostic.span.module == inline_module_id)
            .map(|diagnostic| &diagnostic.span)?,
        DirectPageScriptError::Bridge(BridgeError::UnsupportedRuntimeTarget { span, .. }) => span,
        _ => return None,
    };
    if span.module != inline_module_id
        || span.start >= span.end
        || span.end > source.len()
        || !source.is_char_boundary(span.start)
        || !source.is_char_boundary(span.end)
    {
        return None;
    }
    Some(InlineBlueTsSourcePosition {
        start: u32::try_from(span.start).ok()?,
        end: u32::try_from(span.end).ok()?,
    })
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
        DirectPageScriptError::StrictRuntimeBoundary(diagnostic) => match diagnostic {
            super::contracts::StrictRuntimeBoundaryDiagnostic::MissingContract { .. } => {
                "strict-runtime missing contract rejected the page script"
            }
            super::contracts::StrictRuntimeBoundaryDiagnostic::UnreifiableType { .. } => {
                "strict-runtime unreifiable type rejected the page script"
            }
            super::contracts::StrictRuntimeBoundaryDiagnostic::UncheckedBoundary { .. } => {
                "strict-runtime unchecked boundary rejected the page script"
            }
        },
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
mod tests;
