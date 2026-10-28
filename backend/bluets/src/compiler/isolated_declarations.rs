// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Isolated declaration inference is authorized by the local syntax alone.
use crate::checker::CheckedProject;
use crate::diagnostic::{Diagnostic, DiagnosticCode, SourceSpan, TypeScriptRelatedInformation};
use crate::parser::{Declaration, Module, NestedFunctionBody, Parameter, VariableDeclaration};
use crate::syntax::{Token, TokenKind};
use std::collections::BTreeSet;
mod literals;
mod members;
pub(super) const VERSION: &str = "isolated-declarations-v2";

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
        if !super::is_declaration_module(&checked.module.id) {
            declarations(
                &checked.module,
                &checked.module.declarations,
                &mut diagnostics,
            );
        }
    }
    diagnostics
}

fn declarations(module: &Module, items: &[Declaration], out: &mut Vec<Diagnostic>) {
    let public: BTreeSet<_> = items
        .iter()
        .flat_map(|item| match item {
            Declaration::ValueExport(export) if export.specifier.is_none() => export
                .bindings
                .iter()
                .filter(|b| !b.type_only)
                .map(|b| b.local.clone())
                .collect(),
            Declaration::DefaultExport(export) if !export.expression => vec![export.name.clone()],
            _ => Vec::new(),
        })
        .collect();
    for item in items {
        match item {
            Declaration::Variable(variable)
                if (variable.exported || public.contains(&variable.name))
                    && variable.annotation.is_none() =>
            {
                variable_initializer(module, variable, out);
            }
            Declaration::Function(function)
                if function.exported
                    || function.default_export
                    || public.contains(&function.name) =>
            {
                if function.declared || function.overload {
                    continue;
                }
                parameters(module, &function.parameters, out);
                if function.return_type.is_none() && !items.iter().any(|item|matches!(item,Declaration::Function(other) if other.name==function.name && other.overload)) {
                    returns(module,&function.returns,Target::new(name_span(module,&function.span,&function.name),&function.name,9007,9031),out);
                }
            }
            Declaration::Class(class) if class.exported || public.contains(&class.name) => {
                members::check(module, class, out)
            }
            Declaration::Namespace(namespace)
                if namespace.exported || public.contains(&namespace.name) =>
            {
                declarations(module, &namespace.body, out)
            }
            Declaration::DefaultExport(export) if export.expression => {
                let tokens = tokens(module, &export.span);
                let expression = &tokens[2..];
                let end =
                    expression.len() - usize::from(expression.last().is_some_and(|t| t.is(";")));
                if !literals::simple(&expression[..end]) {
                    out.push(issue(
                        Target::new(export.span.clone(), "", 9037, 9036),
                        span(module, &expression[..end]),
                        9037,
                    ));
                }
            }
            _ => {}
        }
    }
}

fn variable_initializer(
    module: &Module,
    variable: &VariableDeclaration,
    out: &mut Vec<Diagnostic>,
) {
    let target = Target::new(
        name_span(module, &variable.span, &variable.name),
        &variable.name,
        9010,
        9027,
    );
    let tokens = &variable.initializer;
    if let Some(function) = tokens
        .first()
        .and_then(|token| module.nested_functions.get(&token.start))
    {
        parameters(module, &function.parameters, out);
        if function.return_type.is_none() {
            let target = Target::new(function.span.clone(), "", 9007, 9031);
            match &function.body {
                NestedFunctionBody::Expression(tokens) => {
                    returns(module, std::slice::from_ref(tokens), target, out)
                }
                NestedFunctionBody::Block {
                    returns: returned, ..
                } => returns(module, returned, target, out),
            }
        }
    } else if let Some((code, location)) = literals::failure(module, tokens) {
        let location = if code == 9013 {
            target.location.clone()
        } else {
            location
        };
        out.push(issue(
            target,
            location,
            if code == 9013 { 9010 } else { code },
        ));
    }
}

fn parameters(module: &Module, parameters: &[Parameter], out: &mut Vec<Diagnostic>) {
    for parameter in parameters {
        if parameter.annotation.is_none() {
            if let Some(default) = &parameter.default {
                if !literals::simple(default) {
                    out.push(issue(
                        Target::new(parameter.span.clone(), &parameter.name, 9011, 9028),
                        span(module, default),
                        9011,
                    ));
                }
            }
        }
    }
}

fn returns(module: &Module, returned: &[Vec<Token>], target: Target, out: &mut Vec<Diagnostic>) {
    for tokens in returned {
        if tokens.is_empty() {
            continue;
        }
        if let Some((code, location)) = literals::failure(module, tokens) {
            let specialized = matches!(code, 9015 | 9016 | 9017 | 9038);
            out.push(issue(
                target.clone(),
                if specialized {
                    location
                } else {
                    target.location.clone()
                },
                if specialized { code } else { target.code },
            ));
            return;
        }
    }
}

#[derive(Clone)]
struct Target {
    location: SourceSpan,
    name: String,
    code: u32,
    related: u32,
}
impl Target {
    fn new(location: SourceSpan, name: &str, code: u32, related: u32) -> Self {
        Self {
            location,
            name: name.into(),
            code,
            related,
        }
    }
}
fn issue(target: Target, location: SourceSpan, code: u32) -> Diagnostic {
    let mut diagnostic = Diagnostic::error(
        DiagnosticCode::TypeMismatch,
        location,
        "isolated declaration inference requires an explicit annotation",
    )
    .with_typescript(code, Vec::new());
    if let Some(counterpart) = &mut diagnostic.typescript {
        let args = if matches!(target.related, 9027..=9030) {
            vec![target.name]
        } else {
            Vec::new()
        };
        let context = Diagnostic::error(DiagnosticCode::TypeMismatch, target.location.clone(), "")
            .with_typescript(target.related, args);
        if let Some(context) = context.typescript {
            counterpart
                .related_information
                .push(TypeScriptRelatedInformation {
                    code: context.code,
                    message: context.message,
                    span: target.location,
                    position: None,
                });
        }
    }
    diagnostic
}
fn tokens(module: &Module, location: &SourceSpan) -> Vec<Token> {
    crate::syntax::lex(&module.id, &module.source[location.start..location.end])
        .unwrap_or_default()
        .into_iter()
        .map(|mut token| {
            token.start += location.start;
            token.end += location.start;
            token
        })
        .filter(|token| token.kind != TokenKind::Eof)
        .collect()
}
fn span(module: &Module, tokens: &[Token]) -> SourceSpan {
    SourceSpan::new(
        &module.id,
        tokens.first().map_or(0, |t| t.start),
        tokens.last().map_or(0, |t| t.end),
    )
}
fn name_span(module: &Module, location: &SourceSpan, name: &str) -> SourceSpan {
    tokens(module, location)
        .into_iter()
        .find(|token| token.text == name)
        .map_or_else(|| location.clone(), |token| token.span(&module.id))
}
