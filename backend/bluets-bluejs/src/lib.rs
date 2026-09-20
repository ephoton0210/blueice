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
    compile, BlueTsDebugInfo, CompilerOptions, Declaration, Diagnostic, FunctionBodyItem,
    FunctionDeclaration, Module, ModuleLoader, Project, SourceSpan, Token, TokenKind,
    VariableDeclaration, VariableKind, LANGUAGE_VERSION,
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
        }
    }
}

impl std::error::Error for BridgeError {}

/// Parses, resolves and checks one host-authorized TypeScript source graph,
/// then directly constructs one BlueJS Script AST and bytecode unit.
///
/// The v1 direct subset has exactly one non-declaration module and no runtime
/// import/export entries. It accepts `var`/`let`/`const` declarations with an
/// optional literal/identifier/arithmetic initializer, named local functions
/// with required identifier parameters plus structured local/return bodies,
/// and standalone expressions made from those same forms or direct calls.
/// The expression subset includes unary, arithmetic, relational, equality,
/// `&&`, and `||` operators. Static-only declarations disappear before
/// lowering. A broader accepted BlueTS program returns
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
    for parameter in &function.parameters {
        if parameter.rest || parameter.optional {
            return Err(unsupported(
                parameter.span.clone(),
                "optional, default, and rest parameters are not yet in the v1 direct bridge subset",
            ));
        }
        params.push(bluejs::Param {
            pattern: bluejs::Pattern::Identifier(parameter.name.clone()),
            default: None,
            rest: false,
        });
    }

    let mut body = Vec::with_capacity(function.body.len());
    for item in &function.body {
        match item {
            FunctionBodyItem::Variable(variable) => body.push(lower_variable(module, variable)?),
            FunctionBodyItem::Return { tokens, .. } => {
                let value = (!tokens.is_empty())
                    .then(|| ExpressionLowerer::new(&module.id, tokens).parse())
                    .transpose()?;
                body.push(bluejs::Stmt::Return(value));
            }
            FunctionBodyItem::Opaque(span) => {
                return Err(unsupported(
                    span.clone(),
                    "function body syntax is not yet in the v1 direct bridge subset",
                ));
            }
        }
    }

    Ok(bluejs::Stmt::FunctionDecl(bluejs::Function {
        name: Some(function.name.clone()),
        params,
        body,
        generator: false,
        is_async: false,
    }))
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

struct ExpressionLowerer<'a> {
    module: &'a str,
    tokens: &'a [Token],
    index: usize,
}

impl<'a> ExpressionLowerer<'a> {
    fn new(module: &'a str, tokens: &'a [Token]) -> Self {
        Self {
            module,
            tokens,
            index: 0,
        }
    }

    fn parse(mut self) -> Result<bluejs::Expr, BridgeError> {
        let expression = self.parse_logical_or()?;
        if let Some(token) = self.tokens.get(self.index) {
            return Err(unsupported(
                self.token_span(token),
                format!("unsupported expression token `{}`", token.text),
            ));
        }
        Ok(expression)
    }

    fn parse_logical_or(&mut self) -> Result<bluejs::Expr, BridgeError> {
        let mut expression = self.parse_logical_and()?;
        while self
            .tokens
            .get(self.index)
            .is_some_and(|token| token.text == "||")
        {
            self.index += 1;
            expression = bluejs::Expr::Logical {
                op: bluejs::LogicalOp::Or,
                left: Box::new(expression),
                right: Box::new(self.parse_logical_and()?),
            };
        }
        Ok(expression)
    }

    fn parse_logical_and(&mut self) -> Result<bluejs::Expr, BridgeError> {
        let mut expression = self.parse_equality()?;
        while self
            .tokens
            .get(self.index)
            .is_some_and(|token| token.text == "&&")
        {
            self.index += 1;
            expression = bluejs::Expr::Logical {
                op: bluejs::LogicalOp::And,
                left: Box::new(expression),
                right: Box::new(self.parse_equality()?),
            };
        }
        Ok(expression)
    }

