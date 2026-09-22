// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Direct, host-neutral BlueTS to BlueJS structured-program lowering.
//!
//! This is the only crate permitted to depend on both language crates. It
//! consumes BlueTS's checked, already-tokenized declarations and constructs
//! BlueJS AST nodes directly; it never receives or reparses BlueTSC JavaScript
//! emission. The first executable bridge deliberately covers only classic
//! scripts with typed variable and function declarations plus bounded runtime
//! expressions.

use blueice_bluejs as bluejs;
use blueice_bluets::{
    compile, lex, BlueTsDebugInfo, CompilerOptions, Declaration, Diagnostic, FunctionBodyItem,
    FunctionDeclaration, FunctionElseBranch, FunctionIfStatement, Module, ModuleLoader, Project,
    SourceSpan, Token, TokenKind, VariableDeclaration, VariableKind, LANGUAGE_VERSION,
};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;

mod debug_attachment;
mod page_runtime;
pub use debug_attachment::{
    DirectDebugAttachmentError, DirectDebugRegistry, DirectDebugRetentionLimits,
    RetainedDirectDebugInfo,
};
pub use page_runtime::{DirectPageModuleGraphAttachment, DirectPageRealmOwner};

/// The first directly executable BlueTS-to-BlueJS bridge ABI.
pub const BLUE_TS_BLUEJS_BRIDGE_ABI_V1: &str = "blue-ts-bluejs-bridge-v1";

/// The first generation-bound TypeScript-to-BlueJS instruction map format.
pub const BLUEJS_SAFE_POINT_MAP_ABI_V1: &str = "bluejs-safe-point-map-v1";

/// A source identity retained alongside the structured program. Source text is
/// intentionally absent: the bridge preserves only the compiler-provided hash
/// and canonical module ID needed to reject stale attachments later.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeSource {
    pub module: String,
    pub content_hash: String,
}

/// A direct-lowering span. It identifies a BlueTS source range that supplied
/// at least one executable BlueJS AST statement; it is not itself a bytecode
/// safe point until a live attachment resolves its AST node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoweringProvenance {
    pub source: SourceSpan,
    pub kind: LoweringProvenanceKind,
}

/// How a TypeScript source span contributed a direct BlueJS AST statement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LoweringProvenanceKind {
    /// The executable expression was copied without a TypeScript-only rewrite.
    Copied,
    /// TypeScript syntax was erased or lowered while producing executable AST.
    LoweredSyntax,
}

/// A checked, direct BlueJS compilation of one classic TypeScript script.
#[derive(Clone)]
pub struct DirectScript {
    pub bridge_abi: &'static str,
    pub program: bluejs::BlueJsProgramV1,
    pub bytecode: bluejs::Bytecode,
    pub language_version: String,
    pub compiler_options_fingerprint: String,
    pub sources: Vec<BridgeSource>,
    pub provenance: Vec<LoweringProvenance>,
    /// Static BlueTS source/type/symbol metadata for this exact compilation.
    /// It contains no runtime BlueJS values or source text.
    pub debug_info: BlueTsDebugInfo,
}

/// A checked, direct BlueJS compilation of one TypeScript source module.
#[derive(Clone)]
pub struct DirectModule {
    pub bridge_abi: &'static str,
    pub program: bluejs::BlueJsProgramV1,
    pub bytecode: bluejs::Bytecode,
    pub language_version: String,
    pub compiler_options_fingerprint: String,
    pub sources: Vec<BridgeSource>,
    pub provenance: Vec<LoweringProvenance>,
    /// Static BlueTS source/type/symbol metadata for this exact compilation.
    pub debug_info: BlueTsDebugInfo,
}

/// A checked, direct BlueJS compilation of a closed TypeScript module graph.
#[derive(Clone)]
pub struct DirectModuleGraph {
    pub bridge_abi: &'static str,
    pub entry: String,
    pub modules: BTreeMap<String, DirectModule>,
    pub language_version: String,
    pub compiler_options_fingerprint: String,
    pub sources: Vec<BridgeSource>,
    /// Static metadata for the caller-authorized closed source graph.
    pub debug_info: BlueTsDebugInfo,
}

/// A live direct-program attachment with exact source-to-AST provenance and
/// a verified generation-bound safe-point map.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectProgramAttachment {
    /// The live generation-bound BlueJS program handle.
    pub handle: bluejs::BlueJsProgramHandle,
    /// Ordered lowering spans paired with their exact generated AST nodes.
    pub provenance: Vec<AttachedLoweringProvenance>,
    /// Deterministic, validated safe-point entries for bound lowering spans.
    pub safe_point_map: BlueTsSafePointMapV1,
}

/// One source-level lowering span attached to a generated BlueJS AST node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachedLoweringProvenance {
    /// The exact TypeScript source bytes that contributed the node.
    pub source: SourceSpan,
    /// How the source span contributed its generated AST statement.
    pub kind: LoweringProvenanceKind,
    /// The generation-bound BlueJS structured AST node.
    pub node_id: bluejs::BlueJsAstNodeId,
    /// The exact compiler-recorded instruction boundary for this node, or an
    /// explicit unbound result when the node emits no root instruction.
    pub safe_point: DirectSafePointBinding,
}

/// The direct bridge's safe-point resolution for one lowered source span.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectSafePointBinding {
    /// The associated AST node has a verified root-code-unit instruction.
    Bound(bluejs::BlueJsSafePoint),
    /// The AST node emitted no root instruction; callers must report an
    /// unbound breakpoint rather than remapping it heuristically.
    Unbound,
}

