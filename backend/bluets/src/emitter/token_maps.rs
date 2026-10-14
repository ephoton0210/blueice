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
    // A function declaration's keyword belongs to the preceding export
    // modifier range in TypeScript's map, unlike a variable declaration.
    if token.is("function") && index > 0 && tokens[index - 1].is("export") {
        return false;
    }
    // TypeScript keeps an empty parameter/call close on the opening `(`
    // position rather than assigning `)` its own source column. This also
    // lets a following body open inherit the callable-name end.
    if token.is(")") && index > 0 && tokens[index - 1].is("(") {
        return false;
    }
    // A rewritten callable body with no intervening trivia begins at the
    // parameter-list end; TypeScript does not assign `{` a new source column.
    if token.is("{") && index > 0 && tokens[index - 1].is(")") {
        return false;
    }
    // An assigned object literal begins at its own brace. The whitespace
    // before `=`-followed `{` does not make the brace a generated separator.
    if token.is("{") && index > 0 && tokens[index - 1].is("=") {
        return true;
    }
    // A same-line object-literal close after a value inherits the value end
    // until the brace's closing segment, matching TypeScript's token boundary.
    if token.is("}")
        && index > 0
        && matches!(
            tokens[index - 1].kind,
            TokenKind::Number | TokenKind::String
        )
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

pub(super) fn mark_end(token: &Token) -> bool {
    // TypeScript keeps `const`'s following trivia in the same declaration
    // range as the name. Its own map has no boundary at `const`'s end when
    // the emitted JavaScript is otherwise byte-identical.
    if token.is("const") {
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
