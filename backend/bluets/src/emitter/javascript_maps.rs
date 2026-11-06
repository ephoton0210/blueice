// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Original syntax boundaries that survive annotation and generic erasure.
use super::*;
mod class_headers;
mod exports;
mod generator_operands;
mod imports;
mod private_names;
mod retained;
mod spreads;
mod templates;

pub(super) fn token_end_anchors(
    module: &Module,
    options: &CompilerOptions,
) -> BTreeMap<usize, usize> {
    let Ok(tokens) = crate::lex(&module.id, &module.source) else {
        return BTreeMap::new();
    };
    let mut anchors = spreads::end_anchors(&tokens, options);
    generator_operands::end_anchors(module, options, &tokens, &mut anchors);
    retained::end_anchors(module, options, &tokens, &mut anchors);
    private_names::end_anchors(module, options, &mut anchors);
    class_headers::end_anchors(module, options, &mut anchors);
    if options.module_kind == crate::ModuleKind::Esm {
        for (token, origin) in imports::star_exports(module, &tokens) {
            anchors.insert(token.end, origin);
        }
    }
    if options.module_kind == crate::ModuleKind::Esm {
        for (declaration, nested) in runtime_declarations(&module.declarations) {
            let Declaration::Function(function) = declaration else {
                continue;
            };
            if nested
                || !function.default_export
                || !function.exported
                || function.async_function
                || function.generator
            {
                continue;
            }
            for token in tokens
                .iter()
                .skip_while(|token| token.start < function.span.start)
                .take_while(|token| token.start < function.span.end && !token.is("function"))
                .filter(|token| token.is("export") || token.is("default"))
            {
                anchors.insert(token.end, token.end);
            }
        }
    }
    super::binding_patterns::javascript_end_anchors(&tokens, &mut anchors);
    for (keyword, clause_end) in imports::clauses(module, &tokens) {
        anchors.insert(keyword.end, clause_end.end);
    }
    for (token, origin) in imports::bindings(module, &tokens) {
        anchors.insert(token.end, origin);
    }
    for (token, _, end) in exports::boundaries(module, options, &tokens) {
        anchors.insert(token.end, end);
    }
    let mut operands = BTreeMap::new();
    for span in module.erased_assertion_spans() {
        let before = tokens.partition_point(|token| token.end <= span.start);
        if let Some(previous) = before.checked_sub(1).and_then(|index| tokens.get(index)) {
            let operand = operands.get(&previous.end).copied().unwrap_or(previous.end);
            anchors.insert(operand, span.end);
            operands.insert(span.end, operand);
        }
    }
    for (_, operand, close) in super::type_elision::assertion_groups(module, &tokens) {
        anchors.insert(operand.end, close.end);
    }
    anchors
}

