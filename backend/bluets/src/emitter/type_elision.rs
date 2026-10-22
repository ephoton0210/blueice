// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Erase inline type bindings while retaining runtime bindings and graph edges.

use crate::compiler::ModuleKind;
use crate::parser::{Declaration, Module, TextEdit};
use crate::syntax::Token;
use std::borrow::Cow;

/// A value query is erased syntax. An ordinary import used only by these
/// retained query nodes must not require an executable binding in JavaScript.
/// Other uses keep their original binding, including unchecked value uses.
pub(super) fn queries<'a>(
    module: &'a Module,
    project: &crate::Project,
    options: &crate::CompilerOptions,
) -> Cow<'a, Module> {
    if !module.declarations.iter().any(|declaration| matches!(declaration,Declaration::Import(import) if !import.bindings.is_empty() && !import.is_type_only())) { return Cow::Borrowed(module); }
    let mut runtime_names = crate::checker::runtime_import_uses(
        project,
        module,
        options.limits.max_type_expansions,
        options.jsx,
        options.jsx_factory.as_deref(),
        options.jsx_fragment_factory.as_deref(),
    );
    // Imported decorator metadata remains a separately measured ABI boundary.
    // Do not erase an annotation's potential runtime class under that profile.
    if options.emit_decorator_metadata {
        runtime_names.extend(
            module
                .declarations
                .iter()
                .filter_map(|declaration| {
                    let Declaration::Import(import) = declaration else {
                        return None;
                    };
                    Some(import.bindings.iter().map(|binding| binding.local.clone()))
                })
                .flatten(),
        );
    }
    let mut result = module.clone();
    for declaration in &mut result.declarations {
        let Declaration::Import(import) = declaration else {
            continue;
        };
        if import.is_type_only() {
            continue;
        }
        for binding in &mut import.bindings {
            if !runtime_names.contains(binding.local.as_str()) {
                binding.type_only = true;
            }
        }
        if import.is_type_only() {
            result.edits.push(TextEdit {
                start: import.span.start,
                end: import.span.end,
                replacement: String::new(),
            });
        }
    }
    Cow::Owned(result)
}

pub(super) fn lower(module: &Module, kind: ModuleKind, edits: &mut Vec<TextEdit>) {
    let Ok(tokens) = crate::lex(&module.id, &module.source) else {
        return;
    };
    for declaration in &module.declarations {
        match declaration {
            Declaration::Import(import)
                if kind == ModuleKind::Esm
                    && !import.is_type_only()
                    && import.bindings.iter().any(|binding| binding.type_only) =>
            {
                let Some((open, close)) =
                    braces(&tokens, import.span.start, import.specifier_span.start)
                else {
                    continue;
                };
                let retained = import
                    .bindings
                    .iter()
                    .filter(|binding| !binding.type_only && binding.span.start > open.start);
                let names = retained
                    .map(|binding| &module.source[binding.span.start..binding.span.end])
                    .collect::<Vec<_>>();
                let (start, replacement) = if names.is_empty() {
                    let comma = tokens.iter().rev().find(|token| {
                        token.start >= import.span.start && token.end <= open.start && token.is(",")
                    });
                    (comma.map_or(open.start, |token| token.start), String::new())
                } else {
                    (open.start, format!("{{ {} }}", names.join(", ")))
                };
                edits.push(TextEdit {
                    start,
                    end: close.end,
                    replacement,
                });
            }
            Declaration::ValueExport(export) if export.is_type_only() => {
                edits.push(TextEdit {
                    start: export.span.start,
                    end: export.span.end,
                    replacement: String::new(),
                });
            }
            Declaration::ValueExport(export)
                if kind == ModuleKind::Esm
                    && export.bindings.iter().any(|binding| binding.type_only) =>
            {
                let end = export
                    .specifier_span
                    .as_ref()
                    .map_or(export.span.end, |span| span.start);
                let Some((open, close)) = braces(&tokens, export.span.start, end) else {
                    continue;
                };
                let names = export
                    .bindings
                    .iter()
                    .filter(|binding| !binding.type_only)
                    .map(|binding| &module.source[binding.span.start..binding.span.end])
                    .collect::<Vec<_>>();
                edits.push(TextEdit {
                    start: open.start,
                    end: close.end,
                    replacement: format!("{{ {} }}", names.join(", ")),
                });
            }
            _ => {}
        }
    }
}

fn braces(tokens: &[Token], start: usize, end: usize) -> Option<(&Token, &Token)> {
    let mut clause = tokens
        .iter()
        .filter(|token| token.start >= start && token.end <= end);
    let open = clause.find(|token| token.is("{"))?;
    let close = clause.find(|token| token.is("}"))?;
    Some((open, close))
}