/// A generation-bound direct-bridge safe-point map.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlueTsSafePointMapV1 {
    /// Always [`BLUEJS_SAFE_POINT_MAP_ABI_V1`].
    pub format: &'static str,
    /// The BlueJS structured-program ABI used to compile the program.
    pub program_abi: &'static str,
    /// The opaque registry generation that owns every entry.
    pub program_generation: u64,
    /// The exact BlueTS compiler-options fingerprint used by the bridge.
    pub compiler_options_fingerprint: String,
    /// Deterministic fingerprint of the canonical source identities.
    pub source_set_hash: String,
    /// Sorted, unique bound entries. Unbound spans remain in attachment
    /// provenance instead of acquiring a guessed instruction location.
    pub entries: Vec<BlueTsSafePointEntryV1>,
}

/// One bound top-level TypeScript lowering span.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlueTsSafePointEntryV1 {
    /// BlueJS code unit that owns the instruction.
    pub code_unit: bluejs::BlueJsCodeUnitId,
    /// Verified instruction start inside `code_unit`.
    pub bytecode_offset: u32,
    /// Canonical TypeScript module identity.
    pub source: String,
    /// Inclusive UTF-8 source-byte start.
    pub start_byte: usize,
    /// Exclusive UTF-8 source-byte end.
    pub end_byte: usize,
    /// How the original source supplied the executable statement.
    pub provenance_kind: LoweringProvenanceKind,
}

/// The bridge either propagates BlueTS diagnostics, rejects a checker-accepted
/// runtime shape outside its current direct subset, or reports BlueJS bytecode
/// compilation failure. No variant offers a generated-JavaScript fallback.
#[derive(Debug)]
pub enum BridgeError {
    BlueTs(Vec<Diagnostic>),
    UnsupportedRuntimeTarget { span: SourceSpan, message: String },
    BlueJs(bluejs::CompileError),
    BlueJsDebug(bluejs::BlueJsProgramDebugError),
    PageRuntime(bluejs::BlueJsPageRuntimeError),
    InvalidSourceIdentity(String),
    ProvenanceAttachment(String),
    DebugAttachment(DirectDebugAttachmentError),
}

impl fmt::Display for BridgeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BlueTs(diagnostics) => write!(
                formatter,
                "BlueTS rejected the direct script with {} diagnostic(s)",
                diagnostics.len()
            ),
            Self::UnsupportedRuntimeTarget { span, message } => write!(
                formatter,
                "{}:{}:{} cannot lower to the current BlueJS bridge: {message}",
                span.module, span.start, span.end
            ),
            Self::BlueJs(error) => {
                write!(formatter, "BlueJS rejected the lowered program: {error}")
            }
            Self::BlueJsDebug(error) => {
                write!(
                    formatter,
                    "BlueJS rejected the direct-program attachment: {error}"
                )
            }
            Self::PageRuntime(error) => {
                write!(
                    formatter,
                    "BlueJS page runtime rejected the direct program: {error}"
                )
            }
            Self::InvalidSourceIdentity(message) => {
                write!(
                    formatter,
                    "BlueTS direct program has no usable source identity: {message}"
                )
            }
            Self::ProvenanceAttachment(message) => {
                write!(
                    formatter,
                    "cannot attach direct lowering provenance: {message}"
                )
            }
            Self::DebugAttachment(error) => {
                write!(
                    formatter,
                    "cannot attach direct static debug metadata: {error}"
                )
            }
        }
    }
}

impl std::error::Error for BridgeError {}

impl DirectScript {
    /// Installs this direct program in a caller-owned BlueJS registry. The
    /// bridge supplies the exact canonical source identity that BlueTS checked
    /// without an emitted-JavaScript parse; BlueJS compiles the same structured
    /// program while assigning its generation-bound executable AST-node IDs.
    pub fn install_in(
        &self,
        registry: &mut bluejs::BlueJsProgramRegistry,
    ) -> Result<bluejs::BlueJsProgramHandle, BridgeError> {
        Ok(self.attach_in(registry)?.handle)
    }

    /// Installs the program and attaches every top-level BlueTS lowering span
    /// to the exact BlueJS AST statement it generated.
    pub fn attach_in(
        &self,
        registry: &mut bluejs::BlueJsProgramRegistry,
    ) -> Result<DirectProgramAttachment, BridgeError> {
        attach_direct_program(
            registry,
            &self.sources,
            &self.program,
            &self.provenance,
            &self.compiler_options_fingerprint,
        )
    }

    /// Attaches this artifact's provenance to an already-live BlueJS program.
    /// The handle must carry this exact source identity and compiled bytecode;
    /// a page runtime uses this after its own ownership/resource admission,
    /// avoiding a second registry or generated-JavaScript reparse.
    pub(crate) fn attach_existing_in(
        &self,
        registry: &bluejs::BlueJsProgramRegistry,
        handle: bluejs::BlueJsProgramHandle,
    ) -> Result<DirectProgramAttachment, BridgeError> {
        attach_existing_direct_program(
            registry,
            handle,
            &self.sources,
            Some(&self.bytecode),
            &self.provenance,
            &self.compiler_options_fingerprint,
        )
    }