    fn parse_equality(&mut self) -> Result<bluejs::Expr, BridgeError> {
        let mut expression = self.parse_relational()?;
        while let Some(token) = self.tokens.get(self.index) {
            let op = match token.text.as_str() {
                "==" => bluejs::BinaryOp::Eq,
                "!=" => bluejs::BinaryOp::NotEq,
                "===" => bluejs::BinaryOp::StrictEq,
                "!==" => bluejs::BinaryOp::StrictNotEq,
                _ => break,
            };
            self.index += 1;
            expression = bluejs::Expr::Binary {
                op,
                left: Box::new(expression),
                right: Box::new(self.parse_relational()?),
            };
        }
        Ok(expression)
    }

    fn parse_relational(&mut self) -> Result<bluejs::Expr, BridgeError> {
        let mut expression = self.parse_additive()?;
        while let Some(token) = self.tokens.get(self.index) {
            let op = match token.text.as_str() {
                "<" => bluejs::BinaryOp::Lt,
                ">" => bluejs::BinaryOp::Gt,
                "<=" => bluejs::BinaryOp::LtEq,
                ">=" => bluejs::BinaryOp::GtEq,
                _ => break,
            };
            self.index += 1;
            expression = bluejs::Expr::Binary {
                op,
                left: Box::new(expression),
                right: Box::new(self.parse_additive()?),
            };
        }
        Ok(expression)
    }

    fn parse_additive(&mut self) -> Result<bluejs::Expr, BridgeError> {
        let mut expression = self.parse_multiplicative()?;
        while let Some(token) = self.tokens.get(self.index) {
            let op = match token.text.as_str() {
                "+" => bluejs::BinaryOp::Add,
                "-" => bluejs::BinaryOp::Sub,
                _ => break,
            };
            self.index += 1;
            expression = bluejs::Expr::Binary {
                op,
                left: Box::new(expression),
                right: Box::new(self.parse_multiplicative()?),
            };
        }
        Ok(expression)
    }

    fn parse_multiplicative(&mut self) -> Result<bluejs::Expr, BridgeError> {
        let mut expression = self.parse_unary()?;
        while let Some(token) = self.tokens.get(self.index) {
            let op = match token.text.as_str() {
                "*" => bluejs::BinaryOp::Mul,
                "/" => bluejs::BinaryOp::Div,
                "%" => bluejs::BinaryOp::Mod,
                _ => break,
            };
            self.index += 1;
            expression = bluejs::Expr::Binary {
                op,
                left: Box::new(expression),
                right: Box::new(self.parse_unary()?),
            };
        }
        Ok(expression)
    }

    fn parse_unary(&mut self) -> Result<bluejs::Expr, BridgeError> {
        let op = self
            .tokens
            .get(self.index)
            .and_then(|token| match token.text.as_str() {
                "!" => Some(bluejs::UnaryOp::Not),
                "+" => Some(bluejs::UnaryOp::Plus),
                "-" => Some(bluejs::UnaryOp::Neg),
                "~" => Some(bluejs::UnaryOp::BitNot),
                _ => None,
            });
        if let Some(op) = op {
            self.index += 1;
            return Ok(bluejs::Expr::Unary {
                op,
                arg: Box::new(self.parse_unary()?),
            });
        }
        self.parse_primary()
    }

    fn parse_primary(&mut self) -> Result<bluejs::Expr, BridgeError> {
        let expression = self.parse_atom()?;
        self.parse_call_suffixes(expression)
    }

    fn parse_atom(&mut self) -> Result<bluejs::Expr, BridgeError> {
        let Some(token) = self.tokens.get(self.index) else {
            return Err(unsupported(
                SourceSpan::new(self.module, 0, 0),
                "expected a runtime expression",
            ));
        };
        self.index += 1;
        match token.kind {
            TokenKind::Number => token
                .text
                .replace('_', "")
                .parse::<f64>()
                .map(bluejs::Expr::Number)
                .map_err(|_| unsupported(self.token_span(token), "unsupported numeric literal")),
            TokenKind::String => lower_string(self.module, token),
            TokenKind::Identifier => Ok(bluejs::Expr::Identifier(token.text.clone())),
            TokenKind::Keyword => match token.text.as_str() {
                "true" => Ok(bluejs::Expr::Bool(true)),
                "false" => Ok(bluejs::Expr::Bool(false)),
                "null" => Ok(bluejs::Expr::Null),
                "undefined" => Ok(bluejs::Expr::Identifier("undefined".to_string())),
                _ => Err(unsupported(
                    self.token_span(token),
                    format!(
                        "unsupported keyword `{}` in a runtime expression",
                        token.text
                    ),
                )),
            },
            TokenKind::Punct if token.text == "(" => {
                let expression = self.parse_logical_or()?;
                let Some(closing) = self.tokens.get(self.index) else {
                    return Err(unsupported(
                        self.token_span(token),
                        "unterminated parenthesized expression",
                    ));
                };
                if closing.text != ")" {
                    return Err(unsupported(
                        self.token_span(closing),
                        "expected `)` in runtime expression",
                    ));
                }
                self.index += 1;
                Ok(bluejs::Expr::Parenthesized(Box::new(expression)))
            }
            _ => Err(unsupported(
                self.token_span(token),
                format!("unsupported runtime expression token `{}`", token.text),
            )),
        }
    }

