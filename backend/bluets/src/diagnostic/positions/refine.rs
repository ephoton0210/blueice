// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Diagnostic causes select a syntax node; source identities never select rules.
use crate::diagnostic::{Diagnostic, SourceSpan};
use crate::syntax::{Token, TokenKind};

pub(super) fn refine(diagnostic: &Diagnostic, tokens: &[Token]) -> SourceSpan {
    let counterpart = diagnostic.typescript.as_ref().unwrap();
    let span = &counterpart.span;
    let begin = tokens.partition_point(|token| token.end <= span.start);
    let end = tokens.partition_point(|token| token.start < span.end);
    let selected = tokens.get(begin..end).unwrap_or(&[]);
    let code = counterpart.code;
    let chosen = match code {
        2322 | 2741 | 2375 | 2820 => assignment(selected, tokens, begin, diagnostic),
        2345 | 2769 | 2554 | 2555 => call_argument(selected, &diagnostic.message, code),
        2417 | 2415 => class_name(tokens, begin),
        2416 | 2378 | 1054 | 1095 | 1049 | 2390 | 2391 | 2392 | 2393 | 2394 | 2385 | 2377
        | 7030 => declaration_name(selected),
        2300 | 2451 | 2567 | 2434 | 2808 => {
            duplicate_name(tokens, selected, counterpart.arguments.first(), false)
        }
        2717 => duplicate_name(tokens, selected, counterpart.arguments.first(), true),
        7022 | 7023 => inferred_name(tokens, selected, begin, &diagnostic.message),
        2540 | 2339 | 2576 | 2550 | 2551 | 2341 | 2445 | 2446 | 18013 => {
            property_name(tokens, begin, selected, counterpart.arguments.first())
        }
        2792 => selected
            .iter()
            .find(|token| token.kind == TokenKind::String)
            .map(|token| (token.start, token.end)),
        2408 | 18041 => find(selected, "return"),
        2335 | 2337 => find(selected, "super"),
        17009 => find(selected, "this"),
        1051 => find(selected, "?"),
        1029 => selected
            .iter()
            .find(|token| token.is("public") || token.is("private") || token.is("protected"))
            .map(|token| (token.start, token.end)),
        18030 => private_name(selected),
        18037 => await_expression(tokens, begin, selected),
        2366 | 2355 => result_annotation(selected),
        1187 | 2369 | 1317 | 7006 => parameter(tokens, begin, selected),
        1206 | 1249 => find(selected, "@"),
        1207 => second_decorator(tokens, begin),
        1238 | 1240 | 1239 | 1270 | 1271 | 1329 => decorator(selected, code, &diagnostic.message),
        18033 | 2474 => initializer(selected),
        2476 | 7015 => index_argument(tokens, begin, selected),
        2305 | 2459 | 2724 | 2613 | 1192 => counterpart
            .arguments
            .iter()
            .rev()
            .find_map(|name| find(selected, name)),
        2694 | 2708 | 1361 => qualified_name(selected, code, &counterpart.arguments),
        7053 => indexed_expression(tokens, begin),
        7009 => new_expression(tokens, begin),
        6133 => declaration_name(selected),
        7029 => case_label(selected, tokens, begin),
        _ => None,
    };
    if let Some((start, end)) = chosen {
        SourceSpan::new(&span.module, start, end)
    } else {
        span.clone()
    }
}

fn find(tokens: &[Token], name: &str) -> Option<(usize, usize)> {
    tokens
        .iter()
        .find(|token| token.is(name))
        .map(|token| (token.start, token.end))
}

fn assignment(
    selected: &[Token],
    tokens: &[Token],
    begin: usize,
    diagnostic: &Diagnostic,
) -> Option<(usize, usize)> {
    let code = diagnostic.typescript.as_ref()?.code;
    if diagnostic.message.starts_with("catch variable of type") {
        let binding = tokens[..begin]
            .iter()
            .rposition(|token| token.is("const") || token.is("let") || token.is("var"))?;
        let token = tokens.get(binding + 1)?;
        return Some((token.start, token.end));
    }
    if diagnostic.message.starts_with("default initializer") {
        let end = tokens[begin..]
            .iter()
            .position(|token| token.is(",") || token.is(")"))?
            + begin;
        return Some((tokens[begin].start, tokens[end.checked_sub(1)?].end));
    }
    if selected.first().is_some_and(|token| token.is("return")) {
        if code == 2741 && selected.get(1).is_some_and(|token| token.is("this")) {
            return find(selected, "this");
        }
        if let Some(span) = tuple_element(selected, diagnostic) {
            return Some(span);
        }
        return find(selected, "return");
    }
    if let Some(index) = selected
        .iter()
        .position(|token| matches!(token.text.as_str(), "const" | "let" | "var"))
    {
        let name = selected.get(index + 1)?;
        if code == 2741 {
            if let Some(equal) = selected.iter().position(|token| token.is("=")) {
                let right = selected.get(equal + 1)?;
                if right.kind == TokenKind::Identifier
                    && selected[index + 2..equal]
                        .iter()
                        .any(|token| token.is(&right.text))
                {
                    return Some((right.start, right.end));
                }
            }
        }
        return Some((name.start, name.end));
    }
    if selected
        .first()
        .is_some_and(|token| token.kind == TokenKind::Identifier)
        && selected.get(1).is_some_and(|token| token.is(":"))
    {
        return declaration_name(selected);
    }
    if selected.iter().any(|token| token.is("function")) {
        return find(selected, "return");
    }
    let equal = selected.iter().position(|token| token.is("="))?;
    Some((
        selected.first()?.start,
        selected.get(equal.checked_sub(1)?)?.end,
    ))
}

