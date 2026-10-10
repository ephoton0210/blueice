// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Braced statement boundaries before following module declarations.

use super::*;

/// The exclusive token end of a complete braced statement. Unbraced forms keep
/// the existing semicolon boundary. Each matching scan consumes disjoint heads
/// and bodies; else-if chains are iterative rather than recursive.
pub(super) fn end(tokens: &[Token], start: usize, limit: usize) -> Option<usize> {
    let mut cursor = start;
    loop {
        let token = tokens.get(cursor)?;
        let mut next = match token.text.as_str() {
            "{" => block_end(tokens, cursor, limit)?,
            "for" | "while" | "if" | "switch" | "with" => {
                let head = cursor
                    + 1
                    + usize::from(
                        token.is("for")
                            && tokens
                                .get(cursor + 1)
                                .is_some_and(|token| token.is("await")),
                    );
                if !tokens.get(head)?.is("(") {
                    return None;
                }
                let close = matching_closing_delimiter(tokens, head, limit, "(", ")")?;
                block_end(tokens, close + 1, limit)?
            }
            "try" => {
                let mut next = block_end(tokens, cursor + 1, limit)?;
                let mut completed = false;
                if tokens.get(next).is_some_and(|token| token.is("catch")) {
                    next += 1;
                    if tokens.get(next)?.is("(") {
                        next = matching_closing_delimiter(tokens, next, limit, "(", ")")? + 1;
                    }
                    next = block_end(tokens, next, limit)?;
                    completed = true;
                }
                if tokens.get(next).is_some_and(|token| token.is("finally")) {
                    next = block_end(tokens, next + 1, limit)?;
                    completed = true;
                }
                if !completed {
                    return None;
                }
                next
            }
            "do" => {
                let next = block_end(tokens, cursor + 1, limit)?;
                if !tokens.get(next)?.is("while") || !tokens.get(next + 1)?.is("(") {
                    return None;
                }
                matching_closing_delimiter(tokens, next + 1, limit, "(", ")")? + 1
            }
            _ => return None,
        };
        if token.is("if") && tokens.get(next).is_some_and(|token| token.is("else")) {
            next += 1;
            if tokens.get(next)?.is("if") {
                cursor = next;
                continue;
            }
            next = block_end(tokens, next, limit)?;
        }
        return Some(next);
    }
}

fn block_end(tokens: &[Token], open: usize, limit: usize) -> Option<usize> {
    tokens.get(open)?.is("{").then_some(())?;
    matching_closing_delimiter(tokens, open, limit, "{", "}").map(|close| close + 1)
}
