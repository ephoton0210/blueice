// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Structural postfix boundaries; assertion erasure never edits checker tokens.
use super::*;

pub(super) struct Call<'a> {
    pub callee: &'a [Token],
    pub arguments: &'a [Token],
    pub optional: bool,
    pub explicit: Option<usize>,
}

pub(super) fn call(tokens: &[Token]) -> Option<Call<'_>> {
    let tokens = strip_outer_parentheses(tokens);
    if !tokens.last()?.is(")") {
        return None;
    }
    let mut stack = Vec::new();
    let mut open = None;
    for (index, token) in tokens.iter().enumerate() {
        match token.text.as_str() {
            "(" | "[" | "{" => stack.push(index),
            ")" | "]" | "}" => {
                let start = stack.pop()?;
                if index + 1 == tokens.len() && token.is(")") {
                    open = Some(start);
                }
            }
            _ => {}
        }
    }
    let open = open.filter(|open| *open > 0)?;
    let mut end = open;
    let mut explicit = None;
    if tokens[end - 1].is(">") {
        let angle = (1..end).find(|i| {
            tokens[*i].is("<") && matching_call_angle_bracket(tokens, *i) == Some(end - 1)
        })?;
        let name = if tokens[angle - 1].is("?.") {
            angle.checked_sub(2)?
        } else {
            angle - 1
        };
        explicit = Some(tokens[name].start);
        end = angle;
    }
    let optional = tokens.get(end - 1).is_some_and(|t| t.is("?."));
    end -= usize::from(optional);
    if end == 0 {
        return None;
    }
    let arguments = &tokens[open + 1..];
    split_call_arguments(arguments)?;
    Some(Call {
        callee: &tokens[..end],
        arguments,
        optional,
        explicit,
    })
}

pub(super) fn property(tokens: &[Token]) -> Option<usize> {
    if tokens.last()?.kind != TokenKind::Identifier {
        return None;
    }
    let index = tokens.len().checked_sub(2)?;
    if !matches!(tokens[index].text.as_str(), "." | "?.") {
        return None;
    }
    let mut depth = 0usize;
    for token in &tokens[..index] {
        match token.text.as_str() {
            "(" | "[" | "{" => depth += 1,
            ")" | "]" | "}" => depth = depth.checked_sub(1)?,
            _ => {}
        }
    }
    (depth == 0 && index > 0).then_some(index)
}
