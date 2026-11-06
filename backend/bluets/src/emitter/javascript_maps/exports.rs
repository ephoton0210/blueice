// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Retained named export punctuation follows its original binding phases.
use super::*;

pub(super) fn boundaries<'a>(
    module: &Module,
    options: &CompilerOptions,
    tokens: &'a [crate::Token],
) -> Vec<(&'a crate::Token, usize, usize)> {
    if options.module_kind != crate::ModuleKind::Esm {
        return Vec::new();
    }
    let mut anchors = Vec::new();
    for declaration in &module.declarations {
        let Declaration::ValueExport(export) = declaration else {
            continue;
        };
        if export.export_assignment || export.is_type_only() || export.star {
            continue;
        }
        let begin = tokens.partition_point(|token| token.start < export.span.start);
        let end = tokens.partition_point(|token| {
            token.start
                < export
                    .specifier_span
                    .as_ref()
                    .map_or(export.span.end, |span| span.start)
        });
        let header = &tokens[begin..end];
        for (index, token) in header.iter().enumerate() {
            if token.is("export") || token.is("{") {
                anchors.push((token, token.start, token.start));
            } else if token.is("}") && index > 0 {
                anchors.push((token, header[index - 1].end, token.end));
            } else if matches!(token.text.as_str(), "as" | "from") && index > 0 {
                anchors.push((token, header[index - 1].end, header[index - 1].end));
            }
        }
        for binding in export.bindings.iter().filter(|binding| !binding.type_only) {
            if let Some(name) = header
                .iter()
                .rev()
                .find(|token| token.start >= binding.span.start && token.end <= binding.span.end)
            {
                anchors.push((name, name.start, name.end));
            }
        }
    }
    anchors
}