    fn parse_call_suffixes(
        &mut self,
        mut callee: bluejs::Expr,
    ) -> Result<bluejs::Expr, BridgeError> {
        while self
            .tokens
            .get(self.index)
            .is_some_and(|token| token.text == "(")
        {
            self.index += 1;
            let mut args = Vec::new();
            if self
                .tokens
                .get(self.index)
                .is_some_and(|token| token.text == ")")
            {
                self.index += 1;
            } else {
                loop {
                    args.push(bluejs::Argument::Normal(self.parse_logical_or()?));
                    let Some(separator) = self.tokens.get(self.index) else {
                        return Err(unsupported(
                            SourceSpan::new(self.module, 0, 0),
                            "unterminated call expression",
                        ));
                    };
                    match separator.text.as_str() {
                        "," => self.index += 1,
                        ")" => {
                            self.index += 1;
                            break;
                        }
                        _ => {
                            return Err(unsupported(
                                self.token_span(separator),
                                "expected `,` or `)` in call expression",
                            ));
                        }
                    }
                }
            }
            callee = bluejs::Expr::Call {
                callee: Box::new(callee),
                args,
            };
        }
        Ok(callee)
    }

    fn token_span(&self, token: &Token) -> SourceSpan {
        token_span(self.module, token)
    }
}

fn lower_string(module: &str, token: &Token) -> Result<bluejs::Expr, BridgeError> {
    let mut characters = token.text.chars();
    let quote = characters
        .next()
        .filter(|quote| matches!(quote, '\'' | '\"'))
        .ok_or_else(|| unsupported(token_span(module, token), "invalid string token"))?;
    let body = characters
        .next_back()
        .filter(|last| *last == quote)
        .map(|_| &token.text[quote.len_utf8()..token.text.len() - quote.len_utf8()])
        .ok_or_else(|| unsupported(token_span(module, token), "unterminated string token"))?;
    if body.contains('\\') {
        return Err(unsupported(
            token_span(module, token),
            "string escapes are not yet in the v1 direct bridge subset",
        ));
    }
    Ok(bluejs::Expr::String(body.into()))
}

