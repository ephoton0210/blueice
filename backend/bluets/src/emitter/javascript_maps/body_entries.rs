// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Original callable entries and consecutive statement ends remain distinct.
use super::*;
use crate::parser::{FunctionBodyItem, NestedFunctionBody, NestedFunctionKind};

pub(super) fn separate_arrows(
    module: &Module,
    options: &CompilerOptions,
    edits: &mut Vec<TextEdit>,
) {
    if !options.source_map {
        return;
    }
    let Ok(source_tokens) = crate::lex(&module.id, &module.source) else {
        return;
    };
    // Field lowering moves initializer text, so an original arrow's end must
    // already be separate before the initializer is relocated.
    for function in module.nested_functions.values() {
        if function.kind != NestedFunctionKind::Arrow
            || !retains(function.async_function, function.generator, options)
            || options.target < crate::EcmaTarget::Es2015
        {
            continue;
        }
        let Some(body) = super::retained::block_open(&source_tokens, function) else {
            continue;
        };
        let head = &source_tokens[source_tokens
            .partition_point(|token| token.start < function.span.start)
            ..source_tokens.partition_point(|token| token.start < body)];
        let Some(arrow) = super::retained::arrow(module, head) else {
            continue;
        };
        // An adjacent generic close can make the lossless lexer split `=>`
        // across `>=` and `>` tokens. Emit the AST arrow as one original node
        // instead of inventing a source event inside its two glyphs.
        if head.last().is_some_and(|token| !token.is("=>")) {
            edits.push(TextEdit {
                start: arrow,
                end: arrow + 2,
                replacement: format!(
                    "{}=>{}",
                    relocations::source_anchor(arrow),
                    relocations::source_anchor(arrow + 2)
                ),
            });
        }
        if options.javascript_layout == crate::JavaScriptLayout::TypeScript
            && matches!(function.body, NestedFunctionBody::Expression(_))
            && arrow + 2 == body
            && !edits.iter().any(|edit| {
                edit.start <= body && body < edit.end || edit.start == body && edit.end == body
            })
        {
            edits.push(TextEdit {
                start: body,
                end: body,
                replacement: format!("{} ", relocations::source_anchor(body)),
            });
        }
    }
}

pub(super) fn separate(
    module: &Module,
    options: &CompilerOptions,
    anchors: &BTreeMap<usize, Option<usize>>,
    edits: &mut Vec<TextEdit>,
) {
    if !options.source_map || options.javascript_layout != crate::JavaScriptLayout::TypeScript {
        return;
    }
    let mut boundaries = BTreeMap::new();
    for (declaration, _) in runtime_declarations(&module.declarations) {
        if let Declaration::Function(function) = declaration {
            function_body(function, module, options, anchors, &mut boundaries);
        }
    }
    if options.target >= crate::EcmaTarget::Es2015 {
        let classes = runtime_declarations(&module.declarations)
            .into_iter()
            .filter_map(|(declaration, _)| match declaration {
                Declaration::Class(class) => Some(class),
                _ => None,
            })
            .chain(module.class_expressions.values().map(|node| &node.class));
        for class in classes {
            if decorators::lowers_class(class, options) {
                continue;
            }
            let close = class.body_span.end.saturating_sub(1);
            if options.target >= crate::EcmaTarget::Es2022
                && options.defines_class_fields()
                && class.members.last().is_some_and(|member| {
                    member.span.end == close
                        && member
                            .field
                            .as_ref()
                            .is_some_and(|field| !field.declared && !field.accessor)
                })
                && module.source.as_bytes().get(close.saturating_sub(1)) == Some(&b';')
            {
                // The last retained field ends before the class close's reset.
                // Dense `;}` syntax must keep both original boundary events.
                boundaries.entry(close).or_insert(close);
            }
            for member in &class.members {
                if let Some(method) = &member.method {
                    if retains(method.async_function, method.generator, options) {
                        if let Some(items) = &method.body {
                            body(items, module, options, anchors, &mut boundaries);
                        }
                    }
                }
                if let Some(accessor) = &member.accessor {
                    if accessor.body_present {
                        body(&accessor.body, module, options, anchors, &mut boundaries);
                    }
                }
            }
        }
    }
    for function in module.nested_functions.values() {
        if !retains(function.async_function, function.generator, options)
            || options.target < crate::EcmaTarget::Es2015
                && function.kind != NestedFunctionKind::Function
        {
            continue;
        }
        if let NestedFunctionBody::Block { items, .. } = &function.body {
            body(items, module, options, anchors, &mut boundaries);
        }
    }
    // Replaced bodies and existing generated prologues choose their own entry
    // ownership. This pass only separates copied, adjacent original syntax.
    for (at, origin) in boundaries {
        if edits
            .iter()
            .any(|edit| edit.start <= at && edit.end > at || edit.start == at && edit.end == at)
        {
            continue;
        }
        edits.push(TextEdit {
            start: at,
            end: at,
            replacement: format!("{} ", relocations::source_anchor(origin)),
        });
    }
}