    /// Installs the program and retains its static BlueTS metadata only for
    /// the resulting live BlueJS generation. If retention rejects an identity
    /// mismatch or limit, the just-installed generation is invalidated so a
    /// caller cannot execute an unpaired direct program accidentally.
    pub fn attach_debug_in(
        &self,
        registry: &mut bluejs::BlueJsProgramRegistry,
        debug_registry: &mut DirectDebugRegistry,
    ) -> Result<DirectProgramAttachment, BridgeError> {
        let attachment = self.attach_in(registry)?;
        if let Err(error) = debug_registry.retain(
            registry,
            &attachment,
            &self.language_version,
            &self.compiler_options_fingerprint,
            &self.sources,
            &self.debug_info,
        ) {
            registry.invalidate(attachment.handle);
            return Err(BridgeError::DebugAttachment(error));
        }
        Ok(attachment)
    }
}

impl DirectModule {
    /// Installs this direct module in a caller-owned BlueJS registry, retaining
    /// its BlueTS-authorized canonical source identity and AST-node inventory.
    pub fn install_in(
        &self,
        registry: &mut bluejs::BlueJsProgramRegistry,
    ) -> Result<bluejs::BlueJsProgramHandle, BridgeError> {
        Ok(self.attach_in(registry)?.handle)
    }

    /// Installs the module and attaches every top-level BlueTS lowering span
    /// to the exact BlueJS AST statement it generated.
    pub fn attach_in(
        &self,
        registry: &mut bluejs::BlueJsProgramRegistry,
    ) -> Result<DirectProgramAttachment, BridgeError> {
        attach_direct_program(
            registry,
            &self.sources,
            &self.program,
            &self.provenance,
            &self.compiler_options_fingerprint,
        )
    }

    pub(crate) fn attach_existing_in(
        &self,
        registry: &bluejs::BlueJsProgramRegistry,
        handle: bluejs::BlueJsProgramHandle,
    ) -> Result<DirectProgramAttachment, BridgeError> {
        attach_existing_direct_program(
            registry,
            handle,
            &self.sources,
            Some(&self.bytecode),
            &self.provenance,
            &self.compiler_options_fingerprint,
        )
    }

    /// Equivalent to [`DirectScript::attach_debug_in`] for one direct ESM
    /// module. The retained object is static metadata, not a runtime scope or
    /// BlueJS value inspector.
    pub fn attach_debug_in(
        &self,
        registry: &mut bluejs::BlueJsProgramRegistry,
        debug_registry: &mut DirectDebugRegistry,
    ) -> Result<DirectProgramAttachment, BridgeError> {
        let attachment = self.attach_in(registry)?;
        if let Err(error) = debug_registry.retain(
            registry,
            &attachment,
            &self.language_version,
            &self.compiler_options_fingerprint,
            &self.sources,
            &self.debug_info,
        ) {
            registry.invalidate(attachment.handle);
            return Err(BridgeError::DebugAttachment(error));
        }
        Ok(attachment)
    }
}

/// Parses, resolves and checks one host-authorized TypeScript source graph,
/// then directly constructs one BlueJS Script AST and bytecode unit.
///
/// The v1 direct subset has exactly one non-declaration module and no runtime
/// import/export entries. It accepts `var`/`let`/`const` declarations with an
/// optional literal/identifier/arithmetic initializer, named local functions
/// with required or optional identifier parameters, bounded direct-expression
/// defaults, and one final identifier rest parameter plus structured local/return bodies,
/// and standalone expressions made from those same forms or direct calls.
/// The expression subset includes `!`, `+`, `-`, `~`, `typeof`, `void`, and
/// `delete` with a property target; arithmetic, relational (including `in` and
/// `instanceof`), equality, logical,
/// nullish-coalescing, arithmetic exponentiation, bitwise/shift, conditional,
/// non-hole array literals with spread elements, object literals with
/// identifier/string/numeric/computed keys and spread properties, template literals
/// whose substitutions use the same bounded expression subset, dot or bracket
/// property reads, comma sequences, identifier/property prefix/postfix updates,
/// calls and constructors with normal/spread arguments, and identifier/property
/// simple or compound-assignment operators. Static-only
/// declarations disappear before lowering. A broader accepted BlueTS program
/// returns
/// [`BridgeError::UnsupportedRuntimeTarget`] instead of falling back to a
/// JavaScript text round trip.
pub fn compile_direct_script(
    entry: &str,
    loader: &dyn ModuleLoader,
    options: CompilerOptions,
) -> Result<DirectScript, BridgeError> {
    let (module, debug_info) = checked_entry(entry, loader, options)?;

    let (body, provenance) = lower_script(&module)?;
    let program = bluejs::BlueJsProgramV1::Script(bluejs::Program { body });
    let bytecode = program.compile().map_err(BridgeError::BlueJs)?;
    let sources = bridge_sources(&debug_info);
    Ok(DirectScript {
        bridge_abi: BLUE_TS_BLUEJS_BRIDGE_ABI_V1,
        program,
        bytecode,
        language_version: LANGUAGE_VERSION.to_string(),
        compiler_options_fingerprint: debug_info.compiler_options_hash.clone(),
        sources,
        provenance,
        debug_info,
    })
}

