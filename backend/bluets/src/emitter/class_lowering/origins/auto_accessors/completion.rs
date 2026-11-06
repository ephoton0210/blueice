// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A generated constructor completes at its original auto-accessor class range.

use super::*;
use crate::compiler::{CompilerOptions, EcmaTarget};
use crate::diagnostic::Diagnostic;
use crate::emitter::ProvenanceSegment;

fn same_class(
    previous: &EmittedJavaScript,
    class: &ClassDeclaration,
    source: &ClassDeclaration,
    original: &Module,
) -> bool {
    class.name == source.name
        && mapped(previous, class.name_span.start)
            == Some(position(&original.source, source.name_span.start))
        && mapped(previous, class.name_span.end)
            == Some(position(&original.source, source.name_span.end))
}

fn point(
    emitted: &mut EmittedJavaScript,
    original: &Module,
    generated: usize,
    source: Option<usize>,
) {
    let source = source.map(|source| position(&original.source, source));
    point_at(emitted, generated, source);
}

fn point_at(emitted: &mut EmittedJavaScript, generated: usize, source: Option<Position>) {
    let (generated_line, generated_column) = position(&emitted.javascript, generated);
    let (source_line, source_column) = source.unwrap_or((0, 0));
    let key = (generated_line, generated_column);
    let index = emitted
        .provenance
        .partition_point(|segment| (segment.generated_line, segment.generated_column) < key);
    let value = ProvenanceSegment {
        generated_line,
        generated_column,
        source_line,
        source_column,
        mapped: source.is_some(),
        name_index: None,
    };
    if emitted
        .provenance
        .get(index)
        .is_some_and(|segment| (segment.generated_line, segment.generated_column) == key)
    {
        emitted.provenance[index] = value;
    } else if emitted
        .segment_limit
        .is_some_and(|limit| emitted.provenance.len() >= limit)
    {
        emitted.overflowed = true;
    } else {
        emitted.provenance.insert(index, value);
    }
}

