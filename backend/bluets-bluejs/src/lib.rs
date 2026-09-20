// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Direct, host-neutral BlueTS to BlueJS structured-program lowering.
//!
//! This is the only crate permitted to depend on both language crates. It
//! consumes BlueTS's checked, already-tokenized declarations and constructs
//! BlueJS AST nodes directly; it never receives or reparses BlueTSC JavaScript
//! emission. The first executable bridge deliberately covers only classic
//! scripts with typed variable declarations and bounded runtime expressions.

use blueice_bluejs as bluejs;
use blueice_bluets::{
    compile, CompilerOptions, Declaration, Diagnostic, Module, ModuleLoader, SourceSpan, Token,
    TokenKind, VariableDeclaration, VariableKind, LANGUAGE_VERSION,
};
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
/// optional literal/identifier/arithmetic initializer and standalone
/// expressions made from those same forms. Static-only declarations disappear
/// before lowering. A broader accepted BlueTS program returns
/// [`BridgeError::UnsupportedRuntimeTarget`] instead of falling back to a
/// JavaScript text round trip.
pub fn compile_direct_script(
    entry: &str,
    loader: &dyn ModuleLoader,
    options: CompilerOptions,
) -> Result<DirectScript, BridgeError> {
    let compilation = compile(entry, loader, options);
    if compilation.has_errors() {
        return Err(BridgeError::BlueTs(compilation.diagnostics));
    }
    let debug_info = compilation
        .debug_info
        .expect("a successful BlueTS compilation always has debug information");
    if compilation.project.modules.len() != 1 {
        return Err(unsupported(
            SourceSpan::new(entry, 0, 0),
            "the v1 direct bridge supports exactly one runtime module",
        ));
    }
    let module = compilation.project.modules.get(entry).ok_or_else(|| {
        unsupported(
            SourceSpan::new(entry, 0, 0),
            "the requested entry was not retained in the checked source graph",
        )
    })?;
    if module.id.ends_with(".d.ts") {
        return Err(unsupported(
            SourceSpan::new(entry, 0, 0),
            "a declaration module cannot be executed as a direct script",
        ));
    }

    let (body, provenance) = lower_script(module)?;
    let program = bluejs::BlueJsProgramV1::Script(bluejs::Program { body });
    let bytecode = program.compile().map_err(BridgeError::BlueJs)?;
    let sources = debug_info
        .sources
        .into_iter()
        .map(|source| BridgeSource {
            module: source.module,
            content_hash: source.content_hash,
        })
        .collect();
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
            Declaration::Function(function) => {
                return Err(unsupported(
                    function.span.clone(),
                    "function lowering is not yet in the v1 direct bridge subset",
                ));
            }
        }
    }
    Ok((body, provenance))
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
        let expression = self.parse_additive()?;
        if let Some(token) = self.tokens.get(self.index) {
            return Err(unsupported(
                self.token_span(token),
                format!("unsupported expression token `{}`", token.text),
            ));
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
        let mut expression = self.parse_primary()?;
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
                right: Box::new(self.parse_primary()?),
            };
        }
        Ok(expression)
    }

    fn parse_primary(&mut self) -> Result<bluejs::Expr, BridgeError> {
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
                let expression = self.parse_additive()?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use blueice_bluets::{MapLoader, ModuleSource};

    const ENTRY: &str = "memory:///direct.ts";

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
}
