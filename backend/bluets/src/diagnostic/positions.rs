// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Coordinate attachment uses only the compiler's existing authorized inputs.
use super::{Diagnostic, SourceSpan, TypeScriptPosition};
use crate::Project;
use std::collections::BTreeMap;
mod cycles;
mod jsx;
mod refine;

pub(crate) fn attach(project: &Project, diagnostics: &mut [Diagnostic]) {
    let mut lines = BTreeMap::new();
    let mut token_sets = BTreeMap::new();
    for diagnostic in diagnostics {
        cycles::refine(project, diagnostic);
        let Some(counterpart) = &diagnostic.typescript else {
            continue;
        };
        let Some(source) = project.source(&counterpart.span.module) else {
            continue;
        };
        let starts = lines
            .entry(counterpart.span.module.clone())
            .or_insert_with(|| line_starts(source));
        let tokens = token_sets
            .entry(counterpart.span.module.clone())
            .or_insert_with(|| crate::syntax::lex(&counterpart.span.module, source).ok());
        let span = if counterpart.position.is_none() && counterpart.span == diagnostic.span {
            jsx::refine(diagnostic, source).or_else(|| {
                tokens
                    .as_deref()
                    .map(|tokens| refine::refine(diagnostic, tokens))
            })
        } else {
            None
        };
        let counterpart = diagnostic.typescript.as_mut().unwrap();
        if let Some(span) = span {
            counterpart.span = span;
        }
        counterpart.position = coordinates(source, starts, &counterpart.span);
    }
}

fn line_starts(source: &str) -> Vec<usize> {
    let mut result = vec![0];
    let mut chars = source.char_indices().peekable();
    while let Some((offset, ch)) = chars.next() {
        let end = match ch {
            '\r' if chars.peek().is_some_and(|(_, ch)| *ch == '\n') => chars.next().unwrap().0 + 1,
            '\r' | '\n' | '\u{2028}' | '\u{2029}' => offset + ch.len_utf8(),
            _ => continue,
        };
        result.push(end);
    }
    result
}

pub(crate) fn from_source(source: &str, span: &SourceSpan) -> Option<TypeScriptPosition> {
    coordinates(source, &line_starts(source), span)
}

pub(crate) fn coordinates(
    source: &str,
    starts: &[usize],
    span: &SourceSpan,
) -> Option<TypeScriptPosition> {
    let selected = source.get(span.start..span.end)?;
    let line = starts
        .partition_point(|start| *start <= span.start)
        .checked_sub(1)?;
    Some(TypeScriptPosition {
        line: line + 1,
        column: source.get(starts[line]..span.start)?.encode_utf16().count() + 1,
        length: selected.encode_utf16().count(),
    })
}
