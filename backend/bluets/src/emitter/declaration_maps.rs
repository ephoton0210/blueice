// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Original token boundaries for the measured scalar declaration-map surface.

use super::*;

pub(super) fn build(
    id: &str,
    module: &Module,
    declaration: &str,
    options: &CompilerOptions,
) -> Result<SourceMap, Diagnostic> {
    let generated = crate::syntax::lex(id, declaration).map_err(|errors| errors[0].clone())?;
    let mut points = BTreeMap::new();
    let mut cursor = 0;
    for item in &module.declarations {
        let (name, annotation, function) = match item {
            Declaration::Variable(variable) if variable.exported => {
                (&variable.name, variable.annotation.as_ref(), false)
            }
            Declaration::Function(function) if function.exported => {
                if !function.type_parameters.is_empty()
                    || function.parameters.iter().any(|parameter| {
                        parameter.default.is_some()
                            || parameter.pattern.is_some()
                            || parameter.rest
                            || parameter.optional
                            || parameter
                                .annotation
                                .as_ref()
                                .is_none_or(|value| !scalar(value))
                    })
                {
                    return Err(unsupported(item.span()));
                }
                (&function.name, function.return_type.as_ref(), true)
            }
            Declaration::Variable(_) | Declaration::Function(_) => continue,
            _ => return Err(unsupported(item.span())),
        };
        if annotation.is_none_or(|value| !scalar(value)) {
            return Err(unsupported(item.span()));
        }
        let source = crate::syntax::lex(id, &module.source[item.span().start..item.span().end])
            .map_err(|errors| errors[0].clone())?;
        let source: Vec<_> = source
            .into_iter()
            .map(|mut token| {
                token.start += item.span().start;
                token.end += item.span().start;
                token
            })
            .collect();
        let source_name = source
            .iter()
            .position(|token| token.text == *name)
            .ok_or_else(|| unsupported(item.span()))?;
        let generated_name = generated
            .iter()
            .enumerate()
            .skip(cursor)
            .find_map(|(index, token)| (token.text == *name).then_some(index))
            .ok_or_else(|| unsupported(item.span()))?;
        let end = generated
            .iter()
            .enumerate()
            .skip(generated_name)
            .find_map(|(index, token)| token.is(";").then_some(index))
            .ok_or_else(|| unsupported(item.span()))?;
        let begin = generated[..generated_name]
            .iter()
            .enumerate()
            .skip(cursor)
            .find_map(|(index, token)| token.is("export").then_some(index))
            .unwrap_or(cursor);
        point(
            &mut points,
            declaration,
            &module.source,
            generated[begin].start,
            item.span().start,
        );
        if !function {
            if let Some(kind) = generated[begin..generated_name]
                .iter()
                .find(|token| matches!(token.text.as_str(), "const" | "let" | "var"))
            {
                if let Some(original) = source[..source_name]
                    .iter()
                    .find(|token| token.text == kind.text)
                {
                    point(
                        &mut points,
                        declaration,
                        &module.source,
                        kind.start,
                        original.start,
                    );
                }
            }
        }
        point(
            &mut points,
            declaration,
            &module.source,
            generated[generated_name].start,
            source[source_name].start,
        );
        point(
            &mut points,
            declaration,
            &module.source,
            generated[generated_name].end,
            source[source_name].end,
        );
        let source_end = source
            .iter()
            .position(|token| {
                if function {
                    token.is("{")
                } else {
                    token.is("=") || token.is(";")
                }
            })
            .unwrap_or(source.len());
        let mut source_cursor = source_name + 1;
        for token in &generated[generated_name + 1..end] {
            if token.kind != crate::syntax::TokenKind::Identifier
                && !matches!(
                    token.text.as_str(),
                    "number" | "string" | "boolean" | "void"
                )
            {
                continue;
            }
            if let Some(index) = source[source_cursor..source_end]
                .iter()
                .position(|candidate| candidate.text == token.text)
            {
                source_cursor += index;
                let original = &source[source_cursor];
                point(
                    &mut points,
                    declaration,
                    &module.source,
                    token.start,
                    original.start,
                );
                point(
                    &mut points,
                    declaration,
                    &module.source,
                    token.end,
                    original.end,
                );
                source_cursor += 1;
            }
        }
        let original_terminator = if function {
            source
                .iter()
                .take(source_end)
                .next_back()
                .map_or(item.span().start, |token| token.end)
        } else {
            source
                .iter()
                .rev()
                .find(|token| token.is(";"))
                .map_or(item.span().end, |token| token.start)
        };
        point(
            &mut points,
            declaration,
            &module.source,
            generated[end].start,
            original_terminator,
        );
        point(
            &mut points,
            declaration,
            &module.source,
            generated[end].end,
            item.span().end,
        );
        cursor = end + 1;
    }
    if points.len() > options.limits.max_source_map_segments {
        return Err(Diagnostic::error(
            DiagnosticCode::ResourceLimit,
            SourceSpan::new(id, 0, 0),
            "declaration map exceeded the segment limit",
        ));
    }
    let segments: Vec<_> = points
        .into_iter()
        .map(
            |((generated_line, generated_column), (source_line, source_column))| {
                ProvenanceSegment {
                    generated_line,
                    generated_column,
                    source_line,
                    source_column,
                }
            },
        )
        .collect();
    let file = output_file_name(id);
    let file = if let Some(stem) = file.strip_suffix(".mjs") {
        format!("{stem}.d.mts")
    } else if let Some(stem) = file.strip_suffix(".cjs") {
        format!("{stem}.d.cts")
    } else {
        format!("{}.d.ts", file.strip_suffix(".js").unwrap_or(&file))
    };
    Ok(SourceMap {
        file,
        sources: vec![id.into()],
        sources_content: Vec::new(),
        inline_sources: false,
        source_root: options.source_root.clone().unwrap_or_default(),
        mappings: encode_mappings(&segments),
    })
}

fn scalar(value: &Type) -> bool {
    match value {
        Type::Number | Type::String | Type::Boolean | Type::Void => true,
        Type::Array(item) => scalar(item),
        _ => false,
    }
}

type Points = BTreeMap<(usize, usize), (usize, usize)>;

fn point(
    points: &mut Points,
    generated: &str,
    source: &str,
    generated_offset: usize,
    source_offset: usize,
) {
    points.insert(
        position(generated, generated_offset),
        position(source, source_offset),
    );
}

fn position(text: &str, offset: usize) -> (usize, usize) {
    let prefix = &text[..offset];
    (
        prefix.bytes().filter(|byte| *byte == b'\n').count(),
        prefix.rsplit('\n').next().unwrap().encode_utf16().count(),
    )
}

fn unsupported(span: &SourceSpan) -> Diagnostic {
    Diagnostic::error(
        DiagnosticCode::UnsupportedSyntax,
        span.clone(),
        "declaration maps currently require annotated top-level scalar variables and functions",
    )
}
