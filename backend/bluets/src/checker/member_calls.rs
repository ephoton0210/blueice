// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Postfix receiver ranges preserve precedence and nested argument boundaries.

use super::{explicit_generic_call_close, split_call_arguments, Token, TokenKind};
use std::ops::Range;

pub(super) struct MemberCall<'a> {
    pub(super) receiver: &'a [Token],
    pub(super) member: &'a Token,
    pub(super) arguments: &'a [Token],
    pub(super) generic: bool,
}

pub(super) struct IndexedCall<'a> {
    pub(super) callee: &'a [Token],
    pub(super) arguments: &'a [Token],
}

pub(super) fn indexed_call_ranges(
    tokens: &[Token],
    is_generic: impl Fn(usize) -> bool,
) -> Vec<Range<usize>> {
    let starts = postfix_starts(tokens, is_generic);
    let mut open = Vec::new();
    let mut ranges = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        if token.is("(") {
            open.push(index);
        } else if token.is(")") {
            if let Some(begin) = open.pop() {
                if begin > 0 && tokens[begin - 1].is("]") {
                    ranges.push(starts[begin - 1]..index + 1);
                }
            }
        }
    }
    ranges
}

pub(super) fn indexed_call_parts(
    tokens: &[Token],
    is_generic: impl Fn(usize) -> bool,
) -> Option<IndexedCall<'_>> {
    indexed_call_ranges(tokens, is_generic)
        .into_iter()
        .find(|range| range.start == 0 && range.end == tokens.len())?;
    let mut depth = 0usize;
    let mut open = None;
    for (index, token) in tokens.iter().enumerate() {
        match token.text.as_str() {
            "(" if depth == 0 && index > 0 && tokens[index - 1].is("]") => open = Some(index),
            _ => {}
        }
        match token.text.as_str() {
            "(" | "[" | "{" => depth += 1,
            ")" | "]" | "}" => depth = depth.checked_sub(1)?,
            _ => {}
        }
    }
    let open = open?;
    let arguments = &tokens[open + 1..];
    split_call_arguments(arguments)?;
    Some(IndexedCall {
        callee: &tokens[..open],
        arguments,
    })
}

fn postfix_starts(tokens: &[Token], is_generic: impl Fn(usize) -> bool) -> Vec<usize> {
    let mut starts = vec![0; tokens.len()];
    let mut parents = Vec::new();
    let mut start = 0;
    let mut index = 0;
    while index < tokens.len() {
        let token = &tokens[index];
        starts[index] = start;
        if token.kind == TokenKind::Identifier && is_generic(token.start) {
            if let Some(close) = explicit_generic_call_close(tokens, index) {
                starts[index..=close].fill(start);
                index = close + 1;
                continue;
            }
        }
        match token.text.as_str() {
            "(" | "[" | "{" => {
                parents.push(start);
                start = index + 1;
            }
            ")" | "]" | "}" => {
                start = parents.pop().unwrap_or(0);
                starts[index] = start;
                // A completed control-flow block/head can be followed by a
                // new statement without a semicolon. A postfix continuation
                // instead begins with a dot, bracket or call delimiter.
                if matches!(token.text.as_str(), ")" | "}")
                    && tokens.get(index + 1).is_some_and(|next| {
                        next.kind == TokenKind::Identifier
                            || matches!(next.text.as_str(), "this" | "super" | "new")
                    })
                {
                    start = index + 1;
                }
            }
            _ if boundary(tokens, index) => start = index + 1,
            _ => {}
        }
        index += 1;
    }
    starts
}

fn boundary(tokens: &[Token], index: usize) -> bool {
    let previous = index.checked_sub(1).and_then(|index| tokens.get(index));
    if previous.is_some_and(|token| token.is(".")) {
        return false; // Keyword-named members, including Generator.return.
    }
    let token = &tokens[index];
    if token.is("!") {
        return !previous.is_some_and(|token| {
            matches!(
                token.kind,
                TokenKind::Identifier | TokenKind::Number | TokenKind::String | TokenKind::Template
            ) || matches!(
                token.text.as_str(),
                ")" | "]" | "}" | "!" | "this" | "super" | "true" | "false" | "null"
            )
        });
    }
    matches!(
        token.text.as_str(),
        "," | ";"
            | ":"
            | "?"
            | "="
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
            | "+="
            | "-="
            | "*="
            | "/="
            | "%="
            | "**="
            | "<<="
            | ">>="
            | ">>>="
            | "&="
            | "|="
            | "^="
            | "&&="
            | "||="
            | "??="
            | "=>"
            | "~"
            | "typeof"
            | "void"
            | "delete"
            | "await"
            | "yield"
            | "return"
            | "throw"
    )
}

