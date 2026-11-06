// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Only original auto-accessors may give generated private stores source roles.

use crate::emitter::private_lowering::PrivateNames;
use crate::emitter::{
    is_line_break, relocations, runtime_declarations, EmittedJavaScript, TextEdit,
};
use crate::parser::{
    ClassAccessor, ClassDeclaration, ClassField, ClassMemberShell, Declaration, Module,
};
use std::collections::BTreeMap;
mod completion;
mod publication;
pub(in crate::emitter) use completion::mapped_generated_class_edits;
pub(in crate::emitter::class_lowering) use publication::retain_publication_order;

type Position = (usize, usize);

#[derive(Clone, Copy)]
pub(in crate::emitter::class_lowering) struct AutoField {
    class_start: usize,
    name: usize,
    name_end: usize,
    completion: usize,
    original_completion: usize,
    legacy: bool,
}

fn position(source: &str, end: usize) -> Position {
    let (mut line, mut column) = (0, 0);
    let mut characters = source[..end].chars().peekable();
    while let Some(character) = characters.next() {
        if is_line_break(character, characters.peek().copied()) {
            line += 1;
            column = 0;
        } else {
            column += character.len_utf16();
        }
    }
    (line, column)
}

fn mapped(previous: &EmittedJavaScript, offset: usize) -> Option<Position> {
    let position = position(&previous.javascript, offset);
    let index = previous
        .provenance
        .partition_point(|segment| (segment.generated_line, segment.generated_column) < position);
    previous
        .provenance
        .get(index)
        .filter(|segment| {
            segment.mapped && (segment.generated_line, segment.generated_column) == position
        })
        .map(|segment| (segment.source_line, segment.source_column))
}

fn classes(module: &Module) -> Vec<&ClassDeclaration> {
    runtime_declarations(&module.declarations)
        .into_iter()
        .filter_map(|(declaration, _)| {
            if let Declaration::Class(class) = declaration {
                Some(class)
            } else {
                None
            }
        })
        .chain(
            module
                .class_expressions
                .values()
                .map(|expression| &expression.class),
        )
        .collect()
}

fn storage<'a>(
    class: &'a ClassDeclaration,
    shell: &ClassMemberShell,
    accessor: &ClassAccessor,
) -> Option<&'a str> {
    let tokens = &class.body[shell.token_start..shell.token_end];
    let open = tokens.iter().position(|token| token.is("{"))?;
    let body = &tokens[open + 1..];
    let private = if accessor.getter {
        if body.len() != 6
            || !body[0].is("return")
            || !body[2].is(".")
            || !body[4].is(";")
            || !body[5].is("}")
        {
            return None;
        }
        &body[3]
    } else {
        if body.len() != 7
            || !body[1].is(".")
            || !body[3].is("=")
            || !body[5].is(";")
            || !body[6].is("}")
            || accessor.parameters.len() != 1
            || body[4].text != accessor.parameters[0].name
        {
            return None;
        }
        &body[2]
    };
    private
        .text
        .starts_with('#')
        .then_some(private.text.as_str())
}

