// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

pub(super) enum PreparedScript {
    Rejected {
        ordinal: u32,
        language: PageHostScriptLanguage,
        kind: PageHostScriptKind,
        category: &'static str,
    },
    JavaScriptClassic {
        ordinal: u32,
        source: PageHostSource,
        program: BlueJsProgramV1,
    },
    JavaScriptModule {
        ordinal: u32,
        graph: PageHostModuleGraph,
        programs: BTreeMap<String, BlueJsProgramV1>,
    },
    BlueTsClassic {
        ordinal: u32,
        script: Box<DirectScript>,
    },
    BlueTsModule {
        ordinal: u32,
        graph: Box<DirectModuleGraph>,
    },
}

pub(super) fn prepare_script(
    script: PageHostScript,
    page_dom_profile: PageDomProfile,
) -> PreparedScript {
    let ordinal = script.ordinal;
    let language = script.language;
    let kind = script.kind;
    let prepared = match (language, kind) {
        (PageHostScriptLanguage::JavaScript, PageHostScriptKind::Classic) => {
            prepare_classic(script.graph).map(|(source, program)| {
                PreparedScript::JavaScriptClassic {
                    ordinal,
                    source,
                    program,
                }
            })
        }
        (PageHostScriptLanguage::JavaScript, PageHostScriptKind::Module) => {
            prepare_module_graph(script.graph).map(|(graph, programs)| {
                PreparedScript::JavaScriptModule {
                    ordinal,
                    graph,
                    programs,
                }
            })
        }
        (PageHostScriptLanguage::BlueTs, PageHostScriptKind::Classic) => {
            prepare_bluets_classic(script.graph, page_dom_profile).map(|script| {
                PreparedScript::BlueTsClassic {
                    ordinal,
                    script: Box::new(script),
                }
            })
        }
        (PageHostScriptLanguage::BlueTs, PageHostScriptKind::Module) => {
            prepare_bluets_module_graph(script.graph, page_dom_profile).map(|graph| {
                PreparedScript::BlueTsModule {
                    ordinal,
                    graph: Box::new(graph),
                }
            })
        }
    };
    prepared.unwrap_or_else(|category| PreparedScript::Rejected {
        ordinal,
        language,
        kind,
        category,
    })
}

pub(super) fn prepare_classic(
    graph: PageHostModuleGraph,
) -> Result<(PageHostSource, BlueJsProgramV1), &'static str> {
    let modules = validate_graph(&graph)?;
    if modules.len() != 1 || !graph.resolutions.is_empty() {
        return Err("classic JavaScript source graph is not closed");
    }
    let source = modules
        .get(&graph.entry)
        .cloned()
        .ok_or("authorized JavaScript graph has no entry")?;
    let program = BlueJsProgramV1::Script(parse(&source.source).map_err(parse_category)?);
    program.compile().map_err(compile_category)?;
    Ok((source, program))
}

pub(super) fn prepare_module_graph(
    graph: PageHostModuleGraph,
) -> Result<(PageHostModuleGraph, BTreeMap<String, BlueJsProgramV1>), &'static str> {
    let modules = validate_graph(&graph)?;
    let resolutions = validate_resolutions(&graph, &modules)?;
    let mut programs = BTreeMap::new();
    for (module_id, source) in &modules {
        let mut module = parse_module(&source.source).map_err(parse_category)?;
        rewrite_static_module_requests(module_id, &mut module, &resolutions)?;
        let program = BlueJsProgramV1::Module(module);
        // Preflight the full graph before the first program is admitted.
        program.compile().map_err(compile_category)?;
        programs.insert(module_id.clone(), program);
    }
    Ok((graph, programs))
}

