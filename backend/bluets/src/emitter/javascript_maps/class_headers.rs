// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Retained static member syntax keeps its original header and operand phases.
use super::*;

fn phases(module: &Module, options: &CompilerOptions) -> Vec<(usize, usize, usize)> {
    if options.target < crate::EcmaTarget::Es2015 {
        return Vec::new();
    }
    let mut result = Vec::new();
    for class in module.classes() {
        for member in &class.members {
            if !member.decorators.is_empty() {
                continue;
            }
            let tokens = &class.body[member.token_start..member.token_end];
            if member.accessor.is_some() && !decorators::lowers_class(class, options) {
                let Some(key) = member.key.first() else {
                    continue;
                };
                let header = &tokens[..tokens.partition_point(|token| token.start < key.start)];
                if let (Some(modifier), Some(keyword)) = (
                    header.iter().find(|token| token.is("static")),
                    header
                        .iter()
                        .find(|token| token.is("get") || token.is("set")),
                ) {
                    result.push((keyword.start, keyword.end, modifier.end));
                }
            } else if member.static_block.is_some() && options.target >= crate::EcmaTarget::Es2022 {
                if let (Some(modifier), Some(open)) = (
                    tokens.first().filter(|token| token.is("static")),
                    tokens.get(1).filter(|token| token.is("{")),
                ) {
                    result.push((modifier.start, modifier.end, modifier.start));
                    result.push((open.start, open.end, modifier.start));
                }
            }
        }
    }
    result
}

pub(super) fn anchors(
    module: &Module,
    options: &CompilerOptions,
    anchors: &mut BTreeMap<usize, Option<usize>>,
) {
    for (start, _, origin) in phases(module, options) {
        anchors.insert(start, Some(origin));
    }
    if options.target >= crate::EcmaTarget::Es2015 {
        for expression in module.class_expressions.values().filter(|expression| {
            expression.statement && !decorators::lowers_class(&expression.class, options)
        }) {
            let class = &expression.class;
            anchors.insert(class.body_span.end - 1, None);
            for member in &class.members {
                let Some(method) = member
                    .method
                    .as_ref()
                    .filter(|method| method.body.is_some())
                else {
                    continue;
                };
                let tokens = &class.body[member.token_start..member.token_end];
                let after_generics = method
                    .type_parameters
                    .last()
                    .map_or(method.name_span.end, |span| span.span.end);
                let Some(open) = tokens
                    .iter()
                    .position(|token| token.start >= after_generics && token.is("("))
                else {
                    continue;
                };
                let Some(close) = super::closing_parenthesis(tokens, open) else {
                    continue;
                };
                let after_annotation = method
                    .return_type_span
                    .as_ref()
                    .map_or(tokens[close].end, |span| span.end);
                if let Some(body) = tokens
                    .iter()
                    .find(|token| token.start >= after_annotation && token.is("{"))
                {
                    anchors.insert(
                        body.start,
                        Some(if close == open + 1 {
                            tokens[open].start
                        } else {
                            tokens[close].start
                        }),
                    );
                }
            }
        }
    }
    // A prefix update starts an original static-super operand. Its preceding
    // comma belongs to the earlier array element and cannot own this operator.
    if options.target >= crate::EcmaTarget::Es2022 {
        for class in module
            .classes()
            .filter(|class| !decorators::lowers_class(class, options))
        {
            for member in &class.members {
                let tokens =
                    if let Some(field) = member.field.as_ref().filter(|field| field.is_static) {
                        field.initializer.as_deref().unwrap_or_default()
                    } else if member.static_block.is_some() {
                        &class.body[member.token_start..member.token_end]
                    } else {
                        continue;
                    };
                for (index, pair) in tokens.windows(2).enumerate() {
                    if matches!(pair[0].text.as_str(), "++" | "--")
                        && pair[1].is("super")
                        && (index == 0
                            || matches!(
                                tokens[index - 1].text.as_str(),
                                "(" | "[" | "," | "=" | "{" | ";" | "return" | "throw" | "=>"
                            ))
                    {
                        anchors.insert(pair[0].start, Some(pair[0].start));
                    }
                }
            }
        }
    }
}

pub(super) fn end_anchors(
    module: &Module,
    options: &CompilerOptions,
    anchors: &mut BTreeMap<usize, usize>,
) {
    for (_, end, origin) in phases(module, options) {
        anchors.insert(end, origin);
    }
}
