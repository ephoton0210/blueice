// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Token boundaries for copied source text. Replacements keep their explicit
//! edit anchors; only unchanged tokens can claim their original columns.

use crate::syntax::{Token, TokenKind};

pub(super) fn mark_start(source: &str, tokens: &[Token], index: usize) -> bool {
    let token = &tokens[index];
    if token.kind == TokenKind::Eof {
        return false;
    }
    if token.is("?") || token.is(":") && conditional_colon(tokens, index) {
        return true;
    }
    if token.is("(")
        && index > 0
        && matches!(
            tokens[index - 1].text.as_str(),
            "if" | "for" | "while" | "switch" | "with" | "catch"
        )
    {
        return false;
    }
    if token.is(";") && index > 0 && unlabelled_jump(tokens, index - 1) {
        return false;
    }
    // A function declaration's keyword belongs to the preceding export
    // modifier range in TypeScript's map, unlike a variable declaration.
    if token.is("function")
        && index > 0
        && (tokens[index - 1].is("export") || tokens[index - 1].is("async"))
    {
        return false;
    }
    // TypeScript keeps an empty parameter/call close on the opening `(`
    // position rather than assigning `)` its own source column. This also
    // lets a following body open inherit the callable-name end.
    if index > 0
        && (token.is(")") && tokens[index - 1].is("(")
            || token.is("]") && tokens[index - 1].is("["))
    {
        return false;
    }
    if token.is("(")
        && tokens
            .get(index + 1)
            .is_some_and(|next| next.is("yield") || next.is("await"))
    {
        return true;
    }
    if token.is("(") && grouping_open(tokens, index) {
        return true;
    }
    // A rewritten callable body with no intervening trivia begins at the
    // parameter-list end; TypeScript does not assign `{` a new source column.
    if token.is("{") && index > 0 && tokens[index - 1].is(")") {
        return control_body(tokens, index);
    }
    if token.is("{")
        && index > 0
        && matches!(
            tokens[index - 1].text.as_str(),
            "else" | "try" | "finally" | "do"
        )
    {
        return true;
    }
    // An assigned object literal begins at its own brace. The whitespace
    // before `=`-followed `{` does not make the brace a generated separator.
    if matches!(token.text.as_str(), "[" | "{")
        && index > 0
        && matches!(
            tokens[index - 1].text.as_str(),
            "=" | "," | "(" | "[" | "=>" | "import"
        )
    {
        return true;
    }
    // An expression starts at its own delimiter after return/throw. The
    // intervening trivia belongs to the statement keyword, not the value.
    if matches!(token.text.as_str(), "(" | "[" | "{")
        && index > 0
        && matches!(tokens[index - 1].text.as_str(), "return" | "throw")
    {
        return true;
    }
    if token.is("=>") {
        return true;
    }
    // Assignment punctuation belongs to the left-hand node's end even
    // when an erased annotation immediately precedes the equals sign.
    if token.is("=") {
        return false;
    }
    // A same-line object-literal close after a value inherits the value end
    // until the brace's closing segment, matching TypeScript's token boundary.
    if token.is("}")
        && index > 0
        && matches!(
            tokens[index - 1].kind,
            TokenKind::Number | TokenKind::String | TokenKind::Identifier
        )
        && source[tokens[index - 1].end..token.start]
            .bytes()
            .all(|byte| matches!(byte, b' ' | b'\t'))
    {
        return false;
    }
    if token.is("}")
        && index > 0
        && tokens[index - 1].is("}")
        && object_close(tokens, index)
        && source[tokens[index - 1].end..token.start]
            .bytes()
            .all(|byte| matches!(byte, b' ' | b'\t'))
    {
        return false;
    }
    if token.kind == TokenKind::Punct
        && !token.is("}")
        && source[..token.start]
            .chars()
            .next_back()
            .is_some_and(char::is_whitespace)
    {
        // Binary operators and opening braces after trivia inherit the end
        // of the preceding source node instead of the operator's own start.
        return false;
    }
    true
}

pub(super) fn mark_end(tokens: &[Token], index: usize) -> bool {
    let token = &tokens[index];
    // Empty parentheses can only be a parameter/call list, not an expression
    // group. Like their closing token, the following gap retains the opener.
    if token.is(")") && index > 0 && tokens[index - 1].is("(") {
        return false;
    }
    if token.is(")")
        && tokens.get(index + 1).is_some_and(|next| {
            matches!(
                next.text.as_str(),
                "+" | "-" | "*" | "/" | "%" | "**" | "&&" | "||" | "??"
            )
        })
    {
        return true;
    }
    if token.is("]") {
        return true;
    }
    if token.is(")")
        && tokens.get(index + 1).is_some_and(|token| {
            token.is("?") || token.is(":") && conditional_colon(tokens, index + 1)
        })
    {
        return true;
    }
    if token.is(")") {
        let mut depth = 1usize;
        for open in (0..index).rev() {
            if tokens[open].is(")") {
                depth += 1;
            } else if tokens[open].is("(") {
                depth -= 1;
                if depth == 0 {
                    return grouping_open(tokens, open);
                }
            }
        }
    }
    if token.is("?") || token.is(":") && conditional_colon(tokens, index) {
        return true;
    }
    if token.is(":") && case_colon(tokens, index) {
        return inline_case_body(tokens, index);
    }
    if matches!(token.kind, TokenKind::Identifier | TokenKind::Keyword)
        && (index > 0 && matches!(tokens[index - 1].text.as_str(), "." | "?.")
            || tokens.get(index + 1).is_some_and(|token| token.is(":")))
    {
        return true;
    }
    if token.is("{") && index > 0 && tokens[index - 1].is(")") {
        return control_body(tokens, index);
    }
    if token.is("{")
        && index > 0
        && matches!(
            tokens[index - 1].text.as_str(),
            "else" | "try" | "finally" | "do"
        )
    {
        return true;
    }
    if matches!(token.text.as_str(), "get" | "set")
        && tokens.get(index + 1).is_some_and(|next| {
            matches!(
                next.kind,
                TokenKind::Identifier | TokenKind::Keyword | TokenKind::String | TokenKind::Number
            ) || next.is("[")
        })
    {
        return false;
    }
    if token.is("=>")
        || (token.is("*")
            && index > 0
            && (tokens[index - 1].is("function") || tokens[index - 1].is("yield")))
    {
        return true;
    }
    if token.is("export")
        && tokens
            .get(index + 1)
            .is_some_and(|token| token.is("default"))
    {
        return false;
    }
    if token.is("default") && index > 0 && tokens[index - 1].is("export") {
        return false;
    }
    // TypeScript keeps `const`'s following trivia in the same declaration
    // range as the name. Its own map has no boundary at `const`'s end when
    // the emitted JavaScript is otherwise byte-identical.
    if matches!(
        token.text.as_str(),
        "const"
            | "let"
            | "var"
            | "import"
            | "if"
            | "for"
            | "while"
            | "with"
            | "catch"
            | "try"
            | "switch"
            | "case"
            | "function"
            | "class"
            | "return"
            | "throw"
            | "new"
            | "yield"
            | "await"
    ) || unlabelled_jump(tokens, index)
    {
        return false;
    }
    matches!(
        token.kind,
        TokenKind::Identifier
            | TokenKind::Keyword
            | TokenKind::Number
            | TokenKind::String
            | TokenKind::Template
    ) || matches!(token.text.as_str(), ";" | "}")
}

