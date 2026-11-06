// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Original decorated static fields survive reparsing through exact named endpoints.
use crate::emitter::{is_line_break, runtime_declarations, EmittedJavaScript};
use crate::parser::{ClassField, Declaration, Module};
use std::collections::{BTreeMap, BTreeSet};
type Position = (usize, usize);

fn fields(module: &Module, standard: bool) -> impl Iterator<Item = (&ClassField, usize, bool)> {
    runtime_declarations(&module.declarations)
        .into_iter()
        .filter_map(|(declaration, _)| match declaration {
            Declaration::Class(class) => Some(class),
            _ => None,
        })
        .chain(
            module
                .class_expressions
                .values()
                .map(|expression| &expression.class),
        )
        .flat_map(move |class| class.members.iter().map(move |member| (class, member)))
        .filter_map(move |(class, member)| {
            member
                .field
                .as_ref()
                .filter(|field| field.is_static)
                .map(|field| {
                    let owned_super = standard
                        && !class.decorators.is_empty()
                        && field.initializer.as_ref().is_some_and(|tokens| {
                            tokens.iter().any(|token| {
                                token.is("super")
                                    && !module.nested_functions.values().any(|function| {
                                        function.kind != crate::parser::NestedFunctionKind::Arrow
                                            && function.span.start <= token.start
                                            && token.end <= function.span.end
                                    })
                                    && !module.class_expressions.values().any(|expression| {
                                        field.span.start < expression.class.span.start
                                            && expression.class.span.start <= token.start
                                            && token.end <= expression.class.span.end
                                    })
                            })
                        });
                    (
                        field,
                        member.span.start,
                        !member.decorators.is_empty() || owned_super,
                    )
                })
        })
}

fn positions(source: &str, offsets: &BTreeSet<usize>) -> BTreeMap<usize, Position> {
    let (mut line, mut column) = (0, 0);
    let mut result = BTreeMap::new();
    let mut characters = source.char_indices().peekable();
    while let Some((offset, character)) = characters.next() {
        if offsets.contains(&offset) {
            result.insert(offset, (line, column));
        }
        if is_line_break(
            character,
            characters.peek().map(|(_, character)| *character),
        ) {
            line += 1;
            column = 0;
        } else {
            column += character.len_utf16();
        }
    }
    if offsets.contains(&source.len()) {
        result.insert(source.len(), (line, column));
    }
    result
}

fn generated_offsets(source: &str, wanted: &BTreeSet<Position>) -> BTreeMap<Position, usize> {
    let (mut line, mut column) = (0, 0);
    let mut result = BTreeMap::new();
    let mut characters = source.char_indices().peekable();
    while let Some((offset, character)) = characters.next() {
        if wanted.contains(&(line, column)) {
            result.insert((line, column), offset);
        }
        if is_line_break(
            character,
            characters.peek().map(|(_, character)| *character),
        ) {
            line += 1;
            column = 0;
        } else {
            column += character.len_utf16();
        }
    }
    result
}

pub(in crate::emitter::class_lowering) fn decorated_static_blocks(
    original: &Module,
    owned: &Module,
    previous: &EmittedJavaScript,
    standard: bool,
) -> BTreeMap<usize, usize> {
    if previous.segment_limit.is_none() {
        return BTreeMap::new();
    }
    let original_fields = fields(original, standard)
        .filter(|(_, _, decorated)| *decorated)
        .collect::<Vec<_>>();
    let offsets = original_fields
        .iter()
        .flat_map(|(field, header, _)| [field.name_span.start, field.name_span.end, *header])
        .collect();
    let original_positions = positions(&original.source, &offsets);
    let mut identities = BTreeMap::new();
    for (field, header, _) in original_fields {
        identities.insert(
            original_positions[&field.name_span.start],
            (
                field,
                original_positions[&field.name_span.end],
                original_positions[&header],
            ),
        );
    }
    let owned_fields = fields(owned, standard).collect::<Vec<_>>();
    let offsets = owned_fields
        .iter()
        .flat_map(|(field, _, _)| [field.name_span.start, field.name_span.end])
        .collect();
    let owned_positions = positions(&owned.source, &offsets);
    let point = |position| {
        let index = previous.provenance.partition_point(|segment| {
            (segment.generated_line, segment.generated_column) < position
        });
        previous
            .provenance
            .get(index)
            .filter(|segment| {
                segment.mapped && (segment.generated_line, segment.generated_column) == position
            })
            .map(|segment| (segment.source_line, segment.source_column))
    };
    let mut identified = BTreeMap::new();
    for (field, _, _) in owned_fields {
        let Some(start) = point(owned_positions[&field.name_span.start]) else {
            continue;
        };
        let Some((source, end, header)) = identities.get(&start) else {
            continue;
        };
        if source.name == field.name && point(owned_positions[&field.name_span.end]) == Some(*end) {
            identified.insert(field.name_span.start, *header);
        }
    }
    let wanted = identified.values().copied().collect::<BTreeSet<_>>();
    let mut proxies = BTreeMap::new();
    for segment in &previous.provenance {
        let position = (segment.source_line, segment.source_column);
        if segment.mapped && wanted.contains(&position) {
            proxies
                .entry(position)
                .or_insert((segment.generated_line, segment.generated_column));
        }
    }
    let offsets = generated_offsets(&previous.javascript, &proxies.values().copied().collect());
    identified
        .into_iter()
        .filter_map(|(field, original)| {
            proxies
                .get(&original)
                .and_then(|proxy| offsets.get(proxy))
                .map(|offset| (field, *offset))
        })
        .collect()
}
