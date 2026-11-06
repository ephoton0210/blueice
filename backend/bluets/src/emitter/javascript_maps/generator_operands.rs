// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! An original yield-star array owns its opener independently of prior trivia.
use super::*;
use crate::parser::{FunctionBodyItem, FunctionDeclaration, FunctionElseBranch};

pub(super) fn anchors(
    module: &Module,
    tokens: &[crate::Token],
    anchors: &mut BTreeMap<usize, Option<usize>>,
) {
    let scopes = scopes(module);
    for (index, window) in tokens.windows(3).enumerate() {
        if !window[0].is("yield") || !window[1].is("*") || !window[2].is("[") {
            continue;
        }
        if generator(&scopes, window[0].start) && dense_array(tokens, index + 2) {
            anchors.insert(window[2].start, Some(window[2].start));
        }
    }
}

pub(super) fn end_anchors(
    module: &Module,
    options: &CompilerOptions,
    tokens: &[crate::Token],
    anchors: &mut BTreeMap<usize, usize>,
) {
    if options.target >= crate::EcmaTarget::Es2015 {
        return;
    }
    let scopes = scopes(module);
    for (index, token) in tokens.iter().enumerate() {
        if !token.is("switch") || !generator(&scopes, token.start) {
            continue;
        }
        let Some(close) = matching(tokens, index + 1) else {
            continue;
        };
        if !tokens.get(close + 1).is_some_and(|token| token.is("{")) {
            continue;
        }
        let Some(end) = matching(tokens, close + 1) else {
            continue;
        };
        let mut cursor = close + 2;
        while cursor < end {
            if tokens[cursor].is("case") || tokens[cursor].is("default") {
                cursor += 1;
                let mut conditional = 0usize;
                while cursor < end {
                    match tokens[cursor].text.as_str() {
                        "?" => conditional += 1,
                        ":" if conditional == 0 => {
                            anchors.insert(tokens[cursor].end, tokens[cursor].end);
                            break;
                        }
                        ":" => conditional -= 1,
                        "(" | "[" | "{" => {
                            cursor = matching(tokens, cursor).unwrap_or(end);
                        }
                        _ => {}
                    }
                    cursor += 1;
                }
            } else if matches!(tokens[cursor].text.as_str(), "(" | "[" | "{") {
                cursor = matching(tokens, cursor).unwrap_or(end);
            }
            cursor += 1;
        }
    }
}

fn scopes(module: &Module) -> Vec<(usize, usize, bool)> {
    let mut scopes = Vec::new();
    for (declaration, _) in runtime_declarations(&module.declarations) {
        if let Declaration::Function(function) = declaration {
            functions(function, &mut scopes);
        }
    }
    for function in module.nested_functions.values() {
        scopes.push((function.span.start, function.span.end, function.generator));
    }
    scopes
}

fn generator(scopes: &[(usize, usize, bool)], at: usize) -> bool {
    scopes
        .iter()
        .filter(|(start, end, _)| *start <= at && at < *end)
        .min_by_key(|(start, end, _)| end - start)
        .is_some_and(|(_, _, generator)| *generator)
}

fn functions(function: &FunctionDeclaration, scopes: &mut Vec<(usize, usize, bool)>) {
    if let Some(body) = function.body_open {
        scopes.push((body, function.span.end, function.generator));
    }
    items(&function.body, scopes);
}

fn items(body: &[FunctionBodyItem], scopes: &mut Vec<(usize, usize, bool)>) {
    for item in body {
        match item {
            FunctionBodyItem::Function(function) => functions(function, scopes),
            FunctionBodyItem::Try(statement) => {
                items(&statement.block, scopes);
                if let Some(handler) = &statement.handler {
                    items(&handler.body, scopes);
                }
                if let Some(finalizer) = &statement.finalizer {
                    items(finalizer, scopes);
                }
            }
            FunctionBodyItem::If(statement) => {
                items(&statement.consequent, scopes);
                let mut alternate = statement.alternate.as_ref();
                while let Some(branch) = alternate {
                    match branch {
                        FunctionElseBranch::Braced(body) => {
                            items(body, scopes);
                            break;
                        }
                        FunctionElseBranch::ElseIf(statement) => {
                            items(&statement.consequent, scopes);
                            alternate = statement.alternate.as_ref();
                        }
                    }
                }
            }
            FunctionBodyItem::While(statement) => items(&statement.body, scopes),
            _ => {}
        }
    }
}

fn matching(tokens: &[crate::Token], begin: usize) -> Option<usize> {
    let mut closes = Vec::new();
    for (index, token) in tokens.iter().enumerate().skip(begin) {
        match token.text.as_str() {
            "(" => closes.push(")"),
            "[" => closes.push("]"),
            "{" => closes.push("}"),
            ")" | "]" | "}" => {
                if closes.pop() != Some(token.text.as_str()) {
                    return None;
                }
                if closes.is_empty() {
                    return Some(index);
                }
            }
            _ if closes.is_empty() => return None,
            _ => {}
        }
    }
    None
}

fn dense_array(tokens: &[crate::Token], begin: usize) -> bool {
    let mut closes = vec!["]"];
    let mut expects_element = true;
    for token in &tokens[begin + 1..] {
        if closes.len() == 1 {
            if token.is(",") {
                if expects_element {
                    return false;
                }
                expects_element = true;
                continue;
            }
            if token.is("...") {
                return false;
            }
            if !token.is("]") {
                expects_element = false;
            }
        }
        match token.text.as_str() {
            "(" => closes.push(")"),
            "[" => closes.push("]"),
            "{" => closes.push("}"),
            ")" | "]" | "}" => {
                if closes.pop() != Some(token.text.as_str()) {
                    return false;
                }
                if closes.is_empty() {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}