/// Parses, resolves and checks one host-authorized TypeScript source graph,
/// then directly constructs one BlueJS Module AST and bytecode unit.
///
/// The v1 direct module subset has one non-declaration module and local named
/// or default ESM exports. Runtime imports and re-exports remain rejected
/// until the bridge can carry BlueTS's host-authorized module resolution to a
/// BlueJS module graph.
pub fn compile_direct_module(
    entry: &str,
    loader: &dyn ModuleLoader,
    options: CompilerOptions,
) -> Result<DirectModule, BridgeError> {
    let (module, debug_info) = checked_entry(entry, loader, options)?;
    let (module, provenance) = lower_module(None, &module)?;
    let program = bluejs::BlueJsProgramV1::Module(module);
    let bytecode = program.compile().map_err(BridgeError::BlueJs)?;
    let sources = bridge_sources(&debug_info);
    Ok(DirectModule {
        bridge_abi: BLUE_TS_BLUEJS_BRIDGE_ABI_V1,
        program,
        bytecode,
        language_version: LANGUAGE_VERSION.to_string(),
        compiler_options_fingerprint: debug_info.compiler_options_hash.clone(),
        sources,
        provenance,
        debug_info,
    })
}

/// Parses, resolves and checks a caller-authorized TypeScript module graph,
/// then directly constructs the corresponding BlueJS Module ASTs and bytecode.
///
/// Every runtime request carries the canonical target selected by BlueTS's
/// [`ModuleLoader`], rather than re-resolving its original TypeScript
/// specifier under BlueJS's relative-path rules. Declaration modules remain
/// type-only and do not become BlueJS graph nodes.
pub fn compile_direct_module_graph(
    entry: &str,
    loader: &dyn ModuleLoader,
    options: CompilerOptions,
) -> Result<DirectModuleGraph, BridgeError> {
    let compilation = compile(entry, loader, options);
    if compilation.has_errors() {
        return Err(BridgeError::BlueTs(compilation.diagnostics));
    }
    let debug_info = compilation
        .debug_info
        .expect("a successful BlueTS compilation always has debug information");
    let runtime_modules = runtime_module_ids(&compilation.project, entry)?;
    let mut modules = BTreeMap::new();
    for id in runtime_modules {
        let module = compilation
            .project
            .modules
            .get(&id)
            .expect("runtime-reachable module was selected from the project");
        let (module, provenance) = lower_module(Some(&compilation.project), module)?;
        let program = bluejs::BlueJsProgramV1::Module(module);
        let bytecode = program.compile().map_err(BridgeError::BlueJs)?;
        let sources = debug_info
            .sources
            .iter()
            .filter(|source| source.module == id)
            .map(|source| BridgeSource {
                module: source.module.clone(),
                content_hash: source.content_hash.clone(),
            })
            .collect();
        let module_debug_info = debug_info_for_module(&debug_info, &id);
        modules.insert(
            id,
            DirectModule {
                bridge_abi: BLUE_TS_BLUEJS_BRIDGE_ABI_V1,
                program,
                bytecode,
                language_version: LANGUAGE_VERSION.to_string(),
                compiler_options_fingerprint: debug_info.compiler_options_hash.clone(),
                sources,
                provenance,
                debug_info: module_debug_info,
            },
        );
    }
    if !modules.contains_key(entry) {
        return Err(unsupported(
            SourceSpan::new(entry, 0, 0),
            "a declaration module cannot be the direct module-graph entry",
        ));
    }
    let sources = bridge_sources(&debug_info);
    Ok(DirectModuleGraph {
        bridge_abi: BLUE_TS_BLUEJS_BRIDGE_ABI_V1,
        entry: entry.to_string(),
        modules,
        language_version: LANGUAGE_VERSION.to_string(),
        compiler_options_fingerprint: debug_info.compiler_options_hash.clone(),
        sources,
        debug_info,
    })
}

fn runtime_module_ids(project: &Project, entry: &str) -> Result<BTreeSet<String>, BridgeError> {
    let mut modules = BTreeSet::new();
    let mut pending = vec![entry.to_string()];
    while let Some(module_id) = pending.pop() {
        if !modules.insert(module_id.clone()) {
            continue;
        }
        let module = project.modules.get(&module_id).ok_or_else(|| {
            unsupported(
                SourceSpan::new(entry, 0, 0),
                "the requested module-graph entry was not retained in the checked source graph",
            )
        })?;
        if module.id.ends_with(".d.ts") {
            return Err(unsupported(
                SourceSpan::new(&module.id, 0, 0),
                "a declaration module cannot be the direct module-graph entry",
            ));
        }
        for declaration in &module.declarations {
            let Declaration::Import(import) = declaration else {
                continue;
            };
            if import.type_only {
                continue;
            }
            let target = project
                .resolved_module(&module.id, &import.specifier)
                .ok_or_else(|| {
                    unsupported(
                        import.specifier_span.clone(),
                        "BlueTS did not retain a canonical target for this runtime import",
                    )
                })?;
            if !target.ends_with(".d.ts") {
                pending.push(target.to_string());
            }
        }
    }
    Ok(modules)
}

fn checked_entry(
    entry: &str,
    loader: &dyn ModuleLoader,
    options: CompilerOptions,
) -> Result<(Module, BlueTsDebugInfo), BridgeError> {
    let compilation = compile(entry, loader, options);
    if compilation.has_errors() {
        return Err(BridgeError::BlueTs(compilation.diagnostics));
    }
    if compilation
        .project
        .modules
        .keys()
        .filter(|module_id| !module_id.ends_with(".d.ts"))
        .count()
        != 1
    {
        return Err(unsupported(
            SourceSpan::new(entry, 0, 0),
            "the v1 direct bridge supports exactly one executable source module",
        ));
    }
    let module = compilation
        .project
        .modules
        .get(entry)
        .cloned()
        .ok_or_else(|| {
            unsupported(
                SourceSpan::new(entry, 0, 0),
                "the requested entry was not retained in the checked source graph",
            )
        })?;
    if module.id.ends_with(".d.ts") {
        return Err(unsupported(
            SourceSpan::new(entry, 0, 0),
            "a declaration module cannot be executed directly",
        ));
    }
    let debug_info = compilation
        .debug_info
        .expect("a successful BlueTS compilation always has debug information");
    Ok((module, debug_info))
}

