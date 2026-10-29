// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Syntactic inference follows literal containers without consulting values.
use super::*;

pub(super) fn simple(tokens: &[Token]) -> bool {
    matches!(tokens,[token] if matches!(token.kind,TokenKind::Number|TokenKind::String) || matches!(token.text.as_str(),"true"|"false"|"null"))
        || matches!(tokens,[sign,number] if (sign.is("-")||sign.is("+")) && number.kind==TokenKind::Number)
}

pub(super) fn failure(module: &Module, tokens: &[Token]) -> Option<(u32, SourceSpan)> {
    at(module, tokens, false, 0)
}

pub(super) fn failures(module: &Module, tokens: &[Token]) -> Vec<(u32, SourceSpan)> {
    collect(module, tokens, false, 0)
}

fn collect(
    module: &Module,
    tokens: &[Token],
    constant: bool,
    depth: usize,
) -> Vec<(u32, SourceSpan)> {
    if depth <= 128
        && tokens.first().is_some_and(|token| token.is("{"))
        && tokens.last().is_some_and(|token| token.is("}"))
    {
        let mut errors = Vec::new();
        for member in parts(&tokens[1..tokens.len() - 1]) {
            if member.is_empty() {
                continue;
            }
            if member[0].is("...") {
                errors.push((9015, span(module, member)));
            } else if member.len() == 1 {
                errors.push((9016, span(module, member)));
            }
        }
        // These members prevent syntactic object inference as a whole. The
        // declaration serializer still reports each offending member.
        if !errors.is_empty() {
            return errors;
        }
        for member in parts(&tokens[1..tokens.len() - 1]) {
            if let Some(colon) = top(member, ":") {
                if !member
                    .iter()
                    .any(|token| module.nested_functions.contains_key(&token.start))
                {
                    errors.extend(collect(module, &member[colon + 1..], constant, depth + 1));
                }
            }
        }
        if !errors.is_empty() {
            return errors;
        }
    }
    at(module, tokens, constant, depth).into_iter().collect()
}
fn at(
    module: &Module,
    tokens: &[Token],
    constant: bool,
    depth: usize,
) -> Option<(u32, SourceSpan)> {
    if depth > 128 {
        return Some((9013, span(module, tokens)));
    }
    let mut tokens = tokens;
    while tokens.first().is_some_and(|t| t.is("(")) && tokens.last().is_some_and(|t| t.is(")")) {
        let mut depth = 0usize;
        let mut closes = false;
        for (index, token) in tokens.iter().enumerate() {
            if token.is("(") {
                depth += 1;
            } else if token.is(")") {
                depth -= 1;
                if depth == 0 {
                    closes = index + 1 == tokens.len();
                    break;
                }
            }
        }
        if !closes {
            break;
        }
        tokens = &tokens[1..tokens.len() - 1];
    }
    if simple(tokens) {
        return None;
    }
    let mut constant = constant;
    if tokens.len() > 2 && tokens[tokens.len() - 2].is("as") && tokens[tokens.len() - 1].is("const")
    {
        tokens = &tokens[..tokens.len() - 2];
        constant = true;
    } else if top(tokens, "as").is_some() {
        return None;
    }
    if simple(tokens) {
        return None;
    }
    let first = tokens.first()?;
    if first.is("class") {
        return Some((9022, first.span(&module.id)));
    }
    if first.is("[") && tokens.last().is_some_and(|t| t.is("]")) {
        if !constant {
            return Some((9017, span(module, tokens)));
        }
        for value in parts(&tokens[1..tokens.len() - 1]) {
            if let Some(error) = at(module, value, true, depth + 1) {
                return Some(error);
            }
        }
        return None;
    }
    if first.is("{") && tokens.last().is_some_and(|t| t.is("}")) {
        for member in parts(&tokens[1..tokens.len() - 1]) {
            if member.is_empty() {
                continue;
            }
            if member[0].is("...") {
                return Some((9015, span(module, member)));
            }
            if member.len() == 1 {
                return Some((9016, span(module, member)));
            }
            if let Some(function) = member.iter().find_map(|token| {
                module
                    .nested_functions
                    .get(&token.start)
                    .filter(|function| {
                        function.span.start >= member[0].start
                            && function.span.end <= member.last().unwrap().end
                    })
            }) {
                if function.return_type.is_some()
                    && function.parameters.iter().all(|parameter| {
                        parameter.annotation.is_some()
                            || parameter.default.as_deref().is_some_and(simple)
                    })
                {
                    continue;
                }
            }
            if let Some(colon) = top(member, ":") {
                if let Some(error) = at(module, &member[colon + 1..], constant, depth + 1) {
                    return Some(error);
                }
            } else {
                return Some((9013, span(module, member)));
            }
        }
        return None;
    }
    Some((9013, span(module, tokens)))
}
fn top(tokens: &[Token], separator: &str) -> Option<usize> {
    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate() {
        if depth == 0 && token.is(separator) {
            return Some(index);
        }
        if matches!(token.text.as_str(), "(" | "[" | "{") {
            depth += 1;
        } else if matches!(token.text.as_str(), ")" | "]" | "}") {
            depth = depth.saturating_sub(1);
        }
    }
    None
}
fn parts(mut tokens: &[Token]) -> Vec<&[Token]> {
    let mut parts = Vec::new();
    while !tokens.is_empty() {
        let end = top(tokens, ",").unwrap_or(tokens.len());
        parts.push(&tokens[..end]);
        tokens = tokens.get(end + 1..).unwrap_or(&[]);
    }
    parts
}
