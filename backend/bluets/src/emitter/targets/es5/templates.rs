// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Untagged templates retain expression order and string-hint conversion.

use super::*;
use crate::emitter::relocations;
mod tagged;
pub(crate) const TAGGED_SOURCE: &str = tagged::SOURCE;

pub(super) fn lower(
    mut emitted: EmittedJavaScript,
    module: &Module,
    options: &CompilerOptions,
) -> Result<EmittedJavaScript, Diagnostic> {
    let mut remaining = options.limits.parser.max_tokens;
    loop {
        let tokens = crate::syntax::lex_with_limits(
            &module.id,
            &emitted.javascript,
            options.limits.parser.max_source_bytes,
            remaining,
        )
        .map_err(|mut diagnostics| diagnostics.remove(0))?;
        remaining = remaining.saturating_sub(tokens.len());
        let Some((index, token)) = tokens
            .iter()
            .enumerate()
            .find(|(_, token)| token.kind == TokenKind::Template)
        else {
            return Ok(emitted);
        };
        let is_tagged = index.checked_sub(1).is_some_and(|before| {
            let before = &tokens[before];
            before.kind == TokenKind::Identifier
                || matches!(
                    before.kind,
                    TokenKind::String | TokenKind::Number | TokenKind::Template
                )
                || matches!(before.text.as_str(), ")" | "]")
        });
        if is_tagged {
            emitted = tagged::lower(emitted, module, options, token, &mut remaining)?;
            continue;
        }
        let bytes = emitted.javascript.as_bytes();
        let mut position = token.start + 1;
        let mut chunk = position;
        let mut replacement = String::new();
        while position < token.end - 1 {
            if bytes[position] == b'\\' {
                position += 2;
                continue;
            }
            if bytes[position..].starts_with(b"${") {
                let literal = quoted(&emitted.javascript[chunk..position]);
                if replacement.is_empty() {
                    replacement = format!("{}{literal}", relocations::unmapped());
                } else {
                    if chunk < position {
                        replacement.push_str(&format!(", {literal}"));
                    }
                    replacement.push(')');
                }
                let start = position + 2;
                let (end, expression_tokens) =
                    crate::syntax::lex_expression(&module.id, &emitted.javascript, start).map_err(
                        |message| unsupported(module, &SourceSpan::new(&module.id, 0, 0), &message),
                    )?;
                if expression_tokens.len() > remaining {
                    return Err(Diagnostic::error(
                        DiagnosticCode::ResourceLimit,
                        SourceSpan::new(&module.id, 0, 0),
                        "ES5 template expressions exceed the token budget",
                    ));
                }
                remaining -= expression_tokens.len();
                replacement.push_str(&format!(
                    ".concat({}({})",
                    relocations::unmapped(),
                    relocations::relocated_verbatim(start, end)
                ));
                position = end + 1;
                chunk = position;
            } else {
                position += 1;
            }
        }
        let tail = quoted(&emitted.javascript[chunk..token.end - 1]);
        if replacement.is_empty() {
            replacement = format!("{}{tail}", relocations::source_anchor(token.start));
        } else {
            if chunk < token.end - 1 {
                replacement.push_str(&format!(", {tail}"));
            }
            replacement.push(')');
        }
        emitted = mapped_edits(
            emitted,
            vec![TextEdit {
                start: token.start,
                end: token.end,
                replacement: format!(
                    "{}({replacement}{}){}",
                    relocations::source_anchor(token.start),
                    relocations::source_anchor(token.end),
                    relocations::source_anchor(token.end)
                ),
            }],
        );
    }
}

fn quoted(text: &str) -> String {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut output = String::from("\"");
    let mut characters = normalized.chars();
    while let Some(character) = characters.next() {
        match character {
            '\\' => {
                output.push('\\');
                if let Some(next) = characters.next() {
                    output.push(next);
                }
            }
            '"' => output.push_str("\\\""),
            '\n' => output.push_str("\\n"),
            '\u{2028}' => output.push_str("\\u2028"),
            '\u{2029}' => output.push_str("\\u2029"),
            other => output.push(other),
        }
    }
    output.push('"');
    output
}