fn bridge_sources(debug_info: &BlueTsDebugInfo) -> Vec<BridgeSource> {
    debug_info
        .sources
        .iter()
        .map(|source| BridgeSource {
            module: source.module.clone(),
            content_hash: source.content_hash.clone(),
        })
        .collect()
}

/// Produces static debugger metadata for one generated module program. The
/// graph-wide artifact still retains its complete metadata separately; this
/// subset keeps a single program's source set and safe-point map exact.
fn debug_info_for_module(debug_info: &BlueTsDebugInfo, module_id: &str) -> BlueTsDebugInfo {
    let symbols = debug_info
        .symbols
        .iter()
        .filter(|symbol| symbol.span.module == module_id)
        .cloned()
        .collect::<Vec<_>>();
    let type_ids = symbols
        .iter()
        .filter_map(|symbol| symbol.static_type)
        .collect::<BTreeSet<_>>();
    BlueTsDebugInfo {
        language_version: debug_info.language_version.clone(),
        compiler_options_hash: debug_info.compiler_options_hash.clone(),
        sources: debug_info
            .sources
            .iter()
            .filter(|source| source.module == module_id)
            .cloned()
            .collect(),
        types: debug_info
            .types
            .iter()
            .filter(|static_type| type_ids.contains(&static_type.id))
            .cloned()
            .collect(),
        symbols,
    }
}

fn source_identity(sources: &[BridgeSource]) -> Result<bluejs::BlueJsSourceIdentity, BridgeError> {
    let executable_sources = sources
        .iter()
        .filter(|source| !source.module.ends_with(".d.ts"))
        .collect::<Vec<_>>();
    let [source] = executable_sources.as_slice() else {
        return Err(BridgeError::InvalidSourceIdentity(
            "a direct script or module must retain exactly one executable source".to_string(),
        ));
    };
    bluejs::BlueJsSourceIdentity::new(source.module.clone(), source.content_hash.clone())
        .map_err(BridgeError::BlueJsDebug)
}

fn attach_direct_program(
    registry: &mut bluejs::BlueJsProgramRegistry,
    sources: &[BridgeSource],
    program: &bluejs::BlueJsProgramV1,
    provenance: &[LoweringProvenance],
    compiler_options_fingerprint: &str,
) -> Result<DirectProgramAttachment, BridgeError> {
    let source = source_identity(sources)?;
    let handle = registry
        .install(source, program)
        .map_err(BridgeError::BlueJsDebug)?;
    match attach_existing_direct_program(
        registry,
        handle,
        sources,
        None,
        provenance,
        compiler_options_fingerprint,
    ) {
        Ok(attachment) => Ok(attachment),
        Err(error) => {
            registry.invalidate(handle);
            Err(error)
        }
    }
}

fn attach_existing_direct_program(
    registry: &bluejs::BlueJsProgramRegistry,
    handle: bluejs::BlueJsProgramHandle,
    sources: &[BridgeSource],
    expected_bytecode: Option<&bluejs::Bytecode>,
    provenance: &[LoweringProvenance],
    compiler_options_fingerprint: &str,
) -> Result<DirectProgramAttachment, BridgeError> {
    let source = source_identity(sources)?;
    let compiled = registry.get(handle).map_err(BridgeError::BlueJsDebug)?;
    if compiled.source() != &source {
        return Err(BridgeError::ProvenanceAttachment(
            "the live program source identity does not match this direct artifact".to_string(),
        ));
    }
    if expected_bytecode.is_some_and(|expected| !bytecode_matches(expected, compiled.bytecode())) {
        return Err(BridgeError::ProvenanceAttachment(
            "the live program bytecode does not match this direct artifact".to_string(),
        ));
    }
    let nodes = compiled
        .ast_nodes()
        .iter()
        .filter(|node| node.is_top_level_statement())
        .map(|node| node.id())
        .collect::<Vec<_>>();
    if nodes.len() != provenance.len() {
        return Err(BridgeError::ProvenanceAttachment(format!(
            "BlueTS retained {} top-level lowering spans but BlueJS generated {} top-level statements",
            provenance.len(),
            nodes.len()
        )));
    }
    let attached_provenance = provenance
        .iter()
        .zip(nodes)
        .map(|(provenance, node_id)| {
            let safe_point = match registry.safe_point_for_ast_node(handle, node_id) {
                Ok(safe_point) => DirectSafePointBinding::Bound(safe_point),
                Err(bluejs::BlueJsProgramDebugError::AstNodeUnbound) => {
                    DirectSafePointBinding::Unbound
                }
                Err(error) => return Err(BridgeError::BlueJsDebug(error)),
            };
            Ok(AttachedLoweringProvenance {
                source: provenance.source.clone(),
                kind: provenance.kind,
                node_id,
                safe_point,
            })
        })
        .collect::<Result<Vec<_>, BridgeError>>()?;
    let safe_point_map = build_safe_point_map(
        handle,
        compiler_options_fingerprint,
        sources,
        &attached_provenance,
    )?;
    Ok(DirectProgramAttachment {
        handle,
        provenance: attached_provenance,
        safe_point_map,
    })
}

