// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Runtime-token boundaries used while preserving TypeScript erasure spans.

use super::*;

/// Splits a lexically valid JavaScript shift-token run into individual generic
/// closers for the TypeScript grammar. Every replacement token keeps its
/// original source byte, so diagnostics and erasure edits remain source-based.
pub(super) fn split_generic_closers(tokens: Vec<Token>) -> Vec<Token> {
    let mut split = Vec::with_capacity(tokens.len());
    for token in tokens {
        if token.kind == TokenKind::Punct
            && token.text.len() > 1
            && token.text.bytes().all(|byte| byte == b'>')
        {
            for start in token.start..token.end {
                split.push(Token {
                    kind: TokenKind::Punct,
                    text: ">".to_string(),
                    start,
                    end: start + 1,
                });
            }
        } else {
            split.push(token);
        }
    }
    split
}

pub(super) fn is_typed_arrow_parameter(tokens: &[Token], colon: usize, end: usize) -> bool {
    let mut parentheses = 0usize;
    let mut index = colon + 1;
    while index < end {
        match tokens[index].text.as_str() {
            "(" | "[" | "{" => parentheses += 1,
            ")" | "]" | "}" if parentheses == 0 => {
                return tokens.get(index + 1).is_some_and(|token| token.is("=>"));
            }
            ")" | "]" | "}" => parentheses -= 1,
            ";" => return false,
            _ => {}
        }
        index += 1;
    }
    false
}

pub(super) fn exponentiation_base_start(tokens: &[Token], exponent: usize) -> Option<usize> {
    let mut start = exponent.checked_sub(1)?;
    loop {
        match tokens.get(start)?.text.as_str() {
            ")" => {
                let open = matching_opening_delimiter(tokens, start, "(", ")")?;
                if open > 0 && token_ends_runtime_primary(&tokens[open - 1]) {
                    start = open - 1;
                    continue;
                }
                return Some(open);
            }
            "]" => {
                let open = matching_opening_delimiter(tokens, start, "[", "]")?;
                if open > 0 && token_ends_runtime_primary(&tokens[open - 1]) {
                    start = open - 1;
                    continue;
                }
                return Some(open);
            }
            _ if start >= 2
                && tokens[start - 1].is(".")
                && token_ends_runtime_primary(&tokens[start - 2]) =>
            {
                start -= 2;
            }
            _ => return Some(start),
        }
    }
}

pub(super) fn matching_opening_delimiter(
    tokens: &[Token],
    close: usize,
    opening: &str,
    closing: &str,
) -> Option<usize> {
    debug_assert!(tokens.get(close).is_some_and(|token| token.is(closing)));
    let mut depth = 0usize;
    for index in (0..=close).rev() {
        let token = &tokens[index];
        if token.is(closing) {
            depth += 1;
        } else if token.is(opening) {
            depth = depth.checked_sub(1)?;
            if depth == 0 {
                return Some(index);
            }
        }
    }
    None
}

pub(super) fn token_ends_runtime_primary(token: &Token) -> bool {
    matches!(
        token.kind,
        TokenKind::Identifier | TokenKind::Number | TokenKind::String | TokenKind::Template
    ) || matches!(
        token.text.as_str(),
        "true" | "false" | "null" | "undefined" | ")" | "]"
    )
}

pub(super) fn is_unparenthesized_unary_exponent_base(tokens: &[Token], operator: usize) -> bool {
    let token = &tokens[operator];
    match token.text.as_str() {
        "!" | "~" | "typeof" | "void" | "delete" => true,
        "+" | "-" => {
            operator == 0
                || tokens.get(operator - 1).is_some_and(|previous| {
                    matches!(
                        previous.text.as_str(),
                        "(" | "["
                            | "{"
                            | "?"
                            | ":"
                            | ","
                            | ";"
                            | "="
                            | "+"
                            | "-"
                            | "*"
                            | "/"
                            | "%"
                            | "**"
                            | "<<"
                            | ">>"
                            | ">>>"
                            | "&"
                            | "^"
                            | "|"
                            | "&&"
                            | "||"
                            | "??"
                            | "return"
                            | "throw"
                            | "case"
                            | "=>"
                    )
                })
        }
        _ => false,
    }
}

/// Generic arrow functions need type-parameter erasure, but the initial
/// matrix only supports generic declarations and direct calls. Recognize the
/// complete `<...>(...) =>` shape so it cannot be preserved as invalid
/// JavaScript by an otherwise opaque expression span.
pub(super) fn is_generic_arrow_function(tokens: &[Token], start: usize, end: usize) -> bool {
    if !tokens.get(start).is_some_and(|token| token.is("<")) {
        return false;
    }
    let Some(type_parameters_end) = matching_angle_bracket(tokens, start, end) else {
        return false;
    };
    let parameters_start = type_parameters_end + 1;
    if !tokens
        .get(parameters_start)
        .is_some_and(|token| token.is("("))
    {
        return false;
    }
    let Some(parameters_end) = matching_parenthesis(tokens, parameters_start, end) else {
        return false;
    };
    tokens
        .get(parameters_end + 1)
        .is_some_and(|token| token.is("=>"))
}