pub(super) fn member_call_ranges(
    tokens: &[Token],
    is_generic: impl Fn(usize) -> bool,
) -> Vec<Range<usize>> {
    let starts = postfix_starts(tokens, &is_generic);
    let mut open = Vec::new();
    let mut closes = vec![None; tokens.len()];
    for (index, token) in tokens.iter().enumerate() {
        if token.is("(") {
            open.push(index);
        } else if token.is(")") {
            if let Some(start) = open.pop() {
                closes[start] = Some(index);
            }
        }
    }
    (0..tokens.len().saturating_sub(2))
        .filter_map(|dot| {
            let open = if tokens.get(dot + 2).is_some_and(|token| token.is("(")) {
                Some(dot + 2)
            } else if is_generic(tokens[dot + 1].start) {
                explicit_generic_call_close(tokens, dot + 1).map(|close| close + 1)
            } else {
                None
            };
            (tokens[dot].is(".")
                && matches!(
                    tokens[dot + 1].kind,
                    TokenKind::Identifier | TokenKind::Keyword
                )
                && open.is_some())
            .then(|| closes[open.unwrap()].map(|end| starts[dot]..end + 1))
            .flatten()
        })
        .collect()
}

/// Every ordinary property read, including prefixes whose inferred type may
/// otherwise become `unknown` before a later member is checked.
pub(super) fn property_access_ranges(
    tokens: &[Token],
    is_generic: impl Fn(usize) -> bool,
) -> Vec<Range<usize>> {
    let starts = postfix_starts(tokens, &is_generic);
    let mut ranges = Vec::new();
    let mut index = 0;
    while index < tokens.len() {
        if tokens[index].kind == TokenKind::Identifier && is_generic(tokens[index].start) {
            if let Some(close) = explicit_generic_call_close(tokens, index) {
                index = close + 1;
                continue;
            }
        }
        let end = if tokens[index].is(".")
            && tokens.get(index + 1).is_some_and(|member| {
                matches!(member.kind, TokenKind::Identifier | TokenKind::Keyword)
            }) {
            if is_generic(tokens[index + 1].start) {
                index += 1;
                continue;
            }
            Some(index + 2)
        } else if tokens[index].is("]") {
            Some(index + 1)
        } else {
            None
        };
        if let Some(end) = end {
            let next = tokens.get(end);
            let call_or_write = next.is_some_and(|token| {
                token.is("(")
                    || super::scopes::targets::ASSIGNMENTS.contains(&token.text.as_str())
                    || token.is("++")
                    || token.is("--")
            });
            let prefix_write = end == tokens.len()
                && tokens
                    .first()
                    .is_some_and(|token| matches!(token.text.as_str(), "++" | "--" | "delete"));
            if !call_or_write && !prefix_write {
                let mut start = starts[index];
                if matches!(tokens[start].text.as_str(), "++" | "--") {
                    start += 1;
                }
                if tokens[start].is("new")
                    && !tokens[start..index].iter().any(|token| token.is("("))
                {
                    start += 1;
                }
                if start < end {
                    ranges.push(start..end);
                }
            }
        }
        index += 1;
    }
    ranges
}

pub(super) fn member_call_parts(
    tokens: &[Token],
    is_generic: impl Fn(usize) -> bool,
) -> Option<MemberCall<'_>> {
    let mut depth = 0usize;
    let mut dot = None;
    for (index, token) in tokens.iter().enumerate() {
        match token.text.as_str() {
            "(" | "[" | "{" => depth = depth.checked_add(1)?,
            ")" | "]" | "}" => depth = depth.checked_sub(1)?,
            "." if depth == 0 => dot = Some(index),
            _ => {}
        }
    }
    if depth != 0 {
        return None;
    }
    let dot = dot?;
    if dot == 0 || postfix_starts(tokens, &is_generic)[dot] != 0 {
        return None;
    }
    let member = tokens.get(dot + 1)?;
    let generic = tokens.get(dot + 2)?.is("<");
    let open = if generic && is_generic(member.start) {
        explicit_generic_call_close(tokens, dot + 1)? + 1
    } else {
        dot + 2
    };
    let arguments = tokens.get(open + 1..)?;
    (matches!(member.kind, TokenKind::Identifier | TokenKind::Keyword)
        && tokens[open].is("(")
        && split_call_arguments(arguments).is_some())
    .then_some(MemberCall {
        receiver: &tokens[..dot],
        member,
        arguments,
        generic,
    })
}
