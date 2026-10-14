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
        source_position: Option<PageHostScriptSourcePosition>,
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
    BlueTsEmittedClassic {
        ordinal: u32,
        source: PageHostSource,
        program: BlueJsProgramV1,
        source_map: SourceMap,
        top_level_spans: Vec<ParsedStatementSpan>,
        identifier_spans: Vec<blueice_bluejs::ParsedIdentifierSpan>,
    },
    BlueTsEmittedModule {
        ordinal: u32,
        graph: PageHostModuleGraph,
        programs: BTreeMap<String, BlueJsProgramV1>,
        emitted_maps: BTreeMap<String, BoundEmittedMap>,
    },
    BlueTsModule {
        ordinal: u32,
        graph: Box<DirectModuleGraph>,
    },
}

#[derive(Debug)]
pub(super) struct PreparedScriptFailure {
    category: &'static str,
    source_position: Option<PageHostScriptSourcePosition>,
}

impl PreparedScriptFailure {
    fn plain(category: &'static str) -> Self {
        Self {
            category,
            source_position: None,
        }
    }
}

pub(super) fn prepare_script(
    script: PageHostScript,
    page_dom_profile: PageDomProfile,
    emitted_profile: PageHostBlueTsEmissionProfile,
    emitted_classic_page_console_ambient: bool,
    emitted_module_page_console_ambient: bool,
) -> PreparedScript {
    let ordinal = script.ordinal;
    let language = script.language;
    let kind = script.kind;
    let prepared = match (language, kind) {
        (PageHostScriptLanguage::JavaScript, PageHostScriptKind::Classic) => {
            prepare_classic(script.graph)
                .map_err(PreparedScriptFailure::plain)
                .map(|(source, program)| PreparedScript::JavaScriptClassic {
                    ordinal,
                    source,
                    program,
                })
        }
        (PageHostScriptLanguage::JavaScript, PageHostScriptKind::Module) => {
            prepare_module_graph(script.graph)
                .map_err(PreparedScriptFailure::plain)
                .map(|(graph, programs)| PreparedScript::JavaScriptModule {
                    ordinal,
                    graph,
                    programs,
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
        (PageHostScriptLanguage::BlueTsEmitted, PageHostScriptKind::Classic) => {
            prepare_bluets_emitted_classic(
                script.graph,
                page_dom_profile,
                emitted_profile,
                emitted_classic_page_console_ambient,
            )
            .map(
                |(source, program, source_map, top_level_spans, identifier_spans)| {
                    PreparedScript::BlueTsEmittedClassic {
                        ordinal,
                        source,
                        program,
                        source_map,
                        top_level_spans,
                        identifier_spans,
                    }
                },
            )
        }
        (PageHostScriptLanguage::BlueTsEmitted, PageHostScriptKind::Module) => {
            prepare_bluets_emitted_module_graph(
                script.graph,
                page_dom_profile,
                emitted_module_page_console_ambient,
            )
            .map(|(graph, programs, emitted_maps)| {
                PreparedScript::BlueTsEmittedModule {
                    ordinal,
                    graph,
                    programs,
                    emitted_maps,
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
    prepared.unwrap_or_else(|failure| PreparedScript::Rejected {
        ordinal,
        language,
        kind,
        category: failure.category,
        source_position: failure.source_position,
    })
}

type PreparedEmittedClassic = (
    PageHostSource,
    BlueJsProgramV1,
    SourceMap,
    Vec<ParsedStatementSpan>,
    Vec<blueice_bluejs::ParsedIdentifierSpan>,
);

type PreparedEmittedModule = (
    PageHostModuleGraph,
    BTreeMap<String, BlueJsProgramV1>,
    BTreeMap<String, BoundEmittedMap>,
);

/// Type-only description of the ordinary `console` value supplied by a page's
/// System loader. It neither installs a runtime global nor broadens the
/// standard emitted profile; an absent loader still fails at runtime.
const SYSTEM_LOADER_CONSOLE_AMBIENT_ID: &str = "blueice://ambient/system-loader-console.d.ts";
const SYSTEM_LOADER_CONSOLE_AMBIENT_SOURCE: &str =
    "declare const console: { log(value: number): void };\n";
const MODULE_PAGE_CONSOLE_AMBIENT_ID: &str = "blueice://ambient/module-page-console.d.ts";
const MODULE_PAGE_CONSOLE_AMBIENT_SOURCE: &str =
    "declare const console: { log(...values: unknown[]): void };\n";
const CLASSIC_PAGE_CONSOLE_AMBIENT_ID: &str = "blueice://ambient/classic-page-console.d.ts";

pub(super) fn emitted_bluets_compiler_options(
    graph: &PageHostModuleGraph,
    page_dom_profile: PageDomProfile,
    emitted_profile: PageHostBlueTsEmissionProfile,
) -> Result<CompilerOptions, PreparedScriptFailure> {
    let mut options =
        bluets_compiler_options(graph, page_dom_profile).map_err(PreparedScriptFailure::plain)?;
    options.source_map = true;
    options.inline_sources = true;
    match emitted_profile {
        PageHostBlueTsEmissionProfile::Standard => {}
        PageHostBlueTsEmissionProfile::ModulePageConsole => {
            options
                .ambient_declaration_modules
                .push(blueice_bluets::ModuleSource::new(
                    MODULE_PAGE_CONSOLE_AMBIENT_ID,
                    MODULE_PAGE_CONSOLE_AMBIENT_SOURCE,
                ));
        }
        PageHostBlueTsEmissionProfile::Es5 => options.target = EcmaTarget::Es5,
        PageHostBlueTsEmissionProfile::System => {
            options.module_kind = ModuleKind::System;
            options
                .ambient_declaration_modules
                .push(blueice_bluets::ModuleSource::new(
                    SYSTEM_LOADER_CONSOLE_AMBIENT_ID,
                    SYSTEM_LOADER_CONSOLE_AMBIENT_SOURCE,
                ));
        }
        PageHostBlueTsEmissionProfile::LegacyDecorators => {
            options.experimental_decorators = true;
        }
        PageHostBlueTsEmissionProfile::LegacyDecoratorsWithMetadata => {
            options.experimental_decorators = true;
            options.emit_decorator_metadata = true;
        }
    }
    Ok(options)
}

/// Compiles a caller-authorized classic BlueTS graph and retains the exact
/// generated JavaScript plus its map as one child-owned artifact. This is
/// deliberately separate from the direct AST bridge: the executable program
/// below is parsed from the bytes that the source map actually describes.
pub(super) fn prepare_bluets_emitted_classic(
    graph: PageHostModuleGraph,
    page_dom_profile: PageDomProfile,
    emitted_profile: PageHostBlueTsEmissionProfile,
    page_console_ambient: bool,
) -> Result<PreparedEmittedClassic, PreparedScriptFailure> {
    let modules = validate_graph(&graph).map_err(PreparedScriptFailure::plain)?;
    validate_resolutions(&graph, &modules).map_err(PreparedScriptFailure::plain)?;
    let loader = bluets_loader(&graph, modules).map_err(PreparedScriptFailure::plain)?;
    let mut options = emitted_bluets_compiler_options(&graph, page_dom_profile, emitted_profile)?;
    if matches!(
        emitted_profile,
        PageHostBlueTsEmissionProfile::ModulePageConsole
    ) || (page_console_ambient
        && matches!(emitted_profile, PageHostBlueTsEmissionProfile::System))
    {
        return Err(PreparedScriptFailure::plain(
            "BlueTS classic page console ambient conflicts with emission profile",
        ));
    }
    if page_console_ambient {
        options
            .ambient_declaration_modules
            .push(blueice_bluets::ModuleSource::new(
                CLASSIC_PAGE_CONSOLE_AMBIENT_ID,
                MODULE_PAGE_CONSOLE_AMBIENT_SOURCE,
            ));
    }
    let compilation = compile(&graph.entry, &loader, options);
    if compilation.has_errors() {
        let error = BridgeError::BlueTs(compilation.diagnostics);
        return Err(PreparedScriptFailure {
            source_position: bluets_bridge_position(&error, &graph),
            category: bluets_bridge_category(error),
        });
    }
    let output = compilation
        .output
        .ok_or_else(|| PreparedScriptFailure::plain("BlueTS emitted output is unavailable"))?;
    let artifact = output
        .artifacts
        .get(&graph.entry)
        .ok_or_else(|| PreparedScriptFailure::plain("BlueTS entry output is unavailable"))?;
    let source_map = artifact
        .source_map
        .clone()
        .ok_or_else(|| PreparedScriptFailure::plain("BlueTS emitted map is unavailable"))?;
    let generated = PageHostSource::new(
        format!("{}#bluets-emitted-js", graph.entry),
        artifact.javascript.clone(),
    );
    let (parsed, top_level_spans, identifier_spans) =
        blueice_bluejs::parse_with_debug_spans(&generated.source)
            .map_err(|error| PreparedScriptFailure::plain(parse_category(error)))?;
    let program = BlueJsProgramV1::Script(parsed);
    program
        .compile()
        .map_err(|error| PreparedScriptFailure::plain(compile_category(error)))?;
    Ok((
        generated,
        program,
        source_map,
        top_level_spans,
        identifier_spans,
    ))
}

/// Compiles every module in the caller-authorized graph to JavaScript and
/// validates each child-owned map before any generated module is installed.
/// The generated graph retains the original canonical module IDs and static
/// resolution authority, but every source hash describes emitted JavaScript.
pub(super) fn prepare_bluets_emitted_module_graph(
    graph: PageHostModuleGraph,
    page_dom_profile: PageDomProfile,
    page_console_ambient: bool,
) -> Result<PreparedEmittedModule, PreparedScriptFailure> {
    let modules = validate_graph(&graph).map_err(PreparedScriptFailure::plain)?;
    validate_resolutions(&graph, &modules).map_err(PreparedScriptFailure::plain)?;
    let loader = bluets_loader(&graph, modules).map_err(PreparedScriptFailure::plain)?;
    let emission_profile = if page_console_ambient {
        PageHostBlueTsEmissionProfile::ModulePageConsole
    } else {
        PageHostBlueTsEmissionProfile::Standard
    };
    let options = emitted_bluets_compiler_options(&graph, page_dom_profile, emission_profile)?;
    let compilation = compile(&graph.entry, &loader, options);
    if compilation.has_errors() {
        let error = BridgeError::BlueTs(compilation.diagnostics);
        return Err(PreparedScriptFailure {
            source_position: bluets_bridge_position(&error, &graph),
            category: bluets_bridge_category(error),
        });
    }
    let output = compilation
        .output
        .ok_or_else(|| PreparedScriptFailure::plain("BlueTS emitted output is unavailable"))?;
    let mut emitted_sources = Vec::with_capacity(graph.modules.len());
    let mut emitted_maps = BTreeMap::new();
    let mut total_source_bytes = 0_usize;
    let mut total_map_bytes = 0_usize;
    for source in graph.modules {
        let module_id = source.canonical_module_id;
        let artifact = output
            .artifacts
            .get(&module_id)
            .ok_or_else(|| PreparedScriptFailure::plain("BlueTS module output is unavailable"))?;
        let source_map = artifact
            .source_map
            .clone()
            .ok_or_else(|| PreparedScriptFailure::plain("BlueTS emitted map is unavailable"))?;
        let generated = PageHostSource::new(module_id.clone(), artifact.javascript.clone());
        total_source_bytes = total_source_bytes
            .checked_add(generated.source.len())
            .ok_or_else(|| {
                PreparedScriptFailure::plain("BlueTS emitted graph exceeds source budget")
            })?;
        if total_source_bytes > MAX_SOURCE_BYTES_PER_DOCUMENT {
            return Err(PreparedScriptFailure::plain(
                "BlueTS emitted graph exceeds source budget",
            ));
        }
        let (parsed, top_level_spans, identifier_spans) =
            blueice_bluejs::parse_module_with_debug_spans(&generated.source)
                .map_err(|error| PreparedScriptFailure::plain(parse_category(error)))?;
        if parsed.body.len() != top_level_spans.len() {
            return Err(PreparedScriptFailure::plain(
                "BlueTS emitted module parser provenance is incomplete",
            ));
        }
        let non_evaluating = parsed
            .body
            .iter()
            .enumerate()
            .filter_map(|(index, statement)| {
                matches!(
                    statement,
                    blueice_bluejs::Stmt::FunctionDecl(_)
                        | blueice_bluejs::Stmt::ModuleDefaultFunction { .. }
                )
                .then_some(index)
            })
            .collect();
        let bound = BoundEmittedMap::new_with_non_evaluating(
            generated.clone(),
            source_map,
            emission_profile,
            top_level_spans,
            identifier_spans,
            non_evaluating,
        )
        .map_err(PreparedScriptFailure::plain)?;
        total_map_bytes = total_map_bytes
            .checked_add(bound.retained_bytes().ok_or_else(|| {
                PreparedScriptFailure::plain("BlueTS emitted maps exceed retention budget")
            })?)
            .ok_or_else(|| {
                PreparedScriptFailure::plain("BlueTS emitted maps exceed retention budget")
            })?;
        if total_map_bytes > BoundEmittedMap::MAX_RETAINED_BYTES {
            return Err(PreparedScriptFailure::plain(
                "BlueTS emitted maps exceed retention budget",
            ));
        }
        emitted_sources.push(generated);
        emitted_maps.insert(module_id, bound);
    }
    let emitted_graph = PageHostModuleGraph {
        entry: graph.entry,
        modules: emitted_sources,
        resolutions: graph
            .resolutions
            .into_iter()
            .map(|mut resolution| {
                resolution.specifier =
                    blueice_bluets::emitted_javascript_specifier(&resolution.specifier, false);
                resolution
            })
            .collect(),
        resolver_fingerprint: graph.resolver_fingerprint,
    };
    let (emitted_graph, programs) =
        prepare_module_graph(emitted_graph).map_err(PreparedScriptFailure::plain)?;
    Ok((emitted_graph, programs, emitted_maps))
}

/// Installs a fully prepared emitted graph as one debugger attachment. Every
/// retained map must describe the exact generated source admitted under the
/// same canonical module ID. A failed install discards all earlier modules;
/// callers publish opaque debugger identities and maps only after success.
pub(super) fn attach_emitted_module_graph(
    runtime: &mut BlueJsPageRuntime,
    tab_id: u64,
    origin: &BlueJsPageOrigin,
    graph: &PageHostModuleGraph,
    programs: &BTreeMap<String, BlueJsProgramV1>,
    emitted_maps: &BTreeMap<String, BoundEmittedMap>,
) -> Result<EmittedPageModuleGraphAttachment, &'static str> {
    let sources = validate_graph(graph)?;
    if !sources.keys().eq(programs.keys()) || !programs.keys().eq(emitted_maps.keys()) {
        return Err("BlueTS emitted module graph has incomplete maps or programs");
    }
    for (module_id, source) in &sources {
        let bound = &emitted_maps[module_id];
        if bound.generated.canonical_module_id != *module_id
            || bound.generated.source_hash != source.source_hash
            || bound.generated.source != source.source
        {
            return Err("BlueTS emitted module map does not match its generated source");
        }
    }
    let mut installed = Vec::with_capacity(programs.len());
    let mut modules = BTreeMap::new();
    for (module_id, program) in programs {
        let identity = match source_identity(&sources[module_id]) {
            Ok(identity) => identity,
            Err(category) => {
                discard_programs(runtime, tab_id, &installed);
                return Err(category);
            }
        };
        match runtime.install_program(tab_id, origin, identity, program) {
            Ok(handle) => {
                installed.push(handle);
                modules.insert(module_id.clone(), handle);
            }
            Err(error) => {
                discard_programs(runtime, tab_id, &installed);
                return Err(page_runtime_category(error));
            }
        }
    }
    let Some(entry) = modules.get(&graph.entry).copied() else {
        discard_programs(runtime, tab_id, &installed);
        return Err("BlueTS emitted module graph has no entry program");
    };
    Ok(EmittedPageModuleGraphAttachment { entry, modules })
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

/// Prepares an explicit BlueTS classic declaration through a closed
/// caller-authorized graph. Type-only imports may resolve declaration inputs,
/// but the direct script bridge refuses every runtime import. The compiler sees
/// no filesystem, URL, import-map, page-selected profile, or callback
/// authority. Its only ambient declaration comes from the exact generated
/// owner-selected profile: copied snapshots by default, or bounded live DOM
/// text/mutation when the launcher granted that child capability.
pub(super) fn prepare_bluets_classic(
    graph: PageHostModuleGraph,
    page_dom_profile: PageDomProfile,
) -> Result<DirectScript, PreparedScriptFailure> {
    let modules = validate_graph(&graph).map_err(PreparedScriptFailure::plain)?;
    validate_resolutions(&graph, &modules).map_err(PreparedScriptFailure::plain)?;
    let loader = bluets_loader(&graph, modules).map_err(PreparedScriptFailure::plain)?;
    compile_direct_script(
        &graph.entry,
        &loader,
        bluets_compiler_options(&graph, page_dom_profile).map_err(PreparedScriptFailure::plain)?,
    )
    .map_err(|error| PreparedScriptFailure {
        source_position: bluets_bridge_position(&error, &graph),
        category: bluets_bridge_category(error),
    })
}

/// Prepares a complete explicit BlueTS module graph without giving BlueTS a
/// resolver beyond the exact static edges serialized by its caller.
pub(super) fn prepare_bluets_module_graph(
    graph: PageHostModuleGraph,
    page_dom_profile: PageDomProfile,
) -> Result<DirectModuleGraph, PreparedScriptFailure> {
    let modules = validate_graph(&graph).map_err(PreparedScriptFailure::plain)?;
    validate_resolutions(&graph, &modules).map_err(PreparedScriptFailure::plain)?;
    let loader = bluets_loader(&graph, modules).map_err(PreparedScriptFailure::plain)?;
    compile_direct_module_graph(
        &graph.entry,
        &loader,
        bluets_compiler_options(&graph, page_dom_profile).map_err(PreparedScriptFailure::plain)?,
    )
    .map_err(|error| PreparedScriptFailure {
        source_position: bluets_bridge_position(&error, &graph),
        category: bluets_bridge_category(error),
    })
}

fn bluets_bridge_position(
    error: &BridgeError,
    graph: &PageHostModuleGraph,
) -> Option<PageHostScriptSourcePosition> {
    let candidate = |span: &blueice_bluets::SourceSpan| {
        let source = graph
            .modules
            .iter()
            .find(|source| source.canonical_module_id == span.module)?;
        if span.start >= span.end
            || span.end > source.source.len()
            || !source.source.is_char_boundary(span.start)
            || !source.source.is_char_boundary(span.end)
        {
            return None;
        }
        let position = PageHostScriptSourcePosition {
            module_id: span.module.clone(),
            start: u32::try_from(span.start).ok()?,
            end: u32::try_from(span.end).ok()?,
        };
        position.is_well_formed().then_some(position)
    };
    match error {
        BridgeError::BlueTs(diagnostics) => diagnostics
            .iter()
            .find_map(|diagnostic| candidate(&diagnostic.span)),
        BridgeError::UnsupportedRuntimeTarget { span, .. } => candidate(span),
        _ => None,
    }
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
        PageDomProfile::File => PageHostDocumentTypingsV1::generate_dom_window()
            .verified_dom_window_ambient_module(&page_host_dom_window_runtime_bindings_v13()),
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

#[cfg(test)]
mod tests {
    use super::*;
    use blueice_bluets::{Diagnostic, DiagnosticCode, SourceSpan};

    fn graph_with_one_module(module_id: &str, source: &str) -> PageHostModuleGraph {
        PageHostModuleGraph {
            entry: module_id.to_string(),
            modules: vec![PageHostSource::new(module_id, source)],
            resolutions: Vec::new(),
            resolver_fingerprint: "test-fingerprint".to_string(),
        }
    }

    #[test]
    fn bluets_module_file_profile_keeps_live_iteration_tee_and_node_events() {
        let graph=graph_with_one_module("blueice://page/iteration.ts","export function read(stream: BlueIceReadableStream): BlueIceFilePromise<BlueIceStreamReadResult> { return stream.tee()[0].values().next(); }\nexport function emit(node: BlueIceNode): boolean { return node.dispatchEvent(new CustomEvent('data', {detail:7})); }");
        let prepared = prepare_bluets_module_graph(graph, PageDomProfile::File);
        assert!(
            prepared.is_ok(),
            "current live profile must prepare the authorized module graph: {:?}",
            prepared.err()
        );
    }

    #[test]
    fn parse_category_and_compile_category_are_fixed_strings() {
        assert_eq!(
            parse_category(ParseError {
                message: "boom".to_string(),
                resource: None,
                known_syntax: false,
            }),
            "JavaScript parsing rejected the page script"
        );
        assert_eq!(
            compile_category(CompileError::ProgramTooLarge),
            "BlueJS compilation rejected the page script"
        );
    }

    #[test]
    fn discard_programs_accepts_an_empty_handle_list() {
        let mut runtime = BlueJsPageRuntime::new(BlueJsPageRuntimeConfig::default()).unwrap();
        discard_programs(&mut runtime, 1, &[]);
    }

    #[test]
    fn bluets_bridge_position_resolves_a_bluets_diagnostics_matching_span() {
        let graph = graph_with_one_module("blueice://page/a.ts", "const x: number = 1;");
        let error = BridgeError::BlueTs(vec![Diagnostic::error(
            DiagnosticCode::ParseError,
            SourceSpan {
                module: "blueice://page/a.ts".to_string(),
                start: 0,
                end: 5,
            },
            "unexpected token",
        )]);
        let position = bluets_bridge_position(&error, &graph).unwrap();
        assert_eq!(position.module_id, "blueice://page/a.ts");
        assert_eq!(position.start, 0);
        assert_eq!(position.end, 5);
    }

    #[test]
    fn bluets_bridge_position_resolves_an_unsupported_runtime_target_span() {
        let graph = graph_with_one_module("blueice://page/a.ts", "const x: number = 1;");
        let error = BridgeError::UnsupportedRuntimeTarget {
            span: SourceSpan {
                module: "blueice://page/a.ts".to_string(),
                start: 6,
                end: 7,
            },
            message: "unsupported".to_string(),
        };
        let position = bluets_bridge_position(&error, &graph).unwrap();
        assert_eq!(position.start, 6);
        assert_eq!(position.end, 7);
    }

    #[test]
    fn bluets_bridge_position_is_none_for_every_other_error_category() {
        let graph = graph_with_one_module("blueice://page/a.ts", "const x: number = 1;");
        assert!(bluets_bridge_position(
            &BridgeError::InvalidSourceIdentity("bad".to_string()),
            &graph
        )
        .is_none());
    }

    #[test]
    fn bluets_bridge_position_is_none_for_a_span_outside_the_graphs_modules() {
        let graph = graph_with_one_module("blueice://page/a.ts", "const x: number = 1;");
        let error = BridgeError::BlueTs(vec![Diagnostic::error(
            DiagnosticCode::ParseError,
            SourceSpan {
                module: "blueice://page/other.ts".to_string(),
                start: 0,
                end: 1,
            },
            "unexpected token",
        )]);
        assert!(bluets_bridge_position(&error, &graph).is_none());
    }

    #[test]
    fn bluets_bridge_position_is_none_for_a_malformed_span() {
        let graph = graph_with_one_module("blueice://page/a.ts", "const x: number = 1;");
        let backwards = BridgeError::BlueTs(vec![Diagnostic::error(
            DiagnosticCode::ParseError,
            SourceSpan {
                module: "blueice://page/a.ts".to_string(),
                start: 5,
                end: 5,
            },
            "unexpected token",
        )]);
        assert!(bluets_bridge_position(&backwards, &graph).is_none());

        let out_of_bounds = BridgeError::BlueTs(vec![Diagnostic::error(
            DiagnosticCode::ParseError,
            SourceSpan {
                module: "blueice://page/a.ts".to_string(),
                start: 0,
                end: 10_000,
            },
            "unexpected token",
        )]);
        assert!(bluets_bridge_position(&out_of_bounds, &graph).is_none());
    }
}