/// Prepares an explicit BlueTS classic declaration through the same closed
/// caller-authorized graph validation used for JavaScript. The compiler sees
/// no filesystem, URL, import-map, page-selected profile, or callback
/// authority. Its only ambient declaration comes from the exact generated
/// owner-selected profile: copied snapshots by default, or bounded live DOM
/// text/mutation when the launcher granted that child capability.
pub(super) fn prepare_bluets_classic(
    graph: PageHostModuleGraph,
    page_dom_profile: PageDomProfile,
) -> Result<DirectScript, &'static str> {
    let modules = validate_graph(&graph)?;
    if modules.len() != 1 || !graph.resolutions.is_empty() {
        return Err("classic BlueTS source graph is not closed");
    }
    let loader = bluets_loader(&graph, modules)?;
    compile_direct_script(
        &graph.entry,
        &loader,
        bluets_compiler_options(&graph, page_dom_profile)?,
    )
    .map_err(bluets_bridge_category)
}

/// Prepares a complete explicit BlueTS module graph without giving BlueTS a
/// resolver beyond the exact static edges serialized by its caller.
pub(super) fn prepare_bluets_module_graph(
    graph: PageHostModuleGraph,
    page_dom_profile: PageDomProfile,
) -> Result<DirectModuleGraph, &'static str> {
    let modules = validate_graph(&graph)?;
    validate_resolutions(&graph, &modules)?;
    let loader = bluets_loader(&graph, modules)?;
    compile_direct_module_graph(
        &graph.entry,
        &loader,
        bluets_compiler_options(&graph, page_dom_profile)?,
    )
    .map_err(bluets_bridge_category)
}

pub(super) fn bluets_loader(
    graph: &PageHostModuleGraph,
    modules: BTreeMap<String, PageHostSource>,
) -> Result<AuthorizedModuleLoader, &'static str> {
    let resolutions = graph.resolutions.iter().map(|resolution| {
        AuthorizedModuleResolution::new(
            resolution.from_module.clone(),
            resolution.specifier.clone(),
            resolution.canonical_target.clone(),
        )
    });
    AuthorizedModuleLoader::new(
        modules
            .into_values()
            .map(|source| AuthorizedModule::new(source.canonical_module_id, source.source)),
        resolutions,
    )
    .map_err(|_| "authorized BlueTS graph is invalid")
}

pub(super) fn bluets_compiler_options(
    graph: &PageHostModuleGraph,
    page_dom_profile: PageDomProfile,
) -> Result<CompilerOptions, &'static str> {
    let ambient_declaration = match page_dom_profile {
        PageDomProfile::Snapshot => PageHostDocumentTypingsV1::generate()
            .verified_ambient_module(&page_host_document_runtime_bindings_v1()),
        PageDomProfile::Text => PageHostDocumentTypingsV1::generate_dom_text()
            .verified_dom_text_ambient_module(&page_host_dom_text_runtime_bindings_v1()),
        PageDomProfile::Mutation => PageHostDocumentTypingsV1::generate_dom_mutation()
            .verified_dom_mutation_ambient_module(&page_host_dom_mutation_runtime_bindings_v1()),
        PageDomProfile::Event => PageHostDocumentTypingsV1::generate_dom_event()
            .verified_dom_event_ambient_module(&page_host_dom_event_runtime_bindings_v1()),
    }
    .map_err(|_| "verified page-host BlueTS typings are unavailable")?;
    let mut options = CompilerOptions {
        runtime_policy: RuntimePolicy::Checked,
        resolver_fingerprint: graph.resolver_fingerprint.clone(),
        require_declared_global_calls: true,
        ambient_declaration_modules: vec![ambient_declaration],
        ..CompilerOptions::default()
    };
    // The transport and BlueJS child already use the smaller page-host source
    // limits. Carry them into BlueTS too so a direct compilation cannot do
    // substantially more work than the closed graph the child admitted.
    options.limits.max_modules = MAX_MODULES_PER_GRAPH;
    options.limits.max_total_source_bytes = MAX_SOURCE_BYTES_PER_DOCUMENT;
    Ok(options)
}

