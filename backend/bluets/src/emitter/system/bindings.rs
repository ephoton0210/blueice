// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Factory scope owns every name in a lowered declaration, including commas.
use crate::syntax::{Token, TokenKind};

pub(super) fn statement_end(tokens: &[Token], start: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate().skip(start) {
        match token.text.as_str() {
            ";" if depth == 0 => return Some(index),
            "," if depth == 0 => return None,
            "(" | "[" | "{" => depth += 1,
            ")" | "]" | "}" => depth = depth.checked_sub(1)?,
            _ if token.kind == TokenKind::Eof => return None,
            _ => {}
        }
    }
    None
}

pub(super) fn class_end(tokens: &[Token], start: usize) -> Option<usize> {
    let mut index = start + 1;
    let mut grouped = 0usize;
    let mut nested = 0usize;
    while let Some(token) = tokens.get(index) {
        match token.text.as_str() {
            "(" | "[" => grouped += 1,
            ")" | "]" => grouped = grouped.checked_sub(1)?,
            "class" | "function" if grouped == 0 => nested += 1,
            "{" if grouped == 0 => {
                let end = super::closing(tokens, index)?;
                if nested == 0 {
                    return Some(end);
                }
                nested -= 1;
                index = end + 1;
                continue;
            }
            _ if token.kind == TokenKind::Eof => return None,
            _ => {}
        }
        index += 1;
    }
    None
}

pub(super) fn names(tokens: &[Token], start: usize) -> Option<Vec<String>> {
    let mut names = Vec::new();
    let mut index = start;
    loop {
        let name = tokens.get(index)?;
        if name.kind != TokenKind::Identifier {
            return None;
        }
        names.push(name.text.clone());
        index += 1;
        let mut depth = 0usize;
        while let Some(token) = tokens.get(index) {
            match token.text.as_str() {
                "(" | "[" | "{" => depth += 1,
                ")" | "]" | "}" if depth == 0 => return Some(names),
                ")" | "]" | "}" => depth -= 1,
                ";" if depth == 0 => return Some(names),
                "," if depth == 0 => break,
                _ if token.kind == TokenKind::Eof => return Some(names),
                _ => {}
            }
            index += 1;
        }
        index += 1;
    }
}
