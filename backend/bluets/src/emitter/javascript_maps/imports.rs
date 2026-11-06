// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A retained import's `from` and following trivia complete its binding clause.
use super::*;

/// A retained star export has one original header phase before its module name.
pub(super) fn star_exports<'a>(
    module: &Module,
    tokens: &'a [crate::Token],
) -> Vec<(&'a crate::Token, usize)> {
    let mut result = Vec::new();
    for declaration in &module.declarations {
        let Declaration::ValueExport(export) = declaration else {
            continue;
        };
        if !export.star || export.namespace.is_some() || export.export_assignment {
            continue;
        }
        let Some(specifier) = &export.specifier_span else {
            continue;
        };
        for token in &tokens[tokens.partition_point(|token| token.start < export.span.start)
            ..tokens.partition_point(|token| token.start < specifier.start)]
        {
            result.push((token, export.span.start));
        }
    }
    result
}

pub(super) fn clauses<'a>(
    module: &'a Module,
    tokens: &'a [crate::Token],
) -> impl Iterator<Item = (&'a crate::Token, &'a crate::Token)> + 'a {
    module.declarations.iter().filter_map(|declaration| {
        let Declaration::Import(import) = declaration else {
            return None;
        };
        if import.equals_require || import.type_only {
            return None;
        }
        let begin = tokens.partition_point(|token| token.start < import.span.start);
        let end = tokens.partition_point(|token| token.start < import.specifier_span.start);
        let header = &tokens[begin..end];
        let keyword = header.len().checked_sub(1)?;
        (header[keyword].is("from") && keyword > 0)
            .then(|| (&header[keyword], &header[keyword - 1]))
    })
}

/// Alias punctuation inherits its exact original import binding phase.
/// Namespace stars begin a binding; named aliases continue the imported name.
pub(super) fn bindings<'a>(
    module: &Module,
    tokens: &'a [crate::Token],
) -> Vec<(&'a crate::Token, usize)> {
    let mut anchors = Vec::new();
    for declaration in &module.declarations {
        let Declaration::Import(import) = declaration else {
            continue;
        };
        if import.equals_require || import.type_only {
            continue;
        }
        for binding in import.bindings.iter().filter(|binding| !binding.type_only) {
            if binding.imported == "*" {
                let Some(local) = tokens.iter().position(|token| {
                    token.start == binding.span.start && token.text == binding.local
                }) else {
                    continue;
                };
                if local >= 2
                    && tokens[local - 1].is("as")
                    && tokens[local - 2].is("*")
                    && tokens[local - 2].start >= import.span.start
                {
                    let star = &tokens[local - 2];
                    anchors.push((star, star.start));
                    anchors.push((&tokens[local - 1], star.start));
                }
            } else {
                let begin = tokens.partition_point(|token| token.start < binding.span.start);
                let end = tokens.partition_point(|token| token.end <= binding.span.end);
                if let Some(alias) = tokens[begin..end]
                    .windows(3)
                    .find(|part| part[1].is("as") && part[2].text == binding.local)
                {
                    anchors.push((&alias[1], alias[0].end));
                }
            }
        }
    }
    anchors
}