fn bytecode_matches(expected: &bluejs::Bytecode, actual: &bluejs::Bytecode) -> bool {
    expected.bytes() == actual.bytes()
        && expected.constants() == actual.constants()
        && expected.root_statement_offsets() == actual.root_statement_offsets()
        && expected
            .child_code_units()
            .zip(actual.child_code_units())
            .all(|(expected, actual)| bytecode_matches(expected, actual))
        && expected.child_code_units().count() == actual.child_code_units().count()
}

impl BlueTsSafePointMapV1 {
    /// Verifies this map against its exact live BlueJS generation. The caller
    /// must separately compare the retained compiler/source fingerprints with
    /// its authorized page-load request before exposing the map.
    pub fn validate_against(
        &self,
        registry: &bluejs::BlueJsProgramRegistry,
        handle: bluejs::BlueJsProgramHandle,
    ) -> Result<(), BridgeError> {
        if self.format != BLUEJS_SAFE_POINT_MAP_ABI_V1
            || self.program_abi != bluejs::BLUEJS_PROGRAM_ABI_V1
            || self.program_generation != handle.generation().as_u64()
        {
            return Err(BridgeError::ProvenanceAttachment(
                "safe-point map ABI or generation does not match the live program".to_string(),
            ));
        }
        for entry in &self.entries {
            registry
                .validate_safe_point(
                    handle,
                    bluejs::BlueJsSafePoint {
                        code_unit: entry.code_unit,
                        bytecode_offset: entry.bytecode_offset,
                    },
                )
                .map_err(BridgeError::BlueJsDebug)?;
        }
        if !safe_point_entries_are_strictly_valid(&self.entries) {
            return Err(BridgeError::ProvenanceAttachment(
                "safe-point map entries are not sorted and unique".to_string(),
            ));
        }
        Ok(())
    }
}

fn build_safe_point_map(
    handle: bluejs::BlueJsProgramHandle,
    compiler_options_fingerprint: &str,
    sources: &[BridgeSource],
    provenance: &[AttachedLoweringProvenance],
) -> Result<BlueTsSafePointMapV1, BridgeError> {
    let mut entries = provenance
        .iter()
        .filter_map(|provenance| match provenance.safe_point {
            DirectSafePointBinding::Bound(safe_point) => Some(BlueTsSafePointEntryV1 {
                code_unit: safe_point.code_unit,
                bytecode_offset: safe_point.bytecode_offset,
                source: provenance.source.module.clone(),
                start_byte: provenance.source.start,
                end_byte: provenance.source.end,
                provenance_kind: provenance.kind,
            }),
            DirectSafePointBinding::Unbound => None,
        })
        .collect::<Vec<_>>();
    entries.sort_by(safe_point_entry_order);
    if !safe_point_entries_are_strictly_valid(&entries) {
        return Err(BridgeError::ProvenanceAttachment(
            "multiple lowering spans resolved to the same safe-point map entry".to_string(),
        ));
    }
    Ok(BlueTsSafePointMapV1 {
        format: BLUEJS_SAFE_POINT_MAP_ABI_V1,
        program_abi: bluejs::BLUEJS_PROGRAM_ABI_V1,
        program_generation: handle.generation().as_u64(),
        compiler_options_fingerprint: compiler_options_fingerprint.to_string(),
        source_set_hash: source_set_hash(sources),
        entries,
    })
}

fn safe_point_entries_are_strictly_valid(entries: &[BlueTsSafePointEntryV1]) -> bool {
    entries.windows(2).all(|pair| {
        safe_point_entry_order(&pair[0], &pair[1]).is_lt()
            && (pair[0].code_unit != pair[1].code_unit
                || pair[0].bytecode_offset != pair[1].bytecode_offset)
    })
}

fn safe_point_entry_order(
    left: &BlueTsSafePointEntryV1,
    right: &BlueTsSafePointEntryV1,
) -> std::cmp::Ordering {
    (
        left.code_unit.generation(),
        left.code_unit.ordinal(),
        left.bytecode_offset,
        &left.source,
        left.start_byte,
        left.end_byte,
        left.provenance_kind,
    )
        .cmp(&(
            right.code_unit.generation(),
            right.code_unit.ordinal(),
            right.bytecode_offset,
            &right.source,
            right.start_byte,
            right.end_byte,
            right.provenance_kind,
        ))
}