pub(super) fn execute_bluets_classic(
    runtime: &mut BlueJsPageRuntime,
    debug_registry: &mut DirectDebugRegistry,
    tab_id: u64,
    origin: &BlueJsPageOrigin,
    script: &DirectScript,
) -> PageHostScriptOutcome {
    let attachment =
        match script.attach_debug_in_page_realm(runtime, tab_id, origin, debug_registry) {
            Ok(attachment) => attachment,
            Err(error) => return rejected(bluets_bridge_category(error)),
        };
    match runtime
        .execute_program(tab_id, attachment.handle)
        .map(|_: Value| ())
    {
        Ok(()) => PageHostScriptOutcome::Executed,
        Err(error) => rejected(page_runtime_category(error)),
    }
}

pub(super) fn execute_bluets_module_graph(
    runtime: &mut BlueJsPageRuntime,
    debug_registry: &mut DirectDebugRegistry,
    tab_id: u64,
    origin: &BlueJsPageOrigin,
    graph: &DirectModuleGraph,
) -> PageHostScriptOutcome {
    let attachment = match graph.attach_debug_in_page_realm(runtime, tab_id, origin, debug_registry)
    {
        Ok(attachment) => attachment,
        Err(error) => return rejected(bluets_bridge_category(error)),
    };
    match runtime.execute_module_graph(
        tab_id,
        attachment.entry.handle,
        attachment.modules.values().map(|module| module.handle),
    ) {
        Ok(_) => PageHostScriptOutcome::Executed,
        Err(error) => rejected(page_runtime_category(error)),
    }
}

pub(super) fn validate_graph(
    graph: &PageHostModuleGraph,
) -> Result<BTreeMap<String, PageHostSource>, &'static str> {
    if graph.entry.is_empty()
        || graph.entry.contains('\0')
        || graph.resolver_fingerprint.trim().is_empty()
        || graph.resolver_fingerprint.contains('\0')
    {
        return Err("authorized JavaScript graph is invalid");
    }
    if graph.modules.len() > MAX_MODULES_PER_GRAPH {
        return Err("JavaScript module graph exceeds configured policy");
    }
    let mut modules = BTreeMap::new();
    for source in &graph.modules {
        if source.canonical_module_id.is_empty()
            || source.canonical_module_id.contains('\0')
            || source.source.len() > MAX_SOURCE_BYTES_PER_MODULE
            || source.source_hash != page_host::source_hash(&source.source)
        {
            return Err("authorized JavaScript source record is invalid");
        }
        if modules
            .insert(source.canonical_module_id.clone(), source.clone())
            .is_some()
        {
            return Err("authorized JavaScript graph has duplicate modules");
        }
    }
    if !modules.contains_key(&graph.entry) {
        return Err("authorized JavaScript graph has no entry");
    }
    Ok(modules)
}

pub(super) fn validate_resolutions(
    graph: &PageHostModuleGraph,
    modules: &BTreeMap<String, PageHostSource>,
) -> Result<BTreeMap<(String, String), String>, &'static str> {
    let mut resolutions = BTreeMap::new();
    for PageHostStaticResolution {
        from_module,
        specifier,
        canonical_target,
    } in &graph.resolutions
    {
        if from_module.is_empty()
            || specifier.is_empty()
            || canonical_target.is_empty()
            || from_module.contains('\0')
            || specifier.contains('\0')
            || canonical_target.contains('\0')
            || !modules.contains_key(from_module)
            || !modules.contains_key(canonical_target)
        {
            return Err("authorized JavaScript resolution record is invalid");
        }
        if resolutions
            .insert(
                (from_module.clone(), specifier.clone()),
                canonical_target.clone(),
            )
            .is_some()
        {
            return Err("authorized JavaScript graph has duplicate static resolutions");
        }
    }
    Ok(resolutions)
}