fn declaration_name(tokens: &[Token]) -> Option<(usize, usize)> {
    let first = tokens.iter().position(|token| {
        let accessor = (token.is("get") || token.is("set"))
            && tokens
                .iter()
                .position(|candidate| candidate.start == token.start)
                .is_some_and(|index| {
                    tokens
                        .get(index + 1)
                        .is_some_and(|next| next.kind == TokenKind::Identifier)
                });
        (token.kind == TokenKind::Identifier && !accessor) || token.is("constructor")
    })?;
    let token = &tokens[first];
    Some((token.start, token.end))
}

fn class_name(tokens: &[Token], begin: usize) -> Option<(usize, usize)> {
    let class = tokens[..=begin.min(tokens.len().checked_sub(1)?)]
        .iter()
        .rposition(|token| token.is("class"))?;
    let token = tokens.get(class + 1)?;
    Some((token.start, token.end))
}

fn property_name(
    all: &[Token],
    begin: usize,
    tokens: &[Token],
    argument: Option<&String>,
) -> Option<(usize, usize)> {
    if let Some(span) = private_name(tokens) {
        return Some(span);
    }
    if let Some(name) = argument {
        if let Some(found) = find(tokens, name) {
            return Some(found);
        }
    }
    let assignment = tokens
        .iter()
        .position(|token| token.is("="))
        .unwrap_or(tokens.len());
    let dotted = tokens[..assignment]
        .windows(2)
        .rfind(|pair| pair[0].is(".") || pair[0].is("?."))
        .map(|pair| (pair[1].start, pair[1].end));
    dotted
        .or_else(|| index_argument(all, begin, tokens))
        .or_else(|| {
            all.get(begin).filter(|token| token.is("]")).and_then(|_| {
                let open = all[..begin].iter().rposition(|token| token.is("["))?;
                Some((
                    all.get(open + 1)?.start,
                    all.get(begin.checked_sub(1)?)?.end,
                ))
            })
        })
}

fn private_name(tokens: &[Token]) -> Option<(usize, usize)> {
    let index = tokens.iter().position(|token| token.is("#"))?;
    Some((tokens[index].start, tokens.get(index + 1)?.end))
}

fn decorator(tokens: &[Token], code: u32, message: &str) -> Option<(usize, usize)> {
    let at = tokens.iter().position(|token| token.is("@"))?;
    let last = tokens.last()?;
    let include_at = code == 1329
        || message.starts_with("a decorator is called")
        || (message.starts_with("unable to resolve")
            && message.ends_with("(it must accept 1 argument(s))"));
    Some((
        if include_at {
            tokens[at].start
        } else {
            tokens.get(at + 1)?.start
        },
        last.end,
    ))
}

fn initializer(tokens: &[Token]) -> Option<(usize, usize)> {
    let equal = tokens.iter().position(|token| token.is("="))?;
    Some((tokens.get(equal + 1)?.start, tokens.last()?.end))
}

fn qualified_name(tokens: &[Token], code: u32, arguments: &[String]) -> Option<(usize, usize)> {
    if code == 2694 {
        if let Some(name) = arguments.last() {
            if let Some(found) = find(tokens, name.rsplit('.').next().unwrap_or(name)) {
                return Some(found);
            }
        }
        return tokens
            .windows(2)
            .find(|pair| pair[0].is("."))
            .map(|pair| (pair[1].start, pair[1].end));
    }
    if let Some(name) = arguments.first() {
        if let Some(found) = find(tokens, name) {
            return Some(found);
        }
    }
    let equal = tokens.iter().position(|token| token.is("="))?;
    let token = tokens.get(equal + 1)?;
    Some((token.start, token.end))
}

