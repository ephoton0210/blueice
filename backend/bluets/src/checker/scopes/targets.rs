// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Assignment targets, including recursive destructuring and iteration heads.

use super::*;

pub(super) const ASSIGNMENTS: &[&str] = &[
    "=", "+=", "-=", "*=", "/=", "%=", "**=", "<<=", ">>=", ">>>=", "&=", "|=", "^=", "&&=", "||=",
    "??=",
];

/// Keep private identifiers atomic just as the parser's runtime tokens do.
/// The source lexer emits `#` separately; both paths must describe the same
/// mutation target and retain its complete original byte span.
pub(super) fn private_identifiers(tokens: Vec<Token>) -> Vec<Token> {
    let mut joined: Vec<Token> = Vec::with_capacity(tokens.len());
    for token in tokens {
        if matches!(token.kind, TokenKind::Identifier | TokenKind::Keyword)
            && joined
                .last()
                .is_some_and(|hash| hash.is("#") && hash.end == token.start)
        {
            let hash = joined.pop().expect("the private marker was just matched");
            joined.push(Token {
                kind: TokenKind::Identifier,
                text: format!("#{}", token.text),
                start: hash.start,
                end: token.end,
            });
        } else {
            joined.push(token);
        }
    }
    joined
}

pub(super) fn backward(tokens: &[Token], end: usize, limit: usize) -> Result<&[Token], ()> {
    let mut depth = 0usize;
    let mut start = 0;
    for (steps, index) in (0..end).rev().enumerate() {
        if steps >= limit {
            return Err(());
        }
        // A control-flow head or a preceding block ends before the next
        // identifier; calls retain their closing delimiter before `.`/`[`.
        if depth == 0
            && (tokens[index].is(")") || tokens[index].is("}"))
            && tokens
                .get(index + 1)
                .is_some_and(super::expressions::is_value_name)
        {
            start = index + 1;
            break;
        }
        match tokens[index].text.as_str() {
            ")" | "]" | "}" => depth += 1,
            "(" | "[" | "{" if depth > 0 => depth -= 1,
            "(" | "[" | "{" => {
                start = index + 1;
                break;
            }
            _ if depth == 0 && boundary(&tokens[index]) => {
                start = index + 1;
                break;
            }
            _ => {}
        }
    }
    Ok(&tokens[start..end])
}

pub(super) fn forward(tokens: &[Token], start: usize, limit: usize) -> Result<&[Token], ()> {
    let mut depth = 0usize;
    let mut end = tokens.len();
    for (steps, index) in (start..tokens.len()).enumerate() {
        if steps >= limit {
            return Err(());
        }
        match tokens[index].text.as_str() {
            "(" | "[" | "{" => depth += 1,
            ")" | "]" | "}" if depth > 0 => depth -= 1,
            ")" | "]" | "}" => {
                end = index;
                break;
            }
            _ if depth == 0 && boundary(&tokens[index]) => {
                end = index;
                break;
            }
            _ => {}
        }
    }
    Ok(&tokens[start..end])
}

fn boundary(token: &Token) -> bool {
    ASSIGNMENTS.contains(&token.text.as_str())
        || matches!(
            token.text.as_str(),
            "," | ";"
                | ":"
                | "?"
                | "+"
                | "-"
                | "*"
                | "/"
                | "%"
                | "**"
                | "&&"
                | "||"
                | "??"
                | "&"
                | "|"
                | "^"
                | "=="
                | "==="
                | "!="
                | "!=="
                | "<"
                | ">"
                | "<="
                | ">="
                | "<<"
                | ">>"
                | ">>>"
                | "in"
                | "instanceof"
                | "=>"
                | "++"
                | "--"
                | "const"
                | "let"
                | "var"
                | "return"
                | "throw"
                | "else"
                | "yield"
        )
}

pub(super) fn strip(mut tokens: &[Token]) -> &[Token] {
    while tokens.first().is_some_and(|token| token.is("("))
        && close(tokens, 0) == Some(tokens.len() - 1)
    {
        tokens = &tokens[1..tokens.len() - 1];
    }
    tokens
}

