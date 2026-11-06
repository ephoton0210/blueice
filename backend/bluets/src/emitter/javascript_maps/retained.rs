// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Retained callable, object and proposal syntax keeps its original node roles.
use super::*;
use crate::parser::{NestedFunction, NestedFunctionBody, NestedFunctionKind};

fn block_open(tokens: &[crate::Token], function: &NestedFunction) -> Option<usize> {
    if let NestedFunctionBody::Expression(body) = &function.body {
        return body.first().map(|token| token.start);
    }
    let begin = tokens.partition_point(|token| token.start < function.span.start);
    let end = tokens.partition_point(|token| token.end <= function.span.end);
    let mut depth = 0usize;
    for token in tokens[begin..end].iter().rev() {
        if token.is("}") {
            depth += 1;
        } else if token.is("{") {
            depth = depth.checked_sub(1)?;
            if depth == 0 {
                return Some(token.start);
            }
        }
    }
    None
}

fn header<'a>(
    tokens: &'a [crate::Token],
    function: &NestedFunction,
    body: usize,
) -> &'a [crate::Token] {
    &tokens[tokens.partition_point(|token| token.start < function.span.start)
        ..tokens.partition_point(|token| token.start < body)]
}

fn arrow(module: &Module, tokens: &[crate::Token]) -> Option<usize> {
    let last = tokens.last()?;
    if last.is("=>") {
        return Some(last.start);
    }
    // In `Promise<T>=>`, the lossless lexer sees `>=` followed by `>`.
    // The retained arrow AST still identifies the two exact source glyphs.
    (last.is(">") && last.start > 0 && module.source.get(last.start - 1..last.end) == Some("=>"))
        .then_some(last.start - 1)
}

fn retains_arrow(function: &NestedFunction, options: &CompilerOptions) -> bool {
    function.kind == NestedFunctionKind::Arrow
        && (function.async_function || options.target >= crate::EcmaTarget::Es2015)
}

fn member_keys(module: &Module, tokens: &[crate::Token]) -> std::collections::BTreeSet<usize> {
    let mut keys = std::collections::BTreeSet::new();
    for (declaration, _) in runtime_declarations(&module.declarations) {
        if let Declaration::Class(class) = declaration {
            keys.extend(
                class
                    .members
                    .iter()
                    .flat_map(|member| member.key.iter().map(|token| token.start)),
            );
        }
    }
    for expression in module.class_expressions.values() {
        keys.extend(
            expression
                .class
                .members
                .iter()
                .flat_map(|member| member.key.iter().map(|token| token.start)),
        );
    }
    for function in module.nested_functions.values().filter(|function| {
        matches!(
            function.kind,
            NestedFunctionKind::Method | NestedFunctionKind::Getter | NestedFunctionKind::Setter
        )
    }) {
        if let (Some(name), Some(body)) = (&function.name, block_open(tokens, function)) {
            if let Some(key) = header(tokens, function, body)
                .iter()
                .find(|token| token.text == *name)
            {
                keys.insert(key.start);
            }
        }
    }
    keys
}

