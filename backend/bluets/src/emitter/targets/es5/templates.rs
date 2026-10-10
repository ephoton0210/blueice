// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Untagged templates retain expression order and string-hint conversion.

use super::*;

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
        let Some((_, token)) = tokens.iter().enumerate().find(|(index, token)| {
            token.kind == TokenKind::Template
                && !index.checked_sub(1).is_some_and(|before| {
                    let before = &tokens[before];
                    before.kind == TokenKind::Identifier
                        || matches!(
                            before.kind,
                            TokenKind::String | TokenKind::Number | TokenKind::Template
                        )
                        || matches!(before.text.as_str(), ")" | "]")
                })
        }) else {
            return Ok(emitted);
        };
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
                    replacement = literal;
                } else if chunk < position {
                    replacement.push_str(&format!(".concat({literal})"));
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
                replacement.push_str(&format!(".concat(({}))", &emitted.javascript[start..end]));
                position = end + 1;
                chunk = position;
            } else {
                position += 1;
            }
        }
        let tail = quoted(&emitted.javascript[chunk..token.end - 1]);
        if replacement.is_empty() {
            replacement = tail;
        } else if chunk < token.end - 1 {
            replacement.push_str(&format!(".concat({tail})"));
        }
        emitted = mapped_edits(
            emitted,
            vec![TextEdit {
                start: token.start,
                end: token.end,
                replacement: format!("({replacement})"),
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
