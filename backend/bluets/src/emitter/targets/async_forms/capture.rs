// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Lexical captures replace their own tokens and retain every other subtree.
use super::*;
use crate::emitter::relocations;

pub(super) fn range(
    source: &str,
    span: std::ops::Range<usize>,
    module: &Module,
    captures: (&str, &str),
    depth: usize,
    limits: &crate::parser::ParserLimits,
) -> Result<String, Diagnostic> {
    let (start, end) = (span.start, span.end);
    let (receiver, arguments) = captures;
    if depth == 0 {
        return Err(Diagnostic::error(
            DiagnosticCode::ResourceLimit,
            SourceSpan::new(&module.id, start, end),
            "async lexical capture exceeds depth limit",
        ));
    }
    let tokens = crate::syntax::lex_with_limits(
        &module.id,
        &source[start..end],
        limits.max_source_bytes,
        limits.max_tokens,
    )
    .map_err(|mut diagnostics| diagnostics.remove(0))?;
    let mut edits = Vec::new();
    let mut at = 0;
    while at < tokens.len() {
        if tokens[at].is("function") {
            if let Some(open) = (at + 1..tokens.len()).find(|&p| tokens[p].is("(")) {
                if let Some(function) = async_functions::function_at_parameters(&tokens, open) {
                    at = function.body_end + 1;
                    continue;
                }
            }
        }
        if tokens[at].is("(")
            && at > 0
            && matches!(
                tokens[at - 1].kind,
                TokenKind::Identifier | TokenKind::Keyword
            )
            && !matches!(
                tokens[at - 1].text.as_str(),
                "if" | "for" | "while" | "switch" | "catch" | "with"
            )
        {
            if let Some(function) = async_functions::function_at_parameters(&tokens, at) {
                at = function.body_end + 1;
                continue;
            }
        }
        let first = start + tokens[at].start;
        let last = start + tokens[at].end;
        if tokens[at].kind == TokenKind::Template {
            let replacement = super::super::mapped_fragments::template(
                source,
                first,
                last,
                module,
                depth - 1,
                |first, last, depth| {
                    range(
                        source,
                        first..last,
                        module,
                        (receiver, arguments),
                        depth,
                        limits,
                    )
                },
            )?;
            edits.push(TextEdit {
                start: first,
                end: last,
                replacement,
            });
        }
        if matches!(tokens[at].text.as_str(), "this" | "arguments")
            && !at.checked_sub(1).is_some_and(|p| tokens[p].is("."))
            && !tokens.get(at + 1).is_some_and(|p| p.is(":"))
        {
            let alias = if tokens[at].is("this") {
                receiver
            } else {
                arguments
            };
            let prefix = if tokens[at].is("arguments")
                && at
                    .checked_sub(1)
                    .is_some_and(|p| matches!(tokens[p].text.as_str(), "{" | ","))
                && tokens
                    .get(at + 1)
                    .is_some_and(|p| matches!(p.text.as_str(), "}" | ","))
            {
                format!(
                    "{}:{} ",
                    relocations::relocated_verbatim(first, last),
                    relocations::unmapped()
                )
            } else {
                String::new()
            };
            edits.push(TextEdit {
                start: first,
                end: last,
                replacement: format!(
                    "{prefix}{}{alias}{}",
                    relocations::source_anchor(first),
                    relocations::source_anchor(last)
                ),
            });
        }
        at += 1;
    }
    Ok(super::super::mapped_fragments::splice(start, end, edits))
}