pub(super) fn end_anchors(
    module: &Module,
    options: &CompilerOptions,
    tokens: &[crate::Token],
    anchors: &mut BTreeMap<usize, usize>,
) {
    if options.target >= crate::EcmaTarget::Es2015 && options.module_kind == crate::ModuleKind::Esm
    {
        for (declaration, nested) in runtime_declarations(&module.declarations) {
            let Declaration::Class(class) = declaration else {
                continue;
            };
            if nested
                || !class.default_export
                || super::super::decorators::lowers_class(class, options)
            {
                continue;
            }
            let header = &tokens[tokens.partition_point(|token| token.start < class.span.start)
                ..tokens.partition_point(|token| token.start < class.name_span.start)];
            for modifier in header
                .iter()
                .filter(|token| token.is("export") || token.is("default"))
            {
                anchors.insert(modifier.end, modifier.end);
            }
        }
    }
    if options.target >= crate::EcmaTarget::Es2015 {
        for (declaration, _) in runtime_declarations(&module.declarations) {
            if let Declaration::Class(class) = declaration {
                let static_initializer = class.members.iter().any(|member| {
                    member.field.as_ref().is_some_and(|field| {
                        field.is_static && !field.declared && field.initializer.is_some()
                    })
                });
                if !static_initializer && !super::super::decorators::lowers_class(class, options) {
                    anchors.insert(class.body_span.end, class.span.end);
                }
            }
        }
    }
    let member_keys = member_keys(module, tokens);
    for (index, token) in tokens.iter().enumerate().skip(1) {
        let previous = &tokens[index - 1];
        if token.is("instanceof")
            && !previous.is(".")
            && !member_keys.contains(&token.start)
            && !tokens.get(index + 1).is_some_and(|next| next.is(":"))
        {
            anchors.insert(token.end, previous.end);
        }
    }
    if options.target >= crate::EcmaTarget::Es2015 {
        let classes = runtime_declarations(&module.declarations)
            .into_iter()
            .filter(|_| {
                options.target == crate::EcmaTarget::EsNext && options.defines_class_fields()
            })
            .filter_map(|(declaration, _)| {
                if let Declaration::Class(class) = declaration {
                    Some((class, false))
                } else {
                    None
                }
            })
            .chain(
                module
                    .class_expressions
                    .values()
                    .filter(|expression| {
                        !super::super::decorators::lowers_class(&expression.class, options)
                    })
                    .map(|expression| (&expression.class, expression.name.is_none())),
            );
        for (class, anonymous) in classes {
            let Some(base) = &class.extends_span else {
                continue;
            };
            let keyword_start =
                crate::emitter::decorators::class_keyword_start(class, &module.source);
            let begin = tokens.partition_point(|token| token.start < keyword_start);
            let end = tokens.partition_point(|token| token.start < class.body_span.start);
            let head = &tokens[begin..end];
            if let (Some(keyword), Some(heritage)) = (
                head.iter().find(|token| token.is("class")),
                head.iter()
                    .rfind(|token| token.is("extends") && token.start < base.start),
            ) {
                anchors.insert(
                    if anonymous {
                        keyword.end
                    } else {
                        class.name_span.end
                    },
                    heritage.start,
                );
                anchors.insert(heritage.end, heritage.start);
            }
        }
    }
    if options.target >= crate::EcmaTarget::Es2015 {
        for expression in module.class_expressions.values() {
            if !super::super::decorators::lowers_class(&expression.class, options) {
                if let Some(heritage) = &expression.class.extends_span {
                    anchors.insert(heritage.end, heritage.end);
                }
            }
        }
    }
    for expression in module.class_expressions.values() {
        if super::super::decorators::lowers_class(&expression.class, options)
            && super::super::decorators::expressions::unmapped_continuation(
                module,
                &expression.class,
                options,
            )
        {
            if let Some(next) = tokens
                .iter()
                .find(|token| token.start >= expression.class.span.end)
            {
                anchors.insert(next.end, next.end);
            }
        }
    }
    for function in module.nested_functions.values() {
        if function.kind == NestedFunctionKind::Function
            && function.name.is_none()
            && !function.async_function
            && !function.generator
        {
            if let Some(body) = block_open(tokens, function) {
                let head = header(tokens, function, body);
                if let Some(open) = head.iter().position(|token| token.is("(")) {
                    let before = tokens.partition_point(|token| token.start < head[open].start);
                    if let Some(keyword) = before
                        .checked_sub(1)
                        .and_then(|index| tokens.get(index))
                        .filter(|token| token.is("function"))
                    {
                        let first = function.parameters.iter().find(|parameter| {
                            parameter.name != "this"
                                && !(options.target == crate::EcmaTarget::Es5 && parameter.rest)
                        });
                        anchors.insert(
                            head[open].end,
                            first.map_or(keyword.start, |parameter| parameter.span.start),
                        );
                    }
                }
            }
        }
        if !retains_arrow(function, options) {
            continue;
        }
        if let Some(body) = block_open(tokens, function) {
            let head = header(tokens, function, body);
            if let Some(open) = head.iter().position(|token| token.is("(")) {
                if let Some(close) = super::closing_parenthesis(head, open) {
                    anchors.insert(
                        head[close].end,
                        if close == open + 1 {
                            head[open].start
                        } else {
                            head[close].start
                        },
                    );
                }
            }
            if let Some(arrow) = arrow(module, head) {
                anchors.insert(arrow + 2, arrow + 2);
            }
        }
    }
}

