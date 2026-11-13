// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Keep authored grouping when the canonical annotation has the same tokens.
use super::*;
use crate::parser::Parameter;
use crate::syntax::Token;

fn annotation(module: &Module, parameter: &Parameter) -> Option<Vec<Token>> {
    if parameter.annotation.is_none() || parameter.span.module != module.id {
        return None;
    }
    let source = module
        .source
        .get(parameter.span.start..parameter.span.end)?;
    let tokens = crate::syntax::lex(&module.id, source).ok()?;
    let mut depth = 0usize;
    let mut begin = None;
    let mut end = tokens.len();
    for (index, token) in tokens.iter().enumerate() {
        match token.text.as_str() {
            ":" if depth == 0 && begin.is_none() => begin = Some(index + 1),
            "=" if depth == 0 && begin.is_some() => {
                end = index;
                break;
            }
            "(" | "[" | "{" | "<" => depth += 1,
            ")" | "]" | "}" | ">" => depth = depth.saturating_sub(1),
            ">>" => depth = depth.saturating_sub(2),
            ">>>" => depth = depth.saturating_sub(3),
            _ => {}
        }
    }
    Some(tokens.get(begin?..end)?.to_vec())
}

fn grouping(tokens: &[Token]) -> (Vec<&str>, Vec<usize>, Vec<usize>) {
    let mut plain = Vec::new();
    let mut opens = vec![0];
    let mut closes = vec![0];
    for token in tokens {
        if token.is("(") {
            *opens.last_mut().unwrap() += 1;
        } else if token.is(")") {
            *closes.last_mut().unwrap() += 1;
        } else {
            plain.push(token.text.as_str());
            opens.push(0);
            closes.push(0);
        }
    }
    (plain, opens, closes)
}

pub(super) fn parameter(module: &Module, parameter: &Parameter, rendered: String) -> String {
    let Some(authored) = annotation(module, parameter) else {
        return rendered;
    };
    let Ok(tokens) = crate::syntax::lex(&module.id, &rendered) else {
        return rendered;
    };
    let (source, opens, closes) = grouping(&authored);
    let (printed, printed_opens, printed_closes) = grouping(&tokens);
    if source != printed {
        return rendered;
    }
    let plain = tokens
        .iter()
        .filter(|t| !t.is("(") && !t.is(")"))
        .collect::<Vec<_>>();
    let mut insertions = BTreeMap::<usize, String>::new();
    for boundary in 0..=plain.len() {
        let start = boundary.checked_sub(1).map_or(0, |i| plain[i].end);
        let end = plain.get(boundary).map_or(rendered.len(), |t| t.start);
        let close = closes[boundary].saturating_sub(printed_closes[boundary]);
        let open = opens[boundary].saturating_sub(printed_opens[boundary]);
        if close > 0 {
            insertions
                .entry(start)
                .or_default()
                .push_str(&")".repeat(close));
        }
        if open > 0 {
            let at = tokens
                .iter()
                .find(|t| start <= t.start && t.start < end && t.is("("))
                .map_or(end, |t| t.start);
            insertions
                .entry(at)
                .or_default()
                .push_str(&"(".repeat(open));
        }
    }
    let mut result = String::new();
    let mut cursor = 0;
    for (at, extra) in insertions {
        result.push_str(&rendered[cursor..at]);
        result.push_str(&extra);
        cursor = at;
    }
    result.push_str(&rendered[cursor..]);
    result
}