fn result_annotation(tokens: &[Token]) -> Option<(usize, usize)> {
    let close = tokens.iter().position(|token| token.is(")"))?;
    if !tokens.get(close + 1)?.is(":") {
        return None;
    }
    let end = tokens[close + 2..].iter().position(|token| token.is("{"))? + close + 2;
    Some((
        tokens.get(close + 2)?.start,
        tokens.get(end.checked_sub(1)?)?.end,
    ))
}

fn parameter(tokens: &[Token], begin: usize, selected: &[Token]) -> Option<(usize, usize)> {
    let mut start = begin;
    while start > 0
        && matches!(
            tokens[start - 1].text.as_str(),
            "public" | "private" | "protected" | "readonly"
        )
    {
        start -= 1;
    }
    Some((tokens.get(start)?.start, selected.last()?.end))
}

fn case_label(selected: &[Token], tokens: &[Token], begin: usize) -> Option<(usize, usize)> {
    if !selected.first()?.is("case") {
        return None;
    }
    let colon = tokens[begin..].iter().position(|token| token.is(":"))? + begin;
    Some((tokens[begin].start, tokens[colon].end))
}

fn index_argument(tokens: &[Token], begin: usize, selected: &[Token]) -> Option<(usize, usize)> {
    let open = selected
        .iter()
        .position(|token| token.is("["))
        .map(|index| begin + index)
        .or_else(|| {
            tokens
                .get(begin + 1)
                .filter(|token| token.is("["))
                .map(|_| begin + 1)
        })?;
    let close = matching(tokens, open)?;
    Some((
        tokens.get(open + 1)?.start,
        tokens.get(close.checked_sub(1)?)?.end,
    ))
}

