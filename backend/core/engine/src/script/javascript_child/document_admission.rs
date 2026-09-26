// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

pub(super) fn authorized_document(
    tab_id: TabId,
    identity: &LiveDocument,
    snapshot: PageHostDocumentSnapshot,
    document_url: &str,
    declarations: Vec<CombinedPageScriptDeclaration>,
    external_source_authorizer: Option<&dyn OutOfProcessPageScriptSourceAuthorizer>,
    debugger_execution_control: bool,
) -> (
    PageHostDocument,
    Vec<JavaScriptPageExecutionReport>,
    Vec<BlueTsPageExecutionReport>,
) {
    let mut scripts = Vec::new();
    let mut reports = Vec::new();
    let mut blue_ts_reports = Vec::new();
    for declaration in declarations {
        match declaration {
            CombinedPageScriptDeclaration::Inline {
                ordinal,
                language,
                source,
            } => {
                let module_id =
                    inline_module_id(tab_id, identity.document_generation, ordinal, language);
                scripts.push(PageHostScript {
                    ordinal,
                    language: child_language(language),
                    kind: child_kind(language),
                    graph: PageHostModuleGraph {
                        entry: module_id.clone(),
                        modules: vec![PageHostSource::new(module_id, source)],
                        resolutions: Vec::new(),
                        resolver_fingerprint: INLINE_CHILD_RESOLVER_FINGERPRINT.to_string(),
                    },
                });
            }
            CombinedPageScriptDeclaration::External {
                ordinal,
                language,
                src,
            } => match authorize_external_graph(
                tab_id,
                identity.document_generation,
                ordinal,
                language,
                document_url,
                src,
                external_source_authorizer,
            ) {
                Ok(graph) => scripts.push(PageHostScript {
                    ordinal,
                    language: child_language(language),
                    kind: child_kind(language),
                    graph,
                }),
                Err(category) => match language {
                    CombinedPageScriptLanguage::JavaScript(kind) => reports.push(rejected_report(
                        tab_id,
                        identity.document_generation,
                        ordinal,
                        kind,
                        category,
                    )),
                    CombinedPageScriptLanguage::BlueTs(kind) => {
                        blue_ts_reports.push(rejected_blue_ts_report(
                            tab_id,
                            identity.document_generation,
                            ordinal,
                            kind,
                            category,
                        ))
                    }
                },
            },
        }
    }
    (
        PageHostDocument {
            tab_id: tab_id.as_u64(),
            document_generation: identity.document_generation,
            snapshot,
            debugger_execution_control,
            scripts,
        },
        reports,
        blue_ts_reports,
    )
}

/// Obtains a graph only through the startup-selected core authorizer and
/// converts the typed result into the private child wire record. This adapter
/// deliberately has no fallback graph, URL normalization, import-map lookup,
/// cache lookup, network operation, or filesystem operation of its own.
#[allow(clippy::too_many_arguments)]
pub(super) fn authorize_external_graph(
    tab_id: TabId,
    document_generation: u64,
    ordinal: u32,
    language: CombinedPageScriptLanguage,
    document_url: &str,
    declared_src: String,
    authorizer: Option<&dyn OutOfProcessPageScriptSourceAuthorizer>,
) -> Result<PageHostModuleGraph, &'static str> {
    let Some(authorizer) = authorizer else {
        return Err(external_loader_required_category(language));
    };
    let graph = authorizer
        .authorize(&OutOfProcessPageScriptSourceRequest {
            tab_id,
            document_generation,
            ordinal,
            language,
            document_url: document_url.to_string(),
            declared_src,
        })
        .map_err(|_| external_authorization_rejected_category(language))?;
    let graph = match (language, graph) {
        (
            CombinedPageScriptLanguage::JavaScript(_),
            AuthorizedOutOfProcessPageScriptGraph::JavaScript(graph),
        ) => page_host_graph_from_javascript(graph),
        (
            CombinedPageScriptLanguage::BlueTs(_),
            AuthorizedOutOfProcessPageScriptGraph::BlueTs(graph),
        ) => page_host_graph_from_bluets(graph),
        _ => return Err(external_authorization_rejected_category(language)),
    };
    if !valid_page_host_graph(&graph) {
        return Err(external_authorization_rejected_category(language));
    }
    Ok(graph)
}

pub(super) fn external_loader_required_category(
    language: CombinedPageScriptLanguage,
) -> &'static str {
    match language {
        CombinedPageScriptLanguage::JavaScript(_) => {
            "external JavaScript declarations require an authorized loader"
        }
        CombinedPageScriptLanguage::BlueTs(_) => {
            "external BlueTS declarations require an authorized loader"
        }
    }
}

pub(super) fn external_authorization_rejected_category(
    language: CombinedPageScriptLanguage,
) -> &'static str {
    match language {
        CombinedPageScriptLanguage::JavaScript(_) => {
            "external JavaScript source authorization rejected the page script"
        }
        CombinedPageScriptLanguage::BlueTs(_) => {
            "external BlueTS source authorization rejected the page script"
        }
    }
}

/// Copies the existing typed JavaScript graph without changing any selected
/// canonical module ID, static edge, or resolver-policy fingerprint.
pub(super) fn page_host_graph_from_javascript(
    graph: AuthorizedJavaScriptModuleGraph,
) -> PageHostModuleGraph {
    PageHostModuleGraph {
        entry: graph.entry().to_string(),
        modules: graph
            .authorized_modules()
            .map(|module| PageHostSource::new(module.canonical_module_id(), module.source()))
            .collect(),
        resolutions: graph
            .authorized_resolutions()
            .map(|resolution| PageHostStaticResolution {
                from_module: resolution.from_module,
                specifier: resolution.specifier,
                canonical_target: resolution.canonical_target,
            })
            .collect(),
        resolver_fingerprint: graph.resolver_fingerprint().to_string(),
    }
}