fn token_span(module: &str, token: &Token) -> SourceSpan {
    SourceSpan::new(module, token.start, token.end)
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

#[cfg(test)]
mod tests {
    use super::*;
    use blueice_bluets::{MapLoader, ModuleSource};

    const ENTRY: &str = "memory:///direct.ts";
    const MODULE_ENTRY: &str = "memory:///direct-module.ts";
    const GRAPH_ENTRY: &str = "graph/main.ts";

    struct AliasedGraphLoader;

    impl ModuleLoader for AliasedGraphLoader {
        fn load(&self, module_id: &str) -> Result<ModuleSource, String> {
            match module_id {
                "virtual/main.ts" => Ok(ModuleSource::new(
                    module_id,
                    "import { value } from '@runtime'; \
                     export const answer: number = value + 1; answer;",
                )),
                "canonical/runtime.ts" => Ok(ModuleSource::new(
                    module_id,
                    "export const value: number = 41;",
                )),
                _ => Err(format!("unexpected module request `{module_id}`")),
            }
        }

        fn resolve(&self, _from_module: &str, specifier: &str) -> Result<String, String> {
            match specifier {
                "@runtime" => Ok("canonical/runtime.ts".to_string()),
                _ => Err(format!("unexpected import specifier `{specifier}`")),
            }
        }
    }

    #[test]
    fn lowers_typed_source_directly_to_bluejs_ast_and_bytecode() {
        let artifact = compile_direct_script(
            ENTRY,
            &MapLoader::from([ModuleSource::new(
                ENTRY,
                "type Count = number; var answer: Count = 40 + 2; answer;",
            )]),
            CompilerOptions::default(),
        )
        .unwrap();
        assert_eq!(artifact.bridge_abi, BLUE_TS_BLUEJS_BRIDGE_ABI_V1);
        assert_eq!(artifact.program_abi(), bluejs::BLUEJS_PROGRAM_ABI_V1);
        assert_eq!(artifact.provenance.len(), 2);
        assert_eq!(
            artifact.program,
            bluejs::BlueJsProgramV1::Script(bluejs::Program {
                body: vec![
                    bluejs::Stmt::VarDecl(
                        bluejs::DeclKind::Var,
                        vec![bluejs::VarDeclarator {
                            pattern: bluejs::Pattern::Identifier("answer".to_string()),
                            init: Some(bluejs::Expr::Binary {
                                op: bluejs::BinaryOp::Add,
                                left: Box::new(bluejs::Expr::Number(40.0)),
                                right: Box::new(bluejs::Expr::Number(2.0)),
                            }),
                        }],
                    ),
                    bluejs::Stmt::Expr(bluejs::Expr::Identifier("answer".to_string())),
                ],
            })
        );
        assert_eq!(
            bluejs::Vm::default().execute(&artifact.bytecode).unwrap(),
            bluejs::Value::Number(42.0)
        );
    }

    #[test]
    fn rejects_runtime_shapes_not_yet_lowered_without_reparsing_emitted_javascript() {
        let result = compile_direct_script(
            ENTRY,
            &MapLoader::from([ModuleSource::new(
                ENTRY,
                "const answer: number = 42; console.log(answer);",
            )]),
            CompilerOptions::default(),
        );
        let Err(error) = result else {
            panic!("the direct bridge must reject member access");
        };
        let BridgeError::UnsupportedRuntimeTarget { span, .. } = error else {
            panic!("the direct bridge must reject an unsupported runtime shape");
        };
        assert_eq!(span.module, ENTRY);
        assert!(span.start > 0);
    }

    #[test]
    fn lowers_typed_local_functions_and_direct_calls() {
        let artifact = compile_direct_script(
            ENTRY,
            &MapLoader::from([ModuleSource::new(
                ENTRY,
                "function add(left: number, right: number): number { \
                 const sum: number = left + right; return sum; } add(20, 22);",
            )]),
            CompilerOptions::default(),
        )
        .unwrap();
        assert!(matches!(
            artifact.program,
            bluejs::BlueJsProgramV1::Script(bluejs::Program { ref body })
                if matches!(body.as_slice(), [bluejs::Stmt::FunctionDecl(_), bluejs::Stmt::Expr(bluejs::Expr::Call { .. })])
        ));
        assert_eq!(
            bluejs::Vm::default().execute(&artifact.bytecode).unwrap(),
            bluejs::Value::Number(42.0)
        );
    }

    #[test]
    fn lowers_boolean_comparison_logical_and_unary_expressions() {
        let artifact = compile_direct_script(
            ENTRY,
            &MapLoader::from([ModuleSource::new(
                ENTRY,
                "function matches(value: number) { \
                 return !(value < 42) && ~0 === -1 && +value === 42; } matches(42);",
            )]),
            CompilerOptions::default(),
        )
        .unwrap();
        assert_eq!(
            bluejs::Vm::default().execute(&artifact.bytecode).unwrap(),
            bluejs::Value::Bool(true)
        );
    }

    #[test]
    fn gives_logical_and_higher_precedence_than_logical_or() {
        let artifact = compile_direct_script(
            ENTRY,
            &MapLoader::from([ModuleSource::new(
                ENTRY,
                "function isBelow(value: number) { \
                 return value < 42 || value === 42 && false; } isBelow(41);",
            )]),
            CompilerOptions::default(),
        )
        .unwrap();
        assert_eq!(
            bluejs::Vm::default().execute(&artifact.bytecode).unwrap(),
            bluejs::Value::Bool(true)
        );
    }

    #[test]
    fn refuses_to_silently_drop_an_unstructured_function_body_statement() {
        let result = compile_direct_script(
            ENTRY,
            &MapLoader::from([ModuleSource::new(
                ENTRY,
                "function answer(): number { unknown; return 42; } answer();",
            )]),
            CompilerOptions::default(),
        );
        let Err(BridgeError::UnsupportedRuntimeTarget { span, message }) = result else {
            panic!("the direct bridge must reject an opaque function body item");
        };
        assert_eq!(span.module, ENTRY);
        assert!(message.contains("function body syntax"));
    }

    #[test]
    fn lowers_local_named_and_default_exports_to_a_bluejs_module() {
        let artifact = compile_direct_module(
            MODULE_ENTRY,
            &MapLoader::from([ModuleSource::new(
                MODULE_ENTRY,
                "const answer: number = 40 + 2; \
                 export { answer as publicAnswer }; export default answer; answer;",
            )]),
            CompilerOptions::default(),
        )
        .unwrap();
        assert_eq!(artifact.bridge_abi, BLUE_TS_BLUEJS_BRIDGE_ABI_V1);
        let bluejs::BlueJsProgramV1::Module(module) = &artifact.program else {
            panic!("the direct module bridge must produce a BlueJS module AST");
        };
        assert!(matches!(
            module.body.as_slice(),
            [bluejs::Stmt::VarDecl(_, _), bluejs::Stmt::Expr(bluejs::Expr::Identifier(name))]
                if name == "answer"
        ));
        assert!(module.imports.is_empty());
        assert!(module.requests.is_empty());
        assert_eq!(
            module.exports,
            vec![
                bluejs::ExportEntry::Local {
                    export_name: "publicAnswer".to_string(),
                    local_name: "answer".to_string(),
                },
                bluejs::ExportEntry::Local {
                    export_name: "default".to_string(),
                    local_name: "answer".to_string(),
                },
            ]
        );
        assert_eq!(
            bluejs::Vm::default()
                .execute_module(&artifact.bytecode)
                .unwrap(),
            bluejs::Value::Number(42.0)
        );
    }

    #[test]
    fn preserves_bluets_resolved_targets_in_a_direct_module_graph() {
        let graph = compile_direct_module_graph(
            GRAPH_ENTRY,
            &MapLoader::from([
                ModuleSource::new(
                    GRAPH_ENTRY,
                    "import type { Shape } from './types.d.ts'; \
                     import { value } from './dep.ts'; \
                     export const answer: number = value + 1; answer;",
                ),
                ModuleSource::new("graph/dep.ts", "export const value: number = 41;"),
                ModuleSource::new(
                    "graph/types.d.ts",
                    "export interface Shape { label: string; }",
                ),
            ]),
            CompilerOptions::default(),
        )
        .unwrap();
        assert_eq!(graph.entry, GRAPH_ENTRY);
        assert_eq!(graph.modules.len(), 2);
        assert!(!graph.modules.contains_key("graph/types.d.ts"));
        let bluejs::BlueJsProgramV1::Module(main) = &graph.modules[GRAPH_ENTRY].program else {
            panic!("the direct graph entry must produce a BlueJS module AST");
        };
        assert_eq!(
            main.imports,
            vec![bluejs::ImportEntry {
                module_request: "graph/dep.ts".to_string(),
                import_name: bluejs::ImportName::Named("value".to_string()),
                local_name: Some("value".to_string()),
                json: false,
            }]
        );
        assert_eq!(main.requests, vec!["graph/dep.ts".to_string()]);
        assert_eq!(
            bluejs::Vm::default()
                .execute_module_graph(&graph.entry, &graph.bytecode_map())
                .unwrap(),
            bluejs::Value::Number(42.0)
        );
    }

    #[test]
    fn preserves_a_non_relative_caller_authorized_module_alias() {
        let graph = compile_direct_module_graph(
            "virtual/main.ts",
            &AliasedGraphLoader,
            CompilerOptions::default(),
        )
        .unwrap();
        let bluejs::BlueJsProgramV1::Module(main) = &graph.modules["virtual/main.ts"].program
        else {
            panic!("the direct graph entry must produce a BlueJS module AST");
        };
        assert_eq!(main.requests, vec!["canonical/runtime.ts".to_string()]);
        assert_eq!(
            bluejs::Vm::default()
                .execute_module_graph(&graph.entry, &graph.bytecode_map())
                .unwrap(),
            bluejs::Value::Number(42.0)
        );
    }
}
