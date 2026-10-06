// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Bounded case completion: nested-loop breaks cannot exit a case.

use super::*;
use expressions::matching_end;

pub(super) fn terminates(tokens: &[Token], remaining: usize) -> bool {
    if remaining == 0 {
        return false;
    }
    let mut cursor = 0;
    while let Some(token) = tokens.get(cursor) {
        if matches!(
            token.text.as_str(),
            "break" | "continue" | "return" | "throw"
        ) {
            return true;
        }
        if token.is("if") && tokens.get(cursor + 1).is_some_and(|t| t.is("(")) {
            let Some(close) = matching_end(tokens, cursor + 1, "(", ")") else {
                return false;
            };
            let start = close + 1;
            let end = statement_end(tokens, start);
            let consequent = terminates(statement_body(&tokens[start..end]), remaining - 1);
            let literal = tokens
                .get(cursor + 2)
                .filter(|_| close == cursor + 3)
                .map(|t| t.text.as_str());
            if literal == Some("true") && consequent {
                return true;
            }
            cursor = end;
            if tokens.get(cursor).is_some_and(|t| t.is("else")) {
                let start = cursor + 1;
                let end = statement_end(tokens, start);
                let alternate = terminates(statement_body(&tokens[start..end]), remaining - 1);
                if (consequent || literal == Some("false")) && alternate {
                    return true;
                }
                cursor = end;
            }
            continue;
        }
        if token.is("{") {
            let Some(end) = matching_end(tokens, cursor, "{", "}") else {
                return false;
            };
            if terminates(&tokens[cursor + 1..end], remaining - 1) {
                return true;
            }
            cursor = end + 1;
            continue;
        }
        if matches!(token.text.as_str(), "while" | "for" | "switch" | "with") {
            let Some(open) = tokens
                .get(cursor + 1)
                .filter(|t| t.is("("))
                .map(|_| cursor + 1)
            else {
                return false;
            };
            let Some(close) = matching_end(tokens, open, "(", ")") else {
                return false;
            };
            cursor = statement_end(tokens, close + 1);
            continue;
        }
        if matches!(token.text.as_str(), "function" | "class" | "do" | "try") {
            let Some(open) = (cursor + 1..tokens.len()).find(|i| tokens[*i].is("{")) else {
                return false;
            };
            cursor = matching_end(tokens, open, "{", "}").map_or(tokens.len(), |end| end + 1);
            continue;
        }
        let next = statement_end(tokens, cursor);
        cursor = next.max(cursor + 1);
    }
    false
}

fn statement_body(tokens: &[Token]) -> &[Token] {
    if tokens.first().is_some_and(|t| t.is("{")) && tokens.last().is_some_and(|t| t.is("}")) {
        &tokens[1..tokens.len() - 1]
    } else {
        tokens
    }
}

fn statement_end(tokens: &[Token], start: usize) -> usize {
    if tokens.get(start).is_some_and(|t| t.is("{")) {
        return matching_end(tokens, start, "{", "}").map_or(tokens.len(), |end| end + 1);
    }
    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate().skip(start) {
        if depth == 0 && token.is(";") {
            return index + 1;
        }
        match token.text.as_str() {
            "(" | "[" | "{" => depth += 1,
            ")" | "]" | "}" => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    tokens.len()
}
