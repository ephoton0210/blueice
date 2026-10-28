// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Method prefixes retain suspension kind independently from the member name.
use super::*;

pub(super) fn method_prefix(tokens: &[Token], mut index: usize, end: usize) -> (usize, bool, bool) {
    let async_function = tokens.get(index).is_some_and(|token| token.is("async"))
        && tokens.get(index + 1).is_some_and(|token| {
            matches!(
                token.kind,
                TokenKind::Identifier | TokenKind::Keyword | TokenKind::String | TokenKind::Number
            ) || token.is("*")
                || token.is("[")
        })
        && index + 1 < end;
    if async_function {
        index += 1;
    }
    let generator = tokens.get(index).is_some_and(|token| token.is("*"));
    if generator {
        index += 1;
    }
    (index.min(end.saturating_sub(1)), async_function, generator)
}