pub(super) fn token_anchors(
    module: &Module,
    options: &CompilerOptions,
    edits: &[TextEdit],
) -> BTreeMap<usize, Option<usize>> {
    let Ok(tokens) = crate::lex(&module.id, &module.source) else {
        return BTreeMap::new();
    };
    let mut anchors = BTreeMap::new();
    if options.module_kind == crate::ModuleKind::Esm {
        for (token, origin) in imports::star_exports(module, &tokens) {
            anchors.insert(token.start, Some(origin));
        }
    }
    for (keyword, clause_end) in imports::clauses(module, &tokens) {
        anchors.insert(keyword.start, Some(clause_end.end));
    }
    for (token, origin) in imports::bindings(module, &tokens) {
        anchors.insert(token.start, Some(origin));
    }
    for (token, start, _) in exports::boundaries(module, options, &tokens) {
        anchors.insert(token.start, Some(start));
    }
    // Trivia attached to an erased declaration has no surviving source node.
    // A runtime import still owns its comment when lowered to module transport.
    let leading_runtime = module
        .declarations
        .first()
        .is_none_or(|declaration| match declaration {
            Declaration::Import(import) => !import.is_type_only(),
            Declaration::ValueExport(export) => !export.is_type_only(),
            _ => !edits.iter().any(|edit| {
                edit.start <= declaration.span().start
                    && edit.end >= declaration.span().end
                    && edit.replacement.is_empty()
            }),
        });
    if leading_runtime && module.source.starts_with("/*") {
        anchors.insert(0, Some(0));
        if let Some(end) = module.source.find("*/").map(|end| end + 2) {
            anchors.insert(end, Some(end));
        }
    }
    templates::anchors(module, &tokens, &mut anchors);
    generator_operands::anchors(module, &tokens, &mut anchors);
    super::binding_patterns::javascript_anchors(module, &tokens, &mut anchors);
    for (index, token) in tokens.iter().enumerate() {
        if (token.is("else") || token.is("finally"))
            && index > 0
            && (tokens[index - 1].is("}") || tokens[index - 1].is(";"))
            && tokens
                .get(index + 1)
                .is_some_and(|next| next.is("{") || next.is("if"))
        {
            anchors.insert(token.start, None);
        }
    }
    for (declaration, nested) in runtime_declarations(&module.declarations) {
        let retains_export = !nested && options.module_kind == crate::ModuleKind::Esm;
        if let Declaration::DefaultExport(export) = declaration {
            if retains_export && export.expression {
                let header = &tokens[tokens.partition_point(|token| token.start < export.span.start)
                    ..tokens.partition_point(|token| token.start < export.span.end)];
                if let Some(default) = header.iter().find(|token| token.is("default")) {
                    anchors.insert(default.start, Some(export.span.start));
                }
            }
        }
        if let Declaration::Function(function) = declaration {
            function_anchors(
                function,
                retains_export,
                options.target < crate::EcmaTarget::Es2015,
                options.target
                    >= if function.generator {
                        crate::EcmaTarget::Es2018
                    } else {
                        crate::EcmaTarget::Es2017
                    },
                &tokens,
                &mut anchors,
            );
        }
        if let Declaration::Class(class) = declaration {
            // A decorated class still copies its explicit constructor. Its
            // opening parenthesis inherits the constructor keyword even when
            // the surrounding class declaration is replaced by a decorator IIFE.
            if options.target >= crate::EcmaTarget::Es2015 {
                for member in &class.members {
                    if let Some(constructor) = &member.constructor {
                        if let Some(open) = tokens.iter().find(|token| {
                            token.is("(")
                                && token.start >= constructor.span.start
                                && token.end <= constructor.span.end
                        }) {
                            anchors.insert(open.start, Some(constructor.span.start));
                        }
                    }
                }
            }
            if options.target < crate::EcmaTarget::Es2015
                || decorators::lowers_class(class, options)
                || class.body_span.start >= class.body_span.end
                || class.body_span.end > module.source.len()
            {
                continue;
            }
            anchors.insert(class.body_span.end - 1, None);
            if class.exported {
                let header = &tokens[tokens.partition_point(|token| token.start < class.span.start)
                    ..tokens.partition_point(|token| token.start < class.name_span.start)];
                if let (Some(export), Some(keyword)) = (
                    header.iter().find(|token| token.is("export")),
                    header.iter().find(|token| token.is("class")),
                ) {
                    anchors.insert(
                        keyword.start,
                        Some(if retains_export {
                            export.end
                        } else {
                            class.span.start
                        }),
                    );
                }
            }
            for member in &class.members {
                let begin = tokens.partition_point(|token| token.start < member.span.start);
                let end = tokens.partition_point(|token| token.start < member.span.end);
                let header = &tokens[begin..end];
                if let Some(field) = &member.field {
                    if member.key.first().is_some_and(|token| token.is("[")) {
                        if let (Some(key), Some(equal)) =
                            (member.key.last(), header.iter().find(|token| token.is("=")))
                        {
                            anchors.insert(equal.start, Some(key.end));
                        }
                    }
                    if field.initializer.is_none() {
                        if let Some(semicolon) = header.iter().find(|token| {
                            token.is(";")
                                && token.start >= field.name_span.end
                                && token.end <= member.span.end
                        }) {
                            anchors.insert(semicolon.start, Some(field.name_span.end));
                        }
                    }
                }
                if let Some(constructor) = &member.constructor {
                    if let Some(open) = header.iter().find(|token| {
                        token.is("(")
                            && token.start >= constructor.span.start
                            && token.end <= constructor.span.end
                    }) {
                        anchors.insert(open.start, Some(constructor.span.start));
                    }
                    default_parameters(&constructor.parameters, &tokens, &mut anchors);
                }
                let (name, generics, parameters, returned, body) =
                    if let Some(method) = &member.method {
                        (
                            &method.name_span,
                            method.type_parameters.as_slice(),
                            method.parameters.as_slice(),
                            method.return_type_span.as_ref(),
                            method.body.is_some(),
                        )
                    } else if let Some(accessor) = &member.accessor {
                        (
                            &accessor.name_span,
                            [].as_slice(),
                            accessor.parameters.as_slice(),
                            accessor.return_type_span.as_ref(),
                            accessor.body_present,
                        )
                    } else {
                        continue;
                    };
                if !body {
                    continue;
                }
                let after_generics = generics.last().map_or(name.end, |value| value.span.end);
                let Some(open) = header
                    .iter()
                    .position(|token| token.start >= after_generics && token.is("("))
                else {
                    continue;
                };
                let Some(close) = closing_parenthesis(header, open) else {
                    continue;
                };
                let after_annotation = returned.map_or(header[close].end, |span| span.end);
                let Some(body) = header
                    .iter()
                    .find(|token| token.start >= after_annotation && token.is("{"))
                else {
                    continue;
                };
                if !generics.is_empty() {
                    anchors.insert(header[open].start, Some(name.end));
                }
                anchors.insert(
                    body.start,
                    Some(if close == open + 1 {
                        header[open].start
                    } else {
                        header[close].start
                    }),
                );
                default_parameters(parameters, &tokens, &mut anchors);
            }
        }
    }
    retained::anchors(module, options, &tokens, &mut anchors);
    private_names::anchors(module, options, &mut anchors);
    class_headers::anchors(module, options, &mut anchors);
    anchors
}