pub(super) fn anchors(
    module: &Module,
    options: &CompilerOptions,
    tokens: &[crate::Token],
    anchors: &mut BTreeMap<usize, Option<usize>>,
) {
    let mut parameter_opens = std::collections::BTreeSet::new();
    for function in module.nested_functions.values() {
        let Some(body) = block_open(tokens, function) else {
            continue;
        };
        let head = header(tokens, function, body);
        let after_generics = function
            .type_parameters
            .last()
            .map_or(function.span.start, |parameter| parameter.span.end);
        let open = head
            .iter()
            .position(|token| token.start >= after_generics && token.is("("));
        if let Some(open) = open {
            parameter_opens.insert(head[open].start);
            if function.kind == NestedFunctionKind::Method {
                if let Some(close) = super::closing_parenthesis(head, open) {
                    // A method's body follows its runtime parameter list even
                    // when its return type occupies source text in between.
                    anchors.insert(
                        body,
                        Some(if close == open + 1 {
                            head[open].start
                        } else {
                            head[close].start
                        }),
                    );
                }
            }
            if function.kind == NestedFunctionKind::Function
                && function.name.is_none()
                && !function.async_function
                && !function.generator
            {
                let before = tokens.partition_point(|token| token.start < head[open].start);
                if let Some(keyword) = before
                    .checked_sub(1)
                    .and_then(|index| tokens.get(index))
                    .filter(|token| token.is("function"))
                {
                    anchors.insert(head[open].start, Some(keyword.start));
                    if function.parameters.iter().all(|parameter| {
                        parameter.name == "this"
                            || options.target == crate::EcmaTarget::Es5 && parameter.rest
                    }) {
                        if let Some(close) = super::closing_parenthesis(head, open) {
                            anchors.insert(head[close].start, Some(keyword.start));
                            anchors.insert(body, Some(keyword.start));
                        }
                    }
                }
            }
        }
        if options.target >= crate::EcmaTarget::Es2015 {
            for parameter in function
                .parameters
                .iter()
                .filter(|parameter| parameter.rest)
            {
                anchors.insert(parameter.span.start, Some(parameter.span.start));
            }
        }
        if retains_arrow(function, options) {
            if let Some(arrow) = arrow(module, head) {
                anchors.insert(arrow, Some(arrow));
                anchors.insert(arrow + 1, Some(arrow));
                if matches!(function.body, NestedFunctionBody::Block { .. }) {
                    anchors.insert(body, Some(arrow + 2));
                }
            }
            for parameter in &function.parameters {
                let Some(first) = parameter.default.as_ref().and_then(|tokens| tokens.first())
                else {
                    continue;
                };
                let before = tokens.partition_point(|token| token.start < first.start);
                if let Some(equal) = before.checked_sub(1).and_then(|index| tokens.get(index)) {
                    if matches!(equal.text.as_str(), "=" | ">=" | ">>=" | ">>>=") {
                        anchors.insert(equal.end - 1, Some(parameter.span.start));
                    }
                }
            }
        }
    }
    if options.target >= crate::EcmaTarget::Es2015 && options.module_kind == crate::ModuleKind::Esm
    {
        for (declaration, nested) in runtime_declarations(&module.declarations) {
            let Declaration::Class(class) = declaration else {
                continue;
            };
            if nested
                || !class.default_export
                || super::super::decorators::lowers_class(class, options)
            {
                continue;
            }
            let header = &tokens[tokens.partition_point(|token| token.start < class.span.start)
                ..tokens.partition_point(|token| token.start < class.name_span.start)];
            if let (Some(default), Some(keyword)) = (
                header.iter().find(|token| token.is("default")),
                header.iter().find(|token| token.is("class")),
            ) {
                anchors.insert(keyword.start, Some(default.end));
            }
        }
    }
    let classes = runtime_declarations(&module.declarations)
        .into_iter()
        .filter_map(|(declaration, _)| match declaration {
            Declaration::Class(class) => Some(class),
            Declaration::Function(function) => {
                if let Some(body) = function.body_open {
                    let begin = tokens.partition_point(|token| token.start < function.span.start);
                    let end = tokens.partition_point(|token| token.start < body);
                    if let Some(open) = tokens[begin..end].iter().find(|token| token.is("(")) {
                        parameter_opens.insert(open.start);
                    }
                }
                None
            }
            _ => None,
        })
        .chain(
            module
                .class_expressions
                .values()
                .map(|expression| &expression.class),
        )
        .collect::<Vec<_>>();
    let member_keys = member_keys(module, tokens);
    for class in &classes {
        for member in &class.members {
            if options.target >= crate::EcmaTarget::Es2022 && options.defines_class_fields() {
                if let Some(field) = member
                    .field
                    .as_ref()
                    .filter(|field| field.initializer.is_none())
                {
                    if let Some(semicolon) = tokens.iter().find(|token| {
                        token.is(";")
                            && token.start >= field.name_span.end
                            && token.end <= member.span.end
                    }) {
                        anchors.insert(semicolon.start, Some(field.name_span.end));
                    }
                }
            }
            if let Some(key) = member.key.last() {
                let after = tokens.partition_point(|token| token.start < key.end);
                if tokens.get(after).is_some_and(|token| token.is("(")) {
                    parameter_opens.insert(tokens[after].start);
                }
            }
        }
    }
    for (index, token) in tokens.iter().enumerate().skip(1) {
        let previous = &tokens[index - 1];
        if token.is("else")
            && (previous.is(";") || previous.is("}"))
            && !member_keys.contains(&token.start)
        {
            anchors.insert(token.start, None);
        }
        if token.is("instanceof")
            && !previous.is(".")
            && !member_keys.contains(&token.start)
            && !tokens.get(index + 1).is_some_and(|next| next.is(":"))
        {
            anchors.insert(token.start, Some(previous.end));
        }
        if token.is("}") && previous.is("{") && index > 1 {
            let before = &tokens[index - 2];
            if before.is("(") && !parameter_opens.contains(&before.start) {
                anchors.insert(token.start, Some(previous.start));
            }
        }
    }
    for expression in module.class_expressions.values() {
        if super::super::decorators::lowers_class(&expression.class, options)
            && super::super::decorators::expressions::unmapped_continuation(
                module,
                &expression.class,
                options,
            )
        {
            if let Some(next) = tokens
                .iter()
                .find(|token| token.start >= expression.class.span.end)
            {
                anchors.insert(next.start, None);
            }
        }
    }
    if options.target >= crate::EcmaTarget::Es2015 {
        for expression in module.class_expressions.values() {
            if !super::super::decorators::lowers_class(&expression.class, options) {
                anchors.insert(expression.class.body_span.end - 1, None);
            }
        }
    }
    if options.target == crate::EcmaTarget::EsNext && options.defines_class_fields() {
        for pair in tokens.windows(2) {
            if pair[0].is("#") && pair[0].end == pair[1].start {
                anchors.insert(pair[0].start, Some(pair[0].start));
                anchors.insert(pair[1].start, Some(pair[0].start));
            }
        }
        for class in classes {
            for decorator in class
                .decorators
                .iter()
                .chain(class.members.iter().flat_map(|member| &member.decorators))
            {
                if !decorator.tokens.is_empty() {
                    anchors.insert(decorator.span.start, Some(decorator.span.start));
                }
            }
            for member in &class.members {
                let Some(key) = member.key.first() else {
                    continue;
                };
                if member.accessor.is_some() && !member.decorators.is_empty()
                    || member
                        .method
                        .as_ref()
                        .is_some_and(|method| method.generator)
                {
                    let header = &tokens[tokens
                        .partition_point(|token| token.start < member.span.start)
                        ..tokens.partition_point(|token| token.start < key.start)];
                    if member.accessor.is_some() && !options.experimental_decorators {
                        if let Some(modifier) = header
                            .iter()
                            .find(|token| token.is("get") || token.is("set"))
                        {
                            anchors.insert(modifier.start, None);
                        }
                    } else if let Some(star) = header.iter().find(|token| token.is("*")) {
                        anchors.insert(star.start, Some(star.start));
                    }
                }
                if key.text.starts_with('#') {
                    anchors.insert(key.start, Some(key.start));
                    anchors.insert(key.start + 1, Some(key.start));
                }
            }
        }
    }
}
