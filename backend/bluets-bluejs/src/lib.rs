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

/// The first directly executable BlueTS-to-BlueJS bridge ABI.
pub const BLUE_TS_BLUEJS_BRIDGE_ABI_V1: &str = "blue-ts-bluejs-bridge-v1";

/// A source identity retained alongside the structured program. Source text is
/// intentionally absent: the bridge preserves only the compiler-provided hash
/// and canonical module ID needed to reject stale attachments later.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeSource {
    pub module: String,
    pub content_hash: String,
}

/// A direct-lowering span. It identifies a BlueTS source range that supplied
/// at least one executable BlueJS AST statement, not a bytecode safe point.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoweringProvenance {
    pub source: SourceSpan,
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
    InvalidSourceIdentity(String),
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
            Self::InvalidSourceIdentity(message) => {
                write!(
                    formatter,
                    "BlueTS direct program has no usable source identity: {message}"
                )
            }
        }
    }
}

impl std::error::Error for BridgeError {}

impl DirectScript {
    /// Installs this already compiled direct program in a caller-owned BlueJS
    /// registry. The bridge supplies the exact canonical source identity that
    /// BlueTS checked, without an emitted-JavaScript parse or recompilation.
    pub fn install_in(
        &self,
        registry: &mut bluejs::BlueJsProgramRegistry,
    ) -> Result<bluejs::BlueJsProgramHandle, BridgeError> {
        install_precompiled_direct_program(registry, &self.sources, &self.bytecode)
    }
}

impl DirectModule {
    /// Installs this already compiled direct module in a caller-owned BlueJS
    /// registry, retaining its BlueTS-authorized canonical source identity.
    pub fn install_in(
        &self,
        registry: &mut bluejs::BlueJsProgramRegistry,
    ) -> Result<bluejs::BlueJsProgramHandle, BridgeError> {
        install_precompiled_direct_program(registry, &self.sources, &self.bytecode)
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
        compiler_options_fingerprint: debug_info.compiler_options_hash,
        sources,
        provenance,
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
        compiler_options_fingerprint: debug_info.compiler_options_hash,
        sources,
        provenance,
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
        compiler_options_fingerprint: debug_info.compiler_options_hash,
        sources,
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
    if compilation.project.modules.len() != 1 {
        return Err(unsupported(
            SourceSpan::new(entry, 0, 0),
            "the v1 direct bridge supports exactly one source module",
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

fn install_precompiled_direct_program(
    registry: &mut bluejs::BlueJsProgramRegistry,
    sources: &[BridgeSource],
    bytecode: &bluejs::Bytecode,
) -> Result<bluejs::BlueJsProgramHandle, BridgeError> {
    let [source] = sources else {
        return Err(BridgeError::InvalidSourceIdentity(
            "a direct script or module must retain exactly one source".to_string(),
        ));
    };
    let source =
        bluejs::BlueJsSourceIdentity::new(source.module.clone(), source.content_hash.clone())
            .map_err(BridgeError::BlueJsDebug)?;
    registry
        .install_precompiled(source, bytecode.clone())
        .map_err(BridgeError::BlueJsDebug)
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
                });
            }
            Declaration::Raw(raw) => {
                body.push(bluejs::Stmt::Expr(
                    ExpressionLowerer::new(&module.id, &raw.tokens).parse()?,
                ));
                provenance.push(LoweringProvenance {
                    source: raw.span.clone(),
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
                        json: false,
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
                        json: false,
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
            requests,
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