pub(super) fn rewrite_static_module_requests(
    module_id: &str,
    module: &mut Module,
    resolutions: &BTreeMap<(String, String), String>,
) -> Result<(), &'static str> {
    let resolve = |specifier: &str| {
        resolutions
            .get(&(module_id.to_string(), specifier.to_string()))
            .cloned()
            .ok_or("authorized JavaScript graph is missing a static resolution")
    };
    for import in &mut module.imports {
        import.module_request = resolve(&import.module_request)?;
    }
    for export in &mut module.exports {
        match export {
            blueice_bluejs::ExportEntry::Indirect { module_request, .. }
            | blueice_bluejs::ExportEntry::Star { module_request, .. }
            | blueice_bluejs::ExportEntry::Namespace { module_request, .. } => {
                *module_request = resolve(module_request)?;
            }
            blueice_bluejs::ExportEntry::Local { .. } => {}
        }
    }
    for request in &mut module.requests {
        request.specifier = resolve(&request.specifier)?;
    }
    Ok(())
}

pub(super) fn execute_classic(
    runtime: &mut BlueJsPageRuntime,
    tab_id: u64,
    origin: &BlueJsPageOrigin,
    source: PageHostSource,
    program: BlueJsProgramV1,
) -> PageHostScriptOutcome {
    let source = match source_identity(&source) {
        Ok(source) => source,
        Err(category) => return rejected(category),
    };
    let handle = match runtime.install_program(tab_id, origin, source, &program) {
        Ok(handle) => handle,
        Err(error) => return rejected(page_runtime_category(error)),
    };
    match runtime.execute_program(tab_id, handle).map(|_: Value| ()) {
        Ok(()) => PageHostScriptOutcome::Executed,
        Err(error) => rejected(page_runtime_category(error)),
    }
}

pub(super) fn execute_module_graph(
    runtime: &mut BlueJsPageRuntime,
    tab_id: u64,
    origin: &BlueJsPageOrigin,
    graph: PageHostModuleGraph,
    programs: BTreeMap<String, BlueJsProgramV1>,
) -> PageHostScriptOutcome {
    let modules = match validate_graph(&graph) {
        Ok(modules) => modules,
        Err(category) => return rejected(category),
    };
    let mut installed = Vec::with_capacity(programs.len());
    for (module_id, program) in &programs {
        let source = modules
            .get(module_id)
            .expect("prepared module programs derive from exactly this graph");
        let identity = match source_identity(source) {
            Ok(identity) => identity,
            Err(category) => {
                discard_programs(runtime, tab_id, &installed);
                return rejected(category);
            }
        };
        match runtime.install_program(tab_id, origin, identity, program) {
            Ok(handle) => installed.push(handle),
            Err(error) => {
                discard_programs(runtime, tab_id, &installed);
                return rejected(page_runtime_category(error));
            }
        }
    }
    let entry = programs
        .keys()
        .position(|module_id| module_id == &graph.entry)
        .and_then(|index| installed.get(index).copied())
        .expect("prepared graph has an installed entry module");
    match runtime.execute_module_graph(tab_id, entry, installed) {
        Ok(_) => PageHostScriptOutcome::Executed,
        Err(error) => rejected(page_runtime_category(error)),
    }
}

pub(super) fn source_identity(
    source: &PageHostSource,
) -> Result<BlueJsSourceIdentity, &'static str> {
    BlueJsSourceIdentity::new(&source.canonical_module_id, &source.source_hash)
        .map_err(|_| "authorized JavaScript source record is invalid")
}

pub(super) fn discard_programs(
    runtime: &mut BlueJsPageRuntime,
    tab_id: u64,
    handles: &[BlueJsProgramHandle],
) {
    for handle in handles.iter().rev().copied() {
        let _ = runtime.discard_program(tab_id, handle);
    }
}

pub(super) fn parse_category(_: ParseError) -> &'static str {
    "JavaScript parsing rejected the page script"
}

pub(super) fn compile_category(_: CompileError) -> &'static str {
    "BlueJS compilation rejected the page script"
}