pub(super) fn matching_parenthesis(tokens: &[Token], start: usize, limit: usize) -> Option<usize> {
    debug_assert!(tokens.get(start).is_some_and(|token| token.is("(")));
    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate().take(limit).skip(start) {
        match token.text.as_str() {
            "(" => depth += 1,
            ")" => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}

pub(super) fn find_balanced_delimiter(
    tokens: &[Token],
    start: usize,
    limit: usize,
    delimiters: &[&str],
) -> usize {
    let mut index = start;
    let mut parentheses = 0usize;
    let mut brackets = 0usize;
    let mut braces = 0usize;
    while index < limit {
        let token = &tokens[index];
        match token.text.as_str() {
            "(" => parentheses += 1,
            ")" if parentheses > 0 => parentheses -= 1,
            "[" => brackets += 1,
            "]" if brackets > 0 => brackets -= 1,
            "{" => braces += 1,
            "}" if braces > 0 => braces -= 1,
            _ if parentheses == 0
                && brackets == 0
                && braces == 0
                && delimiters.iter().any(|delimiter| token.is(delimiter)) =>
            {
                return index
            }
            _ => {}
        }
        index += 1;
    }
    limit
}

/// Identifies the first token of an expression statement that the bounded
/// parser can retain structurally inside a function body. Control-flow and
/// declaration keywords intentionally remain opaque until they gain their
/// own body-item representation, so the direct bridge cannot reinterpret a
/// statement grammar as an expression.
pub(super) fn starts_runtime_expression_statement(token: &Token) -> bool {
    matches!(
        token.kind,
        TokenKind::Number | TokenKind::String | TokenKind::Template | TokenKind::Identifier
    ) || matches!(
        token.text.as_str(),
        "(" | "["
            | "+"
            | "-"
            | "++"
            | "--"
            | "!"
            | "~"
            | "true"
            | "false"
            | "null"
            | "undefined"
            | "new"
            | "delete"
            | "typeof"
            | "void"
    )
}

/// Determines whether an `if` statement can use the bounded structured
/// function-body representation. Each branch must be explicitly braced so a
/// parser that otherwise preserves unsupported statements as opaque tokens
/// never changes dangling-`else` or automatic-semicolon-insertion behavior.
pub(super) fn is_direct_braced_if_statement(tokens: &[Token], start: usize) -> bool {
    if !tokens.get(start).is_some_and(|token| token.is("if"))
        || !tokens.get(start + 1).is_some_and(|token| token.is("("))
    {
        return false;
    }
    let limit = tokens.len().saturating_sub(1);
    let Some(test_end) = matching_closing_delimiter(tokens, start + 1, limit, "(", ")") else {
        return false;
    };
    let Some(consequent_opening) = tokens.get(test_end + 1) else {
        return false;
    };
    if !consequent_opening.is("{") {
        return false;
    }
    let Some(consequent_end) = matching_closing_delimiter(tokens, test_end + 1, limit, "{", "}")
    else {
        return false;
    };
    let Some(next) = tokens.get(consequent_end + 1) else {
        return true;
    };
    if !next.is("else") {
        return true;
    }
    let Some(alternate_start) = tokens.get(consequent_end + 2) else {
        return false;
    };
    if alternate_start.is("{") {
        return matching_closing_delimiter(tokens, consequent_end + 2, limit, "{", "}").is_some();
    }
    alternate_start.is("if") && is_direct_braced_if_statement(tokens, consequent_end + 2)
}

pub(super) fn matching_closing_delimiter(
    tokens: &[Token],
    start: usize,
    limit: usize,
    opening: &str,
    closing: &str,
) -> Option<usize> {
    debug_assert!(tokens.get(start).is_some_and(|token| token.is(opening)));
    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate().take(limit).skip(start) {
        if token.is(opening) {
            depth += 1;
        } else if token.is(closing) {
            depth = depth.checked_sub(1)?;
            if depth == 0 {
                return Some(index);
            }
        }
    }
    None
}

pub(super) fn matching_angle_bracket(
    tokens: &[Token],
    start: usize,
    limit: usize,
) -> Option<usize> {
    debug_assert!(tokens.get(start).is_some_and(|token| token.is("<")));
    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate().take(limit).skip(start) {
        match token.text.as_str() {
            "<" => depth += 1,
            ">" => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}
