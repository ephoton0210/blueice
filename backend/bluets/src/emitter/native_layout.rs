// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Owner-selected printing composes trivia changes with original provenance.

use super::*;
use crate::syntax::{Token, TokenKind};
mod prefixes;
mod projection;
mod roles;
mod spacing;

pub(super) fn project_trivia(
    emitted: EmittedJavaScript,
    mut edits: Vec<TextEdit>,
) -> EmittedJavaScript {
    edits.sort_by_key(|edit| (edit.start, edit.end));
    projection::apply(emitted, &edits, "", &[], false, &BTreeSet::new(), None)
}

pub(super) fn prefix_trivia(emitted: EmittedJavaScript, prefix: &str) -> EmittedJavaScript {
    projection::apply(emitted, &[], prefix, &[], false, &BTreeSet::new(), None)
}

pub(super) fn format(
    emitted: EmittedJavaScript,
    module: &Module,
    project: &Project,
    options: &CompilerOptions,
) -> Result<EmittedJavaScript, Diagnostic> {
    if options.javascript_layout == crate::JavaScriptLayout::PreserveSource
        && !options.always_strict
    {
        return Ok(emitted);
    }
    let tokens = crate::syntax::lex_with_limits(
        &module.id,
        &emitted.javascript,
        options.limits.parser.max_source_bytes.saturating_mul(8),
        options.limits.parser.max_tokens,
    )
    .map_err(|mut errors| errors.remove(0))?;
    // Template/JSX contents remain opaque when the lexer also indexes their
    // embedded expressions. Printing must never edit overlapping token ranges.
    let mut end = 0;
    let tokens = tokens
        .into_iter()
        .filter(|token| {
            if token.kind == TokenKind::Eof || token.start < end {
                return false;
            }
            end = token.end;
            true
        })
        .collect::<Vec<_>>();
    let strict = options.always_strict
        && !project.is_external_module(module)
        && !has_strict_directive(&tokens);
    if options.javascript_layout == crate::JavaScriptLayout::PreserveSource {
        return Ok(projection::apply(
            emitted,
            &[],
            if strict { "\"use strict\";\n" } else { "" },
            &tokens,
            false,
            &BTreeSet::new(),
            None,
        ));
    }
    let roles = roles::Roles::new(&tokens, &emitted.javascript, module);
    let prefix_owners = prefixes::source_owners(module, options)?;
    let boundaries = projection::mapping_boundaries(&emitted);
    let mut edits = Vec::new();
    let mut projected_bytes = emitted.javascript.len();
    let byte_limit = options.limits.parser.max_source_bytes.saturating_mul(8);
    for index in 0..=tokens.len() {
        let start = index.checked_sub(1).map_or(0, |i| tokens[i].end);
        let end = tokens
            .get(index)
            .map_or(emitted.javascript.len(), |token| token.start);
        let gap = &emitted.javascript[start..end];
        if !gap.chars().all(char::is_whitespace) {
            continue;
        }
        let mut replacement = if index == 0 {
            String::new()
        } else if index == tokens.len() {
            "\n".to_string()
        } else {
            spacing::between(&tokens, index, gap, &roles)
        };
        // Distinct events on an existing carrier must not collapse when its
        // whitespace is removed. Native declaration factories intentionally
        // retain their coincident closure/invocation boundary. A member name
        // and its call/index opener also retain their ordered events when
        // source newlines are removed from that expression.
        if replacement.is_empty()
            && !gap.is_empty()
            && !(index > 0
                && tokens[index - 1].is("}")
                && tokens.get(index).is_some_and(|token| token.is("("))
                && roles.closings[index - 1] == Some(roles::Kind::Factory))
            && !(index > 0
                && spacing::member_name(&tokens, index - 1)
                && tokens
                    .get(index)
                    .is_some_and(|token| matches!(token.text.as_str(), "(" | "[")))
            && projection::requires_separator(&boundaries, gap, start, end)
        {
            replacement.push(' ');
        }
        if gap == replacement {
            continue;
        }
        projected_bytes = projected_bytes
            .saturating_sub(end - start)
            .saturating_add(replacement.len());
        if projected_bytes > byte_limit {
            return Err(Diagnostic::error(
                DiagnosticCode::ResourceLimit,
                SourceSpan::new(&module.id, 0, 0),
                "JavaScript layout exceeds the output byte budget",
            ));
        }
        edits.push(TextEdit {
            start,
            end,
            replacement,
        });
    }
    Ok(projection::apply(
        emitted,
        &edits,
        if strict { "\"use strict\";\n" } else { "" },
        &tokens,
        true,
        &prefix_owners,
        Some(&roles),
    ))
}

fn has_strict_directive(tokens: &[Token]) -> bool {
    let mut index = 0;
    while tokens
        .get(index)
        .is_some_and(|token| token.kind == TokenKind::String)
    {
        if matches!(
            tokens[index].text.as_str(),
            "'use strict'" | "\"use strict\""
        ) {
            return true;
        }
        index += 1;
        if tokens.get(index).is_some_and(|token| token.is(";")) {
            index += 1;
        } else {
            break;
        }
    }
    false
}
