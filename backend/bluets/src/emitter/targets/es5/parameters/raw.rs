// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Lower parameter syntax introduced by retained callable transformations.
use super::*;

pub(in super::super) fn lower(
    mut emitted: EmittedJavaScript,
    module: &Module,
    options: &CompilerOptions,
) -> Result<EmittedJavaScript, Diagnostic> {
    let mut remaining = options.limits.parser.max_tokens;
    let mut sequence = 0;
    loop {
        let tokens = crate::syntax::lex_with_limits(
            &module.id,
            &emitted.javascript,
            options.limits.parser.max_source_bytes,
            remaining,
        )
        .map_err(|mut d| d.remove(0))?;
        remaining = remaining.saturating_sub(tokens.len());
        let candidate = (1..tokens.len()).rev().find_map(|open| {
            if !tokens[open].is("(")
                || matches!(
                    tokens[open - 1].text.as_str(),
                    "if" | "for" | "while" | "switch" | "catch" | "with"
                )
            {
                return None;
            }
            if !(matches!(
                tokens[open - 1].kind,
                TokenKind::Identifier | TokenKind::Keyword
            ) || tokens[open - 1].is("]"))
            {
                return None;
            }
            let close = exponentiation::matching_close(&tokens, open)?;
            if !tokens.get(close + 1)?.is("{") {
                return None;
            }
            let groups = groups(&tokens, open + 1, close)?;
            groups
                .iter()
                .any(|&(a, b)| {
                    a < b
                        && (matches!(tokens[a].text.as_str(), "..." | "{" | "[")
                            || tokens.get(a + 1).is_some_and(|t| t.is("=")))
                })
                .then_some((open, close, groups))
        });
        let Some((open, close, groups)) = candidate else {
            return Ok(emitted);
        };
        let source = &emitted.javascript;
        let mut parameters = Vec::new();
        let mut prologue = String::new();
        for (position, (begin, end)) in groups.into_iter().enumerate() {
            if begin == end {
                continue;
            }
            if tokens[begin].is("...") {
                let name = &tokens[begin + 1].text;
                let index = unused_name(source, "rest_argument", &mut sequence);
                prologue.push_str(&rest::prologue(
                    name,
                    &index,
                    position,
                    tokens[begin].start,
                    tokens[end - 1].end,
                ));
                continue;
            }
            let pattern = matches!(tokens[begin].text.as_str(), "{" | "[");
            let name = if pattern {
                unused_name(source, "pattern_argument", &mut sequence)
            } else {
                tokens[begin].text.clone()
            };
            let head = if pattern {
                exponentiation::matching_close(&tokens, begin).expect("balanced parameter pattern")
                    + 1
            } else {
                begin + 1
            };
            parameters.push(if pattern {
                format!(
                    "{}{}",
                    crate::emitter::relocations::source_anchor(tokens[begin].start),
                    name
                )
            } else {
                format!(
                    "{}{}{}",
                    crate::emitter::relocations::source_anchor(tokens[begin].start),
                    name,
                    crate::emitter::relocations::source_anchor(tokens[begin].end)
                )
            });
            if head < end && tokens[head].is("=") {
                let value = if end == head + 2
                    && (matches!(tokens[head + 1].kind, TokenKind::Number | TokenKind::String)
                        || matches!(tokens[head + 1].text.as_str(), "true" | "false" | "null"))
                {
                    format!(
                        "{}{}",
                        crate::emitter::relocations::source_anchor(tokens[begin].start),
                        tokens[head + 1].text
                    )
                } else {
                    crate::emitter::relocations::relocated_verbatim(
                        tokens[head + 1].start,
                        tokens[end - 1].end,
                    )
                };
                prologue.push_str(&format!(
                    "{}if({name}===void 0){{{name}=({value}{});}}\n",
                    crate::emitter::relocations::source_anchor(tokens[begin].start),
                    crate::emitter::relocations::source_anchor(tokens[end - 1].end)
                ));
            }
            if pattern {
                let pattern = crate::emitter::relocations::relocated_verbatim(
                    tokens[begin].start,
                    tokens[head - 1].end,
                );
                let origin = crate::parser::parse_variable_pattern(
                    &tokens,
                    begin,
                    &module.id,
                    options.limits.parser.max_type_depth,
                )
                .and_then(|(binding, _)| binding.names().last().map(|(_, span)| span.end))
                .unwrap_or(tokens[head - 1].end);
                prologue.push_str(&format!(
                    "{}var {pattern}={}{};\n",
                    crate::emitter::relocations::unmapped(),
                    crate::emitter::relocations::source_anchor(origin),
                    name
                ));
            }
        }
        let insertion = body_directive_end(&tokens, tokens[close + 1].start);
        let empty = parameters.is_empty();
        let mut edits = vec![
            TextEdit {
                start: tokens[open].end,
                end: tokens[close].start,
                replacement: parameters.join(", "),
            },
            TextEdit {
                start: insertion,
                end: insertion,
                replacement: format!("\n{prologue}"),
            },
        ];
        if empty {
            for index in [close, close + 1] {
                let anchor = crate::emitter::relocations::source_anchor(tokens[open].start);
                edits.push(TextEdit {
                    start: tokens[index].start,
                    end: tokens[index].end,
                    replacement: format!("{anchor}{}{anchor}", tokens[index].text),
                });
            }
        }
        emitted = mapped_edits(emitted, edits);
    }
}
fn groups(tokens: &[Token], begin: usize, end: usize) -> Option<Vec<(usize, usize)>> {
    let mut result = Vec::new();
    let mut first = begin;
    let mut at = begin;
    while at < end {
        if tokens[at].is(",") {
            result.push((first, at));
            first = at + 1;
        }
        if matches!(tokens[at].text.as_str(), "(" | "[" | "{") {
            at = exponentiation::matching_close(tokens, at)?;
        }
        at += 1;
    }
    if first < end {
        result.push((first, end));
    }
    Some(result)
}
