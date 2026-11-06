// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Decorated class-valued consts retain their target's native storage phase.
use super::*;
use crate::emitter::relocations;
use crate::parser::{VariableDeclaration, VariableKind};

pub(in crate::emitter) enum Storage {
    Original,
    Direct,
    Retained(usize),
}

pub(in crate::emitter) fn storage(
    module: &Module,
    variable: &VariableDeclaration,
    options: &CompilerOptions,
    tokens: &[Token],
) -> Storage {
    if options.experimental_decorators
        || variable.kind != VariableKind::Const
        || variable.pattern.is_some()
    {
        return Storage::Original;
    }
    let mut initializer = variable.initializer.as_slice();
    while initializer.first().is_some_and(|token| token.is("("))
        && initializer.last().is_some_and(|token| token.is(")"))
    {
        let mut depth = 0usize;
        let close = initializer.iter().position(|token| {
            if token.is("(") {
                depth += 1;
            } else if token.is(")") {
                depth -= 1;
            }
            depth == 0
        });
        if close != Some(initializer.len() - 1) {
            break;
        }
        initializer = &initializer[1..initializer.len() - 1];
    }
    let (Some(first), Some(last)) = (initializer.first(), initializer.last()) else {
        return Storage::Original;
    };
    let Some(expression) = module.class_expression(first.start) else {
        return Storage::Original;
    };
    let class = &expression.class;
    if class.span.end != last.end
        || class.decorators.is_empty()
            && class
                .members
                .iter()
                .all(|member| member.decorators.is_empty())
    {
        return Storage::Original;
    }
    if crate::emitter::decorators::lowers_class(class, options) {
        return Storage::Direct;
    }
    tokens
        .iter()
        .find(|token| {
            token.is("const") && token.start >= variable.span.start && token.end <= first.start
        })
        .map_or(Storage::Original, |token| Storage::Retained(token.start))
}

pub(super) fn retain(
    variable: &VariableDeclaration,
    keyword: usize,
    name: &Token,
    edits: &mut Vec<TextEdit>,
) {
    edits.push(TextEdit {
        start: variable.span.start,
        end: name.start,
        replacement: format!("{}const ", relocations::source_anchor(keyword)),
    });
    let anchor = relocations::source_anchor;
    edits.push(TextEdit {
        start: variable.span.end,
        end: variable.span.end,
        replacement: format!(
            "{} {}exports.{}{} = {};{} {}",
            anchor(variable.span.end),
            anchor(name.start),
            variable.name,
            anchor(name.end),
            variable.name,
            anchor(variable.span.end),
            relocations::unmapped(),
        ),
    });
}
