// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! ES2016 exponentiation on ordinary update/unary operands.

use super::*;

pub(super) fn lower(
    mut emitted: EmittedJavaScript,
    module: &Module,
    options: &CompilerOptions,
) -> Result<EmittedJavaScript, Diagnostic> {
    if options.target >= crate::EcmaTarget::Es2016 {
        return Ok(emitted);
    }
    let mut remaining_tokens = options.limits.parser.max_tokens;
    while emitted.javascript.contains("**") {
        let tokens = crate::syntax::lex_with_limits(
            &module.id,
            &emitted.javascript,
            options.limits.parser.max_source_bytes,
            remaining_tokens,
        )
        .map_err(|mut diagnostics| diagnostics.remove(0))?;
        remaining_tokens = remaining_tokens.saturating_sub(tokens.len());
        // Right-associative chains are replaced from the right. Parenthesized
        // operands and member/call chains retain their original evaluation.
        let Some(operator) = tokens.iter().rposition(|token| token.is("**")) else {
            break;
        };
        let Some((start, end)) =
            operand_start(&tokens, operator).zip(operand_end(&tokens, operator + 1))
        else {
            return Err(Diagnostic::error(
                DiagnosticCode::UnsupportedSyntax,
                SourceSpan::new(&module.id, 0, 0),
                "target exponentiation lowering requires an update or unary operand",
            ));
        };
        let left = &emitted.javascript[tokens[start].start..tokens[operator - 1].end];
        let right = &emitted.javascript[tokens[operator + 1].start..tokens[end - 1].end];
        let replacement = format!("Math.pow({left}, {right})");
        emitted = mapped_edits(
            emitted,
            vec![TextEdit {
                start: tokens[start].start,
                end: tokens[end - 1].end,
                replacement,
            }],
        );
    }
    Ok(emitted)
}

pub(super) fn operand_start(tokens: &[Token], end: usize) -> Option<usize> {
    let mut cursor = end.checked_sub(1)?;
    if matches!(tokens[cursor].text.as_str(), "++" | "--") {
        cursor = cursor.checked_sub(1)?;
    }
    loop {
        if tokens[cursor].kind == TokenKind::Template
            && cursor > 0
            && ends_primary(&tokens[cursor - 1])
        {
            cursor -= 1;
            continue;
        }
        if cursor >= 2 && matches!(tokens[cursor - 1].text.as_str(), "." | "?.") {
            cursor -= 2;
            continue;
        }
        match tokens[cursor].text.as_str() {
            ")" | "]" => {
                let bracket = tokens[cursor].is("]");
                cursor = matching_open(tokens, cursor)?;
                if cursor > 0 && ends_primary(&tokens[cursor - 1]) {
                    cursor -= 1;
                    continue;
                }
                if !bracket && cursor > 0 && tokens[cursor - 1].is("?.") {
                    cursor = cursor.checked_sub(2)?;
                    continue;
                }
            }
            "}" => {
                cursor = matching_open(tokens, cursor)?;
                // A function/class body is not an object operand.
                if cursor > 0 && tokens[cursor - 1].is(")") {
                    return None;
                }
            }
            _ if atom(&tokens[cursor]) => {}
            _ => return None,
        }
        while cursor > 0 && matches!(tokens[cursor - 1].text.as_str(), "new" | "++" | "--") {
            cursor -= 1;
        }
        return Some(cursor);
    }
}

pub(super) fn operand_end(tokens: &[Token], start: usize) -> Option<usize> {
    let mut cursor = start;
    while matches!(
        tokens.get(cursor)?.text.as_str(),
        "+" | "-" | "!" | "~" | "typeof" | "void" | "delete" | "await" | "++" | "--" | "new"
    ) {
        cursor += 1;
    }
    cursor = match tokens.get(cursor)?.text.as_str() {
        "(" | "[" | "{" => matching_close(tokens, cursor)? + 1,
        _ if atom(&tokens[cursor]) => cursor + 1,
        _ => return None,
    };
    loop {
        match tokens.get(cursor)?.text.as_str() {
            "." | "?." => {
                cursor += 1;
                if tokens.get(cursor)?.is("(") || tokens[cursor].is("[") {
                    cursor = matching_close(tokens, cursor)? + 1;
                } else if matches!(
                    tokens[cursor].kind,
                    TokenKind::Identifier | TokenKind::Keyword
                ) {
                    cursor += 1;
                } else {
                    return None;
                }
            }
            "(" | "[" => cursor = matching_close(tokens, cursor)? + 1,
            _ if tokens[cursor].kind == TokenKind::Template => cursor += 1,
            "++" | "--" => return Some(cursor + 1),
            _ => return Some(cursor),
        }
    }
}

fn atom(token: &Token) -> bool {
    matches!(
        token.kind,
        TokenKind::Identifier | TokenKind::Number | TokenKind::String | TokenKind::Template
    ) || matches!(
        token.text.as_str(),
        "this" | "super" | "true" | "false" | "null" | "undefined"
    )
}

fn ends_primary(token: &Token) -> bool {
    atom(token) || matches!(token.text.as_str(), ")" | "]" | "}")
}

pub(super) fn matching_open(tokens: &[Token], close: usize) -> Option<usize> {
    let (opening, closing) = match tokens[close].text.as_str() {
        ")" => ("(", ")"),
        "]" => ("[", "]"),
        "}" => ("{", "}"),
        _ => return None,
    };
    let mut depth = 1usize;
    for index in (0..close).rev() {
        if tokens[index].is(closing) {
            depth += 1;
        } else if tokens[index].is(opening) {
            depth -= 1;
            if depth == 0 {
                return Some(index);
            }
        }
    }
    None
}

pub(super) fn matching_close(tokens: &[Token], open: usize) -> Option<usize> {
    let (opening, closing) = match tokens[open].text.as_str() {
        "(" => ("(", ")"),
        "[" => ("[", "]"),
        "{" => ("{", "}"),
        _ => return None,
    };
    let mut depth = 1usize;
    for (index, token) in tokens.iter().enumerate().skip(open + 1) {
        if token.is(opening) {
            depth += 1;
        } else if token.is(closing) {
            depth -= 1;
            if depth == 0 {
                return Some(index);
            }
        }
    }
    None
}