fn matching(tokens: &[Token], start: usize) -> Option<usize> {
    let close = match tokens.get(start)?.text.as_str() {
        "(" => ")",
        "[" => "]",
        "{" => "}",
        _ => return None,
    };
    let mut stack = vec![close];
    for (index, token) in tokens.iter().enumerate().skip(start + 1) {
        match token.text.as_str() {
            "(" => stack.push(")"),
            "[" => stack.push("]"),
            "{" => stack.push("}"),
            value if stack.last() == Some(&value) => {
                stack.pop();
                if stack.is_empty() {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}

fn call_argument(tokens: &[Token], message: &str, code: u32) -> Option<(usize, usize)> {
    if code == 2554 && tokens.first().is_some_and(|token| token.is("super")) {
        return tokens
            .iter()
            .position(|token| token.is("."))
            .and_then(|index| tokens.get(index + 1))
            .or_else(|| tokens.first())
            .map(|token| (token.start, token.end));
    }
    let open = tokens.iter().position(|token| token.is("("))?;
    let close = matching(tokens, open)?;
    let mut arguments = Vec::new();
    let mut start = open + 1;
    let mut index = start;
    while index < close {
        if tokens[index].is(",") {
            arguments.push((start, index));
            start = index + 1;
        } else if matches!(tokens[index].text.as_str(), "(" | "[" | "{") {
            index = matching(tokens, index)?;
        }
        index += 1;
    }
    if start < close {
        arguments.push((start, close));
    }
    let argument_index = message
        .strip_prefix("argument ")
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(1)
        .saturating_sub(1);
    let &(begin, end) = if code == 2554 {
        arguments.first()?
    } else {
        arguments.get(argument_index)?
    };
    Some((tokens[begin].start, tokens[end.checked_sub(1)?].end))
}

fn duplicate_name(
    tokens: &[Token],
    selected: &[Token],
    argument: Option<&String>,
    subsequent: bool,
) -> Option<(usize, usize)> {
    let name = argument.map(String::as_str).or_else(|| {
        selected
            .iter()
            .find(|token| token.kind == TokenKind::Identifier)
            .map(|token| token.text.as_str())
    })?;
    let start = selected.first()?.start;
    let prefix_end = tokens.partition_point(|token| token.start <= start);
    let class = tokens[..prefix_end]
        .iter()
        .rposition(|token| token.is("class"));
    let member_scope =
        class.filter(|index| tokens.get(index + 1).is_some_and(|token| !token.is(name)));
    let from = member_scope.unwrap_or(0);
    let candidates = tokens
        .iter()
        .enumerate()
        .skip(from)
        .filter(|(index, token)| {
            if !token.is(name) {
                return false;
            }
            let before = index.checked_sub(1).and_then(|index| tokens.get(index));
            if before.is_some_and(|token| token.is(".") || token.is("?.")) {
                return false;
            }
            let after = tokens.get(index + 1);
            before.is_some_and(|token| {
                matches!(
                    token.text.as_str(),
                    "class"
                        | "type"
                        | "interface"
                        | "function"
                        | "enum"
                        | "namespace"
                        | "const"
                        | "let"
                        | "var"
                        | "get"
                        | "set"
                )
            }) || after.is_some_and(|token| token.is(":") || token.is("("))
                || before.is_some_and(|token| token.is("{") || token.is(","))
        })
        .collect::<Vec<_>>();
    let candidate = if subsequent {
        candidates.get(1)?
    } else if member_scope.is_some() {
        let parameter = |index: usize| {
            let open = tokens[..index].iter().rposition(|token| token.is("("));
            let close = tokens[..index].iter().rposition(|token| token.is(")"));
            open.is_some_and(|open| close.is_none_or(|close| open > close))
        };
        let fields = candidates
            .iter()
            .filter(|(index, _)| {
                !parameter(*index) && tokens.get(index + 1).is_some_and(|token| token.is(":"))
            })
            .collect::<Vec<_>>();
        let has_method = candidates.iter().any(|(index, _)| {
            tokens.get(index + 1).is_some_and(|token| token.is("("))
                || index
                    .checked_sub(1)
                    .and_then(|index| tokens.get(index))
                    .is_some_and(|token| token.is("get") || token.is("set"))
        });
        if !fields.is_empty() && !has_method {
            candidates
                .iter()
                .find(|(index, _)| parameter(*index))
                .or_else(|| candidates.last())?
        } else {
            candidates.first()?
        }
    } else {
        candidates.first()?
    };
    let (_, token) = candidate;
    Some((token.start, token.end))
}

fn inferred_name(
    tokens: &[Token],
    selected: &[Token],
    begin: usize,
    message: &str,
) -> Option<(usize, usize)> {
    if message.contains("circular import") {
        let equal = selected.iter().position(|token| token.is("="))?;
        let token = selected.get(equal + 1)?;
        return Some((token.start, token.end));
    }
    if selected
        .iter()
        .any(|token| token.is("function") || token.is("const") || token.is("let"))
    {
        return declaration_name(selected);
    }
    let index = tokens[..begin]
        .iter()
        .rposition(|token| token.is("const") || token.is("let") || token.is("var"))?;
    let token = tokens.get(index + 1)?;
    Some((token.start, token.end))
}

fn await_expression(tokens: &[Token], begin: usize, selected: &[Token]) -> Option<(usize, usize)> {
    let await_index = selected.iter().position(|token| token.is("await"))? + begin;
    let end = tokens[await_index..]
        .iter()
        .position(|token| token.is(";") || token.is("}"))?
        + await_index;
    Some((
        tokens[await_index].start,
        tokens.get(end.checked_sub(1)?)?.end,
    ))
}

fn indexed_expression(tokens: &[Token], begin: usize) -> Option<(usize, usize)> {
    let open = tokens[..begin].iter().rposition(|token| token.is("["))?;
    let close = matching(tokens, open)?;
    Some((tokens.get(open.checked_sub(1)?)?.start, tokens[close].end))
}

fn new_expression(tokens: &[Token], begin: usize) -> Option<(usize, usize)> {
    let new = begin
        .checked_sub(1)
        .filter(|index| tokens[*index].is("new"))?;
    let open = tokens[begin..].iter().position(|token| token.is("("))? + begin;
    let close = matching(tokens, open)?;
    Some((tokens[new].start, tokens[close].end))
}

fn second_decorator(tokens: &[Token], begin: usize) -> Option<(usize, usize)> {
    let next = tokens.iter().skip(begin + 1).find(|token| token.is("@"))?;
    Some((next.start, next.end))
}

fn tuple_element(tokens: &[Token], diagnostic: &Diagnostic) -> Option<(usize, usize)> {
    let arguments = &diagnostic.typescript.as_ref()?.arguments;
    let actual = arguments.first()?.strip_prefix('[')?.strip_suffix(']')?;
    let expected = arguments.last()?.strip_prefix('[')?.strip_suffix(']')?;
    let index = actual
        .split(',')
        .zip(expected.split(','))
        .position(|(actual, expected)| actual.trim() != expected.trim())?;
    let open = tokens.iter().position(|token| token.is("["))?;
    let close = matching(tokens, open)?;
    let mut start = open + 1;
    let mut item = 0;
    for end in open + 1..=close {
        if tokens[end].is(",") || end == close {
            if item == index {
                return Some((tokens[start].start, tokens.get(end.checked_sub(1)?)?.end));
            }
            item += 1;
            start = end + 1;
        }
    }
    None
}
