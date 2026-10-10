// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Declaration-only inference checks use the admitted module's own syntax.

use crate::checker::CheckedProject;
use crate::diagnostic::{Diagnostic, DiagnosticCode, SourceSpan};
use crate::parser::{Declaration, Module, NestedFunctionBody, VariableDeclaration};
use crate::syntax::{Token, TokenKind};

pub(super) fn validate_options(options: &super::CompilerOptions) -> Vec<Diagnostic> {
    if options.declaration {
        return Vec::new();
    }
    [
        ("emitDeclarationOnly", options.emit_declaration_only),
        ("declarationMap", options.declaration_map),
        ("isolatedDeclarations", options.isolated_declarations),
    ]
    .into_iter()
    .filter(|(_, enabled)| *enabled)
    .map(|(name, _)| {
        Diagnostic::error(
            DiagnosticCode::TypeMismatch,
            SourceSpan::new("", 0, 0),
            "declaration options require declaration emission",
        )
        .with_typescript(
            5069,
            vec![name.into(), "declaration".into(), "composite".into()],
        )
    })
    .collect()
}

pub(super) fn check(project: &CheckedProject) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for checked in project.modules.values() {
        let module = &checked.module;
        if !super::is_declaration_module(&module.id) {
            declarations(module, &module.declarations, &mut diagnostics);
        }
    }
    diagnostics
}

fn declarations(module: &Module, declarations: &[Declaration], diagnostics: &mut Vec<Diagnostic>) {
    for declaration in declarations {
        match declaration {
            Declaration::Variable(variable)
                if variable.exported && variable.annotation.is_none() =>
            {
                variable_initializer(module, variable, diagnostics);
            }
            Declaration::Function(function)
                if (function.exported || function.default_export)
                    && !function.declared
                    && !function.overload
                    && function.return_type.is_none()
                    && !constant_returns(&function.returns) =>
            {
                diagnostics.push(error(
                    name_span(module, &function.span, &function.name),
                    9007,
                ));
            }
            Declaration::Namespace(namespace) => {
                self::declarations(module, &namespace.body, diagnostics);
            }
            _ => {}
        }
    }
}

fn variable_initializer(
    module: &Module,
    variable: &VariableDeclaration,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let tokens = &variable.initializer;
    let Some(first) = tokens.first() else {
        return;
    };
    if let Some(function) = module.nested_functions.get(&first.start) {
        let simple = match &function.body {
            NestedFunctionBody::Expression(tokens) => primitive(tokens),
            NestedFunctionBody::Block { returns, .. } => constant_returns(returns),
        };
        if function.return_type.is_none() && !simple {
            diagnostics.push(error(function.span.clone(), 9007));
        }
        return;
    }
    let assertion = tokens
        .windows(2)
        .any(|pair| pair[0].is("as") && pair[1].is("const"));
    if first.is("[") {
        if !assertion {
            let end = closing_delimiter(tokens, 0).map_or(first.end, |close| tokens[close].end);
            diagnostics.push(error(SourceSpan::new(&module.id, first.start, end), 9017));
        }
        return;
    }
    if first.is("{") {
        if let Some(index) = tokens.iter().position(|token| token.is("...")) {
            let mut depth = 0usize;
            let mut end = tokens[index].end;
            for token in &tokens[index + 1..] {
                if depth == 0 && (token.is(",") || token.is("}")) {
                    break;
                }
                if matches!(token.text.as_str(), "(" | "[" | "{") {
                    depth += 1;
                } else if matches!(token.text.as_str(), ")" | "]" | "}") {
                    depth = depth.saturating_sub(1);
                }
                end = token.end;
            }
            diagnostics.push(error(
                SourceSpan::new(&module.id, tokens[index].start, end),
                9015,
            ));
        }
        return;
    }
    if !primitive(tokens) && !assertion {
        diagnostics.push(error(
            name_span(module, &variable.span, &variable.name),
            9010,
        ));
    }
}

fn primitive(tokens: &[Token]) -> bool {
    match tokens {
        [token] => {
            matches!(token.kind, TokenKind::Number | TokenKind::String)
                || matches!(token.text.as_str(), "true" | "false" | "null")
        }
        [sign, number] => (sign.is("-") || sign.is("+")) && number.kind == TokenKind::Number,
        _ => false,
    }
}

fn constant_returns(returns: &[Vec<Token>]) -> bool {
    !returns.is_empty() && returns.iter().all(|tokens| primitive(tokens))
}

fn closing_delimiter(tokens: &[Token], open: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate().skip(open) {
        if matches!(token.text.as_str(), "(" | "[" | "{") {
            depth += 1;
        } else if matches!(token.text.as_str(), ")" | "]" | "}") {
            depth = depth.checked_sub(1)?;
            if depth == 0 {
                return Some(index);
            }
        }
    }
    None
}

fn name_span(module: &Module, span: &SourceSpan, name: &str) -> SourceSpan {
    crate::syntax::lex(&module.id, &module.source[span.start..span.end])
        .ok()
        .and_then(|tokens| tokens.into_iter().find(|token| token.text == name))
        .map_or_else(
            || span.clone(),
            |token| SourceSpan::new(&module.id, span.start + token.start, span.start + token.end),
        )
}

fn error(span: SourceSpan, code: u32) -> Diagnostic {
    Diagnostic::error(
        DiagnosticCode::TypeMismatch,
        span,
        "isolated declaration inference requires an explicit annotation",
    )
    .with_typescript(code, Vec::new())
}