fn source_set_hash(sources: &[BridgeSource]) -> String {
    let mut source_identities = sources
        .iter()
        .map(|source| (&source.module, &source.content_hash))
        .collect::<Vec<_>>();
    source_identities.sort_unstable();
    let mut hash = 0xcbf29ce484222325u64;
    for (module, content_hash) in source_identities {
        for byte in module
            .bytes()
            .chain(std::iter::once(0xff))
            .chain(content_hash.bytes())
            .chain(std::iter::once(0xfe))
        {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
    }
    format!("bts-source-set-{hash:016x}")
}

fn lower_script(
    module: &Module,
) -> Result<(Vec<bluejs::Stmt>, Vec<LoweringProvenance>), BridgeError> {
    let mut body = Vec::new();
    let mut provenance = Vec::new();
    for declaration in &module.declarations {
        match declaration {
            Declaration::TypeAlias(_) | Declaration::Interface(_) | Declaration::TypeExport(_) => {}
            Declaration::Variable(variable) if !variable.declared && !variable.exported => {
                body.push(lower_variable(module, variable)?);
                provenance.push(LoweringProvenance {
                    source: variable.span.clone(),
                    kind: LoweringProvenanceKind::LoweredSyntax,
                });
            }
            Declaration::Raw(raw) => {
                body.push(bluejs::Stmt::Expr(
                    ExpressionLowerer::new(&module.id, &raw.tokens).parse()?,
                ));
                provenance.push(LoweringProvenance {
                    source: raw.span.clone(),
                    kind: LoweringProvenanceKind::Copied,
                });
            }
            Declaration::Import(import) if import.type_only => {}
            Declaration::DefaultExport(export) => {
                return Err(unsupported(
                    export.span.clone(),
                    "ESM default exports require the module bridge",
                ));
            }
            Declaration::ValueExport(export) => {
                return Err(unsupported(
                    export.span.clone(),
                    "ESM named exports require the module bridge",
                ));
            }
            Declaration::Import(import) => {
                return Err(unsupported(
                    import.span.clone(),
                    "runtime imports require the module bridge",
                ));
            }
            Declaration::Variable(variable) => {
                return Err(unsupported(
                    variable.span.clone(),
                    "declared or exported variables require a non-script bridge mode",
                ));
            }
            Declaration::Function(function)
                if !function.exported
                    && !function.default_export
                    && !function.declared
                    && !function.overload =>
            {
                body.push(lower_function(module, function)?);
                provenance.push(LoweringProvenance {
                    source: function.span.clone(),
                    kind: LoweringProvenanceKind::LoweredSyntax,
                });
            }
            Declaration::Function(function) => {
                return Err(unsupported(
                    function.span.clone(),
                    "declared, overloaded, or exported functions require a non-script bridge mode",
                ));
            }
        }
    }
    Ok((body, provenance))
}

fn lower_module(
    project: Option<&Project>,
    module: &Module,
) -> Result<(bluejs::Module, Vec<LoweringProvenance>), BridgeError> {
    let mut body = Vec::new();
    let mut imports = Vec::new();
    let mut exports = Vec::new();
    let mut requests = Vec::new();
    let mut provenance = Vec::new();
    for declaration in &module.declarations {
        match declaration {
            Declaration::TypeAlias(_) | Declaration::Interface(_) | Declaration::TypeExport(_) => {}
            Declaration::Variable(variable) if !variable.declared => {
                body.push(lower_variable(module, variable)?);
                provenance.push(LoweringProvenance {
                    source: variable.span.clone(),
                    kind: LoweringProvenanceKind::LoweredSyntax,
                });
                if variable.exported {
                    exports.push(bluejs::ExportEntry::Local {
                        export_name: variable.name.clone(),
                        local_name: variable.name.clone(),
                    });
                }
            }
            Declaration::Function(function) if !function.declared && !function.overload => {
                body.push(lower_function(module, function)?);
                provenance.push(LoweringProvenance {
                    source: function.span.clone(),
                    kind: LoweringProvenanceKind::LoweredSyntax,
                });
                if function.default_export {
                    exports.push(bluejs::ExportEntry::Local {
                        export_name: "default".to_string(),
                        local_name: function.name.clone(),
                    });
                } else if function.exported {
                    exports.push(bluejs::ExportEntry::Local {
                        export_name: function.name.clone(),
                        local_name: function.name.clone(),
                    });
                }
            }
            Declaration::Raw(raw) => {
                body.push(bluejs::Stmt::Expr(
                    ExpressionLowerer::new(&module.id, &raw.tokens).parse()?,
                ));
                provenance.push(LoweringProvenance {
                    source: raw.span.clone(),
                    kind: LoweringProvenanceKind::Copied,
                });
            }
            Declaration::DefaultExport(export) => exports.push(bluejs::ExportEntry::Local {
                export_name: "default".to_string(),
                local_name: export.name.clone(),
            }),
            Declaration::ValueExport(export) => {
                exports.extend(
                    export
                        .bindings
                        .iter()
                        .map(|binding| bluejs::ExportEntry::Local {
                            export_name: binding.exported.clone(),
                            local_name: binding.local.clone(),
                        }),
                );
            }
            Declaration::Import(import) if import.type_only => {}
            Declaration::Import(import) => {
                let Some(project) = project else {
                    return Err(unsupported(
                        import.span.clone(),
                        "runtime imports require the direct module-graph bridge",
                    ));
                };
                let request = project
                    .resolved_module(&module.id, &import.specifier)
                    .map(str::to_owned)
                    .ok_or_else(|| {
                        unsupported(
                            import.specifier_span.clone(),
                            "BlueTS did not retain a canonical target for this runtime import",
                        )
                    })?;
                if !requests.contains(&request) {
                    requests.push(request.clone());
                }
                if import.bindings.is_empty() {
                    imports.push(bluejs::ImportEntry {
                        module_request: request,
                        import_name: bluejs::ImportName::Named("default".to_string()),
                        local_name: None,
                        module_type: bluejs::ModuleType::JavaScript,
                    });
                } else {
                    imports.extend(import.bindings.iter().map(|binding| bluejs::ImportEntry {
                        module_request: request.clone(),
                        import_name: if binding.imported == "*" {
                            bluejs::ImportName::Namespace
                        } else {
                            bluejs::ImportName::Named(binding.imported.clone())
                        },
                        local_name: Some(binding.local.clone()),
                        module_type: bluejs::ModuleType::JavaScript,
                    }));
                }
            }
            Declaration::Variable(variable) => {
                return Err(unsupported(
                    variable.span.clone(),
                    "declared variables cannot be lowered to a direct module",
                ));
            }
            Declaration::Function(function) => {
                return Err(unsupported(
                    function.span.clone(),
                    "declared or overloaded functions cannot be lowered to a direct module",
                ));
            }
        }
    }
    Ok((
        bluejs::Module {
            body,
            imports,
            exports,
            // BlueTS has no `import defer` / source-phase syntax: every runtime
            // import is an ordinary evaluation-phase request.
            requests: requests
                .into_iter()
                .map(|specifier| bluejs::RequestedModule {
                    specifier,
                    module_type: bluejs::ModuleType::JavaScript,
                    phase: bluejs::ImportPhase::Evaluation,
                })
                .collect(),
        },
        provenance,
    ))
}

fn lower_function(
    module: &Module,
    function: &FunctionDeclaration,
) -> Result<bluejs::Stmt, BridgeError> {
    let mut params = Vec::with_capacity(function.parameters.len());
    for (index, parameter) in function.parameters.iter().enumerate() {
        if parameter.rest && index + 1 != function.parameters.len() {
            return Err(unsupported(
                parameter.span.clone(),
                "a rest parameter must be the final direct function parameter",
            ));
        }
        params.push(bluejs::Param {
            pattern: bluejs::Pattern::Identifier(parameter.name.clone()),
            default: parameter
                .default
                .as_deref()
                .map(|tokens| ExpressionLowerer::new(&module.id, tokens).parse())
                .transpose()?,
            rest: parameter.rest,
        });
    }

    let body = lower_function_body(module, &function.body)?;

    Ok(bluejs::Stmt::FunctionDecl(bluejs::Function {
        name: Some(function.name.clone()),
        params,
        body,
        generator: false,
        is_async: false,
    }))
}

fn lower_function_body(
    module: &Module,
    items: &[FunctionBodyItem],
) -> Result<Vec<bluejs::Stmt>, BridgeError> {
    let mut body = Vec::with_capacity(items.len());
    for item in items {
        match item {
            FunctionBodyItem::Variable(variable) => body.push(lower_variable(module, variable)?),
            FunctionBodyItem::Expression { tokens, .. } => body.push(bluejs::Stmt::Expr(
                ExpressionLowerer::new(&module.id, tokens).parse()?,
            )),
            FunctionBodyItem::Throw { tokens, .. } => body.push(bluejs::Stmt::Throw(
                ExpressionLowerer::new(&module.id, tokens).parse()?,
            )),
            FunctionBodyItem::Return { tokens, .. } => {
                let value = (!tokens.is_empty())
                    .then(|| ExpressionLowerer::new(&module.id, tokens).parse())
                    .transpose()?;
                body.push(bluejs::Stmt::Return(value));
            }
            FunctionBodyItem::If(statement) => body.push(lower_function_if(module, statement)?),
            FunctionBodyItem::Opaque(span) => {
                return Err(unsupported(
                    span.clone(),
                    "function body syntax is not yet in the v1 direct bridge subset",
                ));
            }
        }
    }
    Ok(body)
}

fn lower_function_if(
    module: &Module,
    statement: &FunctionIfStatement,
) -> Result<bluejs::Stmt, BridgeError> {
    let test = ExpressionLowerer::new(&module.id, &statement.test).parse()?;
    let consequent = Box::new(bluejs::Stmt::Block(lower_function_body(
        module,
        &statement.consequent,
    )?));
    let alternate = match &statement.alternate {
        Some(FunctionElseBranch::Braced(alternate)) => Some(Box::new(bluejs::Stmt::Block(
            lower_function_body(module, alternate)?,
        ))),
        Some(FunctionElseBranch::ElseIf(alternate)) => {
            Some(Box::new(lower_function_if(module, alternate)?))
        }
        None => None,
    };
    Ok(bluejs::Stmt::If {
        test,
        consequent,
        alternate,
    })
}

fn lower_variable(
    module: &Module,
    variable: &VariableDeclaration,
) -> Result<bluejs::Stmt, BridgeError> {
    let init = (!variable.initializer.is_empty())
        .then(|| ExpressionLowerer::new(&module.id, &variable.initializer).parse())
        .transpose()?;
    Ok(bluejs::Stmt::VarDecl(
        match variable.kind {
            VariableKind::Const => bluejs::DeclKind::Const,
            VariableKind::Let => bluejs::DeclKind::Let,
            VariableKind::Var => bluejs::DeclKind::Var,
        },
        vec![bluejs::VarDeclarator {
            pattern: bluejs::Pattern::Identifier(variable.name.clone()),
            init,
        }],
    ))
}

fn unsupported(span: SourceSpan, message: impl Into<String>) -> BridgeError {
    BridgeError::UnsupportedRuntimeTarget {
        span,
        message: message.into(),
    }
}

impl DirectScript {
    /// The BlueJS-owned AST ABI used by this direct compilation.
    pub fn program_abi(&self) -> &'static str {
        bluejs::BlueJsProgramV1::ABI
    }
}

impl DirectModule {
    /// The BlueJS-owned AST ABI used by this direct compilation.
    pub fn program_abi(&self) -> &'static str {
        bluejs::BlueJsProgramV1::ABI
    }
}

impl DirectModuleGraph {
    /// Clones graph bytecode into the map accepted by
    /// [`bluejs::Vm::execute_module_graph`]. Module IDs are the exact
    /// canonical identities selected by the caller-authorized BlueTS loader.
    pub fn bytecode_map(&self) -> HashMap<String, bluejs::Bytecode> {
        self.modules
            .iter()
            .map(|(id, module)| (id.clone(), module.bytecode.clone()))
            .collect()
    }
}

mod expression;
use expression::ExpressionLowerer;

#[cfg(test)]
mod tests;