fn function_anchors(
    function: &crate::parser::FunctionDeclaration,
    retains_export: bool,
    downlevel_rest: bool,
    retains_async_modifier: bool,
    tokens: &[crate::Token],
    anchors: &mut BTreeMap<usize, Option<usize>>,
) {
    let Some(body) = function.body_open else {
        return;
    };
    let header = &tokens[tokens.partition_point(|token| token.start < function.span.start)
        ..tokens.partition_point(|token| token.start < body)];
    let Some(keyword) = header.iter().position(|token| token.is("function")) else {
        return;
    };
    let Some(name) = header[keyword + 1..]
        .iter()
        .find(|token| token.text == function.name)
    else {
        return;
    };
    let after_generics = function
        .type_parameters
        .last()
        .map_or(name.end, |value| value.span.end);
    let Some(open) = header
        .iter()
        .position(|token| token.start >= after_generics && token.is("("))
    else {
        return;
    };
    let Some(close) = closing_parenthesis(header, open) else {
        return;
    };
    if function.exported {
        let origin = if retains_export {
            header[..keyword]
                .iter()
                .rev()
                .find(|token| token.is("default") || token.is("export"))
                .map_or(function.span.start, |token| token.end)
        } else {
            function.span.start
        };
        if function.async_function {
            if let Some(asynchronous) = header.iter().find(|token| token.is("async")) {
                anchors.insert(
                    asynchronous.start,
                    Some(if retains_async_modifier {
                        asynchronous.start
                    } else {
                        origin
                    }),
                );
                anchors.insert(
                    header[keyword].start,
                    Some(if retains_async_modifier {
                        asynchronous.end
                    } else {
                        origin
                    }),
                );
            }
        } else {
            anchors.insert(header[keyword].start, Some(origin));
        }
    }
    if !function.type_parameters.is_empty() {
        anchors.insert(header[open].start, Some(name.end));
    }
    let only_rest = downlevel_rest
        && function.parameters.iter().any(|parameter| parameter.rest)
        && function
            .parameters
            .iter()
            .filter(|parameter| parameter.name != "this")
            .all(|parameter| parameter.rest);
    if only_rest {
        anchors.insert(header[close].start, Some(header[open].start));
    }
    anchors.insert(
        body,
        Some(if close == open + 1 || only_rest {
            header[open].start
        } else {
            header[close].start
        }),
    );
    default_parameters(&function.parameters, tokens, anchors);
}

fn closing_parenthesis(tokens: &[crate::Token], open: usize) -> Option<usize> {
    let mut depth = 0usize;
    (open..tokens.len()).find(|&index| {
        if tokens[index].is("(") {
            depth += 1;
        } else if tokens[index].is(")") {
            depth -= 1;
        }
        depth == 0
    })
}

fn default_parameters(
    parameters: &[crate::parser::Parameter],
    tokens: &[crate::Token],
    anchors: &mut BTreeMap<usize, Option<usize>>,
) {
    for parameter in parameters {
        if let Some(first) = parameter.default.as_ref().and_then(|tokens| tokens.first()) {
            let index = tokens.partition_point(|token| token.start < first.start);
            if let Some(equal) = index
                .checked_sub(1)
                .and_then(|index| tokens.get(index))
                .filter(|token| token.is("="))
            {
                let origin = if parameter.annotation.is_some() {
                    parameter.span.start
                } else {
                    index
                        .checked_sub(2)
                        .and_then(|index| tokens.get(index))
                        .map_or(parameter.span.start, |token| token.end)
                };
                anchors.insert(equal.start, Some(origin));
            }
        }
    }
}
