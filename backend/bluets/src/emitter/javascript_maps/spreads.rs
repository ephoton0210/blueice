// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! An ES5 apply call ends at its original final argument, not a retained `)`.
use crate::{CompilerOptions, EcmaTarget, Token, TokenKind};
use std::collections::BTreeMap;

pub(super) fn end_anchors(tokens: &[Token], options: &CompilerOptions) -> BTreeMap<usize, usize> {
    let mut anchors = BTreeMap::new();
    if options.target >= EcmaTarget::Es2015 {
        return anchors;
    }
    let mut stack = Vec::<(usize, bool, bool)>::new();
    for (index, token) in tokens.iter().enumerate() {
        match token.text.as_str() {
            "(" | "[" | "{" => {
                let call = token.is("(")
                    && index > 0
                    && (tokens[index - 1].kind == TokenKind::Identifier
                        || matches!(tokens[index - 1].text.as_str(), ")" | "]"))
                    && !(index >= 2
                        && matches!(tokens[index - 2].text.as_str(), "function" | "new" | "*"));
                stack.push((index, call, false));
            }
            "..." => {
                if let Some((_, true, spread)) = stack.last_mut() {
                    *spread = true;
                }
            }
            ")" | "]" | "}" => {
                if let Some((open, true, true)) = stack.pop() {
                    if token.is(")") && index > open + 1 {
                        anchors.insert(token.end, tokens[index - 1].end);
                    }
                }
            }
            _ => {}
        }
    }
    anchors
}
