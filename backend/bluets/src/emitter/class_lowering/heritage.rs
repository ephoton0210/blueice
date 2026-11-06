// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Move checked constructor aliases after module references have been rewritten.
use super::*;

pub(super) fn lower(module: &Module, edits: &mut Vec<TextEdit>) {
    for declaration in &module.declarations {
        let Declaration::Class(class) = declaration else {
            continue;
        };
        super_call_boundaries(module, class, edits);
        if class.extends_name.is_some() {
            let gap_start = class.name_span.end;
            let gap_end = module.source[gap_start..]
                .char_indices()
                .find(|(_, character)| !character.is_whitespace())
                .map_or(gap_start, |(offset, _)| gap_start + offset);
            if gap_start < gap_end {
                edits.push(TextEdit {
                    start: gap_start,
                    end: gap_end,
                    replacement: format!(
                        "{}{}",
                        super::super::relocations::source_anchor(gap_end),
                        &module.source[gap_start..gap_end]
                    ),
                });
            }
            if module.source[gap_end..].starts_with("extends") {
                let keyword_end = gap_end + "extends".len();
                let after = module.source[keyword_end..]
                    .char_indices()
                    .find(|(_, character)| !character.is_whitespace())
                    .map_or(keyword_end, |(offset, _)| keyword_end + offset);
                if keyword_end < after {
                    edits.push(TextEdit {
                        start: keyword_end,
                        end: after,
                        replacement: format!(
                            "{}{}",
                            super::super::relocations::source_anchor(gap_end),
                            &module.source[keyword_end..after]
                        ),
                    });
                }
            }
        }
        let Some(span) = &class.extends_span else {
            continue;
        };
        let Some(variable) = module.declarations.iter().find_map(|declaration| {
            let Declaration::Variable(variable) = declaration else {
                return None;
            };
            (variable.span.start == class.span.start
                && variable.span.end == variable.span.start
                && class.extends_name.as_ref() == Some(&variable.name))
            .then_some(variable)
        }) else {
            continue;
        };
        let mut local = Vec::new();
        edits.retain(|edit| {
            if edit.start >= span.start && edit.end <= span.end {
                local.push(edit.clone());
                false
            } else {
                true
            }
        });
        let source = if local.is_empty() {
            super::super::relocations::relocated_verbatim(span.start, span.end)
        } else {
            super::render_span(&module.source, &local, span.start, span.end)
        };
        let source = format!(
            "{}{}",
            super::super::relocations::source_anchor(span.start),
            source
        );
        let generated = super::super::relocations::unmapped();
        edits.push(TextEdit {
            start: class.span.start,
            end: class.span.start,
            replacement: format!(
                "{generated}const {} = {source}{generated};\n",
                variable.name
            ),
        });
        edits.push(TextEdit {
            start: span.start,
            end: span.end,
            replacement: format!(
                "{}{}",
                super::super::relocations::source_anchor(span.start),
                variable.name
            ),
        });
    }
}

fn super_call_boundaries(module: &Module, class: &ClassDeclaration, edits: &mut Vec<TextEdit>) {
    let tokens = &class.body;
    for (index, token) in tokens.iter().enumerate() {
        if !token.is("super") {
            continue;
        }
        let open = if tokens.get(index + 1).is_some_and(|token| token.is(".")) {
            index + 3
        } else if tokens.get(index + 1).is_some_and(|token| token.is("[")) {
            let Some(close) = closing(tokens, index + 1, "[", "]") else {
                continue;
            };
            close + 1
        } else {
            continue;
        };
        if !tokens.get(open).is_some_and(|token| token.is("(")) {
            continue;
        }
        let Some(close) = closing(tokens, open, "(", ")") else {
            continue;
        };
        let Some(next) = tokens.get(close + 1).filter(|token| {
            matches!(
                token.text.as_str(),
                "+" | "-"
                    | "*"
                    | "/"
                    | "%"
                    | "**"
                    | "<<"
                    | ">>"
                    | ">>>"
                    | "&"
                    | "|"
                    | "^"
                    | "&&"
                    | "||"
                    | "??"
                    | "=="
                    | "!="
                    | "==="
                    | "!=="
                    | "<"
                    | ">"
                    | "<="
                    | ">="
                    | "in"
                    | "instanceof"
                    | "?"
                    | ";"
            )
        }) else {
            continue;
        };
        let endpoint = tokens[close].end;
        edits.push(TextEdit {
            start: endpoint,
            end: next.end,
            replacement: format!(
                "{}{}{}",
                super::super::relocations::source_anchor(endpoint),
                &module.source[endpoint..next.end],
                super::super::relocations::source_anchor(if next.is(";") {
                    next.end
                } else {
                    endpoint
                })
            ),
        });
    }
}

fn closing(tokens: &[Token], open: usize, first: &str, last: &str) -> Option<usize> {
    let mut depth = 0;
    for (index, token) in tokens.iter().enumerate().skip(open) {
        if token.is(first) {
            depth += 1;
        } else if token.is(last) {
            depth -= 1;
            if depth == 0 {
                return Some(index);
            }
        }
    }
    None
}
