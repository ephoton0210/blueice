// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Local alias writes publish before the containing expression returns.

use super::*;

pub(super) fn lower(
    emitted: EmittedJavaScript,
    module: &Module,
    options: &CompilerOptions,
    changed: &str,
) -> Result<EmittedJavaScript, Diagnostic> {
    let aliases = module.declarations.iter().filter_map(|declaration| {
        if let Declaration::ValueExport(export) = declaration {
            export.specifier.is_none().then_some(export)
        } else { None }
    }).flat_map(|export| export.bindings.iter()).filter(|binding| !binding.type_only)
        .filter(|binding| module.declarations.iter().any(|declaration| matches!(declaration,
            Declaration::Variable(value) if value.name == binding.local && !value.exported && value.kind != crate::parser::VariableKind::Const)))
        .map(|binding| binding.local.as_str()).collect::<BTreeSet<_>>();
    if aliases.is_empty() {
        return Ok(emitted);
    }
    let tokens = crate::syntax::lex_with_limits(
        &module.id,
        &emitted.javascript,
        options.limits.parser.max_source_bytes,
        options.limits.parser.max_tokens,
    )
    .map_err(|mut diagnostics| diagnostics.remove(0))?;
    let mut edits = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        if !aliases.contains(token.text.as_str()) || index == 0 {
            continue;
        }
        let previous = &tokens[index - 1];
        if matches!(previous.text.as_str(), "." | "?." | "let" | "const" | "var") {
            continue;
        }
        let Some(next) = tokens.get(index + 1) else {
            continue;
        };
        let (start, end) = if matches!(next.text.as_str(), "++" | "--") {
            (token.start, next.end)
        } else if matches!(previous.text.as_str(), "++" | "--") {
            (previous.start, token.end)
        } else if matches!(
            next.text.as_str(),
            "=" | "+="
                | "-="
                | "*="
                | "/="
                | "%="
                | "**="
                | "<<="
                | ">>="
                | ">>>="
                | "&="
                | "|="
                | "^="
                | "&&="
                | "||="
                | "??="
        ) {
            let mut depth = 0usize;
            let mut conditional = 0usize;
            let mut end = next.end;
            for current in tokens.iter().skip(index + 2) {
                if depth == 0
                    && (matches!(current.text.as_str(), ";" | "," | ")" | "]" | "}")
                        || current.kind == crate::syntax::TokenKind::Eof
                        || current.is(":") && conditional == 0)
                {
                    break;
                }
                match current.text.as_str() {
                    "(" | "[" | "{" => depth += 1,
                    ")" | "]" | "}" => depth = depth.saturating_sub(1),
                    "?" if depth == 0 => conditional += 1,
                    ":" if depth == 0 => conditional = conditional.saturating_sub(1),
                    _ => {}
                }
                end = current.end;
            }
            (token.start, end)
        } else {
            continue;
        };
        edits.push(TextEdit {
            start,
            end: start,
            replacement: format!("{changed}("),
        });
        edits.push(TextEdit {
            start: end,
            end,
            replacement: ")".to_string(),
        });
    }
    Ok(targets::mapped_edits(emitted, edits))
}
