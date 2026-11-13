// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! JSON-superset string characters need escapes before ES2019.

use super::*;

pub(in crate::emitter) fn lower(
    emitted: EmittedJavaScript,
    module: &Module,
    options: &CompilerOptions,
) -> Result<EmittedJavaScript, Diagnostic> {
    if options.target >= crate::EcmaTarget::Es2019
        || !emitted.javascript.contains(['\u{2028}', '\u{2029}'])
    {
        return Ok(emitted);
    }
    let edits = replacements(&module.id, &emitted.javascript, options)?;
    Ok(mapped_edits(emitted, edits))
}

fn replacements(
    id: &str,
    source: &str,
    options: &CompilerOptions,
) -> Result<Vec<TextEdit>, Diagnostic> {
    let tokens = crate::syntax::lex_with_limits(
        id,
        source,
        options.limits.parser.max_source_bytes,
        options.limits.parser.max_tokens,
    )
    .map_err(|mut errors| errors.remove(0))?;
    let mut remaining = options
        .limits
        .parser
        .max_tokens
        .saturating_sub(tokens.len());
    let mut frames = vec![tokens];
    let mut edits = Vec::new();
    while let Some(tokens) = frames.pop() {
        let mut covered_until = 0;
        for token in tokens {
            if token.start < covered_until {
                continue;
            }
            match token.kind {
                TokenKind::String => {
                    let replacement = escaped(&token.text);
                    if replacement != token.text {
                        edits.push(TextEdit {
                            start: token.start,
                            end: token.end,
                            replacement,
                        });
                    }
                }
                TokenKind::Template => {
                    let bytes = source.as_bytes();
                    let mut position = token.start + 1;
                    let mut closed = false;
                    while position < bytes.len() {
                        if bytes[position] == b'\\' {
                            position += 2;
                        } else if bytes[position] == b'`' {
                            covered_until = position + 1;
                            closed = true;
                            break;
                        } else if bytes[position..].starts_with(b"${") {
                            let (end, tokens) =
                                crate::syntax::lex_expression(id, source, position + 2).map_err(
                                    |message| {
                                        Diagnostic::error(
                                            DiagnosticCode::UnsupportedSyntax,
                                            token.span(id),
                                            message,
                                        )
                                    },
                                )?;
                            remaining = remaining.checked_sub(tokens.len()).ok_or_else(|| {
                                Diagnostic::error(
                                    DiagnosticCode::ResourceLimit,
                                    token.span(id),
                                    "target string interpolation exceeds the token budget",
                                )
                            })?;
                            frames.push(tokens);
                            position = end + 1;
                        } else {
                            position += 1;
                        }
                    }
                    if !closed {
                        return Err(Diagnostic::error(
                            DiagnosticCode::UnsupportedSyntax,
                            token.span(id),
                            "target string lowering found an unterminated template",
                        ));
                    }
                }
                _ => {}
            }
        }
    }
    edits.sort_by_key(|edit| edit.start);
    Ok(edits)
}

fn escaped(token: &str) -> String {
    let mut result = String::with_capacity(token.len());
    let mut escape = false;
    for character in token.chars() {
        match character {
            '\u{2028}' if !escape => result.push_str("\\u2028"),
            '\u{2029}' if !escape => result.push_str("\\u2029"),
            _ => result.push(character),
        }
        escape = character == '\\' && !escape;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_values_and_line_continuations_survive_target_escaping() {
        for (source, expected) in [
            ("\"a\u{2028}z\"", "\"a\\u2028z\""),
            ("'a\u{2029}z'", "'a\\u2029z'"),
            ("\"a\\\u{2028}z\"", "\"a\\\u{2028}z\""),
            ("\"a\\\\\u{2028}z\"", "\"a\\\\\\u2028z\""),
        ] {
            let actual = escaped(source);
            assert_eq!(actual, expected);
            assert_eq!(
                crate::utf16::Utf16String::parse_literal(source).expect("valid source value"),
                crate::utf16::Utf16String::parse_literal(&actual).expect("valid emitted value")
            );
        }
    }

    #[test]
    fn template_chunks_and_comments_stay_verbatim_while_embedded_strings_are_escaped() {
        let source =
            "// keep\u{2028}comment\nconst value = `raw\u{2028}\"chunk\"${`nested${\"a\u{2029}z\"}`}'raw\u{2029}'`;";
        let edits = replacements("memory:///main.ts", source, &CompilerOptions::default()).unwrap();
        assert_eq!(edits.len(), 1);
        assert_eq!(&source[edits[0].start..edits[0].end], "\"a\u{2029}z\"");
        assert_eq!(edits[0].replacement, "\"a\\u2029z\"");
    }
}
