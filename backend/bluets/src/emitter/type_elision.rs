// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Erase inline type bindings while retaining runtime bindings and graph edges.

use crate::compiler::ModuleKind;
use crate::parser::{Declaration, Module, TextEdit};
use crate::syntax::{Token, TokenKind};
use std::borrow::Cow;
use std::collections::BTreeSet;

/// A value query is erased syntax. An ordinary import used only by these
/// retained query nodes must not require an executable binding in JavaScript.
/// Other uses keep their original binding, including unchecked value uses.
pub(super) fn queries(module: &Module) -> Cow<'_, Module> {
    let query_names = module
        .type_references
        .iter()
        .filter(|reference| reference.value_query)
        .map(|reference| reference.name.split('.').next().unwrap_or(&reference.name))
        .collect::<BTreeSet<_>>();
    if query_names.is_empty() {
        return Cow::Borrowed(module);
    }
    let Ok(tokens) = crate::lex(&module.id, &module.source) else {
        return Cow::Borrowed(module);
    };
    let mut ranges = module
        .type_references
        .iter()
        .filter(|reference| reference.value_query)
        .map(|reference| (reference.span.start, reference.span.end))
        .chain(module.declarations.iter().filter_map(|declaration| {
            let Declaration::Import(import) = declaration else {
                return None;
            };
            Some((import.span.start, import.span.end))
        }))
        .collect::<Vec<_>>();
    ranges.sort_unstable();
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (start, end) in ranges {
        if let Some(last) = merged.last_mut().filter(|last| start <= last.1) {
            last.1 = last.1.max(end);
        } else {
            merged.push((start, end));
        }
    }
    let runtime_names = tokens
        .iter()
        .filter(|token| token.kind == TokenKind::Identifier)
        .filter(|token| {
            merged
                .partition_point(|range| range.0 <= token.start)
                .checked_sub(1)
                .is_none_or(|index| token.end > merged[index].1)
        })
        .map(|token| token.text.as_str())
        .collect::<BTreeSet<_>>();
    let mut result = module.clone();
    for declaration in &mut result.declarations {
        let Declaration::Import(import) = declaration else {
            continue;
        };
        if import.is_type_only() {
            continue;
        }
        for binding in &mut import.bindings {
            if query_names.contains(binding.local.as_str())
                && !runtime_names.contains(binding.local.as_str())
            {
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
