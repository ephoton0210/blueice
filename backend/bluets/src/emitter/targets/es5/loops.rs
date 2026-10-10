// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Per-iteration environments for captured, single-name braced for loops.

use super::*;
use std::collections::BTreeSet;

pub(super) fn lower(
    mut emitted: EmittedJavaScript,
    module: &Module,
    options: &CompilerOptions,
    captured: &BTreeSet<String>,
) -> Result<EmittedJavaScript, Diagnostic> {
    if captured.is_empty() {
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
        if !token.is("for")
            || !tokens.get(index + 1).is_some_and(|token| token.is("("))
            || !tokens.get(index + 2).is_some_and(|token| token.is("var"))
        {
            continue;
        }
        let Some(binding) = tokens
            .get(index + 3)
            .filter(|token| captured.contains(&token.text))
        else {
            continue;
        };
        let Some(close) = exponentiation::matching_close(&tokens, index + 1) else {
            continue;
        };
        let Some(open) = tokens.get(close + 1).filter(|token| token.is("{")) else {
            continue;
        };
        let Some(end) = exponentiation::matching_close(&tokens, close + 1) else {
            continue;
        };
        let mut cursor = close + 2;
        while cursor < end {
            if tokens[cursor].is("function") {
                let body = (cursor + 1..end).find(|index| tokens[*index].is("{"));
                if let Some(next) =
                    body.and_then(|body| exponentiation::matching_close(&tokens, body))
                {
                    cursor = next + 1;
                    continue;
                }
            }
            if matches!(
                tokens[cursor].text.as_str(),
                "return" | "break" | "continue" | "yield" | "await" | "var" | "arguments"
            ) {
                return Err(unsupported(
                    module,
                    &SourceSpan::new(&module.id, 0, 0),
                    "captured loop completion requires a retained completion record",
                ));
            }
            cursor += 1;
        }
        edits.push(TextEdit {
            start: open.end,
            end: open.end,
            replacement: format!("{0} = (function ({0}) {{", binding.text),
        });
        edits.push(TextEdit {
            start: tokens[end].start,
            end: tokens[end].start,
            replacement: format!("\nreturn {0}; }}).call(this, {0});\n", binding.text),
        });
    }
    emitted = mapped_edits(emitted, edits);
    Ok(emitted)
}