/// Copies the existing typed BlueTS graph without exposing its loader as an
/// ambient resolver to either the child or the page.
pub(super) fn page_host_graph_from_bluets(graph: AuthorizedPageScriptGraph) -> PageHostModuleGraph {
    let AuthorizedPageScriptGraph {
        entry,
        loader,
        resolver_fingerprint,
    } = graph;
    PageHostModuleGraph {
        entry,
        modules: loader
            .authorized_modules()
            .map(|(module_id, source)| PageHostSource::new(module_id, source))
            .collect(),
        resolutions: loader
            .authorized_resolutions()
            .map(
                |(from_module, specifier, canonical_target)| PageHostStaticResolution {
                    from_module: from_module.to_string(),
                    specifier: specifier.to_string(),
                    canonical_target: canonical_target.to_string(),
                },
            )
            .collect(),
        resolver_fingerprint,
    }
}

/// Rejects malformed typed-graph conversions before the core sends a document
/// to the child. The child repeats independent graph, hash, budget, syntax,
/// and static-edge validation after the protocol boundary, so a malformed or
/// tampered wire record cannot acquire an implicit fallback resolver.
pub(super) fn valid_page_host_graph(graph: &PageHostModuleGraph) -> bool {
    if graph.entry.is_empty()
        || graph.entry.contains('\0')
        || graph.resolver_fingerprint.trim().is_empty()
        || graph.resolver_fingerprint.contains('\0')
    {
        return false;
    }
    let mut modules = BTreeSet::new();
    for source in &graph.modules {
        if source.canonical_module_id.is_empty()
            || source.canonical_module_id.contains('\0')
            || source.source_hash != page_host::source_hash(&source.source)
            || !modules.insert(source.canonical_module_id.as_str())
        {
            return false;
        }
    }
    if !modules.contains(graph.entry.as_str()) {
        return false;
    }
    let mut resolutions = BTreeSet::new();
    graph.resolutions.iter().all(|resolution| {
        !resolution.from_module.is_empty()
            && !resolution.specifier.is_empty()
            && !resolution.canonical_target.is_empty()
            && !resolution.from_module.contains('\0')
            && !resolution.specifier.contains('\0')
            && !resolution.canonical_target.contains('\0')
            && modules.contains(resolution.from_module.as_str())
            && modules.contains(resolution.canonical_target.as_str())
            && resolutions.insert((
                resolution.from_module.as_str(),
                resolution.specifier.as_str(),
            ))
    })
}

/// Builds the only document values that the core may serialize as child
/// bindings. The page neither selects their names/profile nor provides a
/// capability token. The pure contracts match the child protocol's fixed
/// byte budgets, while `live_page_identity` already derives the tuple origin
/// from an admitted HTTP(S) document rather than page script input.
pub(super) fn core_document_snapshot(
    page: &Page,
    identity: &LiveDocument,
) -> Result<PageHostDocumentSnapshot, ()> {
    let document_text = page.script_document_text_content();
    let document_origin = identity.origin.clone();
    let limits = CoreScriptBindingContractLimits::default();
    core_script_binding_contract("dom.document-text")
        .expect("the fixed document-text binding has a contract inventory entry")
        .validate_string(&document_text, limits.document_text)
        .map_err(|_| ())?;
    core_script_binding_contract("dom.document-origin")
        .expect("the fixed document-origin binding has a contract inventory entry")
        .validate_string(&document_origin, limits.document_origin)
        .map_err(|_| ())?;
    if canonical_http_origin(&document_origin).ok().as_deref() != Some(document_origin.as_str()) {
        return Err(());
    }
    Ok(PageHostDocumentSnapshot {
        document_text,
        document_origin,
    })
}

pub(super) fn binding_contract_rejections(
    tab_id: TabId,
    document_generation: u64,
    declarations: Vec<CombinedPageScriptDeclaration>,
) -> (
    Vec<JavaScriptPageExecutionReport>,
    Vec<BlueTsPageExecutionReport>,
) {
    let mut java_script_reports = Vec::new();
    let mut blue_ts_reports = Vec::new();
    for declaration in declarations {
        match declaration {
            CombinedPageScriptDeclaration::Inline {
                ordinal, language, ..
            }
            | CombinedPageScriptDeclaration::External {
                ordinal, language, ..
            } => match language {
                CombinedPageScriptLanguage::JavaScript(kind) => {
                    java_script_reports.push(rejected_report(
                        tab_id,
                        document_generation,
                        ordinal,
                        kind,
                        "host binding contract rejected the page script",
                    ))
                }
                CombinedPageScriptLanguage::BlueTs(kind) => {
                    blue_ts_reports.push(rejected_blue_ts_report(
                        tab_id,
                        document_generation,
                        ordinal,
                        kind,
                        "host binding contract rejected the page script",
                    ))
                }
            },
        }
    }
    (java_script_reports, blue_ts_reports)
}

pub(super) fn live_page_identity(page: &Page) -> Option<LiveDocument> {
    let origin = canonical_http_origin(page.url()?).ok()?;
    Some(LiveDocument {
        document_generation: page.document_generation(),
        origin,
    })
}

pub(super) fn inline_module_id(
    tab_id: TabId,
    document_generation: u64,
    ordinal: u32,
    language: CombinedPageScriptLanguage,
) -> String {
    let extension = match language {
        CombinedPageScriptLanguage::JavaScript(_) => "js",
        CombinedPageScriptLanguage::BlueTs(_) => "ts",
    };
    format!(
        "blueice://page/tab-{}/document-{document_generation}/inline-{ordinal}.{extension}",
        tab_id.as_u64()
    )
}
