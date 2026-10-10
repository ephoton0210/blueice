// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Default/rest parameter prologues from retained parameter records.

use super::*;
use crate::parser::Parameter;

pub(super) fn prologue(
    module: &Module,
    tokens: &[Token],
    parameters: &[Parameter],
    edits: &mut Vec<TextEdit>,
    sequence: &mut usize,
) -> Result<String, Diagnostic> {
    let mut output = String::new();
    for (position, parameter) in parameters
        .iter()
        .filter(|parameter| parameter.name != "this")
        .enumerate()
    {
        if parameter.pattern.is_some() {
            continue;
        }
        if let Some(default) = &parameter.default {
            if let (Some(first), Some(last)) = (default.first(), default.last()) {
                let index = tokens.partition_point(|token| token.start < first.start);
                let equal = tokens
                    .get(index.saturating_sub(1))
                    .filter(|token| token.is("="))
                    .ok_or_else(|| {
                        unsupported(
                            module,
                            &parameter.span,
                            "default parameter head was not retained",
                        )
                    })?;
                edits.push(TextEdit {
                    start: equal.start,
                    end: last.end,
                    replacement: String::new(),
                });
                output.push_str(&format!(
                    "if ({0} === void 0) {{ {0} = {1}; }}\n",
                    parameter.name,
                    decorators::relocated(first.start, last.end),
                ));
            }
        }
        if parameter.rest {
            let start = tokens.partition_point(|token| token.start < parameter.span.start);
            let end = tokens.partition_point(|token| token.start < parameter.span.end);
            let begin = if position > 0
                && tokens
                    .get(start.saturating_sub(1))
                    .is_some_and(|token| token.is(","))
            {
                tokens[start - 1].start
            } else {
                parameter.span.start
            };
            let finish = if tokens.get(end).is_some_and(|token| token.is(",")) {
                tokens[end].end
            } else {
                parameter.span.end
            };
            edits.push(TextEdit {
                start: begin,
                end: finish,
                replacement: String::new(),
            });
            let index = unused_name(&module.source, "argument", sequence);
            output.push_str(&format!(
                "var {0} = []; for (var {1} = {2}; {1} < arguments.length; {1}++) {{ {0}[{1} - {2}] = arguments[{1}]; }}\n",
                parameter.name, index, position,
            ));
        }
    }
    Ok(output)
}

pub(super) fn declarations(
    module: &Module,
    tokens: &[Token],
    edits: &mut Vec<TextEdit>,
    sequence: &mut usize,
) -> Result<(), Diagnostic> {
    for (declaration, _) in runtime_declarations(&module.declarations) {
        let Declaration::Function(function) = declaration else {
            continue;
        };
        let Some(open) = function.body_open else {
            continue;
        };
        let prologue = prologue(module, tokens, &function.parameters, edits, sequence)?;
        if !prologue.is_empty() {
            let insertion = body_directive_end(tokens, open);
            edits.push(TextEdit {
                start: insertion,
                end: insertion,
                replacement: format!("\n{prologue}"),
            });
        }
    }
    for function in module.nested_functions.values().filter(|function| {
        function.kind != NestedFunctionKind::Arrow
            && !function.async_function
            && !function.generator
    }) {
        let NestedFunctionBody::Block { .. } = &function.body else {
            continue;
        };
        let Some(open) = block_open(tokens, function.span.end) else {
            continue;
        };
        let prologue = prologue(module, tokens, &function.parameters, edits, sequence)?;
        if !prologue.is_empty() {
            let insertion = body_directive_end(tokens, open);
            edits.push(TextEdit {
                start: insertion,
                end: insertion,
                replacement: format!("\n{prologue}"),
            });
        }
        if function.kind == NestedFunctionKind::Method {
            let first = tokens.partition_point(|token| token.start < function.span.start);
            let parameter_open = if function.computed_key.is_empty() {
                first + 1
            } else {
                exponentiation::matching_close(tokens, first).map_or(first, |close| close + 1)
            };
            if tokens
                .get(parameter_open)
                .is_some_and(|token| token.is("("))
            {
                edits.push(TextEdit {
                    start: tokens[parameter_open].start,
                    end: tokens[parameter_open].start,
                    replacement: ": function ".into(),
                });
            }
        }
    }
    Ok(())
}

fn block_open(tokens: &[Token], end: usize) -> Option<usize> {
    let mut depth = 0usize;
    for token in tokens.iter().rev().filter(|token| token.end <= end) {
        if token.is("}") {
            depth += 1;
        }
        if token.is("{") {
            depth = depth.checked_sub(1)?;
            if depth == 0 {
                return Some(token.start);
            }
        }
    }
    None
}

pub(super) fn body_directive_end(tokens: &[Token], open: usize) -> usize {
    let first = tokens.partition_point(|token| token.start < open);
    let mut index = first + 1;
    let mut end = tokens[first].end;
    while tokens
        .get(index)
        .is_some_and(|token| token.kind == TokenKind::String)
        && tokens.get(index + 1).is_some_and(|token| token.is(";"))
    {
        end = tokens[index + 1].end;
        index += 2;
    }
    end
}