pub(in crate::emitter) fn mapped_generated_class_edits(
    previous: EmittedJavaScript,
    edits: Vec<TextEdit>,
    original: &Module,
    owned: &Module,
    options: &CompilerOptions,
) -> Result<EmittedJavaScript, Diagnostic> {
    let mut eligible = Vec::new();
    let mut static_fields = Vec::new();
    if previous.segment_limit.is_some()
        && options.experimental_decorators
        && options.target >= EcmaTarget::Es2015
        && options.target < EcmaTarget::Es2022
    {
        let fields = collect(original, owned, &previous, true);
        let before = classes(owned);
        for class in &before {
            for field in class
                .members
                .iter()
                .filter_map(|member| member.field.as_ref())
            {
                if field.is_static {
                    if let Some(origin) = fields.get(&field.name_span.start) {
                        static_fields
                            .push((mapped(&previous, origin.name), origin.original_completion));
                    }
                }
            }
        }
        for (declaration, _) in runtime_declarations(&original.declarations) {
            let Declaration::Class(source) = declaration else {
                continue;
            };
            if source.anonymous
                || source
                    .members
                    .iter()
                    .any(|member| member.constructor.is_some())
            {
                continue;
            }
            let originals = source
                .members
                .iter()
                .filter_map(|member| member.field.as_ref())
                .filter(|field| field.accessor && !field.declared && !field.name.starts_with('#'))
                .collect::<Vec<_>>();
            if !originals.iter().any(|field| !field.is_static) {
                continue;
            }
            let candidates = before
                .iter()
                .filter(|class| same_class(&previous, class, source, original))
                .collect::<Vec<_>>();
            if candidates.len() != 1 {
                continue;
            }
            let class = candidates[0];
            if class
                .members
                .iter()
                .any(|member| member.constructor.is_some())
            {
                continue;
            }
            let identified = class
                .members
                .iter()
                .filter_map(|member| member.field.as_ref())
                .filter_map(|field| fields.get(&field.name_span.start))
                .filter(|field| field.class_start == source.span.start)
                .count();
            if identified == originals.len() {
                eligible.push(source);
            }
        }
    }
    let mut emitted = crate::emitter::targets::mapped_edits(previous, edits);
    if eligible.is_empty() && static_fields.is_empty() {
        return Ok(emitted);
    }
    let after = crate::parser::parse_emitted_module(
        &original.id,
        &emitted.javascript,
        &options.limits.parser,
    )
    .map_err(|mut errors| errors.remove(0))?;
    for (source_name, completion) in static_fields {
        let Some(source_name) = source_name else {
            continue;
        };
        let candidates = after
            .declarations
            .iter()
            .filter_map(|declaration| {
                let Declaration::Raw(raw) = declaration else {
                    return None;
                };
                let tokens = &raw.tokens;
                (tokens.len() >= 7
                    && tokens[0].kind == crate::TokenKind::Identifier
                    && tokens[1].is("=")
                    && tokens[2].is("{")
                    && tokens[3].is("value")
                    && tokens[4].is(":")
                    && tokens[tokens.len() - 1].is("}")
                    && &emitted.javascript[tokens[tokens.len() - 1].end..raw.span.end] == ";"
                    && mapped(&emitted, tokens[0].start) == Some(source_name))
                .then_some(raw.span.end)
            })
            .collect::<Vec<_>>();
        if candidates.len() != 1 {
            continue;
        }
        let end = candidates[0];
        if emitted.javascript.as_bytes().get(end) != Some(&b' ') {
            continue;
        }
        // Preserve the following phase's existing owner while placing the
        // original member completion in its existing separator byte.
        let key = position(&emitted.javascript, end + 1);
        let next = emitted
            .provenance
            .partition_point(|segment| (segment.generated_line, segment.generated_column) <= key);
        let restore = next
            .checked_sub(1)
            .and_then(|index| emitted.provenance.get(index))
            .filter(|segment| segment.mapped)
            .map(|segment| (segment.source_line, segment.source_column));
        point(&mut emitted, original, end, Some(completion));
        point_at(&mut emitted, end + 1, restore);
    }
    let classes = classes(&after);
    for source in eligible {
        let candidates = classes
            .iter()
            .filter(|class| same_class(&emitted, class, source, original))
            .collect::<Vec<_>>();
        if candidates.len() != 1 {
            continue;
        }
        let class = candidates[0];
        let constructors = class
            .members
            .iter()
            .filter_map(|member| member.constructor.as_ref())
            .collect::<Vec<_>>();
        if constructors.len() != 1 {
            continue;
        }
        let constructor = constructors[0];
        let close = class.body.iter().rfind(|token| {
            token.is("}")
                && constructor.span.start <= token.start
                && token.end == constructor.span.end
        });
        let Some(close) = close else { continue };
        point(
            &mut emitted,
            original,
            close.start,
            Some(source.body_span.end - 1),
        );
        point(
            &mut emitted,
            original,
            constructor.span.end,
            Some(source.span.end),
        );
        // The constructor template already ends in a separator. Preserve its
        // exact logical endpoint there, then discard the old class-opening
        // trivia owner before the first independently relocated accessor.
        let following = class
            .members
            .iter()
            .filter(|member| member.span.start >= constructor.span.end)
            .map(|member| member.span.start)
            .min();
        if let Some(following) = following.filter(|following| {
            *following > constructor.span.end + 1
                && emitted.javascript.as_bytes().get(constructor.span.end) == Some(&b' ')
                && emitted.javascript[constructor.span.end..*following]
                    .chars()
                    .all(|character| character.is_ascii_whitespace())
        }) {
            let start = constructor.span.end + 1;
            let first = position(&emitted.javascript, start);
            let last = position(&emitted.javascript, following);
            emitted.provenance.retain(|segment| {
                let key = (segment.generated_line, segment.generated_column);
                key < first || key >= last
            });
            point(&mut emitted, original, start, None);
        }
    }
    Ok(emitted)
}