pub(in crate::emitter::class_lowering) fn collect(
    original: &Module,
    owned: &Module,
    previous: &EmittedJavaScript,
    legacy: bool,
) -> BTreeMap<usize, AutoField> {
    if previous.segment_limit.is_none() || previous.javascript != owned.source {
        return BTreeMap::new();
    }
    let mut identities = BTreeMap::new();
    for class in classes(original) {
        for shell in &class.members {
            if let Some(field) = shell
                .field
                .as_ref()
                .filter(|field| field.accessor && !field.declared && !field.name.starts_with('#'))
            {
                identities.insert(
                    (
                        position(&original.source, field.name_span.start),
                        position(&original.source, field.name_span.end),
                        field.is_static,
                        field.name.clone(),
                    ),
                    (class, field, shell),
                );
            }
        }
    }
    let mut result = BTreeMap::new();
    for class in classes(owned) {
        for shell in &class.members {
            let Some(getter) = shell
                .accessor
                .as_ref()
                .filter(|accessor| accessor.getter && accessor.parameters.is_empty())
            else {
                continue;
            };
            let (Some(start), Some(end)) = (
                mapped(previous, getter.name_span.start),
                mapped(previous, getter.name_span.end),
            ) else {
                continue;
            };
            let Some((original_class, _, original_shell)) =
                identities.get(&(start, end, getter.is_static, getter.name.clone()))
            else {
                continue;
            };
            if mapped(previous, getter.span.end)
                != Some(position(&original.source, original_shell.span.end))
            {
                continue;
            }
            let Some(name) = storage(class, shell, getter) else {
                continue;
            };
            let setters = class
                .members
                .iter()
                .filter_map(|member| member.accessor.as_ref().map(|accessor| (member, accessor)))
                .filter(|(_, accessor)| {
                    !accessor.getter
                        && accessor.is_static == getter.is_static
                        && accessor.name == getter.name
                })
                .collect::<Vec<_>>();
            if setters.len() != 1
                || storage(class, setters[0].0, setters[0].1) != Some(name)
                || mapped(previous, setters[0].1.name_span.start) != Some(start)
                || mapped(previous, setters[0].1.name_span.end) != Some(end)
            {
                continue;
            }
            let fields = class
                .members
                .iter()
                .filter_map(|member| member.field.as_ref())
                .filter(|field| field.name == name && field.is_static == getter.is_static)
                .collect::<Vec<_>>();
            if fields.len() == 1 {
                result.insert(
                    fields[0].name_span.start,
                    AutoField {
                        class_start: original_class.span.start,
                        name: getter.name_span.start,
                        name_end: getter.name_span.end,
                        completion: getter.span.end,
                        original_completion: original_shell.span.end,
                        legacy,
                    },
                );
            }
        }
    }
    result
}

fn initializer(
    source: &str,
    edits: &[TextEdit],
    field: &ClassField,
    origin: AutoField,
) -> (String, Vec<TextEdit>) {
    let Some(tokens) = field.initializer.as_deref() else {
        return (
            format!("{}void 0", relocations::source_anchor(origin.name_end)),
            Vec::new(),
        );
    };
    let first = tokens.first().expect("an initializer contains tokens");
    let last = tokens.last().expect("an initializer contains tokens");
    let (value, nested) = super::super::render_relocated_span(source, edits, first.start, last.end);
    (
        format!("{value}{}", relocations::source_anchor(last.end)),
        nested,
    )
}

pub(in crate::emitter::class_lowering) fn instance(
    source: &str,
    edits: &[TextEdit],
    field: &ClassField,
    private: &PrivateNames<'_>,
    origin: AutoField,
) -> (String, Vec<TextEdit>) {
    let (value, nested) = initializer(source, edits, field, origin);
    (
        format!(
            "{}{}{}",
            relocations::source_anchor(origin.name),
            private.field_store_value(&field.name, Some(value)),
            relocations::source_anchor(origin.completion)
        ),
        nested,
    )
}

pub(in crate::emitter::class_lowering) fn static_statement(
    source: &str,
    edits: &[TextEdit],
    field: &ClassField,
    private: &PrivateNames<'_>,
    class_name: &str,
    origin: AutoField,
) -> Result<(String, Vec<TextEdit>), ()> {
    let (value, nested) = initializer(source, edits, field, origin);
    let value = super::super::bind_static_this(class_name, super::super::tokens_of(field), &value)?;
    let statement = private.static_field_store(&field.name, Some(value));
    let completion = relocations::source_anchor(if origin.legacy {
        origin.name_end
    } else {
        origin.completion
    });
    Ok((
        format!(
            " {}{}{completion};{completion}",
            relocations::source_anchor(origin.name),
            statement
                .strip_prefix(' ')
                .expect("private store leading separator")
                .strip_suffix(';')
                .expect("private store statement terminator")
        ),
        nested,
    ))
}
