// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Tagged sites retain raw/cooked strings, identity and substitution order.
use super::*;
pub(crate) const SOURCE: &str = include_str!("tagged.v1.js");

pub(super) fn lower(
    emitted: EmittedJavaScript,
    module: &Module,
    options: &CompilerOptions,
    token: &Token,
    remaining: &mut usize,
) -> Result<EmittedJavaScript, Diagnostic> {
    let source = &emitted.javascript;
    let mut position = token.start + 1;
    let mut chunk = position;
    let mut raw = Vec::new();
    let mut cooked = Vec::new();
    let mut expressions = Vec::new();
    while position < token.end - 1 {
        if source.as_bytes()[position] == b'\\' {
            position += 1;
            if position < token.end - 1 {
                position += source[position..]
                    .chars()
                    .next()
                    .expect("retained escape")
                    .len_utf8();
            }
            continue;
        }
        if source.as_bytes()[position..].starts_with(b"${") {
            append(&source[chunk..position], &mut raw, &mut cooked);
            let begin = position + 2;
            let (end, tokens) =
                crate::syntax::lex_expression(&module.id, source, begin).map_err(|message| {
                    unsupported(module, &SourceSpan::new(&module.id, 0, 0), &message)
                })?;
            *remaining = remaining.checked_sub(tokens.len()).ok_or_else(|| {
                Diagnostic::error(
                    DiagnosticCode::ResourceLimit,
                    token.span(&module.id),
                    "tagged template expressions exceed the token budget",
                )
            })?;
            expressions.push(format!("({})", &source[begin..end]));
            position = end + 1;
            chunk = position;
        } else {
            position += source[position..]
                .chars()
                .next()
                .expect("retained template character")
                .len_utf8();
        }
    }
    append(&source[chunk..token.end - 1], &mut raw, &mut cooked);
    let mut sequence = 0;
    let site = unused_name(source, "template_site", &mut sequence);
    let external = crate::emitter::helper_selection::external(options);
    let helper = if external {
        crate::emitter::helper_selection::reference(source, "__makeTemplateObject")
    } else {
        unused_name(source, "template_object", &mut sequence)
    };
    let arguments = if expressions.is_empty() {
        String::new()
    } else {
        format!(", {}", expressions.join(", "))
    };
    let replacement = format!(
        "({site} || ({site} = {helper}([{}], [{}]))){arguments}",
        cooked.join(", "),
        raw.join(", ")
    );
    let definitions = if external {
        String::new()
    } else {
        SOURCE.replace("__blueice_target_template_object", &helper)
    };
    let tokens = crate::syntax::lex_with_limits(
        &module.id,
        source,
        options.limits.parser.max_source_bytes,
        options.limits.parser.max_tokens,
    )
    .map_err(|mut diagnostics| diagnostics.remove(0))?;
    let insertion = directive_end(&tokens);
    Ok(mapped_edits(
        emitted,
        vec![
            TextEdit {
                start: token.start,
                end: token.end,
                replacement: format!("({replacement})"),
            },
            TextEdit {
                start: insertion,
                end: insertion,
                replacement: format!("\nvar {site};\n{definitions}\n"),
            },
        ],
    ))
}

fn append(text: &str, raw: &mut Vec<String>, cooked: &mut Vec<String>) {
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    raw.push(serde_json::to_string(&text).expect("raw template text is a string"));
    cooked.push(cooked_string(&text).unwrap_or_else(|| "void 0".into()));
}

fn cooked_string(text: &str) -> Option<String> {
    let mut chars = text.chars().peekable();
    let mut output = String::from("\"");
    while let Some(character) = chars.next() {
        match character {
            '"' => output.push_str("\\\""),
            '\n' => output.push_str("\\n"),
            '\u{2028}' => output.push_str("\\u2028"),
            '\u{2029}' => output.push_str("\\u2029"),
            '\\' => match chars.next()? {
                '\n' => {}
                'x' => {
                    output.push_str("\\x");
                    for _ in 0..2 {
                        let c = chars.next()?;
                        if !c.is_ascii_hexdigit() {
                            return None;
                        }
                        output.push(c);
                    }
                }
                'u' => {
                    if chars.peek() == Some(&'{') {
                        chars.next();
                        let mut digits = String::new();
                        loop {
                            let c = chars.next()?;
                            if c == '}' {
                                break;
                            }
                            if !c.is_ascii_hexdigit() || digits.len() >= 6 {
                                return None;
                            }
                            digits.push(c);
                        }
                        let value = u32::from_str_radix(&digits, 16).ok()?;
                        if value > 0x10ffff {
                            return None;
                        }
                        if value <= 0xffff {
                            output.push_str(&format!("\\u{value:04x}"));
                        } else {
                            let value = value - 0x10000;
                            output.push_str(&format!(
                                "\\u{:04x}\\u{:04x}",
                                0xd800 + (value >> 10),
                                0xdc00 + (value & 0x3ff)
                            ));
                        }
                    } else {
                        output.push_str("\\u");
                        for _ in 0..4 {
                            let c = chars.next()?;
                            if !c.is_ascii_hexdigit() {
                                return None;
                            }
                            output.push(c);
                        }
                    }
                }
                '0' => {
                    if chars.peek().is_some_and(char::is_ascii_digit) {
                        return None;
                    }
                    output.push_str("\\0");
                }
                '1'..='9' => return None,
                c @ ('n' | 'r' | 't' | 'b' | 'f' | 'v' | '\\' | '"') => {
                    output.push('\\');
                    output.push(c);
                }
                other => output.push(other),
            },
            other => output.push(other),
        }
    }
    output.push('"');
    Some(output)
}
