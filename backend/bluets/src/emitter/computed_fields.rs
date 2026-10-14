// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Capture erased field keys at class definition, in their original key order.

use super::*;
use crate::parser::{ClassDeclaration, NestedFunctionBody};
use crate::Token;

pub(super) fn capture(
    module: &Module,
    class: &ClassDeclaration,
    es2022: bool,
    edits: &mut Vec<TextEdit>,
) -> Result<BTreeMap<usize, String>, Diagnostic> {
    let mut keys = BTreeMap::new();
    let mut pending = Vec::new();
    let tokens = crate::lex(&module.id, &module.source).map_err(|mut errors| errors.remove(0))?;
    for member in &class.members {
        if member.abstract_modifier.is_some()
            || member.field.as_ref().is_some_and(|field| field.declared)
            || !member.key.first().is_some_and(|key| key.is("["))
        {
            continue;
        }
        let expression = super::class_lowering::render_tokens(
            &module.source,
            edits,
            &member.key[1..member.key.len() - 1],
        );
        if member.field.is_some() {
            let name = captured_key_name(&tokens, member.key[0].start);
            keys.insert(member.span.start, name.clone());
            pending.push(format!("{name} = {expression}"));
        } else if !pending.is_empty() {
            let first = member.key.first().unwrap().start;
            let last = member.key.last().unwrap().end;
            edits.retain(|edit| !(edit.start >= first && edit.end <= last));
            edits.push(TextEdit {
                start: first,
                end: last,
                replacement: format!("[({}, {expression})]", pending.join(", ")),
            });
            pending.clear();
        }
    }
    if !keys.is_empty() {
        let at = if module.class_expression(class.span.start).is_some() && !es2022 {
            class.span.start
        } else {
            enclosing_function_end(module, class).unwrap_or(0)
        };
        edits.push(TextEdit {
            start: at,
            end: at,
            replacement: format!(
                " var {}; ",
                keys.values().cloned().collect::<Vec<_>>().join(", ")
            ),
        });
    }
    if !pending.is_empty() {
        let at = if es2022 {
            class.body_span.start + 1
        } else {
            class.span.end
        };
        let statements = format!(" {}; ", pending.join("; "));
        edits.push(TextEdit {
            start: at,
            end: at,
            replacement: if es2022 {
                format!(" static {{{statements}}}")
            } else {
                statements
            },
        });
    }
    Ok(keys)
}

pub(in crate::emitter) fn captured_key_name(tokens: &[Token], start: usize) -> String {
    let mut name = format!("_btsKey{start}");
    while tokens.iter().any(|token| token.text == name) {
        name.push('_');
    }
    name
}

fn enclosing_function_end(module: &Module, class: &ClassDeclaration) -> Option<usize> {
    let mut bodies = Vec::new();
    for (declaration, _) in super::runtime_declarations(&module.declarations) {
        match declaration {
            Declaration::Function(function) if function.body_open.is_some() => {
                bodies.push(&function.span)
            }
            Declaration::Class(owner) => {
                for member in &owner.members {
                    if let Some(method) = &member.method {
                        if method.body.is_some() {
                            bodies.push(&method.span);
                        }
                    }
                    if let Some(constructor) = &member.constructor {
                        if constructor.body.is_some() {
                            bodies.push(&constructor.span);
                        }
                    }
                    if let Some(accessor) = &member.accessor {
                        bodies.push(&accessor.span);
                    }
                }
            }
            _ => {}
        }
    }
    bodies.extend(
        module
            .nested_functions
            .values()
            .filter(|function| matches!(function.body, NestedFunctionBody::Block { .. }))
            .map(|function| &function.span),
    );
    bodies
        .into_iter()
        .filter(|span| span.start < class.span.start && span.end > class.span.end)
        .min_by_key(|span| span.end - span.start)
        .map(|span| span.end - 1)
}
