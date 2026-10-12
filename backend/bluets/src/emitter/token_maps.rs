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
    matches!(
        token.kind,
        TokenKind::Identifier
            | TokenKind::Keyword
            | TokenKind::Number
            | TokenKind::String
            | TokenKind::Template
    ) || matches!(token.text.as_str(), ";" | "}")
}
