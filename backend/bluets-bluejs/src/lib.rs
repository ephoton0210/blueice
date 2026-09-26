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
    compile, lex, source_locations_for_spans, BlueTsDebugInfo, CompilerOptions,
    DebugSourceLocation, Declaration, Diagnostic, FunctionBodyItem, FunctionDeclaration,
    FunctionElseBranch, FunctionIfStatement, Module, ModuleLoader, Project, SourceId, SourceSpan,
    SymbolId, SymbolKind, Token, TokenKind, TypeId, VariableDeclaration, VariableKind,
    LANGUAGE_VERSION,
};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;

mod debug_attachment;
pub mod page_host_typings;
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
    /// Precomputed original-source coordinates. The source text is discarded
    /// before the program enters the live debugger registry.
    pub location: DebugSourceLocation,
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
    /// Checked, generation-bound static symbol/type IDs for unambiguous root
    /// declaration slots. This is not a runtime value or type assertion.
    root_symbol_slots: Vec<DirectRootSymbolSlot>,
}

/// One exact root-code-unit lexical slot joined to compiler-owned BlueTS
/// static evidence. It carries no runtime value or static type display.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DirectRootSymbolSlot {
    pub program: bluejs::BlueJsProgramHandle,
    pub code_unit: bluejs::BlueJsCodeUnitId,
    pub slot_ordinal: u32,
    pub source_id: SourceId,
    pub symbol_id: SymbolId,
    pub type_id: TypeId,
}

impl DirectProgramAttachment {
    /// Returns only structurally verified root declaration slots after
    /// rechecking the exact installed generation. A stale or moved attachment
    /// refuses before returning any static IDs; no runtime value is read.
    pub fn live_root_symbol_slots(
        &self,
        registry: &bluejs::BlueJsProgramRegistry,
    ) -> Result<&[DirectRootSymbolSlot], BridgeError> {
        validate_live_root_symbol_slots(
            registry,
            self.handle,
            &self.safe_point_map,
            &self.root_symbol_slots,
        )?;
        Ok(&self.root_symbol_slots)
    }

    /// Resolves a TypeScript UTF-8 byte position in this exact program
    /// generation. A verified nested instruction takes precedence over an
    /// overlapping root declaration; otherwise the nearest following span
    /// wins. An unbound selected span or exhausted source remains explicitly
    /// unbound. This never guesses a bytecode offset.
    pub fn breakpoint_at_or_after(
        &self,
        source: &str,
        source_byte: usize,
    ) -> DirectSafePointBinding {
        resolve_breakpoint_at_or_after(
            self.provenance
                .iter()
                .map(|provenance| {
                    (
                        provenance.source.module.as_str(),
                        provenance.source.start,
                        provenance.source.end,
                        provenance.safe_point,
                    )
                })
                .chain(verified_child_breakpoint_spans(&self.safe_point_map)),
            source,
            source_byte,
        )
    }
}

fn validate_live_root_symbol_slots(
    registry: &bluejs::BlueJsProgramRegistry,
    handle: bluejs::BlueJsProgramHandle,
    safe_point_map: &BlueTsSafePointMapV1,
    slots: &[DirectRootSymbolSlot],
) -> Result<(), BridgeError> {
    let compiled = registry.get(handle).map_err(BridgeError::BlueJsDebug)?;
    safe_point_map.validate_against(registry, handle)?;
    let root = compiled.code_units().first().ok_or_else(|| {
        BridgeError::ProvenanceAttachment("the installed program has no root code unit".to_string())
    })?;
    let root_slots = compiled.bytecode().root_declaration_binding_slots();
    let mut seen = BTreeSet::new();
    if slots.iter().any(|slot| {
        slot.program != handle
            || slot.code_unit != root.id()
            || !root_slots.contains(&Some(slot.slot_ordinal))
            || !seen.insert(slot.slot_ordinal)
    }) {
        return Err(BridgeError::ProvenanceAttachment(
            "root symbol slots do not belong to this live program".to_string(),
        ));
    }
    Ok(())
}

fn verified_child_breakpoint_spans(
    map: &BlueTsSafePointMapV1,
) -> impl Iterator<Item = (&str, usize, usize, DirectSafePointBinding)> {
    map.entries.iter().filter_map(|entry| {
        (entry.code_unit.ordinal() != 0).then_some((
            entry.source.as_str(),
            entry.start_byte,
            entry.end_byte,
            DirectSafePointBinding::Bound(bluejs::BlueJsSafePoint {
                code_unit: entry.code_unit,
                bytecode_offset: entry.bytecode_offset,
            }),
        ))
    })
}

fn resolve_breakpoint_at_or_after<'a>(
    spans: impl Iterator<Item = (&'a str, usize, usize, DirectSafePointBinding)>,
    source: &str,
    source_byte: usize,
) -> DirectSafePointBinding {
    // Prefer the most specific containing span, then the nearest following
    // span. Identical declaration/function ranges prefer the verified child
    // entry; multiple instructions in one range choose the earliest offset.
    spans
        .filter(|(module, _, end, _)| *module == source && *end > source_byte)
        .min_by_key(|(_, start, end, binding)| {
            let (ordinal, offset) = match binding {
                DirectSafePointBinding::Bound(point) => {
                    (point.code_unit.ordinal() as usize, point.bytecode_offset)
                }
                DirectSafePointBinding::Unbound => (0, u32::MAX),
            };
            (
                start.saturating_sub(source_byte),
                usize::MAX - start,
                *end,
                usize::MAX - ordinal,
                offset,
            )
        })
        .map_or(DirectSafePointBinding::Unbound, |(_, _, _, binding)| {
            binding
        })
}

/// One source-level lowering span attached to a generated BlueJS AST node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachedLoweringProvenance {
    /// The exact TypeScript source bytes that contributed the node.
    pub source: SourceSpan,
    /// How the source span contributed its generated AST statement.
    pub kind: LoweringProvenanceKind,
    /// Exact original-source UTF-16 coordinates from the checked artifact.
    pub location: DebugSourceLocation,
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
    /// Sorted, unique bound instructions owned by compiler-recorded root
    /// statement ranges or their direct child functions. Unbound spans remain
    /// in attachment provenance instead of acquiring guessed locations.
    pub entries: Vec<BlueTsSafePointEntryV1>,
}

/// One compiler-owned instruction mapped to its original TypeScript span.
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
    /// Original-source UTF-16 coordinates for this exact bound span.
    pub location: DebugSourceLocation,
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
            &self.debug_info,
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
            DirectArtifactProgram {
                program: &self.program,
                expected_bytecode: Some(&self.bytecode),
            },
            &self.provenance,
            &self.compiler_options_fingerprint,
            &self.debug_info,
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
            &self.debug_info,
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
            DirectArtifactProgram {
                program: &self.program,
                expected_bytecode: Some(&self.bytecode),
            },
            &self.provenance,
            &self.compiler_options_fingerprint,
            &self.debug_info,
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
/// array literals with holes or spread elements (but never both in one
/// literal), object literals with
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
    let contract_ids = symbols
        .iter()
        .filter_map(|symbol| symbol.contract)
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
        contracts: debug_info
            .contracts
            .iter()
            .filter(|contract| contract_ids.contains(&contract.id))
            .cloned()
            .collect(),
    }
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

mod attachment;
mod expression;
mod lowering;
use attachment::{
    attach_direct_program, attach_existing_direct_program, build_safe_point_map, source_identity,
    DirectArtifactProgram,
};
#[cfg(test)]
use expression::ExpressionLowerer;
use lowering::{lower_module, lower_script};

#[cfg(test)]
mod tests;