fn retains(asynchronous: bool, generator: bool, options: &CompilerOptions) -> bool {
    if asynchronous {
        options.target
            >= if generator {
                crate::EcmaTarget::Es2018
            } else {
                crate::EcmaTarget::Es2017
            }
    } else {
        !generator || options.target >= crate::EcmaTarget::Es2015
    }
}

fn function_body(
    function: &crate::parser::FunctionDeclaration,
    module: &Module,
    options: &CompilerOptions,
    anchors: &BTreeMap<usize, Option<usize>>,
    boundaries: &mut BTreeMap<usize, usize>,
) {
    if retains(function.async_function, function.generator, options) {
        body(&function.body, module, options, anchors, boundaries);
    }
}

fn body(
    items: &[FunctionBodyItem],
    module: &Module,
    options: &CompilerOptions,
    anchors: &BTreeMap<usize, Option<usize>>,
    boundaries: &mut BTreeMap<usize, usize>,
) {
    if let Some(first) = items.first().and_then(span) {
        if let Some(open) = first.start.checked_sub(1) {
            if module.source.as_bytes().get(open) == Some(&b'{') {
                if let Some(Some(origin)) = anchors.get(&open) {
                    boundaries.entry(first.start).or_insert(*origin);
                }
            }
        }
    }
    for pair in items.windows(2) {
        if let (Some(previous), Some(next)) = (span(&pair[0]), span(&pair[1])) {
            if previous.end == next.start
                && module.source.as_bytes().get(previous.end.saturating_sub(1)) == Some(&b';')
            {
                boundaries.entry(next.start).or_insert(previous.end);
            }
        }
    }
    for item in items {
        match item {
            FunctionBodyItem::Function(function) => {
                function_body(function, module, options, anchors, boundaries);
            }
            FunctionBodyItem::If(statement) => {
                body(&statement.consequent, module, options, anchors, boundaries);
                if let Some(crate::parser::FunctionElseBranch::Braced(items)) = &statement.alternate
                {
                    body(items, module, options, anchors, boundaries);
                }
            }
            FunctionBodyItem::While(statement) => {
                body(&statement.body, module, options, anchors, boundaries);
            }
            FunctionBodyItem::Try(statement) => {
                body(&statement.block, module, options, anchors, boundaries);
                if let Some(handler) = &statement.handler {
                    body(&handler.body, module, options, anchors, boundaries);
                }
                if let Some(items) = &statement.finalizer {
                    body(items, module, options, anchors, boundaries);
                }
            }
            _ => {}
        }
    }
}

fn span(item: &FunctionBodyItem) -> Option<&SourceSpan> {
    match item {
        FunctionBodyItem::Variable(variable) => Some(&variable.span),
        FunctionBodyItem::Expression { span, .. }
        | FunctionBodyItem::Throw { span, .. }
        | FunctionBodyItem::Return { span, .. } => Some(span),
        FunctionBodyItem::If(statement) => Some(&statement.span),
        FunctionBodyItem::While(statement) => Some(&statement.span),
        FunctionBodyItem::Try(statement) => Some(&statement.span),
        FunctionBodyItem::Function(function) => Some(&function.span),
        FunctionBodyItem::TypeAlias(_) | FunctionBodyItem::Opaque(_) => None,
    }
}