pub(super) fn close(tokens: &[Token], start: usize) -> Option<usize> {
    let end = match tokens.get(start)?.text.as_str() {
        "(" => ")",
        "[" => "]",
        "{" => "}",
        _ => return None,
    };
    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate().skip(start) {
        if token.text == tokens[start].text {
            depth += 1;
        }
        if token.text == end {
            depth = depth.checked_sub(1)?;
            if depth == 0 {
                return Some(index);
            }
        }
    }
    None
}

pub(super) fn top_level(tokens: &[Token], separator: &str) -> Option<usize> {
    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate() {
        if depth == 0 && token.is(separator) {
            return Some(index);
        }
        match token.text.as_str() {
            "(" | "[" | "{" => depth += 1,
            ")" | "]" | "}" => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    None
}

/// Keys, computed-key expressions and default RHS expressions are reads;
/// only the recursively selected binding targets are writes.
pub(super) fn pattern<'a>(
    tokens: &'a [Token],
    out: &mut Vec<&'a [Token]>,
    depth: usize,
) -> Result<(), ()> {
    if depth > 128 {
        return Err(());
    }
    let mut tokens = strip(tokens);
    if tokens.first().is_some_and(|token| token.is("...")) {
        tokens = &tokens[1..];
    }
    if let Some(default) = top_level(tokens, "=") {
        tokens = &tokens[..default];
    }
    let Some(first) = tokens.first() else {
        return Ok(());
    };
    if (first.is("[") || first.is("{")) && close(tokens, 0) == Some(tokens.len() - 1) {
        let object = first.is("{");
        let mut elements = &tokens[1..tokens.len() - 1];
        while !elements.is_empty() {
            let end = top_level(elements, ",").unwrap_or(elements.len());
            let mut element = &elements[..end];
            if object {
                if let Some(colon) = top_level(element, ":") {
                    element = &element[colon + 1..];
                }
            }
            pattern(element, out, depth + 1)?;
            elements = elements.get(end + 1..).unwrap_or(&[]);
        }
    } else {
        out.push(tokens);
    }
    Ok(())
}

pub(super) fn member(tokens: &[Token]) -> Option<(&[Token], Option<&str>, &Token)> {
    let tokens = strip(tokens);
    let last = tokens.last()?;
    let len = tokens.len();
    if len >= 3 && tokens[len - 2].is(".") {
        return Some((&tokens[..len - 2], Some(&last.text), last));
    }
    if !last.is("]") {
        return None;
    }
    let mut depth = 0usize;
    for index in (0..len).rev() {
        if tokens[index].is("]") {
            depth += 1;
        }
        if tokens[index].is("[") {
            depth = depth.checked_sub(1)?;
            if depth == 0 && index > 0 {
                let (key, token) = match &tokens[index + 1..len - 1] {
                    [token] if token.kind == TokenKind::String => (
                        token
                            .text
                            .get(1..token.text.len() - 1)
                            .filter(|text| !text.contains('\\')),
                        token,
                    ),
                    [token] if token.kind == TokenKind::Number => {
                        (Some(token.text.as_str()), token)
                    }
                    _ => (None, last),
                };
                return Some((&tokens[..index], key, token));
            }
        }
    }
    None
}

pub(super) fn iteration(tokens: &[Token], start: usize) -> Option<(&[Token], &Token)> {
    let open = (start + 1..tokens.len()).find(|index| tokens[*index].is("("))?;
    let end = close(tokens, open)?;
    let head = &tokens[open + 1..end];
    if head
        .first()
        .is_some_and(|token| matches!(token.text.as_str(), "const" | "let" | "var"))
    {
        return None;
    }
    if top_level(head, ";").is_some() {
        return None;
    }
    let operator = top_level(head.get(1..)?, "of").or_else(|| top_level(head.get(1..)?, "in"))? + 1;
    Some((&head[..operator], &head[operator]))
}
