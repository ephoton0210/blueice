// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Explicitly configured execution of inline BlueTS page declarations.
//!
//! This is a core-owned page-pipeline seam, not a replacement module loader.
//! It observes parsed, opt-in declarations on a live [`crate::Page`], uses
//! the selected host profile that it generated itself, and executes each
//! inline declaration at most once for a tab/document generation. External
//! declarations remain rejected until an authorized source/resolver transport
//! exists. The default session loop does not construct this type.

use super::{
    direct_page::{
        DirectInlinePageScriptRequest, DirectPageScriptError, DirectPageScriptHost,
        DirectPageScriptKind,
    },
    host_typings::{GeneratedHostTypingsV1, HostTypeSurfaceCatalogV1, HostTypingsError},
    BlueTsPageScriptDeclaration,
};
use crate::{TabId, TabManager};
use blueice_bluets::{CompilerOptions, RuntimePolicy};
use blueice_bluets_bluejs::BridgeError;
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
            host: DirectPageScriptHost::new(profiles),
            compiler_options,
            feature_profile,
            generated_typings,
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
            // Mark before execution, so a rejected document cannot be retried
            // on every unrelated frontend/session event.
            self.observed_documents.insert(tab_id, document_generation);
            for declaration in declarations {
                self.execute_declaration(tabs, tab_id, document_generation, declaration);
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

    /// Exposes only the count of retained static debug records for lifecycle
    /// regression tests. Static metadata remains owned by the live realm.
    pub fn debug_record_count(&self) -> usize {
        self.host.debug_record_count()
    }

    fn execute_declaration(
        &mut self,
        tabs: &TabManager,
        tab_id: TabId,
        document_generation: u64,
        declaration: BlueTsPageScriptDeclaration,
    ) {
        let (ordinal, kind) = match &declaration {
            BlueTsPageScriptDeclaration::Inline { ordinal, kind, .. }
            | BlueTsPageScriptDeclaration::External { ordinal, kind, .. } => (*ordinal, *kind),
        };
        if matches!(declaration, BlueTsPageScriptDeclaration::External { .. }) {
            self.push_report(DirectPageScriptExecutionReport::Rejected {
                tab_id: tab_id.as_u64(),
                document_generation,
                ordinal,
                kind,
                message: "external BlueTS declarations require an authorized loader".to_string(),
            });
            return;
        }

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
    use crate::script::host_typings::HostTypeSurfaceV1;
    use crate::Page;
    use blueice_bluets::LANGUAGE_VERSION;

    fn executor() -> DirectPageInlineExecutor {
        let profiles = HostTypeSurfaceCatalogV1::new([HostTypeSurfaceV1::new(
            LANGUAGE_VERSION,
            "inline-runner-v1",
            "inline-runner-empty-v1",
            Vec::new(),
        )])
        .unwrap();
        DirectPageInlineExecutor::new(
            profiles,
            "inline-runner-empty-v1",
            CompilerOptions::default(),
        )
        .unwrap()
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
