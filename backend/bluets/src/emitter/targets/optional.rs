// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Terminal optional properties and nullish coalescing below ES2020.
//! Original helpers and conditional expressions preserve single reads and
//! keep await/yield and right-side effects in their original execution.

use super::*;
use crate::emitter::relocations;
mod origins;

const PROPERTY_HELPER: &str = include_str!("optional_helpers.v1.js");

pub(super) fn lower(
    mut emitted: EmittedJavaScript,
    module: &Module,
    options: &CompilerOptions,
) -> Result<EmittedJavaScript, Diagnostic> {
    if options.target >= crate::EcmaTarget::Es2020 {
        return Ok(emitted);
    }
    let mut remaining = options.limits.parser.max_tokens;
    let mut sequence = 0;
    loop {
        let tokens = crate::syntax::lex_with_limits(
            &module.id,
            &emitted.javascript,
            options.limits.parser.max_source_bytes,
            remaining,
        )
        .map_err(|mut diagnostics| diagnostics.remove(0))?;
        remaining = remaining.saturating_sub(tokens.len());
        let candidate = tokens
            .iter()
            .rposition(|token| token.is("?."))
            .or_else(|| tokens.iter().rposition(|token| token.is("??")));
        let Some(operator) = candidate else {
            return Ok(emitted);
        };
        let token = &tokens[operator];
        let insertion = directive_end(&tokens);
        let mut edits = Vec::new();
        let (start, end, replacement) = if token.is("?.") {
            let start = exponentiation::operand_start(&tokens, operator).ok_or_else(|| {
                unsupported(
                    module,
                    "optional receiver is outside the target lowering subset",
                )
            })?;
            let property = tokens
                .get(operator + 1)
                .filter(|token| matches!(token.kind, TokenKind::Identifier | TokenKind::Keyword))
                .ok_or_else(|| {
                    unsupported(module, "target optional lowering requires a named property")
                })?;
            let after = tokens.get(operator + 2);
            let mut outer = operator + 2;
            while tokens.get(outer).is_some_and(|token| token.is(")")) {
                outer += 1;
            }
            if after.is_some_and(|token| {
                matches!(token.text.as_str(), "." | "[" | "(") || token.kind == TokenKind::Template
            }) || tokens.get(outer).is_some_and(|token| token.is("("))
                || start > 0 && tokens[start - 1].is("delete")
            {
                return Err(unsupported(
                    module,
                    "target optional property lowering requires a terminal value read",
                ));
            }
            let helper = unused_name(&emitted.javascript, "optional_property", &mut sequence);
            let external = crate::emitter::helper_selection::external(options);
            let temporary = unused_name(&emitted.javascript, "optional_receiver", &mut sequence);
            edits.push(TextEdit {
                start: insertion,
                end: insertion,
                replacement: if external {
                    format!("\nvar {temporary};\n")
                } else {
                    format!(
                        "\n{}\n",
                        PROPERTY_HELPER.replace("__blueice_target_optional_property", &helper)
                    )
                },
            });
            let receiver =
                relocations::relocated_verbatim(tokens[start].start, tokens[operator - 1].end);
            let key = serde_json::to_string(&property.text).expect("property name is serializable");
            (
                tokens[start].start,
                property.end,
                origins::property(
                    &receiver,
                    &key,
                    &temporary,
                    &helper,
                    external,
                    (
                        tokens[start].start,
                        tokens[operator - 1].end,
                        property.start,
                        property.end,
                    ),
                ),
            )
        } else {
            let start = coalesce_start(&tokens, operator);
            let end = coalesce_end(&tokens, operator + 1);
            if start >= operator || end <= operator + 1 {
                return Err(unsupported(
                    module,
                    "target nullish lowering requires both operands",
                ));
            }
            let temporary = unused_name(&emitted.javascript, "coalesce_value", &mut sequence);
            let helper = unused_name(&emitted.javascript, "coalesce_nullish", &mut sequence);
            let external = crate::emitter::helper_selection::external(options);
            edits.push(TextEdit {
                start: insertion,
                end: insertion,
                replacement: if external {
                    format!("\nvar {temporary};\n")
                } else {
                    format!(
                        "\nvar {temporary};\n{}\n",
                        TARGET_HELPER_V1_SOURCE.replace("__blueice_target_is_nullish", &helper)
                    )
                },
            });
            let left =
                relocations::relocated_verbatim(tokens[start].start, tokens[operator - 1].end);
            let right =
                relocations::relocated_verbatim(tokens[operator + 1].start, tokens[end - 1].end);
            (
                tokens[start].start,
                tokens[end - 1].end,
                origins::coalesce(
                    &left,
                    &right,
                    &temporary,
                    &helper,
                    external,
                    (
                        tokens[start].start,
                        tokens[operator - 1].end,
                        tokens[end - 1].end,
                    ),
                ),
            )
        };
        edits.push(TextEdit {
            start,
            end,
            replacement,
        });
        emitted = mapped_edits(emitted, edits);
    }
}

fn unsupported(module: &Module, message: &str) -> Diagnostic {
    Diagnostic::error(
        DiagnosticCode::UnsupportedSyntax,
        SourceSpan::new(&module.id, 0, 0),
        message,
    )
}

fn coalesce_start(tokens: &[Token], operator: usize) -> usize {
    let mut depth = 0usize;
    for index in (0..operator).rev() {
        let token = &tokens[index];
        match token.text.as_str() {
            ")" | "]" | "}" => depth += 1,
            "(" | "[" | "{" if depth == 0 => return index + 1,
            "(" | "[" | "{" => depth -= 1,
            ";" | "," | "=" | "=>" | "?" | ":" | "??" | "return" | "throw" | "yield"
                if depth == 0 =>
            {
                return index + 1
            }
            _ if depth == 0
                && matches!(
                    token.text.as_str(),
                    "+=" | "-=" | "*=" | "/=" | "%=" | "**=" | "&&=" | "||=" | "??="
                ) =>
            {
                return index + 1
            }
            _ => {}
        }
    }
    0
}

fn coalesce_end(tokens: &[Token], start: usize) -> usize {
    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate().skip(start) {
        match token.text.as_str() {
            "(" | "[" | "{" => depth += 1,
            ")" | "]" | "}" if depth == 0 => return index,
            ")" | "]" | "}" => depth -= 1,
            ";" | "," | "?" | ":" if depth == 0 => return index,
            _ if token.kind == TokenKind::Eof => return index,
            _ => {}
        }
    }
    tokens.len()
}