fn grouping_open(tokens: &[Token], index: usize) -> bool {
    index == 0
        || matches!(
            tokens[index - 1].text.as_str(),
            "=" | "new"
                | "return"
                | "throw"
                | "yield"
                | "await"
                | "("
                | "["
                | ","
                | ":"
                | "?"
                | "=>"
                | "+"
                | "-"
                | "*"
                | "/"
                | "%"
                | "**"
                | "??"
                | "||"
                | "&&"
        )
}

fn object_close(tokens: &[Token], index: usize) -> bool {
    let mut depth = 1usize;
    for open in (0..index).rev() {
        if tokens[open].is("}") {
            depth += 1;
        } else if tokens[open].is("{") {
            depth -= 1;
            if depth == 0 {
                return open > 0
                    && matches!(
                        tokens[open - 1].text.as_str(),
                        "=" | "return" | "(" | "[" | ","
                    );
            }
        }
    }
    false
}

fn block_keyword(tokens: &[Token], index: usize) -> Option<&str> {
    let mut depth = 1usize;
    for before in (0..index - 1).rev() {
        if tokens[before].is(")") {
            depth += 1;
        } else if tokens[before].is("(") {
            depth -= 1;
            if depth == 0 {
                return before
                    .checked_sub(1)
                    .map(|index| tokens[index].text.as_str());
            }
        }
    }
    None
}

fn control_body(tokens: &[Token], index: usize) -> bool {
    matches!(
        block_keyword(tokens, index),
        Some("if" | "for" | "while" | "with" | "switch" | "catch")
    )
}

fn case_colon(tokens: &[Token], index: usize) -> bool {
    let mut depth = 0usize;
    let mut conditional = 0usize;
    for token in tokens[..index].iter().rev() {
        match token.text.as_str() {
            ")" | "]" | "}" => depth += 1,
            "(" | "[" | "{" if depth > 0 => depth -= 1,
            ":" if depth == 0 => conditional += 1,
            "?" if depth == 0 && conditional > 0 => conditional -= 1,
            "case" | "default" if depth == 0 && conditional == 0 => {
                let mut braces = 0usize;
                for (open, token) in tokens[..index].iter().enumerate().rev() {
                    if token.is("}") {
                        braces += 1;
                    } else if token.is("{") {
                        if braces == 0 {
                            return open > 0
                                && tokens[open - 1].is(")")
                                && block_keyword(tokens, open) == Some("switch");
                        }
                        braces -= 1;
                    }
                }
                return false;
            }
            "?" | ";" | "{" if depth == 0 => return false,
            _ => {}
        }
    }
    false
}

fn inline_case_body(tokens: &[Token], index: usize) -> bool {
    let mut depth = 0usize;
    let mut statements = 0usize;
    for token in &tokens[index + 1..] {
        match token.text.as_str() {
            "case" | "default" | "}" if depth == 0 => break,
            "(" | "[" | "{" => depth += 1,
            ")" | "]" | "}" => depth = depth.saturating_sub(1),
            ";" if depth == 0 => statements += 1,
            _ => {}
        }
    }
    statements <= 1
}

fn unlabelled_jump(tokens: &[Token], index: usize) -> bool {
    matches!(tokens[index].text.as_str(), "break" | "continue")
        && tokens.get(index + 1).is_some_and(|token| token.is(";"))
        && !index
            .checked_sub(1)
            .is_some_and(|before| tokens[before].is(".") || tokens[before].is("?."))
}

fn conditional_colon(tokens: &[Token], index: usize) -> bool {
    let mut depth = 0usize;
    let mut nested = 0usize;
    for token in tokens[..index].iter().rev() {
        match token.text.as_str() {
            ")" | "]" | "}" => depth += 1,
            "(" | "[" | "{" if depth > 0 => depth -= 1,
            ":" if depth == 0 => nested += 1,
            "?" if depth == 0 && nested > 0 => nested -= 1,
            "?" if depth == 0 => return true,
            ";" | "{" | "case" | "default" if depth == 0 => return false,
            _ => {}
        }
    }
    false
}
